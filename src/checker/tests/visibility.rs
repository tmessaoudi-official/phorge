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

// ── static fields (W0-2, P0 spine repair) ──────────────────────────────────────────────────────
// A `private`/`protected` static read/write from outside its scope used to check clean, print on
// run/runvm, and FATAL on the PHP leg (`Cannot access private property A::$s`) — a three-way spine
// break. Static fields now carry visibility like consts + instance fields, gated on both paths.

#[test]
fn external_private_static_read_is_error() {
    let src = "class A { private static int s = 3; } \
               function main() -> void { var x = A.s; }";
    assert!(has(src, "E-FIELD-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn external_public_static_read_is_ok() {
    let src = "class A { static int s = 3; } \
               function main() -> void { var x = A.s; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn internal_private_static_read_is_ok() {
    let src = "class A { private static int s = 3; \
                         function get() -> int { return A.s; } } \
               function main() -> void { var a = new A(); var x = a.get(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn external_private_static_write_is_error() {
    let src = "class A { private static mutable int s = 3; } \
               function main() -> void { A.s = 9; }";
    assert!(has(src, "E-FIELD-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn external_protected_static_read_is_error() {
    let src = "class A { protected static int s = 3; } \
               function main() -> void { var x = A.s; }";
    assert!(has(src, "E-FIELD-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn protected_static_read_from_subclass_is_ok() {
    let src = "open class B { protected static int s = 3; } \
               class D extends B { function ds() -> int { return D.s; } } \
               function main() -> void { var d = new D(); var v = d.ds(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn internal_private_static_read_from_static_method_is_ok() {
    // A static method of the owning class must still see its own private static — `cur_class` has to
    // be set inside a `static function` body (it has no `this`, but the enclosing class IS in scope
    // for visibility). Guards against the W0-2 gate over-rejecting in-class static-method access.
    let src = "class A { private static int s = 3; \
                         static function peek() -> int { return A.s; } } \
               function main() -> void { var x = A.peek(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn external_private_static_compound_write_is_error() {
    // `A.s += 1` desugars to `A.s = A.s + 1` — the read leg (`A.s`) AND the write leg both hit the
    // W0-2 gate, so a compound write from outside is rejected too (the P0 is closed for `+=`/`++`,
    // not only plain `=`).
    let src = "class A { private static mutable int s = 3; } \
               function main() -> void { A.s += 1; }";
    assert!(has(src, "E-FIELD-VISIBILITY"), "{:?}", errors_of(src));
}
