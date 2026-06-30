//! Multi-file project loader + cross-package name resolution (M5 S2b/S2c).
//!
//! Turns an entry source into a single [`Unit`] (one [`Program`] ready for check + run) and
//! enforces the project structure that the package declaration alone cannot:
//!
//! - **Project mode** — a `phorj.toml` found by walking up from the entry ([`crate::manifest`])
//!   marks the project root. Every `.phg` under the source root is parsed, its package is validated
//!   against its location (**folder = package**, Go's model — `src/acme/util/*.phg` ⇒ `package
//!   acme.util`; `package Main` is folder-exempt and may live anywhere). A resolution pass then
//!   mangles every non-`main` definition to a globally-unique name (`acme.util` + `compute` ⇒
//!   `Acme\Util\compute`) and rewrites call sites — same-package bare calls and qualified user calls
//!   (`util.compute(x)`, via the per-file import map) become bare calls on the mangled name; native
//!   `core.*` calls are untouched (S2c). All items then merge into one flat [`Program`]. Because the
//!   rewrite produces concrete bare names *before* any backend runs, the checker/interpreter/
//!   compiler/VM are unchanged (run==runvm is structural); only the transpiler de-mangles the
//!   `\`-bearing names back into PHP `namespace` blocks. A single-package program has no mangled
//!   names, so it is byte-identical to the pre-S2c output.
//! - **Loose-script mode** — no manifest above the entry. Only `package Main;` is legal (a dotted
//!   library package requires a project); folder = path is suspended.
//!
//! Enforcement and resolution live here (path-aware), never in the type checker, so
//! `cli::cmd_run(&str)`, the differential harness, and the checker's package-agnostic tests are
//! untouched. Library packages export **functions and types** (M-RT cross-package types): a non-`main`
//! `class`/`enum`/`interface` is mangled like a function (`acme.geometry` + `Point` ⇒
//! `Acme\Geometry\Point`) and a consuming file binds it with `import type a.b.C [as D];`; the same
//! Pass-2 rewrite that mangles call sites also rewrites every type-name position (annotations,
//! instantiation, `instanceof`, enum access) to the mangled FQN, so the backends see fully-resolved
//! names and only the transpiler de-mangles into PHP `namespace` blocks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::{
    ClassMember, Expr, Item, LambdaBody, MatchArm, Param, Program, Stmt, StrPart, Type, Visibility,
};
use crate::diagnostic::Diagnostic;
use crate::lexer::lex;
use crate::manifest::{validate_path_component, Project};
use crate::parser::Parser;
use crate::token::Span;

// Cohesion split (M-Decomp W3.2): resolution walkers + fs helpers in sibling files.
mod fs;
mod resolve;
use fs::*;
use resolve::*;

/// Provenance for one top-level definition: where it was declared and how visible it is. Built in
/// Pass 1 (which still has per-file information) and consumed by the visibility lattice during Pass 2.
#[derive(Clone)]
struct DefInfo {
    file: PathBuf,
    package: String,
    vis: Visibility,
}

/// The visibility lattice check. `None` ⇒ the reference is legal; `Some(code)` ⇒ the diagnostic code
/// to report. Same file → always legal. Same package, different file → legal unless `private`.
/// Different package → legal only if `public`.
fn vis_violation(info: &DefInfo, referrer_file: &Path, referrer_pkg: &str) -> Option<&'static str> {
    if info.file == referrer_file {
        return None;
    }
    if info.package == referrer_pkg {
        return if info.vis == Visibility::Private {
            Some("E-VIS-PRIVATE")
        } else {
            None
        };
    }
    match info.vis {
        Visibility::Public => None,
        Visibility::Internal => Some("E-VIS-INTERNAL"),
        Visibility::Private => Some("E-VIS-PRIVATE"),
    }
}

