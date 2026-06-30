//! Checker tests — types (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn empty_program_checks_ok() {
    assert!(errors_of("").is_empty());
}

#[test]
fn var_infers_init_type_and_catches_later_misuse() {
    // `var x = 5` infers int; using it where a string is required is then a type error.
    let errs = errors_of("function main() -> void { var x = 5; string y = x; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("expected `string`, found `int`")),
        "{errs:?}"
    );
}

#[test]
fn var_infers_and_well_typed_use_is_clean() {
    assert!(errors_of("function main() -> void { var x = 5; int y = x; }").is_empty());
}

#[test]
fn type_alias_resolves_and_alias_of_alias_works() {
    // `B` -> `A` -> `int`: a param/return typed `B` checks exactly like `int`.
    let errs = errors_of(
        "type A = int; type B = A; function f(B x) -> B { return x + 1; } function main() -> void {}",
    );
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn type_alias_cycle_is_an_error() {
    let errs =
        errors_of("type A = B; type B = A; function f(A x) -> void {} function main() -> void {}");
    assert!(errs.iter().any(|e| e.message.contains("cycle")), "{errs:?}");
}

#[test]
fn duplicate_type_name_is_an_error() {
    let errs = errors_of("type A = int; type A = float; function main() -> void {}");
    assert!(
        errs.iter().any(|e| e.message.contains("duplicate")),
        "{errs:?}"
    );
}

#[test]
fn unknown_type_carries_a_code() {
    let errs = errors_of("function main() -> void { Nope n = 0; }");
    let d = errs
        .iter()
        .find(|e| e.message.contains("unknown type"))
        .expect("an unknown-type error");
    assert_eq!(d.code, Some("E-UNKNOWN-TYPE"));
}

#[test]
fn expand_aliases_dealiases_the_program_for_backends() {
    // After expansion the backends must see no alias names: `B`/`A` collapse to `int`.
    let p = prog(
        "type A = int; type B = A; function f(B x) -> B { return x; } function main() -> void {}",
    );
    let e = expand_aliases(&p);
    // no TypeAlias items survive
    assert!(
        !e.items
            .iter()
            .any(|it| matches!(it, crate::ast::Item::TypeAlias { .. })),
        "alias items leaked"
    );
    // f's param + return are now `int`
    if let crate::ast::Item::Function(f) = e
        .items
        .iter()
        .find(|it| matches!(it, crate::ast::Item::Function(_)))
        .unwrap()
    {
        assert!(
            matches!(&f.params[0].ty, crate::ast::Type::Named { name, .. } if name == "int"),
            "param not de-aliased: {:?}",
            f.params[0].ty
        );
        assert!(
            matches!(&f.ret, Some(crate::ast::Type::Named { name, .. }) if name == "int"),
            "return not de-aliased: {:?}",
            f.ret
        );
    } else {
        panic!("no function item");
    }
}

#[test]
fn resolve_maps_primitives_and_list() {
    use crate::ast::Type;
    use crate::token::Span;
    let sp = Span {
        start: 0,
        len: 1,
        line: 1,
        col: 1,
    };
    let mut c = Checker::new();
    assert_eq!(
        c.resolve_type(&Type::Named {
            name: "int".into(),
            args: vec![],
            span: sp
        }),
        Ty::Int
    );
    let list = Type::Named {
        name: "List".into(),
        args: vec![Type::Named {
            name: "int".into(),
            args: vec![],
            span: sp,
        }],
        span: sp,
    };
    assert_eq!(c.resolve_type(&list), Ty::List(Box::new(Ty::Int)));
    assert_eq!(c.errors.len(), 0);
}

#[test]
fn unknown_type_in_var_decl_errors() {
    let errs = errors_of("function main() -> void { Nope n = 0; }");
    assert!(
        errs.iter().any(|e| e.message.contains("unknown type")),
        "{errs:?}"
    );
}

#[test]
fn decimal_type_is_a_real_primitive() {
    // M-NUM S1: `decimal` is now a first-class primitive (no longer the deferred-corner stub). A
    // decimal literal flows into a decimal slot; a bare `int` does NOT (the int-widen is operator-
    // level only, never assignment-level — keeping the type wall honest).
    assert!(errors_of("function main() -> void { decimal d = 19.99d; }").is_empty());
    let errs = errors_of("function main() -> void { decimal d = 0; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("expected `decimal`")),
        "{errs:?}"
    );
}

#[test]
fn var_decl_type_mismatch_errors() {
    let errs = errors_of("function main() -> void { int n = true; }");
    assert!(
        errs.iter().any(|e| e.message.contains("expected `int`")),
        "{errs:?}"
    );
}

#[test]
fn good_var_decl_and_arithmetic_ok() {
    assert!(errors_of("function main() -> void { int a = 1; int b = a + 2; }").is_empty());
}

// --- S0a: the two-type nothing model (`void` + `empty`) ---

#[test]
fn capturing_a_void_value_is_an_error() {
    // `noop()` is `-> void` (no value); binding it is E-VOID-CAPTURE.
    let errs = errors_of("function noop() -> void {} function main() -> void { var x = noop(); }");
    assert!(
        errs.iter().any(|e| e.code == Some("E-VOID-CAPTURE")),
        "{errs:?}"
    );
}

#[test]
fn explicit_void_annotation_on_a_binding_is_also_a_capture_error() {
    // `void` is return-only — even an explicit `void x = …` is uncapturable.
    let errs = errors_of("function noop() -> void {} function main() -> void { void x = noop(); }");
    assert!(
        errs.iter().any(|e| e.code == Some("E-VOID-CAPTURE")),
        "{errs:?}"
    );
}

#[test]
fn holding_a_void_value_as_empty_is_the_escape_hatch() {
    // `empty x = noop();` is the explicit way to hold the empty value — `void <: empty`.
    assert!(
        errors_of("function noop() -> void {} function main() -> void { empty x = noop(); }")
            .is_empty()
    );
}

#[test]
fn empty_returning_function_may_be_captured() {
    assert!(errors_of(
        "function nothing() -> empty {} function main() -> void { empty x = nothing(); }"
    )
    .is_empty());
}

#[test]
fn empty_returning_function_need_not_return_on_all_paths() {
    // `empty` is value-less like `void`: falling off the end is fine (no E-MISSING-RETURN).
    let errs = errors_of("function nothing() -> empty {} function main() -> void {}");
    assert!(
        !errs.iter().any(|e| e.code == Some("E-MISSING-RETURN")),
        "{errs:?}"
    );
}

#[test]
fn void_returning_callback_flows_into_an_empty_slot() {
    // The widening edge `void <: empty`: a void value is assignable to an `empty` binding.
    assert!(
        errors_of("function fx() -> void {} function main() -> void { empty e = fx(); }")
            .is_empty()
    );
}

// --- S0b: mandatory return types (E-MISSING-RETURN-TYPE) ---

#[test]
fn function_without_a_return_type_is_an_error() {
    let errs = errors_of("function f() { } function main() -> void {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-MISSING-RETURN-TYPE")),
        "{errs:?}"
    );
}

