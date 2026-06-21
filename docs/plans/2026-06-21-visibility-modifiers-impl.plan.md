# Visibility Modifiers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add three-level declaration visibility (`public` / `internal` / `private`) to every top-level Phorge declaration, enforced in the loader, erased from all backends.

**Architecture:** A new `Visibility` enum rides on `ClassDecl`/`EnumDecl`/`InterfaceDecl`/`FunctionDecl` (default `Public`). The parser reads an optional leading visibility keyword. The loader records each definition's `(file, package, vis)` in a provenance map during Pass 1 and enforces the lattice (`file ⊂ package ⊂ public`) at its three name-resolution chokepoints during Pass 2 — `build_type_imports` (cross-package types), `resolve_type_ref` (same-package types), `resolve_call` (functions). No backend reads the field, so the `run ≡ runvm ≡ real PHP` byte-identity spine is safe by construction.

**Tech Stack:** Rust (edition 2021, std-only). Design: `docs/specs/2026-06-21-visibility-modifiers-design.md`. Decisions: `docs/plans/2026-06-21-visibility-modifiers.plan.md`.

## Global Constraints

- Toolchain: `export PATH=/stack/tools/cargo/bin:$PATH`. Gate (must pass before every commit): `PHORGE_REQUIRE_PHP=1 cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`. The pre-commit hook re-runs fmt+clippy(-D warnings)+test — run them yourself first.
- Byte-identity spine: `run ≡ runvm ≡ real PHP` via `tests/differential.rs` + M7 PHP oracle. Visibility must be **front-end-only** (loader-enforced, never consumed by a backend) so PHP output is unchanged.
- No new `Op`, no `Value` change, no backend (`interpreter.rs`/`vm.rs`/`compiler.rs`/`chunk.rs`) change. If a backend edit seems necessary, STOP — the design is being violated.
- `internal` becomes a reserved keyword (sibling of `public`/`private`). Grep the repo for `internal` used as an identifier and migrate before relying on it.
- Examples ship with features (developer rule): the feature lands with a runnable byte-identity-gated example + a README entry in the same change.
- Git autonomy authorized: commit each green task with a `feat:`/`test:`/`docs:` message, no `Co-Authored-By`.

## File Structure

- `src/ast.rs` — add `Visibility` enum; add `vis: Visibility` field to `ClassDecl`, `EnumDecl`, `InterfaceDecl`, `FunctionDecl`.
- `src/lexer.rs` — add `TokenKind::Internal` + the `"internal"` keyword row.
- `src/parser.rs` — parse the optional leading visibility keyword in `parse_item`; set `.vis` on the parsed decl; reject duplicate/conflicting prefixes. Tests.
- `src/loader.rs` — Pass-1 provenance map; `DefInfo`; `vis_violation` helper; `ResolveCtx` gains `file`/`prov_types`/`prov_fns`/`violations`; enforcement at the three chokepoints; drain violations per file. Tests.
- `src/cli.rs` — `phg explain` entries for `E-VIS-PRIVATE`/`E-VIS-INTERNAL`; add both to the known-codes list.
- `examples/project/visibility/` — new byte-identity-gated example project + README (positive path runnable; the two faults documented).
- `CHANGELOG.md`, `KNOWN_ISSUES.md`, `docs/MILESTONES.md` — docs sweep.

---

### Task 1: `Visibility` enum + AST fields (pure refactor, no behavior change)

**Files:**
- Modify: `src/ast.rs` (near `enum Modifier`, line ~537; the four decl structs at ~642/664/712/734)
- Modify: every struct-literal construction site the compiler flags (`src/parser.rs`, `src/compiler.rs`, `src/loader.rs`, any test literals)

**Interfaces:**
- Produces: `pub enum Visibility { Private, Internal, Public }` (ordered Private < Internal < Public); `ClassDecl.vis`, `EnumDecl.vis`, `InterfaceDecl.vis`, `FunctionDecl.vis`, all `Visibility`.

- [ ] **Step 1: Add the enum.** In `src/ast.rs` after the `Modifier` enum:

```rust
/// Declaration-level visibility on a top-level item (M-visibility). A NEW axis, distinct from the
/// member-level `Modifier::{Public,Private,Protected}`. Ordered so `vis >= Visibility::Internal`
/// reads as "at least package-visible": `Private` (this file only) < `Internal` (this package) <
/// `Public` (cross-package; the default). Enforced entirely in the loader; never read by a backend
/// (PHP has no file/package-private declarations), so it is erased by being ignored downstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Visibility {
    Private,
    Internal,
    Public,
}
```

