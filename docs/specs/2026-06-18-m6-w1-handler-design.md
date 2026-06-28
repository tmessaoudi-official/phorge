# M6 W1 — Handler Model Design (Shape A)

> **Status:** IMPLEMENTED 2026-06-18 (design-locked §7, then built). `bytes.find` + `text.split_once`
> natives + `examples/web/handler.phg` (Request/Response/parse_request/serialize_response/handle) all
> landed; byte-identical on `run`/`runvm` + **real PHP**. Two transpile gotchas surfaced + documented
> in KNOWN_ISSUES (PHP-builtin name collision for `package Main` fns; `private` fields aren't externally
> readable in transpiled PHP — externally-read fields are `public`).
> Parent: `docs/specs/2026-06-18-m6-web-design.md` §2/§3/§10 (W1 = the pure `handle(Request)->Response`
> slice, in-spine, Shape A). Predecessor: `docs/specs/2026-06-18-m6-w0-bytes-design.md` (the `bytes`
> type W1 consumes). W1 ships purely on today's language **plus** at most one small native (`bytes.find`)
> — no new `Op`, no new `Value` variant.

## 0. What W1 delivers

A pure, byte-identity-gated **HTTP handler model written in Phorj**: a `Request`/`Response` pair, a
`parse_request` and `serialize` written in Phorj, a sample `handle`, and an
`examples/web/handler.phg` that feeds a committed-as-`b"…"` fixture request through the whole pipe and
prints a readable ASCII summary. Runs byte-identically on `run`/`runvm` and round-trips through real
PHP. **No socket** (that is W3); W1 is the portable `handle(Request)->Response` value contract only.

## 1. The constraint that reshapes §3 — no mutation, no `core.list`, no lambdas

§3's sketch built `Request(… List<Header> headers)` in the parser. **That is not buildable on today's
language:**

- Phorj is **immutable-by-default** — reassignment / field writes are M3-deferred (`FEATURES.md`:
  "Mutation 🔲 M3"). No mutable accumulator.
- **`core.list` is deferred** (needs S3 lambdas or `List<T>` generics) — no `append`, no `map`.
- **`text.split` is the only producer of a dynamically-sized list**, and it yields `List<string>`.

Therefore the parser **cannot** map N header lines into a `List<Header>`. Two consequences, both folded
into this design:

1. **Headers are carried as `List<string>` raw lines.** `req.header(name)` parses on lookup by a linear
   `for … in` scan with an early `return` (the §3 `header()` accessor pattern, but over lines). The
   method-call API (`req.header("Host")`) is the one public surface (the design-lock), so the raw-line
   representation is an invisible implementation detail — no `Header` *class* is needed for the spike.
2. **The serializer folds headers by recursion**, never a mutable accumulator: `serialize_headers(lines,
   i) -> bytes` returns `bytes.concat(line_bytes(lines[i]), serialize_headers(lines, i+1))`, base case
   `i >= len → b""`. Pure, byte-identical, no new feature.

> A typed `Header` value type (so `req.header` returns a struct, and Response headers are
> `List<Header>`) arrives when **S3 lambdas / `core.list`** make `List<string>` → `List<Header>` mapping
> possible. It layers on under the same `req.header(name)` API — consistent with §8 "the rest layers on
> later, no handler-contract change."

## 2. The body-type fork (headline decision — §7 D1)

§3 shows `string body`; §10 W1 says **"bodies are bytes"** (the reason W0 pulled `bytes` forward —
HTTP bodies are octets, PSR-7). Resolution options:

| | Body type | New natives | Honesty | Cost |
|---|---|---|---|---|
| **HONEST-LITE (recommended)** | `bytes` | **+1** `bytes.find` | binary bodies honest; head decoded ASCII for line/colon split | one general native (`strpos` erasure) |
| HONEST | `bytes` | +2 (`bytes.find`, `text.split_once`) | + robust header parsing (`:` in values) | two natives |
| MINIMAL | `string` | 0 | dishonest on binary bodies; wastes W0 | none — but W0's purpose deferred to W3 |

