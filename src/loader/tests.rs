use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let unique = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "phorge_loader_test_{}_{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &Path {
        &self.0
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

// --- loose mode --------------------------------------------------------

#[test]
fn loose_main_is_accepted() {
    let u = load_loose_src("package Main;\nfunction main() {}").unwrap();
    assert_eq!(u.program.package, ["Main"]);
    assert_eq!(u.diag_src, "package Main;\nfunction main() {}");
}

#[test]
fn loose_non_main_is_rejected() {
    let err = load_loose_src("package app.util;\nfunction f() {}").unwrap_err();
    assert!(err.contains("requires a phorge.toml project"), "got: {err}");
}

#[test]
fn loose_empty_package_defers_to_checker() {
    // No package decl — loader stays silent (checker reports E-NO-PACKAGE downstream).
    let u = load_loose_src("function main() {}").unwrap();
    assert!(u.program.package.is_empty());
}

// --- project mode ------------------------------------------------------

#[test]
fn project_merges_files_flat() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() {}\nfunction local() {}",
    );
    tmp.write(
        "src/acme/util/parse.phg",
        "package acme.util;\nfunction parse() {}",
    );
    let u = load(&entry).unwrap();
    assert_eq!(u.program.package, ["Main"]);
    // Items from both files are merged into one flat program.
    assert!(
        u.program.items.len() >= 3,
        "merged items: {:?}",
        u.program.items.len()
    );
    assert!(u.diag_src.is_empty(), "merged unit has no single source");
}

#[test]
fn project_load_reports_stats() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() {}\nclass C {}",
    );
    tmp.write(
        "src/acme/util/parse.phg",
        "package acme.util;\nfunction parse() {}",
    );
    let u = load(&entry).unwrap();
    let stats = u.stats.expect("project mode reports stats");
    assert_eq!(stats.files, 2, "two source files");
    assert_eq!(stats.packages, 2, "main + acme.util");
    assert_eq!(stats.defs, 3, "main, C, parse");
    // The human summary mentions the project-wide scope.
    let summary = stats.summary();
    assert!(summary.contains("2 files"), "got: {summary}");
    assert!(summary.contains("whole project"), "got: {summary}");
}

#[test]
fn loose_load_has_no_stats() {
    let u = load_loose_src("package Main;\nfunction main() {}").unwrap();
    assert!(u.stats.is_none(), "loose mode reports no project stats");
}

#[test]
fn project_main_is_folder_exempt_at_root() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
    // main lives at the project root, outside src/ — allowed.
    let entry = tmp.write("main.phg", "package Main;\nfunction main() {}");
    let u = load(&entry).unwrap();
    assert_eq!(u.program.package, ["Main"]);
}

#[test]
fn folder_path_mismatch_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() {}");
    // File sits in src/acme/util but declares the wrong package.
    tmp.write(
        "src/acme/util/parse.phg",
        "package acme.wrong;\nfunction parse() {}",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-PKG-PATH"), "got: {err}");
    assert!(err.contains("does not match its location"), "got: {err}");
}

#[test]
fn non_main_directly_in_source_root_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() {}");
    tmp.write("src/loose.phg", "package app;\nfunction f() {}");
    let err = load(&entry).unwrap_err();
    assert!(
        err.contains("cannot sit directly in the source root"),
        "got: {err}"
    );
}

#[test]
fn library_package_outside_source_root_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    tmp.write("src/main.phg", "package Main;\nfunction main() {}");
    // A dotted package living outside the source root entirely.
    tmp.write("lib/parse.phg", "package acme.util;\nfunction parse() {}");
    // Run it as the entry so it is loaded even though it is not under src/.
    let err = load(&tmp.path().join("lib/parse.phg")).unwrap_err();
    assert!(err.contains("lives outside the source root"), "got: {err}");
}

