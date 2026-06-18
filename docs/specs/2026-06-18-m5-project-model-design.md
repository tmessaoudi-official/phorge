# Phorge — M5 Project Model (modules & packages) — Design

> Brainstorm + design, 2026-06-18. Trigger: developer chose to build the full `src/`-rooted,
> mandatory-packaged, enforced folder=path project model — **Go-shaped**, transpiling to idiomatic PHP.
> Supersedes the deferred open items O-B/O-C of `docs/specs/2026-06-18-m3-namespace-system-design.md`.
> Pulls forward roadmap **M5 = "modules + git-based packages"** (`docs/MILESTONES.md`). Grounded in
> three research streams (raw notes `/tmp/m5-research/`): PHP/PSR-4 multi-file emission, the
> script-vs-project + git-dep models of Go/Rust/TS/Deno/Gleam, and a file-by-file blast-radius map of
> Phorge's single-`Program` pipeline. **Draft — locked decisions + sliced plan below.**

## 1. The shape: Phorge is a Go-shaped language

Mandatory package declaration, strict folder=path, directory-inferred membership, a reserved
`package main` entry — this is Go's exact structural profile. Research confirms folder=path enforcement
correlates with directory-inferred module membership (Go, Gleam, Java/PSR-4), so **Go is the closest
precedent** and M5 borrows its model wholesale, layering **Cargo's git-dep + SHA-lockfile** on top.
The discipline is the same family as the project's `strictNullChecks` stance: **stricter than the PHP
runtime, idiomatic PHP on emission.**

## 2. Locked decisions (2026-06-18, developer-confirmed)

- **M5-1 — every file declares a package, NEVER inferred** — including `-e`/stdin one-liners (they must
  write `package main;`). Purest "nothing in the wind".
- **M5-2 — syntax `package app.util;`** at file top (dotted, leading keyword, `;`-terminated; mirrors
  `import a.b.c;`). Emits PHP `namespace App\Util` (each segment PascalCased).
- **M5-3 — reserved `package main;`** is the executable entry (Go model; pairs with `fn main()`).
- **M5-4 — `core.` reserved** as a package root: a user `package core…;` or `import core.Foo;` of a
  non-existent native is a hard error (reserved like a built-in type name; cf. `is_builtin_type_name`).
