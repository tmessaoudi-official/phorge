# Design — Cooperative async/await + deterministic single-threaded scheduler

**Stage 2 — DESIGN.** The gated-concurrency foundation for Phorj: `Core.Async`, a deterministic
single-threaded cooperative scheduler that stays byte-identical across `run` (tree-walker), `runvm`
(bytecode VM), and the transpiled PHP run under real `php -n` 8.5.

**Verdict: Tier A** (byte-identity-gated) for the cooperative core (spawn / await / yield / channels /
ordered `group` / `parallelMap` / pull-streams / `select`-first-in-source-order / `Context`). The
moment resolution order is driven by a real clock, socket, or OS scheduler → **Tier B** (quarantined,
fixture-tested) for those specific natives (`sleep`, `after`, live sockets) — exactly where
`Core.Process` already sits. Shared-mutable-state OS threads → **rejected** (locked).

Confidence: **medium** for the full surface as designed in one slice; **high** for the keystone (a
scheduler-driven cooperative core with no new `Op` and a logical-clock ready-queue is byte-identical
by construction and reuses machinery that already ships).

---

## 1. The byte-identity argument (why this is Tier A at all)

Phorj's three legs are byte-identical only when *evaluation order is a total order fixed by the
language, not by a runtime.* The entire concurrency design is therefore built on one principle:

> **Resolution order is a language rule, not a scheduler artifact.**

The two non-determinism sources a concurrency runtime normally introduces are both *removed by
construction*:

1. **Task interleaving** — eliminated by **cooperative run-to-completion with explicit suspension
   points only**. There is no preemption. A task runs until it `await`s (or completes); control
   returns to the scheduler *only* at an `await`. `exec_op` has no preemption hook and never gains
   one (this is the same reason BEAM reduction-counting is rejected). Verified: the VM main loop in
   `src/vm/closure.rs::run_until` advances one op at a time with no yield point — suspension can only
   be a native call that *chooses* to suspend, never an interrupt.

2. **Wakeup order** — eliminated by a **deterministic ready-queue**: a FIFO of runnable tasks plus a
   logical-clock timer min-heap keyed on `(logical_deadline, insertion_seq)`. `insertion_seq` is a
   monotonic counter so ties break by *spawn/schedule order*, never by wall time. The JS event-loop
   rule is adopted verbatim: **drain all microtasks (resolved continuations) before advancing to the
   next macrotask/timer.**

Given those two, the scheduler is a **pure function of the program text**: same spawns, same awaits,
same resolution order, on all three legs. The PHP leg implements the *same* FIFO + logical-heap rule
in a small emitted runtime (PHP `SplQueue` + a sorted array as the timer heap — both verified present
under `php -n` 8.5), driving PHP 8.1 `Fiber`s (verified present under `php -n` 8.5). The Rust legs
drive Phorj coroutines via the **already-shipped** re-entrant `call_closure_value` / `run_until`
(`src/vm/closure.rs`) — no second interpreter, the parity analogue of the tree-walker's
`call_closure`.

**Logical time is the keystone.** All Tier-A timing is *logical*: `Async.delay(ticks)` schedules a
resume at `now_logical + ticks` where `now_logical` is a virtual counter the scheduler advances by
jumping to the next timer's deadline when the ready-queue empties (the Rx `TestScheduler` /
deterministic-simulation model). No wall clock is read. This is what lets `delay`, ordered timeouts,
and stream throttling be **Tier A** while `sleep`-on-the-real-clock is **Tier B**: the *unit* is a
logical tick reproducible on every leg, not a millisecond.

---

## 2. Surface syntax (Phorj)

Two layers. **Layer 1 is a native library** (`Core.Async`, ships first, zero new syntax). **Layer 2
is `async`/`await` sugar** (front-end-only desugaring to Layer 1, ships second). Layer 1 alone is a
complete, usable, gated foundation; Layer 2 is ergonomic polish.

