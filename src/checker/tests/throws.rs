//! Checker tests — throws (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn propagate_in_result_fn_is_clean() {
    // `?` in a let-initializer inside a `Result`-returning fn unwraps the `Ok` payload (an `int`).
    let ok = errors_of(&format!(
        "{RESULT_DEF} \
             function f() -> Result<int, string> {{ return new Ok(1); }} \
             function g() -> Result<int, string> {{ int x = f()?; return new Ok(x + 1); }} \
             function main() -> void {{}}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn propagate_outside_let_initializer_is_position_error() {
    // `?` nested in a larger expression is `E-PROPAGATE-POSITION` (not a whole let-initializer).
    let bad = errors_of(&format!(
        "{RESULT_DEF} \
             function f() -> Result<int, string> {{ return new Ok(1); }} \
             function g() -> Result<int, string> {{ int x = f()? + 1; return new Ok(x); }} \
             function main() -> void {{}}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-PROPAGATE-POSITION")),
        "expected E-PROPAGATE-POSITION, got {bad:?}"
    );
}

#[test]
fn intrinsic_panic_requires_string_literal() {
    // A non-literal panic message (interpolation) is `E-INTRINSIC-LITERAL`.
    let bad = errors_of(r#"function main() -> void { var n = 1; panic("bad {n}"); }"#);
    assert!(
        bad.iter().any(|d| d.code == Some("E-INTRINSIC-LITERAL")),
        "expected E-INTRINSIC-LITERAL, got {bad:?}"
    );
}

#[test]
fn intrinsic_assert_condition_must_be_bool() {
    let bad = errors_of(r#"function main() -> void { assert(1, "x"); }"#);
    assert!(
        !bad.is_empty(),
        "expected a type error for a non-bool assert condition"
    );
}

#[test]
fn intrinsic_name_is_reserved() {
    let bad = errors_of("function unreachable() -> void { return; } function main() -> void {}");
    assert!(
        bad.iter().any(|d| d.code == Some("E-RESERVED-INTRINSIC")),
        "expected E-RESERVED-INTRINSIC, got {bad:?}"
    );
}

#[test]
fn panic_tail_satisfies_return_totality() {
    // `panic` is `never`-typed, so a value-returning fn ending in it needs no further `return`.
    let ok = errors_of(r#"function f() -> int { panic("x"); } function main() -> void {}"#);
    assert!(
        ok.is_empty(),
        "expected clean (never satisfies totality), got {ok:?}"
    );
}

#[test]
fn propagate_in_non_result_fn_is_context_error() {
    // `?` requires the enclosing fn to return the same `Result` — otherwise `E-PROPAGATE-CONTEXT`.
    let bad = errors_of(&format!(
        "{RESULT_DEF} \
             function f() -> Result<int, string> {{ return new Ok(1); }} \
             function g() -> int {{ int x = f()?; return x; }} \
             function main() -> void {{}}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-PROPAGATE-CONTEXT")),
        "expected E-PROPAGATE-CONTEXT, got {bad:?}"
    );
}

#[test]
fn throw_undeclared_and_uncaught_is_error() {
    // A helper that throws but neither declares `throws` nor wraps it in a `try`.
    let bad = errors_of(&format!(
        "{ERRDEF} function f() -> void {{ throw new BadInput(\"x\"); }} function main() -> void {{}}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-THROW-UNDECLARED")),
        "expected E-THROW-UNDECLARED, got {bad:?}"
    );
}

#[test]
fn throw_declared_then_caught_at_call_is_clean() {
    // `f` declares `throws BadInput` (discharges its own throw); `main` calls it inside a `try`
    // catching `BadInput` (discharges the call). Both sides handled — clean.
    let ok = errors_of(&format!(
        "{ERRDEF} function f() -> void throws BadInput {{ throw new BadInput(\"x\"); }} \
             function main() -> void {{ try {{ f(); }} catch (BadInput e) {{}} }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn throw_in_main_is_uncaught() {
    let bad = errors_of(&format!(
        "{ERRDEF} function main() -> void {{ throw new BadInput(\"x\"); }}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-UNCAUGHT-THROW")),
        "expected E-UNCAUGHT-THROW, got {bad:?}"
    );
}

#[test]
fn main_may_not_declare_throws() {
    let bad = errors_of(&format!(
        "{ERRDEF} function main() -> void throws BadInput {{ throw new BadInput(\"x\"); }}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-UNCAUGHT-THROW")),
        "expected E-UNCAUGHT-THROW, got {bad:?}"
    );
}

#[test]
fn throws_error_root_is_too_broad() {
    let bad = errors_of(&format!(
        "{ERRDEF} function f() -> void throws Error {{ throw new BadInput(\"x\"); }} \
             function main() -> void {{ try {{ f(); }} catch (Error e) {{}} }}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-THROWS-TOO-BROAD")),
        "expected E-THROWS-TOO-BROAD, got {bad:?}"
    );
}

