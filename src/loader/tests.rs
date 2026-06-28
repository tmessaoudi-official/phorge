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
    let u = load_loose_src("package Main;\nfunction main() -> void {}").unwrap();
    assert_eq!(u.program.package, ["Main"]);
    assert_eq!(u.diag_src, "package Main;\nfunction main() -> void {}");
}

#[test]
fn loose_non_main_is_rejected() {
    let err = load_loose_src("package app.util;\nfunction f() -> void {}").unwrap_err();
    assert!(err.contains("requires a phorge.toml project"), "got: {err}");
}

#[test]
fn loose_empty_package_defers_to_checker() {
    // No package decl — loader stays silent (checker reports E-NO-PACKAGE downstream).
    let u = load_loose_src("function main() -> void {}").unwrap();
    assert!(u.program.package.is_empty());
}

// --- project mode ------------------------------------------------------

#[test]
fn project_merges_files_flat() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() -> void {}\nfunction local() -> void {}",
    );
    tmp.write(
        "src/acme/util/parse.phg",
        "package acme.util;\nfunction parse() -> void {}",
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
        "package Main;\nfunction main() -> void {}\nclass C {}",
    );
    tmp.write(
        "src/acme/util/parse.phg",
        "package acme.util;\nfunction parse() -> void {}",
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
    let u = load_loose_src("package Main;\nfunction main() -> void {}").unwrap();
    assert!(u.stats.is_none(), "loose mode reports no project stats");
}

#[test]
fn project_main_is_folder_exempt_at_root() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
    // main lives at the project root, outside src/ — allowed.
    let entry = tmp.write("main.phg", "package Main;\nfunction main() -> void {}");
    let u = load(&entry).unwrap();
    assert_eq!(u.program.package, ["Main"]);
}

#[test]
fn folder_path_mismatch_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() -> void {}");
    // File sits in src/acme/util but declares the wrong package.
    tmp.write(
        "src/acme/util/parse.phg",
        "package acme.wrong;\nfunction parse() -> void {}",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-PKG-PATH"), "got: {err}");
    assert!(err.contains("does not match its location"), "got: {err}");
}

#[test]
fn non_main_directly_in_source_root_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() -> void {}");
    tmp.write("src/loose.phg", "package app;\nfunction f() -> void {}");
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
    tmp.write("src/main.phg", "package Main;\nfunction main() -> void {}");
    // A dotted package living outside the source root entirely.
    tmp.write(
        "lib/parse.phg",
        "package acme.util;\nfunction parse() -> void {}",
    );
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
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() -> void {}");
    // Two files in the same package each define `f` — collides after the flat merge.
    tmp.write(
        "src/acme/util/a.phg",
        "package acme.util;\nfunction f() -> void {}",
    );
    tmp.write(
        "src/acme/util/b.phg",
        "package acme.util;\nfunction f() -> void {}",
    );
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
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() -> void {}");
    // A vendored library must not declare `package Main` (it would collide with the entry).
    tmp.write(
        "vendor/acme/lib/oops.phg",
        "package Main;\nfunction stray() -> void {}",
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
        "package Main;\nimport type acme.geo.Hidden;\nfunction main() -> void { Hidden h = Hidden(); }",
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
        "package Main;\nimport type acme.geo.Shown;\nfunction main() -> void { Shown s = Shown(); }",
    );
    // Public-surface rule: a file with one public type is named after it (`Shown.phg`).
    tmp.write(
        "src/acme/geo/Shown.phg",
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
        "package Main;\nfunction main() -> void { Helper h = Helper(); }",
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
        "package Main;\nfunction main() -> void { Helper h = Helper(); }",
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
        "package Main;\ntype H = Helper;\nfunction main() -> void { H h = Helper(); }",
    );
    tmp.write(
        "src/helper.phg",
        "package Main;\nprivate class Helper { constructor() {} }",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-VIS-PRIVATE"), "got: {err}");
}

// --- public-surface file-naming rule (E-FILE-*) -----------------------

#[test]
fn public_type_in_mismatched_file_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() -> void { Widget w = Widget(); }",
    );
    // A non-`main` file declaring one public type must be named after it; `widget.phg` ≠ `Widget`.
    tmp.write(
        "src/widget.phg",
        "package Main;\npublic class Widget { constructor() {} }",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-FILE-NAME"), "got: {err}");
}

#[test]
fn public_type_in_matching_file_is_allowed() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() -> void { Widget w = Widget(); }",
    );
    tmp.write(
        "src/Widget.phg",
        "package Main;\npublic class Widget { constructor() {} }",
    );
    assert!(load(&entry).is_ok());
}

