# Design — Tier-B Live-Concurrency Escape

**Stage 2 of the extended-modules concurrency design.** This document designs the *escape hatch* for
genuinely-live concurrency: real sockets, wall-clock timers, and physically-parallel side-effecting
tasks. It is the deliberate counterpart to the Tier-A gated cooperative scheduler (the JS-event-loop /
Promise.all / Fibers substrate, designed in the sibling Stage-1 / scheduler design). Tier-A is
byte-identity-gated *by construction* (a total ordering rule with no real clock); Tier-B is everything
that **cannot** be so gated, and the job here is to give it a *principled, already-precedented* home
rather than letting non-determinism leak into the spine.

The short version: **Tier-B live concurrency reuses, verbatim, the seam Phorge already shipped for
`Core.Process`/`Core.Env` and for `src/serve.rs`.** There is essentially no new architecture to
invent — the design is "apply the existing two patterns to a new module family and write down exactly
what each backend can and cannot promise." That is why the feasibility is high and the effort is the
*mechanism wiring*, not a research problem.

---

## 0. The two precedents this design is built on (verified in-tree)

Both seams already exist, are tested, and are the entire foundation:

### Precedent 1 — the `pure: false` native quarantine (`Core.Process`/`Core.Env`)

[Verified by reading `src/native/process.rs`, `src/native/mod.rs`, `tests/process.rs`, and
`tests/differential.rs`.]

- A `NativeFn` carries `pure: bool`. `Core.Process.args`, `Core.Env.get`, `Core.Env.all` are the first
  `pure: false` natives — their result depends on the *process* (argv / env), not the program text.
- `tests/differential.rs::uses_impure_native(src)` reads the `pure` flag **off the registry**
  (`registry().filter(|n| !n.pure)`), collects the impure modules, and **skips any program that
  `import`s one** (`differential: SKIP (impure/quarantined)`). The harness is generic: a *new* impure
  module auto-drops from the oracle with **zero harness edits** — that is precisely the seam the `pure`
  marker exists for (Q1 of the process-quarantine spec).
- Quarantined natives are instead exercised in a **dedicated test file** under a *controlled*
  environment (`tests/process.rs` sets argv/env then asserts a fixed golden; it also asserts
  `run ≡ runvm` — the Rust legs always agree because they share the process global; only the PHP leg
  is unreliable, which is why the *oracle* skip, not the `run≡runvm` skip).
- They still **transpile** (`Core.Process.args` → `array_slice($argv ?? [], 1)`, `Core.Env.get` →
  `getenv(...)`), and they ship a **walkthrough** under `examples/process/` — *not* a gated example.

### Precedent 2 — the `Transport` trait + `src/serve.rs` runtime quarantine (M6 W3)

[Verified by reading `src/serve.rs`.]

- The ONE place sockets + wall-clock non-determinism live. `tests/differential.rs` **never imports the
  module**; conformance is `tests/serve.rs` over a **deterministic in-memory `Transport`**.
- `trait Transport { fn recv() -> io::Result<Option<Vec<u8>>>; fn send(&[u8]) -> io::Result<()>; }` —
  `TcpTransport` is the real socket; the test swaps an in-memory transport (the env-update
  HTTP-fixture-seam pattern) so the loop needs no port and stays deterministic.
- Single-threaded **by force**: the `Rc`-shared heap (`Value` is `!Send`/`!Sync`) makes a thread pool
  impossible. The header comment already names the future: *"real concurrency arrives with M6
  green-threads under this unchanged contract."* This design is that future, generalized.

