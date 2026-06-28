# ADR-0001: Three backends as free functions — no shared IR, no `Backend` trait

- **Status:** Accepted (2026-06-19)
- **Deciders:** project author
- **Fuller design:** `docs/ARCHITECTURE.md` §"Backends today vs. planned"; GA roadmap
  `docs/plans/2026-06-19-phorj-ga-roadmap.plan.md` Decisions Log; ecosystem spec decision E-1.

## Context

Phorj has three backends — the tree-walking interpreter (`run`), the bytecode compiler + stack VM
(`runvm`), and the Phorj→PHP transpiler — and they all consume the **same validated AST**. Today
each is a plain free function dispatched by a string `match` in `main.rs`
(`cmd_run` / `cmd_runvm` / `cmd_transpile`); `grep 'trait ' src/` returns zero. Two unifications are
perennially tempting: a shared intermediate representation (`src/ir.rs`) that all backends lower
through, and a `Backend` trait that abstracts "execute a program."

## Decision

Keep the three-backend model with **no shared IR and no `Backend` trait**. Each backend lowers the
AST directly. Any shared-IR rewrite, and the `Backend` trait, are **deferred** until feature velocity
actually demands them — by the Rule of Three, the trait is justified only once the 4th backend
(`phg build`, M2.5) proves the abstraction. The *unifier* is not a type; it is the **byte-identity
differential spine** (`run ≡ runvm ≡ php`, enforced in `tests/differential.rs`).

## Consequences

- The correctness contract, not a shared abstraction, keeps the backends honest. A divergence is a
  failing test, not a type error.
- **Known evolution tax (accepted):** adding an `Op` variant requires editing three coupled matches
  in the same change — `vm.rs::exec_op`, `chunk.rs::BytecodeProgram::validate`, and
  `compiler.rs::stack_effect`. ADR-aligned mitigation: the M9 `Op` descriptor table (`Op::meta()`)
  shrinks this surface **without** introducing a shared IR.
- No premature abstraction: the three backends remain free to evolve independently (the transpiler's
  PHP concerns never leak into the VM's stack-effect logic).

## Alternatives rejected

- **A shared IR / `Backend` trait now** — premature (Rule of Three unmet); would couple three
  backends that today evolve independently, and buy abstraction the differential spine already
  provides for free.
