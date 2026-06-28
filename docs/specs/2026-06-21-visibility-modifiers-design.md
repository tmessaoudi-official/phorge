# Visibility Modifiers (`public` / `internal` / `private`) — Design

> **Status:** Designed — approved 2026-06-21, not yet implemented.
> **Successor to:** the closed mutation milestone (M-mut.7b, `1593e2c`).
> **Plan:** `docs/plans/2026-06-21-visibility-modifiers.plan.md` (Decisions Log).

## 1. Motivation

Today every top-level declaration in Phorj is implicitly visible everywhere it can be reached —
within its file, across its package's files, and (when `public`-by-mangling) cross-package. There is
no way to mark a declaration as an implementation detail. This feature adds **declaration-level
visibility**: a way to say "this helper is private to this file" or "this type is internal to this
package, not part of its public surface."

This is the *declaration* analog of member visibility, and it follows Phorj's established
strictest-sensible-default tradition (the "nothing in the wind" namespace decision; explicit imports
even for the stdlib): a declaration marked private leaks **nothing**, not even to sibling files,
unless explicitly widened.

## 2. The model — a three-level lattice

| Keyword | Scope | Visible to | PHP-absent? |
|---|---|---|---|
| `public` (default — no keyword) | cross-package | anyone who can import the package | yes (normal decl) |
| `internal` | package | every file of the same package, **not** other packages | yes (normal decl) |
| `private` | file | the declaring `.phg` file only | yes (normal decl) |

Lattice: **`file ⊂ package ⊂ public`**. A reference from site **R** to declaration **D** is legal iff:

- **same file** → always legal (any visibility);
- **same package, different file** → requires `D` ≥ `internal`;
- **different package** → requires `D` = `public`.

Applies to **all top-level declarations**: `class`, `enum`, `interface`, and free `function`.
(Member-level visibility — fields/methods — is a separate, pre-existing axis; see §7.)

### Surface syntax

A prefix keyword immediately before the declaration keyword:

```phorj
class Public { }            // default: public (cross-package)
public class AlsoPublic { } // explicit public — allowed, identical to the above
internal class Shared { }   // package-scoped
private class FileLocal { } // file-scoped

public function exported() -> int { ... }
internal function pkgHelper() -> int { ... }
private function fileHelper() -> int { ... }
```

Explicit `public` is **allowed** (intent clarity), not required. A duplicate or conflicting prefix
(`public private class`, `internal public function`) is a parse error.

## 3. Architecture — loader-enforced

### Why the loader

The loader (`src/loader.rs`) is the **only** stage that retains file + package provenance, and it is
already the single chokepoint for every cross-file/cross-package reference:

- type references → `resolve_type_ref` / `resolve_type` (annotations, instantiation, `instanceof`,
  enum construction, `match` type patterns — every type-name position);
- function calls → the `defined`-table rewrite in `resolve_item` / `resolve_block`;
- explicit imports → `build_type_imports` (`import type …`) and the module-import map.

Pass 1 already holds `parsed: Vec<(PathBuf, Program)>` and indexes every definition by
`(package, name)` into the `types` and `defined` tables. After the Pass-2 flat-merge, the single
`Program` the checker/backends see has **no** file boundaries — by design. Therefore visibility must
be enforced *in the loader*, the same place cross-package import visibility already lives.

Two alternatives were considered and rejected:

- **(B) Checker-enforced** via a provenance side-table threaded into the merged `Program`. Rejected:
  fights the flat-merge by design; would require carrying per-item file tags through the merge.
- **(C) Hybrid** (AST flag + checker for same-file + loader for cross). Rejected: splits one rule
  across two stages — drift risk between the same-file and cross-file halves.

### The provenance map

In Pass 1, alongside the existing `types`/`defined` rename tables, record for each definition:

```
struct DefInfo { decl_file: PathBuf, package: String, vis: Visibility }
```

keyed by `(package, name)`, for both the type and function symbol tables.

In Pass 2, when resolving a reference in file `R` (package `Rpkg`) to a definition `D`
(`DefInfo { decl_file, package, vis }`), enforce the lattice **before** rewriting the name:

```
if R == decl_file                      -> OK   (same file)
else if Rpkg == package && vis >= Internal -> OK   (same package, internal/public)
else if vis == Public                  -> OK   (cross-package, public)
else                                   -> E-VIS-PRIVATE | E-VIS-INTERNAL
```

A name that simply isn't found stays on the existing not-found error path (unchanged). Visibility
only gates references that *would otherwise resolve*.

### Backends + transpiler — byte-identity safe by construction