**Recommendation: HONEST-LITE.** Body is `bytes` (W0 earns its place); the parser locates the
`\r\n\r\n` head/body boundary with one new native `bytes.find`, decodes the **head** (always ASCII per
RFC 7230) to a `string` for line/`:` splitting via existing `core.text`, and keeps the **body** as a
raw `bytes` slice. The handler decodes the body on demand via `bytes.to_string(req.body) ?? ""`.

## 3. The shapes (HONEST-LITE)

```phorj
package Main;                 // E-PKG-TYPE blocks a core.http library today (parent §8)
import core.console;
import core.bytes;
import core.text;

class Request {
  // path = the raw request-target (path+query as one string); query parsing deferred (S4 Map)
  Request(string method, string path, bytes body, List<string> headerLines) {}

  function header(string name) -> string? {                 // linear scan, parse on lookup
    for (string line in this.headerLines) {
      List<string> kv = text.split(line, ":");              // [name, value]  (spike: colon-free values)
      string key = text.trim(kv[0]);
      if (key == name) { return text.trim(kv[1]); }
    }
    return null;
  }
}

class Response {
  // handler supplies headers as a FIXED list literal of "Name: value" lines (literal ⇒ buildable today)
  Response(int status, bytes body, List<string> headerLines) {}

  function withHeader(string name, string value) -> Response {     // immutable copy-on-write (PSR-7)
    // returns a new Response; header append uses a fixed-arity concat, see note
  }
}

function handle(Request req) -> Response {
  string who = req.header("Host") ?? "world";
  bytes body = bytes.from_string("Hello, {who} — {req.path}");
  return Response(200, body, ["Content-Type: text/plain"]);
}
```

**`withHeader` caveat:** appending one header to an existing `List<string>` is still "build a list one
longer" — not possible without mutation/`core.list`. For the spike, `withHeader` is either (a) **dropped**
(handler constructs the full header list up-front in the `Response(...)` literal — recommended, minimal),
or (b) supported only for a **fixed small arity** via nested literals. Recommend **(a) drop `withHeader`
for W1**; document it as arriving with `core.list` (S3). The handler builds headers in one literal.

## 4. parse_request / serialize (Phorj, in-spine)

```
function parse_request(bytes raw) -> Request? {
  int sep = bytes.find(raw, b"\x0d\x0a\x0d\x0a") ?? -1;     // CRLFCRLF boundary; -1 ⇒ malformed
  if (sep < 0) { return null; }                              //   → serve answers 400 (W3)
  bytes headBytes = bytes.slice(raw, 0, sep);
  bytes body      = bytes.slice(raw, sep + 4, bytes.len(raw));
  string head = bytes.to_string(headBytes) ?? "";           // head is ASCII per RFC 7230
  List<string> lines = text.split(head, "\x0d\x0a");        // CRLF line split
  // lines[0] = request-line "METHOD SP target SP HTTP/1.1"; lines[1..] = header lines
  List<string> rl = text.split(lines[0], " ");              // [method, target, version]
  // ... construct Request(rl[0], rl[1], body, <header lines = lines[1..]>)
}

function serialize(Response resp) -> bytes {
  string reason = reason_phrase(resp.status);               // nested expression-if, see §5
  // status line + Content-Length (always recomputed) + headers (recursive fold) + CRLFCRLF + body
}
```

- **`bytes.find(haystack: bytes, needle: bytes) -> int?`** — the one new native. Rust: byte-window
  search → `Option<usize>` → `Value::Int`/`Value::Null`; **`find(h, b"")` ≡ 0** (matches PHP `strpos`).
  PHP erasure: `((($p = strpos($h, $n)) === false) ? null : $p)`. No new `Op` (it is `Op::CallNative`).
- **Header lines for the `Request`:** `lines[1..]` is "all lines after index 0" — but List has no slice
  (no `core.list`). Spike resolution: the `Request` stores the **whole** `lines` list and `header()`
  skips `lines[0]` (the request-line) during the scan, or stores `lines` and the scan tolerates the
  request-line (it has no `:` so it never matches a header key). **Recommend: store all `lines`; the
  request-line is `:`-free so the linear `header()` scan naturally ignores it.** Zero list-slicing.
- **`Content-Length` is always recomputed** by the serializer from `bytes.len(body)` (parent §6) —
  authoritative, overrides any user value.
- **Body re-attached as bytes:** `bytes.concat(headPlusCrlfCrlf, resp.body)`.

