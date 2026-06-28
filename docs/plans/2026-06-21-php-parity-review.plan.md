# PHP-Parity-and-Beyond Review — Plan

> Status: **METHODOLOGY LOCKED — not yet executed.** Created 2026-06-21, right after M-RT S5 shipped
> (`e73cab9`). The developer paused the overloading slice to first run a comprehensive
> Phorj-vs-PHP (and beyond-PHP) feature review. **Overloading design resumes AFTER this review.**
> Plan-location sentinel: `repo`.

## Why (developer intent, verbatim-faithful)

The S5 work surfaced the "Phorj : PHP :: TypeScript : JavaScript" contract, which made the developer
ask: *"what else did we miss that we could do better in Phorj, or forgot to implement but exists in
PHP? … do more research on the internet about other PHP features (spaceship operator `<=>`, etc.)…
think about attributes — it's a very powerful PHP feature; what other languages use similar to
attributes? … what can we add in Phorj that is NOT in PHP but has high ROI? … cover everything, do
extensive research maybe many times, each time covering more ground and learning what it missed."*

## Locked decisions (this session, via ask-human)

- **Engine = multi-agent WORKFLOW** (explicit opt-in given). Parallel web-research sweeps × tracks ×
  multiple passes + a completeness-critic loop. Chosen over inline-iterative for exhaustiveness.
- **Review cadence = BATCHED ask-human by theme** (~8–10 themed batches, per-item verdicts the
  developer can override) — NOT ~70 one-by-one rounds, NOT skim-then-flagged.
- **Scope = options 1+2+3 combined**: the matrix covers *everything* (all ~50 shipped + ~22 gap +
  beyond-PHP ideas), and we review it (batched).
- **Per-feature row format** (developer spec, expanded):
  `| Feature | In PHP? — how | In Phorj? — how | Same/different impl | Verdict | ROI | Notes |`
  Verdict ∈ {Phorj-already-better · adopt · defer · reject}. Each row states explicitly:
  exists-in-PHP? exists-in-Phorj? implemented-differently?

## Deliverable

`docs/specs/2026-06-21-php-parity-and-beyond.md` — a **living** matrix + decision log (the single
source of truth; updated as verdicts are made and features ship).

## The 5 research tracks (each web-researched from authoritative sources)

1. **PHP core language** — full php.net language reference sweep: variadics (`...$args`), default
   parameter values, named arguments, static methods/properties, constants (`const`, class const,
   `define`), abstract classes, `protected` visibility, magic methods (`__construct`/`__toString`/
   `__invoke`/`__get`/`__set`/`__call`/`__callStatic`/`__isset`/`__clone`…), generators/`yield`,
   list/array destructuring (`[$a,$b]=`, `list()`), spread (`...`), references (`&`), `global`/`static`
   locals, anonymous classes, closures+`use`, heredoc/nowdoc, type casts, `clone`, `final`,
   `readonly`, first-class callables, fibers.
2. **PHP operators (COMPLETE set)** — incl. the easy-to-miss ones: spaceship `<=>`, null-coalesce `??`
   + `??=`, nullsafe `?->`, ternary/elvis `?:`, exponent `**` + `**=`, all compound-assign, bitwise
   (`& | ^ ~ << >>`), string concat `.` + `.=`, `instanceof`, error-suppression `@`, `clone`, `yield
   from`, type juggling/comparison semantics (`==` vs `===`).
3. **PHP per-version RFCs** — 8.0 → 8.4 (and note 8.5 if relevant) migration guides; catalogue each
   new feature with the version it landed.
4. **Attributes deep-dive** — PHP `#[Attr]` (syntax, reflection, real uses) **+ cross-language
   analogues**: Rust attributes/derive macros, C#/Java annotations, Python/TypeScript decorators,
   Kotlin annotations, Swift property wrappers/macros. Distil the best design for a Phorj attribute
   system + how it would transpile (PHP `#[...]` is the natural target).
