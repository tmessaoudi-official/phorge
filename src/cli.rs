//! CLI pipeline helpers, kept in the library so they are unit-testable without
//! spawning the binary. `main.rs` is a thin dispatcher over these. Each command
//! is `fn(&str) -> Result<String, String>`: `Ok` is text to print verbatim
//! (newline-terminated where appropriate), `Err` is a rendered error message.

use std::time::{Duration, Instant};

use crate::ast::Program;
use crate::checker::check;
use crate::compiler::compile;
use crate::interpreter::interpret;
use crate::lexer::lex;
use crate::parser::Parser;
use crate::vm::Vm;

/// The `--version` line: `phorge <version>` (from `CARGO_PKG_VERSION`).
pub fn version_line() -> String {
    format!("phorge {}", env!("CARGO_PKG_VERSION"))
}

/// The `--help` text: version banner + commands + source forms + options.
pub fn help_text() -> String {
    format!(
        "{version}\n\
         usage:\n  \
         phorge <command> <source> [options]\n\n\
         commands:\n  \
         run        interpret the program (tree-walking)\n  \
         runvm      run the program on the bytecode VM\n  \
         check      type-check only\n  \
         parse      print the AST\n  \
         lex        print the token stream\n  \
         transpile  emit PHP\n  \
         bench      benchmark run vs runvm\n  \
         build      compile to a standalone executable (-o <out>)\n\n\
         source:\n  \
         <file>     read the program from a file\n  \
         -          read the program from stdin\n  \
         -e <code>  run an inline program (alias: --eval)\n  \
         --         treat the next argument as a file path (even if it starts with '-')\n\n\
         options:\n  \
         -h, --help     print this help and exit\n  \
         -v, --version  print the version and exit\n",
        version = version_line()
    )
}

/// Where a command reads its program from, resolved from the args after the subcommand.
#[derive(Debug, PartialEq, Eq)]
pub enum SourceSpec {
    /// Read the program from this file path.
    File(String),
    /// Read the program from standard input.
    Stdin,
    /// Run this inline program text directly.
    Inline(String),
}

/// Resolve the program source from the args following the subcommand (`args[2..]`):
/// `<file>` | `-` (stdin) | `-e <code>` / `--eval <code>` | `-- <file>`. Returns `None` on a usage
/// error (missing source, dangling `-e`, an unknown leading-`-` arg, or extra positionals) — the
/// caller prints usage and exits 2.
pub fn resolve_source(rest: &[String]) -> Option<SourceSpec> {
    match rest {
        [flag, code] if flag == "-e" || flag == "--eval" => Some(SourceSpec::Inline(code.clone())),
        [sep, path] if sep == "--" => Some(SourceSpec::File(path.clone())),
        [one] if one == "-" => Some(SourceSpec::Stdin),
        [one] if !one.starts_with('-') => Some(SourceSpec::File(one.clone())),
        _ => None,
    }
}

/// Run a pipeline closure on a worker thread with a large (256 MB) stack. The lexer is iterative,
/// but the parser, checker, compiler, and tree-walking interpreter all recurse on the native stack
/// in proportion to expression/call nesting. A generous, *known* stack makes the explicit depth
/// limits (`limits::MAX_NEST_DEPTH`, `limits::MAX_CALL_DEPTH`) — not Rust's ambient frame budget —
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

