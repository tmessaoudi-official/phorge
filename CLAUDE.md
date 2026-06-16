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
evidence).

**M2 is now formally CLOSED** (`docs/MILESTONES.md`, `33c6b78`): all design §10 success criteria met
(backends byte-identical, quality gate green; the mark-sweep GC criterion was revised — `Rc`/`Drop`
reclaims the immutable+acyclic heap fully, tracing GC deferred to M3). A **full-coverage example set**
also landed (`docs/specs/2026-06-16-examples-coverage-design.md`): four real-world programs
(`examples/realworld/`), six focused guide programs (`examples/guide/`), and the Phorge→PHP transpile
bridge (`examples/transpile/`) — `tests/differential.rs` now **globs `examples/**/*.phg`** so every
example (and any added later) is byte-identity-gated automatically; `examples/README.md` is the
living surface showcase. **Gotcha:** zero-payload enum variants need call form `V()` both to
construct AND in a `match` pattern (bare `V =>` is a silent catch-all binding).

**M2.5 `phorge build` (standalone executables) — Phase 1 COMPLETE**
(`docs/specs/2026-06-16-m2.5-phorge-build-design.md`, `docs/plans/2026-06-16-m2.5-phase1-build-linux-gnu.md`):
`phorge build foo.phg` produces a standalone host (`x86_64-linux-gnu`) executable that runs on the VM
with no Phorge install — `src/bundle.rs` embeds the program **source** in a `.phorge` ELF section
(versioned CRC-guarded container + hand-rolled, zero-dep ELF64 reader), and `main()` self-detects +
runs the payload at startup. `tests/build.rs` extends the parity spine to distribution (built binary
byte-identical to `runvm`). v1 limits: host-only, argv ignored, no custom exit code, source
recoverable. The design is the same section+container mechanism as the cross-OS end state.

**Next (locked sequence): M2.5 Phase 2** — cross-targets via zig (the C/linker driver) + PE/Mach-O
reader arms in `bundle.rs` + per-target stub fetch/cache (cache key **must** include the phorge
binary hash); then **Phase 3** (CI stub registry + signing/notarization, `rcodesign` from Linux);
then **M3** (grow the language: indexing, Map/Set, null/optionals, `|>`, exceptions, mutation —
mutation finally motivates the real tracing GC).

Project invariants and layout now live in-repo: **`docs/INVARIANTS.md`** (the load-bearing
correctness rules — read before touching backends, value kernels, or the `Op` set) and
**`docs/ARCHITECTURE.md`** (pipeline + module map). `CHANGELOG.md` tracks milestone progress.
