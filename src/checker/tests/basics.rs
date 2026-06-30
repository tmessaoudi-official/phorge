//! Checker tests — basics (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn unknown_identifier_suggests_the_nearest_in_scope_name() {
    // `cont` is one edit from the in-scope `count` → the diagnostic carries a code + hint.
    let errs = errors_of(
        "import Core.Output; function main() -> void { int count = 0; Output.printLine(\"{cont}\"); }",
    );
    let d = errs
        .iter()
        .find(|e| e.message.contains("unknown identifier"))
        .expect("an unknown-identifier error");
    assert_eq!(d.code, Some("E-UNKNOWN-IDENT"));
    assert!(
        d.hint.as_deref().unwrap_or("").contains("count"),
        "hint: {:?}",
        d.hint
    );
}

#[test]
fn arithmetic_mixing_int_float_errors() {
    let errs = errors_of("function main() -> void { float x = 1 + 2.0; }");
    assert!(!errs.is_empty(), "mixing int and float must error");
}

#[test]
fn power_operator_is_type_directed() {
    // `int ** int` → int, `float ** float` → float (both accepted).
    assert!(errors_of("function main() -> void { int x = 2 ** 10; }").is_empty());
    assert!(errors_of("function main() -> void { float x = 2.0 ** 3.0; }").is_empty());
    // Mixed / non-numeric operands are rejected — `**` never coerces or concatenates.
    let errs = errors_of("function main() -> void { var x = 2 ** 3.0; }");
    assert!(
        errs.iter().any(|e| e
            .message
            .contains("`**` requires matching int or float operands")),
        "{errs:?}"
    );
    assert!(!errors_of("function main() -> void { var x = \"a\" ** \"b\"; }").is_empty());
}

#[test]
fn bitwise_requires_int_operands() {
    // int & int → int (accepted, used as an int).
    assert!(
        errors_of("function main() -> void { int x = 0xFF & 0x0F; int y = x << 2; }").is_empty()
    );
    // unary `~` on an int is fine.
    assert!(errors_of("function main() -> void { int x = ~5; }").is_empty());
    // bitwise on a non-int operand is rejected.
    let errs = errors_of("function main() -> void { int x = 3 & 2.0; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("bitwise operators require `int`")),
        "{errs:?}"
    );
    // unary `~` on a non-int is rejected.
    let e2 = errors_of("function main() -> void { var x = ~true; }");
    assert!(
        e2.iter()
            .any(|e| e.message.contains("unary `~` requires `int`")),
        "{e2:?}"
    );
}

#[test]
fn if_condition_must_be_bool() {
    let errs = errors_of("function main() -> void { if (1) { } }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("condition must be `bool`")),
        "{errs:?}"
    );
}

#[test]
fn equality_requires_same_type() {
    let errs = errors_of("function main() -> void { bool b = 1 == true; }");
    assert!(
        errs.iter().any(|e| e.message.contains("cross-type")),
        "{errs:?}"
    );
}

#[test]
fn unknown_identifier_errors() {
    let errs = errors_of("function main() -> void { int n = missing; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("unknown identifier")),
        "{errs:?}"
    );
}

#[test]
fn block_scoping_pops_bindings() {
    let errs = errors_of("function main() -> void { { int x = 1; } int y = x; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("unknown identifier")),
        "{errs:?}"
    );
}

#[test]
fn return_type_checked_against_signature() {
    let errs = errors_of("function f() -> int { return true; }");
    assert!(
        errs.iter().any(|e| e.message.contains("expected `int`")),
        "{errs:?}"
    );
}

#[test]
fn expression_if_unifies_branch_types() {
    assert!(errors_of(
        "function main() -> void { var x = if (1 < 2) { 10 } else { 20 }; int y = x; }"
    )
    .is_empty());
}

#[test]
fn expression_if_branch_type_mismatch_errors() {
    let errs = errors_of("function main() -> void { var x = if (true) { 1 } else { false }; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("branches must share one type")),
        "{errs:?}"
    );
}

#[test]
fn expression_if_condition_must_be_bool() {
    let errs = errors_of("function main() -> void { var x = if (3) { 1 } else { 2 }; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("condition must be `bool`")),
        "{errs:?}"
    );
}

// --- Phase 1 string slice: `+` concatenation ---

#[test]
fn string_plus_string_is_string() {
    assert!(errors_of(r#"function main() -> void { string s = "a" + "b"; }"#).is_empty());
}

#[test]
fn string_plus_int_is_a_type_error_no_coercion() {
    let errs = errors_of(r#"function main() -> void { string s = "a" + 1; }"#);
    assert!(
        errs.iter().any(|e| e.message.contains("no coercion")),
        "{errs:?}"
    );
}

#[test]
fn string_minus_string_is_still_rejected() {
    // Only `+` concatenates; other arithmetic ops stay numeric-only.
    let errs = errors_of(r#"function main() -> void { string s = "a" - "b"; }"#);
    assert!(!errs.is_empty(), "{errs:?}");
}
