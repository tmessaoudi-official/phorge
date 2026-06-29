# Known Issues & Limitations

Phorj is pre-1.0. This page lists current limitations and known rough edges. Most "limitations" are
**deliberate scope boundaries** — features that are *planned* (see [ROADMAP.md](ROADMAP.md)) rather
than broken. The key property is that out-of-scope constructs are **rejected cleanly** (a type or
parse error, non-zero exit) — never a crash.

## Language features not yet implemented

These are designed but not in the current surface; using them produces a clean compile-time error,
not a panic:

- **Static method call sites — shipped corners + deferrals.** `ClassName.method(args)` calls a `static`
  method directly on the class (the static-factory pattern, e.g. `Greeter.make("w")`); calling an
  *instance* method this way is `E-STATIC-CALL`. **Inherited / trait static methods now work**
  (Statics-A, 2026-06-28): `Child.parentStatic()` resolves the declaring class's body via the shared
  `method_origins` dispatch table, and a `trait`-supplied static is callable on the using class —
  byte-identical run≡runvm≡real PHP. **Overloaded static methods now work too** (Statics-B, 2026-06-28):
  `ClassName.m(args)` over an overloaded `static` selects the matching body at runtime via the VM's
  `Op::CallStaticOverload` (a dummy receiver below the args + the shared `dispatch::select_overload`),
  the same selector the interpreter and the transpiled PHP `static` dispatcher use — byte-identical
  run≡runvm≡real PHP (`examples/guide/overloaded-statics.phg`). All overloads of one name must agree on
  `static`-ness (`E-OVERLOAD-STATIC-MIX`), matching PHP. **Remaining deferrals** (each rejected cleanly,
  never a runtime divergence): (1) A static method using the **class's own type parameter** (a static on
  a generic class) is out of scope — no instance binds the class type argument. (2) **Late static
  binding** (`static::` / `new static()`) is a deliberate non-feature (a runtime called-class concept +
  the `self::`/`static::` footgun) — override the static per subclass instead.

- **PHP-reserved identifiers as symbol names — now guarded (F-m, kind-aware).** Phorj and PHP have
  different keyword sets, so a Phorj identifier that is a *PHP* reserved word would transpile to
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
  comparison/equality, unary `-`, and BCMath transpile. Notes: (1) **`%` and `/` are operators**
  (2026-06-27): bare `decimal % decimal` is the exact remainder (`Op::RemD` → `value::decimal_rem` →
  `bcmod`), no rounding, result scale = `max(operand scales)`, zero divisor faults. Bare `decimal /
  decimal` is **exact-or-fault** (`Op::DivD` → `value::decimal_div_exact`): a terminating quotient
  returns the exact value in minimal form (`10.0d/4.0d → 2.5`); a **non-terminating** quotient
  (`1d/3d`) faults `"decimal division is not exact"`, a zero divisor faults, and a result past i128
  range faults `"decimal overflow"`. Use `Decimal.div(a, b, scale, mode)` for an explicit *rounded*
  quotient. (The non-terminating/zero faults are fault-domain, excluded from the example oracle; the
  exact paths are byte-identity-gated through `decimals.phg`.) (2) **i128 overflow is
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
  overload. (3) **Decimal modulo SHIPPED** (2026-06-27) — `decimal %` is the exact remainder operator
  (`Op::RemD`); the result keeps `max(operand scales)` and a zero divisor faults `"decimal modulo by
  zero"`. (4) **A `scale` past 255** (`u8::MAX`) faults `"decimal scale out of range"` —
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

