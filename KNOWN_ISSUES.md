# Known Issues & Limitations

Phorge is pre-1.0. This page lists current limitations and known rough edges. Most "limitations" are
**deliberate scope boundaries** — features that are *planned* (see [ROADMAP.md](ROADMAP.md)) rather
than broken. The key property is that out-of-scope constructs are **rejected cleanly** (a type or
parse error, non-zero exit) — never a crash.

## Language features not yet implemented

These are designed but not in the current surface; using them produces a clean compile-time error,
not a panic:

- `Map` / `Set` / tuples
- `instanceof` against **unions or intersections** (class operands ship in M-RT S1 and interface
  operands in M-RT S2 — see *Behavioral quirks* below; testing against unions/intersections lands
  with those features in later M-RT slices)
- **interfaces in a library (non-`main`) package** — like classes/enums, an interface must live in
  `package main` this slice (`E-PKG-TYPE`); cross-package types are a later slice
- Exceptions (try / catch / throw)
- Mutation (reassignment and field writes) — Phorge is immutable-by-default today
- Method/function overloading, traits, operator overloading, property accessors
- Sized integers / `decimal`, `const`/`final` enforcement
- `match` outside return / variable-declaration-initializer position

## Lambdas & first-class functions (M3 S3) — deferred refinements

Lambdas (expression + statement body), higher-order functions, first-class named-function
references, and the pipe operator `|>` all ship in M3 S3 and are byte-identical on `run`/`runvm`
and round-trip through real PHP. These refinements are deliberately deferred (each rejected cleanly
or simply unavailable, never a crash):

- **A lambda cannot reference `this`** — rejected with `E-LAMBDA-THIS` (`phg explain E-LAMBDA-THIS`).
  Workaround: `var self = this;` before the lambda, then capture `self`.
- **Lambdas and first-class function references are supported in `package main` (and single-file
  programs), not yet inside library (non-`main`) packages.** The M5 loader's name-mangling pass
  rewrites *call sites*, but not a bare function reference used as a *value* nor the body of a lambda,
  so a same-package call inside a lambda body — or a bare named-fn value — declared in a dotted
  library package is not rewritten to its mangled target. In practice this is rejected cleanly
  (`E-UNKNOWN-IDENT`); avoid lambdas / function values in library packages this slice (the guide
  example and every `package main` program are unaffected). Loader-resolving lambda bodies and
  fn-value references is a follow-up. Qualified / cross-package function *values* (passing
  `acme.util.compute` itself, vs. *calling* it) are likewise deferred — call them directly.
- **Statement-body lambdas require an explicit `-> T`** — the return type of a block-body lambda is
  not inferred (expression-body lambdas infer it from the expression). This is by design this slice.
- **Function-type assignability is exact structural equality** — no parameter/return variance
  (`(int) -> int` is not assignable to `(int) -> int?` etc.).
- **`core.list` higher-order helpers (`map`/`filter`/`reduce`) are not yet available** — they await
  the `List<T>`-generic native signatures; lambdas can already be passed to *user* functions today.

## core.html (Waves 1–3 — escape kernel + element builders + `html"…"` sugar)

- **An `html"…"` hole cannot contain a string literal with quotes.** Like every Phorge
  interpolation (`"…{e}…"`), the lexer scans to the first closing `"`, so a `"` inside a `{e}` hole
  ends the literal early — `html"<a href={url}>"` is fine, but `html"{f("x")}"` is not. Bind the
  value to a local first (`var v = f("x"); html"{v}"`). This is the shared interpolation model, not
  specific to html.
- **Named element helpers cover a curated set, not every HTML tag.** `html.div`/`html.p`/`html.br`/…
  are a hand-picked common subset (flow + sectioning + list + table + inline + the void elements);
  for a tag outside the set use the generic `el(tag, attrs, children)` / `voidEl(tag, attrs)`. The
  set is macro-driven (each tag is monomorphized), so extending it is a one-line addition — not a
  limitation, just a scope choice. (The earlier "no named helpers at all" deferral is resolved.)
