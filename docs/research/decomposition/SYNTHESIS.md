# Decomposition Research — Synthesis (2026-06-23)

Four read-only research agents (3 code-mapping + 1 prior-art). Raw reports in `raw/`.
All four converged on the **same** answer.

## The converged finding

**By-phase sub-split is the backbone; the thin-dispatcher is a selective technique, not the structure.
Pure by-construct is rejected.**

### Evidence
- **Prior art is unanimous** [Verified, raw/prior-art.md]: rustc (`rustc_parse`/`hir_typeck`/`mir_*`,
  typeck sub-split into collect/coherence/check/writeback), Go, TypeScript, Clang, GCC — **every
  production compiler files by PHASE, not construct.** Nanopass is by-phase taken to the extreme (50+
  tiny passes) and proves the whale dies by *sub-cutting*, not axis-flipping.
- **The one by-construct counter-example proves the trap**: Roslyn's `Binder` splits by construct
  (`Binder_Expressions.cs`…) — but only works because C# has **no exhaustive match**; its dispatch
  `default` throws at *runtime*. Phorj's compile-time exhaustiveness is exactly what by-construct gives up.
- **This is the Expression Problem.** Rust `enum`+exhaustive `match` is column-cheap (adding a phase is
  cheap, adding a construct is the expensive-but-safe op). The by-construct vision optimizes the axis the
  language's core safety mechanism structurally fights.
- **The thin-dispatcher DOES preserve exhaustiveness** — you move only the arm *bodies* out; the match
  head stays whole in one file. Cost: a per-construct file must import every phase's types (the coupling
  tax) and, for the backends, would force a `pub(crate)` explosion + hot-path deopt
  [Verified, raw/backends-map.md].

### Mechanism (how to split safely in Rust)
- Keep each split **inside one `mod { }`** so child files keep private-field visibility — the existing
  `bundle/` precedent. Multiple inherent `impl` blocks across sibling files; zero visibility changes.
- The **three coupled `Op` matches stay whole** (`vm::exec_op`, `chunk::validate`,
  `compiler::stack_effect`) — all exhaustive, no `_`, ~60 variants. Same for backend `Expr`/`Stmt`
  matches and ast/loader walkers.

## Per-whale findings
- **checker.rs (9786)** — one monolithic `impl Checker` (already a thin-dispatcher internally) + 3
  AST-rewrite free fns + 3018-line test mod. Splits into sibling `impl Checker` files
  (`resolve/collect/stmt/expr/calls/assign/matches/casing/throws` + `rewrite_*` + `common`).
  **Hard blocker:** the 24 struct fields do NOT partition (hot fields thread everywhere) → cannot split
  the struct, must stay one module. Lowest-risk first move: extract the test mod (−31%).
- **backends** — by-phase sub-split is strongly the natural fit (private state is phase-bound; a
  by-construct "enums.rs" would reach into 4 different structs at once = soup). Thin-dispatcher NOT
  feasible for the coupled trio. `dispatch.rs`/`value::*` are the existing clean by-construct kernels.
- **native.rs (2053)** — splits cleanest, per stdlib module (`native/{console,math,text,…}.rs`), BUT
  **registry index order is a hard invariant** (`Op::CallNative(idx)` bakes the slot; `CONSOLE_PRINTLN=0`):
  `build()` must remain the sole ordering coordinator with a frozen `extend` sequence.
- **Universal Wave-0 win** — extracting inline `#[cfg(test)] mod tests` reclaims ~3000+ lines across the
  whales with **zero** byte-identity risk (the safest possible first slice).

## Tension to resolve with the developer
The developer's stated vision was **by-construct** ("a file for `for`, a file for `while`"). The
evidence says by-construct as the *backbone* is the path no production compiler takes and that gives up
Phorj's exhaustiveness safety net. The honest recommendation is the **hybrid**: by-phase sub-split
backbone + thin-dispatcher applied selectively where arm bodies are large and cleanly construct-shaped.
This is a genuine fork → developer decision.
