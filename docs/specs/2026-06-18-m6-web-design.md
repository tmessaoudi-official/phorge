# M6 Web Capabilities — Design Spec

> **Status:** DESIGNED — not yet implemented (awaiting developer design-lock).
> Research spine: `docs/plans/2026-06-18-m6-web-capabilities-research.md` (decisions log + raw agent
> findings in `docs/research/m6/raw/`). Converged via a full 30/8 3C gate (8/8 at cycle 11).
> Roadmap home: `ROADMAP.md` **M6 — Concurrency + servers** ("a native HTTP server").

## 1. The dominating constraint — determinism

Phorge's correctness spine is the **byte-identical differential harness** (`run` ≡ `runvm`, every
program; `tests/differential.rs`). A web server is the most anti-deterministic feature possible
(sockets, ports, concurrency, client timing). **The whole design exists to quarantine that
non-determinism so the spine survives** — the same rule that defers URL/network features to M6.

## 2. The portable unit — `handle(Request) -> Response` at the *value* level

The single insight that organizes everything (PSR-7/PSR-15, confirmed against Go `net/http`,
Deno/Bun `serve`): **the handler is portable; the SAPI bridge is not.**

- `handle(Request) -> Response` is a pure function of immutable values. It is the **only** thing that
  is transpiled 1:1 and byte-identity-tested. It runs unchanged on the Phorge VM and (transpiled) on
  PHP.
- Turning raw wire-bytes into a `Request` is **runtime glue**, and it differs per host:
  - **Phorge socket side:** `phorge serve` reads raw HTTP/1.1 bytes and builds a `Request`.
  - **PHP side:** the generated front-controller builds a `Request` from superglobals
    (`$_SERVER`/`$_GET`/`php://input`).
  The two bridges are *not* transpiled into each other — only `handle` is. A **conformance test**
  pins that both bridges produce the same `Request` for a canonical input.

This is why we reject a `handle_raw(string) -> string` shape (parsing-in-the-handler): it would force
PHP to reconstruct raw bytes from superglobals — lossy and un-idiomatic. The value-level handler is
the PSR-15 contract and the only shape that transpiles to *idiomatic* stock PHP.

## 3. Request / Response shape — Shape A (recommended): pure-Phorge classes

Three candidate shapes were evaluated (see `docs/research/m6/raw/phorge-fit.md` §2). **Recommendation:
Shape A** — `Request`/`Response` are ordinary Phorge `class`es, parser/serializer written in Phorge.

```phorge
package main;            // spike: types live in package main (see §8 — E-PKG-TYPE blocks a core.http library today)
import core.console;
import core.text;

class Header { Header(string name, string value) {} }

class Request {
  Request(string method, string path, string body, List<Header> headers) {}
  // header lookup by linear scan — no Map surface syntax until S4; returns S2 optional
  function header(string name) -> string? {
    for (Header h in this.headers) { if (h.name == name) { return h.value; } }
    return null;
  }
}

class Response {
  Response(int status, string body, List<Header> headers) {}
  // immutable copy-on-write, PSR-7 style — fits Phorge's immutable-by-default model
  function withHeader(string name, string value) -> Response { /* return new Response(...) */ }
}

function handle(Request req) -> Response {
  return Response(200, "Hello, {req.path()}", []);
}
```

**Why Shape A:**
- **Needs ZERO new language features** — verified: M1 classes/methods/ctor-promotion (P4b/P4c, in the
  spine), S1 list literals + indexing + ranges, S2 optionals (`string?` + `??`), and `core.text`
  (`split`/`trim`/`contains`/`join`) are *sufficient* to write the parser, the linear header scan, and
  the serializer in pure Phorge.
- **Maximal determinism + showcase:** the entire handler model + parser + serializer are *Phorge code*,
  glob-gated by `tests/differential.rs`, run byte-identically on both backends, and transpile to PHP
  for free. It dogfoods the language.
- **No new `Op`, no new `Value` variant** (the fit analysis confirms `Op::CallNative` is the generic
  stdlib path; classes already produce `Value::Instance`).

**Accepted costs (spike-scoped):** headers are `List<Header>` with O(n) lookup (Map at S4 fixes
ergonomics, not correctness); the types live in `package main` until cross-package types land (§8);
bodies are UTF-8 `string` and examples stay ASCII (the `core.text`↔PHP round-trip constraint).

