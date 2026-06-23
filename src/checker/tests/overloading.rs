//! Checker tests — overloading (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn overloaded_functions_by_arity_are_legal() {
    // M-RT overloading: same name, distinct parameter signatures, same return type — a valid
    // overload set (was rejected pre-overloading).
    let errs = errors_of("function f() {} function f(int n) {}");
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn overloaded_functions_by_type_are_legal() {
    let errs = errors_of(
        "function show(int x) -> int { return x; } \
             function show(string s) -> int { return 1; }",
    );
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn overload_set_must_share_return_type() {
    let errs = errors_of(
        "function f(int x) -> int { return x; } \
             function f(string s) -> string { return s; }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERLOAD-RETURN")),
        "{errs:?}"
    );
}

#[test]
fn overload_set_rejects_identical_signatures() {
    let errs = errors_of("function f(int x) {} function f(int y) {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERLOAD-DUPLICATE")),
        "{errs:?}"
    );
}

#[test]
fn overloaded_call_with_no_matching_argument_type_errors() {
    let errs = errors_of(
        "function show(int x) -> int { return x; } \
             function show(string s) -> int { return 1; } \
             function main() { var r = show(true); }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERLOAD-NO-MATCH")),
        "{errs:?}"
    );
}
