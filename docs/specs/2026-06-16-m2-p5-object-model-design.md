# Phorge M2 P5 — Object-Model Performance (shared immutable heap) — Design

> Supersedes the original M2 design's §7 "arena + mark-sweep GC" for the *current* language. The
> original plan (`2026-06-15-m2-bytecode-vm-design.md` §6/§7) assumed a mutable heap that needs a
> tracing collector. P4-1 made the object model **value-native** (instances/enums are cloned
> `Value`s), and the M1 surface is **immutable and acyclic** — which changes the right answer. This
> doc is the frozen design for P5; the implementation plan will live in `docs/plans/`.

## 1. Goal & Non-Goals

**Goal.** Cut the bytecode VM's object-path cost — measured below — by making heap objects
*shared* instead of *deep-cloned*, using the simplest reclamation the language's shape allows.
Stay byte-identical on both backends throughout (the differential harness is the spine).

**Non-goals (explicitly out of P5):**
- A **tracing mark-sweep collector.** Justified only by the original "rival-Java" learning goal;
  on an immutable acyclic heap it can reclaim nothing that `Rc`/`Drop` doesn't. Deferred to **M3**,
  if/when mutation (reassignment, mutable fields, cyclic data) is introduced — *that* is when a
  cycle can form and a tracing GC earns its keep.
- New language features (M3), bundling/`phg build` (M2.5).
- A `Vec<Obj>` **slab arena** with integer handles — more code than `Rc` for a cache-locality
  benefit we have no evidence we need. Revisit only if a post-Phase-A bench demands locality.

## 2. Evidence — the measured cost (bench, 2026-06-16)

`phg bench`, median of 101, output-identity gated, on two `fib(28)` programs (~832k calls):

| Workload | tree-walk | VM | VM advantage |
|---|---|---|---|
| Scalar (no objects) | 1805 ms | **156 ms** | **11.57×** |
| Object-heavy (instance + method + field read per call) | 7277 ms | **1537 ms** | **4.73×** |

The VM still wins everywhere (no correctness pressure), but its advantage **more than halves** on
object code, and absolute VM time is ~10× the scalar version. The dominant hotspot is
`Op::GetLocal` (`vm.rs:195`): `self.stack[idx].clone()` **deep-copies the whole `Box<Instance>`
and its `HashMap<String,Value>`** on every local load. `MakeInstance` also allocates a fresh
`HashMap` per construct. Argument passing is *not* a clone (frames reuse the stack window via
`pop_n_start`). So the cost is overwhelmingly **deep-clone-on-load + per-instance map**.

## 3. Why this shape (the load-bearing finding)

`value.rs` (EV-1) and the M1 immutability invariant guarantee: no reassignment, no
post-construction field mutation, and a constructor's args are fully evaluated *before* the
instance exists. Therefore the runtime object graph is **acyclic — in fact a tree of owned
values**. No cycle can be constructed.

Consequence: **reference counting is sufficient and complete.** `Rc<T>` reclaims every object the
instant its last reference drops, and — because there are no cycles — it never leaks. A tracing
collector would only ever find the same garbage `Rc` already freed. So the *perf* win comes
entirely from **sharing** (clone → refcount bump), not from a collector.

## 4. Approach — staged, evidence-driven

### Phase A — `Rc`-wrapped heap objects (the core win)
Wrap the compound heap variants in `Rc`:
- `Value::Instance(Rc<Instance>)`
- `Value::Enum(Rc<EnumVal>)`
- `Value::List(Rc<[Value]>)` (or `Rc<Vec<Value>>` — TBD at plan time; lists are also deep-cloned
  by `GetLocal`)

`Clone` (the `GetLocal` hot path, plus every interpreter var-read) becomes an O(1) refcount bump.
Reclamation is automatic via `Drop`, correct by §3. The change is confined to `value.rs` plus the
construct/extract sites in **both** backends (`vm.rs`: `MakeInstance`/`MakeEnum`/`GetField`/
`GetEnumField`/`Index`; `interpreter.rs`: the mirror sites). Semantics are identical — structural
equality (`eq_val`) and display are unchanged — so the differential harness stays green and parity
is preserved.

Then **re-bench** the object-heavy program. The Phase-A number decides Phase B.

### Phase B — slot-indexed field layout (bench-gated)
*Only if* the post-Phase-A bench still shows field access dominating: replace
`Instance.fields: HashMap<String,Value>` with a `Vec<Value>` indexed by the compiler's
already-interned field slots (`class_descs` knows field order; `names_index` already interns).
`GetField(slot)` becomes a `Vec` index instead of a string-hash lookup, and construction stops
allocating a per-instance map. Larger blast radius — touches the interpreter's field model, the
checker's field assumptions, and structural equality — so it is its own milestone, taken only with
evidence.

### Rejected — slab arena (`Vec<Obj>` + `Handle(u32)`)
Better cache locality, but its reclamation on acyclic data is no simpler than `Rc`, and it adds
indirection + a free-list/refcount layer. No evidence locality is the bottleneck. Revisit post-B.

## 5. Parity, risk, rollback

- **Parity.** `Rc<T>` shares the value but the value is immutable, so observable behavior
  (output, equality, faults) is unchanged. `tests/differential.rs` (`agree`/`agree_err`) and the
  examples sweep are the gate; every phase lands green.
- **Risk — `Rc` clone semantics.** Non-atomic `Rc` (single-threaded VM); no `RefCell` (no
  mutation). The only behavioral surface is structural equality, which already compares by value.
- **Risk — broad mechanical churn across both backends.** `value.rs` is shared; the construct/
  extract sites are enumerable and small. One atomic, `git revert`-able commit per phase.
- **Rollback.** Each phase is one isolated commit; revert restores the value-native state.

## 6. Success criteria (P5 done)

1. Object-heavy bench improves measurably on the VM (before/after `phg bench` numbers
   recorded), with the differential suite and examples sweep still byte-identical.
2. `cargo test` green, `cargo clippy --all-targets` clean, `cargo fmt --check` clean,
   `#![forbid(unsafe_code)]` intact.
3. The "value-native, GC-deferred" decision is recorded in `CLAUDE.md`/`CHANGELOG.md`/
   `docs/INVARIANTS.md`, and the original design's §7 arena/GC note is marked superseded for the
   current (immutable) language.

## 7. Decisions Log

- [2026-06-16] AGREED: Do **P5 next** (after Wave 4), per the earlier "Wave 4 then P5" lock; the
  bench confirms the object path is a measured weak spot, so P5 is evidence-justified, not blind.
- [2026-06-16] AGREED: **No tracing mark-sweep GC in P5.** The M1 heap is immutable + acyclic, so a
  tracing collector reclaims nothing `Rc`/`Drop` doesn't. A real collector is deferred to **M3**
  (when mutation could create cycles). P5's reclamation is the simplest correct mechanism the
  acyclic heap allows.
- [2026-06-16] AGREED: **Approach = `Rc`-wrapped heap objects (Phase A)**, re-bench, then a
  **bench-gated** slot-indexed field layout (Phase B). The slab arena is rejected unless a
  post-Phase-A bench demands cache locality.
- [2026-06-16] AGREED: Stage as **one differential-green commit per phase**; never build ahead of
  evidence.
