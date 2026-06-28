# Roadmap Completeness Review — Plan

> A single comprehensive research + brainstorming pass to **find every gap** in Phorj's
> roadmap/milestones and **lock each into the planning docs**, so gaps stop being discovered ad hoc.
> Developer-requested 2026-06-21: *"I keep detecting missing things (class private, error handling,
> missing PHP features, beyond-PHP game-changers, small DX/syntax wins). Capture everything and lock
> it in plans/roadmaps/milestones/specs so I stop interrupting you."*

## Decisions Log
- [2026-06-21] AGREED: run **one definitive roadmap-completeness review** (supersedes the narrower
  `php-parity-review`, which becomes Track A of this). Goal: an exhaustive, triaged gap list folded
  into `ROADMAP.md` / `docs/MILESTONES.md` + a consolidated spec, so the developer stops finding gaps
  one at a time. See [[php-parity-review]], [[ga-roadmap-spec-m7-next]], [[philosophy-of-phorj]].
- [2026-06-21] SCOPE LOCKED — **19 tracks (A–S + V)**. The tracks are the *search space*; at run time
  each gets parallel research agents + a completeness-critic loop that enumerates the exhaustive item
  list and cross-checks against shipped features (FEATURES/KNOWN_ISSUES/docs) so nothing is re-listed.
  - **A — PHP parity:** every PHP language/stdlib feature Phorj lacks → port / map / omit (+reason).
    Incl. PHP 8.3/8.4 recents (typed class constants, `#[\Override]`, asymmetric visibility
    `private(set)`, readonly classes, DNF types, `json_validate`), magic methods, backed enums,
    generators/`yield`, references `&`, late static binding, `declare(strict_types)`, streams.
  - **B — Beyond-PHP game-changers** (TS:JS-over-PHP; judged vs [[philosophy-of-phorj]]): deeper
    pattern matching, ADTs, generic bounds/variance/generic-enums, Result/Option, structured
    concurrency (async/channels/actors), design-by-contract, derive-serialization, persistent
    collections, typestate, refinement types/units, comptime/macros, TCO, reactive primitives.
  - **C — DX & syntax ergonomics:** LSP (hover/go-to-def/rename/inlay-hints/quick-fix), `phg fmt`,
    REPL, doc-gen, `phg new`, watch mode, doctests, dead-code/unused-import, numeric separators,
    replay debugging, sharper diagnostics, common-mistake messages.
  - **D — Consolidate already-found gaps:** visibility (DONE), error-handling/traces (IN PROGRESS),
    promotable KNOWN_ISSUES deferrals.
  - **E — PHP interop & migration (the adoption killer, *how TypeScript won*):** gradual PHP→Phorj
    migration, a PHP→Phorj codemod/importer, calling existing Composer/PHP libs, mixing `.php`+`.phg`,
    Phorj as a typed layer over an existing PHP codebase.
  - **F — Tooling & ecosystem maturity (1.0):** LSP, formatter, package registry/publishing, docs
    site, web playground, debugger, test framework+coverage, profiler, editor extensions, CI.
  - **G — Real-world "batteries":** HTTP server (M6)+client, DB/PDO, env/config, logging, CLI
    arg-parsing, file/dir ops, process spawning, datetime, crypto/hashing, UUID, random, base64/hex,
    compression, regex.
  - **H — Correctness & safety guarantees (the "provably-correct" pillar):** exhaustiveness
    everywhere, totality, what can still crash/UB, contracts, a type-system completeness audit.
  - **I — Performance:** VM opt passes, AOT/native (v2), sized ints, perf-vs-PHP tracking, inlining.
  - **J — Semantics edge cases:** identity/`===` (deferred), ordering/hashing, iteration protocols,
    operator overloading, unicode/string encoding, the numeric tower (precision).
  - **K — Security & safety posture (GA M8; GRDF-relevant):** injection-safe-by-construction
    (XSS✓/SQL/command/path), secrets, capability/sandbox, supply-chain (vendor+lock✓, audit),
    production-no-leak (traces✓), auth/CSRF helpers, crypto correctness.
  - **L — Stdlib API design & breadth:** the `Core.*` surface as a designed whole — naming/consistency,
    lazy iterators/sequences, collection-method completeness, which modules should exist.
  - **M — i18n / text:** unicode correctness, locale-aware formatting, message catalogs, segmentation.
  - **N — Numerics & business-data (a real PHP-upgrade game-changer):** a typed **decimal/money type**
    (no float for currency — huge for business/GRDF apps), bigint, fixed-point, the numeric tower,
    **date/time correctness** (timezones/DST) — areas PHP is famously error-prone.
  - **O — Testing & quality story:** first-class test framework, assertions, mocking, property-based
    testing, fuzzing, coverage, snapshot testing (benchmark✓).
  - **P — Build/deploy/distribution:** standalone binaries✓ + cross-compile✓ (consolidate), packaging,
    containers, serverless/FaaS, signing (M2.5 Ph3 parked), reproducible builds.
  - **Q — Observability:** structured logging, tracing/spans, metrics, panics/recovery, introspection.
  - **R — Docs & learnability (GA M12):** language reference, the book/tour, API doc-gen, migration
    guides, error-as-docs (`explain`✓).
  - **S — Governance / release / stability (GA M12):** semver + stability policy, deprecation policy,
    an **editions mechanism** (Rust-style), RFC process, backwards-compat guarantees.
  - **V — Competitive analysis (cross-cutting):** mine TypeScript, Hack, Kotlin, Swift, Rust, Go,
    Gleam, Roc, Elixir for adoption lessons and map onto Phorj.
