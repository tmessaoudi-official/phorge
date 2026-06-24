use phorge::checker::check;
use phorge::lexer::lex;
use phorge::parser::Parser;

/// The complete sample program from the design spec (§6), verbatim.
const SAMPLE: &str = r#"package Main;
import Core.Console;

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
        return "Hello {name}";
    }
}

function main() -> void {
    Greeter g = new Greeter("Tak");
    Console.println(g.greet());

    List<Shape> shapes = [new Circle(2.0), new Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        Console.println("area = {area(s)}");
    }
}
"#;

fn check_src(src: &str) -> Result<(), Vec<phorge::diagnostic::Diagnostic>> {
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
