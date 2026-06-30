use super::*;
use crate::lexer::lex;
use crate::parser::Parser;

/// Lex + parse + interpret; return captured stdout or the runtime error. Auto-prepends the
/// reserved `package Main;` (M5 S1) so existing test programs need no per-case edit; the
/// segment carries no newline, preserving line numbers.
fn run(src: &str) -> Result<String, Diagnostic> {
    let src = with_pkg(src);
    let tokens = lex(&src).expect("lex ok");
    let prog = Parser::new(tokens).parse_program().expect("parse ok");
    interpret(&prog)
}

#[test]
fn interpreter_fault_carries_call_stack() {
    let err = run(
        "function f() -> int { var xs = [1]; return xs[5]; }\nfunction main() { var r = f(); }",
    )
    .unwrap_err();
    assert_eq!(err.frames.len(), 2, "callee + main: {:?}", err.frames);
    assert_eq!(err.frames[0].function, "f");
    assert_eq!(err.frames[1].function, "main");
}

fn with_pkg(src: &str) -> String {
    if src.trim_start().starts_with("package ") {
        src.to_string()
    } else {
        format!("package Main; {src}")
    }
}

fn out(src: &str) -> String {
    run(src).expect("run ok")
}

#[test]
fn prints_a_literal_string() {
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { Output.printLine("hi"); }"#),
        "hi\n"
    );
}

#[test]
fn integer_arithmetic_in_interpolation() {
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { Output.printLine("{1 + 2 * 3}"); }"#),
        "7\n"
    );
}

#[test]
fn float_arithmetic() {
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { Output.printLine("{3.0 * 4.0}"); }"#),
        "12\n"
    );
}

#[test]
fn division_by_zero_is_runtime_error() {
    let e = run(r#"import Core.Output;
function main() -> void { Output.printLine("{1 / 0}"); }"#)
    .unwrap_err();
    assert!(e.message.contains("division by zero"), "{}", e.message);
}

#[test]
fn comparison_and_logical_short_circuit() {
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { Output.printLine("{1 < 2 && 3 >= 3}"); }"#),
        "true\n"
    );
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { Output.printLine("{1 > 2 || false}"); }"#),
        "false\n"
    );
}

#[test]
fn unary_negation_and_not() {
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { Output.printLine("{-5}"); Output.printLine("{!true}"); }"#),
        "-5\nfalse\n"
    );
}

#[test]
fn var_decl_and_use() {
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { int x = 10; Output.printLine("{x + 5}"); }"#),
        "15\n"
    );
}

#[test]
fn if_else_picks_branch() {
    let src = r#"import Core.Output;
function main() -> void { if (1 < 2) { Output.printLine("yes"); } else { Output.printLine("no"); } }"#;
    assert_eq!(out(src), "yes\n");
}

#[test]
fn function_call_and_return() {
    let src = r#"import Core.Output;

            function dbl(int n) -> int { return n * 2; }
            function main() -> void { Output.printLine("{dbl(21)}"); }
        "#;
    assert_eq!(out(src), "42\n");
}

#[test]
fn recursion_works() {
    let src = r#"import Core.Output;

            function fac(int n) -> int {
                if (n <= 1) { return 1; }
                return n * fac(n - 1);
            }
            function main() -> void { Output.printLine("{fac(5)}"); }
        "#;
    assert_eq!(out(src), "120\n");
}

#[test]
fn enum_variant_and_match() {
    let src = r#"import Core.Output;

            enum Shape { Circle(float r), Rect(float w, float h), }
            function area(Shape s) -> float {
                return match s {
                    Circle(r) => r * r,
                    Rect(w, h) => w * h,
                };
            }
            function main() -> void { Output.printLine("{area(Rect(3.0, 4.0))}"); }
        "#;
    assert_eq!(out(src), "12\n");
}

#[test]
fn match_wildcard_is_catch_all() {
    // The `_` arm catches the Rect case (sample-faithful: payload variants).
    let src = r#"import Core.Output;

            enum Shape { Circle(float r), Rect(float w, float h), }
            function kind(Shape s) -> int { return match s { Circle(r) => 1, _ => 2, }; }
            function main() -> void { Output.printLine("{kind(Rect(1.0, 2.0))}"); }
        "#;
    assert_eq!(out(src), "2\n");
}

#[test]
fn class_construction_promotion_and_method() {
    let src = r#"import Core.Output;

            class Greeter {
                private string name;
                constructor(private string name) {}
                function greet() -> string { return "Hi {name}"; }
            }
            function main() -> void { Greeter g = Greeter("Tak"); Output.printLine(g.greet()); }
        "#;
    assert_eq!(out(src), "Hi Tak\n");
}

#[test]
fn for_loop_over_list() {
    let src = r#"import Core.Output;

            function main() -> void {
                List<int> xs = [1, 2, 3];
                for (int x in xs) { Output.printLine("{x}"); }
            }
        "#;
    assert_eq!(out(src), "1\n2\n3\n");
}

