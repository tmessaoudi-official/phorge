# PHP Feature Inventory — PHP 8.0 → 8.6 (for the PHP → Phorj importer)

> Coverage analysis for designing a **PHP → Phorj importer**. Phorj is statically typed,
> immutable/acyclic-heap, no associative `Map` primitive, no `eval`, no variable-variables.
> The goal is the full syntactic + semantic surface, with **explicit flagging of every
> feature a static, immutable, no-Map language CANNOT accept**.
>
> **Verification (bleeding edge):** 8.4 / 8.5 / 8.6 verified by web search against
> authoritative sources on 2026-06-18:
> - PHP 8.5 release page — https://www.php.net/releases/8.5/en.php (shipped 2025-11-20)
> - PHP 8.4 release page — https://www.php.net/releases/8.4/en.php (shipped 2024-11-21)
> - PHP 8.6 RFC list — https://php.watch/versions/8.6/rfcs (in development)
> - PHP Foundation, Partial Function Application — https://thephp.foundation/blog/2025/12/08/partial-application/
> - PHP changes cheatsheet (8.0–8.5 cross-check) — https://eusonlito.github.io/php-changes-cheatsheet/features.html
> - PHP 8.0/8.1/8.2/8.3 — author knowledge, cross-checked against the cheatsheet and php.watch.
>
> **Legend (Type-system relevance column):**
> `core` = central to a static type checker · `aux` = type-adjacent · `none` = no type impact ·
> `IMPORTANT` = a true static-typing decision point · `UN-IMPORTABLE` = dynamic feature a
> static/immutable/no-Map language cannot map (see dedicated section at the bottom).
>
> **Sugar vs runtime semantics** is called out per row at the end of the semantics text:
> `[SUGAR]` = pure syntactic sugar (desugars to existing constructs, no new runtime behaviour) ·
> `[RUNTIME]` = requires real runtime/semantic support.

---

## (1) Type system

| Feature | Version | Category | One-line semantics | Type-system relevance |
|---|---|---|---|---|
| Union types `A\|B` | 8.0 | Type system | A value may be one of several declared types. `[RUNTIME]` (runtime type check) | core — sum-type-like; Phorj needs union or rejection |
| `mixed` type | 8.0 | Type system | Explicit "any type" top type for params/returns/props. `[RUNTIME]` | IMPORTANT — maps to a top/`Any` type Phorj may not have |
| `static` return type | 8.0 | Type system | Return type "the late-static-bound class". `[RUNTIME]` | core — depends on late static binding |
| `false` pseudo-type (in unions) | 8.0 | Type system | `false` usable inside a union (e.g. `string\|false`). `[RUNTIME]` | aux — literal type in a union |
| `null` in union (nullable) `?T` / `T\|null` | 8.0 (pre-existing, formalised) | Type system | Value may be the declared type or null. `[RUNTIME]` | core — Phorj optionals `T?` map here |
| `never` return type | 8.1 | Type system | Function never returns (exits, throws, or loops forever). `[RUNTIME]` (bottom type) | core — bottom type; affects exhaustiveness |
| Intersection types `A&B` | 8.1 | Type system | Value must satisfy all listed (class/interface) types simultaneously. `[RUNTIME]` | core — Phorj has no intersection → likely UN-IMPORTABLE or approximated |
| Pure intersection of interfaces only | 8.1 | Type system | Intersection restricted to class/interface names (no scalars). `[RUNTIME]` | core |
| `readonly` properties | 8.1 | Type system | Property assignable once (in declaring scope), then immutable. `[RUNTIME]` | IMPORTANT — aligns with Phorj immutability |
| `enum` (pure) | 8.1 | Type system / OOP | Named finite set of singleton cases; a first-class type. `[RUNTIME]` | core — maps to Phorj enums |
| `enum` backed (int/string) | 8.1 | Type system / OOP | Enum cases carry a scalar backing value; `from()`/`tryFrom()`. `[RUNTIME]` | core — maps to Phorj backed-ish enums |
| `new` in initializers | 8.1 | Type system | Object instances allowed as param/prop/const default values. `[RUNTIME]` | aux — affects default-value evaluation |
| `readonly` classes | 8.2 | Type system / OOP | All properties of the class are implicitly `readonly`. `[RUNTIME]` | IMPORTANT — whole-type immutability |
| Standalone `null` type | 8.2 | Type system | `null` usable as a sole type declaration. `[RUNTIME]` | aux — unit/null type |
| Standalone `false` type | 8.2 | Type system | `false` usable as a sole type declaration. `[RUNTIME]` | aux — singleton literal type |
| Standalone `true` type | 8.2 | Type system | `true` usable as a sole type declaration. `[RUNTIME]` | aux — singleton literal type |
| DNF types `(A&B)\|C` | 8.2 | Type system | Disjunctive-normal-form combination of intersection + union. `[RUNTIME]` | core — Phorj lacks intersection → mostly UN-IMPORTABLE |
| Constants in traits | 8.2 | OOP / Type system | Traits may declare constants. `[RUNTIME]` | aux |
| Typed class constants | 8.3 | Type system | `const T NAME = …;` — class/interface/trait/enum constants carry a declared type. `[RUNTIME]` | core — typed constants map cleanly |
| Generics | (none, all versions) | Type system | **PHP has NO generics.** Only docblock `@template` (erased, comment-only). | IMPORTANT — Phorj generics are compile-time-only & PHP-absent (erased on transpile) |
| Type juggling / weak typing | all | Type system | Implicit scalar coercion (`"1" + 1`), `==` loose equality. `[RUNTIME]` | IMPORTANT — Phorj is strict; loose coercion is a semantic gap |
| `declare(strict_types=1)` | 7.0 (pre-8) | Type system | Per-file opt-in to strict scalar type enforcement. `[RUNTIME]` | IMPORTANT — only strict-typed PHP is cleanly importable |

