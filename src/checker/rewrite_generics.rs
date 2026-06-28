use super::*;

/// Erase generic type parameters from a checked program (M-RT S7). For every generic free function,
/// every type annotation that names one of *that function's* type parameters is rewritten to
/// `Type::Erased` and the parameter list is cleared, so the interpreter, compiler, and transpiler
/// all see an ordinary, type-variable-free function (PHP `mixed` at the boundary). This is the same
/// "compile-time-only, expanded out before any backend" discipline as `type` aliases and `html"…"`,
/// and it is what keeps generics zero-cost and byte-identical across the three backends: there is no
/// monomorphization, the type variables simply disappear after checking. Type parameters are scoped
/// to their own function, so only `Item::Function` items with a non-empty `type_params` are
/// rewritten; everything else is returned untouched (a program with no generics is byte-for-byte the
/// pre-S7 AST). Runs after a successful [`check`], so the `T`-bearing types it erases were already
/// validated.
pub fn erase_generics(program: Program) -> Program {
    use crate::ast::{
        ClassDecl, ClassMember, Expr, FunctionDecl, Item, LambdaBody, MatchArm, Param, Stmt,
        StrPart, Type,
    };
    use std::collections::HashSet;

    type Params<'a> = HashSet<&'a str>;

    fn member_is_generic(m: &ClassMember) -> bool {
        matches!(m, ClassMember::Method(f) if !f.type_params.is_empty())
    }

    fn rty(ty: &Type, params: &Params) -> Type {
        match ty {
            Type::Named { name, args, span } => {
                // A bare reference to a type parameter erases; a real generic container (`List<T>`)
                // keeps its head and recurses into its arguments.
                if args.is_empty() && params.contains(name.as_str()) {
                    Type::Erased(*span)
                } else {
                    Type::Named {
                        name: name.clone(),
                        args: args.iter().map(|a| rty(a, params)).collect(),
                        span: *span,
                    }
                }
            }
            Type::Optional { inner, span } => Type::Optional {
                inner: Box::new(rty(inner, params)),
                span: *span,
            },
            Type::Function {
                params: ps,
                ret,
                span,
            } => Type::Function {
                params: ps.iter().map(|p| rty(p, params)).collect(),
                ret: Box::new(rty(ret, params)),
                span: *span,
            },
            // A union erases each member (a type-param member becomes `Type::Erased`); the union
            // itself is structural and survives to the backend (M-RT S4).
            Type::Union(members, span) => {
                Type::Union(members.iter().map(|m| rty(m, params)).collect(), *span)
            }
            // An intersection erases each member (a type-param member becomes `Type::Erased`); the
            // intersection itself is structural and survives to the backend (M-RT S5).
            Type::Intersection(members, span) => {
                Type::Intersection(members.iter().map(|m| rty(m, params)).collect(), *span)
            }
            // `[T; N]`: erase a type-param element (`[T; 2]` → `[<erased>; 2]`); the fixed-list head
            // survives to the backend, which treats it as a list either way (Phase 1 types slice).
            Type::FixedList { elem, len, span } => Type::FixedList {
                elem: Box::new(rty(elem, params)),
                len: *len,
                span: *span,
            },
            Type::Infer(s) => Type::Infer(*s),
            Type::Erased(s) => Type::Erased(*s),
        }
    }
    fn rparam(p: &Param, params: &Params) -> Param {
        Param {
            ty: rty(&p.ty, params),
            name: p.name.clone(),
            // A default is a literal (no type params inside) — carry it verbatim.
            default: p.default.clone(),
            span: p.span,
        }
    }
    fn rctorparam(p: &crate::ast::CtorParam, params: &Params) -> crate::ast::CtorParam {
        crate::ast::CtorParam {
            modifiers: p.modifiers.clone(),
            ty: rty(&p.ty, params),
            name: p.name.clone(),
            span: p.span,
        }
    }
    fn rparts(parts: &[StrPart], params: &Params) -> Vec<StrPart> {
        parts
            .iter()
            .map(|p| match p {
                StrPart::Expr(e) => StrPart::Expr(Box::new(rexpr(e, params))),
                StrPart::Literal(s) => StrPart::Literal(s.clone()),
            })
            .collect()
    }
    fn rexpr(e: &Expr, params: &Params) -> Expr {
        match e {
            // The only expression that carries types: a lambda's parameters and return annotation.
            Expr::Lambda {
                params: lp,
                ret,
                body,
                span,
            } => Expr::Lambda {
                params: lp.iter().map(|p| rparam(p, params)).collect(),
                ret: ret.as_ref().map(|t| rty(t, params)),
                body: match body {
                    LambdaBody::Expr(inner) => LambdaBody::Expr(Box::new(rexpr(inner, params))),
                    LambdaBody::Block(stmts) => {
                        LambdaBody::Block(stmts.iter().map(|s| rstmt(s, params)).collect())
                    }
                },
                span: *span,
            },
            Expr::Str(parts, span) => Expr::Str(rparts(parts, params), *span),
            Expr::Html(parts, span) => Expr::Html(rparts(parts, params), *span),
            Expr::List(items, span) => {
                Expr::List(items.iter().map(|i| rexpr(i, params)).collect(), *span)
            }
            Expr::Map(pairs, span) => Expr::Map(
                pairs
                    .iter()
                    .map(|(k, v)| (rexpr(k, params), rexpr(v, params)))
                    .collect(),
                *span,
            ),
            Expr::Unary { op, expr, span } => Expr::Unary {
                op: *op,
                expr: Box::new(rexpr(expr, params)),
                span: *span,
            },
            Expr::Binary { op, lhs, rhs, span } => Expr::Binary {
                op: *op,
                lhs: Box::new(rexpr(lhs, params)),
                rhs: Box::new(rexpr(rhs, params)),
                span: *span,
            },
            Expr::InstanceOf {
                value,
                type_name,
                span,
            } => Expr::InstanceOf {
                value: Box::new(rexpr(value, params)),
                type_name: type_name.clone(),
                span: *span,
            },
            Expr::Cast {
                value,
                type_name,
                span,
            } => Expr::Cast {
                value: Box::new(rexpr(value, params)),
                type_name: type_name.clone(),
                span: *span,
            },
            Expr::Call { callee, args, span } => Expr::Call {
                callee: Box::new(rexpr(callee, params)),
                args: args.iter().map(|a| rexpr(a, params)).collect(),
                span: *span,
            },
            Expr::Member {
                object,
                name,
                safe,
                span,
            } => Expr::Member {
                object: Box::new(rexpr(object, params)),
                name: name.clone(),
                safe: *safe,
                span: *span,
            },
            Expr::Index {
                object,
                index,
                span,
            } => Expr::Index {
                object: Box::new(rexpr(object, params)),
                index: Box::new(rexpr(index, params)),
                span: *span,
            },
            Expr::Force { inner, span } => Expr::Force {
                inner: Box::new(rexpr(inner, params)),
                span: *span,
            },
            Expr::Propagate { inner, span } => Expr::Propagate {
                inner: Box::new(rexpr(inner, params)),
                span: *span,
            },
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => Expr::Match {
                scrutinee: Box::new(rexpr(scrutinee, params)),
                arms: arms
                    .iter()
                    .map(|a| MatchArm {
                        pattern: a.pattern.clone(),
                        guard: a.guard.as_ref().map(|g| rexpr(g, params)),
                        body: rexpr(&a.body, params),
                        span: a.span,
                    })
                    .collect(),
                span: *span,
            },
            Expr::Range {
                start,
                end,
                inclusive,
                span,
            } => Expr::Range {
                start: Box::new(rexpr(start, params)),
                end: Box::new(rexpr(end, params)),
                inclusive: *inclusive,
                span: *span,
            },
            Expr::If {
                cond,
                then_expr,
                else_expr,
                span,
            } => Expr::If {
                cond: Box::new(rexpr(cond, params)),
                then_expr: Box::new(rexpr(then_expr, params)),
                else_expr: Box::new(rexpr(else_expr, params)),
                span: *span,
            },
            Expr::CloneWith {
                object,
                fields,
                span,
            } => Expr::CloneWith {
                object: Box::new(rexpr(object, params)),
                fields: fields
                    .iter()
                    .map(|(n, e)| (n.clone(), rexpr(e, params)))
                    .collect(),
                span: *span,
            },
            // leaves carry no type and no nested expression: Int / Float / Bool / Null / Bytes /
            // Ident / This — clone unchanged.
            leaf => leaf.clone(),
        }
    }
    fn rstmt(s: &Stmt, params: &Params) -> Stmt {
        match s {
            Stmt::VarDecl {
                ty,
                name,
                init,
                mutable,
                span,
            } => Stmt::VarDecl {
                ty: rty(ty, params),
                name: name.clone(),
                init: rexpr(init, params),
                mutable: *mutable,
                span: *span,
            },
            Stmt::Assign {
                target,
                value,
                span,
            } => Stmt::Assign {
                target: rexpr(target, params),
                value: rexpr(value, params),
                span: *span,
            },
            Stmt::Return { value, span } => Stmt::Return {
                value: value.as_ref().map(|e| rexpr(e, params)),
                span: *span,
            },
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                span,
            } => Stmt::If {
                cond: rexpr(cond, params),
                bind: bind.clone(),
                then_block: then_block.iter().map(|s| rstmt(s, params)).collect(),
                else_block: else_block
                    .as_ref()
                    .map(|b| b.iter().map(|s| rstmt(s, params)).collect()),
                span: *span,
            },
            Stmt::For {
                ty,
                name,
                iter,
                body,
                span,
            } => Stmt::For {
                ty: rty(ty, params),
                name: name.clone(),
                iter: rexpr(iter, params),
                body: body.iter().map(|s| rstmt(s, params)).collect(),
                span: *span,
            },
            Stmt::While {
                cond,
                body,
                post_cond,
                span,
            } => Stmt::While {
                cond: rexpr(cond, params),
                body: body.iter().map(|s| rstmt(s, params)).collect(),
                post_cond: *post_cond,
                span: *span,
            },
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                span,
            } => Stmt::CFor {
                init: init.as_ref().map(|s| Box::new(rstmt(s, params))),
                cond: cond.as_ref().map(|e| rexpr(e, params)),
                step: step.as_ref().map(|s| Box::new(rstmt(s, params))),
                body: body.iter().map(|s| rstmt(s, params)).collect(),
                span: *span,
            },
            Stmt::Break(span) => Stmt::Break(*span),
            Stmt::Continue(span) => Stmt::Continue(*span),
            Stmt::Block(stmts, span) => {
                Stmt::Block(stmts.iter().map(|s| rstmt(s, params)).collect(), *span)
            }
            Stmt::Expr(e, span) => Stmt::Expr(rexpr(e, params), *span),
            Stmt::Discard(e, span) => Stmt::Discard(rexpr(e, params), *span),
            Stmt::Throw { value, span } => Stmt::Throw {
                value: rexpr(value, params),
                span: *span,
            },
            Stmt::Try {
                body,
                catches,
                finally_block,
                span,
            } => Stmt::Try {
                body: body.iter().map(|s| rstmt(s, params)).collect(),
                catches: catches
                    .iter()
                    .map(|c| crate::ast::CatchClause {
                        ty: rty(&c.ty, params),
                        name: c.name.clone(),
                        body: c.body.iter().map(|s| rstmt(s, params)).collect(),
                        span: c.span,
                    })
                    .collect(),
                finally_block: finally_block
                    .as_ref()
                    .map(|b| b.iter().map(|s| rstmt(s, params)).collect()),
                span: *span,
            },
            // Slice 5: erase generics in the init expr and the `else` block. The struct head is a bare
            // class name with no type arguments in destructure syntax, so the pattern is cloned as-is.
            Stmt::Destructure {
                pat,
                init,
                else_block,
                span,
            } => Stmt::Destructure {
                pat: pat.clone(),
                init: rexpr(init, params),
                else_block: else_block
                    .as_ref()
                    .map(|b| b.iter().map(|s| rstmt(s, params)).collect()),
                span: *span,
            },
        }
    }

    let Program {
        package,
        items,
        span,
    } = program;
    let items = items
        .into_iter()
        .map(|item| match item {
            Item::Function(f) if !f.type_params.is_empty() => {
                let params: Params = f.type_params.iter().map(String::as_str).collect();
                Item::Function(FunctionDecl {
                    modifiers: f.modifiers.clone(),
                    attrs: f.attrs.clone(),
                    vis: f.vis,
                    name: f.name.clone(),
                    type_params: Vec::new(), // erased
                    params: f.params.iter().map(|p| rparam(p, &params)).collect(),
                    ret: f.ret.as_ref().map(|t| rty(t, &params)),
                    throws: f.throws.iter().map(|t| rty(t, &params)).collect(),
                    body: f.body.iter().map(|s| rstmt(s, &params)).collect(),
                    foreign: f.foreign,
                    span: f.span,
                })
            }
            // A generic class (class-level `<T>`) and/or a class with a generic method (M-RT
            // generics-all): erase the class's type parameters across *every* member (field types,
            // constructor parameters, method signatures + bodies) and each generic method's own
            // `<U>`, then clear all type-parameter lists. The class's `<T>`-typed members become PHP
            // `mixed`; the class declaration itself stays (just non-generic). A class with neither
            // class-level params nor a generic method is returned untouched by the `other` arm, so a
            // non-generic program is byte-for-byte the pre-generics AST.
            Item::Class(c)
                if !c.type_params.is_empty() || c.members.iter().any(member_is_generic) =>
            {
                let class_params: Vec<&str> = c.type_params.iter().map(String::as_str).collect();
                let members = c
                    .members
                    .into_iter()
                    .map(|m| match m {
                        ClassMember::Method(f) => {
                            // erase the class's params *and* this method's own
                            let mut set: Params = class_params.iter().copied().collect();
                            for tp in &f.type_params {
                                set.insert(tp.as_str());
                            }
                            ClassMember::Method(FunctionDecl {
                                modifiers: f.modifiers.clone(),
                                attrs: f.attrs.clone(),
                                vis: f.vis,
                                name: f.name.clone(),
                                type_params: Vec::new(), // erased
                                params: f.params.iter().map(|p| rparam(p, &set)).collect(),
                                ret: f.ret.as_ref().map(|t| rty(t, &set)),
                                throws: f.throws.iter().map(|t| rty(t, &set)).collect(),
                                body: f.body.iter().map(|s| rstmt(s, &set)).collect(),
                                foreign: f.foreign,
                                span: f.span,
                            })
                        }
                        ClassMember::Field {
                            modifiers,
                            ty,
                            name,
                            init,
                            span,
                        } => {
                            let set: Params = class_params.iter().copied().collect();
                            ClassMember::Field {
                                modifiers,
                                ty: rty(&ty, &set),
                                name,
                                init: init.as_ref().map(|e| rexpr(e, &set)),
                                span,
                            }
                        }
                        ClassMember::Constructor {
                            modifiers,
                            params,
                            body,
                            span,
                        } => {
                            let set: Params = class_params.iter().copied().collect();
                            ClassMember::Constructor {
                                modifiers,
                                params: params.iter().map(|p| rctorparam(p, &set)).collect(),
                                body: body.iter().map(|s| rstmt(s, &set)).collect(),
                                span,
                            }
                        }
                        // A property hook (M-mut.7b): erase the class params from its type, get
                        // expression, and set parameter+block (a hook declares no `<T>` of its own).
                        ClassMember::Hook {
                            ty,
                            name,
                            get,
                            set: setter,
                            span,
                        } => {
                            let set: Params = class_params.iter().copied().collect();
                            ClassMember::Hook {
                                ty: rty(&ty, &set),
                                name,
                                get: get.as_ref().map(|e| rexpr(e, &set)),
                                set: setter.as_ref().map(|(p, b)| {
                                    (rparam(p, &set), b.iter().map(|s| rstmt(s, &set)).collect())
                                }),
                                span,
                            }
                        }
                    })
                    .collect();
                Item::Class(ClassDecl {
                    vis: c.vis,
                    name: c.name,
                    type_params: Vec::new(), // erased
                    extends: c.extends,
                    implements: c.implements,
                    open: c.open,
                    is_abstract: c.is_abstract,
                    resolutions: c.resolutions,
                    uses: c.uses,
                    members,
                    foreign: c.foreign,
                    span: c.span,
                })
            }
            // A generic enum (`Option<T>`/`Result<T, E>`, M-RT generic enums): erase the enum's type
            // parameters across every variant's field types (a `T` payload becomes PHP `mixed`) and
            // clear the parameter list, so the backends see an ordinary, type-variable-free enum.
            // Same "expanded out before any backend" discipline as a generic class.
            Item::Enum(e) if !e.type_params.is_empty() => {
                let params: Params = e.type_params.iter().map(String::as_str).collect();
                Item::Enum(crate::ast::EnumDecl {
                    vis: e.vis,
                    name: e.name,
                    type_params: Vec::new(), // erased
                    variants: e
                        .variants
                        .into_iter()
                        .map(|v| crate::ast::EnumVariant {
                            name: v.name,
                            fields: v.fields.iter().map(|p| rparam(p, &params)).collect(),
                            span: v.span,
                        })
                        .collect(),
                    span: e.span,
                })
            }
            other => other,
        })
        .collect();
    Program {
        package,
        items,
        span,
    }
}
