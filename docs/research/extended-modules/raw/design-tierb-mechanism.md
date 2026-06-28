# Tier-B Impurity Mechanism + Per-Feature Tier Framework (CASE-BY-CASE)

**Stage 2 — Design.** Author: extended-modules research agent. Date: 2026-06-27.
Scope: design the *general* mechanism for an impure feature (declare `pure:false`, quarantine from the
byte-identity differential, fixture-test outside `differential.rs`, transpile, document per-backend),
then give a **per-feature** Tier-A / Tier-B / reject recommendation. **No blanket charter** — admission
is per-feature, the developer's locked decision.

---

## 0. Verified ground truth (read the code, not the memory)

Confirmed by reading the tree this session — the Tier-B mechanism *already exists and ships* (M-Batteries
kickoff, `Core.Process`/`Core.Env`). The design below is a *generalization of an existing, working seam*,
not a green-field proposal. [Verified by reading the listed files.]

| Element | Where | What it does |
|---|---|---|
| `NativeFn.pure: bool` | `src/native/mod.rs:62` | Per-native determinism flag. `false` = result depends on the *process*, not the program text. |
| `NativeEval::{Pure,HigherOrder,Reflective}` | `src/native/mod.rs:80` | `Copy` enum of `fn` pointers. `Pure(fn(&[Value],&mut String)->Result<Value,String>)`, `HigherOrder(fn(args,&mut ClosureInvoker))`, `Reflective(fn(args,&ClassTables))`. |
| `NativeFn.php: fn(&[String])->String` | `src/native/mod.rs` | Transpile mapping — given already-emitted PHP arg snippets, returns the PHP this native erases to. **Impure natives still transpile.** |
| `uses_impure_native(src)` | `tests/differential.rs:916` | **Derived, not hardcoded:** builds the impure-module set from `registry().filter(!n.pure)`, returns true if `src` contains `import <module>`. A new impure module is auto-quarantined with no harness edit (line 914 comment). |
| SKIP wiring | `tests/differential.rs:1004, 1903` | Both the example-glob and project-aware oracle skip a program that `uses_impure_native`. The Rust legs (`run`≡`runvm`) are *never* skipped — only the PHP oracle leg. |
| Quarantine test file | `tests/process.rs` | Separate test crate (so it escapes `#![forbid(unsafe_code)]` and may call edition-2024 `unsafe std::env::set_var`). Sets the ambient state it expects, asserts `cmd_run`==golden AND `cmd_run`==`cmd_runvm`. |
| Registration self-test | `src/native/process_tests.rs` | Asserts exactly the ambient natives are `pure:false` (a fence: a future native flipped impure by mistake is caught). |
| `ClosureInvoker` / re-entrant VM | `src/native/mod.rs:68`, `src/vm/closure.rs:13,58` | `call_closure_value` + `run_until(target_depth)` drive the *shared* `exec_op` re-entrantly, so a native can invoke a closure arg byte-identically on both Rust legs. This is the substrate the cooperative scheduler reuses. |
| `Transport` trait + quarantine | `src/serve.rs:25,44,180`, `tests/serve.rs` | The live-I/O seam: `serve<T: Transport>` drives the pure `handle(Request)->Response` over an injected transport; `TcpTransport` is the real socket, an in-memory transport is the deterministic test double. **This is the model for every live Tier-B feature.** |
| `Core.Process`/`Core.Env` | `src/native/process.rs` | Live exemplar: `pure:false`, `PROCESS_ARGS: RwLock` process-global, sorted `Env.all()` (OS order is unstable), transpiles to PHP `$argv`/`getenv`, walkthrough under `examples/process/` (NOT a gated example). |

**Key consequence:** the *whole* Tier-B mechanism the developer asked me to "design" is `pure:false` + a
`tests/<feature>.rs` file + a `php` mapping + an `examples/<feature>/` walkthrough README. There is **no
new core machinery to build** for the mechanism itself. The design work is (a) two small generalizations
of that seam, and (b) the per-feature triage. [Verified.]