- **Tag and attribute *names* are not escaped — only values and text are.** `el`/`voidEl` tags and
  `attr`/`boolAttr` names are treated as trusted author literals (like the surrounding markup);
  only attribute **values** (via `attr`) and **text** (via `text`) pass through
  `htmlspecialchars(_, ENT_QUOTES)`. Do not build a tag or attribute name from untrusted input.
- **Escaping covers text and attribute-value contexts only.** `html.text` / `attr` are correct for
  HTML text and quoted attribute values via `htmlspecialchars(_, ENT_QUOTES)`. They are **not** safe
  for URL contexts (`href="javascript:…"`), inline CSS, or `<script>` bodies — those need
  context-specific escaping and are out of scope until a later wave. Use `html.raw` only for markup
  you have audited.

## Git dependencies (M5 S3)

- **Transitive dependencies are not resolved.** `phg vendor` fetches the direct `[require]` set;
  a dependency's *own* `[require]` is not walked. Vendor flat-named leaf libraries for now (the
  shipped `examples/project/withdeps/` does exactly this).
- **`phg build` is single-file and does not merge `vendor/`.** A program that imports a vendored
  (or any cross-package) dependency runs via `run`/`runvm`/`transpile` (which go through the project
  loader) but cannot yet be compiled to a standalone executable. `build` embeds one source file only
  (M2.5 Phase 1 scope), unchanged by S3.
- **Resolution is offline by design.** `run`/`check`/`transpile` never fetch — they read the
  committed `vendor/`. Only `phg vendor` touches the network; commit `vendor/` + `phorge.lock` so
  builds stay deterministic and reproducible (the same determinism rule that defers URL/network to M6).

## `phg build` limitations (M2.5, in progress)

- **macOS targets are rejected.** The Mach-O/fat section *reader* ships and is tested, but producing a
  signed macOS *stub* is deferred to Phase 3. An apple/darwin `--target` errors with a clear message
  rather than emitting a broken binary.
- **Cross-builds need a source checkout.** `--target`/`--all` compile a stub from source via
  `cargo-zigbuild`, so they must run from a phorge source tree. A *distributed* (sourceless) phorge
  can still do a **host** build (it reuses the running binary as the stub) but not a cross build until
  the Phase 3 prebuilt-stub registry lands.
- **Built binaries ignore argv and always exit 0.** A standalone built binary runs its embedded
  program; command-line arguments passed to it are currently ignored. (`--version`/`--help` are
  features of the `phorge` CLI itself, not of built binaries.)
- **aarch64 / Windows artifacts aren't executed in CI here.** They're validated by an object-section
  round-trip; native execution is verified for the host-runnable `x86_64-musl` target.

## Behavioral quirks

- **Errors inside string interpolation report line 1 (and the caret points there).** A fault *or* a
  type error raised within a `"{ … }"` interpolation is reported at line 1 because the interpolation
  sub-lexer resets position — so the diagnostic caret (S0.4) underlines column 1 of the program rather
  than the real sub-expression. (VM runtime errors carry an accurate line; the interpreter's runtime
  errors generally do not. Errors *outside* interpolation are located and underlined accurately.)
- **Recursion is depth-limited.** Recursion runs on a fixed-size (256 MB) worker stack with explicit
  depth caps (`src/limits.rs`); extremely deep recursion faults cleanly rather than overflowing the
  native stack.
- **Empty list literal `[]` is only inferred in call-argument position.** An empty list has no
  element to infer a type from, so it adopts its type from the **expected parameter type** of a call
  (`el("p", [], […])` works). In a declaration initializer (`List<int> xs = [];`) or a `return`, an
  empty `[]` still errors with "cannot infer element type" — use a non-empty literal there. (This is
  the one place an expected type is threaded into expression checking; full bidirectional inference
  is deliberately out of scope.)
- **Zero-payload enum variants need call form.** A nullary variant `V` must be written `V()` both to
  construct **and** in a `match` pattern. A bare `V =>` arm is parsed as a catch-all *binding*, not a
  variant match — so it silently matches everything. Always use `V()` in patterns for nullary
  variants.
