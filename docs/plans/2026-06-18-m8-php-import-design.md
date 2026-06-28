# M8 — PHP → Phorj Importer (reverse-transpile) — Design Plan

> **Status:** DESIGN PHASE (build deferred). Scope locked 2026-06-18. M6 web work continues
> uninterrupted; this milestone is designed now, built after M6.
> Roadmap home: ROADMAP.md M8 ("the inverse of the transpiler"); FEATURES.md "PHP → Phorj migration".

## Decisions Log

- [2026-06-18] AGREED: Build the **inverse of transpile** — a PHP→Phorj importer (reverse-transpiler),
  NOT a symmetric inverse. Framing is JS→TS: a one-way port over a *subset* of PHP.
- [2026-06-18] AGREED: **Scope = Staged A→B.** Stage A imports exactly the PHP our own transpiler
  emits (bounded grammar → gives a `phg → php → phg' ≡ phg` round-trip property). Stage B grows to
  idiomatic **typed PHP 8** (typed sigs, classes, enums, match), rejecting dynamic features cleanly.
  Stage C (general/dynamic PHP) stays rejected-by-design.
- [2026-06-18] AGREED: **Design now, defer build, keep M6.** Produce spec + milestone lock now; do NOT
  interrupt the locked M6 W2 (static router). Build starts after M6.
- [2026-06-18] AGREED (standing quality bar): every PHP feature we map must be **better-or-equal to PHP,
  never worse** — worst acceptable case is *parity with a syntax improvement*. Each mapped construct
  carries a verdict: BETTER / SAME+syntax / SAME / WORSE(reject).
- [2026-06-18] AGREED: the design must answer **"have we mapped the entire PHP surface through 8.4
  stable / 8.5 / 8.6-dev?"** — explicit coverage matrix incl. attributes, union & intersection types,
  enums, readonly, property hooks, asymmetric visibility, `|>` (PHP 8.5), etc.
- [2026-06-18] AGREED (later research): a **real benchmark proving Phorj > PHP** is required eventually
  — parked for its own research+brainstorm pass, noted here so it is not lost.

- [2026-06-18] AGREED: Stage A round-trip is **behavioral**, not AST-identity — transpile erases
  types (`bytes`→`string`, `T?`→`mixed`, enum→class) so it can't be syntactically inverted. Property:
  transpile `phg` → re-import → run `phg'` → output matches (reuses the real-PHP execution-diff
  machinery, inverted). Honest consequence: Stage A bootstraps the PHP parser; Stage B carries the
  migration product value.
- [2026-06-18] AGREED: language work the importer needs (to be ≥ PHP) = **named arguments**,
  **variadics**, and **union types mapped to tagged enums** (raw `T|U` stays rejected; the union→enum
  mapping is the better idiom). Marked as M8 prerequisites / co-features.
- [2026-06-18] AGREED: the coverage matrix must be **EXHAUSTIVE over the entire living PHP surface** —
  every non-deprecated construct (PHP 3/4-era through 8.6) **and every builtin function** mapped to a
  Phorj `core.*` module or a status verdict. Not limited to the 8.0+ "new feature" surface. Research
  decomposed across 5 parallel domain sweeps (language / OOP+functional / stdlib×3), raw in
  `docs/research/m8/raw/`.

## Formal Plan
<!-- written at Phase 4 approval, after exhaustive research + coverage matrix -->
