//! Checker tests — calls (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn function_call_arity_and_type_checked() {
    assert!(errors_of(
        "function inc(int n) -> int { return n + 1; } function main() -> void { int x = inc(1); }"
    )
    .is_empty());
    let bad_arity = errors_of(
        "function inc(int n) -> int { return n; } function main() -> void { int x = inc(1, 2); }",
    );
    assert!(
        bad_arity
            .iter()
            .any(|e| e.message.contains("expects 1 argument")),
        "{bad_arity:?}"
    );
    let bad_type = errors_of(
        "function inc(int n) -> int { return n; } function main() -> void { int x = inc(true); }",
    );
    assert!(
        bad_type.iter().any(|e| e.message.contains("argument 1")),
        "{bad_type:?}"
    );
}

#[test]
fn unknown_function_call_errors() {
    let errs = errors_of("function main() -> void { nope(); }");
    assert!(
        errs.iter().any(|e| e.message.contains("unknown function")),
        "{errs:?}"
    );
}

#[test]
fn println_accepts_string() {
    assert!(errors_of(
        r#"import Core.Console;
function main() -> void { Console.println("hi"); }"#
    )
    .is_empty());
}

#[test]
fn console_println_rejects_non_string() {
    // The native's signature is `(string)`, so an `int` argument is a type error (M3 Wave 1).
    let errs = errors_of(
        r#"import Core.Console;
function main() -> void { Console.println(42); }"#,
    );
    assert!(
        errs.iter().any(|e| e.message.contains("Console.println")),
        "{errs:?}"
    );
}