---

## 1. The Tier model — three admission classes

- **Tier A (gated).** Deterministic w.r.t. the program text. `pure:true`. Runs in all three legs and is
  **byte-identity-gated** in `differential.rs`. Ships with a gated `examples/guide/*.phg`. This is the
  default and the strong preference.
- **Tier B (quarantined).** Result depends on something outside the program text (clock, randomness,
  network, filesystem mutation, process env, real concurrency timing). `pure:false`. **Auto-skipped from
  the PHP oracle** by `uses_impure_native`, **still transpiled** to PHP, **still byte-identical on the two
  Rust legs** (`run`≡`runvm` — they share one process and one `eval` body), and **fixture-tested** in a
  dedicated `tests/<feature>.rs` under a controlled environment. Ships with an `examples/<feature>/`
  *walkthrough* (README + companion `.phg`), never a gated example.
- **Reject.** Cannot be expressed (`Value` is `!Send` → no shared-mutable OS threads), is non-deterministic
  *by construction even on the Rust legs* (preemptive scheduling, `select` random-poll), or has no
  meaningful semantics in a single-threaded interpreter (`Mutex`/`atomic`/`WaitGroup`-as-thread-sync).

**The byte-identity argument, stated once and reused below:**
- *Tier A is gated* because the result is a pure function of the program text — the three legs compute the
  same bytes by construction (Rust kernels single-sourced in `value.rs`; PHP via the `php` mapping over the
  same algorithm).
- *Tier B's Rust legs stay gated against each other* (`run`≡`runvm`) because they share the one `eval` body
  and one process — the impurity is in the *ambient state*, not in a backend asymmetry. Only the **PHP leg**
  is dropped, because the PHP process's clock/random/env/socket need not match the Rust process's.
- *Reject* is anything that breaks `run`≡`runvm` itself (no fixture can restore parity) or that `Value`'s
  `!Send`-ness makes inexpressible.

---

## 2. The general impure-feature mechanism (the recipe)

A new impure feature is admitted as Tier B by following this **six-step recipe** — every step already has a
shipped precedent (`Core.Process`):

1. **Declare the native(s) `pure:false`** in `src/native/<leaf>.rs`. That single flag is load-bearing:
   `uses_impure_native` derives the quarantine set from it, so no harness edit is needed. Add a
   registration self-test (mirror `process_tests.rs`) asserting exactly this module's natives are impure —
   a fence against an accidental flip in either direction.
2. **Provide a deterministic-by-default surface where possible.** Sort unstable iteration
   (`Env.all()` sorts keys); inject the non-deterministic input as a *parameter* (seed, clock value,
   fixture path) rather than reading ambient state, when the feature allows it. The more the surface is
   parameterized, the smaller the Tier-B blast radius — ideally only a thin "live" leaf is `pure:false` and
   the algorithmic core stays Tier A.
