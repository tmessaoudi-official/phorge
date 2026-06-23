//! Checker tests — types (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn empty_program_checks_ok() {
    assert!(errors_of("").is_empty());
}

#[test]
fn var_infers_init_type_and_catches_later_misuse() {
    // `var x = 5` infers int; using it where a string is required is then a type error.
    let errs = errors_of("function main() { var x = 5; string y = x; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("expected `string`, found `int`")),
        "{errs:?}"
    );
}

#[test]
fn var_infers_and_well_typed_use_is_clean() {
    assert!(errors_of("function main() { var x = 5; int y = x; }").is_empty());
}

#[test]
fn type_alias_resolves_and_alias_of_alias_works() {
    // `B` -> `A` -> `int`: a param/return typed `B` checks exactly like `int`.
    let errs = errors_of(
        "type A = int; type B = A; function f(B x) -> B { return x + 1; } function main() {}",
    );
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn type_alias_cycle_is_an_error() {
    let errs = errors_of("type A = B; type B = A; function f(A x) {} function main() {}");
    assert!(errs.iter().any(|e| e.message.contains("cycle")), "{errs:?}");
}

#[test]
fn duplicate_type_name_is_an_error() {
    let errs = errors_of("type A = int; type A = float; function main() {}");
    assert!(
        errs.iter().any(|e| e.message.contains("duplicate")),
        "{errs:?}"
    );
}

#[test]
fn unknown_type_carries_a_code() {
    let errs = errors_of("function main() { Nope n = 0; }");
    let d = errs
        .iter()
        .find(|e| e.message.contains("unknown type"))
        .expect("an unknown-type error");
    assert_eq!(d.code, Some("E-UNKNOWN-TYPE"));
}

#[test]
fn expand_aliases_dealiases_the_program_for_backends() {
    // After expansion the backends must see no alias names: `B`/`A` collapse to `int`.
    let p = prog("type A = int; type B = A; function f(B x) -> B { return x; } function main() {}");
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
    let errs = errors_of("function main() { Nope n = 0; }");
    assert!(
        errs.iter().any(|e| e.message.contains("unknown type")),
        "{errs:?}"
    );
}

#[test]
fn decimal_type_is_deferred_corner() {
    let errs = errors_of("function main() { decimal d = 0; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("decimal") && e.message.contains("not yet supported")),
        "{errs:?}"
    );
}

#[test]
fn var_decl_type_mismatch_errors() {
    let errs = errors_of("function main() { int n = true; }");
    assert!(
        errs.iter().any(|e| e.message.contains("expected `int`")),
        "{errs:?}"
    );
}

#[test]
fn good_var_decl_and_arithmetic_ok() {
    assert!(errors_of("function main() { int a = 1; int b = a + 2; }").is_empty());
}
