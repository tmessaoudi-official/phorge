# Phorge M2 — Bytecode Compiler + Stack VM + Mark-Sweep GC — Design

> Milestone M2 of the roadmap (`2026-06-15-phorge-language-design.md` §5, decision #24):
> the "rival Java" phase — `AST → bytecode → stack VM → GC`. This document is the frozen
> design for M2; the implementation plan lives in `docs/plans/`.

## 1. Goal & Non-Goals

**Goal.** Execute the *current* M1 language surface on a hand-written bytecode virtual
machine with a real (tracing) garbage collector, implemented in Rust. No new language
features — M2 is a runtime-architecture and learning milestone, not a surface change.

**Non-goals (explicitly out of M2):**
- New language features (exceptions, `Map`/`Set`, null safety, traits, overloading,
  `|>`, value types, operator overloading, sized ints, `decimal`) → **M3** ("grow the
  language"), implemented **once**, on the VM.
- Single-binary bundling (`phorge build` → standalone exe) → **M2.5**, the immediate
  next slice (depends on a working VM; it is packaging, not VM work).
- Concurrency model (async/await vs goroutine+channels) → parked (not in the current
  surface; revisit in M3 before the "Go model" server work).
- Native-AOT / ownership / no-GC → **v2**.
- Diagnostics column-semantics (UTF-8 byte-column bug, §8 of the language spec) → parked
  for the future LSP/diagnostics layer.

## 2. Why this shape (rationale)

- **VM before language enrichment.** *Crafting Interpreters* (cited prior art) keeps the
  bytecode VM running the *same* language as the tree-walker, so features are implemented
  once on the VM rather than twice (tree-walker then VM). Enriching the tree-walker first
  is throwaway work.
- **Tree-walker retained as a differential oracle.** The transpiler milestone proved that
  round-trip-against-a-real-runtime catches semantic bugs unit tests miss. The VM gets the
  same net for free: every program must produce byte-identical stdout under the VM and the
  interpreter.

## 3. Architecture

```
source → lex → parse → check        (existing M1 front-end, unchanged)
              ↓ (typed, checked AST)
            compile  → Chunk         (NEW: AST → bytecode emitter pass)
              ↓
              VM      → stdout        (NEW: stack machine + arena heap + mark-sweep GC)
```

The compiler is a dedicated pass over the **type-checked** AST (decoupled; relies on the
checker's guarantees but is *not* handed an annotated/typed AST — the checker validates without
attaching types, so the compiler re-derives the little it needs via `num_ty`/`TyTag`) — **not** a
clox-style fused parse+compile, because Phorge already
has a separate parser/AST.

## 4. Bytecode format

```rust
struct Chunk { code: Vec<Instr>, consts: Vec<Value>, lines: Vec<u32> }
enum Instr { Const(usize), Add, Sub, /* … */ Jump(usize), Call(u8), Return, /* … */ }
```

**Typed `enum Instr`, not raw `Vec<u8>`.** Rationale: consistent with the `enum Value`
choice; no `unsafe` byte encode/decode; and every unit of VM learning — dispatch loop,
stack discipline, jump-by-offset, constant pool, call frames — is identical to a raw-byte
VM. Raw-byte encoding is parked as a potential v2 perf pass. `consts` is the constant pool;
`lines` maps each instruction to a source line for runtime-error reporting.

## 5. Instruction set (covers the current surface)

| Group | Instructions |
|---|---|
| Constants/literals | `Const(idx)`, `True`, `False` |
| Arithmetic | `AddI/AddF`, `SubI/SubF`, `MulI/MulF`, `DivI/DivF`, `RemI/RemF` (type-specialized; checker guarantees operand types) |
| Comparison | `EqI/EqF/EqBool/EqStr`, `Lt`, `Gt`, `Le`, `Ge`, `NotEq` |
| Unary | `Neg`, `Not` |
| Locals | `GetLocal(slot)`, `SetLocal(slot)` |
| Control flow | `Jump(off)`, `JumpIfFalse(off)`, `Loop(off)` |
| Functions | `Call(argc)`, `Return` |
| Collections | `MakeList(n)`, `Index`, `IterNext` (for-in) |
| Objects | `MakeInstance(class)`, `GetField(idx)`, `CallMethod(idx, argc)` |
| Enums/match | `MakeEnum(tag, n)`, `MatchTag(tag)` (variant test + payload bind) |
| Strings/IO | `Concat(n)` (interpolation), `Print` (`println` builtin) |

(Final opcode list is refined during implementation; this is the design-level inventory.)

**As-built errata (M2 P1–P3, authoritative source `src/chunk.rs`):** the implemented `Op` enum
diverges from this design-level inventory —
- Comparison/equality collapsed to runtime-generic `Eq`/`Ne`/`Lt`/`Gt`/`Le`/`Ge`; there is no
  type-specialized `EqI/EqF/EqBool/EqStr`, and `NotEq` is `Ne` (decision P2-8).
- No `Loop(off)` — loops use absolute `Jump(target)` (decision P2-2). `True`/`False` are folded
  into `Const(idx)` over bool constants.
- `for…in` lowers to `Len` + `Index` + `Jump` rather than a dedicated `IterNext`. `Pop` and `Len`
  exist in the implementation but are absent above; `Print` carries an arg count (`Print(n)`).
- Object/enum/match ops landed in **M2 P4** as `MakeEnum`/`MatchTag`/`GetEnumField`/`MatchFail`
  (P4a), `MakeInstance`/`GetField` (P4b), and `CallMethod` (P4c). **Object-model erratum (decision
  P4-1, authoritative):** instances and enum values are **value-native** — the VM reuses the
  shared `value::Value` (`Instance`/`EnumVal`) directly on the operand stack, clone-on-use,
  mirroring the interpreter. There is **no arena and no heap handle** (so §6's "referenced by heap
  handle" and §7's arena/`Vec<Obj>` are superseded for P4): the language has no field mutation, so
  value semantics are parity-correct, and an arena/handle model is a *perf* change deferred to a
  bench-gated P5 milestone (`docs/INVARIANTS.md` bench-before-perf), not a correctness requirement.

## 6. VM execution model

- **Value stack** (`Vec<Value>`) — operands and locals.
- **Call-frame stack** — each frame is `{ function ref, ip, slot_base }`; locals are a
  window into the value stack starting at `slot_base` (clox-style). `Return` pops the frame
  and the slot window, leaving the return value on the caller's stack.
- **`enum Value`** — scalars (`int` i64, `float` f64, `bool`) inline; compound objects
  (`List`, class instances, enum instances, strings) referenced by **heap handle**.

## 7. Heap & garbage collection

- **Heap** = arena `Vec<Obj>`; references are integer handles (indices). No `unsafe`,
  borrow-checker-friendly. Objects: `List`, `Instance` (class), `EnumValue` (tag + payload),
  `Str`.
- **Allocation** into the arena happens from the moment compound objects exist (P4). The
  **collector** is a later step (P5) — allocate-first, collect-later (clox order).
- **Mark-sweep:** roots = value stack + globals + all live call frames. Mark traces
  reachable handles (objects hold handles to children → transitive mark). Sweep frees
  unmarked arena slots onto a free-list for reuse.
- **Trigger:** an allocation-count/bytes threshold that grows adaptively after each
  collection (avoid collecting every allocation; avoid unbounded growth).

## 8. CLI integration

`phorge run` stays the **tree-walker** for the duration of M2. Add **`phorge runvm <file>`**
(compile → VM). The differential test harness runs both and asserts identical stdout.
After M2 proves out, `run` may default to the VM (with a `--treewalk` escape hatch) — a
post-M2 decision, not part of this milestone.

## 9. Internal plan sequence (each step runnable before the next)

| Step | Delivers | Runnable proof |
|---|---|---|
| **P1** | `Chunk` + `enum Instr` + VM dispatch loop + value stack | VM runs a *hand-built* chunk (arithmetic + `Print`) |
| **P2** | Compiler: expressions, statements, locals, `if`, `for`, blocks | simple programs run; **differential** vs tree-walker |
| **P3** | Functions: call frames, `Call`/`Return`, recursion | `fib` runs on the VM; differential |
| **P4** | Classes (instances/fields/methods) + enums + `match`; arena allocation introduced | `Shape`/`area` sample runs; differential |
| **P5** | Mark-sweep collector (threshold, roots, mark, sweep) | GC stress test reclaims memory; no leaks of unreachable objects |
| **P6** | Strings/interpolation/`println` parity; full differential sweep; VM-vs-tree-walker timing sanity | every example + fixture byte-identical under `runvm` and `run` |

## 10. Success criteria (M2 done)

1. Every `examples/*.phg` and `tests/fixtures/*.phg` produces **byte-identical** stdout
   under `phorge runvm` and `phorge run`.
2. The mark-sweep collector reclaims unreachable objects under a stress test (measured
   heap shrink), with no use-after-free and no panics.
3. `cargo test` green (incl. a differential harness), `cargo clippy --all-targets` clean.

Then **M2.5** adds `phorge build <file>` — embed the compiled bytecode into a copy of the
runtime binary (bun-compile style) to produce a standalone executable.

## 11. Decisions Log

| # | Decision | Choice | Rationale |
|---|---|---|---|
| M2-1 | M2 vs M3 ordering | Bytecode VM first; language enrichment = M3 | Features implemented once (on the VM), not twice; Crafting-Interpreters path |
| M2-2 | Tree-walker fate | Kept as a differential-testing oracle | Round-trip-vs-real-runtime caught transpiler bugs unit tests missed |
| M2-3 | Bundling | Deferred to M2.5 (committed next slice) | Depends on a working VM; it is packaging, not VM learning; crisp M2 done-ness |
| M2-4 | Heap / GC | Handle/arena heap + mark-sweep | Real tracing GC, no `unsafe`, idiomatic Rust |
| M2-5 | Value representation | `enum Value` (tagged union) | Simple, safe; NaN-boxing parked for v2 |
| M2-6 | Compiler structure | AST → bytecode emitter pass | Over the type-checked (un-annotated) AST; re-derives types via `num_ty`/`TyTag`; decoupled |
| M2-7 | Instruction encoding | Typed `enum Instr` | Consistent with `enum Value`, no `unsafe`; same VM learning; raw bytes parked |
| M2-8 | VM kind | Stack machine (clox-style frames) | Per language-spec §5; canonical learning target |
| M2-9 | Concurrency / column-semantics | Parked out of M2 | Not in the current surface; revisit in M3 / LSP layer |
