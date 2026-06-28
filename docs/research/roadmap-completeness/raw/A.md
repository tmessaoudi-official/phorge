# Track A — PHP Parity Gap Audit

## Track summary

Phorj has already absorbed an unusually large slice of PHP 8.0–8.4's *type-system* surface
(interfaces, enums-with-payload, `match`, generics, unions, intersections, `instanceof` + smart-cast,
optionals + `?->` + `??`, arrow/closure lambdas, first-class callables, property hooks lowered to PHP
8.4 hooks, asymmetric-ish *declaration* visibility, packages). The remaining PHP-parity gaps cluster
in three bands: **(1) control-flow & error handling** — `try`/`catch`/`throw`, `finally` — the single
largest user-visible hole, already planned but unbuilt; **(2) class-mechanics PHP devs reach for daily**
— `abstract` classes/methods, late static binding (`static::`/`new static`), typed/interface class
**constants**, `__toString`/`__invoke`/`__clone` and the dynamic magic-method family, `#[\Override]`,
`readonly`, asymmetric *member* visibility `private(set)`; **(3) call-convention sugar** — named
arguments, variadics + spread `...`, `list()`/array destructuring, heredoc/nowdoc, `declare(strict_types)`.
Two PHP features are honest *omits* under the philosophy (references `&`, fibers as a surface) because
Phorj's value/immutability model and uncolored-concurrency plan supersede them. Backed enums and
enum methods/interfaces are a clean, high-fit **adopt** that PHP devs expect. `json_validate`,
SPL/`ArrayAccess`/`Countable`/`Iterator`, generators/`yield`, and streams are stdlib/runtime concerns
that map to existing milestones (M11 stdlib, M6 concurrency). Almost every adopt item is *erase-* or
*lower-to-idiomatic-PHP* shaped — exactly the TypeScript-over-JavaScript contract — so they earn their
surface budget. The few rejects are PL features that would *add surprise* (operator-style coercions,
`goto`) or capability Phorj deliberately reshapes (`&` references).

## Gap table

| id | title | kind | fit | rec | milestone | effort |
|----|-------|------|-----|-----|-----------|--------|
| A-exceptions | `try`/`catch`/`finally`/`throw` + exception types | port | strong | adopt | M3 (error slice 2) | L |
| A-result-type | `Result<T,E>` / typed-throws alternative to exceptions | new | ok | defer | M3 (error slice 2) | L |
| A-abstract | `abstract` classes & methods | port | strong | adopt | M-RT S6 | M |
| A-lsb | Late static binding (`static::`, `new static`) | port | ok | adopt | M-RT S6 | M |
| A-class-const | Class constants (+ typed, + interface constants, `final`) | port | strong | adopt | M-RT | M |
| A-magic-stringable | `__toString` (Stringable) | port | strong | adopt | M-RT | S |
| A-magic-invoke | `__invoke` (callable objects) | port | strong | adopt | M-RT | S |
| A-magic-clone | `__clone` hook for `clone`/`with` | port | ok | adopt | M-mut follow-up | S |
| A-magic-dynamic | `__get`/`__set`/`__isset`/`__unset`/`__call`/`__callStatic` | omit | weak | reject | — | M |
| A-override-attr | `#[\Override]` correctness marker | port | strong | adopt | M-RT S6 | S |
| A-readonly | `readonly` properties & `readonly` classes | map | ok | adopt | M-RT | S |
| A-asym-vis | Asymmetric member visibility `private(set)`/`protected(set)` | port | ok | adopt | M-RT | M |
| A-named-args | Named arguments `f(name: val)` | port | strong | adopt | M3 | M |
| A-default-args | Default parameter values | port | strong | adopt | M3 | S |
| A-variadics | Variadics `...$xs` + argument unpacking/spread `...` | port | strong | adopt | M3 | M |
| A-named-tuples | `list()` / array destructuring (`[$a,$b] = …`) | port | ok | adopt | M3 | M |
| A-heredoc | Heredoc / nowdoc multi-line strings | map | ok | adopt | M3 | S |
| A-strict-types | `declare(strict_types=1)` semantics | map | strong | map | — (already strict) | S |
| A-backed-enums | Backed enums (`enum E: int`) + `from`/`tryFrom`/`cases` | port | strong | adopt | M-RT | M |
| A-enum-methods | Enum methods + enum-implements-interface + enum constants | port | strong | adopt | M-RT | M |
| A-iterators | `Iterator`/`IteratorAggregate`/`foreach` over objects | port | ok | adopt | M11 / M-RT | M |
| A-arrayaccess | `ArrayAccess` / `Countable` / `Stringable` SPL interfaces | port | ok | adopt | M11 | M |
| A-generators | Generators / `yield` / `yield from` (lazy iteration) | port | ok | defer | M6 | L |
| A-fibers | Fibers (stackful coroutines) | omit | weak | reject | — (M6 spawn supersedes) | L |
| A-references | Reference parameters & assignment `&$x` | omit | weak | reject | — | M |
| A-attributes | User attributes `#[Attr]` + reflection read | port | ok | defer | post-M-RT | L |
| A-json-validate | `json_validate()` + `core.json` parse/encode | port | strong | adopt | M11 | M |
| A-spl-ds | SPL data structures (`SplStack`, `SplQueue`, `SplObjectStorage`, heaps) | map | weak | defer | M11 | M |
| A-streams | Stream wrappers / resources (`fopen`, filters) | omit | weak | reject | — (M6 IO instead) | L |
| A-goto | `goto` / labeled break | omit | weak | reject | — | S |
| A-cast-ops | Type-cast operators `(int)`/`(string)` + `settype` | map | ok | map | — (explicit conv fns) | S |
| A-nullsafe-chain-call | Nullsafe method-chain on optionals everywhere | map | ok | map | — (`?.` shipped) | S |
| A-const-expr | `const` top-level constants + constant expressions | port | strong | adopt | M-RT / M11 | S |
| A-final-default | `final` keyword (class/method) — final-by-default inversion | map | strong | map | M-RT S6 | S |

