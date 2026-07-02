# Full Audit & Master Plan (audit-only session) Plan

## Decisions Log
- [2026-07-01 22:10] AGREED: Execution shape = **audit-first, NO code action** — the only deliverables are documents: audit findings, decision register, PHP-gap matrix, and ONE consolidated master plan rich enough for another model (Opus 4.8) to execute unaided, with reasons attached to everything.
- [2026-07-01 22:10] AGREED: Decision adjudication = **interactive for everything** — every recorded decision (plan Decisions Logs, specs, autonomous sessions) comes to the developer via AskUserQuestion, batched 4 at a time, with recommended options.
- [2026-07-01 22:10] AGREED: "100% of the language" = **everything ever mentioned, no cutline** + a **real completion percentage** (done vs full vision) + a **complete PHP↔Phorj gap matrix**: Phorj must cover *all* of PHP (every builtin, every syntax) and do it much better (syntax, performance, safety, security, debugging), plus ideas from all languages/frameworks. No limit on ambition; auditor adds anything critical the developer forgot.
- [2026-07-01 22:10] AGREED: CLAUDE.md = **full rewrite, rules-only**: it contains the rules governing how Claude delivers code (quality, carefulness, invariants, gotchas-as-rules). The language itself (surface, roadmap, milestones, decisions) lives in separate docs files. History relocated, not lost. (Counter-challenge pending: delivery invariants that *mention* language machinery — byte-identity spine, Op-coupling, examples-ship-with-features — stay in CLAUDE.md because they are delivery rules.)

- [2026-07-01 22:20] AGREED: 3C gate runs with defaults (30/8, 3 angles), interactive mode (no autonomous suppression).
- [2026-07-01 22:20] AGREED: CLAUDE.md boundary = "does Claude need this to deliver correct code?" — byte-identity spine, Op-coupling, examples-ship-with-features STAY as delivery rules + pointer block to docs SSOT; language surface/roadmap/history move to docs/.
- [2026-07-01 22:20] AGREED: plan-file deletions are LISTED with evidence this session, executed only after explicit approval.

- [2026-07-02 00:05] AGREED: Plan approved with tweak — add Agent F: cross-language feature scan (features absent from PHP, high ROI / pure-DX-no-tradeoff candidates for Phorj), sourced from TS/Kotlin/Rust/Go/Swift/C#/Elixir and beyond.

## Formal Plan

**Session type:** audit-only — zero code changes. Deliverables are documents.

### Stage 1 — Harvest (6 parallel subagents, ≤5 concurrent, raw → docs/research/full-audit/raw/)
- **A craftsmanship.md** — security / performance / SOLID / error-handling / panic-unwrap paths / test health / dead code over src/, P0–P3 + file:line evidence.
- **B modularity.md** — census of every .rs >500 LOC; cohesion clusters; named target layout per split; byte-identity-gated refactor spec.
- **C decisions.md** — every decision from 65 plans + 81 specs + CLAUDE.md + memory: id, date, decision, source, asked-vs-autonomous, conflicts.
- **D php-surface.md** — pinned PHP 8.5 surface: all syntax + every builtin by extension group (side A, independent).
- **E phorj-surface.md** — Phorj surface from code + ccb2403 language-surface SSOT: keywords, syntax, natives, CLI, diagnostics (side B, independent).
- **F cross-language.md** — high-ROI features PHP lacks, graded by impact/effort/tradeoff, deduped against what Phorj already shipped.

- [2026-07-02 00:20] AGREED (mid-flight): audit perimeter = the WHOLE repo, no human-authored file excepted — src/ + tests/ + examples/ + conformance/showcase surface + docs + CI + tools/scripts + playground + editor extensions. Build outputs (target/) and committed vendored fixtures audited only as presence/policy, not line-by-line. New **Agent G: showcase & adoption-surface audit** added (examples pedagogy, conformance story, onboarding docs, CI quality, benchmark narrative).

