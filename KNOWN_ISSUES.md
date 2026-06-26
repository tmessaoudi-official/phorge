# Known Issues & Limitations

Phorge is pre-1.0. This page lists current limitations and known rough edges. Most "limitations" are
**deliberate scope boundaries** — features that are *planned* (see [ROADMAP.md](ROADMAP.md)) rather
than broken. The key property is that out-of-scope constructs are **rejected cleanly** (a type or
parse error, non-zero exit) — never a crash.

## Language features not yet implemented

These are designed but not in the current surface; using them produces a clean compile-time error,
not a panic:

- **PHP-reserved identifiers as symbol names — now guarded (F-m, kind-aware).** Phorge and PHP have
  different keyword sets, so a Phorge identifier that is a *PHP* reserved word would transpile to
  invalid PHP when it names a symbol (a free `function`/`class`/`enum`/`interface`/`trait`/`type`).
  `is_php_reserved_symbol_name(name, kind)` now rejects the full empirically-verified set with a clean
  **`E-RESERVED-NAME`**: the function-illegal words (`var`/`list`/`print`/`array`/`unset`/`empty`/
  `eval`/`echo`/`clone`/`callable`/… — verified vs PHP 8.5) for a `function`, plus the type words
  (`int`/`float`/`bool`/`string`/`object`/`readonly`/… — legal PHP function names but illegal class
  names) for a `class`/`enum`/`interface`/`trait`. All remain usable as value/parameter/field/method
  names (legal PHP `$list` / `->list()`). A *type alias* only guards `var` (the contextual-keyword
  collision); the built-in type words are already rejected by the alias arm. **Deferred corner:** a
  *method* named after a word PHP forbids as a method (none in the function/class sets are — PHP
  semi-reserves allow method names) is not specially handled; no known case.

