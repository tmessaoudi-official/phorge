//! M8.5 foreign-PHP interop harness — the PHP-target-only counterpart to `tests/differential.rs`.
//!
//! A program that uses `declare` (foreign PHP symbols) cannot run on the Rust backends (it has no PHP
//! runtime), so it is **quarantined** from the byte-identity oracle. Instead each `examples/interop/**.phg`
//! is validated two ways:
//!   1. **`run`/`runvm` refuse it** with the `E-FOREIGN-RUNTIME` pre-flight gate (foreign code needs PHP).
//!   2. **`transpile` → real PHP → golden** — the transpiled PHP runs under a real `php` and must match
//!      the committed sibling `.out` exactly.
//!
//! PHP gating mirrors `tests/conformance.rs`: `PHORGE_PHP=<path>` overrides the binary;
//! `PHORGE_REQUIRE_PHP=1` turns a missing `php` into a failure (CI) rather than a skip. The refuse-gate
//! check runs regardless of `php` availability.

use phorge::cli;
use std::path::{Path, PathBuf};
use std::process::Command;

fn php_bin() -> Option<String> {
    let cand = std::env::var("PHORGE_PHP").unwrap_or_else(|_| "php".to_string());
    let ok = Command::new(&cand)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    ok.then_some(cand)
}

fn php_or_gate(label: &str) -> Option<String> {
    if let Some(p) = php_bin() {
        return Some(p);
    }
    assert!(
        std::env::var("PHORGE_REQUIRE_PHP").as_deref() != Ok("1"),
        "{label}: php required (PHORGE_REQUIRE_PHP=1) but not found on PATH or $PHORGE_PHP"
    );
    eprintln!("SKIP {label}: php not found — set PHORGE_REQUIRE_PHP=1 to make this a failure");
    None
}

fn run_php(php: &str, php_src: &str, label: &str) -> String {
    let safe: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = std::env::temp_dir().join(format!("phorge_interop_{safe}.php"));
    std::fs::write(&path, php_src).expect("write temp php");
    let out = Command::new(php)
        .args(["-n"])
        .arg(&path)
        .output()
        .expect("spawn php");
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "php exited non-zero for {label}:\n{}\n--- transpiled php ---\n{php_src}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 php stdout")
}

/// Collect single-file interop walkthroughs, **skipping project roots** (a dir with `phorge.toml`):
/// a project's files import each other / share ambient `.d.phg` declarations and only resolve when
/// assembled via `loader::load`, so they are gated by `interop_projects_*` instead.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.join("phorge.toml").is_file() {
        return;
    }
    for entry in std::fs::read_dir(dir).expect("read_dir examples/interop") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("phg") {
            out.push(path);
        }
    }
}

/// Recursively collect every project root (a dir holding `phorge.toml`) under `dir`.
fn collect_projects(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.join("phorge.toml").is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    for entry in std::fs::read_dir(dir).expect("read_dir examples/interop") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_projects(&path, out);
        }
    }
}

#[test]
fn interop_examples_refuse_to_run_and_match_php_golden() {
    let mut files = Vec::new();
    collect(Path::new("examples/interop"), &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "expected interop examples under examples/interop"
    );
    let php = php_or_gate("interop");

    for phg in &files {
        let label = phg.display().to_string();
        let src = std::fs::read_to_string(phg).expect("read .phg");

        // 1. `check` must pass — the foreign signatures type-check the calls.
        cli::cmd_check(&src).unwrap_or_else(|e| panic!("{label}: check failed:\n{e}"));

        // 2. The Rust backends must REFUSE it (foreign code needs the PHP runtime).
        let run_err = cli::cmd_run(&src).expect_err("run must refuse a foreign program");
        assert!(
            run_err.contains("E-FOREIGN-RUNTIME"),
            "{label}: run should fail with E-FOREIGN-RUNTIME, got:\n{run_err}"
        );
        let vm_err = cli::cmd_runvm(&src).expect_err("runvm must refuse a foreign program");
        assert!(
            vm_err.contains("E-FOREIGN-RUNTIME"),
            "{label}: runvm should fail with E-FOREIGN-RUNTIME, got:\n{vm_err}"
        );

        // 3. transpile → real PHP → golden (the sibling .out), when a php is available.
        if let Some(php) = &php {
            let php_src = cli::cmd_transpile(&src).expect("transpile ok");
            let got = run_php(php, &php_src, &label);
            let golden_path = phg.with_extension("out");
            let want = std::fs::read_to_string(&golden_path)
                .unwrap_or_else(|_| panic!("missing golden {}", golden_path.display()));
            assert_eq!(got, want, "{label}: transpiled-PHP output != golden");
        }
    }
}

/// M8.5 S3b — multi-file interop projects (a `*.d.phg` declaration file shared across packages). Each
/// project under `examples/interop/` is assembled via `loader::load`, must be refused by the Rust
/// backends (`E-FOREIGN-RUNTIME`), and its transpiled PHP must match the committed `expected.out`.
#[test]
fn interop_projects_refuse_to_run_and_match_php_golden() {
    use phorge::loader;
    let mut projects = Vec::new();
    collect_projects(Path::new("examples/interop"), &mut projects);
    projects.sort();
    if projects.is_empty() {
        return; // no interop projects yet — single-file test still covers the core
    }
    let php = php_or_gate("interop-projects");

    for proj in &projects {
        let label = proj.display().to_string();
        let entry = proj.join("src/main.phg");
        let unit = loader::load(&entry).unwrap_or_else(|e| panic!("{label}: load failed:\n{e}"));

        // 1. check passes (foreign signatures type-check the calls).
        cli::check_program(&unit.program, &unit.diag_src)
            .unwrap_or_else(|e| panic!("{label}: check failed:\n{e}"));

        // 2. Both Rust backends REFUSE it (foreign code needs the PHP runtime).
        let run_err = cli::run_program(&unit).expect_err("run must refuse a foreign project");
        assert!(
            run_err.contains("E-FOREIGN-RUNTIME"),
            "{label}: run should fail with E-FOREIGN-RUNTIME, got:\n{run_err}"
        );
        let vm_err = cli::runvm_program(&unit).expect_err("runvm must refuse a foreign project");
        assert!(
            vm_err.contains("E-FOREIGN-RUNTIME"),
            "{label}: runvm should fail with E-FOREIGN-RUNTIME, got:\n{vm_err}"
        );

        // 3. transpile → real PHP → golden (project-root `expected.out`).
        if let Some(php) = &php {
            let php_src =
                cli::transpile_program(&unit.program, &unit.diag_src).expect("transpile ok");
            let got = run_php(php, &php_src, &label);
            let golden_path = proj.join("expected.out");
            let want = std::fs::read_to_string(&golden_path)
                .unwrap_or_else(|_| panic!("missing golden {}", golden_path.display()));
            assert_eq!(got, want, "{label}: transpiled-PHP output != golden");
        }
    }
}
