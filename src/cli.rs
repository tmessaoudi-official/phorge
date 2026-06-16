//! CLI pipeline helpers, kept in the library so they are unit-testable without
//! spawning the binary. `main.rs` is a thin dispatcher over these. Each command
//! is `fn(&str) -> Result<String, String>`: `Ok` is text to print verbatim
//! (newline-terminated where appropriate), `Err` is a rendered error message.

use crate::ast::Program;
use crate::checker::check;
use crate::compiler::compile;
use crate::interpreter::interpret;
use crate::lexer::lex;
use crate::parser::Parser;
use crate::vm::Vm;

/// Run a pipeline closure on a worker thread with a large (256 MB) stack. The lexer is iterative,
/// but the parser, checker, compiler, and tree-walking interpreter all recurse on the native stack
/// in proportion to expression/call nesting. A generous, *known* stack makes the explicit depth
/// limits (`parser::MAX_NEST_DEPTH`, `value::MAX_CALL_DEPTH`) — not Rust's ambient frame budget —
/// the thing that bounds recursion, so adversarial-but-bounded input faults cleanly instead of
/// aborting, identically whether called from the CLI's main thread or a 2 MB test thread.
fn on_deep_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn_scoped(s, f)
            .expect("spawn pipeline worker thread")
            .join()
            .expect("pipeline worker thread panicked")
    })
}

/// lex + parse, rendering the stage error to a single line. Every stage now returns a unified
/// [`crate::diagnostic::Diagnostic`] that renders itself (stage prefix + position), so the CLI
/// just calls `to_string()` rather than hand-formatting per stage.
fn lex_parse(src: &str) -> Result<Program, String> {
    let tokens = lex(src).map_err(|e| e.to_string())?;
    Parser::new(tokens)
        .parse_program()
        .map_err(|e| e.to_string())
}

/// lex + parse + type-check (the gate). Renders every type error, one per line.
fn parse_checked(src: &str) -> Result<Program, String> {
    let prog = lex_parse(src)?;
    match check(&prog) {
        Ok(()) => Ok(prog),
        Err(errs) => {
            let lines: Vec<String> = errs.iter().map(ToString::to_string).collect();
            Err(lines.join("\n"))
        }
    }
}

/// `run`: lex -> parse -> check (gate) -> interpret -> captured stdout.
pub fn cmd_run(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        interpret(&prog).map_err(|e| e.to_string())
    })
}

/// `runvm`: lex -> parse -> check (gate) -> compile to bytecode -> VM -> captured stdout.
/// The bytecode backend; must produce byte-identical output to `cmd_run` (differential).
pub fn cmd_runvm(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        let program = compile(&prog).map_err(|e| e.to_string())?;
        Vm::new(&program).run().map_err(|e| e.to_string())
    })
}

/// `check`: lex -> parse -> check; report success or the type errors.
pub fn cmd_check(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        parse_checked(src)?;
        Ok("OK (type-checks clean)\n".to_string())
    })
}

/// `parse`: lex -> parse; dump the AST.
pub fn cmd_parse(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = lex_parse(src)?;
        Ok(format!("{prog:#?}\n"))
    })
}

/// `lex`: dump the token stream.
pub fn cmd_lex(src: &str) -> Result<String, String> {
    let tokens = lex(src).map_err(|e| e.to_string())?;
    let mut out = String::new();
    for t in tokens {
        out.push_str(&format!("{:?} @ {}:{}\n", t.kind, t.span.line, t.span.col));
    }
    Ok(out)
}

/// `transpile`: lex -> parse -> check (gate) -> emit PHP source.
pub fn cmd_transpile(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        crate::transpile::emit(&prog)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
import std.io;

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
    function greet() -> string { return "Hello {name}"; }
}

