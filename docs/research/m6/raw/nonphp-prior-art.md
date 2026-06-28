# M6 Web Capabilities — Non-PHP Prior Art Survey

> Purpose: survey the best non-PHP std-lib HTTP server designs to steal the right abstractions
> for Phorj's `phg serve` + a typed `handler(Request) -> Response` model.
> Lens: Phorj today is immutable-by-default, no Map/Set, no closures/lambdas yet (Track A),
> no exceptions, `#![forbid(unsafe_code)]`, std-only/zero-dep, UTF-8 strings, byte-identical
> `run`/`runvm` spine, green-threads (uncolored `spawn` + channels) planned for M6.
> Date: 2026-06-18. All claims sourced to authoritative docs (see Sources per section).

---

## 1. Go `net/http` — the gold-standard std-lib HTTP server

**Why it's the reference:** one interface (`Handler`), one adapter (`HandlerFunc`), one router
(`ServeMux`) that *is itself* a `Handler`, composable middleware as plain handler-wrapping, and a
goroutine-per-request model that the language's concurrency primitive makes invisible to the user.
Long-lived because the core contract is two methods and never changed; everything else
(routing, middleware, timeouts) layers on top without touching it.

### Handler signature (the whole contract)

```go
type Handler interface {
    ServeHTTP(ResponseWriter, *Request)
}
```

### HandlerFunc adapter — lets a bare function BE a Handler

```go
type HandlerFunc func(ResponseWriter, *Request)

func (f HandlerFunc) ServeHTTP(w ResponseWriter, r *Request) {
    f(w, r)
}
```
```go
http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
    io.WriteString(w, "Hello, world!\n")
})
```

### Request — read model (key fields)

```go
type Request struct {
    Method string        // "GET", "POST", ...
    URL    *url.URL      // parsed URL (Path, RawQuery, ...)
    Header Header        // map[string][]string
    Body   io.ReadCloser // streaming body
    // ...
}
```
Helpers: `FormValue`, `Cookie`, `ParseForm`, `Context`, and (1.22+) `PathValue("id")`.

### ResponseWriter — *mutation-based* write model (NOT a returned value)

```go
type ResponseWriter interface {
    Header() Header
    Write([]byte) (int, error)
    WriteHeader(statusCode int)
}
```
The handler **mutates** `w` rather than returning a response. `Write` implicitly calls
`WriteHeader(200)`. After `ServeHTTP` returns, `w` and `r.Body` are invalid.

### ServeMux — the router IS a Handler

```go
func NewServeMux() *ServeMux
func (mux *ServeMux) Handle(pattern string, handler Handler)
func (mux *ServeMux) HandleFunc(pattern string, handler func(ResponseWriter, *Request))
func (mux *ServeMux) ServeHTTP(w ResponseWriter, r *Request)   // ← mux is itself a Handler
```
Because the mux satisfies `Handler`, you pass it straight to `ListenAndServe` — routing is just
*another handler*. **Go 1.22+** added method + wildcard patterns:
- `"POST /items/create"` — method restriction baked into the pattern string.
- `"/items/{id}"` — segment wildcard, read via `r.PathValue("id")`.
- `"/files/{pathname...}"` — trailing `...` matches all remaining segments.
- Overlap resolution: **longest (most-specific) pattern wins, registration order irrelevant.**

### ListenAndServe + concurrency model

```go
func ListenAndServe(addr string, handler Handler) error  // handler nil ⇒ DefaultServeMux
func Serve(l net.Listener, handler Handler) error
```
> "Serve accepts incoming HTTP connections on the listener l, creating a new service goroutine
> for each. The service goroutines read requests and then call handler to reply to them."

**One goroutine per connection.** Handlers run concurrently; shared state must be guarded
(`sync.Mutex`). The goroutine model is what makes the blocking-looking `ServeHTTP` actually scale —
the runtime multiplexes goroutines onto OS threads. **This is the single most important lesson for
Phorj:** the *handler API stays blocking and simple* while the *runtime* provides cheap concurrency.

### Middleware = handler wrapping (no framework needed)

