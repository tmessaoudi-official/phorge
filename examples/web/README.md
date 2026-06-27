# Web examples — the M6 HTTP story

Phorge's web model is **`handle(Request) -> Response` at the value level** (PSR-7/15 shaped). The
request/response types and the parse/route/serialize logic are **pure Phorge**, byte-identity-gated
on `run` / `runvm` like every other example. Two thin, untranspiled runtimes carry those bytes over
a real socket — one native, one PHP — and both call the *same* `handle`.

| File | What it is |
|---|---|
| `handler.phg` | **W1** — the handler model: `Request`/`Response` classes, `parseRequest(bytes) -> Request?`, `serializeResponse(Response) -> bytes`, `handle(Request) -> Response`. Bodies are `bytes`; headers are raw `List<string>` lines behind `req.header(name)`. No socket. |
| `router.phg` | **W2** — a static exact-match router: a `List<Route>` table + linear `(method, path)` scan → a `Handler` enum tag → exhaustive `match` dispatch. Pure Phorge, no new language feature. |
| `server.phg` | **W4** — the full served app: W1 parse/serialize + W2 routing + the single entry `respond(bytes) -> bytes`. This is what `phg serve` runs. |
| `server.php` | **W4** — the PHP front-controller bridge: builds a `Request` from PHP superglobals, calls the transpiled `handle`, emits the `Response`. Runnable under `php -S`. |
| `json-api.phg` | **`Core.Json` + the handler model** — a JSON endpoint: POST a JSON array of ints to get `{"count": N, "sum": S}`; a non-array body or malformed JSON returns `400` with a JSON `{"error": …}`. Bodies are JSON-in-`bytes`; `handle` parses with `Core.Json.parse`, branches with `match`, and answers with `Core.Json.stringify`. Pure value-in/value-out, byte-identical run/runvm/real PHP. |

## Run it natively — `phg serve`

`server.phg` defines `respond(bytes) -> bytes`. `phg serve` binds a socket, frames each HTTP/1.1
request (`Connection: close`, one request per connection), calls `respond` once, and writes the
bytes back. All HTTP logic — parsing, routing, the 400-on-malformed — lives in `respond`, in pure
Phorge; the runtime (`src/serve.rs`) is the thinnest possible glue and knows nothing about the
`Request`/`Response` layout.

```console
$ phg serve examples/web/server.phg --addr 127.0.0.1:8080
phg serve: listening on http://127.0.0.1:8080

$ curl -i http://127.0.0.1:8080/
HTTP/1.1 200 OK
Content-Length: 17
Connection: close
Content-Type: text/plain

Phorge web — home

$ curl -s http://127.0.0.1:8080/greet -H 'Host: phorge.dev'
Hello phorge.dev
$ curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/missing
404
```

The server is **single-threaded by design**: the `Rc`-shared object heap makes runtime values
non-`Send`, so a thread pool is impossible; true concurrency arrives with M6 green-threads under
this *unchanged* `handle(Request) -> Response` contract.

`server.phg` also has a `main()` that exercises `respond` on canned `b"…"` requests, so the program
stays byte-identical on `run` / `runvm` (and through real PHP) — the socket path is the only part
not covered there. That path is covered by `tests/serve.rs`, deliberately **outside** the
byte-identity spine (the determinism quarantine).

## Run it on PHP — `php -S`

The same program transpiles to idiomatic PHP. `server.php` is a hand-written front-controller (the
superglobal↔`Request` adapter is runtime glue, not transpiled — exactly like `src/serve.rs` on the
native side) that calls the transpiled `handle`. Generate the handlers next to it (dropping the demo
`main()` bootstrap), then start PHP's built-in server:

```console
$ phg transpile examples/web/server.phg | sed '$d' > examples/web/web_app.php
$ php -S 127.0.0.1:8080 examples/web/server.php

$ curl -s http://127.0.0.1:8080/greet -H 'Host: phorge.dev'
Hello phorge.dev
```

`web_app.php` is a generated artifact — regenerate it from `server.phg`; it is not committed.

## Why this shape

- **One value contract, two engines.** `handle(Request) -> Response` is the portable unit. `phg
  serve` and `php -S` are interchangeable hosts for it; the byte path is identical because the
  Phorge backends are byte-identical and PHP round-trips the same logic.
- **Determinism stays intact.** Everything testable (parse, route, serialize) is pure Phorge, gated
  on `run ≡ runvm`. The non-deterministic socket is one quarantined module checked over an in-memory
  transport — it never touches `tests/differential.rs`.

## Deferred (Track A / later M6)

Path parameters (`/users/{id}`) and middleware/closure routes gate on later features
(parallel-list-iteration / generics for segment matching; lambdas for middleware). The
`handle(Request) -> Response` contract does **not** change when they land — they layer on top of the
exact-match core shown here.

**Drop the `respond` bridge (gated on `Core.Http`).** Today each app writes the same
`respond(bytes) -> bytes` glue — parse → `handle` → serialize, with a `400`-on-malformed fallback.
That boilerplate is identical everywhere, but it can't be synthesized by the runtime yet because the
`Request`/`Response` types and the parse/serialize/error policy are still defined per-app (and baking
that policy into Rust would break the determinism layering). Once a standard **`Core.Http`** module
ships `Request`/`Response` + `parseRequest`/`serializeResponse`, `phg serve` will run a bare
`handle(Request) -> Response` directly — the `respond` shim disappears, the contract is unchanged.
`Core.Http` is tracked with the native-stdlib wave.