5. **Beyond-PHP, high-ROI** — features NO PHP version has that would give Phorj an edge, surveyed
   from modern langs (Rust, Swift, Kotlin, TS, Scala 3, Gleam, Roc, OCaml): `match` guards,
   refinement/newtype-with-invariants, `const fn`/compile-time eval, effect/exception tracking in
   types, exhaustive-everything, ownership-lite/borrow hints, pattern-binding everywhere, pipeline
   stdlib, derive-style codegen, structured concurrency. Score each by ROI **given the byte-identical
   transpile constraint** (a feature that can't lower to deterministic PHP is low-feasibility).

## Multi-pass methodology (the "many passes, learn what it missed" loop)

```
pass = 1; clean_streak = 0
while clean_streak < 2:                      # loop-until-dry (2 consecutive clean passes)
    parallel sweep of the 5 tracks (web research) -> raw findings per track
    merge into the master matrix (dedupe by feature name)
    completeness-critic agent: "what category / operator / PHP version / source / language
        was NOT covered? what rows are stubs?" -> gap list
    if gap list empty: clean_streak += 1
    else: clean_streak = 0; feed gaps back as targeted sweeps next pass
    log what THIS pass added (no silent caps)
    pass += 1
```
Each feature row carries an evidence grade + source URL. The critic is adversarial about coverage,
not just correctness.

## Workflow shape (to author at execution time)

`Workflow({script})` with phases: **Sweep** (parallel, 5 track agents, web research, schema'd rows) →
**Synthesize** (one agent merges → matrix) → **Critique** (completeness critic → gaps) → loop. Use
`WebSearch`/`WebFetch` inside agents (deferred tools — agents load via ToolSearch). Cap concurrency
per the global rule. Persist each track's raw output to `$DIR/raw/<track>.md` (compaction safety).
Final: write/refresh `docs/specs/2026-06-21-php-parity-and-beyond.md`.

## ON RESUME (post-compact execution checklist)

1. Confirm HEAD = `e73cab9` (S5) + tree clean.
2. **Author + run the research workflow** (engine already approved — do NOT re-ask the opt-in; the
   developer explicitly chose the multi-agent workflow this session). Multi-pass until 2 clean.
3. Build `docs/specs/2026-06-21-php-parity-and-beyond.md` (the matrix, row format above), with my
   recommended Verdict + ROI per row.
4. **Batched ask-human review** (~8–10 themed batches): function-ergonomics / OOP / operators /
   metaprogramming-&-attributes / control-flow / collections / beyond-PHP-innovations / tooling.
   Each batch = one ask-human with per-item verdicts the developer can override; record decisions in
   the matrix's decision log + this plan.
5. From the adopted set, (re)sequence the M-RT / future slices. **Then resume the overloading slice
   (design-first)** — overloading was the chosen next slice before this review interjected.

## Context snapshot (for a cold resume)

- **M-RT S5 (intersection types `A & B`) is COMPLETE** — `e73cab9`; 474 lib + PHP-oracle differential
  + 53 integration green; `examples/guide/intersections.phg` byte-identical run≡runvm≡real PHP.
- **Overloading** is confirmed IN, was the next slice, now deferred until after this review. (Developer:
  *"this language should be equal or better than PHP."*)
- Current surface = ~37 language + ~13 tooling features (FEATURES.md). PHP-gap ≈ 22 (7 roadmapped,
  15 unplanned). See the count breakdown the developer was given this session.
- Backends correctness spine: `run ≡ runvm ≡ real PHP` (PHP oracle, `PHORJ_REQUIRE_PHP=1`). Any
  adopted feature must ship byte-identical + a guide example (the "examples ship with features" rule).
- Toolchain: `export PATH=/stack/tools/cargo/bin:$PATH`; for the gate also put `/stack/tools/zig` on
  PATH (cross-build tests) and `PHORJ_PHP=/stack/tools/phpbrew/php/php-master/bin/php`.
