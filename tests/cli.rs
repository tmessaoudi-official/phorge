use std::process::Command;

/// Path to the compiled `phorj` binary (Cargo sets this for integration tests).
const BIN: &str = env!("CARGO_BIN_EXE_phg");

#[test]
fn run_sample_fixture_prints_expected_output() {
    let out = Command::new(BIN)
        .args(["run", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn phorj");
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "Hello Tak\narea = 12.56636\narea = 12\n"
    );
}

#[test]
fn no_arguments_is_usage_error_exit_2() {
    let out = Command::new(BIN).output().expect("spawn phorj");
    assert_eq!(out.status.code(), Some(2));
}

/// M-DX S3: `phg run --dump-on-fault` appends the faulting frame's locals (secure + deterministic)
/// to the stderr fault render; without the flag it does not. A `Secret` local is redacted.
#[test]
fn dump_on_fault_shows_redacted_locals_only_when_requested() {
    let fixture = "tests/fixtures/dump_fault.phg";

    // Without the flag: fault + backtrace, but no locals dump.
    let plain = Command::new(BIN)
        .args(["run", fixture])
        .output()
        .expect("spawn phorj");
    let plain_err = String::from_utf8_lossy(&plain.stderr);
    assert!(plain_err.contains("list index out of range"), "{plain_err}");
    assert!(
        !plain_err.contains("faulting frame locals"),
        "no dump without the flag: {plain_err}"
    );

    // With the flag: the locals section appears, the Secret is redacted, and no plaintext leaks.
    let dumped = Command::new(BIN)
        .args(["run", "--dump-on-fault", fixture])
        .output()
        .expect("spawn phorj");
    let err = String::from_utf8_lossy(&dumped.stderr);
    assert!(
        err.contains("faulting frame locals:"),
        "dump present: {err}"
    );
    assert!(err.contains("doubled = 10"), "shows a local: {err}");
    assert!(err.contains("xs = [10, 20]"), "shows a list local: {err}");
    assert!(
        err.contains("token = Secret(<redacted>)"),
        "secret redacted: {err}"
    );
    assert!(
        !err.contains("hunter2"),
        "secret plaintext MUST NOT leak: {err}"
    );
    // The stderr dump never bleeds into stdout (side-channel only).
    assert!(
        String::from_utf8_lossy(&dumped.stdout).is_empty(),
        "dump stays off stdout"
    );
}

/// M-DX S3: the VM backend has no named locals, so `runvm --dump-on-fault` emits the (byte-identical)
/// backtrace without a locals section — never slot noise, never a leak.
#[test]
fn dump_on_fault_vm_emits_backtrace_without_locals() {
    let out = Command::new(BIN)
        .args(["runvm", "--dump-on-fault", "tests/fixtures/dump_fault.phg"])
        .output()
        .expect("spawn phorj");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("list index out of range"), "{err}");
    assert!(err.contains("stack trace"), "backtrace present: {err}");
    assert!(
        !err.contains("faulting frame locals"),
        "VM has no named locals: {err}"
    );
    assert!(!err.contains("hunter2"), "no leak: {err}");
}

#[test]
fn version_flag_prints_version_exit_0() {
    for flag in ["--version", "-v"] {
        let out = Command::new(BIN).arg(flag).output().expect("spawn phorj");
        assert!(out.status.success(), "{flag} exit {:?}", out.status.code());
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(s.starts_with("phg "), "{flag} stdout: {s}");
        assert!(
            s.trim().ends_with(env!("CARGO_PKG_VERSION")),
            "{flag} stdout: {s}"
        );
    }
}

#[test]
fn help_flag_prints_usage_exit_0() {
    for flag in ["--help", "-h"] {
        let out = Command::new(BIN).arg(flag).output().expect("spawn phorj");
        assert!(out.status.success(), "{flag} exit {:?}", out.status.code());
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(s.contains("usage:"), "{flag} stdout: {s}");
        assert!(
            s.contains("run") && s.contains("build"),
            "{flag} stdout: {s}"
        );
    }
}