---

## (2) Classes / OOP

| Feature | Version | Category | One-line semantics | Type-system relevance |
|---|---|---|---|---|
| Constructor property promotion | 8.0 | OOP | Declare + assign a property directly in the constructor signature. `[SUGAR]` (desugars to field + assignment) | core — Phorj already supports this |
| Late static binding (`static::`) | 5.3 (pre-8, ubiquitous) | OOP | `static::` resolves to the runtime (not declaring) class. `[RUNTIME]` | core — needed for `static` return type |
| Nullsafe method/prop access `?->` | 8.0 | OOP / control flow | Short-circuits the chain to `null` if the receiver is null. `[RUNTIME]` | core — Phorj `?.` maps directly |
| `::class` on objects | 8.0 | OOP | `$obj::class` yields the FQCN string. `[RUNTIME]` | aux |
| `enum` methods / interfaces / constants | 8.1 | OOP | Enums may implement interfaces, have methods & constants. `[RUNTIME]` | core |
| First-class callable syntax `f(...)` | 8.1 | OOP / functions | `strlen(...)`, `$obj->m(...)`, `C::m(...)` → a `Closure`. `[RUNTIME]` (produces a Closure value) | core — needs first-class function values |
| `final` class constants | 8.1 | OOP | Constants cannot be overridden in subclasses. `[RUNTIME]` | aux |
| `readonly` classes | 8.2 | OOP | (see Type system) all props immutable. `[RUNTIME]` | IMPORTANT |
| `#[\AllowDynamicProperties]` | 8.2 | OOP | Opt back into dynamic property creation (default now forbidden). `[RUNTIME]` | IMPORTANT — dynamic props are otherwise UN-IMPORTABLE |
| Deprecate dynamic properties (default) | 8.2 | OOP | Undeclared property writes are deprecated unless opted in. `[RUNTIME]` | IMPORTANT — pushes PHP toward a fixed shape (importable) |
| `#[\Override]` attribute | 8.3 | OOP | Compile-time assertion that a method overrides a parent. `[RUNTIME]` (engine check) | aux — checked-only, erasable |
| Readonly amendments (clone) | 8.3 | OOP | `readonly` props may be reinitialised during `__clone`. `[RUNTIME]` | aux |
| Property hooks (`get`/`set`) | 8.4 | OOP | Computed properties: inline `get`/`set` accessor bodies on a property. `[RUNTIME]` (accessor invocation) | IMPORTANT — accessor methods behind a field; needs hook lowering |
| Asymmetric visibility `public private(set)` | 8.4 | OOP | Read scope and write scope declared independently. `[RUNTIME]` | IMPORTANT — read/write visibility split |
| Lazy objects | 8.4 | OOP | Object created as a proxy; real init deferred to first access. `[RUNTIME]` (proxy + reflection) | UN-IMPORTABLE — proxy/reflection magic, no static analogue |
| `new C()->method()` without parens | 8.4 | OOP | Chain off a `new` expression without wrapping parentheses. `[SUGAR]` | none — parser sugar |
| `final` in constructor promotion | 8.5 | OOP | A promoted property may be declared `final`. `[RUNTIME]` | aux |
| Static property asymmetric visibility | 8.5 | OOP | Asymmetric read/write visibility on static properties. `[RUNTIME]` | aux |
| Clone-with `clone($o, [p => v])` | 8.5 | OOP | Clone an object while overriding named properties (with-er pattern for `readonly`). `[RUNTIME]` | IMPORTANT — functional immutable update; maps to Phorj "copy-with" |
| Abstract methods in traits | 8.0 | OOP | Traits may declare abstract methods with signatures enforced on the user. `[RUNTIME]` | aux |
| Interfaces with constants typed | 8.3 | OOP | (see typed class constants) | core |

