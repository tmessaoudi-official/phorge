# CLAUDE.md — phorge

Phorge is a statically-typed, PHP-inspired language implemented in Rust (edition 2021, std-only,
no external crates): lexer → parser → type-checker → tree-walking interpreter + Phorge→PHP
transpiler (M1) + bytecode compiler + stack VM (M2). Single developer, commits direct to `master`,
remote is GitHub (`tmessaoudi-official/phorge`).

This sub-project lives under `/stack/projects/` and is handled with the global reasoning framework
(`~/.claude/CLAUDE.md`). It is NOT `/stack` infrastructure — do not route work here to
`global-stack-lead-dev`. The parent `/stack/CLAUDE.md` is excluded via
`/stack/projects/.claude/settings.json` `claudeMdExcludes`.

## Git autonomy (overrides global Rule 10 — authorized by the developer, 2026-06-16)

Autonomous `git add` and `git commit` are **authorized** in this project: stage and commit ready
work without asking, when tests pass (`cargo test`) and the quality gate is clean
(`cargo clippy --all-targets`, `cargo fmt --check`). This mirrors the `/stack` auto-commit
precedent and overrides global Rule 10 **for this project only**.

Scope and limits:
- **Authorized:** `git add`, `git commit` (descriptive messages — `feat:`/`fix:`/`docs:`/`test:`
  prefixes, matching existing history; no `Co-Authored-By` line).
- **NOT authorized without an explicit request:** `git push` (and any force-push — `push --force`
  remains denied globally).
- Commit only green, self-contained changes. Do not commit a broken build or red tests.
- If the safety classifier blocks a specific `git commit`, present the exact command for manual
  execution rather than retrying — do not attempt to bypass it.

## Toolchain & gate

`export PATH=/stack/tools/cargo/bin:$PATH`. Baseline: 243 tests green, clippy clean (pedantic off).
The differential harness (`tests/differential.rs`) is the correctness spine — `run` and `runvm`
must stay byte-identical. Adding an `Op` variant requires extending three exhaustive matches in
the same commit: `src/vm.rs` `exec_op`, `src/chunk.rs` `BytecodeProgram::validate`, and
`src/compiler.rs` `stack_effect`. `phorge bench <file>` measures the two backends (median-of-N,
output-identity gated) — run it for a before/after number before any perf change.

## Active plan

The M2 P3.5 hardening roadmap (`docs/plans/2026-06-16-m2-p3.5-hardening-roadmap.md`, Waves 0–4) is
**complete**. **M2 P4 is COMPLETE** (`docs/plans/2026-06-16-m2-p4-classes-enums-match.md`): P4a
(enums + `match`), P4b (classes + constructor promotion + field reads), and P4c (methods + `this`)
all landed — **`runvm` now covers the full M1 language surface** and `examples/grades.phg` runs
byte-identically on both backends (VM ≈3.2×). The VM object model is value-native (reuses the shared
`Value::Enum`/`Instance`). **M2 Wave 4 is COMPLETE**
(`docs/plans/2026-06-16-m2-wave4-compiler-types.md`): the compiler's operand-type inference is now
class-aware (`enum CTy { Int, Float, Class(String), Other }` + a recursive `ctype(&Expr)` resolver),
so a field read on an arbitrary instance (`p.x + 1`), a method-call result (`c.get() + 1`), a nested
`a.inner.x`, and a class-typed enum payload all compile and run byte-identically — closing the last
known `run`↔`runvm` parity gaps. The only remaining coarse-type note is the deliberately
out-of-M1-surface `Index` (`xs[i]` — rejected on both backends).

**M2 P5a is COMPLETE** (`docs/specs/2026-06-16-m2-p5-object-model-design.md`,
`docs/plans/2026-06-16-m2-p5a-rc-shared-heap.md`): heap objects are now **`Rc`-shared**
(`Value::Instance`/`Enum`/`List`), so the `Op::GetLocal` hot path is a refcount bump, not a deep
clone — object-heavy VM run **1537 ms → 634 ms (2.4×)**, recovering the VM's advantage to 9.35× (≈
scalar's 10.92×). **There is no tracing GC and none is planned for M2:** the M1 heap is immutable +
acyclic, so `Rc`/`Drop` reclaims completely (a tracing collector is deferred to **M3**, when
mutation could create cycles). **Phase B** (slot-indexed `Vec` field layout, replacing the
per-instance `HashMap`) is **bench-gated and unopened** — after P5a the object path is within ~15% of
scalar's advantage, so field access no longer dominates; the slab-arena was rejected (no locality
evidence). Remaining M2 work is essentially the **M2 success-criteria sweep / M2.5 `phorge build`**.

Project invariants and layout now live in-repo: **`docs/INVARIANTS.md`** (the load-bearing
correctness rules — read before touching backends, value kernels, or the `Op` set) and
**`docs/ARCHITECTURE.md`** (pipeline + module map). `CHANGELOG.md` tracks milestone progress.
