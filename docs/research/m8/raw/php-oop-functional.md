# PHP OOP + Functional Model — Exhaustive Feature Map → Phorj

Research date: 2026-06-18. Scope: every **non-deprecated** OOP + functional feature from the PHP 5 era
through **PHP 8.6** (8.6 is in active dev / early alpha as of mid-2026). Deprecated/removed features are
noted but excluded from the verdict accounting.

## Legend

**Bucket**
- ✅ — Phorj has an equivalent **at least as capable** (≥).
- 🔶 — Phorj has a **partial** equivalent.
- 🔲 — **roadmapped** for a named milestone.
- ❌ — **reject-by-design** (incompatible with Phorj's static, immutable, transpile-to-PHP model).

**Verdict**
- BETTER — Phorj's form is stricter/safer/cleaner.
- SAME — semantically equivalent.
- SAME+syntax — equivalent meaning, different (usually nicer) surface.
- WORSE→reject — PHP's form would degrade Phorj's invariants; rejected.

**Phorj facts assumed (per CLAUDE.md / docs):** statically typed, immutable-by-default, VM + PHP
transpile, byte-identical `run`/`runvm` spine. HAS: classes, constructor promotion, instance methods,
field visibility (public/private), enums-with-payloads + exhaustive `match`, optionals `T?`, `this`.
NOT YET: inheritance, abstract, interface, traits, static members, late static binding, closures/lambdas/
arrow fns, first-class callables, generators, magic methods, overloading, property hooks, asymmetric
visibility, anonymous classes, Reflection.

---

## 1. Class System

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| `class` declaration | PHP 4/5 | `class` exists; value-native instances | ✅ | SAME |
| `abstract class` | PHP 5.0 | none yet → M3 S5 | 🔲 M3 S5 | SAME (when landed) |
| `interface` | PHP 5.0 | none yet → M3 S5 | 🔲 M3 S5 | SAME |
| Multiple interface impl (`implements A, B`) | PHP 5.0 | none yet → M3 S5 | 🔲 M3 S5 | SAME |
| Interface constants | PHP 5.0 | none yet; tie to S5 interfaces | 🔲 M3 S5 | SAME |
| Interface default methods | n/a | PHP has **no** interface default methods (that's traits) | ❌ | n/a — PHP doesn't have this |
| `final` class / method | PHP 5.0 | none yet; pairs with inheritance | 🔲 M3 S5 | SAME — but Phorj is immutable/closed-by-default, so `final` is the *default* posture (BETTER default) |
| `extends` (single inheritance) | PHP 5.0 | none yet → M3 S5 | 🔲 M3 S5 | SAME |
| `implements` | PHP 5.0 | none yet → M3 S5 | 🔲 M3 S5 | SAME |
| Class constants (`const`) | PHP 5.0 | none yet; design w/ S5 | 🔲 M3 S5 | SAME |
| Class constant **visibility** | PHP 7.1 | none yet | 🔲 M3 S5 | SAME |
| **Typed** class constants | PHP 8.3 | none yet; should land typed from day 1 | 🔲 M3 S5 | BETTER (typed-by-default fits Phorj) |
| Static properties | PHP 5.0 | none yet | 🔲 M3 S5 | SAME-or-reject (mutable static = global state; see note) |
| Static methods | PHP 5.0 | none yet | 🔲 M3 S5 | SAME |
| Late static binding (`static::`) | PHP 5.3 | none yet; needs inheritance + static dispatch | 🔲 M3 S5 | SAME (deferred behind inheritance) |
| `self::` | PHP 5.0 | none yet | 🔲 M3 S5 | SAME |
| `parent::` | PHP 5.0 | none yet (needs inheritance) | 🔲 M3 S5 | SAME |
| `instanceof` | PHP 5.0 | `match`/enum narrowing covers many cases; explicit `instanceof` → S5 | 🔶 → 🔲 M3 S5 | SAME+syntax — Phorj's exhaustive `match` over a sealed type is BETTER than open `instanceof` chains |
| `::class` (class-name constant) | PHP 5.5 | none yet; transpile-friendly | 🔲 M3 S5 | SAME |
| Namespaces (`namespace`) | PHP 5.3 | **DONE** — Go-style packages, mandatory `package`, `core.` reserved | ✅ | BETTER — mandatory packaging, "nothing in the wind", folder=path strictness |

**Note — static mutable state:** PHP static properties are shared mutable globals. In Phorj's
immutable-by-default model, *mutable* statics are a candidate for `❌ reject-by-design`; **static
constants / static pure methods** are fine. Recommend: allow static constants + associated functions,
reject mutable static properties (or gate them behind M3 mutation with explicit `mut`).

---

## 2. Properties

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| `public` / `private` visibility | PHP 5.0 | **HAS** public/private | ✅ | SAME |
| `protected` visibility | PHP 5.0 | none yet (needs inheritance to be meaningful) | 🔲 M3 S5 | SAME (deferred with inheritance) |
| Static properties | PHP 5.0 | none yet | 🔲 M3 S5 | see §1 note |
| `readonly` property | PHP 8.1 | **immutable-by-default** — every field is effectively readonly | ✅ | BETTER — Phorj makes the *exception* (mutable) opt-in instead of the *default*; PHP makes readonly opt-in |
| Typed properties | PHP 7.4 | **HAS** — all fields are statically typed | ✅ | BETTER — typed is mandatory, not opt-in; no `mixed`-by-omission |
| **Property hooks** (`get`/`set`) | PHP 8.4 | none; needs computed accessors | 🔲 (Track A / S3-adjacent) | SAME+syntax when landed — but Phorj being immutable makes `set` hooks less needed; `get` ≈ a zero-arg method |
| **Asymmetric visibility** (`public private(set)`) | PHP 8.4 | immutable fields make write-visibility moot | ✅ (subsumed) | BETTER — "public read, private write" is the *default* under immutability; no syntax needed |
| Constructor promotion | PHP 8.0 | **HAS** | ✅ | SAME |
| Default values | PHP 4/5 | supported via constructor / initializers | ✅ | SAME |
| Nullable property | PHP 7.1 (`?T`) | **HAS** via optionals `T?` + non-null discipline | ✅ | BETTER — non-optional `T` is *statically guaranteed* non-null (PHP `?T` is runtime-only; `T` can still be null via error paths) |
| Dynamic properties | PHP <8.2 | **deprecated in PHP 8.2** (removed PHP 9) | ❌ reject-by-design | WORSE→reject — needs runtime metaprogramming; statically typed Phorj forbids by construction |
| `#[AllowDynamicProperties]` | PHP 8.2 | escape hatch for above; n/a | ❌ | WORSE→reject — same reason |

---

## 3. Methods

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| Instance methods + visibility | PHP 5.0 | **HAS** instance methods + public/private | ✅ | SAME |
| Static methods | PHP 5.0 | none yet → S5 | 🔲 M3 S5 | SAME |
| `abstract` methods | PHP 5.0 | none yet → S5 | 🔲 M3 S5 | SAME |
| `final` methods | PHP 5.0 | none yet (final-by-default posture) | 🔲 M3 S5 | BETTER default |
| `__construct` | PHP 5.0 | **HAS** (with promotion) | ✅ | SAME |
| `__destruct` | PHP 5.0 | `Rc`/`Drop` reclaims deterministically; no user destructor | ❌ reject-by-design | WORSE→reject — user-visible destructors imply nondeterministic finalization; against the byte-identical spine |
| Return types | PHP 7.0 | **HAS** — mandatory static return types | ✅ | BETTER — mandatory, not opt-in |
| By-reference return (`&method`) | PHP 5.0 | none; immutability makes ref-return meaningless | ❌ reject-by-design | WORSE→reject — aliasing breaks value semantics |
| Variadic methods (`...$args`) | PHP 5.6 | none yet; pairs with closures/Track A | 🔲 Track A / S3 | SAME (when landed) |
| Named arguments | PHP 8.0 | none yet | 🔲 (front-end sugar, low cost) | SAME+syntax (cheap to add; transpiles to PHP named args) |

---

## 4. Magic Methods (ALL) — overwhelmingly reject-by-design

Magic methods require **runtime metaprogramming** / dynamic dispatch on undeclared members — fundamentally
incompatible with static typing + the immutable model. PHP `__sleep`/`__wakeup` are additionally
**soft-deprecated in PHP 8.5** (use `__serialize`/`__unserialize`).

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| `__get` | PHP 5.0 | dynamic read of undeclared prop | ❌ reject-by-design | WORSE→reject — runtime metaprogramming; defeats static field typing |
| `__set` | PHP 5.0 | dynamic write | ❌ reject-by-design | WORSE→reject — same + breaks immutability |
| `__isset` | PHP 5.1 | dynamic existence check | ❌ reject-by-design | WORSE→reject |
| `__unset` | PHP 5.1 | dynamic removal | ❌ reject-by-design | WORSE→reject |
| `__call` | PHP 5.0 | dynamic method dispatch | ❌ reject-by-design | WORSE→reject — no static signature |
| `__callStatic` | PHP 5.3 | dynamic static dispatch | ❌ reject-by-design | WORSE→reject |
| `__invoke` (callable object) | PHP 5.3 | covered better by closures/first-class fns | 🔶 → 🔲 Track A | SAME+syntax — Phorj closures (Track A) supersede the `__invoke` idiom |
| `__toString` | PHP 5.2 | a declared `to_string(): string` method / Stringable-style trait | 🔲 M3 S5 | SAME+syntax — make it an explicit interface method, not magic |
| `__clone` | PHP 5.0 | clone hook; immutability → clone-with instead | 🔲 (M3 mutation / clone-with) | SAME+syntax — see clone-with §12 |
| `__debugInfo` | PHP 5.6 | debug-repr customization | ❌ reject-by-design | WORSE→reject — runtime introspection hook |
| `__sleep` | PHP 4 (**deprecated 8.5**) | serialization hook | ❌ reject-by-design | WORSE→reject — also deprecated upstream |
| `__wakeup` | PHP 4 (**deprecated 8.5**) | deserialization hook | ❌ reject-by-design | WORSE→reject — also deprecated upstream |
| `__serialize` | PHP 7.4 | explicit serialize contract | ❌ reject-by-design (for now) | WORSE→reject — runtime serialization; revisit if a `Serialize` interface is ever specced |
| `__unserialize` | PHP 7.4 | explicit deserialize contract | ❌ reject-by-design (for now) | WORSE→reject — same |
| `__set_state` (used by `var_export`) | PHP 5.1 | export-rehydration hook | ❌ reject-by-design | WORSE→reject — runtime metaprogramming |

**Summary:** the *intent* behind a few magic methods is worth replacing with **explicit, statically-typed
constructs** — `__toString`→a `Stringable`-style interface method (S5); `__invoke`→closures (Track A);
`__clone`→clone-with (mutation milestone). The rest are pure runtime metaprogramming and rejected.

---

## 5. Traits

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| `trait` definition | PHP 5.4 | none yet → **M3 S5** (traits/mixins; MI rejected as MI) | 🔲 M3 S5 | SAME — locked as the multiple-inheritance answer |
| `use Trait;` | PHP 5.4 | none yet → S5 | 🔲 M3 S5 | SAME |
| Conflict resolution `insteadof` | PHP 5.4 | none yet → S5 | 🔲 M3 S5 | SAME |
| Aliasing `as` | PHP 5.4 | none yet → S5 (note: `as` already used for import aliasing) | 🔲 M3 S5 | SAME+syntax |
| Abstract trait methods | PHP 5.4 | none yet → S5 | 🔲 M3 S5 | SAME |
| Static trait properties | PHP 5.4 | static-mutable concerns (see §1 note) | 🔲 M3 S5 (constrained) | partial — likely allow only static consts |
| Trait constants | PHP 8.2 | none yet → S5 | 🔲 M3 S5 | SAME |

---

## 6. Interfaces & Built-in Interfaces

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| `Iterator` | PHP 5.0 | none yet; needs iteration protocol | 🔲 M3 S5 + iteration design | SAME |
| `IteratorAggregate` | PHP 5.0 | none yet | 🔲 M3 S5 | SAME |
| `ArrayAccess` (`$obj[$k]`) | PHP 5.0 | indexing `xs[i]` exists for lists; operator overload on objects → reject | 🔶 / ❌ | WORSE→reject for arbitrary objects (operator overload = runtime dispatch); list indexing is native ✅ |
| `Countable` (`count($obj)`) | PHP 5.1 | `core.list` length / a `len()` method | 🔶 → 🔲 | SAME+syntax — prefer an explicit `len()` method/native over a magic `count()` hook |
| `Stringable` | PHP 8.0 | explicit `to_string` interface | 🔲 M3 S5 | SAME+syntax |
| `Traversable` (base) | PHP 5.0 | iteration protocol marker | 🔲 M3 S5 | SAME |
| `JsonSerializable` | PHP 5.4 | needs dynamic `Json`/`Any` type (deferred w/ `core.json`) | 🔲 (generics / S3) | SAME (deferred) |
| `Throwable` | PHP 7.0 | exceptions → **M3** | 🔲 M3 exceptions | SAME |

---

## 7. Enums (PHP 8.1) — Phorj is already BETTER here

PHP enums are **pure** (no associated data) or **backed** by a single scalar (int/string). They can have
methods, implement interfaces, and define constants — but **cannot hold per-case payload data** (a known
PHP limitation; people emulate it with `match`-on-`$this` lookup methods).

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| Pure enum (`enum X { case A; }`) | PHP 8.1 | enums (subsumed: a zero-payload variant) | ✅ | SAME |
| Backed enum (`: int` / `: string`) | PHP 8.1 | a single-typed-payload variant covers it | ✅ | SAME — and Phorj generalizes to *any* payload shape |
| **Per-case associated data (payloads)** | **n/a in PHP** | **Phorj enums carry payloads** (`Value::Enum`) | ✅ | **BETTER — PHP enums literally cannot do this; Phorj can (sum-type / ADT model)** |
| Enum methods | PHP 8.1 | enum methods (P4a landed) | ✅ | SAME |
| Enum constants | PHP 8.1 | none yet; tie to S5 const work | 🔲 M3 S5 | SAME |
| Enum implementing interface | PHP 8.1 | needs interfaces → S5 | 🔲 M3 S5 | SAME |
| `cases()` | PHP 8.1 | none as a built-in yet; trivial to add | 🔲 (cheap) | SAME |
| `from()` (throws) | PHP 8.1 | maps to a `from(...) -> X` that faults | 🔲 (cheap) | SAME |
| `tryFrom()` (null) | PHP 8.1 | maps to `tryFrom(...) -> X?` (optionals!) | 🔲 (cheap) | **BETTER — return type is `X?` and the non-null discipline forces handling; PHP returns nullable but doesn't force the check** |
| **Exhaustive `match` over enum** | match exists in PHP 8.0 but **non-exhaustive** | **Phorj `match` is exhaustive + null-arm narrowing** | ✅ | **BETTER — PHP `match` throws `UnhandledMatchError` at *runtime*; Phorj enforces exhaustiveness at *compile time*** |

**Headline:** Phorj enums-with-payloads + compile-time-exhaustive `match` are a true **algebraic sum
type** — strictly more expressive and safer than PHP's scalar-backed enums + runtime-checked `match`.
This is Phorj's single biggest OOP-model advantage over PHP.

---

## 8. Closures & Functional

All of this is **roadmapped to Track A / M3 S3** (closures + arrow fns + pipe + first-class callables).

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| Closure `function() {}` | PHP 5.3 | none yet → **Track A / S3** | 🔲 Track A | SAME |
| `use (...)` capture by value | PHP 5.3 | immutable capture is the natural default | 🔲 Track A | SAME+syntax — by-value is default; explicit `use` list may be unnecessary |
| `use (&$x)` capture by reference | PHP 5.3 | by-ref capture conflicts with immutability | ❌ reject-by-design | WORSE→reject — mutable aliasing across closure boundary |
| `static function()` (no `$this`) | PHP 5.4 | natural for pure closures | 🔲 Track A | SAME (likely the default) |
| Arrow fn `fn() => expr` | PHP 7.4 | none yet → Track A; auto-captures by value | 🔲 Track A | SAME — auto by-value capture fits immutability perfectly |
| `Closure::bind` / `bindTo` | PHP 5.4 | rebinds `$this` scope at runtime | ❌ reject-by-design | WORSE→reject — runtime scope rebinding = metaprogramming |
| `Closure::call` | PHP 7.0 | runtime `$this` injection | ❌ reject-by-design | WORSE→reject — same |
| `Closure::fromCallable` | PHP 7.1 | superseded by first-class callable syntax | 🔲 Track A | SAME+syntax |
| First-class callable `f(...)` | PHP 8.1 | none yet → Track A | 🔲 Track A | SAME+syntax — clean transpile to PHP `f(...)` |
| `callable` type | PHP 5.4 | will become a proper function type `(T) -> U` | 🔲 Track A | BETTER — a *typed* function signature beats PHP's untyped `callable` |
| `__invoke` objects as callables | PHP 5.3 | closures supersede | 🔲 Track A | SAME+syntax (see §4) |
| **Partial application** `f(?, 2)` / `f(...)` | **PHP 8.6** | pairs with closures + pipe | 🔲 Track A | SAME+syntax — Phorj can adopt the same `?` placeholder; transpiles to PHP 8.6 PFA or a closure |

---

## 9. Generators

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| `yield` | PHP 5.5 | none; coroutine state machine | 🔲 (deferred — post-S3; ties to iteration protocol) | SAME (long-horizon) |
| `yield key => value` | PHP 5.5 | none | 🔲 deferred | SAME |
| `yield from` (delegation) | PHP 7.0 | none | 🔲 deferred | SAME |
| Generator return value | PHP 7.0 | none | 🔲 deferred | SAME |
| `Generator::current/next/send/throw/getReturn` | PHP 5.5/7.0 | none | 🔲 deferred | SAME |

**Note:** generators require suspendable execution state in **both** the tree-walker and the stack VM while
staying byte-identical — a significant correctness-spine challenge. Realistically post-M3, likely tied to
the M6 green-threads work (the only other place suspension appears). Not on a near-term slice.

---

## 10. Exceptions — roadmapped to M3

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| `try` / `catch` / `finally` | PHP 5.0 / 5.5 (finally) | none yet → **M3 exceptions** | 🔲 M3 | SAME |
| Multi-catch `catch (A\|B $e)` | PHP 7.1 | none yet → M3 | 🔲 M3 | SAME |
| Non-capturing catch `catch (A)` | PHP 8.0 | none yet → M3 | 🔲 M3 | SAME+syntax |
| `throw` as expression | PHP 8.0 | none yet → M3 | 🔲 M3 | SAME — fits expression-oriented Phorj well |
| `Throwable` / `Error` / `Exception` hierarchy | PHP 7.0 | needs interfaces + inheritance (S5) | 🔲 M3 (+ S5) | SAME |
| Custom exceptions | PHP 5.0 | needs class inheritance | 🔲 M3 + S5 | SAME |
| `set_exception_handler` (global handler) | PHP 5.0 | runtime global mutable handler | ❌ reject-by-design | WORSE→reject — global mutable callback registry; nondeterministic; against spine |

**Design opportunity:** Phorj already has a **`FaultKind` fault model** (IndexOob, ForceUnwrap, etc.) on
the byte-identical spine. A typed-result / checked-exception or `Result<T,E>`-style approach would be
*more* Phorj-idiomatic than PHP's open `Throwable` hierarchy — worth a design spike when M3 exceptions
open. (Phorj could end up BETTER here via typed errors.)

---

## 11. Reflection API — reject-by-design (metaprogramming)

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| `ReflectionClass` | PHP 5.0 | runtime type introspection | ❌ reject-by-design | WORSE→reject — runtime metaprogramming; no PHP-erasable static target; breaks transpile contract |
| `ReflectionMethod` | PHP 5.0 | runtime method introspection | ❌ reject-by-design | WORSE→reject |
| `ReflectionProperty` | PHP 5.0 | runtime property introspection | ❌ reject-by-design | WORSE→reject |
| `ReflectionAttribute` / attributes `#[...]` | PHP 8.0 | compile-time annotations are conceivable, but runtime reading is not | ❌ reject-by-design (runtime) / 🔲 (compile-time-only attrs, far future) | WORSE→reject for runtime; a *compile-time-only* attribute system could be revisited much later |

**Bucket call:** all Reflection is `❌ reject-by-design`. Reflection is the canonical runtime-metaprogramming
surface; it has no static, PHP-erasable mapping and would shatter the byte-identical spine. (Compile-time
attributes — erased before backends, like generics — are the only conceivable far-future foothold.)

---

## 12. Anonymous Classes, Lazy Objects, Clone-with

| Feature | First ver | Phorj mapping | Bucket | Verdict |
|---|---|---|---|---|
| Anonymous classes `new class {...}` | PHP 7.0 | no named type → conflicts with static nominal typing | ❌ reject-by-design | WORSE→reject — Phorj is nominally typed + packaged; ad-hoc unnamed types break that. (A *struct/record literal* could serve the same need later — different feature.) |
| **Lazy objects** (`newLazyGhost`/`newLazyProxy`) | PHP 8.4 | Reflection-driven deferred init | ❌ reject-by-design | WORSE→reject — built *on* Reflection; runtime proxy machinery; no erasable target |
| **Clone-with** (`clone $o with { x: 1 }`) | PHP 8.5 | immutable-update idiom — a perfect fit | 🔲 (M3 mutation / dedicated immutable-update slice) | **BETTER fit** — for an immutable-by-default language, `clone … with` *is* the canonical update operation; Phorj should adopt it as a first-class immutable-update expression. Transpiles to PHP 8.5 `clone with`. |

---

## Deprecated / Removed PHP features encountered (excluded from accounting)

| Feature | Status | Note |
|---|---|---|
| Dynamic properties | deprecated PHP 8.2, removed PHP 9.0 | use declared typed props (`#[AllowDynamicProperties]` is the escape hatch) |
| `__sleep` / `__wakeup` | **soft-deprecated PHP 8.5** | use `__serialize` / `__unserialize` |
| `__autoload()` | removed PHP 8.0 | use `spl_autoload_register` (n/a to Phorj — no autoloader) |
| `create_function()` | removed PHP 8.0 | use closures |
| `each()` | removed PHP 8.0 | use `foreach` |

---

## Bucket Accounting (non-deprecated features mapped)

Counting the rows above (excluding the "PHP doesn't have this" interface-default row and the
deprecated/removed table):

- **✅ Phorj has ≥**: ~17 (class, namespaces [BETTER], readonly→immutable [BETTER], typed props [BETTER],
  nullable→optionals [BETTER], asymmetric-visibility-subsumed [BETTER], ctor promotion, instance methods,
  return types [BETTER], `__toString` intent, pure enum, backed enum, payload enums [BETTER],
  enum methods, `tryFrom`→`X?` [BETTER], exhaustive match [BETTER], list indexing).
- **🔶 partial**: ~4 (`instanceof`, `ArrayAccess`/indexing, `Countable`, `__invoke` interim).
- **🔲 roadmapped**: ~38 across M3 S5 (inheritance/abstract/interface/traits/static/LSB/constants/
  Stringable/Iterator family), Track A / S3 (closures/arrow/first-class-callable/PFA/variadics/
  named-args/property-hooks), M3 (exceptions family), and deferred (generators family, clone-with,
  enum cases()/from()).
- **❌ reject-by-design**: ~22 (all magic methods except the few re-homed; all Reflection; dynamic props;
  `__destruct`; by-ref return; by-ref closure capture; `Closure::bind`/`call`; anonymous classes;
  lazy objects; `set_exception_handler`).

(Counts are approximate — several features split a row across two buckets, e.g. `instanceof` is 🔶 now /
🔲 S5.)

---

## ❌ Reject-by-design list (with one-line reason)

1. **`__get` / `__set` / `__isset` / `__unset`** — dynamic access to undeclared members; defeats static field typing.
2. **`__call` / `__callStatic`** — dynamic method dispatch with no static signature.
3. **`__debugInfo`** — runtime introspection hook.
4. **`__sleep` / `__wakeup`** — serialization hooks (also deprecated PHP 8.5).
5. **`__serialize` / `__unserialize`** — runtime serialization contract (revisit only with an explicit interface).
6. **`__set_state`** — `var_export` rehydration hook; metaprogramming.
7. **`__destruct`** — nondeterministic finalization; `Rc`/`Drop` already reclaims deterministically.
8. **Dynamic properties / `#[AllowDynamicProperties]`** — deprecated upstream; impossible under static typing.
9. **By-reference return (`&method`)** — aliasing breaks value semantics.
10. **By-reference closure capture (`use (&$x)`)** — mutable aliasing across boundaries; against immutability.
11. **`Closure::bind` / `bindTo` / `call`** — runtime `$this`-scope rebinding = metaprogramming.
12. **Reflection (all: `ReflectionClass`/`Method`/`Property`/`Attribute`)** — the canonical runtime-metaprogramming surface; no erasable PHP target.
13. **Runtime attributes** (`#[...]` read via Reflection) — same; only a *compile-time-only erased* form is conceivable, far future.
14. **Anonymous classes** — unnamed ad-hoc types break nominal+packaged typing (a record/struct literal is a different, allowable feature).
15. **Lazy objects (8.4)** — built on Reflection + runtime proxies; no erasable target.
16. **`set_exception_handler`** — global mutable callback registry; nondeterministic; breaks byte-identical spine.
17. **Mutable static properties** — shared mutable global state (static *constants* and *associated functions* are fine).

---

## 🔲 Roadmapped list (feature → milestone)

**M3 S5 (traits / interfaces / inheritance — already locked):**
abstract classes, interfaces (+ multiple impl, interface constants), `final`, `extends`, `implements`,
class constants (+ visibility, + **typed** 8.3), static methods + (constrained) static members, late static
binding `static::`, `self::`, `parent::`, `instanceof`, `::class`, `protected` visibility, traits (`use`,
`insteadof`, `as`, abstract trait methods, trait constants 8.2), `Stringable`/`__toString`-as-interface,
built-in iteration interfaces (`Iterator`/`IteratorAggregate`/`Traversable`/`Countable`), enum constants,
enum-implements-interface.

**Track A / M3 S3 (closures + functional — already locked/deferred):**
closures `function(){}`, arrow fns `fn() =>`, value-capture, `static` closures, first-class callable `f(...)`,
typed `callable`/function types, `Closure::fromCallable` (subsumed), `__invoke`-as-callable (subsumed),
partial application (`?`/`...`, PHP 8.6), variadic params, named arguments, property hooks (get/set).

**M3 (exceptions — already roadmapped):**
`try`/`catch`/`finally`, multi-catch, non-capturing catch, throw-expression, `Throwable` hierarchy, custom
exceptions. (Design opportunity: typed `Result`-style errors over PHP's open `Throwable`.)

**Deferred / long-horizon (not yet sliced):**
generators (`yield`, `yield from`, `Generator` methods) — tie to iteration protocol + M6 green-threads;
clone-with (PHP 8.5) — immutable-update slice (BETTER fit); enum `cases()`/`from()`/`tryFrom()` built-ins
(cheap); `JsonSerializable` — needs dynamic `Json`/`Any` type (with generics/`core.json`).

---

## Where Phorj is already BETTER than PHP (the headline wins)

1. **Enums-with-payloads = true algebraic sum types.** PHP enums **cannot** carry per-case data — they're
   pure or single-scalar-backed only. Phorj's `Value::Enum` carries arbitrary typed payloads, making it a
   real ADT. This is Phorj's single biggest model advantage.
2. **Compile-time-exhaustive `match`.** PHP `match` throws `UnhandledMatchError` at **runtime** on a missing
   arm; Phorj enforces exhaustiveness at **compile time** (+ null-arm narrowing over `T?`). Strictly safer.
3. **Immutable-by-default subsumes `readonly` AND asymmetric visibility.** PHP 8.1 `readonly` and PHP 8.4
   `public private(set)` are opt-in ceremonies to claw back safety; in Phorj immutability + "public read /
   no external write" is the *default* — no keyword needed.
4. **Optionals `T?` + non-null discipline beat PHP nullable + nullable `tryFrom`.** A non-optional Phorj
   `T` is statically guaranteed non-null; PHP's `?T` is runtime-only and `tryFrom()`'s nullable result is
   easy to ignore. Phorj *forces* the null check.
5. **Mandatory typed properties + mandatory return types.** PHP made these opt-in (7.4 / 7.0); Phorj makes
   them mandatory — no `mixed`-by-omission, no untyped drift.
6. **Mandatory packaging ("nothing in the wind").** Phorj requires `package` everywhere with strict
   folder=path; stricter and more predictable than PHP's optional namespaces + autoloader sprawl.
7. **`final`-by-default posture.** Phorj classes are closed by default (inheritance opt-in at S5); PHP is
   open by default and bolts on `final` after the fact.
8. **Typed function signatures will beat `callable`.** When Track A lands, a typed `(T) -> U` function type
   is strictly more informative than PHP's untyped `callable`.
9. **Clone-with is a natural fit (when adopted).** PHP added `clone with` (8.5) as a readonly workaround; in
   an immutable language it's simply *the* canonical update expression — a clean adoption, not a patch.

---

## Sources

- PHP Manual — Magic Methods: https://www.php.net/manual/en/language.oop5.magic.php
- PHP Manual — Properties (dynamic props deprecation): https://www.php.net/manual/en/language.oop5.properties.php
- PHP Manual — Backed Enumerations: https://www.php.net/manual/en/language.enumerations.backed.php
- PHP Manual — Lazy Objects: https://www.php.net/manual/en/language.oop5.lazy-objects.php
- PHP Manual — First-class callable syntax: https://www.php.net/manual/en/functions.first_class_callable_syntax.php
- RFC — Property Hooks / how hooks happened: https://thephp.foundation/blog/2024/11/01/how-hooks-happened/
- RFC — Asymmetric visibility v2: https://wiki.php.net/rfc/asymmetric-visibility-v2
- RFC — Lazy Objects: https://wiki.php.net/rfc/lazy-objects
- RFC — new in initializers (8.1): https://wiki.php.net/rfc/new_in_initializers
- RFC — first-class callable syntax (8.1): https://wiki.php.net/rfc/first_class_callable_syntax
- RFC — soft-deprecate `__sleep`/`__wakeup` (8.5): https://wiki.php.net/rfc/soft-deprecate-sleep-wakeup
- PHP 8.6 partial application: https://thephp.foundation/blog/2025/12/08/partial-application/
- PHP 8.5 clone-with v2: https://php.watch/rfcs/clone_with_v2
- Dynamic properties deprecated (8.2): https://php.watch/versions/8.2/dynamic-properties-deprecated
- readonly classes (8.2): https://php.watch/versions/8.2/readonly-classes
- Enums (8.1): https://php.watch/versions/8.1/enums
- Deprecated features cheatsheet (5.3→8.5): https://eusonlito.github.io/php-changes-cheatsheet/deprecated.html
- What's new in PHP 8.4: https://stitcher.io/blog/new-in-php-84
- What's new in PHP 8.5/8.6 (pipe, clone, PFA): https://www.phpeveryday.com/articles/whats-new-in-php-8-5-8-6-pipe-operator-clone-partial-function-applications/