/// Build a standalone executable for the host (x86_64-linux-gnu) from `src`. `input_path` names the
/// source (used to derive the default output name); `out_path` overrides it. Validates the program
/// first (never emits a broken binary), then copies this phorge binary and embeds `src` as a
/// `.phorge` section via `llvm-objcopy`. Returns a one-line success message.
pub fn cmd_build(input_path: &str, src: &str, out_path: Option<&str>) -> Result<String, String> {
    // 1. Validate: reuse the checker pipeline; surface its diagnostics, emit nothing on failure.
    cmd_check(src)?;

    // 2. Resolve output path: explicit -o, else ./<input-stem>.
    let out = match out_path {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let stem = std::path::Path::new(input_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| format!("cannot derive output name from {input_path}"))?;
            std::path::PathBuf::from(stem)
        }
    };

    // 3. The stub is this running phorge binary (Phase 1: host target only).
    let stub = std::env::current_exe().map_err(|e| format!("cannot locate phorge binary: {e}"))?;

    // 4. Write the payload container to a temp file for objcopy.
    let payload = std::env::temp_dir().join(format!("phorge-build-{}.bin", std::process::id()));
    std::fs::write(&payload, crate::bundle::encode_container(src.as_bytes()))
        .map_err(|e| format!("cannot write payload: {e}"))?;

    // 5. objcopy: copy the stub to `out` with the `.phorge` section added.
    let objcopy = std::env::var("PHORGE_OBJCOPY").unwrap_or_else(|_| "llvm-objcopy".into());
    let status = std::process::Command::new(&objcopy)
        .args([
            "--add-section",
            &format!(".phorge={}", payload.display()),
            "--set-section-flags",
            ".phorge=noload,readonly",
        ])
        .arg(&stub)
        .arg(&out)
        .status();
    let _ = std::fs::remove_file(&payload);
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => return Err(format!("{objcopy} failed with status {s}")),
        Err(e) => return Err(format!("cannot run {objcopy}: {e}")),
    }

    // 6. Make it executable (unix).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&out) {
            let mut perm = meta.permissions();
            perm.set_mode(perm.mode() | 0o111);
            let _ = std::fs::set_permissions(&out, perm);
        }
    }

    Ok(format!("built {}\n", out.display()))
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

/// Default sample count for `phorge bench`. Odd, so the median is a real observed sample rather
/// than an average of two; large enough to damp scheduler jitter on the small M2 corpus without
/// making the CLI feel slow.
const BENCH_DEFAULT_ITERS: usize = 101;

/// `bench`: *measure* the M2 thesis ("the VM executes faster than the tree-walker") instead of
/// asserting it. Parses+checks once, then reports median-of-N wall-clock for the front-end
/// (parse+check), the one-time bytecode compile, and each backend's execution phase, plus a
/// speedup verdict. Establishes the baseline that turns every later perf claim (Copy-on-`Op`,
/// deep-copy elimination, hot-path micro-perf) from Speculative into Verified — no perf-motivated
/// change should ship without a before/after number from this harness.
pub fn cmd_bench(src: &str) -> Result<String, String> {
    bench_report(src, BENCH_DEFAULT_ITERS)
}

/// Median wall-clock of `f` over `iters` samples after one untimed warmup. Generic over the
/// closure's `Ok` value so the same path times the interpreter (`String`), the VM (`String`), and
/// the compiler (`BytecodeProgram`). Propagates the first error — a faulting program can't be
/// benchmarked. The warmup pays one-time allocation/cache costs outside the measured window.
fn median_of<T>(
    iters: usize,
    mut f: impl FnMut() -> Result<T, String>,
) -> Result<Duration, String> {
    f()?; // warmup (untimed)
    let mut samples: Vec<Duration> = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        f()?;
        samples.push(t0.elapsed());
    }
    samples.sort_unstable();
    Ok(samples[samples.len() / 2])
}

/// Adaptive duration rendering (ns / µs / ms) so a fast and a slow stage stay legible in the same
/// report instead of a fixed unit truncating one of them to `0.000`.
fn fmt_dur(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 1_000 {
        format!("{ns} ns")
    } else if ns < 1_000_000 {
        format!("{:.3} µs", ns as f64 / 1_000.0)
    } else {
        format!("{:.3} ms", ns as f64 / 1_000_000.0)
    }
}

