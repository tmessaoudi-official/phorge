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

- [2026-06-23 17:30] AGREED (test structure — developer pushed for full test decomposition + isolation):
  **tests are co-located sibling files declared as CHILD modules**, mirroring the source decomposition;
  one test file per concept (`math.rs` ↔ `math_tests.rs` via `#[cfg(test)] #[path] mod tests;`).
  **Tests-outside-`src/` (a `tests/unit/` parallel tree) was tried and REJECTED**: empirically it forced
  13+ encapsulation holes (private modules/methods/fields → `pub(crate)`: `Parser::peek`/`advance`,
  `Checker::resolve_type`/`.errors`, native submodules…) + 313 import fixes — it *destroys* the isolation
  the developer wants. Child-module siblings keep every module a sealed black box (private internals stay
  private; **zero holes**) AND mirror the tree AND co-locate each concept's tests. This is Rust best
  practice (the Book's `#[cfg(test)] mod tests` private-access model), just with the body in a sibling
  file. Rule going forward: **when a module's source splits, its tests split alongside, one per concept.**

- [2026-06-23] AGREED (W2 done `558baf5`): the `impl Checker` whale split into 11 sibling cluster
  files (resolve/collect/throws/program/casing/stmt/expr/calls/assign/matches + common.rs);
  mod.rs 5678→454. **Mechanism refinement:** moved inherent *methods* take `pub(super)` (children
  aren't visible to ancestors/siblings — unlike struct *fields*, which descendants see for free);
  common helpers re-exposed module-wide via one `use common::*;` in mod.rs (picked up by each
  cluster's `use super::*`). Zero crate-public widening. 823 tests green on PHP-8.4 oracle.
- [2026-06-23] AGREED (W2b test structure — developer leaned Option 2, I challenged + concurred):
  **checker tests split BY LANGUAGE FEATURE, not by source cluster.** Rationale: the checker source
  is by-*phase* (exhaustive-match coupling forbids by-construct), but tests are feature-centric
  integration tests through the public `check()` — a feature is checked *across* clusters, so it has
  no honest single-cluster home. By-feature is how the tests already self-name and how a feature is
  developed/navigated (the rustc model: phase-organized source, feature-organized tests). This
  **deliberately does NOT mirror the source clusters** — the "tests mirror source" rule (forged on
  `native`, where source WAS by-feature) is overridden here because source-by-phase + tests-by-feature
  is the honest structure. Taxonomy documented; re-bucketing any test is free (all run via `check()`).

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

- [2026-06-23 16:25] **W1.1 DONE** — `native/mod.rs` (1380) → 191 (header/types/console/parg/build/
  registry/index) + 8 per-leaf submodules (`math/text/file/bytes/html/list/map/set.rs`). `build()` stays
  the sole ordering coordinator (`CONSOLE_PRINTLN==0` assert intact). Helpers widened to `pub(super)`;
  tests import the submodules. 823 green.
  **GOTCHA (recurs every impl split):** `cargo build` does NOT compile test modules — a moved private
  helper that a `tests.rs` calls only errors under `cargo clippy --all-targets` / `cargo test`. Always
  gate with clippy --all-targets, never `build` alone. Fix pattern: `pub(super)` the helper + import the
  submodule in tests.rs; trim globs to only-used (deny-warnings treats unused import as error).

- [2026-06-23 17:35] **W1.1b DONE** — test-structure showcase on `native`: split `native/tests.rs`
  (658) into per-submodule sibling child-module files (`math_tests.rs`…`set_tests.rs`) + kept
  console/registry/import_map tests in `native/tests.rs` (70). Helpers reverted `pub(super)`→**private**
  (tests are children again) — **zero `pub(super)` in native**, fully sealed. 823 green, clippy
  --all-targets + fmt clean. This is the template for every later wave's tests.

- [2026-06-23 18:10] **W1.3 DONE** — extracted checker's 3 self-contained AST-rewrite passes to sealed
  sibling modules: `rewrite_html.rs`/`rewrite_generics.rs`/`rewrite_alias.rs` (~1090 lines out);
  `checker/mod.rs` 6769→5678. Re-exported (`pub use`) so `checker::{resolve_html,erase_generics,
  expand_aliases}` callers are unchanged. 823 green.
  **GOTCHA (bit me 3×: cli, +2 here) — DOC-COMMENT BOUNDARIES:** a `pub fn`'s doc comment sits ABOVE
  the `fn` line. Extracting `[fn_line .. next_fn_line-1]` (a) leaves THIS fn's doc stranded in the source
  (dangling `///` → "expected item after doc comment", OR silently mis-documents the next item) and
  (b) grabs the NEXT fn's doc into this file. When splitting by line range, START the range at the doc
  comment (scan up for the contiguous leading `///` block), not the `fn` line. CRITICAL for W2 (checker
  impl split has ~110 doc'd methods).

- [2026-06-23] **W2 DONE** (`558baf5`) — `impl Checker` whale → 11 sibling cluster files
  (resolve/collect/throws/program/casing/stmt/expr/calls/assign/matches + common.rs); `checker/mod.rs`
  9786(orig)→454. Built with a Rust-lexer-aware extractor (method-end = first `^    }`; doc-block
  scan-up); moved methods `pub(super)`, helpers re-exposed via `use common::*;`. 823 green PHP-8.4.
- [2026-06-23] **W2b DONE** (`bf43ed4`) — flat 2999-line `checker/tests.rs` → `tests/` module, 18
  by-feature files + `support.rs` (helpers + const fixtures `pub(super)`, `pub(super) use super::super::*`).
  251 checker tests preserved, 823 green. **W2 (the headline) is COMPLETE.** Two extractor gotchas hit +
  fixed: (1) helper `pub(super)` substitution must target the fn line inside the doc-led block, not `^fn`;
  (2) the script must also relocate col-0 `const` fixtures (SHAPE/GREETER/OPTION/…), not just `fn`s.

- [2026-06-23] **W3.1 DONE** (`94bb02b`) — `impl Parser` whale → exprs/stmts/items/types/patterns
  cluster files; `parser/mod.rs` 1934→199 (struct + token primitives + 2 free helpers kept). 823 green.
- [2026-06-23] **W3.1b DONE** (`9b072bd`) — `parser/tests.rs` (1392) → `tests/` by construct
  (exprs/stmts/items/types/patterns + support.rs), **mirroring the source** (parser tests ARE
  construct-shaped, unlike checker's). 75 tests preserved, 823 green.
- [2026-06-23] **W3.3 DONE** (`4481093`) — `ast/mod.rs` (1465→682): free-fn analyses → `walk.rs`
  (free_vars + collect_free_*) + `classes.rs` (class graph queries), re-exported so callers unchanged;
  type defs stay. ast/tests.rs kept flat (small); one walk internal made `pub(super)`. 823 green.

**W3 REMAINING:** W3.2 loader (1219, *under* the 1500 target — fs/symbols/resolve cohesion split,
free-fn+struct heavy) · W3.4 lexer (621 — **under target, single cohesive scanner → SKIP/keep**).
**W4 backends (HIGH care):** compiler 3.3k / transpile 2.8k / interpreter 2.1k / vm — the coupled
`Op` trio (`vm::exec_op` / `chunk::validate` / `compiler::stack_effect`) MUST stay whole; `self.height`
discipline; `emit_stmt` guard-arm order. **W5 close:** rebuild release binary, update
ARCHITECTURE/CHANGELOG/MILESTONES, dummy-Op smoke check.

### Status checkpoint (2026-06-23 ~17:45)
**DONE + committed green (823 tests, clippy --all-targets + fmt, PHP 8.4 floor):**
- W0.1 `0d4bdd4` · W0.2 `51adb06` (test extraction) · docs `49d154b`
- W1.1 `3725649` (native split) · W1.1b `d3efa34` (native per-submodule sealed test files — the pattern)
- W1.2 `79164e4` (cli explain/bench split)

**PATTERN LOCKED (apply to every remaining wave):** split source to cohesion sub-modules inside one
`mod`; split that module's tests into per-concept sibling `*_tests.rs` files declared as `#[cfg(test)]
#[path] mod tests;` children → sealed modules (private internals), zero `pub(crate)`/`pub(super)` holes,
tests mirror source. Gate each wave: build → fmt → clippy --all-targets → `PHORGE_REQUIRE_PHP=1` test.

**NEXT (in order):**

**W2 — checker impl-cluster split (`checker/mod.rs` 5678 → ~330 core + cluster files). THE headline.**
Mechanism (validated by W1.1/W1.3 — NO widening needed):
- The whole `impl Checker { … }` is ONE block, lines **177–5324** (unchanged by W0/W1.3 — those touched
  ≥5328). Method doc comments sit INSIDE the impl above each `fn`.
- For each cluster, cut its contiguous methods out of the big impl and paste into `checker/<cluster>.rs`
  as `use super::*;\n\nimpl Checker {\n <methods>\n}`. **No `pub` changes**: child modules of `checker`
  see `Checker`'s private fields AND private methods (struct defined in the parent module → fields/
  methods visible to all descendants). Verified by the bundle/ + native/ precedents.
- **DOC-BOUNDARY RULE (the gotcha that bit 3×):** start each method range at its leading `///` block,
  not the `fn` line; end at the line before the NEXT method's `///`. Otherwise a dangling/mis-attached
  doc → build error or wrong docs.
- Cluster map + exact ORIGINAL line ranges in `docs/research/decomposition/raw/checker-map.md` §1
  (still valid for 177–5324): `resolve` 303–585 · `collect` 586–1672 · `throws` 1400–1527 · `program`
  1922–2100+2425–2703 · `casing` 2101–2424 · `stmt` 2704–3038+loops · `expr` 3039–3665 · `calls`
  3666–4477 · `assign` 4478–4956 · `matches` 5053–5327. (Ranges interleave — cut bottom-up / re-grep
  each boundary live, don't trust stale numbers after the first cut.) Keep in `mod.rs`: struct+24 fields
  (12–176), info structs, `new`, diagnostic ctors (`err`/`err_coded`/`warn_coded`/`err_assign`), scope
  prims (`push_scope`/`declare`/`lookup`…), and the entry fns `run_checker`/`check`/`check_resolutions`
  (5432–5467, pass-orchestration stays in one obvious place). Stateless free helpers 5468–5660
  (`levenshtein`/`is_camel`/`apply_subst`/`ty_has_param`/`is_builtin_type_name`…) → `checker/common.rs`
  as `pub(super) fn` (consumed by clusters via `use super::common::*` or `super::`).
- **Each exhaustive `match` stays whole in its method** (lands in exactly one cluster — they're already
  separate methods; see checker-map §3). Verify post-split: a dummy `Item`/`Stmt`/`Expr` variant still
  fails to compile.
- **Tests:** split the giant `checker/tests.rs` (~3016 lines) into per-cluster sealed child files
  alongside (the W1.1b pattern) — OR, simpler first pass, keep one `checker/tests.rs` (it goes through
  the public `check()`/`prog()`, so it needs no per-cluster split for access) and refine later. Decide
  at execution; the access works either way (tests are children of `checker`).
- Gate every 1–2 clusters: build → fmt → clippy --all-targets → `PHORGE_REQUIRE_PHP=1` test → commit.

**Then:** W3 parser/loader/ast/lexer · W4 backends (compiler/transpile/interpreter/vm — the coupled
`Op` trio `exec_op`/`validate`/`stack_effect` stays whole, `self.height` discipline, `emit_stmt`
guard-arm order) · W5 close (rebuild release binary, update ARCHITECTURE/CHANGELOG/MILESTONES).
- (follow-up, low value) cli per-submodule test split (bench/explain → sealed children).

### Rollback
Each wave is one commit on `master`; a regressing wave is reverted with `git revert <sha>` (clean,
since each commit is self-contained and green). Nothing is pushed until you ask.