3. **Write the `php` mapping** so the feature still transpiles to idiomatic PHP (Tier B ≠ "doesn't
   transpile" — `Core.Process.args()` → `$argv`). The transpiled PHP is exercised by `tests/<feature>.rs`'s
   PHP arm where it is *meaningful to assert* (e.g. a deterministic subset), but it is **never** in the
   byte-identity oracle.
4. **Quarantine the live boundary behind a trait seam** when the impurity is I/O (sockets/timers/process
   spawn), copying `serve.rs`'s `Transport`: the algorithmic logic takes an injected effect handle; the real
   handle is the live one, the test handle is a deterministic in-memory double / replay log. Pure logic
   stays in the library; only the trait's real impl touches the world.
5. **Fixture-test in `tests/<feature>.rs`** (a separate test crate, so it may use edition-2024 `unsafe` env
   APIs and real I/O). Assert `cmd_run`==golden under controlled ambient state, AND `cmd_run`==`cmd_runvm`
   (the Rust-leg parity that survives quarantine).
6. **Document per-backend** with an `examples/<feature>/` walkthrough (README + companion `.phg`, the
   `examples/process/` shape) and a KNOWN_ISSUES line naming the determinism risk and what the PHP leg does
   differently. Faults/non-gated features can't be a gated example — the README *is* the surface.

**Two small generalizations of the existing seam** the framework needs (both ~tens of lines, no new core
machinery):

- **G1 — a `NativeEval::Effectful` variant carrying an injected `&mut dyn Effects` handle.** Today an
  impure native reads a process-global (`PROCESS_ARGS: RwLock`). That works for the *one* ambient case but
  doesn't compose for clock/random/uuid/logging, which want a per-run injected context (so a test can supply
  a frozen clock / seeded RNG / capture buffer). Add `Effectful(fn(&[Value], &mut dyn Effects)->...)`
  alongside `Pure`/`HigherOrder`/`Reflective`, where `Effects` is a backend-supplied trait object (`now()`,
  `rand_u64()`, `log(line)`, …) — the *exact* shape of `ClosureInvoker`/`ClassTables` (backend supplies the
  capability, the one `eval` body uses it, both Rust legs share it). The interpreter and VM each construct
  one `Effects` per run from the same seed/clock config, so `run`≡`runvm` holds; the PHP leg gets PHP's real
  clock/random and is quarantined. *This is the single new substrate, and it is a 4th arm of an existing
  `Copy` enum — no new `Op`, no `Value` change.* [Inferred: by direct analogy to the three existing arms,
  which already follow this exact "backend supplies the capability" pattern — `src/native/mod.rs:80`.]
- **G2 — a thin `Scheduler` over the existing `run_until`.** For the Tier-A cooperative-concurrency surface
  (§4), no new VM machinery: the scheduler is a `VecDeque<Resumable>` plus a logical clock (min-heap on
  `(deadline, insertion_seq)`), and a "resumable" is driven by the already-shipped re-entrant
  `call_closure_value`/`run_until` (`src/vm/closure.rs`). The interpreter mirrors with `call_closure`. The
  ordering is a *language rule* (FIFO ready-queue, ties by insertion order), so it's identical across all
  three legs by construction → **Tier A, gated.** [Inferred: `run_until(target_depth)` already suspends and
  resumes a frame against the shared `exec_op` — exactly a coroutine step.]

---

## 3. Per-feature triage — the eight requested features

Format: **Feature → Tier — one-line rationale.** Detail follows.

| Feature | Tier | One-line rationale |
|---|---|---|
| Persistent / TTL cache | **B** (live) + **A** (in-memory deterministic variant) | Disk/TTL depends on wall-clock + prior runs → B; an in-process `Map`-backed cache with explicit logical-time eviction is pure → A. |
| HTTP client | **B** | Network is non-deterministic *and* TLS is the one hard wall (no std TLS, zero-crate) — http-only `TcpStream` or shell-out, behind a `Transport`-style seam, replay-fixture-tested. |
| DB execution | **B** | Live socket + server-side state + ordering; quarantine behind a `Db` effect trait, fixture/replay-tested; the *query-builder* (string production) is a separate Tier-A feature. |
| Filesystem **writes** | **B** | Mutates the world and reads back prior state → not a function of program text. (Reads of a *committed fixture* — `Core.File.read` — are already Tier A.) |
| `now()` / clock | **B** | Wall-clock is the canonical non-determinism. Inject via G1 `Effects.now()`; frozen-clock fixture test. A *logical* clock (scheduler virtual time) is Tier A. |
| True random | **B** (true) / **A** (seeded) | OS entropy → B. A **seeded** PRNG (`Core.Random.seeded(n)`) is a pure deterministic generator → **A**, the strongly-preferred default (same constraint as seeded Faker). |
| UUID v4 | **B** | v4 needs true randomness (the B half of `Random`). A **seeded/v5-namespace** UUID is deterministic → A. |
| Structured logging emission | **B** | Emitting to stderr/a sink is a side effect outside the output buffer; quarantine the *sink*. **But** capturing log records into an in-memory buffer for assertions is Tier A. |