- [ ] **Step 2: Add the field to all four decl structs.** Add `pub vis: Visibility,` to `ClassDecl`, `EnumDecl`, `InterfaceDecl`, and `FunctionDecl` (a `FunctionDecl` used as a method or interface signature carries `Visibility::Public` and is ignored — only top-level items are checked).

- [ ] **Step 3: Build, let the compiler list every broken literal.**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo build 2>&1 | grep -E "missing field|error\[" | head -40`
Expected: a list of `missing field \`vis\`` errors at each construction site.

- [ ] **Step 4: Fix every flagged literal** by adding `vis: Visibility::Public,` (the default for every site — real parse-driven visibility is set in Task 2; interface-method and compiler-synthesized `FunctionDecl`s are always `Public`). Import `Visibility` where needed (`use crate::ast::Visibility;` / extend an existing `ast::{…}` use).

- [ ] **Step 5: Verify the suite is still green and byte-identical (the refactor introduced no behavior).**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && PHORGE_REQUIRE_PHP=1 cargo test 2>&1 | tail -15 && cargo clippy --all-targets -- -D warnings 2>&1 | tail -3 && cargo fmt --check`
Expected: all tests pass (same count as baseline 666), clippy clean, fmt clean.

- [ ] **Step 6: Commit.**

```bash
git add src/ && git commit -m "feat(ast): Visibility enum + vis field on top-level decls (inert)"
```

---

### Task 2: Parse the leading visibility keyword

**Files:**
- Modify: `src/lexer.rs` (the `keyword` table at line ~359; `TokenKind` enum)
- Modify: `src/parser.rs` (`parse_item` at line 1168)
- Test: `src/parser.rs` `#[cfg(test)]` module; `src/lexer.rs` tests

**Interfaces:**
- Consumes: `Visibility` (Task 1); `parse_class`/`parse_enum`/`parse_interface`/`parse_function` (return the four decls).
- Produces: `parse_item` sets `.vis` from an optional leading `public`/`internal`/`private`.

- [ ] **Step 1: Write the failing parser tests.** In the `src/parser.rs` test module:

```rust
#[test]
fn parses_private_class_visibility() {
    let prog = parse_program_str("package main;\nprivate class P {}");
    match &prog.items[0] {
        Item::Class(c) => assert_eq!(c.vis, Visibility::Private),
        other => panic!("expected class, got {other:?}"),
    }
}

#[test]
fn parses_internal_function_visibility() {
    let prog = parse_program_str("package main;\ninternal function f() {}");
    match &prog.items[0] {
        Item::Function(f) => assert_eq!(f.vis, Visibility::Internal),
        other => panic!("expected function, got {other:?}"),
    }
}

#[test]
fn bare_decl_defaults_to_public() {
    let prog = parse_program_str("package main;\nclass C {}");
    match &prog.items[0] {
        Item::Class(c) => assert_eq!(c.vis, Visibility::Public),
        other => panic!("expected class, got {other:?}"),
    }
}

#[test]
fn explicit_public_enum_parses() {
    let prog = parse_program_str("package main;\npublic enum E { A() }");
    match &prog.items[0] {
        Item::Enum(e) => assert_eq!(e.vis, Visibility::Public),
        other => panic!("expected enum, got {other:?}"),
    }
}

#[test]
fn conflicting_visibility_prefix_is_rejected() {
    let err = parse_program_err("package main;\npublic private class C {}");
    assert!(err.contains("a single visibility"), "got: {err}");
}
```

Use the existing test helpers in the module for parsing a program / capturing a parse error. If a `parse_program_str` / `parse_program_err` helper does not already exist, add a 3-line local helper that lexes + `Parser::new(tokens).parse_program()` and `.unwrap()` / `.unwrap_err().render(src)` respectively. Ensure `use crate::ast::Visibility;` is in scope for the test module.

