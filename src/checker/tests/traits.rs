//! Checker tests — traits (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn s8_use_unknown_trait_errors() {
    // M-RT S8: `use` must name a declared trait.
    let errs = errors_of("class C { use Nope; } function main() {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-USE-UNKNOWN")),
        "got {errs:?}"
    );
}

#[test]
fn s8_use_a_class_as_trait_errors() {
    // M-RT S8: `use` composes a trait, not a class — naming a class is E-USE-UNKNOWN.
    let errs = errors_of("class Base {} class C { use Base; } function main() {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-USE-UNKNOWN")),
        "got {errs:?}"
    );
}

#[test]
fn s8_instanceof_trait_errors() {
    // M-RT S8: a trait is not a type — it cannot be an `instanceof` target.
    let errs = errors_of(
        "trait T { function f() -> int { return 1; } } \
             class C { use T; } \
             function main() { C c = C(); var b = c instanceof T; }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-INSTANCEOF-TYPE")),
        "got {errs:?}"
    );
}

#[test]
fn s8_trait_as_type_annotation_errors() {
    // M-RT S8: a value cannot be typed as a trait.
    let errs = errors_of(
        "trait T { function f() -> int { return 1; } } \
             function main() { T x = x; }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-USE-AS-TYPE")),
        "got {errs:?}"
    );
}

#[test]
fn s8_unmet_trait_abstract_requirement_errors() {
    // M-RT S8: a using class must satisfy a trait's abstract requirement (reuses E-ABSTRACT-UNIMPL).
    let errs = errors_of(
        "trait Greeter { abstract function name() -> string; } \
             class P { use Greeter; } function main() {}",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-ABSTRACT-UNIMPL")),
        "got {errs:?}"
    );
}

#[test]
fn s8_two_trait_constructors_collide() {
    // M-RT S8 T3: a class composing two ctor-bearing traits (no own ctor) is E-TRAIT-CTOR-COLLISION.
    let errs = errors_of(
        "trait A { constructor(public int a) {} } \
             trait B { constructor(public int b) {} } \
             class C { use A; use B; } function main() {}",
    );
    assert!(
        errs.iter()
            .any(|e| e.code == Some("E-TRAIT-CTOR-COLLISION")),
        "got {errs:?}"
    );
}

#[test]
fn s8_class_ctor_shadows_trait_ctor_warns() {
    // M-RT S8 T3 (D8): a class's own ctor shadows a `use`d trait's ctor — warning, not error.
    let warns = warnings_of(
        "trait T { constructor(public int id) {} } \
             class C { use T; constructor() {} } function main() {}",
    );
    assert!(
        warns
            .iter()
            .any(|w| w.code == Some("W-TRAIT-CTOR-SHADOWED")),
        "got {warns:?}"
    );
}

#[test]
fn s8_trait_ctor_skips_parent_warns() {
    // M-RT S8 T3 (D6): trait ctor wins over an inherited parent ctor — warn the silent skip.
    let warns = warnings_of(
        "open class Base { constructor(public int b) {} } \
             trait T { constructor(public int id) {} } \
             class C extends Base { use T; } function main() {}",
    );
    assert!(
        warns
            .iter()
            .any(|w| w.code == Some("W-TRAIT-CTOR-PARENT-SKIPPED")),
        "got {warns:?}"
    );
}