/// The bench engine (separated from [`cmd_bench`] so tests can pass a small `iters`). Runs on the
/// deep-stack worker like every other pipeline command.
fn bench_report(src: &str, iters: usize) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        let program = compile(&prog).map_err(|e| e.to_string())?;

        // Output-identity gate: comparing the speed of two backends that *disagree* is
        // meaningless. This is the differential harness's parity contract, enforced here at run
        // time before any timing — if it ever fails, the divergence (not the timing) is the news.
        let tw_out = interpret(&prog).map_err(|e| e.to_string())?;
        let vm_out = Vm::new(&program).run().map_err(|e| e.to_string())?;
        if tw_out != vm_out {
            return Err(format!(
                "bench aborted: backends disagree — tree-walk produced {} bytes, vm {} bytes; \
                 fix parity (see the differential harness) before benchmarking",
                tw_out.len(),
                vm_out.len()
            ));
        }

        let front = median_of(iters, || parse_checked(src))?;
        let comp = median_of(iters, || compile(&prog).map_err(|e| e.to_string()))?;
        let tw = median_of(iters, || interpret(&prog).map_err(|e| e.to_string()))?;
        let vm = median_of(iters, || Vm::new(&program).run().map_err(|e| e.to_string()))?;

        // Branch on integer nanos (no float equality); convert to f64 only for the ratio display.
        let tw_ns = tw.as_nanos();
        let vm_ns = vm.as_nanos();
        let verdict = if tw_ns == 0 || vm_ns == 0 {
            "verdict: backend execution too fast to measure at this sample size — \
             use a heavier corpus"
                .to_string()
        } else if vm_ns <= tw_ns {
            format!(
                "verdict: vm run is {:.2}× faster than tree-walk run ({} → {})",
                tw_ns as f64 / vm_ns as f64,
                fmt_dur(tw),
                fmt_dur(vm)
            )
        } else {
            format!(
                "verdict: tree-walk run is {:.2}× faster than vm run ({} → {})",
                vm_ns as f64 / tw_ns as f64,
                fmt_dur(vm),
                fmt_dur(tw)
            )
        };

        let mut out = String::new();
        out.push_str(&format!(
            "phorge bench — median of {iters} (warmup 1, std Instant)\n"
        ));
        out.push_str(&format!(
            "output: {} bytes, identical on both backends\n\n",
            tw_out.len()
        ));
        out.push_str(&format!("  parse+check   {}\n", fmt_dur(front)));
        out.push_str(&format!(
            "  compile       {}  (one-time, vm only)\n",
            fmt_dur(comp)
        ));
        out.push_str(&format!("  tree-walk run {}\n", fmt_dur(tw)));
        out.push_str(&format!("  vm run        {}\n\n", fmt_dur(vm)));
        out.push_str(&verdict);
        out.push('\n');
        Ok(out)
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

    fn rest(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn resolve_source_handles_all_forms() {
        assert_eq!(
            resolve_source(&rest(&["prog.phg"])),
            Some(SourceSpec::File("prog.phg".into()))
        );
        assert_eq!(resolve_source(&rest(&["-"])), Some(SourceSpec::Stdin));
        assert_eq!(
            resolve_source(&rest(&["-e", "x"])),
            Some(SourceSpec::Inline("x".into()))
        );
        assert_eq!(
            resolve_source(&rest(&["--eval", "y"])),
            Some(SourceSpec::Inline("y".into()))
        );
        // `--` lets a path start with '-'.
        assert_eq!(
            resolve_source(&rest(&["--", "-weird.phg"])),
            Some(SourceSpec::File("-weird.phg".into()))
        );
    }

    #[test]
    fn resolve_source_rejects_bad_forms() {
        assert_eq!(resolve_source(&rest(&[])), None); // missing source
        assert_eq!(resolve_source(&rest(&["-e"])), None); // -e without code
        assert_eq!(resolve_source(&rest(&["-x"])), None); // unknown flag (use -- to pass it)
        assert_eq!(resolve_source(&rest(&["a", "b"])), None); // too many positionals
    }

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

    #[test]
    fn bench_reports_both_backends_with_identical_output() {
        // Small iteration count keeps the test fast; the report must name both backends, confirm
        // output identity (and the byte count it asserted), and end in a verdict comparing them.
        let src = r#"function main() { int x = 21; println("{x + x}"); }"#;
        let out = bench_report(src, 5).expect("bench");
        assert!(out.contains("tree-walk run"), "{out}");
        assert!(out.contains("vm run"), "{out}");
        assert!(out.contains("identical on both backends"), "{out}");
        assert!(out.contains("verdict:"), "{out}");
        // Output is "42\n" = 3 bytes — the report states the byte count it asserted identical.
        assert!(out.contains("3 bytes"), "{out}");
    }

    #[test]
    fn bench_propagates_type_error_without_timing() {
        // A program that fails the gate can't be benchmarked — the error surfaces, no timing runs.
        let err = bench_report(r#"function main() { int x = "no"; }"#, 5).unwrap_err();
        assert!(err.contains("type error"), "{err}");
    }

    #[test]
    fn bench_default_entry_uses_101_samples() {
        // The public entry runs the default-N path end to end (smoke test of `cmd_bench`).
        let out = cmd_bench(r#"function main() { println("hi"); }"#).expect("bench");
        assert!(out.starts_with("phorge bench — median of 101"), "{out}");
    }
}
