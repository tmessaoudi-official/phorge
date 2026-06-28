# Feasibility Spike — `Core.Http` (HTTP client, Guzzle-like)

**Stage 2 feasibility spike.** Module: HTTP client. Verdict up front: **Tier B (impure) — DEFER**,
not reject. HTTPS-pure is **infeasible** (no TLS in Rust std, and crypto/TLS must not be hand-rolled),
but a constrained client is **buildable and useful** the moment M6's serve infrastructure lands, by
reusing the exact same quarantine seam. Feasibility of a *useful, shippable* client: **~55%** —
gated entirely on the TLS decision, not on engineering effort.

---

## 1. Evidence gathered (verified against the live codebase)

- **Zero external crates is real and enforced.** `Cargo.toml` `[dependencies]` is empty; the crate
  roots are `#![forbid(unsafe_code)]`; `warnings = "deny"` + `clippy::all = "deny"` are *compile*
  gates. The only external crate in the workspace is `wasm-bindgen`, scoped to the `playground/`
  member as a wasm32-only dep — the core `phorj` crate stays dependency-free. [Verified: read
  `Cargo.toml`.] → **Any TLS crate (`rustls`, `native-tls`, `openssl`) is off the table by policy,
  not just by preference.**

- **`std::net` is already in use.** `src/serve.rs` imports `std::net::{TcpListener, TcpStream}` and
  `std::io::{Read, Write}` and runs a real single-threaded HTTP/1.1 server. [Verified: `grep` over
  `src/serve.rs`.] → A *plaintext* `http://` client over `TcpStream` is squarely within std's reach;
  the server proves the wire-level machinery already exists in-tree.

- **The `Transport` seam is the exact precedent.** `src/serve.rs` defines:
  ```rust
  pub trait Transport {
      fn recv(&mut self) -> io::Result<Option<Vec<u8>>>;
      fn send(&mut self, response: &[u8]) -> io::Result<()>;
  }
  ```
  with a real `TcpTransport` (sockets) and an in-memory test transport swapped in `tests/serve.rs`
  (the "env-update HTTP-fixture-seam" pattern). The serve loop is `serve<T: Transport>(...)` and is
  tested *outside* `differential.rs`. [Verified: read `src/serve.rs:22–60`, `:177–208`.] → An HTTP
  **client** is the dual: a `HttpTransport` trait (`send_request(bytes) -> bytes`) with a real
  `TcpStream` impl and an in-memory fixture impl for tests. **The non-determinism quarantine is a
  solved problem in this repo.**

- **The `pure: false` quarantine is automatic and derived, not hardcoded.** `tests/differential.rs`
  `uses_impure_native(src)` builds the impure-module set **from `NativeFn::pure == false`** via
  `phorj::native::registry()`, then SKIPs any program that imports such a module from the
  byte-identity oracle. [Verified: read `differential.rs:908–923`, `:1004`, `:1903`.] `Core.Process`
  / `Core.Env` are the shipped precedent (`src/native/process.rs`, `pure: false`, walkthrough under
  `examples/process/`, tested in `tests/process.rs` under a controlled environment). → **Marking a
  new `Core.Http` native `pure: false` quarantines it from the spine with ZERO harness edits.** This
  is the single most important reuse.

- **The portable value unit already exists in M6's design.** `docs/specs/2026-06-18-m6-web-design.md`
  locks **Shape A**: `Request`/`Response` as pure-Phorj classes, `parse_request(bytes) -> Request?`
  and `serialize_response(Response) -> bytes`, bodies are `bytes`, headers `List<string>` raw lines
  with a `req.header(name)` linear-scan accessor. M6 W1 is **COMPLETE** (per CLAUDE.md). → A client
  reuses these *same* value types: `Http.send(Request) -> Response` is the mirror of the server's
  `handle(Request) -> Response`. No new value model is needed.

- **`bytes` primitive + `Core.Bytes` shipped** (M6 W0): request/response bodies are `bytes`, byte
  ops (`from_string`/`to_string`/`len`/`slice`/`concat`/`find`) exist. [Verified: CLAUDE.md M6 W0
  COMPLETE, `src/native/bytes.rs` present.]

---

## 2. The determinism partition — where HTTP lands

HTTP is **Tier B by construction** on every axis:

| Axis | Tier-B reason |
|------|---------------|
| Network response | A live server's body/headers/status are not a function of the program text. Poisons byte-identity instantly. |
| TLS | No std TLS; cannot hand-roll (policy). Plaintext `http://` only, in std. |
| Timing / timeouts | Wall-clock dependent → non-deterministic (same class as `Time.now()`). |
| DNS resolution | OS resolver, machine-dependent. |
| Connection failures | Network errors are environmental, not textual. |

There is **no pure sub-slice** of an HTTP *request* (unlike `Core.Url`, which is the pure
parse/build companion — already triaged Tier A separately). The closest pure surface is
request/response **serialization**, which M6 W1 already owns as `serialize_response` /
`parse_request`. So `Core.Http` contributes nothing new to Tier A; it is wholly Tier B.