*Rejected:* **Shape B** (native-backed `core.http` accessors — `http.method(req)`, etc.) works as a
real stdlib module today but makes the parser Rust (not a Phorge showcase) and needs awkward
`Value::Instance` construction from Rust; **Shape C** (hybrid native parser → Phorge class) carries the
same construction awkwardness. Both are viable fallbacks if Shape A's verbosity proves unacceptable.

## 4. Runtime glue — `phorge serve` (Phorge side) and `php -S` (PHP side)

### 4a. `phorge serve <file> [--port N]`
A new CLI command modeled on `vendor`/`build` (`src/main.rs` dispatch block + `src/cli.rs` help arm).
It loads the program via `loader::load` (so a multi-file project's `handle` works), validates it via
the gate, then enters the socket loop in a **new `src/serve.rs`**.

- **It blocks** — its dispatch arm prints a startup line and runs until interrupted; it does **not** go
  through `main.rs`'s `Ok(text) => print!` tail.
- `std::net::TcpListener` only — **safe std, no crate, `#![forbid(unsafe_code)]` intact, HTTP-only/no
  TLS** (TLS needs a crypto crate → breaks zero-dep; reverse-proxy's job, like `php -S`).
- **One new runtime path:** the VM enters `main()` today (`cli.rs:398`). `serve` needs to invoke the
  named `handle` function with a constructed `Request` argument. This is an additive entry path that
  does not touch `main()` dispatch.

### 4b. The PHP side
`phorge transpile app.phg` emits the handler module (functions + classes) unchanged — verified, the
transpiler already handles classes/methods/field reads and native erasure. A **~10-line PHP
front-controller** (documented in the serve README, *not* auto-emitted in the spike) builds the
`Request` from superglobals, calls `handle($req)`, and emits the `Response` via `header()` +
`http_response_code()` + `echo`. `php -S localhost:8000 router.php` is then the PHP-side equivalent of
`phorge serve`.

## 5. Determinism quarantine — the `Transport` seam

```rust
// src/serve.rs  — the DIRTY layer, outside differential.rs
pub trait Transport {
    fn next_request(&mut self) -> std::io::Result<Option<Vec<u8>>>; // None = closed
    fn respond(&mut self, bytes: &[u8]) -> std::io::Result<()>;
}
// Real impl: wraps TcpListener/TcpStream — the ONLY non-deterministic code.
// Test impl: canned Vec<Vec<u8>> requests + captured responses (deterministic).
```

| Layer | Where | In the byte-identity spine? |
|---|---|---|
| `handle(Request)->Response`, parse, serialize | Phorge code (`examples/`) | **Yes** — glob-gated, run≡runvm≡PHP |
| `Transport` real socket loop | `src/serve.rs` | **No** — `tests/serve.rs`, skip-aware |
| `phorge serve` CLI | `src/main.rs` + `src/cli.rs` | tooling, not language |
| PHP front-controller | serve README | round-trip-documented |

Tests:
- **In-spine:** a glob-gated `examples/` program builds a fixture `Request`, runs `handle`, prints the
  serialized `Response` → byte-identical on both backends + real PHP (the `examples/guide/file.phg`
  fixture pattern).
- **Out-of-spine:** one thin `tests/serve.rs` binds an ephemeral port, sends one real request, asserts
  the response; skip-aware if a port can't be bound (the `tests/build.rs:8-24` graceful-skip pattern).
- **Conformance:** one test that the socket bridge and a simulated-superglobal bridge build the *same*
  `Request` from a canonical raw request (guards the §2 dual-bridge divergence risk).

## 6. HTTP wire details (spike)

- **HTTP/1.1**, response carries a mandatory **`Content-Length`** (or the client hangs) computed by the
  serializer; status line uses a **status→reason-phrase** table (`200 OK`, `404 Not Found`, …).
- **`Connection: close`**, one request per socket — no keep-alive, no chunked transfer (Content-Length
  bodies only). Keep-alive/streaming/SSE are deferred (need an async/stream abstraction).
- **Methods:** GET + POST; POST body read from the socket (Content-Length) / `php://input` (PHP).
- **Malformed request bytes** → `parse` returns `Request?` null → `serve` answers `400 Bad Request`.
- **Missing `handle` function** in the loaded program → clean startup error before binding the port.

## 7. Concurrency — single-threaded spike (forced), green threads at M6 proper

**The `Rc`-shared heap (P5a) makes `Value` not `Send`** → an OS-thread pool sharing the program is
impossible without re-architecting to `Arc` or cloning the whole program per thread. Therefore:

- **Spike: single-threaded** blocking accept loop (one request at a time). Correct, simple, honest.
- **Real concurrency = the M6 green-thread runtime** (uncolored `spawn` + channels on the VM's reified
  call frames — cooperative, one OS thread). This is already the roadmap plan and **the
  `handle(Request)->Response` API survives the executor swap unchanged** (Go proved this). The spike's
  single-threaded server is replaced by the green-thread executor without touching the handler
  contract.

This is a *feature of the sequencing*, not a limitation: the spike de-risks the architecture
end-to-end without pulling the green-thread runtime forward.

## 8. Sequencing & dependencies

| Capability | Gated on | When |
|---|---|---|
| Pure `handle(Request)->Response` + parser + serializer (Shape A) | nothing — ships on today's language | **spike now** |
| `phorge serve` single-threaded + `tests/serve.rs` + PHP front-controller README | nothing | **spike now** |
| `core.http` as a real **library package** (not `package main`) | M5 cross-package-types follow-up (E-PKG-TYPE) | post-spike |
| Map-based headers (`req.headers: Map<string,string>`) | M3 **S4** (Map surface syntax) | later |
| Router + middleware DSL (`app.get("/p", handler)`) | M3 **S3** lambdas (Track A — NEXT) | later |
| `bytes` request/response bodies (octets, not UTF-8) | a new `bytes` type (unscheduled) | later |
| Multi-threaded / concurrent serving | M6 green-thread runtime | M6 proper |

## 9. Examples (examples-ship-with-features mandate)

Two-part, mirroring `examples/build/` + `examples/cli/`:
1. **`examples/web/handler.phg`** (or `examples/project/webapp/`) — defines `Request`/`Response`, a
   Phorge `parse_request(string) -> Request?` and `serialize(Response) -> string`, a `handle`, and a
   `main()` that feeds a **committed fixture request** through `handle` and prints the serialized
   response. Auto byte-identity-gated by the glob; ASCII bodies; PHP round-tripped.
2. **`examples/web/README.md`** — the live-server walkthrough: `phorge serve handler.phg`, a `curl`
   against it, and the `phorge transpile handler.phg > router.php && php -S localhost:8000 router.php`
   equivalent (with the ~10-line front-controller). The socket loop can't be a byte-identical example.
3. **`examples/README.md`** index + coverage-matrix row.

## 10. Spike plan (phased — no code until design-lock)

- **P0 — handler model in Phorge** (in-spine, pure): `Request`/`Response`/`Header` classes,
  `parse_request`/`serialize`, a `handle`, `examples/web/handler.phg` + fixture. *Acceptance:* the
  example runs byte-identically on `run`/`runvm` + real PHP; auto-gated by the glob.
- **P1 — `src/serve.rs` + `Transport`**: pure `handle_raw(bytes, program) -> bytes` (parse→call
  `handle`→serialize) over the trait; the VM "call named fn with arg" entry path. *Acceptance:* fixture
  `Transport` unit test + the conformance test, all in `tests/serve.rs`/unit.
- **P2 — `phorge serve` CLI**: dispatch block, `--port`, blocking loop, startup/missing-`handle`/`400`
  handling, help + USAGE + `explain` codes. *Acceptance:* `tests/serve.rs` real ephemeral-port request
  (skip-aware); manual `curl`.
- **P3 — PHP bridge + docs**: front-controller README, `examples/web/README.md`, `examples/README.md`
  row, `FEATURES.md`/`CHANGELOG.md`/`ROADMAP.md` updates. *Acceptance:* documented `php -S` round-trip.

Each phase is a green, self-contained commit (quality gate: `cargo test` + `clippy --all-targets` +
`fmt --check`).

## 11. Open decisions for design-lock

1. **Request/Response shape:** Shape A (pure-Phorge classes, recommended) vs Shape B (native-backed
   `core.http`). Shape A is the recommendation; B is the fallback if verbosity bites.
2. **Spike scope:** pure handler + `phorge serve` + PHP-bridge README (recommended) — or also a minimal
   *static* router now (no lambdas needed for a static route table)?
3. **`bytes`:** confirm UTF-8 text-only bodies for v1 (recommended) vs pulling a `bytes` type forward.
4. **Milestone placement:** ship the spike now (interleaved before Track A/S3) vs after S3 lambdas so
   the router can land in the same arc.
