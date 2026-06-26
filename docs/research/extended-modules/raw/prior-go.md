# Concurrency Prior-Art — Go (goroutines, channels, CSP, scheduler, sync, errgroup)

**Lens:** Go's concurrency model, catalogued for what is **deterministic** vs **irredeemably
non-deterministic**, then mapped onto Phorge's hard reality: a single-threaded `Rc`-shared heap
(`Value` is **not** `Send`/`Sync`), three backends that must produce **byte-identical** stdout for
gated features, and a `php -n` transpile floor where **Fibers / PCRE / hash / BCMath are PRESENT**
but pthreads / pcntl-fork / ext-parallel / Swoole are **ABSENT**. Confidence is graded per claim.

---

## 0. The framing, restated for this lens

Go is the *wrong shape* for Phorge in its native form, but it is the *richest possible source* of
ideas because Go's entire value proposition is "make concurrency legible." Go's primitives split
cleanly along the exact axis Phorge cares about:

- **Deterministic-by-construction** (cooperative yield points, ordered fan-out merges, pure
  pipelines) → candidate **Tier A** (byte-identity-gated, transpiled to PHP, in `differential.rs`).
- **Deterministic-only-under-a-discipline** (a `select` over a single ready channel; a `WaitGroup`
  whose results are merged in spawn order) → **Tier A with a stated invariant**.
- **Irredeemably non-deterministic** (the `GMP` preemptive scheduler's interleaving; `select` over
  multiple simultaneously-ready channels; real timers/`time.After`; `runtime.NumGoroutine`) →
  **Tier B** (quarantined like `Core.Process`, fixture-tested in a dedicated `tests/*.rs`, never in
  the differential) **or rejected**.

The locked decision — *all safe paths + a Tier-B live escape; shared-mutable-state OS threads = HARD
NO* — maps onto Go almost mechanically: take Go's **happens-before structure and its API
ergonomics**, drop Go's **physical preemptive parallelism and its memory-sharing model**. We keep
goroutines-as-syntax, channels-as-syntax, `select`-as-syntax, errgroup-as-syntax — and back them with
a **deterministic single-threaded cooperative scheduler** (PHP 8.1 **Fibers** on the transpile leg,
a hand-rolled green-thread loop on the Rust legs), not OS threads.

---

## 1. Catalogue: Go's concurrency models

### 1.1 Goroutines + the GMP scheduler

**What it is.** `go f(x)` spawns a goroutine: a stackful green thread (2 KB initial stack, grows).
The runtime multiplexes M goroutines onto N OS threads (`GOMAXPROCS`) via the **G-M-P** scheduler
(Goroutine–Machine–Processor). Since Go 1.14 the scheduler is **asynchronously preemptive** — a
goroutine can be suspended at *any* safepoint (function prologue, loop back-edge, async signal),
not just at explicit yield points.

**Determinism.** *None.* This is the canonical irredeemably-non-deterministic case. Two goroutines
writing to stdout produce interleaving that depends on the OS scheduler, core count, GC pauses, and
wall-clock timing. `go test -race` exists precisely because Go *cannot* give you deterministic
interleaving for free.

**Phorge mapping.** The *physical* GMP scheduler is **rejected** (no `Send`/`Sync` heap, no `php -n`
target, non-deterministic output). But the *cooperative core* — a scheduler that runs runnable tasks
to their next **explicit yield point** in a **fixed, deterministic order** — is exactly buildable and
exactly what we keep. The key reframe: **Go's preemption is the part that destroys determinism;
cooperative yield is the part that preserves it.** Phorge adopts the latter only.
*Confidence: high.*

### 1.2 Channels (CSP)

**What it is.** Typed conduits: `ch := make(chan int)` (unbuffered, rendezvous) or `make(chan int,
N)` (buffered FIFO). `ch <- v` send, `v := <-ch` receive, `close(ch)`, `for v := range ch`. Channels
are Go's *primary* synchronization mechanism ("share memory by communicating").

**Determinism.** *Conditional, and this is the crux.*
- A **single producer → single consumer** over **one** channel: the receive order equals the send
  order (FIFO). **Deterministic.**