## Rationale per ADOPT item

**A-exceptions — `try`/`catch`/`finally`/`throw`.** This is the single largest PHP-parity hole and the
one PHP devs will notice first. It is already on the roadmap (error-handling slice 2). The philosophy
fit is strong *provided* it lands as a typed, checked exception surface (catch clauses typed by class,
exceptions are ordinary classes) lowering 1:1 to PHP `try`/`catch`/`finally`/`throw`. The hard design
question (resolved in the captured slice-2 design work) is the interplay with the byte-identity spine on
the *fault* path; that's tractable because faults already render identically on all three backends.
Recommend adopt as the next post-overloading control-flow milestone.

**A-abstract — `abstract` classes & methods.** A core OO building block PHP devs use constantly and a
direct prerequisite for a clean `extends` story (M-RT S6). Maps 1:1 to PHP `abstract`. Checker enforces
"cannot instantiate abstract", "must implement abstract methods"; transpiler emits `abstract`. Strong fit.

**A-lsb — Late static binding.** `static::` and `new static` are idiomatic in PHP factory/active-record
patterns. Once `extends` lands, resolving `static::` to the runtime class is expected behavior; without
it inherited factories silently return the wrong class. Maps directly to PHP LSB. Sequence with S6.

**A-class-const — Class & interface constants.** PHP class constants (`const FOO = …`), now *typed*
(PHP 8.3) and `final` (PHP 8.1), plus interface constants. Phorj has top-level `var`/immutability but
no per-class named constant surface; PHP devs reach for `self::FOO` constantly. Erase-friendly: a typed
const checks like an immutable field and emits PHP `const`. Strong fit.

**A-magic-stringable / A-magic-invoke / A-magic-clone.** The *static, type-checkable* magic methods.
`__toString` (and the `Stringable` interface) lets a class flow into string interpolation — a very
common, fully checkable contract; `__invoke` makes an object callable (maps to Phorj's first-class
function story + PHP `__invoke`); `__clone` is the customization hook for the already-shipped
`obj with { … }` / future `clone`. All three are statically dispatchable, map 1:1 to PHP, and remove a
real surprise ("why can't I print my object?"). Adopt. (Contrast with the *dynamic* magic family below.)

