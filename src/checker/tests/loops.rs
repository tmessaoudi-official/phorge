//! Checker tests — loops (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn while_loop_is_ok() {
    assert!(
        errors_of("function main() -> void { mutable int i = 0; while (i < 3) { i += 1; } }")
            .is_empty()
    );
}

#[test]
fn while_condition_must_be_bool() {
    let bad = errors_of("function main() -> void { while (1) { } }");
    assert!(!bad.is_empty(), "expected a non-bool-condition error");
}

#[test]
fn c_for_is_ok() {
    assert!(errors_of(
            "import Core.Console; function main() -> void { for (mutable int i = 0; i < 3; i++) { Console.println(\"{i}\"); } }"
        )
        .is_empty());
}

#[test]
fn c_for_immutable_counter_step_is_error() {
    // The counter is reassigned by the step, so it must be `mutable` (immutable-by-default).
    let bad = errors_of("function main() -> void { for (int i = 0; i < 3; i++) { } }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn break_outside_loop_is_error() {
    let bad = errors_of("function main() -> void { break; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-BREAK-OUTSIDE-LOOP")),
        "{bad:?}"
    );
}

#[test]
fn continue_outside_loop_is_error() {
    let bad = errors_of("function main() -> void { continue; }");
    assert!(
        bad.iter()
            .any(|e| e.code == Some("E-CONTINUE-OUTSIDE-LOOP")),
        "{bad:?}"
    );
}

#[test]
fn break_inside_loop_is_ok() {
    assert!(errors_of(
        "function main() -> void { mutable int i = 0; while (i < 9) { i += 1; if (i == 3) { break; } } }"
    )
    .is_empty());
}
