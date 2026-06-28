# Adversarial Review — Cooperative async/await + deterministic single-threaded scheduler

**Stage 2b — REFUTE.** Target: `docs/research/extended-modules/raw/design-async-scheduler.md`
(claims tier=A, feasibility=70%, "byte-identical by construction"). Verdict below: the **Tier-A
keystone (S1) does not hold as designed** — not because of hidden non-determinism in the *scheduler
rule* (that part is genuinely sound), but because the **suspension mechanism the whole Tier-A claim
rests on does not exist and cannot exist on the tree-walking interpreter in safe std Rust** without
either a CPS lowering pass the design defers out of Layer 1, or a new VM Op the design holds "in
reserve." The byte-identity argument is therefore conditioned on machinery that is not shipped and is
explicitly punted. Additionally, the Tier-B quarantine, as the design places it, **leaks** through the
existing module-level `uses_impure_native` seam.

Confidence in this refutation: **high** on the suspension hole (verified against source); **high** on
the quarantine leak (verified against `tests/differential.rs`); medium on the secondary risks.

---

## R-A (DECISIVE) — The suspension mechanism does not exist on the Rust legs, and Layer 1 ships without the only thing that could fake it

The design's entire Tier-A claim reduces to one sentence (§3.4): *"this is what `run_until` already
supports."* It does not. Verified against `src/vm/closure.rs`:

```rust
pub(super) fn run_until(&mut self, target_depth: usize) -> Result<(), String> {
    while self.frames.len() > target_depth { … exec_op … }
    Ok(())
}
```

`run_until` runs a frame **to completion** — it loops *until the frame returns*. It is a synchronous
"call this closure and give me its value" primitive (used by `Core.List.map`/`reduce`). The
interpreter's analogue `call_tree_closure` (`src/interpreter/call.rs:225`) likewise walks the
closure's AST `body` recursively **on the native Rust call stack** and returns a `Value`. **Neither
leg has any way to pause a closure mid-body and hand control back to a scheduler while keeping the
closure resumable.** The design *itself admits this* in §3.4(A): *"`run_until` returns a value, it
does not leave a resumable suspended Rust frame … a genuine mid-expression suspend would need a
Phorj-level CPS transform or a saved VM frame snapshot."*

So the design's own §3.4(A) refutes §3.2/§3.4's "already-shipped" framing. It then pivots to (B), CPS
trampolining, as the v1 recommendation. But here is the contradiction that breaks Tier-A as scoped:

- **Layer 1 (`Core.Async`) ships FIRST and is "zero new syntax"** (§2.1, §5 S1, O4 "library-first").
  Its showcase is `Async.spawn(fn() -> int { Async.yield(); return n*10; })` — a *plain closure with a
  mid-body `Async.yield()`*.
