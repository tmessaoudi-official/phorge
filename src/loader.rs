//! Multi-file project loader (M5 S2b).
//!
//! Turns an entry source into a single [`Unit`] (one [`Program`] ready for check + run) and
//! enforces the project structure that the package declaration alone cannot:
//!
//! - **Project mode** — a `phorge.toml` found by walking up from the entry ([`crate::manifest`])
//!   marks the project root. Every `.phg` under the source root is parsed, its package is validated
//!   against its location (**folder = package**, Go's model — `src/acme/util/*.phg` ⇒ `package
//!   acme.util`; `package main` is folder-exempt and may live anywhere), and all items are merged
//!   into one flat [`Program`]. The backends still see a flat item set — qualified cross-package
//!   *call resolution* and namespaced PHP are S2c — so this stays byte-identical for existing
//!   single-file programs.
//! - **Loose-script mode** — no manifest above the entry. Only `package main;` is legal (a dotted
//!   library package requires a project); folder = path is suspended.
//!
//! Enforcement lives here (path-aware), never in the type checker, so `cli::cmd_run(&str)`, the
//! differential harness, and the checker's package-agnostic tests are untouched.

use std::path::{Path, PathBuf};

use crate::ast::{Item, Program};
use crate::lexer::lex;
use crate::manifest::Project;
use crate::parser::Parser;
use crate::token::Span;

/// A loaded compilation unit: the (possibly merged) program plus the source text used to render
/// type-error carets. `diag_src` is the single file's source in loose mode (full carets) or empty
/// for a merged multi-file unit, where no single source aligns — diagnostics then print message +
/// position without a source line (a deliberate flat-merge limitation until S2c).
#[derive(Debug, Clone)]
pub struct Unit {
    pub program: Program,
    pub diag_src: String,
}

/// Load the entry at `path`: project mode if a `phorge.toml` is found by walking up, else loose mode.
pub fn load(entry: &Path) -> Result<Unit, String> {
    // Canonicalize so walk-up detection works from a relative entry path; fall back to the raw path
    // when it does not exist yet (the read below then yields the canonical "cannot read" error).
    let canon = entry.canonicalize().ok();
    let probe: &Path = canon.as_deref().unwrap_or(entry);
    match Project::detect(probe)? {
        None => {
            let src = read_file(entry)?;
            load_loose_src(&src)
        }
        Some(project) => load_project(probe, &project),
    }
}

/// Load a loose-mode program from source text (the `-e`/stdin path, and any single file with no
/// project above it). Enforces the reserved `package main;` — a dotted package needs a project.
pub fn load_loose_src(src: &str) -> Result<Unit, String> {
    let program = parse_one(src)?;
    enforce_loose_main(&program)?;
    Ok(Unit {
        program,
        diag_src: src.to_string(),
    })
}

/// Assemble a project's compilation unit: parse + folder=path-validate every `.phg` under the
/// source root (and the entry, if it lives outside it), then merge all items into one flat program.
fn load_project(entry: &Path, project: &Project) -> Result<Unit, String> {
    let mut files = collect_phg(&project.source_root)?;
    if !files.iter().any(|f| same_file(f, entry)) {
        files.push(entry.to_path_buf());
    }
    files.sort();
    files.dedup();

    let mut merged_items: Vec<Item> = Vec::new();
    // The merged unit runs as the entry's package (normally `main`); its span anchors any
    // program-level diagnostic.
    let mut unit_package: Vec<String> = vec!["main".to_string()];
    let mut unit_span = Span {
        start: 0,
        len: 0,
        line: 0,
        col: 0,
    };

    for file in &files {
        let src = read_file(file)?;
        let prog = parse_at(file, &src)?;
        validate_folder_path(&prog, file, &project.source_root)?;
        if same_file(file, entry) {
            unit_package = prog.package.clone();
            unit_span = prog.span;
        }
        merged_items.extend(prog.items);
    }

    Ok(Unit {
        program: Program {
            package: unit_package,
            items: merged_items,
            span: unit_span,
        },
        diag_src: String::new(),
    })
}

/// lex + parse a single source, rendering any front-end error to one line (no path prefix — used
/// for the loose path so CLI output stays byte-identical to the pre-S2b single-file pipeline).
fn parse_one(src: &str) -> Result<Program, String> {
    let tokens = lex(src).map_err(|e| e.render(src))?;
    Parser::new(tokens)
        .parse_program()
        .map_err(|e| e.render(src))
}

/// As [`parse_one`], but prefix errors with the file path (project mode spans many files).
fn parse_at(path: &Path, src: &str) -> Result<Program, String> {
    parse_one(src).map_err(|e| format!("{}: {e}", path.display()))
}