**A-override-attr — `#[\Override]`.** PHP 8.3's correctness marker: the compiler verifies the method
actually overrides a parent method, catching rename drift. Pure safety win, zero runtime surprise,
trivial to check once `extends` exists, emits `#[\Override]`. Exactly the "provably safer" sweet spot.

**A-readonly — `readonly` properties & classes.** Phorj is immutable-by-default, so this largely
*maps* to existing semantics, but PHP devs expect the keyword and the transpiler should emit PHP
`readonly` for immutable fields/classes (currently fields emit plain `public`, per KNOWN_ISSUES). Low
effort, improves the round-tripped PHP's fidelity and self-documentation. Adopt as a transpile-emission
refinement.

**A-asym-vis — Asymmetric member visibility `private(set)`.** PHP 8.4. The read-public/write-restricted
property is a clean, sound pattern that fits "provably safer." Phorj's immutable-by-default already
covers the common case; `mutable` + `private(set)` covers the "mutable internally, read-only externally"
case. Maps to PHP 8.4 `public private(set)`. Adopt (member-visibility axis is already noted deferred in
KNOWN_ISSUES).

**A-named-args / A-default-args.** Named arguments (`f(timeout: 5)`) and default parameter values are
everyday PHP ergonomics that improve call-site legibility and map 1:1 to PHP. Default args are nearly
free; named args need call-site checking against the signature. Both strongly familiar, both lower
directly. Adopt together as a call-convention slice.

**A-variadics — `...$xs` + spread `...`.** Variadic parameters and argument unpacking are pervasive in
PHP. Type as `List<T>` of trailing args; spread an existing `List<T>` into a call. Maps to PHP `...$xs`
and `...$array`. Strong fit, high familiarity.

**A-named-tuples — `list()` / destructuring.** `[$a, $b] = pair()` and `list($a,$b)=…` are common PHP
idioms. Phorj has no tuple type yet (tuples are listed 🚧). Destructuring of a fixed-arity list/tuple
maps to PHP array destructuring. Adopt alongside a small tuple type.

**A-heredoc — heredoc/nowdoc.** Phorj strings are already multi-line (per memory), so this largely
*maps*, but the `<<<EOT` / `<<<'EOT'` syntax (interpolating vs literal) is a familiar PHP affordance for
templated text/SQL/HTML. Emit as a normal interpolated/raw PHP string. Low effort, legibility win.

**A-backed-enums — `enum E: int { case A = 1; }` + `from`/`tryFrom`/`cases`.** Phorj has payload enums
but not *backed* enums (scalar-valued cases) nor the `from`/`tryFrom`/`cases()` API — a heavily used PHP
8.1 feature for mapping DB/JSON values to enums. Backed enums map directly to PHP backed enums; the
three methods erase to the PHP enum API. Strong fit, high demand.

**A-enum-methods — enum methods + `implements` + enum constants.** PHP enums can declare methods,
implement interfaces, and hold constants. Phorj enums currently can't. Pure additive OO-on-enums,
1:1 PHP mapping. Strong fit; pairs naturally with backed enums.

**A-iterators / A-arrayaccess — `Iterator`/`IteratorAggregate`/`ArrayAccess`/`Countable`/`Stringable`.**
The SPL contract interfaces that make a user object usable with `foreach`, `count()`, `[]`, and string
context. These are *interfaces* (Phorj already has interfaces) with checkable method contracts that map
1:1 to PHP. They unlock user-defined collections — a real capability PHP devs expect. Adopt (the
container-iteration ones pair with M11 stdlib breadth; `Stringable` with A-magic-stringable).

**A-json-validate — `json_validate()` + `core.json`.** A real `Core.Json` (parse/encode) is already a
known stdlib gap (deferred to M11, needs a dynamic `Json`/`Any` type). PHP 8.3's `json_validate()` is a
small, expected addition once the JSON surface exists. Maps to PHP `json_decode`/`json_encode`/
`json_validate`. Adopt within the M11 stdlib milestone.

