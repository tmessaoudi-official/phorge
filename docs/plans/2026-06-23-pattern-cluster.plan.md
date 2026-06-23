# Pattern cluster Plan (M-RT follow-up — guards + destructuring + flow-narrowing)

> The post-M-RT language-ergonomics slice: `match`/`if-let` **guards**, **payload destructuring**, and
> **flow-narrowing** — the defining TS/Rust pattern capability a PHP-from-TS migrant expects. Front-end
> only (no new `Op`, no `Value` change targeted). Byte-identical `run ≡ runvm ≡ real PHP 8.4`.
> **Design-first**, then slice-by-slice build.

## Decisions Log
- [2026-06-23 ~10:00] AGREED: post-M-Decomp milestone selection. The full GA top-10 spine items 1–4
  (totality, generic enums/Result, error model, OO slices incl. overloading/extends/traits) are all
  CLOSED; error model + M6 web + M-Decomp verified shipped. Developer chose **all of the remaining open
  spine items (#5–#10)**, and accepted the recommended **risk-adjusted order**:
  **#5 pattern cluster → #7/#9 stdlib breadth+charter → #8 DX trio → #6 M-NUM decimal**, with **#10
  GA-governance docs interleaved** as low-effort filler.
  Rationale: front-load the front-end-only / additive wins (which also validate the fresh
  decomposition cheaply) and defer the single value-kernel-touching, externally-constrained milestone
  (decimal) to last — unless money becomes an explicit near-term business need, which would override.
- [2026-06-23 ~10:00] AGREED: open **#5 with a design pass first** (brainstorm → spec + plan →
  developer approval → slice-by-slice build), not autonomous build-through. #5 is a Large language slice
  touching the parser + checker + all three backends' pattern surfaces.
- [2026-06-23 ~10:15] AGREED: **scope = "Everything" (maximal envelope)** across all three axes:
  (1) **guards** — match-arm + if-let; (2) **payload destructuring** — un-reject nested type-patterns in
  variant payloads (`Wrapper(Circle c)`) **plus** new **class/named-field destructuring** `Point { x, y }`
  (new `Pattern::Struct`); (3) **flow-narrowing** — negative/else narrowing, early-return narrowing,
  post-exhaustive-match narrowing, **plus** equality/null/literal refinement (`== null`, literal `==`).
  Front-end-only target (no new `Op`, no `Value` change); byte-identical `run ≡ runvm ≡ real PHP 8.4`.
  Grounded gap inventory verified against `ast/mod.rs`/`parser/patterns.rs`/`checker/stmt.rs`/`KNOWN_ISSUES.md`.
- [2026-06-23 ~10:25] AGREED: **guard keyword = `when`, as a *contextual* keyword** (special only in
  guard position — after a pattern before `=>`, and in if-let before `)`; like `as` for import aliasing,
  reserves nothing globally). Chosen over `if` after challenge: kills the body-`if`-expr collision
  (`Circle c when … => if (…) {…}` reads cleanly), strong guard-specific precedent (C#/F#/Elixir/Erlang),
  zero reservation cost via contextual treatment. Guarded arms do NOT count toward exhaustiveness — an
  unguarded fallback for that shape is still required (new checker rule).
- [2026-06-23 ~10:30] AGREED: **class/struct destructuring = full nesting + rename** (`Pattern::Struct`):
  shorthand `Point { x, y }`, rename `Point { x: px }`, and nested field patterns
  `Line { from: Point { x, y }, to }`. Chosen over shorthand-only after challenge: uniform with the
  already-committed nested payload patterns (`Wrapper(Circle c)`) — anything less is an arbitrary
  asymmetry/surprise; and struct patterns are single-type tests, so nesting adds **no new exhaustiveness
  surface** (only binding + nested-`instanceof` lowering, which earns its own build sub-slice).

## Formal Plan
<!-- written at Phase 4, after the design pass / brainstorming is approved -->