---

## (3) Attributes / metadata

| Feature | Version | Category | One-line semantics | Type-system relevance |
|---|---|---|---|---|
| `#[Attribute]` native attributes | 8.0 | Attributes | Structured, reflectable metadata replacing docblock annotations. `[RUNTIME]` (read via Reflection) | aux — metadata; importable as erased annotations |
| `#[\SensitiveParameter]` | 8.2 | Attributes | Redacts a parameter's value from stack traces. `[RUNTIME]` | none — diagnostics only, erasable |
| `#[\ReturnTypeWillChange]` | 8.1 | Attributes | Suppresses a tentative-return-type deprecation. `[RUNTIME]` | none — erasable |
| `#[\Override]` | 8.3 | Attributes | Asserts a method overrides a parent (compile error otherwise). `[RUNTIME]` | aux — checked, erasable |
| `#[\Deprecated(message, since)]` | 8.4 | Attributes | User-land deprecation of functions/methods/class-constants. `[RUNTIME]` (emits deprecation) | none — diagnostics, erasable |
| `#[\NoDiscard]` | 8.5 | Attributes | Warns if a function's return value is discarded; `(void)` cast suppresses. `[RUNTIME]` (engine warning) | aux — could map to a Phorj lint |
| Attributes on constants | 8.5 | Attributes | Class/global constants may carry attributes. `[RUNTIME]` | none — metadata |
| `#[\Override]` extended to properties | 8.5 | Attributes | `#[\Override]` now also applies to properties. `[RUNTIME]` | aux |
| `#[\Deprecated]` extended to traits/consts | 8.5 | Attributes | Deprecation attribute now applies to traits and constants. `[RUNTIME]` | none |
| `#[\DelayedTargetValidation]` | 8.5 | Attributes | Defers compile-time attribute-target validation. `[RUNTIME]` | none |
| `#[\Override]` for class constants | 8.6 (accepted) | Attributes | Override assertion extended to class constants. `[RUNTIME]` | aux |

---

## (4) Functions / calls

