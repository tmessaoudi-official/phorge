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
    let src = format!("{SHAPE} function main() -> void {{ Shape s = new Circle(2.0); }}");
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
        "{GREETER} function main() -> void {{ Greeter g = new Greeter(\"Tak\"); string s = g.greet(); }}"
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
    let src = "class Box { constructor(public int n) {} } function main() -> void { Box b = new Box(1); int x = b.n; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn field_accessible_in_method_via_this() {
    // A field is read through `this.` (Phorge requires `this.field` everywhere, like PHP `$this->`).
    let src =
        "class C { constructor(private string name) {} function who() -> string { return this.name; } }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn bare_field_access_is_rejected() {
    // Bare field access (no `this.`) is `E-BARE-FIELD` (2026-06-27, PHP-faithful — PHP has no bare
    // field access). The same code with `this.name` type-checks (see above).
    let src =
        "class C { constructor(private string name) {} function who() -> string { return name; } }";
    let errs = errors_of(src);
    assert!(
        errs.iter().any(|e| e.code == Some("E-BARE-FIELD")),
        "bare field must be E-BARE-FIELD: {errs:?}"
    );
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
               function add(int n) -> int { return this.total + n; } }",
    );
    assert!(errs.is_empty(), "promoted field should resolve: {errs:?}");
}

#[test]
fn explicit_field_decl_wins_over_promotion_type() {
    // Explicit field decl is authoritative regardless of member order; a promoted
    // param of the same name does not override its declared type.
    let errs = errors_of(
        "class C { private int total; constructor(private int total) {} \
               function add(int n) -> int { return this.total + n; } }",
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

// ---- UFCS (Slice 6): `x.f(a)` ≡ `f(x, a)`, method-first --------------------------------------

#[test]
fn ufcs_free_function_fallback() {
    // `n.triple()` with no `int` method resolves to the free function `triple(int)`.
    assert!(errors_of(
        "function triple(int x) -> int { return x * 3; } \
         function main() -> void { var n = 7; int t = n.triple(); }"
    )
    .is_empty());
}

#[test]
fn ufcs_native_fallback_requires_import() {
    // `xs.length()` ≡ `List.length(xs)` once `Core.List` is imported.
    assert!(errors_of(
        "import Core.List; function main() -> void { var xs = [1, 2, 3]; int n = xs.length(); }"
    )
    .is_empty());
    // Without the import there is no candidate, so it stays the original "no method" error.
    let errs = errors_of("function main() -> void { var xs = [1, 2, 3]; int n = xs.length(); }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("no method `length`")),
        "{errs:?}"
    );
}

#[test]
fn ufcs_method_first_beats_free_function() {
    // A real method wins over a same-named free function. The method returns `int`, the free
    // function `string`; assigning the call to an `int` succeeds ONLY if the method was chosen.
    assert!(errors_of(
        "class Box { constructor(public int v) {} function tag() -> int { return 1; } } \
         function tag(Box b) -> string { return \"x\"; } \
         function main() -> void { var b = new Box(5); int t = b.tag(); }"
    )
    .is_empty());
}

#[test]
fn ufcs_chaining_native_pipeline() {
    // `xs.filter(p).map(g)` chains: each step is a native UFCS over the previous result.
    assert!(errors_of(
        "import Core.List; \
         function main() -> void { \
            var xs = [1, 2, 3, 4]; \
            var ys = xs.filter(fn(int x) => x > 1).map(fn(int x) => x * x); \
            int n = ys.length(); }"
    )
    .is_empty());
}

#[test]
fn ufcs_unresolved_still_errors() {
    // No method, no free function, no imported native named `frobnicate` → original error stands.
    let errs = errors_of("function main() -> void { var n = 7; n.frobnicate(); }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("no method `frobnicate`")),
        "{errs:?}"
    );
}

#[test]
fn ufcs_string_native() {
    // `s.upper()` ≡ `Text.upper(s)`; `s.repeat(n)` ≡ `Text.repeat(s, n)`.
    assert!(errors_of(
        "import Core.Text; \
         function main() -> void { var s = \"hi\"; string u = s.upper(); string r = s.repeat(2); }"
    )
    .is_empty());
}

#[test]
fn ufcs_arg_type_still_checked() {
    // A resolved UFCS still type-checks the remaining arguments: `repeat` wants an `int` count.
    let errs = errors_of(
        "import Core.Text; \
         function main() -> void { var s = \"hi\"; string r = s.repeat(\"no\"); }",
    );
    assert!(
        errs.iter().any(|e| e.message.contains("Text.repeat")),
        "{errs:?}"
    );
}

#[test]
fn ufcs_safe_nav_on_optional() {
    // `x?.f()` UFCS: a null-safe member call on an optional receiver resolves the native and
    // yields an optional result (lowered to a `match` over the optional, F-002).
    assert!(errors_of(
        "import Core.Text; \
         function main() -> void { string? s = \"hi\"; string u = s?.upper() ?? \"x\"; }"
    )
    .is_empty());
}

#[test]
fn ufcs_safe_nav_result_is_optional() {
    // The `?.` UFCS result is optional, so binding it to a non-optional type without `??` is an error.
    let errs = errors_of(
        "import Core.Text; \
         function main() -> void { string? s = \"hi\"; string u = s?.upper(); }",
    );
    assert!(
        !errs.is_empty(),
        "expected an optional-to-non-optional error"
    );
}

#[test]
fn deprecated_native_emits_w_deprecated() {
    // The `#[cfg(test)]` sample in `native::deprecation_of` flags `Core.Math.abs` as deprecated, so a
    // call to it warns (the symbol still type-checks — W-DEPRECATED rides the warning channel, never
    // gating). Proves the native-call → W-DEPRECATED wiring end-to-end (rock 3).
    let src = "import Core.Math; function main() -> void { int x = Math.abs(-3); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    let w = warnings_of(src);
    assert!(
        w.iter().any(|d| d.code == Some("W-DEPRECATED")),
        "expected W-DEPRECATED, got {w:?}"
    );
}

#[test]
fn non_deprecated_native_has_no_w_deprecated() {
    // A neighboring native that is NOT flagged must not warn (guards against an over-broad lint).
    let src = "import Core.Math; function main() -> void { int x = Math.max(1, 2); }";
    let w = warnings_of(src);
    assert!(
        !w.iter().any(|d| d.code == Some("W-DEPRECATED")),
        "unexpected W-DEPRECATED, got {w:?}"
    );
}
