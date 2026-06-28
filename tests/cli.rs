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
import Core.Console;
function main() -> void { Console.println("{1 + 2}"); }"#,
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
import Core.Console;
function main() -> void { Console.println("{2 * 3}"); }"#,
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
import Core.Console;
function main() -> void { Console.println("ok"); }"#,
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
        .args(["lex", "tests/fixtures/sample.phg"])
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
import Core.Console;
function main() -> void { Console.println("{1 / 0}"); }"#,
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
import Core.Console;
function main() -> void { Console.println("{1 + 1}"); }"#,
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
import Core.Console;
function main() -> void { Console.println("{1 / 0}"); }"#,
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
        "lex",
        "transpile",
        "disasm",
        "bench",
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
