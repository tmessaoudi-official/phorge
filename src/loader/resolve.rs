//! Cross-package name-resolution walkers (M-Decomp W3.2): the recursive AST rewrite
//! that mangles/qualifies definitions and call sites. Driven by `load_project`, which
//! builds the `ResolveCtx` (kept in mod.rs); these fns only read it.

use super::*;

/// Resolve a type *name* to its mangled FQN, or `None` if it is a local (`package Main`) type or a
/// built-in (left bare). A terminal `import type` binding wins; otherwise a same-package sibling type
/// (a library type referencing another type in its own package).
pub(super) fn resolve_type_ref(name: &str, ctx: &ResolveCtx) -> Option<String> {
    if let Some(m) = ctx.type_imports.get(name) {
        // Cross-package terminal import — already visibility-checked in `build_type_imports`.
        return Some(m.clone());
    }
    let key = (ctx.package.join("."), name.to_string());
    if let Some(m) = ctx.types.get(&key) {
        // Same-package sibling type: enforce file-scoped `private` (visibility modifiers). Here the
        // referrer and definition share a package, so the lattice only ever yields `E-VIS-PRIVATE`.
        if let Some(info) = ctx.prov_types.get(&key) {
            if let Some(code) = vis_violation(info, ctx.file, &ctx.package.join(".")) {
                ctx.violations.borrow_mut().push(format!(
                    "{}: type `{name}` is private to `{}` — mark it `internal` (package-wide) or \
                     `public` (everywhere) to use it from another file [{code}]",
                    ctx.file.display(),
                    info.file.display(),
                ));
            }
        }
        return Some(m.clone());
    }
    None
}

/// Rewrite every type *name* inside a type annotation to its mangled FQN (cross-package types).
/// Mirrors the exhaustive `Type` walk of `checker::erase_generics`'s `rty`; recurses through generic
/// arguments, optionals, and function types so a `List<Point>` or `(Point) -> Point` resolves too.
pub(super) fn resolve_type(ty: &Type, ctx: &ResolveCtx) -> Type {
    match ty {
        Type::Named { name, args, span } => Type::Named {
            name: resolve_type_ref(name, ctx).unwrap_or_else(|| name.clone()),
            args: args.iter().map(|a| resolve_type(a, ctx)).collect(),
            span: *span,
        },
        Type::Optional { inner, span } => Type::Optional {
            inner: Box::new(resolve_type(inner, ctx)),
            span: *span,
        },
        Type::Function { params, ret, span } => Type::Function {
            params: params.iter().map(|p| resolve_type(p, ctx)).collect(),
            ret: Box::new(resolve_type(ret, ctx)),
            span: *span,
        },
        // A union resolves each member (a cross-package member name mangles like anywhere else), M-RT S4.
        Type::Union(members, span) => Type::Union(
            members.iter().map(|m| resolve_type(m, ctx)).collect(),
            *span,
        ),
        // An intersection resolves each member likewise (M-RT S5).
        Type::Intersection(members, span) => Type::Intersection(
            members.iter().map(|m| resolve_type(m, ctx)).collect(),
            *span,
        ),
        // `[T; N]`: resolve the element's type name (a cross-package `[Point; 2]` mangles its element).
        Type::FixedList { elem, len, span } => Type::FixedList {
            elem: Box::new(resolve_type(elem, ctx)),
            len: *len,
            span: *span,
        },
        Type::Infer(s) => Type::Infer(*s),
        Type::Erased(s) => Type::Erased(*s),
    }
}

