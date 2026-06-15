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
    assert!(run.status.success(), "php stderr: {}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "Hello Tak\narea = 12.56636\narea = 12\n"
    );
}
