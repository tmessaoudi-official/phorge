# PHORJ MASTER PLAN — the consolidated roadmap to 100% of the language

> **This is the living plan.** Deliberately undated: it is maintained, not frozen. Produced by the
> 2026-07-01/02 full-audit fleet (session plan `docs/plans/2026-07-01-full-audit-and-master-plan.plan.md`);
> input reports indexed in Appendix C. It supersedes the live content of the 15 MERGE plans
> (P-plan-verdicts §2/§5) — every one of P's **56 live items (LI-A1…LI-H6)** appears below; none dropped.

---

## 0. PREAMBLE

### 0.1 What this plan is and how to execute it

This document is the single roadmap from the current state (v0.5.1-alpha.1, `ccb2403`) to "100% of the
language" — defined by the developer (session Decisions Log, 2026-07-01) as: *everything ever mentioned,
no cutline*, plus a *reproducible completion percentage*, plus *full PHP coverage done much better*.

**Execution protocol for another model (Opus 4.8 or later), no prior session context required:**

1. Read `/stack/projects/phorj/CLAUDE.md` first (after Wave 0 applies the rules-only rewrite, it is
   short) and `docs/INVARIANTS.md`. Those are the delivery rules; this file is the work.
2. Execute waves **top-down** (0 → 6). Within a wave, items are ordered; DEPS lines override order.
3. **Per-item ACCEPTANCE is the definition of done.** Nothing is complete without: the named tests
   green, the full correctness gate green
   (`PHORJ_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORJ_REQUIRE_PHP=1 cargo test --workspace`
   + clippy + fmt + `cargo build --release`), and — for language/stdlib features — a shipped example
   (Governance rule G-5).
4. Items marked **⏳BLOCKED-ON(id)** await a developer ruling (Appendix B maps every id to the
   QUESTION CATALOG in the session plan). Each carries a **default path = the recommended option**;
   the plan stays valid under either outcome — the ⏳ line states the delta if overruled. Do not
   start a ⏳ item's *irreversible* part (codemods, deletions, API surface) before the ruling;
   preparatory work (design docs, tests) may proceed on the default path.
5. Re-compute the percentage ledger (§10) after every wave (recompute rule §0.3).

### 0.2 The two-axis lens (applies to every feature item)

Every feature item is judged on two axes and carries a one-line **LENS** verdict:

- **Axis 1 — quality vs PHP:** `better-than-PHP` (the default ambition) · `PHP-equal` (only where PHP
  is already right) · `MANDATORY-better` (where PHP is wrong — inheriting the flaw is forbidden;
  e.g. Unicode strings, DEF-016).
- **Axis 2 — transpilability:** every runtime feature maps to **idiomatic PHP** (D-L9 / DEC-002);
  PHP-absent features are **compile-time-only + erased** (generics discipline). The one standing
  exception is concurrency (§1.1).

### 0.3 The completion baseline and recompute rule

From the M gap matrix (824 verdict rows: 173 SYN + 631 FN + 20 RT; model in M §4 — arithmetic fully
shown there, weights declared judgment calls):

- **PHP-parity ≈ 58%** (domain-weighted: language 79.8% · stdlib 27.5% row-weighted / 32.5%
  usage-weighted · runtime 69.4%; raw row-parity **floor 38.8%**). Always quote with the weights.
- **Vision ≈ 60%** (70% parity + 30% beyond-PHP programme at 64.4%).
- ⏳BLOCKED-ON(R4-M1) — ratify these numbers + the weight model; **default path: ratified**, and the
  standing rule adopted: **re-run the M §4 arithmetic after every milestone/wave** (re-grade the
  affected Pass-1 rows, recompute the three domain scores, republish both headlines). If overruled:
  keep the raw floor as the only headline.
- Projected per-wave gains: §10 ledger. End-state after Wave 6: **parity ≈ 75% / vision ≈ 81%**, the
  residual being the extension-tier stdlib (§9) and the deliberate GAP-by-design rows (Appendix A).

---

## 1. GOVERNANCE & STANDING RULES

**G-1 · Byte-identity spine.** `phg run` ≡ `phg runvm` ≡ transpiled PHP under a real `php`
(floor **8.5**, `PHORJ_REQUIRE_PHP=1` fails-not-skips), identical stdout AND failure behavior, for
every program and every example; interpreter is the reference oracle. Enforced by
`tests/differential.rs` (globs `examples/**/*.phg`, project-aware) + `tests/conformance.rs` (goldens).

**G-1.1 · The concurrency exception (MUST be disclosed wherever byte-identity is claimed).**
Concurrency (`spawn`/`Channel`/`Task`) is **permanently outside the PHP oracle** (DEC-133):
`run ≡ runvm` holds (deterministic scheduler on both backends); the transpiled-PHP leg is rejected
(`E-CONCURRENCY-NO-PHP`) — sequential-degenerate PHP was rejected as spine-breaking. Additionally,
until W5-13 lands, fault **line numbers** inside interpolation diverge on the VM (H §5) — fault
parity is currently by FaultKind, message and exit code, not line. ⏳BLOCKED-ON(R3-1) — default path:
**ratify DEC-133 + require README/spec disclosure** (Wave 0 doc batch and Wave 6 spec both carry the
disclosure). If overruled toward a PHP-fibers mapping: new M6+ design item, heavy, nothing else here changes.

**G-2 · Quality gate (green means ALL):** `cargo test --workspace` + `cargo clippy --all-targets`
(warnings deny) + `cargo fmt --check` + `cargo build --release`, plus the full PHP-oracle gate before
claiming any feature done. Perf claims need `phg benchmark` before/after; CI regression gate
`scripts/perf-gate.sh`. Pre-commit hook must actually run (Wave 0, W0-1).

**G-3 · Invariants that constrain every HOW below** (detail: `docs/INVARIANTS.md`):
new `Op` ⇒ extend the three coupled matches (`vm::exec_op`, `chunk::validate`,
`compiler::stack_effect`) same commit; **"no new Op" is the default** for front-end features;
value kernels single-sourced in `src/value/`; compile-time sugar expanded out pre-backend via the
`cli::check_and_expand` chokepoint; reified operands thread ALL vm-compile paths
(`check_and_expand_reified` + `compile_with`); CTy-operand trap (any newly-legal expression whose
result can be an arithmetic operand needs a `CTy` resolution + an `expr + 1` differential case);
scratch slots = `self.height - 1` with a two-in-one-expression differential case.

**G-4 · Decision register + question catalog.** Canonical decisions: Agent C register
(`docs/research/full-audit/raw/C-decisions.md`, 141 DEC rows + 10 conflicts + 33 supersessions);
pending rulings: the QUESTION CATALOG in the session plan. **Adjudication protocol for all future
decisions:** interactive, batched 4 per round via AskUserQuestion, recommended option first, and —
standing developer feedback, 2026-07-02 — **every design question ships with a concrete real-world
code example demonstrating the risk**. Ratified rulings baked into this plan: **E-MATCH-BARE-TYPE
hard error** (I-2) · **foreach REPLACES for-in** (I-3) · **E-INTERSECT-SIG relaxed via overload
rules** (I-4) · audit-only session (this plan executes *later*, i.e. now).

**G-5 · Examples-ship-with-features (definition of done).** Every shipped feature lands, in the same
change: a runnable `examples/` program (auto-gated by the differential glob) + `examples/README.md`
index entry. CLI/tooling features: walkthrough README + companion `.phg`. Faults → README capture.
Impure/quarantined features follow the `pure:false` conventions (DEC-156).

**G-6 · Anti-regrowth file-size rule.** ⏳BLOCKED-ON(R3-6) — default path: **adopt** (B §6): soft cap
**800 production lines**/file (inline `#[cfg(test)]` excluded), hard review trigger **1000** with a
tracked exemption list (`vm/exec.rs`, `cli/explain.rs`, `transpile/runtime.rs`), ≤3-word cohesion
test, inline test mods > 150 lines move to sibling `tests.rs`, enforced by `scripts/size-gate.sh` in
CI (W1-6). If overruled: drop W1-6, keep the splits.

**G-7 · Honesty rules.** No silent scope drops: rejected GAP rows live in Appendix A with reasons;
the C-2 lesson (a ratified *replacement* silently softened into an *addition* during an autonomous
slice) is the named failure mode — codemod items below state "REPLACE, not add" explicitly.
Benchmarks: no public perf claim against a dev-built PHP (G P1; fixed in W6-4).

---

## 2. WAVE 0 — REPAIRS & HYGIENE

*Mandatory; no adjudication needed except where ⏳-marked — these restore already-decided guarantees.
Everything here is repo-state repair, docs, or process; zero language-surface change.*

### W0-1 · Restore the local pre-commit gate (hooksPath) — S
- **WHAT:** `git config core.hooksPath scripts/git-hooks` (relative, rename-proof); verify with a
  no-op commit; re-run the full gate over the current tree before the next push.
