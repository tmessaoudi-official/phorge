//! Checker tests — collections (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn map_literal_and_indexing_typecheck() {
    // A well-typed map literal + index of the right key type checks clean.
    let ok = errors_of(
        "function main() -> void { Map<string, int> m = [\"a\" => 1]; int x = m[\"a\"]; }",
    );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
    // Indexing with the wrong key type is an error.
    let bad =
        errors_of("function main() -> void { Map<string, int> m = [\"a\" => 1]; int x = m[0]; }");
    assert!(
        bad.iter().any(|d| d.message.contains("map index must be")),
        "got {bad:?}"
    );
}

#[test]
fn map_key_must_be_hashable() {
    // A `float` key is not hashable → E-MAP-KEY.
    let e = errors_of("function main() -> void { Map<float, int> m = [1.0 => 1]; }");
    assert!(e.iter().any(|d| d.code == Some("E-MAP-KEY")), "got {e:?}");
}

#[test]
fn list_literal_unifies_elements() {
    let src = format!(
        "{SHAPE} function main() -> void {{ List<Shape> xs = [new Circle(1.0), new Rect(2.0, 3.0)]; }}"
    );
    assert!(errors_of(&src).is_empty());
}

#[test]
fn list_literal_mixed_elements_error() {
    let errs = errors_of("function main() -> void { List<int> xs = [1, true]; }");
    assert!(
        errs.iter().any(|e| e.message.contains("list elements")),
        "{errs:?}"
    );
}

#[test]
fn for_in_binds_element_type() {
    let src = format!(
            "{SHAPE} function area(Shape s) -> float {{ return 0.0; }} \
             function main() -> void {{ List<Shape> xs = [new Circle(1.0)]; for (Shape s in xs) {{ float a = area(s); }} }}"
        );
    assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
}

#[test]
fn for_in_requires_list() {
    let errs = errors_of("function main() -> void { for (int i in 5) { } }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("`for`-`in` requires a List")),
        "{errs:?}"
    );
}

#[test]
fn range_in_for_checks_clean_and_binds_int() {
    assert!(
        errors_of("function main() -> void { for (int i in 0..5) { int x = i + 1; } }").is_empty()
    );
    assert!(errors_of("function main() -> void { for (int i in 0..=5) { } }").is_empty());
    // a range bound to a local is `List<int>`
    assert!(errors_of("function main() -> void { List<int> xs = 0..3; }").is_empty());
}

#[test]
fn range_non_int_bound_is_error() {
    let errs = errors_of("function main() -> void { for (int i in 0..3.0) { } }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("range bounds must be `int`")
                && e.code == Some("E-RANGE-TYPE")),
        "{errs:?}"
    );
}

#[test]
fn list_indexing_yields_element() {
    assert!(
        errors_of("function main() -> void { List<int> xs = [1, 2]; int y = xs[0]; }").is_empty()
    );
}
