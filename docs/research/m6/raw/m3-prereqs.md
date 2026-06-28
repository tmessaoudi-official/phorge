# M6 Web Handler — M3 Language Prerequisites (raw findings)

> Research date 2026-06-18. Question: can Phorj ship a **pure top-level `handler(Request) -> Response`**
> on TODAY's language, dodging unbuilt M3 features? Each prerequisite checked against the actual code +
> specs. Evidence cited as `file:line`.

## Ground truth — the type system surface (`src/types.rs:6-26`)

The resolved-type enum `Ty` has **11 variants** (complete list):

| # | Variant | Notes |
|---|---------|-------|
| 1 | `Int` | |
| 2 | `Float` | |
| 3 | `Bool` | |
| 4 | `String` | UTF-8, see bytes section |
| 5 | `Unit` | |
| 6 | `Named(String)` | nominal enum or class, by name (`src/types.rs:13`) |
| 7 | `List(Box<Ty>)` | the only *usable* container today |
| 8 | `Map(Box<Ty>, Box<Ty>)` | **declarable, not constructible** (see §1) |
| 9 | `Set(Box<Ty>)` | **declarable, not constructible** (see §1) |
| 10 | `Optional(Box<Ty>)` | `T?` — M3 S2, non-null discipline in `assignable` (`src/types.rs:34-48`) |
| 11 | `Null` | type of the bare `null` literal |
| (12) | `Error` | poison type, internal — not user-writable |

