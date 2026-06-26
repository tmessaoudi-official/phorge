//! Checker tests — default parameter values (M4 default parameters, free-function-only v1).

use super::support::*;

#[test]
fn default_param_makes_arg_optional() {
    // A trailing defaulted param may be omitted OR supplied.
    let src = "function f(int x, int y = 10) -> int { return x + y; } \
               function main() -> void { int a = f(1); int b = f(1, 2); }";
    assert!(errors_of(src).is_empty(), "got {:?}", errors_of(src));
    // Two defaults; 1, 2, or 3 args all valid.
    let src2 = "function g(int a, int b = 1, int c = 2) -> int { return a + b + c; } \
                function main() -> void { int x = g(1); int y = g(1, 2); int z = g(1, 2, 3); }";
    assert!(errors_of(src2).is_empty(), "got {:?}", errors_of(src2));
}

#[test]
fn too_few_or_too_many_still_errors() {
    // Below the required arity (no default covers `x`) is an error.
    let lo = errors_of("function f(int x, int y = 1) -> int { return x; } function main() -> void { int a = f(); }");
    assert!(!lo.is_empty(), "f() should be too few args");
    // Above the param count is an error.
    let hi = errors_of("function f(int x, int y = 1) -> int { return x; } function main() -> void { int a = f(1, 2, 3); }");
    assert!(!hi.is_empty(), "f(1,2,3) should be too many args");
}

#[test]
fn default_must_be_trailing() {
    let e = errors_of("function f(int x = 1, int y) -> int { return x + y; }");
    assert!(
        e.iter().any(|d| d.code == Some("E-DEFAULT-PARAM-ORDER")),
        "got {e:?}"
    );
}

#[test]
fn default_must_be_literal() {
    // A non-literal default (a call) is rejected.
    let e = errors_of(
        "function side() -> int { return 1; } function f(int x = side()) -> int { return x; }",
    );
    assert!(
        e.iter().any(|d| d.code == Some("E-DEFAULT-PARAM-EXPR")),
        "got {e:?}"
    );
}

#[test]
fn default_type_must_match() {
    let e = errors_of("function f(int x = \"no\") -> int { return x; }");
    assert!(
        e.iter().any(|d| d.code == Some("E-DEFAULT-PARAM-TYPE")),
        "got {e:?}"
    );
    // A null default is allowed only for an optional parameter.
    assert!(errors_of("function f(int? x = null) -> void {}").is_empty());
    let e2 = errors_of("function f(int x = null) -> void {}");
    assert!(
        e2.iter().any(|d| d.code == Some("E-DEFAULT-PARAM-TYPE")),
        "got {e2:?}"
    );
}

#[test]
fn default_on_method_is_rejected_v1() {
    // Methods are a documented v1 deferral.
    let e = errors_of(
        "class C { constructor() {} function m(int x = 1) -> int { return x; } } \
         function main() -> void { C c = new C(); }",
    );
    assert!(
        e.iter().any(|d| d.code == Some("E-DEFAULT-PARAM-CONTEXT")),
        "got {e:?}"
    );
}