- **Math breadth + number formatting (M-NUM S4) — shipped corners.** `Core.Math` gains `sign`/`clamp`/
  `gcd` (int), `log`/`log10`/`exp`/`sin`/`cos`/`tan`/`pi`/`e` (float), and `numberFormat(float, int) ->
  string`. Corners: (1) **transcendentals are not printed *raw*** — `log`/`exp`/`sin`/… erase to PHP's
  libm, and a non-representable result would diverge between Rust's shortest-round-trip and PHP, so the
  guide exercises them at their *exact* IEEE-defined points (`exp(0)`=1, `sin(0)`=0, `cos(0)`=1, …) and
  prints real values through `numberFormat`, which collapses any last-ULP libm difference. The
  `run ≡ runvm` spine is always identical (both Rust). (2) **`numberFormat` rounding is byte-identical**
  (fixed 2026-06-27) — both `value::number_format` and `__phorj_number_format` now **digit-string
  round** the *shortest-round-trip* decimal (`__phorj_float`, identical to Rust's `{}` Display)
  half-away-from-zero by carry, NOT `(value * 10^d).round()`. So a half-way money value rounds the
  intended decimal identically on all three backends (`numberFormat(0.285, 2) == "0.29"`); the old
  `f64::round`-vs-PHP-`round` boundary divergence is gone. (3) **`gcd` with the
  `i64::MIN` magnitude faults** — `gcd(i64::MIN, i64::MIN)`/`gcd(i64::MIN, 0)` would be `2^63`, outside
  i64, so it is a clean `"integer overflow"` fault (EV-7), exercised by the `math_gcd` unit test, not the
  example set.

- **`Core.Json` — shipped corners + deferrals.** (1) **Float magnitude divergence from native
  `json_encode`:** Phorj renders a float with the positional shortest-round-trip form (`__phorj_float`)
  for consistency with `run`/`runvm` everywhere, so an extreme magnitude (`1e20`) stringifies as
  `100000000000000000000`, not json's `1.0e+20`. `run ≡ runvm ≡ real PHP` is always byte-identical (the
  PHP leg uses the same helper); only the comparison to PHP's *native* `json_encode` differs at
  magnitude extremes. (2) **Multi-package now works** (validated 2026-06-29): a multi-package project
  that `import`s `Core.Json` round-trips byte-identically `run ≡ runvm ≡ real PHP`
  (`examples/project/jsonmulti/`). The injected `Json` enum is a `package Main` type, so in a namespaced
  program its variant classes live in `\Main\`; the JSON runtime helpers (emitted in the global block)
  now reference them as `\Main\Obj` etc. instead of bare names. (The companion fix: the loader's
  `Expr::Map` resolution arm — a cross-package call/type nested in a map literal `[k => v]` was
  previously left unresolved.) (3) **Reserved-variant collision
  edge:** an enum literally declaring both `Int` and `Int_` would collide after mangling — adversarial,
  not hit by any first-party code. (4) `NaN`/`Infinity` (non-JSON) stringify to `NaN`/`inf` tokens
  (consistent across backends, not standard JSON).

- **Stack traces — slice 1 (reporting) shipped; deferrals:** (1) catching/handling faults — a
  `try`/`catch` or `Result<T, E>` model — is a separate later slice; this slice only *reports* faults
  that abort. (2) Method/constructor/closure frames show `line`-only (no `file:line`) — their frame
  names are backend-synthesized, not in the loader's function→file map; free functions + `main` get
  full `file:line`. (3) Frame lines are statement-granularity, so a fault inside a multi-line
  expression may report the statement's start line. (4) Trace text is intentionally uncolored
  (matches Phorj's plain-diagnostic convention). (5) Stack traces do not yet print a "caused by"
  cause chain — the *data* exists (M-faults 2c: a `cause` field is preserved and, on transpile, populates
  PHP's native `$previous`), but the Phorj fault renderer does not walk it; folding the cause chain into
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
  A class that declares **its own** constructor *under inheritance* now initializes inherited parent
  state by forwarding with **`parent.constructor(…)`** (B1b shipped — see (5)). Also a
  *non-promoted* ctor-param **name collision across two parents** would emit a duplicate PHP parameter
  (rare; promoted-field collisions are already `E-MI-FIELD-CONFLICT`). (4) A class that is **both a multi-parent leaf and an ancestor of another multi-parent
  class** ("multi-of-multi") takes the `implements/use` path and is not also emitted as a trait — a deep
  edge case outside S6's `package Main` scope. (5) **`super`/`parent` dispatch — B1a shipped (methods, single inheritance).** `parent.m(…)`
  (nearest declaring ancestor) and `parent(A).m(…)` (jump to a named transitive ancestor) invoke the
  inherited method an override shadows; resolution is lexical + non-virtual + single-sourced
  (`ast::resolve_parent_method`), one new `Op::CallParent`, transpiles to native PHP `parent::m`/`A::m`,
  byte-identical run≡runvm≡real PHP (`guide/parent-dispatch.phg`). Errors
  `E-PARENT-OUTSIDE-METHOD`/`-NO-PARENT`/`-NOT-ANCESTOR`/`-NO-METHOD`/`-AMBIGUOUS`.
  **B1b shipped (parent-constructor forwarding, single inheritance):** `parent.constructor(…)` (immediate)
  and `parent(A).constructor(…)` (named ancestor) run the parent constructor's effect — parameter
  bindings, promotions, field initializers, body — on the existing instance, lowered by *front-end
  inlining* before any backend (NO new `Op`/`Value`), byte-identical run≡runvm≡real PHP
  (`guide/parent-constructor.phg`). Statement-only inside a constructor body; codes
  `E-PARENT-CTOR-OUTSIDE`/`-STMT`/`-MI`.
  **B2 shipped (multiple-inheritance parent-*method* dispatch, transpiler trait aliasing):**
  `parent(A).m(…)` / `parent.m(…)` inside an MI class (or a decomposed-ancestor trait body) lower to a
  `private` trait alias — `use … { T<dp>::m as private __super_<dp>_<m>; }` ⇒ `$this->__super_<dp>_<m>(…)`
  (the `run`/`runvm` backends already dispatched MI via `Op::CallParent`; B2 fixes only the PHP emission).
  Byte-identical run≡runvm≡real PHP (`guide/parent-dispatch-mi.phg`). **Deferred:**
  (a) **multiple-inheritance constructor forwarding** via the bare form (`E-PARENT-CTOR-MI`) — the
  idiomatic per-parent `parent(P).constructor(…)` already works on all three backends (B1b inline);
  (b) a parent-method jump to a **non-direct** ancestor under MI (`parent(G).m()` through an MI arm) —
  PHP cannot alias a transitively-`use`d trait method, so this is a **clean transpile error** (the
  `run`/`runvm` backends handle it); (c) the **multi-of-multi** trait lowering — a class that is both an
  MI leaf and an MI ancestor takes the `implements`/`use` path and is not also emitted as a trait (a deep
  edge outside `package Main` scope); (d) an **overloaded** parent method (the compiler resolves via the
  `methods` table, which doesn't carry the overload set — single-method parents only for now).
  **(e) Cross-package single inheritance + parent calls now ship** (validated 2026-06-29): a
  `package Main` class may `extends` a library-package class (imported via `import type`), inherit its
  constructor + fields, override its `open` methods, and call up with both `parent.m(…)` and the named
  `parent(Ancestor).m(…)` form — the loader mangles the `extends` parent name and the
  `parent(Ancestor)` reference to the library FQN, the transpiler emits `extends \Acme\Zoo\Animal` +
  `parent::m()`. Byte-identical `run ≡ runvm ≡ real PHP` over a two-level chain
  (`examples/project/inherit/`). Cross-package **multiple** inheritance (a class decomposed to PHP
  traits across packages) is still out of scope (the MI transpile path is `package Main`-only).

- **Traits — S8 shipped; deferrals (all clean compile-time, or transpile-oracle-gated):** `trait`/`use`
  composition (methods, `mutable`/`static` state, a trait constructor, abstract requirements, property
  hooks) is in, byte-identical across backends + real PHP 8.4. Deferred: (1) **traits as types** —
  intentional and permanent; a trait is reuse, not a type (`E-USE-AS-TYPE`/`E-INSTANCEOF-TYPE`). Use an
  interface for the type side. (2) **generic traits** (`trait T<X>`) — mirror the generic-method gate;
  not yet parsed. (3) **cross-package traits now ship** (validated 2026-06-29) — a `trait` declared in a
  library package is imported with `import type Pkg.Path.Trait [as A];` (it is still NOT a type —
  `Trait x` as an annotation stays `E-USE-AS-TYPE`) and composed with `use Trait;`. The loader registers
  the trait in the type symbol table and mangles both the declaration and the `use` clause to the same
  FQN, so the checker's by-name trait flatten lines up; the transpiler emits a native PHP `trait` in its
  package namespace and the using class composes it via `use \Acme\Mix\Greet`. Method reuse, a private
  helper, and an abstract requirement satisfied by the using class all work byte-identically
  `run ≡ runvm ≡ real PHP` (`examples/project/mixins/`). Narrower remaining edge: a cross-package
  trait-vs-trait *conflict-resolution* clause (`use P.m` across packages) is not yet exercised, and a
  trait whose member calls another *cross-package* free function inside its own body inherits the same
  loader-rewrite scope as a class. (4) **trait-vs-trait
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
  (`E-INTERSECT-SIG`; uninhabited because Phorj has no overloading **yet** — overloading is the next
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
  **`x == null` / equality-literal refinement** — Phorj rejects comparing an optional/union to a
  literal (`T? == null`, `int|string == "ok"`), so there is no such narrowing source (use if-let /
  `??` / match-over-optional / match-over-union instead); **post-match scrutinee narrowing** — a
  `match` is an expression and its arms are expressions (no statement-match with diverging arms), so
  there is no fall-through to narrow. (**if-let and while-let `when` guards both ship** — see the
  pattern-cluster note below.)
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
  **if-let `when` guards ship** (S5.3 — `if (var u = opt when u.active) { … } else { … }`, desugared to
  a nested `if (guard)` in the bound then-scope, the else shared by bind-fail and guard-false) and
  **while-let `when` guards ship** (S2.4 — `while (var x = opt when g) { … }`, desugared so a false
  guard `break`s the loop). Both are pure parser desugars (no `Stmt::If.guard` field, no backend
  change), byte-identical run≡runvm≡real PHP.
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
  transpile to PHP `public` (Phorj does not enforce field visibility at runtime; `readonly`/`final`
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
  (`parent::__construct($message, 0, $cause)` → `getPrevious()`); the Phorj backends read it back as a
  plain field, byte-identical `run ≡ runvm ≡ real PHP`. Two deliberate deferrals remain: **reading a
  cause through PHP's `getPrevious()` accessor** (a `.cause()` method form, as opposed to the field read)
  is only meaningful for a *foreign* PHP exception, so it folds into **PHP interop (M8.5)**; and
  **catching PHP-thrown exceptions across the interop boundary** now ships in **M8.5 S3a** —
  `declare class … implements Error` makes a foreign exception catchable (`catch (\Name $e)`,
  PHP-target-only). Still open: reading a cause via `getPrevious()` as a method, and a typed `?` catch
  over a foreign throw.

## Interop (M8.5) — deferrals

`declare function`/`declare class` (S1/S2), `.d.phg` ambient declaration files + foreign-exception
`catch` (S3) all ship — PHP-target-only, validated via transpile → real PHP golden (`tests/interop.rs`).
Open corners (each is a documented bridge limit, never a crash):

- **A `.d.phg` symbol is a global** (`\strlen`) with no package, so it is not in the package symbol
  table — a bare call to a foreign name that also exists as a package function is resolved foreign-first
  by the transpiler. Keep foreign declaration names distinct from your package function names.
- **Vendored declaration bundling is deferred** — a `*.d.phg` is collected only from the project's own
  source root, not from a dependency's `vendor/<name>/` tree.
- **`phg build` stays single-file** — it does not merge a project's `.d.phg` (or any multi-file project).
- **No `.d.phg` generation from PHP source**, no namespaced foreign symbols beyond a single leading `\`,
  no foreign *generic* PHP types, and **running** a foreign program on the Rust backends (needs a PHP
  FFI — out of scope; `E-FOREIGN-RUNTIME` refuses it, transpile instead).

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

- **Overloaded constructors** are not supported (PHP cannot overload a constructor either; Phorj has
  constructor promotion and — when it lands — default arguments). Overload a static factory method.
- **Return-type overloading SHIPPED for free functions** (M-RT Slice C1, 2026-06-29): free functions
  may share a name AND parameter signature, differing only in return type, resolved at compile time by
  an explicit `<Type>f(args)` selector and mangled per return before any backend
  (`examples/guide/return-overloading.phg`, byte-identical `run ≡ runvm ≡ real PHP`). Remaining C1
  deferrals: (1) **methods SHIPPED** (M-RT S2.2, 2026-06-29): a class method may now return-overload too
  (`examples/guide/method-return-overloading.phg`), resolved by a `<Type>receiver.m(args)` selector and
  mangled per return (`read__ret_int`), byte-identical `run ≡ runvm ≡ real PHP`, no new `Op`. Method
  scope is **C1-equivalent** (deliberately tighter than free fns): the selector is the ONLY resolving
  context — a bare method overload call is `E-OVERLOAD-NO-CONTEXT` even at a typed binding/`return`
  (no C2 sink for methods yet); a **single declaring class** only — a return-overloaded method
  overridden across an `extends`/interface hierarchy (a base-typed receiver resolving the mangled name
  through a subclass) is not yet handled and is a follow-up; and a return-overloaded method on a
  *generic* class works for concrete-return members (the type param is substituted at the selector) but
  a member returning the bare class param is not selectable; (2) **C2 sink-widening is partial** — a *typed
  binding* and a *`return`* now resolve a selector-less call from their declared type, but the remaining
  sinks (typed *reassignment* `x = f()`, typed *field write* `this.f = f()`, *argument* to a
  non-overloaded typed parameter) still need a `<Type>` selector, and `E-OVERLOAD-SELECT-CONFLICT`
  (selector disagrees with a sink) is still reserved (a selector at a sink currently just type-checks
  its result against the declared type); (3) **mixing** parameter- and return-overloading in one name is rejected (`E-OVERLOAD-RETURN`) —
  a name is either parameter-overloaded (distinct params, shared return) or return-overloaded (identical
  params, distinct returns); (4) the per-return mangled name (`f__ret_int`) is a slug of the return
  type's display, so two return types with the same slug (pathological — e.g. a user type literally
  named like another type's slug) could collide — not observed in practice.
- **Generic overloads** are rejected (`E-OVERLOAD-GENERIC`): a generic declaration must be the sole one
  of its name. A first-class *value* of an overloaded function is also rejected (`E-OVERLOAD-FN-VALUE`)
  — call it directly or wrap the intended overload in a lambda.
- **Ambiguity is detected at runtime, not compile time.** A cross-cutting multi-argument overload set
  with no unique most-specific match for some call faults cleanly *when that call runs*
  (`ambiguous overloaded call to …`, byte-identical on both backends). A compile-time ambiguity check
  is a future refinement; identical signatures are already rejected at declaration
  (`E-OVERLOAD-DUPLICATE`).
- **PHP-erasure overload collisions are REJECTED at declaration** (`E-OVERLOAD-ERASE`, 2026-06-27):
  overloads that differ *only* by `string`-vs-`bytes`, or *only* among `List`/`Map`/`Set` (both erase
  to PHP `string` / `array`), are caught at compile time rather than producing a transpile-only
  divergence on an ambiguous call. Differentiate by another parameter or merge them. (The general
  runtime-ambiguity case for distinguishable multi-arg sets is still a runtime fault — see above.)
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

- **A generic-typed *result* is a specialized operand only when the return *echoes a parameter*
  (S2.1 — partial).** A generic free function whose declared return is *exactly* one of its own
  parameters (`id<T>(T x) -> T`, `firstOr<T>(List<T>, T) -> T`) now records that parameter index
  (`FunctionDecl::generic_ret_from_param`, set in `erase_generics`); the VM compiler's `ctype` recovers
  the erased result's operand type from that argument, so **`identity(7) + 1` and `firstOr(xs, -1) * 2`
  now specialize on the VM** exactly as the interpreter evaluates them (byte-identical, gated by
  `examples/guide/generics.phg`). [Verified: both `run` and `runvm` print `8`.] **Still deferred** (the
  result does *not* echo a single parameter, so it stays `CTy::Other` and the VM rejects a bare
  arithmetic use — bind to a typed local first): generic **methods**/**fields** (`box.get() + 1`),
  a `List<T>`-element or container return, and a return computed from *several* parameters. The full
  fix is the reified-result side-table threaded into the compiler (still deferred).
- **Generic *interface* methods** are a non-parse — an interface method's signature is built with an
  empty type-parameter list, so a `<T>` there is never consumed. Generic methods on *classes* work.
- **Cross-package generic *library* types now ship** (validated 2026-06-29) — a generic class
  (`Box<T>`, `Pair<A, B>`) declared in a library package is consumed from another package via
  `import type Pkg.Path.Type`, inferred at construction and recovered at each use site, with invariant
  type arguments enforced across the package boundary. The loader leaves the type parameter untouched
  and `erase_generics` removes it before any backend, so it rides the same erasure path as a
  `package Main` generic class — byte-identical `run ≡ runvm ≡ real PHP`
  (`examples/project/genericbox/`). Generic *enums* in a library package are the same erasure path but
  not yet covered by a shipped example; cross-package generic *methods* on a non-generic library class
  likewise ride the existing method machinery.
- **Explicit type arguments at construction** (`Box<int>(7)`) are not parsed — the type argument is
  inferred from the constructor arguments. An explicit *annotation* (`Box<int> b = Box(7)`) does work.
- **Generic enums** (`enum Option<T>` / `enum Result<T, E>`) ship, with the same scope as generic
  classes: `package Main`-only, inference-only construction (no `Some<int>(7)` explicit-argument form —
  use an annotation, `Option<int> n = None();`), invariant, no bounds, no generic *enum methods* (enums
  have no methods). A match-bound payload is reified at the scrutinee's concrete type (`Some(n)` over an
  `Option<int>` binds `n: int`), but — like every erased generic — that payload is `mixed` to the
  backend, so it is **not a specialized VM arithmetic operand** (the operand limitation above); since
  match arms are single-expression, return the payload into a typed local for arithmetic.
- **Same-head generic types ARE now invariant at an assignment boundary** (fixed — Soundness Batch B,
  finding #2). `Box<string>` / `Option<string>` is correctly **rejected** where `Box<int>` /
  `Option<int>` is expected. The nominal assignability arm now splits same-head (invariant type-arg
  comparison) from a true subtype edge, so the reflexive-name short-circuit no longer smuggles a
  mismatched type argument through. An un-inferred type arg (`new None()` ⇒ `Option<Error>`) still
  binds via the per-arg `Ty::Error` wildcard. (A nested un-inferred placeholder under another generic
  head — e.g. `Box<Option<Error>> -> Box<Option<int>>` — is conservatively rejected rather than bound;
  a rare, safe over-rejection.)
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

- **No bare field access — `this.field` is required everywhere** (2026-06-27, PHP-faithful — PHP has no
  bare field access, always `$this->field`). A bare name in a method resolves to a parameter, local, or
  captured variable, *never* a field; an instance field referenced without `this.` is `E-BARE-FIELD`
  (`E-STATIC-THIS` in a static method, where there is no instance). This removes the refactor footgun
  where adding a local silently rebinds a field reference, and makes method bodies and lambdas
  consistent (the old "bare field works in a method but not its lambda" gap is gone). Diagnostic-quality
  limitation: a bare field used *inside a string interpolation* (`"{name}"`) reports the error at line
  `1:1` rather than the real position (the interpolation sub-expression's span) — the error fires
  correctly, only the location is imprecise; a follow-up. Backend note: the interpreter/compiler retain
  their bare-field resolution paths, but the checker gates every program, so they are unreachable for
  valid code.
- **Lambdas and first-class function references now work inside library (non-`main`) packages**
  (validated 2026-06-29). The loader's name-mangling pass rewrites a same-package function reference in
  every position — at a call site, inside a lambda body (`fn(int x) => dbl(x)`), AND in value position
  (`var f = dbl;` / passing `dbl` to a higher-order call) — to its package FQN, so the backends resolve
  the mangled function. For `package Main` the mangle is a no-op, so single-file programs are
  byte-identical. Verified `run ≡ runvm ≡ real PHP` (`examples/project/funcvalues/`). Still deferred:
  **qualified / cross-package function *values*** — passing `Acme.Calc.dbl` itself (the dotted member as
  a value, vs. *calling* it `Acme.Calc.dbl(x)`) is not yet rewritten; call it, or wrap it in a local
  same-package function and pass that.
- **Statement-body lambdas require an explicit `-> T`** — the return type of a block-body lambda is
  not inferred (expression-body lambdas infer it from the expression). This is by design this slice.
- **Function-type assignability is exact structural equality** — no parameter/return variance
  (`(int) -> int` is not assignable to `(int) -> int?` etc.).
- **`core.list` higher-order helpers (`map`/`filter`/`reduce`) are not yet available** — they await
  the `List<T>`-generic native signatures; lambdas can already be passed to *user* functions today.

## Core.Html (Waves 1–3 — escape kernel + element builders + `html"…"` sugar)

- **An `html"…"` hole cannot contain a string literal with quotes.** Like every Phorj
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
  committed `vendor/`. Only `phg vendor` touches the network; commit `vendor/` + `phorj.lock` so
  builds stay deterministic and reproducible (the same determinism rule that defers URL/network to M6).

## M6 W2 router & `#[Route]` attributes (in progress)