- **Multiple producers** into one channel: the *merge order* is non-deterministic (depends on which
  goroutine the scheduler ran first). **Non-deterministic — unless the scheduler order is fixed.**
- A **closed channel** drains remaining buffered values FIFO then yields the zero value; `range`
  stops on close. Deterministic given a deterministic fill order.

**Phorge mapping.** Channels are *value-level* objects — a `Value::Channel(Rc<RefCell<VecDeque>>)`
fits the existing `Rc`-shared heap perfectly (single-threaded ⇒ `RefCell` is sound, no `Send`
needed). A send/recv is a **yield point**: if a recv blocks on an empty channel, the current task
parks and the scheduler runs the next runnable task **in deterministic spawn order**. The merge
non-determinism is *eliminated by construction* because there is only one scheduler thread choosing
the next runnable task by a fixed rule (lowest task-id ready). **This is Tier A** with the stated
invariant: *channel-merge order is defined by deterministic scheduler order, not wall-clock.*
PHP transpile target: a `SplQueue` wrapped in a small `Channel` class, with send/recv implemented as
**Fiber suspend/resume** (`Fiber::suspend()` on a blocked recv, the scheduler `->resume()`s the next
runnable fiber). *Confidence: medium-high* (the Rust-leg green-thread loop and the PHP-Fiber loop
must drive the *same* runnable-order rule — that is the byte-identity obligation, and it is the one
real engineering risk).

### 1.3 `select`

**What it is.** `select { case v := <-a: …; case b <- x: …; default: … }` — waits on multiple channel
ops; proceeds on whichever is ready.

**Determinism.** *The single most dangerous primitive for byte-identity.* When **multiple** cases are
simultaneously ready, Go chooses **uniformly at random** (deliberately, to prevent starvation). This
is *intentional non-determinism baked into the language semantics.*

**Phorge mapping.** Two options:
- **Tier A, with a determinism rule:** when multiple cases are ready, pick the **first in source
  order** (not random). This is a *deliberate divergence from Go* that buys determinism. Legible
  ("top case wins ties"), transpilable (the PHP scheduler applies the same first-in-source rule). The
  `default:` (non-blocking) case is fully deterministic. **Recommend Tier A with source-order tie-break.**
- The Go-faithful random `select` is **rejected** (non-deterministic by spec; no determinism story).
*Confidence: high* (the divergence is a clean, documentable design choice; Erlang's `receive` and
many actor systems already use ordered selection).

### 1.4 `sync` primitives — Mutex, RWMutex, WaitGroup, Once, Cond, atomic

**What they are.** `sync.Mutex`/`RWMutex` (locks), `sync.WaitGroup` (barrier: `Add`/`Done`/`Wait`),
`sync.Once` (run-exactly-once), `sync.Cond` (condition variable), `sync/atomic` (lock-free ops).

**Determinism.**
- **Mutex/RWMutex/Cond/atomic** only *matter* under true shared-memory parallelism. On a
  single-threaded cooperative scheduler there is **no data race to protect against** — a critical
  section is never interrupted except at an explicit yield. These primitives become **no-ops or
  trivially-satisfied**. Determinism: total (because the thing they guard against cannot occur).
- **WaitGroup** is a *join barrier*: spawn N tasks, `Wait()` until all done. The barrier itself is
  deterministic; the *order results become available* is the same merge-order question as channels.

**Phorge mapping.** `Mutex`/`atomic`/`Cond` are **not needed** and should be **rejected as
user-facing API** — exposing them would be a lie (they protect against a hazard the model forbids).
This is *itself a Phorge selling point over PHP/Go*: "you cannot have a data race, so there is no lock
to forget." **WaitGroup-as-join survives** but is better expressed as the structured `errgroup` /
`parallelMap` form below (§1.7, §2.2) rather than the raw `Add`/`Done` counter. *Confidence: high.*

### 1.5 Context (`context.Context`)

**What it is.** Cancellation + deadline + value propagation across a goroutine tree. `ctx.Done()`
returns a channel closed on cancel/timeout; `context.WithTimeout`/`WithCancel`/`WithValue`.

