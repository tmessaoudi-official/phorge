# Concurrency Prior-Art for the Byte-Identity Lens — PHP

**Stage 1 research.** Catalogue PHP's concurrency / async / reactive models, judge each by output
determinism, and map each onto Phorge's hard constraints: a single-threaded `Rc`-shared heap
(`Value` is **not** `Send`/`Sync`), zero external crates, a three-leg byte-identity spine
(`run` ≡ `runvm` ≡ transpiled PHP under `php -n` 8.5), and a per-feature Tier-A (gated) /
Tier-B (quarantined, fixture-tested) admission.

Verified environment facts (this session):
- `php -n` at the 8.5 floor (`/stack/tools/phpbrew/php/php-8.5.7/bin/php`): `class_exists("Fiber")`
  → **true**, `function_exists("preg_match")` → **true**. [Verified: ran `php -n -r`.]
- The bare `php` on PATH is 8.6.0-dev. The CI floor leg is 8.5. [Verified: `php --version`.]
- `Cargo.toml` core crate has **no `[dependencies]`** (only `wasm-bindgen` in the isolated
  `playground/` workspace member). `#![forbid(unsafe_code)]` on crate roots. [Verified: read
  `Cargo.toml`.]
- The Tier-B seam already exists and is generic: `NativeFn { pure: bool, … }`; the differential's
  `uses_impure_native(src)` reads the `pure` flag **from the registry** (not a hardcoded list) and
  skips any program importing an impure module; impure natives are fixture-tested in a **separate
  test crate** (`tests/process.rs`) that *may* call edition-2024-`unsafe` `std::env::set_var`
  because it is outside the `forbid(unsafe_code)` library. [Verified: read `src/native/process.rs`,
  `src/native/mod.rs`, `tests/process.rs`, grepped `tests/differential.rs`.]

This single seam is the whole reason the per-feature Tier model is cheap: admitting a Tier-B
concurrency native costs one `pure: false` flag + a `tests/<feature>.rs` fixture crate + a `php`
mapping. No harness edits.

---

## 0. The lens, stated precisely

PHP is canonically a **single-request, shared-nothing, synchronous** runtime: a request boots a
fresh interpreter, runs top-to-bottom, dies. Concurrency in PHP is therefore *bolted on* in five
distinguishable families, in rough order of how native they are:

1. **Fibers** (8.1 core) — stackful, cooperative, *explicitly* scheduled coroutines. Present under
   `php -n`. This is the one that matters most for Phorge.
2. **Generators-as-coroutines** (5.5 core, `yield`) — stackless cooperative coroutines; the
   pre-Fiber way to write a scheduler. Core, present under `php -n`.
3. **Userland event loops** — ReactPHP, Amp/AMPHP, Revolt. Pure-PHP libraries (Composer packages)
   that build an async runtime *on top of* `stream_select`/Fibers. **Absent under `php -n`** (no
   autoloader, no Composer), but their *model* is the reference design for a deterministic
   cooperative scheduler.
4. **Process-level parallelism** — `proc_open`/`pcntl_fork`/`popen`. `pcntl` is an extension
   (absent under `php -n`); `proc_open`/`popen` are **core** (present). This is OS-process
   fan-out, not in-process threads.
5. **True parallel threads** — `ext-parallel` (the modern one, ZTS-only), `pthreads` (dead,
   removed in 8.0), Swoole/OpenSwoole (full coroutine+event-loop extensions). **All extensions,
   all absent under `php -n`.** All map to the **hard-no** column for Phorge (shared-mutable OS
   threads; non-deterministic preemption; no `php -n` target).

The byte-identity question for each is: **is its OUTPUT a deterministic function of the program
text, and can all three legs reproduce that exact output?**

---

## 1. Fibers (PHP 8.1 core) — the cornerstone

### What they are

A `Fiber` is a *stackful* coroutine: a full call stack you can `suspend()` from anywhere (not just
the top function, unlike generators) and `resume()` later with a value. The scheduler is **you** —
there is no implicit yielding, no preemption, no timer interrupting a running fiber. A fiber runs
until it *voluntarily* calls `Fiber::suspend()`.

```php
$f = new Fiber(function (): void {
    $x = Fiber::suspend('first');   // hands 'first' back to resume()'s caller
    echo "resumed with $x\n";
});
echo $f->start();        // prints/returns 'first'
$f->resume('second');    // fiber prints "resumed with second"
```

### Scheduling model

**Cooperative, explicit, single-threaded.** Exactly one fiber executes at a time on the one OS
thread. Control transfer happens *only* at `start`/`resume`/`suspend`/`throw` call sites. There is
no run queue in the language itself — a *library* (Revolt, Amp) supplies the event loop that decides
which suspended fiber to resume next, and *that* policy is where determinism lives or dies.

