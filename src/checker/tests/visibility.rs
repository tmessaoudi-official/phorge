//! Checker tests — member visibility enforcement (Wave 1.1).
//!
//! `private`/`protected` on instance fields, promoted ctor params, and methods is now enforced by
//! the checker (not just emitted to PHP), so `run ≡ runvm ≡ transpiled PHP` all agree: an out-of-scope
//! access is rejected up front instead of passing on the Phorj backends and throwing in PHP.

use super::support::*;

fn has(src: &str, code: &str) -> bool {
    errors_of(src).iter().any(|e| e.code == Some(code))
}

// ── instance fields ──────────────────────────────────────────────────────────────────────────

#[test]
fn external_private_field_read_is_error() {
    let src = "class C { constructor(private int secret) {} } \
               function main() -> void { var c = new C(5); var x = c.secret; }";
    assert!(has(src, "E-FIELD-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn external_public_field_read_is_ok() {
    let src = "class C { constructor(public int val) {} } \
               function main() -> void { var c = new C(5); var x = c.val; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn internal_private_field_read_via_this_is_ok() {
    let src = "class C { constructor(private int secret) {} \
                         function get() -> int { return this.secret; } } \
               function main() -> void { var c = new C(5); var x = c.get(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn external_private_field_write_is_error() {
    let src = "class C { constructor(private mutable int secret) {} } \
               function main() -> void { var c = new C(5); c.secret = 9; }";
    assert!(has(src, "E-FIELD-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn external_with_private_field_is_error() {
    // `obj with { f = e }` lowers to PHP `clone($o, ["f" => e])`, which PHP enforces visibility on
    // (`Cannot access private property`) — so an external override of a private field must be rejected.
    let src = "class C { constructor(private int secret) {} } \
               function main() -> void { var c = new C(5); var d = c with { secret = 9 }; }";
    assert!(has(src, "E-FIELD-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn external_with_public_field_is_ok() {
    let src = "class C { constructor(public int val) {} } \
               function main() -> void { var c = new C(5); var d = c with { val = 9 }; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

// ── methods ──────────────────────────────────────────────────────────────────────────────────

#[test]
fn external_private_method_call_is_error() {
    let src = "class C { private function secret() -> int { return 1; } } \
               function main() -> void { var c = new C(); var x = c.secret(); }";
    assert!(has(src, "E-METHOD-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn external_public_method_call_is_ok() {
    let src = "class C { function shown() -> int { return 1; } } \
               function main() -> void { var c = new C(); var x = c.shown(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn internal_private_method_call_is_ok() {
    let src = "class C { private function helper() -> int { return 1; } \
                         function run() -> int { return this.helper(); } } \
               function main() -> void { var c = new C(); var x = c.run(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn external_struct_destructure_of_private_field_is_error() {
    // `var C { secret } = c` lowers to a field read → PHP `$c->secret` → rejected if private.
    let src = "class C { constructor(private int secret) {} } \
               function main() -> void { var c = new C(7); var C { secret } = c; }";
    assert!(has(src, "E-FIELD-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn external_struct_destructure_of_public_field_is_ok() {
    let src = "class C { constructor(public int val) {} } \
               function main() -> void { var c = new C(7); var C { val } = c; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn external_match_struct_pattern_of_private_field_is_error() {
    let src = "class C { constructor(private int secret) {} } \
               function main() -> int { var c = new C(7); return match c { C { secret } => secret, _ => 0 }; }";
    assert!(has(src, "E-FIELD-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn field_initializer_reads_private_sibling_via_this_is_ok() {
    // Field initializers are checked with `cur_class` set (program.rs check_type_body), so reading an
    // earlier private sibling through `this` is in-scope — guards against a visibility false positive.
    let src = "class C { private int a = 5; int b = this.a + 1; \
                         function bOf() -> int { return this.b; } } \
               function main() -> void { var c = new C(); var x = c.bOf(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

// ── protected + inheritance (owner preserved through `extends`) ─────────────────────────────────

#[test]
fn protected_field_read_from_subclass_is_ok() {
    let src = "open class B { constructor(protected int x) {} } \
               class D extends B { function dx() -> int { return this.x; } } \
               function main() -> void { var d = new D(5); var v = d.dx(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn protected_field_read_from_outside_is_error() {
    let src = "class B { constructor(protected int x) {} } \
               function main() -> void { var b = new B(5); var v = b.x; }";
    assert!(has(src, "E-FIELD-VISIBILITY"), "{:?}", errors_of(src));
}
