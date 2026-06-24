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

    fn rexpr(e: Expr, u: &Map) -> Expr {
        match e {
            // A resolved UFCS call: emit the recorded free/native call, re-walking its children for
            // nested UFCS but NOT re-matching this span (the recorded node carries the key span).
            Expr::Call { callee, args, span } => match u.get(&span.start) {
                Some(Expr::Call {
                    callee: rc,
                    args: ra,
                    span: rs,
                }) => Expr::Call {
                    callee: Box::new(rexpr((**rc).clone(), u)),
                    args: ra.iter().cloned().map(|a| rexpr(a, u)).collect(),
                    span: *rs,
                },
                // Defensive: only `Call` replacements are ever recorded. Clone without walking the
                // root (cannot recurse into a UFCS site, so no loop) for any other shape.
                Some(other) => other.clone(),
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