### 3.1 Persistent / TTL cache — **Tier B (live) + Tier A (in-memory)**
- **Split it.** The *cache algorithm* (key→value with capacity + eviction policy) is a pure data structure
  over `Map` → ship a Tier-A `Core.Cache` whose eviction is driven by an explicit *logical* tick the caller
  advances (or LRU by insertion order — fully deterministic). **Gated, byte-identical.**
- The *persistent/TTL* layer (survives across `phg run`s, expires by wall-clock) is Tier B: it reads the
  clock and prior-run state. `pure:false`; behind a `CacheBackend` effect trait (memory double for tests,
  file/APCu-shaped real impl). **APCu is absent under `php -n`** — the PHP transpile target must be a
  file/SQLite-core/`$_SESSION`-free plain-file cache, not `apcu_*`. Replay-fixture-tested.
- *Byte-identity:* the Tier-A core is gated; the TTL layer's clock breaks the PHP leg → quarantined.
- *Determinism risks:* wall-clock expiry, prior-run residue, OS file mtime. *Effort:* medium (A core small,
  B layer medium). *New Op:* none. *Feasibility:* **85%** (high for the A core).

### 3.2 HTTP client — **Tier B** (the hardest case)
- **The one genuinely hard wall.** Zero-crate + no std TLS ⇒ the Rust legs **cannot do HTTPS** without a
  crate (breaks the dependency invariant) or shelling out. Locked escapes: (a) **http-only** over
  `std::net::TcpStream` (a hand-rolled HTTP/1.1 client — feasible std-only, mirrors `serve.rs`'s
  hand-rolled `ChunkedReader`/parser), or (b) **shell out to `curl`** via the existing `Core.Process` seam
  for HTTPS.
- `pure:false`; behind an `HttpTransport` trait (real socket impl + a **replay-log** test double: a recorded
  request→response fixture, the standard way to make a network feature deterministic in test). The portable
  unit is the **value-level** `HttpRequest`/`HttpResponse` (mirror M6 W1's `Request`/`Response` Shape-A
  classes) — only those round-trip; the socket is glue.
- *PHP transpile target:* `file_get_contents` with a stream context, or `curl_*` (curl ext is commonly
  compiled-in but **not guaranteed under `php -n`** — must verify; fall back to a `fsockopen`/`stream_socket_client`
  hand-rolled client to match the http-only Rust leg). KNOWN_ISSUE: TLS via PHP works but the Rust http-only
  leg can't, so HTTPS examples are walkthrough-only.
- *Byte-identity:* impossible (live remote) → quarantined; replay fixtures give deterministic *tests*, not a
  gated example. *Determinism risks:* remote content, latency, DNS, TLS asymmetry between legs. *Effort:*
  large. *New Op:* none. *Feasibility:* **60%** (http-only client is buildable; HTTPS parity across legs is
  the open risk — likely "curl shell-out for HTTPS, native TcpStream for http").

### 3.3 DB execution — **Tier B**
- Live socket + server-side mutable state + result ordering ⇒ pure quarantine. `pure:false` behind a `Db`
  effect trait; real driver impl (a hand-rolled wire protocol over `TcpStream`, or — pragmatically —
  shelling to the DB CLI through `Core.Process`) + an in-memory / replay test double.
- **Separate the Tier-A half:** a *query builder* (typed SQL string production, parameter binding to a safe
  escaped string) is a pure function → Tier A, gated, the same shape as `Core.Html`'s XSS-safe builder. Ship
  that now; the *execution* is the Tier-B follow-up.
- *PHP transpile target:* PDO (`PDO`/`PDOStatement` — bundled in core PHP, present under `php -n`; SQLite
  PDO driver is core). *Byte-identity:* impossible (server state) → quarantined; the builder half is gated.
  *Determinism risks:* row order without `ORDER BY`, server clock, autoincrement ids, isolation. *Effort:*
  large. *New Op:* none. *Feasibility:* **55%** (builder ~90%; live execution is a milestone of its own).

