//! M5 S2b integration: a multi-file project loads through `loader::load`, merges flat, and runs
//! byte-identically on both backends. Cross-file calls resolve *unqualified* in S2b (the items
//! share one flat namespace until S2c adds qualified resolution + namespaced PHP).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use phorge::{cli, loader};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let unique = N.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("phorge_project_it_{}_{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, contents).unwrap();
        p
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_both(entry: &Path) -> (String, String) {
    let unit = loader::load(entry).expect("project loads");
    let run = cli::run_program(&unit.program, &unit.diag_src).expect("interpreter runs");
    let runvm = cli::runvm_program(&unit.program, &unit.diag_src).expect("vm runs");
    (run, runvm)
}

#[test]
fn multi_file_project_merges_and_runs_byte_identically() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "name = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package main;\nimport core.console;\n\
         function main() {\n    console.println(\"{compute(20)}\");\n}",
    );
    // A library file in its folder=path location; its function is called unqualified after merge.
    tmp.write(
        "src/acme/util/compute.phg",
        "package acme.util;\nfunction compute(int n) -> int {\n    return n + n + 2;\n}",
    );

    let (run, runvm) = run_both(&entry);
    assert_eq!(run, "42\n");
    assert_eq!(run, runvm, "run and runvm must be byte-identical");
}

#[test]
fn folder_path_violation_is_reported() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "name = \"acme/app\"");
    let entry = tmp.write("src/main.phg", "package main;\nfunction main() {}");
    tmp.write("src/acme/util/x.phg", "package acme.bad;\nfunction x() {}");
    let err = loader::load(&entry).unwrap_err();
    assert!(err.contains("E-PKG-PATH"), "got: {err}");
}

#[test]
fn loose_non_main_file_is_rejected() {
    let tmp = TempDir::new();
    // No phorge.toml anywhere above → loose mode; a dotted package is illegal.
    let entry = tmp.write("script.phg", "package app.util;\nfunction f() {}");
    let err = loader::load(&entry).unwrap_err();
    assert!(err.contains("requires a phorge.toml project"), "got: {err}");
}
