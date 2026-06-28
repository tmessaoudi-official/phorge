# M6 W2 extensions — middleware, route groups, route constraints, method attributes (design)

> Status: design-locked (2026-06-28). Follow-on to M6 W2 (router + `#[Route]`,
> `docs/specs/2026-06-28-m6-w2-router-attributes-design.md`). Developer-chosen next milestone; built in
> slices, each independently green + byte-identical (`run ≡ runvm ≡ real PHP 8.5`), commit green,
> never push. Slice order: **(1) middleware + route groups → (2) regex/typed route constraints → (3)
> `#[Route]` on class methods.**

## 0. Validated platform facts (prototyped on 3 backends before this spec)
- **Middleware composition** — folding a `List<(Request, (Request) -> Response) -> Response>` around a
  handler by building, per middleware, a lambda `fn(req) => mw(req, prev)` that captures `mw` + the
  previous wrapped handler — works byte-identically on run/runvm/PHP. (It surfaced a *latent* compiler
  bug — a native call as an arithmetic operand, `List.length(xs) - 1`, mis-compiled on the VM — fixed
  separately in `e44bc29` before this slice.)
- **Route-group builder** — a `(Router) -> Router` closure that receives a fresh sub-router, registers
  routes, and returns it works byte-identically (named-fn ref and inline lambda). Passing a class
  instance through a closure is sound.
- Reserved-name gotchas: `fn` can't be a local name (it's the lambda keyword); a param can't shadow a
  top-level function (`E-SHADOW-FN`).

## 1. Slice 1 — middleware + route groups (this slice)

Pure Phorge on the injected `Core.Http` types; **no new `Op`, no new `Value`**.

### 1.1 `Router` gains a middleware list (breaking the W2 ctor — W2 is unpushed, so this is a revision)
`Router` becomes two-field: `constructor(private List<Route> table, private List<MW> mws)` where
`MW = (Request, (Request) -> Response) -> Response`. Every existing `new Router([])` becomes
`new Router([], [])` (the empty middleware list; `[]` infers in call-arg position). Updated in: the
`Http.autoRouter()` desugar (`new Router([], [])`), `examples/web/router{,-attrs}.phg`, and the two
`conformance/web/router*.phg`.

New/changed methods (all return a fresh `Router`, immutable):
- `route(method, pattern, handler)` — unchanged behaviour; now carries `this.mws` forward.
- `use(MW mw)` — append a middleware (applies to every route handled by this router).
- `group(string prefix, (Router) -> Router build)` — run `build` on a fresh empty sub-router, then
  merge: each sub-route's pattern gets `prefix` prepended (`/api` + `/users` → `/api/users`), and the
  sub-router's own middleware is preserved by wrapping each merged route's effective handler. (v1: a
  group's middleware is captured at merge time into the route's pipeline; the outer router's `use`
  middleware still applies on top in `handle`.)
- `handle(Request req)` — match as in W2, then **compose `this.mws` around the matched handler**
  (first-registered = outermost) and call the composed pipeline. 404 path is unchanged (no middleware
  on a miss — a 404 is the router's, not a handler's).

Composition helper (private static, pure): fold the middleware list in reverse, each step building
`fn(req) => mw(req, prev)`. Middleware signature is the one public contract: `(Request, next) -> Response`
where calling `next(req)` continues the chain; a middleware may short-circuit (e.g. return 401 without
calling `next`).

### 1.2 Example + conformance
- `examples/web/middleware.phg` — a logging + auth middleware stack + a `/admin` group, showing
  short-circuit (an unauthenticated `/admin` request gets 401 without reaching the handler) and
  pass-through. Byte-identity-gated.
- `conformance/web/middleware.phg` (+ golden) — deterministic subset.

## 2. Slice 2 — regex/typed route constraints (next)
`r"/users/{id:\d+}"` — a `{name:regex}` segment matches only if the path component matches the (whole-
segment-anchored) pattern, via `Core.Regex` (`Regex.compile` + a full-match check). A constrained
segment that fails to match falls through (so `/users/abc` can 404 while `/users/42` hits). Adds an
`import Core.Regex` to the router prelude; determinism is preserved (regex matching is pure). Constraint
parsing splits the `{...}` on the first `:`. Precedence: a constrained param is more specific than a
bare param but less than a literal.

## 3. Slice 3 — `#[Route]` on class methods (next)
Extend `parse_attributes` to class methods (currently free-functions-only via `E-ATTR-TARGET`), and the
`Http.autoRouter()` desugar to collect `#[Route]` *methods* (registering `Class.method` static-style or
an instance handler). Decide the handler-as-method lowering in that slice's design.

## 4. Invariants
- No new `Op`/`Value`; front-end + injected-stdlib only.
- `run ≡ runvm ≡ real PHP 8.5` byte-identical for every example + conformance program.
- Transpile stays a bridge: middleware/groups emit as ordinary PHP closures + classes; no PHP framework
  dependency.
