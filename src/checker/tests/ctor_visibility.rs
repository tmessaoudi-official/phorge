//! Checker tests — constructor visibility enforcement (Soundness Batch A).
//!
//! A `private`/`protected constructor` must block external `new C(...)` — the missing **7th**
//! member-visibility access site (construction), alongside the six in `visibility.rs`. Modifiers on
//! a constructor were previously parsed and *dropped*; they are now threaded to the AST, the checker
//! enforces visibility at the construction site (`E-CTOR-VISIBILITY`), and non-visibility modifiers
//! on a constructor are rejected (`E-CTOR-MODIFIER`).

use super::support::*;

fn has(src: &str, code: &str) -> bool {
    errors_of(src).iter().any(|e| e.code == Some(code))
}

// ── construction visibility ────────────────────────────────────────────────────────────────────

#[test]
fn external_new_on_private_ctor_is_error() {
    let src = "class Secret { private constructor(public int x) {} } \
               function main() -> void { var s = new Secret(42); }";
    assert!(has(src, "E-CTOR-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn external_new_on_protected_ctor_is_error() {
    let src = "class Secret { protected constructor(public int x) {} } \
               function main() -> void { var s = new Secret(7); }";
    assert!(has(src, "E-CTOR-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn external_new_on_explicit_public_ctor_is_ok() {
    let src = "class C { public constructor(public int x) {} } \
               function main() -> void { var c = new C(5); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn external_new_on_default_ctor_is_ok() {
    // No modifier = public (regression guard: the common case must stay clean).
    let src = "class C { constructor(public int x) {} } \
               function main() -> void { var c = new C(5); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn internal_new_on_private_ctor_in_method_is_ok() {
    // A method of the class itself constructs it — `cur_class == C`, so the private ctor is in scope.
    let src = "class C { private constructor(public int x) {} \
                         function dup() -> C { return new C(this.x + 1); } } \
               function main() -> void { }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn internal_new_on_private_ctor_in_static_init_is_ok() {
    // The singleton pattern: a static field initializer runs in the class's own scope, so it may
    // call the private constructor (it just cannot use `this`).
    let src = "class Config { private constructor(public int port) {} \
                              static Config instance = new Config(8080); } \
               function main() -> void { }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn static_init_constructing_other_private_ctor_class_is_error() {
    // Class `A`'s static init is in `A`'s scope, NOT `B`'s — so it cannot call `B`'s private ctor.
    let src = "class B { private constructor(public int x) {} } \
               class A { static B b = new B(1); } \
               function main() -> void { }";
    assert!(has(src, "E-CTOR-VISIBILITY"), "{:?}", errors_of(src));
}

#[test]
fn this_is_still_forbidden_in_static_init() {
    // Setting `cur_class` for static-init visibility must not re-enable `this` (no instance exists).
    let src = "class C { private constructor(public int x) {} \
                         static int n = this.x; } \
               function main() -> void { }";
    assert!(
        !errors_of(src).is_empty(),
        "expected `this` rejection, got none"
    );
}

// ── modifier sanity (closes the §5 dropped-modifier variants) ────────────────────────────────────

#[test]
fn abstract_constructor_is_error() {
    let src = "class C { abstract constructor(public int x) {} } \
               function main() -> void { }";
    assert!(has(src, "E-CTOR-MODIFIER"), "{:?}", errors_of(src));
}

#[test]
fn static_constructor_is_error() {
    let src = "class C { static constructor(public int x) {} } \
               function main() -> void { }";
    assert!(has(src, "E-CTOR-MODIFIER"), "{:?}", errors_of(src));
}

#[test]
fn const_constructor_is_error() {
    let src = "class C { const constructor(public int x) {} } \
               function main() -> void { }";
    assert!(has(src, "E-CTOR-MODIFIER"), "{:?}", errors_of(src));
}