- [ ] **Step 2: Run the tests to verify they fail.**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib parser:: 2>&1 | grep -E "FAILED|error\[" | head`
Expected: the new tests FAIL (`internal` not lexed; `.vis` always `Public`; conflict not rejected).

- [ ] **Step 3: Add the `internal` keyword.** In `src/lexer.rs`: add `Internal,` to the `TokenKind` enum (near `Public`/`Private`/`Protected`), and add `"internal" => Internal,` to the `keyword` match (line ~370). Grep first: `grep -rn "\binternal\b" src/ examples/ --include=*.phg --include=*.rs | grep -vi "internal," | head` — migrate any `.phg`/identifier use (expected: none).

- [ ] **Step 4: Parse the prefix in `parse_item`.** Rewrite `parse_item` (parser.rs:1168) so it first reads an optional visibility prefix, then dispatches and stamps `.vis`:

```rust
pub fn parse_item(&mut self) -> Result<Item, Diagnostic> {
    let sp = self.peek_span();
    // Optional leading declaration visibility: at most one of public/internal/private.
    let vis = self.parse_decl_visibility()?;
    let item = match self.peek() {
        TokenKind::Import => return self.reject_vis_on_import(vis, sp),
        TokenKind::Function => Item::Function(self.parse_function(Vec::new(), sp)?),
        TokenKind::Enum => Item::Enum(self.parse_enum(sp)?),
        TokenKind::Class => Item::Class(self.parse_class(sp)?),
        TokenKind::Interface => Item::Interface(self.parse_interface(sp)?),
        TokenKind::TypeKw => return self.parse_type_alias_with_vis(vis, sp),
        TokenKind::Package => {
            return Err(self.error("'package' must be the first declaration, before any import or definition"))
        }
        _ => return Err(self.error("a top-level item (import, function, enum, class, interface, or type)")),
    };
    Ok(stamp_visibility(item, vis))
}
```

Then add the helpers in the same `impl` block:

```rust
/// Read an optional single leading declaration-visibility keyword. Two visibility keywords in a
/// row (`public private`) is an error; absent ⇒ the default `Visibility::Public`.
fn parse_decl_visibility(&mut self) -> Result<Visibility, Diagnostic> {
    let first = match self.peek() {
        TokenKind::Public => Visibility::Public,
        TokenKind::Internal => Visibility::Internal,
        TokenKind::Private => Visibility::Private,
        _ => return Ok(Visibility::Public),
    };
    self.advance();
    if matches!(self.peek(), TokenKind::Public | TokenKind::Internal | TokenKind::Private) {
        return Err(self.error("a single visibility (public, internal, or private), not two"));
    }
    Ok(first)
}
```

Add a free `fn stamp_visibility(item: Item, vis: Visibility) -> Item` (top of the impl's module or as a private fn) that sets `.vis` on the `Class`/`Enum`/`Interface`/`Function` arms and returns other items unchanged. For `parse_type_alias_with_vis`: a `type` alias may not carry visibility this slice — if `vis != Visibility::Public`, return `Err(self.error("a type alias cannot carry a visibility modifier yet"))`; else delegate to `parse_type_alias(sp)`. For `reject_vis_on_import`: if `vis != Visibility::Public`, error `"an import cannot carry a visibility modifier"`; else `parse_import(sp)`.

Ensure `use crate::ast::Visibility;` (or extend the existing `ast::{…}` import) is in `parser.rs`.

- [ ] **Step 5: Run the tests to verify they pass.**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib parser:: 2>&1 | tail -5 && cargo test --lib lexer:: 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 6: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORGE_REQUIRE_PHP=1 cargo test 2>&1 | tail -8 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add src/ && git commit -m "feat(parser): leading visibility keyword on top-level decls"
```

---

### Task 3: Loader provenance map + cross-package TYPE enforcement (`E-VIS-INTERNAL`/`E-VIS-PRIVATE`)

**Files:**
- Modify: `src/loader.rs` (Pass-1 loop ~144-180; `build_type_imports` ~308; `DefInfo`/`vis_violation` new; `ResolveCtx` ~389; `load_project` Pass-2 ~194-211)
- Test: `src/loader.rs` test module (~970)

**Interfaces:**
- Produces: `struct DefInfo { file: PathBuf, package: String, vis: Visibility }`; `fn vis_violation(info: &DefInfo, referrer_file: &Path, referrer_pkg: &str) -> Option<&'static str>`; `ResolveCtx` fields `file: &'a Path`, `prov_types: &'a HashMap<(String,String), DefInfo>`, `prov_fns: &'a HashMap<(String,String), DefInfo>`, `violations: RefCell<Vec<String>>`.
- Consumes: `Visibility` (Task 1); the parsed `vis` (Task 2).

