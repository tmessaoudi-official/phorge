//! Checker tests — super/parent dispatch (M-RT super/parent, B1a: methods, single inheritance).

use super::support::*;

#[test]
fn immediate_and_ancestor_jump_parent_calls_are_legal() {
    let errs = errors_of(
        "open class Animal { open function describe() -> string { return \"a\"; } } \
         open class Dog extends Animal { open function describe() -> string { return parent.describe(); } } \
         class Puppy extends Dog { function describe() -> string { \
             string a = parent.describe(); string b = parent(Animal).describe(); return \"{a}{b}\"; } } \
         function main() -> void {}",
    );
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn parent_in_class_with_no_parent_errors() {
    let errs = errors_of(
        "class A { function m() -> int { return parent.m(); } } function main() -> void {}",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-PARENT-NO-PARENT")),
        "{errs:?}"
    );
}

#[test]
fn parent_in_a_free_function_errors() {
    let errs = errors_of(
        "open class A { open function m() -> int { return 1; } } \
         function f() -> int { return parent.m(); } function main() -> void {}",
    );
    assert!(
        errs.iter()
            .any(|e| e.code == Some("E-PARENT-OUTSIDE-METHOD")),
        "{errs:?}"
    );
}

#[test]
fn parent_qualified_with_non_ancestor_errors() {
    let errs = errors_of(
        "open class A { open function m() -> int { return 1; } } \
         open class B { open function m() -> int { return 2; } } \
         class C extends A { function m() -> int { return parent(B).m(); } } \
         function main() -> void {}",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-PARENT-NOT-ANCESTOR")),
        "{errs:?}"
    );
}

#[test]
fn parent_call_to_unknown_method_errors() {
    let errs = errors_of(
        "open class A { open function m() -> int { return 1; } } \
         class C extends A { function m() -> int { return parent.nope(); } } \
         function main() -> void {}",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-PARENT-NO-METHOD")),
        "{errs:?}"
    );
}

#[test]
fn parent_constructor_is_deferred_in_b1a() {
    // `parent.constructor(…)` parses but is not yet implemented (methods-only slice).
    let errs = errors_of(
        "open class A { constructor(public int x) {} } \
         class C extends A { constructor() { parent.constructor(1); } } \
         function main() -> void {}",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-PARENT-NO-METHOD")),
        "{errs:?}"
    );
}

#[test]
fn parent_used_as_a_value_identifier_is_unaffected() {
    // `parent` is contextual — recognized only as a call head (`parent.`/`parent(`). As a string key
    // (or any non-call-head position) it is an ordinary token, so this program is clean.
    let errs =
        errors_of("import Core.Console; function main() -> void { Console.println(\"parent\"); }");
    assert!(errs.is_empty(), "{errs:?}");
}
