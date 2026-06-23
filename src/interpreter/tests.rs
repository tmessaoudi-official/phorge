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
        out(r#"import Core.Console;
function main() { Console.println("hi"); }"#),
        "hi\n"
    );
}

#[test]
fn integer_arithmetic_in_interpolation() {
    assert_eq!(
        out(r#"import Core.Console;
function main() { Console.println("{1 + 2 * 3}"); }"#),
        "7\n"
    );
}

#[test]
fn float_arithmetic() {
    assert_eq!(
        out(r#"import Core.Console;
function main() { Console.println("{3.0 * 4.0}"); }"#),
        "12\n"
    );
}

#[test]
fn division_by_zero_is_runtime_error() {
    let e = run(r#"import Core.Console;
function main() { Console.println("{1 / 0}"); }"#)
    .unwrap_err();
    assert!(e.message.contains("division by zero"), "{}", e.message);
}

#[test]
fn comparison_and_logical_short_circuit() {
    assert_eq!(
        out(r#"import Core.Console;
function main() { Console.println("{1 < 2 && 3 >= 3}"); }"#),
        "true\n"
    );
    assert_eq!(
        out(r#"import Core.Console;
function main() { Console.println("{1 > 2 || false}"); }"#),
        "false\n"
    );
}

#[test]
fn unary_negation_and_not() {
    assert_eq!(
        out(r#"import Core.Console;
function main() { Console.println("{-5}"); Console.println("{!true}"); }"#),
        "-5\nfalse\n"
    );
}

#[test]
fn var_decl_and_use() {
    assert_eq!(
        out(r#"import Core.Console;
function main() { int x = 10; Console.println("{x + 5}"); }"#),
        "15\n"
    );
}

#[test]
fn if_else_picks_branch() {
    let src = r#"import Core.Console;
function main() { if (1 < 2) { Console.println("yes"); } else { Console.println("no"); } }"#;
    assert_eq!(out(src), "yes\n");
}

#[test]
fn function_call_and_return() {
    let src = r#"import Core.Console;

            function dbl(int n) -> int { return n * 2; }
            function main() { Console.println("{dbl(21)}"); }
        "#;
    assert_eq!(out(src), "42\n");
}

#[test]
fn recursion_works() {
    let src = r#"import Core.Console;

            function fac(int n) -> int {
                if (n <= 1) { return 1; }
                return n * fac(n - 1);
            }
            function main() { Console.println("{fac(5)}"); }
        "#;
    assert_eq!(out(src), "120\n");
}

#[test]
fn enum_variant_and_match() {
    let src = r#"import Core.Console;

            enum Shape { Circle(float r), Rect(float w, float h), }
            function area(Shape s) -> float {
                return match s {
                    Circle(r) => r * r,
                    Rect(w, h) => w * h,
                };
            }
            function main() { Console.println("{area(Rect(3.0, 4.0))}"); }
        "#;
    assert_eq!(out(src), "12\n");
}

#[test]
fn match_wildcard_is_catch_all() {
    // The `_` arm catches the Rect case (sample-faithful: payload variants).
    let src = r#"import Core.Console;

            enum Shape { Circle(float r), Rect(float w, float h), }
            function kind(Shape s) -> int { return match s { Circle(r) => 1, _ => 2, }; }
            function main() { Console.println("{kind(Rect(1.0, 2.0))}"); }
        "#;
    assert_eq!(out(src), "2\n");
}

#[test]
fn class_construction_promotion_and_method() {
    let src = r#"import Core.Console;

            class Greeter {
                private string name;
                constructor(private string name) {}
                function greet() -> string { return "Hi {name}"; }
            }
            function main() { Greeter g = Greeter("Tak"); Console.println(g.greet()); }
        "#;
    assert_eq!(out(src), "Hi Tak\n");
}

#[test]
fn for_loop_over_list() {
    let src = r#"import Core.Console;

            function main() {
                List<int> xs = [1, 2, 3];
                for (int x in xs) { Console.println("{x}"); }
            }
        "#;
    assert_eq!(out(src), "1\n2\n3\n");
}

#[test]
fn indexing_reads_elements() {
    assert_eq!(
        out(r#"import Core.Console;
function main() { List<int> xs = [7, 8, 9]; Console.println("{xs[0]} {xs[2]}"); }"#),
        "7 9\n"
    );
}

#[test]
fn indexing_out_of_range_is_runtime_error() {
    let e = run(r#"import Core.Console;
function main() { List<int> xs = [1]; Console.println("{xs[3]}"); }"#)
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
        out(r#"import Core.Console;
function main() { for (int i in 0..3) { Console.println("{i}"); } }"#),
        "0\n1\n2\n"
    );
    assert_eq!(
        out(r#"import Core.Console;
function main() { for (int i in 1..=3) { Console.println("{i}"); } }"#),
        "1\n2\n3\n"
    );
    // empty range (start >= end): body never runs
    assert_eq!(
        out(r#"import Core.Console;
function main() { for (int i in 5..2) { Console.println("{i}"); } Console.println("done"); }"#),
        "done\n"
    );
}

#[test]
fn expression_if_picks_branch_value() {
    assert_eq!(
        out(r#"import Core.Console;
function main() { var x = if (1 < 2) { 7 } else { 9 }; Console.println("{x}"); }"#),
        "7\n"
    );
    assert_eq!(
        out(r#"import Core.Console;
function main() { var x = if (1 > 2) { 7 } else { 9 }; Console.println("{x}"); }"#),
        "9\n"
    );
}

#[test]
fn integer_overflow_is_runtime_error_not_panic() {
    let src = r#"import Core.Console;
function main() { Console.println("{9223372036854775807 + 1}"); }"#;
    let e = run(src).unwrap_err();
    assert!(e.message.contains("overflow"), "{}", e.message);
}

#[test]
fn missing_main_is_runtime_error() {
    let e = run(r#"function other() {}"#).unwrap_err();
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
import Core.Console;
function main() {
    var double = fn(int x) => x * 2;
    Console.println("{double(5)}");
}"#);
    assert_eq!(out, "10\n");
}

#[test]
fn lambda_captures_two_vars_interpreter() {
    let out = out(r#"package Main;
import Core.Console;
function main() {
    var a = 10;
    var b = 100;
    var f = fn(int x) => x + a + b;
    Console.println("{f(1)}");
}"#);
    assert_eq!(out, "111\n");
}

#[test]
fn higher_order_user_function_interpreter() {
    let out = out(r#"package Main;
import Core.Console;
function twice(int x, (int) -> int f) -> int { return f(f(x)); }
function main() {
    Console.println("{twice(3, fn(int n) => n + 1)}");
}"#);
    assert_eq!(out, "5\n");
}

#[test]
fn lambda_cannot_reference_this() {
    let errs = check_errs(
        r#"package Main;
class C { constructor(public int x) {}
  function method() -> (int) -> int { return fn(int n) => n + this.x; } }
function main() { }"#,
    );
    assert!(
        errs.iter().any(|e| e.message.contains("`this`")),
        "{errs:?}"
    );
}

#[test]
fn interpolating_an_object_errors() {
    let src = r#"import Core.Console;

            class C { constructor() {} }
            function main() { C c = C(); Console.println("{c}"); }
        "#;
    let e = run(src).unwrap_err();
    assert!(
        e.message.contains("interpolate") || e.message.contains("print"),
        "{}",
        e.message
    );
}
