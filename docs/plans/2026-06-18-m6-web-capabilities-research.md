# M6 Web Capabilities — Research & Design Plan

> **Status:** RESEARCH (no code yet). Opened 2026-06-18 after M5 closed (`cab81d8`). Developer asked
> to "start thinking/brainstorming/researching adding web capabilities" — launch a web server (à la
> `php -S` / rustup), accept data, request/response — and to **challenge everything before proposing**.
> Roadmap context: `ROADMAP.md:68` already plans *"a native HTTP server"* in **M6 — Concurrency +
> servers**; `VISION.md` names it. This doc is the research spine for that work.

## Decisions Log

- [2026-06-18] AGREED: pursue **Option 1 — research + prototype spike now** (design the handler model
  + a throwaway std-only blocking `phg serve` prototype to de-risk the architecture end-to-end),
  but **defer the polished version until M3 ergonomics land**. No language-feature commitment yet.
- [2026-06-18] AGREED: this requires **deep, real research + brainstorming covering BOTH (a) the M3
  ergonomics prerequisites and (b) the web design** — the developer wants a "perfect", "solid,
  complete, extensible, scalable, parametrized" design, not a quick demo. Target API shape is
  **deliberately UNDECIDED** pending that research (developer rejected picking handler-vs-router-vs-
  socket now — "I need more research and brainstorming").
- [2026-06-18] RECOMMENDATION (not yet locked) on the test boundary: **layered quarantine + a socket
  seam** — pure handler + HTTP parsing + routing are differential/byte-identity-tested; a `Transport`
  trait seam at the socket gives deterministic fixture-driven unit tests for the parser/router; one
  thin real-socket integration test (`tests/serve.rs`, outside `differential.rs`) validates the seam.
  Combines the env-update HTTP-fixture-seam pattern with the existing `tests/build.rs` quarantine
  precedent. Developer asked for pros/cons + a recommendation; this is it, to be confirmed in research.
- [2026-06-18] AGREED (execution): run the research via **parallel research subagents** (~4
  concurrent, ≤5 rate-limit ceiling) — each writes raw findings to `docs/research/m6/raw/<topic>.md`
  (compaction-safe), then synthesize into the design spec. Inline-sequential and prereqs-first were
  the alternatives; fan-out chosen for exhaustive coverage speed.
- [2026-06-18] AGREED (gate): the design-lock 3C convergence gate runs at **full 30/8** (30-cycle cap,
  8 consecutive clean cycles, 3 angles) before the spec is written — matches the "perfect design" bar.
- [2026-06-18] CONVERGED (research → design, 3C 8/8 at cycle 11). Locked design findings:
  - The **portable unit is `handle(Request) -> Response` at the VALUE level**, not raw bytes (PSR-7/15
    insight: handler portable, SAPI bridge not). Parsing wire-bytes→Request is *runtime glue* (Phorge
    socket-side vs PHP superglobal-side) — NOT transpiled 1:1; only `handle` round-trips.
  - **Request/Response shape (recommended): Shape A — pure-Phorge classes** (in `package main` for the
    spike; a `core.http` *library* awaits the M5 cross-package-types follow-up, E-PKG-TYPE). Headers as
    `List<Header>` + `header(k):string?` linear scan (no Map surface until S4); composes with S2 `??`.
    Needs ZERO new language features — M1 classes + S1 list/range + S2 optionals + `core.text`.
  - **Concurrency: single-threaded spike — forced by the `Rc`-shared heap (P5a, not `Send`).** OS-thread
    pools are off the table; real concurrency is the M6 green-thread runtime on the VM's reified frames.
  - **Quarantine:** socket accept-loop in a new `src/serve.rs` behind a `Transport` trait, tested by a
    skip-aware `tests/serve.rs` OUTSIDE `differential.rs` (build.rs/vendor.rs precedent). `handle` +
    parse + serialize are deterministic → in the spine via a glob-gated `examples/` program.
  - **One new runtime path:** invoke a named fn (`handle`) with a constructed arg from Rust (VM enters
    `main()` only today). Additive — does not touch `main()` dispatch.
  - **Wire (spike):** HTTP/1.1, mandatory `Content-Length`, status→reason-phrase, `Connection: close`,
    one request/socket, Content-Length bodies only (no keep-alive/chunked), malformed→`400`, ASCII
    bodies (PHP round-trip). `phg serve <file> [--port]` blocks (own dispatch, not the `print!` tail).
  - **Spike scope:** pure handler + `phg serve` + a documented ~10-line PHP front-controller in the
    serve README. Router/middleware deferred to S3 lambdas; Map ergonomics to S4; `core.http` stdlib to
    the cross-package-types follow-up; `bytes` type deferred (UTF-8 text bodies v1).