- **`instanceof` is the type-test operator (M-RT S1); the value-equality `is` alias is retired.**
  `value instanceof ClassName` parses (the right operand is a class *type name*, not an expression),
  evaluates to `bool` on `run`/`runvm`, and transpiles to PHP `$value instanceof ClassName` —
  byte-identical across all three backends (see `guide/instanceof.phg`). Inside
  `if (x instanceof T) { … }` the checker smart-casts `x` to `T` in the then-block. As of **M-RT S2**
  the right operand may be a **class or an interface** (`guide/interfaces.phg`); a class that
  `implements` an interface is a *subtype* of it, so an instance flows into an interface-typed slot
  and `x instanceof SomeInterface` is true for every implementer. Union / intersection operands still
  arrive with those features in later M-RT slices. The old `is` keyword is gone — `is` is now an
  ordinary identifier. *(Literal
  `match` patterns and expression-position `match` — previously listed here as transpile gaps — were
  **completed in M11**: both now transpile and are PHP-oracle byte-identity-gated, so
  `examples/guide/enums-match.phg` and `examples/guide/match-expr.phg` are enrolled in the oracle, not
  deferred. The empty/reversed-range and integer-division transpile divergences were fixed earlier in
  M7.)*
- **Float division by zero diverges in the fault domain (transpile target).** A finite `float` now
  renders **byte-identically** across all three backends — the transpiler's `__phorge_float` runtime
  helper reproduces Rust's shortest-round-trip, always-positional `f64` Display exactly (so
  `sqrt(2.0)` → `1.4142135623730951`, `1234567890123456.0` → `1234567890123456`, and `0.00001` →
  `0.00001` all match, with no PHP `precision=14` rounding or scientific-notation switch — see
  `guide/floats.phg`, which round-trips every magnitude through real PHP). The *one* remaining float
  caveat is non-finite: Phorge float `1.0 / 0.0` yields `inf`/`NaN` on `run`/`runvm` (a valid `f64`,
  never a fault), but the transpiled PHP's `/` throws `DivisionByZeroError`. This is a fault-domain
  divergence only — the differential harness excludes fault cases by design, and no byte-identity
  example produces a non-finite float. (`__phorge_float` itself renders `inf`/`-inf`/`NaN` the Rust
  way if one is reached through other means.)
- **`opt!`-on-null transpiles to a different message than the Phorge backends.** A null force-unwrap
  faults `force-unwrap of null` on `run`/`runvm` (located, classified `FaultKind::ForceUnwrap`); the
  transpiled PHP throws a `RuntimeException("force-unwrap of null")` via the `__phorge_unwrap()`
  helper without the source name/line. The *present-value* case is byte-identical; only the null-fault
  message differs (a transpile-only caveat, parallel to the range/index-OOB notes). The differential
  harness excludes fault cases by design.
- **`package main` function names must avoid PHP built-in names (transpile target).** A top-level
  function in `package main` transpiles to a *global* PHP function, so naming one `serialize`,
  `strlen`, `header`, … collides with the PHP builtin (`Cannot redeclare function …`). The Phorge
  backends are unaffected (everything is namespaced); only the PHP round-trip fails. Library packages
  are namespaced and immune. Pick non-builtin names for `package main` functions intended to transpile
  (e.g. `serializeResponse`, not `serialize`).
- **Externally-read fields must be `public`, not `private` (transpile target).** Phorge's
  `run`/`runvm` do not enforce field visibility, so an external read `obj.field` of a `private`
  constructor-promoted field works there — but the transpiled PHP enforces `private` and throws
  `Cannot access private property`. Declare a field `public` in the constructor when it is read from
  outside the class (a `private` field read only inside the class's own methods is fine). The
  byte-identity-with-PHP convention used by the examples is: `public` for externally-read fields, or an
  accessor method (`obj.field_of()`), which is always public.

## Reporting

Found something not listed here — especially a panic, hang, or crash on any input? That's a bug.
Please report it (see [SUPPORT.md](SUPPORT.md); for security, [SECURITY.md](SECURITY.md)).
