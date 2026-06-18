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
- [2026-06-18] AGREED (escape hatch): reserved **`package main;`** is the executable entry (Go model) —
  pairs with the existing `fn main()` convention; **not inferred**. Non-`main` packages → folder=path
  enforced; `package main` → runnable entry.
- [2026-06-18] AGREED (mandatory everywhere, NO exceptions): every file declares a package, **never
  inferred** — including `-e`/stdin one-liners (they must write `package main;` explicitly). Purest
  "nothing in the wind". Accepts the one-liner ceremony cost.
- [2026-06-18] CONTEXT (verified): the byte-identity spine (`tests/differential.rs`
  `all_examples_match_between_backends`) globs `examples/**/*.phg` and runs ONE file at a time via
  `cmd_run(&src)`/`cmd_runvm(&src)` — multi-file projects need a project-aware harness. run/check/
  transpile take only `src: &str` (no path); only `cmd_build` gets `input_path` (`src/cli.rs`).
- [2026-06-18] CONTEXT (verified): PSR-4 maps a namespace prefix → base dir; `\`=`/`; FQCN→file path
  (PHP-FIG PSR-4, Composer schema). Phorge's mandatory folder=path = **PSR-4 promoted from convention
  to language rule**; transpile = emit PHP files in PSR-4 layout + a generated autoload/composer block.
  Contract holds: Phorge package resolution : PHP/PSR-4 :: TS module resolution : JS.

## Open items — RESOLVED in the design spec (`docs/specs/2026-06-18-m5-project-model-design.md`)
- O-1 Source root → **convention `src/`, overridable via manifest `source =`** (M5-6).
- O-2 Manifest → **minimal `phorge.toml`** ([package] name/version/source + [dependencies]); its
  presence (walk up) is the sole project-detection signal (M5-5, §3).
- O-3 Multi-file loader → **entry-point loader assembles a compilation unit; backends unchanged until
  qualified calls (S2c)**. Single-file `package` decl (S1) is runtime-inert → byte-safe (§5).
- O-4 Cross-package calls → **leaf-qualified** `import app.util;` → `util.parse(x)`, emit
  `\App\Util\parse($x)` (M5-8/M5-9). Resolution in all four backends = S2c.
- O-5 PHP emission → **single-file brace-namespaces** + nameless bootstrap block; runs with bare
  `php out.php`, no Composer/autoloader (M5-7, §4). Resolves the PSR-4-can't-autoload-functions nuance.
- O-6 Harness → **project-aware differential** (S2d): single-file `package main` examples keep the glob;
  multi-file projects discovered + run by entry.
- O-7 vendor/git → **pinned tag/rev + `phorge.lock` (SHA) + committed `vendor/` auto-used offline**
  (M5-10, S3). Examples resolve offline only — never network (determinism gate, like M6 URL deferral).
- O-8 Migration → **S1 slice**: `package main;` into ~25 examples + ~200 inline programs (mechanical,
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
  `package main;` (test helpers auto-prepend, line-preserving); fixed pre-existing Wave-1 `README.md`
  drift (`import std.io;` + bare `println`).
- [ ] **S2a — manifest + source root + project detection** (`phorge.toml`, walk-up discovery).
- [ ] **S2b — multi-file loader + strict folder=path enforcement**.
- [ ] **S2c — qualified cross-package calls** (4-backend resolution) + multi-namespace PHP emission +
  import aliasing. *(The one byte-identity-risky slice — gate with multi-file `agree`/`agree_err`.)*
- [ ] **S2d — project-aware differential harness + `examples/project/` showcase**.
- [ ] **S3 — git deps + `phorge.lock` + `phorge vendor` + auto-offline** (final M5 slice or follow-up).

> Phase 3C convergence gate runs before S1 implementation begins. Each slice re-enters Phase 5→6→6C.
