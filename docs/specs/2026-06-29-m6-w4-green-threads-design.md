# M6 W4 — Green threads: `spawn` + channels (design)

> Status: **design-locked, build in progress** (Spine-4 S4.3 of the 2026-06-29 marathon). Decided with
> the developer: build now, **Rust-backend-only + quarantined from the PHP oracle** (the `serve`
> precedent). Rejected transpile→synchronous-PHP — it would make a concurrent program behave
> differently under PHP than on the VM, breaking the byte-identical `run≡runvm≡PHP` spine.

## 1. Goal & shape

Uncolored cooperative concurrency: a `spawn` that starts a green task and typed channels for
communication, **on a single OS thread** (the `Value` heap is `Rc`, not `Send` — real OS-thread
parallelism is impossible without a redesign; this is the M6-design's "single-threaded forced" note).
"Uncolored" = no `async`/`await` keyword colouring; any function can be `spawn`ed, and a channel
`recv` simply yields the current task until a value is available.

Determinism is non-negotiable: the scheduler MUST be deterministic so `run` (interpreter) and `runvm`
(VM) produce **byte-identical** output. PHP has no green threads, so a `spawn`/channel program is
**quarantined from the PHP oracle** exactly like `src/serve.rs` (added to the differential SKIP list);
`run≡runvm` stays fully gated.

## 2. Surface syntax (locked)

- **`spawn <call>`** — an expression returning a `Task<T>` handle, where `<call>` is a function/closure
  call returning `T`. The task is *scheduled*, not run, at `spawn`; it runs when the scheduler next
  picks it (cooperative — see §4). Example: `Task<int> t = spawn compute(5);`
- **`Core.Channel`** — a typed FIFO. Constructed `Channel.new<T>()` (or `Channel<T>` literal TBD in
  build step 2); methods `ch.send(v)` (never blocks — unbounded queue this slice) and `ch.recv() -> T`
  (yields the task until a value is available). A bounded/closeable channel is a follow-up.
- **`t.join() -> T`** — yield until task `t` completes, then yield its result. (Build step 3.)
- **`yield`** — an explicit cooperative yield point (optional sugar; `recv`/`join` already yield).

Naming/keyword: `spawn` is a **contextual** keyword in expression position (like `var`/`when`/`as`) so
existing identifiers named `spawn` are unaffected — follow the [[contextual-var-and-reserved-names]]
lesson (contextual = fix, hard-reserve = bug). `Channel`/`Task` are reserved type names under `Core`.

## 3. Value model (new variants — but NOT in the byte-identity-shared kernels)

- `Value::Channel(Rc<RefCell<VecDeque<Value>>>)` — shared mutable FIFO (M-mut handle semantics: a
  channel is a *handle*, like `Instance`, not a COW value).
- `Value::Task(Rc<RefCell<TaskState>>)` where `TaskState { result: Option<Value>, … }`.
- Both are **opaque** to arithmetic/compare kernels (`value.rs`): they never participate in
  `+`/`==`/interpolation as operands (checker forbids it). So `value::` kernels are untouched → the
  single-sourcing invariant ([[value-kernels-single-sourced]]) holds.

## 4. LOCKED ARCHITECTURE (developer decision, 2026-06-29): uniform coroutines + single-sourced scheduler

The chosen model — the complete one, no capability restriction, both backends independent, byte-identical
by construction:

- **One shared, deterministic SCHEDULER kernel** in a neutral module (`src/green/sched.rs` or
  `value`-adjacent), single-sourced exactly like the `value.rs` arithmetic/compare kernels
  ([[value-kernels-single-sourced]]). It owns: the ready run-queue (FIFO), per-channel wait-lists, the
  wake order on `send`/task-completion, and the round-robin pick. **Both backends call the SAME
  scheduler**, so scheduling decisions cannot drift → `run` and `runvm` produce identical task
  interleaving → byte-identical output. This is the load-bearing invariant.
- **Executor stays per-backend and independent** — each task is a **stackful coroutine** running *that
  backend's own* engine: the interpreter walks the AST inside its coroutine; the VM runs bytecode
  inside its coroutine. Same suspension *mechanism* (coroutine yield at `recv`-on-empty / `join`-on-
  incomplete / explicit `yield`), same scheduler driving both, but two genuinely independent executors
  the differential still cross-checks. Stacks switch on **one OS thread**, so the `!Send` `Rc` `Value`
  heap never moves between threads — the reason real OS-thread parallelism is impossible is exactly why
  same-thread coroutines are the fit.
- **Coroutine crate = the 4th dependency** (a stackful-coroutine primitive — candidates `corosensei`
  (modern, maintained) or `generator`; pick by: maintenance, audit, `no_std`-not-required, miri-clean).
  Admitted under the SAME amended dependency-policy criterion as `ctrlc`: std has no stackful-coroutine
  primitive, the only std-native path is hand-rolled `unsafe` stack-switching, and a vetted crate
  confines that `unsafe` out of phorj's `#![forbid(unsafe_code)]`. It is a low-level *primitive*, NOT an
  async runtime/framework (tokio et al. remain disallowed). Feature-gated (`green`/`signals`-style) off
  for the playground? — **NO**: green threads must run in the playground (cooperative, in-browser), so
  the coroutine crate must support `wasm32` OR the playground uses a wasm-friendly fallback. **VERIFY
  wasm32 support of the chosen crate FIRST** — this is a hard gate on the crate choice (corosensei has
  limited wasm support; may need a wasm fallback path). If no crate supports wasm cleanly, the
  playground falls back to the VM-runs-tasks model *only in wasm* (documented), while native keeps the
  full uniform-coroutine model.

### §4b — VERIFIED wasm constraint + final hybrid (developer-locked 2026-06-29)

**[Verified]** `corosensei` (and stackful coroutine crates generally) compile on native but **fail on
`wasm32-unknown-unknown`** (no native stack to switch — confirmed by a scratch `cargo build --target
wasm32-unknown-unknown`, 5 errors). The playground runs BOTH `pg_run` (interpreter) and `pg_runvm` (VM)
in-browser, and the interpreter is the backend needing coroutines → uniform-coroutines cannot run green
threads in the playground.

**LOCKED resolution — Hybrid (matches mechanism to where correctness is gated):**
- **Native:** uniform stackful coroutines on both backends + the shared scheduler kernel — full
  independence, byte-identical `run≡runvm`, enforced by the **native** differential gate.
- **`#[cfg(target_arch="wasm32")]`:** the interpreter delegates task *execution* to the VM's frame-swap
  suspension (no coroutine). Green threads run in the playground; `pg_run≡pg_runvm` holds. Independence
  is reduced ONLY in the browser demo — which never gated correctness, so this is principled, not a
  bandaid. The shared scheduler kernel is identical on both targets; only the *executor wiring* differs.
- The **scheduler kernel (`green::sched`) is target-independent** and built/tested first (this is the
  safe, decoupled first increment — pure logic, wired to no backend).

### Original analysis (superseded by §4/§4b above, kept for context)

**Cooperative, deterministic, single-threaded.** A run-queue of ready tasks (FIFO). The "main" program
is task 0. `spawn` enqueues a new task. A task runs until it **yields** (calls `recv` on an empty
channel, or `join` on an incomplete task, or hits `yield`) or **completes**. On yield, the scheduler
picks the next ready task (round-robin FIFO → deterministic). On completion, a task's result is stored
and any tasks `join`ing it become ready.

**Interpreter:** re-entrant already (`call_closure`/native higher-order). A task = a suspended Rust
call... but the tree-walker can't suspend a native Rust stack mid-evaluation without a coroutine.
**Decision:** the interpreter runs tasks to their next yield via an explicit **trampoline** — model a
task's continuation as a re-runnable closure invocation is NOT enough (deep stacks). Two candidate
implementations, pick in build step 4 with a spike:
  - **(A) Generator/stackful via OS-thread-per-task parked on a channel** — REJECTED: `Value` is `!Send`.
  - **(B) Explicit state machine / CPS at yield points** — the interpreter and VM both treat `recv`/
    `join`/`yield` as *scheduler trap* points: the running task's frame state is saved and control
    returns to the scheduler loop. On the **VM** this is natural — the VM already has a reified frame
    stack (`run_until`, [[higher-order-natives-reentrant-vm]]); a task IS a saved `Vec<Frame>` + value
    stack, and the scheduler swaps frame stacks. On the **interpreter** (native Rust recursion) this is
    the hard case — likely requires running each task's interpreter on its own *stackful coroutine*.
    Since `Value` is `!Send` but coroutines stay on one OS thread, a stackful generator crate would be
    a dependency (dep-policy fork) OR `std`-only via a manual stack — both heavy.

**This is the crux and the real cost.** The VM side is tractable (swap reified frame stacks in the
scheduler). The interpreter side is the blocker: a tree-walking interpreter cannot suspend mid-stack
without coroutines. **Build-step-4 spike MUST resolve this before committing to the model.** Options to
de-risk: (i) restrict `spawn`bed functions to a *non-recursive, yield-at-top-level* subset so the
interpreter can run them as a state machine; (ii) make the interpreter's task execution itself use the
VM (compile spawned bodies to bytecode even under `run`) — but that breaks the "two independent
backends agree" property. **Likely outcome: the interpreter models tasks with a bounded explicit
continuation, accepting a documented subset, and the VM does the full version; both must still agree
byte-for-byte on that subset.** ← OPEN, spike first.

## 5. New `Op`s (VM)

Tentative (confirm during build): `Op::Spawn(fn_idx, argc)` (enqueue a task), `Op::ChannelNew`,
`Op::ChannelSend`, `Op::ChannelRecv` (yields), `Op::Join` (yields). Each extends the three coupled
matches ([[op-variant-match-coupling]]): `vm.rs exec_op`, `chunk.rs validate`, `compiler.rs
stack_effect` — in the same commit. The scheduler lives in `vm.rs` around the main `exec_op` loop
(`run_until` generalized to "run the current task until it traps").

## 6. Quarantine & examples

- The differential harness SKIPs a `spawn`/channel example for the PHP oracle (extend the existing
  `SKIP (impure/quarantined)` list that already covers `dates`/`serve`-style); `run≡runvm` is still
  asserted and MUST be byte-identical.
- `examples/guide/concurrency.phg` ships (producer/consumer over a channel; deterministic output) —
  auto-included in the playground (cooperative green threads run in-browser; regenerate `examples.js`).
- Transpiler: `spawn`/channel ops emit a clean **`E-CONCURRENCY-NO-PHP`** compile error (or are simply
  absent from the transpile surface) — NEVER a silent synchronous lowering.

## 7. Incremental build plan (gate each step green)

1. **Surface + value model:** `spawn` contextual keyword (parser, expr position), `Channel`/`Task`
   reserved types, `Value::Channel`/`Value::Task`, checker types `Ty::Channel(T)`/`Ty::Task(T)`. No
   scheduler yet — `spawn f()` runs synchronously, `recv` on a non-empty channel works. Gets the types
   + surface in; byte-identical (synchronous degenerate case).
2. **Channels (synchronous):** `Channel.new`, `send`, `recv` over the `VecDeque` — single-task, no
   yielding (recv on empty = fault for now). Proves the value/op plumbing.
3. **Coroutine crate spike (GATING):** pick the stackful-coroutine crate, add it (4th dep + policy
   row), and prove a minimal native spike (spawn a coroutine, yield, resume, drop) AND **verify wasm32
   support** (the playground gate — §4). If wasm is unsupported, decide the wasm fallback before going
   further. Land nothing user-facing until this spike is green on native + wasm.
4. **Shared scheduler kernel + uniform coroutine executor:** build `green::sched` (run-queue, channel
   wait-lists, deterministic wake/pick) ONCE; wire BOTH backends to run each task as a coroutine driven
   by it. `recv`-on-empty / `join`-on-incomplete / `yield` suspend the coroutine back to the scheduler.
   Gate: `run≡runvm` byte-identical on a producer/consumer program (quarantined from PHP).
5. **`join`, explicit `yield`, the `examples/guide/concurrency.phg` example (+ regenerate playground
   `examples.js`), transpiler `E-CONCURRENCY-NO-PHP`, quarantine wiring, docs, KNOWN_ISSUES.**

> Steps 1–2 (surface + value model + synchronous channels) are byte-identical foundations and can land
> first; the COMPLETE concurrency arrives at step 4. This is incremental delivery of the complete
> design — NOT a permanent half-solution (the developer rejected restricted-subset / synchronous-only /
> independence-losing models; this model has none of those compromises).

> Each step: design-check → TDD → full gate (`run≡runvm`; the concurrency example quarantined from PHP)
> → clippy/fmt → commit. Stop and surface to the developer if the §4 interpreter-coroutine spike shows
> the determinism/parity cost is higher than the feature warrants.
