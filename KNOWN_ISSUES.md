# Known Issues & Limitations

Phorge is pre-1.0. This page lists current limitations and known rough edges. Most "limitations" are
**deliberate scope boundaries** — features that are *planned* (see [ROADMAP.md](ROADMAP.md)) rather
than broken. The key property is that out-of-scope constructs are **rejected cleanly** (a type or
parse error, non-zero exit) — never a crash.

## Language features not yet implemented

These are designed but not in the current surface; using them produces a clean compile-time error,
not a panic:

- **Stack traces — slice 1 (reporting) shipped; deferrals:** (1) catching/handling faults — a
  `try`/`catch` or `Result<T, E>` model — is a separate later slice; this slice only *reports* faults
  that abort. (2) Method/constructor/closure frames show `line`-only (no `file:line`) — their frame
  names are backend-synthesized, not in the loader's function→file map; free functions + `main` get
  full `file:line`. (3) Frame lines are statement-granularity, so a fault inside a multi-line
  expression may report the statement's start line. (4) Trace text is intentionally uncolored
  (matches Phorge's plain-diagnostic convention). (5) No "cause chain" (needs the slice-2 error model).

- **Declaration visibility** (`public`/`internal`/`private`) ships for top-level declarations, but a
  few related cases are deliberately deferred: a visibility keyword **on a `type` alias**
  (`private type X = …` is a parse error — aliases are file-local and erased, so they cannot re-export
  a type across files anyway); a visibility keyword on an `import` re-export; and **member-level**
  `Modifier` visibility (`private`/`protected` on fields/methods) remains *PHP-only-enforced* after
  transpile (the Phorge backends don't enforce it) — declaration visibility is a separate, fully
  Phorge-enforced axis.

- Tuples / map iteration, and `Set` union & intersection. The erased-generics *mechanism* ships in
  M-RT S7; the **generic stdlib natives** — `Core.Map` `keys`/`values`/`has`/`size`, `Core.List`
  `reverse`/`sum`, `Set` `of`/`contains`/`size`, and the **higher-order** `Core.List` `map`/`filter`/
  `reduce` (a closure run from a native, M-RT S7b-3) — all ship in M-RT S7b (see the *Maps*/*Generic
  natives* notes below). Set union/intersection and map iteration build on that path next. `Map<K,V>`
  literals + `m[k]` indexing ship in M-RT S3 — see the *Maps* note below.
- ~~`instanceof` against a **union**~~ — **now supported** (M-RT S4): a union-typed value is a valid
  `instanceof` left operand, and `if (s instanceof Circle)` narrows it. A union-typed *operand* and an
  intersection-typed *operand* are both accepted; what is still deferred is `instanceof` whose **right
  side** is an intersection (`x instanceof (A & B)`) — `Op::IsInstance` carries a single name, so this
  needs a new op or a lowering to `x instanceof A && x instanceof B` (M-RT S5 deferral).
- **Intersection types (M-RT S5) — deferred corners** (each rejected cleanly, never a panic): **two or
  more concrete classes** (`Cat & Dog` → `E-INTERSECT-MULTI-CLASS`; a value has exactly one class — this
  becomes meaningful only once class `extends` lands in S6), **primitive/enum/optional/function members**
  (`E-INTERSECT-MEMBER`), a **shared method with conflicting signatures** across members
  (`E-INTERSECT-SIG`; uninhabited because Phorge has no overloading **yet** — overloading is the next
  M-RT slice, after which this rule is revisited), `instanceof` with an **intersection right side**
  (above), and the **whole-intersection optional** `(A & B)?`. There is no match-over-intersection
  (an intersection is not a sum type).
- **Union types (M-RT S4) — deferred corners** (each rejected cleanly, never a panic): **enum members**
  in a union (`Color | Circle` → `E-UNION-MEMBER`; an enum is already a closed sum — match its variants
  directly), **optional/function members** (`E-UNION-MEMBER`), a **type pattern nested in a variant
  payload** (`Wrapper(Circle c)` → `E-MATCH-TYPE`; type patterns are top-level-only), **negative/flow
  narrowing** (after `if (s instanceof Circle)` the else-branch does not narrow `s` to the remaining
  members), **common-member access on a raw union** (`(A|B).foo()` without narrowing — narrow first),
  and the **whole-union optional** `(A|B)?` (`?` is postfix on a single member; `A | B?` parses as
  `A | (B?)`). Use `T?` for nullability.
- ~~interfaces/classes/enums in a library (non-`main`) package~~ — **now supported** (M-RT
  cross-package types): a library package exports types, consumed via `import type Pkg.Path.Type [as
  A]`; `E-PKG-TYPE` is retired. Remaining limits: the **module-qualified** type form (`import
  acme.geometry;` then `Geometry.Point`) is deferred (the terminal `import type` is the shipped form);
  variant/type names must be unique across all merged packages; generic *types* (`Box<T>`) are a
  separate pending slice.
- Exceptions (try / catch / throw)
- Method/function overloading, traits, operator overloading, property accessors
- Sized integers / `decimal`, `const`/`final` enforcement
- `match` outside return / variable-declaration-initializer position

## Mutation milestone — deferred corners

In-place mutation ships incrementally (immutable-by-default, `mutable` opt-in): mutable locals +
reassignment (M-mut.1), compound-assign + `++`/`--` + `??=` (M-mut.2), condition loops (M-mut.3),
`clone with` (M-mut.4a), value-type element set `xs[i]=e`/`m[k]=e` (M-mut.5), **shared-mutable
instance fields `o.f=e`** (M-mut.6 — instances are handles; see `examples/guide/mutable-fields.phg`),
**`static`/`static mutable` class fields** `ClassName.field` (M-mut.7a), and **property hooks**
`T name { get => …; set(T v) { … } }` (M-mut.7b — virtual get/set, subsumes the old get-hook plan;
see `examples/guide/property-hooks.phg`). The milestone is **feature-complete**. Each slice is
byte-identical `run ≡ runvm ≡ real PHP`. Still deferred (each is either a clean compile-time error or
an explicit non-goal, never a panic):

- **No cycle collector.** Instances are shared-mutable handles, so `a.next = b; b.next = a` forms a
  reference cycle that `Rc`/`Drop` cannot reclaim — it **leaks until process exit** (the HHVM
  per-process model, Fork-3). Fine for a run-once CLI; a trial-deletion collector lands only if a
  long-lived-cycle need appears (e.g. `serve`). `==` on a cycle is *safe* (cycle-guarded `eq_val`,
  F4) — it terminates rather than overflowing the stack.
- **No identity `===`.** Only structural `==` exists; an `Rc::ptr_eq`-based identity operator is an
  optional future addition.
- **Nested place-stores.** `this.f[i] = e` (index into a field) and compound nested paths are
  rejected (`E-ASSIGN-TARGET`); a field path `a.b.c = e` *is* supported (handle semantics), but an
  *indexed* field target is not. A field-set on an intersection-typed object is also deferred.
- **Property hooks are virtual-only** (M-mut.7b). A hook declares no storage of its own — its get/set
  bodies read and write *other* fields. **Backed hooks** (a hook with its own slot + the PHP
  `$this->name` self-reference), **hooks on `static` fields**, **hooks in interfaces**, and
  **abstract/overridable hooks** are deferred. Promoted/declared fields with no explicit visibility
  transpile to PHP `public` (Phorge does not enforce field visibility at runtime; `readonly`/`final`
  emission is not done — immutable fields are already write-prevented by the checker).

## Totality cluster (M-RT) — deferred refinements

Return-on-all-paths (`E-MISSING-RETURN`), the `never` bottom type, and the `W-UNREACHABLE` /
`W-MATCH-UNREACHABLE` dead-code lints ship and are byte-identical `run ≡ runvm ≡ real PHP` (see
`examples/guide/totality.phg`). The termination analysis is deliberately **structural and
conservative** — it claims divergence only for shapes it can prove, so it never rejects a function
that does return on every path. The corners below are deferred (each is sound, never a crash):

- **`never` is only usefully inhabited by infinite loops today.** A `-> never` function must diverge;
  the only divergence producers in the current language are an infinite loop (`while (true) {}` /
  `for (;;) {}`) and a call to another `never` function. The natural producer — `throw`/`panic` — lands
  with the error model (**M-faults Slice 2**), at which point `never` lights up fully. The type, its
  PHP `never` emission, and the divergence analysis are wired correctly ahead of that.
- **`expr_is_never` recognises only free-function `never`-calls.** A method or closure call that
  returns `never` is not yet treated as a divergence point (it needs receiver typing in the structural
  pass). Workaround: none needed — the only effect is a possible (over-strict) `E-MISSING-RETURN` after
  such a call, not unsoundness; in practice no shipped code hits it.
- **No flow-typing beyond structural termination.** An exhaustive `match` *statement* (not in `return`
  position) whose every arm diverges is not recognised as divergent, and a `break`/`continue` inside a
  conditionally-true loop is analysed only for the `while (true)`-with-no-`break` shape. Restructure to
  a trailing `return` if the checker asks for one.

## Generics (M-RT S7) — deferred refinements

Erased generics ship for **free functions, class methods, classes, and enums**: `function id<T>(T x)
-> T`, `class U { function id<T>(T x) -> T … }`, `class Box<T> { … }` / `class Pair<A, B> { … }`, and
`enum Option<T>` / `enum Result<T, E>`, inferred at the call site / at construction / at the variant
constructor, byte-identical `run ≡ runvm ≡ real PHP` (see `examples/guide/generics.phg`,
`generic-methods.phg`, `generic-types.phg`, `generic-enums.phg`). There is no monomorphization — type
parameters are erased to PHP `mixed` before any backend; a generic class/enum value carries no runtime
type argument (`instanceof Box<int>` ≡ `instanceof Box`). These refinements are deliberately deferred
(each rejected cleanly or simply unavailable, never a crash):

- **A generic-typed *result* is not a specialized arithmetic operand.** Because a `T` erases to PHP
  `mixed`, the bytecode compiler types any generic-function/method/field result as the opaque
  `CTy::Other`, which is not a numeric operand. So `id(7) + 1` (or `box.get() + 1`) type-checks (the
  checker reifies the result as `int`) and runs on the interpreter, but the VM rejects it with
  *"`id` does not return a numeric type"* — a `run`↔`runvm` mismatch. Bind the result to a typed local
  first (`int n = id(7); n + 1`), which the examples do. [Verified: `id(7) + 1` → `run` prints `8`,
  `runvm` errors.] Fixing this needs the compiler to thread reified generic result types (deferred).
- **Generic *interface* methods** are a non-parse — an interface method's signature is built with an
  empty type-parameter list, so a `<T>` there is never consumed. Generic methods on *classes* work.
- **Cross-package generic *library* types** are not validated this slice — a generic class is
  `package main`-only (the loader leaves a class type parameter unchanged and erasure removes it, so it
  may happen to work, but it is untested). Cross-package *monomorphic* types ship (`E-PKG-TYPE` lifted).
- **Explicit type arguments at construction** (`Box<int>(7)`) are not parsed — the type argument is
  inferred from the constructor arguments. An explicit *annotation* (`Box<int> b = Box(7)`) does work.
- **Generic enums** (`enum Option<T>` / `enum Result<T, E>`) ship, with the same scope as generic
  classes: `package main`-only, inference-only construction (no `Some<int>(7)` explicit-argument form —
  use an annotation, `Option<int> n = None();`), invariant, no bounds, no generic *enum methods* (enums
  have no methods). A match-bound payload is reified at the scrutinee's concrete type (`Some(n)` over an
  `Option<int>` binds `n: int`), but — like every erased generic — that payload is `mixed` to the
  backend, so it is **not a specialized VM arithmetic operand** (the operand limitation above); since
  match arms are single-expression, return the payload into a typed local for arithmetic.
- **Same-head generic types are not actually invariant at an assignment boundary.** `Box<string>` /
  `Option<string>` *is* accepted where `Box<int>` / `Option<int>` is expected, because the nominal
  assignability check short-circuits on the reflexive name edge (`subtype("Box","Box")` is true) before
  the invariant type-argument comparison runs. [Verified: `Box<int> b = Box("x")` and `Option<int> o =
  Some("x")` both type-check clean.] This is a **pre-existing gap shared by generic classes** (not new
  to generic enums); the documented "invariant" intent is only enforced for the built-in containers
  (`List`/`Map`/`Set`). A real fix touches the shared subtype oracle (used everywhere) and is deferred.
- **A generic function used as a first-class *value*** (`var f = id;` then `f(x)`) is not supported —
  call a generic function directly so the call site can infer its type parameters. (A monomorphic
  named function as a value already works — M3 S3.)
- **An empty list literal `[]` passed straight to a generic parameter** (`firstOr([], x)`) cannot
  infer the element type and is rejected — pass a non-empty list, or bind it to a typed local first.
- **No bounds and no variance** — a type parameter is unconstrained, and generic instantiations are
  invariant (matching the rest of the type system; sound variance needs in/out annotations and carries
  no runtime information under erasure).

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

## Core.Html (Waves 1–3 — escape kernel + element builders + `html"…"` sugar)

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

## Maps (M-RT S3 — foundation)

`Map<K, V>` ships its **foundation** this slice: literals `[k => v, …]` and indexing `m[k]`,
byte-identical on `run`/`runvm` and round-tripped through real PHP. These are deliberately deferred
(each rejected cleanly or simply unavailable, never a crash):

- **No empty map literal yet.** `[]` is the empty *list*; a map literal needs at least one `k => v`
  pair (the parser can't tell an empty map from an empty list, and there's no element to infer `K`/`V`
  from). An empty/growable map awaits a builder native — which, like the query ops below, needs
  generics. Mixing forms in one literal (`[a, b => c]`) is a clean parse error.
- **Keys are the hashable subset only — `int`/`bool`/`string`.** A `float`, list, instance, or other
  composite key is `E-MAP-KEY` (`phg explain E-MAP-KEY`). This mirrors the runtime `HKey` set.
- **A missing key faults (`"map key not found"`).** Like list out-of-range, `m[k]` on an absent key is
  a clean, byte-identical fault on both backends; the present-key path is byte-identical to PHP
  `$m[$k]`, and the differential harness excludes the fault case by design. A safe `has`/`get`
  accessor awaits generics.
- **`keys` / `values` / `has` / `size` now ship as `Core.Map` natives (M-RT S7b).** They are generic
  (`keys(Map<K,V>) -> List<K>`, `has(Map<K,V>, K) -> bool`, …), inferred at the call site like a
  generic free function, and erase to `array_keys`/`array_values`/`array_key_exists`/`count`. **Map
  *iteration* and `Set` itself are still pending** (Set construction is the next S7b sub-slice). Key
  coercion caveat: PHP arrays coerce integer-like string keys (and bools) to int keys, so `keys()`/
  `values()` over such a map render differently under PHP than on the Rust backends — use plain
  (non-numeric) string keys when transpiling, which PHP keeps verbatim. The `run`/`runvm` spine is
  always byte-identical.
- **A string-literal index inside a `"{…}"` interpolation nests quotes.** `"{m["k"]}"` ends the
  string early (the shared interpolation rule — see Core.Html). Bind the lookup to a local first:
  `var v = m["k"]; "{v}"`. An `int`/identifier index inside `{…}` is fine.
- **Bool map keys: PHP coerces `true`/`false` to `1`/`0` as array keys; Phorge keeps them distinct.**
  A `Map<bool, V>` works and is byte-identical *as long as you don't also use `0`/`1` int keys in the
  same map* (PHP would collapse `true` and `1`). Prefer string/int keys when transpiling to PHP.

## Generic natives (M-RT S7b — `Core.List` / `Core.Map`)

The first generic stdlib natives ship this slice: `Core.List` `reverse`/`sum` and `Core.Map`
`keys`/`values`/`has`/`size`. Their signatures carry `Ty::Param` and unify at the call site exactly
like a generic free function; the parameter is registry-only and never reaches a backend. Two PHP-leg
caveats (the `run`/`runvm` spine is always byte-identical):

- **`List.sum` faults on i64 overflow; PHP `array_sum` promotes to float instead.** The checked sum
  keeps EV-7 (never panics), so a sum exceeding `i64::MAX` is a clean Phorge fault, whereas PHP would
  silently widen to float. Keep sums within i64 range when transpiling (examples do).
- **`Map.keys`/`values` key coercion** — see the *Maps* note above: PHP coerces integer-like string
  keys and bools to int keys, so use plain string keys for byte-identical PHP round-tripping.

`Core.Set` now ships too (M-RT S7b): `of(List<T>) -> Set<T>` (insertion-ordered dedupe),
`contains(Set<T>, T) -> bool`, `size(Set<T>) -> int`. `Value::Set` is an insertion-ordered
`Rc<Vec<HKey>>` (the Map discipline, not a `HashSet`), so it round-trips byte-identically as a deduped
PHP array (`array_values(array_unique($xs, SORT_STRING))` / `in_array(_, _, true)` / `count`).
Element type is the hashable subset (`int`/`bool`/`string`); homogeneous by typing, so the
SORT_STRING dedupe matches `HKey` equality. Set union/intersection and iteration are follow-ups.

Still pending on this path: the higher-order `Core.List` `map`/`filter`/`reduce` (the
closure-from-native mechanism — `NativeEval::HigherOrder` + a re-entrant VM closure invoker).

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
  and `x instanceof SomeInterface` is true for every implementer. Union (**S4**) and intersection
  (**S5**) *left* operands are now both accepted; only an intersection on the **right** is deferred
  (above). The old `is` keyword is gone — `is` is now an
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
