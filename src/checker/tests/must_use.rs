//! Must-use returns (M-must-use Slice A): a non-`void`/`Empty` result used as a bare statement is
//! `E-UNUSED-VALUE`; `discard <expr>;` is the escape hatch; `void`/`Empty` results may be dropped.
use super::support::*;

#[test]
fn unused_non_void_call_is_error() {
    let errs = errors_of("function f(): int { return 1; } function main(): void { f(); }");
    assert!(
        errs.iter().any(|e| e.code == Some("E-UNUSED-VALUE")),
        "got {errs:?}"
    );
}

#[test]
fn discard_exempts_a_non_void_call() {
    let errs = errors_of("function f(): int { return 1; } function main(): void { discard f(); }");
    assert!(errs.is_empty(), "got {errs:?}");
}

#[test]
fn void_call_needs_no_discard() {
    let errs = errors_of("function noop(): void {} function main(): void { noop(); }");
    assert!(errs.is_empty(), "got {errs:?}");
}

#[test]
fn used_result_is_not_flagged() {
    let errs = errors_of(
        "function f(): int { return 1; } function main(): void { int x = f(); discard f(); }",
    );
    assert!(
        !errs.iter().any(|e| e.code == Some("E-UNUSED-VALUE")),
        "got {errs:?}"
    );
}

#[test]
fn bare_arithmetic_statement_is_unused() {
    let errs = errors_of("function main(): void { 1 + 1; }");
    assert!(
        errs.iter().any(|e| e.code == Some("E-UNUSED-VALUE")),
        "got {errs:?}"
    );
}

#[test]
fn discard_contextual_keyword_not_reserved() {
    // `discard` is contextual: a value-use (here a call to a fn literally named `discard`) is not the
    // discard statement. The gate only treats statement-leading `discard <Ident|new>` as the keyword.
    let errs = errors_of("function discard(): void {} function main(): void { discard(); }");
    assert!(errs.is_empty(), "got {errs:?}");
}
