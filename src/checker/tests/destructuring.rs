//! Checker tests — let-destructuring (Phase 1 slice 5).

use super::support::*;

const POINT: &str = "class Point { constructor(public int x, public int y) {} } ";

fn code_present(src: &str, code: &str) -> bool {
    errors_of(src).iter().any(|e| e.code == Some(code))
}

#[test]
fn struct_destructure_binds_fields() {
    let src = format!(
        "{POINT} function f(Point p) -> int {{ var Point {{ x, y }} = p; return x + y; }} \
         function main() -> void {{}}"
    );
    assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
}

#[test]
fn struct_destructure_rename_binds_new_name() {
    let src = format!(
        "{POINT} function f(Point p) -> int {{ var Point {{ x: col, y: row }} = p; return col + row; }} \
         function main() -> void {{}}"
    );
    assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
}

#[test]
fn struct_destructure_else_is_irrefutable_error() {
    let src = format!(
        "{POINT} function f(Point p) -> int {{ var Point {{ x, y }} = p else {{ return 0; }} return x; }} \
         function main() -> void {{}}"
    );
    assert!(
        code_present(&src, "E-DESTRUCTURE-ELSE-IRREFUTABLE"),
        "{:?}",
        errors_of(&src)
    );
}

#[test]
fn struct_destructure_unknown_field_error() {
    let src = format!(
        "{POINT} function f(Point p) -> int {{ var Point {{ z }} = p; return z; }} \
         function main() -> void {{}}"
    );
    assert!(
        code_present(&src, "E-DESTRUCTURE-FIELD-UNKNOWN"),
        "{:?}",
        errors_of(&src)
    );
}

#[test]
fn struct_destructure_wrong_type_error() {
    let src = format!(
        "{POINT} function f(int n) -> int {{ var Point {{ x, y }} = n; return x; }} \
         function main() -> void {{}}"
    );
    assert!(
        code_present(&src, "E-DESTRUCTURE-TYPE"),
        "{:?}",
        errors_of(&src)
    );
}

#[test]
fn struct_destructure_non_class_head_error() {
    // `int` is not a class, so it cannot head a struct destructuring.
    let src =
        "function f(int n) -> int { var int { x } = n; return x; } function main() -> void {}";
    assert!(
        code_present(src, "E-DESTRUCTURE-NOT-CLASS"),
        "{:?}",
        errors_of(src)
    );
}

#[test]
fn list_destructure_with_else_ok() {
    let src =
        "function f(List<int> xs) -> int { var [a, b] = xs else { return -1; } return a + b; } \
               function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn list_destructure_without_else_needs_else() {
    let src = "function f(List<int> xs) -> int { var [a, b] = xs; return a; } \
               function main() -> void {}";
    assert!(
        code_present(src, "E-DESTRUCTURE-NEEDS-ELSE"),
        "{:?}",
        errors_of(src)
    );
}

#[test]
fn list_destructure_else_must_diverge() {
    // The `else` falls through (no return/throw/break) → E-DESTRUCTURE-ELSE-FALLTHROUGH.
    let src = "function f(List<int> xs) -> int { var [a, b] = xs else { var z = 1; } return a; } \
               function main() -> void {}";
    assert!(
        code_present(src, "E-DESTRUCTURE-ELSE-FALLTHROUGH"),
        "{:?}",
        errors_of(src)
    );
}

#[test]
fn fixed_list_destructure_is_irrefutable() {
    // A `[int; 2]` destructured with two binders is irrefutable — no `else`, no error.
    let src = "function main() -> void { [int; 2] pair = [1, 2]; var [a, b] = pair; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn fixed_list_destructure_wrong_arity_error() {
    let src = "function main() -> void { [int; 2] pair = [1, 2]; var [a, b, c] = pair; }";
    assert!(
        code_present(src, "E-FIXEDLIST-DESTRUCTURE-LEN"),
        "{:?}",
        errors_of(src)
    );
}

#[test]
fn fixed_list_destructure_else_is_irrefutable_error() {
    let src =
        "function main() -> void { [int; 2] pair = [1, 2]; var [a, b] = pair else { return; } }";
    assert!(
        code_present(src, "E-DESTRUCTURE-ELSE-IRREFUTABLE"),
        "{:?}",
        errors_of(src)
    );
}

#[test]
fn list_destructure_non_list_error() {
    let src = "function f(int n) -> int { var [a, b] = n else { return 0; } return a; } \
               function main() -> void {}";
    assert!(
        code_present(src, "E-DESTRUCTURE-NOT-LIST"),
        "{:?}",
        errors_of(src)
    );
}

#[test]
fn duplicate_binder_is_error() {
    let src = "function f(List<int> xs) -> int { var [a, a] = xs else { return 0; } return a; } \
               function main() -> void {}";
    assert!(
        code_present(src, "E-DESTRUCTURE-DUP-BIND"),
        "{:?}",
        errors_of(src)
    );
}

#[test]
fn destructured_int_is_an_arithmetic_operand() {
    // The operand trap: a struct-bound `int` must specialize on the VM (`x + 1`); the checker must
    // type it as `int`, not erase it.
    let src = format!(
        "{POINT} function f(Point p) -> int {{ var Point {{ x }} = p; return x + 1; }} \
         function main() -> void {{}}"
    );
    assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
}
