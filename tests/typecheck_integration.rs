use phorj::checker::check;
use phorj::lexer::lex;
use phorj::parser::Parser;

/// The complete sample program from the design spec (§6), verbatim.
const SAMPLE: &str = r#"package Main;
import Core.Output;

enum Shape {
    Circle(float radius),
    Rect(float w, float h),
}

function area(Shape s) -> float {
    return match s {
        Circle(r)  => 3.14159 * r * r,
        Rect(w, h) => w * h,
    };
}

class Greeter {
    private string name;

    constructor(private string name) {}

    function greet() -> string {
        return "Hello {this.name}";
    }
}

function main() -> void {
    Greeter g = new Greeter("Tak");
    Output.printLine(g.greet());

    List<Shape> shapes = [new Circle(2.0), new Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        Output.printLine("area = {area(s)}");
    }
}
"#;

fn check_src(src: &str) -> Result<(), Vec<phorj::diagnostic::Diagnostic>> {
    let tokens = lex(src).expect("lex ok");
    let prog = Parser::new(tokens).parse_program().expect("parse ok");
    // `check` now returns the non-fatal warnings on success (M3 S2.5); this harness only cares
    // about the error/clean contract, so collapse `Ok(warnings)` to `Ok(())`.
    check(&prog).map(|_warnings| ())
}

#[test]
fn sample_program_type_checks_clean() {
    let result = check_src(SAMPLE);
    assert!(result.is_ok(), "expected clean type-check, got: {result:?}");
}

#[test]
fn non_exhaustive_match_in_full_program_errors() {
    let broken = SAMPLE.replace("        Rect(w, h) => w * h,\n", "");
    let errs = check_src(&broken).expect_err("should be non-exhaustive");
    assert!(
        errs.iter().any(|e| e.message.contains("non-exhaustive")),
        "{errs:?}"
    );
}

