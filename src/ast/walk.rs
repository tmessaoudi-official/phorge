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
        Expr::Int(..)
        | Expr::Float(..)
        | Expr::Decimal { .. }
        | Expr::Bool(..)
        | Expr::Null(..)
        | Expr::Bytes(..) => {}
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
        Expr::Cast { value, .. } => collect_free_expr(value, bound, found),
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
        // A `<Type>f(args)` selector (Slice C1) captures whatever its inner call captures.
        Expr::OverloadSelect { call, .. } => collect_free_expr(call, bound, found),
        // A `parent.m(args)` super call captures the free vars of its arguments (M-RT super/parent).
        Expr::ParentCall { args, .. } => {
            for a in args {
                collect_free_expr(a, bound, found);
            }
        }
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
        // `new <call>` (Feature C): captures whatever its inner construction captures.
        Expr::New(inner, _) => collect_free_expr(inner, bound, found),
        Expr::Spawn { call, .. } => collect_free_expr(call, bound, found),
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
        Stmt::Expr(e, _) | Stmt::Discard(e, _) => collect_free_expr(e, bound, found),
        Stmt::Throw { value, .. } => collect_free_expr(value, bound, found),
        // Slice 5: the initializer is evaluated before any binder enters scope; the `else` block (run
        // on the destructure-failed path) sees *none* of the binders; then the binders enter the
        // enclosing scope. Missing the binders here would drop them from `free_vars`, miscompiling a
        // lambda that captures one (the struct-pattern guard-recursion lesson).
        Stmt::Destructure {
            pat,
            init,
            else_block,
            ..
        } => {
            collect_free_expr(init, bound, found);
            if let Some(eb) = else_block {
                let mut else_bound = bound.clone();
                collect_free_block(eb, &mut else_bound, found);
            }
            for (name, _) in pat.binders() {
                bound.insert(name);
            }
        }
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

/// Returns `true` if the lambda body references `this`, **including transitively through nested
/// lambdas** (Phase 1 closures slice). Unlike `free_vars`, this recurses into nested lambda bodies:
/// if an inner lambda touches `this`, the outer lambda must also capture it (so `this` can flow
/// inward). Drives both the `this`-capture machinery (interpreter + VM) and the checker's
/// field-initializer guard (a field-default closure may not capture a partially-built `this`).
pub fn lambda_uses_this(body: &LambdaBody) -> bool {
    fn in_expr(e: &Expr) -> bool {
        match e {
            Expr::This(_) => true,
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Decimal { .. }
            | Expr::Bool(..)
            | Expr::Null(..)
            | Expr::Bytes(..)
            | Expr::Ident(..) => false,
            Expr::Str(parts, _) | Expr::Html(parts, _) => parts.iter().any(|p| match p {
                StrPart::Expr(inner) => in_expr(inner),
                _ => false,
            }),
            Expr::List(items, _) => items.iter().any(in_expr),
            Expr::Map(pairs, _) => pairs.iter().any(|(k, v)| in_expr(k) || in_expr(v)),
            Expr::Unary { expr, .. } => in_expr(expr),
            Expr::Binary { lhs, rhs, .. } => in_expr(lhs) || in_expr(rhs),
            Expr::InstanceOf { value, .. } => in_expr(value),
            Expr::Cast { value, .. } => in_expr(value),
            Expr::Call { callee, args, .. } => in_expr(callee) || args.iter().any(in_expr),
            Expr::Member { object, .. } => in_expr(object),
            Expr::Index { object, index, .. } => in_expr(object) || in_expr(index),
            Expr::Force { inner, .. } => in_expr(inner),
            Expr::Propagate { inner, .. } => in_expr(inner),
            Expr::OverloadSelect { call, .. } => in_expr(call),
            Expr::ParentCall { args, .. } => args.iter().any(in_expr),
            Expr::CloneWith { object, fields, .. } => {
                in_expr(object) || fields.iter().any(|(_, e)| in_expr(e))
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                in_expr(scrutinee)
                    || arms
                        .iter()
                        .any(|a| a.guard.as_ref().is_some_and(in_expr) || in_expr(&a.body))
            }
            Expr::Range { start, end, .. } => in_expr(start) || in_expr(end),
            Expr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => in_expr(cond) || in_expr(then_expr) || in_expr(else_expr),
            // Recurse into a nested lambda: a `this` inside it makes *this* lambda capture `this`
            // too (so it can thread the receiver inward). This is the key difference from the old
            // checker-only predicate, which stopped here because nesting was rejected.
            Expr::Lambda { body, .. } => match body {
                LambdaBody::Expr(e) => in_expr(e),
                LambdaBody::Block(stmts) => in_stmts(stmts),
            },
            Expr::New(inner, _) => in_expr(inner),
            Expr::Spawn { call, .. } => in_expr(call),
        }
    }
    fn in_stmts(stmts: &[Stmt]) -> bool {
        stmts.iter().any(|s| match s {
            Stmt::VarDecl { init, .. } => in_expr(init),
            Stmt::Return { value, .. } => value.as_ref().is_some_and(in_expr),
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                in_expr(cond)
                    || in_stmts(then_block)
                    || else_block.as_ref().is_some_and(|eb| in_stmts(eb))
            }
            Stmt::For { iter, body, .. } => in_expr(iter) || in_stmts(body),
            Stmt::While { cond, body, .. } => in_expr(cond) || in_stmts(body),
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => {
                init.as_deref()
                    .is_some_and(|s| in_stmts(std::slice::from_ref(s)))
                    || cond.as_ref().is_some_and(in_expr)
                    || step
                        .as_deref()
                        .is_some_and(|s| in_stmts(std::slice::from_ref(s)))
                    || in_stmts(body)
            }
            Stmt::Break(_) | Stmt::Continue(_) => false,
            Stmt::Assign { target, value, .. } => in_expr(target) || in_expr(value),
            // Slice 5: `this` may appear in the destructured init or the diverging `else` block.
            Stmt::Destructure {
                init, else_block, ..
            } => in_expr(init) || else_block.as_ref().is_some_and(|eb| in_stmts(eb)),
            Stmt::Block(stmts, _) => in_stmts(stmts),
            Stmt::Expr(e, _) | Stmt::Discard(e, _) => in_expr(e),
            Stmt::Throw { value, .. } => in_expr(value),
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                in_stmts(body)
                    || catches.iter().any(|c| in_stmts(&c.body))
                    || finally_block.as_ref().is_some_and(|fb| in_stmts(fb))
            }
        })
    }
    match body {
        LambdaBody::Expr(e) => in_expr(e),
        LambdaBody::Block(stmts) => in_stmts(stmts),
    }
}

