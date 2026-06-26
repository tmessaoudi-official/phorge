# Extended Modules — Stage 2 Design: Full HTTP (pure type hierarchy + Tier-B client)

**Scope:** (A) the pure **Tier-A** response type hierarchy
(`Response`/`JsonResponse`/`HtmlResponse`/`RedirectResponse`/`StreamResponse`) as M6 extensions over the
already-shipped W1 `Request`/`Response` value model; (B) the **Tier-B** HTTP *client* (the request-making
side), confronting the TLS wall head-on.

**Grounding (all [Verified] against the live tree this session):**
- `src/serve.rs` already exists (M6 W3): a `Transport` trait + `TcpTransport` + an in-memory test
  transport, with the server driven by a single Phorge entry `respond(bytes) -> bytes` (`SERVE_ENTRY`).
  Sockets + wall-clock live here, deliberately **outside** `tests/differential.rs`; conformance is in
  `tests/serve.rs` over a deterministic in-memory transport. *This is the exact quarantine shape Part B
  must mirror.*
- `examples/web/handler.phg` ships the **pure** W1 model: `class Request` (method/path/body:bytes +
  `header(name)`), `class Response` (status/body:bytes/headerLines:List<string>),
  `parseRequest(bytes) -> Request?`, `serializeResponse(Response) -> bytes`. Bodies are `bytes`.
- `src/native/process.rs` is the shipped `pure:false` precedent: `Core.Process`/`Core.Env`,
  quarantined from the differential by `uses_impure_native` (reads `NativeFn::pure`, *not* hardcoded),
  fixture-tested in `tests/process.rs`, transpiled to PHP (`$argv`, `getenv`).
- `php -n` at the **8.5 floor** has `curl_init`, `stream_socket_client`, **and** `fsockopen` compiled
  in — [Verified: `php -n -r 'var_dump(function_exists("curl_init"), ...)'` → `bool(true)` ×3 on both
  bare 8.6-dev and `/stack/tools/phpbrew/php/php-8.5.7/bin/php`]. **curl is core, not an ext** — so the
  PHP leg can do HTTPS even though the Rust legs cannot. This is the load-bearing asymmetry for Part B.
- `Value` enum (`src/value.rs`): `Int/Float/Bool/Str/Bytes(Rc<Vec<u8>>)/List/Map/Set/Instance/Enum/Closure`.
  **`Value` is `!Send`** (Rc heap) → single-threaded forced. No new `Value` variant is needed for either
  part (responses are `Instance`s; client results are `Instance`/`bytes`/`Optional`).
- `NativeEval::{Pure, HigherOrder, Reflective}`; `NativeFn{module,name,params,ret,eval,php,pure}` keyed
  by `(module,name)`; one `src/native/<leaf>.rs` per module. Most natives need **no new `Op`** — they
  ride `Op::CallNative`.

---

## Part A — Pure Tier-A response type hierarchy

### A.0 Tier verdict: **Tier A (gated, byte-identical) — high confidence**

Every type here is *pure Phorge data + pure functions* over `bytes`/`string`/`int`/`Map`. No clock, no
random, no socket, no env. The result of constructing a `JsonResponse(...)` and serializing it to wire
bytes is a deterministic function of the program text. **It is byte-identical on `run`/`runvm`/real PHP
by construction**, exactly like the W1 `Response` that already ships. There is nothing impure to
quarantine.

### A.1 Design stance — *one engine, several constructors* (NOT several runtime classes)

The locked M6 decision is **"one public API / evolving engine"** (Shape A; a native header map is a
*later invisible optimization, not a second API*). I extend that verbatim:

- `JsonResponse`/`HtmlResponse`/`RedirectResponse` are **smart constructors (factory functions) that
  return a `Response`**, *not* subclasses. Reason (Chesterton's Fence on the W1 shape + the project
  philosophy "removes surprises, never capability"):
  1. The portable unit `handle(Request) -> Response` (W1, and `respond(bytes)->bytes` in serve.rs) has a
     **single concrete return type**. If `JsonResponse` were a *subclass*, the serializer
     (`serializeResponse`) and the router would have to be polymorphic over an open class hierarchy —
     and M-RT inheritance, while shipped, would force the served program to expose a base/abstract
     `Response` whose PHP emission is heavier. A factory keeps the wire-format folder monomorphic and
     the PHP output flat.
  2. PHP's own ecosystem (Symfony `JsonResponse extends Response`, Laravel) *does* subclass — but those
     frameworks have a mutable `Response` and an autoloader. Phorge's `Response` is **immutable** and
     emitted as a single-file namespaced class; factory functions transpile to plain PHP functions that
     `return new Response(...)`, which is lighter and keeps the byte-identity spine trivial.
  3. `StreamResponse` is the one genuine shape divergence (a *body producer* rather than a fixed body) —
     see A.4; it stays Tier-A only for **deterministic finite producers** and is otherwise the boundary
     to Tier-B.

