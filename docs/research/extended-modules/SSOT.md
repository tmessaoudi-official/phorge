# Extended Modules — Research SSOT

**Stage 3 — Synthesis.** Single source of truth for the *extended-modules* initiative: concurrency,
the Tier-B impurity framework, the network/persistence cluster (HTTP / Cache / DB), and the testing
suite (assertions + `phg test` + seeded Faker + auto-mocker). Synthesised from 11 Stage-2 designs +
their adversarial reviews + 4 Stage-1 prior-art digests (`docs/research/extended-modules/raw/`).

This document is the *companion* to `docs/research/native-modules/SSOT.md` (Hash/Encoding/Csv/Dump/
Validate/Random/Url/Sql/Time/Http/Db/Regex). Where the two overlap (Core.Random, Core.Http, Core.Db,
Core.Time), the native-modules SSOT owns the *module-shape* design and this one owns the *capability*
design (concurrency that drives a clock, a test runner that consumes Random, etc.). Build order in §7
reconciles both.

Evidence grades inline. All `tests/differential.rs` line numbers and `src/` facts were re-verified by
the Stage-2 agents against the live tree this session and cross-checked by the adversarial pass.

---

## 1. Executive summary + the refined model

### 1.1 The byte-identity partition (Tier A vs Tier B), restated

The correctness spine is a **three-leg byte-identity**: interpreter `run` ≡ bytecode VM `runvm` ≡
Phorj→PHP transpiled under real `php -n` 8.5 (`tests/differential.rs`). The earlier framing "X can't
transpile to PHP" is the **wrong lens** — *everything* transpiles. The real axis is what breaks the
three-leg byte-identity:

1. **Non-determinism** — clock, randomness, network arrival, OS scheduling.
2. **Backend asymmetry** — the *Rust* legs can't do what PHP can (no APCu, can't open a Redis socket,
   no `mt_rand` parity).
3. **The one HARD wall — TLS.** Rust std ships no TLS; the zero-dependency invariant forbids a crate.
   The Rust legs **cannot** do HTTPS. Escapes: http-only `std::net::TcpStream`, or shell out to the
   system `curl` via `std::process::Command` (the project already shells `git`/`php`/`rustc`).
   [Verified: `php -n` at the 8.5 floor has `curl_init`/`stream_socket_client`/`fsockopen` compiled-in
   — `curl` is core, so the *PHP* leg does HTTPS; the asymmetry is load-bearing for the Tier-B client.]

- **Tier A (gated).** Deterministic w.r.t. the program text → `pure:true` → byte-identity-gated in
  `differential.rs`, ships a gated `examples/guide/*.phg`. The strong default.
- **Tier B (quarantined, CASE-BY-CASE).** Result depends on ambient state → `pure:false` →
  auto-skipped from the **PHP oracle** by `uses_impure_native`, **still transpiled**, **still
  `run≡runvm` on the two Rust legs** (they share one `eval` body + one process), **fixture-tested** in
  a dedicated `tests/<feature>.rs`, ships an `examples/<feature>/` *walkthrough* (never a gated
  example). The developer locked admission as **per-feature, no blanket charter** (§3).
- **Reject.** Inexpressible (`Value` is `!Send` → no shared-mutable OS threads) or non-deterministic
  even on the Rust legs (preemptive scheduling, random `select` poll, scheduler introspection).

### 1.2 The concurrency stance (locked by the developer)

- **YES, Tier A:** (a) cooperative async/await over a **deterministic single-threaded scheduler**
  (logical-clock ready-queue → PHP 8.1 Fibers, which are core under `php -n`); (b) **pure
  data-parallelism** (`parallelMap`/`forkJoin` over side-effect-free fns, input-order-preserving
  merge — sequential today = byte-identical, physical Rust threads a *later* output-preserving
  optimisation); (c) **reactive/FRP streams** over deterministic finite sources (operator algebra,
  not asynchrony).
- **YES, Tier B (live escape):** genuinely-live concurrency — real sockets/timers, side-effecting
  physical parallelism (process fan-out) — non-gated, fixture-tested behind the `serve.rs` `Transport`
  / `Core.Process` seam.
- **HARD NO:** shared-mutable-state OS threads, `Mutex`/`atomic`/`WaitGroup`-as-thread-sync, preemptive
  scheduling, runtime scheduler introspection. **`Value` is `!Send` (Rc heap) makes this a type-level
  impossibility**, not merely a policy — the adversarial pass confirmed it is *incoherent*, not just
  hard. [Verified across all reviews.]

### 1.3 The single cross-cutting finding the whole initiative turns on

**The two PROJECT differential harnesses have NO `uses_impure_native` guard.** This is the decisive,
repeated refutation — it lands against *every* Tier-B design (async-live, cache, db, http, the
mechanism itself) and against the test-runner's "gated for free" claim.

[Verified by every adversarial agent against `tests/differential.rs`:]
- `all_examples_match_between_backends` (single-file glob, ~line 990–1020) **DOES** call
  `uses_impure_native(&src)` and `continue`s (~line 1004).
- `all_example_projects_match_between_backends` (project `run≡runvm`, ~line 1030) and
  `all_example_projects_transpile_and_match_php` (project PHP oracle, ~line 1938) call **neither** —
  they `loader::load` every `phorj.toml` project and assert `run.is_ok()` + `run==runvm` /
  `php==interpreter` **unconditionally**.
- `collect_phg` returns early on any dir containing a `phorj.toml`, so a project never reaches the
  single-file skip.

The `Core.Process` precedent works **only by accident of file placement** — `examples/process/
args-env.phg` is a *flat single file*, so it rides the guarded glob. Every Tier-B feature in this
initiative naturally wants a **multi-file project walkthrough** (a server, a worker, a cache app, a DB
app), which lands *with* a `phorj.toml` → picked up by the un-guarded project harness → runs the
**real** SystemClock/TcpStream (no injection seam in that path) → `assert_eq!(run, runvm)` **fails
flakily in CI** for any clock/random/network feature.