/// Whether the program contains any `spawn` expression anywhere — the M6 W4 green-thread **gate**.
/// The run/runvm entry points branch on it: a program with **no** `spawn` takes the unchanged
/// synchronous path (byte-identical to today, zero risk to non-concurrent examples); one **with**
/// `spawn` takes the cooperative scheduler driver. Soundness note: we return `true` only on a real
/// [`Expr::Spawn`], so there are **no false positives** (the only risky direction — it would route a
/// non-spawning program through the cooperative driver). A false *negative* (a missed `spawn`) merely
/// degrades to the eager path, which still satisfies `run≡runvm` — so completeness is best-effort but
/// not load-bearing. Mirrors the exhaustive [`lambda_uses_this`] walker.
#[must_use]
pub fn uses_concurrency(program: &Program) -> bool {
    fn in_expr(e: &Expr) -> bool {
        match e {
            Expr::Spawn { .. } => true,
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Decimal { .. }
            | Expr::Bool(..)
            | Expr::Null(..)
            | Expr::Bytes(..)
            | Expr::Ident(..)
            | Expr::This(_) => false,
            Expr::Str(parts, _) | Expr::Html(parts, _) => parts.iter().any(|p| match p {
                StrPart::Expr(inner) => in_expr(inner),
                _ => false,
            }),
            Expr::List(items, _) => items.iter().any(in_expr),
            Expr::Map(pairs, _) => pairs.iter().any(|(k, v)| in_expr(k) || in_expr(v)),
            Expr::Unary { expr, .. } => in_expr(expr),
            Expr::Binary { lhs, rhs, .. } => in_expr(lhs) || in_expr(rhs),
            Expr::InstanceOf { value, .. } => in_expr(value),
            Expr::Cast { value, .. } => in_expr(value),
            Expr::Call { callee, args, .. } => in_expr(callee) || args.iter().any(in_expr),
            Expr::Member { object, .. } => in_expr(object),
            Expr::Index { object, index, .. } => in_expr(object) || in_expr(index),
            Expr::Force { inner, .. } => in_expr(inner),
            Expr::Propagate { inner, .. } => in_expr(inner),
            Expr::OverloadSelect { call, .. } => in_expr(call),
            Expr::ParentCall { args, .. } => args.iter().any(in_expr),
            Expr::CloneWith { object, fields, .. } => {
                in_expr(object) || fields.iter().any(|(_, e)| in_expr(e))
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                in_expr(scrutinee)
                    || arms
                        .iter()
                        .any(|a| a.guard.as_ref().is_some_and(in_expr) || in_expr(&a.body))
            }
            Expr::Range { start, end, .. } => in_expr(start) || in_expr(end),
            Expr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => in_expr(cond) || in_expr(then_expr) || in_expr(else_expr),
            Expr::Lambda { body, .. } => match body {
                LambdaBody::Expr(e) => in_expr(e),
                LambdaBody::Block(stmts) => in_stmts(stmts),
            },
            Expr::New(inner, _) => in_expr(inner),
        }
    }
    fn in_stmts(stmts: &[Stmt]) -> bool {
        stmts.iter().any(|s| match s {
            Stmt::VarDecl { init, .. } => in_expr(init),
            Stmt::Return { value, .. } => value.as_ref().is_some_and(in_expr),
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                in_expr(cond)
                    || in_stmts(then_block)
                    || else_block.as_ref().is_some_and(|eb| in_stmts(eb))
            }
            Stmt::For { iter, body, .. } => in_expr(iter) || in_stmts(body),
            Stmt::While { cond, body, .. } => in_expr(cond) || in_stmts(body),
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => {
                init.as_deref()
                    .is_some_and(|s| in_stmts(std::slice::from_ref(s)))
                    || cond.as_ref().is_some_and(in_expr)
                    || step
                        .as_deref()
                        .is_some_and(|s| in_stmts(std::slice::from_ref(s)))
                    || in_stmts(body)
            }
            Stmt::Break(_) | Stmt::Continue(_) => false,
            Stmt::Assign { target, value, .. } => in_expr(target) || in_expr(value),
            Stmt::Destructure {
                init, else_block, ..
            } => in_expr(init) || else_block.as_ref().is_some_and(|eb| in_stmts(eb)),
            Stmt::Block(stmts, _) => in_stmts(stmts),
            Stmt::Expr(e, _) | Stmt::Discard(e, _) => in_expr(e),
            Stmt::Throw { value, .. } => in_expr(value),
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                in_stmts(body)
                    || catches.iter().any(|c| in_stmts(&c.body))
                    || finally_block.as_ref().is_some_and(|fb| in_stmts(fb))
            }
        })
    }
    fn member_spawns(m: &ClassMember) -> bool {
        match m {
            ClassMember::Field { init, .. } => init.as_ref().is_some_and(in_expr),
            ClassMember::Constructor { body, .. } => in_stmts(body),
            ClassMember::Method(f) => in_stmts(&f.body),
            ClassMember::Hook { get, set, .. } => {
                get.as_ref().is_some_and(in_expr)
                    || set.as_ref().is_some_and(|(_, stmts)| in_stmts(stmts))
            }
        }
    }
    program.items.iter().any(|item| match item {
        Item::Function(f) => in_stmts(&f.body),
        Item::Class(c) => c.members.iter().any(member_spawns),
        Item::Trait(t) => t.members.iter().any(member_spawns),
        Item::Test { body, .. } => in_stmts(body),
        // Interface methods are bodyless; enums/aliases/imports carry no executable body.
        Item::Interface(_) | Item::Enum(_) | Item::TypeAlias { .. } | Item::Import { .. } => false,
    })
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

