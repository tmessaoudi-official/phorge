//! Checker tests — interfaces (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn interface_conformance_and_subtyping_ok() {
    // A class providing every interface method type-checks; its instance flows into an
    // interface-typed parameter (nominal subtyping) and an interface-typed local.
    let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> string { return \"w\"; } } \
                   function announce(Speaker s) -> string { return s.speak(); } \
                   function main() -> void { Speaker sp = new Dog(); discard announce(sp); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn interface_missing_method_is_unimpl() {
    let src = "interface Speaker { function speak() -> string; } \
                   class Mute implements Speaker {} \
                   function main() -> void {}";
    let e = errors_of(src);
    assert!(e.iter().any(|d| d.code == Some("E-IFACE-UNIMPL")), "{e:?}");
}

#[test]
fn interface_wrong_signature_is_sig() {
    // `speak` must return `string`; returning `int` is a signature mismatch.
    let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> int { return 1; } } \
                   function main() -> void {}";
    let e = errors_of(src);
    assert!(e.iter().any(|d| d.code == Some("E-IFACE-SIG")), "{e:?}");
}

#[test]
fn implements_a_non_interface_is_impl_error() {
    // `implements` must name a declared interface, not a class.
    let src = "class A {} class B implements A {} function main() -> void {}";
    let e = errors_of(src);
    assert!(e.iter().any(|d| d.code == Some("E-IFACE-IMPL")), "{e:?}");
}

#[test]
fn interface_extends_cycle_is_rejected() {
    let src = "interface A extends B { function a() -> int; } \
                   interface B extends A { function b() -> int; } \
                   function main() -> void {}";
    let e = errors_of(src);
    assert!(e.iter().any(|d| d.code == Some("E-IFACE-CYCLE")), "{e:?}");
}

#[test]
fn interface_is_not_assignable_to_unrelated_class() {
    // A Speaker is not a Dog: interface → concrete class is not a subtype.
    let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> string { return \"w\"; } } \
                   function main() -> void { Speaker s = new Dog(); Dog d = s; }";
    let e = errors_of(src);
    assert!(!e.is_empty(), "expected an assignability error, got none");
}

#[test]
fn instanceof_against_interface_narrows() {
    // `instanceof` accepts an interface RHS, and inside the then-block the operand is
    // smart-cast to the interface so its methods resolve.
    let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> string { return \"w\"; } } \
                   function main() -> void { Dog d = new Dog(); \
                     if (d instanceof Speaker) { discard d.speak(); } }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}