- **Route patterns with `{param}` must be raw strings.** Write `r"/users/{id}"`, not `"/users/{id}"` —
  a normal string interpolates `{id}` as a variable (`E-UNKNOWN-IDENT`). Applies to both the
  hand-written `.route(...)` pattern and the `#[Route("GET", r"/users/{id}")]` argument.
- **Attributes are free-function-only.** `#[Route]` (or any `#[…]`) on a class, enum, interface,
  method, or import is `E-ATTR-TARGET`. Attributes on methods/classes are a later slice.
- **Only `#[Route]` has semantics.** The grammar parses any `#[Name(args)]`, but every name other than
  `Route` is a hard `E-UNKNOWN-ATTRIBUTE` (no silent ignore). A general attribute/annotation facility
  is future work.
- **M6 W2 extensions complete** (middleware, route groups, regex constraints, `#[Route]` on methods).
  `router.use(mw)`, `router.group(prefix, build)`, `{name:regex}` constraints, and `#[Route]` on
  **static** methods all work. Still deferred: optional segments / wildcards, **instance-controller
  routing** (a `#[Route]` method must be `static` — `E-ROUTE-METHOD-STATIC` — there is no
  controller-instance lifecycle yet). **W3 concurrency shipped:** `phg serve --workers N` is a bounded
  OS-thread pool (one request per worker, each its own heap), default = CPU cores, `--workers 1` =
  single-threaded; remaining serve work is refinement (HTTP keep-alive — today is `Connection: close`
  one request per connection; graceful shutdown/join; per-worker metrics).