| Feature | Version | Category | One-line semantics | Type-system relevance |
|---|---|---|---|---|
| Named arguments `f(name: $v)` | 8.0 | Functions | Pass arguments by parameter name, in any order. `[RUNTIME]` (call-site rebinding) | IMPORTANT — call-site name binding; needs parameter-name model |
| Variadics `...$args` | 5.6 (pre-8) | Functions | Collect trailing args into an array parameter. `[RUNTIME]` | core — list-collection |
| Argument unpacking `f(...$arr)` | 5.6 / 8.1 | Functions | Spread an array into positional args; 8.1 adds string-keyed (named) unpacking. `[RUNTIME]` | IMPORTANT — named-arg unpacking needs a string-key map (no-Map concern) |
| First-class callable `f(...)` | 8.1 | Functions | (see OOP) creates a `Closure` from any callable. `[RUNTIME]` | core |
| Arrow functions `fn() =>` | 7.4 (pre-8) | Functions | Single-expression closure with auto by-value capture. `[SUGAR]`-ish (auto-capture is `[RUNTIME]`) | core — Phorj Track A (S3) lambdas |
| Closures `function() use($x){}` | 5.3 (pre-8) | Functions | Anonymous function with explicit capture list (by-value or `&`-ref). `[RUNTIME]` | core (by-ref capture is UN-IMPORTABLE) |
| Trailing comma in param lists | 8.0 | Functions | Allow a trailing comma in function/method parameter lists. `[SUGAR]` | none |
| Trailing comma in `use` (closures) | 8.0 | Functions | Trailing comma allowed in closure `use()` lists. `[SUGAR]` | none |
| Partial function application `f(?, ...)` | 8.6 (accepted) | Functions | Placeholders (`?` single, `...` rest) create a `Closure` binding some args. `[RUNTIME]` (produces Closure) | core — needs first-class functions + currying-ish lowering |

---

## (5) Control flow / expressions

| Feature | Version | Category | One-line semantics | Type-system relevance |
|---|---|---|---|---|
| `match` expression | 8.0 | Control flow | Value-returning, strict-`===`, exhaustive-or-throws branch selector. `[RUNTIME]` | core — Phorj `match` maps directly |
| `throw` as expression | 8.0 | Control flow | `throw` usable in expression position (e.g. `?:`, arrow fn, `??`). `[RUNTIME]` | IMPORTANT — `throw` becomes an expression of bottom type `never` |
| Nullsafe operator `?->` | 8.0 | Control flow | Short-circuit member access on null receiver → null. `[RUNTIME]` | core — Phorj `?.` |
| Non-capturing catch `catch (E)` | 8.0 | Control flow | `catch` without binding a variable. `[SUGAR]` | none |
| `??` null-coalescing | 7.0 (pre-8) | Control flow | Left value unless null, else right. `[RUNTIME]` | core — Phorj `??` |
| `??=` null-coalescing assignment | 7.4 (pre-8) | Control flow | Assign right only if left is null/unset. `[RUNTIME]` | aux |
| Pipe operator `\|>` | 8.5 | Control flow / expressions | `$x \|> f \|> g` feeds the left value as the single arg to the right callable. `[SUGAR]` (lowers to nested calls) | core — Phorj Track A pipe `\|>` maps directly |
| `list()` / `[$a,$b]` destructuring | 7.1 (pre-8) | Control flow | Positional + keyed array destructuring assignment. `[RUNTIME]` | aux — keyed destructuring touches the no-Map concern |

---

## (6) Values / literals

