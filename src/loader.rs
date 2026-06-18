//! Multi-file project loader + cross-package name resolution (M5 S2b/S2c).
//!
//! Turns an entry source into a single [`Unit`] (one [`Program`] ready for check + run) and
//! enforces the project structure that the package declaration alone cannot:
//!
//! - **Project mode** — a `phorge.toml` found by walking up from the entry ([`crate::manifest`])
//!   marks the project root. Every `.phg` under the source root is parsed, its package is validated
//!   against its location (**folder = package**, Go's model — `src/acme/util/*.phg` ⇒ `package
//!   acme.util`; `package main` is folder-exempt and may live anywhere). A resolution pass then
//!   mangles every non-`main` definition to a globally-unique name (`acme.util` + `compute` ⇒
//!   `Acme\Util\compute`) and rewrites call sites — same-package bare calls and qualified user calls
//!   (`util.compute(x)`, via the per-file import map) become bare calls on the mangled name; native
//!   `core.*` calls are untouched (S2c). All items then merge into one flat [`Program`]. Because the
//!   rewrite produces concrete bare names *before* any backend runs, the checker/interpreter/
//!   compiler/VM are unchanged (run==runvm is structural); only the transpiler de-mangles the
//!   `\`-bearing names back into PHP `namespace` blocks. A single-package program has no mangled
//!   names, so it is byte-identical to the pre-S2c output.
//! - **Loose-script mode** — no manifest above the entry. Only `package main;` is legal (a dotted
//!   library package requires a project); folder = path is suspended.
//!
//! Enforcement and resolution live here (path-aware), never in the type checker, so
//! `cli::cmd_run(&str)`, the differential harness, and the checker's package-agnostic tests are
//! untouched. S2c scope: library packages export **functions** only (a `class`/`enum` in a non-`main`
//! package is rejected, `E-PKG-TYPE`); cross-package type namespacing is an M5 follow-up.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::{ClassMember, Expr, Item, MatchArm, Program, Stmt, StrPart};
use crate::lexer::lex;
use crate::manifest::Project;
use crate::parser::Parser;
use crate::token::Span;

/// A loaded compilation unit: the (possibly merged) program plus the source text used to render
/// type-error carets. `diag_src` is the single file's source in loose mode (full carets) or empty
/// for a merged multi-file unit, where no single source aligns — diagnostics then print message +
/// position without a source line (a deliberate flat-merge limitation; richer multi-file carets are
/// a later slice).
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