#[test]
fn bare_println_is_unknown_function() {
    // The global `println` is retired: a bare call now resolves as an unknown free function.
    let errs = errors_of(r#"function main() -> void { println("hi"); }"#);
    assert!(
        errs.iter()
            .any(|e| e.message.contains("unknown function") && e.message.contains("println")),
        "{errs:?}"
    );
}

#[test]
fn console_println_without_import_errors() {
    // "nothing in the wind": without `import Core.Console;`, the qualifier is unbound, so the
    // member call cannot resolve to the native and is an error.
    let errs = errors_of(r#"function main() -> void { Console.println("hi"); }"#);
    assert!(!errs.is_empty(), "expected an error without the import");
}

#[test]
fn local_shadowing_imported_qualifier_errors() {
    // A value binding may not shadow an imported module qualifier (keeps all backends
    // consistent — see `declare`). Coded `E-SHADOW-IMPORT`. (Stdlib qualifiers are now
    // PascalCase, so a camelCase local can never collide with one — the guard still bites a
    // lowercase user-package leaf, which is what this exercises.)
    let errs = errors_of(
        r#"import acme.helper;
function main() -> void { int helper = 0; int x = helper; }"#,
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-SHADOW-IMPORT")),
        "{errs:?}"
    );
}

#[test]
fn html_literal_bad_hole_is_coded() {
    // A hole whose type is neither Html, string, nor a primitive is `E-HTML-HOLE` (Core.Html
    // Wave 3): there is no safe HTML rendering for an enum value.
    let errs = errors_of(
        r#"import Core.Html;
enum E { A() }
function main() -> void { var p = html"<h1>{A()}</h1>"; }"#,
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-HTML-HOLE")),
        "{errs:?}"
    );
}

#[test]
fn html_literal_without_import_is_coded() {
    // `html"…"` desugars to Core.Html kernel calls, so the module must be imported; otherwise
    // `E-HTML-IMPORT`.
    let errs = errors_of(r#"function main() -> void { var p = html"<h1>x</h1>"; }"#);
    assert!(
        errs.iter().any(|e| e.code == Some("E-HTML-IMPORT")),
        "{errs:?}"
    );
}

#[test]
fn local_shadowing_function_name_errors() {
    // A value binding may not shadow a top-level function name: a bare `f(…)` call dispatches
    // functions-first in the run backends but locals-first in the transpiler, so an overlap is
    // a silent four-backend divergence (made reachable once functions became first-class values
    // in M3 S3). Coded `E-SHADOW-FN`. See `declare`.
    let errs = errors_of(
        r#"function dbl(int x) -> int { return x * 2; }
function main() -> void { var dbl = fn(int x) => x + 1000; }"#,
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-SHADOW-FN")),
        "{errs:?}"
    );
}

#[test]
fn variant_constructor_returns_enum() {
    let src = format!("{SHAPE} function main() -> void {{ Shape s = Circle(2.0); }}");
    assert!(errors_of(&src).is_empty());
}

#[test]
fn variant_constructor_arg_type_checked() {
    let src = format!("{SHAPE} function main() -> void {{ Shape s = Circle(true); }}");
    let errs = errors_of(&src);
    assert!(
        errs.iter().any(|e| e.message.contains("argument 1")),
        "{errs:?}"
    );
}

#[test]
fn constructor_call_and_method_call_ok() {
    let src = format!(
        "{GREETER} function main() -> void {{ Greeter g = Greeter(\"Tak\"); string s = g.greet(); }}"
    );
    assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
}

#[test]
fn constructor_arg_type_checked() {
    let src = format!("{GREETER} function main() -> void {{ Greeter g = Greeter(123); }}");
    let errs = errors_of(&src);
    assert!(
        errs.iter().any(|e| e.message.contains("argument 1")),
        "{errs:?}"
    );
}

#[test]
fn unknown_method_errors() {
    let src =
        format!("{GREETER} function main() -> void {{ Greeter g = Greeter(\"x\"); g.missing(); }}");
    let errs = errors_of(&src);
    assert!(
        errs.iter()
            .any(|e| e.message.contains("no method `missing`")),
        "{errs:?}"
    );
}

#[test]
fn field_access_typed() {
    let src = "class Box { public int n; constructor(int n) {} } function main() -> void { Box b = Box(1); int x = b.n; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn bare_field_visible_in_method() {
    let src = "class C { private string name; constructor(string name) {} function who() -> string { return name; } }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn this_outside_method_errors() {
    let errs = errors_of("function main() -> void { string s = this; }");
    assert!(
        errs.iter().any(|e| e.message.contains("`this`")),
        "{errs:?}"
    );
}

#[test]
fn interpolation_allows_primitives() {
    assert!(
        errors_of("function main() -> void { float x = 1.5; string s = \"v = {x}\"; }").is_empty()
    );
    assert!(errors_of("function main() -> void { int n = 3; string s = \"n = {n}\"; }").is_empty());
}

#[test]
fn interpolation_rejects_objects() {
    let src = "class C { private int n; constructor(int n) {} } function main() -> void { C c = C(1); string s = \"{c}\"; }";
    let errs = errors_of(src);
    assert!(
        errs.iter()
            .any(|e| e.message.contains("cannot be interpolated")),
        "{errs:?}"
    );
}

#[test]
fn promoted_ctor_param_is_field() {
    // Constructor promotion alone (no explicit `private int total;`) must type-check:
    // the promoted param becomes an instance field, matching the evaluator (EV-4).
    let errs = errors_of(
        "class C { constructor(private int total) {} \
               function add(int n) -> int { return total + n; } }",
    );
    assert!(errs.is_empty(), "promoted field should resolve: {errs:?}");
}

#[test]
fn explicit_field_decl_wins_over_promotion_type() {
    // Explicit field decl is authoritative regardless of member order; a promoted
    // param of the same name does not override its declared type.
    let errs = errors_of(
        "class C { private int total; constructor(private int total) {} \
               function add(int n) -> int { return total + n; } }",
    );
    assert!(
        errs.is_empty(),
        "redundant explicit+promoted (matching type) is fine: {errs:?}"
    );
}

#[test]
fn unmodified_ctor_param_is_not_a_field() {
    // A plain ctor param (no visibility modifier) is NOT promoted, so referencing it
    // bare in a method is still an unknown identifier — matches the evaluator.
    let errs = errors_of(
        "class C { constructor(int total) {} \
               function add(int n) -> int { return total + n; } }",
    );
    assert!(
        errs.iter()
            .any(|e| e.message.contains("unknown identifier")),
        "{errs:?}"
    );
}

#[test]
fn function_typed_binding_rejects_non_function() {
    // (int) -> int f = 5;  -> int not assignable to a function type
    let errs = errors_of("function main() -> void { (int) -> int f = 5; }");
    assert!(
        errs.iter().any(|e| e.message.contains("(int) -> int")),
        "{errs:?}"
    );
}
