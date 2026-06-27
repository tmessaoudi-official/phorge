//! Checker tests — totality (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn never_resolves_as_a_return_type() {
    // A `-> never` function that diverges (infinite loop) type-checks clean.
    let src = "function spin() -> never { while (true) {} } function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn never_is_a_reserved_builtin_type_name() {
    // Aliasing `never` is rejected exactly like aliasing `int`.
    let bad = errors_of("type never = int; function main() -> void {}");
    assert!(
        bad.iter()
            .any(|e| e.message.contains("built-in type `never`")),
        "{bad:?}"
    );
}

#[test]
fn typed_fn_falling_off_the_end_is_error() {
    let bad = errors_of("function f() -> int { } function main() -> void {}");
    assert!(
        bad.iter().any(|e| e.code == Some("E-MISSING-RETURN")),
        "{bad:?}"
    );
}

#[test]
fn if_both_branches_return_is_total() {
    let src = "function f(int x) -> int { if (x > 0) { return 1; } else { return 2; } } \
                   function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn if_without_else_falls_through() {
    let bad = errors_of(
        "function f(int x) -> int { if (x > 0) { return 1; } } function main() -> void {}",
    );
    assert!(
        bad.iter().any(|e| e.code == Some("E-MISSING-RETURN")),
        "{bad:?}"
    );
}

#[test]
fn if_no_else_then_trailing_return_is_total() {
    let src = "function f(int x) -> int { if (x > 0) { return 1; } return 2; } \
                   function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn infinite_loop_tail_is_total() {
    // No explicit return, but `while (true) {}` with no break never falls through.
    let src = "function f() -> int { while (true) {} } function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn while_true_with_break_still_needs_return() {
    let bad =
        errors_of("function f() -> int { while (true) { break; } } function main() -> void {}");
    assert!(
        bad.iter().any(|e| e.code == Some("E-MISSING-RETURN")),
        "{bad:?}"
    );
}

#[test]
fn never_fn_that_can_return_is_error() {
    // A `-> never` body that falls through (could return normally) is rejected.
    let bad = errors_of("function f() -> never { } function main() -> void {}");
    assert!(
        bad.iter().any(|e| e.code == Some("E-NEVER-RETURN")),
        "{bad:?}"
    );
}

#[test]
fn calling_a_never_fn_diverges() {
    // An expression statement calling a `-> never` function terminates the block, so the
    // enclosing `-> int` function needs no further return.
    let src = "function spin() -> never { while (true) {} } \
                   function f() -> int { spin(); } function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn unit_fn_needs_no_return() {
    let src = "import Core.Console; function f() -> void { Console.println(\"hi\"); } function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn return_match_is_total() {
    let src = "enum E { A(), B() } \
                   function f(E e) -> int { return match e { A() => 1, B() => 2 }; } \
                   function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn code_after_return_warns_unreachable_once() {
    let src = "import Core.Console; \
                   function f() -> int { return 1; Console.println(\"x\"); Console.println(\"y\"); } \
                   function main() -> void {}";
    let warns = warnings_of(src);
    let n = warns
        .iter()
        .filter(|w| w.code == Some("W-UNREACHABLE"))
        .count();
    assert_eq!(n, 1, "exactly one dead-region warning: {warns:?}");
}

#[test]
fn clean_function_has_no_unreachable_warning() {
    let src = "function f() -> int { return 1; } function main() -> void {}";
    assert!(
        warnings_of(src)
            .iter()
            .all(|w| w.code != Some("W-UNREACHABLE")),
        "{:?}",
        warnings_of(src)
    );
}

#[test]
fn match_arm_after_catch_all_warns() {
    let src = "function f(int x) -> int { return match x { _ => 0, 1 => 9 }; } \
                   function main() -> void {}";
    assert!(
        warnings_of(src)
            .iter()
            .any(|w| w.code == Some("W-MATCH-UNREACHABLE")),
        "{:?}",
        warnings_of(src)
    );
}

#[test]
fn duplicate_match_literal_arm_warns() {
    let src = "function f(int x) -> int { return match x { 1 => 1, 1 => 2, _ => 0 }; } \
                   function main() -> void {}";
    assert!(
        warnings_of(src)
            .iter()
            .any(|w| w.code == Some("W-MATCH-UNREACHABLE")),
        "{:?}",
        warnings_of(src)
    );
}

#[test]
fn exhaustive_distinct_match_has_no_unreachable_warning() {
    let src = "function f(int x) -> int { return match x { 1 => 1, 2 => 2, _ => 0 }; } \
                   function main() -> void {}";
    assert!(
        warnings_of(src)
            .iter()
            .all(|w| w.code != Some("W-MATCH-UNREACHABLE")),
        "{:?}",
        warnings_of(src)
    );
}

// ── lambda return totality (Soundness Batch F, finding #6) ───────────────────────────────────────

#[test]
fn statement_lambda_falling_off_end_is_missing_return() {
    // A `-> int` statement-body lambda that can fall off the end binds `unit` into an `int` slot —
    // the same leak the totality cluster closed for free fns/methods; now enforced for lambdas too.
    let src = "function main() -> void { var f = fn(int n) -> int { if (n > 0) { return n; } }; }";
    assert!(
        errors_of(src)
            .iter()
            .any(|d| d.code == Some("E-MISSING-RETURN")),
        "{:?}",
        errors_of(src)
    );
}

#[test]
fn statement_lambda_returning_on_all_paths_is_ok() {
    let src = "function main() -> void { var f = fn(int n) -> int { if (n > 0) { return n; } return 0; }; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn void_statement_lambda_may_fall_off_end() {
    // A `-> void` lambda is value-less — falling off the end is fine (regression guard).
    let src = "import Core.Console; function main() -> void { var f = fn(int n) -> void { Console.println(\"{n}\"); }; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}
