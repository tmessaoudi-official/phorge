# Concurrency Prior-Art for Phorge — Reactive/FRP + Actor Model

**Stage 1 research.** Lens: Reactive/FRP (ReactiveX/Rx operators, observables, backpressure,
schedulers) and the actor model (Erlang/Elixir BEAM, message-passing, supervision) — read through
Phorge's reality: a single-threaded `Rc`-shared heap (`Value` is NOT `Send`/`Sync`), three backends
that must produce byte-identical stdout for *gated* features (interpreter `run`, bytecode VM `runvm`,
Phorge→PHP transpile under real `php -n` 8.5), and a Tier-A (gated, byte-identical) / Tier-B
(`pure:false`, quarantined out of `differential.rs`, fixture-tested) model that already exists for
`Core.Process`/`Core.Env`.

Verified environment facts (this session):
- **Fibers are PRESENT under `php -n` 8.5** — `php -n -r 'var_dump(class_exists("Fiber"));'` → `bool(true)`
  [Verified]. This is the load-bearing fact for cooperative async: PHP 8.1 `Fiber` is a *core* class
  (not an extension), so it survives `php -n`. mbstring/PHPUnit/gmp/APCu are ABSENT; PCRE/hash/BCMath
  PRESENT.
- The native registry (`src/native/mod.rs`) keys each `NativeFn` on `(module, name)` with
  `eval: NativeEval::{Pure, HigherOrder, Reflective}`, a `php:` transpile mapping, and a `pure: bool`.
  `uses_impure_native` in `tests/differential.rs` (line 916) skips any program that does
  `import <impure-module>` — the quarantine is **declared per-native, read generically by the harness**
  [Verified: read both].
- `HigherOrder` natives already drive a **re-entrant** closure call into both backends: the interpreter
  wraps `call_closure`, the VM wraps `call_closure_value` + `run_until` over the shared `exec_op`
  (`Core.List.map/filter/reduce`). A native can therefore *invoke a user closure* with byte-identical
  results across `run`/`runvm`. This is the single most important existing primitive for a scheduler:
  **a native that resumes/steps user code already exists in spirit** [Verified: read mod.rs +
  memory `higher-order-natives-reentrant-vm`].
- `src/serve.rs` already quarantines the socket behind a `Transport` trait (`recv`/`send`), with
  `TcpTransport` as the live impl and an in-memory transport for `tests/serve.rs`. This is the
  template for *any* live-I/O Tier-B subsystem [Verified: read serve.rs].

---

## 0. The lens, restated as a determinism axis

The entire concurrency design space, viewed through Phorge, collapses onto **one question per model**:
*does the model's observable OUTPUT depend only on the program text + inputs, or also on a
non-reproducible scheduling/timing/IO decision?*

- **Deterministic-by-construction** → Tier-A, gated, byte-identical across all three legs. The model's
  *scheduling order is a pure function of program structure*, so the interpreter, the VM, and PHP all
  reach the same interleaving.
- **Non-deterministic-by-essence** → Tier-B (or rejected). The interleaving depends on the OS scheduler,
  wall-clock timers, socket arrival order, or true parallelism. Cannot be byte-identical; fixture-tested
  with a controlled/replayed event source, transpiled to PHP, never in `differential.rs`.

