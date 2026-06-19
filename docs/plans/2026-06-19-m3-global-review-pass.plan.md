# M3 Global Phorge Review Pass — Plan

> The "pure review pass" (Option 4) the developer queued **before** the M3 S3 sprint:
> *"after Track A, do a pure review pass, then decide next."* Track A (M3 S3 lambdas +
> first-class functions + pipe) is now COMPLETE (master `687a7bd`, 452 tests green), so the
> precondition is met and this pass is the active next step.
> Source of the commitment: `.git/sdd/progress.md` final RESUME STATE block.

## Decisions Log
- [2026-06-19] AGREED: Sequence is **Option 2 → Option 3 → Option 1** — clear post-sprint TODOs, define review scope, then run the review. (Developer: "Option 2 was done! so Option 3 then 1".)
- [2026-06-19] AGREED: Review tool set = **`/sleuth` + `/inspect` + `/gaps` + `/forge` + `/inspect --vision`** (all five; vision included explicitly — developer wants "a forward future solid plan").
- [2026-06-19] AGREED: Breadth/depth = **whole repo, full depth** (true pure review pass, milestone-gate quality — not just recent-milestone diff).
- [2026-06-19] AGREED: Mode = **report-only first, then decide** — produce findings + a forward plan; do NOT auto-apply fixes. Fix decisions happen after review synthesis. (Matches original "pure review pass, then decide next".)
- [2026-06-19] AGREED: Execution approach = **run the real skills** (not "inspired by"), orchestrated as a **sequential pipeline**, each skill writing its raw report to disk before returning (compaction-safety), ≤5 concurrent LLM agents per stage. `/aggregate-findings` dedupes at the end; `--vision` output feeds the forward plan.

## Pre-existing State (at plan-write time, 2026-06-19)
- master HEAD: `687a7bd` — tree clean, 452 tests green (3× deterministic), clippy + fmt clean.
- M3 S3 Track A COMPLETE (12 commits `42d4ec3..687a7bd`). Byte-identical run≡runvm≡PHP across the full function-value matrix.
- Milestone state: M2 CLOSED, M5 CLOSED, M6 in progress (W0+W1 done, W2 router next), M3 S0/S1/S2 done + S3 Track A done.

## Post-Sprint TODOs — Option 2 status (VERIFIED 2026-06-19)
1. ✅ **DONE** — ask-human gate bypass removed (`~/.claude/tmp/ask-human-gate-bypass` and project-scoped path both absent; per-turn gate enforcement restored).
2. ❌ **STILL PENDING** — dangling branch `worktree-agent-a2764d080140ece46` still exists. Delete manually (classifier blocks force branch-delete here): `! git branch -D worktree-agent-a2764d080140ece46`. Harmless until then. → fold into post-review cleanup.
3. ❌ **STILL PENDING** — S3 plan file `docs/plans/2026-06-18-m3-s3-lambdas-pipe.md` not yet disposed (proposed: delete-plan-keep-spec, Rule 17 Phase 8). → fold into post-review cleanup.
> NOTE: developer believed Option 2 fully done; only item 1 was. Items 2 & 3 don't block the review (housekeeping) — carried to post-review cleanup, not a blocker.

## Formal Plan — Execution Pipeline (post-compact resume target)

**Scope:** whole repo, full depth — `src/` (lexer/parser/checker/interpreter/compiler/vm/transpile/
native/loader/manifest/lock/vendor/bundle/serve/cli/mem/value/chunk/ast), `docs/`, `examples/`, `tests/`.

**Run order (sequential; each stage persists raw output to disk BEFORE the next starts):**

1. **`/sleuth`** (project scope) — behavioral bugs / silent failures / cross-backend (run≡runvm≡PHP) contract violations. *Highest value: the correctness spine.* → write report.
2. **`/inspect`** (project scope) — health P0–P3: security, dead code, deprecations, error handling, docs, tests, config, code quality, perf, tech debt. → write report.
3. **`/gaps`** (project scope) — incomplete impls / stubs / TODOs / unfulfilled promises; cross-check against KNOWN_ISSUES deferral backlog. → write report.
4. **`/forge`** (project scope) — adversarial architecture/design critique (Chesterton's Fence, 9 agents → synthesis). → write report.
5. **`/inspect --vision`** — forward improvement proposals → **the "forward future solid plan"** the developer wants. → write report.
6. **`/aggregate-findings`** — dedupe across all 5 reports → single prioritized master list + cross-references.
7. **Synthesis** — from aggregated findings + vision, draft the forward roadmap; present P0–P3 + next-milestone options. **Report-only — no fixes applied until developer decides.**

**Reports location:** each skill writes under its own report dir (e.g. `~/.claude/projects/meta-reports/` or the skill's configured output). Confirm each report path is recorded here as stages complete so resume can find them.

**Compaction safety:** conversation context does NOT survive compact — only disk does. Every stage's raw report MUST be on disk before proceeding. If post-compact a report is missing, re-run that stage.

**Agent caps:** ≤5 concurrent LLM agents per stage (10 → ~50% rate-limit failures). Skills that fan to ~10 (inspect/sleuth/gaps/vision) must group adjacent domains. Explore agent is read-only → use general-purpose for any agent that must write to disk.

## Resume Instructions (post-compact)
1. Read this plan file + `.git/sdd/progress.md`.
2. Confirm master still `687a7bd` (or note any advance), tree clean.
3. Check which review reports already exist on disk (stages 1–5) — resume at the first missing stage.
4. After stage 5, run aggregate (stage 6) + synthesis (stage 7).
5. Then handle post-review cleanup: TODO items 2 & 3 above + this plan file's own disposition (Rule 17 Phase 8).

## Status
STATUS: Designed — not yet executed. Pipeline defined; awaiting post-compact resume to run stages 1–7.
