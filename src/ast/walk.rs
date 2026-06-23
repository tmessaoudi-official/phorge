//! Free-variable analysis over the AST (M-Decomp W3.3) — the closure-capture walkers
//! (`free_vars` + its `collect_free_*` helpers). Re-exported from `ast`.

use super::*;

/// Compute the **sorted** free variables of a lambda: identifiers referenced in `body`
/// that are NOT the lambda's own params, NOT locals bound inside the body (`var`,
/// `if (var …)`, `for (T x in …)`, match-arm bindings, nested-lambda params), and NOT
/// `this`.
///
/// The result is sorted (invariant #8: deterministic capture order for all backends).
///
/// **Note:** over-reporting is acceptable — a global function name may appear in the
/// result if it is also used as an identifier reference. Call-site consumers (the
/// interpreter, compiler) filter it out by checking whether the name resolves to a
/// function or a local. Under-reporting (missing a real capture) is a correctness bug.
pub fn free_vars(params: &[Param], body: &LambdaBody) -> Vec<String> {
    let mut bound: std::collections::HashSet<String> =
        params.iter().map(|p| p.name.clone()).collect();
    let mut found: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    match body {
        LambdaBody::Expr(e) => collect_free_expr(e, &mut bound, &mut found),
        LambdaBody::Block(stmts) => collect_free_block(stmts, &mut bound, &mut found),
    }
    found.into_iter().collect()
}

fn collect_free_expr(
    e: &Expr,
    bound: &mut std::collections::HashSet<String>,
    found: &mut std::collections::BTreeSet<String>,
) {
    match e {
        Expr::Ident(name, _) => {
            if !bound.contains(name) {
                found.insert(name.clone());
            }
        }
        Expr::This(_) => {} // `this` is never captured (E-LAMBDA-THIS rejects it at check time)
        Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Null(..) | Expr::Bytes(..) => {}
        Expr::Str(parts, _) | Expr::Html(parts, _) => {
            for part in parts {
                if let StrPart::Expr(inner) = part {
                    collect_free_expr(inner, bound, found);
                }
            }
        }
        Expr::List(items, _) => {
            for it in items {
                collect_free_expr(it, bound, found);
            }
        }
        Expr::Map(pairs, _) => {
            for (k, v) in pairs {
                collect_free_expr(k, bound, found);
                collect_free_expr(v, bound, found);
            }
        }
        Expr::Unary { expr, .. } => collect_free_expr(expr, bound, found),
        Expr::Binary { lhs, rhs, .. } => {
            collect_free_expr(lhs, bound, found);
            collect_free_expr(rhs, bound, found);
        }
        Expr::InstanceOf { value, .. } => collect_free_expr(value, bound, found),
        Expr::Call { callee, args, .. } => {
            collect_free_expr(callee, bound, found);
            for a in args {
                collect_free_expr(a, bound, found);
            }
        }
        Expr::Member { object, .. } => collect_free_expr(object, bound, found),
        Expr::Index { object, index, .. } => {
            collect_free_expr(object, bound, found);
            collect_free_expr(index, bound, found);
        }
        Expr::Force { inner, .. } => collect_free_expr(inner, bound, found),
        Expr::Propagate { inner, .. } => collect_free_expr(inner, bound, found),
        Expr::CloneWith { object, fields, .. } => {
            collect_free_expr(object, bound, found);
            for (_, e) in fields {
                collect_free_expr(e, bound, found);
            }
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            collect_free_expr(scrutinee, bound, found);
            for arm in arms {
                // arm-pattern bindings are in scope for the arm guard and body only
                let mut arm_bound = bound.clone();
                collect_pattern_bindings(&arm.pattern, &mut arm_bound);
                if let Some(g) = &arm.guard {
                    collect_free_expr(g, &mut arm_bound, found);
                }
                collect_free_expr(&arm.body, &mut arm_bound, found);
            }
        }
        Expr::Range { start, end, .. } => {
            collect_free_expr(start, bound, found);
            collect_free_expr(end, bound, found);
        }
        Expr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_free_expr(cond, bound, found);
            collect_free_expr(then_expr, bound, found);
            collect_free_expr(else_expr, bound, found);
        }
        Expr::Lambda { params, body, .. } => {
            // Nested lambda: its params shadow outer names; walk the body with an extended
            // bound set (but do NOT add its params to the outer `bound` set).
            let mut inner_bound = bound.clone();
            for p in params {
                inner_bound.insert(p.name.clone());
            }
            match body {
                LambdaBody::Expr(inner_e) => collect_free_expr(inner_e, &mut inner_bound, found),
                LambdaBody::Block(stmts) => collect_free_block(stmts, &mut inner_bound, found),
            }
        }
    }
}

