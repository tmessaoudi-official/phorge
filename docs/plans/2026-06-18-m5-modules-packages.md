# M5 — Modules & Packages (project model) — Plan

> Pulled forward from the frozen roadmap (`docs/MILESTONES.md`: **M5 = modules + git-based
> packages**). Developer chose to build the full `src/`-rooted, mandatory-packaged, enforced
> folder=path project model now, with **proper research / test / brainstorm / reflection** (not a
> single reckless push). Design source (in progress): `docs/specs/2026-06-18-m5-project-model-design.md`.
> Supersedes the deferred open items O-B/O-C of `docs/specs/2026-06-18-m3-namespace-system-design.md`.

## Decisions Log
- [2026-06-18] AGREED: next work = **Track B Wave 3 → reframed as M5 project model** (Wave 2 stdlib
  breadth shipped: core.math/text/file, `eb6c35f`).
- [2026-06-18] AGREED (scope): build the **full** `src/`-rooted PSR-4-style project model — mandatory
  packaging, enforced folder=path, multi-file loader, vendor (git-based) deps, multi-file PHP
  emission, project-aware test harness. (Chose "build the whole project model" over a single
  byte-safe slice, but **with proper search/test/brainstorm/reflection** — design spec + sliced plan
  first, then implement all slices.)
- [2026-06-18] AGREED (syntax): `package app.util;` at file top — dotted, leading keyword,
  semicolon-terminated (matches existing `import a.b.c;`). Emits PHP `namespace App\Util;` (PascalCased
  segments). `core.*` reserved as a user package root (rejected like a built-in type name).
- [2026-06-18] AGREED (escape hatch): reserved **`package Main;`** is the executable entry (Go model) —
  pairs with the existing `fn main()` convention; **not inferred**. Non-`main` packages → folder=path
  enforced; `package Main` → runnable entry.
- [2026-06-18] AGREED (mandatory everywhere, NO exceptions): every file declares a package, **never
  inferred** — including `-e`/stdin one-liners (they must write `package Main;` explicitly). Purest
  "nothing in the wind". Accepts the one-liner ceremony cost.
- [2026-06-18] CONTEXT (verified): the byte-identity spine (`tests/differential.rs`
  `all_examples_match_between_backends`) globs `examples/**/*.phg` and runs ONE file at a time via
  `cmd_run(&src)`/`cmd_runvm(&src)` — multi-file projects need a project-aware harness. run/check/
  transpile take only `src: &str` (no path); only `cmd_build` gets `input_path` (`src/cli.rs`).
- [2026-06-18] AGREED (S2a manifest deps — Composer dialect, honest container): keep `phorj.toml`
  (TOML), but speak Composer's **vocabulary** so a PHP dev reads it natively — `name = "vendor/package"`
  (Composer-style; doubles as the PSR-4 namespace root, `acme/myapp` ⇒ `Acme\Myapp`), **`[require]` /
  `[require-dev]`** section names, values `{ git = "…", tag|rev = "…" }` (Go self-location — no Composer
  `repositories` side-table) plus an optional `"vendor/pkg" = "<git-url>@v1.2.0"` string shorthand.
  **Exact-pin only — no `^`/`~` ranges** (a resolver is deferred; the lockfile pins exact regardless;
  M5-10 says tag/rev only, never branch). **Rejected literal `composer.json`** — the developer's own
  kill-shot: a file the `composer` tool can't actually process (no Packagist, no autoloader Phorj uses)
  is a false promise. Familiarity comes from vocabulary, not the filename/tool.