/// Render the visibility keyword for a diagnostic.
fn vis_word(vis: Visibility) -> &'static str {
    match vis {
        Visibility::Public => "public",
        Visibility::Internal => "internal",
        Visibility::Private => "private",
    }
}

/// A loaded compilation unit: the (possibly merged) program plus the source text used to render
/// type-error carets. `diag_src` is the single file's source in loose mode (full carets) or empty
/// for a merged multi-file unit, where no single source aligns — diagnostics then print message +
/// position without a source line (a deliberate flat-merge limitation; richer multi-file carets are
/// a later slice).
#[derive(Debug, Clone)]
pub struct Unit {
    pub program: Program,
    pub diag_src: String,
    /// Project-load statistics (project mode only; `None` in loose mode). Lets `phg check` report the
    /// *scope* it validated — proving the whole project (every file, including code no route reaches,
    /// plus vendored deps) was type-checked, the PHP-absent superpower of whole-program checking.
    pub stats: Option<LoadStats>,
    /// Per-file source text (project mode), for runtime stack-trace carets. Empty in loose mode (the
    /// single source rides on `diag_src`). Keyed by the file path shown in a `Frame.file`.
    pub sources: std::collections::HashMap<PathBuf, String>,
    /// Function (compiled/mangled) name → origin file, for attributing trace frames to a file
    /// (error-handling slice 1). Covers free functions (incl. `main`); methods/ctors — whose frame
    /// names are backend-synthesized (`Class::m`) — are not keyed here and show line-only.
    pub fn_files: std::collections::HashMap<String, PathBuf>,
}

impl Unit {
    /// Attribute each runtime trace frame to its origin file via [`Unit::fn_files`] (no-op in loose
    /// mode / for backend-synthesized method frames). Returns the source text to render the fault
    /// caret against — the innermost frame's file source in project mode, else `diag_src`.
    #[must_use]
    pub fn attribute_frames(&self, diag: &mut Diagnostic) -> String {
        for f in &mut diag.frames {
            if f.file.is_none() {
                f.file = self.fn_files.get(&f.function).cloned();
            }
        }
        diag.frames
            .first()
            .and_then(|f| f.file.as_ref())
            .and_then(|p| self.sources.get(p))
            .cloned()
            .unwrap_or_else(|| self.diag_src.clone())
    }
}

/// Counts of what a project load assembled and handed to the checker — every `.phg` under the source
/// root (first-party + vendored), merged and validated as one program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadStats {
    pub files: usize,
    pub packages: usize,
    pub defs: usize,
}