**Determinism.**
- **Cancellation** (explicit `cancel()`): deterministic — it's a normal control signal.
- **Deadline/timeout** (`WithTimeout`): **non-deterministic** — driven by the wall clock. Same class
  as `time.After`.

**Phorge mapping.** A **cancellation token** (`Context` with explicit `cancel()` and a `done()`
check) is **Tier A** — purely structural, transpiles to a small PHP class holding a bool + a closed
channel/flag. **Timeout-based cancellation is Tier B** (wall-clock) — quarantine it the way
`Core.Process` is quarantined. Recommend: ship the explicit-cancel `Context` in Tier A now; defer the
deadline variant to the Tier-B live-concurrency escape. *Confidence: high.*

### 1.6 `time` (Ticker, Timer, After, Sleep)

**Determinism.** *Zero* — all wall-clock-driven. `time.After`, `time.Tick`, `time.Sleep` are the
purest non-determinism in Go's concurrency surface.

**Phorge mapping.** **Tier B, always** — the same class as a clock/random native. A *virtual* /
*simulated* clock (the scheduler advances a logical time counter; `sleep(d)` parks the task and the
scheduler resumes it when logical-time reaches the deadline, choosing ties by task-id) **is**
deterministic and is a known technique (Tokio's `time::pause`, discrete-event simulators). That
*virtual-time* variant could be **Tier A** as a simulation primitive — but real wall-clock timers are
Tier B. Recommend: defer all `time`-driven concurrency to Tier B initially; the virtual-clock
scheduler is a *future* deterministic enhancement worth its own design pass. *Confidence: medium.*

### 1.7 `errgroup` (golang.org/x/sync/errgroup)

**What it is.** Structured-concurrency sugar over WaitGroup: spawn N fallible tasks, `Wait()` returns
the **first** error (and cancels the rest via a derived context). The idiomatic "do these N things
concurrently, fail fast" pattern.

**Determinism.** The *set* of results is deterministic; *which* error is "first" depends on timing
under real parallelism — **non-deterministic in Go**, but **deterministic under a fixed scheduler
order** (first-in-spawn-order error wins).

**Phorge mapping.** **This is the single best primitive to lift.** It is *structured* (no dangling
goroutines), it has a clean PHP target, and under the deterministic scheduler "first error =
lowest-task-id error" is a stable, legible rule. It is essentially `parallelMap` + early-exit. **Tier
A.** *Confidence: high.*

---

## 2. The two concrete Tier-A shapes Phorge should adopt from Go

### 2.1 `async`/`await` cooperative coroutines (the goroutine core, made deterministic)

The locked decision (a) is "cooperative async/await over a DETERMINISTIC single-threaded scheduler →
PHP 8.1 Fibers." Go's contribution here is **the ergonomics and the channel vocabulary**, not the
runtime.

**Phorge-syntax API sketch** (illustrative; final syntax is a design decision):

```phorge
package Main;
import Core.Console;
import Core.Async;          // the scheduler + spawn/channel surface

function worker(Channel<int> out, int id) -> void {
    out.send(id * 10);     // a yield point if the channel is full / on rendezvous
}

function main() -> void {
    var ch = Async.channel<int>(0);        // unbuffered (rendezvous) channel
    Async.spawn(fn() => worker(ch, 1));    // deterministic task-id 1
    Async.spawn(fn() => worker(ch, 2));    // task-id 2
    Async.run(fn() -> void {               // drive the scheduler to quiescence
        Console.println("{ch.recv()}");    // 10  (task 1 ran first — fixed order)
        Console.println("{ch.recv()}");    // 20
    });
}
```

**The exact byte-identity argument.** All three backends run the *same scheduler*:
- A single runnable queue ordered by **monotonic task-id (spawn order)**.
- A blocked task parks; the scheduler picks the **lowest-id runnable** task next.
- Every send/recv/`await` is the *only* place control transfers — there is no preemption.

Because the *order of effects* is a pure function of the program text (spawn order + yield points),
stdout is identical on all three legs **provided the three scheduler implementations apply the same
runnable-selection rule.** That last clause is the entire risk surface and must be the central test
target (a fixture program with interleaved prints whose golden output is asserted on all three legs).

**The exact PHP transpile target.** PHP 8.1 **Fibers** (present under `php -n` — verified in the
project brief). A `Channel` is a PHP class over `SplQueue`; the scheduler is a hand-written PHP loop
holding `Fiber` objects in a task-id-ordered array; `send`/`recv`/`await` call `Fiber::suspend()`,
and the loop `->resume()`s the next runnable fiber by the same lowest-id rule. **No Composer package**
(`amphp`/`ReactPHP` are absent under `php -n`) — this is a hand-rolled ~150-line PHP runtime helper,
emitted as a gated `__phorge_async_*` prelude exactly like the existing `__phorge_div`/`uses_*`
helpers. *Confidence: medium-high (Fiber suspend/resume in a hand loop is well-trodden; the parity
obligation between the Rust green-thread loop and the PHP Fiber loop is the work).*

**Std-only Rust feasibility.** **High.** No async runtime crate needed: a coroutine on the Rust legs
is a **state machine the compiler lowers explicitly**, OR — simpler and more faithful — a
**re-entrant interpreter/VM frame stack** exactly like the existing `vm.run_until` /
`call_closure_value` re-entrancy already shipped for higher-order natives (see
[[higher-order-natives-reentrant-vm]]). A blocked task = a suspended frame stack stored in the
scheduler; resume = push it back and continue `exec_op`. **The VM already has the re-entrancy
machinery; the scheduler is a queue of saved frame-stacks.** This is the strongest reuse argument in
the whole document. *Confidence: medium-high.*

**New VM Op?** *Probably none.* `spawn`/`channel`/`send`/`recv` can all be `Op::CallNative` entries in
a new `Core.Async` leaf, with the scheduler living in Rust as the native's `eval` (a new
`NativeEval::Scheduler`-style variant, analogous to `HigherOrder`, that is handed the backend's frame
machinery). `await` *might* want a dedicated yield op for clean stack-unwinding, but the re-entrant
`run_until` pattern suggests it can be done native-side without a new `Op`. **Tentative: zero new
Ops; one new `NativeEval` variant.** This must be validated by a spike — flag as the top open
question. *Confidence: medium.*

