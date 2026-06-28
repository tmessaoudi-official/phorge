# M6 W2 — HTTP Router + path params + `#[Route]` attributes (design)

> Status: design-locked (2026-06-28). Sequence item 2 of the post-rock-3 autonomous run. Spec-first
> per `docs/plans/2026-06-27-ga-sequence.plan.md`. Forks pre-decided in that plan's Decisions Log;
> this spec records the concrete, verified design.

## 1. Goal

Give the M6 web layer an in-language **router** on top of W1's portable `handle(Request) -> Response`
model: register `(method, pattern)` → handler routes, match an incoming request (with `/{name}` **path
parameters**), and dispatch. Plus a PHP-8-style **`#[Route(...)]` attribute** surface that
**desugars at compile time** into explicit router registration — no runtime reflection.

No socket here (that is W3 `phg serve` + `src/serve.rs`). W2 is pure, deterministic, and gated by the
byte-identity spine (`run ≡ runvm ≡ real PHP 8.5`) like every other feature.

## 2. Non-goals / deferred

- Sockets / live serving (W3).
- Middleware / route groups / closure-pipeline routes (a later slice — closures already exist, but the
  composition API is out of scope here).
- Wildcards / regex constraints on params (`{id:\d+}`), optional segments, query-string parsing.
- Attributes on anything but **free functions**, and any attribute other than `Route` having
  *semantics* (the parser accepts `#[Name(args)]` generally; only `Route` is wired — every other
  attribute name is a hard `E-UNKNOWN-ATTRIBUTE`, the safe default).

## 3. Verified platform constraints (drove the design)

These were checked against the binary before writing the spec:

1. **No empty-map literal** — `[:]` is a parse error. Path params therefore ride an **interleaved
   `List<string>`** (`[name, value, name, value, …]`) with a linear scan — the same idiom as the
   existing `Request.header` over `headerLines: List<string>`. `[]` (empty *list*) is fine.
2. **A function-typed *field* can't be invoked with `obj.f(x)`** (`type X has no method f`). It must be
   bound to a local first: `var h = this.handler; h(req)`. Verified byte-identical on run/runvm.
3. **`List.concat`, `List.length`, `xs[i]`, `Text.split/startsWith/endsWith/replace`** all exist and
   are byte-identical across backends — enough to write the matcher in pure Phorj with no new native.
4. **`Http.autoRouter()` parses as** `Call{callee: Member{object: Ident("Http"), name:"autoRouter"}, args:[]}`.
   `Http` is not otherwise a known qualifier, so this exact shape is free to claim.
5. **No new `Op`, no new `Value`.** Router/Request/Route are ordinary classes; attributes are
   front-end-only (the desugar + the checker consume them; backends never read `FunctionDecl.attrs`).
6. **Empty list `[]` is inferred only in call-argument position** (KNOWN_ISSUES) — *not* in an
   assignment (`mutable List<string> out = []` fails) or a `return`. The `extractParams` accumulator is
   therefore seeded through a private static identity helper `Router.idStrs([])` (concrete `List<string>`
   param → `[]` infers in call-arg position). `new Router([])` / `new Request(…, [], [])` are already
   call-arg positions, so they need no helper.
7. **A route pattern containing `{name}` must be a raw string** — `r"/users/{id}"` — because a normal
   string interpolates `{id}` as a variable. Verified: `"/users/{id}"` → `E-UNKNOWN-IDENT id`. This
   applies to hand-written `.route(…)` patterns *and* the `#[Route("GET", r"/users/{id}")]` arg. The
   examples + the generated registration use raw strings; documented as a usability note.

## 4. Part A — Router + path params (pure Phorj, injected with `Core.Http`)

Added to the `HTTP_PRELUDE` (injected when a program `import Core.Http;` and hasn't declared the type
itself). `import Core.List;` is added to the prelude (the matcher needs `List.length`/`List.concat`).

### 4.1 `Request` gains path-parameter attributes (PSR-15 style)

