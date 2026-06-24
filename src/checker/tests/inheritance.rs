//! Checker tests — inheritance (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn subclass_is_assignable_and_inherits_methods() {
    // S6a.3: Dog <: Animal (assignability) + Dog inherits Animal's method.
    let errs = errors_of(
        "open class Animal { function name() -> string { return \"a\"; } } \
             class Dog extends Animal {} \
             function f() -> string { Animal a = Dog(); return a.name(); } \
             function g() -> string { Dog d = Dog(); return d.name(); }",
    );
    assert!(errs.is_empty(), "expected clean, got {errs:?}");
}

#[test]
fn extending_a_non_open_class_errors() {
    let errs = errors_of("class Animal {} class Dog extends Animal {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-EXTEND-FINAL")),
        "got {errs:?}"
    );
}

#[test]
fn extending_an_unknown_name_errors() {
    let errs = errors_of("class Dog extends Bogus {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-EXTEND-UNKNOWN")),
        "got {errs:?}"
    );
}

#[test]
fn class_extends_cycle_errors() {
    let errs = errors_of("open class A extends B {} open class B extends A {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-MI-CYCLE")),
        "got {errs:?}"
    );
}

#[test]
fn overriding_a_final_method_errors() {
    // S6a.4: Animal.kind is final-by-default; Dog redefining it is E-OVERRIDE-FINAL.
    let errs = errors_of(
        "open class Animal { function kind() -> string { return \"a\"; } } \
             class Dog extends Animal { function kind() -> string { return \"d\"; } }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERRIDE-FINAL")),
        "got {errs:?}"
    );
}

#[test]
fn overriding_an_open_method_is_allowed() {
    // S6a.4: marking the parent method `open` permits the override.
    let errs = errors_of(
        "open class Animal { open function kind() -> string { return \"a\"; } } \
             class Dog extends Animal { function kind() -> string { return \"d\"; } }",
    );
    assert!(errs.is_empty(), "got {errs:?}");
}

#[test]
fn instantiating_an_abstract_class_errors() {
    // S6b.3: an abstract class cannot be constructed.
    let errs = errors_of(
        "abstract class Shape { abstract function area() -> int; } \
             function main() -> void { Shape s = Shape(); }",
    );
    assert!(
        errs.iter()
            .any(|e| e.code == Some("E-ABSTRACT-INSTANTIATE")),
        "got {errs:?}"
    );
}

#[test]
fn concrete_subclass_missing_abstract_impl_errors() {
    // S6b.3: a non-abstract subclass must implement every inherited abstract method.
    let errs = errors_of(
        "abstract class Shape { abstract function area() -> int; } \
             class Blob extends Shape {}",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-ABSTRACT-UNIMPL")),
        "got {errs:?}"
    );
}

#[test]
fn abstract_method_in_concrete_class_errors() {
    // S6b.3: a non-abstract class may not itself declare an abstract method (same check, origin is
    // the class itself).
    let errs = errors_of("class Shape { abstract function area() -> int; }");
    assert!(
        errs.iter().any(|e| e.code == Some("E-ABSTRACT-UNIMPL")),
        "got {errs:?}"
    );
}

#[test]
fn concrete_subclass_implementing_abstract_is_ok() {
    // S6b.3: providing the body satisfies the abstract contract — no error.
    let errs = errors_of(
        "abstract class Shape { abstract function area() -> int; } \
             class Square extends Shape { constructor(public int side) {} \
                 function area() -> int { return this.side * this.side; } }",
    );
    assert!(
        !errs
            .iter()
            .any(|e| matches!(e.code, Some("E-ABSTRACT-UNIMPL") | Some("E-OVERRIDE-FINAL"))),
        "got {errs:?}"
    );
}

#[test]
fn open_static_method_errors() {
    // S6b.3: a method cannot be both `open` and `static` (statics are not virtual).
    let errs = errors_of("class C { open static function f() -> int { return 1; } }");
    assert!(
        errs.iter().any(|e| e.code == Some("E-OPEN-STATIC")),
        "got {errs:?}"
    );
}

#[test]
fn unresolved_cross_parent_collision_errors() {
    // S6b.2: two parents each declare `move`; `Duck` neither resolves nor overrides it.
    let errs = errors_of(
        "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer {}",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
        "got {errs:?}"
    );
}

#[test]
fn use_clause_resolves_the_collision() {
    // S6b.2: `use Swimmer.move` picks a winner — no conflict.
    let errs = errors_of(
        "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { use Swimmer.move }",
    );
    assert!(
        !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
        "got {errs:?}"
    );
}

#[test]
fn exclude_clause_resolves_the_collision() {
    // S6b.2: `exclude Flyer.move` drops one source, leaving `move` unambiguous.
    let errs = errors_of(
        "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { exclude Flyer.move }",
    );
    assert!(
        !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
        "got {errs:?}"
    );
}

#[test]
fn child_override_resolves_the_collision() {
    // S6b.2: declaring `move` in the child overrides both parents — no conflict (and the parent
    // methods are `open`, so the override itself is legal).
    let errs = errors_of(
        "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { function move() -> string { return \"d\"; } }",
    );
    assert!(
        !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
        "got {errs:?}"
    );
}

#[test]
fn diamond_shared_base_is_not_a_conflict() {
    // S6b.2: `Mid` reaches `Base.tag` through both arms, but both resolve to the same declaring
    // method — auto-merge, never E-MI-CONFLICT.
    let errs = errors_of(
        "open class Base { open function tag() -> string { return \"b\"; } } \
             open class Left extends Base {} open class Right extends Base {} \
             class Mid extends Left, Right {}",
    );
    assert!(
        !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
        "got {errs:?}"
    );
}

#[test]
fn overriding_a_final_method_of_the_second_parent_errors() {
    // S6b.1: override-finality is checked against *every* parent, not just the first. `Flyer.move`
    // (the second parent) is final-by-default; `Duck` redefining it is E-OVERRIDE-FINAL even
    // though the first parent has no such method.
    let errs = errors_of(
        "open class Swimmer { open function dive() -> string { return \"d\"; } } \
             open class Flyer { function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { function move() -> string { return \"m\"; } }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERRIDE-FINAL")),
        "got {errs:?}"
    );
}