The compiler's parallel operand-type view `CTy` (`src/compiler.rs:46-56`) has only `Int / Float /
Class(String) / List(Box<CTy>) / Other` — Map/Set/optional/bool/string/unit all fold to `Other`
(they are never arithmetic operands). This confirms Map/Set carry **no runtime machinery** in the
compiler.

`Value` (`src/value.rs:13-29`) mirrors this: `Int, Float, Bool, Str, Unit, Null, List(Rc<Vec>),
Map(HashMap<HKey,Value>), Set(HashSet<HKey>), Instance(Rc<Instance>), Enum(Rc<EnumVal>)`. The doc
comment on `Map`/`Set` (`src/value.rs:24-25`, `src/value.rs:44-45`) is explicit: *"Constructible in
principle; the M1 sample never builds or indexes one"* and *"Unused by the M1 sample but required by
the value-type signatures."*

---

## 1. Map / Set / dictionary type

**Built today? NO (usable) / partial (declarable shell only).**

- **Type system**: `Ty::Map(K,V)` and `Ty::Set(T)` exist (`src/types.rs:15-16`) and the checker
  **resolves the annotations** — `Map<string,int>` and `Set<T>` parse and type-check
  (`src/checker.rs:171-182`: `"List"`/`"Set"` one-arg, `"Map"` two-arg with an arity error). The
  parser test `src/parser.rs:1114` proves `Map<string, int>` and nested `List<Map<string,int>>` parse.
- **Value kernel**: `Value::Map`/`Value::Set` exist with an `HKey` hashable-key subset
  (`src/value.rs:25,28,46-51`).
- **BUT there is no way to build or read one**:
  - No map/set **literal syntax** in the parser (grep for map/dict literal in `src/parser.rs` finds
    nothing; only `[..]` list literals and `match` arms).
  - No **bytecode op** to construct a map/set — `enum Op` (`src/chunk.rs:66`) has `MakeList`,
    `MakeRange`, `MakeEnum`, `MakeInstance`, but **no `MakeMap`/`MakeSet`**.
  - `Op::Index` is **list-only**: `src/vm.rs:233-247` matches `Value::List(xs)` and faults
    `"cannot index {type}"` on anything else; index must be `Value::Int`. So `m["key"]` cannot even
    be expressed (string index rejected) let alone dispatched to a map.
  - `Value::as_display` returns `None` for Map/Set (`src/value.rs:82`) — they can't be printed either.

- **Planned**: `Map`/`Set` (+ tuples) are **M3 Slice S4** — "typed collections"
  (`docs/specs/2026-06-17-m3-language-roadmap-design.md:89`, ROI "high"). `ROADMAP.md:55` lists
  `Map`/`Set` as the *first* item in the "planned next" sequence after S2.

- **DODGE for a pure handler? YES.** A handler never needs a *user-facing* `Map` type. Headers /
  query / form / route params can hide behind **accessor methods on the `Request` class** returning
  the already-built `T?`:
  `req.header(string name) -> string?`, `req.query(string name) -> string?`,
  `req.param(string name) -> string?`. The internal storage can be a **Rust-side `HashMap` inside the
  native `Request` value** (the `core.http` native module owns the map; Phorj code never sees a
  `Ty::Map`). This is exactly the S2 `core.file.read -> string?` pattern (a native returns an
  optional; Phorj composes with `??`/if-let). The accessor returns `string?` and composes with the
  *fully-built* null-safety suite. **No Map type needed in v1.**

---

## 2. Mutation (incremental Response building)

**Built today? NO — the language is immutable-only.**

- The heap is **immutable + acyclic** by construction: `src/value.rs:1-6` — *"no reassignment, no
  post-construction field mutation, and a constructor's args are fully evaluated before the instance
  exists (EV-1)."* That immutability is *why* `Rc`-sharing is sound and no GC exists yet.
- No assignment statement: `Stmt` (`src/ast.rs:194-228`) has `VarDecl` (declaration only), `Return`,
  `If`, `For`, `Block`, `Expr` — **no `Assign`**. No `SetField` op in `enum Op` (`src/chunk.rs:66`;
  there is `SetLocal` but it is used only for internal slot bookkeeping, not a user assignment
  statement — no parser surface produces it as reassignment).
- **Planned**: mutation is M3 and is the **trigger for the tracing GC** — `ROADMAP.md:56-58`: *"Planned
  next … and **mutation**. Mutation is the trigger for the real tracing garbage collector — once
  values can be reassigned and fields mutated, the `Rc` graph can form cycles…"*. It is the *last* of
  the "planned next" set, i.e. the heaviest. (No dedicated slice number; tied to the GC work.)

- **DODGE for a pure handler? YES — and it is the *idiomatic* choice.** PSR-7 (`Request`/`Response`
  interfaces, cited in the M6 plan `docs/plans/2026-06-18-m6-web-capabilities-research.md:106`) is
  **immutable with `with*()` copy-on-write methods** by design. A `Response` is built with
  `resp.withStatus(404)`, `resp.withHeader("X", "y")`, `resp.withBody("...")`, each returning a **new**
  `Response` instance — which the immutable model already supports via constructor calls (`MakeInstance`
  + method returning a fresh instance). The handler is a pure `Request -> Response` expression; no
  in-place mutation, no GC dependency. **Mutation is dodgeable AND avoiding it is more idiomatic** for
  the PHP/PSR-7 target.

---

## 3. Exceptions (try/catch/throw) vs a total Result-style alternative

**Built today? NO exception machinery exists.**

- No `throw`/`catch`/`try` keyword in the lexer or parser (grep over `src/lexer.rs`, `src/token.rs`,
  `src/parser.rs` finds none; the only `panic!` hits are Rust-side test asserts). No `Result`/`Either`
  type in `Ty`.
- What *does* exist for error-shaped control flow:
  - **Optionals `T?` + `??` + `?.` + `opt!`** (M3 S2, COMPLETE) — `Ty::Optional` (`src/types.rs:19`),
    `BinaryOp::Coalesce` (`src/ast.rs:84`), `Expr::Member{safe}` (`src/ast.rs:118-126`), `Expr::Force`
    (`src/ast.rs:133-139`).
  - **Clean runtime faults** — `Op::Fault(FaultMsg)` (`src/chunk.rs`, generalized from `MatchFail` in
    S2) produces byte-identical faults on both backends; classified by `FaultKind` in the differential
    harness (force-unwrap → `FaultKind::ForceUnwrap`, OOB → `FaultKind::IndexOob`). These are
    *aborts*, not catchable exceptions.
- **Planned**: `Result`/`Option` + `?` propagation + must-use returns are **M3 Slice S6 — "error
  handling"** (`docs/specs/2026-06-17-m3-language-roadmap-design.md:92`, ROI "med"). The design table
  explicitly favors *"clean runtime errors + `Result`/`Option` (S6)"* over a PHP-style exception soup
  (`...roadmap-design.md:33`). try/catch/throw is listed in `ROADMAP.md:55` but S6 reframes it as
  Result/Option.

- **DODGE for a pure handler? YES — totality is the better fit.** A handler **returns a `Response` for
  every outcome**, including errors: a 404 / 400 / 500 is *just another `Response` value*, produced by
  a normal `if`/`match` branch — no exceptions needed. This is the **total** model and it matches both
  (a) the immutable ethos and (b) the M6 plan's own framing
  (`docs/plans/2026-06-18-m6-web-capabilities-research.md:74`: *"Or a `Result`-style total alternative
  (fits the immutable ethos better…)"*). The handler signature `handler(Request) -> Response` is
  *already* total — there is no error channel to need. **Exceptions fully dodgeable.**

---

## 4. Lambdas / closures (Track A / S3)

**Built today? NO closure support whatsoever.**

- No closure/lambda AST node: `Expr` (`src/ast.rs:90-162`) has no `Closure`/`Lambda`/`Fn` variant.
  No function *type* in `Type` (`src/ast.rs:10-22` — only `Named`, `Optional`, `Infer`).
- The interpreter comment is explicit: `src/interpreter.rs:39` — *"No closures in"* (the scope stack
  is plain `Vec<HashMap<String, Value>>`, captured-environment-free).
- The lexer *does* tokenize `=>` as `FatArrow` (`src/lexer.rs:348`, `src/token.rs:58`) but it is used
  **only** for `match` arms — there is no arrow-function parse path.
- **Planned**: lambdas + first-class functions + pipe `|>` are **M3 Slice S3 — "lambdas + pipeline"**
  (`docs/specs/2026-06-17-m3-language-roadmap-design.md:88`, ROI "high"; this is the **NEXT** slice per
  the project CLAUDE.md "NEXT: Track A"). `BinaryOp::Pipe` (`src/ast.rs:83`) is already a token but is
  not yet a usable pipe operator. S3 also unblocks the deferred `core.list` map/filter/reduce.

- **NEEDED for a pure handler? NO.** A single **top-level named function**
  `handler(Request) -> Response` is an ordinary `FunctionDecl` (`src/ast.rs:231-239`) — fully
  supported today. Lambdas are only needed for the **router/middleware DSL**
  (`app.get("/p", handler)` passing functions as values) — explicitly out of scope for the pure-handler
  spike (`docs/plans/...m6-web-capabilities-research.md:76`: *"not needed for a single top-level
  `handler(Request)->Response`"*). **Confirmed unbuilt; confirmed not needed for v1.** The Layer-2
  server runtime (Rust side) calls the named handler by symbol, not by passing a closure.

---

## 5. bytes type

**Built today? NO bytes type — `string` is UTF-8 only.**

- `Value::Str(String)` (`src/value.rs:17`) wraps a Rust `String`, which is **guaranteed UTF-8**. The
  lexer even asserts it: `src/lexer.rs:183` — `String::from_utf8(bytes).expect("source string body is
  valid UTF-8")`. There is no `Value::Bytes`, no `Ty::Bytes`, no `Vec<u8>` value variant.
- The only `Vec<u8>` in the codebase is internal plumbing (lexer source bytes `src/lexer.rs:142`,
  ELF/section readers in `bundle/`, vendor content hashing `src/vendor.rs:184`) — never a language
  value.
- **Planned**: not a named slice. The M6 plan flags it as a *"Real gap to research"*
  (`docs/plans/2026-06-18-m6-web-capabilities-research.md:77,114`). No M3 slice owns it; it would be a
  new primitive (`Ty`/`Value`/`Op` additions + the three coupled-match discipline).

- **DODGE for a pure handler? YES (v1 contract = text/UTF-8).** A v1 handler contract of **UTF-8
  request/response bodies** covers the overwhelming majority of web payloads (HTML, JSON, form-encoded,
  plain text — all UTF-8 / ASCII). Binary upload/download (images, gzip, multipart binary parts) is
  deferred with the `bytes` type. PHP's own `string` is a byte-string, so the transpile target is
  *more* permissive than Phorj's UTF-8 `string` — the mapping is safe (Phorj UTF-8 ⊂ PHP byte-string).
  **Defer `bytes`; ship a text-only body contract for v1.**

---

## Bonus checks

### Interfaces / "any handler" abstraction

**NONE.** No `interface`/`implements` keyword anywhere (grep over `src/lexer.rs`, `src/token.rs`,
`src/parser.rs` is empty). `TokenKind::Trait` is *lexed* (`src/lexer.rs:232`, `src/token.rs:25`) but
**never parsed** — there is no trait/interface parse path (grep `Trait` in `src/parser.rs` is empty).
The transpiler emits `abstract class` only as the lowering for **payload enums**
(`src/transpile.rs:298-301`), not a user surface.

- **Planned**: interfaces + traits/mixins + records + sealed are **M3 Slice S5 — "OOP done right"**
  (`docs/specs/2026-06-17-m3-language-roadmap-design.md:91`).
- **Impact on handler**: a pure handler is a **concrete named function** `handler(Request)->Response` —
  it needs no interface. A "any handler" abstraction (trait/interface for middleware composition) is an
  S5 concern, only relevant to the router layer, not the pure spike. The `Request`/`Response` types are
  **concrete classes** (M1 classes + constructor promotion + methods, all built). Fine for v1.

### Mutation/GC coupling note

The immutable model is load-bearing for current correctness (no GC, `Rc`/`Drop` reclaims). A
copy-on-write `with*()` `Response` keeps the spike entirely inside the immutable regime, so the spike
does **not** force the GC work forward — a clean architectural win.

---

## Slice ordering (for sequencing the polished version)

From `docs/specs/2026-06-17-m3-language-roadmap-design.md:81-93`:

| Slice | Content | Status | Relevance to web |
|---|---|---|---|
| S0 | DX (var, type alias, diagnostics, explain) | ✅ DONE | — |
| S1 | indexing, ranges, expr-if | ✅ DONE | minor |
| S2 | null-safety (`T?`/`??`/`?.`/`opt!`) | ✅ DONE | **accessor returns `string?`** |
| **S3** | **lambdas + pipe `\|>`** | NEXT (Track A) | router DSL (Layer 2/3) |
| **S4** | **`Map`/`Set`/tuples** | planned | user-facing header maps (post-v1) |
| S4.5 | user generics | planned | typed bodies |
| S5 | interfaces/traits/records/sealed | planned | middleware abstraction, `Response` as record |
| **S6** | **`Result`/`Option` + `?`** | planned | richer error flow (dodged by total handler) |
| S7 | stdlib + imports | planned | `core.http`, `core.json` |

---

## VERDICT

**A pure-functional `handler(Request) -> Response` CAN ship in the spike on TODAY's language.** Every
unbuilt M3 feature (Map S4, mutation, exceptions S6, lambdas S3, bytes) is **dodgeable** for the single
top-level handler:

- **Map → accessor methods** returning `string?` over a Rust-owned internal map (S2 `core.file` pattern).
- **Mutation → PSR-7 immutable `with*()` copy-on-write** (idiomatic AND inside the current immutable model).
- **Exceptions → total handler**: errors are just `Response` values via `if`/`match` (no error channel needed).
- **Lambdas → not needed** for a named top-level handler (only the router DSL needs them).
- **bytes → UTF-8 text-body v1 contract** (PHP byte-string is a superset, so transpile is safe).

**Minimal viable Request/Response surface (all expressible with built features — M1 classes + S2 nulls):**

```
class Request {
  method(): string          // "GET"
  path(): string            // "/users/42"
  header(string k): string? // S2 optional
  query(string k): string?  // S2 optional
  body(): string            // UTF-8 v1
}
class Response {
  constructor(int status, string body)        // M1 ctor promotion
  withStatus(int s): Response                  // copy-on-write (immutable)
  withHeader(string k, string v): Response     // copy-on-write
  withBody(string b): Response                 // copy-on-write
}
function handler(Request req) -> Response { ... }  // M1 top-level fn
```

All of the above uses only **M1 classes/methods/constructor-promotion + M3 S2 optionals** — both
fully built and byte-identity-gated. The internal header/query maps live Rust-side in the `core.http`
native module (no `Ty::Map` exposed). The handler is differential/byte-identity-testable (golden
`Request → Response`), satisfying the determinism spine; the socket accept loop is the separate,
quarantined Layer-2 shell. **No M3 blocker for the spike.**