| Feature | Version | Category | One-line semantics | Type-system relevance |
|---|---|---|---|---|
| Numeric literal separators `1_000` | 7.4 (pre-8) | Values | Underscores as digit-group separators in numeric literals. `[SUGAR]` | none |
| Enum cases as values | 8.1 | Values | Enum cases are first-class singleton values. `[RUNTIME]` | core |
| Backed-enum `->value` | 8.1 | Values | Read the scalar backing of a backed enum case. `[RUNTIME]` | core |
| Heredoc / Nowdoc (flexible, 7.3) | 7.3 (pre-8) | Values | Multi-line string literals; nowdoc is non-interpolating. `[RUNTIME]` (heredoc interpolates) | none |
| Octal explicit prefix `0o` | 8.1 | Values | `0o17` explicit-octal integer literal. `[SUGAR]` | none |
| `array_first()` / `array_last()` | 8.5 | Values / stdlib | First/last value of an array, or null if empty. `[RUNTIME]` | aux — list head/last |
| `array_is_list()` | 8.1 | Values / stdlib | True iff array is a 0-based contiguous list (vs associative map). `[RUNTIME]` | IMPORTANT — distinguishes List vs Map at runtime; key for the no-Map importer |
| `array_find` / `array_any` / `array_all` / `array_find_key` | 8.4 | Values / stdlib | Predicate search/quantifier helpers over arrays. `[RUNTIME]` | aux — map to list combinators (need S3 lambdas) |
| `clamp()` | 8.6 (implemented) | Values / stdlib | Constrain a numeric value to a [min,max] range. `[RUNTIME]` | none |
| `SortDirection` enum | 8.6 (implemented) | Values | Built-in enum for ascending/descending. `[RUNTIME]` | aux |
| `BackedEnum::values()` | 8.6 (under discussion) | Values | All backing values of a backed enum as an indexed array. `[RUNTIME]` | aux |

---

## (7) Error handling

| Feature | Version | Category | One-line semantics | Type-system relevance |
|---|---|---|---|---|
| `\Throwable` hierarchy (`\Error` vs `\Exception`) | 7.0 (pre-8, foundational) | Error handling | `\Throwable` is the root; `\Error` (engine) and `\Exception` (user) are the two trees. `[RUNTIME]` | IMPORTANT — Phorj faults are flat; exception subtyping needs a model |
| `throw` as expression | 8.0 | Error handling | (see control flow) `throw` in expression position. `[RUNTIME]` | IMPORTANT |
| Non-capturing catch | 8.0 | Error handling | `catch (E)` without `$e`. `[SUGAR]` | none |
| Stringable union `string\|\Stringable` | 8.0 | Error handling / type | `\Stringable` auto-implemented by any class with `__toString`. `[RUNTIME]` | aux |
| Fatal errors carry backtraces | 8.5 | Error handling | Timeout/fatal errors now include a stack trace. `[RUNTIME]` | none — diagnostics |
| `get_error_handler()` / `get_exception_handler()` | 8.5 | Error handling | Retrieve the currently-installed handlers. `[RUNTIME]` | none |
| Custom error/exception handlers (`set_error_handler`) | pre-8 | Error handling | Install global runtime handler callbacks. `[RUNTIME]` | UN-IMPORTABLE — global mutable handler state |

---

## (8) Dynamic / un-importable features (flagged)

> Every row here is **UN-IMPORTABLE** into a static, immutable, no-Map language. See the
> consolidated narrative in the next section — these are the hard rejections.

