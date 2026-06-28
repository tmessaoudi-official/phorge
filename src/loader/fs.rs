//! Filesystem + parse helpers for the loader (M-Decomp W3.2): parse entry points,
//! folder=package validation, and the `.phg` directory walk.

use super::*;

/// lex + parse a single source, rendering any front-end error to one line (no path prefix — used
/// for the loose path so CLI output stays byte-identical to the pre-S2b single-file pipeline).
pub(super) fn parse_one(src: &str) -> Result<Program, String> {
    let tokens = lex(src).map_err(|e| e.render(src))?;
    Parser::new(tokens)
        .parse_program()
        .map_err(|e| e.render(src))
}

/// As [`parse_one`], but prefix errors with the file path (project mode spans many files).
pub(super) fn parse_at(path: &Path, src: &str) -> Result<Program, String> {
    parse_one(src).map_err(|e| format!("{}: {e}", path.display()))
}

/// In loose mode, only the reserved `package Main;` runs. An empty package is left to the checker
/// (`E-NO-PACKAGE`) so the error is not double-reported.
pub(super) fn enforce_loose_main(prog: &Program) -> Result<(), String> {
    if prog.package.is_empty() || prog.package == ["Main"] {
        return Ok(());
    }
    Err(format!(
        "package `{}` requires a phorge.toml project; only `package Main` runs as a loose script \
         (add a phorge.toml above the source root, or declare `package Main`)",
        prog.package.join(".")
    ))
}

