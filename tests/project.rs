//! M5 S2b/S2c integration: a multi-file project loads through `loader::load`, resolves cross-package
//! calls, and runs byte-identically on both backends. S2c qualifies cross-package calls
//! (`Util.compute(x)` via an import leaf or alias), tightens the S2b bare-call interim (unqualified
//! cross-package calls now fail), supports cross-package types via `import type`, and transpiles to
//! one PHP `namespace` brace-block per package. Packages are PascalCase (`E-PKG-CASE`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use phorj::{cli, loader};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let unique = N.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("phorj_project_it_{}_{unique}", std::process::id()));
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
    let run = cli::run_program(&unit).expect("interpreter runs");
    let runvm = cli::runvm_program(&unit).expect("vm runs");
    (run, runvm)
}

#[test]
fn multi_file_project_qualified_call_runs_byte_identically() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"\nsource = \"src\"");
    // S2c: cross-package calls are *qualified* via an import leaf (`Util.compute`), no longer the
    // S2b bare form. The loader resolves it against the imported package's mangled symbol.
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport Acme.Util;\n\
         function main() -> void {\n    Output.printLine(\"{Util.compute(20)}\");\n}",
    );
    tmp.write(
        "src/Acme/Util/compute.phg",
        "package Acme.Util;\nfunction compute(int n) -> int {\n    return n + n + 2;\n}",
    );

    let (run, runvm) = run_both(&entry);
    assert_eq!(run, "42\n");
    assert_eq!(run, runvm, "run and runvm must be byte-identical");
}

#[test]
fn import_alias_resolves_qualified_call() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"");
    // `import Acme.Util as U;` binds the leaf `u`; the call qualifies on the alias.
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport Acme.Util as U;\n\
         function main() -> void {\n    Output.printLine(\"{U.compute(20)}\");\n}",
    );
    tmp.write(
        "src/Acme/Util/compute.phg",
        "package Acme.Util;\nfunction compute(int n) -> int {\n    return n + n + 2;\n}",
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
    tmp.write("phorj.toml", "module = \"acme/app\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport Acme.Util;\n\
         function main() -> void {\n    Output.printLine(\"{Util.outer(20)}\");\n}",
    );
    tmp.write(
        "src/Acme/Util/outer.phg",
        "package Acme.Util;\nfunction outer(int n) -> int {\n    return inner(n) + 2;\n}",
    );
    tmp.write(
        "src/Acme/Util/inner.phg",
        "package Acme.Util;\nfunction inner(int n) -> int {\n    return n + n;\n}",
    );

    let (run, runvm) = run_both(&entry);
    assert_eq!(run, "42\n");
    assert_eq!(run, runvm, "run and runvm must be byte-identical");
}

#[test]
fn unqualified_cross_package_call_is_rejected() {
    // The S2b interim (bare cross-package call) is gone: a library function must be qualified.
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport Acme.Util;\n\
         function main() -> void {\n    Output.printLine(\"{compute(20)}\");\n}",
    );
    tmp.write(
        "src/Acme/Util/compute.phg",
        "package Acme.Util;\nfunction compute(int n) -> int {\n    return n + 2;\n}",
    );
    let unit = loader::load(&entry).expect("project loads");
    // Both backends reject identically (the bare `compute` no longer names any function).
    let run = cli::run_program(&unit);
    let runvm = cli::runvm_program(&unit);
    assert!(run.is_err(), "bare cross-package call must fail");
    assert!(
        runvm.is_err(),
        "bare cross-package call must fail on the VM too"
    );
}

#[test]
fn library_package_type_is_usable_cross_package() {
    // The E-PKG-TYPE gate is retired (M-RT cross-package types): a library package may declare a
    // type, and `package Main` consumes it via `import type`, instantiating + reading a field.
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport type Acme.Util.Shape;\n\
         function main() -> void {\n    Shape s = new Shape(5);\n    Output.printLine(\"{s.w}\");\n}",
    );
    tmp.write(
        "src/Acme/Util/Shape.phg",
        "package Acme.Util;\nclass Shape { constructor(public int w) {} }",
    );
    let unit = loader::load(&entry).expect("project with a cross-package type loads");
    // Both backends agree (the type def + every reference were mangled before either backend ran).
    let run = cli::run_program(&unit);
    let runvm = cli::runvm_program(&unit);
    assert_eq!(run.as_deref(), Ok("5\n"), "run output");
    assert_eq!(runvm.as_deref(), Ok("5\n"), "runvm output");
}

/// `import type` of a type a package does not export.
#[test]
fn import_type_unknown_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport type Acme.Util.Nope;\nfunction main() -> void {}",
    );
    tmp.write(
        "src/Acme/Util/Shape.phg",
        "package Acme.Util;\nclass Shape { constructor(public int w) {} }",
    );
    let err = loader::load(&entry).unwrap_err();
    assert!(err.contains("E-TYPE-IMPORT-UNKNOWN"), "got: {err}");
}

/// Two `import type` binding the same bare name without an alias.
#[test]
fn import_type_conflict_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport type Acme.A.Shape;\nimport type Acme.B.Shape;\nfunction main() -> void {}",
    );
    tmp.write(
        "src/Acme/A/Shape.phg",
        "package Acme.A;\nclass Shape { constructor(public int w) {} }",
    );
    tmp.write(
        "src/Acme/B/Shape.phg",
        "package Acme.B;\nclass Shape { constructor(public int w) {} }",
    );
    let err = loader::load(&entry).unwrap_err();
    assert!(err.contains("E-TYPE-IMPORT-CONFLICT"), "got: {err}");
}