/// Rewrite one top-level item: rename a function to its mangled global name and resolve its body;
/// resolve a class's method/constructor bodies (a class is always `package Main` — library types
/// are rejected upstream). Enums/imports/aliases have no call sites to rewrite.
pub(super) fn resolve_item(item: Item, ctx: &ResolveCtx) -> Item {
    match item {
        // M8.5: a foreign `declare` describes a *global* PHP symbol (`\strlen`, `\DateTimeImmutable`) —
        // it has no package, so it must never be mangled to a package-FQN. Pass it through untouched.
        // (Ambient `.d.phg` items bypass this pass entirely; this also covers an inline `declare` inside
        // a library-package file, where mangling would otherwise corrupt the global name.)
        Item::Function(f) if f.foreign => Item::Function(f),
        Item::Class(c) if c.foreign => Item::Class(c),
        Item::Function(mut f) => {
            f.name = mangle(&ctx.package, &f.name);
            for p in &mut f.params {
                p.ty = resolve_type(&p.ty, ctx);
            }
            f.ret = f.ret.as_ref().map(|r| resolve_type(r, ctx));
            f.body = resolve_block(f.body, ctx);
            Item::Function(f)
        }
        Item::Class(mut c) => {
            c.name = mangle(&ctx.package, &c.name);
            for imp in &mut c.implements {
                if let Some(m) = resolve_type_ref(imp, ctx) {
                    *imp = m;
                }
            }
            for m in &mut c.members {
                match m {
                    ClassMember::Method(f) => {
                        for p in &mut f.params {
                            p.ty = resolve_type(&p.ty, ctx);
                        }
                        f.ret = f.ret.as_ref().map(|r| resolve_type(r, ctx));
                        let body = std::mem::take(&mut f.body);
                        f.body = resolve_block(body, ctx);
                    }
                    ClassMember::Constructor { params, body, .. } => {
                        for p in params.iter_mut() {
                            p.ty = resolve_type(&p.ty, ctx);
                        }
                        let b = std::mem::take(body);
                        *body = resolve_block(b, ctx);
                    }
                    ClassMember::Field { ty, .. } => {
                        *ty = resolve_type(ty, ctx);
                    }
                    // A property hook (M-mut.7b) carries a type plus a `get` expression and/or a
                    // `set` block — each of which may name cross-package types or call cross-package
                    // functions, so resolve them exactly like a method body (mangle + type-rewrite).
                    ClassMember::Hook { ty, get, set, .. } => {
                        *ty = resolve_type(ty, ctx);
                        if let Some(e) = get.take() {
                            *get = Some(resolve_expr(e, ctx));
                        }
                        if let Some((p, body)) = set.take() {
                            let pty = resolve_type(&p.ty, ctx);
                            *set = Some((
                                Param {
                                    ty: pty,
                                    name: p.name,
                                    default: p.default,
                                    span: p.span,
                                },
                                resolve_block(body, ctx),
                            ));
                        }
                    }
                }
            }
            Item::Class(c)
        }
        Item::Enum(mut e) => {
            e.name = mangle(&ctx.package, &e.name);
            for v in &mut e.variants {
                for p in &mut v.fields {
                    p.ty = resolve_type(&p.ty, ctx);
                }
            }
            Item::Enum(e)
        }
        Item::Interface(mut i) => {
            i.name = mangle(&ctx.package, &i.name);
            for ext in &mut i.extends {
                if let Some(m) = resolve_type_ref(ext, ctx) {
                    *ext = m;
                }
            }
            for m in &mut i.methods {
                for p in &mut m.params {
                    p.ty = resolve_type(&p.ty, ctx);
                }
                m.ret = m.ret.as_ref().map(|r| resolve_type(r, ctx));
            }
            Item::Interface(i)
        }
        other => other,
    }
}

pub(super) fn resolve_block(stmts: Vec<Stmt>, ctx: &ResolveCtx) -> Vec<Stmt> {
    stmts.into_iter().map(|s| resolve_stmt(s, ctx)).collect()
}