- [2026-06-18] DESIGN-LOCKED (developer answered the §11 open decisions; spec
  `docs/specs/2026-06-18-m6-web-design.md` updated): (1) **Shape A** is the one public API — "do both?"
  resolved to one-API / evolving-engine (Shape B's native map is a later invisible optimization, §3a);
  (2) scope = **pure handler + static exact-match router** (W1–W2); path params→S4, middleware→S3 are
  "the rest"; (3) **`bytes` pulled forward as its own first slice W0** (developer choice) — PHP transpile
  trivial, design is Phorge-side literal + UTF-8 interop; (4) **spike now, before Track A**. Build order:
  W0 bytes → W1 handler → W2 router → W3 `src/serve.rs`+Transport → W4 `phg serve` CLI + PHP bridge +
  docs. No code until the build-gate "go".

## The dominating constraint — determinism

Phorge's correctness spine is the **byte-identical differential harness** (`run` ≡ `runvm`, every
program). A web server is the most anti-deterministic feature possible (sockets, ports, concurrency,
client timing). This is the *same reason* URL/network was deferred to M6 (CLAUDE.md: "network is
non-deterministic → breaks the byte-identical spine; determinism, not the dependency, gates examples").
**The design must quarantine the non-determinism so the spine survives.** std has `std::net::TcpListener`
— the socket itself is easy + zero-dep; the hard part is the boundary, not the I/O.

## The 3-layer decomposition (the core architectural insight)

| Layer | What | Deterministic? | Home | Tested by |
|---|---|---|---|---|
| **1. Handler model** | `Request`/`Response` value types + `fn(Request) -> Response` contract | **Yes** | language/stdlib (`core.http`) | byte-identical differential (golden Request→Response), run≡runvm≡PHP |
| **2. Server runtime** | bind socket, accept loop, route, dispatch | **No** | CLI/tooling (`phg serve`) — *not* a language feature | integration (`tests/serve.rs`, outside the spine) |
| **3. Transpile target** | Phorge web app → idiomatic PHP | n/a | transpiler | real-PHP round-trip |

Layer 1 is pure + testable; Layer 2 is the dirty I/O shell that never touches `differential.rs` —
exactly how `phg build` (tested in `tests/build.rs`, outside the spine) coexists with the pure core
today. **Precedent exists.** PHP mapping is natural: a pure `Request → Response` handler IS PHP's
request-per-invocation model (superglobals + echo); `php -S` is Layer 2 wrapping Layer 1.

## Challenges already raised (to carry into research)

1. **Build the handler model FIRST, not the socket.** The accept loop is ~80 lines of `std::net` you'll
   rewrite when M6 green-threads land; the durable value is the `Request`/`Response` types + contract.
2. **HTTP only, no TLS in core.** TLS needs a crypto crate → breaks zero-dep + `forbid(unsafe)`. `php -S`
   is HTTP-only too; production runs behind a reverse proxy. HTTPS = deferred / proxy's job.
3. **It may be premature.** A real handler wants `Map`/`Set` (headers, query, params), **mutation**
   (build a response), and **exceptions** (error handling) — all **M3, unbuilt**. Building on today's
   immutable, Map-less, exception-less language yields a crippled handler API. → Option 1 (spike now,
   polish post-M3) is the chosen reconciliation.

## Research agenda (NEXT SESSION — be exhaustive, this is the "perfect design" mandate)

### A. M3 ergonomics prerequisites (what web needs from the language)
- **`Map`/`Set`/tuples** — headers, query params, form data, route params. Type-system impact (no
  generics-over-2-params today? `Map<K,V>` shape, literal syntax, iteration, PHP-assoc-array mapping).
- **Mutation** — incremental response building (`resp.header(...)`, status, body). Triggers the M3
  tracing GC (Rc cycles). How much mutation does the handler API actually need vs. a builder/immutable
  fluent style? Can we keep handlers pure-functional and dodge mutation initially?
- **Exceptions (try/catch/throw)** — error handling, 4xx/5xx mapping, middleware error flow. Or a
  `Result`-style total alternative (fits the immutable ethos better; cf. existing `T?`/`opt!` null work)?
- **Lambdas/closures (Track A)** — needed for a router/middleware DSL (`app.get("/p", handler)`); not
  needed for a single top-level `handler(Request)->Response`. Decides feasibility of the router shape.
- **String/bytes** — request bodies are bytes, not just UTF-8 strings. Does Phorge need a `bytes` type?
  Current `string` is UTF-8; HTTP bodies/headers are octets. Real gap to research.

### B. Web design space (research each, pros/cons, PHP-target fidelity, determinism, extensibility)
- **API shape:** (1) pure `handler(Request)->Response`; (2) Express/Sinatra router + middleware;
  (3) low-level `core.net` socket primitives. Likely **layered**: (3) under (1) under (2), shipped in
  that dependency order. Research which is the *public* default.
- **`Request`/`Response` model:** fields, immutability, header map, query/body parsing, status codes,
  content types, streaming vs buffered. Extensible/parametrized: how do users add middleware, custom
  parsers, typed bodies (JSON via the deferred `core.json`)?
- **Routing:** static vs param routes (`/user/{id}`), method dispatch, precedence. Needs Map + (for the
  DSL) lambdas.
- **`phg serve` runtime:** blocking thread-per-request (std::thread, ships now) vs the M6 green-thread
  runtime (uncolored `spawn` + channels on the VM's reified frames). Decouple: simple blocking server
  for the spike, couple to green threads at M6. HTTP/1.1 parsing (keep-alive, chunked, content-length)
  std-only. Graceful shutdown, ephemeral-port binding for tests.
- **Transpile contract (Phorge:PHP::TS:JS):** handler → PHP superglobals + echo? `phg serve` →
  `php -S` dev server? Production → FPM? What does `phg transpile` emit for a web app, and does it run
  under stock PHP? (No Swoole/ReactPHP — those aren't core PHP.)
- **Determinism quarantine** (test boundary): confirm the layered recommendation above; define exactly
  where the pure/dirty line is drawn and the `Transport` seam's interface.
- **Examples-ship-with-features rule:** what's the runnable, byte-identity-gated web example? A pure
  handler example is differential-gateable; the live server is a README walkthrough + a companion
  integration test (cf. `examples/build/`, `examples/cli/`).
- **Extensibility/scalability/parametrization** (the developer's explicit asks): pluggable
  router/middleware/parser traits; config (port/host/workers) parametrized; how it scales (worker model)
  without breaking zero-dep + determinism.

### C. Prior-art research (challenge against real designs)
- PHP: `php -S` dev server, FPM, PSR-7 (Request/Response interfaces), PSR-15 (middleware), Slim, Laravel.
- Rust std-only HTTP (the canonical `std::net` TCP server from the Book — proves zero-dep feasibility).
- Go `net/http` (handler interface = the gold standard for a simple, composable, std-lib server).
- Deno/Bun `serve(req => resp)` (the modern pure-handler shape).

## Open questions (decide in research, via AskUserQuestion)
- Public API shape (handler vs router vs sockets) — deferred by developer pending research.
- Does web wait for full M3, or do we spike on today's language and rework? (Option 1 = spike now.)
- Bytes vs string for bodies — new `bytes` type, or accept UTF-8-only initially?
- Pure-functional handler (dodge mutation) vs mutable response builder?
- Test boundary: confirm layered quarantine + socket seam.

## Next step
Next session: execute the research agenda (A → B → C) exhaustively, then produce a full design spec
(`docs/specs/2026-06-18-m6-web-design.md`) and a prototype-spike plan. No implementation until the
design is locked with the developer.