### 3.4 Filesystem **writes** — **Tier B**
- Writing mutates the world; a subsequent read depends on prior writes, not the program text. `pure:false`.
  (Contrast: `Core.File.read` of a **committed fixture** is already Tier A and gated — determinism comes from
  the fixture being in-repo.) Behind a `Fs` effect trait (a tempdir/in-memory double for tests).
- *PHP transpile target:* `file_put_contents`/`mkdir`/`unlink` (all core). *Byte-identity:* mutation order +
  pre-existing FS state break it → quarantined; tested against a per-test tempdir. *Determinism risks:*
  pre-existing files, mtime, permissions, partial writes, path separators. *Effort:* small–medium. *New Op:*
  none. *Feasibility:* **88%**.

### 3.5 `now()` / clock — **Tier B** (with a Tier-A logical-clock companion)
- Wall-clock is *the* canonical non-determinism. Implement via **G1**: `Core.Time.now() -> Instant` reads
  `Effects.now()`; the interpreter and VM each build their `Effects` from the same run config, so a
  **frozen-clock** test gives identical bytes on both Rust legs; the PHP leg gets PHP's real clock →
  quarantined. *Do not* read `SystemTime::now()` directly in the `eval` body (that would even break
  `run`≡`runvm` across two calls) — always go through the injected handle so a test can freeze it.
- *PHP transpile target:* `time()`/`microtime(true)`/`new DateTimeImmutable()` (core). *Byte-identity:*
  impossible live → quarantined; frozen-clock fixture makes tests deterministic. The **logical** clock the
  scheduler uses (§4) is a *different* feature and is Tier A. *Determinism risks:* wall-clock, timezone, leap
  seconds, monotonic-vs-wall. *Effort:* small. *New Op:* none. *Feasibility:* **90%**.

### 3.6 True random — **Tier B (true)** / **Tier A (seeded)**
- **Default to seeded.** A `Core.Random.seeded(seed) -> Rng` with a pinned deterministic algorithm
  (a documented, fixed PRNG — e.g. a SplitMix64/xorshift implemented identically in the Rust kernel and the
  PHP `php` mapping) is a **pure** generator → **Tier A, gated, byte-identical across all three legs.** This
  is the keystone for seeded Faker and seeded tests (the locked decision).
- **True random** (`Core.Random.system()`) reads OS entropy via `Effects.rand_u64()` → **Tier B**, frozen-
  seed fixture-tested.