- **`Core.Random` under `--workers > 1` shares one global stream.** The RNG state is a process-wide
  `RwLock<u64>` (thread-safe — no data race), but concurrent requests draw from the *same* advancing
  stream, so a given request's random values are not per-request reproducible under the pool (they are
  with `--workers 1`). This is benign — and usually desirable — for a server (distinct randomness per
  request). `Core.Regex`'s compiled-pattern cache is `thread_local`, so each worker compiles its own
  (correct; a small per-worker memory cost). No other native holds unsynchronized global state. A group's
  middleware is
  composed into its routes at merge time; deeply-nested group middleware ordering beyond one level is
  not specially tested.
- **Route constraints depend on `Core.Regex`** — importing `Core.Http` now also pulls in `Core.Regex`
  (the prelude matches constraints with it). With the `regex` cargo feature disabled (e.g. a custom
  playground build), a program that imports `Core.Http` would fail to resolve `Core.Regex`. Constraint
  matching is byte-identical run≡runvm≡PHP for ASCII patterns; exotic patterns inherit `Core.Regex`'s
  documented regex-crate-vs-PCRE caveats.
- **Router lives on the injected `Core.Http` types.** A program that declares its *own* `Request`/
  `Response` (the W1 examples) does not get the injected `Router`; import `Core.Http` to use it.

