//! Checker tests — `const` class constants (Feature A).

use super::support::*;

/// Does `src` produce an error carrying `code`?
fn has(src: &str, code: &str) -> bool {
    errors_of(src).iter().any(|e| e.code == Some(code))
}

#[test]
fn const_declared_and_accessed_is_ok() {
    let src = "class Limits { const int MAX = 100; } \
               function main() -> void { var x = Limits.MAX; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn const_as_arithmetic_operand_is_ok() {
    let src = "class Limits { const int MAX = 100; } \
               function main() -> void { int y = Limits.MAX + 1; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn const_without_initializer_is_error() {
    assert!(has(
        "class C { const int MAX; } function main() -> void {}",
        "E-CONST-NO-INIT"
    ));
}

#[test]
fn const_with_nonliteral_initializer_is_error() {
    // A call is not a compile-time literal.
    let src = "function f() -> int { return 1; } \
               class C { const int MAX = f(); } function main() -> void {}";
    assert!(has(src, "E-CONST-NOT-LITERAL"));
}

#[test]
fn const_mutable_is_error() {
    assert!(has(
        "class C { const mutable int MAX = 1; } function main() -> void {}",
        "E-CONST-MUTABLE"
    ));
}

#[test]
fn const_init_type_mismatch_is_error() {
    assert!(has(
        "class C { const int MAX = \"x\"; } function main() -> void {}",
        "E-CONST-INIT-TYPE"
    ));
}

#[test]
fn const_name_must_be_screaming_snake() {
    assert!(has(
        "class C { const int maxVal = 1; } function main() -> void {}",
        "E-CONST-CASE"
    ));
}

#[test]
fn private_const_read_from_outside_is_error() {
    let src = "class C { private const int SECRET = 1; } \
               function main() -> void { var x = C.SECRET; }";
    assert!(has(src, "E-CONST-VISIBILITY"));
}

#[test]
fn private_const_read_inside_class_is_ok() {
    let src = "class C { private const int SECRET = 1; \
                          function get() -> int { return C.SECRET; } } \
               function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn const_read_through_instance_is_error() {
    let src = "class C { const int MAX = 1; constructor() {} } \
               function main() -> void { C c = C(); var x = c.MAX; }";
    assert!(has(src, "E-CONST-INSTANCE-ACCESS"));
}

#[test]
fn const_reassignment_is_error() {
    let src = "class C { const int MAX = 1; } \
               function main() -> void { C.MAX = 2; }";
    assert!(has(src, "E-CONST-REASSIGN"));
}

#[test]
fn inherited_const_via_subclass_name_is_ok() {
    let src = "open class Base { const int MAX = 100; } \
               class Sub extends Base {} \
               function main() -> void { var x = Sub.MAX; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn protected_const_read_from_subclass_is_ok() {
    let src = "open class Base { protected const int MAX = 100; } \
               class Sub extends Base { function get() -> int { return Sub.MAX; } } \
               function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn protected_const_read_from_outside_is_error() {
    let src = "open class Base { protected const int MAX = 100; } \
               function main() -> void { var x = Base.MAX; }";
    assert!(has(src, "E-CONST-VISIBILITY"));
}