- *PHP transpile target (seeded):* `mt_srand($seed)` + `mt_rand()` — **but `mt_rand` is not bit-identical to
  any Rust PRNG**, so the *seeded* algorithm must be a **hand-rolled identical PRNG in both the Rust kernel
  and the emitted PHP** (don't call `mt_rand`; emit the same xorshift arithmetic), exactly the
  single-sourcing discipline of the `value.rs` kernels. True random → `random_int`/`random_bytes` (core).
- *Byte-identity:* seeded = yes (identical algorithm both sides); true = no → quarantined. *Determinism
  risks:* PHP's native RNG ≠ Rust's (forces the hand-rolled identical algorithm); float-from-bits
  representation. *Effort:* medium (the identical-PRNG transpile is the work). *New Op:* none. *Feasibility:*
  **80%** (seeded ~90% with the hand-rolled PRNG; true random trivially B).

### 3.7 UUID v4 — **Tier B** (Tier-A seeded/v5 companion)
- v4 = 122 random bits → inherits the **true-random** half → **Tier B** (uses `Effects.rand_u64()`).
- A **seeded v4** (over the §3.6 seeded PRNG) or a **v5 namespace** UUID (SHA-1 of namespace+name — *hash is
  present under `php -n`*, deterministic) is **Tier A, gated.** Recommend shipping v5/seeded first.
- *PHP transpile target:* hand-rolled from the §3.6 seeded PRNG bytes for seeded; `random_bytes(16)` +
  version/variant bit-set for true v4; `sha1()` for v5 (hash ext present). *Byte-identity:* seeded/v5 = yes;
  true v4 = no → quarantined. *Determinism risks:* RNG asymmetry (same as §3.6), endianness of the byte
  layout. *Effort:* small (rides §3.6). *New Op:* none. *Feasibility:* **82%**.

### 3.8 Structured logging emission — **Tier B (sink)** / **Tier A (capture)**
- Emitting a log line to stderr / a file / a syslog sink is a side effect *outside the program's stdout
  buffer* (the byte-identity oracle compares stdout) → the **emission** is `pure:false`, Tier B, behind a
  `LogSink` effect trait (a `Vec<String>` capture double for tests).
- **The Tier-A half is the valuable one:** building a structured **log record** (level + message + fields →
  a deterministic formatted string) is a pure function → gated. And capturing records into an in-memory
  buffer for assertions is Tier A (the buffer is program-visible state, deterministic).
- Subtlety: if logs are written to **stdout** they'd pollute the oracle's comparison — so the design must
  route logs to a **separate sink** (stderr/file), which is precisely why the *emission* is Tier B even
  though the formatting is Tier A.
- *PHP transpile target:* `fwrite(STDERR, ...)` / `error_log()` / Monolog-shaped (Monolog is a Composer pkg
  → absent under `php -n`; emit plain `error_log`/`fwrite`). *Byte-identity:* formatting = yes; emission =
  no → quarantined. *Determinism risks:* timestamp in the record (use the §3.5 injected clock — frozen in
  tests), sink ordering, interleaving with stdout. *Effort:* small–medium. *New Op:* none. *Feasibility:*
  **85%** (the Tier-A record/formatter is ~90%).

---

## 4. Concurrency — applying the locked decisions + prior-art digest

The developer locked: **all safe paths are Tier A; a Tier-B live escape; shared-mutable OS threads = HARD
NO.** Mapping the prior-art digest onto the verified substrate (`call_closure_value`/`run_until` +
`serve.rs` Transport):

| Concurrency primitive | Tier | Byte-identity argument / why |
|---|---|---|
| **Cooperative async/await** over a single FIFO ready-queue + drain-microtasks (JS event-loop model) | **A** | Ordering is a *language rule* (total, scheduler-free) → identical on all three legs by construction. Lowers via §2-G2 scheduler over `run_until`; PHP leg = a fixed-order closure-loop (or PHP 8.1 Fibers as a later invisible engine). No new Op. |
| **`Promise.all` / `join!` / `try_join!`** (ordered merge — results in *arg* order regardless of settle order) | **A** | Input-order-preserving merge is deterministic. The canonical structured primitive. |
| **`Async.group` / errgroup** (structured fork-join, fail-fast, first-spawn-order error wins) | **A** | Fixed scheduler ⇒ deterministic winner. Best Go primitive to lift; clean PHP target. |
| **`parallelMap` / `Core.Parallel.map`** (pure body, input-order-preserving merge) | **A** | *Essentially free today* — semantically identical to `Core.List.map`, runs sequentially now; physical Rust-thread parallelism is a LATER invisible optimization the ordered-merge contract keeps output-identical. HigherOrder native, reuses `ClosureInvoker`, no new Op. PHP leg = sequential `array_map`. **Blocked from real threads by `Value: !Send`** until task inputs are `Send`-able / process fan-out. |
| **Channels (CSP)** as `Value::Channel(Rc<RefCell<VecDeque>>)`; send/recv = yield points | **A** | Single-threaded ⇒ `RefCell` sound, no `Send`. Yield points feed the deterministic scheduler. PHP = `SplQueue` + Fiber suspend/resume. *(Adds a `Value::Channel` variant — the one possible `Value` extension in the whole framework; could also be a stdlib class to avoid it.)* |
| **`select`** | **A**, with a *deliberate divergence from Go*: when multiple cases ready, pick **first in source order** (not random); `default` fully deterministic | Source-order tie-break makes it a language rule → gated. Random poll order (Go/Rust `select!`) is rejected as gated. |
| **Lazy pull-based `Stream<T>` / generators** (cold observable, single sync consumer) | **A** | A lazy `List` pulled by a terminal op; scheduler implicitly immediate, never exposed. Byte-identical by construction. PHP = real `Generator` (yield is core) or `array_map`/`array_filter` pipeline. No new Op. |
| **ReactiveX operator algebra over a finite list** (immediate scheduler) | **A** | Pure transforms on a `List` with fixed traversal — half-shipped as `Core.List.map/filter/reduce`. HigherOrder natives. |
| **Cooperative deterministic actor runtime** (share-nothing mailboxes, messages by owned value/clone) | **A** | Single-threaded scheduler + per-actor mailbox; no `Rc` aliasing across actors. spawn/send/run as HigherOrder natives over `call_closure_value`. |
| **Supervision trees / let-it-crash** | **A** | Downstream of the actor runtime; restart = match on strategy + counter over the deterministic scheduler; Phorj already has byte-identical faults (M-faults Slice 2). |
| **`context.Context` cancel/done** (structural signal) | **A** | A flag + closed-done-channel; structural, no clock. PHP = a small class holding a flag. |
| **Virtual-time / logical-clock scheduler** (Rx TestScheduler) | **A** | *The keystone substrate.* Min-heap on `(deadline, insertion_seq)`; deadlines are logical ticks, not wall-clock → fully deterministic. |
| **Backpressure / demand protocol** (`request(n)`) | **A** | A non-problem single-threaded: collapses to ordinary lazy evaluation. |
| **`context` deadline / `time.After` / `Sleep` / Ticker / wall-clock timeouts** | **B** | Real-clock-driven resume order → quarantined like `Core.Process`; fixture-tested. (Logical-time `sleep` over the virtual clock is Tier A.) |
| **Live sockets / real-timer-driven loops / side-effecting physical parallelism** | **B** | The locked "genuinely-live concurrency" escape — `serve.rs` `Transport` seam; non-gated, fixture/replay-tested. |
| **`select!` random poll order / Rx hot sources on real clock / Web Workers / distributed actors over sockets** | **B** | Non-deterministic arrival/poll; transport behind a seam, replay-log fixtures. |
| **Shared-mutable-state OS threads; `sync.Mutex`/`RWMutex`/`Cond`/`atomic`/`WaitGroup`-as-thread-sync; `std::sync`; ext-parallel/pthreads/Swoole; BEAM preemptive scheduling; runtime introspection (`NumGoroutine`/`GOMAXPROCS`)** | **reject** | `Value: !Send` ⇒ inexpressible; non-deterministic by construction; no `php -n` target; `exec_op` has no preemption point. Mutexes/atomics are meaningless single-threaded. |

**Concurrency byte-identity backbone:** every Tier-A primitive's *ordering is a total language rule* (FIFO
ready-queue, source-order `select` tie-break, input-order merge, logical-clock min-heap with insertion-seq
ties). Because the order is defined by the language and not by a real scheduler/clock, all three legs
produce it identically — that is the entire reason these are gated rather than quarantined. The Rust legs
need **no new VM Op** (the scheduler rides `run_until`); the only candidate core change is an optional
`Value::Channel` variant, avoidable by modeling a channel as a stdlib class.