- **WHY:** A-CI-1 (**P0**): `core.hooksPath` still points at the deleted `/stack/projects/phorge/`
  path — git silently runs **no hooks**; every local commit since the directory rename bypassed
  fmt+clippy+test, and the whole run is unpushed so CI never saw it either. Current tree is green
  (A's baseline 1617/0), so history is the only unknown.
- **HOW:** one config line + a CONTRIBUTING callout ("verify with `git config core.hooksPath`" —
  the documented relative form was correct; the config diverged from it, A §14).
- **ACCEPTANCE:** hook fires on a test commit (observe fmt/clippy/test output); CONTRIBUTING updated.
- **DEPS:** none. First action of the plan. ⏳BLOCKED-ON(R3-5) — default: **approved** (it restores a
  decided guarantee); if withheld, everything else proceeds but every commit must run the gate manually.

### W0-2 · P0 soundness repair: static-field visibility (spine break) — M
- **WHAT:** enforce `private`/`protected` on **static fields** (read AND write, from outside/subclass).
- **WHY:** H §2.1 (**P0**): today the access compiles clean, prints on run/runvm, and **fatals on the
  PHP leg** (`Cannot access private property A::$s`) — a live three-way divergence on the exact
  guarantee the developer named first. Root cause verified: `classes[cls].statics` is `name → Ty`
  with no visibility (consts carry `entry.vis` and enforce).
- **HOW:** store vis alongside the static's type (mirror the consts entry, `checker/collect.rs`);
  gate the read path (`checker/calls.rs` ~1501) and write path (`checker/assign.rs` ~209) with the
  E-CONST-VISIBILITY owner/subclass logic; reuse/extend `E-FIELD-VISIBILITY`. Front-end only — no
  backend, no Op.
- **ACCEPTANCE:** H's probes `v7run/v7b/v7d` become compile errors; new should-error tests in
  `conformance/diagnostics/`; full gate green; `phg explain E-FIELD-VISIBILITY` mentions statics.
- **DEPS:** none.

### W0-3 · Static-method-via-instance becomes an error — S
- **WHAT:** `a.staticMethod()` → compile error (mirror of the already-enforced static-field-via-instance).
- **WHY:** H §2.2: developer's own stated rule ("static not via instance"); currently accepted on all
  three backends (no divergence, but rule-inconsistent with the field case one row up).
- **HOW:** extend the existing instance-member check in `checker/calls.rs` (the field analogue's
  logic); did-you-mean `ClassName.m()`. Note the deliberate inverse: cross-class `Class.method()`
  static-call *ergonomics* are a separate LI-E14 item (W2-11).
- **ACCEPTANCE:** H's `s2` probe errors; should-error test; gate green.
- **DEPS:** none.

### W0-4 · Revive the dead diagnostics: loader-side package gates + E-ALIAS-CYCLE — M
- **WHAT:** (a) `E-RESERVED-PACKAGE` (user `package Core;`) and the `E-PKG-CASE` package-decl arm
  enforced **in the loader, per-file, before the flat merge** (loose + project mode); (b) re-attach
  the `E-ALIAS-CYCLE` code to its diagnostic and resolve the alias graph eagerly (an *unused* cycle
  currently passes clean).
- **WHY:** H §1/§2.3 (**P1**): in project mode a user can declare `package Core.Output;` — accepted,
  then **silently dead** (registry wins); the documented "reserved `Core.` root" guarantee is
  unenforced where it matters. This is also R4-H3 (rec: ERROR, loader-side) — folded here because it
  restores an already-decided rule (DEC-020/022), no adjudication needed. E-ALIAS-CYCLE currently
  fires uncoded (`phg explain` documents a code the compiler never attaches).
- **HOW:** the loader already does per-file validation (E-PKG-PATH, E-FILE-*) — add the two decl
  checks there (`src/loader/`, near the E-PKG-PATH site); alias graph: eager cycle walk at collect
  time (`checker/collect.rs`), attach the code via `Diagnostic::new`.
- **ACCEPTANCE:** H's `projects/{corehijack,reserved,pkgcase}` probes all error; `alias-cycle` probe
  errors *with the code* even when unused; gate green.
- **DEPS:** none.

### W0-5 · VM interpolation fault-line divergence: DISCLOSE now, fix in W5-13 — S (doc)
- **WHAT:** document (KNOWN_ISSUES + the G-1.1 disclosure sites) that fault parity currently excludes
  line numbers for faults raised inside `"{…}"` interpolation on the VM (reports line 1; trace frames
  skewed); extend the differential harness with a **line-comparison assertion behind a feature flag**
  (off until fixed) so the fix has a ready gate.
- **WHY:** H §5 (**P1**): real divergence, masked by design (FaultKind comparison). The *fix* needs VM
  debug symbols (scope IP ranges — LI-C1) and is scheduled W5-13; claiming byte-identity without the
  caveat until then violates G-7.
- **ACCEPTANCE:** KNOWN_ISSUES entry; harness flag exists with a `#[ignore]`d red test demonstrating
  the skew (`r1`/`r6`/`r11` shapes).
- **DEPS:** none. Fix: W5-13.

### W0-6 · Fix the front door: doc snippets compile + CLI rename sweep + CI doc-check — M
- **WHAT:** (a) add `: void` (or proper return types) to every markdown-embedded `main` — README hero
  block, both quick-start one-liners, `examples/cli/README.md` ×7; (b) rename `bench/fmt/disasm/lex`
  → `benchmark/format/disassemble/tokenize` across the **19 stale occurrences / 12 files** (README CLI
  table, CONTRIBUTING, examples/bench/README + workload comments, examples/README, examples/cli,
  editors/*/README ×3, docs/GA-CHECKLIST, docs/MILESTONES); (c) add a **CI doc-snippet check**:
  extract every ```` ```phorj ```` fence from README + all doc READMEs and `phg check` it — the
  examples' discipline applied to the brochure; (d) delete the three spent codemods
  (`tools/core_rename.py` — contains a *known-broken* scanner —, `core_rename2.py`,
  `return_type_codemod.py`; git history preserves them) and remove the dangling
  `tools/wave1_migrate.py` CLAUDE.md reference.
- **WHY:** G P0-1/P0-2: a PHP developer's first 60 seconds is a compile error on the front page;
  every stale command is a copy-paste failure. Codemods: project's own one-shot-migration doctrine +
  A-TOOL-2 + B §9.
- **HOW:** mechanical sweep; the CI check is a small script job in ci.yml (extract fences → temp
  files → `phg check`), fail-closed like the existing gates.
- **ACCEPTANCE:** the CI doc-check job exists and is green; grep for the four dead verbs in docs → 0;
  `tools/` holds no spent codemods.
- **DEPS:** none. (Full README *rewrite* — truthfulness, status table — is W6-3; this item is only
  "nothing on the front page is broken".)

### W0-7 · Doc-reconciliation batch + CLAUDE.md rules-only rewrite — M
- **WHAT:** one batch, three parts. (1) **Conflict-record fixes** (register C-1/3/4/5/6/7/10): amend
  D-L3's MI-reject text with a supersession note (→ DEC-062/064); update the stale zero-dep "LOCKED
  FRAMING" (→ DEC-009 four-dep policy); note the Text→String rationale check (LI-H6: confirm the
  06-18 shadowing concern was consciously dismissed — PascalCase moots it); correct the ternary
  perimeter record (deferred won, C-5); add the W3-thread-pool historical note (C-6); CLI-verb drift
  (C-7, largely done in W0-6); variant-construction guidance (`new V()` construct / `V()` pattern,
  C-10). (2) **DRIFT-01..09 fixes** (E §DRIFT): `Op::Assert` claim → `Op::Fault(FaultMsg::Assert)`;
  de-reservation inventory += the 9 reserved numeric words; "flat two-level import" wording; registry
  count 270 (not ~166); stale `Core.Process.args()`/`parse, lex, disasm, bench`/`Channel.new()`
  comments; retired E-PKG-TYPE comments (2 sites); std-only claims; retired explain codes labeled
  historical (E-PKG-TYPE like E-DECIMAL-DIV); SSOT §3 line refs. (3) **CLAUDE.md rewrite**: apply
  Agent Q Artifact 1 (118-line rules-only file — 13 delivery invariants incl. spine/Op-coupling/
  examples-DoD + toolchain/gates/git-autonomy + 9-row pointer block), create `docs/HISTORY.md` from
  Artifact 3, per the Artifact-2 relocation map (all 580 old lines accounted for; 9 drift fixes
  itemized). Also: superglobal→Request doc map (LI-H4 tail) and stale MILESTONES headers.
- **WHY:** C conflicts table; E DRIFT table; session ruling I-1 (CLAUDE.md = rules-only). Q's
  relocation map proves nothing is silently dropped.
- **ACCEPTANCE:** each C-n row's stale record carries a supersession note; grep-verifiable DRIFT
  fixes; new CLAUDE.md ≤ ~130 lines; `docs/HISTORY.md` exists; MILESTONES headers current.
- **DEPS:** W0-6 (shares the rename sweep). ⏳BLOCKED-ON(R2-Q4 + R3-10 + R4-Q) — default: **batch
  approved as one task** (all three recs are "batch/approve"). If Q4 is overruled to walk-each:
  split part (1) into 7 mini-commits; content identical.

### W0-8 · Plan-file deletions (48) + dangling-citation cleanup — S
- **WHAT:** execute P §6's `git rm` of the 48 DELETE-VERIFIED plan files; then fix the **42 dangling
  plan-path citations** this creates (18 CLAUDE.md — mooted by W0-7's rewrite —, 11 MILESTONES.md,
  13 CHANGELOG.md; precedent exists for 7 previously-deleted plans).
- **WHY:** P §1: every file carries 2+ fresh code anchors + register/CHANGELOG cross-refs; the 15
  MERGE plans' live content is fully carried by this plan (§5 inventory absorbed below) — after this
  plan lands, a second small batch (language-evolution, php-fidelity, review-pass, trackB, big-chunk,
  and the 2 KEEP-AS-RECORD files once R2-A/R2-B close) also becomes deletable.
- **ACCEPTANCE:** `docs/plans/` holds only: this file, the session plan, the MERGE files pending
  their second batch, and future plans; no dangling "docs/plans/2026-…" reference in living docs.
- **DEPS:** this plan committed (it is the carrier). ⏳BLOCKED-ON(R4-P) — default: **approve the 48**.
  If withheld: skip; zero downstream impact (deletions are pure hygiene).

### W0-9 · Repo housekeeping: branches, dist/, KNOWN_ISSUES, examples index — S
- **WHAT:** (a) delete the 2 dangling worktree-agent branches (LI-H3, verified present);
  (b) clean `dist/` stale pre-rename binaries (~60 MB, git-ignored — local `rm`, note in a commit
  message); (c) **KNOWN_ISSUES.md prune** (1125 lines): move resolved entries to CHANGELOG, keep the
  filename honest; (d) **examples/README.md restructure**: 71 KB monolith → thin root index
  (name + one-liner tables) + per-directory READMEs; adopt the ~150-line soft cap for guide examples;
  fix the index gaps (`web/json-api.phg`, `examples/random/`, `process/args-env.phg`,
  `debug/README.md` — A-EX-2/G) and the `decimal` self-contradiction (line 286 vs the index);
  fix stale `recv`/`println` comments in `guide/concurrency.phg`/`guide/secret.phg`.
- **WHY:** P LI-H3; B §9/§10/§11; G Part 1; R3-8/R3-9.
- **ACCEPTANCE:** `git branch` clean; README index lists every shipped example; KNOWN_ISSUES contains
  only live issues.
- **DEPS:** none. ⏳BLOCKED-ON(R3-8, R3-9) — default: **adopt all**; if overruled, drop the
  restructure, keep the gap/contradiction fixes (those are plain errors).

### W0-10 · P2 hardening batch (debug tooling + supply chain) — M
- **WHAT:** (a) DAP write errors: track `dead: bool`, end the session on write failure (A-ERR-3);
  (b) malformed `Content-Length` in DAP/LSP framing → protocol error/close, never silent desync
  (A-ERR-4, two sites); (c) playground.yml: replace the unpinned `curl | sh` wasm-pack install with
  `cargo install wasm-pack --locked --version <pin>` or a checksummed release artifact (A-CI-4);
  (d) zig tarball checksum verification + pin GitHub Actions by commit SHA (A-CI-5); (e) move
  `cargo install cargo-zigbuild` after the rust-cache restore (A-CI-6); (f) `usize::try_from` in the
  container decode (A-SEC-3); (g) commit the VS Code extension `package-lock.json` (A-ED-2);
  (h) add the four missing `unreachable!` justification messages (A-ERR-1); (i) document the
  `W-SECRET` one-hop limitation in its explain text (A-SEC-7).
- **WHY:** A §2/§7 — the complete P2 set plus the trivial P3s worth batching.
- **ACCEPTANCE:** DAP/LSP framing tests for the two error paths (A-TEST-1's `json.rs` unit tests ride
  along); workflows show pinned installs; gate green.
- **DEPS:** none. ⏳BLOCKED-ON(R3-11) — default: **adopt as one hardening task**.

### W0-11 · realworld Core.File example — S
- **WHAT:** one `examples/realworld/` program that reads a committed fixture via `Core.File`
  (composing `??`/if-let), + a per-module example-coverage audit note in the examples index.
- **WHY:** P LI-H5 [Verified: no realworld program reads Core.File]; trackB Task 6 was never carried.
- **ACCEPTANCE:** program byte-identical on run/runvm/PHP (file reads are deterministic on a committed
  fixture — the shipped Core.File discipline); indexed.
- **DEPS:** none.

### W0-12 · PUSH + external renames (developer-gated, listed for completeness) — S
- **WHAT:** (a) **LI-H1**: `git push` of everything since `0d952a8` (M-DX, marathons, Lane 1, perf
  gate, this audit) — *never autonomous* (DEC-174); playground Pages deploy + live re-verify rides
  the push. (b) **LI-H2**: GitHub repo rename phorge→phorj + host directory `mv` (manual, DEC-013
  residue). Sequence (b) is prerequisite-free but the dir `mv` is safe *because* W0-1 set the
  relative hooksPath.
- **ACCEPTANCE:** origin/master current; playground live shows the current examples set.
- **DEPS:** W0-1 (gate restored) strongly recommended first; developer action required.

---

## 3. WAVE 1 — DECOMPOSITION

*B's spec (`raw/B-modularity.md`), executed verbatim: moves-only, zero behavior change, one cluster
move per commit, every commit gated by `cargo build` → clippy → fmt → the full differential with the
PHP oracle (G-1; concurrency exception G-1.1 applies to the harness's quarantined cases). The
do-not-split list (B §3: 24 production files incl. `vm/exec.rs`, `cli/explain.rs`, `checker/mod.rs`,
`ast/*`, `loader/resolve.rs`, `types.rs`…) is honored — do not "improve" on it.*

### W1-1 · `tests/differential.rs` split (step 0 — de-risks everything after) — M
- **WHAT:** 2966 lines → `tests/differential/{main,milestones,features,errors_traits,php_oracle,runtime}.rs`
  (directory-form integration test, same single binary, B §8.1 cluster map).
- **WHY:** largest file in the repo; the correctness spine's own harness must be navigable before the
  spine files move. ⏳BLOCKED-ON(R3-7) — default: **adopt as Wave-1 step 0**.
- **ACCEPTANCE:** **test-count parity** (`cargo test --test differential -- --list | wc -l` identical
  before/after — 126) + full green with `PHORJ_REQUIRE_PHP=1`.

### W1-2 · Mechanical off-spine splits (B Wave 1) — M
Six commits, B §5 order 1–6: `chunk.rs` → `chunk/{mod,tests}` (§1.12) · `cli/mod.rs` +
`preludes.rs`/`disasm.rs` (§1.6 — also fixes the "embedded stdlib in the dispatcher" smell) ·
`serve.rs` → `serve/{mod,tcp,pool}` (§2.3) · `lift/parser.rs` → `lift/parser/{mod,exprs,interp}`
(§1.7) · `lift/lifter.rs` + `lift/exprs.rs` (§2.4) · `fmt/printer.rs` → `fmt/printer/{mod,stmt,expr,atoms}`
(§1.5). ACCEPTANCE per commit: build + gate green.

### W1-3 · Front-end splits (B Wave 2) — M
Six commits, order 7–12: `lexer/mod.rs` + `scan.rs` · `parser/items.rs` + `parser/classes.rs` ·
`checker/program.rs` + `checker/totality.rs` · `checker/expr.rs` + `checker/casts.rs` ·
`checker/calls.rs` + `members.rs`/`ufcs.rs` · `checker/collect.rs` + `inherit.rs`/`signatures.rs`.
ACCEPTANCE: full differential per commit (front-end feeds all backends).

### W1-4 · Spine splits (B Wave 3, strictest gate) — L
Five entries, order 13–17: `value.rs` → `value/{mod,containers,arith,decimal,tests}` (kernels stay
single-sourced via `pub use` re-exports — zero caller churn; update INVARIANTS §3 wording
`src/value.rs`→`src/value/` in the same commit) · `interpreter/mod.rs` + `ops.rs` ·
`compiler/mod.rs` + `compiler/types.rs` (`stack_effect` STAYS in mod.rs) · `compiler/expr.rs` +
`compiler/call.rs` (**scratch-slot discipline: verbatim moves; the two-ops-in-one-expression
differential cases are the gate**) · `transpile/program.rs` → `{program,runtime,functions,classes,traits}.rs`
(one cluster per commit, PHP oracle each; emission *order* inside `emit_program` unchanged).
Post-wave: the §3.2 exhaustiveness smoke test (dummy `Op` variant must fail to compile in all three
coupled sites) + `cargo build --release`.

### W1-5 · Structural-smell backlog (non-move commit class, after the splits) — M
- **WHAT:** decompose the three ~500-line functions B flagged (not text moves — behavior-preserving
  extraction, separate commits): `main.rs::main` (511 lines → per-subcommand `cli::` parsers, also
  A-DES-1) · `checker/collect.rs::check_interface_graph` (~480) · `compiler/program.rs::compile_program_with`
  (~667). Plus A-DES-4: enable `clippy::cognitive_complexity` at a generous threshold as a ratchet.
- **ACCEPTANCE:** each extraction gated by the full differential; no public-surface change.

### W1-6 · size-gate CI + standing rule write-back — S
- **WHAT:** `scripts/size-gate.sh` (warn > 800, fail > 1000 unless exempted) wired into ci.yml;
  the G-6 rule text added to INVARIANTS/CONTRIBUTING.
- **DEPS:** W1-1..W1-4 (numbers calibrated to the post-split census). ⏳BLOCKED-ON(R3-6), default: adopt.

### W1-7 · Clarity workstream (LI-F9) — M
- **WHAT:** ARCHITECTURE.md narrated rewrite; module-level `//!` docs across `src/`; **blanket
  `clippy::pedantic` fix-ALL** (developer overrode selective — DEC-176, ASKED).
- **WHY:** post-dogfood W5 (P LI-F9); sequenced after the splits so the churn lands on final layout.
- **ACCEPTANCE:** pedantic clean workspace-wide (or a tracked allow-list each with a one-line reason);
  gate green.

---

## 4. WAVE 2 — RATIFIED LANGUAGE CHANGES + ENFORCEMENT COMPLETION

*The breaking/codemod wave. W2-1 lands FIRST because it converts every later codemod from a hand-rolled
Python risk into a compiler-driven command (F's cluster observation: fix + auto-import + deprecation
codemod de-risk our own breaking changes).*

### W2-1 · `phg fix` — machine-applicable diagnostics (XL-004) — M
- **LENS:** beyond-PHP tooling (no runtime surface; n/a to transpile).
- **WHAT:** diagnostics carry structured `(span, replacement)` edits; `phg fix` applies them
  (check-only default, `--apply` writes); LSP code-actions ride the same data.
- **WHY:** F XL-004 (ADOPT-NOW, DX-pure-win): spans + did-you-mean already computed (S0/M-DX S1) and
  currently die as prose; this is the delivery vehicle for W2-2/W2-3/W2-4 and every future rename.
  ⏳BLOCKED-ON(R3-4 covers the batch) — default: adopt.
- **HOW:** extend `Diagnostic` with an optional `fix: Vec<(Span, String)>`; populate the
  one-candidate cases first (unknown name w/ one suggestion, unused import once W2-8 lands, `->`→`:`,
  bare-type-in-pattern); `src/cli/` new `cmd_fix`; output must be `phg format`-stable.
- **ACCEPTANCE:** `phg fix --check` lists edits for a fixture program; `--apply` result passes
  `phg check` + `phg format --check`; unit tests on span-application (overlap/ordering); example:
  walkthrough README under `examples/cli/`.
- **DEPS:** none (Wave 1 layout settled helps).

### W2-2 · E-MATCH-BARE-TYPE hard error (RATIFIED I-2) — M
- **LENS:** MANDATORY-better (PHP has no exhaustive match; Phorj's own footgun — a bare `Cash =>`
  arm silently disables exhaustiveness). Front-end only; no transpile change.
- **WHAT:** a bare PascalCase identifier in match-pattern position = compile error with did-you-mean
  (`V()` variant / `V x` type pattern / lowercase binding).
- **WHY:** session ruling 2026-07-02 (overturns DEC-056d's deliberate footgun preservation);
  C register AUTONOMOUS-HIGH-IMPACT #1.
- **HOW:** checker pattern pass (`checker/` match arm handling); new code + `phg explain`; `phg fix`
  suggestion for each of the three interpretations; **repo codemod** of any legitimate bare-binding
  arms to lowercase (expected rare — PascalCase bindings already fight E-NAME-CASE).
- **ACCEPTANCE:** should-error conformance programs for all three did-you-mean shapes; every example/
  test still green (byte-identical — the codemod may not change behavior); explain entry.
- **DEPS:** W2-1 (fix suggestions).

### W2-3 · foreach REPLACES for-in — repo codemod (RATIFIED I-3) — L
- **LENS:** PHP-equal-where-right (foreach is PHP's good loop) + better (typed bindings, no by-ref
  pitfall). Transpiles to PHP `foreach` — fully idiomatic.
- **WHAT:** `foreach (coll as x)` / `foreach (m as k => v)` becomes the **ONLY** collection loop;
  `for` reverts to C-style only. **REPLACE, not add** (the C-2 drift is the named failure mode, G-7).
  Includes LI-E12: the A-6 follow-up binding forms (key/value destructure variants) and carrying B1's
  two-binding string/Map iteration onto foreach.
- **WHY:** session ruling 2026-07-02; C-2 conflict resolution (capability drift already happened once:
  B1 landed on for-in only).
- **HOW:** parser: remove `for (T x in xs)` (clean parse error w/ did-you-mean foreach); ensure
  foreach covers the full for-in surface first (list/range/string/Map two-binding, `with int i`
  counter); codemod all examples/tests/docs (via W2-1 where possible; `tools/` one-shot otherwise,
  deleted after per doctrine); FEATURES.md/STABILITY.md updated; lifter (`src/lift/`) maps PHP
  foreach → Phorj foreach 1:1 (one fewer decision).
- **ACCEPTANCE:** byte-identity gate across the entire migrated corpus (the codemod is
  output-preserving by construction); `for … in` → parse error test; guide example updated;
  conformance corpus updated.
- **DEPS:** W2-1. **Do not soften into an alias/deprecation period without a new ruling.**

### W2-4 · `->` return-syntax retirement (DF-1) — M
- **LENS:** consistency repair (DEC-093 decided `: T`; `->` still parsed = two spellings).
- **WHAT:** remove the `TokenKind::Arrow` return-type alias from the parser [P verified: still eaten,
  `src/parser/types.rs:109`]; `phg fix`/`phg format` normalization `->` → `:`.
- **WHY:** P LI-E11; php-fidelity A-1 parked for exactly this tooling; DEC-093 (ASKED) says retired.
- **HOW:** parse error with fix suggestion; sweep any residual `->` in docs/examples (grep suggests
  none in code — verify); also **verify `W-SEQUENCE-MUTATION` shipped** (DEC-096 sweetener, register
  flags status unverified) — if absent, implement here (a lint on multiple `++`/`--` of one variable
  in one expression).
- **ACCEPTANCE:** `-> T` in a signature = parse error + machine fix; W-SEQUENCE-MUTATION either
  verified-present (test exists) or shipped.
- **DEPS:** W2-1.

### W2-5 · E-INTERSECT-SIG relaxed via overload rules (RATIFIED I-4) — M
- **LENS:** better-than-PHP (PHP 8.1 `A&B` never checks member coherence at all). Transpiles
  unchanged (PHP `A&B`).
- **WHAT:** `A & B` legal when the shared method's signatures are **distinguishable under the shipped
  overload-resolution rules**; intersection member access resolves like an overloaded call.
- **WHY:** session ruling (closes register C-8 — the "revisit when overloading lands" that never
  happened); DEC-057 D2 superseded.
- **HOW:** `checker/` intersection member access (the S5 `check_method_call`/`check_member`
  intersection arms) routes through the overload-resolution machinery (`checker/overloads.rs` +
  dispatch); `E-INTERSECT-SIG` only fires when the sigs are overload-*indistinguishable*; update the
  S5 spec with a supersession note.
- **ACCEPTANCE:** positive conformance program (two interfaces sharing an overload-distinguishable
  method, member access through the intersection, byte-identical incl. PHP); negative test for the
  still-illegal indistinguishable case; explain updated.
- **DEPS:** none.

### W2-6 · DEC-047 no-wind implementation (LI-E18) — L
- **LENS:** beyond-PHP explicitness (PHP has ambient everything); compile-time-only (imports erase).
- **WHAT:** the designed-not-implemented no-wind closure: fault intrinsics (`panic`/`todo`/
  `unreachable`/`assert`) move behind mandatory `import Core;` as `Core.assert(...)` etc.
  (**E-UNIMPORTED**, verified absent today); deep imports `import Core.A.B.C` at any depth binding
  bare leaf AND parent-qualified; import aliasing extended to stdlib + deep; de-reserve
  `Attr`→Core.Html, `Error`→Core.Error, `Channel`/`Task`→**`Core.Async`** (E DRIFT-02: the
  de-reservation inventory must also account for the 9 reserved numeric words `double`,`i8`–`u64`).
- **WHY:** DEC-047 (ASKED, 📐); C-9 conflict (the principle was violated for weeks); design SSOT
  `docs/specs/2026-07-01-no-wind-namespace-and-language-surface-design.md`.
- **HOW:** per the spec; loader/checker import maps + registry; **breaking codemod** for intrinsic
  call sites (via W2-1); transpiler unaffected (imports never reach PHP).
- **ACCEPTANCE:** bare `panic(...)` without `import Core;` = E-UNIMPORTED with fix; deep-import +
  alias conformance programs; full corpus migrated byte-identically; DEC-049's keyword-vs-import
  3-way rule restated in the spec/INVARIANTS.
- **DEPS:** W2-1 (codemod); pairs with W5-9 auto-import (XL-009) — the tooling half that makes
  mandatory explicitness humane; acceptable to ship W2-6 first, W5-9 should follow soon after.

### W2-7 · Import-roots PSR-4 `[packages]` map (LI-E17) — M
- **LENS:** better-than-Composer (typed, checked, offline); build-system only.
- **WHAT:** optional `[packages]` map in `phorj.toml` (PSR-4-style root mapping), default root `src/`
  folder=path, `vendor:` prefix for deps, migration codemod.
- **WHY:** P LI-E17; DEC-048 (ASKED, spec committed `8fc85f2`).
- **HOW:** per spec: `manifest.rs` + `loader/` resolution; checker/transpiler unaffected (loader
  mangles pre-backend, DEC-030).
- **ACCEPTANCE:** project fixture with a mapped root loads + byte-identical; `tests/project.rs` cases;
  spec examples runnable.
- **DEPS:** W2-6 (import machinery churn — same files, sequence to avoid conflicts).

### W2-8 · Enforcement adoptions from H (R4-H1/H2/H4/H5/H6) — M
⏳BLOCKED-ON(R4-H1..H6) — defaults = H's recommendation column; each independent (drop any overruled
one without affecting the rest):
- **H1 · E-IMPORT-UNKNOWN** (default: ERROR): an import resolving to nothing (loose, project, vendored)
  errors at the import line — Go model, beats PHP's silent `use`. Needs a single-sourced known-module
  set (native registry + loader symbol tables); this also subsumes the deferred `W-UNKNOWN-IMPORT`
  lint (dogfood W12 — LI-E11 tail, dedupe noted).
- **H2 · assignment to a by-value capture = ERROR** (default: ERROR, Kotlin/Java final-capture model):
  today `x = 5` inside a lambda compiles and the write silently vanishes (H M1 probe) — the worst
  kind of footgun because Phorj's capture is implicit. Explain suggests an explicit mutable container.
- **H4 · W-family**: `W-CATCH-NEVER-THROWN` + `W-UNUSED-LOCAL`/`W-UNUSED-PARAM`/`W-UNUSED-IMPORT`
  (none exist today; the highest-value cheap lints in every modern language), riding the existing
  warning channel; plus the DX-tier shadowing warning (H M4).
- **H5 · float NaN story unified** (default: `Math.sqrt(-1.0)` **faults**, matching the shipped
  `/0.0` fault and the decimal precedent — Phorj already chose faults over silent NaN; alternative
  per ruling: `float?` return or NaN-everywhere + lint).
- **H6 · remaining rule recs as a block**: property-hook syntactic self-reference warning (H M6);
  cyclic-import diagnostics; `==`-across-unrelated-types already enforced (record as brag); catch-order/
  dead-catch diagnostics; the "method references aren't values" dedicated message (H P3 — superseded
  by W3-9 when method refs land; until then, fix the message).
- **ACCEPTANCE:** one should-error/should-warn conformance program per adopted rule; explain entries;
  warnings never gate (DEC-081).
- **DEPS:** W2-1 (unused-import feeds `phg fix`).

### W2-9 · Naming-overhaul remainder (task #25 deltas) — M
- **WHAT:** re-verify the naming SSOT (`docs/specs/2026-06-30-naming-overhaul-design.md`) against the
  tree and execute what remains (P verified Lane 1 done for Path renames/Random.nextFloat/doc drift;
  the spec's full native/package/CLI matrix needs a fresh Phase-0 diff — memory flags "fresh context,
  substring/PHP-target care").
- **WHY:** P LI-E11-adjacent; DEC-113 (ASKED, exhaustive review); memory naming-overhaul-decisions.
- **HOW:** diff spec ↔ registry/CLI/docs; each rename via W2-1-generated edits where possible;
  byte-identity-gated (renames are output-preserving unless a native's *output* embeds a name — check
  `phg explain`/help text separately).
- **ACCEPTANCE:** spec marked EXECUTED with a verification table; zero grep hits for retired names in
  code + living docs.
- **DEPS:** W2-1.

### W2-10 · Narrow soundness-hole batch (LI-E14) — M
- **WHAT:** the KNOWN_ISSUES-tracked corner batch: static-init constructing a parent's `protected`
  ctor (init-expr scan missing); interface-method `throws` not discharged through an interface-typed
  receiver; method-`?` propagation (`x.m()?`); nested un-inferred generic placeholder conservatively
  rejected (improve message or infer); private *static* field via `ClassName.field` + intersection-
  typed-receiver visibility corners (W0-2 covers the main hole; these are the named residuals);
  MI lowering corners (transitive-jump, multi-of-multi, bare `parent.constructor()`, overloaded
  parent methods).
- **WHY:** P LI-E14 (recorded only in the deleted plans — this is their carrier).
- **ACCEPTANCE:** each corner gets a differential/diagnostics test; KNOWN_ISSUES entries closed.
- **DEPS:** W0-2.

### W2-11 · Static-call ergonomics + field-base index-assign (LI-E14 tail + LI-E15) — M
- **WHAT:** (a) cross-class `Class.method()` static calls (own-class-only today) + transpiler
  `static function` emission; (b) **`this.f[i] = e` / `obj.f[i] = e`** field-base index assignment
  (extends `84622c2`'s `Op::SetIndexLocal` model; today `E-ASSIGN-TARGET`).
- **WHY:** P LI-E15 (the key unblock for the in-place benchmark ports, W6-4); LI-E14 static tail.
- **HOW:** (b) follows the COW in-place discipline (memory cow-index-assign-inplace: `make_mut` the
  container IN its slot at refcount 1) — likely one new Op variant for the field-base path (extend
  the three coupled matches, G-3) or a compose of GetField+SetIndexLocal+SetField if provably
  equivalent; interpreter mirrors via the shared kernel.
- **ACCEPTANCE:** differential cases incl. aliasing shapes (shared instance, two handles); `phg
  benchmark` before/after on an index-assign-heavy workload; by-ref params question stays open
  (Appendix B) — do NOT add by-ref here.
- **DEPS:** W1-4 (compiler files settled).

### W2-12 · Erased-generic result as VM operand (LI-E20) — M
- **WHAT:** close the standing run↔runvm surface gap: `id(7) + 1` / `box.get() + 1` runs on the
  interpreter but the VM rejects it (erased result = `CTy::Other`, not a specialized operand).
- **WHY:** P LI-E20 / GA-roadmap M10 residue; the documented CTy-operand trap instance (KNOWN_ISSUES).
- **HOW:** either (i) compiler `ctype` consults the checker's *reified* result type via the
  reified-operand side-table (`d210c62` precedent — memory reified-operands-must-thread-all-vm-compile-paths),
  or (ii) VM gains a dynamic-operand arithmetic fallback for `CTy::Other` that calls the shared value
  kernels (no specialization, still correct). Option (ii) is the smaller, spine-safe default: the
  kernels are single-sourced so semantics cannot fork.
- **ACCEPTANCE:** the exact KNOWN_ISSUES repro becomes a differential case (`expr + 1` per G-3);
  entry removed.
- **DEPS:** W1-4.

### W2-13 · Enforcement audit → should-error conformance suite (LI-E16) — M
- **WHAT:** systematize H's ~300 probes: every language rule gets a committed should-error/should-warn
  program in `conformance/diagnostics/` (H's probe corpus is the seed — it lives in scratchpad and
  must be re-generated/committed); a ratchet test asserts every `E-`/`W-` code in the explain
  registry has ≥1 trigger program (H found 2 CLI-dead + 1 uncoded by exactly this method).
- **WHY:** post-dogfood W1 (P LI-E16); feeds the Wave-6 spec's clause-tagged corpus directly.
- **ACCEPTANCE:** the ratchet test exists and is green; adding an untriggerable code fails CI.
- **DEPS:** W0-4 (dead codes revived first), W2-8 (new rules included).

### W2-14 · `new` on enum variants — decision checkpoint — S
- ⏳BLOCKED-ON(R3-2) — **default path: KEEP mandatory `new`** (DEC-083): consistency ("all
  construction looks the same") and the construction/pattern asymmetry (`new V()` vs `V()`) is now
  pedagogically load-bearing for W2-2's did-you-mean. **If overruled** (exempt variants for
  TS/Rust/Kotlin familiarity): breaking codemod via W2-1, byte-identity-gated, plus doc sweep — a
  self-contained M-size follow-up; nothing else in this plan depends on the outcome.

---

## 5. WAVE 3 — WEB-APP ENABLEMENT SPINE

*M TOP-20's critical path: the gaps that block essentially every real application. Stdlib items follow
the M4 charter (naming/shape conventions, subject-first args, `T?` returns, determinism tier declared
per module — DEC-007 case-by-case Tier A/B admission). Tier-B (impure) modules are quarantined from
the byte-identity example set per the shipped `pure:false` conventions; their PHP mappings still ship
(the native `php` closure) so transpiled programs work — the differential simply doesn't drive them
with live IO. Each item: design-doc-first where marked DESIGN.*

### W3-1 · Database access (M gap #1 — all 10 FN-DB rows) — XL · DESIGN-NEEDED
- **LENS:** better-than-PHP (typed rows, no PDO string soup, injection-resistant by construction);
  transpiles to idiomatic PDO.
- **WHAT:** `Core.Sql` (typed query builder, Tier A — pure value construction) + `Core.Db` execution
  (Tier B — connections; start with SQLite (embeddable, deterministic fixtures) then Postgres per
  ROADMAP M6). Parameterized-only execution (no string-concat query API — the injection class removed
  at the type level); typed row mapping via generics/`derive(Json)`-style decode (W5-2 synergy).
- **WHY:** M TOP-20 #1: "blocks essentially every real app"; P LI-D8 (Sql builder + DB execution,
  Tier B, per-slice decision D-Db recorded there).
- **HOW:** design spec first (connection model on green threads; single-threaded constraint G-1.1;
  the `Transport`-style quarantine seam; vendored driver policy vs DEC-009 — a pure-Rust SQLite or
  postgres protocol impl are the candidates; new dep = developer authorization per policy).
- **ACCEPTANCE:** builder is byte-identity-gated (pure); execution fixture-tested (local SQLite file /
  local postgres in CI); flagship example (W6-2) consumes it; PHP mapping documented (PDO).
- **DEPS:** design ruling on the driver dep; W3-4 (CSPRNG for anything auth-adjacent).

### W3-2 · HTTP client (M gap #2 — all 13 FN-CURL rows; folds DEF-030/URL) — XL · DESIGN-NEEDED
- **LENS:** better-than-PHP (typed Request/Response reuse, no curl_setopt soup); Tier B; PHP mapping
  = curl/streams idiom.
- **WHAT:** **M-HTTP-Client** (P LI-D6 / four-lane Q3): Guzzle-style typed client reusing the shipped
  `Request`/`Response` values, middleware closures, pooling on green threads, HTTPS via a rustls
  feature-fork (dep authorization required, DEC-009), Transport-quarantined. **Folds LI-D7's URL
  breadth + M gap DEF-030:** `Core.Url` gains a spec-compliant URL *parser* + `http_build_query`
  equivalent (landing spec-compliant leapfrogs PHP's non-conformant `parse_url` — the G-url plan).
- **WHY:** M TOP-20 #2 ("second-most-universal capability"); the audited post-M-DX developer question
  **Q3 (HTTP-client callable pattern)** folds here — resolve the callable/middleware shape in the
  design spec (recorded challenge in the four-lane plan).
- **ACCEPTANCE:** URL parser is Tier A (pure, byte-identity + WHATWG/RFC test vectors); client
  fixture-tested against a local `phg serve` instance (loopback only, A-TEST-6 discipline); example
  under `examples/web/` with the `pure:false` convention.
- **DEPS:** design spec (callable pattern ruling folded); rustls dep authorization.

### W3-3 · Sessions / cookies / auth (M gap #3 — 10 FN-SESS rows) — L · DESIGN-NEEDED
- **LENS:** MANDATORY-better (PHP sessions are ambient superglobal state — DEC-092 forbids ambient;
  Phorj's must be value-level). Transpiles to PHP session/cookie idiom at the front-controller bridge.
- **WHAT:** K-auth-csrf-session (parity SSOT §3 M6-deferred): typed `Cookie`/`Session` values on
  `Request`/`Response` (no `$_SESSION` — explicit store), signed/encrypted session payloads (needs
  W3-4 HMAC), CSRF token helpers.
- **WHY:** M TOP-20 #3: every stateful web app.
- **ACCEPTANCE:** pure parts (cookie parse/serialize, token verify) byte-identity-gated; store
  interface fixture-tested; example: login flow in the flagship (W6-2).
- **DEPS:** W3-4 (crypto primitives), W3-6 (serve bridge).

### W3-4 · CSPRNG + timing-safe compare + HMAC/KDF (M gap #10, security-critical) — M
- **LENS:** MANDATORY-better (PHP got random_int right in 7.0; Phorj currently *cannot* generate a
  safe token at all — worse than PHP today, forbidden state).
- **WHAT:** `Core.Random.secureInt/secureBytes` (CSPRNG engine, Tier B — OS entropy, quarantined;
  the deterministic seeded PRNG stays the default/test seam, DEC-150 untouched); `Core.Hash`:
  `hmac`, `equals` (timing-safe), `hkdf`, `pbkdf2` (M FN-HASH GP rows); hash breadth (sha512 etc.
  as needed by HMAC).
- **WHY:** M TOP-20 #10: "tokens/nonces can't be generated safely today"; G-crypto (§3 M8).
- **HOW:** getrandom-style OS entropy (std-only via `/dev/urandom`/getrandom syscall — check DEC-009
  before any dep); HMAC/HKDF over the existing in-tree SHA-256 (bundle sha256.rs precedent); PHP
  mappings: `random_bytes`/`hash_hmac`/`hash_equals` (idiomatic, exact).
- **ACCEPTANCE:** HMAC/HKDF test vectors (RFC) as unit tests + byte-identity for the pure digests;
  secure engine quarantined with a statistical smoke test only; explain/W-SECRET interplay documented.
- **DEPS:** none. **Do first in this wave** (W3-1/2/3 all want it).

### W3-5 · sprintf family / `Core.Fmt` (M gap #4, LI-D3) — M · DESIGN (fork recorded)
- **LENS:** better-than-PHP (type-checked formats — a `%d` on a string is a compile error via W5-1's
  literal validation); transpiles to `sprintf` (exact idiom).
- **WHAT:** `Core.Fmt.format(spec, args…)` covering the sprintf surface (7 FN-STR rows: width,
  precision, padding, hex/oct, thousands). The recorded open fork (variadic-vs-list args,
  `%`-vs-`{}` spec syntax — mega C3 / big-marathon) is resolved in the design doc; **recommendation:**
  `{}`-style specs shared with W5-1's interpolation format specifiers (ONE closed spec grammar, two
  entry points), list-args until W4-1 variadics land, then variadic overload.
- **WHY:** M TOP-20 #4 ("ubiquitous in ported code"); P LI-D3; Lane 4 W3.
- **ACCEPTANCE:** byte-identity incl. PHP against `sprintf` outputs for the full spec matrix;
  guide example; lifter maps `sprintf` → `Fmt.format` (Tier-2 rule, W4-7).
- **DEPS:** design doc; W5-1 shares the grammar (build the grammar module here, W5-1 reuses).

### W3-6 · Filesystem breadth + serve static-handle bridge (M gap #5; LI-D5; LI-E13 part) — L
- **LENS:** better-than-PHP (typed `T?` returns, no resource|false chains — DEF-040 discipline);
  PHP mapping = the fs function family.
- **WHAT:** (a) `Core.Directory` (mkdir/listDir/isDir/metadata/glob/tempdir — the Q2 remainder;
  append/delete/rename/copy/size shipped `a23ca00`); stat/perms subset per charter ruling; the
  **fs-OOP-API developer question** (audited, four-lane plan) resolves here — recommendation:
  keep the static-module shape (`Core.File`/`Core.Directory` statics) over handle objects until
  streams demand handles (XL-019 noted for later). Stream *handles* (the 16-row fopen family) are
  **deferred to the charter** (§9) pending the resource-model design. (b) **serve class-static
  `handle` bridge** [P verified open: `has_fn` matches only top-level `Item::Function`,
  `src/cli/mod.rs:654/674`] — a `static handle` method must be resolvable as the web entry
  (DEC-103 already allows class entry points for `main`).
- **WHY:** M TOP-20 #5; P LI-D5 + LI-E13; the fs-OOP question is one of the three folded dev questions.
- **ACCEPTANCE:** each fs native fixture-tested (committed fixtures where deterministic; Tier-B
  convention otherwise); serve bridge: integration test in `tests/serve.rs` with a class-static
  handler; examples.
- **DEPS:** charter ruling on stat/perms scope.

### W3-7 · Structured logging (M gap #18; LI-D7 W6) — M
- **LENS:** better-than-PHP (typed levels/records vs error_log strings); PHP mapping = error_log/PSR-3 shape.
- **WHAT:** `Core.Log` — record construction pure (Tier A), emission Tier B (stderr/file sink);
  `phg serve` structured request logging (Q-serve-reqlog).
- **WHY:** M TOP-20 #18: "production apps need a log seam before serve is production-usable".
- **ACCEPTANCE:** record formatting byte-identity-gated; sink fixture-tested; serve emits request
  lines under a flag; example.
- **DEPS:** none.

### W3-8 · Json encode + safe-parse hardening (LI-D1) + rich HTTP responses (LI-D8 part) — M
- **WHAT:** `Core.Json.encode` for user types (pre-derive: explicit builders; full `derive(Json)` is
  W5-2's wave 2); safe-parse hardening audit (depth/size limits, malformed-input fuzzing); rich
  response constructors (JsonResponse/RedirectResponse/HtmlResponse — partial vs shipped
  `Response.text`).
- **WHY:** P LI-D1 (Lane 4 W1) + LI-D8 rich-response row; the web spine needs JSON APIs end-to-end.
- **ACCEPTANCE:** encode/decode round-trip property tests; limits produce clean faults byte-identically;
  `web/json-api.phg` upgraded and indexed.
- **DEPS:** none (W5-2 extends).

### W3-9 · Method references as values (DEC-107; the dynamic-reflection question) — M
- **LENS:** better-than-PHP (typed closures vs `$obj->$m()` string dispatch — the un-typeable
  primitive stays rejected); transpiles to PHP first-class callables.
- **WHAT:** `obj.method` → typed closure value + the typed-registry guide pattern; fixes the
  misleading "type A has no field m" diagnostic (H P3) as a side effect.
- **WHY:** the third folded developer question (dynamic reflection → resolved as method-refs,
  DEC-107 ASKED 📐); P LI-E13.
- **HOW:** checker: member access resolving to a method yields `Ty::Function` (zero-capture closure
  binding the receiver); VM: `MakeClosure` over a synthesized thunk or a receiver-capturing closure
  (design chooses; prefer no new Op via synthesized lambda in the front-end, G-3 default);
  PHP: `$obj->method(...)` first-class callable syntax.
- **ACCEPTANCE:** differential cases (pass a method ref to `List.map`; store in a Map registry);
  guide example (`examples/guide/method-refs.phg`); the H P3 diagnostic replaced.
- **DEPS:** none.

---

## 6. WAVE 4 — MIGRATION-BRIDGE COMPLETION

*The features that make `phg lift` viable on modern PHP 8 codebases and close the muscle-memory gaps.
All front-end unless noted; every item byte-identity-gated (concurrency exception G-1.1 n/a here).*

### W4-1 · Named arguments + variadics + spread (M gap #7; LI-E10) — L · DESIGN-NEEDED
- **LENS:** PHP-equal-where-right (8.0 named args are good) + better (compile-checked names; DEF-034
  lesson pre-empted: **decide the param-renaming policy up front** — params become API).
- **WHAT:** `f(x: 1, y: 2)` named args (checker reorders at compile time — no runtime cost, erases to
  positional PHP or PHP named args); `int... xs` variadics (List-typed rest param); `...xs` spread at
  call sites. [P verified all absent.]
- **WHY:** M TOP-20 #7 — "modern PHP idiom; also blocks the lifter on 8.0+ code"; A-named-args/
  A-variadics (SSOT §3, prior-adopted).
- **HOW:** design doc (interaction with defaults/overloading/UFCS is the hard part — overload
  resolution must incorporate named-arg sets); parser + checker call-fill (extend the DEC-101
  call-rewrite-map technique, memory default-parameters-and-fill-technique); likely **no new Op**
  (front-end reordering).
- **ACCEPTANCE:** differential matrix (named×default×overload×variadic); W3-5 Fmt gains its variadic
  overload; lifter Tier-2 consumes both (W4-7); guide example.
- **DEPS:** design; before W4-7.

### W4-2 · Generators / `yield` + iterator protocol (M gap #8; LI-A1; marathon A2) — XL · DESIGN-NEEDED
- **LENS:** PHP-equal (PHP generators are genuinely good) + better (typed `Sequence<T>`); PHP mapping
  = native generators. The prior lazy-seq transpile-divergence reject was superseded by the coroutine
  engine (F XL-040 note; A2 is the named next marathon step).
- **WHAT:** `yield` in functions → lazy `Sequence<T>`; for/foreach consumption; the Iterable protocol
  (A-iterators, SPL Traversable interop story) — user types opt into foreach (ties to W5-11 protocols).
- **HOW:** the corosensei green-thread substrate is the engine (both backends); determinism policy:
  generator scheduling is fully deterministic (single consumer pull) so it stays **inside** the PHP
  oracle (unlike spawn/channels) — the design doc must prove byte-identity vs PHP generators or
  explicitly extend the G-1.1 exception (default: prove it; PHP generators are also deterministic).
- **ACCEPTANCE:** differential + PHP oracle over generator programs (or a ruled exception extension);
  laziness observable-behavior tests (side-effect ordering); guide example; lifter maps PHP
  generators (W4-7).
- **DEPS:** design; green-thread A1 base (shipped).

### W4-3 · Magic-method interop: Printable/`__toString` + `__invoke` (M gap #17) — M
- **LENS:** MANDATORY-better (PHP's `__toString` is implicit coercion magic; Phorj's is an explicit
  checked contract) — this is F XL-011 (ADOPT-NOW) and mega's A-magic-stringable/invoke, one item.
- **WHAT:** `Core.Printable { function toText(): string }` — a class-typed interpolation hole
  requires it (compile error + did-you-mean otherwise; today render is primitive-only,
  value.rs:461); transpiles to `__toString()`. `__invoke` counterpart: a class implementing
  `Core.Callable`-shape gets call syntax, transpiling to `__invoke` (scope-check in design: may
  defer if method-refs (W3-9) cover the use).
- **ACCEPTANCE:** interpolating a Printable instance byte-identical incl. PHP (`__toString` emitted);
  non-implementing class stays a compile error; guide example.
- **DEPS:** none. W5-2 `derive(Show)` auto-implements it.

### W4-4 · M-text: Unicode-correct strings (M gap #6; DEF-016; LI-D9) — XL · DESIGN-NEEDED
- **LENS:** **MANDATORY-better** (⏳BLOCKED-ON(R4-M2), default: **adopt Unicode-correct-by-default**).
  PHP does it wrong (byte strlen + the mb_* second family); Phorj currently **inherits the flaw**
  [M verified: `"héllo".length` = 6] — the one genuinely inherited PHP defect. Breaking, codemod-assisted.
- **WHAT:** scalar `string` ops (length/slice/indexOf/upper/…) become code-point-or-grapheme correct
  (design chooses the unit; recommendation: code points for length/index, explicit grapheme API —
  full design in M-text); byte semantics remain explicitly available via `bytes`/`Core.Bytes`;
  `s[i]` string indexing + `Text.charAt/substring` semantics land here (perimeter deferral);
  chr/ord as `Core.Codepoint` ops; charset conversion contract (ICONV rows); normalization/grapheme
  breadth staged (M-text S2/S3).
- **HOW:** std-only UTF-8 iteration (Rust chars) keeps DEC-009 intact for the core; PHP mapping =
  mb_* family (floor 8.5 has mbstring? — **the oracle runs `php -n`**: core stdlib must map to
  tier-1 PCRE/`preg_*` with `u` or hand-rolled UTF-8 helpers, per the transpile-no-ini-extensions
  memory — this is the design doc's hard constraint).
- **ACCEPTANCE:** `"héllo".length == 5` byte-identical on all three legs; full non-ASCII conformance
  set; codemod for any byte-length-dependent code (expected rare); KNOWN_ISSUES DEF-016 note closed.
- **DEPS:** design; the `php -n` mapping constraint resolved first.

### W4-5 · Date/time breadth (M gap #9) — L
- **LENS:** better-than-PHP (immutable-only stands, DEF-011/DEF-039 stay fixed — no strtotime DWIM).
- **WHAT:** IANA timezones (N-tz-iana, M-TIME-2), formatting breadth (beyond toIso), DatePeriod
  equivalent (range iteration), parsing (explicit format-string only), mktime/checkdate factories.
- **HOW:** tz data: compile-time-embedded subset or system tzdb read (design pick; determinism —
  tz math on fixed instants is pure/Tier A); PHP mapping = DateTimeImmutable/DateTimeZone.
- **ACCEPTANCE:** tz conversion vectors byte-identical incl. PHP; `Time.freeze` seam untouched; example.
- **DEPS:** none.

### W4-6 · Stdlib blitz: array/list long tail + math breadth + misc (gaps #11, #20 part; LI-D2, LI-D12) — L
- **WHAT:** the L-list-breadth remainder (`zip` — deferred from B3 —, diff/intersect on lists,
  splice/column/combine/pad idioms per charter), `Math` long tail (asin/acos/atan/atan2/hyperbolics/
  hypot/deg2rad/log2/…, G-math-breadth), `Math.rem`/`mod` follow-up (LI-D12, never closed),
  ctype completions, filter/validation breadth (email/URL validators), `List.enumerate` extensions.
  Uses the collection-native recipe (memory stdlib-collection-breadth) verbatim.
- **WHY:** M TOP-20 #11 ("muscle-memory blockers for line-by-line migration"); P LI-D2/LI-D12.
- **ACCEPTANCE:** per-native byte-identity incl. PHP mapping tests; charter table (§9) rows flipped
  to COVERED; guide examples extended.
- **DEPS:** tuples (W5-10) make `zip` natural — sequence `zip` after it or return `List<Pair>` class interim.

### W4-7 · Lift Tier-2/Tier-3 depth + playground PHP input (LI-F6) — L
- **WHAT:** lifter inference depth (array→List/Map/Set element inference, foreach element types,
  defaults, backed enums, key-foreach, elvis, assign-as-subexpr, sprintf→Fmt, generators, named
  args/variadics as they land); Tier-3 best-effort with loud `// LIFTED TIER-3 (unsafe — verify)`
  (DEC-166); `phg format` F5 lift-comment fidelity; playground "paste PHP → Phorj" input mode.
- **WHY:** P LI-F6; the bidirectional bridge is the pitch (G) — the ↑ direction must keep pace with
  the ↓ features shipped above.
- **ACCEPTANCE:** `tests/lift_roundtrip.rs` extended per construct (round-trip-gated for Tier-2);
  playground button behind the existing wasm build.
- **DEPS:** W4-1, W4-2, W3-5 (targets must exist before the lifter maps to them).

### W4-8 · General inert attributes (LI-E10 part) — M
- **WHAT:** user-defined `#[Attr(...)]` as **inert metadata** (readable via Core.Reflection),
  beyond the shipped `#[Route]`; the inert-vs-behavior decision recorded as: inert-only (behavioral
  attributes stay closed — the derive channel W5-2 is the sanctioned behavioral surface).
- **WHY:** M TOP-20 #16 (PHP frameworks are attribute-driven); full-bidirectional W2.4.
- **ACCEPTANCE:** attributes survive lift (PHP #[Attr] ↔ Phorj), reflectable, transpile 1:1; example.
- **DEPS:** none.

### W4-9 · Dynamic `Json`/`Any` boundary type (LI-D4) — M
- **WHAT:** the injected-type-pattern `Json` ADT completion for untyped-boundary work (mega C5) —
  ergonomic accessors, path queries, `mixed` escape hatch discipline documented.
- **ACCEPTANCE:** boundary round-trip tests; composes with W3-8 encode.
- **DEPS:** W3-8.

### W4-10 · XML/DOM/XPath (M gap #12) — L · DESIGN-NEEDED (charter admission)
- **LENS:** better-than-PHP (typed nodes, no SimpleXML magic objects); Tier A (parsing is pure).
- **WHAT:** `Core.Xml` — parse to a typed node tree + query subset + emission (Core.Html stays
  emission-only for HTML). Scope per §9 charter ruling: DOM subset + XPath-lite, not the 12-row
  full zoo.
- **WHY:** M TOP-20 #12: "enterprise integration formats; no plan on record" — this is the plan record.
- **ACCEPTANCE:** conformance vectors; PHP mapping (DOMDocument idiom); example.
- **DEPS:** §9 charter ratification.

### W4-11 · Subprocess execution (M gap #13) — M · DESIGN-NEEDED (charter admission)
- **LENS:** better-than-PHP (arg-vector API only — **no shell-string exec, ever** (SYN-015 GD stands);
  the injection class stays removed); Tier B quarantined.
- **WHAT:** `Core.Process.run(cmd, args) -> ProcessResult` (exit/stdout/stderr, typed); no popen
  streaming v1.
- **WHY:** M TOP-20 #13: CLI tooling shells out constantly.
- **ACCEPTANCE:** fixture-tested (run a committed script); `pure:false` conventions; PHP mapping
  proc_open with arg arrays.
- **DEPS:** §9 charter ratification.

### W4-12 · Compression / archives (M gap #15) + regex breadth (M gap #14) — L
- **WHAT:** (a) `Core.Compress` (gzip/deflate — pure, Tier A; std-only DEFLATE impl or a vetted dep
  per DEC-009 ruling) + zip archives read/write subset; (b) regex breadth: `replaceWith(callback)`
  (preg_replace_callback — rides the HigherOrder native machinery), `Regex.quote`, modifier-surface
  audit vs the regex crate (LI-D7 W2).
- **ACCEPTANCE:** round-trip vectors byte-identical; callback-replace differential (HigherOrder
  fault-parity discipline); PHP mappings (zlib fns / preg_replace_callback).
- **DEPS:** (a) dep ruling; (b) none.

### W4-13 · M-NUM-2: BigInt + arbitrary-precision + Money (LI-D10; gap #20 tail) — L · DESIGN-NEEDED
- **WHAT:** `Core.BigInt` (arbitrary-precision int), arbitrary-precision decimal extension,
  `Money` + currency type over `decimal`.
- **WHY:** P LI-D10 (m-num deferrals, ROADMAP); M FN-MATH GP rows (GMP/BCMath).
- **ACCEPTANCE:** vectors vs BCMath outputs (the decimal→BCMath mapping precedent, DEC-147);
  byte-identity; example.
- **DEPS:** none.

---

## 7. WAVE 5 — BEYOND-PHP PROGRAMME

*Where Phorj pulls ahead. F's 13 ADOPT-NOW (⏳BLOCKED-ON(R3-4), default: adopt all 13 — 11 are
front-end-only), the concurrency/M-Parallel cluster, the perf lane, and the DX cluster.*

### The 13 ADOPT-NOW items (F Tier 1; per-item deep dives in F §2)

| # | Item | LENS | HOW (constraint per G-3) | ACCEPTANCE | SIZE |
|---|---|---|---|---|---|
| W5-1 | **XL-001 interpolation format specifiers** `"{price:.2f}"` | beyond-PHP; → `sprintf`/number_format (idiomatic) | closed spec mini-grammar (shared with W3-5), checked vs the hole's static type, desugars in checker to natives — **no new Op** | spec-vs-type error tests; byte-identity incl. PHP; guide example | M |
| W5-2 | **XL-002 closed derive channel** `#[derive(Equals, Show, Hash, Ord, Default)]` (wave 2: `Json` = XL-028) | beyond-PHP; emits real readable PHP methods | front-end synthesis pass in the `erase_generics`/`expand_aliases` chokepoint family — methods exist before any backend; closed set, never user macros (the sanctioned answer to rejected open macros — resolves mega H2/LI-E7) | derived methods byte-identical incl. PHP; `phg transpile` shows the synthesis; Hash feeds user-typed Map/Set keys (HKey extension design note); guide example | L |
| W5-3 | **XL-003 sealed hierarchies** + exhaustive subtype match | beyond-PHP; erases to plain interfaces (compile-time-only, legit) | `sealed` modifier; checker uses the existing whole-program `class_implements` table; type patterns reuse `Op::IsInstance` — no new Op | match over a sealed interface with no `_` compiles; adding an implementor breaks exhaustiveness (test); example | M |
| W5-4 | **XL-005 doc-tests** | beyond-PHP tooling | extract ```` ``` ```` blocks from `///` comments → run through the 3-way oracle like examples; design into the `phg doc` comment format NOW (before W5-15) | a rotted doc block fails CI; stdlib natives get doc examples | M |
| W5-5 | **XL-006 opaque newtypes** `newtype UserId = int;` | beyond-PHP; erases to rep type | rides the `type`-alias machinery with the compatibility flip (no implicit assignability); smart-ctor pattern documented | `takesUserId(orderId)` is a compile error; erased PHP unchanged; example | M |
| W5-6 | **XL-007 Optional/Result combinators** (`map/flatMap/getOr/filter/okOr`; `mapError/andThen/toOptional`) | beyond-PHP stdlib; mechanical PHP mapping | pure stdlib recipe — generic sigs + `NativeEval::HigherOrder` + UFCS, all shipped; new `Core.Optional`/`Core.Result` modules | per-combinator differential incl. fault-parity re-entrancy; example | M |
| W5-7 | **XL-008 compile-time-validated literals** (regex, format specs) | beyond-PHP; no PHP delta (front-end only) | checker special-cases literal args to known-validatable natives; design the small generic "literal validators" registry once | bad regex literal = compile error w/ caret; dynamic strings keep runtime behavior; tests | S |
| W5-8 | **XL-012 let-else** `var x = opt else { return 0; }` | beyond-PHP; plain PHP if-null | parser desugar over shipped if-let + the totality engine verifies the else diverges | flat-binding scope tests; byte-identity; example | S |
| W5-9 | **XL-009 auto-import quickfix + import organizer** | beyond-PHP tooling; load-bearing for no-wind (W2-6) | registry `(module,name)` lookup → W2-1 structured fix + LSP code-action + `phg format` import sorting | unknown-name-with-one-candidate produces the import edit; sorted-imports canonical form | M |
| W5-10 | **XL-010 tuples + multiple return** `(int, string) f()` / `var (a, b) = f();` | beyond-PHP (typed positional product); PHP `[$a,$b]` — the exact idiom, now typed | `Ty::Tuple`; **value rep: reuse the List runtime rep with checker-level arity** (the Map runtime-polymorphism precedent — zero new Value/Op); destructuring rides pattern machinery; positional only, no 1-tuples | differential incl. tuple-in-match; map iteration `(k,v)` ergonomics; unlocks `zip` (W4-6); example | L |
| W5-11 | **XL-011 Printable display protocol** | (= W4-3 — listed in both inventories; **build once in W4-3**, this row is the cross-reference) | — | — | — |
| W5-12 | **XL-013 labeled loops** `outer: for … { break outer; }` | beyond-PHP (upgrades PHP's `break 2` footgun); emits `break N` with N computed | parser + compiler jump bookkeeping — no new Op | refactor-safety test (label survives nesting change); example | S |
| — | **XL-004 `phg fix`** | (pulled forward to W2-1) | — | — | — |

### W5-13 · VM debug symbols (LI-C1) — L
- **WHAT:** the four-lane Lane 3, W1–W5: per-local scope IP ranges → named locals at VM fault
  (`runvm --dump-on-fault`) → VM per-line pause hook → wire the VM into `src/debug.rs` (REPL + DAP
  over runvm) → `examples/debug/`. **Also the fix for H's fault-line divergence** (W0-5): with IP→line
  mapping correct inside interpolation, flip on the harness line-comparison flag.
- **WHY:** P LI-C1; closes the M-DX S3/S5 deviation; upgrades G-1.1's fault-parity claim to include lines.
- **ACCEPTANCE:** H's `r1/r6/r11` skew shapes now agree; the W0-5 flag permanently on; debugger works
  on runvm.
- **DEPS:** W1-4 (compiler layout).

### W5-14 · M-perf lane (LI-B1, LI-B2, LI-B3; A-PERF trio) — L
- **WHAT:** (a) ⏳BLOCKED-ON(R3-12), default adopt: `Op::CallMethod` inline cache (A-PERF-1 — two
  String allocs per call; extend the S1b field-cache shape) + `Op::CallValue` captures borrow
  (A-PERF-2 — deep-clone per closure invocation, paid per element in map/filter/reduce) +
  interpreter static/const borrow-keyed lookup (A-PERF-3); (b) LI-B1 W2 Rc-share `Value::Str`
  (scoped, the declared "NEXT concrete perf step", 164 sites); (c) LI-B2: W3 intern IsInstance ·
  W4 dispatch · W5 const-fold · W6 peephole · W7 lazy for-range; (d) LI-B3 incremental/cached
  compilation keyed on content hash (design-needed).
- **ACCEPTANCE (per G-2/G-3):** every item ships with `phg benchmark` before/after numbers;
  `scripts/perf-gate.sh` floor raised only with evidence; byte-identity untouched.
- **DEPS:** W1-4.

### W5-15 · DX cluster: `phg repl` + `phg doc` + parser multi-error recovery (LI-F1, LI-F2, LI-E19) — L
- **WHAT:** REPL (the M-DX debugger REPL is the seed; interpreter-only is fine — a REPL is a dev
  surface, no parity burden; shadowing/redefinition model designed vs immutable-by-default);
  `phg doc` generator (doc-comment format co-designed with W5-4); parser multi-error recovery
  (today first-error-stops; recover at statement boundaries).
- **ACCEPTANCE:** REPL session test script; `phg doc` renders the stdlib registry; multi-error fixture
  reports ≥2 diagnostics.
- **DEPS:** W5-4 (doc format).

### W5-16 · Concurrency completion: structured concurrency + M-Parallel (LI-A2, LI-A3, LI-A4) — XL · DESIGN
- **LENS:** beyond-PHP; **outside the PHP oracle** (G-1.1 — the exception's scope grows with each
  primitive; disclose per item).
- **WHAT:** (a) LI-A4 green-thread follow-ups: method/overloaded/closure spawn, cooperative
  fault-trace frames, per-task statics, wasm frame-swap executor; (b) **structured concurrency
  scopes** (XL-014: `scope { spawn …; }` join-all-at-exit, error propagation — adopt *before*
  unstructured spawn idioms accumulate) + `select` over channels (XL-015 — deterministic policy:
  declared priority order, never Go's randomization) + deadlines (XL-016) + `Task.all`/race
  (LI-A2 re-scoped: **structured concurrency without colored async/await** — colored functions stay
  REJECTED, XL-041/SSOT); (c) **M-Parallel** (LI-A3): the commissioned deep plan — actor-model
  multicore (XL-017, the only path that keeps the Rc heap) + the D-Async-1 pure data-parallel subset
  re-scoped against green threads. ⏳BLOCKED-ON(DEC-135 deep-plan approval) — default: actor model,
  plan-first; nothing else here blocks on it.
- **ACCEPTANCE:** run≡runvm determinism litmus per primitive (the A1 litmus discipline);
  `E-CONCURRENCY-NO-PHP` coverage extended; examples under the quarantine conventions.
- **DEPS:** design docs; W5-3 (sealed) helps message-type modeling.

### W5-17 · Pending-syntax checkpoints: generics explicit args + UFCS policy + ternary register — S
- **(a)** ⏳BLOCKED-ON(R2-A) — generics explicit type arguments. Default (rec): **adopt both sites**
  — `new Box<int>([])` + `firstOr<int>([], 0)` with TS-style lookahead disambiguation (closes the
  no-turbofish gap; the R2-A examples show un-annotatable positions). If overruled to new-sites-only:
  drop the call-site grammar; if keep-inference-only: record as permanent limitation in the spec.
- **(b)** ⏳BLOCKED-ON(R2-B) — UFCS stability policy. Default (rec): **keep breadth + policy** —
  (1) leaf-name uniqueness across UFCS-eligible natives enforced by a CI registry test (the
  `repeat`→`fill` incident becomes structurally impossible), (2) documented precedence: real methods
  beat UFCS. If overruled: marked-natives-only or primitive-receiver retirement — each a scoped codemod.
- **(c)** LI-E9 ternary `? :`: stays **deferred-not-rejected** (DEC-090); the revisit trigger
  (demand evidence) recorded here as the register carrier; C-5's stale record fixed in W0-7.
- **(d)** ⏳BLOCKED-ON(R2-C) — bulk-ratify the six shipped autonomous items (totality contours ·
  pattern-cluster syntax · generic-invariance retrofit · COW `Op::SetIndexLocal` · dogfood grammar
  patches · `phg debug` surface). Default: ratify all; any pulled item gets its own review slice.

### The 27 ADOPT-LATER (F Tier 2) — charter table
Carried as a standing charter; each activates on its named trigger. XL-014/015/016/017 → W5-16.
XL-018 `defer` + XL-019 `using`/resource blocks → land with the first handle-based IO (W3-1 DB
handles are the likely trigger). XL-020 try-expression→Result bridge → after W5-6. XL-021
semver-check → **flips to NOW at the first tagged public release** (W6-7). XL-022 snapshot tests ·
XL-023 property-based testing → M-Test follow-ups. XL-024 deprecation-with-codemod → after W2-1
(pre-1.0 renames become one command). XL-025 REPL → W5-15 (promoted). XL-026 asset embedding →
M2.5/M6 companion (deterministic — stays inside the differential). XL-027 workspaces → with
transitive deps (W6-6). XL-028 derive(Json) → W5-2 wave 2 (promoted). XL-029 slice syntax ·
XL-030 range patterns · XL-031 @-bindings · XL-032 list patterns → the next pattern slice as a
batch (mega C2 = LI-E4 carrier; XL-032 after XL-029). XL-033 trailing closures · XL-034 pipe
placeholder → collect friction data from W5-6 usage first. XL-035 nameof → a diagnostics/reflection
slice (`Core.nameof` per no-wind). XL-036 wasm build target → M2.5 Phase 3 adjacency. XL-037 purity
annotations → when M-Parallel needs provable purity. XL-038 contracts → profile-gated, after
assertions usage data. XL-039 raw-string ergonomics → with XL-008 regex work (note: `r"…"` already
shipped — this row is the *escaping-form audit*). XL-040 generators → W4-2 (promoted).
**Comprehensions (mega B2 = LI-E1): CONFLICT** — F rejects (XL-049: map/filter is the teachable
spelling) vs the mega backlog carries it live. ⏳ adjudicate; default: **REJECT stands** (Appendix A),
LI-E1 discharged by this record. **Enum methods + associated fns (mega C1 = LI-E3)** and
**ergonomics pack (C4 = LI-E5)**: adopt-later, batch with the pattern slice / scope-at-start
respectively. **Protocols (mega D1 = LI-E6)**: Printable ships in W4-3; Comparable/Equatable/
Iterable follow as design-first slices (Equatable/Hash via W5-2 derive; Iterable via W4-2).
**FFI (mega H3 = LI-E8): CONFLICT** — SSOT §5 rejected E-php-ffi; the extension seam is `.d.phg`
declare-interop (M8.5, shipped). ⏳ adjudicate; default: REJECT stands, `.d.phg` + the §9
extension-policy path is the answer. **Compile-time macros (mega H2 = LI-E7):** resolved — open
macros stay rejected; W5-2's closed derive channel is the sanctioned subset. **Editions (mega H4 =
LI-G7):** M13, post-1.0 — spec hook designed in W6-5, mechanism deferred.

---

## 8. WAVE 6 — SHOWCASE, SPEC & GA

### W6-1 · Promote the flagship + build the killer app — M
- **WHAT:** move/copy `conformance/ddd/` to `examples/flagship/` (conformance golden keeps pointing
  at it); build `examples/flagship/api/`: a ~300-line REST micro-service composing Core.Http routing
  + Json + `decimal` money + throws/Result + `Secret` + a `phg test` file — every ingredient ships
  today (before Wave 3 even); after Wave 3, extend with DB + sessions. Feature it as README section 2
  with its transpiled-PHP twin side by side (the bridge IS the pitch).
- **WHY:** G P1 killer-app gap ("the flagship exists but is hidden in conformance/").
  ⏳BLOCKED-ON(R3-3) — default: adopt all 6 G items (this + W0-6 + W6-3/4/5 + editor refresh).
- **ACCEPTANCE:** flagship indexed, differential-gated, README-featured.

### W6-2 · (reserved — flagship API v2 with the Wave-3 spine) — folds into W6-1's extension step.

### W6-3 · Truthful README rewrite — S
- **WHAT:** status table current (~15 shipped milestones undersold); dependency policy stated proudly
  (4 vetted deps + policy link — "zero deps" is FALSE and stays deleted); "Language at a glance"
  regenerated from STABILITY.md's stable list (traits/unions/generics/decimal/concurrency/web all
  missing today); `phg lift` promoted to headline (README currently denies half the pitch);
  transpiler description current (native match shipped); **G-1.1 concurrency disclosure** placed
  where byte-identity is claimed.
- **WHY:** G P1 batch. DEPS: W0-6 (front door), best after Wave 3 (more to be truthful about).

### W6-4 · Benchmark credibility protocol — M
- **WHAT:** retire the "3.23× vs dev-built PHP 8.6" number; re-measure vs **release PHP 8.5 (the
  floor) with `opcache.enable_cli=1`, JIT on/off as two columns**, ≥3 workloads (scalar, object-heavy,
  string/collection), spawn-per-sample caveat quantified, methodology doc, publish the ratio *range*;
  optional CI job re-measuring against packaged PHP so the number can't rot. Includes LI-F7: port the
  remaining benchforge benchmarks (Search, StringProcessing, ObjectGraph — unblocked by W2-11
  field-base index-assign; in /stack/projects/phorj-app, no auto-commit there) + W6 large-data memory
  stress completion.
- **WHY:** G P1 ("HN's first comment writes itself"); post-wave3 plan's own rule: no public perf
  claims before optimized-PHP baseline. DEPS: W2-11.

### W6-5 · Normative spec + clause-tagged conformance — L
- **WHAT:** `docs/spec/` normative chapters 01-lexical … 10-php-mapping (G Part 2 architecture;
  chapter 10 — the transpile contract — is unique to Phorj and a marketing asset), each clause with a
  stable ID; **spec-tag the conformance corpus** (`// conformance: §3.4.2` headers) + a build-time
  coverage matrix as a failing test (every stable clause names ≥1 program or an explicit `untested`
  marker); factor the duplicated PHP-oracle plumbing into a shared test-support module; version
  front-matter + the M13 editions hook (`since:`/`edition:` fields). Source material: STABILITY.md
  list, explain registry, guide-example prose (60% of the text exists). W2-13's should-error corpus
  is the negative half. **The G-1.1 disclosure is a normative clause.**
- **WHY:** G Part 2: "provably-correct upgrade of PHP" made checkable by outsiders; the corpus is
  currently keyed to a bullet list, coverage unverifiable.
- **ACCEPTANCE:** spec-coverage matrix green; a clause without a program fails CI.
- **DEPS:** W2-13; best current after Wave 4 (spec the shipped surface).

### W6-6 · GA hardening batches (LI-G1..G6) — XL (re-verify each; statuses stale)
- **WHAT:** (a) **LI-G2** M8 security findings batch (15 items: git `--` separation, dep-name
  traversal, serve `catch_unwind`, free-fn/PHP-builtin collision, promoted-field visibility,
  stub-cache atomicity, vendor swap window, malformed Content-Length, slowloris/read timeout,
  symlink escape, `E-VENDOR-TAMPERED` re-validation, lockfile hash verify, git-env isolation,
  `write_atomic`, php_compat lints) — several likely already fixed (A-SEC-4/5 verified some);
  **re-verify each before working it**; (b) **LI-G3** M9 hygiene leftovers (15 items — same
  re-verify rule; several possibly closed by traces Slice 1 / M-DX ratchet / S7b-2); (c) **LI-G4**
  M10/M11 residue: arm-unification null-typing, mangle injectivity, typed Header, library-package fn
  values, block-body return inference, fn-type variance, **transitive git deps** (+ XL-027
  workspaces rider); (d) **LI-G6** post-wave3 punch list: P1-c ext-policy denylist CI scan, P1-f
  fuzz/no-panic EV-7 harness, P1-g loader-routed `check --json`, the P2 transpiler-fidelity cluster,
  P3 batch; (e) **LI-G1** the 11 GA exit criteria as the wave's closing checklist.
- **ACCEPTANCE:** per-item evidence (test or verified-already-fixed note); GA checklist green.

### W6-7 · Release engineering (LI-G5, LI-F8, LI-F5) — L
- **WHAT:** M12: language reference (= W6-5 spec), tour, migration guide, transpile-contract doc
  (spec ch. 10), fuzzing, TextMate/tree-sitter grammar files, release automation + SHA-256 + the
  1.0 bump; XL-021 semver-check flips to NOW here; **LI-F8** M2.5 Phase 3b `--sign`
  (Authenticode + rcodesign notarize) + macOS stub + CI stub-registry productionization
  (cert/SDK-blocked — track externally); **LI-F5** `phg build` merges `vendor/` (multi-file
  projects), bytecode-payload flip decision, project-loader routing.
- **DEPS:** W6-5/W6-6.

### W6-8 · Editor + docs-site surface (G item 6; LI-F3, LI-F4) — M
- **WHAT:** grammar additions (`empty`, `todo`, `unreachable`, `parent`, `html"` prefix), extension
  version tracks phg, drop the committed `.vsix`; **LI-F4** LSP finish: member completion
  (resolved-type index), lambda/match-pattern binders, cross-file *references*, JetBrains plugin
  completion; **LI-F3** docs site + playground polish + tutorial chapters (mega G1/G2).
- **DEPS:** W6-5 (spec is the docs-site backbone).

---

## 9. STDLIB CHARTER — the 259 GAP-unplanned rows

Per M's bucketing recommendation (R4-M5; ⏳BLOCKED-ON(R4-M5), default: adopt the three buckets).
Every FN GAP-unplanned row from M §1.2 lands in exactly one bucket; the per-group counts reconcile
to 259. Governance: **LI-D11** — the M4 charter's *enforcement half* (naming/shape/determinism-tier
checks on new natives; charter spec `docs/specs/2026-06-29-m4-stdlib-charter.md` adopted, minimal
checks unverified) ships as part of the first Wave-3 stdlib item and gates every row below.
Per-module open decisions carried from the extended-scope plan (D-Stream/D-Test-Q1/D-Mock/D-Http/
D-Cache/D-Db) resolve as each module is built (LI-D8 discipline).

**Bucket 1 — ADOPT into waves/M-Batteries** (≈115 rows): everything itemized in Waves 3–4 (DB 10,
CURL 13, SESS 10, FS ~30 incl. dirs — stream *handles* stay pending the resource-model design —,
sprintf 7, HMAC/KDF 7, XML 12, subprocess 6, compression 8, regex 4, mb/iconv ~16 via M-text,
DATE 8, ARR/STR/MATH long tail ~20, url/net log rows) + the LI-D8 module backlog on triggers:
**Core.Serde** (after derive W5-2), **Core.Event**, **Core.Cli** (getopt — G-args), **Core.Template**
(design vs `html"…"` overlap), **Core.Uuid** (v5/v3 pure now; v4 needs W3-4), **Caching/memoize**
(needs purity signal — XL-037 trigger), **Core.Dump** (var_export-shape debug rendering —
`inspect::render` is the seed).

**Bucket 2 — EXTENSION story** (≈75 rows): intl formatters/calendars/collation (ICU-weight), gd/image
(~110-fn family), raw sockets, streams/wrappers zoo, SPL iterator-decorator zoo, finfo/MIME,
readline/process-title. **Decision path:** these need the explicit extension-policy design (tier-3 of
the determinism partition): options are (a) vetted-dep feature forks like regex/argon2 (DEC-009 per-dep
authorization), (b) a `phg`-native plugin seam (rejected so far — RT-015 GD), (c) `.d.phg` declare-
interop to PHP extensions on the PHP leg only (breaks native-first, DEC-005 — likely reject).
DESIGN-NEEDED; until ruled, these rows count against parity honestly (they are in the §10 residual).

**Bucket 3 — REJECT with reason** (≈69 rows, Appendix A carries them): mysqli-procedural aliases +
the ~26 date procedural aliases (OO-only by design), legacy/deprecated fns (each/create_function/
utf8_encode/money_format/fgetss/strftime…), superglobal-mutation APIs (filter_input, ob_* stack,
ini_*, set_error_handler — conflict with value-level Request/compile-time config/three-tier errors),
internal-pointer array cursor family, magic-quotes/addslashes family, similar_text/soundex/metaphone
DWIM-linguistics (candidates for Bucket 1 admission only on demand evidence), weak refs/lazy objects
(no tracing GC by design), mail() (DEF-031 — inject-prone; a future Core.Mail rides Bucket 1 on
demand, never the PHP mail() shape).

---

## 10. PERCENTAGE LEDGER

Model: M §4 (COVERED×1 + PARTIAL×0.5) / (rows − N/A − GD); domain weights 35 SYN / 40 FN(usage-
weighted) / 25 RT; Vision = 0.70×parity + 0.30×programme. Projections are **[Speculative — model-based;
re-run the real arithmetic after each wave per §0.3]**; the wave-3 stdlib arithmetic is shown as the
worked pattern.

| After wave | What moves | SYN | FN (usage-wtd) | RT | **Parity** | Programme | **Vision** |
|---|---|---|---|---|---|---|---|
| baseline | — | 79.8% | 32.5% | 69.4% | **≈58%** | 64.4% | **≈60%** |
| W0–W1 | repairs/docs/splits — no surface rows | 79.8% | 32.5% | 69.4% | **58%** | ~66% (hygiene milestones) | **≈61%** |
| W2 | soundness rows + enforcement (a few SYN PARTIAL→C) | ~81% | 32.5% | 69.4% | **≈59%** | ~70% | **≈62%** |
| W3 | DB+13 CURL+10 SESS+7 sprintf+25 FS+7 crypto+5 url ⇒ T1 score 124.5→168.5/303, T2 18.5→51.5/140 ⇒ weighted (3×168.5+2×51.5)/1264 = **48.1%**; RT +log/serve rows ~72% | 81% | 48.1% | 72% | 28.4+19.2+18.0 = **≈65%** | ~74% | **≈68%** |
| W4 | named-args/variadics/generators/magic (+~7 SYN) ⇒ ~85%; M-text+date+arr+xml+proc+zlib+regex (+~65 FN rows) ⇒ ~58%; RT ~74% | 85% | 58% | 74% | 29.8+23.2+18.5 = **≈71%** | ~80% | **≈74%** |
| W5 | beyond-PHP barely moves parity (attrs/iterator rows) | 86% | 59% | 74% | **≈72%** | ~90% | **≈78%** |
| W6 | RT/ecosystem rows (docs, release, framework story) ~85% | 86% | 60% | 85% | 30.1+24.0+21.3 = **≈75%** | ~95% | **≈81%** |

Residual after W6 (the honest gap to 100): Bucket-2 extension-tier stdlib (§9, pending the extension-
policy ruling), the GAP-by-design rows (excluded from denominators by the model — Appendix A), and
post-1.0 M13 editions. 100% *of the vision* is reached when Bucket 2 is ruled and executed or
formally re-bucketed to reject.

---

## 11. APPENDICES

### Appendix A — REJECTED items (no silent scope drops)

**A.1 · M's 49 GAP-by-design rows** (deliberate removals — the denominator exclusions; full row IDs
in M Pass 1): eval/include/shell-string-exec (SYN-013/015; RCE + determinism), `$$x` variable-
variables & compact/extract (SYN-019/021-023, DEF-021), references (SYN-065/093, DEF-006), `@`
suppression (SYN-059, DEF-007), `goto` (SYN-076), `switch` fall-through (SYN-073 — exhaustive match
covers), runtime magic `__get/__set/__call/__callStatic` + destructors + native object serialization
(SYN-126..136, DEF-013/035), isset/empty truthiness (SYN-131/132/160, DEF-014), locale-sensitive core
(FN-STR setlocale rows, DEF-015), mutable DateTime + strtotime DWIM (FN-DATE, DEF-011/039), pcntl
fork/signals (FN-PROC — green threads model), ini/runtime config + error handlers (SYN-153/166,
DEC-006), ICU Collator/Transliterator (FN-INTL — extension tier), func_get_args/get_defined_vars/
class_exists-at-runtime (static model), dynamic extension .so model (RT-015).

**A.2 · F's 26 cross-language rejects** (XL-041..066, reasons in F Tier 3): colored async/await ·
open macros · comptime · decorators · user operator overloading · extension functions ·
Kotlin scope functions · Dart cascades · **comprehensions** (also discharges mega B2/LI-E1 — ⏳noted
in W5's charter) · LINQ syntax · implicit `it`/`$0` · placeholder partial application · structural
records · refinement/liquid types · typestate/linear types · HKT/typeclasses · variance annotations ·
const generics · GADTs · units of measure · guaranteed TCO (spine violation) · method_missing ·
implicits/givens · do-notation · chained comparisons · hot code reload. Plus **FFI** (mega H3/LI-E8 —
SSOT §5 reject stands by default, `.d.phg` is the seam; ⏳noted in W5).

**A.3 · Register rejects** (C §10 representative bucket of 81): single-quote strings, `<=>`, `.`
concat, ambient superglobals, loose `==`, plus the PL-theory vanity set — all re-graded under the
craftsmanship-apex lens (DEC-004) and still rejected.

**A.4 · Stdlib Bucket-3 rejects**: §9 Bucket 3 (≈69 FN rows with per-family reasons).

### Appendix B — Question-catalog cross-reference (every ⏳ in this plan)

| ⏳ id | Where in this plan | Default path |
|---|---|---|
| R2-A generics explicit type args | W5-17a | adopt both sites |
| R2-B UFCS stability policy | W5-17b | keep breadth + uniqueness CI test + method precedence |
| R2-Q4 doc-reconciliation batch | W0-7 | batch as one task |
| R2-C bulk-ratify six autonomous items | W5-17d | ratify all |
| R3-1 DEC-133 concurrency-outside-oracle | G-1.1 (governance) | ratify + disclose in README/spec |
| R3-2 `new` on enum variants | W2-14 | keep mandatory `new` |
| R3-3 G-showcase roadmap (6 items) | W0-6, W6-1/3/4/5/8 | adopt all |
| R3-4 F ADOPT-NOW batch (13) | W2-1, W5-1..12 | adopt all 13 |
| R3-5 hooksPath repair | W0-1 | approve (one line) |
| R3-6 anti-regrowth size rule | G-6, W1-6 | adopt |
| R3-7 differential.rs split | W1-1 | adopt as step 0 |
| R3-8 examples README restructure | W0-9d | adopt |
| R3-9 KNOWN_ISSUES prune + codemod/dist cleanup | W0-9, W0-6d | adopt all three |
| R3-10 E-drift fix batch (DRIFT-01..09) | W0-7 | one doc-hygiene task |
| R3-11 A P2-security batch | W0-10 | adopt as one hardening task |
| R3-12 VM perf pair | W5-14a | both, with `phg benchmark` evidence |
| R4-M1 completion numbers + weight model | §0.3, §10 | ratify + recompute-per-milestone rule |
| R4-M2 Unicode strings (DEF-016) | W4-4 | adopt Unicode-correct-by-default |
| R4-M3 TOP-20 prioritization | Wave 3 ordering | web-app enablement spine as critical path |
| R4-M4 3 partially-fixed DEFs | DEF-030→W3-2; DEF-024/037 → document-as-design (W6-5 spec wording) | as recommended |
| R4-M5 GAP-unplanned bucketing | §9 | three buckets |
| R4-P plan deletions (48) | W0-8 | approve after this plan lands |
| R4-H1..H6 enforcement adoptions | W2-8 (H3 folded into W0-4) | H's recommendation column |
| R4-Q CLAUDE.md rewrite | W0-7 part 3 | approve draft + relocation |
| DEC-135 M-Parallel deep plan | W5-16c | actor model, plan-first |
| comprehensions conflict (LI-E1 vs XL-049) | W5 charter / A.2 | REJECT stands |
| FFI conflict (LI-E8 vs SSOT) | W5 charter / A.2 | REJECT stands; `.d.phg` seam |
| R4-final master-plan approval + register bulk-ratify | this document | — |

### Appendix C — Input-report index

All under `docs/research/full-audit/raw/`: **M**-gap-matrix (824-row verdicts, %-model, TOP-20, DEF
scorecard, 35-capability brag) · **P**-plan-verdicts (66-plan verdicts; the 56-item live inventory
this plan absorbs; 48-file deletion command) · **B**-modularity (23-split decomposition spec + do-not-
split list + size rule) · **F**-cross-language (66 candidates: 13/27/26) · **G**-showcase (adoption-
surface audit + 6-item roadmap + spec architecture) · **H**-enforcement (~300 probes; P0 holes;
missing-rule recs; 15-rule brag) · **A**-craftsmanship (P0 hooksPath; P2/P3 batches; CI verification)
· **C**-decisions (141-row register, 10 conflicts, 33 supersessions) · **D**-php-surface (869 pinned
PHP 8.5 rows — M's Pass-1 input) · **E**-phorj-surface (code-verified surface + DRIFT-01..09) ·
**Q**-claude-md-draft (rules-only rewrite + relocation map + HISTORY skeleton). Session plan (cursor
+ QUESTION CATALOG): `docs/plans/2026-07-01-full-audit-and-master-plan.plan.md`.

### Appendix D — The marketing arsenal

**D.1 · H's 15 verified-enforced rules PHP lacks** (H §8, all probe-verified): checked exceptions
that actually check · compile-time visibility (incl. lambdas/siblings) · no type juggling · deterministic
overflow faults · compile-time exhaustive match · totality (`E-MISSING-RETURN`/`never`) · unknown-
anything-is-a-compile-error with did-you-mean · immutable-by-default + const discipline · final-by-
default + override-sig checking · null-safety as a type property · invariant generics (erased yet
enforced) · whole-project + vendored-dep type-check · Secret taint-lite (W-SECRET) · four-backend
divergence guards as language rules (E-SHADOW-*) · UFCS ambiguity is an error, not a resolution order.

**D.2 · M's 35 beyond-PHP capabilities** (M Pass 2, each with the PHP pain removed): real generics ·
compile-time null-safety · exhaustive pattern match · totality checking · three-tier error model ·
flow narrowing · `decimal` primitive · `bytes`/`string` split · typed XSS-safe HTML · `Secret<T>` ·
immutable-by-default + COW · checked arithmetic · uncolored green threads · byte-identity dual
backends + PHP oracle (disclose G-1.1) · `phg build` cross-OS binaries · two-way migration bridge
(lift + transpile) · `.d.phg` typed boundaries · toolchain-in-the-box · stable diagnostic codes +
`phg explain` · must-use by default · method + return-type overloading · MI with compile-time
conflict resolution · payload/generic enums · `[T; N]` static bounds · package hygiene as errors ·
deterministic dependency model · deterministic test seam · profiles that cannot change behavior ·
definite-assignment analysis · UFCS · expression-if/match · text blocks + raw strings · `with {}`
record update · WASM playground · whole-project checking.

---

*Maintenance: when an item completes, mark it `✅ <short-sha>` in place (never delete rows); when a ⏳
resolves, replace the marker with the ruling + date; re-run §10 after every wave. This file is the
single forward SSOT — `ROADMAP.md` and `docs/MILESTONES.md` point here after Wave 0's doc batch.*