- [2026-06-18] CONTEXT (verified): PSR-4 maps a namespace prefix → base dir; `\`=`/`; FQCN→file path
  (PHP-FIG PSR-4, Composer schema). Phorj's mandatory folder=path = **PSR-4 promoted from convention
  to language rule**; transpile = emit PHP files in PSR-4 layout + a generated autoload/composer block.
  Contract holds: Phorj package resolution : PHP/PSR-4 :: TS module resolution : JS.

- [2026-06-18] AGREED: after S2d (committed `e54c919`), next = **M5 S3** — git deps + `phorj.lock`
  + `phg vendor` + auto-offline, the final M5 slice (developer chose "M5 S3 — close the milestone").
  Determinism gate: deps pinned (tag/rev → SHA in lockfile) + vendored + resolved offline-only, never
  live network in tests/examples (same rule as the M6 URL deferral). Design source for S3 details:
  `docs/specs/2026-06-18-m5-project-model-design.md` M5-10 + O-7.
- [2026-06-18] AGREED (S3 design, 3C-converged 8/8): build the **full S3 in one slice** — git deps +
  `phorj.lock` + `phg vendor` + auto-offline. Determinism = **vendored + offline-only in tests**
  (developer choice). Locked design:
  - **Vendor = a flat package forest** under `vendor/` with **no nested `phorj.toml`** (required so the
    project-aware harness doesn't treat a dep as a standalone project). `phg vendor` clones each
    `[require]` dep, checks out its pinned tag/rev, reads the dep's own manifest source root, and copies
    that source subtree into `vendor/` preserving package dirs (folder=path validates against `vendor/`).
  - **`phorj.lock`** (TOML subset, like the manifest): per dep `name`, `git`, resolved `rev` (full
    commit SHA = the cryptographic pin), and `hash` = **FNV-1a-64** over the sorted `(rel-path \0 bytes)`
    of the vendored tree (reuses `bundle::cross::fnv1a_64`; a non-crypto integrity checksum, the SHA is
    the real pin).
  - **Auto-offline load**: `loader::load_project` merges vendored packages exactly like local library
    packages (mangle + resolve → zero backend change, run==runvm structural). Vendor is consulted **only
    when `[require]` is non-empty**; requires-present-but-not-vendored → `E-VENDOR-MISSING` (run
    `phg vendor`). **Never fetches on run** — offline only.
  - **`phg vendor` is idempotent without a blanket wipe** (Rule 8): removes only the dep subtrees it
    owns, then repopulates; **network only here**, never on run/check/transpile.
  - New guards: **`E-DUP-DEF`** (duplicate `(package,name)` in the merge — was a latent silent overwrite
    since S2c, made fatal now) and a **rejection of `package Main` inside `vendor/`** (a lib dep is
    functions-only; a vendored `main` would collide with the consumer entry).
  - **Tests** (`tests/vendor.rs`) exercise the fetch path against a **`file://` local-git fixture**
    (offline, deterministic — `git` 2.54 present). The shipped example commits its `vendor/` +
    `phorj.lock`; the differential harness loads it offline → byte-identical on run/runvm + real PHP
    (exact integer/string ops only — dodges the int-`/`-vs-PHP-float-`/` gotcha).
  - **Deferred (documented, not regressions):** transitive dep resolution (vendor resolves direct
    `[require]` only); `phg build` stays single-file (won't merge `vendor/`). Both → KNOWN_ISSUES.
- [2026-06-18] AGREED (S2d): next = **project-aware differential harness + public `examples/project/`
  showcase** (the multi-file example deferred from S2a–S2c, satisfying "examples ship with features").
  Harness lives in `tests/differential.rs`: discover every project root under `examples/` (a dir with
  a `phorj.toml`), load via `loader::load`, run both backends, assert `Ok` + byte-identical. The
  single-file glob is made **project-aware** — it stops descending into any directory that contains a
  `phorj.toml` (structural exclusion, not name-based), so project files are never run standalone and
  the `len() >= 3` floor still gates the flat examples. (Developer chose "S2d — harness + example".)
- [2026-06-18] AGREED (S2c scope): library packages export **functions only** this slice — a
  `class`/`enum` in a non-`main` package is rejected (`E-PKG-TYPE`); cross-package type namespacing is
  an M5 follow-up. The public `examples/project/` showcase is deferred to S2d (the single-file
  differential glob can't run a library file with no `main`); S2c's executable proof is the
  `tests/project.rs` integration suite. (Developer chose "Go — implement as planned".)
- [2026-06-18] AGREED (S2c architecture): **loader-side resolution + name-mangling**, NOT backend-aware
  resolution. The loader (path-aware, runs before checker/backends) rewrites each file's calls using
  that file's import map, mangling every **non-`main`** top-level def to a PHP-FQN-shaped global key
  (`compute` in `acme.util` ⇒ `Acme\Util\compute`); `package Main` defs stay unmangled (auto-invoke +
  single-file byte-identity). Native `core.*` calls are left untouched (classified by import-path root).
  Checker/interpreter/compiler/VM consume the rewritten flat AST **unchanged** ⇒ run==runvm structurally
  guaranteed. Only the **transpiler** changes: mangled names present ⇒ group into `namespace Acme\Util {}`
  brace-blocks + nameless `\Main\main()` bootstrap (M5-7); no mangled names ⇒ today's flat path
  (byte-identical to `demo.php`). 3C gate: full 30/8.
- [2026-06-18] AGREED (S2b approach): **directory = package** (Go's model — `src/acme/util/*.phg` all
  declare `package acme.util`; multiple files per package dir; `package Main` folder-exempt, runnable
  anywhere). **Flat AST merge** (parse each project file → merge `items` into one `Program`; backends
  see a bigger flat set, byte-safe). Enforcement (folder=path `E-PKG-PATH` + loose-mode `main`-only)
  lives in a new **path-aware `src/loader.rs`**, never in `check()` (so `cmd_run(src)`, the differential
  harness, and `checker.rs:1649` `package app.util` stay untouched). File-path sources route through the
  loader on run/runvm/check/transpile; `-e`/stdin/parse/lex/build keep the single-file string path.
  Multi-namespace transpile + qualified call resolution remain S2c; the multi-file example ships at S2d.
  A non-`main` file directly under the source root (empty relative dir) is an error — a dotted package
  needs a matching subdirectory.

## Open items — RESOLVED in the design spec (`docs/specs/2026-06-18-m5-project-model-design.md`)
- O-1 Source root → **convention `src/`, overridable via manifest `source =`** (M5-6).
- O-2 Manifest → **minimal `phorj.toml`** ([package] name/version/source + [dependencies]); its
  presence (walk up) is the sole project-detection signal (M5-5, §3).
- O-3 Multi-file loader → **entry-point loader assembles a compilation unit; backends unchanged until
  qualified calls (S2c)**. Single-file `package` decl (S1) is runtime-inert → byte-safe (§5).
- O-4 Cross-package calls → **leaf-qualified** `import app.util;` → `util.parse(x)`, emit
  `\App\Util\parse($x)` (M5-8/M5-9). Resolution in all four backends = S2c.
- O-5 PHP emission → **single-file brace-namespaces** + nameless bootstrap block; runs with bare
  `php out.php`, no Composer/autoloader (M5-7, §4). Resolves the PSR-4-can't-autoload-functions nuance.
- O-6 Harness → **project-aware differential** (S2d): single-file `package Main` examples keep the glob;
  multi-file projects discovered + run by entry.
- O-7 vendor/git → **pinned tag/rev + `phorj.lock` (SHA) + committed `vendor/` auto-used offline**
  (M5-10, S3). Examples resolve offline only — never network (determinism gate, like M6 URL deferral).
- O-8 Migration → **S1 slice**: `package Main;` into ~25 examples + ~200 inline programs (mechanical,
  Wave-1-migrator pattern; distinguish program literals from help/prose strings).
- O-9 Aliasing → `import a.b as c;` for leaf collisions — lands with user packages (S2c).

## Formal Plan
Slices (each: one+ green commit, run==runvm byte-identical, PHP round-tripped, examples ship with it):

- [x] **S1 — `package` declaration, single-file (byte-safe foundation)** — DONE (2026-06-18, 374
  tests green, clippy + fmt clean, run↔runvm + PHP round-trip byte-identical). `package` keyword +
  parse → `Program.package` (first item; later = parse error) + checker `E-NO-PACKAGE`/
  `E-RESERVED-PACKAGE` (+ `explain`). Transpiler **ignores** the package in S1 (flat PHP unchanged) —
  brace-namespace emission + loose-mode `main`-only enforcement deferred to S2 (folder=path needs the
  project model). Migrated 24 examples + `sample.phg` + all inline/integration test programs to
  `package Main;` (test helpers auto-prepend, line-preserving); fixed pre-existing Wave-1 `README.md`
  drift (`import std.io;` + bare `println`).
- [x] **S2a — manifest + source root + project detection** — DONE (2026-06-18). `src/manifest.rs`:
  std-only TOML-subset parser → `Manifest`/`Dependency`/`Pin`, Composer vocabulary (`name =
  "vendor/package"`, `[require]`/`[require-dev]`, git+tag/rev, `"url@tag"` shorthand), exact-pin only,
  strict rejection (branch/double/missing pin, unknown key/section, unquoted value); `Project::detect`
  walk-up discovery + source-root resolution (default `src`); `namespace_root()` PSR-4 mapping. 18 unit
  tests; byte-safe (unconsumed — no backend touched). Rejected literal `composer.json` (tool can't
  process it). Example showcase deferred to S2d (when behavior is observable).
- [x] **S2b — multi-file loader + strict folder=path enforcement** — DONE (2026-06-18). `src/loader.rs`:
  `load(entry)`/`load_loose_src(src)` → `Unit { program, diag_src }`. Project mode walks up to
  `phorj.toml`, parses every `.phg` under the source root, validates folder=path (`E-PKG-PATH`;
  directory=package Go-model, `main` folder-exempt), merges items flat. Loose mode enforces
  `package Main`-only. Enforcement path-aware (in the loader, never `check()`) → `cmd_run(&str)` +
  differential untouched. `cli::{run,runvm,check,transpile}_program` consume the loaded program;
  `main.rs` routes `<file>` for run/runvm/check/transpile through the loader, keeps `-e`/stdin/parse/
  lex/disasm/bench/build on the string path. Flat-merge interim: cross-file calls resolve unqualified;
  qualified calls + namespaced PHP + aliasing are S2c. 12 tests (9 loader unit + 3 integration, incl.
  byte-identical multi-file run). Example showcase deferred to S2d.
- [x] **S2c — qualified cross-package calls** + multi-namespace PHP emission + import aliasing — DONE
  (2026-06-18, 409 tests green, clippy + fmt clean, run==runvm + real-PHP round-trip byte-identical).
  Implemented as a **loader-side resolution + name-mangling pass** (not backend-aware resolution): the
  loader mangles every non-`main` def to a global PHP-FQN key (`Acme\Util\compute`), rewrites
  same-package bare + qualified user calls (`util.compute(x)`, per-file import map) to bare mangled
  calls, leaves `core.*` natives untouched, then flat-merges. Backends consume the rewritten AST
  unchanged ⇒ run==runvm structural. Transpiler de-mangles into `namespace Acme\Util {}` brace-blocks +
  `\Main\main()` bootstrap. Aliasing via `Item::Import.alias` (contextual `as`). **Scope: library
  packages export functions only** (`E-PKG-TYPE` rejects non-`main` types; cross-package types are an
  M5 follow-up). The S2b bare cross-package interim is tightened (unqualified now fails on both
  backends). The public `examples/project/` showcase ships at S2d (needs the project-aware harness).
- [x] **S2d — project-aware differential harness + `examples/project/` showcase** — DONE
  (2026-06-18, 410 tests green, clippy + fmt clean, run==runvm + real-PHP round-trip byte-identical).
  `examples/project/tempconv/` (two-package C→F converter) is the first public multi-file project:
  mandatory packages + folder=path, cross-package qualified call, import aliasing (`as`), same-package
  bare call across files, namespaced PHP. `tests/differential.rs` discovers every project root (dir
  with `phorj.toml`), loads via `loader::load`, asserts `run` ≡ `runvm`; the single-file glob is made
  project-aware (skips any dir holding a `phorj.toml` — structural, name-independent). Docs refreshed
  (`examples/README.md` rows + corrected "later slice" notes; `examples/project/README.md`; `FEATURES.md`
  Modules/packages → 🚧).
- [x] **S3 — git deps + `phorj.lock` + `phg vendor` + auto-offline** (final M5 slice). `src/lock.rs`
  (strict TOML-subset lockfile, round-tripping) + `src/vendor.rs` (`phg vendor`: clone → checkout
  pin → copy source into `vendor/<vendor>/<package>/` → FNV hash → write `phorj.lock`; idempotent,
  crash-safe, **network only here**). `loader::load_project` merges vendored packages like first-party
  libraries (mangle/resolve before any backend ⇒ run≡runvm structural; transpiler de-mangles to
  `namespace`); offline-only — `run`/`check`/`transpile` never fetch (`E-VENDOR-MISSING` if a
  `[require]` dep isn't vendored). Guards: `E-VENDOR-MAIN` (vendored `package Main`), `E-DUP-DEF`
  (duplicate `(package,name)` — was a silent overwrite). CLI: `cmd_vendor`, dispatch/USAGE/help,
  3 `explain` codes. Example `examples/project/withdeps/` (vendored `acme/strutil`) ships committed
  `vendor/` + `phorj.lock`, byte-identical on run/runvm + real PHP. `tests/vendor.rs` drives the git
  path via a `file://` fixture (offline). Deferred (KNOWN_ISSUES): transitive deps; `phg build`
  stays single-file. **421 tests green, clippy + fmt clean. M5 COMPLETE.**

> Phase 3C convergence gate runs before S1 implementation begins. Each slice re-enters Phase 5→6→6C.