#[test]
fn wrong_constructor_arg_in_full_program_errors() {
    let broken = SAMPLE.replace(r#"new Greeter("Tak")"#, "new Greeter(123)");
    let errs = check_src(&broken).expect_err("should be a type error");
    assert!(
        errs.iter().any(|e| e.message.contains("argument 1")),
        "{errs:?}"
    );
}

#[test]
fn loop_variable_type_mismatch_errors() {
    let broken = SAMPLE.replace("for (Shape s in shapes)", "for (int s in shapes)");
    let errs = check_src(&broken).expect_err("should be a type error");
    assert!(!errs.is_empty());
}

// ── M6 W2: `#[Route(...)]` attribute validation ───────────────────────────────────────────────

fn has_code(errs: &[phorj::diagnostic::Diagnostic], code: &str) -> bool {
    errs.iter().any(|e| e.code == Some(code))
}

// ── M-RT S2.2: method return-type overloading ─────────────────────────────────────────────────

#[test]
fn bare_return_overloaded_method_call_needs_selector() {
    // A bare return-overloaded method call has no type context to pick a member — C1 scope requires
    // a `<Type>` selector at the call site (same rule free functions have without a sink).
    let src = "package Main;\n\
        class C { constructor() {} function f()->int { return 1; } function f()->bool { return true; } }\n\
        function main() -> void { var c = new C(); discard c.f(); }\n";
    let errs = check_src(src).expect_err("bare return-overloaded method call");
    assert!(has_code(&errs, "E-OVERLOAD-NO-CONTEXT"), "{errs:?}");
}

#[test]
fn selector_picks_return_overloaded_method() {
    // The `<Type>` selector resolves the method overload by return type — clean check.
    let src = "package Main;\n\
        class C { constructor() {} function f()->int { return 1; } function f()->bool { return true; } }\n\
        function main() -> void { var c = new C(); int n = <int>c.f(); bool b = <bool>c.f(); }\n";
    assert!(check_src(src).is_ok(), "{:?}", check_src(src));
}

#[test]
fn static_methods_cannot_return_overload() {
    // Return-overloading is instance-only this slice: a `static` call `ClassName.m(args)` has no
    // `<Type>` selector path, so a static return-overload would mangle its def with no call rewrite.
    // Statics keep the classic shared-return rule → E-OVERLOAD-RETURN.
    let src = "package Main;\n\
        class C { static function f()->int { return 1; } static function f()->bool { return true; } }\n\
        function main() -> void {}\n";
    let errs = check_src(src).expect_err("static return-overload");
    assert!(has_code(&errs, "E-OVERLOAD-RETURN"), "{errs:?}");
}

#[test]
fn selector_unknown_return_on_method_is_rejected() {
    // A selector naming a return type no overload has is E-OVERLOAD-SELECT-UNKNOWN.
    let src = "package Main;\n\
        class C { constructor() {} function f()->int { return 1; } function f()->bool { return true; } }\n\
        function main() -> void { var c = new C(); string s = <string>c.f(); }\n";
    let errs = check_src(src).expect_err("unknown return selector");
    assert!(has_code(&errs, "E-OVERLOAD-SELECT-UNKNOWN"), "{errs:?}");
}

#[test]
fn route_attribute_well_formed_checks_clean() {
    // The raw `check` path does not inject the Core.Http prelude (that is `cli::check_and_expand`),
    // so this asserts only the attribute validation itself: a well-formed `#[Route]` (two string
    // literals, good path, one-param + return handler shape) produces no attribute diagnostics. The
    // end-to-end `Request`/`Response` typing is covered by the conformance + differential gates.
    let src = "package Main;\n#[Route(\"GET\", \"/health\")]\nfunction h(int x) -> int { return x; }\nfunction main() -> void {}\n";
    assert!(check_src(src).is_ok(), "{:?}", check_src(src));
}

#[test]
fn unknown_attribute_is_rejected() {
    let src = "package Main;\n#[Cache(60)]\nfunction f() -> void {}\nfunction main() -> void {}\n";
    let errs = check_src(src).expect_err("unknown attribute");
    assert!(has_code(&errs, "E-UNKNOWN-ATTRIBUTE"), "{errs:?}");
}

#[test]
fn route_with_wrong_arg_count_is_rejected() {
    let src = "package Main;\nimport Core.Http;\n#[Route(\"GET\")]\nfunction f(Request req) -> Response { return Response.text(200, \"x\"); }\nfunction main() -> void {}\n";
    let errs = check_src(src).expect_err("bad route args");
    assert!(has_code(&errs, "E-ROUTE-ARGS"), "{errs:?}");
}

#[test]
fn route_with_bad_path_is_rejected() {
    let src = "package Main;\nimport Core.Http;\n#[Route(\"GET\", \"health\")]\nfunction f(Request req) -> Response { return Response.text(200, \"x\"); }\nfunction main() -> void {}\n";
    let errs = check_src(src).expect_err("bad route spec");
    assert!(has_code(&errs, "E-ROUTE-SPEC"), "{errs:?}");
}

#[test]
fn route_handler_wrong_shape_is_rejected() {
    let src = "package Main;\n#[Route(\"GET\", \"/\")]\nfunction f(int a, int b) -> int { return a + b; }\nfunction main() -> void {}\n";
    let errs = check_src(src).expect_err("bad handler shape");
    assert!(has_code(&errs, "E-ROUTE-HANDLER"), "{errs:?}");
}

#[test]
fn route_on_static_method_checks_clean() {
    // `#[Route]` on a static method is valid; Request/Response need not resolve in the raw `check`
    // path (no Core.Http injection here), so use a non-Http handler shape — the attribute + static
    // checks are what this exercises.
    let src = "package Main;\nclass C {\n  #[Route(\"GET\", \"/x\")]\n  static function h(int r) -> int { return r; }\n}\nfunction main() -> void {}\n";
    assert!(check_src(src).is_ok(), "{:?}", check_src(src));
}

#[test]
fn route_on_instance_method_requires_static() {
    let src = "package Main;\nclass C {\n  #[Route(\"GET\", \"/x\")]\n  function h(int r) -> int { return r; }\n}\nfunction main() -> void {}\n";
    let errs = check_src(src).expect_err("instance #[Route] method must fail");
    assert!(has_code(&errs, "E-ROUTE-METHOD-STATIC"), "{errs:?}");
}

#[test]
fn attribute_on_non_function_is_a_parse_error() {
    // E-ATTR-TARGET is a parse-stage error, so it surfaces before the checker.
    let src = "package Main;\n#[Route(\"GET\", \"/\")]\nclass Foo {}\nfunction main() -> void {}\n";
    let tokens = lex(src).expect("lex ok");
    let err = Parser::new(tokens)
        .parse_program()
        .expect_err("attribute on a class must fail to parse");
    assert_eq!(err.code, Some("E-ATTR-TARGET"), "{err:?}");
}