- **The CPS transform that splits a body at each `await`/`yield` is Layer 2** (§2.2: "`async`/`await`
  are pure desugaring", §5 S5, §3.4(B): "`async`/`await` Layer 2 then becomes a front-end CPS
  lowering").
- Therefore **Layer-1 `Async.yield()` inside a `spawn`-ed plain closure has no CPS lowering applied to
  it.** When the Rust legs call that closure (`call_closure` / `call_closure_value`), they run it to
  completion. `Async.yield()` cannot suspend — there is no frame to suspend *to* and no continuation
  was synthesized. The keystone S1 example (`work()` calling `Async.yield()`) **cannot be implemented
  on the Rust legs as written.**

The only escape routes are exactly the two the design defers or reserves:
1. Apply CPS lowering to *every* closure that transitively touches a suspension native — i.e. pull
   Layer-2's transform down into Layer 1. That contradicts "Layer 1 = zero new syntax, ships first"
   and is the hardest, highest-risk part of the whole design (CPS-lowering a tree-walker body to
   match the VM to match Fibers — the design's own R1 and its 70%→risk-concentration paragraph).
2. Add the `Op::Suspend`/`Op::Resume` pair (§4 contingency) to snapshot a VM frame — the "one place a
   new Op would appear." But that only solves the *VM* leg; the *tree-walking interpreter* has no
   bytecode frame to snapshot. There is no safe-std way to snapshot a recursive Rust call stack. So
   even the reserved Op does not give a parity-complete suspension across both Rust legs.

**Net:** the claim "no new Op, byte-identical by construction, reuses shipped machinery" is true only
for a scheduler that never actually suspends a coroutine mid-body — i.e. a scheduler that can run
`spawn`/`all`/`parallelMap`/`group` where every task runs straight through with **no internal
suspension point**. That degenerate subset *is* byte-identical (it's just ordered `List.map` /
sequential evaluation, which already ships). The instant a task contains a real `yield`/`await`/
channel `recv`-on-empty (the entire point of the feature), the design needs machinery that is either
deferred out of the shipping slice or does not exist on one of the three legs. **feasible_std_only =
false** for S1 as scoped.

---

## R-B (DECISIVE for the Tier-B addendum) — The quarantine leaks: impurity is module-level, but `sleep`/`after` live in the SAME `Core.Async` module as the pure core

Verified against `tests/differential.rs:916-924`: the differential harness quarantines a program iff
it imports a **module** that contains *any* impure native:

```rust
let impure: HashSet<&str> = registry().iter().filter(|n| !n.pure).map(|n| n.module).collect();
impure.iter().any(|m| src.contains(&format!("import {m}")))
```

The granularity is `n.module`, not `(module, name)`. The design (§5 "Tier-B addendum",
§1, §6 R5) puts the Tier-B `Async.sleep` / `Async.after` / live-socket natives **inside `Core.Async`**
— the same module as the pure Tier-A `spawn`/`yield`/`all`/`channel`. The moment one
`pure:false` native is registered under module `"Core.Async"`, **every program that does
`import Core.Async;` — including every Tier-A gated example — is auto-quarantined out of the
byte-identity differential.** The whole point of Tier A (gated, byte-identical, in `differential.rs`)
silently evaporates: the harness can no longer prove the cooperative core is byte-identical, because
it stops checking it.

This is not a hypothetical: it is the *designed* layout (one `src/native/async.rs` leaf, Tier-A and
Tier-B natives side by side). The design's own R5 ("wall-clock-anything is `pure:false`, caught by
`uses_impure_native` auto-quarantine") is the bug — that auto-quarantine is module-scoped and will
swallow the entire module. To preserve Tier A, the impure live natives must live in a **separate
module** (e.g. `Core.AsyncLive` or fold them into `Core.Process`/a new `Core.Net`), and the design
does not say that — it explicitly co-locates them. As written, **the quarantine is not airtight; it is
inverted** (it over-quarantines, destroying the Tier-A guarantee rather than leaking a Tier-B item
into the differential — but the effect is the same class of failure: the differential no longer gates
what the design claims it gates).

---

## R-C — Fiber (stackful) vs CPS (split-body) is not order-equivalent for shared mutable state, contra §3.4/R3

The design asserts (§3.4, R3) that "the byte-identity invariant is about order, not mechanism" and
that side-effect order is safe because "a task runs to its next await with no interleaving." That is
true for `Console.println`-style append-only effects between two awaits. It is **not** obviously true
once channels (S2) and `Context` (S4) introduce **shared mutable `Rc<RefCell>` state read across
suspension points**:

- A PHP `Fiber` is stackful: when it resumes, *local variables that were live across the suspend are
  exactly as left*. A CPS-lowered Rust closure reconstructs state from the captured continuation
  environment. If the front-end CPS transform captures a *snapshot* of a value at the split point
  while the Fiber re-reads *live* state, then any mutation of shared channel/context state by an
  interleaved task between the two awaits is observed differently: the Fiber sees the post-mutation
  value (live read), the CPS continuation sees whatever it captured. This is a classic
  capture-by-value vs read-through divergence, and it is exactly the kind of bug the project has hit
  before (the `??`/`?.` scratch-slot break, [[null-op-scratch-slot]]; the higher-order-native
  re-entrancy parity work, [[higher-order-natives-reentrant-vm]]).
- R3's mitigation ("side effects are ordered by run-to-completion") addresses *when prints happen*,
  not *what value a resumed continuation reads from shared state*. The design does not analyze the
  read-through-vs-snapshot divergence at all. This is a named, unmitigated determinism risk for S2+.

Confidence medium: it depends on the not-yet-specified CPS lowering rules. But the design cannot claim
"byte-identical by construction" while leaving the one mechanism that differs between legs (stackful
Fiber vs reconstructed CPS env) unanalyzed for shared-state reads.

---

## R-D — PHP `usort` is NOT stable; the design's own timer-heap mitigation contains the bug it warns about

§7's emitted PHP scheduler sorts the timer heap with:

```php
usort($this->timers, fn($a,$b)=>[$a[0],$a[1]]<=>[$b[0],$b[1]]);   // "stable by (deadline,seq)"
```

The design (R-PHP-1) correctly flags that `SplPriorityQueue` is not insertion-stable and says "emit
our own stable heap." But the *actual sketch it provides* uses `usort`, which **is itself not a stable
sort in PHP** prior to 8.0 and — more to the point — the comment claims stability is achieved "by
(deadline, seq)". That only works **if `seq` is globally unique per timer**, which the sketch does
ensure (`$this->nextSeq++`). So the tie-break is fine *as long as no two timer entries ever share a
`(deadline, seq)`* — they can't, since `seq` is monotonic. **This particular instance survives** — but
only by accident of `seq` uniqueness, and the design's prose ("`usort` … stable") is wrong about *why*
(it's total-order-by-unique-key, not stability). More important: the same `seq` counter
(`$this->nextSeq`) must be incremented in **byte-identical lockstep** with the Rust legs'
`next_seq`/`insertion_seq` for **every** schedule/spawn/timer event, in the same order, or the
tie-break key diverges. The design states this invariant but provides no cross-leg test that the seq
*assignment order* matches (the Rust legs assign seq during spawn/await lowering; the PHP leg assigns
it inside the emitted runtime — two independently-written counters that must agree event-for-event).
That is a real, unmitigated parity surface, not "by construction."

Confidence: low-medium that this bites (it's avoidable), but the design overstates the guarantee.

---

## R-E — `parallelMap` "byte-identical because sequential today" is fine; the LATER optimization is a landmine the design under-rates (minor)

§2/R6: physical Rust-thread parallelism is "a LATER invisible optimization that preserves output."
Two issues: (1) `Value` is `!Send`/`!Sync` (verified: `Rc`-shared heap), so any future thread fan-out
must deep-clone or process-fork inputs *and* re-merge outputs — the design says this (R6). Fine for
v1. (2) But the *fault* path is the trap: if two tasks fault, an ordered-merge contract must pick the
**first-in-input-order** fault deterministically. Under real parallelism, the *physically-first* fault
to surface is non-deterministic; the merge must suppress all-but-input-first. The design says "ordered
merge keeps output identical" but does not specify fault-ordering under future parallelism. Not a v1
blocker (v1 is sequential = trivially correct), so this is a noted-not-fatal finding.

---

## R-F — `Future`/`Channel` as `Instance` (O2) rides `RefCell<HashMap>` fields; safe for access, latent for iteration

Verified: `Instance.fields: RefCell<HashMap<String, Value>>` (`src/value.rs:94`). Field access by name
observes no iteration order, so a `Future`/`Channel`-as-`Instance` is determinism-safe for
`fut.value` / `ch.buf` reads. **Latent risk:** if any scheduler bookkeeping ever stores
futures/channels keyed in a `HashMap` and *iterates* it (e.g. "wake all waiters on this channel"), the
iteration order is non-deterministic and differs from the PHP leg. The design stores waiters in
ordered `Vec`s (§4 `ChannelState.waiters_*`, R4 "FIFO registration order"), so v1 is safe *if that
discipline holds* — but the project's own Map/Set rep deliberately uses `Vec` not `HashMap` precisely
to avoid this ([[value-kernels-single-sourced]], the insertion-ordered Map work). The design should
state the invariant "no scheduler state is ever a `HashMap` that gets iterated" explicitly; it is
currently implicit. Confidence medium, not fatal.

---

## What survives (steelman)

The *scheduler ordering rule itself* — FIFO ready-queue, logical-clock min-heap keyed on
`(deadline, insertion_seq)`, drain-microtasks-before-timers, logical time never reads a wall clock —
**is genuinely deterministic and is the correct design.** The `pure` flag + `uses_impure_native` seam
is a real, shipped quarantine mechanism. PHP 8.1 `Fiber` + `SplQueue` are present under `php -n`
(consistent with the prompt's stated invariant). The *degenerate* subset (tasks with no internal
suspension — `parallelMap`, `all` over straight-line tasks, ordered fork-join with no internal
`recv`-on-empty) is byte-identical and ships cheaply. So the milestone is not incoherent — but its
**Tier-A keystone (real cooperative suspension, byte-identical across a tree-walker + a VM + Fibers) is
not feasible std-only in the shipped slice as scoped**, and the **Tier-B addendum's module placement
breaks the quarantine.**

---

## Verdict

- **determinism_holds = false.** Not because the scheduler rule is non-deterministic (it isn't), but
  because the design cannot deliver real suspension on the tree-walking interpreter in safe std Rust
  without machinery (CPS lowering) it explicitly defers out of the shipping Layer-1 slice — and the
  Fiber-vs-CPS shared-state read divergence (R-C) is unanalyzed. The "byte-identical by construction"
  claim holds only for the suspension-free degenerate subset.
- **feasible_std_only = false** for the full Tier-A surface (S1 keystone). The suspension-free subset
  (ordered fork-join / parallelMap / all-over-straight-line-tasks) **is** feasible and Tier-A — that is
  the part worth salvaging.
- **revised_tier = mixed.** Suspension-free data-parallel/fork-join primitives = Tier A (ship those).
  Real cooperative suspension (`yield`/`await`/channel-block) = **not Tier A as designed** — it needs
  either (a) Layer-1 CPS lowering accepted up front (re-scope, re-estimate well below 70%), or (b)
  demotion to Tier B with fixture tests until the suspension mechanism is proven on all three legs.
  Live timers/sockets = Tier B but **must move out of the `Core.Async` module** or they quarantine the
  whole module.

### Required design changes before this can be Tier A
1. **Split modules:** Tier-A pure async core in `Core.Async`; Tier-B `sleep`/`after`/live in a
   *separate* module (`Core.AsyncLive` / `Core.Net`). Otherwise R-B auto-quarantines the core.
2. **Pull CPS lowering into the shipping slice** (it is not optional Layer-2 polish — it is the
   prerequisite for any task that suspends), and re-estimate S1 feasibility honestly (the design's own
   risk paragraph concentrates the risk exactly here; 70% is too high once CPS is in S1).
3. **Specify and test the Fiber-vs-CPS shared-state read contract** (R-C): a differential example with
   an interleaved channel mutation observed across an await on all three legs.
4. **Add a seq-assignment-order parity test** (R-D): the PHP `nextSeq` and the Rust `insertion_seq`
   must increment event-for-event in the same order.
5. **State the "no iterated `HashMap` in scheduler state" invariant** explicitly (R-F).