- [ ] **Step 1: Write the failing test** (cross-package `import type` of a non-public library type). In the loader test module:

```rust
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
```

- [ ] **Step 2: Run to verify failure.**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib loader::tests::import_type_of_internal 2>&1 | tail -8`
Expected: FAIL (the internal type is currently importable — no `E-VIS-INTERNAL`).

- [ ] **Step 3: Add `DefInfo` + `vis_violation`** near the top of `loader.rs` (after the `use` block):

```rust
use std::cell::RefCell;

/// Provenance for one top-level definition: where it was declared and how visible it is.
#[derive(Clone)]
struct DefInfo {
    file: PathBuf,
    package: String,
    vis: Visibility,
}

/// The lattice check. `None` ⇒ the reference is legal; `Some(code)` ⇒ the diagnostic code to report.
/// Same file → always legal. Same package, different file → legal unless `private`. Different
/// package → legal only if `public`.
fn vis_violation(info: &DefInfo, referrer_file: &Path, referrer_pkg: &str) -> Option<&'static str> {
    if info.file == referrer_file {
        return None;
    }
    if info.package == referrer_pkg {
        return if info.vis == Visibility::Private { Some("E-VIS-PRIVATE") } else { None };
    }
    match info.vis {
        Visibility::Public => None,
        Visibility::Internal => Some("E-VIS-INTERNAL"),
        Visibility::Private => Some("E-VIS-PRIVATE"),
    }
}
```

Add `use crate::ast::Visibility;` to the loader's imports.

- [ ] **Step 4: Build the provenance maps in Pass 1.** In `load_project`, declare alongside `defined`/`types`:

```rust
let mut prov_fns: HashMap<(String, String), DefInfo> = HashMap::new();
let mut prov_types: HashMap<(String, String), DefInfo> = HashMap::new();
```

In the Pass-1 item loop (after the existing dup-check `insert`), record provenance. Replace the `let (name, is_type) = match item { … }` arm so it also captures `vis`:

```rust
let (name, is_type, vis) = match item {
    Item::Function(f) => (&f.name, false, f.vis),
    Item::Class(c) => (&c.name, true, c.vis),
    Item::Enum(e) => (&e.name, true, e.vis),
    Item::Interface(i) => (&i.name, true, i.vis),
    _ => continue,
};
```

After the `(defined|types).insert(...)` dup-guard, add:

```rust
let prov = if is_type { &mut prov_types } else { &mut prov_fns };
prov.insert(
    (pkg.clone(), name.clone()),
    DefInfo { file: file.clone(), package: pkg.clone(), vis },
);
```

- [ ] **Step 5: Enforce cross-package type visibility in `build_type_imports`.** It already takes `file` and `types`; add a `prov_types: &HashMap<(String,String), DefInfo>` parameter. After the successful `types.get(&(pkg, leaf))` lookup (where `mangled` is bound), check:

```rust
if let Some(info) = prov_types.get(&(pkg.clone(), leaf.clone())) {
    if let Some(code) = vis_violation(info, file, &prog.package.join(".")) {
        return Err(format!(
            "{}: type `{leaf}` is not visible from package `{}` — it is declared `{}` in package \
             `{pkg}`; mark it `public` to export it [{code}]",
            file.display(),
            prog.package.join("."),
            match info.vis { Visibility::Internal => "internal", Visibility::Private => "private", Visibility::Public => "public" },
        ));
    }
}
```

Update the one `build_type_imports(&prog, &types, &user_imports, &file)?` call site to pass `&prov_types`.

- [ ] **Step 6: Run the tests to verify they pass.**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib loader::tests::import_type_of 2>&1 | tail -8`
Expected: both PASS.

- [ ] **Step 7: Wire the remaining `ResolveCtx` fields now (so Tasks 4-5 are additive).** Add `file: &'a Path`, `prov_types: &'a HashMap<(String,String), DefInfo>`, `prov_fns: &'a HashMap<(String,String), DefInfo>`, `violations: RefCell<Vec<String>>` to `ResolveCtx`. In the Pass-2 loop, build them per file and **drain** after resolving the file's items:

```rust
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
if let Some(first) = ctx.violations.into_inner().into_iter().next() {
    return Err(first);
}
```