So the hierarchy is: **one `Response` value type** (W1, lightly extended) **+ a `Core.Http` module of
pure factory + accessor functions**. "Hierarchy" = the *naming surface*, realized as constructors.

### A.2 The extended `Response` value (over W1)

W1's `Response` is `constructor(public int status, public bytes body, public List<string> headerLines)`.
I keep it and add only what the factories need, **as pure methods** (no new fields beyond a typed
header carrier, which is the sanctioned "later invisible optimization"):

```phorge
package Main;            // (in examples; library form is package Http when M5-packaged)
import Core.Bytes;
import Core.Text;
import Core.Json;        // shipped: Json.stringify / parse  (c58ea80)

// W1 Response, unchanged in shape; headerLines stays the canonical store (raw "K: V" lines),
// preserving byte-identity with everything already shipped.
class Response {
  constructor(public int status, public bytes body, public List<string> headerLines) {}

  // pure accessors (linear scan, same discipline as Request.header)
  function header(string name) -> string? { /* scan headerLines, splitOnce on ':' */ }
  function withHeader(string name, string value) -> Response {        // immutable "with"
    // returns a NEW Response with the header line appended (clone-with shipped, M-mut)
  }
  function withStatus(int status) -> Response { ... }
}
```

`Core.Http` (new `src/native/http.rs`, **all `pure:true`**) supplies the factories + the accessor
helpers that need byte-exact escaping/encoding folded into a single native (so the three legs cannot
drift on, e.g., JSON spacing or percent-encoding):

```phorge
import Core.Http;

// --- the "hierarchy" as factories, each -> Response ---
Response Http.text(int status, string body)                      // text/plain; charset=utf-8
Response Http.json(int status, Json value)                       // application/json  (uses shipped Core.Json)
Response Http.html(int status, Html body)                        // text/html; charset=utf-8 (uses shipped Core.Html newtype)
Response Http.redirect(int status, string location)              // 3xx + Location: header
Response Http.notFound(string message)                           // 404 sugar over Http.text
Response Http.ok(string body)                                    // 200 text sugar

// --- request/response wire bridge (W1 functions promoted into the module) ---
Request? Http.parseRequest(bytes raw)
bytes    Http.serializeResponse(Response resp)                   // re-computes Content-Length
```

**Why some factories take a native and others are pure Phorge:** `Http.json` and `Http.html` are the
two where the *exact bytes* depend on an encoder whose spacing/escaping must be identical across legs.
- `Http.json(status, value: Json)` reuses the **already-shipped `Core.Json.stringify`** (PHP-faithful
  Int/Float emission, `c58ea80`) for the body, then sets `Content-Type: application/json`. Byte-identity
  is inherited from `Core.Json` (already gated). [Verified: Json module exists, `src/native/json.rs`.]
- `Http.html(status, body: Html)` reuses the shipped **`Core.Html`** newtype + pinned
  `htmlspecialchars(ENT_QUOTES)` escaping (already byte-identical). The body is `Html.render(body)` →
  `bytes`.
- `Http.text`/`redirect`/`notFound`/`ok` are *thin* and could be pure Phorge, **but** I recommend
  implementing them as natives too, for one reason: a single native single-sources the **exact header
  line spelling** (`Content-Type: text/plain; charset=utf-8`) so a future edit can't make the
  interpreter and the transpiler disagree on a space or a casing. (This is the same single-sourcing
  rationale the `process.rs` `php` closures use.)

### A.3 PHP transpile target (Part A)

Each factory native's `php` closure emits a `new \Main\Response(...)` (or `\Http\Response` when
library-packaged) constructor call with the headers as a PHP array literal joined to the W1
`List<string>` shape. Concretely, with already-emitted arg PHP `$status`, `$body`:

```php
// Http.text(status, body)  ->
new Response($status, $body, ["Content-Type: text/plain; charset=utf-8"])

// Http.json(status, value) ->   (value already emitted via Core.Json.stringify's php closure)
new Response($status, <json-bytes-expr>, ["Content-Type: application/json"])

// Http.redirect(status, location) ->
new Response($status, "", ["Location: " . $location])

// Http.serializeResponse(resp) -> (identical to W1's hand-written serializer, as a helper or inline)
```

No `mb_*` (absent under `php -n`); status→reason uses a flat `match`/array (transpiler already emits
native `match`, per memory `transpile-modernization`). **No new `Op`. No new `Value`.** The whole part
is front-end + native-registry only.