pub(super) fn resolve_stmt(stmt: Stmt, ctx: &ResolveCtx) -> Stmt {
    match stmt {
        Stmt::VarDecl {
            ty,
            name,
            init,
            mutable,
            span,
        } => Stmt::VarDecl {
            ty: resolve_type(&ty, ctx),
            name,
            init: resolve_expr(init, ctx),
            mutable,
            span,
        },
        Stmt::Assign {
            target,
            value,
            span,
        } => Stmt::Assign {
            target: resolve_expr(target, ctx),
            value: resolve_expr(value, ctx),
            span,
        },
        Stmt::Return { value, span } => Stmt::Return {
            value: value.map(|e| resolve_expr(e, ctx)),
            span,
        },
        Stmt::If {
            cond,
            bind,
            then_block,
            else_block,
            span,
        } => Stmt::If {
            cond: resolve_expr(cond, ctx),
            bind,
            then_block: resolve_block(then_block, ctx),
            else_block: else_block.map(|b| resolve_block(b, ctx)),
            span,
        },
        Stmt::For {
            ty,
            name,
            iter,
            body,
            span,
        } => Stmt::For {
            ty: resolve_type(&ty, ctx),
            name,
            iter: resolve_expr(iter, ctx),
            body: resolve_block(body, ctx),
            span,
        },
        Stmt::While {
            cond,
            body,
            post_cond,
            span,
        } => Stmt::While {
            cond: resolve_expr(cond, ctx),
            body: resolve_block(body, ctx),
            post_cond,
            span,
        },
        Stmt::CFor {
            init,
            cond,
            step,
            body,
            span,
        } => Stmt::CFor {
            init: init.map(|s| Box::new(resolve_stmt(*s, ctx))),
            cond: cond.map(|e| resolve_expr(e, ctx)),
            step: step.map(|s| Box::new(resolve_stmt(*s, ctx))),
            body: resolve_block(body, ctx),
            span,
        },
        Stmt::Break(span) => Stmt::Break(span),
        Stmt::Continue(span) => Stmt::Continue(span),
        // Slice 5: mangle a cross-package struct head to its FQN (mirrors `instanceof`/`new`), and
        // resolve the init expr + the `else` block. A list pattern carries no type name.
        Stmt::Destructure {
            pat,
            init,
            else_block,
            span,
        } => {
            let pat = match pat {
                crate::ast::DestructurePat::Struct {
                    type_name,
                    fields,
                    span: psp,
                } => crate::ast::DestructurePat::Struct {
                    type_name: resolve_type_ref(&type_name, ctx).unwrap_or(type_name),
                    fields,
                    span: psp,
                },
                list => list,
            };
            Stmt::Destructure {
                pat,
                init: resolve_expr(init, ctx),
                else_block: else_block.map(|b| resolve_block(b, ctx)),
                span,
            }
        }
        Stmt::Block(stmts, span) => Stmt::Block(resolve_block(stmts, ctx), span),
        Stmt::Expr(e, span) => Stmt::Expr(resolve_expr(e, ctx), span),
        Stmt::Discard(e, span) => Stmt::Discard(resolve_expr(e, ctx), span),
        Stmt::Throw { value, span } => Stmt::Throw {
            value: resolve_expr(value, ctx),
            span,
        },
        Stmt::Try {
            body,
            catches,
            finally_block,
            span,
        } => Stmt::Try {
            body: resolve_block(body, ctx),
            catches: catches
                .into_iter()
                .map(|c| crate::ast::CatchClause {
                    ty: resolve_type(&c.ty, ctx),
                    name: c.name,
                    body: resolve_block(c.body, ctx),
                    span: c.span,
                })
                .collect(),
            finally_block: finally_block.map(|b| resolve_block(b, ctx)),
            span,
        },
    }
}

