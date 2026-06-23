use super::*;

/// Replace every `html"…"` literal ([`crate::ast::Expr::Html`]) with its checker-built
/// `html.concat([…])` desugaring (keyed by `Span.start`), so the interpreter, compiler, and
/// transpiler never see the node — the same "compile-time sugar, erased before backends" treatment
/// as `type` aliases. Runs after a successful [`check_resolutions`]; mirrors the owned-AST rewrite
/// walk in `loader::resolve_*`, but also descends into lambda bodies (an `html"…"` may appear there
/// too). A replacement can itself embed an `html"…"` (an Html-typed hole), so the rewrite recurses
/// into each substituted subtree. When no literal was found the program is returned untouched, so
/// programs with no `html"…"` are byte-for-byte identical to the pre-Wave-3 AST.
pub fn resolve_html(program: Program, html: &HashMap<usize, crate::ast::Expr>) -> Program {
    use crate::ast::{ClassMember, Expr, Item, LambdaBody, MatchArm, Stmt, StrPart};
    if html.is_empty() {
        return program;
    }
    type Map = HashMap<usize, Expr>;

    fn rexpr(e: Expr, h: &Map) -> Expr {
        match e {
            Expr::Html(parts, span) => match h.get(&span.start) {
                // Re-walk the substituted tree: an Html-typed hole embeds another `html"…"`.
                Some(r) => rexpr(r.clone(), h),
                None => Expr::Html(parts, span), // defensive; check populated every literal
            },
            Expr::Str(parts, span) => Expr::Str(
                parts
                    .into_iter()
                    .map(|p| match p {
                        StrPart::Expr(e) => StrPart::Expr(Box::new(rexpr(*e, h))),
                        lit => lit,
                    })
                    .collect(),
                span,
            ),
            Expr::List(items, span) => {
                Expr::List(items.into_iter().map(|e| rexpr(e, h)).collect(), span)
            }
            Expr::Map(pairs, span) => Expr::Map(
                pairs
                    .into_iter()
                    .map(|(k, v)| (rexpr(k, h), rexpr(v, h)))
                    .collect(),
                span,
            ),
            Expr::Unary { op, expr, span } => Expr::Unary {
                op,
                expr: Box::new(rexpr(*expr, h)),
                span,
            },
            Expr::Binary { op, lhs, rhs, span } => Expr::Binary {
                op,
                lhs: Box::new(rexpr(*lhs, h)),
                rhs: Box::new(rexpr(*rhs, h)),
                span,
            },
            Expr::Call { callee, args, span } => Expr::Call {
                callee: Box::new(rexpr(*callee, h)),
                args: args.into_iter().map(|a| rexpr(a, h)).collect(),
                span,
            },
            Expr::Member {
                object,
                name,
                safe,
                span,
            } => Expr::Member {
                object: Box::new(rexpr(*object, h)),
                name,
                safe,
                span,
            },
            Expr::Index {
                object,
                index,
                span,
            } => Expr::Index {
                object: Box::new(rexpr(*object, h)),
                index: Box::new(rexpr(*index, h)),
                span,
            },
            Expr::Force { inner, span } => Expr::Force {
                inner: Box::new(rexpr(*inner, h)),
                span,
            },
            // A throws-mode `?` was recorded for erasure (its `Span.start` is in `h`, mapping to the
            // inner call): unwrap it to the bare call — the call's own throw unwinds, so no backend
            // ever sees a throws-mode `Propagate`. A Result-mode `?` is absent from `h` and kept.
            Expr::Propagate { inner, span } => match h.get(&span.start) {
                Some(r) => rexpr(r.clone(), h),
                None => Expr::Propagate {
                    inner: Box::new(rexpr(*inner, h)),
                    span,
                },
            },
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => Expr::Match {
                scrutinee: Box::new(rexpr(*scrutinee, h)),
                arms: arms
                    .into_iter()
                    .map(|a| MatchArm {
                        pattern: a.pattern,
                        body: rexpr(a.body, h),
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
                start: Box::new(rexpr(*start, h)),
                end: Box::new(rexpr(*end, h)),
                inclusive,
                span,
            },
            Expr::If {
                cond,
                then_expr,
                else_expr,
                span,
            } => Expr::If {
                cond: Box::new(rexpr(*cond, h)),
                then_expr: Box::new(rexpr(*then_expr, h)),
                else_expr: Box::new(rexpr(*else_expr, h)),
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
                    LambdaBody::Expr(e) => LambdaBody::Expr(Box::new(rexpr(*e, h))),
                    LambdaBody::Block(stmts) => LambdaBody::Block(rblock(stmts, h)),
                },
                span,
            },
            Expr::CloneWith {
                object,
                fields,
                span,
            } => Expr::CloneWith {
                object: Box::new(rexpr(*object, h)),
                fields: fields.into_iter().map(|(n, e)| (n, rexpr(e, h))).collect(),
                span,
            },
            // leaves carry no nested expression: Int / Float / Bool / Null / Bytes / Ident / This
            leaf => leaf,
        }
    }

    fn rstmt(s: Stmt, h: &Map) -> Stmt {
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
                init: rexpr(init, h),
                mutable,
                span,
            },
            Stmt::Assign {
                target,
                value,
                span,
            } => Stmt::Assign {
                target: rexpr(target, h),
                value: rexpr(value, h),
                span,
            },
            Stmt::Return { value, span } => Stmt::Return {
                value: value.map(|e| rexpr(e, h)),
                span,
            },
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                span,
            } => Stmt::If {
                cond: rexpr(cond, h),
                bind,
                then_block: rblock(then_block, h),
                else_block: else_block.map(|b| rblock(b, h)),
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
                iter: rexpr(iter, h),
                body: rblock(body, h),
                span,
            },
            Stmt::While {
                cond,
                body,
                post_cond,
                span,
            } => Stmt::While {
                cond: rexpr(cond, h),
                body: rblock(body, h),
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
                init: init.map(|s| Box::new(rstmt(*s, h))),
                cond: cond.map(|e| rexpr(e, h)),
                step: step.map(|s| Box::new(rstmt(*s, h))),
                body: rblock(body, h),
                span,
            },
            Stmt::Break(span) => Stmt::Break(span),
            Stmt::Continue(span) => Stmt::Continue(span),
            Stmt::Block(stmts, span) => Stmt::Block(rblock(stmts, h), span),
            Stmt::Expr(e, span) => Stmt::Expr(rexpr(e, h), span),
            Stmt::Throw { value, span } => Stmt::Throw {
                value: rexpr(value, h),
                span,
            },
            Stmt::Try {
                body,
                catches,
                finally_block,
                span,
            } => Stmt::Try {
                body: rblock(body, h),
                catches: catches
                    .into_iter()
                    .map(|c| crate::ast::CatchClause {
                        ty: c.ty,
                        name: c.name,
                        body: rblock(c.body, h),
                        span: c.span,
                    })
                    .collect(),
                finally_block: finally_block.map(|b| rblock(b, h)),
                span,
            },
        }
    }

    fn rblock(stmts: Vec<Stmt>, h: &Map) -> Vec<Stmt> {
        stmts.into_iter().map(|s| rstmt(s, h)).collect()
    }

    let items = program
        .items
        .into_iter()
        .map(|item| match item {
            Item::Function(mut f) => {
                f.body = rblock(f.body, html);
                Item::Function(f)
            }
            Item::Class(mut c) => {
                for m in &mut c.members {
                    match m {
                        ClassMember::Method(f) => {
                            let body = std::mem::take(&mut f.body);
                            f.body = rblock(body, html);
                        }
                        ClassMember::Constructor { body, .. } => {
                            let b = std::mem::take(body);
                            *body = rblock(b, html);
                        }
                        // A property hook's get expression + set block may contain `html"…"`
                        // interpolation — rewrite both (M-mut.7b).
                        ClassMember::Hook { get, set, .. } => {
                            if let Some(e) = get.take() {
                                *get = Some(rexpr(e, html));
                            }
                            if let Some((p, body)) = set.take() {
                                *set = Some((p, rblock(body, html)));
                            }
                        }
                        ClassMember::Field { .. } => {}
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