### Determinism

The fiber primitive itself is **fully deterministic**: given the same suspend/resume sequence, the
interleaving is fixed by the program, byte-for-byte. Non-determinism enters only when the
*scheduler's resume order* is driven by a non-deterministic source — wall-clock timers, socket
readiness, `stream_select` over real fds. **A scheduler that resumes fibers in a fixed, data-driven
order (e.g. round-robin over a static task list, or a priority derived from task values) is
deterministic.**

This is the crux: **cooperative scheduling is deterministic; the I/O readiness that usually drives
real event loops is not.** Separate the two and the deterministic half is Tier-A-eligible.

### Mapping onto Phorge

Phorge's `Value` is `Rc`-shared and **not `Send`** → a single OS thread is forced anyway, which is
*exactly* the fiber execution model. There is no impedance mismatch: cooperative single-thread is
both what Phorge can do and what PHP Fibers do.

- **Rust leg feasibility (std-only):** Rust has no stackful-coroutine primitive in std (no
  `Fiber`). But Phorge does **not** need stackful coroutines to match this — it needs *its
  scheduler's output* to match PHP's. Two std-only implementation routes for the Rust legs:
  1. **Re-entrant VM drive** (already proven): the VM's `call_closure_value` + `run_until` push a
     frame and drive the shared `exec_op` re-entrantly (the higher-order-natives mechanism,
     [[higher-order-natives-reentrant-vm]]). A cooperative scheduler over **lambda/closure tasks**
     (each task a `Value::Closure`) needs **no** stackful suspension at the Rust level — the
     scheduler is an ordinary native loop that invokes closures in a deterministic order. The
     "suspend point" is modeled as *the closure returning a continuation value* (a state machine in
     Phorge), not a stack switch. **This is the recommended shape.**
  2. **Thread-as-fiber** (rejected): you *could* implement stackful fibers on the Rust side with OS
     threads + channels parked at suspend points, but `Value` is not `Send`, so a `Value` cannot
     cross the thread boundary. Dead on arrival. Correct, because it confirms the cooperative-
     closure model is the only viable one.
- **PHP leg target:** a Phorge cooperative scheduler transpiles to **either** a fixed-order loop
  invoking the task closures (if tasks are closure-shaped, the simplest and most byte-stable), **or**
  to real PHP `Fiber` objects with a Phorge-emitted deterministic scheduler loop. The *output* is
  what's gated, not the mechanism — same reframing as M6's `handle(Request)`: the socket glue is
  runtime, only the value-level contract round-trips. A pure-closure scheduler is the lower-risk
  PHP target (no Fiber emission needed); Fibers become an invisible engine optimization later.