/// `import type` naming a built-in type (built-ins are import-free).
#[test]
fn import_type_builtin_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport type Acme.Util.List;\nfunction main() -> void {}",
    );
    tmp.write(
        "src/Acme/Util/u.phg",
        "package Acme.Util;\nfunction noop() -> void {}",
    );
    let err = loader::load(&entry).unwrap_err();
    assert!(err.contains("E-TYPE-IMPORT-BUILTIN"), "got: {err}");
}

/// `import type` whose bound name collides with a module-import qualifier.
#[test]
fn import_type_shadow_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"");
    // A type named `Util` (bound bare by `import type Acme.Types.Util`) clashing with the
    // `Acme.Util` module-import leaf `Util`. The shadow guard keeps the two import kinds disjoint.
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Acme.Util;\nimport type Acme.Types.Util;\nfunction main() -> void {}",
    );
    tmp.write(
        "src/Acme/Util/u.phg",
        "package Acme.Util;\nfunction noop() -> void {}",
    );
    tmp.write(
        "src/Acme/Types/Util.phg",
        "package Acme.Types;\nclass Util { constructor(public int w) {} }",
    );
    let err = loader::load(&entry).unwrap_err();
    assert!(err.contains("E-TYPE-IMPORT-SHADOW"), "got: {err}");
}

