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
    // Each source carries the folder=path root it is validated against (the project's source root
    // for first-party files; each dependency's own `vendor/<name>/` root for vendored files) and a
    // `vendored` flag (a vendored package must be a library — never `package main`).
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
        if src_entry.vendored && (prog.package.is_empty() || prog.package == ["main"]) {
            return Err(format!(
                "{}: a vendored dependency is a library and cannot declare `package main` \
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
    let stats = LoadStats {
        files: sources.len(),
        packages: pkgset.len(),
        defs,
    };

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
/// whether it came from the vendor tree (a vendored file must be a library — never `package main`).
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
    // The file's own type names (collide → SHADOW). A `package main` file's types are its locals.
    let local_types: std::collections::HashSet<&str> = prog
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Class(c) => Some(c.name.as_str()),
            Item::Enum(e) => Some(e.name.as_str()),
            Item::Interface(i) => Some(i.name.as_str()),
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
        "int" | "float" | "bool" | "string" | "bytes" | "List" | "Map" | "Set"
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

/// Resolve a type *name* to its mangled FQN, or `None` if it is a local (`package main`) type or a
/// built-in (left bare). A terminal `import type` binding wins; otherwise a same-package sibling type
/// (a library type referencing another type in its own package).
fn resolve_type_ref(name: &str, ctx: &ResolveCtx) -> Option<String> {
    if let Some(m) = ctx.type_imports.get(name) {
        // Cross-package terminal import — already visibility-checked in `build_type_imports`.
        return Some(m.clone());
    }
    let key = (ctx.package.join("."), name.to_string());
    if let Some(m) = ctx.types.get(&key) {
        // Same-package sibling type: enforce file-scoped `private` (visibility modifiers). Here the
        // referrer and definition share a package, so the lattice only ever yields `E-VIS-PRIVATE`.
        if let Some(info) = ctx.prov_types.get(&key) {
            if let Some(code) = vis_violation(info, ctx.file, &ctx.package.join(".")) {
                ctx.violations.borrow_mut().push(format!(
                    "{}: type `{name}` is private to `{}` — mark it `internal` (package-wide) or \
                     `public` (everywhere) to use it from another file [{code}]",
                    ctx.file.display(),
                    info.file.display(),
                ));
            }
        }
        return Some(m.clone());
    }
    None
}

/// Rewrite every type *name* inside a type annotation to its mangled FQN (cross-package types).
/// Mirrors the exhaustive `Type` walk of `checker::erase_generics`'s `rty`; recurses through generic
/// arguments, optionals, and function types so a `List<Point>` or `(Point) -> Point` resolves too.
fn resolve_type(ty: &Type, ctx: &ResolveCtx) -> Type {
    match ty {
        Type::Named { name, args, span } => Type::Named {
            name: resolve_type_ref(name, ctx).unwrap_or_else(|| name.clone()),
            args: args.iter().map(|a| resolve_type(a, ctx)).collect(),
            span: *span,
        },
        Type::Optional { inner, span } => Type::Optional {
            inner: Box::new(resolve_type(inner, ctx)),
            span: *span,
        },
        Type::Function { params, ret, span } => Type::Function {
            params: params.iter().map(|p| resolve_type(p, ctx)).collect(),
            ret: Box::new(resolve_type(ret, ctx)),
            span: *span,
        },
        // A union resolves each member (a cross-package member name mangles like anywhere else), M-RT S4.
        Type::Union(members, span) => Type::Union(
            members.iter().map(|m| resolve_type(m, ctx)).collect(),
            *span,
        ),
        // An intersection resolves each member likewise (M-RT S5).
        Type::Intersection(members, span) => Type::Intersection(
            members.iter().map(|m| resolve_type(m, ctx)).collect(),
            *span,
        ),
        Type::Infer(s) => Type::Infer(*s),
        Type::Erased(s) => Type::Erased(*s),
    }
}

/// Rewrite one top-level item: rename a function to its mangled global name and resolve its body;
/// resolve a class's method/constructor bodies (a class is always `package main` — library types
/// are rejected upstream). Enums/imports/aliases have no call sites to rewrite.
fn resolve_item(item: Item, ctx: &ResolveCtx) -> Item {
    match item {
        Item::Function(mut f) => {
            f.name = mangle(&ctx.package, &f.name);
            for p in &mut f.params {
                p.ty = resolve_type(&p.ty, ctx);
            }
            f.ret = f.ret.as_ref().map(|r| resolve_type(r, ctx));
            f.body = resolve_block(f.body, ctx);
            Item::Function(f)
        }
        Item::Class(mut c) => {
            c.name = mangle(&ctx.package, &c.name);
            for imp in &mut c.implements {
                if let Some(m) = resolve_type_ref(imp, ctx) {
                    *imp = m;
                }
            }
            for m in &mut c.members {
                match m {
                    ClassMember::Method(f) => {
                        for p in &mut f.params {
                            p.ty = resolve_type(&p.ty, ctx);
                        }
                        f.ret = f.ret.as_ref().map(|r| resolve_type(r, ctx));
                        let body = std::mem::take(&mut f.body);
                        f.body = resolve_block(body, ctx);
                    }
                    ClassMember::Constructor { params, body, .. } => {
                        for p in params.iter_mut() {
                            p.ty = resolve_type(&p.ty, ctx);
                        }
                        let b = std::mem::take(body);
                        *body = resolve_block(b, ctx);
                    }
                    ClassMember::Field { ty, .. } => {
                        *ty = resolve_type(ty, ctx);
                    }
                    // A property hook (M-mut.7b) carries a type plus a `get` expression and/or a
                    // `set` block — each of which may name cross-package types or call cross-package
                    // functions, so resolve them exactly like a method body (mangle + type-rewrite).
                    ClassMember::Hook { ty, get, set, .. } => {
                        *ty = resolve_type(ty, ctx);
                        if let Some(e) = get.take() {
                            *get = Some(resolve_expr(e, ctx));
                        }
                        if let Some((p, body)) = set.take() {
                            let pty = resolve_type(&p.ty, ctx);
                            *set = Some((
                                Param {
                                    ty: pty,
                                    name: p.name,
                                    span: p.span,
                                },
                                resolve_block(body, ctx),
                            ));
                        }
                    }
                }
            }
            Item::Class(c)
        }
        Item::Enum(mut e) => {
            e.name = mangle(&ctx.package, &e.name);
            for v in &mut e.variants {
                for p in &mut v.fields {
                    p.ty = resolve_type(&p.ty, ctx);
                }
            }
            Item::Enum(e)
        }
        Item::Interface(mut i) => {
            i.name = mangle(&ctx.package, &i.name);
            for ext in &mut i.extends {
                if let Some(m) = resolve_type_ref(ext, ctx) {
                    *ext = m;
                }
            }
            for m in &mut i.methods {
                for p in &mut m.params {
                    p.ty = resolve_type(&p.ty, ctx);
                }
                m.ret = m.ret.as_ref().map(|r| resolve_type(r, ctx));
            }
            Item::Interface(i)
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
            mutable,
            span,
        } => Stmt::VarDecl {
            ty: resolve_type(&ty, ctx),
            name,
            init: resolve_expr(init, ctx),
            mutable,
            span,
        },
        Stmt::Assign {
            target,
            value,
            span,
        } => Stmt::Assign {
            target: resolve_expr(target, ctx),
            value: resolve_expr(value, ctx),
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
            ty: resolve_type(&ty, ctx),
            name,
            iter: resolve_expr(iter, ctx),
            body: resolve_block(body, ctx),
            span,
        },
        Stmt::While {
            cond,
            body,
            post_cond,
            span,
        } => Stmt::While {
            cond: resolve_expr(cond, ctx),
            body: resolve_block(body, ctx),
            post_cond,
            span,
        },
        Stmt::CFor {
            init,
            cond,
            step,
            body,
            span,
        } => Stmt::CFor {
            init: init.map(|s| Box::new(resolve_stmt(*s, ctx))),
            cond: cond.map(|e| resolve_expr(e, ctx)),
            step: step.map(|s| Box::new(resolve_stmt(*s, ctx))),
            body: resolve_block(body, ctx),
            span,
        },
        Stmt::Break(span) => Stmt::Break(span),
        Stmt::Continue(span) => Stmt::Continue(span),
        Stmt::Block(stmts, span) => Stmt::Block(resolve_block(stmts, ctx), span),
        Stmt::Expr(e, span) => Stmt::Expr(resolve_expr(e, ctx), span),
        Stmt::Throw { value, span } => Stmt::Throw {
            value: resolve_expr(value, ctx),
            span,
        },
        Stmt::Try {
            body,
            catches,
            finally_block,
            span,
        } => Stmt::Try {
            body: resolve_block(body, ctx),
            catches: catches
                .into_iter()
                .map(|c| crate::ast::CatchClause {
                    ty: resolve_type(&c.ty, ctx),
                    name: c.name,
                    body: resolve_block(c.body, ctx),
                    span: c.span,
                })
                .collect(),
            finally_block: finally_block.map(|b| resolve_block(b, ctx)),
            span,
        },
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
        Expr::Propagate { inner, span } => Expr::Propagate {
            inner: Box::new(resolve_expr(*inner, ctx)),
            span,
        },
        Expr::CloneWith {
            object,
            fields,
            span,
        } => Expr::CloneWith {
            object: Box::new(resolve_expr(*object, ctx)),
            fields: fields
                .into_iter()
                .map(|(n, e)| (n, resolve_expr(e, ctx)))
                .collect(),
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
        // `html"…"` holes can carry cross-package calls, so resolve them like string holes (the
        // literal itself is desugared later, by the post-check `checker::resolve_html` pass).
        Expr::Html(parts, span) => Expr::Html(
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
        // A bare identifier that names a cross-package type (e.g. the head of an enum access
        // `Color.Red`) resolves to the mangled FQN; the shadow guard guarantees an imported type
        // name is never also a local/variable, so rewriting every occurrence is safe.
        Expr::Ident(n, sp) => match resolve_type_ref(&n, ctx) {
            Some(m) => Expr::Ident(m, sp),
            None => Expr::Ident(n, sp),
        },
        Expr::InstanceOf {
            value,
            type_name,
            span,
        } => Expr::InstanceOf {
            value: Box::new(resolve_expr(*value, ctx)),
            type_name: resolve_type_ref(&type_name, ctx).unwrap_or(type_name),
            span,
        },
        Expr::Lambda {
            params,
            ret,
            body,
            span,
        } => Expr::Lambda {
            params: params
                .into_iter()
                .map(|mut p| {
                    p.ty = resolve_type(&p.ty, ctx);
                    p
                })
                .collect(),
            ret: ret.as_ref().map(|r| resolve_type(r, ctx)),
            body: match body {
                LambdaBody::Expr(e) => LambdaBody::Expr(Box::new(resolve_expr(*e, ctx))),
                LambdaBody::Block(stmts) => LambdaBody::Block(resolve_block(stmts, ctx)),
            },
            span,
        },
        // Leaves carry no nested call site or type name: Int / Float / Bool / Null / Bytes / This.
        leaf => leaf,
    }
}

/// Resolve a call. A bare `Ident` head resolves against the caller's own package (mangled if that
/// package is non-`main`; a no-op for `main`, and for variants/classes/unknowns which aren't in the
/// function table). A `Member` head `q.name` is a qualified user call iff `q` is a non-`core` import
/// leaf whose target package defines `name` — rewritten to a bare call on the mangled name;
/// otherwise it is a native call or a method on a value and is left intact (receiver resolved).
/// Buffer a function-visibility violation against `ctx.violations` (no-op when visible). `pkg` is the
/// package the function lives in — the referrer's package for a bare call, the import target for a
/// qualified `q.fn()` call.
fn check_fn_visibility(ctx: &ResolveCtx, pkg: &str, name: &str) {
    if let Some(info) = ctx.prov_fns.get(&(pkg.to_string(), name.to_string())) {
        if let Some(code) = vis_violation(info, ctx.file, &ctx.package.join(".")) {
            ctx.violations.borrow_mut().push(format!(
                "{}: function `{name}` is not visible here — it is `{}` in package `{}`; widen its \
                 visibility to call it [{code}]",
                ctx.file.display(),
                vis_word(info.vis),
                if pkg.is_empty() { "main" } else { pkg },
            ));
        }
    }
}

fn resolve_call(callee: Expr, args: Vec<Expr>, span: Span, ctx: &ResolveCtx) -> Expr {
    let args: Vec<Expr> = args.into_iter().map(|a| resolve_expr(a, ctx)).collect();
    match callee {
        Expr::Ident(n, isp) => {
            // A type name wins (a constructor call `Point(x)` — a name is a type XOR a function in a
            // file, guarded by `E-TYPE-IMPORT-SHADOW`); else the same-package function table.
            let resolved = if let Some(t) = resolve_type_ref(&n, ctx) {
                t
            } else if let Some(f) = ctx
                .defined
                .get(&(ctx.package.join("."), n.clone()))
                .cloned()
            {
                // Same-package function: enforce file-scoped `private` (visibility modifiers).
                check_fn_visibility(ctx, &ctx.package.join("."), &n);
                f
            } else {
                n
            };
            Expr::Call {
                callee: Box::new(Expr::Ident(resolved, isp)),
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
                            // Cross-package qualified call: enforce `internal`/`public`.
                            check_fn_visibility(ctx, &target.join("."), &name);
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
        tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
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
    fn project_load_reports_stats() {
        let tmp = TempDir::new();
        tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
        let entry = tmp.write(
            "src/main.phg",
            "package main;\nfunction main() {}\nclass C {}",
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
        let u = load_loose_src("package main;\nfunction main() {}").unwrap();
        assert!(u.stats.is_none(), "loose mode reports no project stats");
    }

    #[test]
    fn project_main_is_folder_exempt_at_root() {
        let tmp = TempDir::new();
        tmp.write("phorge.toml", "module = \"acme/app\"");
        // main lives at the project root, outside src/ — allowed.
        let entry = tmp.write("main.phg", "package main;\nfunction main() {}");
        let u = load(&entry).unwrap();
        assert_eq!(u.program.package, ["main"]);
    }

    #[test]
    fn folder_path_mismatch_is_rejected() {
        let tmp = TempDir::new();
        tmp.write("phorge.toml", "module = \"acme/app\"");
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
        tmp.write("phorge.toml", "module = \"acme/app\"");
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
        tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
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

    #[test]
    fn duplicate_function_in_package_is_rejected() {
        let tmp = TempDir::new();
        tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
        let entry = tmp.write("src/main.phg", "package main;\nfunction main() {}");
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
        let entry = tmp.write("src/main.phg", "package main;\nfunction main() {}");
        // A vendored library must not declare `package main` (it would collide with the entry).
        tmp.write(
            "vendor/acme/lib/oops.phg",
            "package main;\nfunction stray() {}",
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
            "package main;\nimport type acme.geo.Hidden;\nfunction main() { Hidden h = Hidden(); }",
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
            "package main;\nimport type acme.geo.Shown;\nfunction main() { Shown s = Shown(); }",
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
            "package main;\nfunction main() { Helper h = Helper(); }",
        );
        // A second `package main` file (folder-exempt at root) declaring a file-private type.
        tmp.write(
            "src/helper.phg",
            "package main;\nprivate class Helper { constructor() {} }",
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
            "package main;\nfunction main() { Helper h = Helper(); }",
        );
        tmp.write(
            "src/helper.phg",
            "package main;\ninternal class Helper { constructor() {} }",
        );
        assert!(load(&entry).is_ok());
    }

    #[test]
    fn private_function_called_from_sibling_file_is_rejected() {
        let tmp = TempDir::new();
        tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
        let entry = tmp.write(
            "src/main.phg",
            "package main;\nfunction main() -> int { return helper(); }",
        );
        tmp.write(
            "src/helper.phg",
            "package main;\nprivate function helper() -> int { return 1; }",
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
            "package main;\nimport acme.util;\nfunction main() -> int { return util.secret(); }",
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
            "package main;\nimport acme.util;\nfunction main() -> int { return util.shown(); }",
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
            "package main;\ntype H = Helper;\nfunction main() { H h = Helper(); }",
        );
        tmp.write(
            "src/helper.phg",
            "package main;\nprivate class Helper { constructor() {} }",
        );
        let err = load(&entry).unwrap_err();
        assert!(err.contains("E-VIS-PRIVATE"), "got: {err}");
    }
}
