# Changelog

All notable changes to Phorge. Format follows [Keep a Changelog](https://keepachangelog.com/);
the project is pre-1.0 and unpublished, so versions track milestone progress, not a release
cadence. Milestones and their status live in `docs/MILESTONES.md`.

## [Unreleased]

### Added — M6 W2 extensions: `#[Route]` on class methods (W2-ext complete)

`#[Route(...)]` may now annotate a **static** class method, so a class is a tidy namespace of route
handlers (the controller shape). `Http.autoRouter()` collects `#[Route]` static methods (alongside
`#[Route]` free functions) and compile-time-desugars each into a registration whose handler is a
`fn(Request req) => ClassName.method(req)` lambda — no runtime reflection. Byte-identical
run≡runvm≡real PHP.

- The attribute parser now accepts `#[…]` on class methods (a `#[…]` on a constructor/field/hook is
  `E-ATTR-TARGET`); a non-`static` `#[Route]` method is `E-ROUTE-METHOD-STATIC` (an instance
  controller has no routable receiver this slice). `phg explain E-ROUTE-METHOD-STATIC`.
- `examples/web/controller.phg` + `conformance/web/controller.phg`.

This **completes the M6 W2 extensions** milestone (middleware + groups → constraints → method
attributes). Still deferred: optional segments / wildcards, instance-controller routing, and the W3
serve/concurrency runtime.

### Added — M6 W2 extensions: regex/typed route constraints

A `{name:regex}` route pattern segment captures `name` only when the path component matches the regex,
anchored to the whole segment (`^(?:regex)$`, via `Core.Regex`). `r"/users/{id:\d+}"` matches
`/users/42` but not `/users/ada`. Precedence is **literal > constrained param > bare param**, so a
constrained route is preferred over a bare `{name}` but still loses to an exact literal. A constrained
segment whose component fails its regex makes the whole route not match (it falls through to the next).
The router prelude now imports `Core.Regex`. `examples/web/route-constraints.phg` +
`conformance/web/route-constraints.phg`, byte-identical run≡runvm≡real PHP (ASCII patterns).
**Gotcha fixed:** a constraint regex may contain braces (`\d{4}`), so the `{name:…}` inner text is
extracted by dropping only the **outer** braces (`Text.substring(seg, 1, -1)`), not by stripping every
`{`/`}`.

### Added — M6 W2 extensions: router middleware + route groups

The `Core.Http` `Router` gains a middleware pipeline and sub-router groups — pure Phorge over
first-class functions, **no new `Op`, no new `Value`**, byte-identical `run ≡ runvm ≡ real PHP`.

- **Middleware** — `router.use(mw)` where `mw : (Request, next) -> Response`. A middleware may call
  `next(req)` to continue the chain (and post-process the result) or **short-circuit** by returning a
  `Response` without calling `next` (e.g. a 401 from an auth middleware). Applied outermost-first to
  every matched handler, composed as `fn(req) => mw(req, next)` folded over the list.
- **Route groups** — `router.group(prefix, build)` runs the `(Router) -> Router` builder on a fresh
  sub-router, then merges each sub-route with `prefix` prepended and the group's own middleware
  composed around its handler. The parent's `use` middleware still applies on top.
- `Router` is now two-field (`table` + middleware); the `Http.autoRouter()` desugar and the router
  examples/conformance build it as `new Router([], [])`. `examples/web/middleware.phg` +
  `conformance/web/middleware.phg` showcase a logging + auth stack and an `/admin` group.

### Fixed

- **VM-compiler: a native-qualified call or a static-method call used as an arithmetic operand / a
  function value.** `List.length(xs) - 1` (and `Module.fn(...) <op> n`) compiled on the interpreter
  but failed on the VM (`undefined variable \`List\``); likewise a `var f = Class.staticFn(...)` whose
  result is a function then failed `f(x)` as "not a function". `ctype`'s `Call`→`Member` arm now
  resolves native-qualified and static-method calls to their return `CTy` (a new `ty_to_cty`/
  `native_ret_cty`), closing two latent `run`↔`runvm` breaks (the documented CTy-operand trap).
  Regression: `conformance/lang/native-operand.phg`.

### Added — M2.5 Phase 3a: cross-stub registry (distributed `phg build --target`)

A **distributed** (sourceless) `phg` can now `build --target <triple>` / `--all` for the Phase-2 cross
targets by downloading a prebuilt runtime stub from the release registry, verifying it, caching it, and
embedding the program — closing the Phase-2 "needs a source checkout" limitation. No signing yet
(Phase 3b); no new runtime dependency.

- **`bundle/sha256.rs`** — hand-rolled FIPS-180-4 SHA-256 (std-only, same ethos as the CRC-32),
  known-vector tested; cross-checked against the host `sha256sum` on a real binary in the tests.
- **`bundle/manifest.rs`** — the per-target sha256 manifest (tolerant line parser, `lookup`,
  `registry_base` via `Cargo.toml` `repository` + version, `PHORGE_STUB_REGISTRY`/`PHORGE_STUB_MANIFEST`
  overrides, the `phg-stub-<triple>` asset-name convention).
- **`build.rs`** — bakes `PHORGE_BAKE_STUB_MANIFEST` into the binary (empty when unset), breaking the
  stub↔manifest circularity so cross stubs have manifest-independent, stable hashes.
- **`bundle/cross.rs`** — the cache-miss path is now a 3-way branch: cache hit → local `cargo-zigbuild`
  (source checkout) → **download + sha256-verify + cache** (distributed). Verify-before-cache: a
  tampered/partial download never poisons the cache. Transport is `curl` for `http(s)` (std has no TLS;
  `PHORGE_CURL` override) and `fs::copy` for `file://`/local (the hermetic-test path).
- **`.github/workflows/stub-registry.yml`** — a 2-pass, secret-free CI workflow (build stubs env-unset
  → hash → bake manifest into the Linux primary → publish), complementing the existing `release.yml`
  human archives.
- **Tests:** `tests/registry.rs` (hermetic client: verify/cache, tamper-rejection, missing entry/asset,
  cross-implementation hash check) + a toolchain-gated `tests/build.rs` end-to-end (real musl stub →
  download → verify → embed → run, byte-identical to `runvm`). No user-visible flag change. Phase 3b
  (signing + macOS stub) deferred — see KNOWN_ISSUES.

### Added — M6 W2 `#[Route(...)]` attributes

A PHP-8-style **attribute** surface — `#[Route("GET", r"/users/{id}")]` on a handler — that
**desugars at compile time** into explicit router registration. No runtime reflection, no new `Op`,
no new `Value`; byte-identical `run ≡ runvm ≡ real PHP`.

- **New front-end surface:** the lexer gains a `#[` token; the parser accepts item-level
  `#[Name(args)]` groups on **free functions** (other targets are `E-ATTR-TARGET`); `FunctionDecl`
  carries the parsed `Attribute`s (front-end-only — no backend reads them).
- **Checker validation:** only `#[Route]` is recognized (`E-UNKNOWN-ATTRIBUTE` for any other name);
  a `Route` needs exactly two string-literal args (`E-ROUTE-ARGS`), a non-empty method + `/`-leading
  path (`E-ROUTE-SPEC`), and a one-parameter handler that returns a value (`E-ROUTE-HANDLER`). All
  five codes self-document via `phg explain`.
- **Compile-time desugar:** `Http.autoRouter()` is lowered (before the type-checker, in the injection
  chain) into `new Router([]).route(...).route(...)` — one `.route` per `#[Route]` handler, each
  referenced as a first-class function value — so every backend sees the same explicit registration.
  `examples/web/router-attrs.phg` + `conformance/web/router-attrs.phg` (golden identical to the
  explicit `router.phg` form). Patterns with `{name}` must be raw strings (`r"/users/{id}"`).

### Added — M6 W2 HTTP router + path parameters

`import Core.Http;` now also injects a **`Router`** (+ a `Route` row type): build it by chaining
`.route(method, pattern, handler)` — handlers are ordinary first-class `(Request) -> Response`
functions — then `router.handle(req)` matches and dispatches. Pure Phorge over the W1 model (no new
`Op`, no new `Value`, no socket — that is W3 `phg serve`); byte-identical `run ≡ runvm ≡ real PHP`.

- **Path parameters** — a `{name}` pattern segment captures that path component, read by the handler
  with **`req.param("name") -> string?`** (PSR-15-style request attributes, so the
  `handle(Request) -> Response` contract is unchanged — `Request` gains a 5th private `attrs` field
  carrying the captures, plus `param`/`withParams`).
- **Literal > parameter precedence** — `/users/me` (all-literal) beats `/users/{id}` regardless of
  registration order (specificity = literal-segment count; a true tie goes to the first-registered
  route). Method-sensitive; no match → a 404 response.
- A pattern containing `{…}` **must be a raw string** (`r"/users/{id}"`), otherwise the normal string
  interpolates `{id}` as a variable — documented in `examples/web/router.phg` (rewritten from the W1
  enum-tag placeholder into the real router) and pinned by `conformance/web/router.phg`.

### Added — stability & conformance (GA rock 3)

A stability story for the pre-1.0 surface: a golden-output conformance corpus, written policies, and a
deprecation mechanism.

- **Conformance corpus** (`conformance/`, gated by `tests/conformance.rs`): 32 single-feature programs
  + a flagship multi-package DDD project, each with committed golden output asserted byte-identical on
  the interpreter, the VM, **and** real PHP. Stronger than the example differential (which only checks
  the backends *agree*) — the golden pins the value, catching a regression where all backends drift
  identically. Glob-discovered (incl. project roots via `phorge.toml`). Breadth covers the full stable
  language surface: condition loops + compound-assign (`lang/loops`), `foreach … as … with i`
  (`lang/foreach`), integer ranges (`lang/ranges`), `"""` text blocks + raw strings
  (`lang/text-blocks`), `type` aliases (`lang/type-aliases`), member visibility (`types/visibility`),
  property hooks (`types/property-hooks`), and fixed-length lists `[T; N]` (`types/fixed-lists`),
  alongside the type-system, collection, stdlib, and error programs.
- **`SEMVER.md`** — the versioning contract: in `0.x` minor versions may break but each is documented
  (`### Breaking` CHANGELOG heading); at `1.0` the *stable* tier freezes under strict SemVer.
- **`STABILITY.md`** — every public construct, stdlib module, and CLI command sorted into
  stable / experimental / deprecated tiers; the conformance corpus enforces the stable tier.
- **`docs/DEPRECATION.md`** + the **`W-DEPRECATED`** lint: a deprecated stdlib symbol keeps working but
  emits a warning naming its replacement + removal version (warning channel, never gates the build),
  for ≥1 minor release before removal. Flagged via a `native::deprecation_of` side table (empty in the
  shipping build — the mechanism is ready ahead of the first real deprecation; a `#[cfg(test)]` sample
  exercises the lint). `phg explain W-DEPRECATED`.

### Added — overloaded static methods (Statics-B)

A `static` method may now be **overloaded** and called by the class name: `Color.of(int)` /
`of(int,int,int)` / `of(string)` are selected at the call site by the argument types, runtime
multiple dispatch identical to instance-method overloading. Closes the Statics-A deferral. One new
`Op::CallStaticOverload` (runtime-identical to `Op::CallOverload` — it shares the exec arm and the
`validate` bounds check; it differs only in compile-time `stack_effect`, since the compiler pushes a
dummy receiver below the args that the selected static body's arity pops). Byte-identical
run≡runvm≡real PHP.

- Checker: removed the static-call overload rejection (routes through `check_method_sigs`, the
  instance-overload path); added `E-OVERLOAD-STATIC-MIX` — every overload of one name must agree on
  `static`-ness (a mixed set has no sound call form; PHP forbids it too). Interpreter already
  selected; compiler now consults `method_overloads` at a static call site and emits
  `Op::CallStaticOverload`; transpiler emits a `static` dispatcher with `self::` branch targets.
- `examples/guide/overloaded-statics.phg` (incl. an inherited overloaded static `Swatch.of(..)`);
  checker tests; `phg explain E-OVERLOAD-STATIC-MIX`. **Still deferred:** a static on a generic class
  using the class type parameter; late static binding (`static::` / `new static()`).

### Added — `phg lsp` language server (Item D)

A Language Server over stdio so editors get live Phorge diagnostics, hover, and go-to-definition (GA
rock 2 — daily-use tooling). Design: `docs/specs/2026-06-28-lsp-design.md`. No new `Op`/`Value`; off
the byte-identity spine. Ships with a VS Code thin client (`editors/vscode/`).

- **Hover** — the declaration signature of the symbol under the cursor (top-level *or* a local/param).
- **Go-to-definition** — jump to a function / class / enum / interface / trait / type alias declaration,
  or to a local binding (parameter, `var`, `for` var, `if`-let, `catch`, destructure) in scope.
- **Completion** (v2) — top-level names, the enclosing callable's in-scope locals/params, and keywords.
- **Document symbols** (v2) — a hierarchical outline; classes/enums/interfaces/traits expand to their
  members/variants (`range` `[item..next_item)` so children nest correctly, `selectionRange` = name).
- **True end-ranges** (v2) — diagnostics, hover, and definition ranges span the whole token (re-derived
  from the buffer, since the `Diagnostic` struct is span-less), not a 1-char caret.
- Resolution lives in `src/lsp/scope.rs` (position↔offset, binding collection, enclosing-callable by
  source ordering) + `src/lsp/symbols.rs`; all front-end-only. **Deferred:** member completion
  (needs the resolved-type index) and lambda/match-pattern binders.
- **VS Code thin client** (`editors/vscode/`): registers `*.phg` + launches `phg lsp`. Generic-editor
  registration (incl. a Neovim snippet) documented in the README "Editor support" section.

- **Hand-rolled JSON-RPC in `std`** (`src/lsp/`): an LSP server is not a security-critical primitive,
  so the dependency policy excludes `tower-lsp`/`lsp-server`/`serde`. The module owns a minimal total
  JSON parser (inbound bodies), `Content-Length` framing, the server loop, and the diagnostic mapping.
- **`phg lsp`** speaks LSP on stdin/stdout: `initialize` (advertises `textDocumentSync: full`),
  `didOpen`/`didChange`/`didClose`, `shutdown`/`exit`. On open/change it runs the **same** pipeline as
  `phg check` (lex → parse → check) and pushes `publishDiagnostics`, so editor squiggles equal the CLI.
- Diagnostics map 1-based `line`/`col` → LSP 0-based ranges, error/`W-…` → severity 1/2, and carry the
  stable `code` (resolvable via `phg explain`). `tests/`-style coverage in `src/lsp/tests.rs` (10 tests:
  JSON parser, lifecycle, diagnostics, severity). **Next slice:** hover + go-to-definition (a
  position→symbol index) and a VSCode thin client.

### Added — inherited / trait static methods (Statics-A)

A `static` method is now inherited: `Child.staticFromBase(..)` resolves the declaring class's body,
and a `trait`-supplied static is callable on the using class. Closes the B0 own-class-only limitation.
No new `Op`/`Value`. Research: `docs/specs/2026-06-28-statics-research-design.md`.

- The checker propagates inherited/trait static-method *names* through `merge_inherited` + the
  trait-`use` path (mirroring `methods`), so the `static_methods` gate accepts them; the interpreter's
  `call_static_method` resolves through the shared `method_origins` table (like `call_method`); the
  compiler's `class_method_origins` already aliased the dispatch entry. Byte-identical run≡runvm≡PHP.
- `examples/guide/static-inheritance.phg`; checker tests. **Deferred:** overloaded statics (the VM has
  no static-overload dispatch set) and late static binding (`static::`/`new static()` — a deliberate
  non-feature). An *instance* method called via the class name is still `E-STATIC-CALL`.

### Added — `Secret<T>` opaque wrapper (Fork B)

A type for sensitive values (passwords, API keys, tokens). No new `Op`/`Value`/`Ty` — an injected
generic class reusing the `Box<T>` machinery. Design: `docs/specs/2026-06-28-secret-type-design.md`.

- **Loud, by construction**: a `Secret` is not a string and has no display, so
  `Console.println(secret)` / `"{secret}"` is a **compile error**; the wrapped field is `private`, so
  `.expose()` is the only read path. (Chosen over a runtime-`***`-redacting wrapper, which would need
  a new `Value` variant + a *silent* `***` — loud beats silent.)
- **`import Core.Secret;`** injects `class Secret<T> { constructor(private T value){} expose(): T }`.
  `new Secret(x)` infers `Secret<T>`.
- **`W-SECRET` lint** (non-fatal, stderr) fires when `.expose()` is a *direct* argument to a sink
  (`Console.println`/`print`, `Core.File.write`). Syntactic on the direct argument; `phg explain W-SECRET`.
- **Transpiles** to a `final class Secret` whose constructor parameter carries `#[\SensitiveParameter]`
  (PHP redacts it in stack traces — the `K-secrets-type` intent). Byte-identical run≡runvm≡real PHP.
  Showcase `examples/guide/secret.phg`.

### Added — `Core.Regex` (Fork A) + 2nd vetted dependency

A ReDoS-safe regular-expression engine. No new `Op`, no new `Value` (the compiled value reuses the
injected-type + value-as-first-arg patterns). Design: `docs/specs/2026-06-28-core-regex-design.md`.

- **Engine = the `regex` crate** — the project's **2nd** external dependency (after `argon2`). A
  RE2-style finite automaton with **guaranteed linear-time matching (ReDoS-immune by construction)**,
  unlike PHP/PCRE backtracking. The dependency policy (`docs/specs/2026-06-27-dependency-policy.md`)
  is amended: clause 1 generalizes from "crypto" to "security-critical primitive — crypto **and**
  untrusted-input parsers (regex) where `std` has none and rolling-your-own is the anti-pattern."
  Feature-gated `regex` (default on; OFF for `phorge-playground`, like `crypto`).
- **`import Core.Regex;`** → `Regex.compile(string) -> Regex` (validate once, memoized; faults on an
  invalid/unsupported pattern), `matches`/`find`(→`string?`)/`findAll`(→`List<string>`)/`findGroups`
  (→`Map<string,string>?`, named captures)/`replace`/`split`. `Regex` is a compiler-injected class
  holding the bare pattern; always Unicode (`/u`), case-sensitive.
- **Byte-identity holds on the regular subset**: the crate's no-backref/lookaround feature set is
  exactly what PHP `preg_*` matches identically; unsupported patterns are rejected at `Regex.compile`.
  Transpiles to gated `__phorge_regex_*` helpers (collision-free delimiter + `preg_*`); `run ≡ runvm ≡
  real PHP 8.5`. Showcase `examples/guide/regex.phg`.
- **Patterns use raw strings** `r"..."` — the `{n}` quantifier would otherwise collide with `{expr}`
  string interpolation, and raw strings drop `\` double-escaping.

### Added — `phg fmt` formatter (M-fmt)

A canonical-form source formatter (GA rock 2 — daily-use tooling). No new `Op`, no new `Value`.

- **Comment side-channel** — `lex_with_comments()` collects comments (which the token stream drops)
  as `Comment{span,text,kind,own_line}`; `lex()` is unchanged.
- **Full-surface, meaning-preserving printer** (`src/fmt/`) — prints from the parsed AST (not by
  re-spacing tokens), so `parse(fmt(x))` can't change meaning; exhaustive matches make it
  compiler-proven complete over every Item/Stmt/Expr/Type/Pattern. Idempotent; comments preserved.
- **`phg fmt [--check] [path… | -]`** — in-place (writes only on change), `--check` (exit 1 if any
  file would change, no writes — the CI gate), stdin (`-`), recursive dir/no-path discovery. An
  unparseable file is left untouched (exit 2). A dogfood test formats every repo example and asserts
  behavior is preserved.
- v1 is *tidy + comment-safe* (canonical indentation/spacing/blank-lines, `->`→`:`); no line-wrapping.

### Added — `phg test` runner + `Core.Test` assertions (M-Test)

A first-class testing story so Phorge can dogfood itself (GA rock 2 — daily-use tooling). No new `Op`,
no new `Value`.

- **`test "name" { … }` items** — a contextual `test` keyword (special only at item position before a
  string literal, so it stays a usable identifier). A test body is checked like a `-> void` body (no
  `this`); a `test` block in a normal build is rejected as `E-TEST-OUTSIDE-TESTS` (`phg explain`).
- **`Core.Test` assertions** — `assert(bool, string)`, `assertTrue`/`assertFalse`, `assertEquals`/
  `assertNotEquals` (value equality via the shared `==` kernel; same-type-required, generic),
  `assertNull`/`assertNotNull`, and **`assertFaults(() -> T)`** (a HigherOrder native — passes iff the
  closure faults). A failing assertion raises a fault the runner catches per-test.
- **`phg test [path…]`** — discovers `*.phg` under the project's `tests/` (or a given file/dir), loads
  each through the normal loader, validates in test mode, and runs every `test` block independently on
  the interpreter (each body is lowered into a synthetic `main` and routed through the ordinary
  check/expand/interpret pipeline — no test-specific backend path). cargo-style report; exit `0` iff all
  pass. Runnable showcase under `selftest/`.

### Added — math breadth + number formatting (M-NUM S4) — closes M-NUM

The final M-NUM slice rounds out `Core.Math`. All additive stdlib natives — **no new `Op`, no new
`Value`**:

- **Integer helpers (byte-identical regardless of float display):** `sign(int) -> int` (→ PHP `<=>`),
  `clamp(int, int, int) -> int` (→ `max(lo, min(v, hi))`, never panics when `lo > hi`),
  `gcd(int, int) -> int`. `gcd` has no PHP-core builtin (gmp is absent under `php -n`), so it erases
  to a single-sourced **`__phorge_gcd`** helper (Euclid over the magnitudes); the `i64::MIN` magnitude
  edge faults cleanly (EV-7).
- **Transcendentals:** `log`/`log10`/`exp`/`sin`/`cos`/`tan(float) -> float` (→ the same-named PHP
  libm builtins) and the constants `pi()`/`e() -> float` (→ `M_PI`/`M_E`). A non-representable result
  diverges between Rust's shortest-round-trip and PHP, so the guide exercises them at their *exact*
  (IEEE-defined) values and prints real results through `numberFormat`.
- **`numberFormat(float, int) -> string`** — non-locale `number_format`: rounded half-away-from-zero,
  grouped by threes with `,`, `.` decimal point. Erases to a single-sourced **`__phorge_number_format`**
  helper (identical string assembly to `value::number_format`), so the PHP leg never relies on PHP's
  own `number_format` (its `-0`/locale quirks). A negative `decimals` clamps to `0` on both legs.

`examples/guide/math.phg` extended; byte-identical `run ≡ runvm ≡ real PHP 8.5`. **M-NUM is now
closed** (S1 decimal core → S2 division/rounding → S3 predicates/conversions → S4 math breadth);
`BigInt` / arbitrary-precision decimal / `Money`+currency remain deferred to **M-NUM-2**.

### Added — float predicates + numeric conversions (M-NUM S3)

Rounds out the numeric surface: detect float special values and convert **explicitly** between
`int`/`float`/`decimal` (Phorge has no implicit coercion). All additive stdlib natives — **no new
`Op`, no new `Value`** (reuses the native registry, S2's `Value::Null`/optionals, and S1's
`Value::Decimal`). Every primitive is PHP **core** (available under `php -n` — no extension):

- **`Core.Math` float predicates + special values:** `isNan`/`isFinite`/`isInfinite(float) -> bool`
  (→ PHP `is_nan`/`is_finite`/`is_infinite`); `nan`/`infinity`/`negInfinity() -> float`
  (→ `NAN`/`INF`/`-INF`). The predicates return `bool`, so they are byte-identical even for a
  non-representable float operand (the divergence is in float *display*, not in a `bool`).
- **`Core.Math.intdiv(int, int) -> int`** — integer division truncating toward zero (→ PHP `intdiv`);
  single-sourced with `value::int_intdiv`. A zero divisor faults `"division by zero"` and
  `intdiv(i64::MIN, -1)` faults `"integer overflow"` — both run≡runvm (FaultKind parity), PHP `intdiv`
  throws the matching class (not a runnable example).
- **`Core.Convert` numeric conversions:** `toFloat(int) -> float` (total widening; already present),
  `toInt(float) -> int?` (truncate toward zero; **null** on NaN/±∞/out-of-i64-range — avoids PHP's
  surprising `(int)NAN == 0`), `intToDecimal(int) -> decimal` (exact, scale 0),
  `decimalToFloat(decimal) -> float` (lossy by nature), `decimalToInt(decimal) -> int?` (truncate
  toward zero; null if the integer part is out of i64 range).

The edge-safe guards are **single-sourced** in `value.rs` (`float_to_int`, `decimal_to_int` — exact
i128-carrier math, no BCMath) and mirrored by gated PHP helpers `__phorge_float_to_int` /
`__phorge_dec_to_int`, so the float→int range verdict and the decimal→int truncation agree byte-for-byte
across `run`/`runvm`/real PHP. `int` is documented as a pinned 64-bit signed integer (i64) in
`docs/INVARIANTS.md`. Byte-identical `run ≡ runvm ≡ real PHP 8.5`; `examples/guide/numeric-convert.phg`.

### Added — decimal division + rounding (M-NUM S2)

Exact, **explicitly-rounded** decimal division — the precision-safe complement to S1's `+ - *`.
Bare `decimal / decimal` (and `decimal % decimal`) is now a **compile error** (`E-DECIMAL-DIV`):
division isn't exact, so an operator would have to silently pick a scale and a rounding rule — exactly
the hidden precision loss `decimal` exists to prevent. Division goes through two natives that name
both:

- **`Decimal.div(decimal a, decimal b, int scale, RoundingMode mode) -> decimal`** — the exact
  rational `a / b`, rounded to `scale` fractional digits under `mode`.
- **`Decimal.round(decimal d, int scale, RoundingMode mode) -> decimal`** — re-scale a decimal
  (exact up-scale, rounded down-scale).
- **`RoundingMode`** — a seven-variant enum (`HalfUp`, `HalfDown`, `HalfEven` banker's, `Up`, `Down`,
  `Ceiling`, `Floor`) **injected** when a program imports `Core.Decimal` (the same compiler-injected
  enum pattern as `Core.Json`); construct a mode with `new HalfUp()`.
- **Faults:** a zero divisor → `"decimal division by zero"`; a negative `scale` →
  `"decimal scale out of range"`; any i128 overflow in the intermediate → the existing
  `"decimal overflow"`. Byte-identical run≡runvm (FaultKind parity); the PHP helper throws the same.

The rounding kernel `value::round_div(n, d, mode)` is **single-sourced** (sign-normalise so `d > 0`,
truncating quotient + dividend-signed remainder, a half-comparison via `|rem|` vs `d − |rem|` to avoid
`2*rem` overflow, the seven mode rules, all `checked_*`). It is mirrored step-for-step by gated
BCMath helpers `__phorge_dec_div`/`__phorge_dec_round` (`bcdiv`/`bcmod` truncate toward zero / take
the dividend's sign — verified identical to Rust i128 `/`/`%`), switching on the `RoundingMode` value's
PHP class and reusing S1's `__phorge_dec_check` for the i128 bounds fault. **No new `Op`, no new
`Value`** — division is a `CallNative`, `RoundingMode` rides the existing enum ops. (Transpiler-only:
the injected enum's PHP class name is mangled `RoundingMode → RoundingMode_` to dodge PHP 8.4+'s
built-in `RoundingMode` enum.) Byte-identical `run ≡ runvm ≡ real PHP 8.5`; `examples/guide/decimal-div.phg`;
`phg explain E-DECIMAL-DIV`.

### Added — the `decimal` primitive (M-NUM S1)

An exact fixed-point **`decimal`** scalar primitive for money/fixed-point math — making
float-for-currency a *compile choice*, not a silent bug. Representation is `i128` fixed-point
(`Value::Decimal { unscaled, scale }`, value = `unscaled × 10^(-scale)`), std-only and covering all
realistic money. Surface:

- **Literals `19.99d`** — a numeric literal immediately followed by `d`; the scale comes from the
  literal **text** (`1.50d` ⇒ scale 2, `1.500d` ⇒ scale 3, `100d` ⇒ scale 0). An exponent (`1e3d`)
  is rejected and an i128-overflowing literal is a compile-time error — both `E-DECIMAL-LITERAL`.
- **`Decimal.of(string) -> decimal?`** (`import Core.Decimal;`) — parse the same grammar at runtime,
  `null` on malformed/overflow (composes with `??`).
- **`+ - *`** — exact, single-sourced in `value::decimal_add/sub/mul`: add/sub align to `max` scale,
  mul sums scales; any i128 overflow (incl. alignment) is a clean `"decimal overflow"` fault. Mixed
  **`decimal ⊕ int`** (either order) widens the int to a scale-0 decimal and stays `decimal`. A
  `decimal ⊕ float` mix is rejected (`E-DECIMAL-FLOAT-MIX`) — the bug this primitive exists to
  prevent. `/` and `%` are deferred to S2 (division + rounding).
- **Comparison / equality** — numeric, **scale-insensitive** (`1.50d == 1.5d` is true; `decimal`
  compares with `decimal` or `int`).
- **Unary `-`**, scale-padded rendering (`{1999,2}` → `"19.99"`, never `-0`).

Implementation: the literal rides the constant pool (**no new `Value`-kind/`Op` for it**); the VM
gains three type-specialized ops `AddD`/`SubD`/`MulD` (the three coupled matches — `chunk.rs`
`Op`+`validate`, `vm/exec.rs`, `compiler` emit). Compiler gains `NumTy::Decimal`/`CTy::Decimal` so a
decimal-valued field/map/method-result operand specializes on the VM. Transpiles to **BCMath**
(verified available under `php -n`): a literal → a PHP string, `emit_type(decimal)` → `string`,
arithmetic → gated `__phorge_dec_add/_sub/_mul` helpers that derive operand scales at runtime, call
`bcadd`/`bcsub`/`bcmul` with the rule's scale, then bounds-check the result against i128 range and
`throw` the same fault as Rust. `Decimal.of` → a gated `__phorge_dec_of` (tier-1 PCRE). Byte-identical
`run ≡ runvm ≡ real PHP 8.5`; `examples/guide/decimals.phg`;
`phg explain E-DECIMAL-FLOAT-MIX`/`E-DECIMAL-LITERAL`.

### Added — default parameter values + `Text.parseFloat` (M4)

A PHP-familiar language feature: a trailing parameter may declare a literal **default value**
(`function f(int x, int y = 10)`), making that argument optional at the call site (`f(1)` ≡
`f(1, 10)`). **No new `Op`/`Value` and no backend change** — a call that omits trailing defaulted
arguments is rewritten to full arity (provided args + the default literals) by the existing
call-rewrite pass (`rewrite_ufcs`), so the interpreter/VM/transpiler only ever see complete calls; the
default literal is identical on all three, so `run ≡ runvm ≡ PHP` holds by construction. Rules
(checker): defaults must be **trailing** (`E-DEFAULT-PARAM-ORDER`), **literal** (`E-DEFAULT-PARAM-EXPR`),
and **type-assignable** (`E-DEFAULT-PARAM-TYPE`); **free functions only** in v1 (a method/constructor
default is `E-DEFAULT-PARAM-CONTEXT` — a documented follow-up). Natives may declare defaults via a small
`native_defaults` lookup (no churn across the ~50 registry literals). `phg explain` documents all four
codes.

The motivating native lands with it: **`Text.parseFloat(string, bool permissive = false) -> float?`** —
parse a base-10 float, or `None`. `permissive` defaults to **strict**: `[+-]?digits(.digits)?(e±digits)?`
(accepts `1`, `1.5`, `-2.5e3`; rejects `.5`, `5.`, hex, surrounding whitespace). `parseFloat(s, true)`
additionally accepts a lone leading/trailing dot (`.5`, `5.`). **Both reject `inf`/`nan`** — Rust's
`f64::from_str` accepts them but PHP can't, and the float rendering would diverge, so rejecting keeps the
spine byte-identical. Rust is the value source of truth (grammar validator + `f64::from_str`); gated
`__phorge_parse_float` PHP helper mirrors it (PCRE, tier-1). `examples/guide/default-params.phg`.

### Added — `Core.List` / `Core.Text` / `Core.Set` breadth (M4 stdlib sweep)

A breadth pass over the collection + text modules, all additive natives (no new `Op`/`Value`),
byte-identical run/runvm/real PHP 8.5, each with a guide example:

- **`Core.List`**: `slice(xs, offset, len)` (PHP `array_slice`; negatives count from the end,
  out-of-range clamps to empty — the Rust kernel replicates the normalization), `indexOf(xs, x) ->
  int?` (gated `__phorge_index_of`, mapping `array_search`'s `false` to `null`), `concat(a, b)` (PHP
  `array_merge`), `first(xs)` / `last(xs) -> T?`. Each returns a fresh list (immutable). Example
  `examples/guide/list-ops.phg`.
- **`Core.Text`**: `padLeft` / `padRight(s, width, pad)` (PHP `str_pad`), `indexOf(s, needle) -> int?`
  (gated `__phorge_text_index_of`, from `strpos`), `substring(s, start, len)` (PHP `substr`). Byte-based
  / tier-1 (no mbstring) — ASCII domain; a slice/pad that splits a multibyte char faults cleanly (EV-7)
  rather than panicking. Example `examples/guide/text-ops.phg`.
- **`Core.Set`**: `union` / `intersection` / `difference(a, b) -> Set<T>` (PHP `array_unique(array_merge)`
  / `array_intersect` / `array_diff`); the result follows the first set's order. Example
  `examples/guide/set-ops.phg`.

### Added — `Core.Map` access + functional update (M4 stdlib breadth)

`Map<K, V>` was read-only (`keys`/`values`/`has`/`size` + faulting `m[k]`); these add access and
immutable update. `get(m, k) -> V?` is a **safe** lookup — the value when present, else `null` (so a
missing key is an optional, not a fault — composes with `??`/if-let; `V` is non-optional so `null`
unambiguously means "absent"). `set(m, k, v) -> Map<K, V>` and `remove(m, k) -> Map<K, V>` return a
**new** map (Phorge maps are immutable), insertion-ordered like PHP `$m[$k] = $v` / `unset($m[$k])` —
the `set` kernel reuses `value::map_set`. `get` erases inline (`($m[$k] ?? null)`); `set`/`remove` use
gated `__phorge_map_set`/`__phorge_map_remove` helpers (PHP arrays are COW value types, so the by-value
`$m` is already a copy). Byte-identical run/runvm/real PHP; `examples/guide/map-ops.phg`. **No new
`Op`/`Value`.**

### Added — the checked `as` downcast operator (M4 casting, axis 2)

`value as Type` is a **checked** downcast: it yields `Type?` — the value itself when it really is a
`Type` at runtime, else `null` (the Kotlin/Swift `as?` model, the honest form of TS's unchecked
`<T>v` — no lying to the compiler, no later crash). It composes with `??` (`(x as Circle) ?? d`) and
if-let smart-cast (`if (var c = v as Circle) { … c.radius … }`); the scrutinee may be a class,
interface, or union value, and the target a class or interface (a primitive target like `x as int` is
rejected — that's value *conversion*, the `Core.Convert` axis — with a hint, `E-CAST-TYPE`). `value`
is evaluated **exactly once** (the example bakes a side-effecting scrutinee into its byte-identity
gate to prove it). `as` is a *contextual* word (it also separates `foreach (xs as x)` and aliases
imports); a parser restriction keeps the foreach separator from being read as a cast, with brackets as
the escape. Lowers with **no new `Op`** — reuses `Op::IsInstance` + a branch on the backends (the
`??`/`$match` scratch-slot trick, so the operand isn't re-evaluated); transpiles to a PHP arrow-fn
IIFE `(fn($x) => $x instanceof T ? $x : null)($value)`. Byte-identical run/runvm/real PHP;
`examples/guide/as-cast.phg`; `phg explain E-CAST-TYPE`. **No new `Op`/`Value`.**

### Added — `Core.Convert` value conversion (M4 casting, axis 1)

Explicit value conversion — Phorge has no implicit coercion, so you convert on purpose, and lossy
conversions are *named* (no silent `(int)`). `Convert.toString(T) -> string` (generic, reuses the
`__phorge_str` rendering — bool→`true`/`false`, float→shortest-round-trip), `toFloat(int) -> float`
(total widening), `truncate(float) -> int` (toward zero), `round(float) -> int` (half away from zero).
Because UFCS ships, `Convert.toFloat(n)` ≡ `n.toFloat()` — module + method API in one. (The type
*cast*/reinterpret is the separate `as` operator, axis 2, next slice.) Byte-identical run/runvm/real
PHP; `examples/guide/convert.phg`. **No new `Op`/`Value`.**

### Added — `Core.List.sort` / `sortWith` (M4 stdlib breadth)

Ordering for lists, mirroring PHP `sort`/`usort`. `Core.List.sort(List<T>) -> List<T>` returns a new
list in natural ascending order (the input is unchanged — Phorge lists are immutable): ints/floats
numeric, strings **lexicographic by byte** (`"10"` before `"9"`) — deliberately *not* PHP's
numeric-string-juggling `<=>`, so the PHP helper dispatches to `strcmp` for strings to match Rust's
`String` ordering. `Core.List.sortWith(List<T>, (T, T) -> int) -> List<T>` orders by a comparator
closure (higher-order, reusing the `map`/`reduce` re-entrant machinery; a comparator fault propagates
cleanly). Both stable (Rust `sort_by` ≡ PHP 8.0+ `usort`); gated `__phorge_sort`/`__phorge_sort_with`
helpers; byte-identical run/runvm/real PHP. `examples/guide/sort.phg`. **No new `Op`/`Value`.**

### Added — `Core.Text.parseInt` (the first optional-return native)

`Core.Text.parseInt(string) -> int?` — `None` when the whole string is not a valid base-10 integer
(no partial parse, no overflow clamp), unlike PHP's lenient `(int)`. Mirrors Rust's `i64::from_str`
(optional sign, base-10 digits incl. leading zeros, in `i64` range, no surrounding whitespace);
composes with `??` / `if (var n = …)`. PHP erases to a gated `__phorge_parse_int` helper whose
overflow detection matches Rust's `None` (PHP's `(int)` would silently clamp). Byte-identical
run/runvm/real PHP (incl. `+5`/`007`/overflow). `examples/guide/parse-int.phg`.

### Added — `Core.Json` (JSON parse / stringify)

A std-only, deterministic JSON module over a compiler-injected `Json` enum (`Null`/`Bool`/`Int`/
`Float`/`Str`/`Arr`/`Obj`) — expressible now that generic enums + `Map` + `List` all ship. The enum
is injected (head of `cli::check_and_expand`) only when a program `import Core.Json`s, then flows
through every backend as an ordinary enum.

- `Core.Json.parse(string) -> Json?` (None on malformed), `stringify(Json) -> string` (compact,
  matches `json_encode`), `stringifyPretty(Json) -> string` (4-space, matches `JSON_PRETTY_PRINT`).
- **PHP-faithful numbers:** `parse("42")` → `Int`, `"42.0"`/`"1e3"` → `Float` (mirrors `json_decode`;
  an `i64` overflow falls back to `Float`). Objects preserve `Map` key order; duplicate keys keep
  first position / last value (PHP assoc semantics). Strings escape to match `json_encode`'s default
  (`\/`, `\uXXXX` non-ASCII, surrogate pairs).
- **No new `Op`/`Value`:** three `Pure` natives; the one `eval` body is shared by both Rust backends,
  the PHP leg uses gated `__phorge_json_*` recursive helpers. Floats render via the positional
  shortest-round-trip form (`format!("{}")`/`__phorge_float`), so `run ≡ runvm ≡ real PHP 8.5` is
  byte-identical. `examples/guide/json.phg`.

### Added — PHP-reserved enum variant names are mangled in the transpiler

A variant named after a PHP-reserved class word (`Int`/`Float`/`Bool`/`Null`/…) now transpiles to a
mangled PHP class name (`Int` → `Int_`) at the declaration, `new`, and `instanceof` sites, instead of
emitting an invalid `final class Int`. Transpiler-only (the backends address a variant by its Phorge
name), so stdout byte-identity is untouched; reusable for any enum and load-bearing for the clean
`Core.Json` variant API. `examples/guide/enum-reserved-variants.phg`.

### Changed — `E-RESERVED-NAME` now guards the full PHP-reserved-word set (F-m)

The reserved-symbol-name check (previously `var`-only) now rejects every PHP-reserved word that is a
usable Phorge identifier but would transpile to an invalid PHP symbol — turning a latent PHP-oracle
parse error into a clean Phorge diagnostic. **Kind-aware** (empirically verified vs PHP 8.5): a
`function` is checked against the function-illegal set (`var`/`list`/`print`/`array`/`unset`/`empty`/
`eval`/`echo`/`clone`/`callable`/…), a `class`/`enum`/`interface`/`trait` additionally against the
type words (`int`/`float`/`bool`/`string`/`object`/`readonly`/…) — so a `function int()` stays legal
(legal PHP function name) while `class int {}` is rejected. All remain usable as value / parameter /
field / method names. `phg explain E-RESERVED-NAME`.

### Changed — `var` is now a contextual keyword

`var` was a hard-reserved keyword, so it could not be used as an identifier — naming a parameter,
field, or variable `var` was a parse error, and lifting PHP `$var` produced invalid Phorge. `var` is
now **contextual** (like `foreach`/`as`/`when`): it is the inference-binding keyword only at a
declaration start (`var x = …`, `var [a, b] = …`, struct destructure, `if (var x = opt)`), and an
ordinary identifier everywhere else. The change is **purely additive and backward-compatible** — every
existing program parses identically; only previously-rejected positions are now accepted.

- `var` is usable as a **variable / parameter / field / property / method** name (it maps to a legal
  PHP `$var` / `->var` / `->var()`, verified against PHP 8.5). Mutability stays the orthogonal
  `mutable` axis — `var` carries no mutability meaning.
- Naming a **free function / class / enum / interface / trait / type** `var` is rejected with the new
  **`E-RESERVED-NAME`** (PHP reserves `var` in those symbol positions — `function var(){}` / `class
  var{}` are PHP parse errors; `phg explain E-RESERVED-NAME`).
- Front-end-only (lexer keyword table + parser dispatch + one checker guard); **no new `Op`/`Value`**,
  byte-identical `run ≡ runvm ≡ real PHP 8.5`. Unblocks lifting PHP `$var` → Phorge `var` verbatim.
  `examples/guide/contextual-var.phg`.

### Added — `this`-capture in closures (Phase 1 closures slice)

A method-body lambda may now reference `this`: `function reader() -> (() -> int) { return fn() =>
this.n; }`. The receiver is captured **live** (the same instance handle), so a field write made after
the closure is built is visible when it runs. Byte-identical `run ≡ runvm ≡ real PHP 8.5`; **no new
`Op`/`Value`** — `this` rides the existing value-capture path (interpreter: a `this_capture` on the
tree closure; VM: an implicit first capture at the sub-frame's slot 0; PHP: arrow-fns auto-bind `$this`).

- The `E-LAMBDA-THIS` guard is **narrowed to field/static initializers only** — a field-default lambda
  may not capture `this` (the instance is only partially built when an initializer runs). `this`-capture
  also threads through nested lambdas and into closures passed to higher-order natives (`List.map`).
  `examples/guide/closures-this.phg`.

### Added — fixed-length lists `[T; N]` (Phase 1 types slice)

`[int; 3] rgb = [255, 128, 0];` — a `List<T>` whose length is a compile-time constant. Byte-identical
`run ≡ runvm ≡ real PHP 8.5`; **no new `Op`/`Value`** — at runtime a `[T; N]` *is* a list (erases to a
PHP array); the length is a compile-time-only guarantee.

- **Checker-only distinction:** the length is tracked, a list-literal initializer must have exactly `N`
  elements (`E-FIXEDLIST-LEN`), a *literal* index is bounds-checked at compile time (`pair[5]` on
  `[int; 2]` is `E-FIXEDLIST-BOUNDS`; a dynamic index falls back to the runtime check), and `[T; N]` is
  assignable **to** `List<T>` (a fixed list is a list) but not the reverse (a list has unknown length).
- **Element-set** `pair[i] = e` is allowed on a `mutable` fixed list (length-preserving). Erases to a
  PHP array everywhere (`emit_type` → `array`, `CTy::List` so `pair[i]` specializes as an operand).
  `examples/guide/fixed-lists.phg`. The irrefutable-destructuring payoff (`var [a, b] = pair`) arrives
  with let-destructuring (slice 5).

### Fixed — parenthesized function type in return position (Phase 1 types slice)

`function f() -> ((int) -> bool) { … }` now parses. Previously a `(` in type position was always read
as a function-type parameter list demanding a following `->`, so an explicitly parenthesized function
type in return position failed (only the parens-free right-assoc `() -> (int) -> bool` worked — both now
parse to the same type). A `(` is now disambiguated by whether a `->` follows the `)`: with `->` it's a
parameter list, without it it's a **grouped** type `(T)` ≡ `T` (Phorge has no tuples — `()`/`(A, B)`
without `->` are parse errors). Parser-only; byte-identical (`examples/guide/lambdas-pipe.phg`).

### Added — or-patterns in `match` (Phase 1 operators slice)

`match n { 1 | 2 | 3 => "low", _ => "hi" }` — group alternatives that share one arm body with `|`.
No fall-through, still exhaustive (each alternative discharges its own shape). Works for literals and
enum variants. Byte-identical `run ≡ runvm ≡ real PHP 8.5`; **no new `Op`/`Value`, no backend change**.

- **Front-end only:** the parser collects `|`-separated alternatives and **desugars** them to one arm
  per alternative (sharing the cloned body + guard), so every backend sees ordinary arms —
  exhaustiveness, duplicate-arm (`W-MATCH-UNREACHABLE`), and flow-narrowing all work unchanged.
- **Restriction:** alternatives must be **binding-free** — no `_`, no bare name, no variable-binding
  sub-pattern (`Some(_) | None()` is fine; `Some(n) | None()` is `E-OR-PATTERN-BIND`), since the shared
  body cannot know which alternative matched. Split into separate arms if you need to bind.
  `examples/guide/pattern-matching.phg`.

### Added — `**` power operator + `Math.ipow` (Phase 1 operators slice)

`2 ** 10`, `2.0 ** 3.0`, `Math.ipow(5, 2)`. The `**` operator is **type-directed** (`int ** int → int`,
`float ** float → float`), **right-associative**, and binds tighter than `* / %` — PHP-identical.
Byte-identical `run ≡ runvm ≡ real PHP 8.5`; **no new `Op`/`Value`**.

- **Lowering:** the compiler lowers `**` to an `Op::CallNative` to `Core.Math.ipow`/`pow` (resolved at
  compile time — no `import Core.Math` needed). Both the interpreter's `**` arm and the native call the
  single-sourced `value::int_pow`/`float_pow` kernels, so the two Rust backends compute and fault
  identically. The transpiler emits PHP's native `**` (compound operands parenthesized, so `-a ** 2` is
  `(-$a) ** 2` = `(-a)**2`, matching Phorge rather than PHP's default `**`-before-unary-minus).
- **Semantics:** integer power is overflow-checked; a negative exponent faults (`negative exponent`)
  rather than widening to a float — use `float ** float` for fractional powers. `Math.ipow(int, int) ->
  int` is the named, value-level twin (`Math.pow` stays the float power). `examples/guide/operators.phg`.

### Changed — mandatory `new` for construction (Feature C, breaking)

Every class instantiation and enum-variant construction now **requires** `new`: `new Counter()`,
`new Some(7)`, `new Circle(2.0)`. One uniform rule (a deliberate Phorge departure — no surface
language `new`s a sum-type variant). Byte-identical `run ≡ runvm ≡ real PHP 8.5`; **no new
`Op`/`Value`/backend change**.

- **Front-end only:** the parser wraps a construction in `Expr::New`; the checker validates it
  (`E-NEW-REQUIRED` for a bare construction, `E-NEW-ON-NONCONSTRUCT` for `new` on a free function /
  value — both `phg explain`-documented) then a new `checker::unwrap_new` pass strips `Expr::New` to
  its inner `Call` (alongside `expand_aliases`/`erase_generics`/`resolve_html`) **before any backend**,
  so construction semantics and the byte-identity spine are untouched. The project loader's
  cross-package resolution pass also descends into `Expr::New` (so `new Rect(…)` mangles to
  `new \Acme\Geometry\Rect(…)`).
- **Migration:** `phg rewrite-new <file>` — an AST-span codemod that wraps every class/variant
  construction (patterns and free-function calls are left untouched; idempotent). Applied across all
  examples, projects, and the test corpus. Match patterns (`Some(n) =>`), enum-variant *declarations*,
  and the raw `lex→parse→interpret` test path keep bare names.

### Added — runtime static field initializers (Feature B-static)

`examples/guide/static-init.phg`; byte-identical `run ≡ runvm ≡ real PHP 8.5`. No new `Op`/`Value`.

- **`static TYPE name = <expr>;`** — a static field may now carry an **arbitrary** expression (a call,
  arithmetic, a read of an earlier static), lifting PHP's constant-expression-only static-property
  restriction. Evaluated **once at program start, in declaration order, before `main`** (eager — the
  decided model; lazy + runtime config were rejected, see the master-plan Decisions Log). A literal
  static still works and stays a plain PHP `static $x = <lit>;` default.
- **Lowering:** the interpreter evaluates non-literal statics in `eval_static_inits` (after collect,
  before `main`); the compiler emits a `SetStatic` prelude at the start of `main` (literals stay seeded
  in `static_inits`, non-literals get a `Unit` placeholder); the transpiler declares a non-literal
  static without a PHP default and sets it in a generated `__phorge_init_statics()` called before
  `main()`. The static-init type-check moved to a post-collection checker pass (`E-STATIC-INIT-TYPE`),
  so an initializer may reference a function or another static; the literal-only `E-STATIC-INIT-CONST`
  is retired.
- **Deferred** (KNOWN_ISSUES): static-init mode is fixed (eager) — configurability is an M13 edition
  flag (compile-time only); a static initializer reading a *later* static, and trait static fields with
  non-literal initializers, are not guarded this slice.

### Added — expression field initializers (Feature B, instance)

`examples/guide/field-init.phg`; byte-identical `run ≡ runvm ≡ real PHP 8.5`. No new `Op`/`Value`.

- **`TYPE name = <expr>;` on an instance field** — lifts PHP's constant-expression-only property
  defaults (PHP forbids calls/`$this`/other-property reads — "Constant expression contains invalid
  operations"). Phorge allows **any** expression (calls, closures, arithmetic, `this`/sibling reads),
  evaluated **per-instance at construction in declaration order, after the promoted ctor params are
  bound and before the constructor body**.
- **Declaration-order scope** — an initializer may read `this` and any **earlier-declared** field (or
  a promoted param); a later/self reference is `E-FIELD-INIT-FORWARD-REF`. A field-default closure
  that captures `this` is rejected by the existing `E-LAMBDA-THIS` (this-capture defers to the
  closures slice); a non-capturing closure default is fine.
- **Lowering** — the shared `ast::field_initializers` (the own initializers of the class whose
  constructor PHP actually invokes — PHP doesn't auto-chain `parent::__construct`) drives all three
  backends: the interpreter sets each field after promotion, the compiler emits `SetField`, and the
  transpiler prepends `$this->f = <expr>;` to the constructor prelude (synthesizing a `__construct`
  when the class has field initializers but no constructor). New codes `E-FIELD-INIT-FORWARD-REF`,
  `E-FIELD-INIT-TYPE` (both `phg explain`-documented).
- **Deferred** (KNOWN_ISSUES): a static field still takes a literal-only initializer (Feature B-static
  lands next); inherited field initializers run via PHP's single-constructor inheritance, matching the
  Rust backends, but cross-class chaining of multiple ancestors' initializers is not synthesized.

### Added — `const` class constants (Feature A)

`examples/guide/constants.phg`; byte-identical `run ≡ runvm ≡ real PHP 8.5`. No new `Op`/`Value`.

- **`[visibility] const TYPE NAME = <literal>;`** — a compile-time, immutable, class-level constant
  with member visibility (`public` default / `private` / `protected`), accessed **class-name-only**
  (`ClassName.NAME`, never through an instance). Names are SCREAMING_SNAKE_CASE.
- **Inlined on the Rust backends, idiomatic on PHP** — the shared `ast::class_consts` table (with
  inheritance + trait consts flattened, own/nearer wins) feeds all three backends: the interpreter
  returns the literal `Value`, the compiler emits `Op::Const` (+ a `CTy` so `MAX + 1` specializes —
  the CTy-operand discipline), and the transpiler emits a PHP **typed class constant**
  (`public const int MAX = 100;`, 8.3+) accessed as `ClassName::MAX` (no `$`).
- **Inheritance** — a subclass reads an inherited constant via its own name (`Sub.MAX`), matching PHP.
- **Visibility is enforced at the access site** (the one place Phorge checks member visibility) —
  required because the transpiled PHP `private const` would otherwise diverge from the Rust backends.
- New diagnostics (all `phg explain`-documented): `E-CONST-NO-INIT`, `E-CONST-NOT-LITERAL`,
  `E-CONST-MUTABLE`, `E-CONST-INIT-TYPE`, `E-CONST-CASE`, `E-CONST-VISIBILITY`,
  `E-CONST-INSTANCE-ACCESS`, `E-CONST-REASSIGN`.

### Added — Language Evolution Phase 1 (string slice): `+` concat, `\u{}`, literal braces, raw strings

`examples/guide/strings-ext.phg`; all byte-identical `run ≡ runvm ≡ real PHP 8.5`.

- **String concatenation with `+`** — `string + string` → `string`, type-directed with **no
  coercion** (`"x" + 1` is a compile error, killing JS's `"1" + 1` footgun). Only `+` concatenates;
  `-`/`*`/`/`/`%` stay numeric. Reuses `Op::Concat(2)` on the VM (new `CTy::Str` so a string operand
  is recognized — no new `Op`); transpiles via a new `__phorge_add` runtime helper (`is_string ? . :
  +`, since PHP's `+` is numeric-only).
- **`\u{HEX}` Unicode escapes** — 1–6 hex digits naming a codepoint, expanded to UTF-8 bytes at lex
  time (independent of i18n string indexing).
- **Literal braces `\{` / `\}`** — a literal brace inside an interpolated string (`"\{a {n} b\}"` →
  `{a … b}`). The interpolation split moved into the lexer (`TokenKind::Str` now carries pre-split
  literal/interpolation segments) so a `\{` literal brace is never confused with an interpolation
  brace — a flat parser-side split couldn't tell `\{` from `\\{`.
- **Raw strings `r"…"` / `r#"…"#`** — every byte literal, no escapes, no interpolation (JSON, regex,
  templates); a Rust-style `#`-run delimiter makes embedded `"` expressible.

### Added — Language Evolution Phase 0: `void`/`Empty` + mandatory return types

The foundation slice for the language-evolution roadmap
(`docs/plans/2026-06-24-language-evolution-master.plan.md`). Two front-end-only changes, byte-identical
`run ≡ runvm ≡ real PHP 8.5`.

- **S0a — the two-type "nothing" model.** Replaced the implicit `Ty::Unit` with `void` (the common,
  *uncapturable* nothing — the implicit + side-effect return type) and `Empty` (the rare *holdable*
  nothing — a real type a caller may bind). The one widening edge `void <: Empty` keeps it ergonomic.
  New `E-VOID-CAPTURE` (binding a void value, unless annotated `Empty`). Transpiles `void` → PHP
  `: void`, `Empty` → a hint-less PHP function (capturable `null`). `examples/guide/void-empty.phg`.
- **S0b — mandatory return types.** Every named function, method (incl. `abstract` + interface
  signatures), and statement-body lambda must declare a return type (`E-MISSING-RETURN-TYPE`),
  **including `main`**. Expression-body lambdas (`fn(x) => e`) keep inferring (the `=>` form's whole
  point; PHP arrow fns carry no return type). Constructors and property hooks are exempt. A repo-wide
  codemod (`tools/return_type_codemod.py`, a balanced-paren scanner) annotated every existing function
  with `-> void`. Both new error codes self-document via `phg explain`.

## [0.5.0-alpha.1] - 2026-06-24

First tagged pre-release. Rolls up all work since the internal 0.4.0 mark: M3 + the full M-RT
rich-type system (instanceof, interfaces, Map/Set, generics-all, unions, intersections, overloading,
inheritance, traits), the three-tier error model, M5 packages + git deps, M2.5 cross-OS `phg build`,
M6 web (partial), the pattern cluster + primitives sweep, and the WASM playground. All backends remain
byte-identical (`run ≡ runvm ≡ real PHP 8.4`). Pre-release: APIs and surface may still change before 1.0.

### Added — WASM playground (DX)

A free, zero-backend browser playground (`playground/`), auto-deployed to GitHub Pages on every push
to `master` so the live site always runs the latest `phg`. Spec
`docs/specs/2026-06-24-playground-wasm-design.md`, plan `docs/plans/2026-06-24-playground-wasm.plan.md`.

- New `phorge-playground` **workspace member** (cdylib): thin `#[wasm_bindgen]` exports over plain,
  native-testable `*_json` wrappers (`check`/`run`/`runvm`/`transpile`/`explain`) that bypass
  `on_deep_stack` (no threads on wasm) and call the public pipeline directly. The core `phorge` crate
  is unchanged — still dependency-free + `#![forbid(unsafe_code)]`; `wasm-bindgen` is a wasm32-only dep
  confined to the member. New `cli::parse_program` seam for non-aborting diagnostics. 9 native tests.
- Browser frontend (CodeMirror 6 + a Web Worker with a runaway-program timeout): all three backends
  live — `run`, `runvm`, transpiled-PHP **source**, and that PHP **executed in-browser** (php-wasm,
  PHP 8.4) — with a 3-way agreement badge / diff-on-mismatch. Examples picker (from `examples/guide/`),
  shareable permalink (source in the URL hash, browser-native compression), and clickable `phg explain`
  diagnostics.
- `.github/workflows/playground.yml` builds the wasm + deploys to Pages (additive to `ci.yml`).

### Added — Pattern cluster (M-RT S5) + primitives sweep

Post-M-RT language-ergonomics, front-end-only (no new `Op`, no `Value` change), byte-identical
`run ≡ runvm ≡ real PHP 8.4`. Plan `docs/plans/2026-06-23-pattern-cluster.plan.md`.

- **Match-arm guards** (S5.1): `pat when <cond> => …` (contextual `when`); a guarded arm does not
  discharge its shape for exhaustiveness (`E-MATCH-GUARD-EXHAUST`); non-bool guard `E-GUARD-TYPE`.
- **Struct destructuring** (S5.2): `Pattern::Struct` — shorthand `Point { x, y }`, rename
  `Point { x: px }`, full nesting `Line { from: Point { x, y }, to }`; reuses `Op::IsInstance` + field
  reads. Plus **nested type patterns in variant payloads** (`W(Circle c)`); a refutable payload no
  longer falsely discharges exhaustiveness (also closed the `Some(0)`-alone gap). Codes
  `E-STRUCT-PAT-TYPE` / `E-STRUCT-FIELD-UNKNOWN` / `E-PATTERN-DUP-BIND`.
- **Flow-narrowing** (S5.3): `narrow_from_condition` — `instanceof` then/else (else narrows a union to
  its remaining members), `!`/`&&`/`||` composition, and **early-return guards** narrow the rest of a
  block. Checker-only. Plus **if-let `when` guards** (`if (var x = e when g)`), parser-desugared to a
  nested `if` (no `Stmt::If.guard` field).
- **Primitives sweep**: number-literal formats (`0xFF`/`0b1010`/`0o17`/`1_000`/`1e3`), bitwise
  `& | ^ ~ << >>` (int-only; `>>` is two adjacent `Gt`, never a token), `Console.print` (no newline),
  and a byte-safe stdlib subset (`Text.startsWith`/`endsWith`/`repeat`, `Math.round`, `List.length`).

### Changed — M-Decomp: behavior-preserving codebase decomposition

The whale source files were split into cohesion sub-modules — **zero behavior change** (the
`run ≡ runvm ≡ real PHP 8.4` byte-identity spine is the proof; 823 tests green throughout, every
wave its own commit). Plan `docs/plans/2026-06-23-decomposition-milestone.plan.md`, design
`docs/specs/2026-06-23-decomposition-milestone-design.md`, module map in `docs/ARCHITECTURE.md`.

- **Axis = hybrid by-phase** (cohesion sub-files inside one `mod`), not by-construct: the three
  coupled exhaustive `Op` matches (`vm::exec_op`, `chunk::validate`, `compiler::stack_effect`) stay
  **whole** — verified by a dummy-`Op`-variant smoke check (all three fail to compile, then reverted).
- **Mechanism:** splits live inside one module so child files see the parent struct's private
  fields/methods; moved inherent methods take `pub(super)`, **nothing crate-public widens**.
- **`checker/`** 9786→454 (mod.rs): `resolve`/`collect`/`throws`/`program`/`casing`/`stmt`/`expr`/
  `calls`/`assign`/`matches`/`common`. **`parser/`** 1934→199: `exprs`/`stmts`/`items`/`types`/
  `patterns`. **`ast/`** 1465→669: `walk`/`classes`. **`loader/`** 1220→588: `resolve`/`fs`.
  **`compiler/`** 2967→740 · **`transpile/`** 2407→355 · **`interpreter/`** 1757→612 · **`vm/`**
  915→322 (`exec`/`closure`). No source file exceeds ~1500 lines; `lexer/` and `chunk.rs` left single.
- **Tests mirror the split** as sealed child modules — **by language feature** for `checker/tests/`
  (cross-cutting integration tests through `check()`) and **by construct** for `parser/tests/`.

### Added — M-RT S8: traits (`trait` / `use`) — M-RT CLOSED

Horizontal code reuse via `trait T { … }` composed by a class with `use T;` (design
`docs/specs/2026-06-23-m-rt-s8-traits-design.md`, plan `docs/plans/2026-06-23-m-rt-s8-traits.plan.md`).
A trait is **reuse, not a type** (`use` = has-the-behavior-of, vs `extends` = is-a): a value can never
be typed as a trait and `instanceof Trait` is rejected. Trait members flatten into the using class
**before any backend** (the interpreter/VM see ordinary members); the transpiler reconstructs a native
PHP `trait` + `use`. Byte-identical `run ≡ runvm ≡ real PHP 8.4`; `examples/guide/traits.phg`.

- **Members (maximal set):** methods with any visibility (incl. `private`); `mutable` instance fields
  (set via the using class's ctor) and `static` fields (a **per-using-class copy**, PHP `use`
  semantics); a trait **constructor** (promotion + body) adopted by a using class with no ctor of its
  own; an **abstract requirement** the using class must satisfy (reuses `E-ABSTRACT-UNIMPL`); and
  **property hooks** (`get`/`set`, PHP 8.4 hooks in a trait).
- **Constructor folding:** a trait ctor folds into `ctor_plan` (the single source for all three
  backends) and **wins over an inherited parent ctor** (PHP P2). Footguns become clean ahead-of-time
  diagnostics: `E-TRAIT-CTOR-COLLISION` (two trait ctors), `W-TRAIT-CTOR-SHADOWED` (class ctor wins,
  P1), `W-TRAIT-CTOR-PARENT-SKIPPED` (parent ctor not auto-run, P2).
- **Syntax:** `use T;` is disambiguated from an S6b `use P.m` resolution clause by **dot-lookahead**
  (a `.` after the name = resolution clause). New codes `E-USE-UNKNOWN` / `E-USE-AS-TYPE`; all new
  codes self-document via `phg explain`. **No new `Op`** — traits are front-end + native PHP.
- Closes **M-RT (Rich Types)**: `instanceof` → interfaces → Map/Set → generics-all → unions →
  intersections → totality → overloading → S6 inheritance → **traits**.

### Changed — package/namespace reshape COMPLETE: PascalCase everywhere + `package Main` (slices 2b + 3)

The package model's casing reshape is finished (design `docs/specs/2026-06-20-package-namespace-reshape-design.md`).

- **`E-PKG-CASE`** — package-declaration segments, import path segments, and import `as` aliases must be
  PascalCase (`package Acme.StringUtil;`, `import Acme.StringUtil as Strutil;`), joining the existing
  `E-NAME-CASE`/`E-TYPE-CASE` casing family. This makes the source→PHP-namespace mapping 1:1 with no
  casing transform (`Acme.Convert` ⇒ `Acme\Convert`). The reserved roots `Main` and `Core` are already
  PascalCase; an empty package stays `E-NO-PACKAGE` (no double-report). `phg explain E-PKG-CASE` added.
- **Reserved entry `package main` → `package Main`** — casing-consistent (spec D2); the entry *function*
  `main()` stays camelCase (a value identifier).
- **Migration**: every example, multi-file project, vendored dependency, and test fixture moved to
  PascalCase packages/folders. Distributable coordinates (manifest `module`, `[require]` keys, vendor
  directories, lockfile `name`) stay lowercase — concept C, separate from the namespace.
- **Output-preserving** (the loader's `pascal()` already PascalCased segments for PHP), so
  `run≡runvm≡real PHP 8.4` stayed byte-identical throughout; the differential harness was the safety net.
- Earlier slices: slice 1 (manifest `module`), slice 2a (identifier casing), slice 4 (library types /
  `E-PKG-TYPE` lifted) had already landed. **The reshape is now closed.**

### Added — multiple inheritance: `extends A, B` with explicit resolution (M-RT S6b)

A class may inherit from several parents at once (`class C extends A, B`). Cross-parent method
collisions are never silent: they must be resolved explicitly, and the whole feature is byte-identical
across the interpreter, the VM, and transpiled PHP 8.4 (`examples/guide/inheritance-multi.phg`).

- **Dispatch is single-sourced** through `ast::class_method_origins` — one resolved
  `(class, name) → (declaring class, method)` table both backends consume (the interpreter looks it up;
  the compiler aliases its bytecode method-table entry to it). This replaced the prior split where the
  interpreter walked only the first-parent chain while the compiler BFS-flattened every parent — a
  latent `run`≠`runvm` divergence on any method inherited from a non-first parent.
- **Resolution clauses** in the class body: `use P.m` (pick a parent's method for the colliding name),
  `rename P.m as n` (keep both, the renamed one under a fresh name), `exclude P.m` (drop one). An
  unresolved collision is `E-MI-CONFLICT`. A **diamond** shared base auto-merges (a method reached
  identically through two arms is never a conflict).
- **`abstract` classes & methods**: an `abstract class` cannot be instantiated
  (`E-ABSTRACT-INSTANTIATE`); a concrete subclass must implement every abstract method it declares or
  inherits (`E-ABSTRACT-UNIMPL`); an abstract method is implicitly `open`; `open static` is rejected
  (`E-OPEN-STATIC`, statics aren't virtual).
- **No new `Op`, no `Value` change** — all composition, collision detection, and resolution happen in
  the checker/AST before any backend runs (the same front-end-only discipline as `erase_generics`).
- **Transpile**: PHP has no multiple inheritance, so each parent lowers to an `interface I<name>` +
  `trait T<name>`; a multi-parent class emits `class C implements I…, I… { use T…, T… { …insteadof/as… } }`
  and each decomposed ancestor also gets a concrete `class <name> implements I<name> { use T<name>; }`.
  Resolution clauses become `insteadof`/`as`; the diamond shared base auto-dedups in PHP.
- New diagnostics self-document via `phg explain`: `E-MI-CONFLICT`, `E-ABSTRACT-INSTANTIATE`,
  `E-ABSTRACT-UNIMPL`, `E-OPEN-STATIC` (plus S6a's `E-EXTEND-FINAL`/`E-OVERRIDE-FINAL`/`E-MI-CYCLE`).

### Added — method & function overloading: dynamic multiple dispatch (M-RT)

Several free functions or class methods may share a name with distinct parameter signatures. Phorge
overloading is **dynamic multiple dispatch**: the *runtime* types of the arguments select the
most-specific matching overload — identically in the interpreter, the VM, and the transpiled PHP, so
a program runs byte-identically on all three (`examples/guide/overloading.phg`). This is the
spine-safe, surprise-free realization of overloading (no Java-style static-supertype footgun) and
matches what a PHP developer hand-writes (`if (is_int($x)) … elseif (is_string($x)) …`).

- **Selection** lives in `src/dispatch.rs` (shared by both backends): a `ParamKind` runtime summary
  of each parameter type, and `select_overload` (most-specific-wins). A class subtype beats its
  supertype; primitives are disjoint. An ambiguous (cross-cutting multi-argument) or unmatched call
  is a clean, byte-identical runtime fault.
- **One new `Op::CallOverload(set_id, argc)`** for overloaded free-function calls; overloaded
  *methods* reuse `Op::CallMethod` (no second new op) via a `method_overloads` table. Both consult a
  shared `overloads` dispatch table on `BytecodeProgram`.
- **Checker** treats a name as an overload *set* (`E-OVERLOAD-RETURN` — all overloads share a return
  type; `E-OVERLOAD-DUPLICATE` — no two identical signatures; `E-OVERLOAD-GENERIC` — a generic
  declaration can't be overloaded; `E-OVERLOAD-NO-MATCH`; `E-OVERLOAD-FN-VALUE` — an overloaded
  function has no single first-class value). All self-document via `phg explain`.
- **Transpile**: each overload body emits under a mangled `<name>__ovl_<i>`; one PHP dispatcher under
  the original name selects with an `is_*`/`instanceof` chain, branches ordered most-specific-first.

Scope: free functions + class methods. **Deferred** (KNOWN_ISSUES): overloaded constructors; a union
return type; compile-time ambiguity detection (today an ambiguous call faults at runtime); generic
overloads; and two PHP-erasure limits — overloads differing only by `string`-vs-`bytes` or among
`List`/`Map`/`Set` can't be told apart in PHP (both erase to `string`/`array`), and an ambiguous call
faults in the backends while the PHP chain would take the first match (faulting input only).

### Added — error model Slice 2c: exception cause chains (M-faults)

Closes the M-faults exception tier. A conventional **`cause` field of type `Error?`** on an `Error`
subtype preserves the lower-level error that triggered a higher-level one. On transpile it is routed
into PHP's native exception chain — `parent::__construct($message, 0, $cause)` — so the generated PHP
reports an idiomatic "caused by" via `getPrevious()`, while the Phorge backends read it back as an
ordinary field. Byte-identical `run ≡ runvm ≡ real PHP` (`examples/guide/cause-chain.phg`);
**transpiler-only — no new `Op`, no backend or checker change** (a `cause` field already round-tripped
as a plain field; 2c adds the native-chain routing + a `?\Throwable` property type so the `Error` marker
is not mistaken for PHP's unrelated engine `Error` class). Recognition is gated on field name + marker
type, so a mis-typed or non-`Error` `cause` stays a plain field. The remaining interop pieces — reading
a *foreign* exception's cause via `getPrevious()` and catching PHP-thrown exceptions — fold into PHP
interop (M8.5), which does not exist yet.

### Added — error model Slice 2b: checked exceptions (`throws`/`throw`/`try`/`catch`/`finally`) (M-faults)

The enforced exception tier of the three-tier error model. Byte-identical `run ≡ runvm ≡ real PHP`
(`examples/guide/errors.phg`); **three new `Op`s** (`Throw`/`PushHandler`/`PopHandler`), each extending
the three coupled matches (`chunk.rs` validate + `vm.rs` exec_op + `compiler.rs` stack_effect) in one
change.

- **`throws E` declarations + compile-time enforcement** — a function declares the checked exceptions it
  may raise (`throws A | B`, a set). Every `throw` and every call to a throwing function must be
  *discharged*: caught by an enclosing `try`, or propagated with `?` and a matching enclosing `throws`.
  A throwable type must implement the built-in **`Error`** marker; `throws Error` is too broad
  (`E-THROWS-TOO-BROAD` — declare the specific type); `main` may not let an exception escape
  (`E-UNCAUGHT-THROW`). New codes `E-THROW-TYPE`/`E-THROW-UNDECLARED`/`E-CALL-UNHANDLED`/`E-CATCH-TYPE`
  and the `W-CATCH-UNREACHABLE` lint, all self-documenting via `phg explain`.
- **`throw e;`** unwinds to the nearest matching `catch`. **`try { } catch (T e) { } … [finally { }]`** —
  multiple sequential `catch` clauses dispatch by type, a union `catch (A | B e)` catches either, and a
  shadowed clause is a `W-CATCH-UNREACHABLE` lint. `finally` runs on *every* exit edge (normal, caught,
  re-thrown, or a `return`/`break`/`continue` escaping the block). A `Runtime` fault/panic is **not**
  catchable — it passes straight through every `catch` (panics are an uncaught-by-design tier).
- **`?`-throws propagation** — `f()?` on a throwing call propagates `f`'s exceptions to the enclosing
  `throws` (front-end-only: the checker erases the marker, the call's own throw already unwinds).
- **Native unwinding on both backends** — the interpreter uses a `Signal::Throw` (caught at the `try`
  boundary); the VM uses a handler stack (`PushHandler`/`PopHandler`) and unwinds frames + the operand
  stack to the landed handler. A `throws E` subtype transpiles to a PHP class `extends \Exception`, and
  `throw`/`try`/`catch`/`finally` transpile to the PHP constructs 1:1.

### Added — error model Slice 2a: `Result` `?` propagation + fault intrinsics (M-faults)

The first slice of the three-tier error model — the value tier and the panic tier (the enforced
`throws E` exception tier lands in 2b). Byte-identical `run ≡ runvm ≡ real PHP`
(`examples/guide/result.phg`); **no new `Op`**.

- **`?` error-propagation operator** — postfix `expr?` on a `Result<T, E>` (an enum with `Ok`/`Err`
  variants), in a let-initializer: unwraps the `Ok` payload, or **early-returns the `Err`** from the
  enclosing function (which must return the same `Result`). The lexer already munches `??`/`?.`
  separately, so a lone `?` needs no new token. Lowers via the existing `MatchTag`/`GetEnumField`/
  `Return` ops (the VM's `do_return` truncates to the frame base, so the mid-expression early-return is
  clean); transpiles to a PHP statement hoist (`$t = e; if ($t instanceof Err) return $t; $x =
  $t->value;`) since PHP can't caller-return from an expression. Restricted to a let-initializer
  (`E-PROPAGATE-POSITION`); the function must return the matching `Result` (`E-PROPAGATE-CONTEXT`/
  `E-PROPAGATE-ERR`). The `throws`-call mode is deferred to 2b.
- **Fault intrinsics** — `panic("msg")`, `todo()`, `unreachable()` (all **`never`-typed**, so they
  satisfy return-on-all-paths and complete the totality story) and `assert(cond[, "msg"])`. They reuse
  the existing `Op::Fault` (new data-carrying `FaultMsg` variants — no new `Op`); messages are
  compile-time string literals (`E-INTRINSIC-LITERAL`) single-sourced so both backends render
  identically (`FaultKind::Panic`). The names are reserved (`E-RESERVED-INTRINSIC`). Transpile to PHP
  `throw new \RuntimeException`/`\LogicException` and a ternary-`throw` for `assert`.

All five new diagnostics self-document via `phg explain`.

### Added — generic enums `enum Option<T>` / `enum Result<T, E>` (Rich Types, M-RT)

TypeScript-style type parameters on **enums**, the sum-type companion to generic classes. An enum may
declare `<T, …>` after its name; a type parameter is in scope across every variant's payload, **inferred
at the variant constructor** (`Some(7)` ⇒ `Option<int>`, `Ok(1)` ⇒ `Result<int, …>`) by the same
first-binding-wins unifier as a generic class constructor, and **recovered at every `match`** — matching
an `Option<int>` binds `Some(n)` with `n: int`. A variant that mentions no parameter (`None`) can't infer
it; annotate the binding to fix it (`Option<int> n = None();`). Byte-identical `run ≡ runvm ≡ real PHP`
(new `examples/guide/generic-enums.phg`).

Built by mirroring the shipped generic-class machinery with **zero backend changes**: `EnumDecl`/
`EnumInfo` gain a `type_params` list; `try_variant_or_class_call` infers the enum's arguments at the
variant constructor; a new `enum_subst` substitutes them at a `match`; `erase_generics` gains an
`Item::Enum` arm that rewrites a `<T>` payload to `Type::Erased` (PHP `mixed`) and clears the parameter
list before any backend. **No new `Op`, no `Value` change** — `Ty::Named` type arguments are checker-only
and the parameter list is erased pre-backend, so the byte-identity spine is safe by construction. Scope
mirrors generic classes: `package Main` only, inference-only construction, invariant, no bounds, no
generic enum methods. Reuses `E-GENERIC-PARAM`; **GENERICS-ALL now covers functions, methods, classes,
and enums.**

### Added — totality cluster (M-RT): return-on-all-paths, `never`, dead-code lints

Closed the type system's #1 soundness leak: a function whose declared return type carries a value now
must `return` (or diverge) on **every** path — falling off the end is `E-MISSING-RETURN`. Four
front-end-only sub-features, all byte-identical `run ≡ runvm ≡ real PHP` (see
`examples/guide/totality.phg`):

- **Return-on-all-paths** (`E-MISSING-RETURN`), driven by a conservative structural termination
  analysis (`return` / both-branch `if` / infinite loop / `never`-call diverge).
- **`never`** — the bottom type (`Ty::Never`): a subtype of every `T`, inhabited by nothing. A
  `-> never` function is verified to diverge (`E-NEVER-RETURN` otherwise). Transpiles to PHP 8.1
  native `never`.
- **`W-UNREACHABLE`** — a non-fatal lint for a statement after a `return`/diverging statement.
- **`W-MATCH-UNREACHABLE`** — a non-fatal lint for a `match` arm after a catch-all, or a duplicate
  literal/variant/type arm.

No new `Op`, no `Value` change: `never` erases to a PHP return hint and is otherwise checker-only; the
`E-*` errors reject before any backend runs; the `W-*` lints ride the existing warning channel (stderr,
never gating). All four codes are self-documenting via `phg explain`.

### Added — stack traces & beautiful fault reporting (error-handling slice 1)

An uncaught runtime fault now reports a **call stack** instead of a bare message — innermost frame
first, each with `function` + `line` (and `file:line` in a multi-file project), plus the source line of
the fault. Identical on both backends: the VM walks its live call frames, the interpreter keeps a
logical `trace_stack` that mirrors them, and a `run ≡ runvm` **trace-parity** test enforces byte-equal
output. The fault line is backfilled from the innermost frame, so the tree-walker now reports a line
too (the old interpreter/VM asymmetry is gone).

- **CLI:** `phg run`/`phg runvm` render the message, the offending source line, and the frame list.
- **Web:** `phg serve --dev` returns a styled HTML 500 page (fault + stack + request context, every
  value `Core.Html`-escaped). **Production returns a bare generic 500** — no trace/source/message leak.
- Front-end-only with respect to correctness: program stdout is unchanged, `FaultKind` classification
  is preserved, and the M7 PHP oracle is unaffected (traces ride on stderr). No new `Op`.
- See `examples/errors/README.md`. Catching faults (`try`/`catch` vs `Result`) is a later slice.

### Changed — `phg check` reports whole-project scope

`phg check` on a project now reports the scope it validated — e.g. *"OK — whole project type-checks
clean: 3 files, 2 packages, 5 definitions validated (every file + vendored deps)"* — making explicit
the PHP-absent superpower it already had: because the loader merges every `.phg` under the source root
(first-party **and** vendored) into one program and type-checks it before any backend runs, a broken
class or bad import in a file **no route reaches** fails up front (unlike PHP's autoload-on-demand,
where it hides until that file is interpreted). Loose mode (single file / `-e` / stdin) keeps the plain
`OK (type-checks clean)`. (Counts ride on a new `loader::LoadStats`, project mode only.)

### Added — declaration visibility (`public` / `internal` / `private`)

A three-level visibility lattice on every **top-level declaration** (class, enum, interface, free
function): `public` (default — cross-package), `internal` (this package's files only), `private`
(this `.phg` file only). Lattice `file ⊂ package ⊂ public`. A new axis distinct from member-level
`Modifier` visibility, carried as a dedicated `Visibility` enum on each declaration.

- **Parser**: an optional leading `public`/`internal`/`private` keyword before any top-level decl
  (`internal` is a new reserved keyword); explicit `public` allowed; a doubled prefix is a parse error.
- **Loader-enforced, backend-erased**: the M5 loader records each definition's `(file, package, vis)`
  in Pass 1 and applies the lattice at its three resolution chokepoints — `build_type_imports`
  (cross-package types), `resolve_type_ref` (same-package types), `resolve_call` (functions). No
  backend reads the field, so the `run ≡ runvm ≡ real PHP` byte-identity spine is safe by construction
  (PHP has no file/package-private declarations → emitted as a normal `class`/`function`).
- New codes (both with `phg explain`): `E-VIS-PRIVATE`, `E-VIS-INTERNAL`.
- New byte-identity-gated example project `examples/project/visibility/` (+ README documenting the
  two rejected cases, which can't be runnable examples).

### Added — in-place mutation (mutation milestone, M-mut.1–.7b) — feature-complete

Phorge was a pure single-assignment language (the AST had no assignment statement); the mutation
milestone adds in-place mutation **immutable-by-default, `mutable` opt-in**, with no tracing GC. The
locked spine (forced by the real-PHP oracle): `List`/`Map`/`Set`/`Bytes` are **copy-on-write value
types** (can't cycle ⇒ `Rc`/`Drop` reclaims fully); `Instance` is a **shared-mutable handle**
(PHP/Java semantics). Every slice is byte-identical `run ≡ runvm ≡ real PHP`.

- **M-mut.1** mutable locals + reassignment (`mutable` binding modifier; reuses `Op::SetLocal`).
- **M-mut.2** compound assignment + `++`/`--` + `??=` (pure parser desugar, no new `Op`).
- **M-mut.3** condition loops (`while`/`do-while`/C-`for`/while-let) + `break`/`continue` (no new `Op`).
- **M-mut.4a** `obj with { f = e }` functional update (fresh instance via `Op::MakeInstance`).
- **M-mut.5** value-type element set `xs[i] = e` / `m[k] = e` (one new `Op::SetIndex`, COW).
- **M-mut.6** shared-mutable instance fields `o.f = e` / `this.f = e` (instances are **handles**; one
  new `Op::SetField`; cycle-safe `eq_val`; **no cycle collector** — Fork-3 defer-to-process-exit).
- **M-mut.7a** `static`/`static mutable` class fields, read/written as `ClassName.field` (dot, not
  `::`); new `Op::GetStatic`/`SetStatic`; literal-const initializers seeded once at load.
- **M-mut.7b** **property hooks** `T name { get => expr; set(T v) { stmts } }` — virtual get/set; a get
  computes on read, a set intercepts a write; get-only = read-only, set-only = write-only. Lowers on
  the VM to synthetic `<Class>::<name>$get`/`$set` methods dispatched via the existing `Op::CallMethod`
  (**no new `Op`**); transpiles 1:1 to a PHP 8.4 property hook (new `examples/guide/property-hooks.phg`).
  New codes (all with `phg explain`): `E-HOOK-NO-GET`, `E-HOOK-NO-SET`, `E-HOOK-TYPE`, `E-HOOK-DUP`.

Deferred (see KNOWN_ISSUES): no cycle collector, no identity `===`, nested place-stores (`this.f[i]=e`),
and backed/static/interface/abstract property hooks.

### Added — intersection types `A & B` (Rich Types, M-RT S5)

- **Intersection types:** `A & B` is a value that satisfies *all* members at once — the narrowing dual
  of a union. Members are interfaces plus **at most one** concrete class (two distinct classes are the
  bottom type — a value has exactly one class). A value flows into `Drawable & Named` iff it implements
  both, and **inside, every member's methods are in scope** (member access searches each member, the
  one genuinely new mechanism vs. S4). Lexes a lone `&` to a new `TokenKind::Amp` (distinct from `&&`),
  which **binds tighter than `|`** (`A | B & C` ≡ `A | (B & C)`); normalized like a union
  (`Ty::intersection_of`); the assignability arms are the exact dual of S4's. **No new `Op`, no `Value`
  change** — an intersection is checker- and PHP-signature-only; the runtime value is always a concrete
  instance. Transpiles to PHP 8.1 native `A&B`. Byte-identical `run ≡ runvm ≡ real PHP`
  (new `examples/guide/intersections.phg`).
- New codes (all with `phg explain`): `E-INTERSECT-MEMBER` (a primitive/enum/optional/function member),
  `E-INTERSECT-MULTI-CLASS` (two or more concrete classes — uninhabited until S6 `extends`),
  `E-INTERSECT-ARITY` (collapses to one member), `E-INTERSECT-SIG` (two members share a method with
  conflicting signatures — no class can implement both, since Phorge has no overloading **yet**), and
  `E-INTERSECT-NO-MEMBER` (a member access resolves on no member). `instanceof` now also accepts an
  intersection-typed operand. **Deferred** (see KNOWN_ISSUES): `instanceof` against an intersection,
  optional/function members, whole-intersection optional `(A & B)?`.
- **Method overloading confirmed for M-RT** (sequenced next, right after S5): a Phorge-level feature
  lowered to a single dispatching PHP method (PHP forbids same-name redeclaration) — the
  TypeScript-over-JavaScript relationship the transpile contract is built for.

### Added — union types `A | B` + match-over-union (Rich Types, M-RT S4)

- **Union types:** `A | B | C` is a value that is *one of* several types — the open-composition
  counterpart to a closed `enum`. Members may be classes, interfaces, and primitives (`int | string`),
  and a value of any member flows into a union-typed slot (`Circle` → `Circle | Square`). A union is
  **normalized** (`Ty::union_of`: flatten nested, dedupe, canonical-sort by `Display`), so `A | B` and
  `B | A` are the same type. Lexes a lone `|` to a new `TokenKind::Bar` (distinct from `|>`/`||`);
  transpiles to PHP 8.0 native `A|B`. Byte-identical `run ≡ runvm ≡ real PHP`
  (new `examples/guide/unions.phg`).
- **match-over-union via type patterns:** `match s { Circle c => …, Square sq => … }` matches each arm
  by a runtime type test, binding the narrowed instance — **exhaustive over the union's member set**
  like an enum match. This is the one new pattern kind (`Pattern::Type`), threaded through the parser
  (disambiguated as two identifiers in pattern position — `Circle c`; a lone `Circle =>` stays a
  catch-all binding), checker (binding + narrowing + exhaustiveness), and all four backends. It reuses
  the S1 `instanceof` machinery — **no new `Op`** (the interpreter threads `class_implements`; the
  compiler emits load-path + `Op::IsInstance` + `JumpIfFalse`; the transpiler emits a PHP `instanceof`
  guard). `instanceof` narrowing now also accepts a union operand. Type patterns are top-level-only
  (nesting in a variant payload is a clean `E-MATCH-TYPE`). New codes: `E-UNION-MEMBER` (enum/optional/
  function members rejected), `E-UNION-ARITY` (a union needs ≥2 distinct members), `E-MATCH-TYPE`; all
  carry `phg explain` entries. **Deferred:** enum members in a union, intersection/negative-flow
  narrowing, common-member access on a raw union, whole-union optional `(A|B)?` (see KNOWN_ISSUES).

### Added — erased generics `<T>` on classes (Rich Types, M-RT generics-all)

- **Generic types/classes:** a class may declare type parameters after its name —
  `class Box<T> { … }`, `class Pair<A, B> { … }` — used in its field, constructor, and method
  signatures. The parameter is **inferred at construction** from the constructor arguments
  (`Box(7)` ⇒ `Box<int>`) and **recovered at every use site** (`Box(7).get()` is `int`; a method
  taking a `T` checks its argument at the instance's concrete type). Byte-identical
  `run ≡ runvm ≡ real PHP` (new `examples/guide/generic-types.phg`). This completes generics-all.
- **The TypeScript model — reified in the checker, erased in the backend.** `Ty::Named` now carries
  type arguments (`Ty::Named(String, Vec<Ty>)`): construction unifies the constructor parameters
  against the call's arguments to bind them, and member access substitutes the class's type parameters
  with the instance's arguments — full use-site precision (`string s = Box(7).get()` is a type error).
  After checking, `erase_generics` rewrites a generic class's own `<T>`-typed members (fields,
  constructor, methods) to `Type::Erased`, so the field becomes PHP `mixed` and an instance carries no
  runtime type argument (`instanceof Box<int>` ≡ `instanceof Box`). **No new `Op`, no `Value` change,
  and zero backend changes** — `resolve_cty`/`emit_type` already key a class type on its name and
  ignore arguments, so the byte-identity spine is safe by construction (a front-end-only slice). New
  diagnostic reuse: `E-GENERIC-PARAM` (a method type parameter shadowing a class one). Scope:
  `package Main` only (cross-package generic library types deferred); inference-only construction (no
  `Box<int>(7)`); invariant, no bounds, no generic enums.

### Added — cross-package types: `import type` (Rich Types, M-RT)

- **The `E-PKG-TYPE` gate is retired.** A library (non-`main`) package may now declare a
  `class`/`enum`/`interface`, and another package consumes it with the terminal
  **`import type acme.geometry.Point [as Pt];`** form (binds a bare type name; functions still use the
  Go-qualified `pkg.fn()` form; built-ins like `List` stay import-free). Nominal subtyping,
  `instanceof`, and enum `match` all work across packages. New example `examples/project/shapes/`
  (a library `class` + `interface` + `enum` consumed from `package Main`), byte-identical
  `run ≡ runvm ≡ real PHP`.
- **Mechanism — the cross-package *function* mangle/resolve pass, extended to types.** The loader
  gains a `types` symbol table (`(package, Type) ⇒ Acme\Geometry\Point`) and a per-file type-import
  map; Pass 2 rewrites every type-name position — annotations, instantiation (`Point(…)`),
  `instanceof`, enum construction/`match` (via the bare variant whose enum is mangled) — to the
  mangled FQN, mirroring `erase_generics`'s exhaustive `Type`/`Expr` walk. The checker and both
  backends see fully-resolved names (`run ≡ runvm` by construction); only the transpiler de-mangles,
  bucketing each type into its `namespace Acme\Geometry { … }` block and emitting references as
  absolute FQNs (`new \Acme\Geometry\Rect(…)`, `instanceof \Acme\Geometry\Shape`). **No new `Op`, no
  `Value` change**; a single-package program is byte-identical to the pre-lift output.
- New diagnostics: `E-TYPE-IMPORT-UNKNOWN` (no such exported type), `E-TYPE-IMPORT-CONFLICT` (two
  terminal imports bind one name — alias with `as`), `E-TYPE-IMPORT-BUILTIN` (built-ins are
  import-free), `E-TYPE-IMPORT-SHADOW` (collides with a local type or a module-import qualifier).
- Deferred: the module-qualified type form (`import acme.geometry;` → `Geometry.Point`); generic
  *types* (`Box<T>`); generic interface methods.

### Added — erased generics `<T>` on methods (Rich Types, M-RT generics-all)

- **Generic methods:** a class method may declare type parameters (`class U { function id<T>(T x) -> T
  { return x; } }`), inferred at the call site from the arguments exactly like a generic free function
  (`u.id(7)` → `int`, `u.firstOr(xs, -1)`, `u.applyTwice(5, fn(int v) => v + 1)`). The class itself is
  not generic — only the method introduces `T`. Byte-identical `run ≡ runvm ≡ real PHP` (new
  `examples/guide/generic-methods.phg`).
- **Reuses the S7a free-function machinery, zero backend changes.** The parser drops the now-vestigial
  "methods can't be generic" gate; the checker registers a method signature with its `type_params` in
  scope (so a bare `T` resolves to `Ty::Param`) and routes a generic method call through the same
  first-binding-wins `check_generic_call`/`unify`; `erase_generics` gains a class arm that rewrites
  each generic method's signature + body to `Type::Erased` (PHP `mixed`/`array`/`\Closure`) before any
  backend — so the interpreter, VM, and transpiler never see a type variable. **No new `Op`, no
  `Value` change.** Generic *interface* methods stay deferred (their signatures are built with an empty
  type-param list); generic types/classes (`Box<T>`) are the next generics-all sub-slice.

### Added — generic stdlib natives: `Core.List` & `Core.Map` query ops (Rich Types, M-RT S7b)

- **The first generic native functions**: `Core.List` `reverse(List<T>) -> List<T>` and
  `sum(List<int>) -> int`; `Core.Map` `keys(Map<K,V>) -> List<K>`, `values(Map<K,V>) -> List<V>`,
  `has(Map<K,V>, K) -> bool`, `size(Map<K,V>) -> int`. A native whose stored signature carries a
  `Ty::Param` is now checked at the call site by the **same unifier as a generic free function**
  (`check_native_call` routes through `check_generic_call` when the signature has a type parameter),
  so the parameter resolves to the concrete argument types and the result type is substituted. No new
  `Op`, no `Value` change: each erases to a PHP array builtin (`array_reverse`/`array_sum`/`array_keys`/
  `array_values`/`array_key_exists`/`count`), and the native's `Ty::Param` is registry-only — the
  compiler types a native call by expression shape (`CTy::Other`) and the transpiler emits via the
  `php` closure, so no type variable reaches a backend. Byte-identical `run ≡ runvm ≡ real PHP` (new
  `examples/guide/collections-query.phg`, oracle-gated). Caveats (KNOWN_ISSUES): `List.sum` faults on
  i64 overflow where PHP `array_sum` promotes to float; PHP coerces integer-like/bool map keys, so
  `keys`/`values` round-trip byte-identically only with plain string keys. (The higher-order
  `map`/`filter`/`reduce` build on this path in the next S7b sub-slice.)
- **`Set<T>` (`Core.Set`):** `of(List<T>) -> Set<T>` (deduplicate, insertion-ordered), `contains(Set<T>,
  T) -> bool`, `size(Set<T>) -> int`. `Value::Set` is realigned from a bare `HashSet<HKey>` to an
  insertion-ordered, `Rc`-shared `Rc<Vec<HKey>>` (the same byte-identity discipline as `Map`, risk R1),
  built only through the single `value::build_set` kernel so both backends dedup identically; `Set`
  equality is order-independent membership. Erases to a deduped PHP array (`array_values(array_unique(
  $xs, SORT_STRING))` / `in_array(_, _, true)` / `count`). Byte-identical `run ≡ runvm ≡ real PHP` (new
  `examples/guide/sets.phg`). Set union/intersection and iteration are follow-ups.
- **Higher-order `Core.List` ops (S7b-3):** `map(List<T>, (T) -> U) -> List<U>`, `filter(List<T>,
  (T) -> bool) -> List<T>`, `reduce(List<T>, U, (U, T) -> U) -> U` — the first natives that take a
  **closure** argument. A native's `eval` becomes a `NativeEval` enum: `Pure(fn(args, out))` (every
  existing native) or `HigherOrder(fn(args, invoke))`, where `invoke` is a backend-supplied
  [`ClosureInvoker`] that runs a `Value::Closure` and returns its result. The one native body drives
  **both** backends: the interpreter's invoker wraps `call_closure`; the VM gains a re-entrant
  `call_closure_value` + `run_until` that pushes the closure's frame and drives the **shared**
  `exec_op` until it returns — so a closure's result and any fault it raises are byte-identical to the
  interpreter (the parity discipline of the value kernels, extended to control flow). **No new `Op`, no
  `Value` change.** Generic over the element/result type (same call-site unifier as a generic free
  function); erase to PHP `array_map` / `array_values(array_filter(…))` / `array_reduce`. Byte-identical
  `run ≡ runvm ≡ real PHP` (new `examples/guide/higher-order.phg`, oracle-gated). This **completes
  M-RT S7b.**

### Changed — stdlib namespace is now PascalCase `Core.*` (namespace reshape)

- **The standard-library root and leaf modules are PascalCase**: `Core.Console` → **`Core.Console`**,
  and likewise `Core.Math` / `Core.Text` / `Core.File` / `Core.Bytes` / `Core.Html`. Function names stay
  camelCase (`println`, `sqrt`, `splitOnce`). `import Core.Console;` becomes `import Core.Console;` and
  the call site `Console.println(...)` becomes `Console.println(...)`. `Core` is the reserved package
  root (`E-RESERVED-PACKAGE`). This aligns the stdlib with the namespace-reshape rule that package
  *segments* are PascalCase. A repo-wide breaking codemod across every example, fixture, test program,
  and the native registry; byte-identical `run ≡ runvm ≡ real PHP` preserved (the namespace is a
  compile-time organizing layer — natives still erase to flat PHP builtins). *Consequence:* a stdlib
  qualifier (PascalCase) can no longer be shadowed by a camelCase local, so `E-SHADOW-IMPORT` now only
  bites a lowercase **user**-package leaf. (The broader reshape — `package Main` → `package Main`,
  user-package-segment casing enforcement, manifest `name`→`module` — remains pending.)

### Added — erased generics `<T>` on free functions (Rich Types milestone, M-RT S7)

- **TypeScript-style generic type parameters** on free functions: `function id<T>(T x) -> T`,
  `function firstOr<T>(List<T> xs, T fallback) -> T`, `function applyTwice<T>(T x, (T) -> T f) -> T`.
  The type parameter is **inferred at each call site** from the argument types (structural,
  first-binding-wins unification that descends into `List<T>`, `Map<K,V>`, `T?`, and function types),
  and the call's result type is the substituted return type — so `id(42)` is `int` and `id("x")` is
  `string` from one definition. Byte-identical `run ≡ runvm ≡ real PHP` (new `examples/guide/generics.phg`,
  oracle-gated).
- **Full erasure, no monomorphization, no new `Op`.** A new `Ty::Param(String)` exists *only* in a
  generic function's stored signature + body (it is opaque there — assignable only to the same
  parameter); a new post-check pass `checker::erase_generics` rewrites every type annotation that
  names a type parameter into the new `Type::Erased` and clears the parameter list **before any
  backend runs** — the same "compile-time-only, expanded out" discipline as `type` aliases and
  `html"…"`. The interpreter, VM, and transpiler never see a type variable: erased types compile to
  `CTy::Other` and emit PHP `mixed` (containers stay `array`, function values `\Closure`).
- **Scope this slice:** free functions only (`E-GENERIC-PARAM` on a type param that shadows a built-in
  or is duplicated; generic *methods* are a clean parse error; type params are PascalCase like all type
  names). Bounds, variance, generic types/classes, generic functions as first-class *values*, and an
  empty `[]` literal passed straight to a generic parameter are deferred (see KNOWN_ISSUES). This is
  the unblocker for `Set`, the generic-typed Map/Set query ops, and `core.list` — built on it next.

### Added — `Map<K, V>` foundation: literals + indexing (Rich Types milestone, M-RT S3)

- **`Map<K, V>` literals `[k => v, …]`** and **indexing `m[k]`**, byte-identical `run ≡ runvm ≡ real
  PHP` (verified; new `examples/guide/maps.phg`, oracle-gated). The map literal is distinguished from a
  list literal by the `=>` after the first element; `[]` stays the empty *list* (an empty map literal
  is deferred). Keys are the hashable subset — `int`/`bool`/`string` (`E-MAP-KEY` otherwise) — and a
  missing key is a clean, byte-identical fault (`"map key not found"`), like list out-of-range.
- **Insertion-ordered representation.** `Value::Map` is now an `Rc<Vec<(HKey, Value)>>` (not a
  `HashMap`), so map order is part of the value — keeping a future `keys()`/iteration byte-identical
  with PHP's insertion-ordered arrays. Building (first-position/last-value dedup) and lookup are
  single-sourced in `value::build_map` / `value::map_index` kernels, so the two backends agree.
- **One new `Op::MakeMap(n)`** (across the three coupled matches + `validate`); the existing
  `Op::Index` is made **runtime-polymorphic** (a `List` bounds-checks an int index; a `Map` does a key
  lookup) rather than adding a separate `IndexMap`. The compiler gains `CTy::Map(K, V)` so a map-index
  result is a first-class arithmetic operand (`m["k"] + 1` specializes on the VM — without it the VM
  would fail to compile what the interpreter accepts). Transpiles to a PHP `[k => v]` array; `$m[$k]`.
- **Scope this slice (foundation only):** `Set`, and the generic-typed query ops (`keys`/`has`/`size`/
  `contains`/iteration), are deferred to **erased generics (S7, reordered to immediately follow S3)** —
  they hit the same no-type-variable wall that defers `core.list`. New `E-MAP-KEY` in `phg explain`.

### Added — interfaces + `implements`/`extends` (Rich Types milestone, M-RT S2)

- **`interface I { method sigs }`**, **`class C implements I, J`**, and **`interface K extends I`**.
  An interface is a named contract of method signatures (no bodies). A class that `implements` an
  interface is a **nominal subtype** of it: a concrete instance flows into an interface-typed binding,
  parameter, or return, and code written against the interface works for every implementer
  (polymorphism). Interface-typed receivers resolve methods through the interface's flattened
  (`extends`-closure) signature set.
- **`instanceof` now accepts an interface** on the right (extending M-RT S1's class-only operand):
  `x instanceof SomeInterface` is true for every implementer (transitively, through interface
  `extends`), and inside `if (x instanceof I)` the operand smart-casts to `I`.
- **One shared `class_implements` table.** The transitively-flattened, sorted class→interface map is
  computed once by `ast::class_implements(program)` and consumed verbatim by the checker (subtyping +
  conformance), the interpreter, and the VM (`BytecodeProgram.class_implements`) — one algorithm, so
  the runtime `instanceof` test can never diverge across backends. **No new `Op`** (S1's
  `Op::IsInstance` gained the table lookup). Nominal subtyping threads through a new
  `Ty::assignable_with(from, to, &subtype_oracle)` (the old `Ty::assignable` is the no-subtype
  delegate), keeping the optional/function recursion in one chokepoint.
- **Transpiles to a PHP `interface` / `implements` / `extends`** — byte-identical `run ≡ runvm ≡ real
  PHP` (verified). New `examples/guide/interfaces.phg` (oracle-gated). New diagnostics
  `E-IFACE-IMPL` / `E-IFACE-UNIMPL` / `E-IFACE-SIG` / `E-IFACE-CYCLE` (+ the missing `E-INSTANCEOF-TYPE`
  explain entry, backfilled from S1) are in `phg explain`. Scope this slice: interfaces are
  `package Main`-only (`E-PKG-TYPE`), and method signatures match exactly (no variance yet).

### Added — `instanceof` type test, retiring the `is` stub (Rich Types milestone, M-RT S1)

- **`value instanceof ClassName`** is now a real runtime type test that evaluates to `bool` on
  `run`/`runvm` and transpiles to PHP `$value instanceof ClassName` — byte-identical across all three
  backends (verified against real PHP). The right operand is parsed as a class *type name* (not an
  expression), so it is a dedicated `Expr::InstanceOf` node, not a `BinaryOp`. The VM uses one new
  `Op::IsInstance(String)` (carries the class name inline, like `Op::Fault` — no name-pool entry,
  extends the three coupled `Op` matches).
- **Smart-cast narrowing:** inside `if (x instanceof C) { … }`, the checker narrows `x` to `C` in the
  then-block (reusing the if-let scope mechanism), so member access through it type-checks.
- **The value-equality `is` alias is retired.** `is` is no longer a keyword (it is now an ordinary
  identifier); the old `BinaryOp::Is` (which merely aliased `==` and the transpiler rejected) is gone.
  This closes the GA blocker where `is` parsed and type-checked but could not transpile.
- New `examples/guide/instanceof.phg` (oracle-gated). Scope notes (KNOWN_ISSUES): the operand is a
  **class** today (interface/union/intersection tests arrive with those features in later M-RT
  slices), and with no subtyping yet the test compares a concrete class to a concrete class.

### Added / Fixed — `match` transpiler completion + an Assign-position correctness fix (GA P1-b, M11)

- **Literal-pattern `match` now transpiles.** `0 => …` / `"a" => …` / `true => …` / `1.5 => …` arms
  emit a strict `=== <literal>` guard, mirroring the interpreter's exact value match. This enrolls
  `examples/guide/enums-match.phg` in the PHP oracle (previously `DEFER`'d).
- **Expression-position `match` now transpiles.** A `match` used as a sub-expression (operand, call
  argument, interpolation) lowers to an immediately-invoked PHP closure wrapping the *same* if-chain
  the statement form emits — one lowering, no divergence. Enclosing locals are captured by value via
  `use(…)` (Phorge values are immutable, so by-value is exact); `$this` auto-binds in method closures.
  New `examples/guide/match-expr.phg` (oracle-gated).
- **Fixed: `var x = match …` could throw `UnhandledMatchError` in transpiled PHP.** `emit_match`
  previously emitted independent `if`s plus an unconditional defensive `throw`; that only
  short-circuited in `return` position. In assign (var-decl-init) position the arms fell through and
  the throw ran unconditionally. The chain is now `if/elseif/else`, so exactly one arm runs and the
  throw is the terminal `else` — correct for both positions. (The `run`/`runvm` backends were always
  correct; this was a transpile-leg bug.)
- **Honesty:** KNOWN_ISSUES corrected — at P1-b the `is` operator was **value-equality (a synonym for
  `==`), not a type test**, and the transpiler rejected it. (The earlier claim that all three
  constructs "run fine, only transpile rejects" was inaccurate for `is`.) *This was superseded almost
  immediately by M-RT S1 above, which retired `is` and shipped a real `instanceof` type test.*

### Fixed — transpiled `float` now byte-identical to the Rust backends (GA P1-a)

- A finite `float` rendered through the transpiler previously diverged from `run`/`runvm`: PHP's
  default string cast uses `precision=14` and switches to scientific notation for large/small
  magnitudes (`sqrt(2.0)` → `1.4142135623731`, `1e15` → `1.0E+15`, `0.00001` → `1.0E-5`), while the
  Rust backends print the shortest round-trip, always positional. The transpiler now routes every
  float through a new **`__phorge_float`** runtime helper that reproduces Rust's `f64` Display exactly
  (shortest round-trip, positional for any magnitude, integer-valued floats drop the trailing `.0`,
  `inf`/`-inf`/`NaN` spelled the Rust way). Tier-1 PHP functions only, so it stays correct under
  `php -n`. New `examples/guide/floats.phg` round-trips irrational/large/small magnitudes through real
  PHP. The earlier KNOWN_ISSUES "exactly-representable floats only" caveat is **resolved** for all
  finite floats; the sole remaining float caveat is the fault-domain float-÷-by-zero divergence
  (PHP throws vs. Rust `inf`/`NaN`), which the differential harness excludes by design.

### Security — `phg serve` made DoS-resilient (GA blockers B3, B4 + P1-d)

- **One connection can no longer take the server down (B3).** A per-connection `recv`/`send` error
  (client reset, broken pipe, transient `accept`) previously propagated out of the accept loop and
  exited the process — an unauthenticated remote DoS. The loop now logs and skips such errors and
  continues serving; only `MAX_CONSECUTIVE_TRANSPORT_ERRORS` (64) accept errors in a row with no
  progress shuts it down (a genuinely dead listener). A per-request fault still degrades to a 500.
- **Slowloris closed with a read/write timeout (B4).** Each accepted connection now gets a
  `set_read_timeout`/`set_write_timeout` (default **30s**, configurable with `phg serve --timeout
  SECONDS`; `0` disables). A slow/idle client times out and is dropped, and the single-threaded server
  moves on to the next connection instead of being wedged indefinitely.
- **Framing is now unit-tested + a CPU-DoS fixed (P1-d).** `read_http_request` is generic over `Read`
  and covered by unit tests (Content-Length present/absent/malformed/case-insensitive, terminator &
  body split across chunks, EOF-before-headers, the 8 MiB cap), and the real-socket smoke test is
  un-`#[ignore]`d. Fixed a latent **O(n²)** re-scan of the whole buffer for the header terminator on
  every chunk (a CPU-DoS on a large no-terminator request) — it now scans only newly-arrived bytes.
- `phg serve --help` and SECURITY.md document the single-thread posture, the `127.0.0.1` default, and
  `--timeout`. All changes are in the quarantined `src/serve.rs` runtime — the `run ≡ runvm ≡ php`
  byte-identity spine is untouched.

### Security — `phg vendor` supply-chain hardening (GA blockers B1, B2)

- **Git argument-injection / arbitrary-command-execution closed.** `phg vendor` passed a
  dependency's `git` URL and `tag`/`rev` pin straight to the `git` CLI. An attacker-authored
  `phorge.toml` could therefore inject git options (a leading `-`, e.g. `--upload-pack=…`) or a
  command-executing remote helper (`ext::sh -c '…'`). The clone now uses a `--` end-of-options
  separator and `-c protocol.ext.allow=never`, and both the URL and the pin are rejected up front if
  they start with `-` or use the `ext::`/`file::` transports. The ordinary `file://` URL scheme (used
  by the offline test fixtures) is unaffected.
- **Path traversal via dependency name / `source` closed.** A `[require]` key or a `source` value was
  joined verbatim onto a filesystem path (`vendor/<name>`, `<root>/<source>`), so `"../../.."` or an
  absolute path could make `phg vendor`'s `remove_dir_all`/`rename` — or the loader's scan — operate
  outside the project tree. Both are now validated at manifest-parse time (rejecting `..` traversal,
  absolute paths, empty/`-`-leading segments, and characters outside `[A-Za-z0-9._-]`) and
  defensively re-checked at every path-join site. `source = "."` stays valid.
- Both fixes are confined to the `phg vendor` / loader supply-chain path; the `run ≡ runvm ≡
  transpiled-PHP` byte-identity spine is untouched.

### Packaging — identifier casing enforced (namespace reshape, slice 2a)

- **Identifier casing is now a hard, checked rule.** Value identifiers — functions, methods,
  parameters, fields, `var`/typed local bindings, `for`-loop variables, if-let bindings, and lambda
  parameters — must be **camelCase** (`E-NAME-CASE`); type identifiers — class names, enum names,
  enum variant names, and `type` alias names — must be **PascalCase** (`E-TYPE-CASE`). camelCase is a
  lowercase first letter with no `_` (a single lowercase word like `main` is valid); PascalCase is an
  uppercase first letter with no `_`. Each diagnostic suggests the converted form (`split_once` →
  `splitOnce`, `shape` → `Shape`) and both have `phg explain` entries.
- **The shipped stdlib public API is migrated to camelCase:** `Core.Text.split_once` → `splitOnce`,
  `Core.Html.bool_attr` → `boolAttr`, `Core.Html.void_el` → `voidEl`, `Core.Bytes.from_string` →
  `fromString`, `Core.Bytes.to_string` → `toString`. The native `eval`/PHP mappings are unchanged —
  only the call-site name.
- **Front-end-only, so byte-identity is untouched.** The casing pass lives in the checker (shared by
  all three backends) and only gates *which* programs are accepted; the AST every backend sees is
  identical, so the `run ≡ runvm ≡ transpiled-PHP` spine is unaffected. Casing applies to the original
  source identifier, so a loader-mangled cross-package name (`Acme\Util\compute`) is validated on its
  leaf (`compute`). All examples, fixtures, and inline test programs are migrated.
- This is reshape slice 2a (`docs/specs/2026-06-20-package-namespace-reshape-design.md`);
  **package-segment casing (`E-PKG-CASE`) is deferred to slice 2b.**

### Packaging — manifest distributable key renamed `name` → `module` (namespace reshape, slice 1)

- **`phorge.toml`'s top-level distributable is now `module = "vendor/package"`** (was `name`). The
  *keyword* `package` names the code unit (folder=path, `Main` entry) while `module` names the
  distributable — Go's `go.mod` split — removing the `package`-keyword vs `name = "vendor/package"`
  overload (reshape design D1). The `[require]`/`[require-dev]` dependency keys and the `phorge.lock`
  `name` field are unchanged (they are *dependency coordinates*, not the project's own identity).
  Rename-only and output-preserving: the emitted PHP namespace root (`namespace_root()`) and the
  `run≡runvm≡php` byte-identity spine are untouched. This is the first slice of the
  package/namespace reshape (`docs/specs/2026-06-20-package-namespace-reshape-design.md`); the
  example projects' `phorge.toml` files are migrated.

### Tooling — `phg check --json` (machine-readable diagnostics, LSP foothold)

- **`phg check --json`** emits the checker's diagnostics as a single-line JSON array to stdout (the
  seam `src/diagnostic.rs` always intended): each object carries `stage`/`severity`/`message`/
  `line`/`col`/`code`/`hint` (`code`/`hint` are `null` when absent), errors first then warnings.
  Exit 0 when clean (or warnings only), 1 when any error is present — but the array is always the
  output and nothing goes to stderr, so an editor/LSP can parse it unconditionally. Serializer is
  std-only (RFC-8259 escaping, no serde) on the existing `Diagnostic` type — no backend touched, no
  byte-identity surface. Plain `phg check` is unchanged.

### Core.Html — typed auto-escaping HTML (Waves 1–3: escape kernel + element builders + `html"…"` sugar)

- **Named per-tag helpers (Option 1).** A curated common HTML5 tag set — `html.div`/`html.p`/`html.a`/
  `html.ul`/`html.li`/`html.h1`–`h6`/`html.section`/`html.table`/… and the void elements
  `html.br`/`html.hr`/`html.img`/`html.input`/… — each `html.<tag>(attrs, children) -> Html` (or
  `(attrs) -> Html` for void), sugar over `el`/`void_el` with the tag baked in. Resolved the deferred
  "fn-pointer natives can't bake a tag" blocker by **monomorphizing**: two `macro_rules!` emit a
  per-tag `eval`+`php` pair with the tag literal compiled in via `concat!`, so every tag is a uniform,
  byte-identity-tested registry entry — **no new `Op`, no lexer/parser/checker/backend change** (the
  four-backend native call path is already registry-generic, like Wave 2). `examples/guide/html.phg`
  showcases them, byte-identical on `run`/`runvm`/**real PHP**.
- **Wave 3 — the `html"…"` literal sugar.** A prefixed literal `html"<h1>{name}</h1>"` (lexed by a
  dedicated `scan_html`, mirroring `b"…"`; multi-line for free, since string bodies already span
  lines) that desugars to the Wave-1/2 kernel: literal chunks → `html.raw(chunk)`, and each `{e}`
  hole is resolved **by `e`'s type** in the checker — an `Html` value embeds verbatim (no
  double-escape), a `string`/`int`/`float`/`bool` is auto-escaped via `html.text` (the safe
  default — injecting trusted markup requires writing `{html.raw(x)}` explicitly), anything else is
  `E-HTML-HOLE`. The whole literal becomes `html.concat([…])` and is **erased before any backend**
  (`checker::resolve_html`, the `expand_aliases` precedent), so there is **no new `Op`, no new
  runtime, and no new byte-identity surface** — parity is inherited from the kernel. `html"…"`
  requires `import Core.Html;` (`E-HTML-IMPORT`, robust to `import Core.Html as h;`).
  `examples/guide/html.phg` now showcases the sugar, byte-identical on `run`/`runvm`/**real PHP**.
- **Wave 2 — typed element builders.** A new distinct type `Attr` (like `Html`, erases to PHP
  `string`, non-interchangeable) plus five `Core.Html` natives compose HTML from typed fragments
  rather than hand-written markup: `attr(string, string) -> Attr` (value escaped, name trusted),
  `bool_attr(string) -> Attr` (valueless), `el(string, List<Attr>, List<Html>) -> Html`,
  `void_el(string, List<Attr>) -> Html` (self-closing), and `concat(List<Html>) -> Html`. Each
  builder's `eval` and its PHP emission are held byte-identical by a unit test (the `el`/`void_el`
  PHP uses an IIFE so the tag expression evaluates exactly once). No new `Op`; the safety wall and
  zero runtime divergence carry over from Wave 1. `examples/guide/html.phg` now also exercises the
  builders, byte-identical on `run`/`runvm`/**real PHP**.
- **Empty list literal `[]` as a call argument** now adopts its element type from the expected
  parameter type (a small, call-argument-only bit of bidirectional checking in `check_args`), so a
  zero-attribute or zero-child builder call reads naturally — `el("p", [], [text(x)])`. An empty
  `[]` in a declaration initializer or `return` still requires a non-empty literal.
- **`Html` type + `Core.Html` escape kernel (Wave 1).** The Phorge-idiomatic answer to "how do I write HTML"
  (design: `docs/specs/2026-06-19-core-html-design.md`). `Html` is a distinct checker type
  (`Ty::Html`) that erases to PHP `string` and rides `Value::Str` at runtime — but is **not
  interchangeable with `string`**, so untrusted text cannot reach rendered HTML except through
  `Core.Html.text` (auto-escape) or the audited `Core.Html.raw` (trusted markup). This makes XSS a
  *compile error*, not a runtime hazard — enforced by the type checker, zero new `Op`, zero runtime
  divergence. Boundary natives: `text(string) -> Html`, `raw(string) -> Html`, `render(Html) ->
  string`. Escaping erases to the **pinned** `htmlspecialchars($s, ENT_QUOTES, 'UTF-8')` (tier-1,
  `php -n`-safe) and is mirrored by a Rust five-char table held byte-identical by a unit test.
  `examples/guide/html.phg` runs byte-identically on `run`/`runvm`/**real PHP**. (Builders shipped in
  Wave 2 and the `html"…"` literal sugar in Wave 3, both above.)

### M9 — Engineering Hygiene (CI enforcement)

- **GitHub Actions CI (`.github/workflows/ci.yml`) — locks in M7.** A `gate` job runs the same three
  checks as the local pre-commit hook (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`) on the toolchain pinned in `rust-toolchain.toml`, and sets `PHORGE_REQUIRE_PHP=1` (with
  `php` installed via `setup-php`) so the M7 PHP oracle in `tests/differential.rs` **fails** rather than
  skips if transpiled PHP diverges from the interpreter/VM. A `cross-build` job installs Zig +
  `cargo-zigbuild` + the four Phase-2 cross targets + `llvm-objcopy` (from `llvm-tools-preview`, via
  `PHORGE_OBJCOPY`) and runs `tests/build.rs` for real (x86_64-musl native exec + windows-gnu PE
  round-trip), plus an aarch64-gnu/musl compile smoke. This makes CONTRIBUTING.md's "CI runs the same
  gate" true (no workflow existed before).

### M7 — Correctness Closure (the third backend leg, enforced)

The transpiler→PHP backend is now inside the automated correctness loop. Previously
`tests/differential.rs` gated only `run ≡ runvm`; the transpiled PHP was never executed, so
transpiler→PHP divergences shipped silently — including inside examples advertising three-way
byte-identity.

- **PHP oracle (closes P0-ROOT).** `tests/differential.rs` gains `all_examples_transpile_and_match_php`
  and `all_example_projects_transpile_and_match_php`: every runnable example/project is transpiled,
  executed by a real `php`, and its stdout asserted byte-identical to the interpreter's (⇒ all three
  backends identical, since `run ≡ runvm` is already gated). **Fails-not-skips:** `PHORGE_REQUIRE_PHP=1`
  makes a missing `php` a test **failure** (CI mode); unset, it skips *loudly* (logged), never a silent
  green. `PHORGE_PHP=<path>` overrides the binary. Examples using a not-yet-transpiled construct are
  loudly deferred (logged `DEFER`, counted), not silently passed. The two narrow self-skipping PHP
  round-trip tests in `tests/cli.rs` (and their if-let/opt!/match-optional siblings — five in all) are
  removed, subsumed by the oracle.
- **P0-1 — integer division.** `7 / 2` now transpiles to `__phorge_div(7, 2)` (a runtime helper:
  `is_int($a)&&is_int($b) ? intdiv : /`), matching Phorge's truncate-toward-zero integer `/`. PHP's
  always-float `/` previously made `7/2` print `3.5` instead of `3`, live in `operators.phg`.
- **P0-4 — float modulo.** `5.5 % 2.0` transpiles to `__phorge_rem(…)` (`is_int…? % : fmod`), matching
  Phorge's `fmod`-style float `%`. PHP's integer `%` previously printed `1` instead of `1.5`.
- **P0-3 — bool interpolation.** An interpolated value is coerced via `__phorge_str` (`is_bool ?
  "true"/"false" : (string)$v`), mirroring `Value::as_display`. PHP's bool-in-string previously printed
  `1`/`` (empty) instead of `true`/`false`, live in `control-flow.phg`/`operators.phg`.
- **P0-2 — operand grouping.** Compound operands of unary/binary ops are now parenthesized
  (`a - (b - c)` → `$a - ($b - $c)`, `!(a && b)` → `!($a && $b)`), so PHP precedence can't
  re-associate them.
- **QW-13 — empty/reversed ranges.** Ranges transpile through `__phorge_range($a, $b, $inclusive)`,
  which yields `[]` for an empty/reversed range (PHP's bare `range()` descends). The KNOWN_ISSUES
  caveat is removed.
- **P1-#9 — large ranges fault cleanly.** A range wider than the new single-sourced
  `value::MAX_RANGE_LEN` (10M) now faults `"range too large"` (classified `FaultKind::RangeTooLarge`,
  `agree_err`-gated on both backends) instead of OOM-aborting (exit 101). Length is computed with
  `checked_sub` (EV-7). `value::build_range` single-sources the size-guarded materialization for both
  backends.

The four P0 fixes use runtime PHP helpers (mirroring Phorge's type-driven value kernels) rather than a
transpiler-side static type resolver — no duplicated operand-type inference, no inference-completeness
risk. `run ≡ runvm` was always correct; the bug class was php-leg-only.

### M3 S3 (Track A) — lambdas, first-class functions, and the pipe operator

- **Lambdas / closures.** `fn(int x) => x * 2` (expression body, return type inferred) and
  `fn(int x) -> int { … }` (statement body, explicit `-> T` required, `E-LAMBDA-THIS` if it touches
  `this`). Free enclosing locals are captured **by value** (the heap is immutable + acyclic, so no GC
  is needed). New surfaces: `Ty::Function` / `Type::Function`, `Expr::Lambda` + `LambdaBody`,
  `ast::free_vars`, `Value::Closure`, `CTy::Fn`, and two VM ops `Op::MakeClosure` / `Op::CallValue`.
- **First-class function values.** A bare named function is a value — `twice(3, dbl)` passes `dbl`
  itself; the function type is `(int) -> int`. On the VM a named-fn reference compiles to a
  zero-capture `MakeClosure`; the transpiler emits a PHP first-class callable `dbl(...)`.
- **Pipe operator `|>`.** `x |> f ≡ f(x)`, left-associative, **lowered to a plain call in the
  parser** (no new `Op`, no new backend semantics; the four dead `BinaryOp::Pipe` stubs are retired
  to `unreachable!`). `5 |> dbl |> inc` is `inc(dbl(5))`; `1 + 2 |> dbl` is `dbl(1 + 2)`.
- **Transpile targets** (Phorge : PHP :: TypeScript : JavaScript): expression lambda → arrow fn
  `fn($x) => …`; statement lambda → `function($x) use ($cap) { … }` (by-value `use`); named-fn ref →
  first-class callable; a lambda literal in call position → `(fn(…) => …)(args)`.
- All byte-identical on `run`/`runvm` and round-tripped through real PHP 8.6. Example:
  `examples/guide/lambdas-pipe.phg`. Deferred refinements (this-capture, cross-package value refs,
  block-body return inference, function-type variance, `core.list` map/filter/reduce) are recorded in
  `KNOWN_ISSUES.md`.

### M6 slices W2–W4 — routing, the serve runtime, and `phg serve`

- **W2 — static router (pure Phorge, no new feature).** A data-driven `List<Route>` table is scanned
  linearly for an exact `(method, path)` match, yielding a `Handler` enum tag dispatched by an
  exhaustive `match` to named handler functions; a method-sensitive 404 fallback. Routing is fully
  expressible with today's enums + classes + lists + `match`, so it is byte-identical on `run`/`runvm`
  and round-trips through real PHP. Example: `examples/web/router.phg`.
- **W3 — the serve runtime (`src/serve.rs`), the determinism quarantine.** The one module holding
  sockets + wall-clock non-determinism, deliberately **outside** `tests/differential.rs`. A `Transport`
  trait (`recv`/`send`) seams the loop from the world; `TcpTransport` is the real single-threaded
  socket (`Connection: close`, CRLFCRLF + `Content-Length` framing capped at 8 MiB, EV-7 no-panic).
  `serve()` routes each raw buffer through the program's single entry `respond(bytes) -> bytes`,
  degrading a request fault to a 500. **Single-threaded by force** — the `Rc`-shared heap makes runtime
  values non-`Send`, so a thread pool is impossible; true concurrency awaits M6 green-threads under the
  unchanged contract.
- **`interpreter::call_named(program, name, args)`** — invoke a named top-level function with a
  constructed argument (reuses `run_call`). The interpreter is the reference backend and `run ≡ runvm`
  guarantees the VM would agree, so a VM `call_named` (no return-value capture today) is deferred. No
  new `Op`, no new `Value` variant.
- **W4 — `phg serve <file> [--addr 127.0.0.1:8080]`.** Loads the program project-aware (like `run`),
  type-checks it, then runs the blocking HTTP serve loop on the 256 MB deep-stack worker (so the
  interpreter's `MAX_CALL_DEPTH` guard has the same headroom `run`/`runvm` rely on). Per-command
  `--help` with worked examples. Built binaries still ignore argv.
- **PHP bridge (`php -S`).** `examples/web/server.php` is a hand-written front-controller that builds a
  `Request` from PHP superglobals and calls the *transpiled* `handle(Request) -> Response` — the same
  value unit `phg serve` calls natively. The superglobal↔`Request` adapter is runtime glue, not
  transpiled (mirroring `src/serve.rs`). Documented end-to-end in `examples/web/README.md`.
- **Example** `examples/web/server.phg` — the full served app (W1 parse/serialize + W2 routing + the
  `respond` entry + `handle`); its `main()` exercises `respond` on canned `b"…"` requests so it stays
  byte-identical on `run`/`runvm` + real PHP. **Conformance** for the socket path lives in
  `tests/serve.rs` (an in-memory `FixtureTransport`, outside the byte-identity spine).

### M6 slice W1 — the HTTP handler model (`handle(Request) -> Response`, pure Phorge)

- **The portable handler contract** — `Request`/`Response` are ordinary Phorge classes and
  `parse_request(bytes) -> Request?` / `serialize_response(Response) -> bytes` are written in pure
  Phorge (PSR-7/15 shaped). Bodies are `bytes` (HTTP bodies are octets); the head is decoded ASCII for
  line/`:` splitting. Headers ride as `List<string>` raw lines with a `req.header(name) -> string?`
  linear-scan accessor (the method-call API is the public surface; a typed `Header` value arrives with
  S3). No socket yet — that is W3's `phg serve`. No new `Op`, no new `Value` variant.
- **`bytes.find(bytes, bytes) -> int?`** — first-occurrence byte search (`null` when absent, `0` for an
  empty needle, matching PHP 8 `strpos`); locates the CRLFCRLF head/body boundary. Erases to
  `(($p = strpos(…)) === false ? null : $p)`.
- **`text.split_once(string, string) -> List<string>`** — split on the first separator → `[head, tail]`
  (robustly parses `Name: value` headers whose value contains `:`). Erases to `explode($sep, $s, 2)`.
- **Example** `examples/web/handler.phg` — builds a canonical request as a `b"…"` literal, parses it,
  runs `handle`, and serializes the response (Content-Length recomputed from the body). Byte-identical
  on `run`/`runvm` + **real PHP**, auto-gated by the `examples/**/*.phg` glob.

### CLI binary renamed `phorge` → `phg`

- The CLI binary is now **`phg`** (matches the `.phg` extension; ripgrep's model — package `ripgrep`
  ships binary `rg`). All help/usage/version output, the cross-build `--bin`/artifact/cache names,
  release-asset naming, and docs use `phg`. The Cargo **package/lib name stays `phorge`**, as do
  `phorge.toml`/`phorge.lock`, the `.phorge` executable section, `PHORGE_*` env vars, and the
  `~/.cache/phorge` stub namespace.

### M6 slice W0 — the `bytes` type

- **`bytes`** — a new primitive: raw octet sequences distinct from UTF-8 `string`. `Value::Bytes`
  is `Rc`-shared (like `List`); `Ty::Bytes` is a built-in type name. No new `Op` — a `b"…"` literal
  rides the constant pool (`Op::Const`), interop rides `Op::CallNative`, `==` rides `Op::Eq`.
- **`b"…"` literals** — raw byte strings (no interpolation), escapes `\n \t \r \\ \"` plus `\xHH`
  (two hex digits → one arbitrary octet, so a literal can hold non-UTF-8 bytes).
- **`Core.Bytes`** interop module (`import Core.Bytes;`): `from_string(string) -> bytes`,
  `to_string(bytes) -> string?` (UTF-8 decode; `null` on invalid — composes with S2 `??`/if-let,
  never a fault), `len(bytes) -> int` (BYTE count, vs `Core.Text.len`'s character count),
  `concat(bytes, bytes) -> bytes`, `slice(bytes, int, int) -> bytes` (half-open, bounds-clamped —
  total, no fault).
- **Transpile** — `bytes` erases to PHP `string` (PHP strings are byte arrays); `b"…"` → a PHP
  double-quoted literal with `\xHH` preserved; the natives map to `strlen`/`mb_check_encoding`/`.`/
  `substr`. Example `examples/guide/bytes.phg` runs byte-identically on `run`/`runvm` + **real PHP**.
- First slice of the **M6 web-capabilities spike** (design-locked,
  `docs/specs/2026-06-18-m6-web-design.md`); bytes was pulled forward so HTTP bodies can be honest
  octets.

### M5 slice S3 — git dependencies + `phorge.lock` + `phg vendor` + auto-offline

- **`phg vendor`** — the only network-touching command. It clones each `[require]` git dependency
  at its pinned `tag`/`rev`, copies the dependency's source into `vendor/<vendor>/<package>/`, and
  writes `phorge.lock` pinning the **resolved commit SHA** + an FNV-1a-64 content hash. Idempotent and
  crash-safe (stages into a temp dir, swaps atomically, touches only each dependency's own subtree).
- **`phorge.lock`** (`src/lock.rs`) — a strict, deterministic TOML-subset lockfile (`[[package]]`
  blocks: `name`, `git`, `rev`, `hash`); round-trips through its own parser.
- **Auto-offline resolution** — `loader::load_project` merges vendored packages exactly like
  first-party library packages (mangle + resolve before any backend runs ⇒ `run` ≡ `runvm`
  structural; the transpiler de-mangles into `namespace …` blocks). `run`/`check`/`transpile`
  **never fetch** — they read the committed `vendor/`. New guards: `E-VENDOR-MISSING` (a `[require]`
  dep not vendored), `E-VENDOR-MAIN` (a vendored `package Main`), `E-DUP-DEF` (a duplicate
  `(package, name)` after the merge — previously a silent overwrite).
- **Example** — `examples/project/withdeps/` (a project consuming a vendored `acme/strutil` library):
  ships its committed `vendor/` + `phorge.lock`; the project-aware differential harness loads it
  offline and gates `run` ≡ `runvm`, and it round-trips through real PHP. `phg vendor` gains a
  `--help` entry, USAGE/dispatch wiring, and three `phg explain` codes.
- **Tests** — `tests/vendor.rs` drives the real `git clone`/`checkout`/`rev-parse` path against a
  `file://` local-git fixture (offline, deterministic): fetch + lock + offline byte-identical load,
  idempotent re-vendor, and `E-VENDOR-MISSING`.

### M5 slice S2d — project-aware differential harness + public multi-file example

- **First public multi-file project** — `examples/project/tempconv/` (a two-package Celsius→Fahrenheit
  converter) showcases the M5 project model end-to-end: mandatory packages + folder=path, a
  cross-package qualified call (`convert.c_to_f(0)`), import aliasing (`import acme.label as fmt;` →
  `fmt.tag(...)`), and a same-package bare call across two files. Plus `examples/project/README.md`.
- **Project-aware byte-identity gate** — `tests/differential.rs` now discovers every project root (a
  directory with a `phorge.toml`) under `examples/`, loads it through `loader::load`, and asserts
  `run` ≡ `runvm` (and that it runs). The single-file glob is made project-aware — it stops descending
  into any directory holding a `phorge.toml`, so project files are never run standalone (structural,
  name-independent; flat examples keep their `len() >= 3` floor). A project added later is auto-gated.
- **Verified** — the example runs `freezing = 32F` / `boiling = 212F` byte-identically on `run`,
  `runvm`, **and real PHP 8.6** (exact integer math, chosen so PHP's float `/` agrees).
- Docs refreshed for shipped multi-file support: `examples/README.md` (index + matrix rows; the two
  "arrives in a later slice" notes corrected) and `FEATURES.md` (Modules/packages → 🚧, git deps = S3).

### M5 slice S2c — qualified cross-package calls + namespaced PHP + import aliasing

- **Cross-package calls resolve** — `import acme.util;` then `util.compute(x)` now works across files.
  A new resolution pass in the loader (`src/loader.rs`) mangles every non-`main` definition to a
  globally-unique name (`acme.util` + `compute` ⇒ `Acme\Util\compute`; `package Main` defs stay bare),
  then rewrites call sites against each file's package + import map: same-package bare calls and
  qualified user calls become bare calls on the mangled name. Native `core.*` calls are untouched.
- **Import aliasing** — `import a.b as c;` binds the call-site leaf `c` (AST `Item::Import.alias`,
  parsed as a contextual `as` keyword so `as` stays a valid identifier). Resolves leaf collisions (O-9).
- **Namespaced PHP emission** (M5-7/M5-8) — a multi-package program transpiles to one
  `namespace Acme\Util { … }` brace-block per package + a `namespace Main { … }` block + a nameless
  `namespace { \Main\main(); }` bootstrap. Cross-package calls emit fully-qualified (`\Acme\Util\compute`);
  global-function natives gain a leading `\`. A single-package program has no mangled names and stays on
  the flat path — byte-identical to the pre-S2c output.
- **S2c scope: functions only** — a `class`/`enum` in a non-`main` (library) package is rejected
  (`E-PKG-TYPE`); cross-package type namespacing is an M5 follow-up. The S2b bare cross-package call
  interim is tightened: an unqualified cross-package call now fails on both backends.
- **Byte-identity** — resolution runs in the loader *before* any backend, so checker/interpreter/
  compiler/VM are unchanged (run==runvm is structural). Verified end-to-end: a two-file project runs
  `42` on `run`, `runvm`, **and real PHP 8.6** (`php out.php`).
- **`explain`** gains `E-PKG-TYPE` and `E-PKG-PATH` (the latter backfilled from S2b).
- 7 new tests (`tests/project.rs` qualified/alias/same-package-cross-file/unqualified-rejection/
  type-rejection/transpile-structure + a `native.rs` alias-`import_map` case). 409 total green.

### M5 slice S2b — multi-file loader + folder=path enforcement

- **Project loader** (`src/loader.rs`) — resolves an entry source to one `Unit` (a single, possibly
  multi-file-merged `Program` + the source text for diagnostics). **Project mode**: a `phorge.toml`
  found by walking up marks the root; every `.phg` under the source root is parsed, validated against
  its location (**folder = package**, Go's model — `src/acme/util/*.phg` ⇒ `package acme.util`;
  `package Main` is folder-exempt), and all items are merged into one flat program. **Loose mode** (no
  manifest above): only `package Main;` runs — a dotted library package requires a project.
- **`E-PKG-PATH`** — a file whose package does not match its directory under the source root, a dotted
  package sitting directly in the source root, or a non-`main` package living outside the source root.
- **Byte-identity preserved** — enforcement is path-aware and lives in the loader, never in the type
  checker, so `cli::cmd_run(&str)` and the differential harness are untouched. `run`/`runvm`/`check`/
  `transpile` route a `<file>` source through the loader (new `cli::run_program`/`runvm_program`/
  `check_program`/`transpile_program` consume the loaded program); `-e`, stdin, `parse`, `lex`,
  `disasm`, `bench`, and `build` keep the single-file string path. A loose single-file program through
  the loader produces identical output to the pre-S2b pipeline.
- **Flat-merge interim** — until S2c, the merged items share one flat namespace, so a cross-file call
  resolves **unqualified**; qualified cross-package calls (`util.parse(x)`) + one-brace-block-per-package
  PHP emission + import aliasing are S2c. `transpile` of a multi-*package* project therefore emits flat
  PHP for now (correct for `package Main` / single-package). Multi-file type-error diagnostics omit the
  source-line caret (no single aligned source). The `examples/project/` showcase ships at S2d.
- 12 new tests (9 `loader` unit + 3 `tests/project.rs` integration, incl. a multi-file project running
  byte-identically on both backends).

### M5 slice S2a — project manifest + source root + project detection

- **`phorge.toml` manifest** — new `src/manifest.rs` parses a minimal, std-only TOML subset into
  `Manifest { name, version, source, require, require_dev }`. The manifest speaks **Composer's
  vocabulary in an honest TOML container**: `name = "vendor/package"` (doubles as the PSR-4 namespace
  root — `acme/myapp` ⇒ `Acme\Myapp`), `[require]` / `[require-dev]` sections, dependency values as
  `{ git = "…", tag|rev = "…" }` or the `"<git-url>@<tag>"` string shorthand. Each dep self-locates
  via its git URL (no Packagist, no Composer `repositories` side-table); versions are **exact-pin
  only** — a `branch` pin, a missing/double pin, an unknown key/section, or an unquoted value are hard
  errors. A literal `composer.json` was rejected on purpose: the `composer` tool cannot process it, so
  the filename would be a false promise.
- **Project detection** — `Project::detect(path)` walks up from a source file/dir for a `phorge.toml`;
  the first one found marks the project root and resolves the source root (`root/<source>`, default
  `src`). No manifest above ⇒ `Ok(None)` (loose-script mode). Manifest presence is the sole
  project-vs-loose signal (Go's model).
- **Byte-identity preserved** — S2a is parse + represent only; nothing consumes the manifest yet, so no
  `.phg` execution path changes and `run`/`runvm` stay byte-identical. The multi-file loader +
  folder=path enforcement (S2b), qualified cross-package calls + brace-namespace PHP (S2c), and the
  `examples/project/` showcase (S2d) follow. Coverage = 18 `manifest` unit tests (the showcase example
  ships with the observable behavior at S2d).

### M5 slice S1 — package declaration (project-model foundation)

- **Mandatory `package` declaration** — every file declares its package as the first line, never
  inferred (`package app.util;`). The reserved **`package Main;`** is the runnable entry (Go's model;
  pairs with `fn main()`); `core` is reserved for the standard library. New checker codes
  `E-NO-PACKAGE` / `E-RESERVED-PACKAGE` (both `phg explain`-documented). The parser captures the
  path on `Program.package`; a `package` after any item is a parse error (it must be first).
- **Byte-identity preserved** — S1 is front-end only: the interpreter, VM, and transpiler ignore the
  package (flat PHP emission unchanged — `package Main` → no namespace), so `run`/`runvm` and the PHP
  round-trip stay byte-identical. Multi-file projects, strict folder=path, cross-package imports, and
  brace-namespace PHP emission arrive in later M5 slices
  (`docs/specs/2026-06-18-m5-project-model-design.md`).
- All 24 examples + every test program migrated to `package Main;`; the minimal program is now
  `package Main;` + `import Core.Console;` + `Console.println`. (Also fixed pre-existing Wave-1 doc
  drift: `README.md` showed `import std.io;` + bare `println`.)

### M3 slice S0 — developer experience

- **`var` local type inference** — `var x = expr;` infers the binding's type from its initializer
  (still fully static + immutable). The VM derives the local's operand type from the initializer, so
  arithmetic on a `var` still specializes (`AddI`/`AddF`); `ctype` now also resolves a `match` value.
- **`type` aliases** — `type Name = T;`, compile-time only. The checker resolves aliases (with cycle,
  built-in-shadow, and duplicate detection); a post-check pass (`checker::expand_aliases`) expands
  them out of the AST so the interpreter, VM, and transpiler all see alias-free types and the PHP
  output never mentions the alias.
- **Sharper diagnostics** — front-end (lex/parse/type) errors render the offending source line with a
  caret, attach a "did you mean `…`?" hint (nearest in-scope name, Levenshtein ≤ 2), and carry a
  stable code. `Diagnostic` gains `code`/`hint` fields + a `render` method; all construction is
  centralized through `Diagnostic::new`. Runtime-error strings are unchanged (differential parity).
- **`phg explain <CODE>`** — print the explanation for a diagnostic code (`E-UNKNOWN-IDENT`,
  `E-UNKNOWN-TYPE`, `E-INFER-NULL`, `E-ALIAS-CYCLE`).
- **Per-command help** — `phg <command> --help` / `-h` prints a description, the source/flag forms,
  and 1–2 worked examples.
- New guide example `examples/guide/inference.phg` (auto byte-identity-gated by the differential
  harness).

### M3 slice S1 — core ergonomics

- **List indexing `xs[i]`** — un-rejected in both backends (the checker already typed it), reusing the
  bounds-checked `Op::Index`. An out-of-range read is a clean `list index out of range` runtime fault,
  byte-identical across `run`/`runvm` (classified `FaultKind::IndexOob` in the differential harness).
  Transpiles to `$xs[$i]`.
- **Integer ranges `a..b` / `a..=b`** — exclusive / inclusive integer ranges, materialized to a
  `List<int>` by the one new `Op::MakeRange(bool)` (which extends the three coupled matches —
  `vm::exec_op`, `compiler::stack_effect`; `chunk::validate` needs no arm: no static index). Both
  backends build the list via Rust's native `start..end` / `start..=end` (no counter overflow), so
  `for (int i in 0..n)` works unchanged. The lexer adds `..` / `..=` (longest-match). Transpiles to PHP
  `range()`; a non-int bound is `E-RANGE-TYPE` (a `phg explain` entry).
- **Expression `if`** — `if (c) { e } else { e }` in value position (`var x = if (c) { 1 } else { 2 };`).
  Parens + a mandatory `else`; single-expression arms. Disambiguated from the statement `if` by parse
  position; lowers to the existing branch ops (no new `Op`); transpiles to a PHP ternary.
- New guide example `examples/guide/ergonomics.phg` (indexing + ranges + expression `if`),
  auto byte-identity-gated and round-tripped through real PHP.
- **S1.4 (smart-cast narrowing) deferred to S2** — it only narrows optionals (`T?`), which arrive in S2.

### M3 slice S2 — null-safety

PHP-native nullable with a compile-time non-null guarantee (TypeScript `strictNullChecks` over PHP's
nullable runtime). `T?` is the existing `null` value at runtime; the guarantee lives in the checker
(a non-optional `T` can never be `null`). All byte-identical on `run`/`runvm` and 1:1 to PHP.

- **Optionals `T?` + non-null discipline** — `Ty::Optional` + `Value::Null`; `T` auto-widens to `T?`,
  but a `T?` cannot flow into a non-optional `T` (`E-OPT-ASSIGN`), nor be used as an operand/receiver
  without unwrapping (`E-OPT-USE`).
- **`??` null-coalesce** — `a ?? b`; `?.` safe access — `opt?.member` / `opt?.method()` short-circuits
  a null receiver to `null` (PHP `?->`). Both lower to a null-test + branch, **no new `Op`**.
- **`if (var x = opt)`** — binds the non-null inner `T` (smart-cast S1.4) inside the then-block only;
  `E-IF-LET-TYPE` on a non-optional scrutinee. Transpiles to `if (($x = E) !== null) { … }`.
- **`opt!` checked force-unwrap** — `T?` → `T`, a clean `force-unwrap of null` fault on null (never a
  crash; `FaultKind::ForceUnwrap` parity). `E-OPT-UNWRAP` on a non-optional; the **`W-FORCE-UNWRAP`**
  lint flags every use. Transpiles to a once-per-file `__phorge_unwrap()` helper.
- **`match` over `T?`** — `match opt { null => …, v => … }` is exhaustive; the binding arm narrows
  `v` to the non-null inner after a `null` arm.
- **Warning channel (first lint)** — the checker now collects non-fatal warnings; `check()` returns
  them on success and the CLI renders them to stderr without gating the build.
- **No new `Op` variant** — `Op::MatchFail` was generalized to `Op::Fault(FaultMsg)` (single-sourced
  message), serving both match-exhaustiveness and `opt!`-on-null.
- New guide example `examples/guide/null-safety.phg`, auto byte-identity-gated + PHP round-tripped.

### M3 Track B Wave 1 — namespaced native foundation

- **Everything is namespaced — "nothing in the wind".** The free global `println` is retired. A
  program now `import Core.Console;` and calls `Console.println(...)`. Stdlib modules are reserved
  under the `core.*` root; the root lives in the import and the leaf qualifies the call (Go's
  `import "fmt"` → `fmt.Println`). Explicit import is required even for the stdlib.
- **`native` registry** (`src/native.rs`) — each built-in single-sources its four facets in one
  entry keyed by `(module, name)`: checker signature (`params`/`ret`), a runtime `eval` shared
  verbatim by the interpreter *and* the VM (structural parity, like the value kernels), and a PHP
  emission mapping (`Console.println` → `echo … . "\n"`). Built once via `OnceLock`.
- **`Op::Print` → `Op::CallNative(idx, argc)`** — the migrated former print op now indexes the
  registry and pushes the native's result (extends the three coupled `Op` matches + a `validate`
  bound on the native index). No separate `Const(Unit)`.
- **Import-driven resolution across all four backends** — a member call `Console.println(x)` whose
  head is an imported module qualifier dispatches to the native: the interpreter and compiler resolve
  locals-first then by leaf (they track scope); the checker and transpiler use the import map.
- **Shadowing guard** — a value binding may not shadow an imported module qualifier (`E-SHADOW-IMPORT`),
  keeping the import-map-driven transpiler consistent with the locals-first run backends.
- Migrated every `println` call site — all examples, fixtures, and inline test programs — to
  `import Core.Console;` + `Console.println`. The example differential test now also asserts each
  example *runs* (`Ok`), not merely that the backends agree (closing a vacuous-green gap).

### M3 Track B Wave 2 — stdlib breadth (`Core.Math` / `Core.Text` / `Core.File`)

- **`Core.Math`** — `sqrt`/`pow`/`floor`/`ceil` (float) and `abs`/`min`/`max` (int). Concrete-typed
  (the registry's `params`/`ret` have no type variable, so no overloading); each erases to the PHP
  builtin of the same name. `abs` faults cleanly on `i64::MIN` (EV-7).
- **`Core.Text`** — `len`/`upper`/`lower`/`trim`/`contains`/`split`/`join`/`replace`. `split` returns
  `List<string>` and `join` consumes one (the type system already carries `List<string>` end to end).
  The PHP erasures reorder args where PHP differs (`explode`/`implode` separator-first, `str_replace`
  search-first).
- **`Core.File`** — `read` (→ `string?`, `null` on any failure — composes with the S2 `??` / if-let),
  `exists`, and `write`. File *reads* stay byte-identical by reading a **committed fixture**
  (`examples/guide/fixtures/poem.txt`); `write` is a non-deterministic side effect, unit-tested but
  kept out of the byte-identity-gated example set.
- Each module ships a byte-identity-gated guide example (`examples/guide/math|text|file.phg`),
  round-tripped through real PHP. `KNOWN_ISSUES` now documents the pre-existing irrational-`float`
  precision divergence that `Core.Math` makes easy to reach (Rust shortest-round-trip vs PHP's
  default `echo` precision); examples keep to exactly-representable values.
- **Deferred:** `core.list` (needs S3 lambdas / `List<T>` generics) and `core.json` (needs a dynamic
  `Json` type) — they land once generics or S3 exist.

_Next: Track B Wave 3 (user packages: `package` decl + folder=path + PHP `namespace` emission), then
Track A (S3 lambdas/pipeline). M2.5 Phase 3 (CI stub registry; opt-in `--sign`) remains parked._

## [0.4.0] — 2026-06-17

The first fully-documented release: CLI UX, profiling, a disassembler, cross-OS standalone builds,
and a complete OSS doc set.

### Profiling & introspection

- `phg bench` now reports **memory** alongside timing: peak-RSS growth of one cold execution plus
  the process `VmHWM`/`VmRSS`, via a std-only, Linux-only `src/mem.rs` (`/proc/self/status` +
  `/proc/self/clear_refs`). Non-Linux hosts print `memory: unavailable on this platform`.
- `phg disasm <source>` — print the compiled bytecode: per-function instruction listings (index,
  source line, op, and a resolved annotation for index-carrying ops) plus the program-level
  enum/class/method descriptor tables.
- New profiling example `examples/bench/workload.phg` (CPU recursion + heap allocation) with
  `examples/bench/README.md` documenting how the time and memory numbers are collected.

### CLI UX

- `-v` / `--version` — print `phg <version>` and exit; `-h` / `--help` — full usage banner.
- Flexible program source for the run-family commands
  (`run`/`runvm`/`check`/`parse`/`lex`/`transpile`/`disasm`/`bench`): `<file>` | `-` (read from **stdin**) |
  `-e <code>` / `--eval <code>` (run **inline** source) | `--` (next arg is a path even if it starts
  with `-`).

### M2.5 Phase 2 — cross-OS standalone builds

- `phg build --target <triple>` / `--all` cross-compiles a runtime stub via
  [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) (zig as the linker) and embeds the
  program as a named object-file section. Targets: `x86_64-unknown-linux-musl`,
  `aarch64-unknown-linux-{gnu,musl}`, `x86_64-pc-windows-gnu`.
- `src/bundle.rs` → a `bundle/` module: CRC-guarded `container`, per-format readers `elf`/`pe`/`macho`
  (thin + fat), a magic-sniffing `section::find_section` dispatcher, and a `cross` orchestrator. The
  hand-rolled, std-only **PE/COFF**, **Mach-O 64**, and **fat/universal** readers use checked arithmetic
  (EV-7: adversarial input → `None`, never a panic) so a produced binary self-reads its own format.
- Stub cache keyed on an FNV-1a-64 of the phg binary's own bytes (a rebuilt phorge invalidates stale
  stubs, protecting the parity spine). Precise "missing rustup target" / "needs a source checkout"
  errors. apple/darwin targets are rejected with a clear message (macOS stub deferred to Phase 3; the
  Mach-O reader ships and is tested). `--sign` reserved for Phase 3.
- Cross-parity tests (toolchain-gated): `x86_64-musl` native-execution parity vs `runvm`, and a real
  windows-PE section round-trip.

### Documentation

- Full OSS project doc set: rewritten README, dual **MIT OR Apache-2.0** license, CONTRIBUTING,
  CODE_OF_CONDUCT, SECURITY, SUPPORT, GOVERNANCE, AUTHORS, ROADMAP, VISION, FEATURES, KNOWN_ISSUES,
  THIRD-PARTY-NOTICES, CITATION.cff, `.editorconfig`, and `.github/` templates.

Built standalone binaries are unchanged: they run their embedded program and ignore argv.

## [0.3.0] — 2026-06-16

First tagged POC. Usable end-to-end on `x86_64-linux-gnu`: the full M1 language on two
byte-identical backends (`run` interpreter + `runvm` bytecode VM), a Phorge→PHP transpiler, and
`phg build` producing a standalone native Linux executable. Bundles all post-M2-P3 work — the
P3.5 hardening pass, M2 P4 (classes/enums/match/methods), Wave 4 (class-aware compiler types), P5a
(`Rc`-shared heap), the full-coverage example set, and M2.5 Phase 1 (standalone build). Known v1
limits: `build` is host-only; the artifact ignores argv and always exits 0; the language has no
indexing/`Map`/`Set`/optionals/`|>`/exceptions/mutation (all M3).

### M2.5 Phase 1 — `phg build` (x86_64-linux-gnu) (2026-06-16) — **distribution**
`phg build foo.phg` produces a standalone host executable that runs `foo.phg` on the VM with no
Phorge install — by copying the running phg binary, embedding the program **source** in a
`.phorge` ELF section, and self-detecting + running that payload at startup. Same section+container
mechanism as the cross-OS end state (design §7). See
`docs/specs/2026-06-16-m2.5-phorge-build-design.md` + `docs/plans/2026-06-16-m2.5-phase1-build-linux-gnu.md`.

- **Added**
  - `src/bundle.rs` (std-only, zero new deps): a bitwise CRC-32, a versioned CRC-guarded payload
    **container** (`magic | version | header_len | kind | comp | enc | flags | len | payload_crc32 |
    header_crc32`), a hand-rolled **ELF64 section reader** (no `object`/`goblin` — it links into the
    produced binary, so it must stay zero-dep), and `embedded_source()` (graceful `None` on every
    malformed/tampered/absent input).
  - `cli::cmd_build` — validates the program (no broken binary is ever emitted), copies `current_exe`,
    and shells `llvm-objcopy --add-section .phorge=…` (override via `PHORGE_OBJCOPY`).
  - `phg build <file> [-o out]` CLI command; `main()` runs an embedded payload at startup before
    any arg parsing.
  - `tests/build.rs` — the parity spine extended to distribution: a built binary's output is
    byte-identical to `runvm`; argv is ignored (v1); ill-typed programs fail with diagnostics and
    emit no binary.
  - **Hardening (post-review):** the ELF64 reader uses fully-checked offset arithmetic — adversarial/
    malformed input returns `None`, never overflow-panics under the debug/test profile
    (regression-tested per EV-7); `phg build` rejects a dangling `-o`, an unrecognized flag, or any
    extra argument with a usage error (exit 2) instead of a silent default-named build. `docs/INVARIANTS.md`
    #1 now records the build binary as the third `cmd_runvm` parity surface.
- **Notes** (v1 limits) — host-only (`x86_64-linux-gnu`); the embedded program ignores argv and
  cannot set a custom exit code; the source is recoverable from the artifact (not obfuscated).
  Cross-targets (zig), PE/Mach-O reader arms + stub cache = Phase 2; CI stub registry + signing/
  notarization (rcodesign-from-Linux) = Phase 3.

### Examples — full-coverage showcase (2026-06-16) — **docs/tests**
A living example set covering the entire runnable language surface, plus the Phorge→PHP bridge. See
`docs/specs/2026-06-16-examples-coverage-design.md` + `docs/plans/2026-06-16-examples-coverage.md`.

- **Added**
  - Four real-world programs (`examples/realworld/{ledger,library,shop,rpg}.phg`) and six focused
    guide programs (`examples/guide/{operators,control-flow,collections,classes,enums-match,strings}.phg`),
    each exercising a different slice of the surface; an `examples/README.md` index + coverage matrix.
  - `examples/transpile/{demo.phg,demo.php,README.md}` — the Phorge→PHP transpile bridge (the only
    PHP-ecosystem path: output, not input), with a `tests/cli.rs::transpile_demo_matches_committed_php`
    snapshot test that fails on transpiler drift.
- **Changed**
  - `tests/differential.rs` now **globs `examples/**/*.phg`** instead of listing examples explicitly,
    so every current and future example is byte-identity-gated with no test edit.
- **Notes** (honest boundary, documented in `examples/README.md`)
  - Zero-payload enum variants need call form `V()` to construct **and** in a `match` pattern — a
    bare `V =>` arm is a catch-all binding (a silent logic bug both backends agree on).
  - `import` is decorative (no module resolution until M5); `null`/`T?`/`Map`/`Set`/`|>`/exceptions
    /traits/overloading remain M3+ and are deliberately absent.

### M2 P5a — `Rc`-shared heap objects (2026-06-16) — **object-path perf**
Makes compound heap objects *shared* instead of *deep-cloned*. The M1 heap is immutable + acyclic
(no reassignment, no field mutation, args evaluated before the instance exists), so `Rc` is both
sufficient and complete for reclamation — `Drop` frees everything, no cycle can leak, no tracing
collector is needed (that stays deferred to M3). See
`docs/specs/2026-06-16-m2-p5-object-model-design.md` + `docs/plans/2026-06-16-m2-p5a-rc-shared-heap.md`.

- **Changed**
  - `Value::Instance(Rc<Instance>)`, `Value::Enum(Rc<EnumVal>)`, `Value::List(Rc<Vec<Value>>)`
    (were `Box`/`Vec`). Cloning a `Value` — the `Op::GetLocal` hot path and every interpreter
    var-read — is now an O(1) refcount bump instead of a deep `HashMap`/`Vec` copy. The constructor
    now shares one `Rc` between the `this` receiver and the returned instance (no double build).
  - Three move-out sites adjusted (can't move out of an `Rc`): `vm.rs` `GetEnumField`
    (`into_iter().nth` → `.get().cloned()`), the interpreter's list `for` (iterate by ref + clone),
    and the ctor double-build (folded into one shared `Rc`). No `Op`/bytecode/AST/checker change.
- **Perf** (`phg bench`, median of 101, `fib(28)`)
  - Object-heavy VM run **1537 ms → 634 ms (2.4× faster)**; the VM's advantage over the tree-walker
    recovered from **4.73× → 9.35×**, essentially on par with the scalar baseline (10.92×) — i.e.
    the object-path penalty (deep-clone-on-load) is largely eliminated.
  - **Phase B deferred (bench-gated, not opened):** slot-indexed `Vec` field layout. With the object
    path now ~within scalar's advantage, field access (HashMap lookup) is no longer dominating, so
    there is no evidence to justify the larger interpreter-touching change.
- **Parity** — behavior-preserving refactor; the full differential suite + examples sweep stay
  byte-identical (244 tests green), clippy + fmt clean, `#![forbid(unsafe_code)]` intact.

### M2 Wave 4 — Class-aware compiler types (2026-06-16) — **closes the last `num_ty` parity gap**
Makes the compiler's operand-type inference class-aware, so the VM no longer rejects checker-valid
programs that read a field of an arbitrary instance, a method-call result, or a nested member as an
arithmetic operand. `runvm` is now a faithful drop-in across the full checker-valid surface. See
`docs/plans/2026-06-16-m2-wave4-compiler-types.md`.

- **Changed**
  - The compiler's coarse `enum TyTag { Int, Float, Other }` became `enum CTy { Int, Float,
    Class(String), Other }` — an instance now carries *which class* it is, derived structurally from
    the AST's declared `Type` annotations (`type_tag` → `resolve_cty`); the AST, the `Op` set, the
    VM, and `value.rs` are untouched.
  - `num_ty` is now the numeric projection (`as_num`) of a new recursive `ctype(&Expr)` resolver
    that walks `Ident`/`This`/`Member`/`Call` to a class-aware type. New per-program tables —
    `class_field_ctys` (class → field → type) and `method_rets` (`(class, method)` → return type) —
    plus a `cur_class` on the compiler back the `Member`/method-call/`this` resolution. The
    P4c-era `this.field`-only `num_ty` `Member` arm is subsumed by the general resolver.
- **Parity**
  - Five programs that ran on the interpreter but failed to *compile* on the VM now agree
    byte-identically (`tests/differential.rs::WAVE4_PROGRAMS`): a field of an arbitrary instance
    (`p.x + 1`), a method result (`c.get() + 1`), a nested field (`a.inner.x + 1`), a class-typed
    enum payload bound in `match` (`Some(p) => p.x + 1`), and a free function returning an instance
    (`mk().x + 1`).
  - The only remaining coarse-type note is the deliberately out-of-M1-surface `Index` (`xs[i]`
    arithmetic faults on both backends — M1 has no user indexing).

### M2 P4c — Methods + `this` on the VM (2026-06-16) — **M2 P4 complete**
Brings instance methods and `this` to the bytecode VM. With this, **`runvm` covers the full M1
language surface** and `examples/grades.phg` runs on both backends. See
`docs/plans/2026-06-16-m2-p4-classes-enums-match.md`.

- **Added**
  - `Op::CallMethod(name_idx, argc)` — runtime method dispatch off the receiver instance's class,
    via a program-level `(class, method) → function index` table; the frame opens with the
    receiver at slot 0 (`this`).
  - Methods compile to functions (receiver at slot 0, params at `1..=argc`); `this` and bare field
    reads inside a method/ctor body resolve against the receiver.
  - `examples/grades.phg` joined the differential examples sweep; `phg bench examples/grades.phg`
    runs (VM ≈3.2× the tree-walker on it).
- **Removed**
  - The last two `(M2 P4)` compile-error stubs (`Expr::This`, method calls) — `grep "M2 P4"` in
    `compiler.rs`/`vm.rs` is now clean.
- **Parity notes**
  - Method existence is checker-enforced, so the VM's method-not-found fault is a defensive
    backstop (no `agree_err` case, like P4a's exhaustiveness).
  - `num_ty` now classifies a `this.field`/bare-field arithmetic operand (via the class's field
    tags). At this commit a field read on an *arbitrary* instance was still the coarse-`TyTag` gap;
    **closed in M2 Wave 4** (see the Wave 4 entry above) by making the type class-aware (`CTy`).

### M2 P4b — Classes on the VM (2026-06-16)
Brings class construction (with constructor promotion + body side effects) and field reads to the
bytecode VM. See `docs/plans/2026-06-16-m2-p4-classes-enums-match.md`.

- **Added**
  - `Op::MakeInstance` (build a `Value::Instance` from promoted-field values) and `Op::GetField`
    (runtime field lookup, with a `no field` fault byte-identical to the interpreter).
  - A program-level `ClassDesc` table (per-class promoted-field names) and an interned
    field-name pool, both validated by `BytecodeProgram::validate`.
  - Each constructor compiles to a synthetic `<Class>::new` function: it promotes its params into
    fields via `MakeInstance`, runs the body for side effects with the instance in scope, and
    returns the instance. `ClassName(args)` resolves to a `Call` into it.
- **Object model**
  - Instances are value-native: the VM reuses the shared `Value::Instance`, clone-on-use,
    mirroring the interpreter (decision P4-1). No arena.
- **Parity notes**
  - A ctor body's `return` is discarded and the promoted instance is always returned (interpreter
    parity): the synthetic ctor redirects body `return`s to an epilogue that loads + returns the
    instance, so an early `return;` cannot change the result.
  - Reading an explicit (uninitialized) `Field` member type-checks but faults `no field` at
    runtime on **both** backends — construction populates only promoted ctor params.
- **Known limitation at this commit (coarse-type gap — since closed in M2 Wave 4)**
  - A field read used as the *direct left operand* of arithmetic (`p.x + …`) couldn't be classified
    by the compiler's coarse `TyTag`. Field reads worked everywhere else: interpolation, equality,
    call arguments, arithmetic right-operand, or bound through a typed local first. **M2 Wave 4
    closed this** by making the compiler's type class-aware (`CTy`); see the Wave 4 entry above.
  - `examples/grades.phg` still needs P4c (it calls an instance method).

### M2 P4a — Enums + `match` on the VM (2026-06-16)
Brings single-payload enums and exhaustive `match` to the bytecode VM (already in the
interpreter since M1). See `docs/plans/2026-06-16-m2-p4-classes-enums-match.md`.

- **Added**
  - `Op::MakeEnum`/`MatchTag`/`GetEnumField` (enum construction, variant tag test, payload
    extraction) + `Op::MatchFail` (checker-unreachable non-exhaustive backstop, byte-identical
    to the interpreter's fault).
  - A program-level `EnumDesc` table (the enum analogue of the constant pool), validated by
    `BytecodeProgram::validate`.
  - Compiler operand-height tracking, so a `match` used mid-expression (e.g. as a binary
    operand, or nested in another arm) spills its scrutinee to the correct stack slot.
- **Object model**
  - Enums are value-native: the VM reuses the shared `Value::Enum`, clone-on-use, mirroring the
    interpreter (decision P4-1). No arena — deferred to a bench-gated perf milestone.
- **Known limitation (pre-existing, shared by both backends)**
  - `match` cannot appear inside string interpolation — the lexer's `{…}` interpolation does not
    nest a `match`'s braces. Not a parity issue (both backends reject it identically).

### M2 P3.5 — Hardening (in progress, 2026-06-16)
Closing the parity/no-crash contract gaps before P4 widens the surface. See
`docs/plans/2026-06-16-m2-p3.5-hardening-roadmap.md`.

- **Added**
  - `phg bench <file>` — median-of-N timing of both backends, output-identity gated; measures
    the "VM faster than tree-walker" thesis (≈10× on `examples/fib.phg`) instead of asserting it.
  - `agree_err` error-parity oracle in the differential harness (faults classified by semantic
    `FaultKind`).
  - Central `src/limits.rs` (recursion/nesting caps + numeric-width policy); unified
    `diagnostic::Diagnostic` for all stages; `BytecodeProgram::validate`; `docs/INVARIANTS.md`,
    `docs/ARCHITECTURE.md`; `rust-toolchain.toml`.
- **Changed**
  - Arithmetic/comparison single-sourced into `value.rs` (both backends call the same kernels).
  - VM runtime errors now carry the source line (`Chunk.lines`).
  - Constant pool interns scalar duplicates.
  - `interpreter::Frame` → `CallScopes` (removes the name collision with `vm::Frame`); scope-verbs
    unified (`push_scope`/`pop_scope`).
  - Quality gate is now compile-time (`warnings = "deny"`, `clippy.all = "deny"`,
    `#![forbid(unsafe_code)]`) + a tracked pre-commit hook.
- **Fixed**
  - `Op::Neg` on `i64::MIN` aborted the VM (P0) — now a clean `integer overflow` fault, matching
    the interpreter.
  - Interpreter/parser/checker no longer SIGABRT on deep recursion/nesting — explicit limits fault
    cleanly.
  - Determinism: checker's non-exhaustive-`match` error sorts its missing-variant list.

## M2 — Bytecode + VM (P1–P3, 2026-06-16)
- **P1** — `Chunk` + typed `Op` enum + stack VM dispatch loop.
- **P2** — AST→bytecode compiler for the `main`-only surface + `phg runvm` + the differential
  harness (`runvm` byte-identical to `run`).
- **P3** — user function calls, clox-style call frames, recursion/mutual recursion; `examples/fib.phg`
  runs on the VM.

## M1 — Tree-walking interpreter + transpiler — 2026-06-15 (`9da6e56`)
- Full pipeline: lexer → parser → type-checker → tree-walking evaluator.
- Phorge → PHP transpiler, round-trip-verified against real PHP.
- CLI: `phg <run|check|parse|lex|transpile>`.
- Language surface: static types, immutable-by-default bindings, functions, classes + constructor
  promotion, single-payload enums + exhaustive `match`, string interpolation, `List<T>` literals,
  `for…in`, checked int/float arithmetic. 162 tests green at the tag.
