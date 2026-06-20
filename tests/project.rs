//! M5 S2b/S2c integration: a multi-file project loads through `loader::load`, resolves cross-package
//! calls, and runs byte-identically on both backends. S2c qualifies cross-package calls
//! (`util.compute(x)` via an import leaf or alias), tightens the S2b bare-call interim (unqualified
//! cross-package calls now fail), rejects library-package types (`E-PKG-TYPE`), and transpiles to
//! one PHP `namespace` brace-block per package.

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
fn multi_file_project_qualified_call_runs_byte_identically() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    // S2c: cross-package calls are *qualified* via an import leaf (`util.compute`), no longer the
    // S2b bare form. The loader resolves it against the imported package's mangled symbol.
    let entry = tmp.write(
        "src/main.phg",
        "package main;\nimport core.console;\nimport acme.util;\n\
         function main() {\n    console.println(\"{util.compute(20)}\");\n}",
    );
    tmp.write(
        "src/acme/util/compute.phg",
        "package acme.util;\nfunction compute(int n) -> int {\n    return n + n + 2;\n}",
    );

    let (run, runvm) = run_both(&entry);
    assert_eq!(run, "42\n");
    assert_eq!(run, runvm, "run and runvm must be byte-identical");
}

#[test]
fn import_alias_resolves_qualified_call() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
    // `import acme.util as u;` binds the leaf `u`; the call qualifies on the alias.
    let entry = tmp.write(
        "src/main.phg",
        "package main;\nimport core.console;\nimport acme.util as u;\n\
         function main() {\n    console.println(\"{u.compute(20)}\");\n}",
    );
    tmp.write(
        "src/acme/util/compute.phg",
        "package acme.util;\nfunction compute(int n) -> int {\n    return n + n + 2;\n}",
    );

    let (run, runvm) = run_both(&entry);
    assert_eq!(run, "42\n");
    assert_eq!(run, runvm, "run and runvm must be byte-identical");
}

#[test]
fn same_package_cross_file_bare_call_resolves() {
    // Two files in the *same* library package: one calls the other by its bare (same-package) name;
    // the loader mangles both consistently so the intra-package call still resolves.
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
    let entry = tmp.write(
        "src/main.phg",
        "package main;\nimport core.console;\nimport acme.util;\n\
         function main() {\n    console.println(\"{util.outer(20)}\");\n}",
    );
    tmp.write(
        "src/acme/util/outer.phg",
        "package acme.util;\nfunction outer(int n) -> int {\n    return inner(n) + 2;\n}",
    );
    tmp.write(
        "src/acme/util/inner.phg",
        "package acme.util;\nfunction inner(int n) -> int {\n    return n + n;\n}",
    );

    let (run, runvm) = run_both(&entry);
    assert_eq!(run, "42\n");
    assert_eq!(run, runvm, "run and runvm must be byte-identical");
}

#[test]
fn unqualified_cross_package_call_is_rejected() {
    // The S2b interim (bare cross-package call) is gone: a library function must be qualified.
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
    let entry = tmp.write(
        "src/main.phg",
        "package main;\nimport core.console;\nimport acme.util;\n\
         function main() {\n    console.println(\"{compute(20)}\");\n}",
    );
    tmp.write(
        "src/acme/util/compute.phg",
        "package acme.util;\nfunction compute(int n) -> int {\n    return n + 2;\n}",
    );
    let unit = loader::load(&entry).expect("project loads");
    // Both backends reject identically (the bare `compute` no longer names any function).
    let run = cli::run_program(&unit.program, &unit.diag_src);
    let runvm = cli::runvm_program(&unit.program, &unit.diag_src);
    assert!(run.is_err(), "bare cross-package call must fail");
    assert!(
        runvm.is_err(),
        "bare cross-package call must fail on the VM too"
    );
}

#[test]
fn library_package_type_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
    let entry = tmp.write("src/main.phg", "package main;\nfunction main() {}");
    tmp.write(
        "src/acme/util/shape.phg",
        "package acme.util;\nclass Shape { constructor(public int w) {} }",
    );
    let err = loader::load(&entry).unwrap_err();
    assert!(err.contains("E-PKG-TYPE"), "got: {err}");
}

#[test]
fn multi_package_transpiles_to_brace_namespaces() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
    let entry = tmp.write(
        "src/main.phg",
        "package main;\nimport core.console;\nimport acme.util;\n\
         function main() {\n    console.println(\"{util.compute(20)}\");\n}",
    );
    tmp.write(
        "src/acme/util/compute.phg",
        "package acme.util;\nfunction compute(int n) -> int {\n    return n + n + 2;\n}",
    );
    let unit = loader::load(&entry).expect("project loads");
    let php = cli::transpile_program(&unit.program, &unit.diag_src).expect("transpiles");
    assert!(php.contains("namespace Acme\\Util {"), "got:\n{php}");
    assert!(php.contains("namespace Main {"), "got:\n{php}");
    assert!(php.contains("\\Main\\main();"), "got:\n{php}");
    // The cross-package call is emitted fully-qualified.
    assert!(php.contains("\\Acme\\Util\\compute("), "got:\n{php}");
    // The library function is declared by its bare leaf inside its namespace block.
    assert!(php.contains("function compute("), "got:\n{php}");
}

#[test]
fn folder_path_violation_is_reported() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
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