- [2026-07-02 01:05] AGREED (mid-flight): add **Agent H: enforcement & soundness audit** — adversarially verify every static guarantee Phorj claims (visibility, static-vs-instance, unknown types, throws discharge, exhaustiveness, totality, invariance, …) by running negative programs through check/run/runvm/transpile; enforcement holes = P0; also inventory enforcement rules Phorj SHOULD have (PHP's unenforced surface) but lacks.

### Stage 2 — Cross
Gap matrix (D×E, both inventories stated before comparison); decision-conflict detection; delta-audit of the two SSOTs (php-parity-and-beyond 2026-06-21, language-surface ccb2403).

### Stage 3 — Adjudicate (interactive)
AskUserQuestion rounds, 4 decisions/round, conflicts + PHP-test failures first. Register rows: status pending|ratified|overruled; resume cursor kept here:
**ADJUDICATION CURSOR: Round 1 CLOSED (I-2/I-3/I-4 ratified). ALL remaining questions are prepared in the QUESTION CATALOG below (Parts II, III, IV — every part now READY, no placeholders). SYNTHESIS COMPLETE: docs/plans/MASTER-PLAN.md is written (1209 lines, 12 sections, 33 ⏳BLOCKED-ON markers keyed to this catalog). On the developer's return: surface Part I as a review block, then Parts II→III→IV, 4 questions per round, examples included.**

- [2026-07-02 01:40] RATIFIED: C-8/DEC-057 revisit — `E-INTERSECT-SIG` relaxed: `A & B` legal when the shared method's signatures are distinguishable under shipped overload-resolution rules; intersection member access resolves like an overloaded call.
- [2026-07-02 01:40] FEEDBACK (standing, all future rounds): every adjudication question MUST ship with concrete real-world code examples demonstrating the risk/problem, so the developer can judge from evidence, not description.
- [2026-07-02 02:00] RATIFIED (with examples reviewed): DEC-056d overturned — **E-MATCH-BARE-TYPE hard error**: bare PascalCase identifier in match-pattern position = compile error with did-you-mean (`V()` variant / `V x` type pattern / lowercase binding). Front-end only.
- [2026-07-02 02:00] RATIFIED (with examples reviewed): C-2/DEC-094 — **execute the original A-6 replacement**: `foreach (coll as x)` / `foreach (m as k => v)` becomes the ONLY collection loop; `for` reverts to C-style only; examples/docs/tests codemodded under the byte-identity gate.
- [2026-07-02 return-round A] RATIFIED: **R2-A generics explicit type arguments, BOTH sites** — `new Box<int>([])` + `firstOr<int>([], 0)` (TS-style lookahead). Unblocks MASTER-PLAN Wave-5 item; closes the empty-container/nested-call KNOWN_ISSUE.
- [2026-07-02 return-round A] RATIFIED: **R2-B UFCS keep + stability policy** — CI leaf-name-uniqueness registry test + documented precedence (real methods beat UFCS). repeat→fill break class becomes impossible.
- [2026-07-02 return-round A] RATIFIED: **R2-Q4 doc-reconciliation BATCHED** — one Wave-0 task; C-1/3/4/5/6/7/10 + DRIFT-01..09 supersession notes.
- [2026-07-02 return-round A] REOPENED at developer request: I-2 / I-3 / I-4 — re-presented with full before/after examples in Round B.
- [2026-07-02 FULL RE-ASK] Developer ordered a from-scratch re-ask of ALL questions with embedded examples/explanations per option. ALL prior in-session ratifications (I-2, I-3, I-4, R2-A, R2-B, R2-Q4) are held PROVISIONAL until reconfirmed in the re-ask sequence. Rounds: R1 core-language (match/foreach/intersect/generics) → R2 (UFCS/doc-batch/bulk-six/new-variants) → R3 (concurrency-disclosure/showcase/cross-language/hooksPath) → R4 (M-numbers/H-rules/P-deletions/Q-CLAUDE.md) → final (master-plan approval).
- [2026-07-02 RE-ASK R1 — all four CONFIRMED with embedded examples]: **E-MATCH-BARE-TYPE hard error** ✓ · **foreach replaces for-in** (E-RETIRED-SYNTAX + phg-fix rewrite) ✓ · **E-INTERSECT-SIG relaxed via overload rules** ✓ · **generics explicit type args, BOTH sites** ✓. These four are now FINAL.
- [2026-07-02 RE-ASK R2]: **doc-reconciliation batch** FINAL ✓ · **bulk-six autonomous items** RATIFIED-FINAL ✓ · **mandatory `new` on enum variants** KEPT-FINAL ✓ (asymmetry now load-bearing for E-MATCH-BARE-TYPE did-you-mean). UFCS: developer counter-proposed TYPE-SCOPED resolution (same leaf name legal across receiver types) — being confirmed with specificity rules + rebind-gotcha challenge in R3.
- [2026-07-02 RE-ASK R3]: **UFCS type-scoped + specificity ranking + CI rebind guard** FINAL ✓ (developer's design, refined — supersedes blanket leaf-uniqueness) · **hooksPath repair AUTHORIZED + executed** ✓ · **showcase all-six ADOPTED** ✓ + developer directive: enforcement-first messaging ("compile-time catches what PHP only finds at runtime") becomes the headline · Concurrency: deep-dive requested — use-case enumeration + parallelism ceiling + proposed GENERAL RULE (no-faithful-PHP-mapping ⇒ surface to developer, default native-only + oracle exclusion; candidate CLAUDE.md rule) — re-asked in R4.
- [2026-07-02 RE-ASK R5]: **completion numbers RATIFIED** ✓ (parity ≈58% / vision ≈60%, recompute-per-milestone rule) · **Unicode-correct-by-default strings ADOPTED** ✓ (DEF-016 fix; bytes stays the explicit byte view; M-text Wave 4) · **gap priority: web spine first AND lifter-unblockers wanted — confirmed Wave 3 then Wave 4 (generators design runs in parallel during W3)** · **3-bucket stdlib charter ADOPTED** ✓ (A→M-Batteries ~72 · B→extension-policy-gated ~55 · C→rejected-with-reasons ~132).
- [2026-07-02 RE-ASK R7-1]: **E-CAPTURE-WRITE hard error + Core.Ref<T> stdlib box** RATIFIED ✓ (instance-handle sharing verified by live probe run≡runvm; error help teaches the Ref pattern).
- [2026-07-02 RE-ASK R7-3]: **enforcement small-wins batch 2 (all ten) ADOPTED + batch-3 linter-catalog sweep COMMISSIONED** ✓ (Agent L: clippy/PHPStan/ESLint/detekt correctness rules, always-wrong-code + checker-only + zero-false-positive filter).
- [2026-07-02 RE-ASK R7-4]: **cross-language ADOPT-NOW all 13 ADOPTED** ✓ (phg fix ships FIRST in Wave 2 — load-bearing for the ratified codemods; format specifiers, derive channel, sealed hierarchies, doc-tests + 8 polish items).
- [2026-07-02 RE-ASK R7-2]: pending refined re-ask — developer wants: manual/EXPLICIT override (not implicit) + ability to add UFCS functions (`1.xzy()`). Clarified: UFCS extension is ALREADY ratified (type-scoped UFCS covers user functions incl. primitive receivers); remaining question = scoped manifest override now vs parked-with-guardrails.
- [2026-07-02 RE-ASK R6]: **E-IMPORT-UNKNOWN hard error** ✓ · **enforcement block (W-CATCH-NEVER-THROWN, W-UNUSED-*, NaN→fault unify, 5 smaller recs) + Wave-4 timing for lifter-unblockers** ✓ · capture-write: error accepted, developer asks for a mutation mechanism (probe VERIFIED: instance-handle capture + `mutable` field works today, run≡runvm) — Ref<T> proposal in R7 · Core-package reservation accepted in principle, developer floats a core-OVERRIDE mechanism — challenged in R7 (supply-chain risk) with extension-via-UFCS counter · developer asks for MORE enforcement small wins (e.g. throws-never-thrown) — enumerated in R7.
- [2026-07-02 RE-ASK R4 — all four ADOPTED]: **THE LADDER RULE → CLAUDE.md** ✓ (1 faithful-PHP→transpile · 2 no-mapping→native-only w/ E-TRANSPILE-<FEATURE> + harness quarantine + disclosure · 3 silent semantic downgrade FORBIDDEN; every no-mapping feature surfaced to developer, never autonomous) · **concurrency PHP leg = hard error E-TRANSPILE-CONCURRENCY + explicit `--sequential-concurrency` opt-in w/ warning** ✓ · **M-Parallel direction = WORKER ISOLATES** ✓ (own heap per worker, channel messaging, no shared state; shared-memory Arc rewrite rejected) · **messaging = CO-HEADLINE: enforcement + the two-way bridge** ✓ ("Your PHP, compiled honest" — lift surfaces latent bugs, transpile keeps the escape hatch).

### Stage 4 — Synthesize
D6 master plan (no-cutline, Opus-executable, reasons everywhere) + completion % (ratified weight model) + D5 plan verdict table (DELETE-evidence/MERGE/KEEP) + D7 CLAUDE.md rules-only rewrite draft + docs split.

### Stage 5 — Gate & close
Phase 6/6C on the documents, four-dimension evidence table, verification guide, learning capture.

**Acceptance:** every deliverable exists with evidence grades; every decision ruled or cursored; gap matrix covers 100% of the pinned PHP surface; master plan names files/order/reasons per item. **Rollback:** git checkout (docs-only).

---

## QUESTION CATALOG — surface ALL of this when the developer returns
> Standing format rule: every design question ships with a concrete current-syntax risk example.
> Recommended option always FIRST. Ask via AskUserQuestion, 4 per round, in the order below.

### Part I — Already RULED this session (present as one review block, not re-asked individually)
| # | Ruling | Status |
|---|---|---|
| I-1 | Session frame: audit-only (docs are the only writes); interactive-for-everything; "100%" = everything-ever-mentioned + reproducible completion % + full PHP gap matrix; CLAUDE.md = rules-only rewrite (delivery invariants stay; language content → docs/); whole-repo perimeter; agents F/G/H added | RULED |
| I-2 | **E-MATCH-BARE-TYPE hard error** — bare PascalCase ident in pattern position = compile error + did-you-mean (`V()` / `V x` / lowercase binding). Example shown: bare `Cash =>` as last arm silently disables exhaustiveness; adding `Crypto(...)` variant later ⇒ crypto fees silently 0 | RATIFIED w/ examples |
| I-3 | **foreach replaces for-in** (execute original A-6): `foreach (xs as x)` / `foreach (m as k => v)` = the ONLY collection loop; `for` = C-style only; repo-wide codemod, byte-identity-gated. Example shown: capability drift (B1 two-binding landed on for-in only), lifter must pick a side, PHP wins one-obvious-way row | RATIFIED w/ examples |
| I-4 | **E-INTERSECT-SIG relaxed** — `A & B` legal when shared-method sigs are distinguishable under shipped overload rules; access resolves like an overloaded call | RATIFIED |

### Part II — Round 2: ASKED, developer AFK, re-present verbatim
**R2-A Generics explicit type arguments** [rec: **adopt both sites** — `new Box<int>([])` + `firstOr<int>([], 0)` w/ TS-style lookahead; alternatives: new-sites-only / keep inference-only]
```phorj
Box<List<int>> b = new Box([]);      // works ONLY via binding annotation
process(firstOr([], defaultItem));   // ✗ nested call: no annotation position exists
var q = new Box([]);                 // ✗ cannot infer T — no syntax to say it
```
**R2-B UFCS stability policy** [rec: **keep breadth + policy** — (1) leaf-name uniqueness across UFCS-eligible natives enforced by CI registry test, (2) documented precedence: real methods beat UFCS; alternatives: marked-natives-only / retire primitive receivers]
```phorj
import Core.Math;  var n = -5;  Output.println("{n.abs()}");   // UFCS: abs(n)
// History: adding a 2nd `repeat` native broke existing x.repeat() sites (E-UFCS-AMBIGUOUS)
// → forced renaming a SHIPPED native (repeat→fill, 5954a2f). Stdlib growth = breaking-change generator.
```
**R2-Q4 Doc-reconciliation batch** [rec: **batch** — C-1 MI spec text · C-3 zero-dep framing · C-4 Text→String rationale · C-5 ternary records · C-6 thread-pool statement · C-7 CLI verb docs · C-10 variant-construction docs → ONE master-plan task adding supersession notes; alternative: walk each]
**R2-C Bulk-ratify six shipped autonomous items** [rec: **ratify all**; alternative: pull out any by name] — totality contours · pattern-cluster syntax sweep (`when`, destructuring, number literals, bitwise, `>>`=2×Gt) · generic-invariance retrofit · COW `Op::SetIndexLocal` model · dogfood grammar patches (empty-list init, comma-throws, nested-quote interp) · `phg debug` REPL/DAP surface

### Part III — Round 3+ queue (ready now)
**R3-1 DEC-133: concurrency permanently outside the PHP oracle** — the ONE standing exception to the 3-leg identity claim (spawn/receive/join tested run≡runvm only; transpiled PHP is sequential-degenerate). [rec: **ratify + require README/spec disclosure** — honesty demands the exception be documented where the byte-identity claim is made; alternatives: demand a PHP-fibers mapping (heavy, M6+), drop concurrency from transpilable surface (loses bridge)]
```phorj
spawn worker(job);        // run/runvm: green thread. Transpiled PHP: executes inline, sequentially.
var r = receive(ch);      // identical OUTPUT only when program doesn't depend on interleaving
```
**R3-2 DEC-083 re-surface: mandatory `new` on ENUM VARIANTS** — you overruled the rec once (thin, mid-marathon); no other language writes `new Some(7)`. Now load-bearing (I-2's did-you-mean says `V()` in patterns vs `new V()` in construction — asymmetry teaches the distinction). [rec: **keep mandatory `new`** — consistency ("all construction looks the same") + the pattern/construction asymmetry is now pedagogically useful; alternative: exempt variants (TS/Rust/Kotlin familiarity), costs a breaking codemod]
```phorj
var p = new Card("1234");          // construction: new required (DEC-083)
match p { Card(l4) => ..., ... }   // pattern: bare call form — the asymmetry IS the signal
```
**R3-3 G-showcase roadmap ratification (6 items, adopt-all recommended):** (1) fix the front door — README hero + quick-starts DON'T COMPILE, 12 files teach removed CLI names; add CI check that `phg check`s every ```phorj fence in docs; (2) promote flagship — surface `conformance/ddd/` + build ~300-line REST micro-service example (Http+Json+decimal+throws+Secret+test); (3) truthful README rewrite (zero-deps claim FALSE since 4 vetted deps; M2.5 "in progress" stale; lift buried); (4) normative spec + clause-tagged conformance corpus (docs/spec/ + spec-tags in the 55 corpus programs + coverage-matrix-as-failing-test); (5) benchmark credibility — current public 3.23× is vs a DEV-BUILT PHP; redo vs release PHP 8.5 + opcache/JIT + methodology doc + CI re-measure; (6) editor surface refresh (grammar missing `empty`/`todo`/`unreachable`/`parent`/`html"`, drop committed .vsix).
**R3-4 F cross-language ADOPT-NOW batch (13 items)** [rec: **adopt all 13 as master-plan items** (11 are front-end-only); walk the top 5 individually if desired]: XL-001 interpolation format specifiers `"{price:.2f}"` (type-checked, → sprintf) · XL-002 closed derive channel `#[derive(Equals, Show, Hash, Ord)]` (sanctioned macro subset; feeds user-typed Map keys + derive(Json)) · XL-003 sealed hierarchies + exhaustive subtype match (nearly free via class_implements) · XL-004 `phg fix` machine-applicable diagnostics (also de-risks OUR breaking codemods: foreach, E-MATCH-BARE-TYPE) · XL-005 doc-tests (examples-ship-with-features applied to API docs) · + 8 more in F report.
**R3-5 hooksPath P0 repair authorization** — `git config core.hooksPath scripts/git-hooks` (one line, restores the void pre-commit gate; audit-only ruling kept me from running it).
**R3-6 B anti-regrowth rule adoption** — soft cap 800 production lines / hard 1000 w/ exemption list / ≤3-word cohesion test / size-gate.sh in CI. [rec: adopt]
**R3-7 tests/differential.rs split (B §8.1)** — 2966 lines → tests/differential/{main,milestones,features,errors_traits,php_oracle,runtime}.rs, gate = test-count parity + full green w/ PHORJ_REQUIRE_PHP=1. [rec: adopt as master-plan Wave-1 step 0]
**R3-8 examples/README.md restructure** — 71KB monolith → thin root index + per-directory READMEs; ~150-line soft cap for guide examples. [rec: adopt]
**R3-9 KNOWN_ISSUES.md prune** (1125 lines, resolved entries linger) + tools/ stale-codemod policy (archive/delete spent one-shots: core_rename.py/core_rename2.py; add "spent" marker convention) + dist/ 60MB pre-rename binaries cleanup. [rec: adopt all three]
**R3-10 E-drift fix batch (9 findings, DRIFT-01…09)** — SSOT/comment corrections (Op::Assert claim, de-reservation inventory missing 9 numeric words, "~166 registry" vs actual 270, stale Channel.new()/E-PKG-TYPE comments, std-only claim vs 4 deps, retired explain codes). [rec: one master-plan doc-hygiene task, merge with R2-Q4 batch]
**R3-11 P2-security batch from A** — DAP/LSP silent write-error handling + Content-Length desync fix · playground.yml unpinned `curl | sh` wasm-pack install · zig tarball no checksum · actions tag-pinned-not-SHA-pinned. [rec: adopt as one hardening task]
**R3-12 VM perf pair from A** — Op::CallMethod 2-String-clone per call (GetField got the S1b inline cache, method dispatch didn't) + Op::CallValue captures-Vec deep-clone per closure invocation (paid per element in map/filter/reduce). [rec: both as M-perf items with before/after `phg benchmark` evidence]

### Part IV — placeholders (fill when agents land, then ask)
- **R4-M (READY — matrix landed 06:25):**
  - **R4-M1 Ratify the completion numbers + weight model**: PHP-parity **≈58%** (domain-weighted: language 79.8% · stdlib 27.5% row / 32.5% usage-weighted · runtime 69.4%; raw row-parity floor 38.8%) and Vision **≈60%** (70% parity + 30% beyond-PHP programme at 64.4%). Model is reproducible (arithmetic shown in M §4); weights are declared judgment calls. [rec: **ratify**, and adopt "re-compute after every milestone" as a standing master-plan rule]
  - **R4-M2 The one INHERITED defect — Unicode-incorrect strings (DEF-016)**: `String.length` counts BYTES [Verified by M in code]; `"héllo".length` ≠ 5. PHP has the same flaw (mb_* split). Your lens: PHP-does-it-wrong ⇒ MANDATORY better. [rec: **adopt Unicode-correct-by-default string ops** (length/slice/upper on scalar values = code points or graphemes — design in M-text), byte semantics explicitly via `bytes`/`Core.Bytes`; breaking, codemod-assisted]
  - **R4-M3 TOP-20 gap prioritization**: [rec: adopt the **web-app enablement spine** as master-plan critical path — #1 database access, #2 HTTP client, #3 sessions/auth, #10 CSPRNG/HMAC (security-critical: safe tokens are IMPOSSIBLE today), #4 sprintf family, #5 filesystem breadth — then #7 named-args/variadics + #8 generators (they also unblock the LIFTER on modern PHP 8 code); remaining 12 sequenced in the plan]
  - **R4-M4 The 3 remaining PARTIALLY-FIXED defects**: DEF-024 value/handle asymmetry (documented-by-design? or unify), DEF-030 URL parsing absent-not-fixed (fold into #2 HTTP work), DEF-037 trait state duplication [rec: DEF-030 fold into HTTP; DEF-024 + DEF-037 = document-as-design with explicit spec wording]
  - **R4-M5 GAP-unplanned bulk (259 FN rows)**: [rec: charter them in the master plan into three buckets — ADOPT-into-M-Batteries (XML/DOM, subprocess, regex-callback, compression: gaps #12–#15), EXTENSION-story (intl/gd/sockets — needs the extension-policy design), REJECT-with-reason (mysqli-procedural aliases, legacy aliases, superglobal mutation APIs)]
- **R4-P (READY)**: plan deletions — **48 DELETE-VERIFIED** (each w/ 2+ fresh code anchors + register/CHANGELOG cross-refs; exact `git rm` command in P report) · 15 MERGE (live content → master plan) · 2 KEEP-AS-RECORD (m-rt-rich-types + overnight-design-forks — cited by PENDING questions R2-A/R2-B, re-verdict after those rulings) · 1 KEEP-ACTIVE. [rec: **approve the 48 deletions** after master plan lands (their live content provably carried); MERGE/KEEP handled automatically]. P also freshly verified: variadics+named-args truly absent; `->` return syntax STILL PARSED (DF-1 open); E-UNIMPORTED absent (DEC-047 unimplemented); Q2 filesystem partial; serve class-static `handle` bridge open; 2 dangling worktree-agent branches (hygiene).
- **R4-H (READY — ~300 probes, 193/197 codes verified enforced, 15-item brag list):**
  - **R4-H0 — NOT questions, mandatory fixes (report to developer, no adjudication needed):** (a) **P0 spine break**: private/protected STATIC FIELD read+write from outside compiles clean and runs on run/runvm but FATALS in transpiled PHP — root cause: checker `statics` map carries no visibility (consts do). Fix = thread visibility through statics, master-plan Wave-1. (b) static method callable via instance `a.m()` (the developer's own named requirement; field analogue already errors — extend same check). (c) VM fault-line divergence: interpolation faults report line 1 + skewed traces on VM only (interpreter correct; FaultKind harness masks it — harness should also compare lines once fixed). (d) dead codes: E-RESERVED-PACKAGE + E-PKG-CASE decl-arm dead on project mode (loader merges before check — move the gate loader-side), E-ALIAS-CYCLE fires uncoded.
  - **R4-H1 — Unknown import silently accepted** [rec: **ERROR E-IMPORT-UNKNOWN** — an import that resolves to nothing is always a typo; PHP's `use` is also silent-until-use, and we must beat PHP]
  - **R4-H2 — Lambda capture-write silently vanishes** (by-value capture: `x = 5` inside a lambda compiles, mutation invisible outside) [rec: **ERROR on assignment to a by-value capture** (Kotlin/Java final-capture model) with explain suggesting an explicit mutable container; alternative: warning only]
  - **R4-H3 — `package Core;` / user Core.* accepted in project mode** (natives win, user code silently dead) [rec: **ERROR, enforce reservation loader-side** — silently-dead code is the worst outcome]
  - **R4-H4 — W-family adoption**: W-CATCH-NEVER-THROWN + W-UNUSED-LOCAL/PARAM/IMPORT (none exist today) [rec: **adopt all four as warnings** riding the existing warning channel; unused-* are the highest-value cheap lints in every modern language]
  - **R4-H5 — Float edge inconsistency**: `sqrt(-1.0)` yields NaN but `x/0.0` faults [rec: **unify on fault** for both (Phorj already chose faults over silent NaN for division; sqrt should match) — alternative: allow NaN everywhere + W-FLOAT-NAN lint]
  - **R4-H6 — remaining 5 missing-rule recs from H's table** (shadowing rules, cyclic-import diagnostics, `==` across unrelated types tightening, self-extend guard, catch-order) [rec: adopt H's per-row recommendation column as a block]
- **R4-Q (READY)**: CLAUDE.md rewrite approval — Q's draft is 118 lines rules-only (13 numbered delivery invariants incl. byte-identity spine/Op-coupling/kernel-single-sourcing/examples-DoD, toolchain+gates, git autonomy, 9-row pointer block to docs SSOTs), relocation map covers all 580 old lines (nothing silently dropped: 7 kept, 28→HISTORY.md, 3 already-owned, 2 obsolete, 1→register), 9 drift fixes applied and itemized (std-only claim, renamed CLI verbs, fn→function, stale counts…). [rec: **approve draft + relocation**, applied as master-plan step 0 alongside the doc-reconciliation batch]
- **R4-final**: master-plan approval; remaining register bulk-ratify by theme (9 tables); the three consolidated open questions from the post-M-DX plan (dynamic-reflection / fs-OOP API / HTTP-client callable pattern) fold into the master plan's stdlib section.