pub(super) fn resolve_expr(expr: Expr, ctx: &ResolveCtx) -> Expr {
    match expr {
        Expr::Call { callee, args, span } => resolve_call(*callee, args, span, ctx),
        Expr::Member {
            object,
            name,
            safe,
            span,
        } => Expr::Member {
            object: Box::new(resolve_expr(*object, ctx)),
            name,
            safe,
            span,
        },
        Expr::Index {
            object,
            index,
            span,
        } => Expr::Index {
            object: Box::new(resolve_expr(*object, ctx)),
            index: Box::new(resolve_expr(*index, ctx)),
            span,
        },
        Expr::Unary { op, expr, span } => Expr::Unary {
            op,
            expr: Box::new(resolve_expr(*expr, ctx)),
            span,
        },
        Expr::Binary { op, lhs, rhs, span } => Expr::Binary {
            op,
            lhs: Box::new(resolve_expr(*lhs, ctx)),
            rhs: Box::new(resolve_expr(*rhs, ctx)),
            span,
        },
        Expr::Force { inner, span } => Expr::Force {
            inner: Box::new(resolve_expr(*inner, ctx)),
            span,
        },
        Expr::Propagate { inner, span } => Expr::Propagate {
            inner: Box::new(resolve_expr(*inner, ctx)),
            span,
        },
        Expr::CloneWith {
            object,
            fields,
            span,
        } => Expr::CloneWith {
            object: Box::new(resolve_expr(*object, ctx)),
            fields: fields
                .into_iter()
                .map(|(n, e)| (n, resolve_expr(e, ctx)))
                .collect(),
            span,
        },
        Expr::List(items, span) => Expr::List(
            items.into_iter().map(|e| resolve_expr(e, ctx)).collect(),
            span,
        ),
        Expr::Str(parts, span) => Expr::Str(
            parts
                .into_iter()
                .map(|p| match p {
                    StrPart::Expr(e) => StrPart::Expr(Box::new(resolve_expr(*e, ctx))),
                    lit => lit,
                })
                .collect(),
            span,
        ),
        // `html"…"` holes can carry cross-package calls, so resolve them like string holes (the
        // literal itself is desugared later, by the post-check `checker::resolve_html` pass).
        Expr::Html(parts, span) => Expr::Html(
            parts
                .into_iter()
                .map(|p| match p {
                    StrPart::Expr(e) => StrPart::Expr(Box::new(resolve_expr(*e, ctx))),
                    lit => lit,
                })
                .collect(),
            span,
        ),
        Expr::Match {
            scrutinee,
            arms,
            span,
        } => Expr::Match {
            scrutinee: Box::new(resolve_expr(*scrutinee, ctx)),
            arms: arms
                .into_iter()
                .map(|a| MatchArm {
                    pattern: a.pattern,
                    guard: a.guard.map(|g| resolve_expr(g, ctx)),
                    body: resolve_expr(a.body, ctx),
                    span: a.span,
                })
                .collect(),
            span,
        },
        Expr::Range {
            start,
            end,
            inclusive,
            span,
        } => Expr::Range {
            start: Box::new(resolve_expr(*start, ctx)),
            end: Box::new(resolve_expr(*end, ctx)),
            inclusive,
            span,
        },
        Expr::If {
            cond,
            then_expr,
            else_expr,
            span,
        } => Expr::If {
            cond: Box::new(resolve_expr(*cond, ctx)),
            then_expr: Box::new(resolve_expr(*then_expr, ctx)),
            else_expr: Box::new(resolve_expr(*else_expr, ctx)),
            span,
        },
        // A bare identifier that names a cross-package type (e.g. the head of an enum access
        // `Color.Red`) resolves to the mangled FQN; the shadow guard guarantees an imported type
        // name is never also a local/variable, so rewriting every occurrence is safe.
        Expr::Ident(n, sp) => match resolve_type_ref(&n, ctx) {
            Some(m) => Expr::Ident(m, sp),
            None => Expr::Ident(n, sp),
        },
        Expr::InstanceOf {
            value,
            type_name,
            span,
        } => Expr::InstanceOf {
            value: Box::new(resolve_expr(*value, ctx)),
            type_name: resolve_type_ref(&type_name, ctx).unwrap_or(type_name),
            span,
        },
        // `value as TypeName` — the cast target is a type name, so (like `instanceof`) a cross-package
        // imported type must be mangled to its FQN here, before any backend sees it (M4 casting).
        Expr::Cast {
            value,
            type_name,
            span,
        } => Expr::Cast {
            value: Box::new(resolve_expr(*value, ctx)),
            type_name: resolve_type_ref(&type_name, ctx).unwrap_or(type_name),
            span,
        },
        Expr::Lambda {
            params,
            ret,
            body,
            span,
        } => Expr::Lambda {
            params: params
                .into_iter()
                .map(|mut p| {
                    p.ty = resolve_type(&p.ty, ctx);
                    p
                })
                .collect(),
            ret: ret.as_ref().map(|r| resolve_type(r, ctx)),
            body: match body {
                LambdaBody::Expr(e) => LambdaBody::Expr(Box::new(resolve_expr(*e, ctx))),
                LambdaBody::Block(stmts) => LambdaBody::Block(resolve_block(stmts, ctx)),
            },
            span,
        },
        // Feature C: `new <call>` — resolve the inner construction so a cross-package class/variant
        // callee is mangled to its FQN (`new Rect(…)` ⇒ `new \Acme\Geometry\Rect(…)`); the checker
        // later validates + unwraps it. Without this it would fall into the `leaf` arm unresolved.
        Expr::New(inner, span) => Expr::New(Box::new(resolve_expr(*inner, ctx)), span),
        // Leaves carry no nested call site or type name: Int / Float / Bool / Null / Bytes / This.
        leaf => leaf,
    }
}