#[cfg(test)]
mod uses_concurrency_tests {
    use super::uses_concurrency;

    fn parse(src: &str) -> crate::ast::Program {
        crate::loader::load_loose_src(src).expect("parse").program
    }

    #[test]
    fn no_spawn_is_false() {
        assert!(!uses_concurrency(&parse(
            "package Main;\nimport Core.Console;\n\
             function main() -> void { Console.println(\"hi\"); }\n"
        )));
    }

    #[test]
    fn spawn_in_main_is_true() {
        assert!(uses_concurrency(&parse(
            "package Main;\n\
             function sq(int n) -> int { return n * n; }\n\
             function main() -> void { var t = spawn sq(3); }\n"
        )));
    }

    #[test]
    fn spawn_nested_in_a_helper_body_is_true() {
        // spawn buried in a non-main free function, inside an `if` inside a `for` — exercises the
        // statement recursion (a false negative here would silently route to the eager path).
        assert!(uses_concurrency(&parse(
            "package Main;\n\
             function sq(int n) -> int { return n * n; }\n\
             function work() -> void {\n\
                 for (int i in 0..3) { if (i > 0) { var t = spawn sq(i); } }\n\
             }\n\
             function main() -> void { work(); }\n"
        )));
    }

    #[test]
    fn spawn_in_a_method_body_is_true() {
        assert!(uses_concurrency(&parse(
            "package Main;\n\
             function sq(int n) -> int { return n * n; }\n\
             class Runner { function go() -> void { var t = spawn sq(2); } }\n\
             function main() -> void { new Runner().go(); }\n"
        )));
    }
}
