# M6 W3 — serve concurrency (design)

> Status: design — **model + CLI defaults pending developer confirmation** (the developer asked for
> spec-first with the model decision brought back before building). Follow-on to the existing M6 serve
> runtime (`src/serve.rs`: `Transport` trait + `serve()` + `TcpTransport` + `serve_tcp`, single-threaded)
> and the W2/W2-ext router. Serve is **runtime glue, outside the byte-identity spine** (the `handle`
> contract is unchanged); tested in `tests/serve.rs`, never in `differential.rs`.

## 1. Goal
Handle multiple in-flight HTTP requests concurrently under `phg serve`, instead of strictly one at a
time. The application contract `handle(Request) -> Response` (and the `respond(bytes) -> bytes` bridge)
is **unchanged** — concurrency is a property of the serve loop, not the program.

## 2. The constraint, and the key finding (VERIFIED)
The heap is `Rc`-based (`Value` is **not `Send`**), so a thread pool that *shares live values* is
impossible — this is what the serve doc's "single-threaded by force" refers to, and why the original
plan was M6 *green-threads*.

**But serve never shares a value across requests.** `respond_once(&program, raw, dev)` builds the
`Value` heap *fresh per request* (in `call_named` → a new interpreter run) and returns `Vec<u8>`; the
heap is created and dropped entirely within one call, on one thread. The only datum shared across
requests is the immutable program. And — verified by a compile-time `assert_send_sync::<ast::Program>()`
probe — **`ast::Program` is `Send + Sync`** (the AST has no `Rc`/`RefCell`). Serve runs the
**interpreter** over `&ast::Program` (not the VM — `BytecodeProgram` embeds `Value` constants and is
*not* Send, but serve doesn't use it).

⇒ **OS-thread-per-request is feasible with no `Value: Send` change:** share `Arc<Program>`, give each
worker thread its own per-request `Rc` heap. Real multi-core parallelism, std-only (`std::thread`), no
`unsafe`, and the single-threaded perf path (the hot `Rc` heap) is untouched.

## 3. Options
| Option | Verdict | Why |
|--------|---------|-----|
| **A. Bounded OS-thread pool, thread-per-request** | **Recommended** | Verified feasible; real multi-core; std-only; no `unsafe`; no `Value` change; ~tens of lines. |
| B. Green-threads (cooperative coroutines) | Rejected | Hard std-only (no async runtime; generators unstable; stack-switching needs `unsafe`), and single-core only — strictly dominated by A. |
| C. Multiprocess prefork | Rejected | `std` has no `fork`; needs a crate or `unsafe`/libc. |
| D. Stay single-threaded | Fallback | Simplest; no concurrency (status quo). |

The original "green-threads" plan was written before noticing that *values never cross threads* in
serve — A removes the need for it.

## 4. Design (Option A)
- **`Arc<Program>`** shared across workers. A fixed pool of **N persistent worker threads** is spawned
  once at `phg serve` start.
- The **main thread** runs the `accept()` loop and hands each accepted `TcpStream` (which *is* `Send`)
  to the workers over a **bounded `std::sync::mpsc::sync_channel`**. The bound gives natural
  backpressure: when all workers are busy and the queue is full, `accept` simply stops pulling new
  connections until a worker frees up — no unbounded thread spawn, no dropped connections.
- Each **worker** loops: receive a `TcpStream`, read the HTTP request (reuse the existing
  request-reader factored out of `TcpTransport`), `respond_once(&program, &raw, dev)` **with its own
  heap**, write the response, close. A panic in one worker is caught (`catch_unwind`) and degrades to a
  500 — one bad request never kills a worker (and the pool is resilient if a worker dies).
- **Per-connection timeout** (`--timeout`, slowloris guard) is preserved per worker.
- **`--workers 1` keeps today's exact path** (the generic `serve()` loop over `TcpTransport`) — zero
  behavior change for the single-threaded case; the pool path is taken only when `workers > 1`.
- The **`Transport` trait + generic `serve()` stay single-threaded** — the in-memory test transport
  doesn't need concurrency; the pool lives in the real-socket `serve_tcp` path only.

## 5. CLI (pending confirmation)
`phg serve <file> [--addr …] [--timeout …] [--workers N]`.
- **Default workers:** `std::thread::available_parallelism()` (fall back to 1 if unavailable).
- `--workers 1` = the current single-threaded server.
- Startup log states the worker count + the bind/timeout (and keeps the "bind 127.0.0.1 on untrusted
  networks" note).

## 6. Tests (outside the byte-identity spine)
`tests/serve.rs` gains: (a) a **concurrency test** — start `serve_tcp` (pool, N>1) on an ephemeral port
in a background thread, fire M>N concurrent clients each hitting a deliberately slow handler, assert all
get `200` and that total wall-time < M×(handler time) (i.e. they genuinely overlapped); (b) a
**backpressure / resilience test** — more concurrent clients than the queue bound all still complete;
(c) `--workers 1` still serves correctly (regression). The existing single-threaded transport tests are
unchanged.

## 7. Invariants
- Serve is runtime glue, **outside** the byte-identity spine; `run ≡ runvm ≡ PHP` is unaffected (serve
  isn't in `differential.rs`).
- `handle(Request) -> Response` / `respond(bytes) -> bytes` contract unchanged.
- **No new `Op`, no new `Value`, no `Value: Send` change** (the single-threaded `Rc` hot path is
  untouched — concurrency is purely in the serve loop).
- std-only; `#![forbid(unsafe_code)]` intact (`std::thread` + `mpsc` + `catch_unwind`, all safe).

## 8. Decisions to confirm (before building)
1. **Model = Option A** (bounded OS-thread pool), revising the documented green-threads plan.
2. **Default `--workers` = number of CPU cores** (`available_parallelism`), `--workers 1` = today.
3. **Saturation = backpressure** (bounded queue; accept pauses when full) rather than reject-with-503.