`Request` keeps its W1 surface and gains a 5th constructor field — an interleaved param list — plus two
methods. The contract `handle(Request) -> Response` is **unchanged** (params live *on* the request):

```
class Request {
  constructor(public string method, public string path, public bytes body,
              private List<string> headerLines, private List<string> attrs) {}
  function header(string name): string? { /* unchanged */ }
  // PSR-15 attribute lookup: the captured path parameter, or null.
  function param(string name): string? {
    mutable int i = 0;
    int n = List.length(this.attrs);
    while (i + 1 < n) {
      if (this.attrs[i] == name) { return this.attrs[i + 1]; }
      i += 2;
    }
    return null;
  }
  // Immutable "withAttribute": a copy of this request carrying the matched params.
  function withParams(List<string> p): Request {
    return new Request(this.method, this.path, this.body, this.headerLines, p);
  }
  static function parse(bytes raw): Request? { /* … new Request(method, path, body, lines, []) */ }
}
```

`Request.parse` passes `[]` for `attrs` (no params until the router matches). Existing callers
(`Request.parse`, `core-http.phg`'s `handle`) are unaffected — `parse` is the only construction site in
the prelude, and the examples that declare their *own* `Request` skip injection entirely.

### 4.2 `Route` + `Router`

```
class Route {
  constructor(public string method, public string pattern, public (Request) -> Response handler) {}
}
class Router {
  constructor(private List<Route> table) {}
  // Append a route immutably; chainable (this is what the #[Route] desugar emits, and what a user writes).
  function route(string method, string pattern, (Request) -> Response handler): Router {
    return new Router(List.concat(this.table, [new Route(method, pattern, handler)]));
  }
  function handle(Request req): Response { /* see 4.3 */ }
  // pure helpers, private static so they never leak into the global PHP namespace:
  static function segScore(string pattern, string path): int { /* -1 = no match; else literal-seg count */ }
  static function extractParams(string pattern, string path): List<string> { /* interleaved name,value */ }
}
```

### 4.3 Matching — literal beats param, first-registered breaks ties, 404 fallback

`handle` scans the table, keeping the **best** match by *specificity* = number of literal (non-`{…}`)
segments that matched (`segScore`). A route matches only if its method equals `req.method` and its
pattern has the same `/`-segment count as the path, with every literal segment equal. Higher score wins;
a route only replaces the incumbent on a **strictly greater** score, so the **first-registered** route
wins a true tie. No match → `Response.text(404, …)`.

This makes `/users/me` (score 2) beat `/users/{id}` (score 1) regardless of registration order — the
locked precedence rule. For the winner, `extractParams` builds the interleaved param list and the
request is enriched via `withParams` before the (locally-bound) handler is called.

`segScore` / `extractParams` split the work so params are extracted **once**, only for the winner. A
param segment is detected by `Text.startsWith(p,"{") && Text.endsWith(p,"}")`; its name is the braces
stripped via `Text.replace` (avoids `Text.substring`'s start/len ambiguity).

## 5. Part B — `#[Route(...)]` attribute syntax (new front-end surface)

### 5.1 Lexer
A new two-char token `TokenKind::HashBracket` (`#[`), added to the two-char dispatch
(`(b'#', Some(b'[')) => HashBracket`). The closing bracket reuses the existing `RBracket`. A bare `#`
remains an error (it has no other use; raw-string `r#"…"#` is lexed in the string path before this
dispatch, so no conflict).

### 5.2 AST
```
pub struct Attribute { pub name: String, pub args: Vec<Expr>, pub span: Span }
// FunctionDecl gains:  pub attrs: Vec<Attribute>,   // front-end-only; backends never read it
```
`attrs` defaults to `Vec::new()` at every existing construction site (~11). It is documented as
front-end metadata (like `throws`): erased/inert before any backend, so the byte-identity spine is safe
by construction.

### 5.3 Parser
At **item position only** (`parse_item`), before visibility/modifiers, parse zero-or-more
`#[ Ident ( exprOpt , … ) ]` attribute groups into a `Vec<Attribute>`. (`#[Name]` with no parens =
empty args.) The collected attrs are attached to the following item — **only a free `function` may
carry attributes this milestone**; attributes preceding any other item are `E-ATTR-TARGET`. The arg
list reuses the normal expression parser (so `"GET"`, `"/users/{id}"` are ordinary string-literal
exprs).

### 5.4 Checker validation (good diagnostics, span-anchored)
A pass over every function's `attrs`:
- name ≠ `Route` → **`E-UNKNOWN-ATTRIBUTE`** (hard error; lists the supported set).
- `Route` must have exactly two **string-literal** args (method, pattern) → **`E-ROUTE-ARGS`**.
- method must be a non-empty uppercase HTTP-ish token, pattern must start with `/` →
  **`E-ROUTE-SPEC`** (cheap sanity; not a full grammar).
- the annotated function's signature must be `(Request) -> Response` → **`E-ROUTE-HANDLER`** (so the
  generated registration type-checks; checked here for a clear message rather than as a downstream
  type error).

Each new code is added to `phg explain`.

## 6. Part C — `Http.autoRouter()` compile-time desugar

A new injection-phase pass `desugar_auto_router(prog)` placed in `check_and_expand`'s injection chain
**before** `check_resolutions` (so the generated nodes are type-checked normally — unlike the post-check
expand passes). It runs only when `Core.Http` is imported. It:

1. Collects every free function carrying a `#[Route(method, pattern)]` attribute, in source order.
2. Walks all expressions; replaces each `Http.autoRouter()` call (the shape in §3.4) with a constructed
   router expression:
   `new Router([]) .route("M1","/p1", fn1) .route("M2","/p2", fn2) …`
   — handlers referenced as **first-class function values** (a bare `Ident(fnName)`), which the backends
   already support. (If `new Router([])` empty-list inference proves problematic, the fallback is a
   single `new Router([new Route(...), …])` literal — decided empirically during implementation;
   either is byte-identical.)

Because the desugar runs before check, every backend sees the *same* explicit registration AST — the
expand-before-backends discipline makes byte-identity trivial and adds **no runtime attribute
machinery**. The `#[Route]` attrs remain on the functions for the checker's validation pass, then are
inert for the backends.

If no `Http.autoRouter()` call exists, `#[Route]` functions are simply normal functions (the attribute
is passive metadata, exactly like an unused PHP attribute) — still validated by §5.4.

## 7. Examples, conformance, docs

- **`examples/web/router.phg`** — rewritten from the W1 enum-tag placeholder into the real Router:
  hand-written `.route(...)` registration with a `/users/{id}` param route, a literal-vs-param
  precedence case (`/users/me` beats `/users/{id}`), a 404, exercised over canonical `b"…"` raw
  requests. Byte-identity-gated by `tests/differential.rs`.
- **`examples/web/router-attrs.phg`** — the same routes via `#[Route(...)]` + `Http.autoRouter()`,
  proving the desugar produces identical behaviour.
- **`conformance/web/router.phg`** (+ golden) — a deterministic subset pinned in the golden corpus.
- **`examples/README.md`** entry (incl. the rejected/error cases that can't be a runnable example).
- **`CHANGELOG.md`** (`### Added — M6 W2`), **`KNOWN_ISSUES.md`** (deferred middleware/constraints),
  **`STABILITY.md`** (Router/attributes land as **experimental**, under the in-progress M6 web layer),
  **`docs/MILESTONES.md`** (W2 done), `phg explain` for the new codes.

## 8. Invariants honored
- No new `Op`, no new `Value`, no bytecode/format change (front-end + stdlib only).
- `run ≡ runvm ≡ real PHP 8.5` byte-identical for every example and the conformance program.
- Transpile is a bridge: the Router emits as ordinary PHP classes; `#[Route]` is consumed at compile
  time (the desugar), so the PHP output has explicit registration, not PHP attributes — we do not rely
  on PHP's attribute reflection at runtime.