### 2.1 Layer 1 — `Core.Async` library (no new syntax, ships first)

```phorj
package Main;
import Core.Async;
import Core.Console;

// A task is a zero-arg closure. spawn schedules it; it runs cooperatively.
function main() -> int {
    Task<int> a = Async.spawn(fn() -> int { return work(1); });
    Task<int> b = Async.spawn(fn() -> int { return work(2); });

    // ordered merge — results in SPAWN order regardless of completion order (Promise.all shape)
    List<int> results = Async.all([a, b]);   // [work(1), work(2)]

    Console.println("done");
    return 0;
}

// A coroutine that suspends: Async.yield() returns control to the scheduler (a logical microtask).
function work(int n) -> int {
    Async.yield();          // suspension point — re-queued at the tail of the ready-FIFO
    return n * 10;
}
```

Channels (CSP, single-threaded ⇒ `Rc<RefCell>` sound):

```phorj
Channel<int> ch = Async.channel();     // unbounded; bounded variant Async.channel(cap)
Async.spawn(fn() -> int { Async.send(ch, 7); return 0; });   // send is a yield point if bounded+full
int v = Async.recv(ch);                // recv is a yield point if empty; resumes when a value arrives
```

Structured fork-join (the single best primitive lifted from Go — no dangling tasks, clean PHP target):

```phorj
// group runs all tasks, returns results in submission order, fail-fast on first fault
// (first-in-submission-order fault wins under the fixed scheduler).
Result<List<int>, Fault> r = Async.group([
    fn() -> int { return fetch(1); },
    fn() -> int { return fetch(2); },
]);
```

Pure data-parallelism (essentially free today — semantically identical to `List.map`):

```phorj
// INPUT-ORDER-PRESERVING merge. Ships sequentially now (byte-identical to List.map);
// physical Rust-thread parallelism is a LATER invisible optimization that preserves output.
List<int> doubled = Async.parallelMap([1, 2, 3], fn(int x) -> int { return x * 2; });
```

`select` — **deliberate divergence from Go: first-ready in SOURCE ORDER, not random** (so it is gated):

```phorj
// Each arm is (channel, handler). When several are ready, the FIRST in source order fires.
// A `default` arm makes the select non-blocking and fully deterministic.
int picked = Async.select([
    Async.case(chA, fn(int v) -> int { return v; }),
    Async.case(chB, fn(int v) -> int { return v + 100; }),
], fn() -> int { return -1; });   // default (optional)
```

Cancellation (structural, not clock-based):

```phorj
Context ctx = Async.context();
Async.spawn(fn() -> int { if (ctx.done()) { return 0; } return loop_work(ctx); });
ctx.cancel();    // sets a flag + closes the done-channel; resumption checks are explicit
```

### 2.2 Layer 2 — `async` / `await` sugar (front-end-only, ships second)

```phorj
async function fetchBoth() -> List<int> {
    Future<int> a = fetch(1);          // calling an async fn returns a Future (does not block)
    Future<int> b = fetch(2);
    int x = await a;                   // await is a suspension point
    int y = await b;
    return [x, y];
}

async function fetch(int id) -> int {
    await Async.delay(1);              // logical tick, Tier A
    return id * 10;
}
```

`async fn` and `await` are **pure desugaring** in the parser/checker — they lower to Layer-1
`Async.spawn` + a scheduler-driven `Future` value and a `CallNative` await. They add **no runtime
mechanism** the library doesn't already have. (Same discipline as `|>` lowering to `Call`, and `??`
lowering — front-end-only, runtime parity untouched.)

---

## 3. Runtime model — what executes, on each leg

### 3.1 The scheduler (shared logical rule, three implementations)

State (identical conceptual shape on every leg):

- `ready: FIFO<Task>` — runnable continuations, drained front-to-back.
- `timers: MinHeap<(logical_deadline, insertion_seq, Task)>` — logical-clock waits.
- `now_logical: u64` — virtual clock, advanced only when `ready` empties (jump to next timer).
- `next_seq: u64` — monotonic tie-breaker; every schedule/spawn/timer takes the next value.