function main() {
    Greeter g = Greeter("Tak");
    println(g.greet());
    List<Shape> shapes = [Circle(2.0), Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        println("area = {area(s)}");
    }
}
"#;

    #[test]
    fn run_executes_sample() {
        assert_eq!(
            cmd_run(SAMPLE).unwrap(),
            "Hello Tak\narea = 12.56636\narea = 12\n"
        );
    }

    #[test]
    fn run_reports_type_error_and_does_not_execute() {
        // `area` returns float; returning an int literal is a type error.
        let src =
            r#"function area() -> float { return 1; } function main() { println("{area()}"); }"#;
        let err = cmd_run(src).unwrap_err();
        assert!(err.contains("type error"), "{err}");
    }

    #[test]
    fn run_reports_runtime_error() {
        let err = cmd_run(r#"function main() { println("{1 / 0}"); }"#).unwrap_err();
        assert!(err.contains("runtime error"), "{err}");
    }

    #[test]
    fn run_reports_parse_error() {
        let err = cmd_run("function main( {").unwrap_err();
        assert!(err.contains("parse error"), "{err}");
    }

    #[test]
    fn check_passes_on_clean_program() {
        let ok = cmd_check(SAMPLE).unwrap();
        assert!(ok.contains("OK"), "{ok}");
    }

    #[test]
    fn check_fails_on_type_error() {
        let src = r#"function f() -> float { return 1; } function main() {}"#;
        assert!(cmd_check(src).unwrap_err().contains("type error"));
    }

    #[test]
    fn parse_dumps_ast() {
        let out = cmd_parse(r#"function main() {}"#).unwrap();
        assert!(out.contains("Program"), "{out}");
    }

    #[test]
    fn lex_dumps_tokens() {
        let out = cmd_lex(r#"function main() {}"#).unwrap();
        assert!(out.contains("@ 1:1"), "{out}");
    }

    #[test]
    fn cmd_transpile_emits_php_for_sample() {
        let php = cmd_transpile(SAMPLE).expect("transpile");
        assert!(php.starts_with("<?php\n"), "{php}");
        assert!(php.contains("abstract class Shape {}"), "{php}");
        assert!(
            php.contains("function __construct(private string $name) {}"),
            "{php}"
        );
    }

    #[test]
    fn cmd_transpile_rejects_ill_typed() {
        let err = cmd_transpile(r#"function main() { int x = "no"; }"#).unwrap_err();
        assert!(err.contains("type error"), "{err}");
    }

    #[test]
    fn runvm_matches_run_on_simple_program() {
        let src = r#"function main() { int x = 21; println("{x + x}"); }"#;
        assert_eq!(cmd_runvm(src).unwrap(), cmd_run(src).unwrap());
        assert_eq!(cmd_runvm(src).unwrap(), "42\n");
    }

    #[test]
    fn runvm_reports_type_error_via_the_gate() {
        let err = cmd_runvm(r#"function main() { int x = "no"; }"#).unwrap_err();
        assert!(err.contains("type error"), "{err}");
    }

    #[test]
    fn runvm_reports_runtime_error_with_prefix() {
        let err = cmd_runvm(r#"function main() { println("{1 / 0}"); }"#).unwrap_err();
        assert!(err.contains("runtime error"), "{err}");
    }

    #[test]
    fn runvm_runtime_error_carries_source_line() {
        // div-by-zero in a statement on line 3. The VM now locates the fault via `Chunk.lines`
        // and renders `runtime error at 3: …`, while the canonical body ("division by zero")
        // stays intact so the differential `agree_err` oracle still classifies it identically.
        // NB: the division is *not* inside string interpolation — `split_interpolation`
        // re-lexes interpolated sub-expressions with a fresh lexer that resets to line 1, so a
        // fault inside `"{…}"` reports line 1 (a pre-existing interpolation-position limitation,
        // orthogonal to this task — see the M2 P3.5 roadmap decisions log).
        let src = "function main() {\n    int z = 0;\n    int x = 1 / z;\n    println(\"{x}\");\n}";
        let err = cmd_runvm(src).unwrap_err();
        assert!(err.contains("division by zero"), "{err}");
        assert!(err.starts_with("runtime error at 3:"), "{err}");
    }

    #[test]
    fn run_runtime_error_has_no_line() {
        // The tree-walking interpreter tracks no source position, so its runtime errors keep
        // the position-less `runtime error: …` form (deliberate asymmetry — documented).
        let src = "function main() {\n    int z = 0;\n    int x = 1 / z;\n    println(\"{x}\");\n}";
        let err = cmd_run(src).unwrap_err();
        assert!(err.starts_with("runtime error: "), "{err}");
        assert!(!err.contains(" at "), "{err}");
    }
}