#[test]
fn indexing_reads_elements() {
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { List<int> xs = [7, 8, 9]; Output.printLine("{xs[0]} {xs[2]}"); }"#),
        "7 9\n"
    );
}

#[test]
fn indexing_out_of_range_is_runtime_error() {
    let e = run(r#"import Core.Output;
function main() -> void { List<int> xs = [1]; Output.printLine("{xs[3]}"); }"#)
    .unwrap_err();
    assert!(
        e.message.contains("list index out of range"),
        "{}",
        e.message
    );
}

#[test]
fn ranges_iterate_like_lists() {
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { for (int i in 0..3) { Output.printLine("{i}"); } }"#),
        "0\n1\n2\n"
    );
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { for (int i in 1..=3) { Output.printLine("{i}"); } }"#),
        "1\n2\n3\n"
    );
    // empty range (start >= end): body never runs
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { for (int i in 5..2) { Output.printLine("{i}"); } Output.printLine("done"); }"#),
        "done\n"
    );
}

#[test]
fn expression_if_picks_branch_value() {
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { var x = if (1 < 2) { 7 } else { 9 }; Output.printLine("{x}"); }"#),
        "7\n"
    );
    assert_eq!(
        out(r#"import Core.Output;
function main() -> void { var x = if (1 > 2) { 7 } else { 9 }; Output.printLine("{x}"); }"#),
        "9\n"
    );
}

#[test]
fn integer_overflow_is_runtime_error_not_panic() {
    let src = r#"import Core.Output;
function main() -> void { Output.printLine("{9223372036854775807 + 1}"); }"#;
    let e = run(src).unwrap_err();
    assert!(e.message.contains("overflow"), "{}", e.message);
}

#[test]
fn missing_main_is_runtime_error() {
    let e = run(r#"function other() -> void {}"#).unwrap_err();
    assert!(e.message.contains("main"), "{}", e.message);
}

// ---- lambda tests (M3 S3, Task 3 — interpreter-only) ----

/// Lex + parse + type-check `src`; return the error diagnostics (empty = well-typed).
/// Auto-prepends `package Main;` if absent. Used to test checker rejections without
/// running the interpreter.
fn check_errs(src: &str) -> Vec<crate::diagnostic::Diagnostic> {
    let src = with_pkg(src);
    let tokens = lex(&src).expect("lex ok");
    let prog = Parser::new(tokens).parse_program().expect("parse ok");
    match crate::checker::check(&prog) {
        Ok(_warnings) => Vec::new(),
        Err(e) => e,
    }
}

#[test]
fn lambda_value_call_interpreter() {
    let out = out(r#"package Main;
import Core.Output;
function main() -> void {
    var double = function(int x) => x * 2;
    Output.printLine("{double(5)}");
}"#);
    assert_eq!(out, "10\n");
}

#[test]
fn lambda_captures_two_vars_interpreter() {
    let out = out(r#"package Main;
import Core.Output;
function main() -> void {
    var a = 10;
    var b = 100;
    var f = function(int x) => x + a + b;
    Output.printLine("{f(1)}");
}"#);
    assert_eq!(out, "111\n");
}

#[test]
fn higher_order_user_function_interpreter() {
    let out = out(r#"package Main;
import Core.Output;
function twice(int x, (int) -> int f) -> int { return f(f(x)); }
function main() -> void {
    Output.printLine("{twice(3, function(int n) => n + 1)}");
}"#);
    assert_eq!(out, "5\n");
}

#[test]
fn method_lambda_may_capture_this_but_field_init_lambda_may_not() {
    // A method-body lambda MAY capture `this` (Phase 1 closures slice) — no error.
    let ok = check_errs(
        r#"package Main;
class C { constructor(public int x) {}
  function method() -> ((int) -> int) { return function(int n) => n + this.x; } }
function main() -> void { }"#,
    );
    assert!(
        ok.is_empty(),
        "method-body lambda + this should check: {ok:?}"
    );
    // A field-initializer lambda may NOT capture `this` (partially-built instance) → E-LAMBDA-THIS.
    let bad = check_errs(
        r#"package Main;
class C { int x = 1; (() -> int) f = function() => this.x; }
function main() -> void { }"#,
    );
    assert!(
        bad.iter().any(|e| e.code == Some("E-LAMBDA-THIS")),
        "field-init this-capture should be E-LAMBDA-THIS: {bad:?}"
    );
}

#[test]
fn interpolating_an_object_errors() {
    let src = r#"import Core.Output;

            class C { constructor() {} }
            function main() -> void { C c = C(); Output.printLine("{c}"); }
        "#;
    let e = run(src).unwrap_err();
    assert!(
        e.message.contains("interpolate") || e.message.contains("print"),
        "{}",
        e.message
    );
}
