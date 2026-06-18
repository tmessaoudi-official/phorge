# M2 Plan 4 — Classes, Enums, and `match` on the Bytecode VM

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement
> this plan task-by-task (inline; subagents deadlock on the ask-human gate in this repo).
> Steps use checkbox (`- [ ]`) syntax. Phorge git autonomy applies (commit green,
> self-contained waves; see `CLAUDE.md`). Read `docs/INVARIANTS.md` before touching the
> backends, `value.rs` kernels, or the `Op` set.

**Goal:** Compile and execute the remaining M1 language surface — single-payload **enums**,
exhaustive **`match`**, **classes** (construction + constructor promotion + field reads), and
**methods** (`this`) — on the bytecode VM, so `phg runvm <file>` produces byte-identical
stdout *and* byte-identical failures to `phg run` for these programs. `examples/grades.phg`
runs on the VM. After P4, the VM covers the full interpreter surface.

**Architecture:** The interpreter is the reference oracle (`docs/INVARIANTS.md`), and both
backends already share one `Value` type. `Value::Instance { class, fields: HashMap<String,
Value> }` and `Value::Enum { ty, variant, payload: Vec<Value> }` *already exist* in
`value.rs` and the interpreter uses them with **plain value semantics, clone-on-use, and no
post-construction mutation** (the language has no field assignment). The VM therefore stores
instances and enum values **directly on its operand stack**, exactly like the interpreter —
no `Rc`, no `RefCell`, no arena. The compiler gains a program-level **descriptor table**
(class field-name lists, enum-variant descriptors, interned member/variant/field-name
strings) — validated like the constant pool. New index-carrying ops construct and deconstruct
these values; method dispatch and field access resolve at **runtime** off the instance's own
`class` string, keeping the compiler decoupled from the (deliberately untyped) AST.

**Tech Stack:** Rust (std only), `enum Op` bytecode, per-function `Chunk`, `value::Value`
reused verbatim for objects + enums (no new heap). Toolchain: `export
PATH=/stack/tools/cargo/bin:$PATH` (cargo 1.96, pinned in `rust-toolchain.toml`).

---

## P4 Scope (frozen)

**In:**
- **Enums** — single-payload variants; construction `Variant(args)` and bare `Variant`;
  structural equality already lives in `value.rs`.
- **`match`** — literal patterns (`Int`/`Float`/`Str`/`Bool`/`Null`), `Wildcard`, `Binding`,
  and `Variant { name, fields }` with recursive payload destructuring; `match` as an
  expression (in interpolation/arithmetic/return position).
- **Classes** — instantiation `ClassName(args)`, **constructor promotion** (params with
  visibility modifiers become fields), constructor **body** execution with `this` in scope
  (runs for side effects; the promoted instance is the result — mirrors the interpreter), and
  **field reads** `obj.field`.
- **Methods** — `obj.method(args)` dispatched on the instance's runtime class; `this` bound
  as the method's slot 0.

**Out (clean compile/runtime parity, unchanged from M1 — not regressions):**
- **Field mutation / assignment** — the language has none (`value.rs:2-3`); not added here.
- **First-class functions / method values** — calls resolve to a static or runtime-named
  target, never a value on the stack.
- **The arena/handle object model** — deferred to a *measured* perf milestone (see P4-1). It
  is a performance change; `docs/INVARIANTS.md` (bench-before-perf) forbids shipping it
  without a `phg bench` before/after number, and value-native objects are already
  parity-correct.
- `null`, user `a[i]` indexing, `|>` — not in the M1 interpreter surface; differential tests
  never exercise them.

## Design decisions (review these)