## `phg build` limitations (M2.5, in progress)

- **Cross-builds: source checkout OR a published registry (Phase 3a).** `--target`/`--all` compile a
  stub from source via `cargo-zigbuild` when run from a phorj source tree; a *distributed* (sourceless)
  phg instead **downloads** a prebuilt stub from the release registry and sha256-verifies it against its
  baked manifest. So a sourceless cross build works **once a tagged release has published the stubs**
  (the `stub-registry.yml` workflow); before the first such release, a sourceless binary still errors
  with the "needs a source checkout" message (its baked manifest is empty). Host builds always work
  offline (the running binary is the stub).
- **No code signing (Phase 3b deferred).** Downloaded/produced binaries are unsigned. Windows
  Authenticode + macOS codesign/notarize (and the macOS stub itself) need certs + a Mac SDK the
  maintainer does not currently have; `--sign` is not a flag yet. Integrity rests on the sha256 manifest
  (tamper-evident), not signatures.
- **macOS `--target` is rejected.** The Mach-O/fat section *reader* ships and is tested, but producing a
  macOS *stub* needs a macOS SDK for zig (Phase 3b). An apple/darwin `--target` errors with a clear
  message rather than emitting a broken binary.
- **The manifest is baked only into the `x86_64-linux-gnu` primary.** Cross-building *from* a Windows or
  aarch64 host isn't supported in v1 (those binaries carry an empty manifest → the "needs a source
  checkout" message); the primary dev host is the only cross-build origin needed now.