- **M5-5 — project detection = manifest-presence (walk up).** A `phorge.toml` found by walking up from
  a source file's directory marks the project root. **No manifest above ⇒ loose-script mode**, where
  folder=path is suspended and **only `package main;` is legal** (a non-`main` package as a loose
  script is a hard error: *"package `app.util` requires a phorge.toml project; only `package main` runs
  loose"*). `package main` is *also* valid inside a project as the entry. Manifest presence — not the
  `src/` dir, not the package name — is the sole trigger. (Go's `GO111MODULE=auto` model.)
- **M5-6 — strict folder=path in project mode.** With source root `src/` (manifest `source =`
  overridable), `src/app/util/parse.phg` ⇒ `package app.util;`. On-disk path under the source root MUST
  equal the dotted package (Java/PSR-4 rule). `package main` is folder-exempt (runnable anywhere).
- **M5-7 — PHP emission = single-file brace-namespaces.** One self-contained `php out.php`-runnable
  file: `namespace App\Util { … }` blocks + a nameless `namespace { \App\Main\main(); }` bootstrap.
  **Zero Composer, zero autoloader, deterministic.** (See §4 — the free-function nuance forces this.)
- **M5-8 — call emission = fully-qualified, leading backslash.** `\App\Util\parse($x)` for user calls;
  `\strlen(...)`, `\sqrt(...)`, `\file_get_contents(...)` for erased `core.*` (the leading `\` is
  mandatory so an unqualified builtin call inside a namespace block still resolves to the global).
- **M5-9 — leaf-qualified call sites** (consistent with Wave 1 core.*): `import app.util;` binds leaf
  `util`; call `util.parse(x)`. Leaf collisions resolved by aliasing — `import app.util as autil;` (O-9).
- **M5-10 — git deps, pinned + vendored; Composer *vocabulary* in a TOML container.** Deps live under
  **`[require]` / `[require-dev]`** (Composer's words — the transpile-target audience reads them
  natively) as `"vendor/pkg" = { git = "…", tag|rev = "v1" }` (tag/rev only — never bare URL/branch),
  with an optional `"vendor/pkg" = "<git-url>@v1.2.0"` string shorthand. **Exact-pin only — no `^`/`~`
  ranges** (the lockfile pins exact, so a resolver/SAT-solve is unnecessary; deferred). `phorge.lock`
  pins resolved commit SHA + content hash. A committed `vendor/` (via `phorge vendor`) is **used
  automatically with zero network** when present — the only way examples stay byte-identical. (Go's
  `vendor/` + self-locating import path + Cargo's lock, fused.) **Rejected: literal `composer.json`** —
  a file the `composer` tool cannot actually process (no Packagist, no autoloader Phorge uses) is a
  false promise; familiarity is vocabulary, not the filename. (2026-06-18, developer-confirmed.)

## 3. The project manifest — minimal `phorge.toml`

```toml
name = "acme/myapp"   # vendor/package — Composer-style; doubles as the PSR-4 namespace root (Acme\Myapp)
version = "0.1.0"
source = "src"        # source root anchoring folder=path (default "src")

[require]
"acme/parser"  = { git = "https://github.com/acme/parser.phg", tag = "v1.2.0" }
"acme/json"    = "https://github.com/acme/json.phg@v0.3.1"   # string shorthand → desugars to { git, tag }

[require-dev]
"acme/testkit" = { git = "https://github.com/acme/testkit.phg", rev = "a1b2c3d" }
```

`name` doubles as the PSR-4-style vendor prefix in emitted PHP (`acme/myapp` ⇒ namespace `Acme\Myapp`).
**Composer's vocabulary** (`name = "vendor/package"`, `[require]`/`[require-dev]`) over a TOML container
that `phorge` actually runs — honest, not a `composer.json` the `composer` tool can't process. Each dep
self-locates via `git` + a pinned `tag`/`rev` (no Packagist, no `repositories` side-table); ranges are
intentionally absent. **S2a parses + represents this only** — resolution/vendoring is S3.

## 4. PHP emission (transpile contract D-L9)

**The free-function nuance (load-bearing):** PSR-4 autoloads *classes only* — PHP has no
function-autoloading hook. Phorge is function-heavy, so a PSR-4 directory tree would need eager
`require`s or a Composer `files` map. **The single-file brace-namespace form avoids the whole problem**
and runs with a bare `php out.php`:

```php
<?php
declare(strict_types=1);
namespace App\Util {
    function parse(string $s): int { /* … */ }
}
namespace App\Main {
    function main(): void { echo \App\Util\parse("42") . "\n"; }
}
namespace {                 // nameless global block — program bootstrap
    \App\Main\main();
}
```

Three hard codegen constraints (each fatal if violated): (1) **all** namespaces bracketed (no mixing
with statement-form `namespace X;`); (2) nothing outside the brackets except a single leading
`declare(...)`; (3) global/bootstrap code lives in the nameless `namespace { }` block. `core.*` stays
erased to PHP flat builtins with a leading `\`. A PSR-4 directory-tree emission mode (`composer.json` +
hand-rolled autoloader + eager function `require`s) is a possible *later* "emit a real editable PHP
project" feature — not needed to run the output.

## 5. Pipeline impact (verified blast radius)

All four backends iterate a flat `Program.items` and key names by bare string
(`checker.rs:238`, `interpreter.rs:116`, `compiler.rs:154`, `transpile.rs:57`); the interpreter/VM/
transpiler auto-invoke a global `main` (`interpreter.rs:103`, `vm.rs:63`, `transpile.rs:100`).
`cmd_run/check/transpile` take only `src: &str` (no path); the differential harness globs
`examples/**/*.phg` and runs one file at a time (`tests/differential.rs:535`). **Consequence:** a
single-file `package` decl changes *nothing* at runtime (run==runvm trivially green); multi-file +
qualified cross-package calls is the only part that touches name resolution in all four backends, and
it is isolated to its own slice. Lowest-risk order below.

## 6. Sliced plan (each slice: one+ green commit, byte-identical, examples ship with it)

- **S1 — `package` declaration, single-file (byte-safe foundation).** Lexer `package` keyword; parser
  → a `Program.package: Vec<String>` (or `Item::Package`); checker enforces mandatory + `core.`
  reservation + reserved `main`; loose-script mode allows only `package main` (folder=path suspended,
  no manifest yet); transpiler emits the single brace-namespace block + nameless bootstrap (or, for
  `package main`, the existing flat form — decide: simplest is to always brace, with `main` → a chosen
  namespace e.g. `Main`). **Migration:** add `package main;` to all ~25 examples + ~200 inline test
  programs (mechanical; reuse the Wave-1 migrator, but distinguish program literals from help/prose
  strings — Wave-1 gotcha). run==runvm unchanged; PHP round-tripped.
- **S2 — project model.** Sub-sliced:
  - **S2a — manifest + source root + project detection** (`phorge.toml`, walk-up discovery, `source`).
  - **S2b — multi-file loader + folder=path enforcement** (discover project `.phg` files, validate
    path↔package, assemble a compilation unit). Backends still see a flat merged set until S2c.
  - **S2c — qualified cross-package calls** (`import app.util;` → `util.parse(x)` resolution in all four
    backends; the only flat-namespace-breaking change) + **multi-namespace PHP emission** (one brace
    block per package). + aliasing (O-9).
  - **S2d — project-aware differential harness** + a multi-file `examples/project/` showcase.
- **S3 — git deps + vendor + lockfile.** `[dependencies]` git+tag, `phorge.lock` (SHA), `phorge vendor`,
  auto-offline when `vendor/` present. May land as the final M5 slice or split to a follow-up; design
  must not preclude it. Examples needing deps ship vendored sources for determinism.

## 7. Risks & guardrails

- **Byte-identity spine (INVARIANTS C1):** S1 is runtime-inert; S2c is the one risky slice — gate it
  with multi-file `agree`/`agree_err` cases before merging. Never let run≠runvm land.
- **Migration churn (S1):** mandatory `package main;` everywhere is a Wave-1-scale edit; mechanical but
  must not corrupt the deliberately-bare negative tests (Wave-1 migrator pitfall). Recurse subdirs.
- **Determinism for deps (S3):** examples MUST resolve offline from a committed `vendor/` — never the
  network (same reason URL/network is M6). Pin commit SHAs, never floating tags/branches.
- **`-e`/stdin ergonomics:** accepted cost — one-liners write `package main;` explicitly (M5-1).

## 9. S1 implementation notes (resolved in the 3C convergence gate, 2026-06-18)

- **AST (F1):** add `Program.package: Vec<String>` (one decl per file), not an `Item::Package` — the
  decl is a file-level attribute, parsed first, before any `import`.
- **Ordering (F5):** `package …;` MUST be the first item, before `import`s (Go/PHP/Java convention).
- **PHP emission in S1 (F2 — the key byte-safety move):** `package main` emits **flat** PHP (today's
  output, unchanged) — so `examples/transpile/demo.php` + `tests/fixtures/sample.phg` stay
  byte-identical. Brace-namespace emission (§4) is introduced only for **non-`main`** packages in S2c
  (multi-file). S1 is therefore fully byte-identical on run/runvm **and** PHP round-trip.
- **`package` keyword (F15):** verified used as an identifier nowhere — making it a keyword is safe.
- **Error codes (F11/F12):** `E-NO-PACKAGE` (missing decl), `E-RESERVED-PACKAGE` (user `package core…`
  or `import core.X` of a non-native), `E-PACKAGE-LOOSE` (non-`main` package run as a loose script).
  Each needs a `phorge explain <CODE>` entry (S0 diagnostics registry).
- **Migration scope (F4/F6/F14):** only programs that reach the **checker** (`parse_checked`/`cmd_run`/
  `cmd_runvm`/`cmd_transpile`/`cmd_disasm`/`cmd_bench`/`cmd_build`) need `package main;` — lexer/parser-
  only fragment tests do not. **Exclude negative/error-path tests** from any migrator (Wave-1 pitfall —
  must not mask what they assert). Prepending shifts line numbers: only **6** sites assert diagnostic
  lines (verified) — update them by hand. `-e`/stdin tests + S0 `--help` worked examples must show
  `package main;`. Scope: 24 `examples/**/*.phg` + `sample.phg` + the inline checker-reaching programs.
- **`fn main()` vs `package main` (F13):** they coexist cleanly (a package decl ≠ a function decl); no
  parser/checker ambiguity. The interpreter/VM still auto-invoke the `main` *function*.
- **Build/bench/disasm (F8/F9):** `examples/build/app.phg`, `examples/bench/workload.phg`,
  `examples/cli/demo.phg` migrate with the rest; their READMEs + the "minimal program" doc line gain
  `package main;`.

## 8. ROI

High and roadmap-aligned (M5). Namespacing the *user* surface now, while the language is small, is far
cheaper than retrofitting; the single-file brace-namespace target keeps the PHP output runnable with no
toolchain; Go's manifest-walk + `package main` escape hatch reconciles strict projects with runnable
scripts; Cargo's pinned-lock + Go's vendored offline model keeps the deterministic spine intact.