#[test]
fn missing_entry_file_errors() {
    let tmp = TempDir::new();
    let err = load(&tmp.path().join("does-not-exist.phg")).unwrap_err();
    assert!(err.contains("cannot read"), "got: {err}");
}

#[test]
fn duplicate_function_in_package_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() {}");
    // Two files in the same package each define `f` — collides after the flat merge.
    tmp.write("src/acme/util/a.phg", "package acme.util;\nfunction f() {}");
    tmp.write("src/acme/util/b.phg", "package acme.util;\nfunction f() {}");
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-DUP-DEF"), "got: {err}");
    assert!(err.contains("duplicate definition of `f`"), "got: {err}");
}

#[test]
fn vendored_package_main_is_rejected() {
    let tmp = TempDir::new();
    tmp.write(
            "phorge.toml",
            "module = \"acme/app\"\nsource = \"src\"\n\n[require]\n\"acme/lib\" = { git = \"u\", tag = \"v1\" }",
        );
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() {}");
    // A vendored library must not declare `package Main` (it would collide with the entry).
    tmp.write(
        "vendor/acme/lib/oops.phg",
        "package Main;\nfunction stray() {}",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-VENDOR-MAIN"), "got: {err}");
}

// --- declaration visibility (visibility modifiers) ---------------------

#[test]
fn import_type_of_internal_library_type_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport type acme.geo.Hidden;\nfunction main() { Hidden h = Hidden(); }",
    );
    tmp.write(
        "src/acme/geo/geo.phg",
        "package acme.geo;\ninternal class Hidden { constructor() {} }",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-VIS-INTERNAL"), "got: {err}");
}

#[test]
fn import_type_of_public_library_type_is_allowed() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport type acme.geo.Shown;\nfunction main() { Shown s = Shown(); }",
    );
    tmp.write(
        "src/acme/geo/geo.phg",
        "package acme.geo;\npublic class Shown { constructor() {} }",
    );
    assert!(load(&entry).is_ok());
}

#[test]
fn private_type_referenced_from_sibling_file_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() { Helper h = Helper(); }",
    );
    // A second `package Main` file (folder-exempt at root) declaring a file-private type.
    tmp.write(
        "src/helper.phg",
        "package Main;\nprivate class Helper { constructor() {} }",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-VIS-PRIVATE"), "got: {err}");
}

#[test]
fn internal_type_referenced_from_sibling_file_is_allowed() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() { Helper h = Helper(); }",
    );
    tmp.write(
        "src/helper.phg",
        "package Main;\ninternal class Helper { constructor() {} }",
    );
    assert!(load(&entry).is_ok());
}

#[test]
fn private_function_called_from_sibling_file_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() -> int { return helper(); }",
    );
    tmp.write(
        "src/helper.phg",
        "package Main;\nprivate function helper() -> int { return 1; }",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-VIS-PRIVATE"), "got: {err}");
}

#[test]
fn internal_function_called_cross_package_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport acme.util;\nfunction main() -> int { return util.secret(); }",
    );
    tmp.write(
        "src/acme/util/util.phg",
        "package acme.util;\ninternal function secret() -> int { return 7; }",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-VIS-INTERNAL"), "got: {err}");
}

#[test]
fn public_function_called_cross_package_is_allowed() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport acme.util;\nfunction main() -> int { return util.shown(); }",
    );
    tmp.write(
        "src/acme/util/util.phg",
        "package acme.util;\npublic function shown() -> int { return 7; }",
    );
    assert!(load(&entry).is_ok());
}

#[test]
fn type_alias_does_not_launder_private_type() {
    // A type alias names a type but the *construction* still names the real type directly, so the
    // file-scoped `private` check on `Helper()` fires regardless of the alias (aliases are
    // file-local + erased, so they cannot re-export across files).
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\ntype H = Helper;\nfunction main() { H h = Helper(); }",
    );
    tmp.write(
        "src/helper.phg",
        "package Main;\nprivate class Helper { constructor() {} }",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-VIS-PRIVATE"), "got: {err}");
}
