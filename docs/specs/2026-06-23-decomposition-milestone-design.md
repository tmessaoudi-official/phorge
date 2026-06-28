# M-Decomp — Codebase Decomposition Milestone (Design)

> Status: DESIGN (approved axis; pending plan approval). Research: `docs/research/decomposition/`
> (SYNTHESIS.md + 4 raw maps). Successor milestone to M-RT (CLOSED 2026-06-23).

## 1. Goal & non-goals

**Goal.** Reduce navigation pain in the whale source files by behavior-preserving cohesion splits,
so a single feature touches fewer, smaller, well-named files. The byte-identity spine
(`run ≡ runvm ≡ real PHP 8.4`) is the verifier: a green refactor is correct by construction.

**Non-goals (explicit).**
- NO behavior change of any kind. No bug fixes folded in. No new features.
- NO OOP/SOLID/GoF dogma; NO visitor/strategy/`dyn` abstraction (these defeat exhaustiveness).
- NO performance change (hot paths stay `impl`-method calls on `&mut self`, not free fns by value).
- NOT a public-API change: the only cross-module surface is the existing `pub fn` set per file.

## 2. Axis decision (resolved by developer 2026-06-23, post-research)

**HYBRID — by-phase sub-split backbone + selective thin-dispatcher.** Pure by-construct REJECTED.

Rationale (`docs/research/decomposition/SYNTHESIS.md`): every production compiler (rustc, Go, TS,
Clang, GCC, nanopass) files by phase; the only by-construct example (Roslyn) only works because C# has
no exhaustive match (runtime-default dispatch). By-construct as a *backbone* would surrender Phorj's
#1 safety net (compile-time exhaustive `match`). The thin-dispatcher *technique* — move arm bodies out
while the `match` head stays whole in one file — preserves exhaustiveness and is applied **selectively**
where a phase's arm bodies are large and cleanly construct-shaped (e.g. `compiler/{binary,call,match}.rs`
as `impl` blocks). The shared by-construct kernels that already exist (`dispatch.rs`, `value::*`) are the
model and stay.

## 3. Hard invariants (every wave must preserve these)

1. **Byte-identity spine** — gate every step with the full differential incl. PHP oracle:
   `PHORJ_PHP=/stack/tools/phpbrew/php/php-8.4.22/bin/php PHORJ_REQUIRE_PHP=1 cargo test`
   (8.4 floor — the local php-master 8.6 is too permissive; see memory `php-transpile-floor-84`).
2. **Exhaustiveness stays compile-checked** — the three coupled `Op` matches
   (`vm::exec_op`, `chunk::validate`, `compiler::stack_effect`) and every backend `Expr`/`Stmt`/
   `Pattern`/`Item` match stay **textually exhaustive, no `_` wildcard**. Post-split smoke test:
   adding a dummy `Op`/`Expr` variant must still fail to compile in all coupled sites.
