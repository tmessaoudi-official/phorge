//! Checker tests — basics (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn unknown_identifier_suggests_the_nearest_in_scope_name() {
    // `cont` is one edit from the in-scope `count` → the diagnostic carries a code + hint.
    let errs = errors_of(
        "import Core.Console; function main() { int count = 0; Console.println(\"{cont}\"); }",
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
    let errs = errors_of("function main() { float x = 1 + 2.0; }");
    assert!(!errs.is_empty(), "mixing int and float must error");
}

#[test]
fn if_condition_must_be_bool() {
    let errs = errors_of("function main() { if (1) { } }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("condition must be `bool`")),
        "{errs:?}"
    );
}

#[test]
fn equality_requires_same_type() {
    let errs = errors_of("function main() { bool b = 1 == true; }");
    assert!(
        errs.iter().any(|e| e.message.contains("cross-type")),
        "{errs:?}"
    );
}

#[test]
fn unknown_identifier_errors() {
    let errs = errors_of("function main() { int n = missing; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("unknown identifier")),
        "{errs:?}"
    );
}

#[test]
fn block_scoping_pops_bindings() {
    let errs = errors_of("function main() { { int x = 1; } int y = x; }");
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
    assert!(
        errors_of("function main() { var x = if (1 < 2) { 10 } else { 20 }; int y = x; }")
            .is_empty()
    );
}

#[test]
fn expression_if_branch_type_mismatch_errors() {
    let errs = errors_of("function main() { var x = if (true) { 1 } else { false }; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("branches must share one type")),
        "{errs:?}"
    );
}

#[test]
fn expression_if_condition_must_be_bool() {
    let errs = errors_of("function main() { var x = if (3) { 1 } else { 2 }; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("condition must be `bool`")),
        "{errs:?}"
    );
}
