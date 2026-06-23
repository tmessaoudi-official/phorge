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
