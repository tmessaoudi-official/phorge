//! Checker tests — static method semantics (Soundness Batch E, finding #5).
//!
//! A `static` method has no instance, so it must not access instance state: `this` and bare instance
//! fields are rejected (`E-STATIC-THIS`). It may still access static members and construct the class
//! (the factory pattern keeps `cur_class` for ctor visibility).

use super::support::*;

fn has(src: &str, code: &str) -> bool {
    errors_of(src).iter().any(|e| e.code == Some(code))
}

#[test]
fn static_method_using_this_is_error() {
    let src = "class C { int x = 0; static function f() -> int { return this.x; } } \
               function main() -> void { }";
    assert!(has(src, "E-STATIC-THIS"), "{:?}", errors_of(src));
}

#[test]
fn static_method_using_bare_instance_field_is_error() {
    let src = "class C { int x = 0; static function f() -> int { return x; } } \
               function main() -> void { }";
    assert!(has(src, "E-STATIC-THIS"), "{:?}", errors_of(src));
}

#[test]
fn static_method_not_touching_instance_is_ok() {
    let src = "class C { static function f(int n) -> int { return n + 1; } } \
               function main() -> void { }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn static_method_reading_static_field_is_ok() {
    let src = "class C { static int count = 0; static function f() -> int { return C.count; } } \
               function main() -> void { }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn instance_method_using_this_is_ok() {
    // Regression guard: a non-static method still sees `this` and bare fields.
    let src = "class C { int x = 0; function f() -> int { return this.x + x; } } \
               function main() -> void { }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}
