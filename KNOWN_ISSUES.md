# Known Issues & Limitations

Phorge is pre-1.0. This page lists current limitations and known rough edges. Most "limitations" are
**deliberate scope boundaries** — features that are *planned* (see [ROADMAP.md](ROADMAP.md)) rather
than broken. The key property is that out-of-scope constructs are **rejected cleanly** (a type or
parse error, non-zero exit) — never a crash.

## Language features not yet implemented

These are designed but not in the current surface; using them produces a clean compile-time error,
not a panic:

- `Map` / `Set` / tuples
- The `is` operator
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
- **No named element helpers.** There is one generic `el(tag, attrs, children)`, not a per-tag set
  (`div(...)`, `p(...)`, …) — fn-pointer natives can't bake a tag name, so a named set would need a
  different mechanism. Pass the tag as a string. (Deferred, not a regression.)
- **Tag and attribute *names* are not escaped — only values and text are.** `el`/`void_el` tags and
  `attr`/`bool_attr` names are treated as trusted author literals (like the surrounding markup);
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
- **A few constructs are not yet transpiled (oracle-deferred to M11).** The transpiler still rejects
  *literal* `match` patterns (`0 => …`, `"a" => …`), expression-position `match`, and the `is`
  operator — all run fine on `run`/`runvm` but emit `transpile error: … not yet supported`. The M7
  PHP oracle (`tests/differential.rs`: `all_examples_transpile_and_match_php`) **loudly skips** any
  example that hits one of these (it logs `DEFER <file>` and a count), so the gap is visible, not
  silent. `examples/guide/enums-match.phg` is the one currently-deferred example. As M11 implements
  each construct the deferral disappears and the example auto-enrolls in the oracle. (The
  empty/reversed-range and integer-division transpile divergences that used to live here were **fixed
  in M7**, when the oracle began executing the transpiled PHP of every example.)
- **Irrational `float` values render with more digits on the Phorge backends than in transpiled PHP.**
  The Phorge backends stringify a `float` with Rust's shortest-round-trip formatting (e.g.
  `sqrt(2.0)` → `1.4142135623730951`), while the transpiled PHP relies on PHP's default `echo`
  precision (`precision=14` → `1.4142135623731`). For *exactly representable* values (integers-as-
  floats, short terminating decimals) both render identically, so `guide/math.phg` keeps to such
  values. This is a transpile-only caveat — the `run`/`runvm` spine is byte-identical (both Rust); it
  predates `core.math` (any irrational float interpolation hits it) and `core.math` merely makes it
  easy to reach via `sqrt`/`pow`. Round-trip through PHP only with exactly-representable floats.
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
  (e.g. `serialize_response`, not `serialize`).
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
