# Track S — Governance / release / stability (GA M12)

## Track summary

Phorj's stability story is, today, almost entirely *aspirational*. `GOVERNANCE.md` documents
exactly two things: the single-maintainer (BDFL) decision model and where decisions are recorded
(`docs/specs/` + `docs/plans/` as ADRs, now supplemented by `docs/adr/`). It explicitly *defers* a
deprecation/RFC process to "future evolution." The GA roadmap (`docs/plans/2026-06-19-phorj-ga-roadmap.plan.md`)
carries three thin M12 checkboxes — "semver / stability commitment," "frozen 1.0 grammar," and
"release automation + SHA-256 checksums" — but none are specified, and there is **no document that
states what 1.0 promises, what counts as a breaking change, how a language feature is deprecated, or
how syntax evolves after 1.0 without breaking existing code.** `SECURITY.md` has a one-line supported-
versions table ("latest release / master only"); there is no LTS or backport policy. `Cargo.toml` is
at `0.4.0` with no published-release machinery (`cargo dist`/tags/checksums). The correctness spine
(`run ≡ runvm ≡ php`) is a *technical* invariant, not a *promised* contract to users.

The single highest-leverage gap, judged through the philosophy, is a **Rust-style editions mechanism**
— because Phorj is pre-1.0 and *still reshaping its own syntax weekly* (the namespace reshape, the
`package Main` → `package Main` rename, stdlib PascalCase migration are all live breaking codemods).
Editions are the one governance primitive that lets a language keep evolving syntax *without a churn of
broken code*, and they map cleanly to Phorj's existing front-end-erasure discipline (an edition is a
front-end flag, the backends see one lowered tree). They are also a genuine **upgrade over PHP** —
PHP has no editions; its only evolution tool is the multi-major-version deprecation treadmill, which is
exactly the "surprise" Phorj exists to remove. Most other items here are documentation/process work
(adopt, low effort) or honestly deferrable until contributors actually arrive (RFC process, formal
governance evolution).

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|----|-------|------|-----|-----|-----------|--------|
| S-semver-policy | Documented semver + stability policy for the language surface | port | strong | adopt | GA-M12 | S |
| S-breaking-change-def | Explicit definition of what is a breaking change (BC contract) | new | strong | adopt | GA-M12 | S |
| S-deprecation-policy | Deprecation policy + `@deprecated` / `W-DEPRECATED` lint lane | port | strong | adopt | GA-M12 | M |
| S-editions | Rust-style editions mechanism (`edition` in `phorj.toml`) | new | strong | defer | new milestone M13 (post-1.0) | L |
| S-frozen-grammar | Frozen, versioned 1.0 grammar reference (the BNF/EBNF SSOT) | port | strong | adopt | GA-M12 | M |
| S-release-automation | Release automation: tags, signed-ish artifacts, SHA-256 checksums | port | ok | adopt | GA-M12 | M |
| S-changelog-discipline | Keep-a-Changelog → versioned release notes at 1.0 cutover | map | strong | adopt | GA-M12 | S |
| S-rfc-process | Lightweight RFC process for language changes | port | ok | defer | new milestone M13 (post-1.0) | M |
| S-lts-backport | LTS / backport / supported-versions policy beyond "latest only" | port | weak | reject | — | M |
| S-stdlib-stability-tiers | Per-API stability tiers (stable / experimental / `internal`) for stdlib | new | ok | defer | M11 (stdlib breadth) | M |
| S-msrv-policy | Documented MSRV + MSRV-bump policy (toolchain stability) | port | ok | adopt | GA-M12 | S |
| S-governance-evolution | Formal governance evolution (core team, commit rights) when contributors arrive | map | ok | defer | post-1.0 | S |
| S-feature-gating | Unstable-feature gating flag (ship behind `--unstable`/edition gate) | new | ok | defer | new milestone M13 | M |
| S-version-binary-contract | `.phorj` bundle / bytecode version compatibility contract | port | ok | adopt | GA-M12 | S |
| S-security-disclosure-sla | Security disclosure SLA / advisory cadence (extends SECURITY.md) | map | weak | defer | post-1.0 | S |

## Rationale for ADOPT items