/// Assemble a project's compilation unit (M5 S2c). Two passes over every `.phg` under the source
/// root (plus the entry, if outside it):
///
/// 1. Parse + folder=path-validate each file; reject library-package types (S2c namespaces
///    *functions* only). Build the global function symbol table — `(package, name)` ⇒ a globally
///    unique **mangled** name (`acme.util` + `compute` ⇒ `Acme\Util\compute`); `package main` defs
///    keep their bare name (the auto-invoked entry + single-file byte-identity).
/// 2. Per file, rewrite call sites against that file's package + import map: a same-package bare
///    call becomes the mangled target (a no-op for `main`); a qualified user call `util.compute(x)`
///    (leaf `util` imported from a non-`core` package that defines `compute`) becomes a bare call
///    on the mangled name. Native (`core.*`) calls and unresolvable heads are left untouched. Then
///    all items merge into one flat program.
///
/// Because the rewrite produces concrete, globally-unique bare names *before* any backend runs, the
/// checker / interpreter / compiler / VM consume the result unchanged — run==runvm is structural.
/// Only the transpiler de-mangles the `\`-bearing names back into PHP `namespace` blocks.
fn load_project(entry: &Path, project: &Project) -> Result<Unit, String> {
    let mut files = collect_phg(&project.source_root)?;
    if !files.iter().any(|f| same_file(f, entry)) {
        files.push(entry.to_path_buf());
    }
    files.sort();
    files.dedup();

    // Pass 1 — parse, validate, and index every function by (package, name) ⇒ mangled global name.
    let mut parsed: Vec<(PathBuf, Program)> = Vec::with_capacity(files.len());
    let mut defined: HashMap<(String, String), String> = HashMap::new();
    for file in &files {
        let src = read_file(file)?;
        let prog = parse_at(file, &src)?;
        validate_folder_path(&prog, file, &project.source_root)?;
        reject_library_types(&prog, file)?;
        let pkg = prog.package.join(".");
        for item in &prog.items {
            if let Item::Function(f) = item {
                defined.insert(
                    (pkg.clone(), f.name.clone()),
                    mangle(&prog.package, &f.name),
                );
            }
        }
        parsed.push((file.clone(), prog));
    }

    // Pass 2 — resolve call sites per file, then flat-merge.
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

    for (file, prog) in parsed {
        if same_file(&file, entry) {
            unit_package = prog.package.clone();
            unit_span = prog.span;
        }
        let ctx = ResolveCtx {
            package: prog.package.clone(),
            user_imports: user_import_map(&prog.items),
            defined: &defined,
        };
        for item in prog.items {
            merged_items.push(resolve_item(item, &ctx));
        }
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

/// The globally-unique name for a top-level definition. `package main` (and the malformed empty
/// package) keep the bare name — so the entry stays byte-identical to a single-file program; any
/// other package is mangled to a PHP-FQN-shaped key (`acme.util` + `compute` ⇒ `Acme\Util\compute`),
/// which the transpiler later splits back into a `namespace Acme\Util` block.
fn mangle(package: &[String], name: &str) -> String {
    if package.is_empty() || package == ["main"] {
        return name.to_string();
    }
    let ns = package
        .iter()
        .map(|s| pascal(s))
        .collect::<Vec<_>>()
        .join("\\");
    format!("{ns}\\{name}")
}

/// PascalCase one package segment (`util` ⇒ `Util`) for the PHP namespace mapping (M5-2).
fn pascal(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A file's **user** import map: bound qualifier ⇒ target package segments, for non-`core` imports
/// only. Native (`core.*`) imports are excluded — their member calls stay native and are resolved by
/// the backends (and the transpiler) as before. An alias (`import a.b as c;`) binds `c`, else the
/// path's last segment.
fn user_import_map(items: &[Item]) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    for item in items {
        if let Item::Import { path, alias, .. } = item {
            if path.first().map(String::as_str) == Some("core") {
                continue;
            }
            let qualifier = alias.clone().or_else(|| path.last().cloned());
            if let Some(q) = qualifier {
                map.insert(q, path.clone());
            }
        }
    }
    map
}

/// S2c scope: a non-`main` (library) package may export functions only. A top-level `class`/`enum`
/// in a library package is rejected (`E-PKG-TYPE`) — cross-package type namespacing is an M5
/// follow-up. `package main` (and the empty package, left to the checker) may define anything.
fn reject_library_types(prog: &Program, file: &Path) -> Result<(), String> {
    if prog.package.is_empty() || prog.package == ["main"] {
        return Ok(());
    }
    for item in &prog.items {
        let kind = match item {
            Item::Class(_) => "class",
            Item::Enum(_) => "enum",
            _ => continue,
        };
        return Err(format!(
            "{}: a {kind} in the library package `{}` is not yet supported — S2c namespaces \
             functions only; move the type to `package main` (or await the M5 follow-up) [E-PKG-TYPE]",
            file.display(),
            prog.package.join(".")
        ));
    }
    Ok(())
}

/// The resolution context for one file: its package (caller side of a bare call), its user-import
/// map (for qualified calls), and the shared global symbol table.
struct ResolveCtx<'a> {
    package: Vec<String>,
    user_imports: HashMap<String, Vec<String>>,
    defined: &'a HashMap<(String, String), String>,
}

/// Rewrite one top-level item: rename a function to its mangled global name and resolve its body;
/// resolve a class's method/constructor bodies (a class is always `package main` — library types
/// are rejected upstream). Enums/imports/aliases have no call sites to rewrite.
fn resolve_item(item: Item, ctx: &ResolveCtx) -> Item {
    match item {
        Item::Function(mut f) => {
            f.name = mangle(&ctx.package, &f.name);
            f.body = resolve_block(f.body, ctx);
            Item::Function(f)
        }
        Item::Class(mut c) => {
            for m in &mut c.members {
                match m {
                    ClassMember::Method(f) => {
                        let body = std::mem::take(&mut f.body);
                        f.body = resolve_block(body, ctx);
                    }
                    ClassMember::Constructor { body, .. } => {
                        let b = std::mem::take(body);
                        *body = resolve_block(b, ctx);
                    }
                    ClassMember::Field { .. } => {}
                }
            }
            Item::Class(c)
        }
        other => other,
    }
}

fn resolve_block(stmts: Vec<Stmt>, ctx: &ResolveCtx) -> Vec<Stmt> {
    stmts.into_iter().map(|s| resolve_stmt(s, ctx)).collect()
}

fn resolve_stmt(stmt: Stmt, ctx: &ResolveCtx) -> Stmt {
    match stmt {
        Stmt::VarDecl {
            ty,
            name,
            init,
            span,
        } => Stmt::VarDecl {
            ty,
            name,
            init: resolve_expr(init, ctx),
            span,
        },
        Stmt::Return { value, span } => Stmt::Return {
            value: value.map(|e| resolve_expr(e, ctx)),
            span,
        },
        Stmt::If {
            cond,
            bind,
            then_block,
            else_block,
            span,
        } => Stmt::If {
            cond: resolve_expr(cond, ctx),
            bind,
            then_block: resolve_block(then_block, ctx),
            else_block: else_block.map(|b| resolve_block(b, ctx)),
            span,
        },
        Stmt::For {
            ty,
            name,
            iter,
            body,
            span,
        } => Stmt::For {
            ty,
            name,
            iter: resolve_expr(iter, ctx),
            body: resolve_block(body, ctx),
            span,
        },
        Stmt::Block(stmts, span) => Stmt::Block(resolve_block(stmts, ctx), span),
        Stmt::Expr(e, span) => Stmt::Expr(resolve_expr(e, ctx), span),
    }
}

fn resolve_expr(expr: Expr, ctx: &ResolveCtx) -> Expr {
    match expr {
        Expr::Call { callee, args, span } => resolve_call(*callee, args, span, ctx),
        Expr::Member {
            object,
            name,
            safe,
            span,
        } => Expr::Member {
            object: Box::new(resolve_expr(*object, ctx)),
            name,
            safe,
            span,
        },
        Expr::Index {
            object,
            index,
            span,
        } => Expr::Index {
            object: Box::new(resolve_expr(*object, ctx)),
            index: Box::new(resolve_expr(*index, ctx)),
            span,
        },
        Expr::Unary { op, expr, span } => Expr::Unary {
            op,
            expr: Box::new(resolve_expr(*expr, ctx)),
            span,
        },
        Expr::Binary { op, lhs, rhs, span } => Expr::Binary {
            op,
            lhs: Box::new(resolve_expr(*lhs, ctx)),
            rhs: Box::new(resolve_expr(*rhs, ctx)),
            span,
        },
        Expr::Force { inner, span } => Expr::Force {
            inner: Box::new(resolve_expr(*inner, ctx)),
            span,
        },
        Expr::List(items, span) => Expr::List(
            items.into_iter().map(|e| resolve_expr(e, ctx)).collect(),
            span,
        ),
        Expr::Str(parts, span) => Expr::Str(
            parts
                .into_iter()
                .map(|p| match p {
                    StrPart::Expr(e) => StrPart::Expr(Box::new(resolve_expr(*e, ctx))),
                    lit => lit,
                })
                .collect(),
            span,
        ),
        Expr::Match {
            scrutinee,
            arms,
            span,
        } => Expr::Match {
            scrutinee: Box::new(resolve_expr(*scrutinee, ctx)),
            arms: arms
                .into_iter()
                .map(|a| MatchArm {
                    pattern: a.pattern,
                    body: resolve_expr(a.body, ctx),
                    span: a.span,
                })
                .collect(),
            span,
        },
        Expr::Range {
            start,
            end,
            inclusive,
            span,
        } => Expr::Range {
            start: Box::new(resolve_expr(*start, ctx)),
            end: Box::new(resolve_expr(*end, ctx)),
            inclusive,
            span,
        },
        Expr::If {
            cond,
            then_expr,
            else_expr,
            span,
        } => Expr::If {
            cond: Box::new(resolve_expr(*cond, ctx)),
            then_expr: Box::new(resolve_expr(*then_expr, ctx)),
            else_expr: Box::new(resolve_expr(*else_expr, ctx)),
            span,
        },
        // Leaves carry no nested call site: Int / Float / Bool / Null / Ident / This.
        leaf => leaf,
    }
}

/// Resolve a call. A bare `Ident` head resolves against the caller's own package (mangled if that
/// package is non-`main`; a no-op for `main`, and for variants/classes/unknowns which aren't in the
/// function table). A `Member` head `q.name` is a qualified user call iff `q` is a non-`core` import
/// leaf whose target package defines `name` — rewritten to a bare call on the mangled name;
/// otherwise it is a native call or a method on a value and is left intact (receiver resolved).
fn resolve_call(callee: Expr, args: Vec<Expr>, span: Span, ctx: &ResolveCtx) -> Expr {
    let args: Vec<Expr> = args.into_iter().map(|a| resolve_expr(a, ctx)).collect();
    match callee {
        Expr::Ident(n, isp) => {
            let mangled = ctx
                .defined
                .get(&(ctx.package.join("."), n.clone()))
                .cloned()
                .unwrap_or(n);
            Expr::Call {
                callee: Box::new(Expr::Ident(mangled, isp)),
                args,
                span,
            }
        }
        Expr::Member {
            object,
            name,
            safe,
            span: msp,
        } => {
            if !safe {
                if let Expr::Ident(q, _) = object.as_ref() {
                    if let Some(target) = ctx.user_imports.get(q) {
                        if let Some(mangled) = ctx.defined.get(&(target.join("."), name.clone())) {
                            return Expr::Call {
                                callee: Box::new(Expr::Ident(mangled.clone(), msp)),
                                args,
                                span,
                            };
                        }
                    }
                }
            }
            Expr::Call {
                callee: Box::new(Expr::Member {
                    object: Box::new(resolve_expr(*object, ctx)),
                    name,
                    safe,
                    span: msp,
                }),
                args,
                span,
            }
        }
        other => Expr::Call {
            callee: Box::new(resolve_expr(other, ctx)),
            args,
            span,
        },
    }
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
