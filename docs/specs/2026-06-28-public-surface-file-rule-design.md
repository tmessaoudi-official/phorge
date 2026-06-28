# Public-surface file-naming rule — design

Status: **design-locked + approved** (2026-06-28, brainstormed with the developer; approved "build it",
hard errors). Plan SSOT `docs/plans/2026-06-27-ga-sequence.plan.md` (REPRIORITIZED section).

## Goal

Make a file's name tell you its public surface, cleanly, **without** importing PSR-4's micro-file tax or
contradicting Phorj's Go-shaped, function-heavy, `folder=path` package model. "Go packages, PSR-4-ish
public-type files."

## The rule

A non-`main` file is exactly one of two kinds, decided by what it **exports** (`public`):

- **Type module** — declares exactly **one** public named type (`class`/`enum`/`interface`/`trait`). The
  file stem must equal that type name **byte-exactly, casing included** (`class Circle` ⇒ `Circle.phg`).
- **Function module** — declares **zero** public types and any number of public free functions. Named
  with a lowercase/topic stem (unconstrained beyond "not a type name it doesn't contain").

Both kinds may additionally contain any number of `private`/`internal` helper **types and functions** —
these are single-file-scoped, invisible across files, so they ride along free (the ergonomic allowance,
AskUserQuestion option 1). A file may **not** mix a public type with a public free function, and may not
declare two public types (the clean-separation half, option 3).

**A file that declares the entry point `main` is fully exempt** — entry/program files mix types,
functions, and `main` freely under any name. (Detected via `ast::entry_point`; this covers every
single-file guide example, all loose `phg run x.phg`, and `-e`/stdin, which are `main`-only by M5.)

### Diagnostics

| code | when |
|------|------|
| `E-FILE-NAME` | a type module's file stem ≠ its public type's name (incl. casing) |
| `E-FILE-MULTI-PUBLIC` | a non-`main` file declares two or more public types |
| `E-FILE-MIXED-PUBLIC` | a non-`main` file declares a public type **and** a public free function |

All three self-document via `phg explain`.

## Why this is non-contradictory

- `folder=path` (`E-PKG-PATH`) governs *packages* (directory = dotted package). This new rule governs
  *the public surface within a package*. Orthogonal axes.
- The two things that made PSR-4 impossible for Phorj — **free functions** and **helper types** — are
  explicitly carved out: public functions get their own (topic-named) module; private/internal helpers
  ride along anywhere.
- `package Main` / entry files were never library modules, so exempting them costs nothing and keeps the
  "every example runs on the 3-backend oracle" contract intact.

## Enforcement site

In the **loader** (`src/loader.rs`), project mode only, in the same per-file pass that already enforces
`folder=path`/`E-PKG-PATH`. Each parsed file knows its path; we inspect its top-level `Item`s:
- find the set of `public` types and `public` free functions, and whether it declares `main`;
- if `main` present → skip (exempt);
- else classify and emit the relevant `E-FILE-*` on violation, attributed to the file.

The check is **front-end / loader-only** — it never touches a backend, so the byte-identity spine is
untouched (a renamed file produces identical output). `check`/`run`/`runvm`/`transpile` all route through
the loader in project mode, so the rule is enforced uniformly there.

`Item` visibility is already `Visibility::{Public,Internal,Private}` (default `Public`); `main` detection
is `ast::entry_point`. No new AST, no new `Op`/`Value`.

## Scope / blast radius

- **Zero guide-example churn** — every `examples/guide/*.phg` declares `main` ⇒ exempt.
- Shapes real multi-package projects (`examples/project/…`). Existing project examples
  (`tempconv`, `shapes`, `withdeps`, `ddd`) must be audited: any non-`main` file declaring a public type
  under a non-conforming name is renamed (or its types adjusted). A new showcase project demonstrates a
  clean type-module + function-module split.
- Loose single-file + `-e`/stdin: `main`-only ⇒ exempt.

## Examples shipped with the feature

A small multi-package project under `examples/project/` (e.g. a `Geometry` library package with
`Circle.phg`/`Square.phg` type modules + an `area-ops.phg` function module, consumed by a `Main` entry),
gated by the project-aware `differential.rs` harness. Plus a README note. The `E-FILE-*` codes are
demonstrated in `KNOWN_ISSUES`/`phg explain` (faults can't be runnable examples).

## Verification

`cargo test --workspace` (+ PHP 8.5 oracle), `cargo clippy -D warnings`, `cargo fmt --check`. New loader
unit tests for each `E-FILE-*` (positive + negative). The showcase project is byte-identity-gated.

## Deferred / out of scope

A per-project opt-out knob; applying the rule inside `package Main` (entry files stay exempt by design);
auto-rename tooling (`phg fmt --rename-files`) — a possible follow-up.
