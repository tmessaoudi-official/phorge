//! Checker tests — matching (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn match_over_optional() {
    // null arm + catch-all binding is exhaustive for `T?`, and the binding narrows to inner `T`
    // (so it can be used as a non-optional — here as an `int` arithmetic operand)
    assert!(
        errors_of("function f(int? o) -> int { return match o { null => -1, v => v + 1 }; }")
            .is_empty()
    );
    // a `null` pattern requires an optional scrutinee
    let e1 = errors_of("function main() { int n = 3; int x = match n { null => 0, v => v }; }");
    assert!(
        e1.iter().any(|d| d.message.contains("`null` pattern")),
        "got {e1:?}"
    );
    // a `null` arm alone (no catch-all for the non-null case) is non-exhaustive
    let e2 = errors_of("function f(int? o) -> int { return match o { null => -1 }; }");
    assert!(
        e2.iter().any(|d| d.message.contains("non-exhaustive")),
        "got {e2:?}"
    );
}

#[test]
fn match_over_enum_is_typed_and_exhaustive() {
    let src = format!(
        "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => 3.14 * r * r, Rect(w, h) => w * h, }}; }}"
    );
    assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
}

#[test]
fn non_exhaustive_match_errors() {
    let src = format!(
        "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => 3.14 * r * r, }}; }}"
    );
    let errs = errors_of(&src);
    assert!(
        errs.iter()
            .any(|e| e.message.contains("non-exhaustive") && e.message.contains("Rect")),
        "{errs:?}"
    );
}

#[test]
fn non_exhaustive_match_lists_missing_variants_sorted() {
    // Variants declared out of alphabetical order; covering the middle one leaves Gamma+Beta
    // missing. The list must render sorted ("Beta, Gamma") regardless of the HashMap key order,
    // so the error message is deterministic across runs (no intermittent test/diff hazard).
    let src = "enum E { Gamma(int x), Alpha(int x), Beta(int x) } \
                   function f(E e) -> int { return match e { Alpha(x) => x, }; } \
                   function main() {}";
    let errs = errors_of(src);
    assert!(
        errs.iter().any(|e| e
            .message
            .contains("non-exhaustive match: missing Beta, Gamma")),
        "{errs:?}"
    );
}

#[test]
fn wildcard_makes_match_exhaustive() {
    let src = format!(
        "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, _ => 0.0, }}; }}"
    );
    assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
}

#[test]
fn match_arm_type_mismatch_errors() {
    let src = format!(
        "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, Rect(w, h) => true, }}; }}"
    );
    let errs = errors_of(&src);
    assert!(
        errs.iter().any(|e| e.message.contains("match arms")),
        "{errs:?}"
    );
}

#[test]
fn variant_pattern_arity_checked() {
    let src = format!(
        "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r, x) => r, Rect(w, h) => w, }}; }}"
    );
    let errs = errors_of(&src);
    assert!(
        errs.iter().any(|e| e.message.contains("expects 1 field")),
        "{errs:?}"
    );
}

#[test]
fn unknown_variant_pattern_errors() {
    let src = format!(
        "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, Triangle(x) => x, Rect(w,h) => w, }}; }}"
    );
    let errs = errors_of(&src);
    assert!(
        errs.iter()
            .any(|e| e.message.contains("no variant `Triangle`")),
        "{errs:?}"
    );
}

#[test]
fn struct_destructuring_patterns() {
    const SHAPES: &str = "class Circle { constructor(public float r) {} } \
                          class Square { constructor(public float side) {} }";
    const PL: &str = "class Point { constructor(public int x, public int y) {} } \
                      class Line { constructor(public Point from, public Point to) {} }";

    // Shorthand struct patterns over a union are exhaustive and type-check; the bound fields are
    // usable at their declared type (here `float`).
    let ok = format!(
        "{SHAPES} function f(Circle | Square s) -> float {{ return match s {{ \
             Circle {{ r }} => r, Square {{ side }} => side, }}; }}"
    );
    assert!(errors_of(&ok).is_empty(), "{:?}", errors_of(&ok));

    // Rename + nested destructuring + a CTy operand (`x + y`, both `int` binds): type-checks.
    let nested = format!(
        "{PL} function f(Line l) -> int {{ return match l {{ \
             Line {{ from: Point {{ x, y }}, to }} => x + y + to.x, _ => 0, }}; }}"
    );
    assert!(errors_of(&nested).is_empty(), "{:?}", errors_of(&nested));

    // E-STRUCT-PAT-TYPE — the head names something that isn't a class.
    let bad_type = format!(
        "{SHAPES} function f(Circle | Square s) -> float {{ return match s {{ \
             Nope {{ r }} => r, _ => 0.0, }}; }}"
    );
    assert!(
        errors_of(&bad_type)
            .iter()
            .any(|d| d.code == Some("E-STRUCT-PAT-TYPE")),
        "{:?}",
        errors_of(&bad_type)
    );

    // E-STRUCT-FIELD-UNKNOWN — a field that the class does not declare.
    let bad_field = format!(
        "{SHAPES} function f(Circle | Square s) -> float {{ return match s {{ \
             Circle {{ q }} => 0.0, Square {{ side }} => side, }}; }}"
    );
    assert!(
        errors_of(&bad_field)
            .iter()
            .any(|d| d.code == Some("E-STRUCT-FIELD-UNKNOWN")),
        "{:?}",
        errors_of(&bad_field)
    );

    // E-PATTERN-DUP-BIND — `x` is bound twice (field `x` and renamed field `y`).
    let dup = "class Point { constructor(public int x, public int y) {} } \
         function f(Point p) -> int { return match p { Point { x, y: x } => x, _ => 0, }; }";
    assert!(
        errors_of(dup)
            .iter()
            .any(|d| d.code == Some("E-PATTERN-DUP-BIND")),
        "{:?}",
        errors_of(dup)
    );
}

#[test]
fn match_arm_guards() {
    // A guarded arm plus an unguarded fallback for the same shape is exhaustive and type-checks.
    let ok = format!(
        "{SHAPE} function f(Shape s) -> float {{ return match s {{ \
             Circle(r) when r > 0.0 => r, Circle(r) => 0.0, Rect(w, h) => w * h, }}; }}"
    );
    assert!(errors_of(&ok).is_empty(), "{:?}", errors_of(&ok));

    // A shape covered ONLY by a guarded arm (no unguarded fallback) is non-exhaustive — the guard
    // may fall through — and is reported with the E-MATCH-GUARD-EXHAUST code.
    let guarded_only = format!(
        "{SHAPE} function f(Shape s) -> float {{ return match s {{ \
             Circle(r) when r > 0.0 => r, Rect(w, h) => w * h, }}; }}"
    );
    let e = errors_of(&guarded_only);
    assert!(
        e.iter().any(|d| d.code == Some("E-MATCH-GUARD-EXHAUST")),
        "{e:?}"
    );

    // A non-boolean guard is rejected with E-GUARD-TYPE.
    let bad_guard = format!(
        "{SHAPE} function f(Shape s) -> float {{ return match s {{ \
             Circle(r) when r => r, Circle(r) => 0.0, Rect(w, h) => w * h, }}; }}"
    );
    let e2 = errors_of(&bad_guard);
    assert!(e2.iter().any(|d| d.code == Some("E-GUARD-TYPE")), "{e2:?}");
}