**S-semver-policy** — Phorj needs one short document stating that *after 1.0* the language surface
follows semver: a major bump for source-incompatible language changes, minor for additive features,
patch for fixes. This is the literal content behind the M12 "semver / stability commitment" checkbox,
which is currently a checkbox with no text. It is pure upside for adoption — a PHP dev evaluating
Phorj wants to know "if I write this today, will it compile next year?" Familiar (semver is universal),
legible, cheap. [Verified: GA roadmap M12 lists the commitment as a `[ ]` checkbox with no backing
doc; `GOVERNANCE.md` does not mention semver.]

**S-breaking-change-def** — A stability policy is meaningless without a precise definition of what
constitutes a break. Phorj has an unusually *crisp* one available because of its correctness spine:
a change is breaking iff a previously-`Ok` program changes its byte-identical output, fails to
type-check, or fails to run. That definition is provable and testable against the differential harness
— a genuine strength PHP cannot state this precisely. Low effort, strong fit; pairs with S-semver-policy
in the same document. [Inferred: the `run ≡ runvm ≡ php` invariant in `docs/INVARIANTS.md` gives an
operational BC oracle; no doc currently frames it as the BC contract.]

**S-deprecation-policy** — PHP's deprecation lifecycle (`E_DEPRECATED` in one minor, removal in the
next major) is the canonical PHP evolution tool; a PHP dev expects "deprecated, then removed, with a
window." Phorj already has the exact mechanism to do this *better*: the warning channel
(`check()` returns `Ok(warnings)`, shipped in M3 S2; lints like `W-FORCE-UNWRAP`, `W-PHP-BUILTIN-SHADOW`).
A `@deprecated` annotation surfacing a `W-DEPRECATED` lint, plus a written policy ("deprecated for one
minor before removal in the next major"), is additive, front-end-only, and maps to PHP's idiom while
being compile-time-visible rather than runtime-only. Medium effort (annotation parse + lint lane +
policy doc). [Verified: warning channel exists per CLAUDE.md M3 S2 + KNOWN_ISSUES lints; no deprecation
annotation or policy exists.]

**S-frozen-grammar** — The M12 "frozen 1.0 grammar" checkbox needs an actual artifact: a single
versioned grammar reference (EBNF) that *is* the stability boundary for syntax. Phorj has no grammar
SSOT today (the grammar lives implicitly in `src/parser.rs`). For a language whose whole pitch is
"legible and provably correct," a published grammar is table stakes and the anchor every later edition/
deprecation decision references. Medium effort (extract + verify against the parser). [Verified: no
grammar file in repo root or `docs/`; M12 lists "frozen 1.0 grammar" as an unbacked checkbox.]

**S-release-automation** — 1.0 cannot be hand-tagged. The GA exit criteria already require
"reproducible builds + SHA-256 checksums per artifact" (the unblocked half of M2.5 Phase 3). This is
the release-engineering spine: git tags ↔ `Cargo.toml` version, CI-built `phg build` artifacts per
target, checksums published with each. Ok fit (it's plumbing, not language design) but a hard GA
requirement. The std-only/zero-dep philosophy means the *runtime* stays clean; release tooling (CI,
checksums) is explicitly exempt per the M2.5 spec. [Verified: GA roadmap M12 + Deferred table both
name SHA-256 checksums; `Cargo.toml` at 0.4.0, no release workflow beyond `ci.yml` gate/cross-build.]

**S-changelog-discipline** — `CHANGELOG.md` already follows Keep-a-Changelog with an `[Unreleased]`
section. The only gap is the discipline of *cutting* versioned sections at release and treating the
changelog as the human-facing record of what changed (and what broke). This is the natural companion to
semver; near-zero effort, just a documented convention + the 1.0 cutover. [Verified: `CHANGELOG.md`
head shows Keep-a-Changelog format with an `[Unreleased]` block.]

**S-msrv-policy** — Phorj pins `rust-version = "1.74"` and reads the pin from `rust-toolchain.toml` in
CI. A one-paragraph MSRV policy ("we support Rust ≥ X; bumping MSRV is a minor-version change,
announced in the changelog") makes the *build-time* stability contract explicit, mirroring the
language-surface one. This matters because Phorj is a compiler users build from source (std-only,
"builds in seconds" is a stated value). Low effort, ok fit. [Verified: `Cargo.toml` `rust-version =
"1.74"`; CI reads `rust-toolchain.toml` per GA roadmap M9; no MSRV *policy* documented.]

**S-version-binary-contract** — `phg build` embeds the program in a **versioned, CRC-guarded** `.phorj`
container; the bytecode VM has a `chunk::validate` EV-7 boundary. A short compatibility contract — "a
`.phorj` artifact's container version is bumped on format change; a phorj binary refuses an
incompatible container with a clean error, never a panic" — turns an existing technical mechanism into a
stated stability promise for the *distribution* surface. This is consistent with EV-7 (no panic on bad
input) and cheap to document since the versioning already exists. Low effort, ok fit. [Verified:
versioned CRC-guarded container described in MILESTONES M2.5 + CLAUDE.md; FNV-1a stub cache invalidates
on binary hash change — the mechanism exists, the *contract* is undocumented.]

## Notes on DEFER / REJECT (for completeness)

- **S-editions (defer → new M13):** the single most *valuable* governance feature long-term and a true
  upgrade over PHP, but **pre-1.0 churn is the wrong time to freeze an edition** — you can't have a
  "2026 edition" promise while syntax reshapes weekly. Build it *immediately post-1.0*, when the first
  edition is the frozen 1.0 grammar. It maps perfectly to Phorj's front-end-erasure discipline (edition
  = front-end flag in `phorj.toml`, backends see one lowered tree). Strong fit, deferred on *timing*,
  not merit. [Speculative on the milestone placement; the timing argument is the load-bearing reason.]
- **S-rfc-process / S-governance-evolution (defer):** GOVERNANCE.md itself says these arrive "as
  contributors arrive." Premature for a single-developer project; the spec/ADR logs already serve the
  recording function. Revisit when there's a second committer.
- **S-stdlib-stability-tiers (defer → M11):** marking stdlib APIs stable/experimental belongs with the
  stdlib-breadth milestone (`core.list`/`core.json`), not before those APIs exist.
- **S-feature-gating (defer → M13):** an `--unstable` gate is the natural partner of editions; same
  timing logic.
- **S-lts-backport (reject):** an LTS/backport policy implies maintaining multiple release lines — a
  multi-maintainer cost that contradicts the single-developer reality and the "latest only" SECURITY.md
  stance. Weak fit; do not adopt for 1.0. The supported-versions table already covers the honest answer.
- **S-security-disclosure-sla (defer):** SECURITY.md already has private reporting; a formal SLA is a
  post-contributors refinement.

## Critic pass

Read the shipped surface before judging: `GOVERNANCE.md` (BDFL + spec/plan ADRs + explicit "future
evolution" deferral of deprecation/RFC), `SECURITY.md` ("latest release / master only"; no SLA;
strong threat-model section), `SUPPORT.md`, `CONTRIBUTING.md`, `CHANGELOG.md` (Keep-a-Changelog,
`[Unreleased]`), `Cargo.toml` (`version = "0.4.0"`, `rust-version = "1.74"`), the GA roadmap M12
checkboxes (semver/stability, frozen grammar, release automation + SHA-256 — all unbacked `[ ]`),
`docs/adr/` (5 ADRs, 0001–0005). **No mis-listings found** — every one of the 15 original items is
genuinely open (none of semver-policy / BC-def / deprecation / grammar SSOT / release-automation /
MSRV / editions exists in any form on disk; confirmed by grep over `*.md` + `docs/`). The original
recommendations also survive the philosophy lens: ADOPT the cheap documentation/contract items for
GA-M12, DEFER editions/RFC/feature-gating to post-1.0 timing, REJECT LTS. Agreed.

Five long-tail items the first pass **missed** — all in this track's governance/release/stability
domain:

| id | title | kind | fit | rec | milestone | effort |
|----|-------|------|-----|-----|-----------|--------|
| S-conformance-corpus | Frozen 1.0 conformance corpus — golden byte-identical outputs as the executable BC guardrail | new | strong | adopt | GA-M12 | M |
| S-diagnostic-code-stability | Diagnostic codes (`E-*`/`W-*` + `phg explain`) declared a stable API | new | strong | adopt | GA-M12 | S |
| S-version-provenance | Embed git SHA + build metadata into `phg --version` (bug-report provenance) | port | ok | adopt | GA-M12 | S |
| S-upgrading-guide | Dedicated `UPGRADING.md` migration-notes artifact per breaking release | port | strong | adopt | GA-M12 | S |
| S-zerodep-promise | Std-only / zero-runtime-dependency framed as a user-facing stability promise | map | strong | adopt | GA-M12 | S |

**Rationale for the newly-found items:**

- **S-conformance-corpus** — `S-breaking-change-def` *defines* a break ("a previously-`Ok` program
  changes output / fails to type-check / fails to run"); it does not *operationalize* one. A frozen,
  versioned corpus of programs whose stdout is locked at 1.0 — gated by the existing differential
  harness (`tests/differential.rs`) — is the executable guardrail that *catches* a break in CI. This
  is a genuine PHP-upgrade: PHP's BC is asserted by prose + scattered tests; Phorj can make "did 1.0
  code change behavior?" a single green/red signal because the byte-identity spine already exists. The
  `examples/**/*.phg` glob is the seed; the gap is *freezing* a subset and treating its golden output
  as immutable. [Verified: harness globs examples and gates `run ≡ runvm ≡ php`; no *frozen* corpus
  concept exists — the glob grows freely.] Strong fit; distinct enough from S-breaking-change-def to be
  its own item (definition vs. enforcement).

- **S-diagnostic-code-stability** — Phorj's entire DX pitch rests on stable diagnostic codes
  (`E-PKG-PATH`, `W-FORCE-UNWRAP`, …) surfaced through `phg explain <CODE>`. Every tutorial, doc, and
  user CI grep keys on those literals. Renaming or renumbering a code post-1.0 is a *silent* break that
  no general semver document covers (it's not a syntax or output change). A one-paragraph policy —
  "diagnostic codes are a stable API; a code is never reused for a different meaning; removal follows
  the deprecation window" — is near-zero effort and uniquely Phorj (PHP has no stable error-code
  surface; its messages are prose that changes freely). Pairs naturally with `S-deprecation-policy`.
  [Verified: `phg explain` + stable codes shipped per CLAUDE.md M3 S0; no stability *promise* on the
  code namespace.] Strong fit, S effort.

- **S-version-provenance** — `S-release-automation` covers tags + checksums; it does *not* cover the
  binary knowing *which build it is*. `phg --version` ships (CLAUDE.md v0.4.0) but reports only the
  Cargo version — a bug report from a `master` build or a checksum-renamed artifact can't be tied to a
  commit. Embedding the git SHA + build metadata (a `build.rs` env capture, std-only-compatible) is
  standard PHP/tool release hygiene (`php -v` reports the build date + ZTS flags). Cheap, mechanical,
  improves the SUPPORT.md "include `phg --version`" loop. [Verified: README shows `-v`/`--version`
  exists; no SHA/build-metadata embedding visible.] Ok fit (plumbing), S effort.

- **S-upgrading-guide** — `S-changelog-discipline` cuts versioned changelog sections; PHP additionally
  ships a dedicated `UPGRADING` file per release that is *only* the breaking changes + the migration
  steps for each. A PHP dev expects that artifact by name. For a language that is *still doing breaking
  codemods weekly* (the namespace reshape, `package Main`→`package Main`, stdlib PascalCase), a
  dedicated migration-notes file — even pre-1.0 — is the honest companion to the BC contract and the
  natural home for "here's the codemod" notes. Strong familiarity fit, S effort, complements rather
  than duplicates the changelog. [Verified: no `UPGRADING.md` in repo root; breaking reshapes are
  currently recorded only in CLAUDE.md/specs.]

- **S-zerodep-promise** — The std-only / zero-external-crate line is documented as an *architectural
  fact* (THIRD-PARTY-NOTICES, SECURITY threat model, "builds in seconds") but never framed as a
  *forward stability commitment to users*: "Phorj's runtime will never acquire a third-party
  dependency." That promise is load-bearing for the supply-chain/audit story and trivially honest given
  the existing invariant — it's a `map` (the fact exists; the commitment doesn't). Strong philosophy
  fit (it's a *removed surprise* — no transitive dependency tree to audit, the opposite of Composer),
  S effort. Belongs in the semver/stability doc alongside the language-surface and MSRV promises.
  [Verified: `#![forbid(unsafe_code)]` + zero crates per SECURITY.md/THIRD-PARTY-NOTICES; framed as
  present-tense fact, not a 1.0 guarantee.]

**Adjacent items considered and deliberately NOT added** (avoid scope-creep / out-of-track):
versioned-docs hosting, a public roadmap board, contributor CLA/DCO (governance-evolution territory,
already deferred), and a release cadence/calendar (rejected for the same single-maintainer reason as
S-lts-backport — ROADMAP.md explicitly omits dates by design). None earns its surprise budget for a
pre-1.0 single-developer project.
