# Big Marathon: Cross-pkg lift → Soundness → Stdlib charter → Concurrency Plan

> Started 2026-06-29 from `e9d95a6`. Fully autonomous (`_AUTONOMOUS_3C=1`, full 30/8).
> Byte-identical `run ≡ runvm ≡ real PHP 8.5` spine; examples-ship-with-features; commit green
> self-contained changes as we go (project git autonomy). Gate:
> `PHORJ_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORJ_REQUIRE_PHP=1 cargo test --workspace`
> + `cargo clippy --all-targets` + `cargo fmt --check`.

## Decisions Log
- [2026-06-29] DONE (S4.3 **step 2 — surface + value model + synchronous channels**, `ce2b2c3` +
  `2ce715e`): `spawn <call>`→`Task<T>` (contextual kw, `Expr::Spawn`), `t.join()`, typed `Channel<T>`
  (`Channel.create()`/`send`/`recv`). `Value::Channel`/`Task`; `Channel`/`Task` reserved built-ins
  (modeled `Ty::Named`, dispatch via `CTy::Class` — NO new `Ty`/`CTy`); 5 new ops
  `Spawn`/`ChannelNew`/`ChannelSend`/`ChannelRecv`/`Join` (3 coupled matches). **Synchronous-degenerate:**
  spawn runs eagerly (byte-identical by construction — no scheduler to drift); recv-on-empty faults.
  Quarantined from PHP (`E-CONCURRENCY-NO-PHP` + harness skip). `examples/guide/concurrency.phg`; +6
  differential tests; 8 codes in `phg explain`. 1517 tests green, clippy+fmt clean, PHP-8.5 oracle.
  **Steps 1+2 landed together.** Constructor renamed `Channel.new()`→`Channel.create()` (`new` is a
  keyword token). `Task`/`Channel` reservation forced renaming `field-init.phg`'s `class Task`→`Parcel`.
- [2026-06-29] AGREED (S4.3 step 3 executor): developer chose **Option A — the locked uniform-coroutine
  design** (uniform stackful coroutines on BOTH backends native + interpreter→VM on wasm; adds
  corosensei) over my simplification (B). Proceed with A. First action = the spec-mandated coroutine
  spike: prove corosensei suspends from deep in a nested call **without unsafe in phorj's crate**
  (`#![forbid(unsafe_code)]` × deep-stack-suspend is the crux); resolve before committing the model.
- [2026-06-29] OPEN FORK (S4.3 **step 3 — coroutine executor**, surfaced to developer): the
  byte-identity-critical scheduler-wiring step. Recommendation pending developer choice — see the
  session's recommendation: VM frame-swap (target-independent, no coroutine, the "natural" path the
  spec calls tractable) + interpreter stackful-coroutine on native / interpreter-delegates-to-VM on
  wasm. This is a simplification of the locked "uniform coroutines on both backends" (§4b) that removes
  the riskiest piece (VM-on-coroutine, impossible on wasm anyway) with zero correctness/independence
  cost. Adds the 4th dependency (corosensei) — a hard-to-reverse step worth confirming before sinking
  the integration. Per the design spec's own guidance ("stop and surface if the interpreter-coroutine
  cost is higher than the feature warrants").
- [2026-06-29] AGREED (session 3, S4.3 architecture — developer wanted "the most complete, no half-
  solution"): green threads = **uniform stackful coroutines on BOTH backends + a single-sourced
  deterministic scheduler kernel.** One shared `green::sched` (run-queue, channel wait-lists, wake/pick)
  drives both — like `value.rs` kernels are single-sourced, so scheduling can't drift → byte-identical
  `run≡runvm`. Each task is a stackful coroutine running that backend's own engine (interpreter walks
  AST / VM runs bytecode), suspending at `recv`/`join`/`yield` — preserves backend independence AND full
  capability (no restricted subset). Adds a **4th dependency** (stackful-coroutine crate, e.g.
  corosensei/generator) under the ctrlc criterion (std lacks it; confines unsafe). **HARD GATES before
  user-facing code:** (1) crate must support **wasm32** (playground runs green threads in-browser) or a
  wasm fallback is decided; (2) a native+wasm coroutine spike must be green. Rejected literal "1+3" as
  incoherent (coroutines + VM-delegation = worst of both). Full design: `docs/specs/2026-06-29-m6-w4-
  green-threads-design.md` §4 (LOCKED) + §7 build steps. **Milestone-scale implementation — begins fresh.**
- [2026-06-29] AGREED (session 3, Spine-4 forks): **S4.2 = add the `ctrlc` dependency + build graceful
  shutdown now** (developer authorized spending dependency-policy budget; SIGINT/SIGTERM → shutdown flag
  → stop accepting → drain in-flight → exit 0; NOT the unsafe handler). **S4.3 = build the green-thread
  runtime now, Rust-backend-only + quarantined from the PHP oracle** (like `serve`; rejected
  transpile→sync-PHP because it breaks the byte-identical spine on concurrent programs). Design-spec
  first, then build incrementally. **The `spawn`/channels example SHIPS even with no PHP equivalent** —
  byte-identical `run≡runvm`, added to the differential PHP-oracle SKIP/quarantine list (like
  `dates.phg`).
