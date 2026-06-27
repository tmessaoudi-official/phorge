//! Checker tests — the checked `as` downcast operator (M4 casting axis 2).

use super::support::*;

/// Two classes implementing one interface, plus a union-typed local, reused across the cases.
fn prelude() -> String {
    "interface Shape { function area() -> int; } \
     class Circle implements Shape { constructor(public int r) {} function area() -> int { return this.r; } } \
     class Square implements Shape { constructor(public int s) {} function area() -> int { return this.s; } } "
        .to_string()
}

#[test]
fn cast_yields_optional_of_target() {
    // `v as Class` types as `Class?` — usable via `??` and if-let.
    let ok = prelude()
        + "function main() -> void { Shape s = new Circle(2); int r = (s as Circle)?.r ?? -1; }";
    assert!(errors_of(&ok).is_empty(), "got {:?}", errors_of(&ok));
    // The result is genuinely optional: binding it to a non-optional `Circle` must fail.
    let bad =
        prelude() + "function main() -> void { Shape s = new Circle(2); Circle c = s as Circle; }";
    let e = errors_of(&bad);
    assert!(
        e.iter().any(|d| d.code == Some("E-OPT-ASSIGN")),
        "expected E-OPT-ASSIGN (cast is T?), got {e:?}"
    );
    // Binding to `Circle?` is fine.
    let ok2 =
        prelude() + "function main() -> void { Shape s = new Circle(2); Circle? c = s as Circle; }";
    assert!(errors_of(&ok2).is_empty(), "got {:?}", errors_of(&ok2));
}

#[test]
fn cast_smart_cast_via_if_let() {
    // `if (var c = s as Circle)` binds `c: Circle` (the if-let narrows `Circle?` → `Circle`).
    let ok = prelude()
        + "function main() -> void { Shape s = new Circle(2); if (var c = s as Circle) { int r = c.r; } }";
    assert!(errors_of(&ok).is_empty(), "got {:?}", errors_of(&ok));
}

#[test]
fn cast_to_interface_and_union_scrutinee() {
    // Interface target is allowed.
    let ok_iface = prelude()
        + "class Dog { constructor() {} } function main() -> void { Dog d = new Dog(); Shape? s = d as Shape; }";
    assert!(
        errors_of(&ok_iface).is_empty(),
        "got {:?}",
        errors_of(&ok_iface)
    );
    // A union-typed scrutinee can be downcast to one of its members.
    let ok_union = prelude()
        + "function main() -> void { Circle|Square v = new Circle(2); Circle? c = v as Circle; }";
    assert!(
        errors_of(&ok_union).is_empty(),
        "got {:?}",
        errors_of(&ok_union)
    );
}

#[test]
fn cast_rejects_primitive_target() {
    // `x as int` is rejected — `as` is assertion, not value conversion (use Core.Convert).
    let e = errors_of("function main() -> void { int x = 3; int? y = x as int; }");
    assert!(
        e.iter().any(|d| d.code == Some("E-CAST-TYPE")),
        "expected E-CAST-TYPE for a primitive target, got {e:?}"
    );
}

#[test]
fn cast_rejects_unknown_and_trait_target() {
    // An undeclared name on the right is E-CAST-TYPE.
    let e1 = errors_of(
        "class C { constructor() {} } function main() -> void { C c = new C(); C? x = c as Nope; }",
    );
    assert!(
        e1.iter().any(|d| d.code == Some("E-CAST-TYPE")),
        "got {e1:?}"
    );
    // A trait is reuse, not a type → E-CAST-TYPE.
    let e2 = errors_of(
        "trait T { function hi() -> int { return 1; } } \
         class C { use T; constructor() {} } \
         function main() -> void { C c = new C(); C? x = c as T; }",
    );
    assert!(
        e2.iter().any(|d| d.code == Some("E-CAST-TYPE")),
        "got {e2:?}"
    );
}

#[test]
fn cast_rejects_non_instance_left_operand() {
    // The left operand must be a class instance / union — a plain `int` is rejected.
    let e = errors_of(
        "class C { constructor() {} } function main() -> void { int n = 3; C? x = n as C; }",
    );
    assert!(e.iter().any(|d| d.code == Some("E-CAST-TYPE")), "got {e:?}");
}