## 5. status → reason phrase (no new feature)

`match` is over enums, not ints — so `reason_phrase(int) -> string` is a **nested expression-`if`**
(each `else` arm is itself a single expression-`if`):

```phorj
function reason_phrase(int s) -> string {
  return if (s == 200) { "OK" }
    else if (s == 201) { "Created" }
    else if (s == 204) { "No Content" }
    else if (s == 400) { "Bad Request" }
    else if (s == 404) { "Not Found" }
    else if (s == 405) { "Method Not Allowed" }
    else { "Internal Server Error" };
}
```
(Confirm `else if` parses as `else { if-expr }` — expected, since an expression-`if` is an expression.)

## 6. The example — `examples/web/handler.phg` (+ README)

- **Fixture as an in-source `b"…"` literal**, NOT a committed file — `\x0d\x0a` for CRLF dodges git
  autocrlf/editor normalization entirely and dogfoots W0:
  ```phorj
  bytes raw = b"GET /hi HTTP/1.1\x0d\x0aHost: localhost\x0d\x0aAccept: text/plain\x0d\x0a\x0d\x0abody!";
  ```
- `main()` parses it, calls `handle`, serializes, and prints a **readable ASCII summary** (method, path,
  a `header()` lookup, the decoded body, the response status + decoded body, and `bytes.len(serialize…)`)
  — never dumps raw CRLF bytes, so output stays newline-clean and byte-identical on run/runvm/**real PHP**.
- Response/Request headers in the example carry ≥1 real header (sidesteps empty-`[]` element-type
  inference, an unverified edge — D-note).
- **No local named `console`/`bytes`/`text`/`file`** (E-SHADOW-IMPORT, Wave-1 guard).
- `examples/web/README.md` (the live-server walkthrough) is W4, not W1 — W1 ships the `.phg` + an
  `examples/README.md` index/coverage row.

## 7. Design-lock (2026-06-18) — confirmed, build under TDD

- **D1 — body type = HONEST.** Body is **`bytes`**, and W1 adds **both** new natives:
  `bytes.find(bytes, bytes) -> int?` (head/body boundary) **and** `text.split_once(string, string) ->
  List<string>` (robust `Name: value` split — handles `:` in values like `Host:port`/`Date`). No
  spike colon-free constraint. Both are `Op::CallNative` — **no new `Op`**.
- **D2 — header representation = `List<string>` raw lines + `req.header(name)` accessor.** No `Header`
  class for the spike (the method-call API is the one public surface; a typed `Header` value type
  arrives with S3/`core.list`).
- **D3 — `withHeader` dropped for W1.** The handler builds headers in one `Response(...)` literal;
  copy-on-write `withHeader` returns with `core.list` (S3).
- **D4 — fixture = in-source `b"…"` literal** with `\x0d\x0a` CRLF (not a committed `.http` file) —
  deterministic, dodges git autocrlf, dogfoods W0.
- **D5 — removed.** `text.split_once` (D1) makes header parsing robust; no colon-free constraint.

**New native surfaces to add in W1** (registry entries in `src/native.rs`, four-backend-generic path):
| native | sig | Rust eval | PHP erasure |
|---|---|---|---|
| `bytes.find(h, n)` | `bytes, bytes -> int?` | byte-window search → `Option<usize>`; **`find(h, b"")` = 0** | `((($p=strpos($h,$n))===false)?null:$p)` |
| `text.split_once(s, sep)` | `string, string -> List<string>` | split on first `sep` → `[head, tail]` (1 elem if absent) | `explode($sep, $s, 2)` |

## 8. Invariants honored

- **No new `Op`** — handler/parser/serializer are classes/methods/`for`/recursion/optionals/`core.*`;
  `bytes.find` is `Op::CallNative`. The three-match `Op` coupling is untouched.
- **No new `Value` variant** — `Value::Instance` (classes), `Value::List`, `Value::Bytes` all exist.
- **Byte-identity spine** — the whole pipe is glob-gated Phorj; run≡runvm by construction (shared
  kernels), PHP round-trip kept ASCII-clean.
- **`#![forbid(unsafe_code)]`, std-only** — `bytes.find` is safe slice search.
```