(The `file` borrow: `parsed` holds `(PathBuf, Program)`; `for (file, prog) in parsed` already owns `file` — pass `&file`.) No reads of `prov_fns`/`violations` yet ⇒ silence dead-field warnings with `#[allow(dead_code)]` on those two `ResolveCtx` fields *only if clippy complains*; they are consumed in Tasks 4-5, so prefer to land Task 4 immediately after to avoid the allow.

- [ ] **Step 8: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORGE_REQUIRE_PHP=1 cargo test 2>&1 | tail -8 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add src/ && git commit -m "feat(loader): visibility provenance + cross-package type enforcement"
```

---

### Task 4: Same-package cross-file TYPE enforcement (`private`)

**Files:**
- Modify: `src/loader.rs` (`resolve_type_ref` ~403)
- Test: `src/loader.rs` test module

**Interfaces:**
- Consumes: `ResolveCtx.{file,prov_types,violations}` (Task 3); `vis_violation`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn private_type_referenced_from_sibling_file_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package main;\nfunction main() { Helper h = Helper(); }",
    );
    tmp.write("src/helper.phg", "package main;\nprivate class Helper { constructor() {} }");
    // NOTE: a non-main file directly in src/ is rejected; both files here are `package main`,
    // and main is folder-exempt — so put the sibling at the root with the entry instead:
    let err = load(&entry).unwrap_err();
    assert!(err.contains("E-VIS-PRIVATE"), "got: {err}");
}

#[test]
fn internal_type_referenced_from_sibling_file_is_allowed() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write("src/main.phg", "package main;\nfunction main() { Helper h = Helper(); }");
    tmp.write("main2.phg", "package main;\ninternal class Helper { constructor() {} }");
    assert!(load(&tmp.path().join("src/main.phg")).is_ok());
}
```

NOTE on layout: `package main` files are folder-exempt and may sit at the root; two `package main` files (entry under `src/`, sibling at root) is the cleanest multi-file-same-package setup — mirror `project_main_is_folder_exempt_at_root`. Adjust the `private` test's sibling to a root-level `main2.phg` (`package main;`) the same way as the `internal` test so the only variable under test is `private` vs `internal`.