- **Default parameter values (M4) — shipped corners + deferrals.** A trailing parameter may declare a
  literal default (`function f(int x, int y = 10)`); a call that omits it is filled to full arity before
  the backends. Deferrals (each a clean compile error, never a panic): (1) **free functions only** — a
  default on a method or constructor parameter is `E-DEFAULT-PARAM-CONTEXT` (the fill pass resolves
  free/native calls, not method dispatch); (2) **literal defaults only** — a non-literal default
  (`x = f()`) is `E-DEFAULT-PARAM-EXPR`; (3) **direct calls only** — a function **value** (closure /
  named-fn ref) called with missing args is the ordinary arity error, not filled (closures carry no
  default metadata). (4) `Text.parseFloat`'s `(float)` cast matches Rust `f64::from_str` for typical
  decimals; an extreme-precision input could differ in the last ULP (examples use simple values), and
  `inf`/`nan` are **rejected by design** in both strict and permissive modes (byte-identity — PHP's
  cast can't produce them).

- **`decimal` primitive (M-NUM S1) — shipped corners + deferrals.** The exact fixed-point `decimal`
  ships with `19.99d` literals, `Decimal.of(string) -> decimal?`, `+ - *`, scale-insensitive
  comparison/equality, unary `-`, and BCMath transpile. Deferrals: (1) **`/` and `%` are NOT operators**
  — division landed in M-NUM S2 as the explicit `Decimal.div(a, b, scale, mode)` /
  `Decimal.round(d, scale, mode)` natives (bare `decimal /`/`%` is a clean compile error,
  `E-DECIMAL-DIV`); the result scale + rounding mode is a deliberate, explicit choice, not a default,
  and there is **no decimal modulo**. (2) **i128 overflow is
  a runtime fault, not a compile error** — an exact `+ - *` result (or a scale alignment) that leaves
  the `i128` range faults `"decimal overflow"` (byte-identical on `run`/`runvm` and in the emitted
  BCMath, which bounds-checks the result against i128 range and `throw`s the same body). Because every
  shipped example must produce identical *Ok* output, the fault is **not** a runnable example — it is
  exercised by the kernel unit tests (`value::decimal_overflow_is_a_clean_fault`); a program that
  overflows simply faults identically on all three backends. (3) **No `decimal`↔`float` coercion** — by
  design (`E-DECIMAL-FLOAT-MIX`); the only operator-level widen is `decimal ⊕ int`. (4) **No
  arbitrary-precision decimal / `BigInt` / `Money`+currency** — those are M-NUM-2 (they share a
  hand-rolled bignum core); the i128 range (~10^36 at scale 2) covers all realistic money.
  (5) **Transpiled decimal output requires the PHP BCMath extension** — `decimal` arithmetic emits
  `bcadd`/`bcsub`/`bcmul`/`bcdiv`/`bcmod`, so the generated PHP must run under a `php` with BCMath
  enabled (it ships in PHP's standard distribution and is on by default in most builds). The
  byte-identity oracle runs `php -n` (hermetic, no ini); since BCMath is usually a *shared* extension
  that `-n` disables, the test harness loads it explicitly via `-d extension=bcmath`
  (`tests/differential.rs::php_n_args`) and CI installs it (`setup-php` `extensions: bcmath`). This is
  the one deliberate exception to the "transpiled output uses only `-n`-available core" rule.

- **Decimal division + rounding (M-NUM S2) — shipped corners + deferrals.** `Decimal.div`/`Decimal.round`
  ship with the full seven-mode `RoundingMode` enum (injected on `import Core.Decimal`), single-sourced
  in `value::round_div` and mirrored by BCMath. Deferrals/corners: (1) **The fault cases are not runnable
  examples** — a zero divisor (`"decimal division by zero"`), a negative `scale`
  (`"decimal scale out of range"`), and an intermediate i128 overflow (`"decimal overflow"`) are clean
  faults, byte-identical on `run`/`runvm` (FaultKind parity) and the emitted PHP helper `throw`s the same
  body; but because every shipped example must produce identical *Ok* output, the faults are exercised by
  the kernel + native unit tests (`value::decimal_div_by_zero_is_a_clean_fault`, …) and the differential
  `agree_err` cases, not the example set. (2) **No default-scale division** — `Decimal.div` always takes
  an explicit `scale` (the whole point: no silent precision choice); there is no `Decimal.div(a, b)`
  overload. (3) **No decimal modulo** — `decimal %` is rejected with `E-DECIMAL-DIV` (no decimal-remainder
  native this milestone). (4) **A `scale` past 255** (`u8::MAX`) faults `"decimal scale out of range"` —
  far beyond any realistic money use, and an i128 decimal can carry at most ~38 significant digits anyway.

- **Float predicates + numeric conversions (M-NUM S3) — shipped corners + deferrals.** `Core.Math`'s
  `isNan`/`isFinite`/`isInfinite`/`nan`/`infinity`/`negInfinity`/`intdiv` and `Core.Convert`'s
  `toFloat`/`toInt`/`intToDecimal`/`decimalToFloat`/`decimalToInt` ship as additive natives.
  Corners: (1) **`intdiv` faults are not runnable examples** — a zero divisor (`"division by zero"`) and
  the `intdiv(i64::MIN, -1)` overflow (`"integer overflow"`) are clean faults, byte-identical on
  `run`/`runvm` (FaultKind parity) and PHP `intdiv` throws the matching class; but every shipped example
  must produce identical *Ok* output, so the faults are exercised by the `value::int_intdiv_truncates_and_faults`
  kernel test + the `math_intdiv` native test, not the example set. (2) **`Math.nan()`/`infinity()`/`negInfinity()`
  must not be *printed*** — Rust renders `NaN`/`inf`/`-inf` while PHP `echo`es `NAN`/`INF`/`-INF`
  (the pre-existing float-display divergence, also noted for `Core.Json`); the example exercises them
  only through the `bool`-returning predicates, never `Console.println(infinity())`. The `run ≡ runvm`
  spine is always byte-identical (both Rust); only printing a special value would diverge from PHP.
  (3) **`toInt(float) -> int?` / `decimalToInt(decimal) -> int?` return `null` on out-of-range / special
  inputs** — `toInt` is `null` for NaN/±∞/out-of-i64-range (deliberately avoiding PHP's `(int)NAN == 0`);
  `decimalToInt` is `null` when the integer part is outside i64. The i64 *edge* is closed with a shared
  exclusive upper bound (`9.2233720368547758E18`) on both sides because `i64::MAX` is not exactly
  f64-representable — verified by a near-edge probe (`value::float_to_int_guards_the_edge`). (4) **No
  `floatToDecimal`** — by design (float→decimal is lossy/surprising; use `Decimal.of(string)`); for a
  *rounded* decimal→int, compose `Decimal.round(d, 0, mode)` then `decimalToInt`. (5) **`decimalToFloat`
  is lossy by nature** — examples keep printed results to exactly-representable values (`12.5d`).

- **`Core.Json` — shipped corners + deferrals.** (1) **Float magnitude divergence from native
  `json_encode`:** Phorge renders a float with the positional shortest-round-trip form (`__phorge_float`)
  for consistency with `run`/`runvm` everywhere, so an extreme magnitude (`1e20`) stringifies as
  `100000000000000000000`, not json's `1.0e+20`. `run ≡ runvm ≡ real PHP` is always byte-identical (the
  PHP leg uses the same helper); only the comparison to PHP's *native* `json_encode` differs at
  magnitude extremes. (2) **`package Main` only:** the injected `Json` enum is emitted flat, so a
  multi-package project that `import`s `Core.Json` is a follow-up. (3) **Reserved-variant collision
  edge:** an enum literally declaring both `Int` and `Int_` would collide after mangling — adversarial,
  not hit by any first-party code. (4) `NaN`/`Infinity` (non-JSON) stringify to `NaN`/`inf` tokens
  (consistent across backends, not standard JSON).

- **Stack traces — slice 1 (reporting) shipped; deferrals:** (1) catching/handling faults — a
  `try`/`catch` or `Result<T, E>` model — is a separate later slice; this slice only *reports* faults
  that abort. (2) Method/constructor/closure frames show `line`-only (no `file:line`) — their frame
  names are backend-synthesized, not in the loader's function→file map; free functions + `main` get
  full `file:line`. (3) Frame lines are statement-granularity, so a fault inside a multi-line
  expression may report the statement's start line. (4) Trace text is intentionally uncolored
  (matches Phorge's plain-diagnostic convention). (5) Stack traces do not yet print a "caused by"
  cause chain — the *data* exists (M-faults 2c: a `cause` field is preserved and, on transpile, populates
  PHP's native `$previous`), but the Phorge fault renderer does not walk it; folding the cause chain into
  the trace output is a later refinement.

- **Multiple inheritance — S6b/S6c shipped; deferrals:** `class C extends A, B` with `use`/`rename`/
  `exclude` resolution, diamond auto-merge, and `abstract` classes/methods are in. (1) **Decomposed-ancestor
  type/`instanceof` references — SHIPPED (S6c.3).** A multi-parent class lowers to `implements I…/use T…`,
  so the transpiler emits an ancestor type reference (a `Swimmer s = duck;` binding, an ancestor-typed
  parameter, or `duck instanceof Swimmer`) in its **interface form** (`ISwimmer`); full subtyping across the
  lattice is observable on all three backends byte-identically (`guide/inheritance-lattice.phg`).
  (2) **Field-collision detection shipped (S6c.1):** a
  same-named instance field inherited from ≥2 distinct parents is `E-MI-FIELD-CONFLICT` (no `insteadof`
  for PHP properties; resolve by redeclaring in the child). (3) **Constructor inheritance shipped (S6c.2a + S6c.2b):** a class with **no own
  constructor** inherits its parents' — single-parent runs the (transitively chained) ancestor's ctor;
  **multi-parent** runs a synthesized orchestrating ctor whose params are the parents' ctor params
  concatenated in `extends` order, executing each parent's ctor (with its arg slice) on the one instance.
  Still deferred: a class that declares **its own** constructor *under inheritance* — there is no
  parent-forwarding mechanism (`super`/`parent` is reserved-ambiguous), so it cannot initialize inherited
  parent state; needs the explicit per-parent init call (the `super`-replacement follow-up). Also a
  *non-promoted* ctor-param **name collision across two parents** would emit a duplicate PHP parameter
  (rare; promoted-field collisions are already `E-MI-FIELD-CONFLICT`). (4) A class that is **both a multi-parent leaf and an ancestor of another multi-parent
  class** ("multi-of-multi") takes the `implements/use` path and is not also emitted as a trait — a deep
  edge case outside S6's `package Main` scope. (5) **`super`/`parent` is not a language construct at all**
  (inherited methods dispatch via `this.m()`), so the planned `E-MI-SUPER-AMBIGUOUS` reservation is moot
  until that feature lands.

- **Traits — S8 shipped; deferrals (all clean compile-time, or transpile-oracle-gated):** `trait`/`use`
  composition (methods, `mutable`/`static` state, a trait constructor, abstract requirements, property
  hooks) is in, byte-identical across backends + real PHP 8.4. Deferred: (1) **traits as types** —
  intentional and permanent; a trait is reuse, not a type (`E-USE-AS-TYPE`/`E-INSTANCEOF-TYPE`). Use an
  interface for the type side. (2) **generic traits** (`trait T<X>`) — mirror the generic-method gate;
  not yet parsed. (3) **cross-package traits** — this slice is `package Main`-only (like every M-RT
  slice); a library-package trait + cross-package `use` is a follow-up. (4) **trait-vs-trait
  conflict-resolution transpilation — SHIPPED (Wave 1.3).** A collision resolved by `use P.m`/`rename`/
  `exclude` now lowers to a combined PHP `use P, Q { P::m insteadof Q; P::m as n; }` block (mirroring the
  MI-decomposition path), byte-identical run≡runvm≡real PHP (`guide/trait-conflicts.phg`). Narrower
  remaining edge: a collision where one trait supplies the method only via its *own* nested `use`
  (not a direct declaration) isn't detected by the clause builder — caught by the PHP oracle if it
  arises. (5) **immutable trait instance
  fields need a trait constructor** to initialize (promotion) — the same M-mut rule as a plain class
  (an immutable field can't be assigned via `this.f = …`, even in the using class's ctor). (6) `const`
  *class/trait* members are a pre-existing non-feature (`E-FIELD-INIT`), unrelated to traits.

- **Declaration visibility** (`public`/`internal`/`private`) ships for top-level declarations, but a
  few related cases are deliberately deferred: a visibility keyword **on a `type` alias**
  (`private type X = …` is a parse error — aliases are file-local and erased, so they cannot re-export
  a type across files anyway); and a visibility keyword on an `import` re-export. **Member-level**
  `Modifier` visibility (`private`/`protected` on instance fields, promoted ctor params, and methods)
  is now **checker-enforced** (Wave 1.1, `E-FIELD-VISIBILITY`/`E-METHOD-VISIBILITY`): an out-of-scope
  read/write/call is rejected up front so `run ≡ runvm ≡ transpiled PHP` all agree. Remaining
  *not-yet-enforced* corners (still PHP-only, narrower than before): a `private`/`protected` **static
  field** read externally (`ClassName.field`), and a member reached through an **intersection-typed**
  receiver. Both are rare and tracked for a follow-up; instance-field/method access — the documented
  hole — is closed.

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
- **The checked `as` cast (M4 casting axis 2) — deferred corners** (each rejected cleanly, never a
  panic). The cast **target** is a single class/interface *name* — exactly like `instanceof`'s right
  side — so a **union/intersection target** (`x as (A | B)`, `x as (A & B)`) and an explicit **generic
  argument** (`x as Box<int>`) are not parsed; a generic target erases its args (`x as Box` ≡
  `x as Box<…erased…>`, no runtime type arguments, same as `instanceof`). The cast **scrutinee** must
  be a class/union/intersection value (a primitive or an `Optional` left operand is `E-CAST-TYPE`), so
  a **chained cast on the optional result** (`(x as A) as B`, where `x as A` is `A?`) is rejected —
  bind/if-let the first cast, then cast the narrowed value. **Primitive targets** (`x as int`) are
  rejected by design (value *conversion* is the `Core.Convert` axis).
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
  directly), **optional/function members** (`E-UNION-MEMBER`),
  **common-member access on a raw union** (`(A|B).foo()` without narrowing — narrow first),
  and the **whole-union optional** `(A|B)?` (`?` is postfix on a single member; `A | B?` parses as
  `A | (B?)`). Use `T?` for nullability. (Else/negative flow-narrowing now *does* narrow the else-branch
  — see the flow-narrowing row below.)
- **Flow-narrowing (M-RT pattern cluster S5.3) — what narrows and what doesn't.** Narrows: `if (x
  instanceof T)` (then → `T`, else → the remaining union members), `!(…)` / `&&` (true side) / `||`
  (false side) composing those, and an **early-return guard** (`if (!(x instanceof T)) { return … }`
  narrows the rest of the block). **Not narrowed** (deferred): the *true* side of `a || b` (a
  disjunction implies no single fact); **common-member access on a raw union** without narrowing;
  **`x == null` / equality-literal refinement** — Phorge rejects comparing an optional/union to a
  literal (`T? == null`, `int|string == "ok"`), so there is no such narrowing source (use if-let /
  `??` / match-over-optional / match-over-union instead); **post-match scrutinee narrowing** — a
  `match` is an expression and its arms are expressions (no statement-match with diverging arms), so
  there is no fall-through to narrow. **while-let `when` guards** are not implemented (if-let only).
- ~~interfaces/classes/enums in a library (non-`main`) package~~ — **now supported** (M-RT
  cross-package types): a library package exports types, consumed via `import type Pkg.Path.Type [as
  A]`; `E-PKG-TYPE` is retired. Remaining limits: the **module-qualified** type form (`import
  acme.geometry;` then `Geometry.Point`) is deferred (the terminal `import type` is the shipped form);
  variant/type names must be unique across all merged packages; generic *types* (`Box<T>`) are a
  separate pending slice.
- Exceptions (try / catch / throw)
- Method/function overloading, traits, operator overloading, property accessors
- Sized integers (`i8`..`u64`), `const`/`final` enforcement (the `decimal` primitive now ships — M-NUM S1)
- `match` outside return / variable-declaration-initializer position

## Pattern cluster (M-RT S5.1 / S5.2) — deferred refinements
- **Match-arm guards ship** (`pat when <cond> => …`, contextual `when`, byte-identical, no new `Op`).
  **if-let / while-let guards** (`if (var u = opt when u.active)`) are **deferred to a follow-up**:
  the match-arm machinery doesn't apply (the binding is statement-level, not an arm), so it needs
  either a new `Stmt::If.guard` field threaded through ~18 construction/consumer sites (incl. the
  `rewrite_*`/loader AST-rebuild passes) or a synthetic-local desugar — disproportionate to its
  marginal value. Workaround today: bind, then test inside the block (`if (var u = opt) { if (u.active) … }`).
- **Struct destructuring ships** (S5.2: shorthand `Point { x, y }`, rename `Point { x: px }`, full
  nesting, plus nested type patterns in variant payloads `W(Circle c)`). Deferred corners:
  (1) a struct pattern reads instance fields by name, so it assumes **initialized fields** — fine for
  the universal case (promoted ctor params, always populated); destructuring a declared-but-uninitialized
  explicit field is unsupported (the interpreter treats an absent field as a no-match while the VM's
  `GetField` faults — a narrow run↔runvm asymmetry only for the binding-bound-but-unused case). (2) A
  refutable nested pattern never discharges its variant/struct's exhaustiveness, even when it is in
  fact total over a concrete payload type (`W(Circle c)` on a `Circle`-typed payload still needs a
  fallback) — the checker doesn't prove payload-subtype totality. (3) Struct patterns on **generic
  classes** bind fields at their declared (un-substituted) type. (4) Flow-narrowing (negative/else,
  early-return, post-match, equality) is the remaining **S5.3** sub-slice.
- **Fixed-length lists `[T; N]` ship** (Phase 1 types slice: compile-time length, static literal-index
  bounds, `[T; N]` → `List<T>` assignability, length-preserving element-set; erases to a PHP array).
  Deferred: (1) the **irrefutable-destructuring payoff** (`var [a, b] = pair`) lands with
  let-destructuring (slice 5); (2) a **zero-length `[T; 0]`** can't be initialized from a literal (the
  empty `[]` has no inferable element type — "cannot infer element type of empty list literal"); (3)
  static bounds cover only **literal** indices — a constant-folded expression index (`p[1 + 1]`) is left
  to the runtime check; (4) the length is invariant and not assignable from a `List<T>` (a list has
  unknown length) — round-trip through a typed local if you need to narrow.
- **Or-patterns ship** (Phase 1 operators slice: `1 | 2 | 3 => …`, `Red() | Yellow() => …`, parser-
  desugared to one arm per alternative, no backend change). Deferred: alternatives must be
  **binding-free** (`E-OR-PATTERN-BIND`) — `Some(_) | None()` is fine but `Some(n) | None()` is
  rejected, since the shared body cannot know which alternative matched. Same-binding-across-
  alternatives (Rust's `Some(n) | Other(n)`) would need a binding-consistency check; split into
  separate arms for now. Or-patterns are also only available at the **arm top level** (not as a
  nested sub-pattern inside a variant/struct payload).

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

## Error model Slice 2a (M-faults) — deferred refinements

The value tier (`Result<T, E>` + `?`) and the panic tier (`panic`/`todo`/`unreachable`/`assert`) ship in
2a, byte-identical `run ≡ runvm ≡ real PHP`. The enforced `throws E` exception tier (with `try`/`catch`/
`finally`) is Slice 2b. Deliberately deferred (each rejected cleanly, never a crash):

- **`?` is allowed only as a whole let-initializer** (`int a = f()?;`). Nested (`g(f()?)`) or
  `return f()?` is `E-PROPAGATE-POSITION` — bind to a local first. [Verified: PHP cannot caller-return
  from inside an expression; a general A-normal-form hoist is deferred.]
- **`?` works on `Result` only this slice** — the `throws`-call propagation mode lands with 2b.
- **A fault intrinsic's message must be a string literal** (`E-INTRINSIC-LITERAL`) — it is baked into the
  fault at compile time. Interpolated/computed panic messages are deferred (would need a runtime-string
  fault path).
- **`?`-unwrapped payloads are not specialized arithmetic operands on the VM** — the unwrapped `Ok`
  value types as `CTy::Other` (the same erased-generics operand limitation), so `f()? + 1` in a
  let-init would run on the interpreter but the VM rejects the arithmetic; bind to a typed local.

## Error model Slice 2b (M-faults) — deferred refinements

Checked exceptions — `throws`/`throw`/`try`/`catch`/`finally` and `?`-throws — ship in 2b, byte-identical
`run ≡ runvm ≡ real PHP` (`examples/guide/errors.phg`). Notes and deliberate deferrals:

- **Panics/faults are uncatchable by design.** A `panic`/`todo`/`unreachable`/failed `assert`, or a
  runtime fault (division by zero, index out of range, …), is a separate tier from a `throw`: it passes
  straight through every `catch` and aborts the program with a stack trace. Only a `throw` of an `Error`
  subtype is catchable. This is intentional — panics signal bugs, not recoverable conditions.
- **Multi-type catch is supported** — both multiple sequential `catch (X e) catch (Y e)` clauses and a
  union `catch (A | B e)`. A clause shadowed by an earlier (broader/equal) one is `W-CATCH-UNREACHABLE`
  (a non-fatal lint, like the dead-code lints).
- **A raw union catch binding cannot read a common member** — `catch (A | B e) { e.message }` is rejected
  because `e: A | B` and common-member access on a raw union is itself deferred (pre-existing S4 gap).
  Catch the types in separate clauses, or narrow with `instanceof`, to read a field.
- **Throw-across-a-higher-order-native is implemented but not yet source-reachable.** The runtime unwinds
  a `throw` out of a closure passed to `Core.List.map`/`filter`/`reduce` correctly on both backends, but
  a lambda **cannot declare `throws`** yet, so an uncaught `throw` inside such a closure is
  `E-THROW-UNDECLARED` at compile time. The mechanism is in place ahead of lambda-`throws`.
- **`throws` on a *method* or *interface* is parsed and checked inside the body, but not discharged at
  the call site**, and **`?`-throws works on a free-function call only** (not a method call). Both are
  follow-ups; free-function `throws` is fully enforced.
- **`finally` cannot return a value** (a `return` inside `finally` overriding the try's value is
  unsupported) — a deliberate non-goal (PHP allows it but it is a well-known footgun).
- **Cause-chains ship in Slice 2c** (`examples/guide/cause-chain.phg`): a conventional `cause` field of
  type `Error?` on an `Error` subtype is routed into PHP's native exception chain
  (`parent::__construct($message, 0, $cause)` → `getPrevious()`); the Phorge backends read it back as a
  plain field, byte-identical `run ≡ runvm ≡ real PHP`. Two deliberate deferrals remain: **reading a
  cause through PHP's `getPrevious()` accessor** (a `.cause()` method form, as opposed to the field read)
  is only meaningful for a *foreign* PHP exception, so it folds into **PHP interop (M8.5)**; and
  **catching PHP-thrown exceptions across the interop boundary** likewise lands with M8.5 (Phorge has no
  PHP-import mechanism yet, so the bridge has nothing to bridge today).

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

## Method & function overloading (M-RT) — deferred refinements

Dynamic multiple dispatch over free functions and class methods ships and is byte-identical
`run ≡ runvm ≡ real PHP` (`examples/guide/overloading.phg`). Deliberate deferrals:

- **Overloaded constructors** are not supported (PHP cannot overload a constructor either; Phorge has
  constructor promotion and — when it lands — default arguments). Overload a static factory method.
- **A single return type is required** across an overload set (`E-OVERLOAD-RETURN`). A union-of-returns
  result type is a future relaxation; today differing returns are rejected (use a generic function when
  the return co-varies parametrically with the argument).
- **Generic overloads** are rejected (`E-OVERLOAD-GENERIC`): a generic declaration must be the sole one
  of its name. A first-class *value* of an overloaded function is also rejected (`E-OVERLOAD-FN-VALUE`)
  — call it directly or wrap the intended overload in a lambda.
- **Ambiguity is detected at runtime, not compile time.** A cross-cutting multi-argument overload set
  with no unique most-specific match for some call faults cleanly *when that call runs*
  (`ambiguous overloaded call to …`, byte-identical on both backends). A compile-time ambiguity check
  is a future refinement; identical signatures are already rejected at declaration
  (`E-OVERLOAD-DUPLICATE`).
- **Two PHP-erasure limits** (the transpile target cannot distinguish what the Phorge backends can):
  overloads that differ *only* by `string`-vs-`bytes`, or *only* among `List`/`Map`/`Set`, are
  indistinguishable in PHP (both erase to `string` / `array`) — avoid such pairs in transpiled code;
  and on a genuinely *ambiguous* call the Phorge backends fault while the emitted PHP `if`-chain takes
  the first matching branch (a transpile-only divergence on faulting input — a runnable example, which
  must produce identical `Ok` output, never exercises it).
- **Overload × intersection types**: the S5 `E-INTERSECT-SIG` agreement check uses the first overload
  as the representative when an intersection member's method is itself overloaded — a full
  overload-aware intersection check is a follow-up.

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
  `package Main`-only (the loader leaves a class type parameter unchanged and erasure removes it, so it
  may happen to work, but it is untested). Cross-package *monomorphic* types ship (`E-PKG-TYPE` lifted).
- **Explicit type arguments at construction** (`Box<int>(7)`) are not parsed — the type argument is
  inferred from the constructor arguments. An explicit *annotation* (`Box<int> b = Box(7)`) does work.
- **Generic enums** (`enum Option<T>` / `enum Result<T, E>`) ship, with the same scope as generic
  classes: `package Main`-only, inference-only construction (no `Some<int>(7)` explicit-argument form —
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

- **`this`-capture ships** (Phase 1 closures slice): a method-body lambda may reference `this`,
  captured *live* (the instance handle), byte-identical on `run`/`runvm`/PHP. `E-LAMBDA-THIS` now fires
  only inside a **field/static initializer** (partially-built instance). Deferred corner: a **bare
  field reference inside a lambda** (`fn() => v` instead of `fn() => this.v`) is *not* captured — it
  type-checks (the field is in the enclosing method scope) but isn't resolved at runtime (interpreter:
  "undefined variable"; VM: a compile error). This is pre-existing (a non-`this` lambda was never
  rejected); **write `this.v` explicitly** inside a lambda. Recognising a bare field as `this.v`
  (and triggering capture) needs field-set awareness in the capture walker — a follow-up.
- **Lambdas and first-class function references are supported in `package Main` (and single-file
  programs), not yet inside library (non-`main`) packages.** The M5 loader's name-mangling pass
  rewrites *call sites*, but not a bare function reference used as a *value* nor the body of a lambda,
  so a same-package call inside a lambda body — or a bare named-fn value — declared in a dotted
  library package is not rewritten to its mangled target. In practice this is rejected cleanly
  (`E-UNKNOWN-IDENT`); avoid lambdas / function values in library packages this slice (the guide
  example and every `package Main` program are unaffected). Loader-resolving lambda bodies and
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
- **`package Main` function names must avoid PHP built-in names (transpile target).** A top-level
  function in `package Main` transpiles to a *global* PHP function, so naming one `serialize`,
  `strlen`, `header`, … collides with the PHP builtin (`Cannot redeclare function …`). The Phorge
  backends are unaffected (everything is namespaced); only the PHP round-trip fails. Library packages
  are namespaced and immune. Pick non-builtin names for `package Main` functions intended to transpile
  (e.g. `serializeResponse`, not `serialize`).
- **Member visibility is enforced (Wave 1.1 — was a byte-identity hole).** An external read/write of a
  `private`/`protected` instance field (incl. a promoted ctor param), or an external call of a
  `private`/`protected` method, is now a **compile error** (`E-FIELD-VISIBILITY`/`E-METHOD-VISIBILITY`)
  — so `run`/`runvm`/transpiled PHP all reject it instead of the Phorge backends accepting what PHP
  would throw on (`Cannot access private property`). Declare the member `public` (the default) when it
  is accessed from outside, or expose it through a public accessor (`obj.valueOf()`). A `private` member
  used only inside the declaring class — and a `protected` one inside that class or a subclass — is
  fine. (Remaining narrower corners — `private` *static* fields and intersection-typed receivers — are
  noted near the declaration-visibility entry above.)

- **`Core.Reflect.traits` is not provided.** `Reflect.interfaces`/`parents`/`methods`/`fields` are
  available, but there is no `traits` enumeration native. A Phorge `trait`'s members are *folded into*
  the using class before any backend runs (a trait is reuse, not a runtime type — unlike an
  interface), so there is no runtime trait identity to report, and PHP's `class_uses` is direct-only,
  which would not match the folded model. Use `Reflect.methods`/`fields` to inspect what a trait
  contributed. Also unprovided: reflection over enum variants (`interfaces(variant)` etc. return `[]`)
  and `Reflect.*` across packages with namespaced (FQN) class names.

## Reporting

Found something not listed here — especially a panic, hang, or crash on any input? That's a bug.
Please report it (see [SUPPORT.md](SUPPORT.md); for security, [SECURITY.md](SECURITY.md)).