- [2026-06-29] AGREED (session 3): developer set the **project-scoped ask-human-gate bypass** ("Yes — set bypass, run it all") + the autonomous-3c bypass — run the remaining marathon (S2.1-broad remainder → S2.3 must-use B/C → Spine-4 M6 W4 concurrency capstone) **fully autonomously, back-to-back**, gating each slice on the full PHP-oracle + differential + clippy/fmt before commit; stop only on a genuine design fork.
- [2026-06-29] AGREED: Marathon = **all four spines**, in the recommended dependency order, **fully autonomous** (full 30/8).
- [2026-06-29] AGREED: Order = (1) Cross-package M-RT lift → (2) Soundness long-tail close → (3) Stdlib charter + breadth → (4) Concurrency + server (M6 W4). Rationale: #1 unifies type system ↔ modules and unblocks core.json multi-package + cross-package stdlib; #2 cleans the now-unified base; #3 writes the charter then breadth (multi-package core.json now possible); #4 capstone capability on a solid foundation.
- [2026-06-29] AGREED (session 3, "new big thing + marathon"): developer chose **"all of 1 and 2 and 4 in the recommended order autonomously"** = full Spine-2 soundness long-tail → Spine-4 M6 W4 concurrency capstone, with Spine-3 breadth interleaved as low-risk warm-ups. Pacing: **one heavy slice per context window, commit green, let compaction carry the marathon.** Immediate next = S2.2 method return-overloading (design recorded checkpoint #4).
- [2026-06-29] AGREED (session 2, post-breadth): developer pushed the 13 marathon commits; directive = **do all the rest**, in this **confirmed order** — **Spine 2 soundness first (tractable→heaviest): S2.4 while-let guards → S2.2 method return-overloading → S2.1 generic-result VM operand → S2.3 must-use B/C; then Spine 4 W4 concurrency (capstone) on the cleaned base; Spine-3 breadth interleaved as low-risk warm-ups.** Rationale: don't build the concurrency layer atop known run↔runvm parity gaps; ramp difficulty up rather than opening on the heaviest item.

## S2.1-broad REMAINDER — implementation design (pick-up-ready, for a fresh context)

> The narrow free-fn case (`1163e47`) and the generic-method-param-echo case (`3a95755`) both rode an
> AST field (`generic_ret_from_param`) into the compiler. The REMAINDER cannot — it needs the
> *reified instance type argument* at a call/read site, which the AST field can't carry:
> - `box.get() + 1` where `Box<int>` and `get()` returns the **class** `T` (via a field) — the operand
>   type is `int` only because *this receiver* is `Box<int>`; a different `Box<string>` differs.
> - a generic **field** read `box.value + 1` (value: `T`).
> - a `List<T>`-element/`Map<K,V>`-value return, or a return computed from several params.
>
> **Root cause:** the compiler's `CTy::Class(String)` carries **no type arguments**, and `ctype` has no
> per-expression reified-type source. The checker DOES compute the precise reified type at each such
> expression (it already types `box.get()` as `int`).
>
> **Chosen approach — checker-produced, span-keyed reified-operand side-table (NOT a CTy::Class arg
> extension).** Extending `CTy::Class` to carry args touches every CTy match site (huge blast radius)
> and still wouldn't cover `List<T>` returns. Instead:
> 1. **Checker:** during `check_expr`, when an expression's resolved `Ty` is a concrete scalar
>    (`Int`/`Float`/`Bool`/`String`) **but** the expression is a generic call/method-call/field-read
>    whose *static* shape would erase to `Other` (i.e. the precise type is only known via generics),
>    record `reified_operand: HashMap<usize /*expr span.start*/, CTy>`. Map `Ty -> CTy` via the existing
>    `resolve_cty`-equivalent. Keep it MINIMAL: only insert when the Ty is a specializable operand
>    (Int/Float) — that is the only thing the VM `ctype` needs; everything else stays `Other` safely.
> 2. **Thread it out** of `check_resolutions` as a 5th return (alongside `html`/`ufcs`/`overload_renames`)
>    and into `compile`/`compile_program` — the friction point: `compile_program(&Program)` has no
>    side-channel today. Add a parallel entry `compile_program_with(program, &reified)` (keep the old
>    one delegating with an empty map) so the many `compile`/test callers stay source-compatible; the
>    `cmd_runvm` path passes the map, tests/`compile()` default to empty.
> 3. **Compiler `ctype`:** as the FIRST check in `ctype`, `if let Some(cty) = self.reified_operand.get(&span_of(e)) { return Ok(cty.clone()); }`. Every `Expr` variant carries a `span` — add a small `expr_span(&Expr)` helper (or reuse one if present). This subsumes the field-based `generic_ret_from_param` paths too (they can stay; the side-table just wins first).
> **Span stability:** the expand pipeline (alias/html/generics-erase/ufcs/overload) preserves expression
> spans (rewrites carry original spans), so the checker-time span keys still align with the compiled AST.
> **VERIFY THIS FIRST** in the fresh context with a probe (a generic field read through the pipeline),
> because UFCS/overload rewrites REPLACE call nodes — a replaced node's span may differ. If a key misses,
> the operand falls back to `Other` (VM rejects) — a *safe* failure (no silent wrong answer), caught by
> an `agree` test. **Gate every case with an `agree_out_php` test**: `box.get()+1`, `box.value+1`,
> `List<int>`-element return `+1`, `Map`-value `+1`. Example: extend `examples/guide/generic-types.phg`.
> Scope: still `package Main`; no new `Op`/`Value`. **Do in a fresh context — multi-site + byte-identity-critical.**

## Spine-4 (M6 W4 concurrency capstone) — scoping for a fresh context

> Spine-2 + Spine-3 are done; this is the LAST marathon item. It is **milestone-scale** — three sub-slices
> of ascending risk. The serve layer (`src/serve.rs`, 617 lines) is **quarantined OUTSIDE `differential.rs`**
> (tested only in `tests/serve.rs`), so S4.1/S4.2 do NOT touch the byte-identity spine — but S4.3 does.
>
> Current state (M6 W3, `84ddc32`): concurrent serving via a bounded **OS-thread pool** (`serve_pool`,
> `--workers N`, `Arc<Program>` Send+Sync), one request per connection (`Connection: close`).
>
> **S4.1 — HTTP/1.1 keep-alive (bounded, serve-layer only, LOW risk).** Today `recv` frames one request
> then drops the stream. Add a per-connection loop: read request → dispatch → write response → if the
> request is HTTP/1.1 and lacks `Connection: close` (or HTTP/1.0 *with* `Connection: keep-alive`), keep
> the socket and read the next; else close. Needs: a read **timeout** (`stream.set_read_timeout`) to reap
> idle keep-alive sockets; emit `Connection: keep-alive`/`close` + always a correct `Content-Length` (the
> serializer already sets it); cap requests-per-connection (EV-7, e.g. 100). Test in `tests/serve.rs`:
> two requests on one socket (pipelined / sequential) get two responses; an idle socket closes on timeout.
> No `differential.rs` impact. **This is the safe first down-payment.**
>
> **S4.2 — graceful shutdown / join (bounded, serve-layer only, LOW-MED risk).** Install a SIGINT/SIGTERM
> handler (std-only: a `static AtomicBool` flipped by `signal_hook`-free raw handler, or a self-pipe;
> verify std capability — may need a tiny `libc`-free trick or accept a `Ctrl-C`-only `ctrlc` shim, but
> the dependency policy forbids casual deps → prefer the self-pipe / atomic + non-blocking accept loop).
> On signal: stop `accept()`ing, let in-flight workers drain (join the pool), exit 0. Per-worker request
> counters for the `--dev` banner. Test: spawn server, fire a request, send the signal, assert clean exit
> + the in-flight response completed.
>
> **S4.3 — uncolored `spawn` + channels (green threads) — the HARD part, milestone in itself, HIGH risk
> (touches the VM + spine).** Design FIRST (its own spec). Cooperative scheduler over the VM's reified
> call frames: a `spawn <call>` expression starts a green task; `Channel<T>` send/recv yield to the
> scheduler. The `Value` heap is `Rc` (not `Send`), so this is **single-OS-thread cooperative** (matches
> the design's "single-threaded forced" note) — NOT the OS-thread pool. New surface: `spawn` keyword,
> `Core.Channel` (or a `chan` primitive), scheduler in the VM. Determinism: a deterministic scheduler
> (round-robin, fixed yield points) so `run ≡ runvm` stays gated; PHP transpile target = ??? (PHP has no
> green threads — likely `spawn` transpiles to immediate synchronous call + a documented divergence, OR
> the concurrency layer is Rust-backend-only and quarantined from the PHP oracle like `serve`). **This
> needs a full design pass + developer decision on the PHP-target story before any code.**
>
> **Recommended fresh-context order:** S4.1 (ship) → S4.2 (ship) → then OPEN S4.3 with a dedicated design
> spec + a developer fork on the PHP-target story (it may be a Rust-backend-only feature, breaking the
> 3-way oracle for `spawn` specifically — a genuine design decision, not autonomous).

## Progress

- **Marathon checkpoint #13 (session 3): S4.3 build STARTED — scheduler kernel landed.** VERIFIED
  `corosensei` fails `wasm32` compile (scratch build, 5 errors) ⇒ locked the **Hybrid** (coroutines
  native both backends + interpreter-delegates-to-VM in wasm only; correctness gate is native, so
  browser-only independence reduction is principled). First safe increment: **`src/green/sched.rs`** —
  the single-sourced, backend-agnostic scheduler kernel (`Scheduler`, `TaskId`/`ChanId`/`Trap`;
  FIFO run-queue, per-channel FIFO recv wait-lists, join-wake-in-order, deadlock detect). Pure logic,
  wired to NO backend ⇒ zero byte-identity-spine risk; 6 unit tests green. Next increments (spec §7):
  surface+value model → channels → coroutine executor (native) + VM-frame-swap (wasm) driven by this
  kernel → join/yield/example/quarantine. Commit pending gate-green.

- **Marathon checkpoint #12 (session 3): Spine-4 S4.3 DESIGN-SPEC written** (`docs/specs/2026-06-29-m6-w4-green-threads-design.md`),
  Rust-only-quarantine model locked. **It surfaced a genuine architectural fork (escalated to developer):**
  the VM side of a cooperative scheduler is tractable (swap reified frame stacks, building on `run_until`),
  but the **tree-walking interpreter cannot suspend mid-stack without coroutines**, and `Value` is `!Send`
  (no OS-thread-per-task). So deterministic byte-identical `run≡runvm` green threads need one of: (A)
  restricted spawned-body subset the interpreter runs as a state machine; (B) run tasks on the VM even
  under `run` (breaks backend independence); (C) a stackful-coroutine dependency (dep-policy fork); (D)
  ship only steps 1–2 (surface + synchronous channels, byte-identical, bounded) now and spike the scheduler
  fresh. Incremental build plan (steps 1–5) in the spec; **step-4 interpreter-coroutine spike is the gating
  risk.** Awaiting the developer's call on A/B/C/D before coding the scheduler.

- **Marathon checkpoint #11 (session 3): Spine-4 S4.2 graceful shutdown DONE.** Added `ctrlc` (3rd
  dependency, developer-authorized; dependency policy amended with a narrowly-scoped "OS-signal
  handling" 3rd domain + ctrlc's unsafe stays inside the crate so phorj keeps `forbid(unsafe_code)`).
  `install_shutdown_handler()` (cfg `signals`) flips an `Arc<AtomicBool>` on SIGINT/SIGTERM; both serve
  paths poll it via a non-blocking listener + `ACCEPT_POLL_INTERVAL` (std has no accept-timeout):
  single-threaded `TcpTransport::recv` returns `Ok(None)` (clean exhaustion → serve loop exits); the
  pool's new `serve_pool_with` stops accepting, drops the work channel, and **joins** all workers
  (in-flight drains). `signals`-off (playground) = run-until-killed, verbatim pre-S4.2. New
  `tests/serve.rs::pool_graceful_shutdown_drains_and_returns` (+ both signals-on/off builds verified).
  `examples/web/README.md` + dependency-policy doc updated. Serve-layer only (OUTSIDE the byte-identity
  spine). Commit pending gate-green. **Next: S4.3 green-thread runtime (design-spec → incremental build,
  Rust-only quarantine).**

- **Marathon checkpoint #10 (session 3): Spine-4 S4.1 HTTP/1.1 keep-alive DONE.** Transport-internal —
  NO `Transport` trait change (my earlier worry was overstated): the keep-alive decision is made from the
  request's `Connection` header (`request_wants_keepalive`) + the response's (`response_keeps_alive`,
  false on the `Connection: close` 500s), in BOTH paths. Single-threaded `TcpTransport`: `recv` retries
  the kept `self.current` first, `send` keeps it when permitted. Pool `worker_loop`: an inner per-stream
  keep-alive loop. **Safety rule: keep-alive only when a `--timeout` is set** (the idle-socket guard — no
  timeout ⇒ one request per connection, verbatim pre-S4.1, so an idle client can't pin a thread/worker).
  Per-connection cap `MAX_REQUESTS_PER_CONN=100` (EV-7). Serve-layer only (OUTSIDE the byte-identity
  spine); 3 new `tests/serve.rs` (single-threaded 2-req, `Connection: close` closes-after-one, pool
  2-req); `examples/web/README.md` updated. Commit pending gate-green. **Next: S4.2 graceful shutdown,
  then the S4.3 green-thread fork (needs the developer's PHP-target call).**

- **Marathon checkpoint #9 (session 3): SPINE-2 SOUNDNESS COMPLETE.** S2.1 full (narrow `1163e47` +
  methods `3a95755` + broad `d210c62`), S2.2 method return-overloading `9b1864a`, S2.4 while-let guards
  `33f4d0d`, S2.5 LSB closed `3d3faf9`, **S2.3 must-use B/C closed as moot** (subsumed by Slice A's
  universal rule — no opt-in attribute to propagate). Plus Spine-3 breadth `a38ff45`/`b983fb9`. **The
  ONLY remaining marathon work is Spine-4 (M6 W4 concurrency capstone)** — milestone-scale (server
  keep-alive + graceful shutdown → uncolored `spawn` + channels green-threads on the VM's reified frames,
  Tier-3 quarantined OUTSIDE `differential.rs`). Builds on M6 W3's concurrent OS-thread-pool `phg serve`
  (`84ddc32`). **Start fresh** — it's a milestone, not a slice.
- **Marathon checkpoint #8 (session 3 cont., fully autonomous — bypass set): S2.1-broad CLOSED.** The
  reified-operand side-table shipped exactly per the design above: checker records `expr span.start → Ty`
  for concrete `Call`/`Member`/`Index` results (`Checker::reified_operands`, hooked in `check_expr`),
  returned as a 5th element from `check_resolutions`, threaded via new `check_and_expand_reified` +
  `compile_with`/`compile_program_with` (the run-family `compile` path delegates with an empty map →
  byte-identical), and consulted FIRST in the compiler's `ctype` (guarded by `!is_empty()`; `Other`
  entries dropped at the `ty_to_cty` boundary so non-operands never override). Closes `box.get() + 1`,
  `box.value + 1`, `List<T>`/`Map` returns, multi-param returns. **Span-stability verified** by the full
  example glob (the only failure was my own example reading a *private* field — fixed, not a regression).
  `examples/guide/generic-types.phg` + differential `generic_class_member_results_are_vm_operands`;
  KNOWN_ISSUES S2.1 marked CLOSED. **Spine-2 soundness is now effectively complete** (S2.1 full, S2.2,
  S2.4, S2.5; only S2.3 must-use B/C remains). Commit pending gate-green. **Next: S2.3 → Spine-4 capstone.**

- **Marathon checkpoint #7 (session 3 cont.): `Core.Math.lcm`** — pairs with `gcd` (`|a|/gcd*|b|`,
  `lcm(_,0)=0`, EV-7 overflow fault), gated `__phorj_lcm` (inlines Euclid). Byte-identical;
  `examples/guide/math.phg` + unit tests (values + php-mapping) + README. Commit pending gate-green.
  **Also recorded the S2.1-broad-remainder design above (the genuinely heavy reified-result side-table).**
- **Marathon checkpoint #6 (session 3 cont.): two more commits.**
  - **`a38ff45` Spine-3 breadth: `Core.List.lastIndexOf`** — last structural-match index → `int?`,
    symmetric companion to `indexOf` (gated `__phorj_last_index_of` over `array_keys(…, true)`); unique
    leaf, no UFCS clash; byte-identical, `examples/guide/list-breadth.phg` extended.
  - **S2.1-methods (generic-method-param-echo) — the tractable half of S2.1-broad.** A generic *method*
    whose result is exactly one of its own params (`pick<T>(T a, T b) -> T`) now specializes as a VM
    arithmetic operand (`u.pick(7, 8) + 1`), closing a real run↔runvm parity gap (was: VM "cannot infer
    numeric type", interpreter fine). Mirror of the free-fn S2.1-narrow: `erase_generics` computes the
    echo index for class methods (`generic_ret_echo_param`, keyed on the method's own `<T>` so it never
    fires for a class-`T` return), threaded into the compiler as a new `method_generic_ret_from_param`
    map, recovered in the method-call `ctype` arm before the erased `method_rets` fallback. No new
    `Op`/`Value`; `examples/guide/generic-methods.phg` extended (operand line) + differential
    `generic_method_result_echoing_param_is_vm_operand`. **Still deferred (the genuinely heavy remainder,
    needs the reified-result side-table threaded through `compile_program`):** `box.get() + 1` (method
    returns the *class* `T` via a field), generic field reads, `List<T>`-element/container returns,
    multi-param-derived returns. **Commit pending gate-green.**
- **Marathon checkpoint #5 (session 3, fresh context): S2.2 method return-overloading DONE + committed
  `9b1864a`** — full gate green (1259 lib + 115 differential + 16 typecheck, PHP-8.5 oracle), clippy+fmt
  clean, release binary rebuilt. Zero backend changes (the free-fn pipeline was already parameterized).
  P0 caught in Phase-6 sweep: gated to instance methods (`!is_static`) so statics keep the classic
  shared-return rule. **Next in recommended order: S2.1-broad (generic-result VM operand — heavy,
  needs the checker→compiler type side-table) → S2.3 must-use B/C → S2.5 LSB → Spine-4 W4 concurrency
  (capstone); Spine-3 breadth interleaved as low-risk warm-ups. One heavy slice per fresh context.**
- [2026-06-29] S1.4 cross-package generic library types — DONE `718fa3d` (example-only, already worked).
- [2026-06-29] S1.1 cross-package traits — DONE `cc711b9` (loader symbol-table + resolve `Item::Trait`/`uses` rewrite + transpiler namespace bucketing).
- [2026-06-29] S1.2 lambdas/fn-values in library packages — DONE `5d7beb9` (loader `Expr::Ident` value-resolution arm; Main no-op).
- [2026-06-29] S1.3 core.json multi-package + cross-package map literals — DONE `d63cb9d` (JSON helper `\Main\` prefix + loader `Expr::Map` arm).
- [2026-06-29] S1.5 cross-package single inheritance + parent dispatch — DONE `41fa646` (loader `c.extends` resolution + `Expr::ParentCall` arm). **SPINE 1 COMPLETE.**
- [2026-06-29] **Spine 2 DEFERRED to a dedicated session** (recorded autonomously; reorder, not drop). Rationale: every Spine-2 slice is architecturally heavy and each has a clean documented workaround, so rushing one under context pressure risks the byte-identity spine.
  - **S2.1 generic-result VM operand (`id(7)+1`)** — **PARTIAL DONE (narrow)**: shipped the `generic_ret_from_param: Option<usize>` field on `FunctionDecl` (set in `erase_generics` from the pre-erasure signature when the return is *exactly* an own parameter), copied into the compiler's `FnMeta`, consumed in `ctype`'s `Call`/`Ident` arm (recurse into the echoed argument). Closes `identity(7)+1` / `firstOr(xs,-1)*2` byte-identically (`examples/guide/generics.phg`). **Still deferred** (needs the full span-keyed reified-result side-table threaded into `compile_program`): generic *methods*/*fields* (`box.get()+1`), `List<T>`-element/container returns, multi-param-derived returns. The narrow field rides the AST into the compiler — no `compile()` signature change, no span table (avoids the staleness-across-rewrites trap).
  - **S2.2 method return-overloading** — ✅ **DONE (session 3, this context)**: instance methods may now
    return-overload (identical params, distinct returns), resolved by a `<Type>receiver.m(args)` selector
    and mangled per return (`read__ret_int`) before any backend — **zero backend changes, no new `Op`/
    `Value`**. The free-fn pipeline was already fully parameterized: flipped `validate_new_overload`'s
    `allow_return_overload` to `!sig.is_static` for methods (instance-only; statics keep the classic
    shared-return rule via `E-OVERLOAD-RETURN` — they have no selector call-site path); added
    `finalize_method_overloads` (classify `(class,method)` sets, reuse `ret_overload_mangle` +
    `overload_def_renames`); `check_overload_select` gained a `Member`-callee arm →
    `resolve_method_return_overload` (resolve receiver class, substitute class type args, pick member by
    substituted return, record a mangled *method*-call rewrite into the shared `overload_resolutions`);
    `check_method_call` rejects a bare set with `E-OVERLOAD-NO-CONTEXT`; `rename_overload_defs` gained an
    `Item::Class` arm renaming method members. Byte-identical run≡runvm≡**real PHP 8.5**
    (`examples/guide/method-return-overloading.phg`); new tests: differential `agree_out_php` ×2 (incl.
    `this`-receiver + interpolation), typecheck `bare_…needs_selector`/`selector_picks`/
    `selector_unknown`/`static_methods_cannot_return_overload`. Scope: `package Main` instance methods,
    single declaring class, selector-only (no C2 sink yet); deferred (KNOWN_ISSUES): C2 sink for methods,
    return-overload override across an inheritance/interface hierarchy, generic-class bare-param-return
    member. **Commit pending gate-green.**
  - **S2.2 method return-overloading [original design, now implemented above]** — extend C1's `OverloadSelect`/per-return mangle from free fns to methods. **FULL DESIGN (mapped, pick-up-ready):** per-class method overload sets already exist (`checker::classes[cls].methods[name]: Vec<MethodSig>`), so mirror the free-fn machinery: (1) a `finalize_method_overloads` classifying each `(class, method)` with ≥2 sigs / shared params / distinct returns into a method analog of `return_overload_sets`; (2) `check_overload_select` — currently *rejects* a `Member` callee (calls.rs ~1095) — gains a method arm: resolve the receiver's static class (`check_expr(object)` → `Ty::Named(cls,_)`), pick the member by selector/expected return, mangle (`m__ret_int`); (3) a sink path in `check_method_call` (calls.rs:1012) mirroring `try_resolve_sink_overload`; (4) a method-def mangle pass (extend `rename_overload_defs`, overloads.rs:305 — currently skips methods) renaming the `ClassMember::Method`; (5) the call-site rewrite produces a **method** call to the mangled name (`obj.m__ret_int(args)` — a `Call` with a `Member` callee, preserving the receiver) — NOT a free `Call`. **4-backend dispatch:** interpreter + VM key methods on `(class, name)`; both def-rename and call-rewrite to the mangled name keeps dispatch consistent; transpiler emits `$obj->m__ret_int(...)` (the class must define it). **Scope it C1-equivalent: single declaring class, no override of an overload member across the hierarchy** (defer the inheritance/polymorphic-dispatch interaction — a base-typed receiver resolving the mangled name needs every implementer to rename consistently). Irreducibly multi-commit + byte-identity-critical across all 4 backends — **do in a fresh context.**
  - **S2.3 must-use B/C — ✅ CLOSED (session 3) as MOOT / subsumed by Slice A** (no code). Slice A
    (`53fa3af`) shipped the **strictest possible** must-use: *any* non-`void`/`Empty` expression-statement
    whose value is unused is `E-UNUSED-VALUE` (universal, no opt-in). "Bidirectional must-use propagation"
    is a concept from languages with an *opt-in* `#[must_use]` attribute (Rust) that must be threaded
    through wrappers — but Phorj has no such attribute: must-use is determined purely by a value's type,
    applied at every expression-statement by construction, so there is nothing to propagate. The only
    genuinely-stricter direction is unused-**local** / dead-store analysis (a value bound then never read),
    which is a *separate* future lint (`W-UNUSED-LOCAL`), NOT must-use B/C. **S2.3 requires no further work;
    Spine-2 soundness is COMPLETE.**
  - **S2.5 LSB — ✅ CLOSED (session 3) as a documented deliberate non-feature** (no code; the decision was
    already adjudicated in `docs/specs/2026-06-28-statics-research-design.md` §C: defer + reject cleanly).
    LSB (`static::`/`new static()`) introduces a runtime called-class concept + the `self::`/`static::`
    footgun + an `F`-bounded `new static()` type Phorj lacks — against the legible/no-surprises stance.
    **Clean path documented in KNOWN_ISSUES:** inherited + overloaded statics (A+B, already shipped) cover
    the everyday cases; the factory-returns-subclass idiom = override the static factory per subclass
    (explicit > magic). Revisit as its own milestone only on concrete need. **S2.5 requires no further work.**
  - ~~**S2.4 while-let guards**~~ — **DONE** (session 2): `while (var x = opt when g)` — a pure parser desugar mirroring the if-let guard (wrap BODY in `if (g) { BODY } else { break }`, so a false guard exits the loop). No `Stmt::If.guard` field, no backend change; byte-identical run≡runvm≡real PHP. Tractable-first pick paid off. `examples/guide/loops.phg`, KNOWN_ISSUES updated (both if-let + while-let guards now ship).
- [2026-06-29] S3.1 stdlib charter — DONE `3a6d2ea` (`docs/specs/2026-06-29-m4-stdlib-charter.md`, ROADMAP M4 adopted).
- [2026-06-29] S3.2 `Core.List.chunk` — DONE `ddfabc4` (charter-compliant; `List<List<T>>`, `array_chunk`, size<1 faults).
- [2026-06-29] S3.3 `Core.Text.lines` — DONE `8ea0b67` (split on `\n`, `explode` semantics).
- [2026-06-29] S3.4 **`Core.Path`** (new module) — DONE (basename/dirname/extension/stem/join; pure path-string manipulation, Tier 1; PHP `basename`/`dirname`/`pathinfo`; `src/native/path.rs` + `path_tests.rs`, `examples/guide/paths.phg`). Algorithms derived from PHP 8.5 ground truth, oracle-verified byte-identical run≡runvm≡real PHP.
- [2026-06-29] S3.3 **`Core.Text` ergonomic breadth** — DONE (`lastIndexOf` → `int?`/`strrpos`; `removePrefix`/`removeSuffix` → Kotlin-style affix trim, `str_starts_with`/`str_ends_with`+`substr` single-eval arrow-IIFE). Extended `examples/guide/text-ops.phg`; oracle-verified byte-identical.
- [2026-06-29] S3.5 **`Core.List.fill`** — DONE (generic `fill(value, count) -> List<T>`; `array_fill(0, n, value)`; element type inferred at the call site; `count < 0` faults, EV-7). **Named `fill`, not `repeat`** — a generic-subject native (bare `Ty::Param` first param) unifies with *every* receiver under UFCS, so sharing the `repeat` leaf with `Text.repeat` made `x.repeat(n)` `E-UFCS-AMBIGUOUS` (caught by the differential `ufcs.phg`). `fill` is unique-leafed → no clash; resolver semantics untouched (the principled "exclude bare-Param-first from UFCS" alternative was rejected — it would break the intentionally-UFCS-eligible `Convert.toString`/`Reflect.kind`/`className`). Extended `examples/guide/list-breadth.phg`; byte-identical run≡runvm≡real PHP. **LESSON: a new generic-subject native must use a leaf name unique across all UFCS-eligible natives.**
- **Spine 3 has a charter + a new module + breadth natives** (`Core.Path` new; `Text.lastIndexOf`/`removePrefix`/`removeSuffix`; `List.chunk`/`fill`; `Text.lines`/`Text.capitalize`; core.json encode/safe-parse earlier; sprintf — genuine design fork (variadic vs list / `%` vs `{}`), deferred for an explicit design call).
- **Spine 4 (M6 W4 concurrency/server) NOT started** — a large milestone (keep-alive, graceful shutdown, then uncolored `spawn`+channels green threads on the VM's reified frames, Tier-3 quarantined per the charter). Start fresh.
- **Marathon checkpoint (8 commits): Spine 1 complete, Spine 3 charter+2 natives; Spine 2 deferred (architectural), Spine 4 pending.**
- **Marathon checkpoint #4 (session 2 cont.): Spine 2 — S2.4 (`33f4d0d`) + S2.1 narrow (`1163e47`) DONE.** Next, in order: **S2.2 method return-overloading** (full design recorded below — fresh context), then **S2.3 must-use B/C**, then **Spine 4 W4 concurrency** (capstone). Session 2 total: 8 commits (3 stdlib breadth + 2 soundness + 3 checkpoints), all green, pushed through `1163e47`-ish (developer pushes).
- **Marathon checkpoint #3 (session 2 cont.): Spine 2 OPENED — `33f4d0d` S2.4 while-let `when` guards DONE** (tractable-first, pure parser desugar, green). **Remaining Spine-2 items are all heavier/architectural and best done in a fresh context (one per session for quality):** S2.2 method return-overloading (multi-site: overload sets are free-fn/bare-name keyed in `overloads.rs`; `check_overload_select` rejects method-call selectors — needs `(class,method)` keying + a `<Type>obj.m(args)` selector grammar + per-return method mangle + method dispatch/sink resolution), S2.1 generic-result VM operand (checker→compiler type side-table threaded through `compile_program`), S2.3 must-use B/C. Then Spine 4 W4 concurrency (capstone). **Pace: one heavy slice per fresh context — do NOT batch them under accumulated context pressure (byte-identity risk).**
- **Marathon checkpoint #2 (13 commits, all green, NOT pushed): + Spine-3 breadth this session** — `48a8f03` Core.Path (new module, 5 fns), `c59bf51` Core.Text `lastIndexOf`/`removePrefix`/`removeSuffix`, `5954a2f` Core.List.fill. Each byte-identical run≡runvm≡real-PHP-8.5, 1259 lib + workspace green, clippy+fmt clean. **Spine 2 (soundness) + Spine 4 (M6 W4 concurrency) still pending — both deliberately deferred to a fresh context (Spine 4 is milestone-scale; the handoff says start it fresh).**

## Formal Plan

### Spine 1 — Cross-package M-RT lift
Lift the `package Main`-only wall. Loader mangle-pass + transpiler namespacing are the heavy machinery.
- S1.1 Cross-package **traits** (`trait` in a library package + cross-package `use`).
- S1.2 Lambdas / first-class fn-values **inside library packages** (loader rewrites lambda bodies + bare fn-value refs to mangled targets).
- S1.3 **core.json multi-package** (injected `Json` enum emitted namespaced, not flat).
- S1.4 Cross-package **generic library types** (`Box<T>` in a library package).
- S1.5 Cross-package **parent calls** (`parent.m()` across package boundary).

### Spine 2 — Soundness long-tail close
- S2.1 **Generic-result VM operand fix** (`id(7)+1` / `box.get()+1` on the VM) — thread reified generic result types into the compiler `CTy`.
- S2.2 **Method return-type overloading** (extend C1 from free-fns to methods).
- S2.3 **must-use Slice B/C** (bidirectional propagation of must-use).
- S2.4 **Pattern-cluster refinements** (while-let guards, same-binding or-patterns where provable).
- S2.5 **Late-static-binding alternative** ergonomics (or document as permanent non-feature with a clean path).

### Spine 3 — Stdlib charter + breadth (M4 / M-Batteries)
- S3.1 Write **`docs/specs/…-m4-stdlib-charter.md`** (naming, subject-first arg order, optional-vs-fault discipline, determinism tiers, native-vs-`.phg` policy).
- S3.2 **core.json encode + safe parse** breadth (now multi-package, post S1.3).
- S3.3 **sprintf / string-format** + more `Core.Text`.
- S3.4 **path / url** breadth on the determinism seam.

### Spine 4 — Concurrency + server (M6 W4)
- S4.1 Server hardening: HTTP **keep-alive**, **graceful shutdown/join**, per-worker metrics.
- S4.2 Uncolored **`spawn`** + **channels** (green threads on the VM's reified call frames), quarantined behind the determinism seam, tested outside `differential.rs`.
- S4.3 `phg serve` CLI + docs + example.

> Each slice: design-check → TDD → implement → full gate green → example + KNOWN_ISSUES/README → commit.
> Scope/deferrals captured in KNOWN_ISSUES as we go. Adjust slice boundaries as discovery dictates.