impl LoadStats {
    /// A one-line human summary for `phg check`'s success message.
    pub fn summary(&self) -> String {
        format!(
            "OK — whole project type-checks clean: {} file{}, {} package{}, {} definition{} \
             validated (every file + vendored deps)\n",
            self.files,
            plural(self.files),
            self.packages,
            plural(self.packages),
            self.defs,
            plural(self.defs),
        )
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Recursively collect every `*.phg` under `dir` (sorted, deterministic). Public wrapper over the
/// internal walker, used by the `phg test` runner (M-Test T3) to discover test files. An empty Vec
/// for a non-directory or empty tree.
pub fn discover_phg(dir: &Path) -> Result<Vec<PathBuf>, String> {
    collect_phg(dir)
}

/// Load the entry at `path`: project mode if a `phorj.toml` is found by walking up, else loose mode.
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
/// project above it). Enforces the reserved `package Main;` — a dotted package needs a project.
pub fn load_loose_src(src: &str) -> Result<Unit, String> {
    let program = parse_one(src)?;
    enforce_loose_main(&program)?;
    Ok(Unit {
        program,
        diag_src: src.to_string(),
        stats: None,
        sources: std::collections::HashMap::new(),
        fn_files: std::collections::HashMap::new(),
    })
}

/// Assemble a project's compilation unit (M5 S2c). Two passes over every `.phg` under the source
/// root (plus the entry, if outside it):
///
/// 1. Parse + folder=path-validate each file; reject library-package types (S2c namespaces
///    *functions* only). Build the global function symbol table — `(package, name)` ⇒ a globally
///    unique **mangled** name (`acme.util` + `compute` ⇒ `Acme\Util\compute`); `package Main` defs
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
    // Each source carries the folder=path root it is validated against (the project's source root
    // for first-party files; each dependency's own `vendor/<name>/` root for vendored files) and a
    // `vendored` flag (a vendored package must be a library — never `package Main`).
    let vendor_root = project.root.join("vendor");
    let mut sources: Vec<Source> = Vec::new();
    for f in collect_phg(&project.source_root)? {
        // Defensive: if `source = "."`, the vendor tree sits under the source root — never compile a
        // vendored file as a first-party one (it is added with its own root below instead).
        if f.starts_with(&vendor_root) {
            continue;
        }
        sources.push(Source::first_party(f, &project.source_root));
    }
    if !sources.iter().any(|s| same_file(&s.file, entry)) {
        sources.push(Source::first_party(
            entry.to_path_buf(),
            &project.source_root,
        ));
    }
    // Vendored dependencies (M5 S3): consulted only when `[require]` is non-empty, always offline —
    // each declared dependency must already be vendored under `vendor/<name>/` (run `phg vendor`).
    for dep in &project.manifest.require {
        // Defensive re-check before joining the name onto a path (validated at parse time too) — a
        // traversal name must never reach `collect_phg` on an out-of-tree directory (GA blocker B2).
        validate_path_component("dependency name", &dep.name)?;
        let dep_root = vendor_root.join(&dep.name);
        let dep_files = collect_phg(&dep_root)?;
        if dep_files.is_empty() {
            return Err(format!(
                "dependency `{}` is declared in [require] but not vendored — run `phg vendor` \
                 (no `.phg` source found under `{}`) [E-VENDOR-MISSING]",
                dep.name,
                dep_root.display()
            ));
        }
        for f in dep_files {
            sources.push(Source::vendored(f, &dep_root));
        }
    }
    sources.sort_by(|a, b| a.file.cmp(&b.file));
    sources.dedup_by(|a, b| a.file == b.file);

    // Pass 1 — parse, validate, and index every top-level definition by (package, name) ⇒ mangled
    // global name. Functions and types live in separate symbol tables (PHP namespaces functions and
    // classes separately), so a `compute` function and a `Compute` type never collide. Library
    // packages may now declare types (the old `E-PKG-TYPE` gate is retired — cross-package types).
    let mut parsed: Vec<(PathBuf, Program)> = Vec::with_capacity(sources.len());
    let mut defined: HashMap<(String, String), String> = HashMap::new();
    let mut types: HashMap<(String, String), String> = HashMap::new();
    // Declaration-visibility provenance (visibility modifiers): where each definition lives + its
    // visibility, keyed by (package, name) like the rename tables. Consumed by the lattice in Pass 2.
    let mut prov_fns: HashMap<(String, String), DefInfo> = HashMap::new();
    let mut prov_types: HashMap<(String, String), DefInfo> = HashMap::new();
    // Whole-project scope counters for `phg check`'s success summary.
    let mut pkgset: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut defs: usize = 0;
    // Trace-attribution maps (error-handling slice 1): per-file source + function → file.
    let mut src_map: HashMap<PathBuf, String> = HashMap::new();
    let mut fn_files: HashMap<String, PathBuf> = HashMap::new();
    for src_entry in &sources {
        let file = &src_entry.file;
        let src = read_file(file)?;
        src_map.insert(file.clone(), src.clone());
        let prog = parse_at(file, &src)?;
        validate_folder_path(&prog, file, &src_entry.root)?;
        validate_public_surface(&prog, file)?;
        if src_entry.vendored && (prog.package.is_empty() || prog.package == ["Main"]) {
            return Err(format!(
                "{}: a vendored dependency is a library and cannot declare `package Main` \
                 (it would collide with the consumer's entry) [E-VENDOR-MAIN]",
                file.display()
            ));
        }
        let pkg = prog.package.join(".");
        pkgset.insert(if pkg.is_empty() {
            "main".to_string()
        } else {
            pkg.clone()
        });
        for item in &prog.items {
            let (name, is_type, vis) = match item {
                Item::Function(f) => (&f.name, false, f.vis),
                Item::Class(c) => (&c.name, true, c.vis),
                Item::Enum(e) => (&e.name, true, e.vis),
                Item::Interface(i) => (&i.name, true, i.vis),
                // A trait is a public named symbol in the type namespace (it carries no visibility
                // modifier — always public reuse). Register it so a cross-package `import type` +
                // `use T;` can resolve and mangle it to its FQN, exactly like a class/interface.
                Item::Trait(t) => (&t.name, true, crate::ast::Visibility::Public),
                _ => continue,
            };
            let table = if is_type { &mut types } else { &mut defined };
            if table
                .insert((pkg.clone(), name.clone()), mangle(&prog.package, name))
                .is_some()
            {
                return Err(format!(
                    "{}: duplicate definition of `{}` in package `{}` \
                     (a name must be unique within its package) [E-DUP-DEF]",
                    file.display(),
                    name,
                    if pkg.is_empty() { "main" } else { &pkg }
                ));
            }
            let prov = if is_type {
                &mut prov_types
            } else {
                &mut prov_fns
            };
            prov.insert(
                (pkg.clone(), name.clone()),
                DefInfo {
                    file: file.clone(),
                    package: pkg.clone(),
                    vis,
                },
            );
            // A free function's trace frame is keyed by its compiled (mangled) name — map it to its
            // file so a runtime trace can show `file:line` (methods/ctors are synthesized elsewhere).
            if !is_type {
                fn_files.insert(mangle(&prog.package, name), file.clone());
            }
            defs += 1;
        }
        parsed.push((file.clone(), prog));
    }

    // M8.5 S3b — ambient `*.d.phg` declaration files: a file of foreign `declare`s carrying no package,
    // loaded into the project (the `.d.ts` analog). Parsed + validated (no package, all foreign) but
    // NOT folder=path-validated and NOT indexed as package definitions; their foreign items merge
    // ambiently into the unit (the checker's prebind makes merge order irrelevant) and are emitted by
    // the transpiler as global `\Name` symbols. First-party only — vendored decl bundling is deferred.
    // Excluded from `collect_phg`, so a decl file is never compiled as a package source.
    let mut decl_items: Vec<Item> = Vec::new();
    let mut decl_files = 0usize;
    for f in collect_decl_phg(&project.source_root)? {
        if f.starts_with(&vendor_root) {
            continue;
        }
        let src = read_file(&f)?;
        let prog = parse_at(&f, &src)?;
        validate_decl_file(&prog, &f)?;
        src_map.insert(f.clone(), src);
        decl_items.extend(prog.items);
        decl_files += 1;
    }

    let stats = LoadStats {
        files: sources.len() + decl_files,
        packages: pkgset.len(),
        defs,
    };

    // Pass 2 — resolve call sites per file, then flat-merge.
    let mut merged_items: Vec<Item> = Vec::new();
    // The merged unit runs as the entry's package (normally `main`); its span anchors any
    // program-level diagnostic.
    let mut unit_package: Vec<String> = vec!["Main".to_string()];
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
        let user_imports = user_import_map(&prog.items);
        let type_imports = build_type_imports(&prog, &types, &prov_types, &user_imports, &file)?;
        let ctx = ResolveCtx {
            package: prog.package.clone(),
            user_imports,
            defined: &defined,
            types: &types,
            type_imports,
            file: &file,
            prov_types: &prov_types,
            prov_fns: &prov_fns,
            violations: RefCell::new(Vec::new()),
        };
        for item in prog.items {
            merged_items.push(resolve_item(item, &ctx));
        }
        // Surface the first visibility violation collected while resolving this file (the
        // infallible `resolve_*` chain buffers them).
        if let Some(first) = ctx.violations.into_inner().into_iter().next() {
            return Err(first);
        }
    }