#[test]
fn main_without_a_return_type_is_an_error() {
    // Even `main` must be annotated (the developer's explicit ask).
    let errs = errors_of("function main() {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-MISSING-RETURN-TYPE")),
        "{errs:?}"
    );
}

#[test]
fn method_without_a_return_type_is_an_error() {
    let errs = errors_of("class C { function m() { } } function main() -> void {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-MISSING-RETURN-TYPE")),
        "{errs:?}"
    );
}

#[test]
fn interface_method_without_a_return_type_is_an_error() {
    let errs = errors_of("interface I { function m(); } function main() -> void {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-MISSING-RETURN-TYPE")),
        "{errs:?}"
    );
}

#[test]
fn annotated_functions_are_clean() {
    assert!(errors_of(
        "function f() -> void {} function g() -> int { return 1; } function main() -> void {}"
    )
    .is_empty());
}

#[test]
fn constructors_and_hooks_are_exempt() {
    // Constructors have no return slot; property hooks are typed by the property — neither needs
    // an annotation. (Expression-body lambdas infer and are tested with the closures slice.)
    assert!(errors_of(
        "class C { constructor(public int x) {} int doubled { get => this.x * 2; } } \
             function main() -> void { C c = new C(2); int d = c.doubled; }"
    )
    .is_empty());
}

#[test]
fn expression_lambda_return_annotation_matches_is_ok() {
    // Option 1: an expression-body lambda may carry an explicit `: T` return type
    // (`function(int x): int => …`). When the inferred body type is assignable to the annotation, the
    // lambda type-checks cleanly — and the lambda's type is the *declared* return type.
    assert!(
        errors_of("function main() -> void { var f = function(int x): int => x + 1; }").is_empty()
    );
}

#[test]
fn expression_lambda_return_annotation_mismatch_errors() {
    // The annotation is enforced: a body whose inferred type is not assignable to the declared
    // return type is a type error (`expected `int`, found `string``).
    let errs = errors_of("function main() -> void { var f = function(int x): int => \"no\"; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("expected `int`") && e.message.contains("found `string`")),
        "{errs:?}"
    );
}
