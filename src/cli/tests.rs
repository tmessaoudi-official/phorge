use super::*;
// bench_report helpers moved to the bench submodule (M-Decomp W1.2); the timing tests call them.
use super::bench::{bench_report, bench_report_opts};

/// Prepend the reserved `package Main;` (M5 S1: every file is packaged, never inferred) unless
/// already declared, so the CLI command tests need no per-case package boilerplate. The segment
/// carries no newline, so line numbers in fault diagnostics are preserved.
fn wp(src: &str) -> String {
    if src.trim_start().starts_with("package ") {
        src.to_string()
    } else {
        format!("package Main; {src}")
    }
}

const SAMPLE: &str = r#"package Main;
import Core.Output;

enum Shape {
    Circle(float radius),
    Rect(float w, float h),
}

function area(Shape s): float {
    return match s {
        Circle(r)  => 3.14159 * r * r,
        Rect(w, h) => w * h,
    };
}

class Greeter {
    private string name;
    constructor(private string name) {}
    function greet(): string { return "Hello {this.name}"; }
}

function main(): void {
    Greeter g = new Greeter("Tak");
    Output.printLine(g.greet());
    List<Shape> shapes = [new Circle(2.0), new Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        Output.printLine("area = {area(s)}");
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
fn resolve_source_and_args_splits_program_argv_on_terminator() {
    // `<file> -- a b c` → File + argv.
    assert_eq!(
        resolve_source_and_args(&rest(&["app.phg", "--", "a", "b"])),
        Some((SourceSpec::File("app.phg".into()), rest(&["a", "b"])))
    );
    // No `--` → empty argv.
    assert_eq!(
        resolve_source_and_args(&rest(&["app.phg"])),
        Some((SourceSpec::File("app.phg".into()), vec![]))
    );
    // `-e code -- args` and `- -- args`.
    assert_eq!(
        resolve_source_and_args(&rest(&["-e", "x", "--", "a"])),
        Some((SourceSpec::Inline("x".into()), rest(&["a"])))
    );
    assert_eq!(
        resolve_source_and_args(&rest(&["-", "--", "a", "b"])),
        Some((SourceSpec::Stdin, rest(&["a", "b"])))
    );
    // Leading `--` is the literal-path escape; a SECOND `--` then carries argv.
    assert_eq!(
        resolve_source_and_args(&rest(&["--", "-weird.phg"])),
        Some((SourceSpec::File("-weird.phg".into()), vec![]))
    );
    assert_eq!(
        resolve_source_and_args(&rest(&["--", "-weird.phg", "--", "a"])),
        Some((SourceSpec::File("-weird.phg".into()), rest(&["a"])))
    );
    // An empty argv after `--` is allowed (`app.phg --`).
    assert_eq!(
        resolve_source_and_args(&rest(&["app.phg", "--"])),
        Some((SourceSpec::File("app.phg".into()), vec![]))
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
    let src = wp(r#"import Core.Output;
function area(): float { return 1; } function main(): void { Output.printLine("{area()}"); }"#);
    let err = cmd_run(&src).unwrap_err();
    assert!(err.contains("type error"), "{err}");
}

#[test]
fn run_reports_runtime_error() {
    let err = cmd_run(&wp(r#"import Core.Output;
function main(): void { Output.printLine("{1 / 0}"); }"#))
    .unwrap_err();
    assert!(err.contains("runtime error"), "{err}");
}

#[test]
fn run_reports_parse_error() {
    let err = cmd_run(&wp("function main( {")).unwrap_err();
    assert!(err.contains("parse error"), "{err}");
}

#[test]
fn library_file_without_main_checks_and_transpiles_but_run_errors_clearly() {
    // Batch-1 A: a library/web file with no `main` is valid — it type-checks and transpiles. Only
    // *running* needs an entry point; the run/runvm error names it clearly (not a bare "no main").
    let lib = wp("function helper(int n) -> int { return n + 1; }");
    assert!(cmd_check(&lib).unwrap().contains("OK"), "check should pass");
    assert!(
        cmd_transpile(&lib)
            .expect("transpile")
            .contains("function helper"),
        "transpile should emit the library function"
    );
    let run_err = cmd_run(&lib).unwrap_err();
    assert!(
        run_err.contains("no entry point") && run_err.contains("main"),
        "run error: {run_err}"
    );
    let vm_err = cmd_runvm(&lib).unwrap_err();
    assert!(vm_err.contains("no entry point"), "runvm error: {vm_err}");
}

#[test]
fn check_passes_on_clean_program() {
    let ok = cmd_check(SAMPLE).unwrap();
    assert!(ok.contains("OK"), "{ok}");
}

#[test]
fn check_fails_on_type_error() {
    let src = wp(r#"function f(): float { return 1; } function main(): void {}"#);
    assert!(cmd_check(&src).unwrap_err().contains("type error"));
}

#[test]
fn parse_dumps_ast() {
    let out = cmd_parse(r#"function main(): void {}"#).unwrap();
    assert!(out.contains("Program"), "{out}");
}

#[test]
fn lex_dumps_tokens() {
    let out = cmd_lex(r#"function main(): void {}"#).unwrap();
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
    let err = cmd_transpile(&wp(r#"function main(): void { int x = "no"; }"#)).unwrap_err();
    assert!(err.contains("type error"), "{err}");
}

#[test]
fn cmd_lift_emits_annotated_phorj_draft() {
    let phg =
        cmd_lift("<?php function add(int $a, int $b): int { return $a + $b; }").expect("lift");
    // The banner makes the review-required contract visible in the file.
    assert!(phg.starts_with("// lifted (verify)"), "{phg}");
    assert!(phg.contains("package Main;"), "{phg}");
    assert!(phg.contains("function add(int a, int b): int {"), "{phg}");
}

#[test]
fn cmd_lift_refuses_outside_tier1_loudly() {
    // An `array` type has no faithful Phorj form yet — a clear lift error, not a guess.
    let err = cmd_lift("<?php function f(array $xs): void {}").unwrap_err();
    assert!(err.contains("`array` type"), "{err}");
}

#[test]
fn runvm_matches_run_on_simple_program() {
    let src = wp(r#"import Core.Output;
function main(): void { int x = 21; Output.printLine("{x + x}"); }"#);
    assert_eq!(cmd_runvm(&src).unwrap(), cmd_run(&src).unwrap());
    assert_eq!(cmd_runvm(&src).unwrap(), "42\n");
}

#[test]
fn runvm_reports_type_error_via_the_gate() {
    let err = cmd_runvm(&wp(r#"function main(): void { int x = "no"; }"#)).unwrap_err();
    assert!(err.contains("type error"), "{err}");
}

#[test]
fn runvm_reports_runtime_error_with_prefix() {
    let err = cmd_runvm(&wp(r#"import Core.Output;
function main(): void { Output.printLine("{1 / 0}"); }"#))
    .unwrap_err();
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
    let src = wp("import Core.Output; function main(): void {\n    int z = 0;\n    int x = 1 / z;\n    Output.printLine(\"{x}\");\n}");
    let err = cmd_runvm(&src).unwrap_err();
    assert!(err.contains("division by zero"), "{err}");
    assert!(err.starts_with("runtime error at 3:"), "{err}");
}

#[test]
fn run_runtime_error_carries_line_via_trace() {
    // Error-handling slice 1 removed the old interpreter/VM asymmetry: the tree-walker now keeps a
    // logical call stack, so a runtime fault backfills the diagnostic line from the innermost
    // frame — the interpreter reports `runtime error at <line>: …`, matching the VM.
    let src = wp("import Core.Output; function main(): void {\n    int z = 0;\n    int x = 1 / z;\n    Output.printLine(\"{x}\");\n}");
    let err = cmd_run(&src).unwrap_err();
    assert!(err.contains("division by zero"), "{err}");
    assert!(err.starts_with("runtime error at 3:"), "{err}");
}

#[test]
fn bench_reports_both_backends_with_identical_output() {
    // Small iteration count keeps the test fast; the report must name both backends, confirm
    // output identity (and the byte count it asserted), and end in a verdict comparing them.
    let src = wp(r#"import Core.Output;
function main(): void { int x = 21; Output.printLine("{x + x}"); }"#);
    let out = bench_report(&src, 5).expect("bench");
    assert!(out.contains("tree-walk run"), "{out}");
    assert!(out.contains("vm run"), "{out}");
    assert!(out.contains("identical on both backends"), "{out}");
    assert!(out.contains("verdict:"), "{out}");
    // Output is "42\n" = 3 bytes — the report states the byte count it asserted identical.
    assert!(out.contains("3 bytes"), "{out}");
}

#[test]
fn bench_vs_php_emits_a_php_section() {
    // `--vs-php` always emits a "vs PHP" section — either the comparison (php present) or a
    // graceful skip note (php absent). Both start with "vs PHP", so the test is host-agnostic.
    let src = wp(r#"import Core.Output;
function main(): void { int x = 21; Output.printLine("{x + x}"); }"#);
    let out = bench_report_opts(&src, 3, true).expect("bench");
    assert!(out.contains("vs PHP"), "{out}");
    // The standard report is still present.
    assert!(out.contains("vm run"), "{out}");
}

#[test]
fn bench_reports_a_memory_section() {
    // Beyond timing, the report carries a memory block. The header is printed unconditionally
    // (the per-phase numbers are present on Linux, "unavailable" elsewhere), so asserting the
    // header keeps the test platform-independent.
    let src = wp(r#"import Core.Output;
function main(): void { Output.printLine("hi"); }"#);
    let out = bench_report(&src, 5).expect("bench");
    assert!(out.contains("memory"), "{out}");
}

#[test]
fn disasm_dumps_bytecode_with_mnemonics_and_annotations() {
    // The disassembler names the function, prints the type-specialized int-add op, the native
    // call op (the migrated former `Print`), and annotates a constant load with its value.
    let out = cmd_disasm(&wp(
        r#"import Core.Output; function main(): void { int x = 1 + 2; Output.printLine("{x}"); }"#,
    ))
    .expect("disasm");
    assert!(out.contains("fn #"), "{out}");
    assert!(out.contains("main/0"), "{out}");
    assert!(out.contains("AddI"), "{out}");
    // `Output.printLine` lowers to `Op::CallNative`, annotated with the resolved native path.
    assert!(out.contains("CallNative"), "{out}");
    assert!(out.contains("Core.Output.printLine"), "{out}");
    // Const loads carry a `; <value>` annotation resolved from the pool.
    assert!(out.contains("Const(") && out.contains("; "), "{out}");
}

#[test]
fn explain_covers_shadow_import_code() {
    // The M3 Wave 1 shadowing diagnostic is self-documenting via `phg explain`.
    let body = explain_text("E-SHADOW-IMPORT").expect("E-SHADOW-IMPORT has an explanation");
    assert!(body.contains("module qualifier"), "{body}");
}

#[test]
fn explain_covers_totality_codes() {
    // The M-RT totality cluster diagnostics are self-documenting via `phg explain`.
    for code in [
        "E-MISSING-RETURN",
        "E-NEVER-RETURN",
        "W-UNREACHABLE",
        "W-MATCH-UNREACHABLE",
    ] {
        let body = explain_text(code).unwrap_or_else(|| panic!("{code} has an explanation"));
        assert!(body.starts_with(code), "{body}");
    }
}

#[test]
fn explain_covers_main_signature_code() {
    // Batch-1 B: the entry-point signature diagnostic self-documents via `phg explain`.
    let body = explain_text("E-MAIN-SIGNATURE").expect("E-MAIN-SIGNATURE has an explanation");
    assert!(body.starts_with("E-MAIN-SIGNATURE"), "{body}");
    assert!(body.contains("exit code"), "{body}");
}

#[test]
fn explain_covers_test_outside_tests_code() {
    // M-Test: the test-block placement diagnostic self-documents via `phg explain`.
    let body =
        explain_text("E-TEST-OUTSIDE-TESTS").expect("E-TEST-OUTSIDE-TESTS has an explanation");
    assert!(body.starts_with("E-TEST-OUTSIDE-TESTS"), "{body}");
    assert!(body.contains("phg test"), "{body}");
}

#[test]
fn explain_covers_member_visibility_codes() {
    // Wave 1.1 member-visibility diagnostics self-document via `phg explain`.
    for code in ["E-FIELD-VISIBILITY", "E-METHOD-VISIBILITY"] {
        let body = explain_text(code).unwrap_or_else(|| panic!("{code} has an explanation"));
        assert!(body.starts_with(code), "{body}");
    }
}

#[test]
fn explain_covers_struct_pattern_codes() {
    // The pattern-cluster S5.2 struct-destructuring diagnostics self-document via `phg explain`.
    for code in [
        "E-STRUCT-PAT-TYPE",
        "E-STRUCT-FIELD-UNKNOWN",
        "E-PATTERN-DUP-BIND",
    ] {
        let body = explain_text(code).unwrap_or_else(|| panic!("{code} has an explanation"));
        assert!(body.starts_with(code), "{body}");
    }
}

#[test]
fn explain_covers_destructuring_codes() {
    // The Phase 1 slice 5 let-destructuring diagnostics self-document via `phg explain`.
    for code in [
        "E-DESTRUCTURE-TYPE",
        "E-DESTRUCTURE-NOT-CLASS",
        "E-DESTRUCTURE-FIELD-UNKNOWN",
        "E-DESTRUCTURE-NOT-LIST",
        "E-DESTRUCTURE-NEEDS-ELSE",
        "E-DESTRUCTURE-ELSE-IRREFUTABLE",
        "E-DESTRUCTURE-ELSE-FALLTHROUGH",
        "E-DESTRUCTURE-DUP-BIND",
        "E-FIXEDLIST-DESTRUCTURE-LEN",
    ] {
        let body = explain_text(code).unwrap_or_else(|| panic!("{code} has an explanation"));
        assert!(body.starts_with(code), "{body}");
    }
}

#[test]
fn explain_covers_error_model_2a_codes() {
    // The M-faults Slice 2a diagnostics (`?` propagation + fault intrinsics) self-document.
    for code in [
        "E-PROPAGATE-POSITION",
        "E-PROPAGATE-CONTEXT",
        "E-PROPAGATE-ERR",
        "E-RESERVED-INTRINSIC",
        "E-INTRINSIC-LITERAL",
    ] {
        let body = explain_text(code).unwrap_or_else(|| panic!("{code} has an explanation"));
        assert!(body.starts_with(code), "{body}");
    }
}

#[test]
fn explain_covers_error_model_2b_codes() {
    // The M-faults 2b exception codes are self-documenting via `phg explain`.
    for code in [
        "E-THROW-TYPE",
        "E-THROW-UNDECLARED",
        "E-CALL-UNHANDLED",
        "E-UNCAUGHT-THROW",
        "E-THROWS-TOO-BROAD",
        "E-CATCH-TYPE",
        "W-CATCH-UNREACHABLE",
    ] {
        let body = explain_text(code).unwrap_or_else(|| panic!("{code} has an explanation"));
        assert!(body.starts_with(code), "{body}");
    }
}

#[test]
fn explain_covers_m5_package_codes() {
    // The M5 S1 package diagnostics are self-documenting via `phg explain`.
    let np = explain_text("E-NO-PACKAGE").expect("E-NO-PACKAGE has an explanation");
    assert!(np.contains("package Main"), "{np}");
    let rp = explain_text("E-RESERVED-PACKAGE").expect("E-RESERVED-PACKAGE has an explanation");
    assert!(rp.contains("standard library"), "{rp}");
}

#[test]
fn explain_covers_visibility_codes() {
    // The declaration-visibility diagnostics are self-documenting via `phg explain`.
    let p = explain_text("E-VIS-PRIVATE").expect("E-VIS-PRIVATE has an explanation");
    assert!(p.contains("`private`") && p.contains(".phg"), "{p}");
    let i = explain_text("E-VIS-INTERNAL").expect("E-VIS-INTERNAL has an explanation");
    assert!(i.contains("`internal`") && i.contains("package"), "{i}");
}

#[test]
fn explain_covers_s8_trait_codes() {
    // M-RT S8: the trait diagnostics are self-documenting via `phg explain`.
    let u = explain_text("E-USE-UNKNOWN").expect("E-USE-UNKNOWN has an explanation");
    assert!(u.contains("trait") && u.contains("extends"), "{u}");
    let t = explain_text("E-USE-AS-TYPE").expect("E-USE-AS-TYPE has an explanation");
    assert!(t.contains("NOT a type") && t.contains("instanceof"), "{t}");
    let cc = explain_text("E-TRAIT-CTOR-COLLISION").expect("E-TRAIT-CTOR-COLLISION explained");
    assert!(cc.contains("constructor") && cc.contains("collide"), "{cc}");
    let sh = explain_text("W-TRAIT-CTOR-SHADOWED").expect("W-TRAIT-CTOR-SHADOWED explained");
    assert!(sh.contains("shadow") || sh.contains("wins"), "{sh}");
    let ps =
        explain_text("W-TRAIT-CTOR-PARENT-SKIPPED").expect("W-TRAIT-CTOR-PARENT-SKIPPED explained");
    assert!(ps.contains("parent") && ps.contains("auto-run"), "{ps}");
}

#[test]
fn explain_covers_mi_field_conflict_code() {
    // The M-RT S6c.1 field-collision diagnostic is self-documenting via `phg explain`.
    let body = explain_text("E-MI-FIELD-CONFLICT").expect("E-MI-FIELD-CONFLICT has an explanation");
    assert!(
        body.contains("insteadof") && body.contains("redeclar"),
        "{body}"
    );
}

#[test]
fn explain_covers_lambda_this_code() {
    // E-LAMBDA-THIS now covers only the field-initializer case (a method-body lambda may capture
    // `this`); the explanation is self-documenting via `phg explain`.
    let body = explain_text("E-LAMBDA-THIS").expect("E-LAMBDA-THIS has an explanation");
    assert!(
        body.contains("`this`") && body.contains("initializer"),
        "{body}"
    );
}

#[test]
fn disasm_propagates_type_error() {
    // A program that fails the gate can't be disassembled — the type error surfaces instead.
    let err = cmd_disasm(&wp(r#"function main(): void { int x = "no"; }"#)).unwrap_err();
    assert!(err.contains("type error"), "{err}");
}

#[test]
fn bench_propagates_type_error_without_timing() {
    // A program that fails the gate can't be benchmarked — the error surfaces, no timing runs.
    let err = bench_report(&wp(r#"function main(): void { int x = "no"; }"#), 5).unwrap_err();
    assert!(err.contains("type error"), "{err}");
}

#[test]
fn bench_default_entry_uses_101_samples() {
    // The public entry runs the default-N path end to end (smoke test of `cmd_bench`).
    let out = cmd_bench(&wp(r#"import Core.Output;
function main(): void { Output.printLine("hi"); }"#))
    .expect("bench");
    assert!(out.starts_with("phg bench — median of 101"), "{out}");
}

#[test]
fn help_for_known_command_has_examples_and_name() {
    let h = help_for("run");
    assert!(h.contains("examples:"), "{h}");
    assert!(h.contains("phg run"), "{h}");
}

#[test]
fn help_for_unknown_command_falls_back_to_top_level() {
    assert_eq!(help_for("bogus"), help_text());
}

#[test]
fn var_transpiles_to_plain_php_assignment() {
    // `var` is erased; PHP locals are untyped, so it emits a bare `$x = …;`.
    let php = cmd_transpile(&wp(
        "import Core.Output; function main(): void { var x = 1; Output.printLine(\"{x}\"); }",
    ))
    .unwrap();
    assert!(php.contains("$x = 1;"), "{php}");
}

#[test]
fn type_alias_is_erased_in_php() {
    // The alias declaration vanishes and `Count` resolves to `int` in the emitted signature.
    let php = cmd_transpile(&wp(
        "type Count = int; function tally(Count n): Count { return n + 1; } function main(): void {}",
    ))
    .unwrap();
    assert!(!php.contains("Count"), "alias leaked into PHP:\n{php}");
    assert!(php.contains("function tally(int $n): int"), "{php}");
}

#[test]
fn explain_known_code_returns_paragraph_unknown_errors() {
    let ok = cmd_explain("E-UNKNOWN-IDENT").unwrap();
    assert!(ok.contains("E-UNKNOWN-IDENT"), "{ok}");
    assert!(ok.len() > 40, "explanation too short: {ok}");
    assert!(cmd_explain("E-NOPE").is_err());
}
