# Post-Wave-3 Four-Track Plan

> **Status:** active (started 2026-06-20). Code state at start: master `a6d64bf`, tree clean.
> The developer authorized all four tracks ("all 4 options as agreed"), executed as independent
> green slices with a commit + compaction point between each. Order is the developer's:
> Option 1 → Option 2 → namespace reshape → review pass.

## Decisions Log
- [2026-06-20] AGREED: do all four queued items in order (Opt1 named tags, Opt2 `phg check --json`,
  namespace reshape, review pass) — execute as independent green slices, commit each, compact between.
- [2026-06-20] AGREED: Option 1 = macro-monomorphized per-tag natives (uniform registry, real
  eval+php, byte-identity-testable). Solves the deferred "fn-ptr can't bake a tag" blocker without
  any lexer/parser/checker/backend change — purely additive, like Wave 2. Tag names are single
  lowercase words ⇒ already reshape-safe (no camelCase migration later).

## Track 1 — core.html Option 1 (named per-tag helpers)
**Approach:** two `macro_rules!` (`tag_el!`, `tag_void!`) in `src/native.rs`, each producing a
`NativeFn` whose `eval`+`php` bake the tag literal via `concat!`/`format!`. Append a curated common
HTML5 tag set to `html_natives()`.
- Files: `src/native.rs` (macros + entries + a unit test pinning one el + one void pair),
  `examples/guide/html.phg` (Option-1 demo section), `examples/README.md`, `FEATURES.md`,
  `CHANGELOG.md`, `docs/specs/2026-06-19-core-html-design.md` (named set → shipped),
  `tests/differential.rs` (agree + transpile-shape case).
- Acceptance: `cargo test` green; PHP oracle (`PHORGE_REQUIRE_PHP=1`) byte-identical; clippy+fmt clean.
- Risk: macro Rust-eval vs PHP-php drift → pinned by unit test + example oracle.

## Track 2 — core.html Option 2 (`phg check --json`)
**Approach:** structured diagnostics — serialize the existing `Diagnostic` surface to JSON (std-only,
hand-rolled) behind a new `--json` flag on the `check` command. LSP foothold.
- Files: `src/cli.rs` (flag + JSON path), a diagnostic serializer (likely `src/diagnostic.rs` or
  inline), `tests/cli.rs`, docs.
- Acceptance: `phg check --json good.phg` → `[]`; on error → JSON array of {code,message,severity,span}.
- Risk: JSON escaping correctness → unit-test against a message containing `"`/`\`/newline.

## Track 3 — Namespace reshape (spec `docs/specs/2026-06-20-package-namespace-reshape-design.md`)
Milestone-scale, breaking. Build order (each slice independently green):
1. Manifest `name` → `module`. 2. PascalCase enforce + codemod (`E-PKG-CASE`). 3. `package main` →
`package Main`. 4. Types in libraries (lift `E-PKG-TYPE` + cross-package type mangling).
- Scoped + planned in detail when Tracks 1–2 land (re-read the spec at that point).

## Track 4 — Review pass
Act on / re-run the 2026-06-19 review reports (sleuth/inspect/gaps/forge) against the post-reshape
tree, or run a fresh pass. Hardening, not features.

## Formal Plan
- **Track 1 — DONE** (`9ca5a47`, pre-commit OK): macro-monomorphized per-tag natives, byte-identical
  run/runvm/PHP, docs + memory updated.
- **Track 2 — DONE**: `phg check --json` — std-only diagnostics serializer on `Diagnostic`
  (`diagnostic.rs`), `cli::check_json_program`, `--json` wired in `main.rs` (stdout + exit 0/1),
  unit + 2 CLI tests, FEATURES/CHANGELOG/`--help` updated. Gate green (FMT/CLIPPY 0, tests pass).
- **Track 3 — in progress** (namespace reshape):
  - **Slice 1 — DONE**: manifest distributable `name` → `module` (`src/manifest.rs` struct/parser/
    `namespace_root`; `src/loader.rs` + `tests/project.rs` + `tests/vendor.rs` fixtures; both example
    `phorge.toml`; CHANGELOG + spec §5.1 + example README). Lockfile `name` (dep coordinate) and
    `[require]` keys unchanged. Rename-only, output-preserving; 471 tests green, PHP oracle ran,
    clippy + fmt clean.
  - Slice 2 — next: PascalCase enforcement (`E-PKG-CASE`) + identifier-casing lint + codemod across
    all `.phg`/stdlib/fixtures (keyword stays `package`; entry stays `main` temporarily).
  - Slice 3: entry `package main` → `package Main`.
  - Slice 4: types in libraries (lift `E-PKG-TYPE` + cross-package type mangling + namespaced PHP +
    D5b type-vs-leaf guard).
- Track 4 planned above; refined at its turn.