#[test]
fn throw_non_error_value_is_type_error() {
    let bad = errors_of("function main() -> void { throw 42; }");
    assert!(
        bad.iter().any(|d| d.code == Some("E-THROW-TYPE")),
        "expected E-THROW-TYPE, got {bad:?}"
    );
}

#[test]
fn bare_call_to_throwing_fn_is_unhandled() {
    let bad = errors_of(&format!(
        "{ERRDEF} function f() -> void throws BadInput {{ throw new BadInput(\"x\"); }} \
             function main() -> void {{ f(); }}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-CALL-UNHANDLED")),
        "expected E-CALL-UNHANDLED, got {bad:?}"
    );
}

#[test]
fn propagate_throws_to_declared_is_clean() {
    // `g` propagates `f`'s `BadInput` with `?` and declares it — clean; `main` catches the call.
    let ok = errors_of(&format!(
        "{ERRDEF} function f() -> void throws BadInput {{ throw new BadInput(\"x\"); }} \
             function g() -> void throws BadInput {{ f()?; }} \
             function main() -> void {{ try {{ g(); }} catch (BadInput e) {{}} }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn propagate_throws_without_declaration_is_unhandled() {
    // `g` uses `?` but does not declare `throws BadInput` — the propagation is unhandled.
    let bad = errors_of(&format!(
        "{ERRDEF} function f() -> void throws BadInput {{ throw new BadInput(\"x\"); }} \
             function g() -> void {{ f()?; }} function main() -> void {{}}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-CALL-UNHANDLED")),
        "expected E-CALL-UNHANDLED, got {bad:?}"
    );
}

#[test]
fn catch_non_error_type_is_error() {
    let bad = errors_of(&format!(
        "{ERRDEF} function main() -> void {{ try {{}} catch (int e) {{}} }}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-CATCH-TYPE")),
        "expected E-CATCH-TYPE, got {bad:?}"
    );
}

#[test]
fn shadowed_catch_clause_warns() {
    // A second `catch (BadInput …)` after the first can never run — a non-fatal lint.
    let warns = warnings_of(&format!(
        "{ERRDEF} function f() -> void throws BadInput {{ throw new BadInput(\"x\"); }} \
             function main() -> void {{ try {{ f(); }} catch (BadInput e) {{}} catch (BadInput e2) {{}} }}"
    ));
    assert!(
        warns.iter().any(|d| d.code == Some("W-CATCH-UNREACHABLE")),
        "expected W-CATCH-UNREACHABLE, got {warns:?}"
    );
}

#[test]
fn try_with_returning_arms_satisfies_totality() {
    // A `-> int` fn whose `try` body and `catch` both return diverges on every path — total.
    let ok = errors_of(&format!(
            "{ERRDEF} function g() -> int {{ try {{ return 1; }} catch (BadInput e) {{ return 0; }} }} \
             function main() -> void {{}}"
        ));
    assert!(ok.is_empty(), "expected clean (try totality), got {ok:?}");
}

#[test]
fn try_falling_through_misses_return() {
    // Both arms fall through, so the `-> int` fn does not return on all paths.
    let bad = errors_of(&format!(
        "{ERRDEF} function g() -> int {{ try {{}} catch (BadInput e) {{}} }} function main() -> void {{}}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-MISSING-RETURN")),
        "expected E-MISSING-RETURN, got {bad:?}"
    );
}

#[test]
fn throw_tail_satisfies_totality() {
    // A `throw` diverges, so a `-> int` fn whose only statement is a `throw` is total.
    let ok = errors_of(&format!(
        "{ERRDEF} function g() -> int throws BadInput {{ throw new BadInput(\"x\"); }} \
             function main() -> void {{ try {{ var n = g(); }} catch (BadInput e) {{}} }}"
    ));
    assert!(ok.is_empty(), "expected clean (throw diverges), got {ok:?}");
}

#[test]
fn throws_mode_propagate_is_recorded_for_erasure() {
    // A throws-mode `?` is a checker-only marker: it must be recorded (mapped to the bare call)
    // so `resolve_html` erases the `Propagate` node before any backend sees it.
    let p = prog(&format!(
        "{ERRDEF} function f() -> void throws BadInput {{ throw new BadInput(\"x\"); }} \
             function g() -> void throws BadInput {{ f()?; }} function main() -> void {{}}"
    ));
    let (_warns, subst, _ufcs) = check_resolutions(&p).expect("checks clean");
    assert_eq!(
        subst.len(),
        1,
        "exactly one throws-? recorded, got {subst:?}"
    );
    assert!(
        matches!(subst.values().next(), Some(crate::ast::Expr::Call { .. })),
        "the erased `?` must map to the bare call, got {subst:?}"
    );
    // And the substitution actually removes the `Propagate` node.
    let expanded = resolve_html(p, &subst);
    assert!(
        !program_has_propagate(&expanded),
        "throws-mode `?` Propagate node was not erased"
    );
}
