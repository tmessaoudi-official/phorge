# M7 — Correctness Closure — Execution Plan

> Companion to `docs/specs/2026-06-19-m7-correctness-closure-design.md`. First milestone of the GA
> roadmap (`docs/plans/2026-06-19-phorj-ga-roadmap.plan.md`). TDD throughout: each wave writes the
> failing test first, then the fix; every wave ends `cargo test` green + `cargo clippy --all-targets`
> + `cargo fmt --check` clean before commit. Code state at plan time: master `8c6fbb2`
> (code spine `687a7bd`), 452 tests green, `php 8.6.0-dev` on PATH.

## Decisions Log
- [2026-06-19] AGREED: spec M7 first (developer), full 30/8 3C gate run before authoring (converged 8/8).
- [2026-06-19] AGREED (developer approved spec): runtime PHP helpers (`__phorj_div`/`__phorj_rem`/
  `__phorj_str`/`__phorj_range`) for P0-1/3/4 + QW-13 over a static transpiler type resolver
  (see spec §4.0); P0-2 = syntactic precedence parens; `MAX_RANGE_LEN = 10_000_000` shared const;
  oracle folded into `tests/differential.rs`; the 2 self-skip `cli.rs` php tests deleted.
- [2026-06-19] AGREED: begin TDD Wave 1 (oracle red), then W2→W4.

## Wave 1 — Oracle harness (red: must FAIL, proving the P0 bugs)
1. `tests/differential.rs`: add `php_bin()` (honor `PHORJ_PHP`, else probe PATH) + `require_or_skip()`
   (panic if `PHORJ_REQUIRE_PHP=1` and absent; else loud `eprintln!` skip).
2. Add `all_examples_transpile_and_match_php` (glob, §3.2) + `all_example_projects_transpile_and_match_php`
   (§3.3): transpile → unique temp `.php` → `php -n` → assert `status.success()` + stdout == `cmd_run`.
3. Run with php present → **expect RED** on `operators.phg` (P0-1 `7/2`→`3.5`) and any bool-interp /
   precedence / range example. Capture the failing output as the proof the oracle works.
4. Delete the two self-skip php tests in `tests/cli.rs`
   (`transpiled_php_runs_and_matches_interpreter`, `safe_access_transpiles_and_runs_in_php`); keep the
   golden `demo.php` drift test. Confirm no other refs.
- **Gate:** the new oracle tests FAIL for the right reason (P0 divergences), clippy/fmt clean. Do **not**
  commit red — W1 is the red half of W1+W2; commit only after W2 turns it green.

## Wave 2 — The four emitter fixes (turns W1 green)
1. `src/transpile.rs`: add `uses_div`/`uses_rem`/`uses_str`/`uses_range` flags + emit the four helper
   bodies (spec §4.1) in the once-per-file helper block, namespaced-`\` aware (mirror `__phorj_unwrap`).
2. P0-1/P0-4: special-case `Div`/`Rem` in the `Binary` arm → `__phorj_div`/`__phorj_rem`; make them
   `unreachable!` in `binop()` (§4.2).
3. P0-3: wrap each `StrPart::Expr` in `emit_string` with `__phorj_str(…)` (§4.3). **Do not touch
   `value.rs`.**
4. P0-2: add `is_primary(&Expr)` + wrap non-primary operands in Unary and the generic Binary join
   (§4.4); resolve the `Coalesce`-double-paren corner (oracle-verified, harmless either way).
5. Per-P0 inline unit tests (boundary values: `-7/2`, `7/-2`, `5.5%2.0`, `!(a&&b)`, bool local).
- **Gate:** W1 oracle now GREEN; `run≡runvm` unchanged; full suite + clippy + fmt clean.
  **Commit W1+W2 together** (`fix(transpile): close the PHP correctness leg — oracle + 4 P0 fixes`).

## Wave 3 — Range fixes
1. QW-13: route `Expr::Range` emit through `__phorj_range` (§5.1); add empty/reversed examples/tests.
2. P1-#9: add `const MAX_RANGE_LEN: i64 = 10_000_000;` (single-sourced) + `checked_sub` length guard +
   `"range too large"` fault, applied **identically** in `src/vm.rs:252-263` and
   `src/interpreter.rs:380-389` (§5.2).
3. `tests/differential.rs`: add `FaultKind::RangeTooLarge` + `classify()` arm; add
   `large_range_faults_identically` (`agree_err`).
- **Gate:** `agree_err` green on both backends; large range no longer aborts (exit 101 gone); suite +
  clippy + fmt clean. Commit (`fix(lang): range edge-cases — empty/reversed PHP emit + large-range fault`).

## Wave 4 — Examples + docs (doc-truth for the closed leg)
1. Examples: ensure division/bool/precedence/range cases live in runnable examples (fold into
   `examples/guide/operators.phg` or add `examples/guide/division.phg`) + `examples/README.md` entry —
   they are then auto oracle-gated.
2. Docs: KNOWN_ISSUES (remove the now-fixed P0 entries; keep the float-format caveat), FEATURES/CHANGELOG,
   and the M7 status note (the third leg `run≡runvm≡php` is now enforced locally; CI wiring = M9).
3. Update `CLAUDE.md` active-plan line + GA roadmap M7 checkboxes; flip the spec/this plan Status.
- **Gate:** oracle green over the expanded example set; Completion-Gate evidence table (coverage/docs/
  config/blast-radius) in the response. Commit (`docs: M7 correctness-closure examples + doc-truth`).

## Completion Gate (Rule 6 — produced at Phase 6, all four dimensions)
- **Coverage:** paste `cargo test` output incl. the oracle test names + counts; oracle red→green proof.
- **Docs:** KNOWN_ISSUES diff (P0 entries removed), CHANGELOG/FEATURES, MILESTONES leg-closed note.
- **Config:** `PHORJ_REQUIRE_PHP` / `PHORJ_PHP` documented; M9 CI seam noted.
- **Blast radius:** `grep` for the deleted cli.rs tests, `binop()` `Div`/`Rem` callers, every
  `MakeRange`/`Expr::Range` site, and `as_display` (untouched) accounted for.

## Phase 8 disposition
On M7 completion: per Rule 17, propose deleting this plan (keep the spec) and updating the GA roadmap
M7 section to DONE. Carried review cleanup (dangling branch, S3 plan disposition) handled separately.

## Status
STATUS: ✅ Implemented — W1–W4 complete. Oracle (examples + projects, fails-not-skips), 4 P0 fixes via
runtime helpers, P0-2 precedence parens, QW-13 `__phorj_range`, P1-#9 large-range cap +
`FaultKind::RangeTooLarge`, 5 self-skip `cli.rs` tests removed, regression tests added, `demo.php`
golden regenerated, docs updated (KNOWN_ISSUES / CHANGELOG / CLAUDE.md / GA roadmap). ~453 tests green,
clippy + fmt clean. Phase-8 disposition (delete plan, keep spec) pending developer confirmation.