fn collect_free_block(
    stmts: &[Stmt],
    bound: &mut std::collections::HashSet<String>,
    found: &mut std::collections::BTreeSet<String>,
) {
    for s in stmts {
        collect_free_stmt(s, bound, found);
    }
}

pub(super) fn collect_free_stmt(
    s: &Stmt,
    bound: &mut std::collections::HashSet<String>,
    found: &mut std::collections::BTreeSet<String>,
) {
    match s {
        Stmt::VarDecl { name, init, .. } => {
            // The initializer is evaluated before the name enters scope
            collect_free_expr(init, bound, found);
            bound.insert(name.clone());
        }
        Stmt::Return { value, .. } => {
            if let Some(e) = value {
                collect_free_expr(e, bound, found);
            }
        }
        Stmt::If {
            cond,
            bind,
            then_block,
            else_block,
            ..
        } => {
            collect_free_expr(cond, bound, found);
            let mut then_bound = bound.clone();
            if let Some(name) = bind {
                then_bound.insert(name.clone());
            }
            collect_free_block(then_block, &mut then_bound, found);
            if let Some(eb) = else_block {
                let mut else_bound = bound.clone();
                collect_free_block(eb, &mut else_bound, found);
            }
        }
        Stmt::For {
            name, iter, body, ..
        } => {
            collect_free_expr(iter, bound, found);
            let mut loop_bound = bound.clone();
            loop_bound.insert(name.clone());
            collect_free_block(body, &mut loop_bound, found);
        }
        Stmt::While { cond, body, .. } => {
            collect_free_expr(cond, bound, found);
            let mut loop_bound = bound.clone();
            collect_free_block(body, &mut loop_bound, found);
        }
        Stmt::CFor {
            init,
            cond,
            step,
            body,
            ..
        } => {
            // `init` declares into the loop's own scope; `cond`/`step`/`body` see those bindings.
            let mut loop_bound = bound.clone();
            if let Some(s) = init {
                collect_free_stmt(s, &mut loop_bound, found);
            }
            if let Some(c) = cond {
                collect_free_expr(c, &mut loop_bound, found);
            }
            if let Some(s) = step {
                collect_free_stmt(s, &mut loop_bound, found);
            }
            collect_free_block(body, &mut loop_bound, found);
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Block(stmts, _) => {
            let mut inner = bound.clone();
            collect_free_block(stmts, &mut inner, found);
        }
        Stmt::Assign { target, value, .. } => {
            // Reassignment: the target names an existing binding (a use, not a new binding),
            // and the value is evaluated against the current scope.
            collect_free_expr(target, bound, found);
            collect_free_expr(value, bound, found);
        }
        Stmt::Expr(e, _) => collect_free_expr(e, bound, found),
        Stmt::Throw { value, .. } => collect_free_expr(value, bound, found),
        Stmt::Try {
            body,
            catches,
            finally_block,
            ..
        } => {
            let mut try_bound = bound.clone();
            collect_free_block(body, &mut try_bound, found);
            for c in catches {
                // The catch binding is in scope only inside its own clause body.
                let mut catch_bound = bound.clone();
                catch_bound.insert(c.name.clone());
                collect_free_block(&c.body, &mut catch_bound, found);
            }
            if let Some(fb) = finally_block {
                let mut fin_bound = bound.clone();
                collect_free_block(fb, &mut fin_bound, found);
            }
        }
    }
}

fn collect_pattern_bindings(pat: &Pattern, bound: &mut std::collections::HashSet<String>) {
    match pat {
        Pattern::Binding { name, .. } => {
            bound.insert(name.clone());
        }
        Pattern::Variant { fields, .. } => {
            for f in fields {
                collect_pattern_bindings(f, bound);
            }
        }
        // A type pattern (`Circle c`, M-RT S4) binds its `binding` (if any) for the arm body.
        Pattern::Type {
            binding: Some(name),
            ..
        } => {
            bound.insert(name.clone());
        }
        // A struct pattern (`Point { x, y }`, S5.2) binds via each field's sub-pattern (recurse —
        // a nested struct or rename binds too). Missing this would drop struct-bound names from
        // `free_vars`, miscompiling a lambda that captures one (the guard-recursion lesson).
        Pattern::Struct { fields, .. } => {
            for f in fields {
                collect_pattern_bindings(&f.pat, bound);
            }
        }
        _ => {}
    }
}