The loop (the JS event-loop rule):

```
while ready non-empty OR timers non-empty:
    while ready non-empty:                 # drain microtasks
        t = ready.pop_front()
        run t until it suspends or completes   # cooperative, no preemption
    if timers non-empty:
        now_logical = timers.peek().deadline    # advance virtual clock
        move all timers with deadline == now_logical into ready (ascending insertion_seq)
```

This loop is a **total order over the program text**: tie-breaking is `insertion_seq`, which is a
function of spawn order, which is a function of the source. No wall clock, no OS, no randomness.

### 3.2 Rust legs (`run` + `runvm`)

A Phorj coroutine is a closure value (`Value::Closure`). "Running until it suspends" maps onto the
**already-shipped re-entrant drive**:

- **Interpreter (`run`)**: a coroutine runs via `call_closure`. Suspension = the coroutine calls the
  `Async.yield` / `Async.await` native, which returns a sentinel that unwinds back to the scheduler
  native. Because the interpreter is a tree-walker, a *stackful* suspend mid-expression is hard — so
  the v1 model is **`await` only at statement granularity at a suspension point the scheduler owns**
  (see §6 open question O1: stackful vs. CPS). The pragmatic v1 is **the scheduler is itself a native
  that owns the loop**, and coroutines suspend by *returning a continuation token* rather than
  unwinding the Rust stack — see §3.4.
- **VM (`runvm`)**: identical, driving via `call_closure_value` / `run_until` (`src/vm/closure.rs`).
  `run_until(target_depth)` already drives `exec_op` re-entrantly until a frame returns — the exact
  shape a scheduler needs.