- **Built binaries honor argv + the exit code (Batch-1 B).** A standalone built binary passes its
  real command-line arguments to `Core.Process.args()` / `main`'s `List<string>` parameter and exits
  with `main`'s `int` return. (`--version`/`--help` remain features of the `phorj` CLI itself, not of
  built binaries — a built binary's argv belongs entirely to its embedded program.)
- **Process exit codes follow the OS 8-bit convention (0–255).** `main`'s `int` return is passed
  verbatim to the OS exit (`std::process::exit` / PHP `exit($n)`), so a value outside 0–255 wraps the
  same way on every backend (all defer to the OS); a value outside `i32` range from the Rust backends
  becomes exit 1. Use small, conventional codes.
- **aarch64 / Windows artifacts aren't executed in CI here.** They're validated by an object-section
  round-trip; native execution is verified for the host-runnable `x86_64-musl` target.

## `as` → primitives (M4 as-matrix) — deferred cells

The primitive `as` matrix (Unified, fallibility-typed) ships S1–S4: every concrete-primitive
conversion, primitive-union assertion, the bool cells, and `float`/`string as decimal?`. Deferred
(each rejected cleanly with `E-CAST-TYPE`, never a crash):

- **`as decimal` on a *union* source is unsupported.** A decimal's PHP carrier is a string, so
  `is_*` cannot distinguish a `decimal` union member from a `string` one at runtime — the assertion
  would diverge between the Rust backends and PHP. Convert the concrete arm explicitly instead.
- **Erased-generic / `mixed` sources are not assertable.** `as` on a primitive target requires a
  concrete primitive or a primitive *union* source; an erased generic value (`mixed`) has no
  distinguishable static shape. Bind it to a typed local first.
- **`float as decimal?` captures the *displayed* value, not the exact binary.** It parses the float's
  shortest round-trip string (`2.5 → 2.5`, `0.1 → 0.1`), so it matches what the float prints, not the
  exact IEEE-754 value. A float whose shortest string overflows i128 → `null` (the overflow boundary
  is not guaranteed byte-identical to PHP at the extreme edge — examples stay in range).
- **`string as bool` is strict** (`"true"`/`"false"` only) — `"1"`, `"yes"`, `""`, `"false"`-as-true
  are all `null`. This is deliberate: Phorj never inherits PHP's string truthiness.

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
- **Bool map keys: PHP coerces `true`/`false` to `1`/`0` as array keys; Phorj keeps them distinct.**
  A `Map<bool, V>` works and is byte-identical *as long as you don't also use `0`/`1` int keys in the
  same map* (PHP would collapse `true` and `1`). Prefer string/int keys when transpiling to PHP.

## Generic natives (M-RT S7b — `Core.List` / `Core.Map`)

The first generic stdlib natives ship this slice: `Core.List` `reverse`/`sum` and `Core.Map`
`keys`/`values`/`has`/`size`. Their signatures carry `Ty::Param` and unify at the call site exactly
like a generic free function; the parameter is registry-only and never reaches a backend. Two PHP-leg
caveats (the `run`/`runvm` spine is always byte-identical):

- **`List.sum` faults on i64 overflow; PHP `array_sum` promotes to float instead.** The checked sum
  keeps EV-7 (never panics), so a sum exceeding `i64::MAX` is a clean Phorj fault, whereas PHP would
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

## Core.Text breadth (M4) — ASCII only

`Text.reverse`/`equalsIgnoreCase`/`containsIgnoreCase` (like all of `Core.Text`) are **ASCII-oriented** —
the PHP oracle runs `php -n` (no mbstring), so they map to byte/ASCII core functions
(`strrev`/`strcasecmp`/`stripos`). `reverse` reverses by chars (== bytes for ASCII; a non-ASCII string
would byte-reverse differently in PHP `strrev`); the case-insensitive pair folds only ASCII letters.
Shipped examples keep ASCII subjects, where all three backends agree. Codepoint-aware (Unicode) text ops
are deferred to a future M-text milestone (would need an mbstring-or-equivalent strategy).

## Public-surface file-naming rule — scope

The rule (`E-FILE-NAME`/`E-FILE-MULTI-PUBLIC`/`E-FILE-MIXED-PUBLIC`) is enforced by the loader in
**project mode** only.

- A file declaring `main` is fully exempt (programs mix freely). Loose single-file (`phg run x.phg`) and
  `-e`/stdin are `main`-only ⇒ exempt. So the rule shapes multi-package projects, not single-file guides.
- `private`/`internal` helper types and functions ride along free (no PSR-4 micro-file tax); only the
  *public* surface is constrained.
- **Deferred:** a per-project opt-out; applying the rule inside `package Main` (entry files stay exempt
  by design); auto-rename tooling (`phg fmt --rename-files`).

## Foreign PHP interop (M8.5) — scope + deferrals

`declare function …;` (S1) describes a foreign PHP function so Phorj can type-check calls and transpile
to `\name(...)`. Interop is a **migration bridge**, transpile-target-only by nature.

