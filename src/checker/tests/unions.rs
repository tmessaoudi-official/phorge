//! Checker tests — unions (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn union_catch_covers_each_member() {
    // `catch (BadInput | NotFound e)` discharges a call that throws `BadInput` (a member).
    let ok = errors_of(&format!(
        "{ERRDEF} function f() -> void throws BadInput {{ throw new BadInput(\"x\"); }} \
             function main() -> void {{ try {{ f(); }} catch (BadInput | NotFound e) {{}} }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn union_param_accepts_each_member() {
    let ok = errors_of(&format!(
        "{SHAPES} function f(Circle | Square s) -> void {{}} \
             function main() -> void {{ f(new Circle(1)); f(new Square(2)); }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn union_param_rejects_non_member() {
    let bad = errors_of(&format!(
        "{SHAPES} function f(Circle | Square s) -> void {{}} \
             function main() -> void {{ f(Triangle(3)); }}"
    ));
    assert!(
        !bad.is_empty(),
        "expected a type error passing a non-member"
    );
}

#[test]
fn match_over_union_exhaustive_ok() {
    let ok = errors_of(&format!(
        "{SHAPES} function area(Circle | Square s) -> int {{ \
               return match s {{ Circle c => c.radius, Square sq => sq.side }}; }} \
             function main() -> void {{ int a = area(new Circle(2)); }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn match_over_union_non_exhaustive_lists_missing() {
    let bad = errors_of(&format!(
        "{SHAPES} function area(Circle | Square s) -> int {{ \
               return match s {{ Circle c => c.radius }}; }} \
             function main() -> void {{}}"
    ));
    assert!(
        bad.iter()
            .any(|e| e.message.contains("non-exhaustive") && e.message.contains("Square")),
        "{bad:?}"
    );
}

#[test]
fn union_rejects_enum_member() {
    let bad = errors_of(&format!(
            "{SHAPES} enum Color {{ Red, Green }} function f(Circle | Color x) -> void {{}} function main() -> void {{}}"
        ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-UNION-MEMBER")),
        "{bad:?}"
    );
}

#[test]
fn union_arity_collapse_is_error() {
    let bad = errors_of(&format!(
        "{SHAPES} function f(Circle | Circle x) -> void {{}} function main() -> void {{}}"
    ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-UNION-ARITY")),
        "{bad:?}"
    );
}

#[test]
fn type_pattern_must_name_a_class_or_interface() {
    let bad = errors_of(&format!(
        "{SHAPES} function f(Circle | Square s) -> int {{ \
               return match s {{ Circle c => c.radius, Nope n => 0 }}; }} function main() -> void {{}}"
    ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-MATCH-TYPE")),
        "{bad:?}"
    );
}

#[test]
fn instanceof_narrows_a_union_operand() {
    let ok = errors_of(&format!(
        "{SHAPES} function f(Circle | Square s) -> int {{ \
               if (s instanceof Circle) {{ return s.radius; }} return 0; }} function main() -> void {{}}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn primitive_union_literal_match_ok() {
    let ok = errors_of(
        "function classify(int | string code) -> string { \
               return match code { 0 => \"zero\", \"ok\" => \"okay\", _ => \"other\" }; } \
             function main() -> void { string s = classify(0); }",
    );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn primitive_union_accepts_int_and_string() {
    let ok = errors_of(
        "function f(int | string x) -> void {} function main() -> void { f(1); f(\"a\"); }",
    );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn type_pattern_nested_in_variant_is_accepted() {
    // S5.2-T2: a type pattern nested in a variant payload is now allowed (every backend recurses
    // variant fields). It is refutable, so it does not discharge the variant's coverage — an
    // irrefutable fallback (here a bare `One(other)`) is required for exhaustiveness.
    let ok = errors_of(&format!(
        "{SHAPES} enum Wrap {{ One(Circle inner) }} \
             function f(Wrap w) -> int {{ return match w {{ One(Circle c) => c.radius, One(o) => 0 }}; }} \
             function main() -> void {{}}"
    ));
    assert!(
        !ok.iter().any(|e| e.code == Some("E-MATCH-TYPE")),
        "no longer rejected: {ok:?}"
    );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");

    // Without the fallback the refutable arm leaves `One` undischarged — non-exhaustive.
    let bad = errors_of(&format!(
        "{SHAPES} enum Wrap {{ One(Circle inner) }} \
             function f(Wrap w) -> int {{ return match w {{ One(Circle c) => c.radius }}; }} \
             function main() -> void {{}}"
    ));
    assert!(
        bad.iter().any(|e| e.message.contains("non-exhaustive")),
        "{bad:?}"
    );
}
