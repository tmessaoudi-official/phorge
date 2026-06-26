# Concurrency / Async / Reactive Prior-Art — Rust & JavaScript, through the Phorge byte-identity lens

**Stage 1 of the extended-modules research.** Lens = Rust (async/await + executors like tokio,
plus rayon data-parallelism / fork-join) and JavaScript (single-threaded event loop, Promises,
async/await, microtask determinism). Goal: catalogue each concurrency/async/reactive model, how it
*schedules*, whether its *output* is deterministic and under what constraints, and how (if at all)
it maps onto Phorge's single-threaded `Rc`-heap + byte-identity-or-Tier-B reality.

Verified codebase facts this analysis is built on (read this session):
- `Cargo.toml` core crate `[dependencies]` is empty; only `playground/` (wasm32-only) pulls
  `wasm-bindgen`. `#![forbid(unsafe_code)]`, `warnings=deny`, `clippy::all=deny`. [Verified]
- `Value` is `Rc`-shared (`src/serve.rs` doc: "the `Rc`-shared heap (P5a) makes `Value`
  non-`Send`, so a thread pool is impossible"). Single-threaded is *forced*, not chosen. [Verified]
- The `pure:false` quarantine seam exists and is **generic**: `tests/differential.rs::uses_impure_native`
  reads `phorge::native::registry().filter(|n| !n.pure)` and SKIPs any program importing such a
  module — no hardcoding (`differential.rs:916`, `:1004`). `Core.Process`/`Core.Env` are the live
  precedent (`src/native/process.rs`), fixture-tested in `tests/process.rs`. [Verified]
- The `Transport` trait (`src/serve.rs:25`) already quarantines sockets + wall-clock out of the
  spine; `tests/serve.rs` swaps an in-memory transport for determinism. [Verified]
- The VM is already **re-entrant**: `Vm::call_closure_value` + `run_until` (`src/vm/closure.rs`)
  drive the *shared* `exec_op` for a closure called from a `HigherOrder` native — so a scheduler
  that resumes Phorge closures has a working primitive today. [Verified]
- `std::thread::scope` appears once (`src/cli/mod.rs:257`) purely to run the pipeline on a
  256 MB-stack worker — NOT user-facing concurrency. [Verified]

---

## 0. The deterministic/non-deterministic dividing line (the whole point)

A concurrency model is byte-identity-safe **iff its observable output is a pure function of the
program text and its (gated, fixed) inputs** — i.e. the *interleaving* of concurrent work is either
(a) fixed by the language semantics, or (b) irrelevant because the work is side-effect-free and
merged in a fixed order. Everything that admits a *scheduler-dependent* or *wall-clock-dependent*
observable is Tier-B (quarantined, fixture-tested) at best, or rejected.

| Property of the model | Deterministic output? | Phorge tier |
|---|---|---|
| Cooperative scheduling, fixed run-to-completion order | **Yes** (order is a language rule) | **A — gated** |
| Pure data-parallelism, ordered merge | **Yes** (merge order fixed; bodies side-effect-free) | **A — gated** |
| Reactive/FRP over a *deterministic* source (a fixed list/range) | **Yes** | **A — gated** |
| Microtask vs macrotask ordering (JS) | Yes *within* a fixed task set | A — if sources gated |
| `Promise.race` / first-settled-wins over real I/O | **No** (timing decides winner) | **B** (or reject) |
| Reactive over a *live* source (timer/socket/clock) | **No** | **B** |
| Preemptive OS threads sharing mutable state | **No** (interleaving is the OS's) | **REJECT** |
| Real-time events (`setTimeout`, intervals, animation frames) | **No** (wall clock) | **B** |

The locked developer decision matches this exactly: safe paths = (a) cooperative async/await over a
deterministic single-thread scheduler, (b) pure data-parallelism with ordered merge, (c) reactive
over deterministic sources; Tier-B escape = genuinely-live concurrency; shared-mutable OS threads =
HARD NO.

---

## 1. JavaScript — the model Phorge should *be*, semantically

### 1.1 The single-threaded event loop (run-to-completion + microtask queue)

**How it schedules.** One call stack. Work units (tasks/macrotasks) run **to completion** — a task
is never preempted mid-execution. Between tasks, the engine drains the **microtask queue**
(Promise callbacks, `queueMicrotask`) fully before picking the next macrotask. Within each queue,
order is **strict FIFO**.

**Determinism.** *Given a fixed set of enqueued work*, the output is **fully deterministic**: FIFO +
run-to-completion + "drain microtasks before next macrotask" is a total order with zero scheduler
freedom. The famous example —

```js
console.log('A');
setTimeout(() => console.log('D'), 0);   // macrotask
Promise.resolve().then(() => console.log('C')); // microtask
console.log('B');
// → A B C D, every time, every engine
```

— is byte-identical across V8/SpiderMonkey/JSC *because the ordering is specified*, not because of
luck. **The non-determinism in real JS comes entirely from the *sources* that enqueue work** (timers
fire by wall clock, `fetch` resolves by network), never from the loop itself.

**The Phorge lesson (high confidence).** Phorge can adopt **exactly this scheduler** — a single
cooperative event loop with a FIFO ready-queue and a microtask-before-next-task drain rule — and get
*deterministic output for free*, **provided every source that enqueues work is itself gated/pure**.
This is the cleanest fit for the `Rc`-heap: one stack, no `Send`, no data races possible.

### 1.2 Promises + async/await

A `Promise` is a state machine (`pending → fulfilled|rejected`) whose `.then` callbacks are
**microtasks**. `async fn` is sugar: `await` suspends the function, schedules resumption as a
microtask when the awaited value settles. **Resolution order is deterministic** given deterministic
producers. `await` of an already-resolved value still yields one microtask tick (a spec rule that is
itself deterministic).

`Promise.all([...])` — runs all, resolves when *all* settle, **preserves input order in the result
array** regardless of settle order. **This is the canonical ordered-merge primitive** and is
deterministic by construction. `Promise.allSettled` likewise. `Promise.race`/`Promise.any` resolve
to the *first* settled — **non-deterministic if the inputs settle by real time** (Tier-B), but
*deterministic* if the inputs are eager/already-resolved (the winner is the first in source order
that's ready, a spec rule).

### 1.3 Generators / iterators (`function*`, `yield`)

Cooperative coroutines: explicit `yield` suspends, `.next()` resumes. **Fully deterministic** — the
caller drives every step. This is the lower-level primitive async/await is built on. Maps directly to
a Phorge coroutine the scheduler resumes.

### 1.4 Reactive — RxJS observables / async iterators

`Observable`/`AsyncIterator` push values over time. **Determinism is entirely a property of the
source**: an observable over `from([1,2,3])` (a fixed array) emits a fixed sequence → deterministic;
an observable over `interval(1000)` or DOM events → non-deterministic. Operators (`map`/`filter`/
`scan`/`reduce`) are pure transforms — they preserve determinism. **So a reactive *pipeline* is
Tier-A iff its source is gated.**

### 1.5 Web Workers (the one true-parallel JS escape)

Separate threads with **no shared mutable memory** — communication is by structured-clone message
passing (`postMessage`). Output ordering of messages from multiple workers is **non-deterministic**
(real OS scheduling). This is JS's "Tier-B live concurrency" and confirms the universal pattern:
*even JS only gets real parallelism by giving up shared mutable state and accepting non-determinism.*

---

## 2. Rust — the implementation toolbox (and its determinism traps)

### 2.1 `async`/`.await` + executors (tokio, async-std, smol)

**How it schedules.** `async fn` compiles to a state-machine `Future`; an **executor** polls futures
to completion, parking them on `Pending` and waking them via a `Waker`. Single-threaded executors
(`tokio::runtime` `current_thread`, `LocalSet`, `smol::LocalExecutor`) run futures cooperatively on
one thread — **no `Send` bound required**, so they can hold `Rc`/non-`Send` data. Multi-threaded
executors (`tokio` default work-stealing) require `Future: Send` and steal tasks across threads.

**Determinism.**
- *Single-threaded executor, fixed ready futures*: deterministic *if* the poll order is fixed.
  But `tokio::select!` and `FuturesUnordered` poll in a **non-specified / pseudo-random order** (tokio
  randomises `select!` branch polling to avoid starvation) → **non-deterministic** without care.
- *Multi-threaded work-stealing*: **non-deterministic** by design (which worker grabs which task is a
  runtime decision).
- The futures themselves are deterministic; the **executor's polling order is the non-determinism**.

**The Phorge lesson (high confidence).** Phorge must NOT expose a tokio-style executor with
`select!`/`FuturesUnordered` semantics as a *gated* feature — the unspecified poll order breaks
byte-identity. Phorge's own scheduler must instead use a **specified, fixed** poll/ready order
(FIFO, like JS), giving up tokio's anti-starvation randomisation in exchange for determinism. The
*mechanism* (a state machine polled to completion, parked on suspend) is reusable; the *scheduling
policy* must be Phorge's own deterministic one, not tokio's. Crucially, **Phorge does not need any
async runtime crate** — the existing re-entrant VM (`run_until`/`call_closure_value`) already
suspends and resumes Phorge code; a cooperative scheduler is a `VecDeque` of resumable closures, not
a `Future` executor. Std-only feasible. [Verified mechanism exists; scheduler design is Speculative]

### 2.2 rayon — data-parallelism / fork-join (THE model for `parallelMap`)

**How it schedules.** `par_iter().map(f).collect()` splits a collection across a work-stealing thread
pool; results are **reassembled in the original index order**. `join(a, b)` forks two closures,
runs them (possibly in parallel), returns both results. The hallmark: **the *merge* is order-
preserving even though the *execution* is parallel and out-of-order.**

**Determinism.** rayon's output is deterministic **iff the mapped closures are pure (no shared
mutable state, no order-dependent side effects)** — which rayon's API *encourages* but does not
*enforce* (you can `par_iter().for_each(|x| vec.lock().push(x))` and get non-deterministic order).
For a *pure* `map`/`filter`/`reduce` with an **associative** reduce, the result is bit-identical to
the sequential version, every run.

**The Phorge lesson (high confidence — this is the core of decision (b)).** This is the *exact*
shape the developer locked: `parallelMap(list, fn)` / fork-join over **side-effect-free** functions
with a **deterministic, order-preserving merge**. The byte-identity argument is airtight:

> Today, all three Phorge legs run the bodies **sequentially in index order**, so the output is
> trivially identical to a plain `List.map`. Rust-side *physical* parallelism (a rayon-style split)
> is a **pure performance optimization added LATER** that, by the order-preserving-merge contract,
> produces byte-identical output to the sequential run. The PHP leg stays sequential
> (`array_map`). So `parallelMap` is **Tier-A, gated, byte-identical on day one**, and the parallel
> speedup is an invisible engine upgrade — never an API or output change.

The enforcement Phorge *can* do that rayon can't: the closure passed to `parallelMap` is already
**`E-LAMBDA-THIS`-restricted and captures by value** (immutable `Rc` heap), and the checker can
require the body be side-effect-free (no `Console.*`, no `pure:false` native) → the purity rayon
merely *encourages* is **statically enforced**. Reduce must be declared **associative** (or only a
fixed set of built-in associative reducers `sum`/`min`/`max`/`concat` offered) so the split is sound.

Mechanism reuse: `parallelMap`/`parallelReduce` are **`HigherOrder` natives** — same shape as
`Core.List.map`, using the existing `ClosureInvoker`. **No new `Op`.** Sequential impl ships first;
the parallel impl is a later, output-gated `src`-internal change. The body purity check is a new
checker pass but front-end-only. Std-only: a future parallel impl would need `std::thread::scope`
(already used elsewhere) over `Send`-able *inputs only* — but inputs to a pure map are cloned
`Value`s, and **`Value` isn't `Send`** → the parallel impl actually needs the bodies to operate on
`Send` projections (e.g. parallelise over `i64`/`String`, not `Rc<Instance>`). **This is the real
constraint and a likely reason the first (and possibly only) shipped impl stays sequential.**
[Verified `Value` non-Send; parallel feasibility Medium confidence — gated on a Send-able element subset]

### 2.3 `std::thread` + channels (`mpsc`) / scoped threads

OS threads sharing state via `Arc<Mutex<_>>` or messaging via `mpsc`. **Interleaving is the OS
scheduler's** → output ordering is **non-deterministic** for anything observable across threads.
`Value` is non-`Send` so this is doubly impossible to gate. **REJECT** as a gated feature (matches
the locked "shared-mutable OS threads = HARD NO"). A *message-passing* actor model with a
**deterministic delivery order** (single consumer, FIFO mailbox) could in principle be Tier-A, but
it collapses to "the cooperative scheduler of §1.1 with mailboxes" — i.e. not really threads.

### 2.4 `std::sync::atomic`, `Mutex`, `RwLock`

Shared-memory synchronisation primitives. Only meaningful with real threads → same verdict as §2.3.
(Note: `process.rs` uses `RwLock` for the `PROCESS_ARGS` global, but that's process-setup state set
*before* the single-threaded run, not concurrency.) Not a user-facing concurrency surface. REJECT.

### 2.5 `Future` combinators (`join!`, `select!`, `try_join!`)

`join!(a, b)` polls both to completion, **returns results in argument order** — the Rust analogue of
`Promise.all`, **deterministic, ordered merge** → maps to Tier-A. `select!` returns the
**first-ready** branch in **randomised poll order** → **non-deterministic** → Tier-B / reject as
gated. The split mirrors JS `Promise.all` (safe) vs `Promise.race` (unsafe).

---

## 3. Synthesis — the mapping onto Phorge

### 3.1 The recommended gated (Tier-A) surface

A **JS-shaped cooperative model on a deterministic scheduler**, with a rayon-shaped pure-parallel
escape hatch. Concretely, three layers, all byte-identical across the three legs:

**(a) Cooperative async/await — `async`/`await`, deterministic event loop.**
- *Scheduler*: a single FIFO ready-queue + microtask-drain-before-next-task, **JS ordering rules
  adopted verbatim** (the only ordering that's both intuitive-to-PHP/JS devs *and* total/deterministic).
- *Phorge surface sketch*:
  ```
  async function fetchAll() -> List<int> {
      var a = await compute(1);   // suspends, resumes deterministically
      var b = await compute(2);
      return [a, b];
  }
  ```
- *Transpile target*: PHP 8.1 **Fibers** (present under `php -n` — confirmed in the prompt). An
  `async fn` ↔ a Fiber; `await` ↔ `Fiber::suspend`/`resume`; the event loop ↔ a small PHP scheduler
  emitted as a runtime helper (`__phorge_scheduler`), driven in the **same FIFO order** as the Rust
  legs. The Rust legs drive their own scheduler over resumable closures via the existing
  `run_until`/`call_closure_value` primitive.
- *Byte-identity argument*: ordering is a **language rule** (FIFO + microtask drain), identical in
  all three legs by construction; no wall clock, no scheduler freedom. The await-able producers must
  themselves be pure/gated (a pure computation, a gated data source) — **awaiting real I/O is Tier-B**
  (§3.2).
- *New VM Op?* **Likely none** — suspension/resumption already exists (`run_until`). A coroutine is a
  closure + a saved continuation; the scheduler is a `VecDeque<Resumable>` in the interpreter/VM, not
  a bytecode primitive. *If* a dedicated `Op::Await`/`Op::Yield` proves cleaner than desugaring, it
  costs the usual 3 coupled matches — but the design should first try desugaring `await` into a
  `MakeClosure` + scheduler-native call. [Medium confidence — needs a spike]
- *Determinism risk named*: the ONLY risk is a non-FIFO or wall-clock-dependent enqueue. Mitigation:
  no `setTimeout`-style timer in the gated set (timers are Tier-B); `await` only of pure/gated values.

**(b) Pure data-parallelism — `Core.Parallel.map` / `.reduce` (rayon-shaped, ordered merge).**
- *Phorge surface sketch*:
  ```
  import Core.Parallel;
  var doubled = Parallel.map(xs, fn(int x) => x * 2);     // List<int>, input order preserved
  var total   = Parallel.reduce(xs, 0, fn(int a, int b) => a + b);  // associative reducer
  ```
- *Transpile target*: PHP `array_map` / `array_reduce` (sequential — PHP has no gated parallelism;
  output identical because the merge is order-preserving).
- *Byte-identity argument*: all legs sequential today ⇒ identical to `List.map`. A later Rust-side
  parallel split preserves output by the ordered-merge + pure-body contract. **Strongest Tier-A case
  in the whole space** — it's gated *and* the optimization is invisible.
- *New VM Op?* **None** — `HigherOrder` native, reuses `ClosureInvoker`.
- *Determinism risk named*: a side-effecting body (`Console.*` inside the map) would make a future
  parallel impl non-deterministic. Mitigation: **checker enforces a pure body** (no `pure:false`
  native, no `this`); reduce restricted to associative reducers (built-in set, or a declared property).
- *Std-only feasibility*: sequential = trivially std-only. Parallel = `std::thread::scope` over a
  **`Send`-able element subset** (scalars/strings), since `Value` is non-`Send`. The parallel impl
  may be permanently limited to scalar element types — that's an acceptable optimization scope.

**(c) Reactive / streams over deterministic sources — `Core.Stream` (Rx-shaped, pull-based).**
- *Phorge surface sketch*:
  ```
  import Core.Stream;
  var result = Stream.of([1, 2, 3])      // deterministic source
      .map(fn(int x) => x * 2)
      .filter(fn(int x) => x > 2)
      .collect();                         // List<int>
  ```
- *Transpile target*: PHP generators (`yield`) or eager array pipeline — both deterministic over a
  fixed source.
- *Byte-identity argument*: a stream over a **fixed list/range** is just lazy `map`/`filter` — pure,
  order-preserving. Tier-A iff the source is gated (a list, a `Core` range). A stream over a **live**
  source (a socket, a timer, `Core.Time.now`) is **Tier-B**.
- *New VM Op?* None — desugars to the existing higher-order natives / a generator built on coroutines
  from (a).
- *Determinism risk named*: a live/timed source. Mitigation: gated streams accept only deterministic
  sources; live sources route through the Tier-B stream variant.

### 3.2 The Tier-B escape (genuinely-live concurrency)

Quarantined exactly like `Core.Process`/`Core.Env`: `pure:false` natives, auto-SKIPped from
`differential.rs` by the existing seam, fixture-tested in a dedicated `tests/*.rs` over a
deterministic `Transport`-style seam, transpiled to PHP, documented per-backend.

Candidates (per-feature admission, not blanket): real socket I/O in an async context (already the
M6 `Transport` quarantine), wall-clock timers (`setTimeout`/`Core.Time`-driven scheduling),
`select`/`race`-style first-ready combinators over live producers, side-effecting physical
parallelism. Each is `pure:false` ⇒ out of the spine ⇒ fixture-tested. **No new mechanism needed** —
the seam already exists and is generic.

### 3.3 What is REJECTED (matches the locked decision)

Shared-mutable-state OS threads, `Arc<Mutex>`/atomics as a user surface, tokio-style work-stealing,
`select!`-with-randomised-poll as a *gated* feature. Reasons, all verified: `Value` is non-`Send`
(no shared mutable cross-thread state is even expressible); the interleaving is the OS's (no
byte-identity possible); no `php -n` target for true threads (PHP has no shared-memory threading in
core — `pcntl`/`parallel` are absent extensions).

### 3.4 The one cross-cutting design rule this stage surfaces

**Determinism lives in the *scheduler's ordering policy* and the *purity of sources/bodies*, never in
the parallelism mechanism.** Both JS (specified loop order) and rayon (ordered merge over pure
bodies) prove the same thing: you get deterministic concurrency by *fixing the order* and *forbidding
shared mutation* — both of which Phorge's `Rc`-heap + checker can enforce *more strongly* than either
prior-art language. The mechanisms (state-machine suspension, work-stealing split) are reusable; the
**ordering policy must be Phorge's own deterministic one** (FIFO + microtask drain for async;
index-order merge for parallel), explicitly NOT tokio's randomised `select!` or rayon's
unspecified-without-purity behavior.

---

## 4. Confidence summary

| Claim | Confidence | Basis |
|---|---|---|
| JS event-loop ordering is fully specified & deterministic given fixed work | High | Spec (HTML/ECMAScript), universal cross-engine behavior |
| rayon output is deterministic iff bodies pure + reduce associative | High | rayon docs/semantics; order-preserving collect |
| tokio `select!`/`FuturesUnordered` poll order is non-specified/random | High | tokio docs (bias randomisation) |
| Phorge can adopt JS scheduler semantics for byte-identical async | High | maps to fixed FIFO ordering; PHP 8.1 Fibers present under `php -n` |
| `parallelMap` is Tier-A byte-identical day-one (sequential), parallel later invisible | High | all legs sequential today = identical to `List.map`; ordered-merge contract |
| A future Rust parallel `parallelMap` impl is limited by `Value` non-`Send` | Medium | verified `Value` non-Send; parallel needs Send-able scalar element subset |
| Async needs NO new VM Op (desugar over `run_until`) | Medium | re-entrant primitive verified; whether desugar beats a dedicated Op needs a spike |
| Reactive streams over fixed sources are Tier-A | High | reduces to lazy pure map/filter |
| Shared-mutable OS threads are unmappable (reject) | High | `Value` non-Send + no `php -n` threading target |
| Tier-B live concurrency reuses the existing quarantine seam unchanged | High | `uses_impure_native` is generic; `Transport` seam exists |
