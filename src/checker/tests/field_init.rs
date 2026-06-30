//! Checker tests — expression field initializers (Feature B).

use super::support::*;

fn has(src: &str, code: &str) -> bool {
    errors_of(src).iter().any(|e| e.code == Some(code))
}

#[test]
fn computed_default_with_call_is_ok() {
    let src = "function f() -> int { return 9; } \
               class C { int x = f() + 1; constructor() {} } \
               function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn reads_this_and_earlier_sibling_is_ok() {
    let src = "class C { int a = 10; int b = this.a * 2; constructor() {} } \
               function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn no_constructor_class_with_field_init_is_ok() {
    let src = "class C { int x = 5; } function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn reads_later_sibling_is_forward_ref() {
    let src = "class C { int a = this.b; int b = 1; } function main() -> void {}";
    assert!(has(src, "E-FIELD-INIT-FORWARD-REF"));
}

#[test]
fn self_reference_is_forward_ref() {
    let src = "class C { int a = this.a + 1; } function main() -> void {}";
    assert!(has(src, "E-FIELD-INIT-FORWARD-REF"));
}

#[test]
fn reads_promoted_param_is_ok() {
    // A promoted ctor param is set before field inits, so reading it is allowed.
    let src = "class C { int doubled = this.n * 2; constructor(public int n) {} } \
               function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn init_type_mismatch_is_error() {
    let src = "class C { int x = \"nope\"; constructor() {} } function main() -> void {}";
    assert!(has(src, "E-FIELD-INIT-TYPE"));
}

#[test]
fn closure_default_capturing_this_is_lambda_this_error() {
    // A field-default closure that touches `this` is rejected by the existing E-LAMBDA-THIS guard
    // (this-capture defers to the closures slice).
    let src = "class C { int n = 1; (int) -> int f = function(int x) => this.n + x; } \
               function main() -> void {}";
    assert!(has(src, "E-LAMBDA-THIS"));
}

#[test]
fn non_capturing_closure_default_is_ok() {
    let src = "class C { (int) -> int dbl = function(int x) => x * 2; constructor() {} } \
               function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

// --- Feature B-static: runtime static-field initializers ---

#[test]
fn static_computed_initializer_is_ok() {
    let src = "function seed() -> int { return 42; } \
               class C { static int answer = seed(); } \
               function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn static_reading_earlier_static_is_ok() {
    let src = "class C { static int a = 10; static int b = C.a + 1; } \
               function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn static_without_initializer_is_error() {
    assert!(errors_of("class C { static int x; }")
        .iter()
        .any(|e| e.code == Some("E-STATIC-NO-INIT")));
}

#[test]
fn static_initializer_type_mismatch_is_error() {
    let src = "function seed() -> string { return \"x\"; } \
               class C { static int answer = seed(); } function main() -> void {}";
    assert!(errors_of(src)
        .iter()
        .any(|e| e.code == Some("E-STATIC-INIT-TYPE")));
}

// ── definite assignment of instance fields (Soundness Batch D) ───────────────────────────────────

#[test]
fn non_optional_field_never_assigned_is_error() {
    // A non-optional field with no initializer and no ctor assignment is a latent runtime fault
    // ("no field x") — the `T` promise is unbacked. Reject it up front.
    let src =
        "class C { mutable int x; constructor() {} } function main() -> void { var c = new C(); }";
    assert!(has(src, "E-FIELD-UNINITIALIZED"), "{:?}", errors_of(src));
}

#[test]
fn non_optional_field_no_ctor_is_error() {
    let src = "class C { int x; } function main() -> void { }";
    assert!(has(src, "E-FIELD-UNINITIALIZED"), "{:?}", errors_of(src));
}

#[test]
fn non_optional_field_assigned_in_ctor_is_ok() {
    let src = "class C { mutable int x; constructor(int v) { this.x = v; } } \
               function main() -> void { var c = new C(5); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn non_optional_field_with_initializer_is_ok() {
    let src =
        "class C { int x = 0; constructor() {} } function main() -> void { var c = new C(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn promoted_non_optional_field_is_ok() {
    let src =
        "class C { constructor(public int x) {} } function main() -> void { var c = new C(5); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn conditionally_assigned_field_is_error() {
    // Assigned only in one branch — not on all paths (finding #4 GAP-2).
    let src = "class C { mutable int x; constructor(bool flag) { if (flag) { this.x = 7; } } } \
               function main() -> void { var c = new C(true); }";
    assert!(has(src, "E-FIELD-UNINITIALIZED"), "{:?}", errors_of(src));
}

#[test]
fn field_assigned_in_both_if_branches_is_ok() {
    let src = "class C { mutable int x; constructor(bool flag) { if (flag) { this.x = 1; } else { this.x = 2; } } } \
               function main() -> void { var c = new C(true); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn optional_field_without_initializer_is_ok() {
    // An optional field defaults to null — no assignment required (DEFAULT-NULL policy).
    let src = "class C { int? n; constructor() {} } function main() -> void { var c = new C(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}