- **A program using `declare` cannot run on the Rust backends** (`E-FOREIGN-RUNTIME`) — foreign PHP needs
  the PHP runtime. `check`/`transpile` work; run it via `phg transpile app.phg > app.php && php app.php`.
  This is by design (the byte-identity spine covers pure Phorj only); such programs are quarantined from
  the `differential.rs` oracle and gated by `tests/interop.rs` (transpile → real PHP golden).
- **`declare class` (foreign PHP classes) shipped (S2):** constructor / instance methods / static
  methods / public fields → `new \Name`, `$o->m`, `\Name::s`, `$o->f`. Scope: `package Main`, no
  `extends`/`implements` on a foreign class, no foreign generics.
- **`.d.phg` declaration files and foreign-exception `catch` are not yet implemented** (M8.5 S3). Inline
  `declare` in the program covers the core today.
- **No foreign generics, no namespaced foreign imports beyond a single leading `\`**, no automatic
  `.d.phg` generation from PHP, no Composer-package declaration bundling.
- A `declare`d function's parameter *names* are never emitted, so they are not casing-checked; the
  function *name* is emitted verbatim (snake_case PHP names are intended).

## Core.Time (M-TIME) — determinism + scope

`Core.Time` models `Instant`/`Duration` (S1) as an injected pure-Phorj prelude, so all arithmetic is
byte-identical by construction. The clock is the one non-deterministic surface, deliberately quarantined.

- **Unfrozen `Instant.now()` is non-deterministic** and therefore cannot appear in a byte-identity-gated
  example/conformance program — it reads the real wall clock, which differs per run and per backend. A
  program that wants reproducible output calls `Time.freeze(ms)` first (the `Core.Random` pattern); all
  shipped examples freeze. `Time.unfreeze()` restores real-clock behavior. The frozen clock is
  process-global, so under `phg serve --workers > 1` it is shared across worker threads (same caveat as
  `Core.Random`).
- **UTC-only, no timezones.** Civil breakdown (S2/S3) is always UTC. A `ZonedDateTime` / timezone
  database is out of scope — timezones are environment-dependent and would break the byte-identity spine.
- **Millisecond precision; no sub-millisecond.** `Instant` is integer epoch-millis; nanos are not modeled.
- **No locale-aware or arbitrary-format parsing** (S3 ships fixed ISO-8601 output only).

## Core.Regex (Fork A) — documented edges + deferrals

`Core.Regex` is backed by the `regex` crate (RE2-style, linear-time, ReDoS-immune). The byte-identity
spine (`run ≡ runvm ≡ real PHP`) holds on the **regular subset** the engine accepts; the items below
are deliberate edges, each either rejected cleanly or kept inside ASCII where the three backends agree.

- **Backreferences / lookaround are rejected at `Regex.compile`** (the engine omits them by design —
  they would force backtracking, the ReDoS hazard). A clean fault, never a divergence. This *is* the
  "restricted-subset dual-engine parity" — the omitted set is exactly the non-regular part of PCRE.
- **`\d` / `\w` / `\s` are Unicode-aware on the Rust backends, ASCII-only in transpiled PCRE** (no
  `(*UCP)`). So a Unicode-digit subject would diverge between the backends and the PHP leg. Shipped
  examples keep **ASCII** subjects, where all three agree. (A future `(*UCP)` emission could align them.)
- **Named captures only** — `findGroups` returns `Map<string,string>?` keyed by group name; numbered
  groups are intentionally not exposed. A named group that does not participate in the match is omitted.
- **Always Unicode (`/u`), case-sensitive.** Inline flags / case-insensitivity (`Regex.compileWith`)
  are deferred — add when requested.
- **`replace` replacement syntax** uses the `$1` / `${name}` form shared by the `regex` crate and PHP
  `preg_replace`; PCRE-only `\1` backslash references are not portable (use `$1`).
- **Patterns must use raw strings** `r"..."`: a normal `"\d{4}"` parses `{4}` as `{expr}` string
  interpolation (silently yielding `\d4`) — both backends agree, but the pattern is wrong. Not a bug in
  Regex; a consequence of interpolation. The guide example and docs use raw strings throughout.
- **Multi-package transpile is a follow-up** (same boundary as `Core.Json`): the injected `Regex`
  class lives in the entry package, so a *namespaced* multi-package program emitting `new Regex(...)`
  inside another package block is untested. Single-package `run ≡ runvm ≡ real PHP` is gated green.

## Secret<T> (Fork B) — scope

`Secret<T>` is an opaque wrapper whose guarantee is by construction: a `Secret` is non-printable
(`Console.println(s)` / interpolation is a type error) and its value is private (`.expose()` is the
only read path). Deliberate scope edges:

- **`W-SECRET` is syntactic on the direct sink argument.** It flags `sink(secret.expose())` (where
  the sink is `Console.println`/`print` or `Core.File.write`) but **not** a value laundered through a
  local (`var p = s.expose(); println(p);`). Full taint/flow analysis is out of scope — the
  type-system non-printability is the real guarantee; the lint is a convenience for the common slip.
- **No runtime `***` redaction.** Path 1 (opaque + non-printable) was chosen over a runtime-redacting
  wrapper, so there is no `Value::Secret` and a Secret never renders as `***` — it simply can't be
  printed. (PHP gets `#[\SensitiveParameter]` for *trace* redaction; Phorj's own traces don't dump
  local values, so there is no in-Phorj leak vector to redact.)
- **The lint keys on the type name `Secret`.** A user-defined class also named `Secret` with an
  `expose()` method would be linted too (harmless — the signal still applies).
