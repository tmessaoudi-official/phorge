# Roadmap Completeness Review — Plan

> A single comprehensive research + brainstorming pass to **find every gap** in Phorge's
> roadmap/milestones and **lock each into the planning docs**, so gaps stop being discovered ad hoc.
> Developer-requested 2026-06-21: *"I keep detecting missing things (class private, error handling,
> missing PHP features, beyond-PHP game-changers, small DX/syntax wins). Capture everything and lock
> it in plans/roadmaps/milestones/specs so I stop interrupting you."*

## Decisions Log
- [2026-06-21] AGREED: run **one definitive roadmap-completeness review** (supersedes the narrower
  `php-parity-review`, which becomes Track A of this). Goal: an exhaustive, triaged gap list folded
  into `ROADMAP.md` / `docs/MILESTONES.md` + a consolidated spec, so the developer stops finding gaps
  one at a time. See [[php-parity-review]], [[ga-roadmap-spec-m7-next]], [[philosophy-of-phorge]].
- [2026-06-21] SCOPE (four tracks):
  - **A — PHP parity:** every PHP language/stdlib feature Phorge lacks; for each, decide port / map /
    intentionally-omit (with reason). (The existing php-parity-review scope.)
  - **B — Beyond-PHP game-changers:** features that make Phorge a clear *upgrade* (the TS:JS-over-PHP
    play) — richer types, exhaustiveness, immutability/mutation ergonomics, pattern matching depth,
    concurrency model, tooling/LSP — judged against [[philosophy-of-phorge]] (pragmatic, legible,
    no-surprises; familiarity-first; remove surprises NEVER capability).
  - **C — DX & syntax ergonomics:** the "many small features + syntax improvements" — quality-of-life
    wins, sharper diagnostics, sugar that pays its weight, the papercuts the developer keeps hitting.
  - **D — Consolidate already-found gaps:** fold in what's already discovered/decided so nothing is
    re-discovered — class visibility (DONE), error handling/stack traces (IN PROGRESS), and any open
    KNOWN_ISSUES deferrals worth promoting to the roadmap.
- [2026-06-21] METHOD: a **multi-agent workflow** (workflow opt-in already standing from the
  php-parity-review) — parallel web-research tracks (PHP docs/RFCs, TS/Hack/other transpiled langs,
  modern-language DX surveys) × a **completeness-critic loop** (keep finding until N dry rounds) ×
  **BATCHED `ask-human` review** (triage each candidate: port / defer / reject + milestone slot), then
  write-back into ROADMAP/MILESTONES/specs. Deliverable: `docs/specs/2026-06-21-php-parity-and-beyond.md`
  (broadened to cover all four tracks) + roadmap/milestone edits.
- [2026-06-21] TIMING: developer is compacting soon; **this review RUNS after compaction** (it is a
  long multi-agent effort, better fresh) unless the developer says run-now. State is saved so it
  resumes as the first post-compaction action.

## Formal Plan
<!-- author the workflow script at run time; see METHOD above. Each track → parallel researchers →
     completeness-critic loop → batched ask-human triage → write-back to ROADMAP.md/MILESTONES.md +
     the consolidated spec. -->