/// Resolve a call. A bare `Ident` head resolves against the caller's own package (mangled if that
/// package is non-`main`; a no-op for `main`, and for variants/classes/unknowns which aren't in the
/// function table). A `Member` head `q.name` is a qualified user call iff `q` is a non-`core` import
/// leaf whose target package defines `name` — rewritten to a bare call on the mangled name;
/// otherwise it is a native call or a method on a value and is left intact (receiver resolved).
/// Buffer a function-visibility violation against `ctx.violations` (no-op when visible). `pkg` is the
/// package the function lives in — the referrer's package for a bare call, the import target for a
/// qualified `q.fn()` call.
pub(super) fn check_fn_visibility(ctx: &ResolveCtx, pkg: &str, name: &str) {
    if let Some(info) = ctx.prov_fns.get(&(pkg.to_string(), name.to_string())) {
        if let Some(code) = vis_violation(info, ctx.file, &ctx.package.join(".")) {
            ctx.violations.borrow_mut().push(format!(
                "{}: function `{name}` is not visible here — it is `{}` in package `{}`; widen its \
                 visibility to call it [{code}]",
                ctx.file.display(),
                vis_word(info.vis),
                if pkg.is_empty() { "main" } else { pkg },
            ));
        }
    }
}

pub(super) fn resolve_call(callee: Expr, args: Vec<Expr>, span: Span, ctx: &ResolveCtx) -> Expr {
    let args: Vec<Expr> = args.into_iter().map(|a| resolve_expr(a, ctx)).collect();
    match callee {
        Expr::Ident(n, isp) => {
            // A type name wins (a constructor call `Point(x)` — a name is a type XOR a function in a
            // file, guarded by `E-TYPE-IMPORT-SHADOW`); else the same-package function table.
            let resolved = if let Some(t) = resolve_type_ref(&n, ctx) {
                t
            } else if let Some(f) = ctx
                .defined
                .get(&(ctx.package.join("."), n.clone()))
                .cloned()
            {
                // Same-package function: enforce file-scoped `private` (visibility modifiers).
                check_fn_visibility(ctx, &ctx.package.join("."), &n);
                f
            } else {
                n
            };
            Expr::Call {
                callee: Box::new(Expr::Ident(resolved, isp)),
                args,
                span,
            }
        }
        Expr::Member {
            object,
            name,
            safe,
            span: msp,
        } => {
            if !safe {
                if let Expr::Ident(q, _) = object.as_ref() {
                    if let Some(target) = ctx.user_imports.get(q) {
                        if let Some(mangled) = ctx.defined.get(&(target.join("."), name.clone())) {
                            // Cross-package qualified call: enforce `internal`/`public`.
                            check_fn_visibility(ctx, &target.join("."), &name);
                            return Expr::Call {
                                callee: Box::new(Expr::Ident(mangled.clone(), msp)),
                                args,
                                span,
                            };
                        }
                    }
                }
            }
            Expr::Call {
                callee: Box::new(Expr::Member {
                    object: Box::new(resolve_expr(*object, ctx)),
                    name,
                    safe,
                    span: msp,
                }),
                args,
                span,
            }
        }
        other => Expr::Call {
            callee: Box::new(resolve_expr(other, ctx)),
            args,
            span,
        },
    }
}