Visibility is **never consumed downstream**: the loader fully validates it before the merged
`Program` reaches the checker, interpreter, VM, or transpiler. PHP has no file- or package-private
classes, so the transpiler emits a normal `class`/`function`. The byte-identity spine
(`run ≡ runvm ≡ real PHP`) is safe by construction — this is a **front-end-only** feature, exactly
like cross-package type mangling and generic erasure. No erase pass is needed; the `Visibility` field
simply rides on the AST, unread by any backend.

## 4. Single-file / loose mode

`-e`, stdin, and any single `.phg` with no `phorj.toml` run through `load_loose_src`, which performs
no Pass-2 resolution. There, everything is one file, so `private`/`internal` are **no-ops** (there is
no "outside"). The keywords still parse and are accepted (forward-compatible). Consequently:

- all single-file `examples/guide/*.phg` stay byte-identical (the field is erased/unread);
- only `examples/project/*` (which have a `phorj.toml`) receive enforcement.

## 5. Diagnostics

| Code | Trigger | Message (caret on the reference site) |
|---|---|---|
| `E-VIS-PRIVATE` | reference to a file-private decl from another file | *"`Foo` is private to `<decl_file>`; mark it `internal` (package-wide) or `public` (everywhere) to widen it."* |
| `E-VIS-INTERNAL` | reference to an `internal` decl from another package | *"`Foo` is internal to package `Acme.Geometry`; mark it `public` to export it."* |

An `import type Acme.Geometry.Foo` (or module import) of a non-visible declaration reports the same
codes. Both codes get `phg explain` entries (`cli.rs`).

## 6. Parser / AST

- New `Visibility { Public, Internal, Private }` enum (default `Public`).
- A field `vis: Visibility` on `ClassDecl`, `EnumDecl`, `InterfaceDecl`, and `FunctionDecl`.
- `internal` becomes a new reserved keyword; `public`/`private` are already member-modifier keywords.
- Parser: accept an optional single leading visibility keyword before
  `class`/`enum`/`interface`/`function`; absent ⇒ `Visibility::Public`. Reject duplicates/conflicts.
- The loader's `resolve_item` preserves the `vis` field unchanged (it is read from the provenance map
  built in Pass 1, not from the rewritten item).

## 7. Relationship to existing member modifiers

This feature is **orthogonal** to the existing `Modifier::{Public,Private,Protected}` (ast.rs), which
are *class-member* visibility (fields/methods) and are currently **not** Phorj-enforced — only PHP
enforces them after transpile (per `KNOWN_ISSUES.md`). Declaration visibility is a **new axis**,
carried as the dedicated `Visibility` field on each top-level decl and enforced in the loader. The two
are deliberately not conflated: a `private class` with a `public` method is coherent (the class is
file-scoped; were it visible, the method would be too).

## 8. Testing

TDD throughout; the byte-identity spine is the correctness gate.

- **Positive example** (byte-identity-gated): a new multi-file project `examples/project/visibility/`
  — an `internal` helper shared across two files of one package, consumed via a `public` type across
  packages — runs identically on `run` / `runvm` / **real PHP**. Auto-gated by the project-aware
  `tests/differential.rs`.
- **Negative cases** (`E-VIS-PRIVATE`, `E-VIS-INTERNAL`): per the project rule, a fault cannot be a
  runnable example (every example must produce identical *Ok* output). These are captured in the
  example project's `README.md` and covered by **loader unit tests** (one per code, plus a same-file
  OK case and a same-package-internal OK case) and **parser tests** (each keyword parses; conflicts
  reject).
- **No-op confirmation**: a single-file program using `private`/`internal` still runs byte-identically
  (the keywords are accepted and erased).

## 9. Scope & non-goals

- **In scope:** `public`/`internal`/`private` on `class`/`enum`/`interface`/`function`; loader
  enforcement across files and packages; two diagnostics; one example project.
- **Out of scope (deferred):** a file-scoped opt-in *stronger* than `private` (already maximal);
  visibility on individual `import` re-exports; making member-level `Modifier` visibility
  Phorj-enforced (a separate pre-existing gap, tracked in KNOWN_ISSUES); a visibility keyword *on*
  type aliases (`private type X = …`). Note: type aliases are expanded by the checker, which runs
  *after* the loader, so an alias could in principle launder a reference to an `internal`/`private`
  type past the loader's check — this aliasing-as-visibility-bypass case is flagged as a follow-up to
  verify during implementation (likely closed by also resolving alias bodies in the loader's Pass 2,
  or by gating alias targets at expansion time).

## 10. Blast radius

`ast.rs` (+`Visibility` enum, +4 `vis` fields), `parser.rs` (leading-modifier parse + tests),
`loader.rs` (provenance map + lattice check — the bulk of the work), `cli.rs` (2 `explain` entries),
`transpile.rs` (ignores the field — expected zero change), one new example project + README. **No new
`Op`, no `Value` change, no backend change.**