#[test]
fn two_public_types_in_one_file_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() -> void { A a = A(); }",
    );
    tmp.write(
        "src/A.phg",
        "package Main;\npublic class A { constructor() {} }\npublic class B { constructor() {} }",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-FILE-MULTI-PUBLIC"), "got: {err}");
}

#[test]
fn public_type_plus_public_fn_in_one_file_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() -> void { Box b = Box(); }",
    );
    tmp.write(
        "src/Box.phg",
        "package Main;\npublic class Box { constructor() {} }\npublic function helper() -> int { return 1; }",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-FILE-MIXED-PUBLIC"), "got: {err}");
}

#[test]
fn private_helper_type_rides_along_in_a_type_module() {
    // A type module may carry private/internal helper types + functions — they ride along free.
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() -> void { Widget w = Widget(); }",
    );
    tmp.write(
        "src/Widget.phg",
        "package Main;\npublic class Widget { constructor() {} }\nprivate class Cache { constructor() {} }\nprivate function tweak() -> int { return 1; }",
    );
    assert!(load(&entry).is_ok(), "private helpers should ride along");
}

#[test]
fn main_file_with_multiple_public_types_is_exempt() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    // The entry file declares `main` → exempt: multiple public types + functions are fine, any name.
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\npublic class A { constructor() {} }\npublic class B { constructor() {} }\nfunction main() -> void { A a = A(); B b = B(); }",
    );
    assert!(
        load(&entry).is_ok(),
        "a main file is exempt from the public-surface rule"
    );
}

#[test]
fn forward_and_cross_file_type_references_resolve() {
    // Order-independence (the prebind pre-pass): `Order` references `OrderLine`, which sorts/merges
    // AFTER it — and a forward reference within the entry file — both resolve.
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nfunction main() -> void { B b = makeB(); }\nfunction makeB() -> B { return B(7); }",
    );
    // Two library files; `Order.phg` (merged first, alphabetically) references `OrderLine` (later).
    tmp.write(
        "src/acme/lib/Order.phg",
        "package acme.lib;\npublic class Order { constructor(public OrderLine line) {} }",
    );
    tmp.write(
        "src/acme/lib/OrderLine.phg",
        "package acme.lib;\npublic class OrderLine { constructor(public int qty) {} }",
    );
    tmp.write(
        "src/B.phg",
        "package Main;\npublic class B { constructor(public int x) {} }",
    );
    assert!(
        load(&entry).is_ok(),
        "forward + cross-file type refs must resolve"
    );
}

// --- M8.5 S3b: `.d.phg` ambient declaration files -----------------------

#[test]
fn decl_file_is_loaded_ambiently_and_not_mangled() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() -> void {}");
    // A package-free declaration file under the source root — loaded ambiently, never compiled as a
    // package source (so no folder=path / package decl required).
    tmp.write(
        "src/php.d.phg",
        "declare function strtoupper(string s) -> string;",
    );
    let u = load(&entry).unwrap();
    // The foreign function is merged into the unit, with its bare global name (never mangled).
    let foreign_fns: Vec<&str> = u
        .program
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Function(f) if f.foreign => Some(f.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        foreign_fns,
        ["strtoupper"],
        "merged foreign fns: {foreign_fns:?}"
    );
}

#[test]
fn decl_file_with_package_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() -> void {}");
    tmp.write(
        "src/php.d.phg",
        "package Main;\ndeclare function strtoupper(string s) -> string;",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-DECL-PACKAGE"), "got: {err}");
}

#[test]
fn decl_file_with_nonforeign_item_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() -> void {}");
    // A real (non-`declare`) function has no place in a declaration file.
    tmp.write(
        "src/php.d.phg",
        "declare function strtoupper(string s) -> string;\nfunction local() -> void {}",
    );
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-DECL-NONFOREIGN"), "got: {err}");
}

#[test]
fn decl_file_is_not_counted_as_a_package_source() {
    // A `.d.phg` is excluded from `collect_phg` — it must never be folder=path-validated. Place one
    // directly in the source root (where a real non-`main` `.phg` would be rejected) and confirm load
    // succeeds: only its ambient-merge path ran, not the package-source path.
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() -> void {}");
    tmp.write(
        "src/builtins.d.phg",
        "declare function strlen(string s) -> int;",
    );
    assert!(
        load(&entry).is_ok(),
        "a `.d.phg` in the source root must load fine"
    );
}