- **Multi-package transpile is a follow-up** (same boundary as `Core.Json`/`Core.Regex`): the injected
  `Secret` class lives in the entry package; namespaced multi-package emission is untested.

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
- **Float display is byte-identical across all three backends.** A finite `float` renders identically —
  the transpiler's `__phorj_float` runtime helper reproduces Rust's shortest-round-trip,
  always-positional `f64` Display exactly (so `sqrt(2.0)` → `1.4142135623730951`,
  `1234567890123456.0` → `1234567890123456`, and `0.00001` → `0.00001` all match, with no PHP
  `precision=14` rounding or scientific-notation switch — see `guide/floats.phg`, which round-trips
  every magnitude through real PHP). **Float division by zero now FAULTS** (resolved 2026-06-27, the
  "any division by zero throws" rule): `1.0 / 0.0` → `"division by zero"` and `1.0 % 0.0` → `"modulo by
  zero"` on `run`/`runvm` (no IEEE `inf`/`NaN`), and the transpiled PHP throws `DivisionByZeroError`
  to agree (`/` throws natively; float `%` routes through `__phorj_rem`, which guards `$b == 0`). A
  finite overflow-to-`inf` (huge ÷ tiny non-zero) is *not* a zero division and stays `inf`;
  `__phorj_float` renders `inf`/`-inf`/`NaN` the Rust way if one is reached through other means.
- **`opt!`-on-null fault: message body matches across backends; only the source location differs.**
  A null force-unwrap faults with the body `force-unwrap of null` on **all three** backends — `run`/
  `runvm` (located, classified `FaultKind::ForceUnwrap`) and the transpiled PHP, which throws
  `RuntimeException("force-unwrap of null")` (same body, verified 2026-06-27). The only residual
  difference is the *location*: PHP's exception carries the generated `.php` file:line, not the Phorj
  source line — inherent to transpilation (a PHP exception has no Phorj source position) and
  fault-domain (the differential harness excludes fault cases by design), so it never affects the
  byte-identity spine. The *present-value* case is fully byte-identical.
- **`package Main` function names must avoid PHP built-in names (transpile target).** A top-level
  function in `package Main` transpiles to a *global* PHP function, so naming one `serialize`,
  `strlen`, `header`, … collides with the PHP builtin (`Cannot redeclare function …`). The Phorj
  backends are unaffected (everything is namespaced); only the PHP round-trip fails. Library packages
  are namespaced and immune. Pick non-builtin names for `package Main` functions intended to transpile
  (e.g. `serializeResponse`, not `serialize`).
- **Member visibility is enforced (Wave 1.1 — was a byte-identity hole).** An external read/write of a
  `private`/`protected` instance field (incl. a promoted ctor param), or an external call of a
  `private`/`protected` method, is now a **compile error** (`E-FIELD-VISIBILITY`/`E-METHOD-VISIBILITY`)
  — so `run`/`runvm`/transpiled PHP all reject it instead of the Phorj backends accepting what PHP
  would throw on (`Cannot access private property`). Declare the member `public` (the default) when it
  is accessed from outside, or expose it through a public accessor (`obj.valueOf()`). A `private` member
  used only inside the declaring class — and a `protected` one inside that class or a subclass — is
  fine. (Remaining narrower corners — `private` *static* fields and intersection-typed receivers — are
  noted near the declaration-visibility entry above.)

- **`Core.Reflect.traits` is not provided.** `Reflect.interfaces`/`parents`/`methods`/`fields` are
  available, but there is no `traits` enumeration native. A Phorj `trait`'s members are *folded into*
  the using class before any backend runs (a trait is reuse, not a runtime type — unlike an
  interface), so there is no runtime trait identity to report, and PHP's `class_uses` is direct-only,
  which would not match the folded model. Use `Reflect.methods`/`fields` to inspect what a trait
  contributed. Also unprovided: reflection over enum variants (`interfaces(variant)` etc. return `[]`)
  and `Reflect.*` across packages with namespaced (FQN) class names.

- **`phg test` — known limitations (M-Test).** The test runner is intentionally minimal this
  milestone. (1) **Tests run on the interpreter only** — there is no `--vm` mode yet to also run each
  test on the bytecode VM as a free parity check. (2) **No fixtures / setup-teardown, no parameterized
  (table) tests, no TAP/JUnit output, and no PHPUnit-emitting bridge** — each is an additive follow-up
  on top of the core runner. (3) A failing test's **stack-trace frame is named `main`** (the test body
  is lowered into a synthetic `main` to reuse the normal check/expand/run pipeline) — the test's own
  name is on the result line, not in the trace. (4) **A project whose entry is a class `static main`**
  could collide when a test file is loaded in project mode (the runner drops a *top-level* `main` when
  synthesizing the per-test entry, not a class-static one) — keep test files self-contained or use a
  library project. `Core.Test` is `pure` but only meaningful under `phg test`; its PHP emission exists
  only for a future `--emit-phpunit` bridge and is **not** byte-identity-gated.

- **`phg fmt` — v1 limitations (M-fmt).** The formatter is *tidy + comment-safe*, not yet opinionated.
  (1) **No line-wrapping / width reflow** — a long line stays long; canonical indentation, spacing,
  and blank-line collapse only. (2) **Comment reattachment is position-based**, not a full lossless
  CST: an own-line comment formats above the following declaration/statement, but a **trailing
  same-line comment** (`x = 1; // note`) reattaches as a *leading* comment of the next node, and a
  comment **above the `package` line** moves just below it. Comments are never lost, and the result is
  idempotent — just occasionally relocated. (3) A **statement-body lambda** (`fn(x) -> T { … }`) is
  rendered on a single line (a lambda is an expression; no reflow yet). All three are additive
  follow-ups; the hard guarantee — formatting never changes program meaning (`parse(fmt(x))`
  preserved) — holds today, gated by a dogfood test over the whole example corpus.

## Reporting

Found something not listed here — especially a panic, hang, or crash on any input? That's a bug.
Please report it (see [SUPPORT.md](SUPPORT.md); for security, [SECURITY.md](SECURITY.md)).
