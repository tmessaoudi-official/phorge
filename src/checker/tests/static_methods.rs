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
    // Regression guard: a non-static method sees `this` (fields are always `this.field`, 2026-06-27).
    let src = "class C { int x = 0; function f() -> int { return this.x + this.x; } } \
               function main() -> void { }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

// --- slice B0: static method call sites `ClassName.method(args)` ------------------------------

#[test]
fn static_call_on_class_name_is_ok() {
    let src = "class C { static function f(int n) -> int { return n + 1; } } \
               function main() -> void { var r = C.f(2); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn static_call_type_checks_arguments() {
    // Wrong argument type is still an ordinary type error (the call resolves, the arg doesn't match).
    let src = "class C { static function f(int n) -> int { return n; } } \
               function main() -> void { var r = C.f(\"x\"); }";
    assert!(!errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn static_call_result_is_typed() {
    // The call's result type flows: assigning an `int`-returning static call to a `string` is an error.
    let src = "class C { static function f() -> int { return 1; } } \
               function main() -> void { string s = C.f(); }";
    assert!(!errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn instance_method_via_class_name_is_static_call_error() {
    let src = "class C { constructor() {} function inst() -> int { return 1; } } \
               function main() -> void { var r = C.inst(); }";
    assert!(has(src, "E-STATIC-CALL"), "{:?}", errors_of(src));
}

#[test]
fn unknown_static_method_is_error() {
    let src = "class C { static function f() -> int { return 1; } } \
               function main() -> void { var r = C.nope(); }";
    assert!(!errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn overloaded_static_call_is_rejected_for_now() {
    let src = "class C { static function f(int x) -> int { return x; } \
               static function f(string s) -> int { return 0; } } \
               function main() -> void { var r = C.f(1); }";
    assert!(has(src, "E-STATIC-CALL"), "{:?}", errors_of(src));
}

#[test]
fn static_factory_returns_instance() {
    // The static-factory pattern: a static method constructs and returns an instance.
    let src = "class C { constructor(public int x) {} \
               static function make(int v) -> C { return new C(v); } } \
               function main() -> void { var c = C.make(5); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}
