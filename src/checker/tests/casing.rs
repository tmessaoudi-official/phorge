//! Checker tests — casing (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn package_is_mandatory_and_core_is_reserved() {
    // M5 S1: every file is packaged, never inferred. No declaration → E-NO-PACKAGE.
    let e = errors_of_raw("function main() {}");
    assert!(
        e.iter().any(|d| d.code == Some("E-NO-PACKAGE")),
        "got {e:?}"
    );
    // The `Core` root is reserved for the standard library → E-RESERVED-PACKAGE.
    let e2 = errors_of_raw("package Core; function main() {}");
    assert!(
        e2.iter().any(|d| d.code == Some("E-RESERVED-PACKAGE")),
        "got {e2:?}"
    );
    let e3 = errors_of_raw("package Core.Evil; function main() {}");
    assert!(
        e3.iter().any(|d| d.code == Some("E-RESERVED-PACKAGE")),
        "got {e3:?}"
    );
    // A well-formed user package (and the reserved `Main`) type-check cleanly.
    assert!(check(&prog_raw("package App.Util; function main() {}")).is_ok());
    assert!(check(&prog_raw("package Main; function main() {}")).is_ok());
}

#[test]
fn package_and_import_segments_must_be_pascalcase() {
    // Reshape slice 2b: a lowercase package segment is rejected (E-PKG-CASE).
    let e = errors_of_raw("package app.util; function main() {}");
    assert!(e.iter().any(|d| d.code == Some("E-PKG-CASE")), "got {e:?}");
    // Each non-PascalCase segment is flagged; a single-segment lowercase package too.
    let e2 = errors_of_raw("package acme; function main() {}");
    assert!(
        e2.iter().any(|d| d.code == Some("E-PKG-CASE")),
        "got {e2:?}"
    );
    // A lowercase import path segment is rejected.
    let e3 = errors_of_raw("package Main; import acme.util; function main() { int x = util.f(); }");
    assert!(
        e3.iter().any(|d| d.code == Some("E-PKG-CASE")),
        "got {e3:?}"
    );
    // A lowercase import alias is rejected (it occupies a leaf position).
    let e4 = errors_of_raw(
        "package Main; import Acme.Util as util; function main() { int x = util.f(); }",
    );
    assert!(
        e4.iter().any(|d| d.code == Some("E-PKG-CASE")),
        "got {e4:?}"
    );
    // PascalCase package + import + alias type-check cleanly (no E-PKG-CASE noise).
    let ok = errors_of_raw("package App.Util; function main() {}");
    assert!(
        !ok.iter().any(|d| d.code == Some("E-PKG-CASE")),
        "got {ok:?}"
    );
}

#[test]
fn snake_case_function_is_rejected() {
    // A function name with `_` is not camelCase → E-NAME-CASE, with a converted-form hint.
    let errs = errors_of("function c_to_f(int c) -> int { return c; } function main() {}");
    let d = errs
        .iter()
        .find(|d| d.code == Some("E-NAME-CASE"))
        .unwrap_or_else(|| panic!("expected E-NAME-CASE, got {errs:?}"));
    assert!(
        d.hint.as_deref().unwrap_or("").contains("cToF"),
        "hint: {:?}",
        d.hint
    );
}

#[test]
fn snake_case_var_binding_is_rejected() {
    // A `var`/typed local binding with `_` is a value identifier → E-NAME-CASE.
    let errs = errors_of("function main() { int my_count = 0; }");
    assert!(
        errs.iter().any(|d| d.code == Some("E-NAME-CASE")),
        "got {errs:?}"
    );
}

#[test]
fn non_pascal_type_enum_variant_is_rejected() {
    // class name, enum name, and a variant name that are not PascalCase → E-TYPE-CASE.
    let cls = errors_of("class box {} function main() {}");
    assert!(
        cls.iter().any(|d| d.code == Some("E-TYPE-CASE")),
        "class: {cls:?}"
    );
    let en = errors_of("enum color { red() } function main() {}");
    // both the enum name `color` and the variant `red` violate PascalCase.
    assert!(
        en.iter().filter(|d| d.code == Some("E-TYPE-CASE")).count() >= 2,
        "enum: {en:?}"
    );
    let alias = errors_of("type myInt = int; function main() {}");
    assert!(
        alias.iter().any(|d| d.code == Some("E-TYPE-CASE")),
        "alias: {alias:?}"
    );
}

#[test]
fn conformant_casing_is_clean() {
    // camelCase fns/params/vars + PascalCase types/enums/variants type-check with no casing error.
    let src = "enum Shape { Circle(float r) } \
                   class Box { constructor(private int width) {} function widthOf() -> int { return width; } } \
                   function areaOf(Shape s) -> int { int localCount = 0; return localCount; } \
                   function main() {}";
    let errs = errors_of(src);
    assert!(
        !errs
            .iter()
            .any(|d| d.code == Some("E-NAME-CASE") || d.code == Some("E-TYPE-CASE")),
        "expected no casing errors, got {errs:?}"
    );
}

#[test]
fn case_converters() {
    assert!(is_camel("main") && is_camel("splitOnce") && !is_camel("split_once"));
    assert!(is_pascal("Shape") && !is_pascal("shape") && !is_pascal("Http_Request"));
    assert_eq!(to_camel("split_once"), "splitOnce");
    assert_eq!(to_camel("c_to_f"), "cToF");
    assert_eq!(to_pascal("shape"), "Shape");
    assert_eq!(to_pascal("http_request"), "HttpRequest");
}
