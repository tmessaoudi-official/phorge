# ADR-0003: Reference counting (`Rc`/`Drop`), not a tracing GC

- **Status:** Accepted (2026-06-19)
- **Deciders:** project author
- **Fuller design:** m2-p5 object-model design (consolidated 2026-07-02; git history ≤`60540fc`) (supersedes the original
  M2 design's §7 "arena + mark-sweep GC" for the current language).

## Context

The object model is **value-native** (instances/enums are `Value`s, P4-1). On the M1 language
surface, an object can only be constructed from values that already exist, so the runtime object
graph is **acyclic — in fact a tree of owned values**. The original M2 design had pencilled in an
arena + tracing mark-sweep collector, motivated chiefly by a "rival-Java" learning goal.

## Decision

Use an **`Rc`-shared heap**; `Rc`/`Drop` performs all reclamation. Ship **no tracing collector**.
A tracing mark-sweep GC is **deferred to M3**, the point at which mutation could introduce cycles
and a tracing collector would finally earn its keep.

## Consequences

- **Complete and immediate reclamation, no leaks:** because the heap is acyclic, `Rc` frees each
  object the instant its last reference drops, and never leaks. A tracing collector on this heap
  would only ever find garbage `Rc` has already freed.
- **Performance win:** making the heap `Rc`-shared turned the `Op::GetLocal` hot path into a
  refcount bump instead of a deep clone — object-heavy VM runs dropped **1537 ms → 634 ms (2.4×)**.
- The decision is **revisitable by design**: when M3 adds mutation, the deferred tracing GC is
  reconsidered against a heap that can finally form cycles.

## Alternatives rejected

- **Tracing mark-sweep GC (now)** — on an immutable, acyclic heap it can reclaim **nothing** that
  `Rc`/`Drop` doesn't; its only justification was the learning goal. Deferred to M3.
- **`Vec<Obj>` slab arena with integer handles** — more code than `Rc` for a cache-locality bet with
  **no locality evidence** to back it.