---

## 5. Determinism risks (named, framework-wide)

1. **PHP native RNG ≠ Rust RNG** → seeded random/UUID must emit a *hand-rolled identical PRNG*, never
   `mt_rand`. (§3.6, §3.7) [Verified: `mt_rand` is implementation-specific; bit-identity is not contractual.]
2. **`php -n` extension absence** — APCu (cache), Monolog/PHPUnit (Composer pkgs), mbstring all absent;
   present: PCRE, hash, BCMath, Fibers, PDO-SQLite-core. Every Tier-B `php` mapping must target *core-only*.
3. **Float formatting divergence** — irrational/extreme floats differ between Rust and PHP's 14-digit `echo`
   (existing KNOWN_ISSUE); any feature emitting floats (random doubles, time-as-float) keeps to exactly-
   representable values in examples.
4. **OS iteration order** (env, dir listing, channel-of-channels) — sort or insertion-order everywhere
   (`Env.all()` precedent).
5. **Wall-clock vs logical clock conflation** — the scheduler MUST use logical time; only `Core.Time.now()`
   touches the wall clock, and only via the injected `Effects` handle (so two calls in one run can't even
   break `run`≡`runvm`).
6. **`Value: !Send`** — hard ceiling on every "real parallelism" ambition; physical parallelism is only ever
   a later optimization over `Send`-able inputs or process fan-out, and must preserve the ordered-merge
   output contract.