- [ ] **Step 2: Run to verify failure.**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib loader::tests::private_type 2>&1 | tail -8`
Expected: FAIL (no `E-VIS-PRIVATE` yet).

- [ ] **Step 3: Enforce in the same-package branch of `resolve_type_ref`** (leave the `type_imports` branch alone — that path was already checked at import):

```rust
fn resolve_type_ref(name: &str, ctx: &ResolveCtx) -> Option<String> {
    if let Some(m) = ctx.type_imports.get(name) {
        return Some(m.clone()); // cross-package, already visibility-checked at import
    }
    let key = (ctx.package.join("."), name.to_string());
    if let Some(m) = ctx.types.get(&key) {
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
```

(Same-package branch ⇒ `info.package == referrer_pkg` always ⇒ `vis_violation` only returns `Some("E-VIS-PRIVATE")` here; `internal` and `public` pass.)

- [ ] **Step 4: Run to verify pass.**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib loader::tests 2>&1 | tail -8`
Expected: the new tests PASS; all prior loader tests still PASS.

- [ ] **Step 5: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORGE_REQUIRE_PHP=1 cargo test 2>&1 | tail -8 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add src/ && git commit -m "feat(loader): file-scoped private type enforcement"
```

---

### Task 5: Function visibility enforcement (bare same-package + qualified cross-package)

**Files:**
- Modify: `src/loader.rs` (`resolve_call` ~788)
- Test: `src/loader.rs` test module

**Interfaces:**
- Consumes: `ResolveCtx.{file,prov_fns,violations}`; `vis_violation`.

- [ ] **Step 1: Write the failing tests** — (a) a same-package cross-file `private` function call → `E-VIS-PRIVATE`; (b) a cross-package qualified call to an `internal` function → `E-VIS-INTERNAL`; (c) a cross-package call to a `public` function → OK.

```rust
#[test]
fn private_function_called_from_sibling_file_is_rejected() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write("src/main.phg", "package main;\nfunction main() -> int { return helper(); }");
    tmp.write("helper.phg", "package main;\nprivate function helper() -> int { return 1; }");
    let err = load(&tmp.path().join("src/main.phg")).unwrap_err();
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
    tmp.write("src/acme/util/util.phg", "package acme.util;\ninternal function secret() -> int { return 7; }");
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
    tmp.write("src/acme/util/util.phg", "package acme.util;\npublic function shown() -> int { return 7; }");
    assert!(load(&entry).is_ok());
}
```

- [ ] **Step 2: Run to verify failure.**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib loader::tests::private_function 2>&1 | tail -6 && cargo test --lib loader::tests::internal_function 2>&1 | tail -6`
Expected: FAIL.

- [ ] **Step 3: Add a helper and call it at both `resolve_call` function sites.** Add to `loader.rs`:

```rust
/// Record a function-visibility violation against `ctx.violations` (no-op when visible). `pkg` is the
/// package the function lives in (same as the referrer for a bare call; the import target for a
/// qualified call).
fn check_fn_visibility(ctx: &ResolveCtx, pkg: &str, name: &str) {
    if let Some(info) = ctx.prov_fns.get(&(pkg.to_string(), name.to_string())) {
        if let Some(code) = vis_violation(info, ctx.file, &ctx.package.join(".")) {
            ctx.violations.borrow_mut().push(format!(
                "{}: function `{name}` is not visible here — it is declared `{}` in package `{pkg}`; \
                 widen its visibility to call it [{code}]",
                ctx.file.display(),
                match info.vis { Visibility::Internal => "internal", Visibility::Private => "private", Visibility::Public => "public" },
            ));
        }
    }
}
```

In `resolve_call`, the bare-`Ident` arm: when the name resolves via the `ctx.defined` branch (i.e. it is NOT a type — `resolve_type_ref` returned `None`), call `check_fn_visibility(ctx, &ctx.package.join("."), &n)` before building the call. Restructure the `or_else` so you know which branch matched:

```rust
Expr::Ident(n, isp) => {
    let resolved = if let Some(t) = resolve_type_ref(&n, ctx) {
        t
    } else if let Some(f) = ctx.defined.get(&(ctx.package.join("."), n.clone())).cloned() {
        check_fn_visibility(ctx, &ctx.package.join("."), &n);
        f
    } else {
        n.clone()
    };
    Expr::Call { callee: Box::new(Expr::Ident(resolved, isp)), args, span }
}
```

In the qualified `Member` arm, right after the successful `ctx.defined.get(&(target.join("."), name…))`:

```rust
if let Some(mangled) = ctx.defined.get(&(target.join("."), name.clone())) {
    check_fn_visibility(ctx, &target.join("."), &name);
    return Expr::Call { callee: Box::new(Expr::Ident(mangled.clone(), msp)), args, span };
}
```

- [ ] **Step 4: Run to verify pass.**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib loader::tests 2>&1 | tail -8`
Expected: all new + prior loader tests PASS.

- [ ] **Step 5: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORGE_REQUIRE_PHP=1 cargo test 2>&1 | tail -8 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add src/ && git commit -m "feat(loader): function visibility enforcement (internal/private)"
```

---

### Task 6: `phg explain` entries + alias-bypass guard

**Files:**
- Modify: `src/cli.rs` (the `explain` table + known-codes list — mirror the `E-HOOK-*` entries added for M-mut.7b)
- Modify: `src/loader.rs` (alias-bypass follow-up from spec §9)
- Test: an integration test for `phg explain`; a loader test for the alias case

**Interfaces:**
- Consumes: the two codes from Tasks 3-5.

- [ ] **Step 1: Add explain entries.** Find the explain match in `src/cli.rs` (`grep -n "E-HOOK-NO-GET" src/cli.rs`). Add arms:

```rust
"E-VIS-PRIVATE" => "A declaration marked `private` is visible only within its own .phg file. \
    Referencing it from another file fails. Mark it `internal` (package-wide) or `public` \
    (everywhere) to widen its visibility.",
"E-VIS-INTERNAL" => "A declaration marked `internal` is visible only within its own package. \
    Referencing it from another package fails. Mark it `public` to export it.",
```

Append `"E-VIS-PRIVATE"` and `"E-VIS-INTERNAL"` to the known-codes list (the same list `E-HOOK-*` was added to).

- [ ] **Step 2: Decide the alias-bypass case (spec §9).** Check whether a `type` alias can launder a reference to a non-visible type past the loader. Write the probe test first:

```rust
#[test]
fn type_alias_does_not_launder_private_type() {
    let tmp = TempDir::new();
    tmp.write("phorge.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write(
        "src/main.phg",
        "package main;\ntype H = Helper;\nfunction main() { H h = Helper(); }",
    );
    tmp.write("main2.phg", "package main;\nprivate class Helper { constructor() {} }");
    // Either the direct `Helper()` ctor call is already caught (E-VIS-PRIVATE), in which case the
    // alias adds nothing; assert the violation still fires.
    let err = load(&tmp.path().join("src/main.phg")).unwrap_err();
    assert!(err.contains("E-VIS-PRIVATE"), "got: {err}");
}
```

- [ ] **Step 3: Run the probe.** Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib loader::tests::type_alias_does_not 2>&1 | tail -8`. Expected: it likely **passes already** — the alias body `type H = Helper;` is not a *reference* the loader rewrites, but every *use* site (`Helper()`) is checked by Task 4. If it passes, the alias is not a bypass vector (aliases name types but the construction/annotation use is still gated); add a one-line code comment in `resolve_type_ref` noting this and move on. If it FAILS, extend `resolve_item`'s `Item::TypeAlias` handling to run the aliased type through `resolve_type` (which calls `resolve_type_ref`, triggering the check), then re-run.

- [ ] **Step 4: Add an explain integration test** (mirror the existing explain test; `grep -rn "explain" tests/ src/cli.rs | grep -i test`):

```rust
// in the appropriate integration/cli test surface
#[test]
fn explain_knows_visibility_codes() {
    // invoke the explain path for E-VIS-PRIVATE / E-VIS-INTERNAL and assert non-empty, code-prefixed output
}
```

If explain is only reachable via the binary, assert through the same harness the `E-HOOK-*` explain test uses.

- [ ] **Step 5: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORGE_REQUIRE_PHP=1 cargo test 2>&1 | tail -8 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add src/ tests/ && git commit -m "feat(cli): explain E-VIS-* + alias-bypass guard"
```

---

### Task 7: Example project + README (byte-identity-gated)

**Files:**
- Create: `examples/project/visibility/phorge.toml`
- Create: `examples/project/visibility/src/main.phg`
- Create: `examples/project/visibility/src/acme/shapes/shapes.phg`
- Create: `examples/project/visibility/src/acme/shapes/helpers.phg`
- Create: `examples/project/visibility/README.md`
- Modify: `examples/README.md` (index + coverage matrix row)

**Interfaces:**
- Consumes: the whole feature. The differential project harness (`tests/differential.rs` `collect_projects`) auto-discovers this root.

- [ ] **Step 1: Write the example (positive path runs byte-identically).**

`phorge.toml`:
```toml
module = "acme/visibility"
source = "src"
```

`src/acme/shapes/shapes.phg` — a library package; one exported type, one package-internal helper:
```phorge
package acme.shapes;

// `public` (the default could be omitted; written for intent) — importable by other packages.
public class Rect {
  constructor(public int w, public int h) {}
  function area() -> int {
    // calls a package-INTERNAL helper declared in the sibling file helpers.phg — allowed (same package).
    return scale(this.w * this.h);
  }
}

// `internal` — visible across this package's files, but NOT to other packages.
internal function scale(int n) -> int {
  return n * factor();
}
```

`src/acme/shapes/helpers.phg` — same package, second file; declares a file-`private` helper:
```phorge
package acme.shapes;

// `internal` — used by shapes.phg (sibling file, same package).
internal function factor() -> int {
  return clamp(1);
}

// `private` — visible only inside THIS file. helpers.phg uses it; shapes.phg cannot.
private function clamp(int n) -> int {
  return n;
}
```

`src/main.phg` — different package; may use only the `public` type:
```phorge
package main;

import Core.Console;
import type acme.shapes.Rect; // public — OK across packages

function main() {
  Rect r = Rect(3, 4);
  Console.println("area: {r.area()}"); // area() internally uses scale()/factor()/clamp()
}
```

- [ ] **Step 2: Run it on both backends + real PHP.**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo run -q --bin phg -- run examples/project/visibility/src/main.phg && cargo run -q --bin phg -- runvm examples/project/visibility/src/main.phg`
Expected: both print `area: 12` identically. Then check the transpile round-trips: `cargo run -q --bin phg -- transpile examples/project/visibility/src/main.phg | php -n` ⇒ `area: 12`.

- [ ] **Step 3: Write the README** documenting all three levels and the two error cases (which cannot be runnable examples):

`examples/project/visibility/README.md` — explain `public`/`internal`/`private`, show the layout, and include the two **rejected** snippets with their error codes:
```
// In main.phg (another package) — each of these is a COMPILE ERROR:
import type acme.shapes.Scale;   // E-VIS-INTERNAL: scale is internal to acme.shapes
util.clamp(1);                    // E-VIS-PRIVATE:  clamp is private to helpers.phg
```
Mirror the prose density of `examples/project/shapes/`’s inline comments + `examples/README.md` entries.

- [ ] **Step 4: Add the `examples/README.md` row** (index + coverage matrix) — one line pointing at `project/visibility/` with the feature name "declaration visibility (public/internal/private)".

- [ ] **Step 5: Run the differential harness to confirm the new project is gated and green.**

Run: `export PATH=/stack/tools/cargo/bin:$PATH && PHORGE_REQUIRE_PHP=1 cargo test --test differential 2>&1 | tail -12`
Expected: `all_example_projects_match_between_backends` (and the PHP oracle) include `visibility/` and PASS.

- [ ] **Step 6: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORGE_REQUIRE_PHP=1 cargo test 2>&1 | tail -8 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add examples/ && git commit -m "docs(examples): declaration-visibility example project + README"
```

---

### Task 8: Docs sweep + close-out

**Files:**
- Modify: `CHANGELOG.md`, `KNOWN_ISSUES.md`, `docs/MILESTONES.md`, `docs/plans/2026-06-21-visibility-modifiers.plan.md` (STATUS)

- [ ] **Step 1: CHANGELOG** — add an "Added — declaration visibility (`public`/`internal`/`private`)" entry under `[Unreleased]` summarizing the three-level lattice, loader enforcement, and front-end-only erasure.

- [ ] **Step 2: KNOWN_ISSUES** — record the deferrals from spec §9: visibility keyword on type aliases (`private type X`); member-level `Modifier` visibility still PHP-only-enforced; visibility on `import` re-exports.

- [ ] **Step 3: docs/MILESTONES.md** — add a short "Visibility modifiers — COMPLETE (2026-06-21)" entry (mirror the M-mut close-out section).

- [ ] **Step 4: Plan STATUS** — append a `## STATUS` section to `docs/plans/2026-06-21-visibility-modifiers.plan.md` marking the feature complete with the final test count and commit shas.

- [ ] **Step 5: Commit.**

```bash
git add CHANGELOG.md KNOWN_ISSUES.md docs/ && git commit -m "docs: close out visibility modifiers feature"
```

---

## Self-Review

**1. Spec coverage:**
- §2 model (3 levels, lattice) → Tasks 1, 3-5 (`vis_violation`). ✓
- §2 syntax (prefix keyword, explicit public, conflict) → Task 2. ✓
- §3 enforcement (loader, 3 chokepoints) → Task 3 (`build_type_imports`), Task 4 (`resolve_type_ref`), Task 5 (`resolve_call`). ✓
- §3 backends erase (byte-identity) → every task ends on the full `PHORGE_REQUIRE_PHP=1` gate; Task 7 proves a runnable byte-identical example. ✓
- §4 single-file no-op → covered by the loose-mode path not being touched; Task 7's single-package examples already exercise it (guide examples stay green in every gate). ✓
- §4 orthogonal to member `Modifier` → Task 1 adds a dedicated `Visibility` enum, not a `Modifier` variant. ✓
- §5 diagnostics (`E-VIS-PRIVATE`/`E-VIS-INTERNAL` + hints + explain) → Tasks 3-5 (messages), Task 6 (explain). ✓
- §6 parser/AST → Tasks 1-2. ✓
- §8 testing (positive example + negative loader/parser tests + no-op) → Tasks 2-7. ✓
- §9 alias-bypass follow-up → Task 6 Step 2-3. ✓

**2. Placeholder scan:** No "TBD"/"handle edge cases"/"similar to". Every code step shows real code; the one investigative step (Task 6 Step 3) is a labelled probe with both branches specified. ✓

**3. Type consistency:** `Visibility { Private, Internal, Public }` and the four `.vis` fields are introduced in Task 1 and referenced identically thereafter. `DefInfo { file, package, vis }` and `vis_violation(&DefInfo, &Path, &str) -> Option<&'static str>` match across Tasks 3-5. `ResolveCtx` field names (`file`, `prov_types`, `prov_fns`, `violations`) are consistent. ✓
