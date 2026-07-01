//! Conformance corpus (GA rock 3) — a **golden-output** regression net for Phorj's *stable* surface.
//!
//! Each `conformance/**/*.phg` single-file program has a sibling `.out` holding its exact expected
//! stdout; a directory holding a `phorj.toml` is a multi-file project whose expected output is
//! `expected.out` in the project root. The test asserts the interpreter, the VM, **and** (when a `php`
//! is available) the transpiled PHP all produce *exactly* that golden output.
//!
//! This is strictly stronger than `tests/differential.rs`'s example gate, which only checks the three
//! backends *agree*: a regression where every backend drifts identically (a wrong-but-consistent
//! value) passes `agree` but fails here, because the golden pins the *value*. The corpus enumerates the
//! constructs listed as `stable` in `STABILITY.md`, so a stable-surface regression is caught loudly.
//!
//! PHP gating mirrors `tests/differential.rs`: `PHORJ_PHP=<path>` overrides the binary;
//! `PHORJ_REQUIRE_PHP=1` turns a missing `php` into a failure (CI) rather than a skip.

use phorj::cli::{cmd_run, cmd_runvm, cmd_transpile};
use phorj::{cli, loader};
use std::path::{Path, PathBuf};
use std::process::Command;

// ── php oracle (mirrors tests/differential.rs) ────────────────────────────────────────────────

fn php_bin() -> Option<String> {
    let cand = std::env::var("PHORJ_PHP").unwrap_or_else(|_| "php".to_string());
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
        std::env::var("PHORJ_REQUIRE_PHP").as_deref() != Ok("1"),
        "{label}: php required (PHORJ_REQUIRE_PHP=1) but not found on PATH or $PHORJ_PHP"
    );
    eprintln!("SKIP {label}: php not found — set PHORJ_REQUIRE_PHP=1 to make this a failure");
    None
}

fn php_n_args(php: &str) -> Vec<String> {
    let has_builtin = Command::new(php)
        .args(["-n", "-r", "exit(extension_loaded('bcmath') ? 0 : 1);"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if has_builtin {
        vec!["-n".to_string()]
    } else {
        [
            "-n",
            "-d",
            "display_errors=stderr",
            "-d",
            "extension=bcmath",
        ]
        .iter()
        .map(ToString::to_string)
        .collect()
    }
}

fn run_php(php: &str, php_src: &str, label: &str) -> String {
    let safe: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = std::env::temp_dir().join(format!("phorj_conf_{safe}.php"));
    std::fs::write(&path, php_src).expect("write temp php");
    let out = Command::new(php)
        .args(php_n_args(php))
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

// ── corpus discovery ──────────────────────────────────────────────────────────────────────────

/// Every single-file `*.phg` under `dir`, **skipping project roots** (a directory with a
/// `phorj.toml` is handled by the project gate). Mirrors the differential's structural exclusion.
fn collect_single(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.join("phorj.toml").is_file() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("conformance dir entry").path();
        if path.is_dir() {
            // `conformance/diagnostics/` is a *negative* corpus (programs that must FAIL, gated by
            // `tests/diagnostics.rs` against `.expected` files) — not golden-*output* programs.
            if path.file_name().and_then(|n| n.to_str()) == Some("diagnostics") {
                continue;
            }
            collect_single(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("phg") {
            out.push(path);
        }
    }
}

/// Every project root (a directory holding a `phorj.toml`) under `dir`.
fn collect_projects(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.join("phorj.toml").is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("conformance dir entry").path();
        if path.is_dir() {
            collect_projects(&path, out);
        }
    }
}

/// The `package Main` entry of a project: the file named `main.phg` under the project root.
fn find_main(project_dir: &Path) -> PathBuf {
    fn walk(dir: &Path) -> Option<PathBuf> {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        entries.sort();
        for p in &entries {
            if p.is_file() && p.file_name().and_then(|n| n.to_str()) == Some("main.phg") {
                return Some(p.clone());
            }
        }
        for p in &entries {
            if p.is_dir() {
                if let Some(found) = walk(p) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(project_dir)
        .unwrap_or_else(|| panic!("project {} has no main.phg entry", project_dir.display()))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ── gates ─────────────────────────────────────────────────────────────────────────────────────

/// Every single-file conformance program must produce its golden `.out` on the interpreter, the VM,
/// and real PHP. Glob-based discovery: a program added later is gated with no test edit.
#[test]
fn conformance_single_file_golden() {
    let mut files = Vec::new();
    collect_single(Path::new("conformance"), &mut files);
    files.sort();
    assert!(
        files.len() >= 10,
        "conformance corpus unexpectedly small ({} programs) — did the directory move?",
        files.len()
    );
    for path in &files {
        let label = path.display().to_string();
        let src = read(path);
        let out_path = path.with_extension("out");
        let expected = std::fs::read_to_string(&out_path)
            .unwrap_or_else(|e| panic!("missing golden output {}: {e}", out_path.display()));
        let run = cmd_run(&src).unwrap_or_else(|e| panic!("{label}: run errored: {e}"));
        assert_eq!(run, expected, "interpreter ≠ golden for {label}");
        let runvm = cmd_runvm(&src).unwrap_or_else(|e| panic!("{label}: runvm errored: {e}"));
        assert_eq!(runvm, expected, "VM ≠ golden for {label}");
        if let Some(php) = php_or_gate(&label) {
            let php_src = cmd_transpile(&src).expect("transpile ok");
            let got = run_php(&php, &php_src, &label);
            assert_eq!(got, expected, "PHP ≠ golden for {label}");
        }
    }
}

/// Every multi-file conformance **project** must produce its `expected.out` golden on the interpreter,
/// the VM, and real PHP (assembled through `loader::load`).
#[test]
fn conformance_projects_golden() {
    let mut projects = Vec::new();
    collect_projects(Path::new("conformance"), &mut projects);
    projects.sort();
    assert!(
        !projects.is_empty(),
        "expected at least the flagship conformance project under conformance/, found none"
    );
    for project in &projects {
        let label = project.display().to_string();
        let entry = find_main(project);
        let unit =
            loader::load(&entry).unwrap_or_else(|e| panic!("{label}: project must load: {e}"));
        let expected = read(&project.join("expected.out"));
        let run = cli::run_program(&unit).unwrap_or_else(|e| panic!("{label}: run errored: {e}"));
        assert_eq!(run, expected, "interpreter ≠ golden for project {label}");
        let runvm =
            cli::runvm_program(&unit).unwrap_or_else(|e| panic!("{label}: runvm errored: {e}"));
        assert_eq!(runvm, expected, "VM ≠ golden for project {label}");
        if let Some(php) = php_or_gate(&label) {
            let php_src = cli::transpile_program(&unit.program, &unit.diag_src)
                .expect("transpile project ok");
            let got = run_php(&php, &php_src, &label);
            assert_eq!(got, expected, "PHP ≠ golden for project {label}");
        }
    }
}