3. **One module, many `impl` blocks** — splits live inside one `mod foo { … }` (the `bundle/` precedent)
   so child files keep private-field visibility. NEVER promote a cluster to a separate top-level module
   (would force `pub(crate)` on every field). No struct-splitting (the whale structs' fields do not
   partition — verified for `Checker`'s 24 fields).
4. **Native registry index stability** — `Op::CallNative(idx)` bakes a slot; `CONSOLE_PRINTLN = 0`.
   `native::build()` stays the sole ordering coordinator; its `extend(...)` sequence is frozen.
5. **Stateful-method discipline** — `compiler` `self.height`/scratch-slot math
   (`m_slot = self.height - 1`) and the VM `run`/`run_until` shared `exec_op` must NOT be refactored
   to pass state by value or fork the dispatch loop (memories `null-op-scratch-slot`,
   `lambda-function-table-layout`).
6. **Position-sensitive guard arms** — `transpile::emit_stmt`'s `Return{Some(Match)}` /
   `VarDecl{init:Match|Propagate}` arms MUST keep their order before the generic arms.
7. **`unreachable!`/erasure-contract arms are load-bearing** (`Expr::Html`, `BinaryOp::Pipe`,
   `Expr::Propagate` in `emit_expr`) — they document the pipeline contract; keep them.
8. **Pass-ordering lives in callers** (`run_checker`/`check`, `cli::check_and_expand`,
   `loader::load_project`) — keep these orchestrators in their `mod.rs`; splitting method *bodies*
   does not change call order.

## 4. Mechanism

For a stateful whale (`Checker`, `Compiler`, `Vm`, …): keep `struct` + fields + entry/orchestration
fns + shared diagnostic/scope primitives + private info structs in `mod.rs`; move method *bodies*
verbatim into sibling files as additional `impl Type { … }` blocks grouped by cohesion. A mechanical
text move; the only failure mode is `use`-import drift, caught instantly by `cargo build`. For a
data/registry whale (`ast`, `native`): split by concept with the central table/walkers staying in
`mod.rs`.

## 5. Target layouts (from the raw maps — line ranges there)

- **`checker/`** (9786→~330 mod.rs): `mod`(struct+entry+diag/scope+info structs), `resolve`, `collect`,
  `throws`, `program`(driver+totality), `casing`, `stmt`, `expr`, `calls`, `assign`, `matches`,
  `common`(stateless helpers), `rewrite_html`, `rewrite_generics`, `rewrite_alias`, `tests`.
- **`compiler/`**: `mod,program,expr,stmt,binary,call,match,pattern,classes,control,types`.
- **`transpile/`**: `mod,program,types,stmt,expr,call,match,helpers`.
- **`interpreter/`**: `mod,stmt,expr,call,construct,match,scope`.
- **`vm/`**: `mod,exec`(keep `exec_op` whole),`closure`. **`chunk.rs` stays single** (shared contract;
  `validate` next to `Op`). **`dispatch.rs` stays** (by-construct template).
- **`native/`**: `mod`(build/registry/index/`CONSOLE_PRINTLN`) + `{console,math,text,file,bytes,html,
  list,map,set}.rs` (one factory each).
- **`parser/`**: `mod`(struct+lex/keyword)+`{exprs,stmts,items,types,patterns}` (multi-`impl`; soft
  TokenKind dispatch — cross-reference entry points).
- **`ast/`**: `{mod,walk(exhaustive free-var walkers),classes}` (+ optional data split); re-export.
- **`loader/`**: `{mod(keep load_project sequencing),fs,symbols,resolve(keep exhaustive walk whole)}`.
- **`cli/`**: `mod` + `{explain(168-arm table),bench}` first; per-command later.
- **`lexer/`**: `{mod,scan}` — lowest priority.

## 6. Verification

TDD-for-refactor = the differential harness already IS the test. Per wave: `cargo build` →
`cargo clippy --all-targets` → `cargo fmt --check` → full `cargo test` with `PHORJ_REQUIRE_PHP=1` on
the 8.4 floor → commit. Plus the per-milestone exhaustiveness smoke check (§3.2) once after the coupled
backends are split. Release binary rebuilt at the end (memory `build-binary-after-each-feature`).

## 7. Risks & mitigations

- **Silent byte-identity break from a moved stateful arm** → mitigated by the 8.4-floor differential
  gate every commit + the §3.5/§3.6 rules; small waves (one whale's one cluster per commit).
- **Native index drift** → §3.4; assert `CONSOLE_PRINTLN == 0` stays in `build()`.
- **Reviewer transcription error** → mechanical verbatim moves; `cargo build` + clippy catch drift.
- **Scope creep into "while I'm here" fixes** → forbidden by §1 non-goals; any real bug found is logged
  to KNOWN_ISSUES and fixed in a SEPARATE non-refactor commit.

## 8. Open sub-decisions (deferred to execution, low-stakes)
- `checker/calls.rs` (~810) may further split into `calls`+`members` (~400 each) — pure navigation call,
  decide when editing it.
- Whether `common.rs` exists or folds into `mod.rs` per whale — decide per file by helper count.