The reframing the developer locked ("everything transpiles to PHP; the real axis is three-leg
byte-identity") is *exactly* this axis. The contribution of this prior-art survey is to map each
concurrency model onto one of three buckets and extract the deterministic kernel even from the
"non-deterministic" ones.

Three buckets used throughout:
- **D (Deterministic core)** — output is a pure function of program + inputs. Tier-A candidate.
- **Q (Quarantinable)** — non-deterministic *only* at the I/O boundary; the *logic* is deterministic if
  the events are supplied as data. Tier-B with a replay/fixture seam (the `Transport`/`HigherOrder`
  pattern generalizes here).
- **R (Rejected for Phorge)** — non-determinism is intrinsic to the programming model's semantics
  (preemption, shared-mutable threads, real-time). No `php -n` target, not `Send`, not reproducible.

---

## PART A — REACTIVE / FRP (ReactiveX, observables, FRP proper, schedulers, backpressure)

### A.1 ReactiveX (Rx) — observables + operators + schedulers

**What it is.** A push-based dataflow: an `Observable<T>` emits 0..N values then completes or errors;
operators (`map`, `filter`, `scan`, `merge`, `zip`, `flatMap`, `take`, `debounce`, `window`…) compose
into a pipeline; a `Scheduler` decides *when/where* each emission is delivered (immediate, trampoline,
new-thread, event-loop, test/virtual-time).

**How it schedules.** Operators are pure transformations on the *event stream*; the **Scheduler** is the
sole locus of timing and concurrency. With the `immediate`/`trampoline` scheduler an Rx pipeline is
*synchronous and deterministic* — emissions propagate depth-first/breadth-first in a fixed order with no
clock. With `newThread`/`io`/`computation` schedulers it becomes non-deterministic (real threads). The
**TestScheduler / virtual-time scheduler** is the proof that Rx *can* be fully deterministic: it replaces
wall-clock time with a logical clock advanced manually, so `debounce(200ms)` etc. resolve at exact logical
ticks and marble tests are reproducible byte-for-byte.

**Output determinism.** DETERMINISTIC under (a) the immediate/trampoline scheduler, or (b) the
virtual-time scheduler with a logical clock — both eliminate the OS scheduler and the wall clock. The
*combinators themselves* (`map`/`filter`/`scan`/`zip`/`merge` of already-ordered finite sources) are
order-deterministic. NON-DETERMINISTIC the moment a real-thread scheduler or a real-time source (`interval`,
network) is introduced.

**Maps onto Phorge — bucket D (the pure operator layer) + Q (live sources).** The Rx *operator algebra over
a finite, eagerly-known sequence* is just transformations on a `List<T>` with a fixed traversal order —
already byte-identical, already half-shipped (`Core.List.map/filter/reduce` are exactly `map`/`filter`/`scan`
restricted to a materialized list). A **pull-based lazy `Stream<T>`** (cold observable, single
synchronous consumer, immediate scheduler) is the deterministic Tier-A sweet spot: define a stream as a
*description* of a computation pulled by a terminal operator (`forEach`/`collect`/`reduce`), evaluated
single-threaded depth-first, with **no Scheduler choice exposed** (the scheduler is implicitly `immediate`).
That is byte-identical by construction on all three legs.

- **Phorge-syntax sketch (Tier-A, gated):**
  ```
  import Core.Stream;
  // cold, pull-based, synchronous — a lazy List with operators
  var total = Stream.of([1, 2, 3, 4])
      |> Stream.map(fn(int x) => x * x)
      |> Stream.filter(fn(int x) => x > 4)
      |> Stream.fold(0, fn(int acc, int x) => acc + x);   // 9 + 16 = 25
  ```
- **PHP transpile target.** A `Stream` over an eager list is just chained `array_map`/`array_filter`/
  `array_reduce` (the existing `Core.List` erasure), OR a generator-backed pipeline (`yield`) if laziness
  must be preserved for `take`/short-circuit — PHP `Generator` is core, present under `php -n` [Inferred:
  generators are core syntax, not an extension; consistent with Fibers being core]. The deterministic
  pull order makes both backends and PHP agree. **No new VM Op** — `Stream` ops are `HigherOrder` natives
  (closure args, re-entrant invoker) exactly like `Core.List.map`.
- **Byte-identity argument.** Single consumer, single thread, immediate scheduler, eager source ⇒ the
  emission order is `for i in 0..len` — a pure function of the source list. The Rust legs share the
  `value::build_list` kernel + `call_closure` invoker; PHP runs the same fold left-to-right.
  CONFIDENCE: **high**.
- **What is Tier-B / rejected:** any *hot* observable (a live `interval`, a socket, UI events), any
  non-immediate Scheduler, `debounce`/`throttle`/`delay` against the *real* clock. The combinator that
  *merges two live sources by arrival time* is intrinsically non-deterministic → Q with a replayed event
  log (see A.4) or R.

**Sharpest takeaway from Rx:** the operator algebra and the *timing/concurrency* are **already
architecturally separated** in Rx (operators vs Scheduler). Phorge can adopt the operator algebra wholesale
into Tier-A by *hard-wiring the immediate scheduler* and *forbidding live sources in the gated subset* —
the exact split Rx already drew, with Phorge simply declining to expose the non-deterministic schedulers in
the byte-identical tier. The TestScheduler/virtual-time idea is the bridge: a **logical-time scheduler is
itself a deterministic Tier-A construct** (see A.3).

### A.2 FRP proper (Elm, classic FRP: Behaviors + Events; signal graphs)

**What it is.** Classic FRP models time-varying values as `Behavior<T>` (continuous) and discrete
`Event<T>` (occurrences); a static signal graph propagates changes. Elm's architecture (TEA) is the
pragmatic descendant: `Model -> Msg -> (Model, Cmd)` — a *pure* `update` function folds a stream of
messages into a state, with all effects pushed to the edge as `Cmd`/`Sub` (the runtime, not the program,
performs them).

**How it schedules.** The signal graph propagates in **topological order** — a deterministic, glitch-free
single pass per input event (Elm/Reflex guarantee no intermediate "glitch" states are observed). The
program logic is *pure*; the runtime is the only impure part.

**Output determinism.** The *fold over a known message sequence is fully deterministic* — this is just
`reduce(messages, init, update)`. Non-determinism lives entirely in *where the message sequence comes from*
(clicks, timers, HTTP) — pushed to the runtime edge.

**Maps onto Phorge — bucket D (the update fold) is a perfect fit.** The Elm `update : Msg -> Model ->
Model` *pure reducer* is byte-identical by construction: given a fixed `List<Msg>`, folding it is the
existing `Core.List.reduce`. This is arguably the **most Phorge-idiomatic concurrency model in the entire
survey**: it isolates all non-determinism behind a data boundary (the message list / `Cmd`), leaving a pure
core that the three legs trivially agree on.

- **Phorge-syntax sketch (Tier-A core, gated):**
  ```
  // The pure heart: deterministic, byte-identical.
  function update(Msg msg, State s) -> State { match msg { ... } }
  var final = Stream.of(messages) |> Stream.fold(initialState, update);
  ```
- **Tier-B edge:** an actual event loop that *collects* live messages (timers/sockets) and drives
  `update` — that is the `serve.rs` `Transport` pattern generalized (a `MsgSource` trait with a live
  impl and an in-memory fixture impl). The *driver* is Tier-B; the *reducer* is Tier-A and is the only
  thing in `differential.rs`.
- CONFIDENCE: **high** that the pure-reducer core is Tier-A; the edge driver is standard Tier-B.

**Takeaway:** FRP's Behavior/Event distinction is overkill for Phorge's first cut, but **TEA's
"pure reducer + effects-at-the-edge" decomposition IS the recommended shape for any deterministic
reactive feature.** It is the same decomposition `serve.rs` already uses (`respond(bytes)->bytes` pure,
`Transport` impure).

### A.3 Virtual-time / logical-clock schedulers (the deterministic-async keystone)

**What it is.** A scheduler whose clock is a *logical counter advanced by the program/test*, not the wall
clock. Used by Rx TestScheduler, deterministic-simulation testing (FoundationDB), and discrete-event
simulators. Tasks are enqueued with a logical deadline; the scheduler pops the earliest-deadline task,
ties broken by a **stable insertion order**, advancing logical time to that deadline.

**Output determinism.** FULLY DETERMINISTIC — the entire point. Two runs with the same program produce the
same logical-time interleaving because there is no real clock and ties are broken deterministically.

**Maps onto Phorge — bucket D, and this is the keystone for cooperative async (see B.1/B.3).** A
**single-threaded cooperative scheduler over a logical clock** is byte-identical across all three legs
*provided the task-ordering rule is identical in all three*. The ordering rule must be a pure function of
(enqueue order, logical deadlines) — e.g. a min-heap keyed on `(deadline, insertion_seq)`. PHP can
implement the same heap (`SplPriorityQueue` is core, OR a hand-rolled array heap to avoid SPL-extension
ambiguity). The Rust legs share one scheduler kernel (like the value kernels).

- **Byte-identity argument.** If `sleep(d)` means "yield until logical clock ≥ current+d" and the ready
  queue is a deterministic priority structure, then for a fixed program the resume order is a pure
  function — no `Instant::now()`, no OS scheduler. CONFIDENCE: **high**, *contingent on never reading the
  real clock in the gated path* (a `Core.Time.now()` would be Tier-B; logical `sleep`/`delay` is Tier-A).
- **Risk named:** the moment a task's resumption depends on real I/O completing (a real socket read), the
  deadline is wall-clock and determinism is gone → that task source is Tier-B. The clean line is: **logical
  timers + cooperative yields = Tier-A; real timers + real I/O completions = Tier-B.**

**Takeaway:** virtual time is what lets "async/await" be Tier-A at all. Phorge should adopt a logical-time
single-threaded scheduler as the substrate; async/await (B.1) and reactive `interval`/`delay` (A.1) then
ride it deterministically, with real time/IO as the Tier-B escape.

### A.4 Backpressure & flow control (Reactive Streams, pull vs push)

**What it is.** When a producer is faster than a consumer, Reactive Streams (`Publisher`/`Subscriber`/
`Subscription.request(n)`) makes demand explicit: the consumer pulls `n` items; the producer never
overruns. Strategies: buffer, drop, latest, error.

**Output determinism.** In a **single-threaded pull model backpressure is trivially deterministic and in
fact disappears** — the consumer pulls exactly when ready, the producer is a coroutine that runs only when
pulled. Backpressure is fundamentally a *concurrency* artifact (producer and consumer on different
threads); remove the concurrency and it reduces to ordinary lazy evaluation.

**Maps onto Phorge — bucket D, and mostly a non-problem.** A pull-based `Stream<T>` (A.1) with a lazy
producer (a generator-shaped closure or a Fiber that yields) *is* the backpressure-free model. There is
**nothing to design** for the gated tier: demand = a synchronous pull. Only a *live, push-based* Tier-B
source (a real socket firing faster than processed) needs a real buffer/drop policy — and that lives in the
Tier-B I/O layer, fixture-tested.

- CONFIDENCE: **high** that backpressure is a non-issue in the single-threaded deterministic core. It is a
  property the constraint *gives us for free*, not a feature to build.

**Takeaway:** Phorge's single-threaded `Rc` heap, usually framed as a limitation, is here an *advantage* —
the entire Reactive-Streams demand protocol is unnecessary because pull-based lazy streams have no
producer/consumer race.

---

## PART B — ACTOR MODEL (Erlang/Elixir BEAM, message-passing, supervision)

### B.1 Core actor semantics — isolated state, async messages, mailbox

**What it is.** An actor = private mutable state + a mailbox + a behavior `(state, message) -> state'`.
Actors communicate ONLY by asynchronous message send (`!`/`send`); no shared memory; each actor processes
one message at a time to completion (run-to-completion, no internal preemption). BEAM runs millions of
lightweight processes preemptively scheduled across OS threads.

**How it schedules.** BEAM uses **preemptive** scheduling (reduction counting) across multiple OS
schedulers ⇒ the *interleaving of independent actors is non-deterministic*. BUT: (a) within one actor,
message processing is sequential and deterministic; (b) the *causal* order of messages on a single mailbox
from a single sender is preserved (FIFO per sender pair); (c) the model has **no shared mutable state by
construction** — actors are share-nothing.

**Output determinism.** The cross-actor interleaving is NON-DETERMINISTIC on BEAM (preemptive, multi-core).
HOWEVER, the actor model is *defined* independently of the scheduler: a **single-threaded cooperative actor
scheduler with a deterministic message-delivery order is a valid actor runtime** and is fully
deterministic. Many actor libraries ship exactly such a scheduler for testing (Akka TestKit, CAF
deterministic scheduler, Pony's deterministic test mode).

**Maps onto Phorge — bucket D is achievable, and the share-nothing discipline is a GIFT to the `Rc` heap.**
The actor model's *defining constraint* — no shared mutable state, communicate only by message — is
**precisely what makes single-threaded determinism natural** and sidesteps the `Value: !Send` wall: actors
never share a `Value`, they send *copies/owned values* through a mailbox. Phorge can implement a
**single-threaded, cooperative, deterministic actor runtime**:

- One scheduler (the A.3 logical-time scheduler), a run-queue of actors with pending mail.
- `send(actor, msg)` appends to the target's mailbox (a `List` / VecDeque of `Value`), and enqueues the
  actor if idle. Messages are *values*, sent by move/clone — no `Rc` aliasing across actor boundaries
  (preserves share-nothing → no need for `Send`, no cycles, `Rc`/`Drop` still reclaims).
- Delivery order = a **deterministic rule**: process the run-queue in FIFO of "became-runnable" order,
  each actor draining its mailbox to quiescence (or one message per turn — a fixed policy). For a fixed
  program with no real-time inputs, the global interleaving is then a pure function of send order.
- `receive`/behavior = a `match` over the message type (Phorge already has match-over-union + enums).

- **Phorge-syntax sketch (Tier-A cooperative actors, gated):**
  ```
  import Core.Actor;
  enum Msg { Inc(int), Get(Actor) }
  // behavior: (state, msg) -> state'    (pure-ish; may send, may not touch shared state)
  function counter(int state, Msg m) -> int {
      match m {
          Inc(n) => state + n,
          Get(reply) => { Actor.send(reply, state); state }
      }
  }
  function main() -> void {
      var c = Actor.spawn(0, counter);
      Actor.send(c, Inc(5));
      Actor.send(c, Inc(3));
      Actor.run();          // drive the deterministic scheduler to quiescence
  }
  ```
- **PHP transpile target.** A single-threaded actor loop is plain PHP: an array of `[state, mailbox,
  behavior]` records, a run-queue, a `while` draining it, calling the behavior closure. No extension —
  pure core PHP arrays + closures. (NOT `parallel`/`pthreads`/`pcntl_fork` — those are extensions absent
  under `php -n` AND non-deterministic.) The Rust legs share the scheduler kernel; PHP runs the identical
  drain order.
- **Byte-identity argument.** No real threads, no clock, deterministic run-queue discipline, messages are
  values ⇒ the message-processing trace is a pure function of the program. Shared scheduler kernel across
  `run`/`runvm`; PHP mirrors the drain loop. CONFIDENCE: **high** for the *cooperative deterministic*
  variant; the runtime needs care that the run-queue rule is single-sourced (a parity invariant like the
  value kernels).
- **New VM Op?** Likely **none** — `spawn`/`send`/`run` are `HigherOrder` natives (they store and later
  invoke behavior closures via the existing re-entrant `call_closure_value`/`run_until`). The scheduler is
  a Rust struct threaded through the native call, NOT bytecode. This mirrors how `Core.List.reduce`
  re-enters the VM. The one subtlety: the scheduler must persist *across* native calls within one
  `Actor.run()`, so `run` is a single `HigherOrder` native that owns the whole loop internally and only
  returns when quiescent — clean. CONFIDENCE: **medium-high** (needs a spike to confirm the invoker
  lifetime threads cleanly through nested actor turns, but `run_until` already supports nested closure
  frames).

- **What is Tier-B / rejected:**
  - **Preemptive scheduling** (BEAM reduction-counting) → R: non-deterministic interleaving, no `php -n`
    target, and Phorge's `exec_op` has no preemption point. Cooperative run-to-completion is the gated
    substitute.
  - **Distributed actors / real message passing over sockets** → Q (Tier-B): the *transport* is a real
    socket (`serve.rs` `Transport` pattern), non-deterministic arrival; fixture-tested with a replay log.
  - **True multi-core parallelism of actors** → R for shared-state, but see B.4 for the pure-parallel
    escape.

**Takeaway:** the actor model's share-nothing message-passing is the *ideal* concurrency discipline for an
`Rc` heap that can't be `Send` — it never needs to be, because nothing is shared. A **cooperative
deterministic actor scheduler is Tier-A**; only preemption and real distribution are non-gated. This is
the single richest Tier-A opportunity in the survey, but also the largest (a scheduler + mailboxes + a
spawn/send API).

### B.2 Supervision trees & "let it crash"

**What it is.** Actors are organized in supervision trees; a supervisor restarts a crashed child per a
strategy (one-for-one, one-for-all, rest-for-one) with restart-intensity limits. "Let it crash" =
don't defensively code; isolate failure and restart from a known-good state.

**Output determinism.** A restart driven by a *deterministic crash* (a fault on a known input) is itself
deterministic; a restart driven by a transient (network blip) is not. Restart *ordering* under a
deterministic scheduler is reproducible.

**Maps onto Phorge — bucket D for the supervision *logic*, given Phorge's existing fault model.** Phorge
already has a typed fault/`throws`/`Result` error model (M-faults Slice 2, CLOSED) with byte-identical
fault traces across backends. A supervisor is just: run a child behavior; if it faults, apply a restart
strategy (a `match` on the strategy + a restart counter). Because faults are already deterministic and
byte-identical, **supervision-tree restart behavior is byte-identical** as long as it rides the
deterministic scheduler (B.1) and the crash is deterministic.

- **Phorge fit:** layer on top of `Core.Actor` once B.1 exists. A `Supervisor` is an actor whose children
  are actors; restart strategies are an enum. CONFIDENCE: **medium** (depends entirely on B.1 landing
  first; the restart logic itself is trivial and deterministic).
- **Tier-B caveat:** restart-intensity limits that use *wall-clock windows* ("max 3 restarts in 5
  seconds") are non-deterministic → use *logical* time (count restarts per N scheduler ticks) to stay
  Tier-A, or push the wall-clock window to Tier-B.

**Takeaway:** supervision is a *cheap, deterministic add-on* to a cooperative actor runtime because Phorge
already owns a byte-identical fault model — but it is strictly downstream of B.1 and should not be designed
before it.

### B.3 Cooperative async/await (Fibers/coroutines) — the bridge between Rx and actors

**What it is.** `async`/`await` over a single-threaded event loop (JS, Python asyncio, PHP
ReactPHP/Amp/Fibers): a function suspends at `await`, the scheduler resumes it when its awaited value is
ready. No preemption — suspension points are explicit (`await`).

**How it schedules.** A cooperative single-threaded loop. Determinism depends ENTIRELY on what is awaited:
awaiting a *logical* timer or an *already-resolved* value ⇒ deterministic resume order; awaiting *real
I/O* ⇒ non-deterministic.

**Output determinism.** DETERMINISTIC over logical/resolved awaits with a deterministic ready-queue
(exactly A.3). NON-DETERMINISTIC over real I/O.

**Maps onto Phorge — bucket D core, and PHP Fibers are the transpile target (VERIFIED present under
`php -n`).** This is the developer's locked "cooperative async/await over a DETERMINISTIC single-threaded
scheduler → PHP 8.1 Fibers" decision, and the prior-art confirms it is sound:
- **Rust legs:** a single-threaded scheduler (A.3) + suspendable user functions. The cleanest std-only
  Rust mechanism is **NOT** real coroutines (Rust async needs an executor crate — forbidden, zero-dep) but
  a **scheduler-as-native** that drives user *closures/continuations* via the existing re-entrant
  `call_closure_value`/`run_until` — i.e. CPS or a state-machine'd task, where each `await` is a native
  that yields control back to the `Actor.run`-style driver. The VM does NOT need stackful coroutines:
  `run_until` already supports nested re-entrant frames; a task is a closure the scheduler resumes.
  CONFIDENCE: **medium** — needs a spike to confirm a Phorge `async fn` can be *lowered to a
  resumable form the existing invoker can step*; the alternative (a real new `Op` for suspend/resume) is
  the fallback if CPS lowering is too invasive (then it costs the 3 coupled matches).
- **PHP leg:** PHP 8.1 `Fiber` (core, present under `php -n` [Verified]) is the natural target — a Phorge
  `async fn` transpiles to a fiber, `await` to `Fiber::suspend`, the scheduler to a `Fiber::resume` loop.
  This is the ONE place Fibers pay off and they are available.
- **Byte-identity argument.** Over logical timers + resolved values + a deterministic ready-queue, the
  resume order is a pure function ⇒ byte-identical. The risk is the **scheduler ordering rule diverging
  between the Rust kernel and the PHP fiber loop** — must be single-sourced conceptually (same heap/FIFO
  discipline documented as a parity invariant). CONFIDENCE: **medium-high** for the deterministic subset,
  **explicitly Tier-B** for any `await realSocket`/`await realTimer`.

**Takeaway:** async/await is *the bridge* — it is the user-facing surface that both Rx (`interval`,
`delay`) and actors (`receive` as `await mailbox`) can be expressed over. Build the deterministic
single-threaded scheduler (A.3) ONCE; async/await, streams, and actors are all thin layers atop it. PHP
Fibers make the transpile honest. This is the highest-leverage substrate to build first.

### B.4 Pure data-parallelism (fork/join, parallelMap) — the locked-in safe path

**What it is.** Apply a *side-effect-free* function to many inputs independently, merge results in a
**deterministic, order-preserving** way (`parallelMap`, fork/join, `Stream.parallel().map().collect()`).
NO shared mutable state; the only "concurrency" is that independent pure computations *could* run on
different cores.

**Output determinism.** FULLY DETERMINISTIC *by the merge contract*: if the merge preserves input order
and the mapped function is pure, the output is identical whether the work ran sequentially or in parallel.
This is the developer's locked decision ("(b) PURE data-parallelism … all legs sequential today =
byte-identical, with Rust-side physical parallelism as a LATER optimization that must preserve output").

**Maps onto Phorge — bucket D, and it is essentially FREE today.** `parallelMap(list, pureFn)` is, *as a
semantics*, identical to `Core.List.map` — the only difference is an *implementation* freedom to run the
maps concurrently. Today all three legs run it sequentially ⇒ byte-identical with `map`. The "parallel" is
a **promise about referential transparency**, enforced by: the mapped function must be pure (no impure
native, no `this`-mutation — Phorge can check "calls no `pure:false` native" statically, reusing the same
`uses_impure_native` analysis at the type level).

- **Phorge-syntax sketch (Tier-A, gated):**
  ```
  import Core.Parallel;
  var squared = Core.Parallel.map([1,2,3,4], fn(int x) => x * x);  // [1,4,9,16], order preserved
  ```
- **PHP transpile target.** `array_map` (sequential, deterministic) — PHP has no safe parallel under
  `php -n`, and it doesn't matter because the *contract* is order-preserving. CONFIDENCE: **high**.
- **Rust-side physical parallelism** is a LATER, std-only-feasible optimization (`std::thread::scope` over
  chunks) — BUT it cannot touch the `Rc` heap (`!Send`). It would require the mapped function + its inputs
  to be *deep-cloned into owned, `Send`-safe values* per chunk (or the parallel path restricted to
  scalar/`Copy` element types initially). This is genuinely hard with `Rc`-shared `Value` and should be
  **deferred** — ship the sequential semantics now (free), optimize later only if a benchmark demands it,
  and only by cloning across the thread boundary. The key insight: **the user-visible semantics never
  changes**, so the parallel optimization is invisible and gate-safe by construction.
- **New VM Op?** None — `Core.Parallel.map` is a `HigherOrder` native, byte-identical to `Core.List.map`
  until/unless the Rust backend privately parallelizes.

**Takeaway:** pure data-parallelism is the lowest-risk, highest-determinism concurrency feature — it is
*already byte-identical* because the parallel implementation is an unobservable optimization over a pure
ordered map. Ship the semantics immediately as a `HigherOrder` native; treat physical threading as a
deferred, optional, output-preserving perf tweak gated on a benchmark (and walled off from the `Rc` heap by
cloning).

---

## PART C — Cross-cutting synthesis: what maps, what doesn't, in what order

### C.1 The determinism verdict table

| Model | Scheduling | Output determinism | Phorge bucket | New `Op`? | Transpile target | Confidence |
|---|---|---|---|---|---|---|
| Rx operator algebra over eager list | immediate | deterministic | **A** (D) | no (HigherOrder) | `array_map/filter/reduce` | high |
| Lazy pull `Stream<T>` (cold) | immediate/pull | deterministic | **A** (D) | no | generator / chained array fns | high |
| Rx live sources / non-immediate scheduler | real thread/clock | NON-det | **B** (Q) or R | — | fiber loop (Tier-B) | high (that it's Tier-B) |
| FRP TEA pure reducer | topo / fold | deterministic | **A** (D) | no | `array_reduce` | high |
| Virtual-time / logical scheduler | logical clock | deterministic | **A** (D) | maybe (suspend/resume) | hand-rolled heap loop | high |
| Backpressure (Reactive Streams) | pull | deterministic (trivial) | **A** (D) — non-issue | no | n/a (lazy eval) | high |
| Cooperative actors (share-nothing) | logical run-queue | deterministic | **A** (D) | likely none | PHP array actor loop | medium-high |
| Supervision trees | logical, on deterministic faults | deterministic | **A** (D), downstream of actors | no | PHP match on strategy | medium |
| Cooperative async/await | logical event loop | deterministic (logical awaits) | **A** (D) core | maybe (suspend/resume) | **PHP 8.1 Fibers (verified)** | medium-high |
| Pure data-parallelism (parallelMap) | sequential-now, parallel-later | deterministic (ordered merge) | **A** (D) | no (HigherOrder) | `array_map` | high |
| BEAM preemptive scheduling | reduction-counted, multi-core | NON-det | **R** | — | — | high (reject) |
| Real-thread Rx schedulers / OS threads sharing state | preemptive | NON-det | **R** | — | — | high (reject) |
| Distributed actors over sockets | real network | NON-det at edge | **B** (Q) | no (Transport seam) | fiber + socket (Tier-B) | high |
| Real timers / real I/O completions | wall clock | NON-det | **B** (Q) | — | Tier-B `Transport`/`Time` | high |

### C.2 The one substrate that unlocks everything: a deterministic single-threaded scheduler

Every Tier-A model above reduces to **one shared primitive**: a single-threaded cooperative scheduler over
a *logical* clock with a deterministic ready-queue (A.3). On top of it:
- **async/await** (B.3) = tasks that suspend at `await`, resumed by the scheduler (PHP Fibers leg).
- **cooperative actors** (B.1) = scheduler entries that are mailbox-draining behaviors.
- **reactive streams' `interval`/`delay`** (A.1) = logical timers on the scheduler.
- **supervision** (B.2) = restart logic in the scheduler's actor turns.

Building this scheduler ONCE, as a Rust kernel single-sourced across `run`/`runvm` (the value-kernel
discipline) with a documented ordering invariant the PHP leg mirrors, is the central architectural move.
The re-entrant `call_closure_value`/`run_until` invoker already proves the VM can be *driven by a native*
that steps user code — the scheduler is the same idea, persisted across an `Actor.run()`/`asyncMain()`
boundary. CONFIDENCE: **medium-high** that no new `Op` is needed (scheduler-as-native), with a new
suspend/resume `Op` as the named fallback if CPS-lowering `await` proves too invasive.

### C.3 The `Rc`-heap constraint is an ASSET here, not just a limitation

The recurring surprise across the survey: Phorge's "limitations" are *gifts* for deterministic concurrency.
- `Value: !Send` forbids shared-state threads — which is exactly what the actor model and pure-parallelism
  *also* forbid. Phorge is structurally pushed toward the deterministic, message-passing/share-nothing
  designs, which are *also* the byte-identical ones.
- Single-threaded forces cooperative scheduling — which is exactly the deterministic kind.
- Backpressure (a multi-threaded artifact) disappears entirely in pull-based single-threaded streams.

The non-deterministic models (BEAM preemption, real-thread Rx, OS threads) are rejected for the *same*
reason they'd break byte-identity: they need `Send`, real schedulers, and have no `php -n` target. The
constraints are self-consistent.

### C.4 Per-feature Tier recommendation (the locked "case-by-case" call)

- **Tier-A, ship-able now, lowest risk:** `Core.Parallel.map` (B.4) and a lazy pull `Stream<T>` with the
  Rx operator algebra restricted to eager/lazy single-consumer pipelines (A.1) — both are `HigherOrder`
  natives, no new `Op`, byte-identical by construction, transpile to `array_*`/generators.
- **Tier-A, high-value, larger build (needs the scheduler substrate + a spike):** cooperative async/await
  (B.3, PHP Fibers) and the cooperative deterministic actor runtime (B.1). These share the A.3 scheduler.
  Recommend a **spike** to confirm scheduler-as-native vs a suspend/resume `Op` before committing.
- **Tier-A, cheap add-on, strictly after actors:** supervision trees (B.2) and FRP/TEA reducer helpers
  (A.2 — arguably just a documented pattern over `Stream.fold`, may need no new API at all).
- **Tier-B (fixture-tested, transpiled, out of `differential.rs`):** any live source — real timers, real
  sockets/distributed actors, real-I/O `await`. Reuse the `serve.rs` `Transport` seam pattern (a live impl
  + an in-memory replay impl) and the `pure:false` quarantine flag.
- **Rejected (R):** preemptive scheduling, shared-mutable OS threads, BEAM-style multi-core actor
  parallelism with shared state — `!Send`, non-deterministic, no `php -n` target.

### C.5 Named determinism risks to design against
1. **Scheduler ordering drift between the Rust kernel and the PHP fiber/array loop** — the ready-queue
   discipline must be single-sourced as a documented parity invariant (like the value kernels); a marble/
   trace test in `differential.rs` should pin a multi-task interleaving byte-for-byte.
2. **Accidental real-clock leakage** — a `Core.Time.now()` or `Instant::now()` anywhere in the gated path
   silently destroys determinism. Logical time only in Tier-A; real time is `pure:false`.
3. **Closure purity for `parallelMap`** — must statically reject a mapped fn that calls a `pure:false`
   native (reuse the `uses_impure_native` analysis at check time), or the "parallel" promise is a lie.
4. **`Rc` aliasing across actor mailboxes** — messages must be sent by owned value/clone so no `Value` is
   shared across actor boundaries (preserves share-nothing AND the acyclic-heap `Rc`/`Drop` reclamation —
   no GC needed, consistent with the M2 decision).
5. **Re-entrant invoker lifetime across scheduler turns** — `run_until` supports nested closure frames, but
   a *persisted* scheduler that resumes tasks across many turns is a deeper re-entrancy than `List.reduce`;
   the named spike must confirm the borrow/lifetime threads cleanly (medium-confidence item).

---

## D. std-only Rust feasibility notes
- Deterministic scheduler, mailboxes, run-queue, logical clock: trivially std-only (`Vec`/`VecDeque`/
  `BinaryHeap`, no crate). [Verified: all in `std`.]
- Cooperative tasks: best via scheduler-as-native + re-entrant invoker (already in-tree) OR a new
  suspend/resume `Op`; **NOT** Rust `async` (needs an executor crate — forbidden). [Inferred from zero-dep
  invariant + existing `run_until`.]
- Physical parallelism (deferred B.4 optimization): `std::thread::scope` exists in std, but `Value: !Send`
  means inputs must be deep-cloned to owned `Send`-safe values per chunk — feasible but non-trivial; defer
  until a benchmark demands it. [Inferred.]
- PHP leg: Fibers (verified present under `php -n`), core arrays/closures for actor loops, generators for
  lazy streams (core syntax). No extension needed for any Tier-A model. [Verified for Fibers; Inferred for
  generators/closures being core.]

## E. Bottom line for the design stage
Build **one** deterministic single-threaded logical-time scheduler (A.3) as the substrate. Ship the two
zero-risk wins first (`Core.Parallel.map`, lazy `Stream`). Spike async/await (PHP Fibers) and cooperative
actors on the shared scheduler — both Tier-A, both likely needing **no new `Op`** (scheduler-as-native via
the existing re-entrant invoker), with a suspend/resume `Op` as the named fallback. Supervision and TEA
reducers are cheap downstream add-ons. Everything live (real timers/sockets/distributed) is Tier-B behind
the proven `Transport`/`pure:false` quarantine. Preemption and shared-state threads are rejected — they are
exactly the things that would break both byte-identity AND the `Rc` heap, so the constraint is
self-consistent.
