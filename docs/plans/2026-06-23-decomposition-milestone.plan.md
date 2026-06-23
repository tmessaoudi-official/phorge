# Decomposition Milestone Plan

> Status: RESEARCH + BRAINSTORM (pre-design). Spec will land under `docs/specs/`.

## Decisions Log
- [2026-06-23] AGREED: M-RT is CLOSED; this is the next milestone (developer-chosen).
- [2026-06-23] AGREED: research method = **scoped research-first** — read-only Explore agents map the
  whale files + a prior-art survey, THEN interactive brainstorm. (Rejected: full multi-agent workflow as
  overkill; rejected: brainstorm-now as designing on an unverified mental model.)
- [2026-06-23] FRAMING (from memory `decomposition-milestone`): a compiler is a 2-D grid — phase axis
  (current files) × construct axis (dev's "one file per construct" vision). The only by-construct form
  that preserves exhaustiveness is the **thin-dispatcher** (central match stays whole, 1 line/arm →
  delegate). Honest middle = two-level (keep phase files, split whales into cohesion sub-modules).
- [2026-06-23] CONSTRAINTS (non-negotiable): no OOP/SOLID/GoF dogma; preserve every exhaustive-match
  coupling; byte-identity spine (`run≡runvm≡real PHP 8.4`) is the verifier; prioritize by navigation
  pain (checker.rs first), not raw line count.
- [2026-06-23 15:30] AGREED (session takeover): resume per the locked method = **scoped research-first**.
  Dispatched read-only agents to (a) map checker.rs cohesion seams, (b) map the backend whales' exhaustive-
  match couplings, (c) map parser/native/ast front, (d) survey prior art (rustc HIR/query, nanopass,
  by-node vs by-phase). Raw findings → `docs/research/decomposition/raw/`. Interactive brainstorm follows.

- [2026-06-23 15:35] AGREED (axis RESOLVED by developer post-research): **Hybrid** — by-phase
  sub-split backbone (cohesion sub-modules inside one `mod`, exhaustiveness intact) + thin-dispatcher
  applied *selectively* where arm bodies are large & construct-shaped. Pure by-construct REJECTED
  (no production compiler does it; gives up exhaustiveness — Roslyn-only, runtime-default). Evidence:
  `docs/research/decomposition/SYNTHESIS.md` (4 converged agents).

## ~~Open question for the brainstorm~~ RESOLVED → Hybrid (see Decisions Log)
Mechanism locked by research: keep splits inside one `mod { }` (bundle/ precedent → child files keep
private-field visibility, zero `pub(crate)` churn); the three coupled `Op` matches stay whole; native
registry index order frozen. Wave-0 = test-mod extraction (~3000+ lines, zero byte-identity risk).

## Formal Plan

Design spec: `docs/specs/2026-06-23-decomposition-milestone-design.md`. Axis = **Hybrid** (locked).
Each wave = its own commit; **gate before every commit**: `cargo build` → `cargo clippy --all-targets`
→ `cargo fmt --check` → `PHORGE_PHP=/stack/tools/phpbrew/php/php-8.4.22/bin/php PHORGE_REQUIRE_PHP=1
cargo test`. Order is lowest-risk-first; behavior-preserving throughout (`run≡runvm≡PHP 8.4`).

### Wave 0 — test-module extraction (zero behavior surface, ~3000+ lines reclaimed)
For each whale, move its `#[cfg(test)] mod tests` to a sibling `tests.rs` re-included via `mod tests;`.
- W0.1 `checker.rs` → `checker/tests.rs` (−3018, the −31% headline win)
- W0.2 the other whales' test mods (parser/compiler/transpile/interpreter/native/…) → sibling files
- Verify: tests still discovered & green (count unchanged).

### Wave 1 — cleanest cohesion splits (no exhaustive-match risk)
- W1.1 `native/` per-module split (freeze `build()` order + assert `CONSOLE_PRINTLN==0`)
- W1.2 `cli/explain.rs` + `cli/bench.rs` (self-contained, no AST coupling)
- W1.3 checker's 3 self-contained AST rewrites → `checker/{rewrite_html,rewrite_generics,rewrite_alias}.rs`
       + stateless helpers → `checker/common.rs` (−1436 from the whale, pure fns)

### Wave 2 — checker impl cohesion split (the #1 navigation-pain payoff)
Split the `impl Checker` whale into sibling `impl Checker` blocks: `resolve/collect/throws/program/
casing/stmt/expr/calls/assign/matches.rs`; keep struct + entry fns + diag/scope prims + info structs in
`checker/mod.rs`. Keep each exhaustive `Item`/`Stmt`/`Expr`/`Pattern` match whole in its method.
One commit per 1–2 clusters (small waves) so a byte-identity regression is bisectable.

### Wave 3 — front-end & loader splits
- W3.1 `parser/{exprs,stmts,items,types,patterns}` (multi-`impl`; cross-ref soft TokenKind dispatch)
- W3.2 `loader/{fs,symbols,resolve}` (keep `load_project` sequencing + exhaustive resolve walk intact)
- W3.3 `ast/{walk,classes}` (keep exhaustive free-var walkers together; re-export from `ast/mod.rs`)
- W3.4 `lexer/{mod,scan}` (lowest priority)

### Wave 4 — backend impl splits (highest care — the coupled trio lives here)
- W4.1 `compiler/{program,expr,stmt,binary,call,match,pattern,classes,control,types}` (keep `stack_effect`
       whole + `self.height` discipline)
- W4.2 `transpile/{program,types,stmt,expr,call,match,helpers}` (keep `emit_stmt` guard-arm order)
- W4.3 `interpreter/{stmt,expr,call,construct,match,scope}`
- W4.4 `vm/{exec,closure}` (keep `exec_op` whole in `exec.rs`; both `run`/`run_until` call it)
- `chunk.rs` / `dispatch.rs` stay single (shared contract / by-construct template).
- **After W4:** exhaustiveness smoke check — add a dummy `Op` variant, confirm all three coupled sites
  fail to compile, then revert.

### Wave 5 — close-out
Rebuild release binary; update `docs/ARCHITECTURE.md` module map + `CHANGELOG.md`; mark milestone CLOSED
in `docs/MILESTONES.md`; prune research raw/ if desired; final full gate.

### Acceptance criteria
- Every wave commit is green on the 8.4 floor (`run≡runvm≡real PHP`), clippy + fmt clean.
- No `_` wildcard introduced in any coupled exhaustive match; dummy-variant smoke check passes.
- `wc -l src/**/*.rs`: no single file > ~1500 lines (target; the whales are gone).
- Zero behavior change (no new examples needed — this is a refactor; existing 600+ tests are the proof).
- `docs/ARCHITECTURE.md` reflects the new module map.

### Progress
- [2026-06-23 15:50] APPROVED by developer: "Go — run the whole way" (autonomous; commit each green
  wave; pause only on a byte-identity break or design fork; no push).
- [2026-06-23 15:50] BASELINE established: **823 tests green**, clippy + fmt clean, PHP 8.4 floor.
- [2026-06-23 15:55] **W0.1 DONE** (`0d4bdd4`) — `checker.rs` (9786) → `checker/mod.rs` (6770) +
  `checker/tests.rs` (3016). 823 green, clippy + fmt clean. Mechanical, zero behavior surface.
- [2026-06-23 16:05] **W0.2 DONE** — 10 whales → `foo/mod.rs` + `foo/tests.rs`: parser/compiler/
  transpile/interpreter/native/vm/loader/ast/lexer/cli. ~5k test lines moved out of the impl files.
  823 green, clippy + fmt clean. (chunk/value/types/manifest/diagnostic/serve etc. left single — not
  whales, not slated for dir splits.) **Wave 0 COMPLETE.**

### Rollback
Each wave is one commit on `master`; a regressing wave is reverted with `git revert <sha>` (clean,
since each commit is self-contained and green). Nothing is pushed until you ask.