```go
func StripPrefix(prefix string, h Handler) Handler
func TimeoutHandler(h Handler, dt time.Duration, msg string) Handler
func MaxBytesHandler(h Handler, n int64) Handler

// user middleware: a function that takes a Handler and returns a Handler
type loggingHandler struct{ handler Handler }
func (lh *loggingHandler) ServeHTTP(w ResponseWriter, r *Request) {
    log.Printf("%s %s", r.Method, r.URL.Path)
    lh.handler.ServeHTTP(w, r)
}
```
Middleware composability comes *for free* from the interface: any `Handler -> Handler` function
chains. **This requires closures / first-class functions** — Phorj does not have these yet.

**Sources:** [pkg.go.dev/net/http](https://pkg.go.dev/net/http) · [Routing Enhancements for Go 1.22](https://go.dev/blog/routing-enhancements) · [go.dev/src/net/http/server.go](https://go.dev/src/net/http/server.go)

---

## 2. Deno `Deno.serve` — modern pure-handler over web-standard Request/Response

**Shape:** the handler is a pure function `(Request) -> Response`. No mutable writer; you *return*
the response. This is the cleanest possible API and maps perfectly onto an immutable language.

### Handler signature

```typescript
type ServeHandler<Addr> =
    (req: Request, info: ServeHandlerInfo<Addr>) => Response | Promise<Response>

Deno.serve(handler)                 // handler-first
Deno.serve(options, handler)        // options-first
Deno.serve({ port, hostname, onListen, signal }, handler)
```
- `Request` / `Response` are the **web-standard Fetch API objects** (immutable-ish value objects).
- 2nd arg `info` carries connection metadata (remote addr) — optional.
- `signal: AbortSignal` for graceful shutdown; `onListen` callback on bind.
- Defaults: port 8000, hostname "0.0.0.0". Speaks HTTP/1.1 + HTTP/2.

### Request / Response object model (Fetch API)

- `Request`: `req.method`, `req.url`, `req.headers` (Headers object), `req.body` (ReadableStream),
  plus consumers `await req.text()` / `req.json()` / `req.arrayBuffer()`.
- `Response`: `new Response(body, { status, headers })`; body may be a string, bytes, or a
  `ReadableStream` (streaming). `Response.json(...)`, `Response.redirect(...)`.

### Routing — layered ON TOP, not built in

`Deno.serve` itself has **no router**. You match inside the handler, conventionally with the
web-standard `URLPattern` API (`/users/:id` → named params). Routing is pure user code over the
single handler — exactly the "raw handler + optional router layer" stratification Phorj wants.

### Streaming bodies

Response body can be a `ReadableStream` (e.g. emit a chunk per second). Request body is also a
stream (`req.body`). **Caveat:** the response stream is *cancelled* when the client disconnects —
write() then errors. Streaming needs an async/stream abstraction Phorj lacks today.

### Concurrency

Per-request handler invocation on Deno's event loop; async handlers return `Promise<Response>`.
Single-threaded event loop + async (same family as Node), not OS threads.

**Sources:** [docs.deno.com/api/deno/~/Deno.serve](https://docs.deno.com/api/deno/~/Deno.serve) · [Writing an HTTP Server | Deno Docs](https://docs.deno.com/runtime/fundamentals/http_server/) · [HTTP Server: Routing](https://examples.deno.land/http-server-routing)

---

## 3. Bun `Bun.serve` — pure handler PLUS a declarative routes object

**Shape:** same web-standard `Request`/`Response` pure-handler core as Deno, but Bun ships a
first-class **declarative `routes` table** in the options object — the router is data, not code.

### fetch handler (the fallback / catch-all)

```ts
Bun.serve({
  port: 3000,          // defaults $BUN_PORT/$PORT/$NODE_PORT else 3000
  hostname: "0.0.0.0",
  fetch(req: Request) {
    return new Response("Bun!");
  },
});
```

### routes object model (Bun 1.2.3+) — declarative, data-driven router

```ts
Bun.serve({
  routes: {
    "/api/status": new Response("OK"),                       // static Response value
    "/users/:id": req => new Response(`Hello ${req.params.id}`), // dynamic :param
    "/api/posts": {                                          // per-method object
      GET:  () => new Response("List posts"),
      POST: async req => Response.json({ created: true, ...(await req.json()) }),
    },
    "/api/*": Response.json({ message: "Not found" }, { status: 404 }), // wildcard
    "/blog/hello": Response.redirect("/blog/hello/world"),   // redirect
    "/favicon.ico": Bun.file("./favicon.ico"),               // file serving
  },
  fetch(req) { return new Response("Not Found", { status: 404 }); }, // optional fallback
  error(err) { return new Response("ISE", { status: 500 }); },
});
```
- A route value may be: a **static `Response`**, a **`(req) => Response` function**, or an
  **object of per-method handlers**. `req.params.id` for `:id` segments.
- `Response.json(...)`, `new Response(body, {status,headers})`, `Response.redirect(...)`.
- `server.reload({ routes })` hot-swaps handlers without restart.
- Streaming via async generator / `ReadableStream` bodies; `server.timeout(req, 0)` for SSE.

### Concurrency

Event-loop, async; `server.pendingRequests` counter. Same single-thread+async family as Deno/Node.
Bun's pitch is throughput (~2.5× Node on their bench) via a fast native runtime, not a threading
model difference.

**Key takeaway for Phorj:** Bun proves the *declarative routes table* can sit on top of the pure
`fetch(req) -> Response` core. A static-Response route (`"/status": Response("OK")`) needs **no
closures** — it's just a value in a map. Function routes and per-method objects need closures + a
Map type. The static-route subset is shippable before Track A.

**Sources:** [bun.sh/docs/api/http](https://bun.sh/docs/api/http) · [Bun.serve reference](https://bun.sh/reference/bun/serve)

---

## 4. Rust std-only TCP server — proves zero-dependency feasibility (~40 lines)

**Why it matters:** Phorj's runtime is Rust, std-only, zero-dep, `forbid(unsafe)`. This is the
*exact* template for how `phg serve`'s Rust host implementation reads sockets and writes HTTP/1.1
by hand. The Rust Book's final project does it in **~40 lines** for the socket+parse+response layer.

### Socket bind + accept loop

```rust
use std::net::TcpListener;

let listener = TcpListener::bind("127.0.0.1:7878").unwrap();
for stream in listener.incoming() {
    let stream = stream.unwrap();
    handle_connection(stream);
}
```
`bind` binds the port; `incoming()` yields an iterator of `TcpStream` (one per connection).

### Read the request line off the stream

```rust
use std::io::{BufReader, prelude::*};
use std::net::TcpStream;

fn handle_connection(mut stream: TcpStream) {
    let buf_reader = BufReader::new(&stream);
    let request_line = buf_reader.lines().next().unwrap().unwrap();
}
```

### Hand-write the HTTP/1.1 response

```rust
let (status_line, filename) = if request_line == "GET / HTTP/1.1" {
    ("HTTP/1.1 200 OK", "hello.html")
} else {
    ("HTTP/1.1 404 NOT FOUND", "404.html")
};
let contents = fs::read_to_string(filename).unwrap();
let length = contents.len();
let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");
stream.write_all(response.as_bytes()).unwrap();
```
Response wire format: `status-line CRLF` + `headers CRLF` + blank `CRLF` + body.

### HTTP/1.1 parsing concerns the toy server SKIPS (the real work for `phg serve`)

- **Request line**: only the literal `"GET / HTTP/1.1"` is matched — no method/path/version parse.
- **Headers**: read but *ignored* — must be parsed into a name→value collection.
- **Body**: GET has none; POST/PUT bodies need **Content-Length** (or **chunked**
  `Transfer-Encoding`) handling — neither is implemented.
- **Keep-alive**: not handled (connection closes per request). HTTP/1.1 defaults to persistent
  connections; a real server must honor `Connection: keep-alive` / `close`.
- **Errors**: `.unwrap()` everywhere — production must degrade gracefully.
- **Bodies are octets**, not text — Phorj's UTF-8 `string` cannot model arbitrary request bodies
  (binary uploads). Need a bytes type or `string`-as-UTF-8-only contract with a documented limit.

### Concurrency: bounded thread pool (NOT unbounded spawn)

```rust
type Job = Box<dyn FnOnce() + Send + 'static>;

pub fn new(size: usize) -> ThreadPool {           // bounded, pre-spawned workers
    assert!(size > 0);
    let (sender, receiver) = mpsc::channel();
    let receiver = Arc::new(Mutex::new(receiver)); // shared queue
    let mut workers = Vec::with_capacity(size);
    for id in 0..size { workers.push(Worker::new(id, Arc::clone(&receiver))); }
    ThreadPool { workers, sender }
}

pub fn execute<F>(&self, f: F) where F: FnOnce() + Send + 'static {
    self.sender.send(Box::new(f)).unwrap();
}

// worker loop — lock held ONLY for recv(), dropped before job() runs:
let job = receiver.lock().unwrap().recv().unwrap();
job();
```
> "if we had our program create a new thread for each request ... someone making 10 million
> requests could ... use up all our server's resources" — **bounded pool prevents DoS**.

This `mpsc::channel` + `Arc<Mutex<Receiver>>` + worker-pool pattern is *precisely* the M6
"uncolored `spawn` + channels" green-thread model, prototyped first with real OS threads.

**Sources:** [Rust Book ch21-01 (single-threaded)](https://doc.rust-lang.org/book/ch21-01-single-threaded.html) · [ch21-02 (multithreaded)](https://doc.rust-lang.org/book/ch21-02-multithreaded.html)

---

## 5. Node `http.createServer` — the mutation/streaming contrast point

```js
const http = require('node:http');
const server = http.createServer((req, res) => {
  res.writeHead(200, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify({ data: 'Hello World!' }));
});
server.listen(8000);
```
- **`req`** is an `IncomingMessage` = **readable stream** (`'data'`/`'end'` events; `.headers`,
  `.method`, `.url`). **`res`** is a `ServerResponse` = **writable stream**
  (`res.writeHead`, `res.write`, `res.end`).
- The handler **mutates `res` and must explicitly `.end()`** — it does NOT return a value. Same
  family as Go's `ResponseWriter` (mutation), opposite of Deno/Bun (return a `Response`).
- Single-threaded **event loop**; long synchronous work blocks *all* requests.

**Lesson:** the mutation model (Go `ResponseWriter`, Node `res`) couples the handler to a live
stream object with ordering rules (headers before body, must-call-end). The **return-a-Response**
model (Deno/Bun) is stateless, order-free, and trivially testable — strictly better for an
immutable language. Node is the cautionary contrast, not a model to copy.

**Sources:** [nodejs.org/api/http.html](https://nodejs.org/api/http.html)

---

## 6. Comparison across the spectrum

| Design | Handler signature | Req/Resp model | Routing | Concurrency model |
|---|---|---|---|---|
| **Go net/http** | `ServeHTTP(w ResponseWriter, r *Request)` (mutation) | `*Request` read; `ResponseWriter` **mutated** | `ServeMux` (router *is a* Handler); 1.22 method+`{wildcard}` patterns | **goroutine per connection** (cheap green threads, blocking API) |
| **Deno.serve** | `(Request) => Response \| Promise` (**pure return**) | web-std `Request`/`Response` (immutable values) | none built in; `URLPattern` in user code | event loop + async |
| **Bun.serve** | `fetch(req) => Response` + **declarative `routes` table** | web-std `Request`/`Response` | **data-driven routes**: static value / `(req)=>Resp` / per-method object; `:param` | event loop + async |
| **Rust std TCP** | hand-rolled `handle_connection(TcpStream)` | parse bytes → write bytes by hand (~40 LoC) | none (you `if`/match the request line) | **bounded thread pool** (mpsc + Arc<Mutex<Receiver>>) |
| **Node http** | `(req, res) => {}` (mutation, must `.end()`) | `req` readable stream / `res` writable stream | none built in | single-thread event loop |

### The three-layer stratification (raw → pure handler → router+middleware)

1. **Raw socket** (Rust-book layer): `TcpListener` + hand-written HTTP/1.1. This is the **Rust host
   implementation** of `phg serve`, invisible to Phorj users.
2. **Pure handler `fn(Request) -> Response`** (Deno layer): the **public Phorj default**. A single
   top-level function, return an immutable `Response`. No closures, no Map, no streams required.
3. **Router + middleware** (Go/Bun layer): a `ServeMux`-style router and `Handler -> Handler`
   middleware chains, OR Bun's declarative `routes` table. **Requires closures + a Map type** for
   the function/per-method cases; the *static-route subset* (Bun `"/status": Response("OK")`)
   needs neither.

---

## 7. Adversarial feasibility — what Phorj can ship TODAY vs. what's blocked

Phorj constraints checked against each shape:

| Shape | Closures? | Map/Set? | Streams/async? | Bytes (non-UTF8)? | Feasible NOW? |
|---|---|---|---|---|---|
| **Pure `handler(Request) -> Response`** (Deno-style), top-level fn | **No** | No (headers via list/struct) | No (buffer whole body) | Needed only for binary bodies — punt to UTF-8-text contract initially | **YES — ship first** |
| **Declarative static routes** (Bun `"/path": Response(...)`) | No (values only) | **Yes — needs a Map type** | No | as above | Blocked on Map (or model routes as a typed list of `(pattern, Response)` pairs — feasible without Map) |
| **Function routes / per-method** (Bun `(req)=>Resp`) | **Yes — needs Track A lambdas** | Yes | No | as above | Blocked on Track A (closures) |
| **ServeMux router + `Handler->Handler` middleware** (Go) | **Yes (middleware = closures)** | Yes | No | as above | Blocked on Track A |
| **`ResponseWriter`/`res` mutation model** (Go/Node) | No | No | implies streaming | yes | Possible but **rejected** — fights immutability, needs mutable handle + ordering rules |
| **Streaming bodies** (Deno/Bun ReadableStream, SSE) | Yes | — | **Yes — needs an async/stream abstraction** | yes | Blocked — defer past M6 spike |

### Ranking — how soon Phorj could ship each API shape

1. **Pure top-level `handler(Request) -> Response`, whole-body-buffered, return immutable
   `Response`** — shippable in the M6 spike with **zero new language features**. Request is a
   read-only struct (`method`, `path`, `headers`, `body: string`); `Response` is a constructor
   (`Response(status, body)` / `Response.text` / `Response.json`). Maps perfectly to PHP transpile
   target (a function taking a request array, returning a response array).
2. **Static declarative routes as a typed list** of `(method, pattern, Response)` — shippable
   without Map (use a list + linear match in the Rust host); pure values, no closures.
3. **Function routes / per-method handlers / middleware chains** — gated on **Track A (lambdas)**
   and ideally a **Map** type. Land after Track A.
4. **Streaming / SSE / chunked bodies / keep-alive niceties** — gated on an async or
   generator/stream abstraction; **defer beyond the M6 spike**.

### Determinism quarantine (Phorj's byte-identical `run`/`runvm` spine)

A live server is inherently non-deterministic (network, timing, client identity) — it **cannot** be
a byte-identity-gated example. The handler *function itself* IS deterministic and testable:
`handler(Request) -> Response` is a pure function over a value, so the **handler** can be unit-tested
/ differential-gated by feeding a constructed `Request` and asserting the `Response` is byte-identical
on `run`/`runvm` — exactly the Deno/Bun `server.fetch(request)` test seam. The `phg serve` runtime
(socket loop) stays *outside* the differential harness (like vendor/network code already does).

---

## 8. Recommendation summary (full rationale; synthesis returned separately)

- **Public default = pure `handler(Request) -> Response`** (Deno's shape). Immutable, return-based,
  testable, transpiles cleanly to a PHP `function(array $req): array`. Reject the Go/Node mutation
  (`ResponseWriter`/`res.end`) model — it fights Phorj's immutability and adds ordering hazards.
- **Layer routing on top, never into the core.** Ship the pure handler first; add a Bun-style
  declarative routes *table* (as a typed list initially, Map later) and Go-style middleware once
  Track A closures land. The router should itself be expressible as a handler (Go's "mux is a
  Handler" insight) so layers compose without special-casing.
- **Concurrency: blocking thread-pool spike NOW, green threads later.** Implement the M6 spike with
  the Rust-book bounded `mpsc` thread pool (real OS threads) so the *handler API is blocking and
  simple* — then swap the executor for M6 uncolored `spawn` + channels *without changing the public
  `handler(Request) -> Response` contract* (Go proved the API survives the executor swap). Bounded,
  not unbounded-spawn, for DoS safety.
- **Host implementation = the Rust-book ~40-line TCP/HTTP-1.1 reader**, hardened for the parts the
  toy skips: real request-line/header parsing, Content-Length bodies, keep-alive, graceful errors —
  all std-only, `forbid(unsafe)`, zero-dep. HTTP-only, no TLS (matches the M6 direction memo).
- **Bodies: UTF-8 `string` contract initially** with a documented "text bodies only" limit; a true
  bytes type and streaming are deferred past the spike.