| Feature | Version | Category | One-line semantics | Type-system relevance |
|---|---|---|---|---|
| Variable variables `$$x` | pre-8 | Dynamic | Name a variable by the runtime value of another variable. `[RUNTIME]` | UN-IMPORTABLE — name resolved at runtime, no static binding |
| Variable property access `$o->$prop` | pre-8 | Dynamic | Property selected by a runtime string. `[RUNTIME]` | UN-IMPORTABLE — non-static field shape |
| Variable method call `$o->$m()` | pre-8 | Dynamic | Method selected by a runtime string. `[RUNTIME]` | UN-IMPORTABLE |
| Variable function call `$fn()` / `call_user_func` | pre-8 | Dynamic | Call a function named by a runtime string. `[RUNTIME]` | UN-IMPORTABLE (string-named; first-class `Closure` IS importable) |
| Dynamic class instantiation `new $cls` | pre-8 | Dynamic | Class chosen by a runtime string. `[RUNTIME]` | UN-IMPORTABLE |
| Dynamic constant fetch `constant($name)` / `$c::{$k}` | pre-8 / 8.3 | Dynamic | Constant chosen by a runtime string. `[RUNTIME]` | UN-IMPORTABLE |
| `eval()` | pre-8 | Dynamic | Execute a runtime-constructed string as PHP. `[RUNTIME]` | UN-IMPORTABLE — Phorj has no eval |
| Magic `__get` / `__set` / `__isset` / `__unset` | pre-8 | Dynamic | Intercept access to undeclared properties. `[RUNTIME]` | UN-IMPORTABLE — shape not statically known |
| Magic `__call` / `__callStatic` | pre-8 | Dynamic | Intercept calls to undefined methods. `[RUNTIME]` | UN-IMPORTABLE |
| Dynamic properties (un-opted) | pre-8.2 / opt-in via `#[\AllowDynamicProperties]` | Dynamic | Create properties not declared on the class. `[RUNTIME]` | UN-IMPORTABLE — open object shape |
| References `&$x` (alias / by-ref params / by-ref `foreach`) | pre-8 | Dynamic / mutation | Two names bound to the same storage cell; in-place mutation. `[RUNTIME]` | UN-IMPORTABLE — Phorj heap is immutable/acyclic; aliasing breaks it |
| By-reference closure capture `use (&$x)` | pre-8 | Dynamic / mutation | Closure mutates an enclosing variable. `[RUNTIME]` | UN-IMPORTABLE |
| `global` / `$GLOBALS` | pre-8 | Dynamic / global state | Import/mutate global mutable state inside a function. `[RUNTIME]` | UN-IMPORTABLE — no global mutable state |
| `static $x` function-local persistence | pre-8 | Dynamic / state | A local variable persisting (mutating) across calls. `[RUNTIME]` | UN-IMPORTABLE — hidden mutable state |
| `extract()` / `compact()` | pre-8 | Dynamic | Materialise variables from / into an array by key. `[RUNTIME]` | UN-IMPORTABLE — names from runtime keys |
| Associative arrays as maps/objects/everything | pre-8 | Dynamic / values | One `array` type doubles as list, map, struct, set. `[RUNTIME]` | UN-IMPORTABLE as Map — Phorj has List but no Map primitive; only the list-shaped subset (`array_is_list`) imports |
| Mixed-key arrays (int+string keys) | pre-8 | Dynamic / values | A single array freely mixes integer and string keys. `[RUNTIME]` | UN-IMPORTABLE — no static shape |
| `func_get_args()` / `func_num_args()` | pre-8 | Dynamic | Read all passed args at runtime (beyond the signature). `[RUNTIME]` | UN-IMPORTABLE — signature not authoritative |
| Loose equality `==` / juggling | pre-8 | Dynamic / semantics | Cross-type comparison with implicit coercion. `[RUNTIME]` | UN-IMPORTABLE (semantics differ) — only `===` maps cleanly |
| Variadic-as-array + named spread to assoc | 8.1 | Dynamic / values | Named-arg unpacking from a string-keyed array. `[RUNTIME]` | UN-IMPORTABLE — string-keyed map at the call site |
| Lazy objects (proxy/reflection) | 8.4 | Dynamic / OOP | Proxy object whose initialiser fires on first access. `[RUNTIME]` | UN-IMPORTABLE — reflection-driven proxy |
| Reflection API (`Reflection*`) | pre-8 (+ 8.4/8.5/8.6 additions) | Dynamic / introspection | Inspect/modify classes, props, methods at runtime. `[RUNTIME]` | UN-IMPORTABLE — runtime metaprogramming |
| `set_error_handler` / `set_exception_handler` | pre-8 | Dynamic / global | Install global runtime handlers. `[RUNTIME]` | UN-IMPORTABLE — global mutable handler state |
| Backtick shell-exec `` `cmd` `` (deprecated 8.5) | pre-8 | Dynamic / IO | Inline shell command execution. `[RUNTIME]` | UN-IMPORTABLE — and now deprecated upstream |
| Goto `goto label;` | pre-8 | Control flow | Unstructured jump. `[RUNTIME]` | UN-IMPORTABLE — no structured-control analogue |

---

## Un-importable dynamic features — narrative summary

The PHP → Phorj importer must **reject (or refuse to faithfully model)** the following classes
of feature. They violate one of Phorj's three load-bearing invariants — **static shape**,
**immutability/acyclic heap**, or **no associative-Map primitive** — or rely on capabilities
Phorj deliberately omits (`eval`, reflection, global mutable state).