**A-const-expr — top-level `const` + constant expressions.** Named compile-time constants (`const PI =
3.14159;`) and constant expressions in default values / array keys. Maps to PHP `const`/`define`. Small,
familiar, legibility win. Adopt with the class-const work.

---

### Map / defer / reject notes (non-adopt, for completeness)

- **A-strict-types (map):** Phorj is *always* strictly typed with no coercion — `declare(strict_types=1)`
  is the floor, not an option. The transpiler should emit `declare(strict_types=1)` in generated PHP so the
  round-trip matches Phorj semantics; no language surface needed.
- **A-final-default (map):** the developer has chosen final-by-default (S6); PHP's `final` keyword inverts
  to an `open`/`extends`-permitted opt-in. Map the concept, don't import the keyword as-is.
- **A-cast-ops / A-nullsafe-chain-call (map):** explicit conversion functions and the shipped `?.`/`??`
  already cover these; importing PHP's `(int)` cast operators would *add* coercion surprise — express as
  named conversion functions instead.
- **A-magic-dynamic (reject):** `__get`/`__set`/`__call`/`__callStatic` defeat static checking — the exact
  dynamic-dispatch surprise Phorj exists to remove. Capability is recovered via explicit interfaces /
  generics, not magic interception.
- **A-references (reject):** `&$x` reference params/assignment contradict immutable-by-default + the
  value/handle split (M-mut). Mutation is already expressed via mutable handles; references add aliasing
  surprise with no new capability.
- **A-fibers / A-streams (reject):** superseded by Phorj's planned model — uncolored `spawn` + channels
  (M6) replace fibers as the concurrency surface; structured IO (M6) replaces raw stream resources.
  Importing them would duplicate capability with a lower-level, more surprising API.
- **A-generators (defer):** `yield`/`yield from` lazy iteration is valuable but interacts with the VM call
  model and the byte-identity spine; align with M6 concurrency (green-thread frames already reify the call
  stack). Defer, don't reject.
- **A-attributes (defer):** user `#[Attr]` + reflection is a real PHP capability but needs a reflection
  story; defer past M-RT. (`#[\Override]` is adopted separately as a built-in marker, not user attributes.)
- **A-spl-ds (defer):** concrete SPL data structures map onto the generic `List`/`Map`/`Set` + iterator
  interfaces once those land; provide as stdlib types in M11 rather than as language features.
