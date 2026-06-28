//! `Http.autoRouter()` compile-time desugar (M6 W2).
//!
//! Collects every free function annotated `#[Route("METHOD", "/pattern")]` (in source order) and
//! rewrites each `Http.autoRouter()` call into an explicit router construction —
//! `new Router([]).route("M1", "/p1", fn1).route("M2", "/p2", fn2) …` — referencing each handler as a
//! first-class function value. This runs in `cli::check_and_expand`'s injection chain **before** the
//! type-checker, so the generated registration is type-checked like hand-written code and every
//! backend sees the *same* explicit `.route(…)` chain (the expand-before-backends discipline ⇒
//! byte-identity is trivial, with no runtime attribute machinery). The `#[Route]` attributes stay on
//! the functions for the checker's validation pass, then are inert for the backends.
//!
//! Loop-safe by construction: the synthesized router expression contains only `new`/`.route(…)` calls
//! — never an `Http.autoRouter()` — so re-walking it can match nothing. The walker mirrors
//! `rewrite_ufcs::rexpr` (the proven complete Expr/Stmt walk); the one behavioural difference is the
//! `Expr::Call` arm, which substitutes a freshly built router for an `Http.autoRouter()` shape.

use crate::ast::{
    CatchClause, ClassMember, Expr, Item, LambdaBody, MatchArm, Program, Stmt, StrPart,
};
use crate::token::Span;

/// One collected route: the `#[Route]` method literal, the pattern literal (both kept as the original
/// argument `Expr`s, so a raw-string pattern survives), and the handler function name.
type Route = (Expr, Expr, String);

