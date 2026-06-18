# PHP Web Ecosystem — Prior Art for Phorge M6 (Web/HTTP Capability)

**Research date:** 2026-06-18
**Purpose:** Map the stock-PHP web ecosystem so Phorge's web handler model transpiles
faithfully to *idiomatic, stock* PHP 8.x (no Swoole / ReactPHP / Composer extensions).
**Transpile contract:** Phorge : PHP :: TypeScript : JavaScript.

Each section captures: API shape, type signatures, what makes it composable/extensible,
the **pure (Request→Response value transform) vs I/O (socket, superglobal read)** split,
and the cleanest minimal subset Phorge could target. An **ADVERSARIAL** subsection flags
every friction point for a statically-typed, immutable-by-default, no-generics-yet language.

---

## 1. `php -S` — Built-in Dev Server (likely `phorge serve` transpile target)

### What it is
Single-threaded HTTP server bundled with the **CLI SAPI**. Explicitly development-only;
"should never be used on public networks." Started with:

```bash
php -S localhost:8000                  # listen on host:port
php -S localhost:8000 -t public/       # set document root
php -S 0.0.0.0:8000 router.php         # all interfaces + router script
```

### Execution model (the load-bearing fact for Phorge)
- **Request-per-invocation**: each HTTP request re-runs PHP fresh. *No state persists
  between requests* in the language layer (PHP's shared-nothing model). This is exactly
  the model a transpiled stateless handler wants — there is no long-lived process to manage.
- **Single-threaded, one request at a time**: if a request blocks, the whole server stalls;
  subsequent requests queue.
- **No concurrency/config tuning** beyond `PHP_CLI_SERVER_WORKERS=<N>` (PHP 7.4+,
  forks N worker processes; **not supported on Windows**). That is the *only* scaling knob.

### Router script
Add a PHP file as the last arg: `php -S localhost:8000 router.php`.
- "The script is run at the start of each HTTP request."
- **`return false;`** → "the requested resource is returned as-is" (i.e. serve the static
  file at the document root). **Any other outcome** → "the script's output is returned to
  the browser." So a router that wants to handle everything dynamically simply never returns
  false; it `echo`s the body.

```php
<?php // router.php
if (preg_match('/\.(?:png|jpg|css|js)$/', $_SERVER['REQUEST_URI'])) {
    return false;            // let the built-in server serve the static asset
}
// otherwise: dispatch dynamically
echo "<p>Welcome to PHP</p>";
```

### $_SERVER keys it populates
`REQUEST_URI`, `REQUEST_METHOD`, `SCRIPT_FILENAME`, `PATH_INFO` (trailing URI part when an
index is returned; unreliable when the URI contains a dot like `.json`).

### Pure vs I/O
- **I/O (everything here)**: binding the socket, accepting connections, reading the request
  line, writing the response. This is the part Phorge's `phorge serve` runtime *owns* and
  quarantines from determinism — it is not part of the byte-identical spine.
- The **router script body** is the only "pure-ish" surface: given the already-parsed request
  (superglobals), produce output. That is where a transpiled Phorge handler lives.

### Cleanest minimal subset for Phorge
`phorge serve <file>` = launch `php -S host:port router.php` where `router.php` is the
**transpiled** Phorge program acting as a front-controller: read request from superglobals →
call the user's `handle(Request): Response` → emit headers + body. The dev server itself is
PHP's; Phorge only generates the router script. Native `phorge serve` (Rust `std::net`) is
the *interpreter/VM* side; the *transpile* side is literally this router.php.

### ADVERSARIAL friction
- **None at the language level** — this is pure tooling/codegen. The friction is *philosophical*:
  the request-per-invocation, shared-nothing model means "no global mutable server state"
  maps cleanly onto Phorge's immutable-by-default stance. Good fit.
- Single-threaded blocking means any Phorge example that does blocking I/O (file read) stalls
  the server — acceptable for dev, must be documented.

---

## 2. The PHP Request Model — superglobals & emit primitives (the substrate)

This is what a transpiled handler reads from and writes to. **All untyped.**

### Inputs (superglobals — populated by the SAPI before the script runs)
| Superglobal | Shape | Notes |
|---|---|---|
| `$_GET` | `array<string, string\|array>` | query-string params |
| `$_POST` | `array<string, string\|array>` | form body (urlencoded / multipart) |
| `$_REQUEST` | `array` | merged GET+POST+COOKIE (order config-dependent) |
| `$_SERVER` | `array<string, string>` | `REQUEST_METHOD`, `REQUEST_URI`, `HTTP_*` headers, etc. |
| `$_COOKIE` | `array<string,string>` | |
| `$_FILES` | `array` | nested upload metadata |
| `php://input` | stream | raw request body (read via `file_get_contents('php://input')`) — needed for JSON bodies, since `$_POST` only parses form encodings |

All of these are **untyped string-maps**. There is no compile-time guarantee a key exists or
that a value is a string vs an array (e.g. `?a[]=1&a[]=2` makes `$_GET['a']` an array).

### Outputs (emit primitives)
| Primitive | Signature | Effect |
|---|---|---|
| `header(string $header, bool $replace = true, int $code = 0): void` | sets a response header; **must be called before any body output** | I/O side-effect |
| `http_response_code(int $code = ...): int\|bool` | set/get status code | I/O side-effect |
| `setcookie(...)` | sets `Set-Cookie` | I/O side-effect |
| `echo` / `print` | writes to the response body | I/O side-effect |

### Pure vs I/O
- **Reading superglobals** = I/O (snapshot of request state injected by the SAPI). Pure code
  should *receive* these as a parameter, not reach for the global.
- **`header()`/`http_response_code()`/`echo`** = I/O side-effects performed at the *edge*.
- The clean architecture: a **pure** `handle(Request): Response` core, sandwiched by an
  **impure adapter** that (a) builds `Request` from superglobals and (b) walks `Response` →
  `header()` + `echo`. This is precisely the 3-layer decomposition in Phorge's M6 direction
  memory (pure handler / runtime / PHP transpile).

### Cleanest minimal subset for Phorge
A transpiled handler should **never** emit raw superglobal reads or bare `echo` in user code.
Instead the generated front-controller does, once:
```php
$req = Request::fromGlobals();          // adapter: reads $_SERVER/$_GET/php://input
$res = handle($req);                    // pure Phorge-transpiled function
http_response_code($res->status);
foreach ($res->headers as $k => $v) header("$k: $v");
echo $res->body;
```
User code stays in the pure middle.

### ADVERSARIAL friction
- **Untyped string-maps**: `$_GET` is `array<string, string|array>`. Phorge has no `array`/`Map`
  type yet and no generics. Exposing `getQueryParams()` requires *some* map type. Workaround
  candidates: (a) a built-in opaque `StringMap` value type with `get(key): string?` (leans on
  S2 optionals — a missing key is `null`); (b) defer to M-when-generics-land.
- **string vs array values**: `?a[]=1` makes a value an array — a `Map<string,string>` model
  *loses* this. PHP-fidelity vs Phorge-simplicity tension. Likely acceptable to model values
  as `string?` and document that array-valued params are out of scope until generics.
- **`header()` ordering rule** ("before any output") is a runtime invariant invisible to the
  type system — the pure-core/impure-edge split *naturally* enforces it (body is built as a
  value, emitted last), which is a point *in Phorge's favor*.
- **Stringly-typed status codes / methods**: `REQUEST_METHOD` is a string; Phorge would want
  an `enum Method { Get, Post, ... }` but must transpile to/from PHP strings.

---

## 3. PSR-7 — HTTP Message Interfaces (the gold-standard typed Request/Response shape)

Models HTTP messages as **immutable value objects**. "The identity is the aggregate of all
parts of the message; a change to any aspect is essentially a new message." All mutation goes
through copy-on-write `with*()` methods that **return a new instance** (`static`).

### Interface map & full signatures

**MessageInterface** (common to request + response):
```php
getProtocolVersion(): string
withProtocolVersion($version): static
getHeaders(): string[][]          // map<string, list<string>>
hasHeader($name): bool
getHeader($name): string[]
getHeaderLine($name): string
withHeader($name, $value): static
withAddedHeader($name, $value): static
withoutHeader($name): static
getBody(): StreamInterface
withBody(StreamInterface $body): static
```

**RequestInterface** (client request; extends Message):
```php
getRequestTarget(): string
withRequestTarget($t): static
getMethod(): string
withMethod($method): static
getUri(): UriInterface
withUri(UriInterface $uri, $preserveHost = false): static
```

**ServerRequestInterface** (server-side; extends Request — the one a handler receives):
```php
getServerParams(): array          // $_SERVER
getCookieParams(): array
withCookieParams(array $cookies): static
getQueryParams(): array           // $_GET
withQueryParams(array $query): static
getUploadedFiles(): array
withUploadedFiles(array $files): static
getParsedBody(): null|array|object
withParsedBody($data): static
getAttributes(): mixed[]          // arbitrary derived state (e.g. route params)
getAttribute($name, $default = null): mixed
withAttribute($name, $value): static
withoutAttribute($name): static
```

**ResponseInterface** (extends Message):
```php
getStatusCode(): int
withStatus($code, $reasonPhrase = ''): static
getReasonPhrase(): string
```

**StreamInterface** (the body — **NOT immutable**):
```php
__toString(): string
close(): void
detach(): resource|null
getSize(): int|null
tell(): int
eof(): bool
isSeekable(): bool
seek($offset, $whence = SEEK_SET): void
rewind(): void
isWritable(): bool
write($string): int
isReadable(): bool
read($length): string
getContents(): string
getMetadata($key = null): array|mixed|null
```

**UriInterface** (immutable):
```php
getScheme/getAuthority/getUserInfo/getHost(): string
getPort(): null|int
getPath/getQuery/getFragment(): string
withScheme/withUserInfo/withHost/withPort/withPath/withQuery/withFragment(...): static
__toString(): string
```

### Immutability rationale (PSR-7 meta)
- **Value-object integrity**: "changes in URI state cannot alter the request composing the URI
  instance"; "changes in headers cannot alter the message composing them."
- **Base-request reuse**: build a foundational request, derive variants without resetting state.
- **No bidirectional deps** "which can often go out-of-sync or lead to debugging or performance
  issues."
- **Explicit mutations**: state transitions are deliberate and auditable.
- **Performance rebuttal**: implementations "may safely `return $this;`" when a `with*()` would
  not change the value, so cloning isn't forced.
- **StreamInterface is the deliberate exception**: it wraps a live PHP resource — "once the
  stream has been updated, any instance that wraps that stream will also be updated — making
  immutability impossible to enforce." Recommendation: read-only streams for req/resp bodies.

### Pure vs I/O
- **Pure (value transforms)**: *everything* on Message/Request/ServerRequest/Response/Uri.
  `withHeader`, `withStatus`, `withAttribute` are pure functions producing new values. This is
  the entire point and is **exactly Phorge's immutable-by-default model**.
- **I/O**: only `StreamInterface` (wraps a socket/file/`php://input`). The body is the I/O
  escape hatch; the envelope (headers/status/uri/attributes) is pure.

### Cleanest minimal subset for Phorge
Phorge does NOT need the full PSR-7 surface. The faithful minimal kernel:
- `Request`: `method: string` (or enum), `path: string`, `query: Map<string,string>` (or
  `queryParam(k): string?`), `header(k): string?`, `body: string`.
- `Response`: `status: int`, `headers: Map<string,string>`, `body: string`, with
  `withStatus(int): Response`, `withHeader(string,string): Response`, `withBody(string): Response`.
- Drop `StreamInterface` entirely — model body as an eager `string` (Phorge has no streams and
  the dev server reads the whole body anyway). This is the single biggest *simplification* that
  stays PHP-faithful (a stock handler usually `echo`s a string).
- Drop `UriInterface` to a parsed `path` + `query` map. Skip `getUploadedFiles`/multipart for M6.

### ADVERSARIAL friction
- **`withHeader(): static` returning a NEW instance** — *aligns* with Phorge immutability, BUT
  Phorge classes are currently value-native and immutable already; the `with*` pattern needs a
  cheap "copy with one field changed" idiom. Phorge has no record-update syntax (`{ ...r, status }`).
  Friction: each `withX` must be a hand-written method that constructs a new instance copying all
  fields. Tolerable at small field counts; a future `with`-expression would help.
- **`getQueryParams(): array` / `getHeaders(): string[][]`** — needs a **Map type**. Phorge has
  no `Map`/`array` type and **no generics yet**. This is the hard blocker for full PSR-7 fidelity.
  `string[][]` (map of header-name → list-of-values) is doubly-generic. Minimal subset sidesteps
  it with accessor methods (`header(k): string?`) instead of exposing the raw map.
- **`getParsedBody(): null|array|object`** — a union of three types incl. untyped `array`/`object`.
  Phorge has no union types and no `Any`/dynamic type (`Ty` has no type variable — same blocker
  that defers `core.json`). Model body as `string` and let the user parse it once `core.json` lands.
- **`getAttribute($name): mixed`** — `mixed`/`Any` has no Phorge equivalent. Route params (the main
  use of attributes) should be passed as an explicit typed argument to the handler instead.
- **`StreamInterface` mutability** — Phorge has no streams and no mutable-resource concept;
  dropping it is *cleaner* for Phorge and still PHP-faithful for the common echo-a-string case.
- **Case-insensitive header retrieval with original-case preservation** — a runtime contract
  (`getHeader` matches case-insensitively but preserves stored case). Hard to encode in types;
  becomes a runtime-library obligation of the Phorge `Request`/`Response` impl.

---

## 4. PSR-15 — Request Handlers & Middleware (the composable pipeline)

Builds on PSR-7. Two tiny interfaces:

```php
interface RequestHandlerInterface {
    public function handle(ServerRequestInterface $request): ResponseInterface;
}

interface MiddlewareInterface {
    public function process(
        ServerRequestInterface $request,
        RequestHandlerInterface $handler
    ): ResponseInterface;
}
```

### The model
- A **handler** is the terminal: `Request → Response`, no delegation.
- A **middleware** receives the request *and* the next handler. It MAY:
  - act on the request, then call `$handler->handle($request)` to delegate inward;
  - act on / replace the returned response;
  - **short-circuit**: "A middleware component MAY create and return a response without
    delegating to a request handler" — never invoking `$handler` (auth/validation gates).
- Composition = nesting: each middleware wraps the next, forming a "concentric layers" pipeline.
  Request flows inward, response flows outward. (Slim adds in **LIFO** order: last-added runs
  first — see §5.)
- Recommended: an exception-catching middleware runs first to convert throws into responses.

### Pure vs I/O
- Both interfaces are **pure signatures** — `Request → Response` value transforms. The middleware
  *contract* introduces NO I/O. (A given middleware *may* do I/O internally — logging, DB — but
  the interface is pure.) This is an ideal fit for a deterministic, byte-identical spine: a
  middleware pipeline over pure values is fully testable without a socket.
- The only I/O is at the very edge (the server adapter that builds the first Request and emits
  the final Response — §2).

### Cleanest minimal subset for Phorge
- `Handler = fn(Request): Response` — Phorge needs **first-class function values / lambdas** to
  express this directly (Track A / S3, currently NEXT but not yet built).
- `Middleware = fn(Request, next: fn(Request): Response): Response`.
- A pipeline builder: fold a list of middleware around a terminal handler.
- This is the *most composable* and the *most aligned* abstraction — but it is **gated on S3
  lambdas**. Until then, the OO form (a `Handler` class with a `handle` method, a `Middleware`
  class with a `process` method taking a handler instance) is expressible *today* with Phorge
  classes + methods (M2 P4c).

### ADVERSARIAL friction
- **First-class functions / higher-order types** — `process(Request, Handler): Response` where
  `Handler` is "anything with a `handle` method." Phorge has no function type and no lambdas yet
  (S3). Expressible only via the **OO form** (interface-like class + method) until S3. The closure
  form (`fn($req, $handler) => ...`) is strictly post-S3.
- **No interfaces / structural typing** — PSR-15 leans on `interface`. Does Phorge have
  interfaces? (M1 surface is classes + enums + methods + `this`; interfaces unverified in the
  language surface — likely absent.) Without interfaces, "any handler" must be a concrete base
  class or a function value. Flag: middleware composition wants either interfaces OR first-class
  functions; Phorge has neither in shipped form → **OO-with-concrete-base-class is the only
  expressible path right now**, and it is clumsy.
- **Recursion/nesting depth** — a deep middleware stack nests `handle` calls; Phorge has a
  `MAX_CALL_DEPTH` guard (Wave 0). Deep pipelines are fine at realistic depths but the limit
  exists.
- **Generics absence** doesn't bite here — the pipeline is monomorphic over the concrete
  `Request`/`Response` types. PSR-15 is the *least* generics-hungry of the standards.

---

## 5. Slim & Laravel — Routing + Request/Response (high-level)

### Slim 4 (minimal, PSR-7/PSR-15 native — closest to a Phorge target)
Route definition by HTTP verb; placeholders in `{}`; handler is a callable returning a Response:
```php
$app->get('/users/{id}', function ($request, $response, array $args) {
    $userId = $args['id'];                       // route param via $args map
    $response->getBody()->write("Hi $userId");   // write to body stream
    return $response;                            // MUST return the Response
});
$app->post('/books', $handler);                  // put/delete/patch/options too
```
- **Handler signature**: `function(ServerRequestInterface $req, ResponseInterface $res,
  array $args): ResponseInterface`. Note Slim *passes in* an empty `$response` to write to and
  return (a PSR-7-friendly convenience over "construct your own response").
- **Route params**: extracted into the `array $args` map; optional segments `[/{id}]`, regex
  constraints `{id:[0-9]+}`.
- **Middleware**: `$app->add(mw)` (app-level), `->add()` on a route or group. PSR-15 `process`
  or closure `function($request, $handler){ $r = $handler->handle($request); return $r; }`.
  **LIFO**: last-added runs first; "concentric layers."
  ```php
  $after = function ($request, $handler) {
      $response = $handler->handle($request);
      return $response->withHeader('X-Added', 'v');   // post-process, COW
  };
  ```

### Laravel (batteries-included — less faithful as a *minimal* target)
```php
Route::get('/users/{user}', [UserController::class, 'show']);
Route::get('/posts/{post}/comments/{comment}', function (string $postId, string $commentId) {});
```
- **Route params**: bound positionally to handler args (or via `$request->route('id')`), not a map.
- **Request**: `Illuminate\Http\Request` (its own type, *not* PSR-7), via type-hint or `Request`
  facade. `$request->input('name')`, `$request->query('id')`.
- **Response**: return a string, array, or model → Laravel auto-converts arrays/models to **JSON**.
  Or `response()->json([...], 201)`. "Convention over configuration."
- Heavy use of facades, DI container, Eloquent models — *none* of which are stock-PHP-faithful or
  Phorge-expressible. Laravel is useful as a **routing-ergonomics reference** (param binding,
  return-a-value-becomes-JSON), not as a transpile target.

### Pure vs I/O
- **Routing** (match a method+path pattern → handler + extracted params) is a **pure** function:
  `(method, path) → (handler, params) | NotFound`. Fully deterministic, testable, byte-identical.
- **Handler invocation** is pure (`Request → Response`).
- **I/O** lives only in the framework's edge (read superglobals, emit response) — §2.

### Cleanest minimal subset for Phorge
- A **router** as a pure value transform: register `(Method, pattern) → Handler`; `dispatch(req)`
  returns the matched handler + a `params` map (or a 404 Response). Pattern matching on `{id}`
  segments is a small pure string algorithm — no generics needed if `params` is `Map<string,string>`
  / accessor-based.
- **Slim's shape is the model to mirror** (PSR-7/15 native, minimal). Adopt:
  `app.get("/users/{id}", handler)` where `handler: fn(Request, params) -> Response`.
- **Borrow from Laravel** only the ergonomic "return a value → auto-JSON" idea *later* (needs
  `core.json` + dynamic type). For M6, handler returns an explicit `Response`.

### ADVERSARIAL friction
- **Route param map `$args` is an untyped `array<string,string>`** — same Map/generics blocker as
  §2/§3. Mitigate with accessor (`params.get(k): string?`) or pass params as explicit typed args
  (Laravel positional style) — but positional binding from a dynamic pattern needs runtime arity
  the type system can't check.
- **Callable handlers** — Slim's `$app->get(path, callable)` needs **first-class functions** (S3).
  Pre-S3, registration must take a handler *object* (concrete class instance), which is verbose.
- **Variadic / heterogeneous handler signatures** — Laravel binds route params positionally with
  per-route arity (`function($postId, $commentId)`). A statically-typed language can't express
  "N string args where N depends on the route string" — Phorge must use the **map/args** style,
  not positional binding.
- **Auto-JSON of arbitrary return** — Laravel returning a model/array → JSON needs a dynamic/`Any`
  type and reflection. Not Phorge-expressible until generics + `core.json`. Keep handlers
  returning explicit `Response`.
- **Slim passing in a mutable-feeling `$response` to write into** — `$response->getBody()->write()`
  mutates a stream. Phorge has no streams; the faithful Phorge form is `Response.withBody(str)` /
  building a Response value, which is *more* immutable than Slim. Minor transpile-shape mismatch:
  Phorge emits "construct/return a Response value," Slim idiom is "write into the given one." Both
  are valid stock PHP; Phorge's is closer to PSR-7's pure intent.

---

## Cross-cutting synthesis (for the design doc)

**The clean architecture all five sources converge on** (and which matches Phorge's M6
3-layer memory: pure handler / `phorge serve` runtime / PHP transpile):

```
[ I/O edge: read superglobals → build Request ]      (impure, framework/runtime owned)
        ↓
[ pure middleware pipeline (PSR-15) ]                 (Request → Response value transforms)
        ↓
[ pure router (method,path → handler,params) ]        (deterministic, byte-identity-gated)
        ↓
[ pure handler: Request → Response ]                  (the user's Phorge code)
        ↓
[ I/O edge: Response → http_response_code + header + echo ]   (impure, runtime owned)
```

The **pure core (middleware + router + handler over immutable Request/Response values)** is
fully deterministic and byte-identity-testable on Phorge's `run`/`runvm` spine. The **I/O edges**
are quarantined into the `phorge serve` runtime (native: Rust `std::net`; transpile: the
generated `router.php` front-controller using superglobals + `header`/`echo`). This is the
determinism-quarantine boundary M6 already chose.

**What stays out of M6** (gated on later milestones): a real `Map`/generics type (forces
accessor-based query/header access for now), `core.json` + dynamic `Any` (no auto-JSON, no
`getParsedBody` union), streams (body is an eager `string`), first-class functions (handler
registration is OO/class-based until S3 lambdas land), file uploads/multipart.