**Verdict: Tier-A** for a *deterministic cooperative scheduler over closure tasks* (an `async`/`await`
sugar that lowers to a state machine + a fixed-order driver). The byte-identity argument: with a
fixed resume order and side-effect-free-or-ordered tasks, all three legs produce the same interleaved
output. **Tier-B** the moment the scheduler's resume order is driven by real timers/socket readiness
(that's the live-concurrency escape).

Confidence: **high** on the determinism analysis and the Rust-leg mechanism (the re-entrant VM drive
is already shipped); **medium** on the exact surface syntax (`async fn`/`await` vs explicit
`spawn`/`yield`) — that's a design choice, not a feasibility question.

---

## 2. Generators as coroutines (PHP 5.5+ core, `yield`)

### What they are

A `function` containing `yield` is a `Generator` — a *stackless* coroutine that produces a sequence
lazily and can receive values back via `$gen->send($v)`. Pre-Fibers, this was *the* way to write
cooperative multitasking in PHP (Nikic's "Cooperative multitasking using coroutines" is the
canonical reference; `yield` a "promise", a trampoline scheduler resumes with the result).

```php
function counter(): Generator {
    $n = 0;
    while (true) { $reply = yield $n; $n += ($reply ?? 1); }
}
```

### Scheduling / determinism

Same shape as Fibers — cooperative, explicit, single-thread — but **stackless**: you can only yield
from the generator's *own* top frame, not from a nested call. Output determinism is identical to
Fibers: fully deterministic unless the trampoline's resume order is driven by non-deterministic I/O.

### Mapping onto Phorge

This is the **closest analogue to a Phorge lazy iterator / stream**. Phorge has no `yield` today.
A `yield`-style generator maps to:
- **Rust legs:** a lazy iterator is most naturally a *Phorge-level state machine* (a struct holding
  the resumption state) advanced by a `.next()` native — again no Rust stackful coroutine needed,
  std-only trivially. Or: eagerly materialize into a `List` if the sequence is finite and pure
  (loses laziness but is byte-trivial — the M3 range `0..n` precedent already does this).
- **PHP leg:** transpile to a real PHP `Generator` (`yield`) — `yield` is core, present under
  `php -n`. The output (the produced sequence) is what's gated.

**Verdict: Tier-A** for *finite, pure lazy sequences* (a `Stream<T>`/generator whose elements are a
deterministic function of the source). Determinism is clean. **Relationship to the FRP/reactive item
below:** a pull-based deterministic stream *is* the gated reactive primitive. Note the open design
fork: eager-materialize (byte-trivial, matches the range precedent, loses infinite streams) vs a true
lazy state machine (supports infinite/large streams, more machinery, still deterministic). Recommend
**lazy state machine** so infinite generators (`counter()` above) are expressible.

Confidence: **high** (determinism + PHP target); **medium** on whether generators land as their own
feature or fold into the reactive-stream item (§5) — they're the same deterministic-pull primitive.

---

## 3. Userland event loops — ReactPHP / Amp / Revolt (the *model*, not the code)

### What they are

Pure-PHP libraries implementing an async runtime:
- **ReactPHP** — promise + event-loop, callback-style (`$loop->addTimer`, `->then()`).
- **Amp / AMPHP v3** — Fiber-based, `async`/`await`-style coroutines over Revolt's loop; reads like
  synchronous code.
- **Revolt** — the shared event-loop both modern Amp and React target.

All are **Composer packages** → **absent under `php -n`** (no autoloader). [Inferred: they are not
bundled with PHP core; `php -n` disables the ini that would even load an autoloader, and they are
not extensions — confirmed by the `php -n` no-Composer constraint stated in the project context.]

### Scheduling / determinism

Their loops multiplex **real I/O**: `stream_select`/`ext-event`/`ext-ev` over sockets, files, signals,
and **wall-clock timers**. This is **irredeemably non-deterministic** for byte-identity: the order in
which sockets become readable, and when timers fire relative to each other, depends on the OS, the
network, and the clock — none reproducible across the three legs.

### Mapping onto Phorge

- The **callback/promise/`async`-`await` *programming model*** is borrowable and Tier-A — *if* the
  underlying scheduler is Phorge's deterministic cooperative driver (§1), not a real-I/O event loop.
  i.e. Phorge can offer `async`/`await` ergonomics that *look* like Amp, lowered to a deterministic
  scheduler.
- The **real-I/O event loop itself is Tier-B** (the live-concurrency escape): a `phg serve`-style
  loop over real sockets/timers is non-gated, fixture-tested, transpiled to PHP that uses
  `stream_select` (core) or — *if* a dependency were ever allowed — a real event-loop lib. This is
  the same quarantine `Core.Process` already lives in, and aligns with M6's `Transport`-behind-a-trait
  plan (socket quarantined in a future `src/serve.rs`, tested outside `differential.rs`).
- A **Promise/Future *value type*** with deterministic resolution order (resolved by the cooperative
  scheduler, not I/O) is Tier-A and a clean value-level contract that round-trips.

**Verdict:** the *ergonomic surface* (promises / `async`-`await`) is **Tier-A** over a deterministic
driver; the *real event loop* is **Tier-B**. Do not import the libraries (impossible under `php -n`);
import the design vocabulary.

Confidence: **high** on the determinism split; **high** on the absent-under-`php -n` fact;
**medium** on whether a Promise value type is worth shipping before a scheduler exists.

---

## 4. Process / thread parallelism — `pcntl`, `proc_open`, `ext-parallel`, Swoole

### Sub-families and `php -n` availability

| Mechanism | Kind | Core or ext? | Under `php -n`? | Determinism |
|---|---|---|---|---|
| `proc_open` / `popen` / `exec` | OS subprocess | **core** | **present** | depends on child + read order |
| `pcntl_fork` / `pcntl_*` | OS fork | extension (`ext-pcntl`) | **absent** | non-deterministic interleave |
| `ext-parallel` (`\parallel\Runtime`) | real threads (ZTS) | extension | **absent** | non-deterministic |
| `pthreads` | real threads | extension (removed 8.0) | **absent / dead** | non-deterministic |
| Swoole / OpenSwoole | coroutine+loop+threads | extension | **absent** | non-deterministic |

[Determinism + core/ext classification: Inferred from PHP's documented extension model; `php -n`
disables ini-loaded extensions, and `pcntl`/`parallel`/Swoole are not compiled into a bare core.
`proc_open`/`popen`/`exec` are core stream/process functions, present. PCRE/Fibers/hash/BCMath
verified present this session; the *absence* of `pcntl`/`parallel` under `php -n` is consistent with
the project's stated "mbstring/PHPUnit/gmp/APCu ABSENT" line but was not directly run.]

### Determinism analysis — the two important splits

**(a) True shared-mutable-state OS threads (`ext-parallel`, `pthreads`, Swoole coroutine-with-shared-
state):** preemptively or opaquely scheduled, shared memory, data races. Output ordering between
threads is **non-deterministic by construction.** Maps to Phorge's **HARD NO**: `Value` is not
`Send`, there is no `php -n` target, and the output isn't reproducible across legs. *Explicitly
rejected by the developer.* No Tier even applies — it cannot be a feature.

**(b) Process fan-out (`proc_open`/`popen`):** spawn N OS subprocesses, collect their outputs. This
is **non-deterministic in completion order** but can be made **deterministic in *merged output***
by an **ordered merge**: launch tasks, but emit results in *submission order*, not completion order
(wait on child *i* before emitting result *i*). This is the realization of the developer's locked
**pure data-parallelism (`parallelMap`/fork-join with deterministic ordered merge)**:
  - **Today, all three legs run sequentially** → the "parallel" map is literally `List.map` with an
    ordered result → **byte-identical, Tier-A.** The parallelism is *semantic permission*, not yet
    physical.
  - **Later**, the *Rust legs* may execute the pure tasks on a real thread pool **iff** each task is
    side-effect-free and the merge re-imposes submission order — the output is unchanged, so it stays
    byte-identical. But `Value` not being `Send` blocks sharing `Value`s across threads: a physically-
    parallel `parallelMap` would need tasks expressed over `Send`-able inputs (e.g. transpile the task
    body to operate on primitives / serialized payloads) or `proc_open`-style process fan-out on the
    Rust side too. **Physical parallelism is a deferred optimization that must preserve output; the
    gated feature is the ordered-merge contract, which is sequential today.**
  - **PHP leg:** `parallelMap` transpiles to a deterministic `array_map` (sequential, ordered) — the
    simplest byte-stable target — with `proc_open` fan-out as a later invisible optimization, same
    discipline.

**Verdict:**
- Shared-state OS threads → **rejected, not a feature.**
- `parallelMap` / pure fork-join with ordered merge → **Tier-A** (sequential-today, output-defined),
  with physical parallelism a later non-output-changing optimization.
- Raw `proc_open`/`popen` subprocess spawning (live, side-effecting) → **Tier-B**, exactly where
  `Core.Process` already sits (impure, quarantined, fixture-tested, transpiles to PHP `proc_open`).

Confidence: **high** on the shared-threads rejection and the ordered-merge determinism argument;
**medium** on the `php -n`-absence of `pcntl`/`parallel` (consistent but not directly executed —
worth a one-line `php -n -r 'var_dump(function_exists("pcntl_fork"));'` confirmation in the design
phase).

---

## 5. Reactive / FRP streams (PHP: RxPHP, plus the deterministic-pull idea)

### What exists in PHP

**RxPHP** (ReactiveX port) — observables, operators (`map`/`filter`/`merge`/`debounce`), schedulers.
A Composer package → **absent under `php -n`.** Its *time-based* operators (`debounce`,
`throttle`, `interval`) are wall-clock-driven → **non-deterministic.** Its *pure transformation*
operators over a fixed source (`map`/`filter`/`scan`/`take`) are **deterministic.**

### Determinism split

- **Pull-based deterministic streams** (a lazy sequence + pure operators, resolved on demand): this
  is just §2's generator with a combinator API. **Deterministic, Tier-A.** Maps to a Phorge
  `Stream<T>` with `map`/`filter`/`take`/`fold` — lowering to either eager `List` ops (finite) or a
  lazy state machine; the PHP leg is `array_map`/`array_filter` (finite) or a `Generator` pipeline.
- **Push-based / time-driven reactive** (`interval`, `debounce`, hot observables over real events):
  driven by timers/external events → **non-deterministic, Tier-B** (the live escape), or simply out
  of scope until M6's event loop exists.

**Verdict:** the **deterministic pull-stream subset is Tier-A** and is the same primitive as §2's
generators (recommend unifying them: one `Stream<T>` feature covers "generator" and "reactive map/
filter over a fixed source"). The **time/event-driven reactive subset is Tier-B / deferred.**

Confidence: **medium-high** — the determinism split is clear; the *priority* of an FRP layer vs just
shipping generators + `Core.List` higher-order ops is a design-judgment call (`Core.List`
`map`/`filter`/`reduce` already ship via higher-order natives, so a separate `Stream` only adds
laziness/infinite sources).

---

## 6. The Tier-B mechanism, applied to concurrency (how a live-concurrency native lands)

The seam is already built and generic. To admit a Tier-B concurrency native (e.g.
`Core.Async.spawn` over real timers, or `Core.Process.run` subprocess fan-out):

1. **Declare it `pure: false`** in its `src/native/<leaf>.rs` `NativeFn`. [Verified mechanism:
   `process.rs` does exactly this for `Core.Process`/`Core.Env`.]
2. **It is auto-quarantined** — `tests/differential.rs::uses_impure_native(src)` reads the `pure`
   flag from the registry and skips any program importing the module. **No harness edit.** [Verified:
   grepped the function; it filters `registry().filter(|n| !n.pure)` and matches `import {module}`.]
3. **Fixture-test it in a dedicated crate** (`tests/<feature>.rs`) under a controlled environment.
   That crate is outside `#![forbid(unsafe_code)]`, so it may set up process state / env / fixed
   inputs the way `tests/process.rs` sets argv and env vars. Assert `cmd_run ≡ cmd_runvm` (the Rust
   legs always agree — they share the process) even though the PHP leg is not gated.
4. **Provide a `php` mapping** — the impure native still transpiles (e.g. `proc_open`, `stream_select`,
   real `Fiber` scheduler), it's just not *byte-gated*.
5. **Document per-backend** + ship a walkthrough (not a gated example) under `examples/<feature>/`, the
   `examples/process/` precedent.

The genuinely-live concurrency escape (real sockets/timers, side-effecting physical parallelism) rides
this seam unchanged. The future `src/serve.rs` + `Transport`-trait quarantine (M6 plan) is the natural
home for socket-driven scheduling.

---

## 7. Summary table — every model, its determinism, its Phorge home

| # | PHP model | `php -n`? | Scheduling | Output determinism | Phorge mapping | New VM Op? |
|---|---|---|---|---|---|---|
| 1 | **Fibers** (8.1) | ✅ present | cooperative, explicit, 1-thread | **deterministic** unless resume order is I/O-driven | **Tier-A** cooperative scheduler over closure tasks (re-entrant VM drive, already shipped); Tier-B if timer/socket-driven | **No** (closure-task scheduler = `CallNative` + re-entrant `run_until`) |
| 2 | **Generators** `yield` (5.5) | ✅ present | cooperative, stackless | **deterministic** (pure finite/lazy seq) | **Tier-A** `Stream<T>`/generator as Phorge state machine → PHP `Generator`; or eager `List` (finite) | **No** (state machine in Phorge; or new `yield` desugar — front-end only) |
| 3 | **ReactPHP/Amp/Revolt** | ❌ Composer | event loop over real I/O+timers | **non-deterministic** (real I/O) | ergonomics (promise/`async`-`await`) **Tier-A** over deterministic driver; real loop **Tier-B** | No (sugar lowers to scheduler) |
| 4a | **`ext-parallel`/`pthreads`/Swoole threads** | ❌ ext | preemptive, shared mem | **non-deterministic** | **HARD NO** (`Value` not `Send`, no target, rejected) | — |
| 4b | **`proc_open`/`popen` fan-out** | ✅ core | OS subprocess | non-det completion; **det with ordered merge** | `parallelMap`/fork-join **Tier-A** (seq today, ordered merge); raw spawn **Tier-B** | **No** (`List.map`-shaped native) |
| 4c | **`pcntl_fork`** | ❌ ext | OS fork | non-deterministic | covered by 4a/4b; no extra surface | — |
| 5 | **RxPHP** reactive | ❌ Composer | pull (det) / push+timers (non-det) | **det** for pure pull; **non-det** for time/event | pull-stream subset **Tier-A** (= #2); time/event **Tier-B/deferred** | No |

**One-line bottom line for the design phase:** the deterministic spine of PHP concurrency —
cooperative Fiber/generator scheduling with a **fixed, data-driven resume order**, and pure
data-parallelism with an **ordered merge** — is **Tier-A and needs no new VM Op** (it rides the
already-shipped re-entrant `run_until` closure-invoker and the higher-order-native shape). Everything
non-deterministic (real event loops, timers, sockets, shared-state OS threads) is either **Tier-B**
on the existing `pure: false` quarantine seam, **deferred to M6's `Transport`-quarantined server**, or
**hard-rejected** (shared-mutable threads). The reframing holds perfectly: *everything transpiles*;
the only axis that matters is whether the **resume/merge order** is a function of the program text
(gated) or of the wall clock / OS scheduler (quarantined).