**Conclusion:** Tier-B live concurrency = (Precedent 1's `pure:false` quarantine) **for the native
surface** + (Precedent 2's `Transport`-behind-a-trait + dedicated test file) **for any runtime that
touches the OS**. No third mechanism is required.

---

## 1. Tier verdict: **Tier-B (quarantined), per-feature; with two sub-features REJECTED**

Following the locked CASE-BY-CASE rule, this is not a blanket charter. The live-concurrency *family*
is Tier-B, but admission is per-feature:

| Sub-feature | Verdict | One-line reason |
|---|---|---|
| **Wall-clock timers** (`Time.sleep`, `Timer.after`, `Ticker`) | **Tier-B** | Real clock = non-deterministic; quarantine + fixture with an injected logical clock. |
| **Live sockets** (`Net.connect`, raw TCP client) | **Tier-B** | Real socket arrival order is non-deterministic; `Transport`-style seam, fixture-tested. |
| **Physically-parallel side-effecting tasks** (`Async.spawnLive`) | **Tier-B** | Interleaving of effects is OS-scheduled; pinned to process/thread fan-out with effects observed via a replay log. |
| **`select`/race over live sources** (first-*wall-clock*-ready) | **Tier-B** | Readiness driven by the real clock/socket; non-gated. (Tier-A `select` over logical readiness is the Stage-1 design's, source-order-deterministic.) |
| Shared-mutable-state OS threads / `Mutex`/`atomic` / channels *across OS threads* | **REJECT** | Developer-locked HARD NO. `Value` is `!Send`; non-deterministic; no `php -n` target; `exec_op` has no preemption point. |
| Runtime scheduler introspection (`NumGoroutine`, `GOMAXPROCS`) | **REJECT** | Exposes the scheduler/OS state — inherently non-portable and non-deterministic; nothing to gate. |

**Why not Tier-A for any of these:** Tier-A's whole claim is a *total ordering rule with no real
clock* — the moment readiness, arrival, or interleaving is decided by the wall clock, a socket, or the
OS scheduler, the three legs can diverge (a `Time.sleep(10)` then `Time.sleep(20)` race resolves in
clock order on PHP, and in whatever order the Rust sampler observes — not a language rule). That is the
exact line: **logical/resolved readiness → Tier-A; physical/wall-clock readiness → Tier-B.**

---

## 2. The byte-identity argument (i.e. why these are NOT gated — and what replaces the gate)

There is **no byte-identity argument** for Tier-B; that is the point. The three legs are *not* required
to produce identical stdout, because the inputs (clock, socket arrival, OS interleaving) are not a
function of the program text. Instead, three weaker but *real* guarantees replace the spine:

1. **Auto-exclusion from the oracle (free, already implemented).** Every Tier-B native is `pure:false`.
   `uses_impure_native` reads that flag off the registry, so any program importing `Core.Time`,
   `Core.Net`, or `Async`(-live) is **automatically skipped** by `tests/differential.rs` — no harness
   edit. (Verified: this is exactly how `Core.Process` is skipped today.)

2. **`run ≡ runvm` still holds for the *deterministic core* under an injected clock/transport.** The
   Rust legs share one execution core (`exec_op` via `run_until`), so when the *source* of
   non-determinism is **injected** (a logical clock, an in-memory transport, a replay log), the two
   Rust backends produce identical output. This is the `tests/process.rs` discipline (set the
   environment, assert a fixed golden, *and* assert `run ≡ runvm`) lifted to time/sockets. The injected
   clock is the keystone: **Tier-B logic is deterministic if the live source is supplied as data.**

3. **Behavioral / property guarantees, fixture-tested in a dedicated file** (`tests/concurrency.rs`,
   sibling of `tests/process.rs`/`tests/serve.rs`): submission-order merge is preserved
   (`parallelMap` emits in input order regardless of completion order), a cancelled context stops
   issuing new work, a fork-join's first-submitted error wins. These are asserted against the
   **injected** clock/transport, so they are deterministic *tests of non-deterministic machinery*.

> **The single most important invariant:** the *deterministic skeleton* of a live program (its
> scheduling rule, its merge order, its cancellation semantics) is Tier-A-grade and lives in the
> cooperative scheduler; Tier-B contributes only the **live event source**, injected at the edge. A
> well-written Tier-B program is "Tier-A logic + a thin Tier-B driver." This is the FRP/actor digest's
> recurring conclusion ("actor logic stays deterministic if events are supplied as data") made
> structural.

---

## 3. Module surface and Phorge-syntax API sketch

Three Tier-B leaf modules, each a `src/native/<leaf>.rs` (the no-god-file rule), all `pure:false`.

### 3.1 `Core.Time` — wall-clock + sleep (the simplest, ship first)

```phorge
package Main;
import Core.Console;
import Core.Time;

function main() -> void {
    var start = Time.nowMillis();          // int — ms since epoch (Tier-B: real clock)
    Time.sleep(50);                        // void — block this fiber/process 50ms
    var elapsed = Time.nowMillis() - start;
    Console.println("slept");              // deterministic line
    // NEVER println(elapsed) in a gated example — it's wall-clock-dependent (Tier-B walkthrough only).
}
```

- `Time.nowMillis() -> int`, `Time.nowMicros() -> int`, `Time.sleep(int millis) -> void`.
- `pure:false`. A program importing `Core.Time` is auto-quarantined from the oracle.
- **Determinism risk:** the result is the real clock. Named risk T-1 (clock skew across legs), T-2
  (sleep granularity differs PHP `usleep` vs Rust `thread::sleep`). Handled by *never* asserting a
  timing value in a fixture; assert only the deterministic side effects (`"slept"`).

### 3.2 `Core.Net` — live TCP client (mirrors `serve.rs`'s server seam)

```phorge
package Main;
import Core.Console;
import Core.Net;

function main() -> void {
    var conn = Net.connect("127.0.0.1", 8080)!;   // Net.Conn? — Tier-B, null on failure
    Net.send(conn, b"GET / HTTP/1.0\r\n\r\n");      // void
    var reply = Net.recv(conn, 4096);               // bytes? — null on EOF/error
    Net.close(conn);
    Console.println("done");
}
```

- `Net.connect(string host, int port) -> Conn?`, `Net.send(Conn, bytes) -> void`,
  `Net.recv(Conn, int max) -> bytes?`, `Net.close(Conn) -> void`. (No TLS — the locked HARD wall;
  HTTPS escapes via shelling out to `curl` through `Core.Process`, never a Rust TLS stack.)
- **`Conn` is a new opaque `Value` carrier (see §5).** It must NOT be `Send` (it owns a `TcpStream`),
  which is consistent with the `!Send` heap.
- **Determinism risk N-1** (arrival order / partial reads non-deterministic), **N-2** (connection
  failure depends on the machine). Handled by the **injected transport**: `tests/concurrency.rs` runs
  `Core.Net` against an in-memory loopback (the `serve.rs` in-memory `Transport` pattern, reused), with
  a scripted byte timeline → deterministic test of non-deterministic I/O.

### 3.3 `Async` (live escape) — physically-parallel side-effecting tasks

This is the genuinely-live spawn — distinct from Tier-A `Async.spawn` (cooperative, deterministic).
Named `Async.spawnLive` / `Async.parallelLive` to make the Tier crossing *visible at the call site*.

```phorge
package Main;
import Core.Console;
import Core.Async;

function main() -> void {
    // PHYSICAL fan-out of SIDE-EFFECTING tasks. Effects interleave non-deterministically.
    var handles = Async.parallelLive([
        fn() -> void => { fetchAndLog("a"); },
        fn() -> void => { fetchAndLog("b"); }
    ]);
    Async.joinAll(handles);   // structured: blocks until all complete; first-submitted error wins
    Console.println("all complete");   // deterministic terminal line
}
```

- `Async.parallelLive(List<() -> T>) -> List<Handle>`, `Async.joinAll(List<Handle>) -> List<T>`
  (ordered merge — results in *submission* order, never completion order; this is the one determinism
  we DO keep), `Async.cancel(Handle) -> void`.
- **Critical constraint (the `!Send` wall):** because `Value` is `!Send`, the Rust legs **cannot share
  the `Rc` heap across OS threads**. Two honest options, both Tier-B:
  - **(a) Process fan-out** (exactly where `Core.Process` already sits): each task is run in a child
    `phg` invocation; inputs/outputs cross as serialized bytes (the task body must be effect-only or
    return a serializable value). Transpiles to PHP `proc_open` fan-out.
  - **(b) Sequential-today, physical-later** for the *pure* subset only — but a pure body is Tier-A
    `Async.parallelMap`, not this. So **`parallelLive` is genuinely (a): process fan-out.**
- **Determinism risk A-1** (effect interleaving is OS-scheduled — two tasks both `println` → bytes
  interleave). This is *unavoidable and accepted*: that is the definition of the escape. The merge of
  *return values* is ordered; the interleaving of *effects* is not. Documented loudly.

---

## 4. Exact PHP transpile targets

Every Tier-B native still transpiles (Precedent 1: even `pure:false` natives emit PHP) — it is just not
oracle-gated. All targets are **`php -n` core** (no extensions: PHP 8.1 Fibers, streams, `proc_open`,
`stream_socket_client` are core; mbstring/PHPUnit absent).

| Native | PHP target (`php -n`-safe) |
|---|---|
| `Time.nowMillis()` | `(int)(microtime(true) * 1000)` |
| `Time.sleep(ms)` | `usleep({ms} * 1000)` |
| `Net.connect(h,p)` | `(($c = @stream_socket_client("tcp://{h}:{p}", $e, $s, 5)) === false ? null : $c)` |
| `Net.send(c,b)` | `fwrite({c}, {b})` |
| `Net.recv(c,n)` | `(($r = fread({c}, {n})) === '' ? null : $r)` |
| `Net.close(c)` | `fclose({c})` |
| `Async.parallelLive([fns])` | `proc_open` fan-out: spawn N child PHP procs, return handle array |
| `Async.joinAll(hs)` | loop `proc_close` in submission order, collect outputs in order |

The Fibers-shaped path (ReactPHP/Revolt ergonomics) is the **Tier-A scheduler's** transpile target, not
this document's — Tier-B's live timers/sockets *drive* that scheduler from the edge but the live ops
themselves are the raw `stream_socket_client`/`usleep`/`proc_open` calls above.

---

## 5. New VM Op / Value needed?

- **No new `Op`.** Every Tier-B native is an ordinary `Op::CallNative(idx, argc)` — the existing
  dispatch (interpreter + VM share the registry `eval`). Sockets/timers/process-spawn happen *inside*
  the native body, never as a bytecode instruction. This matches `Core.Process` (also no new Op).
  - The *cooperative-scheduler* (Tier-A, sibling design) likewise expects **no new Op** (await desugars
    to `MakeClosure` + a scheduler native, driven by the existing re-entrant `run_until` /
    `call_closure_value` — verified those exist in `src/vm/closure.rs`). So the whole concurrency
    milestone may need **zero** new Ops.
- **New `Value` carrier(s), opaque, `!Send`:** `Core.Net` needs a `Conn` handle (owns a `TcpStream`),
  and `Async.parallelLive` needs a `Handle` (owns a child-process handle). Options:
  - **Recommended:** a single `Value::Native(Rc<dyn Any>)` opaque-handle variant (or, to avoid a
    `dyn Any` in the `#![forbid(unsafe_code)]` value core, a small closed `enum NativeHandle { Conn,
    Proc }` behind one `Value::Handle(Rc<RefCell<NativeHandle>>)`). It is **never** constructed by a
    gated program (those are quarantined), so it never crosses the byte-identity spine — its `Debug`
    /`type_name`/equality can be coarse (`"<handle>"`), and `Drop` closes the resource.
  - This carrier is `!Send` by holding a `TcpStream`/`Child`, *reinforcing* the locked HARD-NO on OS
    threads — you cannot move a `Conn` to another thread, by type.
- **`HKey` / hashing:** a `Handle` is **not** hashable (cannot be a Map/Set key) — `build_map`/
  `build_set` reject it like a closure. Cheap to enforce.

---

## 6. Quarantine wiring (concrete, per the precedents)

1. **Registry:** add `src/native/time.rs`, `net.rs`, `async_live.rs`; each `*_natives()` returns
   `NativeFn { pure: false, .. }`; pin them after the existing slots in `mod.rs::build()`.
2. **Oracle skip:** *nothing to do* — `uses_impure_native` already reads `pure` off the registry, so
   `import Core.Time` etc. auto-skip. (This is the load-bearing reuse; verified in `tests/differential.rs`.)
3. **Runtime seam for the OS-touching parts:** the socket loop / process fan-out / sleep goes behind a
   trait à la `serve.rs::Transport` — `trait Clock { fn now_millis(&self) -> i64; fn sleep(&mut self,
   ms); }` and the existing `Transport` for sockets — so `tests/concurrency.rs` injects a **logical
   clock** + **in-memory transport** and asserts deterministic goldens (incl. `run ≡ runvm`). The real
   `SystemClock`/`TcpTransport` is only used by `phg run`/built binaries, never by the harness.
4. **Examples:** `examples/concurrency/` is a **walkthrough README + small companion `.phg`** (like
   `examples/process/` and `examples/build/`) — NOT a `examples/**/*.phg` glob-gated example, because
   it cannot produce a fixed golden. The glob harness must never see it; place it so the differential's
   project/example discovery excludes it (the process precedent already does this).
5. **Docs:** `KNOWN_ISSUES.md` entry (Tier-B is non-gated by design), `FEATURES.md` Tier column,
   `ROADMAP.md` milestone line.

---

## 7. Per-backend guarantee matrix (what each leg can/cannot promise)

| Capability | Interpreter (`run`) | VM (`runvm`) | PHP (`php -n`) |
|---|---|---|---|
| `Time.now*` / `sleep` | real clock / `thread::sleep` | same (shared native body) | `microtime`/`usleep` |
| Live TCP client | real `TcpStream` | same | `stream_socket_client` |
| Physical parallel side-effects | **process fan-out only** (heap `!Send`) | same | `proc_open` fan-out |
| Shared-mutable OS threads | **impossible by type** (`!Send`) | impossible | rejected |
| `run ≡ runvm` for the deterministic core under injected clock/transport | ✅ | ✅ | n/a (not gated) |
| Byte-identical to PHP leg | ❌ by design (quarantined) | ❌ by design | ❌ by design |
| Ordered merge of return values (`joinAll`/`parallelLive`) | ✅ guaranteed | ✅ guaranteed | ✅ guaranteed |
| Ordered interleaving of *effects* | ❌ (OS-scheduled, accepted) | ❌ | ❌ |

**The honest promise:** *return-value order* and *the deterministic logic skeleton* are guaranteed on
all three legs; *effect interleaving* and *wall-clock timing* are explicitly NOT, on any leg. The Rust
legs additionally agree with each other (`run ≡ runvm`) whenever the live source is injected.

---

## 8. Named determinism risks (consolidated)

- **T-1 / T-2:** wall-clock skew + sleep granularity across legs. *Mitigation:* never assert timing;
  inject a logical `Clock` in tests.
- **N-1 / N-2:** socket arrival order, partial reads, connection failure are machine-dependent.
  *Mitigation:* in-memory `Transport` with a scripted byte timeline (the `serve.rs` pattern).
- **A-1:** physical-parallel effect interleaving is OS-scheduled. *Mitigation:* accepted (it IS the
  escape); keep return-value merge ordered; observe effects via a per-task captured-output log merged
  in submission order so the *test* is deterministic.
- **H-1 (carrier leak):** a `Conn`/`Handle` is `!Send` by type — a compile error if anyone tries to
  cross a thread, which is the desired wall. *Mitigation:* by construction (hold a `TcpStream`/`Child`).
- **Q-1 (gate leak):** a Tier-B native accidentally marked `pure:true` would enter the oracle and flake
  CI. *Mitigation:* a unit assertion that every `Core.Time`/`Core.Net`/`Async`-live native has
  `pure == false` (cheap registry test), plus the existing `uses_impure_native` is the only gate path.

---

## 9. Effort

**Medium → milestone-slice.** No new Op; one new opaque `Value` carrier; three small `pure:false`
native modules; one `Clock` trait sibling to `Transport`; one new `tests/concurrency.rs`; walkthrough
example + docs. The architecture is *entirely* the two existing precedents applied to new modules —
the risk is in the careful per-leg fixture authoring (the in-memory transport + logical clock), not in
inventing mechanism. Realistic: `Core.Time` is small (a day-scale slice); `Core.Net` is medium (the
in-memory transport reuse pays off); `Async.parallelLive` (process fan-out) is the largest single
piece and the most likely to be deferred or split (process fan-out has its own serialization design).

---

## 10. Honest feasibility

**~85%.** The two precedents are real, tested, and verified in-tree this session; the `pure:false`
auto-quarantine needs *zero* harness work; the `!Send` heap *helps* (it makes the rejected HARD-NO a
type-level impossibility rather than a discipline). The genuine open work — and where the 15% risk
lives — is `Async.parallelLive`: process fan-out needs a value-serialization story (what can cross a
`proc_open` boundary as bytes), and a transpile target that stays `php -n`-safe under fan-out. `Core.Time`
and `Core.Net` are near-certain; the physical-parallelism piece is the one that may slip or shrink.

---

## 11. Open questions for the developer

1. **`Async.parallelLive` scope:** ship it now as process fan-out (`proc_open`), or defer the physical
   piece and ship only `Core.Time` + `Core.Net` first (with `parallelMap` Tier-A covering the *pure*
   parallel case)? Recommendation: defer `parallelLive`; the pure `parallelMap` (Tier-A) covers the
   common "fan out work" need byte-identically, and live fan-out's serialization design wants its own
   slice.
2. **Opaque carrier shape:** a single `Value::Handle(Rc<RefCell<NativeHandle>>)` closed enum
   (`#![forbid(unsafe_code)]`-clean, recommended) vs. `Value::Native(Rc<dyn Any>)`? The closed enum
   keeps the value core auditable and avoids `dyn Any` downcasts.
3. **HTTPS / TLS escape:** confirm the locked stance — HTTPS is *only* via shelling out (`curl` through
   `Core.Process`), never a Rust TLS crate (would break zero-dep). Should `Core.Net` expose a
   convenience `Net.fetchInsecure(url)` that documents the http-only / shell-out boundary, or keep it
   raw-socket-only and leave HTTPS entirely to `Core.Process`?
4. **`Clock` injection surface:** is a `Clock` trait (sibling to `Transport`) the right seam, or fold
   time into the existing transport seam? Recommendation: separate `Clock` — sleep/now is orthogonal to
   sockets and `Core.Time` should be usable without `Core.Net`.
5. **Naming the Tier crossing:** is `spawnLive`/`parallelLive` (Tier visible at the call site) the
   right ergonomic, or a module split (`Core.Async` cooperative vs `Core.AsyncLive` physical) to make
   the gated-vs-quarantined boundary a *module import* the reader can't miss? Recommendation: module
   split — `import Core.AsyncLive` is a louder Tier-B signal than a function-name suffix, and matches the
   `Core.Process` "importing it quarantines you" model exactly.