7. **Logs polluting stdout** — structured logging must route to a separate sink, else it corrupts the
   oracle's stdout comparison even for the Tier-A formatting half.
8. **TLS** — the one wall with no std solution; HTTPS parity across legs is unsolved without a crate or a
   `curl` shell-out (the http-only leg simply can't).

---

## 6. New Op / Value summary

- **New VM `Op`: NONE** across the entire framework. Every impure native rides `Op::CallNative`; the
  scheduler rides the already-shipped re-entrant `run_until`/`call_closure_value`.
- **New `Value`: at most one optional `Value::Channel`** (CSP channels), avoidable by a stdlib-class model.
- **One new `NativeEval` arm (`Effectful`)** — a 4th variant of the existing `Copy` enum, the single new
  substrate, by direct analogy to `HigherOrder`/`Reflective` (backend supplies the capability handle).
- Everything else is: a `pure:false` flag, a `php` mapping, a `tests/<feature>.rs`, an `examples/<feature>/`
  walkthrough, and (for live I/O) a `Transport`-shaped effect trait.

---

## 7. Open questions for the developer

1. **`Effects` handle shape (G1).** One fat `Effects` trait (`now`/`rand_u64`/`log`/…) injected into every
   `Effectful` native, or one trait *per capability* (`Clock`, `Rng`, `LogSink`) composed into a context?
   (Per-capability is cleaner but more plumbing; the fat trait mirrors `ClassTables`'s single-struct
   precedent.)
2. **Channels: core `Value::Channel` or stdlib class?** The only place the framework might touch `Value` —
   do we accept one variant for ergonomics/perf, or keep `Value` frozen and model channels as a class?
3. **HTTPS strategy.** Accept "http-only on the native Rust leg, HTTPS only via a `curl` shell-out (and PHP's
   real TLS)"? Or defer all HTTP to a milestone where a vetted single TLS crate is allowed *only* in a
   non-core feature crate (like the playground's wasm-bindgen carve-out)?
4. **Seeded-by-default policy.** Confirm `Core.Random`/UUID default to **seeded** (Tier A), with `system()`/
   v4 as the explicit Tier-B opt-in — matching the seeded-Faker decision.
5. **`select` divergence from Go.** Confirm source-order (not random) tie-break is acceptable as a permanent
   language rule (it's what makes `select` gateable).
6. **Cache scope.** Ship only the Tier-A in-memory `Core.Cache` first and defer persistent/TTL to a later
   Tier-B slice? (Recommended — the A core is 90% feasible and immediately useful.)
7. **Logging sink default.** stderr (simplest, core PHP `fwrite(STDERR)`) vs a file sink — and confirm logs
   never go to stdout (oracle-safety).

---

## 8. Feasibility & confidence

- **The Tier-B *mechanism* itself: ~95% feasible, high confidence** — it already ships (`Core.Process`); the
  framework is a recipe over an existing seam plus two small generalizations (G1 `Effectful` arm, G2
  scheduler-over-`run_until`), neither of which needs a new `Op`.
- **Per-feature feasibility:** clock 90 / fs-write 88 / logging 85 / cache 85 / random 80 / uuid 82 / http
  60 / db 55 — the two network features carry the TLS/live-state risk; everything else is high-confidence.
- **Concurrency Tier-A surface: high confidence, no new Op** — the substrate (`run_until`) is shipped and
  the ordering rules are total/deterministic.

**Overall mechanism feasibility: 90%. Confidence: high** (grounded in read code, not memory).
