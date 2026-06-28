# M2 P5 Phase A — `Rc`-shared heap objects

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (inline; subagents
> deadlock on the ask-human gate in this repo). Steps use checkbox (`- [ ]`) syntax. Phorj git
> autonomy applies (commit green, self-contained). Read `docs/INVARIANTS.md` and the design spec
> `docs/specs/2026-06-16-m2-p5-object-model-design.md` before touching the backends.

**Goal.** Make compound heap objects *shared* instead of *deep-cloned*: wrap `Instance`, `EnumVal`,
and list payloads in `Rc`, so `Op::GetLocal`'s clone (and every interpreter var-read) becomes an
O(1) refcount bump instead of a deep `HashMap`/`Vec` copy. Reclamation stays automatic via `Drop`
and is provably correct (the M1 heap is immutable + acyclic — no `Rc` cycle can leak; design §3).
**Behavior is unchanged** — this is a pure perf refactor gated by the differential harness, with a
before/after `phg bench` as the "did it help" evidence.

**Scope.** `src/value.rs` (the `Value` variants + the `Box`→`Rc` swap) and the construct/extract
sites in **both** backends. **No** `Op` set / bytecode-format / AST / checker change. The slab arena
and the slot-indexed field layout are out (the latter is the bench-gated Phase B; design §4).

## Representation

- `Value::Instance(Rc<Instance>)` (was `Box<Instance>`)
- `Value::Enum(Rc<EnumVal>)` (was `Box<EnumVal>`)
- `Value::List(Rc<Vec<Value>>)` (was `Vec<Value>`) — `Rc<Vec>` chosen over `Rc<[Value]>` for
  construction simplicity (`Rc::new(vec)`); revisit only if it matters. `Map`/`Set`/`Str` are left
  as-is (not stressed by the bench; trivial follow-on if ever needed).

`#[derive(Clone)]` on `Value` still holds (`Rc: Clone`); `eq_val`/`as_display`/`type_name` match by
reference and auto-deref through `Rc`, so their bodies need **no** change.

## Construct sites (→ `Rc::new`)

- `src/vm.rs`: `MakeEnum` (~295), `MakeInstance` (~330).
- `src/interpreter.rs`: list literal (~251), enum construct (~371), instance construct (~430/454/456
  — fold the `inst.clone()` double-build into one `Rc`: build `let rc = Rc::new(inst);` once, share
  `rc.clone()` for `this`, return `Value::Instance(rc)`).

## Extract sites needing more than a type swap (can't move out of an `Rc`)

- `src/vm.rs` `GetEnumField` (~308): `ev.payload.into_iter().nth(i)` → `ev.payload.get(i).cloned()`.
- `src/interpreter.rs` `For` (~214): `Value::List(items) => items; for item in items` →
  iterate `items.iter()` and `declare(name, item.clone())`.
- `src/vm.rs` `Index` (~237) / `GetField` (~338): already clone the element via deref — confirm they
  still compile unchanged under `Rc` (auto-deref), adjust only if the borrow checker complains.

All other sites (`MatchTag`, `match_pattern`, field reads, `eq_val`) are read-only/auto-deref → no
change.

## Phasing — one TDD-safe, parity-gated, bench-measured commit

- [x] **A0 (baseline bench):** object VM 1537 ms (4.73×); scalar VM 156 ms (11.57×).
- [x] **A1:** `src/value.rs` — three `Value` variants → `Rc<…>` + `use std::rc::Rc`;
      `eq_val` list-zip fixed to `.zip(b.iter())`; `value.rs` unit tests adjusted (`Rc::new`). Green.
- [x] **A2:** construct sites → `Rc::new` (vm `MakeList`/`MakeEnum`/`MakeInstance`; interp list/enum/
      ctor); three move-out sites fixed (vm `GetEnumField` `.get().cloned()`, interp list-`for`
      `.iter()`+clone, ctor double-build folded into one shared `Rc`); `chunk.rs` test `.into()`.
      `cargo build` clean.
- [x] **A3:** `cargo test` green (244, full differential + examples sweep byte-identical);
      `cargo clippy --all-targets` clean; `cargo fmt --check` clean.
- [x] **A4:** **object VM 1537 ms → 634 ms (2.4×)**; advantage **4.73× → 9.35×** (≈ scalar's
      10.92×). `CHANGELOG.md`/`CLAUDE.md` updated; INVARIANTS.md had no stale value-model claim.
      Commit: `perf(vm): Rc-share heap objects — refcount instead of deep-clone-on-load (M2 P5a)`.
- [x] **A5 (Phase B decision): DO NOT open Phase B.** Post-P5a the object-path advantage (9.35×) is
      within ~15% of scalar (10.92×), so field access (HashMap lookup) no longer dominates — no
      evidence justifies the larger interpreter-touching slot-indexed-field change. Stays bench-gated.

## Acceptance criteria

- Full suite green (244), differential + examples byte-identical, clippy + fmt clean,
  `#![forbid(unsafe_code)]` intact.
- A measured object-heavy bench improvement (before/after recorded); no regression on the scalar
  bench beyond noise.
- No `Op`/bytecode/AST/checker change; `Box`→`Rc` confined to `value.rs` + enumerated sites.

## Risks & rollback

- **Risk — borrow-checker friction at move-out sites.** Mitigation: the three are enumerated above;
  each has a known `.cloned()`/`.iter()` fix. **Risk — a missed construct site.** Mitigation: the
  type swap makes any missed `Box::new` a *compile error*, not a silent bug.
- **Rollback:** single isolated commit; `git revert` restores the value-native state.

## Decisions Log

- [2026-06-16] AGREED: P5 Phase A = `Rc`-wrap `Instance`/`Enum`/`List`; behavior-preserving, gated by
  the differential harness, measured by `phg bench`. (Design: `docs/specs/2026-06-16-m2-p5-object-model-design.md`.)
- [2026-06-16] AGREED: `Value::List` becomes `Rc<Vec<Value>>` (not `Rc<[Value]>`) for construction
  simplicity; `Map`/`Set`/`Str` left unchanged (not bench-stressed).
- [2026-06-16] AGREED: Phase B (slot-indexed field layout) is **bench-gated** on A4 — not started
  without evidence that field access still dominates.