/// Rewrite `Http.autoRouter()` calls into explicit `Router` construction. A no-op (returns the program
/// unchanged) unless `Core.Http` is imported — so a user's own unrelated `Http.autoRouter()` is never
/// touched when the web layer isn't in play.
pub fn desugar_auto_router(program: Program) -> Program {
    let imports_http = program.items.iter().any(|it| {
        matches!(it, Item::Import { path, type_only: false, .. }
            if path.len() == 2 && path[0] == "Core" && path[1] == "Http")
    });
    if !imports_http {
        return program;
    }
    let routes = collect_routes(&program);

    let items = program
        .items
        .into_iter()
        .map(|item| match item {
            Item::Function(mut f) => {
                f.body = rblock(f.body, &routes);
                Item::Function(f)
            }
            Item::Class(mut c) => {
                for m in &mut c.members {
                    match m {
                        ClassMember::Method(f) => {
                            let body = std::mem::take(&mut f.body);
                            f.body = rblock(body, &routes);
                        }
                        ClassMember::Constructor { body, .. } => {
                            let b = std::mem::take(body);
                            *body = rblock(b, &routes);
                        }
                        ClassMember::Hook { get, set, .. } => {
                            if let Some(e) = get.take() {
                                *get = Some(rexpr(e, &routes));
                            }
                            if let Some((p, body)) = set.take() {
                                *set = Some((p, rblock(body, &routes)));
                            }
                        }
                        ClassMember::Field { init, .. } => {
                            if let Some(e) = init.take() {
                                *init = Some(rexpr(e, &routes));
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

/// Every free function carrying a well-formed `#[Route(method, pattern)]`, in source order. A
/// malformed `Route` (wrong arg count) is skipped here — the checker reports it (`E-ROUTE-ARGS`).
fn collect_routes(program: &Program) -> Vec<Route> {
    let mut out = Vec::new();
    for it in &program.items {
        if let Item::Function(f) = it {
            for attr in &f.attrs {
                if attr.name == "Route" && attr.args.len() == 2 {
                    out.push((attr.args[0].clone(), attr.args[1].clone(), f.name.clone()));
                }
            }
        }
    }
    out
}

/// `new Router([]).route(m1, p1, fn1).route(m2, p2, fn2) …` — built fresh at each call site (the
/// handler refs are bare `Ident`s = first-class function values). All synthesized wrapper nodes carry
/// the `Http.autoRouter()` call's span, so any downstream type error points at the call site.
fn build_router(routes: &[Route], sp: Span) -> Expr {
    let mut e = Expr::New(
        Box::new(Expr::Call {
            callee: Box::new(Expr::Ident("Router".into(), sp)),
            // `new Router([], [])` — empty route table + empty middleware list (M6 W2-ext slice 1).
            args: vec![Expr::List(Vec::new(), sp), Expr::List(Vec::new(), sp)],
            span: sp,
        }),
        sp,
    );
    for (method, pattern, name) in routes {
        e = Expr::Call {
            callee: Box::new(Expr::Member {
                object: Box::new(e),
                name: "route".into(),
                safe: false,
                span: sp,
            }),
            args: vec![
                method.clone(),
                pattern.clone(),
                Expr::Ident(name.clone(), sp),
            ],
            span: sp,
        };
    }
    e
}

/// Is this `callee(args)` an `Http.autoRouter()` (no-arg, the exact `Http.autoRouter` member shape)?
fn is_auto_router(callee: &Expr, args: &[Expr]) -> bool {
    if !args.is_empty() {
        return false;
    }
    matches!(callee, Expr::Member { object, name, safe: false, .. }
        if name == "autoRouter"
            && matches!(object.as_ref(), Expr::Ident(q, _) if q == "Http"))
}

fn rexpr(e: Expr, r: &[Route]) -> Expr {
    match e {
        Expr::Call { callee, args, span } => {
            if is_auto_router(&callee, &args) {
                build_router(r, span)
            } else {
                Expr::Call {
                    callee: Box::new(rexpr(*callee, r)),
                    args: args.into_iter().map(|a| rexpr(a, r)).collect(),
                    span,
                }
            }
        }
        Expr::Str(parts, span) => Expr::Str(
            parts
                .into_iter()
                .map(|p| match p {
                    StrPart::Expr(e) => StrPart::Expr(Box::new(rexpr(*e, r))),
                    lit => lit,
                })
                .collect(),
            span,
        ),
        Expr::List(items, span) => {
            Expr::List(items.into_iter().map(|e| rexpr(e, r)).collect(), span)
        }
        Expr::Map(pairs, span) => Expr::Map(
            pairs
                .into_iter()
                .map(|(k, v)| (rexpr(k, r), rexpr(v, r)))
                .collect(),
            span,
        ),
        Expr::Unary { op, expr, span } => Expr::Unary {
            op,
            expr: Box::new(rexpr(*expr, r)),
            span,
        },
        Expr::Binary { op, lhs, rhs, span } => Expr::Binary {
            op,
            lhs: Box::new(rexpr(*lhs, r)),
            rhs: Box::new(rexpr(*rhs, r)),
            span,
        },
        Expr::InstanceOf {
            value,
            type_name,
            span,
        } => Expr::InstanceOf {
            value: Box::new(rexpr(*value, r)),
            type_name,
            span,
        },
        Expr::Cast {
            value,
            type_name,
            span,
        } => Expr::Cast {
            value: Box::new(rexpr(*value, r)),
            type_name,
            span,
        },
        Expr::Member {
            object,
            name,
            safe,
            span,
        } => Expr::Member {
            object: Box::new(rexpr(*object, r)),
            name,
            safe,
            span,
        },
        Expr::Index {
            object,
            index,
            span,
        } => Expr::Index {
            object: Box::new(rexpr(*object, r)),
            index: Box::new(rexpr(*index, r)),
            span,
        },
        Expr::Force { inner, span } => Expr::Force {
            inner: Box::new(rexpr(*inner, r)),
            span,
        },
        Expr::Propagate { inner, span } => Expr::Propagate {
            inner: Box::new(rexpr(*inner, r)),
            span,
        },
        Expr::Match {
            scrutinee,
            arms,
            span,
        } => Expr::Match {
            scrutinee: Box::new(rexpr(*scrutinee, r)),
            arms: arms
                .into_iter()
                .map(|a| MatchArm {
                    pattern: a.pattern,
                    guard: a.guard.map(|g| rexpr(g, r)),
                    body: rexpr(a.body, r),
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
            start: Box::new(rexpr(*start, r)),
            end: Box::new(rexpr(*end, r)),
            inclusive,
            span,
        },
        Expr::If {
            cond,
            then_expr,
            else_expr,
            span,
        } => Expr::If {
            cond: Box::new(rexpr(*cond, r)),
            then_expr: Box::new(rexpr(*then_expr, r)),
            else_expr: Box::new(rexpr(*else_expr, r)),
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
                LambdaBody::Expr(e) => LambdaBody::Expr(Box::new(rexpr(*e, r))),
                LambdaBody::Block(stmts) => LambdaBody::Block(rblock(stmts, r)),
            },
            span,
        },
        Expr::CloneWith {
            object,
            fields,
            span,
        } => Expr::CloneWith {
            object: Box::new(rexpr(*object, r)),
            fields: fields.into_iter().map(|(n, e)| (n, rexpr(e, r))).collect(),
            span,
        },
        Expr::New(inner, span) => Expr::New(Box::new(rexpr(*inner, r)), span),
        Expr::Html(parts, span) => Expr::Html(parts, span),
        // leaves carry no nested expression: Int / Float / Bool / Null / Bytes / Ident / This
        leaf => leaf,
    }
}

fn rstmt(s: Stmt, r: &[Route]) -> Stmt {
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
            init: rexpr(init, r),
            mutable,
            span,
        },
        Stmt::Assign {
            target,
            value,
            span,
        } => Stmt::Assign {
            target: rexpr(target, r),
            value: rexpr(value, r),
            span,
        },
        Stmt::Return { value, span } => Stmt::Return {
            value: value.map(|e| rexpr(e, r)),
            span,
        },
        Stmt::If {
            cond,
            bind,
            then_block,
            else_block,
            span,
        } => Stmt::If {
            cond: rexpr(cond, r),
            bind,
            then_block: rblock(then_block, r),
            else_block: else_block.map(|b| rblock(b, r)),
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
            iter: rexpr(iter, r),
            body: rblock(body, r),
            span,
        },
        Stmt::While {
            cond,
            body,
            post_cond,
            span,
        } => Stmt::While {
            cond: rexpr(cond, r),
            body: rblock(body, r),
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
            init: init.map(|s| Box::new(rstmt(*s, r))),
            cond: cond.map(|e| rexpr(e, r)),
            step: step.map(|s| Box::new(rstmt(*s, r))),
            body: rblock(body, r),
            span,
        },
        Stmt::Break(span) => Stmt::Break(span),
        Stmt::Continue(span) => Stmt::Continue(span),
        Stmt::Block(stmts, span) => Stmt::Block(rblock(stmts, r), span),
        Stmt::Expr(e, span) => Stmt::Expr(rexpr(e, r), span),
        Stmt::Throw { value, span } => Stmt::Throw {
            value: rexpr(value, r),
            span,
        },
        Stmt::Try {
            body,
            catches,
            finally_block,
            span,
        } => Stmt::Try {
            body: rblock(body, r),
            catches: catches
                .into_iter()
                .map(|c| CatchClause {
                    ty: c.ty,
                    name: c.name,
                    body: rblock(c.body, r),
                    span: c.span,
                })
                .collect(),
            finally_block: finally_block.map(|b| rblock(b, r)),
            span,
        },
        Stmt::Destructure {
            pat,
            init,
            else_block,
            span,
        } => Stmt::Destructure {
            pat,
            init: rexpr(init, r),
            else_block: else_block.map(|b| rblock(b, r)),
            span,
        },
    }
}

fn rblock(stmts: Vec<Stmt>, r: &[Route]) -> Vec<Stmt> {
    stmts.into_iter().map(|s| rstmt(s, r)).collect()
}
