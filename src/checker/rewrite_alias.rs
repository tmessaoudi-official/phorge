use super::*;

/// Expand every `type` alias into its underlying type and drop the alias declarations, so the
/// interpreter, compiler, and transpiler all see alias-free types (aliases are pure front-end
/// sugar). Runs *after* [`check`] succeeds — which has already rejected cycles and built-in
/// shadowing — so a fixed depth bound is a sufficient guard against a residual self-reference, and
/// the resolver can be a simple "look the name up, recurse" walk. `Expr` nodes carry no `Type` in
/// M1, so they are cloned unchanged.
pub fn expand_aliases(program: &Program) -> Program {
    use crate::ast::{
        ClassDecl, ClassMember, CtorParam, EnumDecl, EnumVariant, FunctionDecl, InterfaceDecl,
        Item, Param, Stmt, Type,
    };
    type Aliases = HashMap<String, Type>;

    let mut aliases: Aliases = HashMap::new();
    for item in &program.items {
        if let Item::TypeAlias { name, ty, .. } = item {
            aliases.insert(name.clone(), ty.clone());
        }
    }

    fn rt(ty: &Type, a: &Aliases, depth: usize) -> Type {
        if depth > 64 {
            return ty.clone(); // defensive: check() already rejected alias cycles
        }
        match ty {
            Type::Named { name, args, span } => {
                if let Some(target) = a.get(name) {
                    rt(target, a, depth + 1)
                } else {
                    Type::Named {
                        name: name.clone(),
                        args: args.iter().map(|x| rt(x, a, depth + 1)).collect(),
                        span: *span,
                    }
                }
            }
            Type::Optional { inner, span } => Type::Optional {
                inner: Box::new(rt(inner, a, depth + 1)),
                span: *span,
            },
            Type::Function { params, ret, span } => Type::Function {
                params: params.iter().map(|p| rt(p, a, depth + 1)).collect(),
                ret: Box::new(rt(ret, a, depth + 1)),
                span: *span,
            },
            // A union expands each member (an alias used as a member dealiases here), M-RT S4.
            Type::Union(members, span) => {
                Type::Union(members.iter().map(|m| rt(m, a, depth + 1)).collect(), *span)
            }
            // An intersection expands each member likewise (M-RT S5).
            Type::Intersection(members, span) => {
                Type::Intersection(members.iter().map(|m| rt(m, a, depth + 1)).collect(), *span)
            }
            // `[T; N]`: dealias the element (`[MyAlias; 2]` expands its element here).
            Type::FixedList { elem, len, span } => Type::FixedList {
                elem: Box::new(rt(elem, a, depth + 1)),
                len: *len,
                span: *span,
            },
            Type::Infer(s) => Type::Infer(*s),
            Type::Erased(s) => Type::Erased(*s),
        }
    }
    fn rparam(p: &Param, a: &Aliases) -> Param {
        Param {
            ty: rt(&p.ty, a, 0),
            name: p.name.clone(),
            span: p.span,
        }
    }
    fn rstmt(s: &Stmt, a: &Aliases) -> Stmt {
        match s {
            Stmt::VarDecl {
                ty,
                name,
                init,
                mutable,
                span,
            } => Stmt::VarDecl {
                ty: rt(ty, a, 0),
                name: name.clone(),
                init: init.clone(),
                mutable: *mutable,
                span: *span,
            },
            Stmt::For {
                ty,
                name,
                iter,
                body,
                span,
            } => Stmt::For {
                ty: rt(ty, a, 0),
                name: name.clone(),
                iter: iter.clone(),
                body: body.iter().map(|s| rstmt(s, a)).collect(),
                span: *span,
            },
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                span,
            } => Stmt::If {
                cond: cond.clone(),
                bind: bind.clone(),
                then_block: then_block.iter().map(|s| rstmt(s, a)).collect(),
                else_block: else_block
                    .as_ref()
                    .map(|b| b.iter().map(|s| rstmt(s, a)).collect()),
                span: *span,
            },
            Stmt::While {
                cond,
                body,
                post_cond,
                span,
            } => Stmt::While {
                cond: cond.clone(),
                body: body.iter().map(|s| rstmt(s, a)).collect(),
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
                init: init.as_ref().map(|s| Box::new(rstmt(s, a))),
                cond: cond.clone(),
                step: step.as_ref().map(|s| Box::new(rstmt(s, a))),
                body: body.iter().map(|s| rstmt(s, a)).collect(),
                span: *span,
            },
            Stmt::Block(stmts, span) => {
                Stmt::Block(stmts.iter().map(|s| rstmt(s, a)).collect(), *span)
            }
            // A `try`'s catch clause carries a type annotation (possibly an alias) — resolve it and
            // recurse into the bodies (this pass rewrites type annotations only; exprs are cloned).
            Stmt::Try {
                body,
                catches,
                finally_block,
                span,
            } => Stmt::Try {
                body: body.iter().map(|s| rstmt(s, a)).collect(),
                catches: catches
                    .iter()
                    .map(|c| crate::ast::CatchClause {
                        ty: rt(&c.ty, a, 0),
                        name: c.name.clone(),
                        body: c.body.iter().map(|s| rstmt(s, a)).collect(),
                        span: c.span,
                    })
                    .collect(),
                finally_block: finally_block
                    .as_ref()
                    .map(|b| b.iter().map(|s| rstmt(s, a)).collect()),
                span: *span,
            },
            // Throw/Assign/Return/Expr/break/continue carry only exprs or nothing (no type
            // annotations this pass rewrites).
            Stmt::Throw { .. }
            | Stmt::Return { .. }
            | Stmt::Expr(..)
            | Stmt::Assign { .. }
            | Stmt::Break(_)
            | Stmt::Continue(_) => s.clone(),
        }
    }
    fn rfunc(f: &FunctionDecl, a: &Aliases) -> FunctionDecl {
        FunctionDecl {
            modifiers: f.modifiers.clone(),
            vis: f.vis,
            name: f.name.clone(),
            type_params: f.type_params.clone(),
            params: f.params.iter().map(|p| rparam(p, a)).collect(),
            ret: f.ret.as_ref().map(|t| rt(t, a, 0)),
            throws: f.throws.iter().map(|t| rt(t, a, 0)).collect(),
            body: f.body.iter().map(|s| rstmt(s, a)).collect(),
            span: f.span,
        }
    }
    fn rmember(m: &ClassMember, a: &Aliases) -> ClassMember {
        match m {
            ClassMember::Field {
                modifiers,
                ty,
                name,
                init,
                span,
            } => ClassMember::Field {
                modifiers: modifiers.clone(),
                ty: rt(ty, a, 0),
                name: name.clone(),
                // A field initializer is a literal const (no type alias can appear inside it).
                init: init.clone(),
                span: *span,
            },
            ClassMember::Constructor { params, body, span } => ClassMember::Constructor {
                params: params
                    .iter()
                    .map(|p| CtorParam {
                        modifiers: p.modifiers.clone(),
                        ty: rt(&p.ty, a, 0),
                        name: p.name.clone(),
                        span: p.span,
                    })
                    .collect(),
                body: body.iter().map(|s| rstmt(s, a)).collect(),
                span: *span,
            },
            ClassMember::Method(f) => ClassMember::Method(rfunc(f, a)),
            // A property hook (M-mut.7b): expand aliases in its type + set parameter type; the get
            // expression and set block carry no stmt-level type annotations this pass rewrites
            // (consistent with how method-body exprs are treated above), so they pass through.
            ClassMember::Hook {
                ty,
                name,
                get,
                set,
                span,
            } => ClassMember::Hook {
                ty: rt(ty, a, 0),
                name: name.clone(),
                get: get.clone(),
                set: set.as_ref().map(|(p, b)| {
                    (
                        Param {
                            ty: rt(&p.ty, a, 0),
                            name: p.name.clone(),
                            span: p.span,
                        },
                        b.iter().map(|s| rstmt(s, a)).collect(),
                    )
                }),
                span: *span,
            },
        }
    }

    let items = program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::TypeAlias { .. } => None,
            Item::Import { .. } => Some(item.clone()),
            Item::Function(f) => Some(Item::Function(rfunc(f, &aliases))),
            Item::Class(c) => Some(Item::Class(ClassDecl {
                vis: c.vis,
                name: c.name.clone(),
                type_params: c.type_params.clone(),
                extends: c.extends.clone(),
                implements: c.implements.clone(),
                open: c.open,
                is_abstract: c.is_abstract,
                resolutions: c.resolutions.clone(),
                uses: c.uses.clone(),
                members: c.members.iter().map(|m| rmember(m, &aliases)).collect(),
                span: c.span,
            })),
            // M-RT S8: a trait's member type annotations are alias-rewritten exactly like a class's.
            Item::Trait(t) => Some(Item::Trait(crate::ast::TraitDecl {
                name: t.name.clone(),
                members: t.members.iter().map(|m| rmember(m, &aliases)).collect(),
                span: t.span,
            })),
            Item::Interface(i) => Some(Item::Interface(InterfaceDecl {
                vis: i.vis,
                name: i.name.clone(),
                extends: i.extends.clone(),
                methods: i.methods.iter().map(|m| rfunc(m, &aliases)).collect(),
                span: i.span,
            })),
            Item::Enum(e) => Some(Item::Enum(EnumDecl {
                vis: e.vis,
                name: e.name.clone(),
                type_params: e.type_params.clone(),
                variants: e
                    .variants
                    .iter()
                    .map(|v| EnumVariant {
                        name: v.name.clone(),
                        fields: v.fields.iter().map(|p| rparam(p, &aliases)).collect(),
                        span: v.span,
                    })
                    .collect(),
                span: e.span,
            })),
        })
        .collect();

    Program {
        package: program.package.clone(),
        items,
        span: program.span,
    }
}