- [2026-06-21] METHOD: a **multi-agent workflow** (workflow opt-in already standing from the
  php-parity-review) — parallel web-research tracks (PHP docs/RFCs, TS/Hack/other transpiled langs,
  modern-language DX surveys) × a **completeness-critic loop** (keep finding until N dry rounds) ×
  **BATCHED `ask-human` review** (triage each candidate: port / defer / reject + milestone slot), then
  write-back into ROADMAP/MILESTONES/specs. Deliverable: `docs/specs/2026-06-21-php-parity-and-beyond.md`
  (broadened to cover all four tracks) + roadmap/milestone edits.
- [2026-06-21] TIMING: developer is compacting soon; **this review RUNS after compaction** (it is a
  long multi-agent effort, better fresh) unless the developer says run-now. State is saved so it
  resumes as the first post-compaction action.

- [2026-06-22] EXECUTED: the workflow ran (41 agents, 20/20 tracks, no drops) → 555 candidates
  (290 adopt / 187 defer / 81 reject). SSOT written to `docs/specs/2026-06-21-php-parity-and-beyond.md`;
  raw per-track reports under `docs/research/roadmap-completeness/raw/`.
- [2026-06-22] AGREED (batched triage, all developer-confirmed):
  1. **Error model (theme 1 / Slice 2):** three tiers — enforced typed `throws E` (PHP-familiar
     *default*, fixes PHP's unchecked `@throws`, → PHP exceptions) + `Result<T,E>` (value surface) +
     unchecked faults/panics for bugs (the fix to Java's checked-everything mistake). Both checked tiers
     typed + checker-enforced + `?`-composable; `throws` erases before backends (no new `Op`).
     Developer's addition over the bare "Result-first" recommendation; challenged (Java-checked-exception
     failures) and reconciled via the three-tier model.
  2. **M-RT order:** totality cluster (return-totality + `never` + unreachable-after-return) lands
     **before** method overloading (was previously "overloading next").
  3. **New milestones approved:** M4, M-NUM, M-TIME, M-text, M-Test, M-perf, M-Batteries, M8.5, M13.
  4. **Write-back scope:** full restructure into ROADMAP.md + MILESTONES.md (spec stays detailed SSOT).
  5. **PascalCase packages enforced everywhere incl. vendor** (PHP/Composer deps case-mapped at the
     importer boundary); the namespace reshape added as a tracked roadmap item (audit had missed it).

## Formal Plan
<!-- DONE 2026-06-22: workflow authored + run; batched triage resolved; written back to
     ROADMAP.md / docs/MILESTONES.md + the consolidated spec. Method as designed: 20 parallel research
     tracks → per-track completeness-critic → synthesis → batched ask-human triage → write-back. -->
**STATUS: COMPLETE — audit delivered, decisions locked, written back (2026-06-22).**