### 2.2 `parallelMap` / fork-join (pure data-parallelism — locked decision (b))

This is the *cleanest* Tier-A win and barely needs Go at all (it's `errgroup` + `map`), but Go's
`errgroup` is the right *shape*.

**Phorge-syntax API sketch:**

```phorge
import Core.Async;
// Apply a SIDE-EFFECT-FREE fn to each element; merge results in INPUT ORDER.
var results = Async.parallelMap(xs, fn(int x) => expensive_pure(x));
```

**Determinism: total, by construction.** The contract is: the mapped fn is **pure** (no
side-effects), and the merge is **order-preserving** (result[i] = f(xs[i])). Today all three legs run
it **sequentially** → trivially byte-identical to a plain `List.map`. The *future* optimization —
running the Rust legs' iterations on real OS threads — is permitted **only because the merge is
ordered and the fn is pure**, so the *output* is unchanged even though the *execution* parallelizes.
The PHP leg stays sequential (no threads under `php -n`) and still matches.

**Byte-identity argument:** result vector is `[f(xs[0]), f(xs[1]), …]` regardless of execution order;
purity guarantees no observable interleaving; ordered merge guarantees positional stability. **This
is byte-identical even if one leg parallelizes physically and another doesn't.** *Confidence: high —
this is the safest concurrency primitive in the entire design.*

**PHP transpile target:** plain `array_map` (sequential). **Std-only Rust feasibility:** the
sequential form is trivial (`List.map`); the optional physical-parallel form needs `std::thread` +
`Send` — and here the `Rc` heap bites: the *inputs and outputs* must be `Send`-cloneable into worker
threads (deep-clone the `Value` subset that's `Send`-able, or restrict `parallelMap` to a `Send`-safe
value subset). **Recommend: ship the sequential form (Tier A, byte-identical) now; gate physical
parallelism behind a later, separate design** — it's an optimization, not a semantic feature.
**New Op: none** (it's a `HigherOrder` native, like `List.map`). *Confidence: high for sequential,
medium for the physical-parallel optimization.*

### 2.3 Reactive/FRP streams (locked decision (c)) — Go's relationship

Go does **not** ship FRP; the idiomatic analog is **channel pipelines** (`gen() -> sq() -> merge()`
stages connected by channels, the "Go Concurrency Patterns: Pipelines" model). This *is* a
deterministic-under-fixed-scheduler construct (each stage is single-producer/single-consumer) and
maps onto the §2.1 channel machinery directly. A richer FRP layer (`Observable`/`map`/`filter`/
`merge` over deterministic sources) is a *library on top of channels* and inherits their Tier-A
determinism as long as the sources are deterministic (a list, a range — **not** a timer or socket).
**Recommend: channel pipelines as the deterministic stream primitive; full FRP as a later library
slice.** *Confidence: medium.*

---

## 3. What is explicitly REJECTED from Go (and why)

| Go construct | Verdict | Reason |
|---|---|---|
| GMP preemptive scheduler / real OS-thread multiplexing | **Reject** | Non-deterministic interleaving; `Value` not `Send`/`Sync`; no `php -n` target |
| `sync.Mutex` / `RWMutex` / `Cond` / `sync/atomic` | **Reject (as API)** | Guard against a hazard the cooperative single-thread model forbids; exposing them is a lie. *Phorge selling point: no data races possible.* |
| Random-tie `select` | **Reject** | Intentional non-determinism by spec; replaced by source-order tie-break |
| `time.After` / `Ticker` / wall-clock timeouts | **Tier B** | Wall-clock ⇒ non-deterministic (virtual-clock variant is a future Tier-A enhancement) |
| `runtime.NumGoroutine` / `GOMAXPROCS` / scheduler introspection | **Reject** | Exposes physical scheduling state that doesn't exist in the model |
| Real-socket/network goroutines | **Tier B** | The locked "genuinely-live concurrency" escape — non-gated, fixture-tested |
| `go test -race` analog | **N/A** | No races are possible by construction; no race detector needed |

---

## 4. Tier recommendations (per-feature, as instructed)

| Feature (from Go) | Phorge form | Tier | Determinism basis | New Op? |
|---|---|---|---|---|
| Goroutine core → coroutines | `Async.spawn` + cooperative scheduler | **A** | Fixed lowest-id runnable order; explicit yields only | None (1 new `NativeEval`) — *spike-gated* |
| Channels (SPSC) | `Channel<T>` send/recv/close/range | **A** | FIFO + deterministic scheduler order | None likely |
| `select` | source-order tie-break + `default` | **A** | Top-case-wins tie rule (divergence from Go's random) | None |
| `errgroup` | `Async.group` / fail-fast fork-join | **A** | First-spawn-order error wins | None |
| `parallelMap` (pure) | `Async.parallelMap` (sequential now) | **A** | Pure fn + ordered merge ⇒ output-invariant | None (`HigherOrder` native) |
| Channel pipelines / FRP | streams over deterministic sources | **A** | Inherits channel determinism | None |
| `context` explicit cancel | `Context.cancel()` / `done()` | **A** | Structural control signal | None |
| `context` deadline / `time.*` | timeouts, tickers | **B** | Wall-clock | n/a (quarantined) |
| Live sockets / physical parallelism | the Tier-B live escape | **B** | Non-deterministic; fixture-tested outside `differential.rs` | future |
| `sync.Mutex`/atomic | — | **Reject** | No hazard exists | — |

---

## 5. Reuse map onto existing Phorge mechanisms

- **Native registry** (`src/native/mod.rs`, `(module,name)`): a new `Core.Async` leaf
  (`src/native/async.rs`) holds `spawn`/`channel`/`send`/`recv`/`select`/`group`/`parallelMap` as
  `NativeFn` entries — each single-sources checker sig + `eval` + `php`. No god-file (per existing
  one-leaf-per-module rule).
- **`NativeEval`**: today `Pure | HigherOrder | Reflective`. The scheduler needs a *re-entrant,
  frame-driving* variant — call it `NativeEval::Coroutine` (handed the backend's frame machinery,
  exactly as `HigherOrder` is handed a `ClosureInvoker`). The VM's existing `run_until` /
  `call_closure_value` re-entrancy ([[higher-order-natives-reentrant-vm]]) is the *direct precedent*
  — the scheduler is a queue of saved frame-stacks driven by the same loop.
- **Quarantine seam**: the **deterministic** Async natives are `pure: true` ⇒ stay **in** the
  differential (byte-identity-gated, transpiled, with `examples/guide/async.phg`). The **live**
  Tier-B concurrency natives (real sockets/timers) are `pure: false` ⇒ automatically dropped from
  `differential.rs` by `uses_impure_native` (which reads the `pure` flag dynamically — **no harness
  edit needed**, exactly the seam `Core.Process` uses) and fixture-tested in `tests/async_live.rs`.
- **Value model**: `Value::Channel(Rc<RefCell<VecDeque<Value>>>)` and a `Value::Task`/scheduler
  handle fit the `Rc`-shared single-threaded heap with **zero `Send` requirement** — the single
  thread makes `RefCell` sound. No tracing GC concern (acyclic + `Rc`/`Drop`, per the M2 invariant)
  *unless* a channel can hold a closure that captures the channel — a potential cycle worth a note,
  but deferred (the immutable-heap assumption is being relaxed in M-mut anyway).
- **Transpile helpers**: a gated `__phorge_async_*` PHP prelude (Fiber scheduler + `Channel` class),
  emitted under a `uses_async` flag like the existing `__phorge_div`/`uses_*` helpers.

---

## 6. Top open questions for the design phase

1. **Spike: zero new Ops?** Validate that `spawn`/`await`/`send`/`recv` ride `Op::CallNative` + a new
   `NativeEval::Coroutine` with no dedicated VM yield op, by extending the proven `run_until`
   re-entrancy to *save-and-resume* a frame stack (not just nest one). This is the make-or-break
   feasibility question. *Confidence the answer is "yes": medium.*
2. **Scheduler-parity test harness.** The byte-identity obligation reduces entirely to "the Rust
   green-thread loop and the PHP Fiber loop apply the identical runnable-selection rule." A dedicated
   interleaving fixture (deterministic golden, asserted on all three legs) must exist *before* the
   feature is declared gated.
3. **`select` tie-break divergence from Go** — confirm the source-order rule is acceptable (it is a
   *deliberate* legibility-over-Go-fidelity choice, consistent with the Phorge philosophy of removing
   surprises).
4. **Virtual clock** as a *future* Tier-A deterministic enhancement (deferred; Tokio `time::pause`
   precedent) vs wall-clock timers staying Tier B (now).
5. **Physical `parallelMap`** — the `Send`-able-value-subset deep-clone strategy, deferred behind the
   sequential (byte-identical) form.

---

## 7. Bottom line

Go gives Phorge the **vocabulary and ergonomics** of legible concurrency — goroutines, channels,
`select`, `errgroup`, structured fork-join — while Phorge **swaps the runtime** from Go's
preemptive-parallel GMP scheduler to a **deterministic single-threaded cooperative scheduler**
(Fibers on PHP, a re-entrant frame loop on the Rust legs). The split is clean and principled:
**keep cooperative yield + ordered fan-out + pure data-parallelism (Tier A, byte-identical), reject
preemption + shared-memory locks + random `select`, quarantine wall-clock timers + live sockets
(Tier B).** The single highest-value, lowest-risk lift is **`parallelMap` (sequential)** — Tier A,
byte-identical by construction, a `HigherOrder` native, no new Op. The highest-value, highest-risk
lift is the **cooperative async/channel core** — Tier A *if* the three-leg scheduler-parity holds,
which a spike must prove. *Overall confidence: medium-high on the framing and the Tier split; medium
on the zero-new-Op coroutine implementation pending a spike.*