Both legs share the *same scheduler native* (a `NativeEval::HigherOrder`-shaped entry that takes the
backend's `ClosureInvoker`), so the loop logic lives once and both legs call closures byte-identically
— the same parity discipline already proven for `Core.List.map`/`reduce` (M-RT S7b-3, re-entrant VM).

### 3.3 PHP leg (transpile target)

Emitted, gated behind `uses_async` (like the existing `uses_* + __phorj_*` helper pattern). A small
**emitted runtime** (`__phorj_scheduler`) implements the §3.1 loop using:

- `ready` → `SplQueue` (verified under `php -n`).
- `timers` → a plain PHP array sorted by `(deadline, seq)` on insert, or a small binary-heap class
  (no ext dependency — verified `SplPriorityQueue` is *core* but its ordering for equal priorities is
  **not** insertion-stable, so we emit our own stable heap to preserve `insertion_seq` tie-breaking —
  a named determinism risk, R-PHP-1 below).
- A coroutine → a PHP 8.1 `Fiber` (verified present under `php -n` 8.5). `Async.yield`/`await` →
  `Fiber::suspend($token)`; the scheduler resumes via `$fiber->resume($value)`. `Fiber::getReturn()`
  yields the task result.

Byte-identity holds because **the PHP scheduler drives the SAME FIFO + stable-logical-heap order** as
the Rust scheduler — the Fiber is just the suspension *mechanism*, the *order* is the language rule.

### 3.4 Suspension representation (the one real design choice)

Two candidate mechanisms for "a coroutine pauses and the scheduler resumes it":

- **(A) Stackful** (Rust legs use a re-entrant native frame; PHP uses a real `Fiber`). The
  coroutine's Rust call stack is *not* unwound; the scheduler is one frame up and resumes by
  returning into it. **Problem on the Rust legs:** Phorj has no stackful coroutine primitive in std
  (no `generator`/`async` Rust feature on stable for *Phorj* values), and `run_until` returns a
  *value*, it does not leave a resumable suspended Rust frame. A genuine mid-expression suspend would
  need a Phorj-level CPS transform or a saved VM frame snapshot.

- **(B) Trampolined / CPS-at-await-boundaries** (recommended for v1). A coroutine is driven to its
  *next await boundary*, at which point it returns a continuation descriptor (which channel/timer it
  is waiting on + the closure to resume with). The scheduler stores it; when the wait resolves it
  re-invokes the continuation closure. This is what `run_until` *already supports* (run a frame to
  completion and get a value) — the value just happens to be a "suspended" marker carrying the
  resume closure. **`async`/`await` Layer 2 then becomes a front-end CPS lowering** (split an
  `async fn` body at each `await` into continuation closures), exactly like Rust's own async
  state-machine lowering but emitted as Phorj closures — no new VM Op needed.

**Recommendation: (B) for v1** — it reuses shipped machinery on the Rust legs and maps cleanly to
Fibers on PHP (a Fiber *is* the trampolined-suspend made native; the PHP leg can be stackful while
the Rust legs are CPS, because both produce the same *resolution order* — the byte-identity invariant
is about order, not mechanism). **(A) (saved VM frame snapshots / a true Phorj fiber)** is a later
optimization (M6+) that preserves output. This split (Rust = CPS, PHP = Fiber, same order) is the
single most important decision in the design and the biggest open question (O1).

---

## 4. New `Op` / `Value` — what's needed

### Value

- **`Value::Channel(Rc<RefCell<ChannelState>>)`** — REQUIRED. Single-threaded ⇒ `Rc<RefCell>` is
  sound (no `Send` needed, consistent with the whole heap). `ChannelState { buf: VecDeque<Value>,
  cap: Option<usize>, waiters_send, waiters_recv, closed: bool }`. Erases to a PHP `Channel` class
  emitted in the runtime (`SplQueue`-backed). This is genuinely new heap state (a channel is mutable
  shared, like an `Instance` post-M-mut) — but it rides the existing `Rc`-shared value discipline; no
  GC concern (single-threaded, the M-mut COW/shared-mutable split already covers it).

- **`Future<T>` / `Task<T>`** — can be a **library type, not a `Value` variant**: a `Future` is an
  `Instance` of an emitted `Core.Async` class holding `(state, value, continuations)`. Reusing
  `Value::Instance` (M-RT) means **no new Value variant for futures** — only `Channel` is genuinely
  new. (Open question O2: is `Channel` worth a dedicated variant, or can it also be an `Instance`
  with native methods? An `Instance` with `pure` native methods avoids a new `Value` variant
  entirely — strongly preferred if the borrow/RefCell ergonomics work out.)

### Op

- **Target: NO new `Op`.** Every operation is a `CallNative` (`Async.spawn`/`yield`/`send`/`recv`/
  `all`/`group`/`parallelMap`/`select`/`channel`/`context`). The scheduler is a `HigherOrder` native
  driving closures via the existing re-entrant `ClosureInvoker`. `async`/`await` sugar lowers to those
  natives in the front end (no Op). This matches the project's strong "no new Op unless forced" track
  record (S2 null-safety, S4 unions, generics-all, higher-order natives all added zero Ops).

- **Contingency (only if CPS lowering proves insufficient):** a single `Op::Suspend` /
  `Op::Resume(usize)` pair to snapshot/restore a VM frame would be the stackful escape (mechanism A).
  This is the *only* scenario that needs the 3-coupled-match Op dance (`chunk.rs` validate / `vm`
  exec_op / `compiler` stack_effect). Held in reserve; v1 aims to avoid it.

---

## 5. Effort, slicing, feasibility

**Effort: large (a milestone, M-Async or an M6 sub-slice).** Slice it:

- **S1 — scheduler core + `spawn`/`yield`/`all`** (the §3.1 loop + CPS suspension on Rust legs +
  emitted PHP Fiber runtime). The keystone; everything else is downstream. *medium-large.*
- **S2 — channels** (`Value::Channel` or Instance-backed, `send`/`recv` as yield points). *medium.*
- **S3 — `group` (structured fork-join) + `parallelMap`** (parallelMap is nearly free; group rides
  the scheduler). *small-medium.*
- **S4 — `select` (source-order) + `Context` (cancel)**. *medium.*
- **S5 — `async`/`await` sugar** (front-end CPS lowering to S1 primitives). *medium.*
- **S6 — pull-streams** (`Core.Stream` map/filter/scan over a lazy state machine; unify with
  generators per the prior-art digest — same primitive). *medium, can ride after S1.*
- **Tier-B addendum** — `Async.sleep`/`after`/live-socket integration (`pure:false`, quarantined,
  fixture-tested in a dedicated `tests/async_live.rs`, never in `differential.rs`). *small per item.*

**Feasibility: ~70%** that the full Tier-A surface (S1–S6) ships byte-identical in one milestone with
no new Op. The keystone (S1) is **~85%** — a logical-clock cooperative scheduler over the shipped
re-entrant drive + PHP Fibers is well-trodden and every dependency is verified present. The risk is
concentrated in §3.4 (CPS lowering of `async`/`await` on the *tree-walking interpreter* matching the
*VM* matching *Fibers* exactly) and in channel borrow ergonomics under `Rc<RefCell>`. If CPS proves
too invasive on the interpreter, the fallback is S1–S4 as an explicit-continuation library (no
`async`/`await` sugar) — still a complete, gated foundation, just less ergonomic — which raises S1–S4
feasibility to ~88%.

---

## 6. Named determinism risks

- **R1 — interpreter ≠ VM suspension granularity.** If the tree-walker can only suspend at coarser
  boundaries than the VM (mid-expression `await`), the *resolution order* could diverge. **Mitigation:**
  v1 restricts `await` to statement-level suspension points the scheduler owns (CPS boundaries), so
  both legs split at identical points. Gated example must exercise interleaved awaits across ≥3 tasks.
- **R2 — `select` ties.** Resolved by the locked rule: first-ready in *source order* (not Go's
  random). Deterministic by construction; the example must hit a multi-ready `select`.
- **R-PHP-1 — PHP timer-heap stability.** `SplPriorityQueue` is not insertion-stable for equal
  priorities → would break `insertion_seq` tie-breaking. **Mitigation:** emit a custom stable heap
  (sort key `(deadline, seq)`) in the runtime; never use `SplPriorityQueue`. Verified `SplQueue` is
  stable-FIFO and is enough for the `ready` queue; only the timer heap needs the custom class.
- **R3 — Fiber vs CPS observable difference.** A PHP Fiber is stackful; the Rust legs are CPS. If a
  coroutine has an *observable side effect* (a `Console.println`) at a point that the two mechanisms
  reach in a different order, output diverges. **Mitigation:** side effects are themselves ordered by
  the scheduler's run-to-completion rule (a task runs to its next await with no interleaving), so
  print order = task-run order on every leg. The example suite must include a print-ordering test
  across cooperating tasks (this is the highest-value differential test).
- **R4 — channel fairness.** `recv` waiters woken in *registration order* (FIFO), `send` likewise —
  a language rule, not OS fairness. Must be identical in the PHP `Channel` class.
- **R5 — clock leakage.** Any accidental wall-clock read (e.g. a future `Async.now()`) silently
  poisons determinism. **Mitigation:** wall-clock-anything is `pure:false` (Tier B) by policy, caught
  by `uses_impure_native` auto-quarantine; logical time only on the Tier-A path.
- **R6 — `parallelMap` future physical parallelism.** When Rust-side threads are added later, `Value`
  is `!Send` → cannot share across threads. The ordered-merge contract keeps *output* identical, but
  the implementation must fan out *owned/cloned* task inputs (or process fan-out), never shared `Rc`.
  This is a future-optimization constraint, not a v1 risk (v1 is sequential).

---

## 7. Exact PHP transpile target (sketch)

`spawn`/`all` and the scheduler (emitted once, gated by `uses_async`):

```php
// __phorj runtime (emitted), php -n 8.5 — Fiber + SplQueue + custom stable timer heap
final class __PhorjScheduler {
    private SplQueue $ready;            // microtask FIFO
    private array $timers = [];         // [ [deadline, seq, Fiber, resumeVal], ... ] kept sorted
    private int $nowLogical = 0;
    private int $nextSeq = 0;
    public function __construct() { $this->ready = new SplQueue(); }

    public function spawn(Closure $task): __PhorjFuture {
        $fut = new __PhorjFuture();
        $fib = new Fiber(function () use ($task, $fut) { $fut->resolve($task()); });
        $this->ready->enqueue([$fib, null]);
        return $fut;
    }
    public function delay(int $ticks): void {                 // Tier A: logical
        $fib = Fiber::getCurrent();
        $this->timers[] = [$this->nowLogical + $ticks, $this->nextSeq++, $fib, null];
        usort($this->timers, fn($a,$b)=>[$a[0],$a[1]]<=>[$b[0],$b[1]]);   // stable by (deadline,seq)
        Fiber::suspend();
    }
    public function run(): void {
        while (!$this->ready->isEmpty() || $this->timers) {
            while (!$this->ready->isEmpty()) {
                [$fib, $val] = $this->ready->dequeue();
                $fib->isStarted() ? $fib->resume($val) : $fib->start();
            }
            if ($this->timers) {
                $this->nowLogical = $this->timers[0][0];
                while ($this->timers && $this->timers[0][0] === $this->nowLogical) {
                    [, , $fib] = array_shift($this->timers);
                    $this->ready->enqueue([$fib, null]);
                }
            }
        }
    }
}
```

`Async.all([a,b])` → collect each future's resolved value **in argument order** (`array_map` over the
task list after `run()`), regardless of completion order. `Async.parallelMap` → sequential
`array_map` (parallelism is a Rust-side optimization the PHP leg never needs). `Channel` → an emitted
`__PhorjChannel` class wrapping `SplQueue` + FIFO waiter lists, `send`/`recv` calling
`Fiber::suspend`/scheduler-resume. `Context.cancel()` → set a `bool` flag + mark a done-channel
closed. All erase to **core-only** PHP (no mbstring/BCMath needed) — verified `Fiber`/`SplQueue`
present under `php -n`.