/// In loose mode, only the reserved `package main;` runs. An empty package is left to the checker
/// (`E-NO-PACKAGE`) so the error is not double-reported.
fn enforce_loose_main(prog: &Program) -> Result<(), String> {
    if prog.package.is_empty() || prog.package == ["main"] {
        return Ok(());
    }
    Err(format!(
        "package `{}` requires a phorge.toml project; only `package main` runs as a loose script \
         (add a phorge.toml above the source root, or declare `package main`)",
        prog.package.join(".")
    ))
}

/// Validate a file's package against its on-disk location: directory under the source root = the
/// dotted package (folder = path). `package main` is exempt (runnable anywhere); an empty package
/// is left to the checker.
fn validate_folder_path(prog: &Program, file: &Path, source_root: &Path) -> Result<(), String> {
    if prog.package.is_empty() || prog.package == ["main"] {
        return Ok(());
    }
    let Some(rel) = relative_under(file, source_root) else {
        return Err(format!(
            "{}: package `{}` lives outside the source root `{}` — only `package main` may live \
             outside it [E-PKG-PATH]",
            file.display(),
            prog.package.join("."),
            source_root.display()
        ));
    };
    let expected: Vec<String> = match rel.parent() {
        Some(dir) => dir
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str().map(String::from),
                _ => None,
            })
            .collect(),
        None => Vec::new(),
    };
    if expected.is_empty() {
        return Err(format!(
            "{}: package `{}` cannot sit directly in the source root — a dotted package needs a \
             matching subdirectory (expected under `{}/`) [E-PKG-PATH]",
            file.display(),
            prog.package.join("."),
            prog.package.join("/")
        ));
    }
    if expected != prog.package {
        return Err(format!(
            "{}: package `{}` does not match its location — directory `{}` implies \
             `package {};` (folder = path) [E-PKG-PATH]",
            file.display(),
            prog.package.join("."),
            expected.join("/"),
            expected.join(".")
        ));
    }
    Ok(())
}

/// The path of `file` relative to `source_root`, resolving symlinks/`.`/`..` via canonicalization
/// when possible. Returns `None` when `file` is not under `source_root`.
fn relative_under(file: &Path, source_root: &Path) -> Option<PathBuf> {
    if let (Ok(f), Ok(root)) = (file.canonicalize(), source_root.canonicalize()) {
        return f.strip_prefix(&root).ok().map(Path::to_path_buf);
    }
    file.strip_prefix(source_root).ok().map(Path::to_path_buf)
}

/// Two paths refer to the same file (canonicalized; falls back to a raw compare).
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// All `*.phg` files under `dir`, recursively, in a deterministic (sorted) order.
fn collect_phg(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    if dir.is_dir() {
        walk(dir, &mut out)?;
    }
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let rd = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read directory {}: {e}", dir.display()))?;
    let mut entries: Vec<PathBuf> = Vec::new();
    for e in rd {
        let e = e.map_err(|e| format!("cannot read an entry in {}: {e}", dir.display()))?;
        entries.push(e.path());
    }
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out)?;
        } else if p.extension().and_then(|s| s.to_str()) == Some("phg") {
            out.push(p);
        }
    }
    Ok(())
}

fn read_file(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
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
        let u = load_loose_src("package main;\nfunction main() {}").unwrap();
        assert_eq!(u.program.package, ["main"]);
        assert_eq!(u.diag_src, "package main;\nfunction main() {}");
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
        tmp.write("phorge.toml", "name = \"acme/app\"\nsource = \"src\"");
        let entry = tmp.write(
            "src/main.phg",
            "package main;\nfunction main() {}\nfunction local() {}",
        );
        tmp.write(
            "src/acme/util/parse.phg",
            "package acme.util;\nfunction parse() {}",
        );
        let u = load(&entry).unwrap();
        assert_eq!(u.program.package, ["main"]);
        // Items from both files are merged into one flat program.
        assert!(
            u.program.items.len() >= 3,
            "merged items: {:?}",
            u.program.items.len()
        );
        assert!(u.diag_src.is_empty(), "merged unit has no single source");
    }

    #[test]
    fn project_main_is_folder_exempt_at_root() {
        let tmp = TempDir::new();
        tmp.write("phorge.toml", "name = \"acme/app\"");
        // main lives at the project root, outside src/ — allowed.
        let entry = tmp.write("main.phg", "package main;\nfunction main() {}");
        let u = load(&entry).unwrap();
        assert_eq!(u.program.package, ["main"]);
    }

    #[test]
    fn folder_path_mismatch_is_rejected() {
        let tmp = TempDir::new();
        tmp.write("phorge.toml", "name = \"acme/app\"");
        let entry = tmp.write("src/main.phg", "package main;\nfunction main() {}");
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
        tmp.write("phorge.toml", "name = \"acme/app\"");
        let entry = tmp.write("src/main.phg", "package main;\nfunction main() {}");
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
        tmp.write("phorge.toml", "name = \"acme/app\"\nsource = \"src\"");
        tmp.write("src/main.phg", "package main;\nfunction main() {}");
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
}
