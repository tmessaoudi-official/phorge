use phorge::interpreter::interpret;
use phorge::lexer::lex;
use phorge::parser::Parser;

/// The complete sample program from the language design spec (§6), verbatim.
const SAMPLE: &str = r#"
import core.console;

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

function main() {
    Greeter g = Greeter("Tak");
    console.println(g.greet());

    List<Shape> shapes = [Circle(2.0), Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        console.println("area = {area(s)}");
    }
}
"#;

fn run(src: &str) -> Result<String, phorge::diagnostic::Diagnostic> {
    let tokens = lex(src).expect("lex ok");
    let prog = Parser::new(tokens).parse_program().expect("parse ok");
    interpret(&prog)
}

#[test]
fn sample_program_runs_and_prints_expected_output() {
    let out = run(SAMPLE).expect("sample should run clean");
    assert_eq!(out, "Hello Tak\narea = 12.56636\narea = 12\n");
}

#[test]
fn program_without_main_errors() {
    let e = run(r#"function helper() -> int { return 1; }"#).unwrap_err();
    assert!(e.message.contains("main"), "{}", e.message);
}

#[test]
fn division_by_zero_does_not_panic() {
    let e = run(r#"import core.console;
function main() { console.println("{1 / 0}"); }"#)
    .unwrap_err();
    assert!(e.message.contains("division by zero"), "{}", e.message);
}
