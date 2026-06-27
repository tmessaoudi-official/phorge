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

// ── S1: `as` → primitive conversions (Unified, fallibility-typed) ────────────────────────────────

#[test]
fn as_int_to_float_is_total() {
    // Lossless widening → total `float` (not optional). Binding to `float` is fine; to `int` fails.
    assert!(errors_of("function main() -> void { int n = 3; float f = n as float; }").is_empty());
    let e = errors_of("function main() -> void { int n = 3; int bad = n as float; }");
    assert!(
        e.iter()
            .any(|d| d.message.contains("expected `int`, found `float`")),
        "{e:?}"
    );
}

#[test]
fn as_float_to_int_is_optional() {
    // Lossy narrowing → `int?` (exact-or-null, never a silent truncate). Binding to non-optional
    // `int` must fail; to `int?` is fine.
    let e = errors_of("function main() -> void { float f = 3.0; int n = f as int; }");
    assert!(
        e.iter().any(|d| d.code == Some("E-OPT-ASSIGN")),
        "expected E-OPT-ASSIGN (float as int is int?), got {e:?}"
    );
    assert!(errors_of("function main() -> void { float f = 3.0; int? n = f as int; }").is_empty());
}

#[test]
fn as_string_parse_is_optional() {
    // `string as int`/`as float` are fallible parses → `T?`.
    assert!(
        errors_of("function main() -> void { string s = \"5\"; int? n = s as int; }").is_empty()
    );
    assert!(
        errors_of("function main() -> void { string s = \"5\"; float? f = s as float; }")
            .is_empty()
    );
}

#[test]
fn as_any_to_string_is_total() {
    // Every primitive → string is total (reuses Convert.toString).
    assert!(errors_of("function main() -> void { int n = 3; string s = n as string; }").is_empty());
    assert!(
        errors_of("function main() -> void { float f = 1.5; string s = f as string; }").is_empty()
    );
}

#[test]
fn as_int_and_decimal_round_trip_types() {
    // int → decimal (total widen); decimal → int (exact-or-null → int?).
    assert!(
        errors_of("function main() -> void { int n = 3; decimal d = n as decimal; }").is_empty()
    );
    let e = errors_of("function main() -> void { decimal d = 3.00d; int n = d as int; }");
    assert!(
        e.iter().any(|d| d.code == Some("E-OPT-ASSIGN")),
        "expected E-OPT-ASSIGN (decimal as int is int?), got {e:?}"
    );
}

#[test]
fn as_identity_warns_redundant_but_is_not_an_error() {
    // `T as T` is the identity — no error, but a `W-REDUNDANT-CAST` lint fires.
    let src = "function main() -> void { int n = 3; int m = n as int; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    let w = warnings_of(src);
    assert!(
        w.iter().any(|d| d.code == Some("W-REDUNDANT-CAST")),
        "expected W-REDUNDANT-CAST, got {w:?}"
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