**Conclusion:** `Core.Http` natives are `pure: false`, quarantined from `differential.rs`,
fixture-tested in a new `tests/http.rs`, and shipped as a `examples/http/` **walkthrough** (not a
gated example) — exactly mirroring `Core.Process`.

---

## 3. Byte-identity strategy

**There is none for the live request — and that is correct, not a failure.** The byte-identity
spine deliberately excludes this module via the `pure: false` flag. What we gate instead:

1. **Fixture parity in `tests/http.rs`** (outside `differential.rs`): an in-memory `HttpTransport`
   returns canned response bytes; assert the interpreter and VM both produce the identical
   `Response` value from the identical canned bytes. This pins **run ≡ runvm** for the
   request-building + response-parsing logic without any socket.
2. **Conformance test** (M6 pattern): pin that `serialize_response`/`parse_request` round-trip a
   canonical request identically across both Rust backends — already covered by M6 W1.
3. **Transpile is verified structurally, not by oracle**: a `Core.Http`-importing program is SKIPped
   by the PHP oracle (it's impure). Transpile correctness is checked by a fixture test asserting the
   emitted PHP is the expected curl/stream call — not by running it against a live server.

This is the *same* strategy `Core.Process`/`Core.Env` use and the same one M6's `serve` uses.

---

## 4. Exact PHP transpile target

Two candidate targets; the prior-art digest correctly flags the trap. **`php -n` is the oracle
environment, but `Core.Http` programs are quarantined from the oracle**, so the transpile target may
use extensions *not* present under `-n` — they only need to exist in a real deployment PHP. Still,
prefer the most portable:

- **`curl` is ABSENT under `php -n`** (ext-curl is not compiled-in to core) and is unavailable in
  the oracle. Since Http programs aren't oracle-gated this is *tolerable*, but a curl target means
  the transpiled PHP only runs where ext-curl is installed.
- **`file_get_contents` + HTTP stream context is PHP CORE** (the `http://`/`https://` stream
  wrapper is part of core when `allow_url_fopen=1`), so it survives more environments and needs no
  extension. **Recommended primary target.**

**Recommended transpile (core, no extension):**
```php
// Http.get(string url) -> Response  (simplified; real version threads headers + status)
(function(string $url) {
    $ctx = stream_context_create(['http' => ['method' => 'GET', 'ignore_errors' => true]]);
    $body = @file_get_contents($url, false, $ctx);
    // $http_response_header is populated by the wrapper; status parsed from $http_response_header[0]
    return __phorj_http_response($http_response_header ?? [], $body === false ? '' : $body);
})($url)
```
backed by a **gated runtime helper** `__phorj_http_response($rawHeaders, $body)` (emitted via the
`uses_http` bool + `emit_runtime_helpers` pattern) that builds the Phorj `Response` shape from the
wrapper's `$http_response_header` array — keeping the per-call PHP small and the parsing logic
single-sourced. A `curl`-based variant can be a documented opt-in for environments where
`allow_url_fopen` is disabled.

**Note:** because the bodies are `bytes` (PHP `string`) and headers are `List<string>` raw lines,
the Phorj `Response` maps to PHP cleanly with no mbstring dependency (byte-level only).

---

## 5. Phorj API sketch

Reuse M6 W1's `Request`/`Response` verbatim. Add a `Core.Http` leaf:

```phorj
package Main;
import Core.Console;
import Core.Http;        // pure:false → program is quarantined from the oracle

function main(): void {
    // Convenience verbs (thin wrappers over send):
    var resp = Http.get("http://example.com/data");          // -> Response
    Console.println("status = {resp.status}");
    Console.println("body   = {Bytes.toString(resp.body) ?? \"\"}");

    // Full control — the portable unit, mirror of the server's handle(Request)->Response:
    var req  = Request("POST", "http://example.com/api", body, headers);
    var resp2 = Http.send(req);                               // -> Response
}
```

Native surface (all `pure: false`, `NativeEval::Pure` over the `HttpTransport` seam):

| Native | Signature | Notes |
|--------|-----------|-------|
| `Http.send` | `(Request) -> Response` | The one real primitive; all verbs lower to it. Faults on connection error (clean `FaultKind`, byte-identical run≡runvm via the value kernel). |
| `Http.get` | `(string url) -> Response` | Sugar: `send(Request("GET", url, b"", []))`. |
| `Http.post` | `(string url, bytes body) -> Response` | Sugar. |

`Request`/`Response` are the existing M6 classes (no new types). `Response` fields:
`status: int`, `headers: List<string>`, `body: bytes`, plus `header(name) -> string?`.

**Scope decisions (locked by analogy to Process):** `http://` only in the Rust legs (no TLS);
single request per call, no connection pooling (matches serve's single-threaded model); no
streaming bodies (whole-body `bytes`, bounded); timeout is a fixed conservative default, *not* a
user knob in Tier A surface (a timeout knob is fine — it's environmental either way). HTTPS works
**only in the transpiled PHP** (where the core stream wrapper has TLS) — a documented asymmetry, the
same shape as "the PHP leg has mbstring, the Rust legs don't" but inverted.

---

## 6. New VM Op needed?

**No.** Every `Core.Http` entry is a native → `Op::CallNative(idx, argc)`, which already exists and
is the established path for all stdlib. Bodies are `bytes` (existing `Value::Bytes`), `Request`/
`Response` are `Value::Instance` (existing). No new `Value`, no new `Op`, no `chunk.rs`/`vm/exec.rs`/
`compiler` match changes. The re-entrant machinery (`HigherOrder`) isn't needed — `send` takes no
closure. This is purely additive, like every Wave-2 stdlib module. [Inferred: from the native
registry path being fully generic for multi-arg/value-returning/Instance-valued natives, verified by
the existing `Core.Process`/`Core.File` natives that return Optional/Instance-adjacent values.]

---

## 7. Named determinism risks

1. **TLS is the gate, not a risk to manage** — `https://` cannot be served by the Rust legs in std;
   hand-rolling TLS is policy-blocked. Mitigation: `http://`-only in Rust, `https://` works only in
   transpiled PHP, documented asymmetry. This is the feasibility ceiling.
2. **Live network in any test** — a single accidental real `TcpStream::connect` in a test that's
   *not* quarantined would make CI flaky/offline-failing. Mitigation: the `HttpTransport` fixture
   seam (mirror of `serve`'s in-memory transport); `tests/http.rs` never touches a real socket; the
   `pure: false` flag auto-SKIPs the oracle.
3. **Timeout / timing leakage** — any reachable wall-clock read (timeout measurement, retry backoff)
   is non-deterministic. Mitigation: confined to Tier B (already quarantined); never surfaces in a
   gated example.
4. **DNS / connection-error message divergence** — error strings differ across OS resolvers and
   between Rust `std::io::Error` and PHP's wrapper. Mitigation: faults must be classified by
   `FaultKind` (semantic), not raw string, per the `agree_err` precedent — but since Http programs
   are oracle-SKIPped, only run≡runvm parity matters, and both Rust legs share the same value-kernel
   fault path.
5. **Header ordering** — response headers must preserve wire order (insertion-ordered `List<string>`,
   never re-sorted via a HashMap), matching M6 W1's `List<string>` raw-line model. Already correct by
   the chosen representation.
6. **gzip/`Content-Encoding`** — a server returning gzipped bytes embeds a non-deterministic mtime in
   the gzip header; decompression also needs ext-zlib (absent under `-n`). Mitigation: send
   `Accept-Encoding: identity` and treat bodies as opaque `bytes`; defer any decompression.
7. **`allow_url_fopen` disabled** in the deployment PHP would break the `file_get_contents` target.
   Mitigation: document the curl opt-in fallback.

---

## 8. Effort & dependency

- **Engineering effort: medium** — a `src/http.rs` mirroring `src/serve.rs` (an `HttpTransport`
  trait + real `TcpStream` impl + in-memory fixture impl), 1–3 `Core.Http` natives in
  `src/native/http.rs`, a gated `__phorj_http_response` helper, `tests/http.rs`, an
  `examples/http/` walkthrough. No backend/Op surgery. The serve infrastructure cuts the cost
  roughly in half (the wire-level HTTP/1.1 read/write code can be shared).
- **Hard dependency: M6 W1 `Request`/`Response`** (COMPLETE) and ideally M6 W3 `src/serve.rs`'s
  `Transport`/HTTP-codec code to share. Building the client *before* M6 W3 lands would duplicate the
  HTTP/1.1 codec; building it *after* lets it reuse `src/serve.rs`. → **Sequence after M6 W3/W4.**

---

## 9. Recommendation — DEFER (Tier B), schedule into M6 as the client dual

Not **reject**: the module is genuinely buildable, useful (it's the #1 "real program" capability),
and has a clean precedent-backed quarantine. Not **adopt-now**: it has a hard dependency on M6's
serve codec (to avoid duplicating the HTTP/1.1 wire layer) and contributes nothing to the Tier-A
spine, so it earns its place *after* the server is in. The TLS asymmetry (Rust=http-only,
PHP=https-capable) is acceptable and parallels the existing mbstring asymmetry, but it's a real
ceiling worth a developer decision before building.

**Feasibility: ~55%** — high confidence the engineering works (the seam, the values, the transpile
target are all proven in-tree); the discount is entirely the TLS ceiling making a *fully useful*
(HTTPS-on-all-legs) client impossible without violating the zero-dep/no-hand-rolled-crypto policy.

**Confidence: medium** — the mechanisms are all verified present (`Transport`, `pure:false`
quarantine, `bytes`, M6 `Request`/`Response`), but the exact transpile target portability
(`file_get_contents` vs curl, `allow_url_fopen`) and the TLS decision are open developer choices
that materially affect usefulness.