    // Ambient foreign declarations merge unmangled (they are global PHP symbols — never namespaced).
    merged_items.extend(decl_items);

    Ok(Unit {
        program: Program {
            package: unit_package,
            items: merged_items,
            span: unit_span,
        },
        diag_src: String::new(),
        stats: Some(stats),
        sources: src_map,
        fn_files,
    })
}

/// One source file in a project load, paired with the folder=path root it validates against and
/// whether it came from the vendor tree (a vendored file must be a library — never `package Main`).
struct Source {
    file: PathBuf,
    root: PathBuf,
    vendored: bool,
}

impl Source {
    fn first_party(file: PathBuf, source_root: &Path) -> Source {
        Source {
            file,
            root: source_root.to_path_buf(),
            vendored: false,
        }
    }
    fn vendored(file: PathBuf, dep_root: &Path) -> Source {
        Source {
            file,
            root: dep_root.to_path_buf(),
            vendored: true,
        }
    }
}

/// The globally-unique name for a top-level definition. `package Main` (and the malformed empty
/// package) keep the bare name — so the entry stays byte-identical to a single-file program; any
/// other package is mangled to a PHP-FQN-shaped key (`acme.util` + `compute` ⇒ `Acme\Util\compute`),
/// which the transpiler later splits back into a `namespace Acme\Util` block.
fn mangle(package: &[String], name: &str) -> String {
    if package.is_empty() || package == ["Main"] {
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

/// A file's **user** import map: bound qualifier ⇒ target package segments, for non-`Core` imports
/// only. Native (`Core.*`) imports are excluded — their member calls stay native and are resolved by
/// the backends (and the transpiler) as before. An alias (`import a.b as c;`) binds `c`, else the
/// path's last segment.
fn user_import_map(items: &[Item]) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    for item in items {
        if let Item::Import {
            path,
            alias,
            type_only: false,
            ..
        } = item
        {
            if path.first().map(String::as_str) == Some("Core") {
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

/// Build a file's **type-import map**: bare name (or `as` alias) ⇒ the mangled FQN of a cross-package
/// type, from each `import type a.b.C [as D];`. Validates against the global `types` table and the
/// file's own definitions / module imports (cross-package types, M-RT generics-all):
/// - `E-TYPE-IMPORT-BUILTIN` — the leaf is a built-in type (`List`/`Map`/`Set`/scalars); built-ins
///   are import-free, like `int`.
/// - `E-TYPE-IMPORT-UNKNOWN` — the package exports no such type.
/// - `E-TYPE-IMPORT-CONFLICT` — two terminal imports bind the same bare name (alias one with `as`).
/// - `E-TYPE-IMPORT-SHADOW` — the bound name collides with a local type in this file or a module-import
///   qualifier (the two import kinds stay disjoint, the `E-SHADOW-IMPORT` discipline).
fn build_type_imports(
    prog: &Program,
    types: &HashMap<(String, String), String>,
    prov_types: &HashMap<(String, String), DefInfo>,
    user_imports: &HashMap<String, Vec<String>>,
    file: &Path,
) -> Result<HashMap<String, String>, String> {
    // The file's own type names (collide → SHADOW). A `package Main` file's types are its locals.
    let local_types: std::collections::HashSet<&str> = prog
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Class(c) => Some(c.name.as_str()),
            Item::Enum(e) => Some(e.name.as_str()),
            Item::Interface(i) => Some(i.name.as_str()),
            Item::Trait(t) => Some(t.name.as_str()),
            _ => None,
        })
        .collect();
    let mut map: HashMap<String, String> = HashMap::new();
    for item in &prog.items {
        if let Item::Import {
            path,
            alias,
            type_only: true,
            ..
        } = item
        {
            let (leaf, pkg_segs) = match path.split_last() {
                Some((leaf, pkg)) if !pkg.is_empty() => (leaf, pkg),
                _ => {
                    return Err(format!(
                        "{}: `import type` needs a package-qualified type (e.g. \
                         `import type acme.geometry.Point;`) [E-TYPE-IMPORT-UNKNOWN]",
                        file.display()
                    ))
                }
            };
            if is_builtin_type_leaf(leaf) {
                return Err(format!(
                    "{}: `{leaf}` is a built-in type and needs no import (built-ins are \
                     import-free, like `int`) [E-TYPE-IMPORT-BUILTIN]",
                    file.display()
                ));
            }
            let pkg = pkg_segs.join(".");
            let mangled = types.get(&(pkg.clone(), leaf.clone())).ok_or_else(|| {
                format!(
                    "{}: package `{pkg}` exports no type `{leaf}` [E-TYPE-IMPORT-UNKNOWN]",
                    file.display()
                )
            })?;
            // Visibility: a cross-package `import type` may only reach a `public` type.
            if let Some(info) = prov_types.get(&(pkg.clone(), leaf.clone())) {
                if let Some(code) = vis_violation(info, file, &prog.package.join(".")) {
                    return Err(format!(
                        "{}: type `{leaf}` is not visible from package `{}` — it is `{}` in package \
                         `{pkg}`; mark it `public` to export it [{code}]",
                        file.display(),
                        prog.package.join("."),
                        vis_word(info.vis),
                    ));
                }
            }
            let bound = alias.clone().unwrap_or_else(|| leaf.clone());
            if local_types.contains(bound.as_str()) || user_imports.contains_key(&bound) {
                return Err(format!(
                    "{}: imported type `{bound}` shadows a local type or an imported module \
                     qualifier — alias it with `as` [E-TYPE-IMPORT-SHADOW]",
                    file.display()
                ));
            }
            if map.insert(bound.clone(), mangled.clone()).is_some() {
                return Err(format!(
                    "{}: two `import type` bind the name `{bound}` — alias one with `as` \
                     [E-TYPE-IMPORT-CONFLICT]",
                    file.display()
                ));
            }
        }
    }
    Ok(map)
}

/// Built-in type names that are import-free (resolved by the checker/compiler, not a package member).
/// An `import type` naming one of these is `E-TYPE-IMPORT-BUILTIN`.
fn is_builtin_type_leaf(name: &str) -> bool {
    matches!(
        name,
        "int" | "float" | "bool" | "string" | "bytes" | "void" | "empty" | "List" | "Map" | "Set"
    )
}

/// The resolution context for one file: its package (caller side of a bare call), its user-import
/// map (for qualified calls), and the shared global symbol table.
struct ResolveCtx<'a> {
    package: Vec<String>,
    user_imports: HashMap<String, Vec<String>>,
    defined: &'a HashMap<(String, String), String>,
    /// Global type symbol table `(package, type) ⇒ mangled FQN` — for resolving a same-package
    /// sibling type reference inside a library package.
    types: &'a HashMap<(String, String), String>,
    /// This file's terminal type imports: bare name (or `as` alias) ⇒ mangled FQN.
    type_imports: HashMap<String, String>,
    /// The file currently being resolved (the referrer side of the visibility lattice).
    file: &'a Path,
    /// Visibility provenance for type and function definitions (visibility modifiers).
    prov_types: &'a HashMap<(String, String), DefInfo>,
    prov_fns: &'a HashMap<(String, String), DefInfo>,
    /// Visibility violations collected while resolving this file's references (the `resolve_*` chain
    /// is infallible, so violations are buffered here and surfaced after the file is resolved).
    violations: RefCell<Vec<String>>,
}

#[cfg(test)]
mod tests;
