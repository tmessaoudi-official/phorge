//! Checker tests — declaration uniqueness (Soundness Batch G, finding #7).
//!
//! Duplicate parameter names (free fn / method / constructor) and duplicate field names (explicit,
//! promoted, or a collision between the two) were silently accepted (last declaration won). They are
//! now rejected: `E-DUP-PARAM` / `E-DUP-FIELD`.

use super::support::*;

fn has(src: &str, code: &str) -> bool {
    errors_of(src).iter().any(|e| e.code == Some(code))
}

#[test]
fn duplicate_free_fn_params_is_error() {
    let src = "function add(int a, int a) -> int { return a; } function main() -> void { }";
    assert!(has(src, "E-DUP-PARAM"), "{:?}", errors_of(src));
}

#[test]
fn duplicate_free_fn_params_different_type_is_error() {
    let src = "function f(int a, string a) -> int { return 0; } function main() -> void { }";
    assert!(has(src, "E-DUP-PARAM"), "{:?}", errors_of(src));
}

#[test]
fn duplicate_method_params_is_error() {
    let src =
        "class C { function m(int a, int a) -> int { return a; } } function main() -> void { }";
    assert!(has(src, "E-DUP-PARAM"), "{:?}", errors_of(src));
}

#[test]
fn duplicate_ctor_params_is_error() {
    let src = "class C { constructor(int a, int a) {} } function main() -> void { }";
    assert!(has(src, "E-DUP-PARAM"), "{:?}", errors_of(src));
}

#[test]
fn duplicate_promoted_fields_is_error() {
    let src = "class C { constructor(public int x, public int x) {} } function main() -> void { }";
    // Same name twice as ctor params → E-DUP-PARAM (the promotion collision is implied).
    assert!(has(src, "E-DUP-PARAM"), "{:?}", errors_of(src));
}

#[test]
fn duplicate_explicit_fields_is_error() {
    let src = "class C { int x = 0; int x = 1; constructor() {} } function main() -> void { }";
    assert!(has(src, "E-DUP-FIELD"), "{:?}", errors_of(src));
}

#[test]
fn explicit_field_matching_a_promoted_param_is_ok() {
    // Intentional: an explicit field declaration is authoritative; a promoted ctor param of the same
    // (matching) name/type is allowed (the explicit decl wins). Not a duplicate.
    let src = "class C { private int total; constructor(private int total) {} \
                         function add(int n) -> int { return this.total + n; } } \
               function main() -> void { }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn distinct_params_and_fields_are_ok() {
    let src = "class C { int a = 0; constructor(public int b, int c) { } \
                         function m(int x, int y) -> int { return x + y; } } \
               function main() -> void { }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

// ── M-DX S1: duplicate enum variant / static field (silent-overwrite soundness holes C/D) ──

#[test]
fn duplicate_enum_variant_is_error() {
    // Was silently accepted (a HashMap insert let the second `A` overwrite the first).
    let src = "enum E { A, A } function main() -> void { }";
    assert!(has(src, "E-DUP-VARIANT"), "{:?}", errors_of(src));
}

#[test]
fn distinct_enum_variants_are_ok() {
    let src = "enum E { A, B, C } function main() -> void { }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn duplicate_static_field_is_error() {
    // Was silently accepted (statics were `continue`d past the E-DUP-FIELD loop, then a HashMap
    // insert overwrote the first).
    let src = "class C { static int x = 1; static int x = 2; } function main() -> void { }";
    assert!(has(src, "E-DUP-STATIC"), "{:?}", errors_of(src));
}

#[test]
fn distinct_static_fields_are_ok() {
    let src = "class C { static int x = 1; static int y = 2; } function main() -> void { }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn duplicate_const_is_error() {
    let src = "class C { const int X = 1; const int X = 2; } function main() -> void { }";
    assert!(has(src, "E-DUP-CONST"), "{:?}", errors_of(src));
}
