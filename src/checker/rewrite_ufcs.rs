use super::*;

/// Rewrite every resolved UFCS member call `x.f(a)` into the ordinary free/native call `f(x, a)` the
/// checker chose (keyed by the enclosing `Call` node's `Span.start`), so the interpreter, compiler,
/// and transpiler never see a UFCS-shaped `Member` call — the same "compile-time sugar, erased before
/// any backend" treatment as `type` aliases / generics / `html"…"` (Slice 6, F-001). Runs last in
/// [`crate::cli::check_and_expand`], after the other front-end sugar is gone, so the receiver and
/// arguments it relocates are already fully de-sugared. A recorded replacement embeds the original
/// receiver/argument subtrees, which may themselves contain UFCS (`xs.filter(p).map(g)`), so the
/// rewrite re-walks each substituted subtree — but never re-matches the replacement's own root span
/// (which equals the key), which would loop. When no UFCS was recorded the program is returned
/// untouched, so programs without UFCS are byte-for-byte identical to the pre-Slice-6 AST.
pub fn rewrite_ufcs(program: Program, ufcs: &HashMap<usize, crate::ast::Expr>) -> Program {
    use crate::ast::{ClassMember, Expr, Item, LambdaBody, MatchArm, Stmt, StrPart};
    if ufcs.is_empty() {
        return program;
    }
    type Map = HashMap<usize, Expr>;

    // Apply a recorded replacement, re-walking its children for nested sugar but reconstructing the
    // root directly (its span is the original key / synthetic, so it is never re-matched → no loop).
    // `#[inline(never)]` + a separate function keep these (relatively large) arms off `rexpr`'s
    // frame: `rexpr` recurses once per expression-tree level, so bloating its frame overflows the
    // stack on a deeply-nested program (a regression the differential's example sweep catches).
    #[inline(never)]
    fn apply_repl(repl: &Expr, u: &Map) -> Expr {
        match repl {
            // A resolved UFCS call, or a `Reflect.typeName` object-case `className` / erased-case
            // `kind` call: re-walk the children. The call's own span is the original key or synthetic
            // (never re-matched — the root is reconstructed directly), and its args carry their own
            // distinct spans, so nested sugar in them resolves without looping.
            Expr::Call { callee, args, span } => Expr::Call {
                callee: Box::new(rexpr((**callee).clone(), u)),
                args: args.iter().cloned().map(|a| rexpr(a, u)).collect(),
                span: *span,
            },
            // A `match` replacement: the `?.` UFCS null-safe desugar (`x?.f(a)`) AND `typeName`'s
            // optional null-branch. Re-walk ONLY the scrutinee (the embedded original receiver /
            // argument — its span differs from this call's key, so nested sugar in it resolves
            // safely); CLONE the arms. The arms must not be walked: the `?.` desugar reuses this
            // call's own span for its arm-body call, so walking it would re-match this very key and
            // recurse forever. Reflect's optional arms hold only a literal or `className(<fresh
            // binding>)` (no user sugar), so cloning them is complete; the `?.` arm body was never
            // walked in the original either, so this is no regression.
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => Expr::Match {
                scrutinee: Box::new(rexpr((**scrutinee).clone(), u)),
                arms: arms.clone(),
                span: *span,
            },
            // Any other replacement shape (e.g. `typeName`'s value-type baked string literal) carries
            // no embedded original subtree — clone it without walking.
            other => other.clone(),
        }
    }

    fn rexpr(e: Expr, u: &Map) -> Expr {
        match e {
            // A recorded call site (UFCS rewrite, or a `Reflect.typeName` substitution): apply the
            // replacement via the out-of-line `apply_repl` — kept a SEPARATE function on purpose, so
            // its (larger) match over replacement shapes does not inflate `rexpr`'s own stack frame.
            // `rexpr` is the deeply-recursive walker (a nested expression recurses one `rexpr` frame
            // per level), so its frame size is stack-critical; a recorded match is rare and shallow.
            Expr::Call { callee, args, span } => match u.get(&span.start) {
                Some(repl) => apply_repl(repl, u),
                None => Expr::Call {
                    callee: Box::new(rexpr(*callee, u)),
                    args: args.into_iter().map(|a| rexpr(a, u)).collect(),
                    span,
                },
            },
            Expr::Str(parts, span) => Expr::Str(
                parts
                    .into_iter()
                    .map(|p| match p {
                        StrPart::Expr(e) => StrPart::Expr(Box::new(rexpr(*e, u))),
                        lit => lit,
                    })
                    .collect(),
                span,
            ),
            Expr::List(items, span) => {
                Expr::List(items.into_iter().map(|e| rexpr(e, u)).collect(), span)
            }
            Expr::Map(pairs, span) => Expr::Map(
                pairs
                    .into_iter()
                    .map(|(k, v)| (rexpr(k, u), rexpr(v, u)))
                    .collect(),
                span,
            ),
            Expr::Unary { op, expr, span } => Expr::Unary {
                op,
                expr: Box::new(rexpr(*expr, u)),
                span,
            },
            Expr::Binary { op, lhs, rhs, span } => Expr::Binary {
                op,
                lhs: Box::new(rexpr(*lhs, u)),
                rhs: Box::new(rexpr(*rhs, u)),
                span,
            },
            Expr::InstanceOf {
                value,
                type_name,
                span,
            } => Expr::InstanceOf {
                value: Box::new(rexpr(*value, u)),
                type_name,
                span,
            },
            // A primitive `as`-cast the checker rewrote to a native conversion call (M4 as-matrix),
            // keyed by the `Cast` node's span (the `as` token). `apply_repl` re-walks the embedded
            // original value (its span differs from this key) but reconstructs the call root directly,
            // so it never re-matches this key. An identity cast is not recorded → falls to the None
            // branch (the `Cast` survives; each backend emits the value).
            Expr::Cast {
                value,
                type_name,
                span,
            } => match u.get(&span.start) {
                Some(repl) => apply_repl(repl, u),
                None => Expr::Cast {
                    value: Box::new(rexpr(*value, u)),
                    type_name,
                    span,
                },
            },
            Expr::Member {
                object,
                name,
                safe,
                span,
            } => Expr::Member {
                object: Box::new(rexpr(*object, u)),
                name,
                safe,
                span,
            },
            Expr::Index {
                object,
                index,
                span,
            } => Expr::Index {
                object: Box::new(rexpr(*object, u)),
                index: Box::new(rexpr(*index, u)),
                span,
            },
            Expr::Force { inner, span } => Expr::Force {
                inner: Box::new(rexpr(*inner, u)),
                span,
            },
            // A return-overload selector `<Type>f(args)` (M-RT Slice C1): the checker recorded the
            // resolved mangled `Call` keyed by this node's span. A successful check guarantees an
            // entry (an unresolved selector is a hard error → the program never reaches a backend);
            // `apply_repl` re-walks the embedded args without re-matching this key.
            Expr::OverloadSelect { ty, call, span } => match u.get(&span.start) {
                Some(repl) => apply_repl(repl, u),
                None => Expr::OverloadSelect {
                    ty,
                    call: Box::new(rexpr(*call, u)),
                    span,
                },
            },
            Expr::Propagate { inner, span } => Expr::Propagate {
                inner: Box::new(rexpr(*inner, u)),
                span,
            },
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => Expr::Match {
                scrutinee: Box::new(rexpr(*scrutinee, u)),
                arms: arms
                    .into_iter()
                    .map(|a| MatchArm {
                        pattern: a.pattern,
                        guard: a.guard.map(|g| rexpr(g, u)),
                        body: rexpr(a.body, u),
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
                start: Box::new(rexpr(*start, u)),
                end: Box::new(rexpr(*end, u)),
                inclusive,
                span,
            },
            Expr::If {
                cond,
                then_expr,
                else_expr,
                span,
            } => Expr::If {
                cond: Box::new(rexpr(*cond, u)),
                then_expr: Box::new(rexpr(*then_expr, u)),
                else_expr: Box::new(rexpr(*else_expr, u)),
                span,
            },
            Expr::Lambda {
                params,
                ret,
                body,
                span,
            } => Expr::Lambda {
                params,
                ret,
                body: match body {
                    LambdaBody::Expr(e) => LambdaBody::Expr(Box::new(rexpr(*e, u))),
                    LambdaBody::Block(stmts) => LambdaBody::Block(rblock(stmts, u)),
                },
                span,
            },
            Expr::CloneWith {
                object,
                fields,
                span,
            } => Expr::CloneWith {
                object: Box::new(rexpr(*object, u)),
                fields: fields.into_iter().map(|(n, e)| (n, rexpr(e, u))).collect(),
                span,
            },
            Expr::New(inner, span) => Expr::New(Box::new(rexpr(*inner, u)), span),
            Expr::Html(parts, span) => Expr::Html(parts, span),
            // leaves carry no nested expression: Int / Float / Bool / Null / Bytes / Ident / This
            leaf => leaf,
        }
    }

    fn rstmt(s: Stmt, u: &Map) -> Stmt {
        match s {
            Stmt::VarDecl {
                ty,
                name,
                init,
                mutable,
                span,
            } => Stmt::VarDecl {
                ty,
                name,
                init: rexpr(init, u),
                mutable,
                span,
            },
            Stmt::Assign {
                target,
                value,
                span,
            } => Stmt::Assign {
                target: rexpr(target, u),
                value: rexpr(value, u),
                span,
            },
            Stmt::Return { value, span } => Stmt::Return {
                value: value.map(|e| rexpr(e, u)),
                span,
            },
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                span,
            } => Stmt::If {
                cond: rexpr(cond, u),
                bind,
                then_block: rblock(then_block, u),
                else_block: else_block.map(|b| rblock(b, u)),
                span,
            },
            Stmt::For {
                ty,
                name,
                iter,
                body,
                span,
            } => Stmt::For {
                ty,
                name,
                iter: rexpr(iter, u),
                body: rblock(body, u),
                span,
            },
            Stmt::While {
                cond,
                body,
                post_cond,
                span,
            } => Stmt::While {
                cond: rexpr(cond, u),
                body: rblock(body, u),
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
                init: init.map(|s| Box::new(rstmt(*s, u))),
                cond: cond.map(|e| rexpr(e, u)),
                step: step.map(|s| Box::new(rstmt(*s, u))),
                body: rblock(body, u),
                span,
            },
            Stmt::Break(span) => Stmt::Break(span),
            Stmt::Continue(span) => Stmt::Continue(span),
            Stmt::Block(stmts, span) => Stmt::Block(rblock(stmts, u), span),
            Stmt::Expr(e, span) => Stmt::Expr(rexpr(e, u), span),
            Stmt::Discard(e, span) => Stmt::Discard(rexpr(e, u), span),
            Stmt::Throw { value, span } => Stmt::Throw {
                value: rexpr(value, u),
                span,
            },
            Stmt::Try {
                body,
                catches,
                finally_block,
                span,
            } => Stmt::Try {
                body: rblock(body, u),
                catches: catches
                    .into_iter()
                    .map(|c| crate::ast::CatchClause {
                        ty: c.ty,
                        name: c.name,
                        body: rblock(c.body, u),
                        span: c.span,
                    })
                    .collect(),
                finally_block: finally_block.map(|b| rblock(b, u)),
                span,
            },
            Stmt::Destructure {
                pat,
                init,
                else_block,
                span,
            } => Stmt::Destructure {
                pat,
                init: rexpr(init, u),
                else_block: else_block.map(|b| rblock(b, u)),
                span,
            },
        }
    }

    fn rblock(stmts: Vec<Stmt>, u: &Map) -> Vec<Stmt> {
        stmts.into_iter().map(|s| rstmt(s, u)).collect()
    }

    let items = program
        .items
        .into_iter()
        .map(|item| match item {
            Item::Function(mut f) => {
                f.body = rblock(f.body, ufcs);
                Item::Function(f)
            }
            Item::Class(mut c) => {
                for m in &mut c.members {
                    match m {
                        ClassMember::Method(f) => {
                            let body = std::mem::take(&mut f.body);
                            f.body = rblock(body, ufcs);
                        }
                        ClassMember::Constructor { body, .. } => {
                            let b = std::mem::take(body);
                            *body = rblock(b, ufcs);
                        }
                        ClassMember::Hook { get, set, .. } => {
                            if let Some(e) = get.take() {
                                *get = Some(rexpr(e, ufcs));
                            }
                            if let Some((p, body)) = set.take() {
                                *set = Some((p, rblock(body, ufcs)));
                            }
                        }
                        // A field initializer (Feature B) may contain UFCS — rewrite it (resolve_html
                        // skips fields, but the checker checks field-init expressions, so a recorded
                        // UFCS site here must be applied or the backend would see the raw member call).
                        ClassMember::Field { init, .. } => {
                            if let Some(e) = init.take() {
                                *init = Some(rexpr(e, ufcs));
                            }
                        }
                    }
                }
                Item::Class(c)
            }
            other => other,
        })
        .collect();

    Program {
        package: program.package,
        items,
        span: program.span,
    }
}