/// Validate a file's package against its on-disk location: directory under the source root = the
/// dotted package (folder = path). `package Main` is exempt (runnable anywhere); an empty package
/// is left to the checker.
pub(super) fn validate_folder_path(
    prog: &Program,
    file: &Path,
    source_root: &Path,
) -> Result<(), String> {
    if prog.package.is_empty() || prog.package == ["Main"] {
        return Ok(());
    }
    let Some(rel) = relative_under(file, source_root) else {
        return Err(format!(
            "{}: package `{}` lives outside the source root `{}` — only `package Main` may live \
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

/// The public-surface file-naming rule (`docs/specs/2026-06-28-public-surface-file-rule-design.md`): a
/// non-`main` file's public face is either exactly one public named type (and the file stem equals it,
/// byte-exact incl. casing) or some public free functions (no public type) — never both, never two
/// public types. `private`/`internal` helpers and `declare` (foreign) items ride along free; an entry
/// file (declares `main`) is fully exempt. Loader-only — never touches a backend.
pub(super) fn validate_public_surface(prog: &Program, file: &Path) -> Result<(), String> {
    use crate::ast::Visibility;
    // Entry/program files mix freely under any name.
    if crate::ast::entry_point(prog, "main").is_some() {
        return Ok(());
    }
    let mut pub_types: Vec<&str> = Vec::new();
    let mut pub_fns: Vec<&str> = Vec::new();
    for item in &prog.items {
        match item {
            // A foreign `declare` describes external PHP — it is not an export of this file, so it does
            // not count toward the public surface (a `.d.phg`-style declaration file is exempt).
            Item::Class(c) if c.foreign => {}
            Item::Function(f) if f.foreign => {}
            Item::Class(c) if c.vis == Visibility::Public => pub_types.push(&c.name),
            Item::Enum(e) if e.vis == Visibility::Public => pub_types.push(&e.name),
            Item::Interface(i) if i.vis == Visibility::Public => pub_types.push(&i.name),
            // A trait carries no visibility modifier (always public reuse); it is a public named type.
            Item::Trait(t) => pub_types.push(&t.name),
            Item::Function(f) if f.vis == Visibility::Public => pub_fns.push(&f.name),
            _ => {}
        }
    }
    if pub_types.len() > 1 {
        return Err(format!(
            "{}: a file may declare at most one public type, but this declares {} ({}) — split them \
             into one file each (`<TypeName>.phg`), or mark the extras `private`/`internal` [E-FILE-MULTI-PUBLIC]",
            file.display(),
            pub_types.len(),
            pub_types.join(", ")
        ));
    }
    if pub_types.len() == 1 && !pub_fns.is_empty() {
        return Err(format!(
            "{}: a public type (`{}`) and public free function(s) ({}) cannot share a file — move the \
             function(s) to a function module, make them methods, or mark them `private`/`internal` [E-FILE-MIXED-PUBLIC]",
            file.display(),
            pub_types[0],
            pub_fns.join(", ")
        ));
    }
    if let Some(ty) = pub_types.first() {
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem != *ty {
            return Err(format!(
                "{}: the public type `{}` must live in a file named `{}.phg` (byte-exact, casing \
                 included), not `{}.phg` [E-FILE-NAME]",
                file.display(),
                ty,
                ty,
                stem
            ));
        }
    }
    Ok(())
}

/// Validate a `*.d.phg` declaration file (M8.5 S3b): it describes only foreign PHP, so it must carry
/// **no `package`** (`E-DECL-PACKAGE` — global foreign symbols have no package) and contain **only
/// `declare` (foreign) items** (`E-DECL-NONFOREIGN`). A valid decl file's foreign items are then merged
/// ambiently into the project. Loader-only — never touches a backend.
pub(super) fn validate_decl_file(prog: &Program, file: &Path) -> Result<(), String> {
    if !prog.package.is_empty() {
        return Err(format!(
            "{}: a `.d.phg` declaration file must not declare a `package` (it describes global foreign \
             PHP, which has no package) — remove the `package` line [E-DECL-PACKAGE]",
            file.display()
        ));
    }
    for item in &prog.items {
        let ok = match item {
            Item::Function(f) => f.foreign,
            Item::Class(c) => c.foreign,
            _ => false,
        };
        if !ok {
            return Err(format!(
                "{}: a `.d.phg` declaration file may contain only foreign `declare` items (every \
                 function/class must be `declare`d) [E-DECL-NONFOREIGN]",
                file.display()
            ));
        }
    }
    Ok(())
}

/// The path of `file` relative to `source_root`, resolving symlinks/`.`/`..` via canonicalization
/// when possible. Returns `None` when `file` is not under `source_root`.
pub(super) fn relative_under(file: &Path, source_root: &Path) -> Option<PathBuf> {
    if let (Ok(f), Ok(root)) = (file.canonicalize(), source_root.canonicalize()) {
        return f.strip_prefix(&root).ok().map(Path::to_path_buf);
    }
    file.strip_prefix(source_root).ok().map(Path::to_path_buf)
}

/// Two paths refer to the same file (canonicalized; falls back to a raw compare).
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Whether `path` is a `*.d.phg` **declaration file** (M8.5 S3b): a file of foreign `declare`s, loaded
/// ambiently into a project (the `.d.ts` analog), carrying no `package` and not validated against
/// folder=path. Detected by the `.d.phg` double-suffix (its `extension()` is plain `phg`).
pub(super) fn is_decl_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".d.phg"))
}

/// All ordinary `*.phg` source files under `dir`, recursively, sorted. **Excludes** `*.d.phg`
/// declaration files (collected separately via [`collect_decl_phg`]) — they are not package members.
pub(super) fn collect_phg(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    if dir.is_dir() {
        walk(dir, &mut out)?;
    }
    out.sort();
    Ok(out)
}

/// All `*.d.phg` declaration files under `dir`, recursively, sorted (M8.5 S3b).
pub(super) fn collect_decl_phg(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    if dir.is_dir() {
        walk_decls(dir, &mut out)?;
    }
    out.sort();
    Ok(out)
}

pub(super) fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
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
        } else if p.extension().and_then(|s| s.to_str()) == Some("phg") && !is_decl_file(&p) {
            out.push(p);
        }
    }
    Ok(())
}

fn walk_decls(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
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
            walk_decls(&p, out)?;
        } else if is_decl_file(&p) {
            out.push(p);
        }
    }
    Ok(())
}

pub(super) fn read_file(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}