1. **Runtime-name resolution** — `$$x`, `$o->$prop`, `$o->$m()`, `$fn()`, `new $cls`,
   `constant($name)`, `extract`/`compact`. Names are not knowable statically → no binding,
   no type. (First-class callables `f(...)` and `Closure` values ARE importable — they carry
   a static identity; only *string-named* dynamic dispatch is rejected.)
2. **Metaprogramming & code-as-data** — `eval()`, the entire Reflection API, magic methods
   (`__get`/`__set`/`__isset`/`__unset`/`__call`/`__callStatic`), lazy objects. The object
   shape and behaviour are not statically determinable.
3. **Mutation & aliasing** — references (`&$x`, by-ref params, by-ref `foreach`, `use (&$x)`),
   `static $x` locals, `global`/`$GLOBALS`. Phorj's heap is immutable + acyclic; aliasing and
   hidden persistent state have no representation.
4. **The associative-array-as-everything idiom** — PHP's single `array` is list + map + struct +
   set. Phorj has a typed `List` but **no Map primitive**. Only the list-shaped subset
   (`array_is_list($a) === true`) imports cleanly; string-keyed / mixed-key arrays, named-arg
   unpacking from assoc arrays, and `extract`/`compact` do not.
5. **Global mutable runtime state** — `set_error_handler`/`set_exception_handler`, superglobals,
   INI-driven behaviour switches.
6. **Weak typing semantics** — loose `==` and silent type juggling. Only `===`/`match`
   (strict) map to Phorj's strict equality; importing `==` would change program meaning.
7. **Unstructured control** — `goto`, backtick shell-exec (also deprecated in 8.5).

**Partially importable (shape-dependent):** constructor promotion, named args (need a
parameter-name model but are static), variadics, array destructuring (positional yes; keyed
touches the Map concern), arrow/closure functions (by-*value* capture importable; by-*ref*
capture is not).

---

## Sugar vs runtime semantics — quick index

**Pure syntactic sugar** (desugars to existing constructs, no new runtime behaviour):
constructor property promotion, `new C()->m()` without parens, trailing commas (params / `use`),
numeric separators `1_000`, octal `0o` prefix, non-capturing `catch`, the pipe `|>` operator
(lowers to nested calls), and arrow-function *syntax* (the auto-capture itself is runtime).

**Require real runtime/semantic support** (new engine behaviour): everything in the type-system
table (union/intersection/DNF/`never`/`readonly`/enums), property hooks, asymmetric visibility,
lazy objects, clone-with, named arguments (call-site rebinding), first-class callables &
partial application (Closure synthesis), `match` (strict + exhaustive throw), nullsafe `?->`,
`throw`-as-expression, `??`/`??=`, all attributes that emit warnings/deprecations
(`#[\NoDiscard]`, `#[\Deprecated]`), and every dynamic feature in section (8).

---

## Sources verified (bleeding edge: 8.4 / 8.5 / 8.6)

- PHP 8.5 release announcement — https://www.php.net/releases/8.5/en.php
- PHP 8.4 release announcement — https://www.php.net/releases/8.4/en.php
- PHP 8.6 RFC list (php.watch) — https://php.watch/versions/8.6/rfcs
- PHP Foundation, "PHP 8.6 kicks off with partial function application" — https://thephp.foundation/blog/2025/12/08/partial-application/
- PHP changes cheatsheet (8.0–8.5 cross-reference) — https://eusonlito.github.io/php-changes-cheatsheet/features.html
- PHP 8.1 release announcement — https://www.php.net/releases/8.1/en.php
- PHP.Watch enums (8.1) — https://php.watch/versions/8.1/enums

> Note: PHP 8.6 rows are **in-development** (RFC status as of 2026-06-18). `clamp()`,
> `SortDirection`, debugable enums, polling API are reported implemented; partial function
> application and `#[\Override]` for class constants are accepted; `BackedEnum::values()` is
> under discussion. Statuses can still change before 8.6 GA.
