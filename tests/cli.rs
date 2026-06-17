use std::process::Command;

/// Path to the compiled `phorge` binary (Cargo sets this for integration tests).
const BIN: &str = env!("CARGO_BIN_EXE_phorge");

#[test]
fn run_sample_fixture_prints_expected_output() {
    let out = Command::new(BIN)
        .args(["run", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn phorge");
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "Hello Tak\narea = 12.56636\narea = 12\n"
    );
}

#[test]
fn no_arguments_is_usage_error_exit_2() {
    let out = Command::new(BIN).output().expect("spawn phorge");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn version_flag_prints_version_exit_0() {
    for flag in ["--version", "-v"] {
        let out = Command::new(BIN).arg(flag).output().expect("spawn phorge");
        assert!(out.status.success(), "{flag} exit {:?}", out.status.code());
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(s.starts_with("phorge "), "{flag} stdout: {s}");
        assert!(
            s.trim().ends_with(env!("CARGO_PKG_VERSION")),
            "{flag} stdout: {s}"
        );
    }
}

#[test]
fn help_flag_prints_usage_exit_0() {
    for flag in ["--help", "-h"] {
        let out = Command::new(BIN).arg(flag).output().expect("spawn phorge");
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
        .expect("spawn phorge");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn check_clean_fixture_exits_0() {
    let out = Command::new(BIN)
        .args(["check", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn phorge");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("OK"));
}

#[test]
fn transpile_sample_exits_0_with_php() {
    let out = Command::new(BIN)
        .args(["transpile", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn phorge");
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("<?php"));
}

/// The committed `examples/transpile/demo.php` must equal freshly-generated output, so transpiler
/// drift fails the suite (regenerate with `phorge transpile examples/transpile/demo.phg > …`).
#[test]
fn transpile_demo_matches_committed_php() {
    let expected =
        std::fs::read_to_string("examples/transpile/demo.php").expect("read committed demo.php");
    let out = Command::new(BIN)
        .args(["transpile", "examples/transpile/demo.phg"])
        .output()
        .expect("spawn phorge");
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

/// The strongest correctness signal: the emitted PHP, run by a real `php`, prints exactly
/// what the interpreter prints. Self-skips (passes) if `php` is not on PATH.
#[test]
fn transpiled_php_runs_and_matches_interpreter() {
    let have_php = Command::new("php")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !have_php {
        eprintln!("skipping round-trip: php not on PATH");
        return;
    }
    let php = Command::new(BIN)
        .args(["transpile", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn transpile");
    assert!(php.status.success());

    let dir = std::env::temp_dir().join("phorge_rt");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sample.php");
    std::fs::write(&path, &php.stdout).unwrap();

    let run = Command::new("php").arg(&path).output().expect("spawn php");
    let _ = std::fs::remove_file(&path);
    assert!(
        run.status.success(),
        "php stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "Hello Tak\narea = 12.56636\narea = 12\n"
    );
}

/// `?.` transpiles to PHP's nullsafe `?->` and behaves identically once run by real PHP.
/// Uses a getter (not a direct private-field read): PHP enforces `private` visibility while
/// Phorge's own backends do not. Self-skips (passes) if `php` is not on PATH.
#[test]
fn safe_access_transpiles_and_runs_in_php() {
    let have_php = Command::new("php")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !have_php {
        eprintln!("skipping ?. round-trip: php not on PATH");
        return;
    }
    let src = "import std.io;\n\
               class Box {\n\
               \x20   constructor(private int v) {}\n\
               \x20   function v_of() -> int { return v; }\n\
               }\n\
               function main() {\n\
               \x20   Box? a = null;\n\
               \x20   println(\"{(a?.v_of()) ?? -1}\");\n\
               \x20   Box? b = Box(7);\n\
               \x20   println(\"{(b?.v_of()) ?? -1}\");\n\
               }\n";
    let dir = std::env::temp_dir().join("phorge_rt_safe");
    std::fs::create_dir_all(&dir).unwrap();
    let phg = dir.join("safe.phg");
    std::fs::write(&phg, src).unwrap();
    let php = Command::new(BIN)
        .arg("transpile")
        .arg(&phg)
        .output()
        .expect("spawn transpile");
    assert!(
        php.status.success(),
        "transpile stderr: {}",
        String::from_utf8_lossy(&php.stderr)
    );
    let php_path = dir.join("safe.php");
    std::fs::write(&php_path, &php.stdout).unwrap();
    let run = Command::new("php")
        .arg(&php_path)
        .output()
        .expect("spawn php");
    let _ = std::fs::remove_file(&phg);
    let _ = std::fs::remove_file(&php_path);
    assert!(
        run.status.success(),
        "php stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "-1\n7\n");
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
        .expect("spawn phorge");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"function main() { println("{1 + 2}"); }"#)
        .unwrap();
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
}

#[test]
fn run_eval_inline_code() {
    for flag in ["-e", "--eval"] {
        let out = Command::new(BIN)
            .args(["run", flag, r#"function main() { println("{2 * 3}"); }"#])
            .output()
            .expect("spawn phorge");
        assert!(out.status.success(), "{flag} exit {:?}", out.status.code());
        assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n");
    }
}

#[test]
fn run_double_dash_then_path_is_a_file() {
    let path = write_temp("dashdash", r#"function main() { println("ok"); }"#);
    let out = Command::new(BIN)
        .args(["run", "--", path.to_str().unwrap()])
        .output()
        .expect("spawn phorge");
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n");
}

/// Write `src` to a uniquely-named temp file so parallel tests never collide.
fn write_temp(name: &str, src: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("phorge_cli_{name}.phg"));
    std::fs::write(&path, src).expect("write temp fixture");
    path
}

#[test]
fn parse_subcommand_dumps_ast_exit_0() {
    let out = Command::new(BIN)
        .args(["parse", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn phorge");
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Program"));
}

#[test]
fn lex_subcommand_dumps_tokens_exit_0() {
    let out = Command::new(BIN)
        .args(["lex", "tests/fixtures/sample.phg"])
        .output()
        .expect("spawn phorge");
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert!(String::from_utf8_lossy(&out.stdout).contains("@ 1:1"));
}

#[test]
fn transpile_ill_typed_exits_1_with_type_error() {
    let path = write_temp("ill_typed", r#"function main() { int x = "no"; }"#);
    let out = Command::new(BIN)
        .args(["transpile", path.to_str().unwrap()])
        .output()
        .expect("spawn phorge");
    let _ = std::fs::remove_file(&path);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("type error"));
}

#[test]
fn run_runtime_error_exits_1() {
    let path = write_temp("runtime_err", r#"function main() { println("{1 / 0}"); }"#);
    let out = Command::new(BIN)
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn phorge");
    let _ = std::fs::remove_file(&path);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("runtime error"));
}

#[test]
fn runvm_simple_program_exits_0() {
    let path = write_temp("runvm_ok", r#"function main() { println("{1 + 1}"); }"#);
    let out = Command::new(BIN)
        .args(["runvm", path.to_str().unwrap()])
        .output()
        .expect("spawn phorge");
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
}

#[test]
fn runvm_runtime_error_exits_1() {
    let path = write_temp("runvm_rt", r#"function main() { println("{1 / 0}"); }"#);
    let out = Command::new(BIN)
        .args(["runvm", path.to_str().unwrap()])
        .output()
        .expect("spawn phorge");
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
            .expect("spawn phorge");
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
        .expect("spawn phorge");
    assert!(ok.status.success(), "explain exit {:?}", ok.status.code());
    assert!(String::from_utf8_lossy(&ok.stdout).contains("E-UNKNOWN-IDENT"));

    let bad = Command::new(BIN)
        .args(["explain", "E-NOPE"])
        .output()
        .expect("spawn phorge");
    assert_eq!(bad.status.code(), Some(1));
}