#[test]
fn missing_file_is_error_exit_1() {
    let out = Command::new(BIN)
        .args(["run", "tests/fixtures/does_not_exist.phg"])
        .output()
        .expect("spawn phorj");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn check_clean_fixture_exits_0() {
    let out = Command::new(BIN)
        .args(["check", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn phorj");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("OK"));
}

#[test]
fn check_json_clean_emits_empty_array_exit_0() {
    let out = Command::new(BIN)
        .args(["check", "--json", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn phg");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "[]\n");
}

#[test]
fn check_json_error_emits_diagnostic_array_exit_1_no_stderr() {
    let out = Command::new(BIN)
        .args([
            "check",
            "--json",
            "-e",
            "package Main; function main()-> void { var x = nope; }",
        ])
        .output()
        .expect("spawn phg");
    // Errors → exit 1, but the JSON array is on stdout (parseable) and nothing on stderr.
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with('['), "{stdout}");
    assert!(stdout.contains("\"severity\":\"error\""), "{stdout}");
    assert!(stdout.contains("\"code\":\"E-UNKNOWN-IDENT\""), "{stdout}");
    assert!(
        String::from_utf8_lossy(&out.stderr).is_empty(),
        "stderr should be empty in --json mode"
    );
}

#[test]
fn transpile_sample_exits_0_with_php() {
    let out = Command::new(BIN)
        .args(["transpile", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn phorj");
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("<?php"));
}

/// The committed `examples/transpile/demo.php` must equal freshly-generated output, so transpiler
/// drift fails the suite (regenerate with `phg transpile examples/transpile/demo.phg > …`).
#[test]
fn transpile_demo_matches_committed_php() {
    let expected =
        std::fs::read_to_string("examples/transpile/demo.php").expect("read committed demo.php");
    let out = Command::new(BIN)
        .args(["transpile", "examples/transpile/demo.phg"])
        .output()
        .expect("spawn phorj");
    assert!(
        out.status.success(),
        "transpile exit {:?}",
        out.status.code()
    );
    let actual = String::from_utf8(out.stdout).expect("utf-8 php");
    assert_eq!(
        actual, expected,
        "generated PHP drifted from examples/transpile/demo.php — regenerate it"
    );
}

#[test]
fn run_reads_program_from_stdin() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(BIN)
        .args(["run", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn phorj");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            br#"package Main;
import Core.Output;
function main() -> void { Output.printLine("{1 + 2}"); }"#,
        )
        .unwrap();
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
}

#[test]
fn run_eval_inline_code() {
    for flag in ["-e", "--eval"] {
        let out = Command::new(BIN)
            .args([
                "run",
                flag,
                r#"package Main;
import Core.Output;
function main() -> void { Output.printLine("{2 * 3}"); }"#,
            ])
            .output()
            .expect("spawn phorj");
        assert!(out.status.success(), "{flag} exit {:?}", out.status.code());
        assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n");
    }
}

#[test]
fn run_double_dash_then_path_is_a_file() {
    let path = write_temp(
        "dashdash",
        r#"package Main;
import Core.Output;
function main() -> void { Output.printLine("ok"); }"#,
    );
    let out = Command::new(BIN)
        .args(["run", "--", path.to_str().unwrap()])
        .output()
        .expect("spawn phorj");
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n");
}

/// Write `src` to a uniquely-named temp file so parallel tests never collide.
fn write_temp(name: &str, src: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("phorj_cli_{name}.phg"));
    std::fs::write(&path, src).expect("write temp fixture");
    path
}

#[test]
fn parse_subcommand_dumps_ast_exit_0() {
    let out = Command::new(BIN)
        .args(["parse", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn phorj");
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Program"));
}

#[test]
fn lex_subcommand_dumps_tokens_exit_0() {
    let out = Command::new(BIN)
        .args(["tokenize", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn phorj");
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert!(String::from_utf8_lossy(&out.stdout).contains("@ 1:1"));
}

#[test]
fn transpile_ill_typed_exits_1_with_type_error() {
    let path = write_temp(
        "ill_typed",
        r#"package Main; function main() -> void { int x = "no"; }"#,
    );
    let out = Command::new(BIN)
        .args(["transpile", path.to_str().unwrap()])
        .output()
        .expect("spawn phorj");
    let _ = std::fs::remove_file(&path);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("type error"));
}

#[test]
fn run_runtime_error_exits_1() {
    let path = write_temp(
        "runtime_err",
        r#"package Main;
import Core.Output;
function main() -> void { Output.printLine("{1 / 0}"); }"#,
    );
    let out = Command::new(BIN)
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn phorj");
    let _ = std::fs::remove_file(&path);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("runtime error"));
}

#[test]
fn runvm_simple_program_exits_0() {
    let path = write_temp(
        "runvm_ok",
        r#"package Main;
import Core.Output;
function main() -> void { Output.printLine("{1 + 1}"); }"#,
    );
    let out = Command::new(BIN)
        .args(["runvm", path.to_str().unwrap()])
        .output()
        .expect("spawn phorj");
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
}

#[test]
fn runvm_runtime_error_exits_1() {
    let path = write_temp(
        "runvm_rt",
        r#"package Main;
import Core.Output;
function main() -> void { Output.printLine("{1 / 0}"); }"#,
    );
    let out = Command::new(BIN)
        .args(["runvm", path.to_str().unwrap()])
        .output()
        .expect("spawn phorj");
    let _ = std::fs::remove_file(&path);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("runtime error"));
}

#[test]
fn per_command_help_prints_examples_exit_0() {
    for cmd in [
        "run",
        "runvm",
        "check",
        "parse",
        "tokenize",
        "transpile",
        "disassemble",
        "benchmark",
        "build",
    ] {
        let out = Command::new(BIN)
            .args([cmd, "--help"])
            .output()
            .expect("spawn phorj");
        assert!(
            out.status.success(),
            "{cmd} --help exit {:?}",
            out.status.code()
        );
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(
            s.contains("examples:"),
            "{cmd} --help missing examples:\n{s}"
        );
        assert!(
            s.contains(cmd),
            "{cmd} --help missing the command name:\n{s}"
        );
    }
}

#[test]
fn explain_subcommand_known_and_unknown() {
    let ok = Command::new(BIN)
        .args(["explain", "E-UNKNOWN-IDENT"])
        .output()
        .expect("spawn phorj");
    assert!(ok.status.success(), "explain exit {:?}", ok.status.code());
    assert!(String::from_utf8_lossy(&ok.stdout).contains("E-UNKNOWN-IDENT"));

    let bad = Command::new(BIN)
        .args(["explain", "E-NOPE"])
        .output()
        .expect("spawn phorj");
    assert_eq!(bad.status.code(), Some(1));
}

/// B2 — a multiple-inheritance super-method call (`parent(A).m(…)`) transpiles to a `private` trait
/// alias (PHP has no native `parent::`/`A::` in an MI class). The run≡runvm≡real-PHP byte-identity is
/// gated by `examples/guide/parent-dispatch-mi.phg`; this locks the *shape* of the emitted PHP.
#[test]
fn mi_super_method_transpiles_to_a_trait_alias() {
    let path = std::env::temp_dir().join("phg_b2_mi_super.phg");
    std::fs::write(
        &path,
        "package Main;\n\
         import Core.Output;\n\
         open class A { open function m(): string { return \"A\"; } }\n\
         open class B { open function m(): string { return \"B\"; } }\n\
         class C extends A, B { function m(): string { return \"{parent(A).m()}+{parent(B).m()}+C\"; } }\n\
         function main(): void { C c = new C(); Output.printLine(c.m()); }\n",
    )
    .unwrap();
    let out = Command::new(BIN)
        .args(["transpile", path.to_str().unwrap()])
        .output()
        .expect("spawn phorj");
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let php = String::from_utf8_lossy(&out.stdout);
    assert!(php.contains("TA::m as private __super_A_m;"), "{php}");
    assert!(php.contains("TB::m as private __super_B_m;"), "{php}");
    assert!(php.contains("$this->__super_A_m()"), "{php}");
    assert!(php.contains("$this->__super_B_m()"), "{php}");
}

/// B2 — a parent-method jump to a *non-direct* ancestor under multiple inheritance is not yet lowerable
/// to PHP (the trait alias requires a directly-`use`d ancestor); it is a clean transpile error, not
/// invalid PHP. The `run`/`runvm` backends still handle it (resolution is MI-aware).
#[test]
fn mi_transitive_parent_jump_is_a_clean_transpile_error() {
    let path = std::env::temp_dir().join("phg_b2_mi_transitive.phg");
    std::fs::write(
        &path,
        "package Main;\n\
         import Core.Output;\n\
         open class G { open function m(): string { return \"G\"; } }\n\
         open class A extends G { open function m(): string { return \"A\"; } }\n\
         open class B { open function m(): string { return \"B\"; } }\n\
         class C extends A, B { function m(): string { return \"{parent(G).m()}+C\"; } }\n\
         function main(): void { C c = new C(); Output.printLine(c.m()); }\n",
    )
    .unwrap();
    // run still works (MI-aware resolution).
    let run = Command::new(BIN)
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn phorj");
    assert!(run.status.success(), "run should still work under MI");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "G+C\n");
    // transpile is a clean error (exit 1), not broken PHP.
    let tr = Command::new(BIN)
        .args(["transpile", path.to_str().unwrap()])
        .output()
        .expect("spawn phorj");
    let _ = std::fs::remove_file(&path);
    assert_eq!(tr.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&tr.stderr).contains("non-direct ancestor"),
        "stderr: {}",
        String::from_utf8_lossy(&tr.stderr)
    );
}

/// Regression: every inline-source command that builds a `BytecodeProgram` (`disasm`, `bench`) must
/// thread the checker's reified-operand side-table into the VM compile (`check_and_expand_reified` +
/// `compile_with`), exactly like `runvm`. The concurrency example uses `a.join() + b.join()` — a
/// method result as an arithmetic operand — which the VM compiler's `ctype` can only resolve from
/// that side-table; with the old map-dropping `compile` path these commands rejected it with
/// "no method `join` on `Task`" while `run`/`runvm` accepted it (the same root cause that broke the
/// playground's runvm). Guards all three surfaces against re-diverging.
#[test]
fn disasm_and_bench_accept_reified_operand_program() {
    let ex = "examples/guide/concurrency.phg";
    for cmd in ["disassemble", "benchmark"] {
        let out = Command::new(BIN)
            .args([cmd, ex])
            .output()
            .expect("spawn phorj");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "`{cmd}` must accept a reified-operand program; exit {:?}\nstdout: {stdout}\nstderr: {stderr}",
            out.status.code()
        );
        assert!(
            !stdout.contains("no method") && !stderr.contains("no method"),
            "`{cmd}` must not reject `Task.join()` used as an operand\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}
