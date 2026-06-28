//! Checker tests — overloading (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn overloaded_functions_by_arity_are_legal() {
    // M-RT overloading: same name, distinct parameter signatures, same return type — a valid
    // overload set (was rejected pre-overloading).
    let errs = errors_of("function f() -> void {} function f(int n) -> void {}");
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
    let errs = errors_of("function f(int x) -> void {} function f(int y) -> void {}");
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
             function main() -> void { var r = show(true); }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERLOAD-NO-MATCH")),
        "{errs:?}"
    );
}

#[test]
fn overload_set_rejects_php_erasure_collisions() {
    // `string`/`bytes` both erase to PHP `string` → indistinguishable in transpiled PHP.
    let e1 = errors_of(
        "function f(string s) -> int { return 1; } function f(bytes b) -> int { return 2; }",
    );
    assert!(
        e1.iter().any(|e| e.code == Some("E-OVERLOAD-ERASE")),
        "string vs bytes must be E-OVERLOAD-ERASE: {e1:?}"
    );
    // `List`/`Set` both erase to PHP `array`.
    let e2 = errors_of("function g(List<int> xs) -> int { return 1; } function g(Set<int> ys) -> int { return 2; }");
    assert!(
        e2.iter().any(|e| e.code == Some("E-OVERLOAD-ERASE")),
        "List vs Set must be E-OVERLOAD-ERASE: {e2:?}"
    );
    // `int` vs `string` ARE distinguishable in PHP (is_int / is_string) — no error.
    let e3 = errors_of(
        "function h(int x) -> int { return 1; } function h(string s) -> int { return 2; }",
    );
    assert!(
        !e3.iter().any(|e| e.code == Some("E-OVERLOAD-ERASE")),
        "int vs string must NOT collide: {e3:?}"
    );
}

// ── Return-type overloading (M-RT Slice C1) ──

#[test]
fn return_overload_set_is_legal_and_resolves_via_selector() {
    // Identical params, differing returns — a return-overload set — resolved by `<Type>` selectors.
    let errs = errors_of(
        "function read(string s) -> int { return 1; } \
         function read(string s) -> bool { return true; } \
         function main() -> void { int a = <int>read(\"x\"); bool b = <bool>read(\"y\"); \
             discard a; discard b; }",
    );
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn bare_return_overloaded_call_without_context_errors() {
    let errs = errors_of(
        "function read(string s) -> int { return 1; } \
         function read(string s) -> bool { return true; } \
         function main() -> void { discard read(\"x\"); }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERLOAD-NO-CONTEXT")),
        "{errs:?}"
    );
}

#[test]
fn selector_naming_no_overloads_return_type_errors() {
    let errs = errors_of(
        "function read(string s) -> int { return 1; } \
         function read(string s) -> bool { return true; } \
         function main() -> void { discard <string>read(\"x\"); }",
    );
    assert!(
        errs.iter()
            .any(|e| e.code == Some("E-OVERLOAD-SELECT-UNKNOWN")),
        "{errs:?}"
    );
}

#[test]
fn selector_matching_two_subtypes_is_ambiguous() {
    // Both overloads return a subtype of `Animal`, so `<Animal>` is ambiguous (no exact match, two
    // assignable) — the fix is an exact selector (`<Dog>`/`<Cat>`).
    let errs = errors_of(
        "interface Animal {} \
         class Dog implements Animal { constructor() {} } \
         class Cat implements Animal { constructor() {} } \
         function make(int n) -> Dog { return new Dog(); } \
         function make(int n) -> Cat { return new Cat(); } \
         function main() -> void { discard <Animal>make(1); }",
    );
    assert!(
        errs.iter()
            .any(|e| e.code == Some("E-OVERLOAD-AMBIGUOUS-RETURN")),
        "{errs:?}"
    );
}

#[test]
fn mixing_parameter_and_return_overloading_errors() {
    // `f(string)` has two return overloads AND `f(int)` is a parameter overload — mixed, rejected.
    let errs = errors_of(
        "function f(string s) -> int { return 1; } \
         function f(string s) -> bool { return true; } \
         function f(int n) -> int { return n; } \
         function main() -> void {}",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERLOAD-RETURN")),
        "{errs:?}"
    );
}

#[test]
fn selector_on_non_return_overloaded_function_errors() {
    let errs = errors_of(
        "function g(int n) -> int { return n; } \
         function main() -> void { discard <int>g(1); }",
    );
    assert!(
        errs.iter()
            .any(|e| e.code == Some("E-OVERLOAD-SELECT-UNKNOWN")),
        "{errs:?}"
    );
}

#[test]
fn identical_param_and_return_is_still_a_duplicate() {
    let errs = errors_of(
        "function f(int x) -> int { return x; } \
         function f(int y) -> int { return y; }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERLOAD-DUPLICATE")),
        "{errs:?}"
    );
}