**Required, non-optional, before ANY Tier-B feature ships a project example:** extend the quarantine
to the project path — call `uses_impure_native` (or, more robustly, check the **post-load merged
program's resolved native set**) inside both project harnesses, scanning **every `.phg` under the root**
(the impure import can live in any package file, not just `main.phg`). The single-`src` substring scan
is also non-transitive and substring-brittle (`import Core.TimeZone` matches needle `import Core.Time`;
double-space / comment defeats it) → the correct gate runs on the resolved AST, not entry-file text.
**Until that harness edit lands, the standing rule is: Tier-B examples are single-file flat only.**

---

## 2. CONCURRENCY design

Substrate (all [Verified] in-tree): `NativeEval::{Pure, HigherOrder, Reflective}`; the **re-entrant**
VM `call_closure_value` + `run_until(target_depth)` (`src/vm/closure.rs`) and the interpreter
`call_closure` drive the *shared* `exec_op`, so a native can invoke a closure arg byte-identically on
both Rust legs; the `serve.rs` `Transport` trait quarantines live I/O.

### 2.1 Pure data-parallelism (`Core.Parallel`) — Tier A, **the strongest case, build first**

`Parallel.map(xs, pureFn)` is `List.map` wearing a permission slip: input-order-preserving, sequential
today, byte-identical by construction. **No new `Op`, no new `Value`** — HigherOrder natives over the
existing `ClosureInvoker`. feas **95%**, conf **high**, `determinism_holds=true`, `feasible_std_only=true`.

Ship **`Parallel.map` + `Parallel.forkJoin`** only. PHP targets: `array_map($f,$xs)` (verified
left-to-right, side-effects in order under `php -n` 8.5) and `array_map(fn($t)=>$t(),$tasks)`.

**Adversarial corrections (must apply):**
- **R1 [decisive] — the glob gates only TWO legs.** `agree()` (`differential.rs:50`) runs `cmd_run` vs
  `cmd_runvm` only — it never transpiles or runs PHP. The PHP oracle is a *separately authored*
  `agree_out_php` test. So "auto byte-identity-gated on all three legs by the glob" is **false** —
  a dedicated PHP-oracle test for `Parallel.map`/`forkJoin` is a **hard enumerated deliverable**, not a
  glob freebie. (This is a general harness property, but the parallelism design uniquely rests its Tier-A
  safety argument on the false premise.)
- **R3 [real, deferred] — `pure:false` ≠ order-independent.** A future seeded `Core.Random` is
  `pure:true` (text-deterministic) yet **order-sensitive** when drawn from shared state inside parallel
  bodies. The future physical backend would diverge. The parallel-safety deny-list must be **"no
  *stateful*-native reach"** (strictly wider than `pure:false`) — reserve it now since Random is planned.
- **R4 — `E-PARALLEL-CAPTURE` is not a free `free_vars` reuse.** `ast::free_vars` returns names only; an
  immutable-captures-only guard needs a name-resolution pass reading each binding's `VarDecl.mutable`.
- **R5 [drop it] — `Parallel.reduce` must NOT ship.** A left-fold has zero parallelism benefit (fold is
  sequential); the name implies a benefit it can't deliver → violates the no-surprises rule.
- Physical threading later: `Value` is `!Send` → fan out **owned/cloned** inputs or process-fork, re-merge
  into submission order; bench-gated ("if it pays"), output-preserving.

### 2.2 Reactive / FRP streams (`Core.Stream`) — Tier A subset (A1), build small

A1 = eager finite operator algebra over a `List`/range source (`of`/`from`/`range`/`map`/`filter`/
`scan`/`take`/`drop`/`zip`/`concat`/`flatMap`/`distinct` + terminals `collect`/`fold`/`forEach`/`count`),
chained via the shipped **UFCS** over a `List` runtime value (zero new `Value`, zero new class). **No new
`Op`.** feas **88%**, conf **high**, `feasible_std_only=true`. PHP targets are core array builtins +
gated `__phorj_*` helpers for `zip`/`scan`/`flatMap`/`takeWhile`/`dropWhile`.

**Adversarial corrections (must apply):** `determinism_holds=false` **as written** because of `distinct`:
- **`distinct()` is NOT byte-identical** (verified PHP 8.5.7): `array_unique($xs, SORT_REGULAR)` uses
  **loose numeric-coercing** comparison (`[1,"1"]→[1]`, `[1.0,1]→one elem`) but Phorj `eq_val` is
  type-strict (`Int(1) != Float(1.0)`). Any heterogeneous-numeric / `int|string` (S4 unions ship)
  `Stream.distinct()` diverges. **Fix:** emit `__phorj_distinct` via a strict `in_array($x,$seen,true)`
  loop (helper-over-builtin), OR add `E-STREAM-DISTINCT-TYPE` limiting `distinct` to one primitive type.
- **`merge` = concat-in-arg-order** (locked, total order). Do **not** also ship `interleave` — scope
  creep, no Rx-fidelity gain.
- **Document the kernel invariant: every operator MUST emit a sequentially-keyed array** (PHP
  `array_filter` leaves sparse keys; current ops re-index via `array_values`/`array_slice`/`array_merge`,
  but a future operator forwarding a sparse array to a key-sensitive builtin would silently diverge).
- A2 (lazy/cold pull streams) and B (live `interval`/`debounce`/`fromSocket`) **defer** — A2 rides the
  scheduler slice, B rides `serve.rs`/`Transport` (`pure:false`, separate module — see §2.4).

### 2.3 Cooperative async/await + deterministic scheduler (`Core.Async`) — Tier A keystone, **but NOT shippable as designed**

The scheduler *ordering rule* is genuinely sound and is the correct design: FIFO ready-queue + logical-
clock min-heap on `(deadline, insertion_seq)`, drain-microtasks-before-timers (JS event-loop), logical
time never reads a wall clock. PHP leg = the same rule over a `SplQueue` + a **custom stable** timer heap
(NOT `SplPriorityQueue` — not insertion-stable) driving PHP 8.1 `Fiber`s. Surface: `spawn`/`yield`/`all`/
`group`(errgroup)/`channel`/`select`(first-in-**source**-order, the deliberate divergence from Go's
random)/`Context`. `async`/`await` = front-end CPS desugaring to the library.

**Verdict: `revised_tier=mixed`, `determinism_holds=false`, `feasible_std_only=false` for the keystone.**
The adversarial pass (high confidence, source-verified) found two decisive holes:

- **R-A [decisive] — the suspension primitive does not exist on the Rust legs.** `run_until`
  (`src/vm/closure.rs:58`) loops `while frames.len() > target_depth` — it runs a closure *to completion*;
  `call_tree_closure` (`src/interpreter/call.rs:225`) walks the body on the native Rust stack. **There is
  no suspend/yield/coroutine primitive anywhere in `src/`** (the design admits this in its own §3.4(A)).
  Layer 1 ships first "zero new syntax" with a mid-body `Async.yield()` in a plain `spawn`-ed closure,
  but the CPS transform that would make `yield` suspend is scoped as Layer-2 sugar — so the keystone
  example is **unimplementable on the Rust legs as written**. The byte-identity claim only covers the
  *suspension-free degenerate subset* (= ordered `List.map` / sequential fork-join — already shipped via
  §2.1). The instant a task contains a real `yield`/`await`/channel-`recv`-on-empty, the design needs
  machinery that is deferred out of the slice or doesn't exist on the tree-walker in safe std Rust.
- **R-B [decisive] — the Tier-B quarantine INVERTS.** `uses_impure_native` is **module-granular**
  (`differential.rs:916`, `n.module`). The design co-locates Tier-B `sleep`/`after`/live-socket in the
  *same* `Core.Async` module as the pure core → one `pure:false` native auto-quarantines **every**
  `import Core.Async` program, including all Tier-A gated examples → the differential stops gating the
  thing the design claims it gates. **Fix:** logical/cooperative core in `Core.Async`; wall-clock/live in
  a **separate module** (`Core.AsyncLive` / `Core.Time` / `Core.Net`) so the substring check fires on the
  *clock* import, not the scheduler import. (This is also §3-R3: determinism is per-call-graph, not
  per-module — the module split is what makes the per-module gate sound.)
- R-C [medium] Fiber (stackful, live re-read) vs CPS (captured-env snapshot) can diverge on shared
  `Rc<RefCell>` state mutated by an interleaved task between two awaits — **unanalysed**. R-D the PHP
  `nextSeq` and Rust `insertion_seq` are two independently-written counters that must increment
  event-for-event in lockstep — needs a cross-leg seq-assignment-order parity test, not "by construction".
  R-F state the **"no iterated `HashMap` in scheduler state"** invariant explicitly.

**What survives as Tier A (ship now):** the suspension-free subset — `parallelMap`/`forkJoin`/`all`-over-
straight-line-tasks (= §2.1). **What must change before the suspending core is Tier A:** pull CPS lowering
into the shipping slice (re-estimate well below 70%) **or** demote the suspending surface to Tier B with
fixtures until a suspension mechanism is proven on all three legs; **split the live module out** of
`Core.Async`; specify+test the Fiber-vs-CPS shared-state read contract.

**New Op/Value (only if the suspending core is pursued):** an optional `Value::Channel(Rc<RefCell<…>>)`
(avoidable by a stdlib class) and, only if CPS proves insufficient, the reserved `Op::Suspend`/`Op::Resume`
pair (the one place the 3-coupled-match Op dance would appear; note it solves *only* the VM leg — the
tree-walker has no frame to snapshot). The suspension-free subset needs **neither**.

### 2.4 Tier-B live-concurrency escape — Tier B (architecture sound, quarantine wiring is the work)

`Core.Time` (`nowMillis`/`sleep` → `microtime`/`usleep`), `Core.Net` (raw `TcpStream` client →
`stream_socket_client`, **no TLS** — HTTPS only via `curl` shell-out), and `Async.parallelLive` (process
fan-out → `proc_open`, deferred). All `pure:false`, all `php -n`-core targets. **No new `Op`.** **One new
opaque `Value` carrier** — a closed `enum NativeHandle { Conn, Proc }` behind `Value::Handle(Rc<RefCell<…>>)`
(NOT `dyn Any`, to keep `#![forbid(unsafe_code)]` clean), `!Send` by holding a `TcpStream`/`Child` —
which *enforces* the OS-thread reject at the type level. feas **85%**, conf **high**, `feasible_std_only=true`.

The honest promise: return-value *order* + the deterministic *logic skeleton* are guaranteed on all three
legs; effect *interleaving* and wall-clock *timing* are explicitly NOT, on any leg; the Rust legs agree
with each other (`run≡runvm`) **only when the live source is injected** (logical clock / in-memory transport).

**Adversarial corrections (must apply):**
- **P0 — the project-harness leak (§1.3).** "Auto-exclusion is free, zero harness edits, verified" is
  **FALSE for projects**; a multi-file live example breaks CI flakily (the project harness runs the real
  clock, no injection seam in `cli::run_program`/`runvm_program`). Mirror the skip into both project
  harnesses (gate on the resolved native set) — **required, not optional.**
- P1 the substring gate is non-transitive + brittle (§1.3). P1 `Value::Handle` touches ~125 `Value::`
  match arms; its coarse `Debug`/`eq_val` is "free" only if the quarantine is airtight (a leaked handle
  hitting `assert_eq!(run,runvm)` either masks a real diff or — if `Debug` includes a pointer — diverges
  per leg; the two refutations compound). P2 `proc_open` ordered-merge + first-error-wins is unverified
  on the PHP leg (pipe-fill deadlock risk; no emitted-PHP sketch) → **defer `parallelLive`** (the §2.1 pure
  `parallelMap` covers the common fan-out need byte-identically).
- **Module split confirmed sound:** `import Core.AsyncLive` / `Core.Time` is a louder Tier-B signal than a
  `spawnLive` suffix and makes the quarantine detector fire on the import.

---

## 3. The Tier-B MECHANISM + per-feature impurity table

### 3.1 The mechanism (a recipe over a shipped seam — verified, already ships via `Core.Process`)

The Tier-B mechanism is **not new machinery**. It is: (1) declare the native `pure:false` (load-bearing —
`uses_impure_native` derives the impure set from the flag, no harness edit *for single-file*; `NativeFn`
has no `Default`, so a missed flag is a compile error, not a silent gate — 120 explicit `pure:`
declarations); (2) parameterise the non-determinism where possible (sort unstable iteration, inject seed/
clock as args) to shrink the Tier-B blast radius; (3) write the `php` mapping (Tier B still transpiles);
(4) for live I/O, quarantine behind a `Transport`-style effect trait (the `serve.rs` model); (5)
fixture-test in `tests/<feature>.rs` (separate crate, may use real I/O + edition-2024 `unsafe` env APIs),
asserting `cmd_run==golden` AND `cmd_run==cmd_runvm`; (6) ship an `examples/<feature>/` walkthrough +
KNOWN_ISSUES line. feas **90%**, conf **high** for the mechanism itself.

**Two proposed generalisations — one survives, one is over-sold:**
- **G2 (`Scheduler` over `run_until`)** — sound *for the suspension-free subset only* (see §2.3 R-A).
- **G1 (`NativeEval::Effectful` carrying an injected `&mut dyn Effects` for clock/rng/log)** — the
  adversarial pass **refuted the "tens of lines, 4th enum arm" estimate** (R2b). `cmd_run`/`cmd_runvm`
  take only `&str`; `run_program`/`runvm_program` take only `&Unit` — **none carry an Effects/seed/
  frozen-clock parameter.** Threading a *test-supplied* frozen handle through `on_deep_stack`, `Vm::new`,
  the interpreter entry, and all four public runners is a **cross-surface signature change**, not a
  localized arm. The disanalogy: `HigherOrder`/`Reflective`/`ClassTables` inject capabilities **derivable
  from the Program** (built inside the existing entry, no caller param); G1 injects capabilities that
  exist **only in the run environment / test harness**. **The `serve.rs` `Transport` model works precisely
  because the impurity lives *outside* the `eval` body** (`serve` takes `&Program`, is never a native) —
  the opposite of G1's inside-eval injection. **Recommendation: prefer the `Transport`/process-global
  precedent (set-once-then-read, like `PROCESS_ARGS`) over G1; if a per-run injected clock/seed is truly
  needed, cost the full plumbing change explicitly — do not treat it as a free enum arm.**

### 3.2 Per-feature impurity table (CASE-BY-CASE)

| Feature | Tier | Rationale (one line) |
|---|---|---|
| **Clock `now()`** | **B** (logical clock = **A**) | Wall-clock = canonical non-determinism; never read `SystemTime` in `eval` (breaks `run≡runvm` across two calls). The scheduler's *logical* clock is Tier A. |
| **Seeded random** | **A run≡runvm / B three-leg** ⚠ | A hand-rolled identical PRNG (never `mt_rand`) is text-deterministic on the Rust legs — but see §3.3: **three-leg PHP parity needs a sub-2^53 / 32-bit PRNG**; a 64-bit PRNG is **not** losslessly reproducible under `php -n` core (no u64, GMP absent). |
| **True random / UUID v4** | **B** | OS entropy. Seeded-v4 / **v5-namespace** (SHA-1, `hash` ext present) = Tier A. |
| **Filesystem reads** (committed fixture) | **A** | Already shipped (`Core.File.read` of an in-repo fixture). |
| **Filesystem writes** | **B** | Mutates the world; read-back depends on prior writes. `file_put_contents`/`mkdir`/`unlink` (core). |
| **Structured logging — formatting** | **A** | Building a record (level+msg+fields → string) is pure. |
| **Structured logging — emission** | **B** | Side effect outside stdout; **must route to a separate sink (stderr/file)**, never stdout (would pollute the oracle). |
| **Cache — request-scoped `Core.Memo`** | **A** | Pure COW `Map`; `getOrCompute` reuses HigherOrder + shared `ClosureInvoker` (one-vs-two invocation structurally impossible). |
| **Cache — persistent `Core.Cache`** | **B** | Cross-process state + TTL clock. APCu **absent** under `php -n` → file fallback. (§4.2) |
| **HTTP response types** | **A** | Pure data over already-gated `Core.Json`/`Core.Html`. (§4.1) |
| **HTTP client** | **B** | Network + the TLS wall. curl shell-out (Rust) / curl ext (PHP). (§4.1) |
| **DB execution** | **B** | Live socket + server state; Rust legs **cannot connect** (no driver). The `Sql` *builder* is a separate Tier-A slice. (§4.3) |
| **Cooperative scheduler (pure tasks)** | **A** | Ordering is a language rule — *if* live natives live in a separate module (§2.3 R-B). |
| **Shared-mutable OS threads / mutex / atomic** | **REJECT** | `Value: !Send`; non-deterministic; no `php -n` target; no preemption point in `exec_op`. |

---

## 4. Network & persistence Tier-B designs

(Full prose: `raw/design-http.md`, `raw/design-cache.md`, `raw/design-db.md` + their `refute-*.md`.)

### 4.1 Full HTTP — `mixed` (Part A pure types = Tier A; Part B client = Tier B)

- **Part A (pure response factories)** — `Core.Http.text/json/html/redirect/ok/notFound` returning **one
  `Response` value** (factories, **not** a subclass hierarchy — keeps the wire folder monomorphic + the PHP
  flat; Symfony subclasses are rejected for Phorj's immutable single-file model). `Http.stream(List<bytes>)`
  is Tier A only for finite deterministic producers (reduces to `Bytes.concat`). **No new `Op`/`Value`**;
  reuses shipped `Core.Json`/`Core.Html`/W1 `Request`/`Response`. feas **~90%**, `determinism_holds=true`.
  *Constraint (hard rule, not footnote):* no non-exactly-representable float in a gated JSON body (inherited
  `Core.Json` Ryū-vs-PHP-14-digit divergence).
- **Part B (HTTP client)** — `Core.Http.Client.get/post/request` returning `Response?`. **TLS wall:** Rust
  legs shell out to system `curl` (HTTPS, zero-crate) with a `TcpStream` plain-`http://` fallback; PHP leg
  → `curl` ext (core under `php -n`, [Verified]). `pure:false`, fixture-tested. **No new `Op`/`Value`**.
  feas **~72%** (refute docked from 78%).
- **Refute caveats (close before ship):** (i) **no path-based backstop** — exclusion is import-driven only;
  a future `examples/web/fetch-demo.phg` doing a live request without the exact impure import line runs in
  the glob → hang/non-determinism. (ii) **the loopback fixture is NOT "deterministic+offline"** — the
  client has **no `Transport` seam** (it goes straight to curl/`TcpStream`), so serve.rs's in-memory-transport
  trick does **not** transfer; Part B needs its **own injectable `HttpTransport` seam** or the fixture carries
  port-bind races + curl-presence dependence. (iii) `index_of_by_leaf` resolves on the bare leaf → a future
  `Core.Grpc.Client`/`Core.Ws.Client` silently collides on `Client` (no duplicate-leaf guard).
  *Refute confirmed sound:* three-segment `Core.Http.Client` resolves today; pure `Core.Http` is NOT
  false-quarantined by the longer `import Core.Http.Client` needle.

### 4.2 Cache — `mixed` (`Core.Memo` = Tier A; `Core.Cache` = Tier B)

Two-module split (the quarantine is per-module-import, so mixed purity in one module would quarantine the
pure half). `Core.Memo` (request-scoped COW `Map` memoise, `getOrCompute` HigherOrder) is Tier A, gated;
`Core.Cache` (PSR-16-shaped persistent get/set/has/delete/getOrCompute + TTL) is Tier B. Rust legs: an
in-process `static CACHE: RwLock<HashMap>` (`open()`) + file backend (`openFile`). PHP: runtime-select
APCu→file (**APCu absent under `php -n`**). **No new `Op`; opaque `int` handle (no new `Value`).** feas
**~78%** (refute docked from 85%).

**Refute caveats (P0):** (i) the project-harness leak (§1.3). (ii) **in-process `static` cross-contaminates
`run→runvm`** — `agree()` runs `cmd_run` then `cmd_runvm` in **one process**; a `static`-backed cache
populated by the interpreter run is warm for the VM run → `Cache.get` hits on `runvm` where it missed on
`run` → guaranteed `run≠runvm` (the single-file glob avoids this only by skipping *before* `cmd_run`). (iii)
**concurrent-test race** — `cargo test` runs tests in parallel; two cache tests sharing one `static` race,
and the zero-dep invariant forbids `serial_test` → use per-test key namespaces / a static reset. (iv) the
APCu-vs-file PHP branch is **ship-untested** unless `tests/cache.rs` runs the transpiled adapter under
`php -n` directly. *Confirmed sound:* `Core.Memo` is genuinely pure; import-alias does not defeat the gate.

### 4.3 DB execution — Tier B (gateability ≈ 0% by construction)

`Core.Db.connect/query/execute/queryRaw/close/transaction` over the planned **Tier-A `Sql` builder** (slice
#8 in native-modules SSOT — ships first; the injection-safe `(sql, params)` pair is the security win on the
gated spine). Execution transpiles to **PDO prepared statements** (core under `php -n`; driver presence
build-specific). **The closed `Value` hosts the connection by NOT hosting it** — `Connection` is a
`Value::Instance` carrying an opaque `int` id into a process-global `RwLock<Vec<Box<dyn DbBackend>>>`;
default `NullDbBackend` **faults cleanly** on the Rust legs; `tests/db.rs` injects a fixture backend
(in-memory canned + opt-in `/stack` docker Postgres via `PHORJ_DB_DSN`). **No new `Op`/`Value`.** feas
**~85% of mechanism** (refute: optimistic — omits the mandatory project-path quarantine fix).

**Refute caveats:** the project-harness leak (§1.3) — a DB walkthrough shipped as a project leaks in and
the `NullDbBackend` clean-fault fails `run.is_ok()`; the "zero harness edits" claim is false. PDO *class*
is core but `pdo_pgsql`/`pdo_mysql` are driver extensions (moot for the spine, matters for the fixture).

---

## 5. Testing suite

(Full prose: `raw/design-test-runner.md`, `raw/design-faker.md`, `raw/design-mocker.md`.)

### 5.1 Assertions + `phg test` runner — Tier A (one false central claim to correct)

`Core.Test` assertion natives (`assertEquals`/`assertTrue`/`assertNull`/`assertThrows`/… , generic via the
S7b-1 generic-native path, equality via single-sourced `eq_val`) + a `phg test` runner that **discovers
`test_*` free fns, sorts by `(package, fn-name)`** (the determinism keystone), runs each in a synthesized
`try/catch` runner `main`, emits a **timing-free / memory-free / sorted** report + exit codes (0/1/2). PHP
target: hand-written `echo`/`if` reporter (**NOT PHPUnit** — a Composer pkg, absent under `php -n`). **No
new `Op`/`Value`.** feas **~90%**, conf **high**, `feasible_std_only=true`.

**Refute corrections (must apply) — `revised_tier=mixed`:**
- **FALSE CENTRAL CLAIM — the runner is NOT "gated for free."** Verified: the example globs call
  `cmd_run(&src)`/`cmd_transpile(&src)` on the **file text** — they never invoke `cli::cmd_test`. A
  test-suite `.phg` under `examples/` runs as an ordinary program; its `test_*` fns are **never called**
  (no driver) unless the file has its own hand-written `main`. The runner's discovery-sort, isolation,
  report formatting, and exit-code logic are **NOT on the byte-identity spine without a NEW harness arm**
  (a `cmd_test`+transpile path). "No harness edit / for free" is refuted — and the **PHP leg never sees the
  runner** without that arm.
- **Q1 is load-bearing, not a side question — and it is unbuilt.** Verified `src/interpreter/mod.rs:28-31`:
  **only a `Throw` is catchable; a `Runtime` fault (panic/index-OOB/the intrinsic `assert`'s `FaultMsg::Assert`)
  passes straight through every `catch`.** So as shipped, `assert()` in a test is **NOT catchable** — the
  runner cannot isolate it. The fix (assertions must `throw` a catchable typed exception, **not** reuse the
  intrinsic `assert`) is a **genuine new mechanism** for the assertion natives, contradicting the design's
  "they do not invent a new faulting mechanism." Resolve Q1 in favor of catchable-throw before claiming
  Tier-A-high-confidence.
- **EXIT-CODE / oracle tension:** a single intentional red test → `exit(1)` → `run_php` asserts
  `out.status.success()` → the oracle **hard-fails** (not just disagrees). So gated example suites **must be
  all-green**; the FAIL-path is README + a Rust `tests/test_runner.rs` integration test only.
- **Composite `assertEquals`** needs a recursive `__phorj_eq` mirroring `eq_val` — verify it exists before
  assuming reuse (Q3). Float: exact-equality only in v1; `assertApproxEquals` (compares a bool) later.

### 5.2 Seeded Faker (`Core.Faker`) — Tier A (the one design the refute UPGRADED)

A seeded fake-data generator over **embedded ASCII corpora** + a **seeded integer-only PRNG**
(`Core.Random`). The byte-identity proof rests on a hand-rolled **63-bit LCG with a shift-add `mul_mod`**
where *every intermediate stays < 2^63* — so Rust `i64` wrapping == PHP signed-int arithmetic with **no
float promotion** (PHP has no u64; `mt_rand` is never emitted). **No new `Op`/`Value`** — `Rng`/`Faker` are
injected Phorj classes (the `inject_json_prelude` pattern) over `Value::Instance`. feas **82% (refute
raises to ~92-95%)**, conf medium, `determinism_holds=true`, `feasible_std_only=true`.

**Refute notes:** the only sub-95% component (i64/PHP-int PRNG parity) was **empirically verified** by the
refuter → realized feas is higher than stated. Two spec gaps to close: (i) **empty-range fault** —
`nextInt(lo,hi)` divides by `(hi-lo)`; `hi==lo` is PHP `DivisionByZeroError` vs Rust `% 0` panic — add a
clean `nextInt requires lo < hi` fault *before* the modulo (byte-identity-safe only because `agree_err`
compares by FaultKind). (ii) **pin a verified <2^62 L'Ecuyer multiplier** (masking a 64-bit constant
doesn't guarantee full period — quality, not byte-identity). Defer `nextFloat` (float-divergence tail) and
use hand-rolled integer `date` math (no `DateTime`/clock).

> **Cross-link to §3.3 (the central PRNG finding):** the tierb-mechanism refute (R4) showed a **64-bit**
> PRNG is **not** three-leg-reproducible under `php -n` core (no lossless u64, GMP absent), degrading
> seeded-random to `run≡runvm`-only = Tier B. The Faker's **shift-add `mul_mod` confined to < 2^63 is
> exactly the mitigation** — it keeps every step in the PHP-representable domain, which is why the Faker
> *is* genuinely Tier A while a naive SplitMix64 `Core.Random` would not be. **The seeded PRNG MUST be
> built to the sub-2^63 / shift-add discipline, not a generic 64-bit generator.**

### 5.3 Auto-mocker (`Core.Test.Mock`) — Tier A (foundational code path is broken as written)

`Mock.of<T>()` synthesizes a `ClassDecl implementing` the interface, injected pre-checker (the
`inject_json_prelude` pattern) — so the mock is *ordinary Phorj code*, byte-identical by construction;
records calls in an **ordered `List<string>`**; canned returns via per-primitive-kind slots; verification
(`timesCalled`/`calledWith`) are pure folds. `Core.Reflect` is read-only (no construct-by-name) so this is
**compile-time codegen, not runtime reflection** — reusing the `InterfaceDecl` (full sigs) + `ClassTables`
sorted-name discipline. **No new `Op`/`Value`.** feas **80%**, conf medium, `feasible_std_only=true`,
`determinism_holds=false` **as written**.

**Refute corrections (must apply):**
- **DECISIVE — the `?? 0` default-return is a self-inflicted byte-identity break.** The generated bodies do
  `this.__intReturns["now"] ?? 0`, but **Map indexing on a missing key FAULTS** ("map key not found",
  `value.rs:179-185`) — it does NOT yield null. So the **first unstubbed call** (the whole basis of the
  loose-mock zero-default) raises a fault on `run`/`runvm`, while transpiled PHP `$m["now"] ?? 0` silently
  returns 0 → structural `run/runvm`-vs-PHP divergence. **Fix:** `Map.has` guard / present-check before index.
- **Float arg stringification is a data divergence (not display).** Call-log keys stringify args via
  `Convert.toString`; a float renders differently on the Rust legs (Rust shortest-round-trip) vs PHP
  (precision-14) → `calledWith`/`timesCalled` fold to **divergent return values the test asserts on**.
- The generated source calls `List.append`/`Map.put` **which do not exist** (only `Text.join` ships) — the
  "zero new mechanism" claim is unverified; `Map.put` for heterogeneous V hits the no-type-variable wall.
- Bool display divergence (`true`/`false` vs `1`/``) for any printed verification bool. The injected
  **class** path is structurally analogous to the proven **enum** prelude but is NOT the same path
  (`Mock.of<I>()` with a generic type arg + zero value args is a new intrinsic shape, parsing unverified).

---

## 6. Where the adversarial pass OVERTURNED a claim

| Topic | Overturned | The refutation (compact) |
|---|---|---|
| **Async scheduler** | `determinism_holds=false`, `feasible_std_only=false` | R-A: **no suspension primitive exists** on the Rust legs in safe std (CPS deferred to Layer 2 / a new Op solves only the VM); only the suspension-free subset is Tier A. R-B: live natives co-located in `Core.Async` **invert** the module-granular quarantine. |
| **Pure parallelism** | `determinism_holds=true` but **design has a load-bearing false claim** | R1: the glob gates **two** legs (`agree()` never runs PHP); a `agree_out_php` test is a hard deliverable. R3: deny-list must be "no *stateful* native", wider than `pure:false`. R5: drop `Parallel.reduce`. |
| **Reactive streams** | `determinism_holds=false` | `distinct()` diverges — PHP `array_unique(SORT_REGULAR)` is loose-numeric vs strict `eq_val`; fix with `__phorj_distinct` strict loop or `E-STREAM-DISTINCT-TYPE`. |
| **Tier-B live concurrency** | `determinism_holds=false` | P0: the **project harness has no `uses_impure_native` guard** → a multi-file live example runs the real clock → `assert_eq!(run,runvm)` **flakily fails CI**. A harness edit is required, not optional. |
| **Tier-B mechanism** | `determinism_holds=false`, `feasible_std_only=false` | R1/R2 (project leak + `run≡runvm` breaks for ambient state across two sequential invocations). R2b: **G1 `Effects` has no plumbing path** — `cmd_run`/`run_program` take no caller param; it's a cross-surface signature change, not a 4th enum arm. R4: **seeded 64-bit PRNG is not three-leg-reproducible under `php -n` core** (no u64). |
| **Cache** | `determinism_holds=false` | P0: project leak + **in-process `static` warms `runvm` after `run` in one process** → `run≠runvm`. (Memo is genuinely Tier A.) |
| **DB** | (Tier B unchanged) | Project leak; the `NullDbBackend` clean-fault fails the un-guarded `run.is_ok()`; "zero harness edits" false. |
| **Test runner** | `revised_tier=mixed` | The runner is **NOT gated for free** (globs run file text, never `cmd_test`); the PHP leg never sees the runner without a new harness arm; **`assert` is currently uncatchable** (Q1 is unbuilt new mechanism). |
| **Auto-mocker** | `determinism_holds=false` | The `?? 0` default **faults on missing Map key** (Rust) vs silently returns 0 (PHP); float arg-keys diverge into asserted return values; `List.append`/`Map.put` don't exist. |
| **HTTP** | (held as `mixed`, but) | No path-based glob backstop; the client has **no `Transport` seam** so the loopback fixture is not "deterministic+offline"; `index_of_by_leaf` future leaf-collision. |
| **Faker** | **UPGRADED** (82% → ~92-95%) | The one design the refute *improved* — PRNG i64/PHP parity empirically verified; only an empty-range fault spec + a vetted multiplier remain. |

**The single recurring root cause across 6 of these:** the **project differential harnesses bypass
`uses_impure_native`** (§1.3). Fixing that one harness gap converts most of the Tier-B "determinism_holds=
false" verdicts back to sound.

---

## 7. Recommended BUILD ORDER (reconciled with the native-modules SSOT)

**Sequencing principle:** ship the cleanest-adversarial-pass Tier-A modules first to establish the pattern;
the **one harness prerequisite (H0) gates every Tier-B project example**; concurrency's suspending core and
the test-runner's catchability are *design-gated* (don't start until the named decision lands).

**Phase 0 — prerequisites (do these first, they unblock everything Tier-B):**
- **H0 [REQUIRED]** — extend `uses_impure_native` to **both project harnesses**, gating on the post-load
  resolved native set (scan every `.phg` under the root). Until done, **Tier-B examples are single-file flat
  only.** (Blocks every Tier-B project walkthrough; converts most Tier-B determinism verdicts back to sound.)
- **PR0** — the seeded **`Core.Random`** kernel built to the **sub-2^63 shift-add `mul_mod`** discipline
  (§5.2/§3.3), with a `mul_mod` Rust-vs-emitted-PHP parity fixture **as a gate on locking** (it is the
  foundation for Faker + seeded UUID + deterministic test fixtures, and the *only* PRNG shape that is
  three-leg-byte-identical under `php -n`).

**Phase 1 — Tier A, byte-identity-gated (build in this order):**
1. **`Core.Parallel`** (`map` + `forkJoin` only) — feas 95%, cleanest pass; establishes the HigherOrder
   data-parallel pattern. **+ an explicit `agree_out_php` test** (R1). *Gated.*
2. **`Core.Stream` A1** — feas 88%; ship with the `__phorj_distinct` strict-loop fix and the
   "sequentially-keyed array" invariant. *Gated.*
3. **`Core.Faker` + the PR0 `Core.Random`** — feas ~92-95%; the upgraded design. Add the empty-range fault
   + a pinned <2^62 multiplier. *Gated.*
4. **`Core.Memo`** (request-scoped cache) — feas 92%; resolve the `getOrCompute` return-shape (injected
   `MemoResult` recommended). *Gated.*
5. **HTTP Part A** (pure response factories over shipped Json/Html) — feas ~90%; factories not subclasses;
   no float in a gated JSON body. *Gated.*
6. **`Core.Test` assertions + `phg test` runner** — **design-gated on Q1** (assertions MUST `throw`
   catchable, not the intrinsic `assert`) and on a **new `cmd_test` harness arm** (the runner is not gated
   for free). All-green example suites only. *Gated (runner output), + `tests/test_runner.rs` for the FAIL path.*
7. **`Core.Test.Mock`** (auto-mocker) — ship **after** the runner; apply the `Map.has`-guard /
   present-check fix (the `?? 0` break), single-source arg stringification, and add the missing
   `List.append` native. *Gated.*

**Phase 2 — Tier B, quarantined / fixture-tested (require H0; build last):**
8. **`Core.Cache`** (persistent) — feas ~78%; per-test key namespaces (no `serial_test`); APCu→file PHP
   adapter tested under `php -n` directly. *Quarantined.*
9. **`Core.Time` (wall clock) + `Core.Net` (raw TCP, no TLS)** — feas 85%; the live escape; `Value::Handle`
   opaque carrier; module-split from any cooperative core. *Quarantined.*
10. **HTTP Part B (client)** — feas ~72%; needs its **own `HttpTransport` seam** + a path-based glob
    backstop; curl shell-out (Rust HTTPS) / curl ext (PHP). *Quarantined.*
11. **`Core.Db` execution** — feas ~85% mechanism; **depends on the Tier-A `Sql` builder (native-modules
    SSOT #8) shipping first**; PDO transpile; `NullDbBackend` + injected fixture backend. *Quarantined.*
12. **`Async.parallelLive`** (process fan-out) — deferred (P2 `proc_open` ordered-merge unverified on PHP).

**Design-gated / not-yet-shippable (do NOT start until the decision lands):**
- **`Core.Async` suspending core** (`yield`/`await`/channel-block) — needs the §2.3 decision: pull CPS into
  the slice (re-estimate ≪70%) **or** demote to Tier B with fixtures. **Module-split the live natives out of
  `Core.Async` regardless.** The *suspension-free subset already ships as #1.*

**Reconciliation with `native-modules/SSOT.md`:** that initiative's Tier-A order (Hash → Encoding → Csv →
Dump → Validate → **Random** → Url → **Sql** → Time) and Tier-B (Http → Db) **interleaves** here: its
**Random** = this PR0 (same sub-2^63 discipline); its **Sql builder** is the prerequisite for this **#11
Db execution**; its **Http (module shape)** and this **#5/#10 (capability)** are the same feature from two
angles. Its **Core.Time pure date parts** are Tier A there; this initiative's **`Core.Time.now()`/sleep**
is the Tier-B wall-clock half (#9). Recommended global thread: ship the native-modules clean Tier-A trio
(Hash/Encoding/Csv) and Random first → then this initiative's Phase 1 → then Sql → then all Tier-B
(this Phase 2 + native-modules Http/Db) behind H0.

---

## 8. Open decisions the developer must make before each build

**Cross-cutting (decide first):**
- **D-H0:** approve the project-harness `uses_impure_native` edit (resolved-native-set gate) — or accept the
  "Tier-B examples single-file only" standing rule as the permanent policy. (Blocks all Tier-B walkthroughs.)
- **D-PRNG:** confirm the seeded PRNG is built to the **sub-2^63 shift-add** discipline (three-leg-safe), not
  a generic 64-bit generator; pin a specific <2^62 L'Ecuyer multiplier.
- **D-G1:** reject `NativeEval::Effectful` in favor of the `Transport`/process-global precedent — or fund the
  full cross-surface plumbing change for a per-run injected clock/seed (cost it explicitly).

**Concurrency:**
- **D-Async-1:** for the suspending core — CPS-into-the-slice (re-estimate) vs Tier-B-with-fixtures vs defer
  the whole suspending surface and ship only the suspension-free subset (#1)?
- **D-Async-2:** confirm the **module split** (`Core.Async` pure / `Core.AsyncLive`+`Core.Time`+`Core.Net`
  impure) and the **`select` source-order** tie-break as a permanent language rule.
- **D-Stream:** `distinct` via `__phorj_distinct` strict loop vs `E-STREAM-DISTINCT-TYPE` single-primitive
  restriction? Ship `merge` only (no `interleave`)?

**Testing:**
- **D-Test-Q1 [must answer first]:** assertions use a **catchable `throw`** (recommended — runner isolates
  assertion failures, a hard `panic` legitimately aborts) vs a privileged catch-all over all faults?
- **D-Test-harness:** approve a new `cmd_test`+transpile harness arm (the runner is NOT gated for free)?
- **D-Mock:** API shape (stringly `c.returns("now",42)` vs checked generated setters); canned-return repr
  (per-primitive slots v1 vs `Map<string,Json>`); interfaces-only v1; loose vs strict default.

**Network / persistence:**
- **D-Http:** factories (recommended) vs subclass hierarchy; curl-subprocess HTTPS (recommended) vs
  http-only `TcpStream`; `Response?` (now) vs `Result<Response,HttpError>` (later); add the missing
  `HttpTransport` seam + path-based glob backstop.
- **D-Cache:** `getOrCompute` return shape (injected `MemoResult`); `open()` default backend (in-process vs
  file); cache value `string`-only v1 vs `Core.Json`; confirm Redis deferral.
- **D-Db:** fixture backend (canned + opt-in docker Postgres via `PHORJ_DB_DSN`); `Row` typing (`string?`
  v1); add `Db.withConnection(dsn, fn)`; confirm `Sql` builder lands first.
```