---

## 8. Open questions for the developer

- **O1 (the big one): suspension mechanism.** Accept the v1 split — **Rust legs CPS-trampolined,
  PHP leg stackful Fibers, same resolution order** — and restrict v1 `await` to statement-level
  boundaries? Or invest up front in saved-VM-frame snapshots (`Op::Suspend`/`Resume`, the one place
  a new Op would appear) for true mid-expression suspension across all three legs?
- **O2: `Channel` as a `Value` variant vs. an `Instance`.** A dedicated `Value::Channel` is cleaner
  but adds heap state to thread through both backends; an `Instance` with native methods adds **zero**
  new Value but pushes channel state into a class. Which?
- **O3: `select` tie rule.** Confirm **first-ready in source order** (the locked deliberate divergence
  from Go's random) — and confirm `default:` makes it non-blocking + fully deterministic.
- **O4: ship `async`/`await` sugar in the same milestone as the library, or library-first then sugar?**
  Library-first de-risks (S1–S4 gated and useful before any CPS lowering); sugar-first is the nicer
  demo. Recommend **library-first**.
- **O5: logical-time unit.** Confirm `Async.delay` takes opaque *logical ticks* (Tier A) and that any
  real-millisecond `sleep` is a separate **Tier B** native (`pure:false`, quarantined) — i.e. two
  distinct functions, never one `sleep(ms)` that's "sometimes" deterministic.
- **O6: streams unification.** Confirm `Core.Stream` (pull-based, deterministic source) and
  generators are the *same* feature (one lazy state machine), shipped as S6 on the scheduler — not two
  parallel surfaces.