| # | Decision | Choice | Rationale |
|---|---|---|---|
| P4-1 | **Object/identity model** | **Value-native — reuse the shared `Value::Instance`/`Value::Enum`, clone-on-use, mirror the interpreter.** No arena, no handles, no `Rc`/`RefCell`. | The interpreter (the oracle) uses owned `HashMap`/`Vec` value semantics with no interior mutability (`value.rs:24-34`, `interpreter.rs:256-262`); the language has no field mutation. A handle/arena model introduces *reference* semantics and is a **perf** change — `docs/INVARIANTS.md` forbids unmeasured perf changes and demands oracle parity first. Arena is a post-P4 milestone *with a bench number*. The roadmap's "arena object model" wording is superseded by these in-repo invariants. |
| P4-2 | **Descriptor table** | Add a program-level table of **enum-variant descriptors** `{ ty, variant, arity }`, **class descriptors** `{ class, promoted_field_names, ctor_fn }`, and an **interned-name pool** (variant/field/method strings). Validated alongside the constant pool. | Index-carrying ops need a validated, deduplicated side table; names are interned once so ops carry a `usize`, not a `String`. Mirrors the existing constant-pool discipline (`chunk.rs`). |
| P4-3 | **Enum construction** | `Op::MakeEnum(desc_idx)` — pop `arity` payload values (in source order), build `Value::Enum`, push. Bare variants have `arity == 0`. | Single op; arity is in the descriptor (single source of truth, like `Call`'s arity-from-function). |
| P4-4 | **Class construction** | Each constructor compiles to a **synthetic function** (`<Class>::new`); `ClassName(args)` compiles to `Call` into it. Inside, `Op::MakeInstance(desc_idx)` pops the promoted-field values and builds the `Value::Instance`; the ctor **body** then runs with `this` bound (slot 0), for side effects; the function returns the instance. | Reuses the existing call-frame machinery for the body + `this`, exactly matching `interpreter::construct` (`interpreter.rs:412-457`). Keeps `MakeInstance` a pure value constructor. |
| P4-5 | **Field read** | `Op::GetField(name_idx)` — pop instance, look up `fields[name]`, push clone; runtime fault `no field \`{name}\` on \`{class}\`` on miss. | Runtime lookup keeps the compiler untyped (honors the untyped-AST invariant); the fault string matches the interpreter (`interpreter.rs:256-262`) for `agree_err` parity. |
| P4-6 | **Method dispatch** | `Op::CallMethod(name_idx, argc)` — args + receiver on stack; at runtime read the receiver instance's `class`, resolve `(class, method)` → fn index via the descriptor table, push a frame with the receiver as slot 0. | Dynamic resolution off the runtime class avoids threading types through the AST. The set of `(class, method)` pairs is closed and known at compile time, so resolution is a table lookup, not a search. Method-not-found is a checker error (already), with a defensive VM fault. |
| P4-7 | **`match` lowering** | Store the scrutinee in a hidden `$match` local (evaluated once). Per arm, in source order: `Wildcard` → no test; `Binding` → `SetLocal name` (always matches); literal → `GetLocal $match` + `Const lit` + `Eq` + `JumpIfFalse next`; `Variant` → `GetLocal $match` + `Op::MatchTag(variant_idx)` + `JumpIfFalse next`, then per nested field `Op::GetEnumField(i)` + recurse. On match, compile the arm body, then `Jump end`. | Reuses the existing jump/`Eq`/local machinery (`compiler::compile_if`, `compile_for` precedent). Evaluate-scrutinee-once matches the interpreter (`interpreter.rs:487-502`). |
| P4-8 | **Exhaustiveness** | Stays a **checker** concern (already implemented, `checker.rs:808-878`). The VM keeps the interpreter's defensive `"non-exhaustive match at runtime"` fault as the fall-through after the last arm. | No duplication of exhaustiveness logic across backends; parity on the runtime backstop. |
| P4-9 | **`match` as expression** | A `match` in expression position leaves exactly one value on the stack (each arm body pushes one); the hidden `$match` local is cleaned via scope exit. | The interpreter's `match` is an expression; arms must be stack-neutral except for the one result value. |
| P4-10 | **Op↔match coupling** | Every new `Op` extends **both** `vm::exec_op` and `BytecodeProgram::validate` in the same commit (the dispatch + validate matches are exhaustive). | `docs/INVARIANTS.md` / memory `op-variant-match-coupling`. Descriptor-index ops also gain a `validate` bounds-check arm. |

---

## File Structure

- **Modify** `src/chunk.rs` — add `Op::MakeEnum`, `MakeInstance`, `GetField`, `CallMethod`,
  `MatchTag`, `GetEnumField`; add the descriptor table types (`EnumDesc`, `ClassDesc`, name
  pool) onto `BytecodeProgram`; extend `BytecodeProgram::validate` with bounds checks for the
  new index-carrying ops; update the heap/P4 doc-comments.
- **Modify** `src/vm.rs` — `exec_op` arms for the six new ops; runtime field lookup, runtime
  method resolution + frame push, enum construction/inspection; the `no field`/method faults
  routed through `value`/`diagnostic` so they carry a line.
- **Modify** `src/compiler.rs` — drop the five P4 compile-error stubs (`Expr::This`,
  `Expr::Member`, `Expr::Match`, non-function `Ident` call, method call); compile enum/class
  declarations into descriptors + synthetic ctor functions; `compile_match`; `compile_member`
  (field vs method); enum-variant + class-ctor call resolution; `this` as slot 0 in
  methods/ctors; extend `num_ty`/`type_tag` only as far as parity needs.
- **Modify** `tests/differential.rs` — P4a/P4b/P4c program sets (enum+match, class+field,
  method+`this`), Ok-path (`agree`) and failure-path (`agree_err`) cases; run
  `examples/grades.phg`.
- **Modify** `docs/MILESTONES.md`, `CHANGELOG.md` — mark M2 P4 progress.
- **Modify** `docs/specs/2026-06-15-m2-bytecode-vm-design.md` — fold the as-built object-model
  decision (value-native, arena deferred) into the §5 errata.
- **Check (Phase 7)** `README.md`, `CLAUDE.md` — update the surface/baseline claims.

> `src/main.rs` is **not** modified — `runvm` is already wired into USAGE + dispatch.

---

## Phasing — three green, parity-gated waves

Each wave is TDD-first (differential tests before the op work), ends green
(`cargo test` + `cargo clippy --all-targets` + `cargo fmt --check`), and is one commit.

### P4a — Enums + `match` (no objects) ✅ DONE (2026-06-16)

- [x] **A1 (test):** added `P4A_PROGRAMS` to `tests/differential.rs` — bare + payload variants,
      literal/`Wildcard`/`Binding`/`Variant` patterns, payload destructuring, `match` in
      return/var-decl/transient positions. *No `agree_err` case:* exhaustiveness is
      checker-enforced, so a non-exhaustive `match` is rejected at the check stage on **both**
      backends — there is no checker-passing program that reaches the runtime fall-through, so
      none can be constructed. The runtime backstop (`MatchFail`) exists for defence-in-depth.
- [x] **A2:** `chunk.rs` — added `Op::MakeEnum`/`MatchTag`/`GetEnumField` **and `Op::MatchFail`**
      (a 7th op, amendment vs the original 6-op plan — needed for the checker-unreachable
      non-exhaustive backstop, mirroring the interpreter's exact fault string for EV-7 parity).
      Added `EnumDesc` + `enum_descs` to `BytecodeProgram`; extended `validate` (descriptor
      bounds for `MakeEnum`/`MatchTag`; `GetEnumField`/`MatchFail` carry no static-bounded index).
      One descriptor table indexed by variant (variant names are globally unique) — no separate
      name pool was needed.
- [x] **A3:** `vm.rs` — `exec_op` arms for the four ops.
- [x] **A4:** `compiler.rs` — collect enum descriptors + `variants` map in the pre-pass; resolve
      `Variant(args)`/bare `Variant` to `MakeEnum`; `compile_match` (scrutinee spill + per-arm
      tests + payload re-extraction). Added **operand-height tracking** (`stack_effect` in
      `emit`, reset per statement, fixed at `&&`/`||`/`match` merges) so `match` mid-expression
      spills its scrutinee to the correct slot. Removed the `match` + variant-call stubs.
- [x] **A5:** suite green (231 tests), clippy + fmt clean; committed.
      **Correction:** `examples/grades.phg` is **not** unblocked by P4a — it contains a
      `class Counter`, so it needs P4c. The grades example run moves to P4c.
      **Lexer limitation found:** `match` cannot appear inside string interpolation (`"{match …}"`
      does not lex — the interpolation lexer doesn't nest the `match` braces). Pre-existing,
      shared by both backends (not a parity issue). Transient-context coverage instead uses
      `match` as a binary operand and `match` nested in an arm body.

### P4b — Classes: construction + field reads ✅ DONE (2026-06-16)

- [x] **B1 (test):** added `P4B_PROGRAMS` to `tests/differential.rs` (8 agree-path programs: promoted
      fields, field reads in interpolation, field-via-typed-local arithmetic, side-effecting ctor
      body, no-ctor empty instance, structural instance equality, promoted-vs-bare param, field as a
      call arg, early-`return` ctor) **and** a real `agree_err` `no field` case — reachable because
      an explicit (uninitialized) `Field` member type-checks but is unpopulated by construction (a
      runtime fault on both backends, unlike P4a's checker-enforced exhaustiveness). Added a
      `FaultKind::NoField` classifier (by `"no field"` body substring, tolerating the VM line prefix).
- [x] **B2:** `chunk.rs` — added `Op::MakeInstance`/`GetField`; `ClassDesc { class, fields }`
      (promoted-field names); a program-level `names` field-name pool on `BytecodeProgram`; extended
      `validate` (class-descriptor bounds for `MakeInstance`; name-pool bounds for `GetField`).
- [x] **B3:** `vm.rs` — `exec_op` arms for both ops; `GetField`'s `no field`/`cannot read` faults
      byte-identical to the interpreter (`Expr::Member`).
- [x] **B4:** `compiler.rs` — class pre-pass builds `ClassDesc`s + the name pool; each constructor
      compiles to a synthetic `<Class>::new` (indexed *after* all free functions, so free/`main`
      indices are unchanged) via a `compile_constructor` helper: promoted-param `MakeInstance`
      prologue → body → epilogue that loads + returns the instance. `ClassName(args)` resolves to a
      `Call` into it; `Expr::Member` lowers to `GetField`. Removed the member + class-ctor-call stubs
      (the `this`/method-call stubs stay for P4c). Added `Compiler::new` to share the program tables.
- [x] **B5:** suite green (239 tests), clippy + fmt clean; committed.
      **As-built notes:**
      - **Ctor body `return` redirect:** the checker pins a ctor body's return type to `Unit`, and
        `interpreter::construct` discards the body's return and always yields the promoted instance.
        The synthetic ctor mirrors this by redirecting body `return`s to the epilogue (never an
        `Op::Return`), so an early `return;` cannot change the constructed value. A new
        `ctor_return_jumps` compiler field carries the redirect.
      - **`this` deferred to P4c:** the `Expr::This` stub stays — P4b ctor bodies reference promoted
        params by name (resolved as locals), not via `this`. `examples/grades.phg` runs at P4c (it
        calls an instance method).
      - **`num_ty(Member)` gap (pre-existing, Wave 4):** a field read used as the *direct left
        operand* of arithmetic isn't classifiable by the coarse `TyTag` (can't recover field types
        yet — same gap as `Index`). Not in the corpus; field reads work in every other position.

### P4c — Methods + `this` ✅ DONE (2026-06-16)

- [x] **C1 (test):** added `P4C_PROGRAMS` to `tests/differential.rs` (bare-field method, method→method
      via `this`, mixed bare/`this.` field reads, recursion through `this.fact`, void method as a
      statement) **and** added `examples/grades.phg` to the examples sweep. No `agree_err` case:
      method existence is checker-enforced (the VM's method-not-found fault is a checker-unreachable
      backstop, like P4a's exhaustiveness).
- [x] **C2:** `chunk.rs` — `Op::CallMethod(name_idx, argc)`; a program-level
      `methods: HashMap<(class, method), fn idx>` dispatch table (rather than threading it through
      `ClassDesc`); `validate` checks `CallMethod`'s name-pool index and every dispatch target.
- [x] **C3:** `vm.rs` — `CallMethod` resolves the receiver's runtime class against `methods`, opens
      a frame with the receiver at slot 0 (args at `1..=argc`); defensive `no method`/`cannot call`
      faults byte-identical to the interpreter.
- [x] **C4:** `compiler.rs` — methods compile to functions (`compile_method`, receiver at slot 0);
      `obj.m(args)` → `CallMethod`; `Expr::This` → `GetLocal(this_slot)`; a bare field name in a
      method/ctor body resolves to `this.field` (`this_slot` + `field_tags`); `num_ty` classifies a
      `this.field` arithmetic operand. Removed the last two stubs (`Expr::This`, method calls). A
      ctor body can now use `this` too (`this_slot` set to the instance slot).
- [x] **C5:** suite green (243 tests), clippy + fmt clean; committed.
      **As-built notes:**
      - **Function index layout** is `[free fns | constructors | methods]`; the method dispatch
        table maps `(class, method)` to the method's index in that space.
      - **`num_ty(Member)` gap narrowed:** `this.field`/bare-field operands are now classifiable;
        a field read on an *arbitrary* instance or a `List` element stays the coarse-`TyTag` gap
        (Wave 4). Not in the corpus.
      - `grep "(M2 P4)"` in `compiler.rs`/`vm.rs` is clean; `phg bench examples/grades.phg` runs
        (VM ≈3.2× the tree-walker, output identical).

---

## Acceptance criteria

- `phg runvm` is byte-identical to `phg run` (stdout **and** `FaultKind`) for every P4
  program in `tests/differential.rs`, including `examples/grades.phg`.
- The five `(M2 P4)` compile-error stubs in `src/compiler.rs` are all removed; no remaining
  `(M2 P4)` deferral string in `compiler.rs`/`vm.rs` (grep clean).
- `Op` additions appear in **both** `vm::exec_op` and `BytecodeProgram::validate` (exhaustive
  matches compile; `validate` bounds-checks every new index-carrying op).
- Gate green at each commit: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`;
  `#![forbid(unsafe_code)]` intact.
- `phg bench examples/grades.phg` runs (output-identity gated) — establishes the
  classes/enums perf baseline (informational; no perf target asserted).

## Risks & rollback

- **Risk — method dispatch parity:** runtime class resolution must match the interpreter's
  method lookup order/faults. *Mitigation:* `agree_err` cases for method-not-found; the
  checker rejects unknown methods first, so the VM path is a defensive backstop.
- **Risk — `match` stack discipline:** an arm that leaves the wrong number of values corrupts
  the stack. *Mitigation:* P4-9 stack-neutrality + the `Pop`/scope-exit precedent from
  `compile_for`; differential tests with `match` in expression position catch imbalance via
  output divergence.
- **Risk — constructor body semantics:** the interpreter runs the body but the promoted
  instance is the result (body cannot mutate fields). *Mitigation:* P4-4 mirrors this exactly
  (body for side effects, instance returned); a differential test with a side-effecting ctor
  body (`println` in ctor) locks it.
- **Rollback:** each wave is an isolated green commit; `git revert <sha>` drops a wave without
  touching the others. The descriptor table is additive — reverting P4c leaves P4a/P4b green.

---

## Decisions Log

- [2026-06-16] AGREED: Full P4 in one plan doc (classes + constructor promotion + member
  access + enums + `match`), phased P4a→P4b→P4c.
- [2026-06-16] AGREED: Object model **A** — value-native (reuse shared `Value::Instance`/
  `Value::Enum`, clone-on-use, mirror the oracle); the arena/handle model is deferred to a
  measured post-P4 perf milestone (bench-before-perf invariant).