- **A-result-type (defer):** a `Result<T,E>` / typed-throws alternative is worth designing alongside
  exceptions (it's the more provably-correct option), but it competes with `try`/`catch` for the same slot;
  decide in the error-model design rather than shipping both blindly.
- **A-goto (reject):** unstructured control flow, pure surprise, no PHP dev reaches for it; labeled
  `break N` covers the legitimate cases and can be a tiny separate adopt if demanded.

## Critic pass

### Mis-listings (already shipped — none to remove)

Verified every listed item against `FEATURES.md`, `KNOWN_ISSUES.md`, the project `CLAUDE.md` milestone
log, and `src/` greps. **Zero mis-listings** — no listed gap is already shipped. Two clarifications that
do *not* change the verdicts: (a) the first researcher's `A-goto` rationale mentions "labeled `break N`";
plain `break;`/`continue;` already **shipped** (M-mut.3, `src/ast.rs` `Stmt::Break/Continue`), so only the
*labeled* `break N`/`continue N` form is a gap — folded into the new `A-labeled-loop` row. (b) `while`,
`do…while`, and C-style `for(;;)` **all shipped** (M-mut.3, `Stmt::While`/`Stmt::CFor`) — confirming the
researcher correctly did **not** list them as gaps.

### Newly-found gaps (long-tail PHP-parity items the first pass missed)

The first pass covered OO/type-system and call-convention surface thoroughly but **under-covered the
stdlib/string-formatting surface and a few operator/literal affordances** PHP devs use daily.

| id | title | kind | fit | rec | milestone | effort |
|----|-------|------|-----|-----|-----------|--------|
| A-sprintf | `sprintf`/`printf`/`number_format` formatted output | port | strong | adopt | M11 | M |
| A-print-nonewline | `print`/`Console.print` (no-newline) + `Console.eprintln` (stderr) | port | strong | adopt | M11 | S |
| A-corelist-breadth | `Core.List` breadth (`sort`/`indexOf`/`contains`/`slice`/`concat`/`first`/`last`/`flatMap`) | port | strong | adopt | M11 | M |
| A-numeric-sep | Numeric literal separators `1_000_000` / `0xFF_FF` | port | strong | adopt | M3 | S |
| A-spaceship | Spaceship `<=>` (three-way compare) + `sort`-by-comparator | port | ok | adopt | M11 | S |
| A-ternary-elvis | Ternary `c ? a : b` + Elvis `a ?: b` | map | ok | defer | — (expr-`if`/`??` cover) | S |
| A-switch | `switch`/`case`/`default` statement | map | strong | reject | — (`match` covers) | S |
| A-isset-empty | `isset()`/`empty()`/`unset()` dynamic predicates | map | weak | reject | — (`?`/`??`/`if-let` cover) | S |
| A-destruct | `__destruct` destructor | omit | weak | reject | — (Rc/Drop, no det. GC) | M |
| A-anon-class | Anonymous classes `new class { … }` | omit | ok | defer | post-M-RT | M |
| A-new-in-init | `new` in default-arg/const initializers | port | ok | defer | with A-default-args | S |
| A-func-static | Function-`static` locals + `global` keyword | omit | weak | reject | — (closures/params cover) | S |
| A-sized-int | Sized integers (`int8`…`int64`/`uint*`) + `decimal` | new | ok | defer | post-GA / v2 | L |
| A-compact-extract | `compact()`/`extract()`/variable-variables `$$x` | omit | weak | reject | — (dynamic, defeats checking) | S |
| A-labeled-loop | Labeled `break N`/`continue N` (multi-level) | port | ok | defer | M3 (control-flow) | S |
| A-printf-debug | `var_dump`/`print_r`-style structured dump (`Console.debug`/`inspect`) | new | ok | adopt | M11 | S |

**Newly-found rationale (adopt/defer items only):**

- **A-sprintf — formatted output.** The single biggest *stdlib* parity hole the first pass missed.
  `Console` ships only `println` (`src/native.rs`); there is **no `sprintf`/`printf`/`number_format`**.
  PHP devs format strings constantly (padding, precision, currency). A typed `Core.Text.format(fmt,
  args…)` is the legible form — but the format-string is dynamic, so the type-safe answer is a
  *checked* subset (`{}`-style positional, or PHP `%`-specifiers validated against the arg list). Maps
  to PHP `sprintf`/`number_format`. Strong fit, real daily need; the variadic-args slice (A-variadics)
  is a soft prerequisite for the ergonomic form. Adopt in the M11 stdlib milestone.
- **A-print-nonewline / A-printf-debug — `print` + `debug`.** `Console.println` always appends `\n`;
  there is no no-newline `print`, no `eprintln` (stderr), and no structured-value dump. All three are
  small additive natives (erase to PHP `print`/`fwrite(STDERR,…)`/`var_export`). `Console.debug` is a
  *new* beyond-PHP nicety (a guaranteed-deterministic structured render usable in the byte-identity
  spine) — adopt-able because it lowers to a pinned PHP `var_export`-equivalent. Low effort, real
  ergonomics.
- **A-corelist-breadth — list stdlib breadth.** `Core.List` ships `reverse`/`sum`/`map`/`filter`/
  `reduce` (S7b); PHP devs also reach for `sort`/`usort`/`in_array`/`array_search`/`array_slice`/
  `array_merge`/`array_column`. These are exactly the same higher-order-native + generic path already
  proven in S7b-3 — purely additive, no new `Op`/`Value`. The first pass mentioned `A-spl-ds` (defer)
  but never the everyday `array_*` breadth. Adopt within M11. (`sort` pairs with **A-spaceship**.)
- **A-numeric-sep — `1_000_000`.** PHP 7.4 numeric-literal separators. A pure lexer affordance, zero
  semantic surprise, big legibility win for money/byte constants, erases trivially (PHP supports the
  exact same syntax). Cheap adopt; fold into the M3 ergonomics surface.
- **A-spaceship — `<=>`.** The three-way comparison operator, the idiomatic PHP basis for custom sorts.
  Returns `int` (-1/0/1); pairs with a comparator-taking `Core.List.sort`. Maps 1:1 to PHP `<=>`.
  Adopt with A-corelist-breadth.
- **A-ternary-elvis (map → defer).** PHP's `c ? a : b` and `a ?: b`. Phorj's **expression-`if`**
  (shipped) covers the full ternary and **`??`** covers the null-Elvis; the *value-truthy* Elvis
  (`a ?: b` where `a` is falsy-but-non-null) has no Phorj analogue **by design** (no truthiness
  coercion — the exact surprise Phorj removes). So the concept *maps*; the only open question is
  whether to add `?:` as pure sugar for expression-`if` for familiarity. Defer (cosmetic; expr-`if`
  already reads clearly).
- **A-anon-class / A-new-in-init (defer).** Anonymous classes (`new class implements I { … }`) are a
  real PHP-8 capability (one-off implementers); they need a name-mangling + capture story, defer past
  M-RT. `new` in initializers becomes relevant the moment default arguments land (PHP 8.1 allows
  `function f(Logger $l = new NullLogger())`) — sequence it *with* A-default-args.
- **A-sized-int — sized integers / `decimal` (defer).** Already flagged deferred in `KNOWN_ISSUES`
  ("Sized integers / `decimal`"), but the first pass omitted it from the gap table. PHP itself has only
  one `int` + `GMP`/`bcmath` extensions, so this is **beyond-PHP** (a provably-safer money/overflow
  story). It breaks the single-`int` model and the byte-identity float story → genuinely post-GA / v2.
  Surfacing it so it isn't re-discovered ad hoc.
- **A-labeled-loop — `break N`/`continue N` (defer).** Plain `break`/`continue` shipped; PHP's
  *multi-level* `break 2;` is the legitimate slice of `goto` (which is rejected). Low-surprise, maps
  1:1. Defer to a future control-flow slice — rarely needed, but the honest home for the "labeled break"
  the first pass mentioned under `A-goto`.

**Newly-found rejects (for completeness):**

- **A-switch (reject — maps to `match`).** `switch`/`case` is fall-through-prone (the classic
  forgotten-`break` bug) — the exact surprise Phorj removes. `match` (shipped, exhaustive, no
  fall-through) is the legible replacement. Reject the C-style statement.
- **A-isset-empty (reject — maps to optionals).** `isset`/`empty` are dynamic existence/truthiness
  predicates; Phorj's `T?` + `??` + `if (var x = opt)` + `opt!` express the *checkable* subset and
  reject the truthiness-coercion footgun (`empty("0")` is `true` in PHP — pure surprise).
- **A-destruct (reject).** `__destruct` fires on refcount-zero / scope-exit; Phorj's `Rc`/`Drop` model
  has **no deterministic finalization guarantee** (cycles leak until exit, per KNOWN_ISSUES), so a
  destructor contract would be a promise the runtime can't keep — a correctness surprise. Resource
  cleanup belongs to the structured-IO model (M6), not object finalizers.
- **A-func-static / A-compact-extract (reject).** Function-`static` locals + `global` are hidden
  mutable state (closures/params/handles cover the legitimate cases); `compact`/`extract`/`$$x`
  variable-variables defeat static name resolution entirely — the canonical dynamic-PHP footguns
  Phorj exists to remove.

**Sanity-check on the first pass's verdicts (philosophy lens):** all adopt/map/reject calls survive
scrutiny. One nuance worth recording: **A-class-const** and **A-const-expr** overlap — class constants
are the per-class case of named compile-time constants; build them as one slice (constant-expression
evaluator shared by both). And **A-strict-types** + **A-final-default** are correctly *maps* (a
transpile-emission refinement and a design inversion), not features — keeping them as `map` rather than
`port` is the philosophy-correct call.