### A.4 `StreamResponse` — the one nuanced type

A streaming response is a *body producer* (a function the server pulls chunks from), not a fixed
`bytes`. Two sub-cases:

- **Tier A (deterministic finite producer):** a `StreamResponse` whose body is produced by a **pure
  closure over a fixed/finite source** (a `List<bytes>`, a `0..n` range, a lazy `Core.Stream` over a
  fixed list — see the concurrency digest's "lazy pull-based `Stream<T>`"). Because the source is fixed
  and the producer is pure, the *concatenation of all chunks* is a deterministic value — it reduces to
  `Http.bytes(status, Bytes.concat(...chunks))` for the differential. Ship it as:
  ```phorge
  Response Http.stream(int status, List<bytes> chunks)        // Tier A: finite, deterministic
  ```
  Byte-identity: the gated semantics is "the wire body is the ordered concatenation of `chunks`"; all
  three legs concatenate the same fixed list → identical. PHP leg emits `implode('', $chunks)` (or
  `echo` per chunk in the server front-controller, same bytes). The *physical* chunked
  `Transfer-Encoding` is a **serve.rs-layer concern** (Tier B, see B) — invisible to the byte-identity
  body assertion, which compares the decoded body, not the framing.
- **Tier B (live/infinite producer):** a `StreamResponse` whose producer pulls from a socket, a timer,
  or an unbounded generator (SSE, chunked proxy). This is **not gated** — it lives behind the serve.rs
  `Transport`/the Tier-B client and is fixture-tested. The *type* is the same `Response` carrying a
  closure; only the *driving* is impure.

**Recommendation:** ship `Http.stream(List<bytes>)` now (Tier A); defer the live-producer `StreamResponse`
to the M6 serve/event-loop slice, reusing the existing `Transport` seam.

---

## Part B — Tier-B HTTP **client** (the TLS wall)

### B.0 Tier verdict: **Tier B (quarantined, fixture-tested) — high confidence on the mechanism, medium on the cross-leg HTTPS story**

An HTTP client *makes a network request*: the response depends on the world, not the program text →
**non-deterministic → cannot be byte-identity-gated**. It is the textbook Tier-B feature and slots into
the *exact* mechanism `Core.Process` and `serve.rs` already use:
1. `pure: false` on every `Core.Http.Client.*` native → `uses_impure_native` auto-drops any importing
   program from `tests/differential.rs` (no harness edit — the seam the `pure` flag exists for).
2. Fixture-tested in a **new `tests/http_client.rs`** over an in-memory/loopback transport (mirroring
   `tests/serve.rs`'s in-memory `Transport` + `tests/process.rs`'s controlled-environment pattern).
3. Transpiled to PHP and run for real (not gated).

### B.1 The TLS wall — stated precisely, and the chosen escape

Three legs, and **they are not symmetric** on TLS:

| Leg | Plain HTTP (`http://`) | HTTPS (`https://`) |
|-----|------------------------|--------------------|
| Interpreter (`run`) | ✅ `std::net::TcpStream` (std-only, zero-dep) | ❌ **no std TLS** — would need `rustls`/`native-tls` (breaks zero-dep) |
| VM (`runvm`) | ✅ same `TcpStream` path | ❌ same wall |
| PHP (`php -n`) | ✅ `stream_socket_client`/`curl` | ✅ **`curl` is compiled-in** → TLS works [Verified] |

**The wall:** the *Rust* legs cannot do HTTPS without an external crate, which violates the project's
hard zero-dependency invariant. PHP can. So a naive "all three legs do the request" is impossible for
`https://`.

**This does not matter — because the client is Tier B (not gated).** The three legs do **not** have to
agree on a client call; the differential never runs it. So the design is free to give *each leg its best
available transport*, and only the **fixture tests** assert behavior. Concretely:

- **Chosen escape for the Rust legs: shell out to the system `curl` binary via
  `std::process::Command`** (the same std-only mechanism `src/vendor.rs`, `src/cli/bench.rs`, and
  `src/bundle/cross.rs` already use to invoke `git`/`php`/`rustc`/`cargo-zigbuild`). This gives the Rust
  legs **full HTTPS** with **zero crates** and **no `unsafe`**, by delegating TLS to the OS. It is the
  honest analogue of the digest's "escapes = http-only TcpStream, or shell out to `curl`."
  - **Why curl-shell-out over http-only `TcpStream`:** an http-only client is a toy (no real API is
    plaintext in 2026); shelling to `curl` makes the feature actually useful AND makes the Rust legs'
    capability match the PHP leg's (curl on both sides), which keeps the fixture tests meaningful
    (a real `https://` fixture can be exercised on all three legs if desired, though fixtures should
    prefer a loopback `http://` server for determinism).
  - **`TcpStream` is retained as the *zero-subprocess* fallback** for plain `http://` (so a minimal
    request works even where `curl` is absent), selected automatically by scheme. This is not a bandaid:
    the failure mode it handles (no `curl` on PATH for a plaintext request) is real and the root
    behavior (plain HTTP needs no TLS) is correct — documented, not suppressed.

- **PHP leg: transpile to `curl`** (compiled-in, [Verified]). One `php` closure per client native emits
  a small `curl_init`/`curl_setopt_array`/`curl_exec`/`curl_close` block returning the body/status.

**Net:** Rust legs → `curl` subprocess (HTTPS) or `TcpStream` (plain-http fallback); PHP leg → `curl`
ext. All three reach the same real endpoint when a fixture wants them to; none is gated.

### B.2 API sketch — `Core.Http.Client`

```phorge
import Core.Http.Client;       // a Tier-B leaf; pure:false natives

// Result is a Response (reusing Part A's value) or null on transport failure (composes with ?? / if-let).
Response? Client.get(string url)
Response? Client.get(string url, Map<string,string> headers)
Response? Client.post(string url, bytes body, Map<string,string> headers)
Response? Client.request(string method, string url, bytes body, Map<string,string> headers)

// Ergonomic typed reads over the returned Response (pure, Tier A — operate on the value):
int      resp.status
bytes    resp.body
string?  resp.header(string name)
Json?    Http.parseJson(Response resp)            // Tier A: pure, reuses Core.Json.parse
```

The client returns the **same `Response` value as Part A** — so a fetched response and a constructed one
are the same type, and all the pure accessors/`parseJson` work uniformly. Failure (DNS, refused,
timeout, non-2xx-at-transport) → `null` (an explicit error variant `Result<Response, HttpError>` is a
later refinement once M-faults `Result` ergonomics settle; `?` is the chosen surface for the spike).

### B.3 PHP transpile target (Part B)

```php
// Client.get(url, headers) ->
(function($url, $headers) {
  $ch = curl_init($url);
  curl_setopt_array($ch, [
    CURLOPT_RETURNTRANSFER => true,
    CURLOPT_HEADER => true,
    CURLOPT_HTTPHEADER => array_map(fn($k,$v)=>"$k: $v", array_keys($headers), $headers),
    CURLOPT_TIMEOUT => 30,
  ]);
  $raw = curl_exec($ch);
  if ($raw === false) { curl_close($ch); return null; }
  $status = curl_getinfo($ch, CURLINFO_RESPONSE_CODE);
  curl_close($ch);
  // split head/body, wrap into new Response($status, body, headerLines)
  ...
})($url, $headers)
```

(`curl` is core under `php -n` — [Verified]. No `mb_*`, no Composer.) `CURLOPT_TIMEOUT` is a fixed
constant so the *transpiled* call has no per-run knob (config-is-compile-time, per memory
`config-must-be-compile-time`).

### B.4 New VM Op / Value?

**No new `Op`, no new `Value`.** Every client call is an `Op::CallNative(idx, argc)`; the return is an
`Instance` (`Response`) wrapped in `Optional`, plus `Value::Null` on failure — all existing. The
subprocess/socket work happens inside the native's `eval: NativeEval::Pure` body on the Rust side
(it threads no closure, so `Pure` suffices; the impurity is the `pure:false` flag, not the eval shape).

### B.5 Quarantine + tests (the mechanism, concretely)

- Mark every `Core.Http.Client.*` native `pure: false`. `tests/differential.rs::uses_impure_native`
  already scans `import <module>` for any registry module with a non-pure native — so
  `import Core.Http.Client;` auto-excludes the program. **One verification gap to close:** confirm the
  scan keys on the *module string* `Core.Http.Client` (it currently does `src.contains("import {m}")`
  over impure module names) — adding the module to the registry with `pure:false` is sufficient; no
  harness edit. [Inferred from the `uses_impure_native` source read this session.]
- New `tests/http_client.rs`: drive the client against a **loopback `http://` server** spun up in-test
  via the existing `serve.rs` `TcpTransport`/`TcpListener` (deterministic: bind `127.0.0.1:0`, serve one
  canned response, assert the client parses it). This reuses serve.rs verbatim and needs no live
  internet → deterministic + offline (the `tests/vendor.rs` `file://`-fixture discipline).
- A **walkthrough** (not a gated example) under `examples/web/` (README + a `.phg` that the README shows
  but the glob doesn't gate — same as `examples/process/`). Faults/impurity can't be a runnable gated
  example (per the project's examples rule).

---

## Cross-cutting

### New VM Op / Value — **none for either part.**
Part A is factories+accessors over the existing `Response` instance (front-end + native registry). Part
B is `Op::CallNative` returning existing value shapes. The serve.rs `Transport`/`SERVE_ENTRY` plumbing
for live streaming already exists.

### Determinism risks (named)
1. **(Part B) Network** — DNS/refused/latency/non-2xx → handled by Tier-B quarantine; fixtures use
   loopback. *Mitigated by mechanism, not gated.*
2. **(Part B) `curl` presence/version** on the Rust legs — subprocess may be absent or differ. Mitigated:
   plain-`http://` falls back to `TcpStream`; fixtures run against loopback (curl version irrelevant to a
   canned response). The *transpiled* PHP always uses ext-curl (present).
3. **(Part A) JSON/HTML byte spelling** — folded into the already-gated `Core.Json`/`Core.Html` natives,
   so no *new* divergence surface; the risk is inherited and already controlled (the known float-extreme
   `Core.Json` divergence in KNOWN_ISSUES applies — examples keep to exactly-representable values).
4. **(Part A) Header line casing/spacing** — single-sourced in each factory native's `php` closure +
   `eval`, so interpreter/VM/PHP can't drift (the `process.rs` discipline).
5. **(Part B) `Content-Length`/chunked framing** — the gated body assertion compares the *decoded* body,
   never the wire framing; framing is a serve.rs/Tier-B concern.
6. **(Part A `StreamResponse`)** — only the finite-deterministic form is gated (reduces to a concat);
   live producers are Tier-B.

### Std-only Rust feasibility
- Part A: trivially std-only (pure data). ✅
- Part B: `TcpStream` (plain http) is std; `curl` via `std::process::Command` is std (the project already
  shells out to `git`/`php`/`rustc`). **HTTPS without a crate is achievable only by subprocess** — which
  is the deliberate Tier-B escape. No `unsafe`, no crate. ✅

### Effort
- **Part A:** small–medium. New `src/native/http.rs` (~6–8 factory natives + 2 accessors), reuse Json/Html/Bytes;
  one gated `examples/web/responses.phg`; ~no checker/VM/Op work. ≈ 1 focused slice.
- **Part B:** medium. New `src/native/http_client.rs` (Rust dual-path curl/TcpStream + PHP curl emit),
  new `tests/http_client.rs` (loopback), walkthrough example, KNOWN_ISSUES + docs. ≈ 1 slice, gated by
  the loopback-test harness.

### Honest feasibility
- Part A: **~92%** — pure, reuses three shipped modules + the W1 type; only open question is the
  factory-vs-subclass call (I recommend factories, high confidence).
- Part B: **~78%** — mechanism is proven (curl is core; quarantine seam shipped; subprocess precedent
  exists); the medium risk is the curl-subprocess parser robustness (header/body split across redirects,
  binary bodies) and confirming `uses_impure_native` keys cleanly on a dotted `Core.Http.Client` module
  name vs. a substring of `Core.Http`.

### Open questions for the developer
1. **Response shape:** factories returning one `Response` (my recommendation) vs. an actual
   subclass hierarchy (`JsonResponse extends Response`)? Factories keep the wire-folder monomorphic and
   the PHP flat; subclasses match Symfony familiarity but cost polymorphism in `serializeResponse`/serve.
2. **Client TLS escape:** confirm **curl-subprocess** for the Rust legs' HTTPS (my recommendation) vs.
   plain-`http://`-only `TcpStream` (zero-subprocess but toy). Curl-subprocess makes the feature real and
   matches the PHP leg's capability.
3. **Client error surface:** `Response?` (null on failure, ships now) vs. `Result<Response, HttpError>`
   (waits on M-faults `Result` ergonomics). I default to `?` for the spike, `Result` as the follow-up.
4. **Module naming:** `Core.Http` for Part A factories + `Core.Http.Client` for Part B (a sub-leaf), or a
   flat `Core.HttpClient`? Sub-leaf reads better but I should confirm the registry+`import` resolver
   handles a three-segment dotted module (the deepest current is `Core.X`).
5. **`uses_impure_native` substring check:** does `Core.Http` (Part A, pure) sharing a prefix with
   `Core.Http.Client` (impure) risk a false quarantine of pure Part-A programs via the `src.contains`
   scan? If so, the scan must match `import {m};`/`import {m} ` boundaries, not a bare substring — a
   one-line harness hardening to verify before shipping Part B alongside Part A.