#[test]
fn multi_package_transpiles_to_brace_namespaces() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport Acme.Util;\n\
         function main() -> void {\n    Output.printLine(\"{Util.compute(20)}\");\n}",
    );
    tmp.write(
        "src/Acme/Util/compute.phg",
        "package Acme.Util;\nfunction compute(int n) -> int {\n    return n + n + 2;\n}",
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
    tmp.write("phorj.toml", "module = \"acme/app\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() -> void {}");
    tmp.write(
        "src/Acme/Util/x.phg",
        "package Acme.Bad;\nfunction x() -> void {}",
    );
    let err = loader::load(&entry).unwrap_err();
    assert!(err.contains("E-PKG-PATH"), "got: {err}");
}

#[test]
fn loose_non_main_file_is_rejected() {
    let tmp = TempDir::new();
    // No phorj.toml anywhere above → loose mode; a dotted package is illegal.
    let entry = tmp.write("script.phg", "package App.Util;\nfunction f() -> void {}");
    let err = loader::load(&entry).unwrap_err();
    assert!(err.contains("requires a phorj.toml project"), "got: {err}");
}

// --- Cross-package traits (M-RT S8, cross-package) ---

/// A `trait` declared in a library package is composed into a `package Main` class via
/// `import type Pkg.Trait;` + `use Trait;`, and runs byte-identically on both backends. The loader
/// mangles the trait declaration and the `use` clause to the same FQN, so the by-name flatten lines up.
#[test]
fn cross_package_trait_composition_runs_byte_identically() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/mix\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport type Acme.Mix.Greet;\n\
         class Person {\n  use Greet;\n  constructor(public string name) {}\n}\n\
         function main() -> void {\n  var p = new Person(\"ada\");\n  Output.printLine(\"{p.name}: {p.hello()}\");\n}",
    );
    tmp.write(
        "src/Acme/Mix/Greet.phg",
        "package Acme.Mix;\ntrait Greet {\n  function hello() -> string { return \"hi\"; }\n}",
    );
    let (run, runvm) = run_both(&entry);
    assert_eq!(run, "ada: hi\n");
    assert_eq!(run, runvm, "run and runvm must be byte-identical");
}

/// The cross-package trait transpiles to a native PHP `trait` in its package namespace, composed by
/// the using class via a fully-qualified `use \Acme\Mix\Greet`.
#[test]
fn cross_package_trait_transpiles_to_namespaced_trait() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/mix\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport type Acme.Mix.Greet;\n\
         class Person {\n  use Greet;\n  constructor(public string name) {}\n}\n\
         function main() -> void {\n  var p = new Person(\"ada\");\n  Output.printLine(p.hello());\n}",
    );
    tmp.write(
        "src/Acme/Mix/Greet.phg",
        "package Acme.Mix;\ntrait Greet {\n  function hello() -> string { return \"hi\"; }\n}",
    );
    let unit = loader::load(&entry).expect("project loads");
    let php = cli::transpile_program(&unit.program, &unit.diag_src).expect("transpiles");
    assert!(php.contains("namespace Acme\\Mix {"), "got:\n{php}");
    assert!(php.contains("trait Greet {"), "got:\n{php}");
    assert!(php.contains("use \\Acme\\Mix\\Greet;"), "got:\n{php}");
}

/// A trait is reuse, not a type — typing a value as a cross-package trait is `E-USE-AS-TYPE`, exactly
/// as for a same-package trait. (A check-time error: the project loads, the checker rejects it.)
#[test]
fn cross_package_trait_used_as_type_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/mix\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport type Acme.Mix.Greet;\n\
         function f(Greet x) -> void { Output.printLine(\"no\"); }\n\
         function main() -> void { Output.printLine(\"hi\"); }",
    );
    tmp.write(
        "src/Acme/Mix/Greet.phg",
        "package Acme.Mix;\ntrait Greet {\n  function hello() -> string { return \"hi\"; }\n}",
    );
    let unit = loader::load(&entry).expect("project loads (trait import resolves)");
    let err = cli::run_program(&unit).unwrap_err();
    assert!(err.contains("E-USE-AS-TYPE"), "got: {err}");
}

/// A qualified cross-package call inside a **map literal** value (`["k" => Util.f()]`) is resolved by
/// the loader — the `Expr::Map` arm descends both key and value, so a cross-package reference nested
/// in a map is rewritten like one in a list (the multi-package map-literal gap).
#[test]
fn cross_package_call_inside_map_literal_resolves() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport Acme.Util;\n\
         function main() -> void {\n  Map<string, int> m = [\"k\" => Util.compute(20)];\n  Output.printLine(\"{m[\\\"k\\\"]}\");\n}",
    );
    tmp.write(
        "src/Acme/Util/compute.phg",
        "package Acme.Util;\nfunction compute(int n) -> int { return n + 22; }",
    );
    let (run, runvm) = run_both(&entry);
    assert_eq!(run, "42\n");
    assert_eq!(run, runvm, "run and runvm must be byte-identical");
}

/// A `package Main` class `extends` a library-package class (imported via `import type`), inheriting
/// its constructor + field, overriding an `open` method, and calling up with the named
/// `parent(Ancestor).m()` form — all resolved across the package boundary, byte-identical.
#[test]
fn cross_package_inheritance_and_parent_calls_run_byte_identically() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/zoo\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport type Acme.Zoo.Animal;\n\
         class Dog extends Animal {\n  open function speak() -> string { return \"woof/\" + parent(Animal).speak(); }\n}\n\
         function main() -> void {\n  Output.printLine(new Dog(\"rex\").speak());\n}",
    );
    tmp.write(
        "src/Acme/Zoo/Animal.phg",
        "package Acme.Zoo;\nopen class Animal {\n  constructor(public string name) {}\n  open function speak() -> string { return \"(animal)\"; }\n}",
    );
    let (run, runvm) = run_both(&entry);
    assert_eq!(run, "woof/(animal)\n");
    assert_eq!(run, runvm, "run and runvm must be byte-identical");
}

/// The cross-package parent class is emitted as `extends \Acme\Zoo\Animal` and the parent call as
/// `parent::speak()` in the using class's namespace block.
#[test]
fn cross_package_inheritance_transpiles_to_qualified_extends() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/zoo\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nimport type Acme.Zoo.Animal;\n\
         class Dog extends Animal {\n  open function speak() -> string { return parent.speak(); }\n}\n\
         function main() -> void {\n  Output.printLine(new Dog(\"rex\").speak());\n}",
    );
    tmp.write(
        "src/Acme/Zoo/Animal.phg",
        "package Acme.Zoo;\nopen class Animal {\n  constructor(public string name) {}\n  open function speak() -> string { return \"(animal)\"; }\n}",
    );
    let unit = loader::load(&entry).expect("project loads");
    let php = cli::transpile_program(&unit.program, &unit.diag_src).expect("transpiles");
    assert!(php.contains("extends \\Acme\\Zoo\\Animal"), "got:\n{php}");
    assert!(php.contains("parent::speak()"), "got:\n{php}");
}

// ── W0-4: loader-side reserved-package + package-casing gates (project mode) ─────────────────────
// H §2.3 (P1): in project mode the flat merge mangles per-file defs *before* `check()`, so a file's
// own `package` decl never reaches program.rs — a `package Core.*;` hijack or a lowercase package
// declaration was silently accepted. The loader now validates each file's package decl per-file,
// before the merge, mirroring the checker's E-RESERVED-PACKAGE / E-PKG-CASE.

#[test]
fn project_reserved_core_package_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package Main;\nimport Core.Output;\nfunction main() -> void { Output.printLine(\"hi\"); }",
    );
    // Lives at the folder that matches its (reserved) package, so E-PKG-PATH passes and the
    // reserved-root rule is what fires.
    tmp.write(
        "src/Core/Output/sneak.phg",
        "package Core.Output;\nfunction sneak() -> void {}",
    );
    let err = loader::load(&entry).expect_err("a `package Core.*` file must be rejected");
    assert!(err.contains("E-RESERVED-PACKAGE"), "got: {err}");
}

#[test]
fn project_lowercase_package_decl_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() -> void {}");
    // Folder matches the (lowercase) package, so E-PKG-PATH passes and E-PKG-CASE is what fires.
    tmp.write(
        "src/acme/util.phg",
        "package acme;\nfunction u() -> void {}",
    );
    let err = loader::load(&entry).expect_err("a lowercase package decl must be rejected");
    assert!(err.contains("E-PKG-CASE"), "got: {err}");
}
