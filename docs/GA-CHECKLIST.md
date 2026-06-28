# Phorj GA Definition-of-Done

> Created 2026-06-27 because the GA "percentage" had become uncalibrated gut-feel stuck near ~70-77%.
> This file is the **real denominator**: GA% is computed from the weighted table below, not estimated.
> Update the per-rock status as work lands; recompute the total. Supersedes any vibe-% in chat.
>
> **GA = a shippable 1.0**: stable surface, usable day-to-day, documented, validated — NOT "how much
> language exists." A finished language with no tooling/docs/stability is *not* shippable, so language
> being ~done does not make us ~done.

## The rocks (weighted by GA-criticality)

| # | Rock | Weight | Status | Contribution | Done-criteria (the bar) |
|---|------|-------:|-------:|-------------:|---|
| 1 | **Language & type system** | 25% | **95%** | 23.75 | core/types/generics-all/unions/intersections/inheritance/traits/errors/packages/decimals shipped. Remaining: generic variance, erased-operand edges, statics (inherited/overloaded/LSB). |
| 2 | **Daily-use tooling** | 20% | **70%** | 14.0 | `run/runvm/check/transpile/build/bench/disasm/explain/lift/vendor/serve` ✓; **`phg test` + `Core.Test` ✓ (M-Test)**; **`phg fmt` ✓ (M-fmt — comment-safe, meaning-preserving)**. **Missing: an LSP** (editor errors/hover/completion/go-to-def) + fmt line-reflow. Bar: author + test + format + edit-with-feedback. |
| 3 | **Stability & conformance** | 20% | **15%** | 3.0 | Missing: frozen language surface, a **conformance test corpus** asserting the spec, a written **semver/BC + deprecation policy**. Bar: surface frozen + conformance suite green + BC policy published. |
| 4 | **Stdlib completeness** | 15% | **70%** | 10.5 | ~22 modules ✓. Missing: **regex, datetime**, IO/streams beyond `File`, full higher-order collections, `sprintf`/format, path, log. Bar: charter-complete coverage of the common-program surface. |
| 5 | **Documentation** | 12% | **40%** | 4.8 | README/ROADMAP/VISION/FEATURES/CONTRIBUTING ✓. Missing: a real **language reference**, a **tutorial**, a complete **stdlib reference**, a PHP-migration guide. Bar: a newcomer can learn + a user can look up any feature/native. |
| 6 | **Validation / dogfooding** | 8% | **10%** | 0.8 | Missing: a **nontrivial real app built in Phorj**, perf targets stated+met vs PHP, CI on it. Bar: ≥1 real program shipped in Phorj + perf bar met. |
| | **GA total** | 100% | | **≈ 57%** | |

**Honest GA ≈ 57%** (baseline 49% → +3 M-Test → +5 M-fmt). The gap from the old vibe-77% is the whole
point: that number weighted "language exists," not "1.0-shippable." **Global** (the full VISION incl.
M7–M13, IDE, batteries, editions) is lower still — roughly **~45%** — but GA is the number that
matters for a release.

## Why it's felt stuck
The language (rock 1, 25% weight) was already ~done, so shipping *more language* (a cast operator, a
hash fn) barely moves the total. The needle lives in rocks 2–6, which were getting pebbles, not focus.

## The critical path (what actually moves GA, in leverage order)
1. **Rock 2 — daily-use tooling.** `phg test` → `phg fmt` → minimal LSP. Unblocks rock 6 (dogfooding),
   which exposes what's really missing. Biggest single GA mover (+~14 points if taken to 100%).
2. **Rock 3 — stability/conformance.** Literally what "1.0" means (+~17 points).
3. **Rock 4 — stdlib (regex/datetime)** then **Rock 5 — docs**, in parallel where possible.

## Burn-down log
- 2026-06-27: baseline established. GA ≈ 49%. Next focus: rock 2 (tooling), starting M-Test
  (`docs/specs/2026-06-27-m-test-design.md`, T1).
- 2026-06-27: **M-Test COMPLETE** (T1–T5) — `phg test` runner + `Core.Test` assertions + `assertFaults`
  + `selftest/` showcase. Rock 2 30% → 45%. **GA ≈ 49% → 52%.** Next on the critical path: `phg fmt`
  (comment-safe), then a minimal LSP, to finish rock 2.
- 2026-06-27: **M-fmt COMPLETE** (F1–F4) — `phg fmt`: lexer comment side-channel, full-surface
  meaning-preserving AST printer (chose a real AST printer over a token reformatter — challenged the
  spec's false "reuse the printer" premise), gofmt-shaped CLI, dogfood over the whole example corpus
  (caught 3 printer bugs). Rock 2 45% → 70%. **GA ≈ 52% → 57%.** Next on the critical path: a minimal
  **LSP** (reuse the checker's Diagnostic surface) to finish rock 2; then rock 3 (stability/conformance).
<!-- Update this file when a rock's status changes; recompute the total; append a dated burn-down line. -->
