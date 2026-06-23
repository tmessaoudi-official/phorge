//! Checker tests — unions (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn union_catch_covers_each_member() {
    // `catch (BadInput | NotFound e)` discharges a call that throws `BadInput` (a member).
    let ok = errors_of(&format!(
        "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function main() {{ try {{ f(); }} catch (BadInput | NotFound e) {{}} }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn union_param_accepts_each_member() {
    let ok = errors_of(&format!(
        "{SHAPES} function f(Circle | Square s) {{}} \
             function main() {{ f(Circle(1)); f(Square(2)); }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn union_param_rejects_non_member() {
    let bad = errors_of(&format!(
        "{SHAPES} function f(Circle | Square s) {{}} \
             function main() {{ f(Triangle(3)); }}"
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
             function main() {{ int a = area(Circle(2)); }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn match_over_union_non_exhaustive_lists_missing() {
    let bad = errors_of(&format!(
        "{SHAPES} function area(Circle | Square s) -> int {{ \
               return match s {{ Circle c => c.radius }}; }} \
             function main() {{}}"
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
            "{SHAPES} enum Color {{ Red, Green }} function f(Circle | Color x) {{}} function main() {{}}"
        ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-UNION-MEMBER")),
        "{bad:?}"
    );
}

#[test]
fn union_arity_collapse_is_error() {
    let bad = errors_of(&format!(
        "{SHAPES} function f(Circle | Circle x) {{}} function main() {{}}"
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
               return match s {{ Circle c => c.radius, Nope n => 0 }}; }} function main() {{}}"
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
               if (s instanceof Circle) {{ return s.radius; }} return 0; }} function main() {{}}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn primitive_union_literal_match_ok() {
    let ok = errors_of(
        "function classify(int | string code) -> string { \
               return match code { 0 => \"zero\", \"ok\" => \"okay\", _ => \"other\" }; } \
             function main() { string s = classify(0); }",
    );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn primitive_union_accepts_int_and_string() {
    let ok = errors_of("function f(int | string x) {} function main() { f(1); f(\"a\"); }");
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn type_pattern_nested_in_variant_is_rejected() {
    // A type pattern is top-level-only; nesting it in a variant payload would diverge from the
    // transpiler (which emits only simple payload bindings), so the checker rejects it.
    let bad = errors_of(&format!(
        "{SHAPES} enum Wrap {{ One(Circle inner) }} \
             function f(Wrap w) -> int {{ return match w {{ One(Circle c) => c.radius }}; }} \
             function main() {{}}"
    ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-MATCH-TYPE")),
        "{bad:?}"
    );
}
