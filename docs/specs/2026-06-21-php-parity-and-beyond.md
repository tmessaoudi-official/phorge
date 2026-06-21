# Phorge PHP-Parity & Beyond — Feature Review

**Date:** 2026-06-21 · **Baseline:** M-RT S5 (commit `e73cab9`) · **Status:** Review deliverable, verdicts pending batched ask-human approval (see Decision Log).

## Purpose

This is the canonical, deduplicated catalogue of every PHP language/stdlib feature (plus a curated set of beyond-PHP design ideas from Rust, Swift, Scala, Kotlin, Go, Gleam, OCaml, Elm, C#, Python and TypeScript) measured against Phorge's current surface. Each row carries a verdict (adopt / phorge-already-better / defer / reject), a ROI estimate, transpile feasibility, and a rationale. It exists to (a) prove Phorge's coverage of PHP is honest and complete, (b) surface the highest-leverage features to build next, and (c) record which features are deliberately out of scope and *why*.

## The byte-identical-transpile constraint (the gate every "adopt" must pass)

Phorge's correctness spine is **`run` ≡ `runvm` ≡ real PHP** — byte-identical stdout, enforced by a PHP oracle (`tests/differential.rs`, `PHORGE_REQUIRE_PHP=1`). Therefore **every "adopt" verdict is bound by two hard constraints**:

1. **Deterministic PHP lowering.** The feature must lower to PHP whose output is identical to both Phorge backends. Anything non-deterministic — wall-clock reads, RNG, network, shell-out, hash/iteration-order instability, async scheduling, GC-timing-dependent destruction — is **low-feasibility / reject or defer-behind-a-fixture-seam**, regardless of how useful it is.
2. **Std-only, zero external Rust crates.** No `regex`, `syn`/`quote`, `chrono`, bignum, or `im`/`rpds` crates. A feature that *needs* an external crate is reject/defer unless a hand-rolled std-only implementation is realistic.

A secondary oracle constraint: the PHP leg runs under **`php -n`**, so **tier-2 extensions (mbstring, intl, bcmath, gmp) are ABSENT**. Any feature whose deterministic PHP target requires a tier-2 extension is reject/defer under the current extension policy (`docs/specs/2026-06-19-extension-policy-design.md`).

## Legend

| Column | Values | Meaning |
|---|---|---|
| **Verdict** | `adopt` | Worth building; passes the byte-identical + std-only gate. |
| | `phorge-already-better` | Phorge already has it, usually in a sounder form than PHP. |
| | `defer` | Worth having but blocked on a prerequisite (mutation+GC, exceptions, generics-as-values, traits/extends, core.json, an iterator protocol, etc.). |
| | `reject` | Out of scope by design — breaks an invariant (determinism, immutability, no-reflection, no-coercion) or has no clean PHP target. |
| **ROI** | `high` / `medium` / `low` | Estimated value-to-cost of building it now. |
| **Transpile** | `yes` | Deterministic 1:1 (or near) PHP lowering exists. |
| | `partial` | Lowers deterministically only for a subset, or needs a helper/fixture seam. |
| | `no` | No deterministic PHP target. |

Evidence grades on the *recommendations* (Executive Summary, re-sequencing) follow CLAUDE.md Rule 18: **[Verified]** (confirmed against the dataset / project surface), **[Inferred]** (consistent with stated mechanisms), **[Speculative]** (design judgment).

---

## Executive Summary

**Total feature rows reviewed:** 646 (deduplicated across 5 research tracks and multiple passes).

**Counts by verdict:**

| Verdict | Count |
|---|---:|
| `adopt` | 195 |
| `phorge-already-better` | 115 |
| `defer` | 196 |
| `reject` | 140 |
| **Total** | **646** |

*(Counts are computed from the dataset's `verdict` field; the same feature catalogued under multiple research tracks contributes one row per track, matching the 646-row corpus — these are deliberate cross-track duplicates that corroborate one verdict, not contradictions. The verdict distribution is stable across duplicates.)* — [Verified: tallied from the provided `verdict` values]

**Headline findings** — [Inferred from the verdict + roi + transpile fields]:

- Phorge's coverage of PHP is strong: 115 rows are *already in Phorge, usually in a sounder form* (mandatory static typing, exhaustive match, `T?` non-null guarantee, payload-carrying ADT enums, typed `==`, the package model, first-class functions, `|>`, range operator, Elm-grade diagnostics).
- The two largest *honest gaps* both have clean deterministic lowerings and no prerequisite: **regex/PCRE** (the single biggest stdlib omission, tier-1 safe) and **sprintf-style formatting** (gated only on variadics).
- A cluster of cheap, deterministic, front-end-only wins (numeric literal forms, destructuring/match patterns, named+default args, derive-style attributes) would close most remaining PHP-parity perception gaps without touching the runtime.
- The big *defers* all trace to four known milestones: mutation+tracing-GC (compound assigns, `++`/`--`, static/global state, `while`/`do-while`/`for`, `clone`-with, property set-hooks), exceptions (`try`/`catch`/`finally`, `throw`, Throwable hierarchy), generics-as-values / a `Json`/`Any` type (`core.json`, `derive(Json)`), and class `extends`/traits (abstract, LSB, `#[\Override]`).
- The *rejects* are principled and consistent: dynamic/reflective features (`eval`, `__get`/`__set`, runtime reflection, dynamic const fetch, `define()`), coercion footguns (loose `==`, `empty()`, cast operators, word-form `and`/`or`), aliasing/mutation (`&` references, `global`, `static` locals), and non-determinism (backticks, `@`, clock reads, RNG, async).

### Top 10 highest-ROI `adopt` candidates (ranked)

1. **Regular expressions / PCRE** (`Core.Regex` match/replace/split/quote) — the single biggest stdlib hole in a text-oriented language; PCRE is tier-1 (oracle-safe under `php -n`); deterministic. Caveat: byte-identity spine needs a **std-only Rust matcher** (the `regex` crate is forbidden) — a hand-rolled subset is the real cost. *[Verified: dataset flags it "BIGGEST single omission", roi=high, tier-1 safe; the std-only build is the gating effort]*
2. **sprintf/printf format specifiers** (`Core.Text.format`) — closes the width/precision/padding/alignment gap that interpolation can't express; lowers 1:1 to PHP `sprintf` (pin `%F`/`LC_NUMERIC=C` for float determinism). Gated only on variadics (#3). *[Verified: roi=high, transpile=yes, deterministic]*
3. **Variadic parameters `...args`** — foundational enabler for sprintf, multi-arg `println`, `max(list)`; transpiles to PHP `...$args`; deterministic, no mutation. *[Verified: roi=high, "foundational ENABLER"]*
4. **Default + named arguments (together)** — table-stakes ergonomics; pure compile-time reorder/fill against the known signature → positional on Phorge backends, native on PHP. Often *replaces* the need for method overloading (one fn with optional params vs N signatures), so it should be weighed alongside the locked overloading slice. *[Verified: roi=high, transpile=yes, front-end-only]*
5. **Enum/variant payload destructuring + match guards + or-patterns** — the obvious next step after S4 type patterns; front-end lowering over existing branch ops (`IsInstance`+`JumpIfFalse`), **no new `Op`**; a guarded arm must be treated as non-covering for exhaustiveness (Rust rule). *[Verified: roi=high cluster, reuses S4 machinery]*
6. **Sorting (`Core.List.sort`/`sortBy`)** — rides the existing `NativeEval::HigherOrder` re-entrant-VM path; Rust `sort_by` is stable matching PHP 8 stable sort → byte-identical; comparator returns the spaceship int. *[Verified: roi=high, reuses higher-order machinery, stable=identical]*
7. **`Result<T,E>` + `?` operator** — the principled no-exceptions recoverable-error story; `Result` is a 2-variant enum (deferred until generic enums), but `?` over **optionals** ships now with no prerequisite (lowers to match-and-return). The exceptions-vs-Result fork resolves Result-first. *[Verified: roi=high; `?`-over-optionals has no prereq, full Result deferred behind generic enums]*
8. **Derive-style compile-time attributes (`#[derive(Eq/Show/Ord/Default)]`)** — the expand-before-backends discipline Phorge already uses (aliases, `erase_generics`, `resolve_html`); generated code is ordinary deterministic PHP, **no runtime reflection**. The native-fit attribute model. *[Verified: roi=high cluster, matches existing erase discipline]*
9. **Class constants + module-level `const`** — compile-time constant folding → inline on Phorge backends, emit PHP `const`; immutable, deterministic, a natural fit for a function-heavy namespaced language. *[Verified: roi=high, "fits the model perfectly"]*
10. **`list()`/array destructuring + spread (`[...$a]`, call-site `f(...$args)`)** — pure desugaring to indexed binds / list-concat; binds-not-mutation so it fits immutability; pairs with variadics. *[Verified: roi=high, deterministic, fits immutable model]*

Honorable mentions (high-ROI, not in the top 10 only for sequencing): **`foreach` over Map/Set** (Map rep is already insertion-ordered → `foreach($m as $k=>$v)`), **opaque newtypes / refinement-with-smart-constructor** (the purest fit for the erasure discipline — erases like `Core.Html`), **numeric literal forms** (hex/binary/exponent + `1_000_000` separators — cheapest wins in the corpus), **debug `inspect`/dump** (via a Phorge-canonical format, not PHP `var_export`, to sidestep the format-match), **bitwise operators** (pure-int, deterministic, needed for bytes/protocol work in M6).

---

## Matrix

Each section is sorted: `adopt` (high → medium → low ROI) → `phorge-already-better` → `defer` → `reject`.

### Track 1 — PHP Core Language

| Feature | In PHP? — how | In Phorge? — how | Relationship | Verdict | ROI | Transpile | Notes |
|---|---|---|---|---|---|---|---|
| Variadic params `...$args` | collects trailing args into array, type-hintable | absent (fixed arity) | php-only | adopt | high | yes | `...args: List<T>` + arity relax + list spread at call sites; deterministic, std-only. |
| Default parameter values | constant-expr defaults | absent (every param supplied) | php-only | adopt | high | yes | Emit PHP defaults; fill at call site on Phorge backends. Keep to const exprs for determinism. |
| Class constants + const expressions | `const FOO=1`; interface consts | absent | php-only | adopt | high | yes | Compile-time fold → PHP `const`; inline on Phorge backends. Immutable, deterministic. |
| `list()`/array destructuring (positional/keyed/nested/foreach) | `[$a,$b]=$arr`, keyed, nested, in foreach | absent (must index) | php-only | adopt | high | yes | Pure desugar to indexed binds; emit PHP `[$a,$b]=`. Binds, no mutation. |
| Single polymorphic `array()` vs split List/Map/Set | one ordered-map for list/dict/set | three typed collections | different | adopt¹ | high | yes | ¹ Central design fork — verdict is **keep the split** (the row is tagged better below); recorded here as the load-bearing decision. |
| `foreach` over Map/Set (k=>v) | `foreach ($arr as $k=>$v)` | for..in over List; Map/Set iteration R1-deferred | php-only | adopt | high | yes | Map rep already insertion-ordered → `foreach($m as $k=>$v)`, deterministic. |
| `match` expression | strict `===`, value position, no fall-through | exhaustive over enums AND unions, arm binding, null narrowing | different | phorge-already-better | low | yes | PHP match is value-equality only; Phorge does ADT + type-pattern exhaustiveness at compile time. |
| Enum/variant payload destructuring in match arms | absent | binds payload as whole value, no field extraction | both-absent | adopt | high | yes | Front-end lowering (bind then read); no new Op; obvious next step after S4. |
| Pure & backed enums | flat enums, methods, `cases()`/`from()` | single+multi-payload ADTs, exhaustive match | different | phorge-already-better | low | yes | Phorge enums are full ADTs; `from()`/`tryFrom()` convenience is the one PHP nicety (covered separately). |
| Constructor property promotion | promoted ctor params (8.0) | ctor promotion (M2 P4b) | same | phorge-already-better | high | yes | Phorge has it and is immutable-by-default. |
| Interfaces + implements | nominal, multiple implements, interface extends | M-RT S2: nominal subtyping, polymorphic dispatch | same | phorge-already-better | low | yes | Exact-signature checked; cross-package interface members work in intersections. |
| Union types A\|B | union types (8.0) | S4 unions + exhaustive match-over-union | same | phorge-already-better | high | yes | Phorge unions are exhaustively checked; PHP unions are not. |
| Pure intersection types A&B | intersection types (8.1) | S5 (interfaces + ≤1 class) | same | phorge-already-better | high | yes | Member access over intersection + instanceof operand + require-agreement sig checks. |
| Param type declarations | optional type hints | mandatory static types | different | phorge-already-better | low | yes | Phorge is stricter; transpiles to PHP type hints. |
| `readonly` properties / classes | retrofit immutability | whole heap immutable by construction | php-only | phorge-already-better | low | yes | `readonly` is Phorge's default; transpiler could emit it to signal intent. |
| `__construct` | constructor | only ctor, with promotion | same | phorge-already-better | low | yes | Single ctor form with mandatory promotion. |
| Static methods | `Class::method()` | absent (instance methods + free fns) | php-only | adopt | medium | yes | Pure dispatch, no state → PHP static function, deterministic. |
| Visibility (public/protected/private) | three levels | fields effectively public | php-only | adopt | medium | yes | Compile-time access check; emit PHP modifiers; closes a real W1 divergence. Asymmetric visibility deferred (needs mutation). |
| `__toString` | object→string coercion | absent | php-only | adopt | medium | yes | A `toString()` recognized by checker → PHP `__toString`; call when non-primitive flows into interpolation. |
| `$object::class` | `$obj::class` (8.0) | absent | php-only | adopt | medium | yes | Transpiles to `$obj::class`; needs a class-name string value decision. |
| Heredoc / Nowdoc | interpolated / literal multi-line | strings already multi-line + interpolated | different | phorge-already-better | low | yes | Only gap is a raw (no-interpolation) string literal → trivial lexer add → PHP nowdoc/single-quote. |
| `never` return type | always-diverges (8.1) | clean-fault model | php-only | adopt | medium | yes | Annotation for always-faulting fn improves exhaustiveness; → PHP `never`. |
| `declare(strict_types=1)` | opt-in strict scalars | always strictly typed | different | phorge-already-better | high | yes | No weak mode to opt out of; transpiler emits `declare(strict_types=1)`. |
| Enumerations (8.1) | no per-case payload | payload ADTs + exhaustive match | same | phorge-already-better | high | yes | Phorge enums carry data; PHP enums cannot. |
| `if`/`elseif`/`else` | standard | if/else + expression-if | same | phorge-already-better | high | yes | Phorge's expression-if is a value → PHP ternary. |
| `switch` | fall-through + loose `==` | exhaustive `match` | different | phorge-already-better | high | yes | `match` is the strictly superior replacement; no fall-through, exhaustive. |
| `return` | value from function | present | same | phorge-already-better | low | yes | No gap. |
| `echo` (multi-arg) / `print` | output constructs | `Console.println` namespaced native | same | phorge-already-better | low | yes | Namespaced, typed, explicit-import; → PHP `echo`. |
| `isset()` | set-and-not-null test | `T?` non-null guarantee + if-let | different | phorge-already-better | high | yes | A non-optional `T` is statically never null; isset unnecessary. |
| Sealed / closed type hierarchies | absent | exhaustive over enums + explicit unions | both-absent | adopt | high | yes | Sealing lets `match` be exhaustive over a NAMED hierarchy; reuses `Op::IsInstance`, no new Op. |
| `abstract` classes/methods | non-instantiable, subclass implements | absent (interfaces exist) | php-only | defer | medium | yes | Gated on roadmapped S6 class-extends. |
| Class `extends` (single inheritance) | parent::, override | absent | php-only | defer | medium | yes | Roadmapped S6 (final-by-default). |
| Traits (insteadof/as) | horizontal reuse | absent | php-only | defer | medium | yes | Roadmapped S8; conflict-resolution syntax is the open question. |
| `final` keyword | cannot extend/override | final-by-default chosen | php-only | defer | low | partial | Only matters once inheritance exists (S6). |
| Late static binding (`static::`) | runtime-class resolution | absent | php-only | defer | low | partial | Double-gated: static methods AND inheritance (S6). |
| Covariance / contravariance | covariant returns, contravariant params | exact-match only | php-only | defer | low | partial | Gated on S6 + a variance design. |
| Property hooks (get/set) | computed/intercepted access (8.4) | absent | php-only | defer | medium | partial | get-hooks immutability-OK (could land sooner); set-hooks need mutation/GC. |
| Generators / `yield` / `yield from` | lazy iterators | absent | php-only | defer | medium | partial | Finite cases modelable as eager List now; true lazy needs coroutine machinery. |
| Fibers | suspendable coroutines (8.1) | single-threaded forced (Rc heap) | php-only | defer | low | partial | Heavy prerequisite: concurrency/green-threads milestone (M6). Reconciled to defer (not reject). |
| `try`/`catch`/`finally` | exception handling | clean-fault model, un-catchable | php-only | defer | high | yes | The gating next-big-feature; Result-first recommended, try/catch as PHP-interop bridge. |
| Multi-catch (`catch A\|B`) | one handler, unrelated types | absent | php-only | defer | medium | yes | Rides on try/catch; reuses S4 union machinery. |
| Throwable/Error/Exception hierarchy | class hierarchy | internal FaultKind, not a value | php-only | defer | high | yes | Prerequisite of try/catch; needs classes-as-throwable-values. |
| `throw` as expression | `?? throw` (8.0) | `opt!` faults but can't throw a value | php-only | defer | medium | yes | Rides on the exception value model. |
| Static (mutable) properties | shared class state | absent | php-only | defer | low | partial | Needs mutation+tracing-GC. |
| `null`/`false`/`true` standalone types | literal types (8.2) | null via optionals; no literal true/false | different | adopt | low | yes | `true`/`false` are cheap singletons → PHP literal types. |
| `void` return annotation | `void` (7.1) | implicit unit | php-only | adopt | low | yes | Explicit `void`/Unit annotation → PHP `void`; clarity win. |
| `self` type hint | self/parent/static | partially implied | php-only | adopt | low | yes | `self` return type adoptable now; parent/static need extends (defer). |
| `while` loop | `while(cond){}` | absent (for..in only) | php-only | defer | medium | partial | Useful condition-loop needs mutation to advance the condition. |
| `do-while` loop | `do{}while();` | absent | php-only | defer | low | partial | Same mutation dependency as while. |
| Classic C-for `for(init;cond;step)` | three-clause | for..in over ranges | php-only | phorge-already-better | low | yes | Range for..in covers the counted 90% bounds-safe; arbitrary-step C-for defers behind mutation. |
| `break`/`continue` | loop control | absent (no loops yet) | php-only | adopt | medium | yes | Plain break/continue over for..in additive; gate alongside while/do-while. |
| `break N`/`continue N` | numeric-level | absent | php-only | reject | low | yes | Multi-level numeric break is a readability footgun; prefer function extraction / labels. |
| Labelled break / loops | absent in PHP | absent | both-absent | adopt | low | yes | Reads better than `break N`; → PHP `break N`. Low ROI nicety. |
| `exit`/`die` | terminate | absent | php-only | adopt | low | yes | A clean `Process.exit(code)` → PHP `exit`; the bare `or die()` idiom rejected. |
| Type casts `(int)`/`(string)`/… | lossy juggling | absent | php-only | reject | medium | partial | Loose-cast operator rejected; adopt narrow checked conversion FUNCTIONS instead (`Int.parse(string)->int?`). |
| `clone` | shallow copy + `__clone` | absent | php-only | defer | low | partial | No-op on immutable values; meaningful only with mutation. |
| Clone-with (`clone with [...]`) | functional update (8.5) | absent | php-only | defer | medium | yes | Functional copy-with-changes is very Phorge-aligned; needs a clone-with construct. |
| `??=` null-coalesce assign | assign if null | absent (no reassignment) | php-only | defer | medium | yes | Blocked on mutation; → PHP `??=` once locals reassignable. |
| Compound assigns `+= -= *= /= %=` | arithmetic compound | absent | php-only | defer | medium | yes | All blocked on mutation; transpile 1:1 once mutation lands. |
| `++`/`--` increment/decrement | pre/post, on strings | absent | php-only | defer | medium | partial | Blocked on mutation; PHP string-increment is a quirk NOT to replicate. |
| Hexadecimal literal `0x1A` | `0x`/`0X` prefix | absent | php-only | adopt | medium | yes | Pure lexer change; PHP accepts `0x1A` → passthrough. |
| Binary literal `0b1010` | `0b`/`0B` | absent | php-only | adopt | medium | yes | Lexer-only, value-identical. |
| Numeric separator `1_000_000` | underscores (7.4) | absent | php-only | adopt | high | yes | Scanner strips `_`; cheapest high-ROI win; works on all bases. |
| Float exponent `1e3` | all versions | partial/unverified | php-only | adopt | medium | yes | Standard float lexing; confirm exponent already parses before treating as new. |
| Octal `0o14` prefix | (8.1); reject legacy bare-0 | absent | php-only | adopt | low | yes | Adopt explicit `0o`; reject legacy bare-0 octal (PHP's own footgun). |
| Unicode escape `\u{1F600}` | double-quoted (7.0) | absent | php-only | adopt | medium | partial | Lexer emits UTF-8; → PHP `\u{}`. Clean with the bytes/string model. |
| Magic const `__LINE__` | compile-time line | absent | php-only | adopt | medium | yes | Compile-time int from AST → PHP `__LINE__`; useful for diagnostics. |
| Magic const `__FUNCTION__`/`__METHOD__`/`__CLASS__` | compile-time names | absent | php-only | adopt | low | yes | All compile-time strings from AST → same PHP magic constant. |
| Magic const `__NAMESPACE__` | current namespace | absent | php-only | adopt | low | yes | Maps to current `package` / mangled FQN. |
| Magic const `__FILE__`/`__DIR__` | absolute path | absent | php-only | defer | low | partial | Path is environment-dependent → breaks byte-identity; gate behind canonical-path normalization. |
| Magic const `__TRAIT__`/`__PROPERTY__` | trait / 8.4 hooks | absent | php-only | defer | low | yes | Blocked on traits (S8) / property hooks. |
| `const` (module-level named constant) | compile-time, ns-scoped | absent (locals + class consts only) | php-only | adopt | medium | yes | → PHP namespaced `const`. Natural for a function-heavy namespaced language. |
| `callable` type hint | opaque | structural `(T)->R` | different | phorge-already-better | medium | yes | Phorge's function type is more precise; erases to PHP `\Closure`. |
| `iterable` type hint | array\|Traversable (7.1) | absent | php-only | defer | low | partial | Waits on a user iterator protocol. |
| `object` (top object type) | (7.2) | absent | php-only | defer/reject | low | partial | Weakens static typing; low value pre-reflection. |
| Nullable `?T` (prefix) | runtime-nullable (7.1) | postfix `T?` + compile-time non-null | different | phorge-already-better | low | yes | Phorge's `T?` gives a guarantee PHP's `?T` lacks. |
| Typed properties (7.4) | opt-in | always typed | different | phorge-already-better | low | yes | Mandatory not opt-in. |
| User-definable iterator protocol | Iterator / IteratorAggregate | for..in over built-ins only | php-only | defer | high | partial | Eager "return a List" form deterministic now; stateful cursor needs mutation. |
| Namespace + use / group use | `use A\{B,C}`, `\` separator | mandatory package, Go-qualified, strict folder=path | different | phorge-already-better | high | yes | Stricter and sounder ("nothing in the wind"); → PHP namespaces. |
| `array_find`/`array_any`/`array_all` (8.4) | predicate-based | map/filter/reduce present | same | adopt | high | yes | Additive over the higher-order-native path; → PHP 8.4 (polyfill <8.4). |
| `array_first`/`array_last` (8.5) | first/last value | absent | php-only | adopt | medium | yes | Trivial natives returning `T?` (better than bare null); → `array_first`/`array_last`. |
| `references` (`&`) / aliasing | `$b = &$a` | absent by design | php-only | reject | low | no | Aliasing breaks the immutable+acyclic heap. |
| `global` keyword | import a global | absent ("nothing in the wind") | php-only | reject | low | no | Mutable global state; off-design. |
| `static` local variables | persists across calls | absent | php-only | reject | low | no | Per-call mutable state; breaks referential transparency. |
| `goto` / labels | restricted goto | absent | php-only | reject | low | partial | Unstructured control flow fights the analyzable IR. |
| `__halt_compiler()` | trailing binary data | absent (`phg build` instead) | php-only | reject | low | no | Superseded by `phg build` (embedded `.phorge` section). |
| `declare(ticks=N)` | tick handler | absent | php-only | reject | low | no | Runtime hook, non-deterministic. |
| Anonymous classes | inline one-off classes | absent | php-only | reject | low | partial | Cuts against mandatory-package, named-types model. |
| `__destruct` | on destruction | absent (Rc/Drop) | php-only | reject | low | no | Destruction timing non-deterministic → breaks spine. |
| `__invoke` | callable object | absent (first-class fns) | php-only | reject | low | partial | Superseded by first-class functions. |
| `__get`/`__set`/`__isset`/`__unset` | dynamic property interception | absent | php-only | reject | low | no | Needs mutation + dynamic shapes vs typed immutable fields. |
| `__call`/`__callStatic` | dynamic method interception | absent | php-only | reject | low | no | Dynamic dispatch defeats the static checker. |
| `__clone` | customize clone | absent | php-only | reject | low | no | Meaningless on immutable values. |
| `__debugInfo`/`__set_state` | var_dump/var_export hooks | absent | php-only | reject | low | partial | Tooling-specific; no deterministic transpile need. |
| `empty()` | falsy test | explicit comparisons | php-only | reject | low | yes | Conflates 0/'0'/''/[]/null — notorious footgun. |
| `unset()` | destroy a variable | absent | php-only | reject | low | no | Conflicts with immutable+acyclic heap. |
| `eval()` | execute a string | absent | php-only | reject | low | no | Breaks determinism + spine + security. |
| `require`/`include`/`*_once` | runtime file inclusion | static package/import | php-only | reject | low | no | `import` is strictly better (compile-time, no scope pollution). |
| Alternative syntax (`endif`/`endforeach`) | template colon syntax | absent | php-only | reject | low | yes | Template-era PHP-ism; zero value in a brace language. |
| `WeakMap` | weak refs (8.0) | absent | php-only | reject | low | no | Needs mutation + weak refs + GC. |
| Distinct `char` type | none (1-len string) | string + bytes | both-absent | reject | low | no | No idiomatic PHP target; grapheme needs mbstring (oracle-absent). |
| Unicode codepoint/grapheme ops (mb_*/intl) | mbstring / intl | byte-only | php-only | reject | low | no | Tier-2 extensions ABSENT under `php -n` oracle. |
| BigInt / arbitrary precision | GMP / BcMath\Number | checked i64 (clean overflow fault) | php-only | reject/defer | low | no | Ext breaks `php -n` + std-only; checked-int already beats silent overflow. |
| Integer overflow (checked vs silent float-promote) | silent promotion to float | checked → clean fault | different | phorge-already-better | low | yes | Deliberate correctness choice; transpile helper fault-matches (M7 pattern). |
| `define()` runtime/dynamic constant | `define($name,$val)` | absent | php-only | reject | low | no | Dynamic name construction; `const` covers the legitimate use. |
| Dynamic class const fetch `C::{$name}` | (8.3) | absent | php-only | reject | low | no | Dynamic name resolution breaks static typing. |
| Runtime reflection (get_class/method_exists/…) | full Reflection API | compile-time instanceof + smart-cast | php-only | reject | low | partial | Dynamic-typing-adjacent; instanceof/exhaustive-match is the principled answer. |
| Doc-comment metadata (PHPDoc `@param`) | comment convention | real static types + attributes | php-only | reject | low | partial | Redundant with the type system; structured metadata is the attributes track. |

### Track 2 — PHP Operators (Complete Set)

| Feature | In PHP? — how | In Phorge? — how | Relationship | Verdict | ROI | Transpile | Notes |
|---|---|---|---|---|---|---|---|
| Spaceship `<=>` | three-way, −1/0/1 | absent | php-only | adopt | medium | yes | Deterministic 1:1; pairs with a future `Core.List.sort` comparator; reuses `compare_ord`. |
| Bitwise `&` `\|` `^` `~` `<<` `>>` | integer bitwise | absent (`&`/`\|` claimed by intersection/union types) | php-only | adopt | medium | yes | Pure-int, deterministic, 1:1 PHP; needed for bytes/protocol (M6). Parser disambiguates value-vs-type position. |
| Boolean XOR | `xor` / `^` on bools | absent (`a != b`) | php-only | adopt | low | yes | Minor convenience; already expressible as `a != b`. |
| Exponentiation `**` | right-assoc | absent (`Core.Math.pow`) | php-only | adopt | low | yes | Pure sugar over existing `pow`; → PHP `**`. Low ROI. |
| Arithmetic `+ - * / %` | silent coercion / juggling | checked, numeric-only `+` | different | phorge-already-better | high | yes | `+` is numeric-only (no concat/array-union); checked-overflow faults; runtime helpers keep byte-identity. |
| Loose `==`/`!=`/`<>` | type juggling | typed structural `==` | different | phorge-already-better | high | yes | One safe `==`; transpiles to PHP `===` to preserve identity. |
| Relational `< <= > >=` | with juggling | typed | same | phorge-already-better | high | yes | Applied only to compatible typed operands. |
| Logical `&& \|\| !` | short-circuit | bool-typed, lowered via branch ops | different | phorge-already-better | high | yes | Same semantics; no truthiness coercion. |
| `??` null-coalesce | null/unset fallback | type-driven over `T?` | different | phorge-already-better | high | yes | Compile-time non-null; PHP's also swallows undefined keys. |
| `?->` nullsafe | short-circuit member access | `?.` | same | phorge-already-better | high | yes | Result type is itself optional; → PHP `?->`. |
| Force-unwrap `opt!` | absent (no non-null type) | checked + `W-FORCE-UNWRAP` lint | phorge-only | phorge-already-better | high | yes | Genuine advantage over PHP's nullable-everywhere. |
| Pipe `\|>` | (8.5), single-param | lowered to a Call, any target | same | phorge-already-better | high | yes | Phorge shipped first and is more general than PHP 8.5's restriction. |
| Ternary `?:` / Elvis `?:` | full + short-form | expression-if + `??` | different | phorge-already-better | medium | yes | Expression-if (mandatory else) → PHP ternary; avoids `?:` truthiness coercion. |
| String concat `.` / `.=` | concat (coerces) | interpolation / `Core.Text.join` | different | phorge-already-better | medium | yes | Avoids `+`-vs-`.` ambiguity; `.=` needs mutation anyway. |
| Index `[]` subscript | missing key warns+null | polymorphic List/Map, clean faults | same | phorge-already-better | high | yes | Faults cleanly on OOB/missing-key vs PHP silent null+warning. |
| `new` instantiation | `new Class(args)` | call-form `Class(args)` | different | phorge-already-better | medium | yes | Cleaner, fewer keywords; → PHP `new`. |
| Member access `->`/`::` | instance / static | `.` ; module-qualified for statics | different | phorge-already-better | high | yes | One accessor; → PHP `->` / `::`. |
| Range `a..b` / `a..=b` | none (range() fn only) | `Op::MakeRange` → List\<int\> | phorge-only | phorge-already-better | high | yes | Phorge has a range OPERATOR; → PHP `range()`. |
| First-class callable `f(...)` | (8.1) | bare named-fn is a value | different | phorge-already-better | high | yes | No `(...)` ceremony; → PHP first-class callable. |
| Collection comparison `==` | order-insensitive assoc | typed structural (Set eq order-independent) | same | phorge-already-better | medium | yes | One `==` suffices; shipped for List/Map/Set. |
| Compound assigns `+= … **= .= &= …` | full family | absent | php-only | defer | medium | yes | Entire family gated on mutation+GC; `.=` also needs concat (rejected). |
| `??=` | assign if null | absent | php-only | defer | medium | yes | Mutation-dependent. |
| Bitwise compound `&= \|= ^= <<= >>=` | compound bitwise | absent | php-only | defer | low | yes | Gated on mutation. |
| `++`/`--` | pre/post, on strings | absent | php-only | defer | low | partial | Mutation-dependent; reject string-increment sub-behavior. |
| `clone` / clone-with | `clone $o`; `clone with` (8.5) | absent | php-only | defer | low | yes | No-op on immutable heap; revisit with mutation. |
| `$obj::class` | (8.0) | absent | php-only | defer | low | yes | Reflection-adjacent; low ROI until attributes. |
| Array union `+` | left keys win | absent | php-only | reject | low | yes | Operator-overloading-by-fiat; a Core merge native is idiomatic. |
| Array unpacking `...` (string keys) | (8.1) | absent | php-only | adopt/defer | medium | partial | Useful for literals + call sites; needs a variadic/rest design first; → PHP `...`. |
| `===` / `!==` strict | (because `==` loose) | absent (`==` already strict) | php-only | reject | low | yes | Redundant; Phorge `==` transpiles to PHP `===`. |
| Word-form `and`/`or`/`xor` | low-precedence | absent | php-only | reject | low | yes | Known precedence footgun. |
| Error-suppression `@` | suppress warnings | absent | php-only | reject | low | no | Hides errors; conflicts with clean-fault model. |
| Execution backticks `` `cmd` `` | shell execution | absent | php-only | reject | low | no | Non-deterministic + security hazard; a future explicit `Core.Process` native, never an operator. |
| Reference operator `&` | aliasing | absent | php-only | reject | low | no | Aliasing/shared-mutable conflicts with the immutable heap. |
| `yield` / `yield from` | generators | absent | php-only | defer | medium | partial | Needs suspension machinery; → PHP generators. |
| `throw` (expression) | (8.0) | clean faults | php-only | defer | medium | yes | Needs the exception value model. |
| `print` (construct) | returns 1 | `Console.println` | different | phorge-already-better | low | yes | Explicit namespaced native; the returns-1 quirk intentionally absent. |
| Cast operators `(int)`/… | lossy juggling | conversion functions | php-only | reject | low | partial | Cast-operator form breaks the no-coercion invariant. |
| `instanceof` | class/interface | S1 with smart-cast, accepts union/intersection | different | phorge-already-better | high | yes | Adds smart-cast narrowing beyond PHP; → PHP `instanceof`. |
| BcMath\Number (operator-overloaded objects) | + − * / etc. (8.4) | absent | php-only | defer | medium | partial | Real operator-overloading precedent; a `Core.Decimal` would be its first client; bignum is std-only-hard. |

### Track 3 — Per-Version RFC Catalogue (PHP 7.0–8.5)

| Feature | In PHP? — how | In Phorge? — how | Relationship | Verdict | ROI | Transpile | Notes |
|---|---|---|---|---|---|---|---|
| Scalar type declarations (7.0) | coercive/strict hints | static types by default | different | phorge-already-better | high | yes | Statically-typed-by-default with a checker. |
| Return type declarations (7.0) | `: T` | `-> T` mandatory on stmt-body | same | phorge-already-better | medium | yes | `-> T` → PHP `: T`. |
| `??` (7.0) | yes | S2 | same | phorge-already-better | low | yes | Shipped with compile-time null-safety. |
| Group use (7.0) | `use ns\{A,B as C}` | per-import import | different | adopt | low | yes | Brace-grouped imports are pure sugar over existing import. |
| `intdiv()` (7.0) | because `/` floats | int `/` already exact + checked | different | phorge-already-better | low | yes | A `Core.Math.intdiv` trivial if wanted. |
| Nullable `?T` (7.1) | runtime-nullable | `T?` compile-time non-null | different | phorge-already-better | high | yes | TS-style strictNullChecks vs PHP runtime-nullable. |
| `void` (7.1) | yes | implicit unit | different | adopt | low | yes | A void/Unit annotation → PHP `: void`. |
| Symmetric destructuring `[$a,$b]=` (7.1) | yes | absent | php-only | adopt | medium | yes | → PHP `[$a,$b]=`; pairs with match destructuring. |
| `Closure::fromCallable` (7.1) | yes | first-class fn values | different | phorge-already-better | low | yes | First-class fn values native (S3). |
| Arrow functions (7.4) | by-value capture | `fn(x)=>e` capture by value | same | phorge-already-better | medium | yes | Direct analogue; → PHP arrow fn. |
| Spread in array literals `[...$a]` (7.4) | yes | absent | php-only | adopt | medium | yes | List-spread → PHP `[...$a]`; ties to argument unpacking. |
| First-class callable `f(...)` (8.1) | yes | yes | same | phorge-already-better | high | yes | Shipped (S3); → PHP `f(...)`. |
| Named arguments `f(name: x)` (8.0) | yes | absent | php-only | adopt | medium | yes | Pure compile-time reorder/fill; → PHP named args. |
| Saner numeric-string `==` (8.0) | `0 == "foo"` now false | no loose `==` | different | phorge-already-better | low | yes | Whole footgun class structurally absent. |
| `never` (8.1) | yes | clean-fault model | php-only | adopt | low | yes | → PHP `never`; improves exhaustiveness. |
| Octal `0o` (8.1) | yes | absent | php-only | adopt | low | yes | Pure lexer; → PHP `0o` or decimal. |
| Numeric separators `1_000_000` (7.4) | yes | absent | php-only | adopt | medium | yes | Lexer strips `_`; high readability. |
| RoundingMode enum (8.4) | round() modes | absent | php-only | adopt | low | yes | `Core.Math.round(x,mode)` → PHP `round()`+RoundingMode (const for <8.4). |
| `array_find`/`any`/`all`/`find_key` (8.4) | predicates | map/filter/reduce | php-only | adopt | medium | yes | Trivial in the existing higher-order mold; → PHP 8.4. |
| Casts in const expressions (8.5) | `(int)0.3` | absent | php-only | adopt | low | yes | Compile-time cast folding; deterministic. |
| Readonly properties (8.1) | opt-in | immutable-by-default | same | phorge-already-better | high | yes | Readonly is the default. |
| Readonly classes (8.2) | `readonly class` | whole-class immutability default | same | phorge-already-better | high | yes | Default. |
| DNF types `(A&B)\|C` (8.2) | DNF | `&` binds tighter than `\|` since S5 | same | phorge-already-better | low | yes | Already expressible; verify normalization emits 8.2-valid DNF. |
| Deprecated dynamic properties (8.2) | now error | no dynamic properties | different | phorge-already-better | low | yes | The whole bug class is structurally absent. |
| `static` return type / LSB (8.0) | yes | absent | php-only | defer | low | yes | Needs extends + LSB (S6). |
| Stringable (auto from `__toString`) (8.0) | yes | absent | php-only | defer | low | partial | Needs a toString/display convention; pairs with derive(Show). |
| Trailing comma in param lists (8.0) | allowed | absent | php-only | adopt | low | yes | Trivial parser tolerance; PHP accepts both forms. |
| `new` in initializers (8.1) | default/attr args | absent | php-only | defer | low | yes | Needs CTFE of construction. |
| Named args after unpacking (8.1) | `f(...$a, named:x)` | absent | php-only | adopt/defer | low | yes | Rides on named-args adoption. |
| `false`/`true` standalone types (8.2) | literal types | absent | php-only | defer | low | yes | Pairs with future refinement/literal types. |
| Final class constants (8.1) | yes | absent | php-only | defer | low | yes | `final` only matters with inheritance (S6). |
| Typed class constants (8.3) | yes | absent | php-only | defer | medium | yes | No class constants yet; deterministic once a design lands. |
| `#[\Override]` (8.3) | compiler-verified | absent | php-only | adopt/defer | medium | yes | Compile-checkable once S6 extends lands; → PHP `#[\Override]`. |
| Readonly amendments (clone reinit) (8.3) | yes | absent | php-only | defer | medium | yes | Needs clone + controlled re-init. |
| `json_validate()` (8.3) | yes | absent | php-only | defer | medium | yes | Land with core.json (needs dynamic Json/Any). |
| Asymmetric visibility `private(set)` (8.4) | controlled mutability | absent | php-only | reject/defer | low | partial | The point is a writable-but-restricted property; moot under immutability. |
| Lazy objects (8.4) | newLazyGhost/Proxy | absent | php-only | reject | low | no | Reflection-driven deferred init; needs mutation + reflection. |
| `#[\Deprecated]` (8.4) | engine warns | warning channel exists | php-only | adopt/defer | medium | yes | Compile-time `W-DEPRECATED` lint + PHP `#[\Deprecated]`. |
| `new X()->m()` without parens (8.4) | yes | absent | php-only | adopt | medium | yes | Pure parser/precedence ergonomics; deterministic. |
| `request_parse_body()` (8.4) | RFC1867 | absent | php-only | reject | low | no | SAPI/superglobal runtime; out of the value-level M6 contract. |
| Identical symbols across ns blocks (8.4) | symbol reuse | loader mangles per-package FQNs | phorge-only | phorge-already-better | low | yes | Per-package mangling already isolates symbols. |
| Implicit-nullable deprecation (8.4) | `T $x=null` needs `?T` | non-null enforced | different | phorge-already-better | low/medium | yes | Validates Phorge's optional/non-null split. |
| Closures in const expressions (8.5) | yes | absent | php-only | defer | low | yes | Needs CTFE of closures. |
| `#[\NoDiscard]` (8.5) | must-use return | absent | php-only | defer | medium | yes | `W-UNUSED-RESULT` via warning channel once attributes land. |
| Clone-with (8.5) | functional update | absent | php-only | defer | high | yes | The idiomatic way to "change" an immutable value; high ROI when unblocked. |
| Final property promotion (8.5) | yes | promoted fields already immutable | different | phorge-already-better | low | yes | Already immutable. |
| Constant arrays via `define()` (7.0) | runtime registration | absent | php-only | reject | low | no | `define()` is dynamic; use `const`. |
| `Closure::call()` (7.0) | rebind scope | no this-capture | php-only | reject | low | no | Dynamic scope rebinding conflicts with by-value capture. |
| Negative string offsets `s[-2]` (7.1) | yes | absent (bounds-checked) | php-only | reject | low | partial | Clashes with bounds-checked index + clean fault. |
| Covariant returns / contravariant params (7.4) | yes | exact-match | different | defer | low | yes | Deferred in S3/S5 KNOWN_ISSUES; needs subtyping-aware sig checks. |
| Weak refs / preloading / OPcache (7.4) | VM internals | absent | php-only | reject | low | no | No transpile target; conflicts with Rc model. |
| Random extension (8.2) | Randomizer | absent | php-only | reject | low | no | Non-deterministic. |
| Dynamic class const fetch (8.3) | `C::{$name}` | absent | php-only | reject | low | no | Dynamic name resolution. |
| Static var initializers (8.3) | arbitrary expr | absent | php-only | reject | low | no | Static mutable locals conflict with immutability. |
| Anonymous readonly classes (8.3) | yes | absent | php-only | defer | low | yes | Blocked on anon classes. |
| `final` on trait methods (8.3) | yes | absent | php-only | defer | low | yes | Blocked on traits. |
| INI fallback `${VAR:-default}` (8.3) | php.ini | absent | php-only | reject | low | no | Engine-config, not a language feature. |

### Track 4 — Attributes & Metadata

| Feature | In PHP? — how | In Phorge? — how | Relationship | Verdict | ROI | Transpile | Notes |
|---|---|---|---|---|---|---|---|
| Attribute concept `#[Attr]` | structured declaration metadata (8.0) | absent | php-only | adopt | high | yes | **Anchor.** Adopt as an INERT PASSTHROUGH channel + compile-time DERIVE; checker validates placement, transpiler re-emits literal PHP `#[...]`. |
| Architecture fork: compile-time-consumed vs reflection-consumed | runtime reflection | absent | different | adopt | high | yes/partial | Adopt the derive/codegen channel (native fit) + inert passthrough; **reject** the runtime-reflection reader. |
| Inert (passthrough) attribute channel | attrs inert until reflected | absent | different | adopt | high | yes | Go-struct-tag / `@Retention(SOURCE-re-emitted)` model; byte-identity trivially holds (Phorge runtime ignores it). |
| `#[derive(Eq/PartialEq)]` structural equality | absent | absent | both-absent | adopt | high | yes | Checker expands field-wise eq into AST before backends → plain PHP method; deterministic, std-only. |
| `#[derive(Show/Display)]` toString | manual `__toString` | absent | both-absent | adopt | high | yes | Synthesize toString from fields → PHP method; also the clean answer to the var_dump gap. |
| Compile-time source generation (C# SourceGen / Swift macros / APT / KSP) | absent | checker passes do AST codegen | both-absent | adopt | high | yes | Validates the closed-derive direction; generated code is ordinary, deterministic, std-only. |
| Rust `#[derive(...)]` (reference model) | absent | absent | both-absent | adopt | high | yes | The reference for the compile-time-consumed mechanism. |
| Go struct tags (erased inert metadata) | PHP attrs (loosely) | absent | both-absent | adopt | high | yes | Cleanest model for the erasure discipline; maps 1:1 to inert passthrough. |
| Routing attributes `#[Route(...)]` | Symfony reflection | absent | php-only | adopt | high | yes | Pure passthrough; Symfony consumes at cache-warm; Phorge runtime never reads it. |
| Validation attributes `#[Assert\…]` | Symfony Validator | absent | php-only | adopt | high | yes | Passthrough, or (better long-term) derive-style generated `validate()`. |
| ORM mapping `#[ORM\Entity/Column]` | Doctrine reflection | absent | php-only | adopt¹ | high | yes | ¹ As passthrough only; namespaced attribute names need backslash-qualified parsing. (Conflicts with the no-DB rejection elsewhere — passthrough emits, Phorge has no ORM semantics.) |
| `#[derive(Ord/Compare)]` ordering | manual usort | absent | both-absent | adopt | medium | yes | Field-lexicographic compare → PHP `<=>`; pairs with future sort. |
| `#[derive(Default)]` default ctor | absent | absent | both-absent | adopt | medium | yes | Default constructor from field types; composes with promotion. |
| Attribute syntax + target restriction + IS_REPEATABLE | TARGET_* bitmask | absent | php-only | adopt | medium | yes | Compile-time placement validation (E-code) + passthrough; start with per-attr allow-lists. |
| `#[\Deprecated]` (8.4) | engine warns | warning channel | php-only | adopt | medium | yes | Dual-mode: emit `W-DEPRECATED` + pass through PHP `#[\Deprecated]`. |
| `#[\NoDiscard]` (8.5) | must-use return | absent | php-only | adopt | medium | yes | Checked: emit `W-NODISCARD` when result dropped + re-emit PHP. |
| `#[SensitiveParameter]` (8.2) | redact from traces | absent | php-only | adopt | low | yes | Pure passthrough on a parameter target. |
| Attributes on parameters | per-param reflection | absent | php-only | adopt | medium | yes | Parser accepts `#[...]` in param position; re-emit in the PHP slot. |
| Java/C# retention (`@Retention`) | SOURCE/CLASS/RUNTIME | absent | different | adopt | medium | yes/partial | The channel-selector concept (derive-erased vs passthrough), not user-facing syntax. |
| DI attributes `#[Required]/#[Autowire]` | Symfony DI reflection | absent | php-only | defer | low/medium | partial | Passthrough works; DI is a PHP-framework runtime concern Phorge doesn't model. |
| Test-discovery `#[Test]/#[DataProvider]` | PHPUnit reflection | absent | php-only | adopt/reject | medium | yes/no | Passthrough lets PHPUnit test Phorge output; Phorge's own harness is the Rust oracle. |
| `#[\Override]` (8.3) | compiler-checked | absent | php-only | defer | medium | yes | Blocked on S6 extends; then a CHECKED attribute. |
| Attributes on enum cases | per-case metadata | absent | php-only | defer | low | partial | Passthrough deterministic; Phorge-side consumption needs reflection (rejected). |
| `#[derive(Hash)]` | absent | HKey internal | both-absent | defer | low | partial | Only matters once class instances are valid Map/Set keys. |
| `#[derive(Json)]` serialize/deserialize | manual jsonSerialize | absent | both-absent | defer | high | partial | Blocked on core.json (needs dynamic Json/Any). Serialize-only could land sooner. |
| Phantom/marker attributes (Annotated-style) | analyzer markers | absent | both-absent | defer | low | partial | Overlaps refinement/newtype types; better solved there. |
| Swift property wrappers | ≈ property hooks | absent | both-absent | defer | low | partial | Overlaps roadmapped property hooks; set-hook needs mutation. |
| Reflection-based consumption (getAttributes/newInstance) | runtime reflection | absent | php-only | reject | low | no | No runtime reflection in Phorge; non-deterministic; the dividing line. |
| User-defined / open proc-macros (syn/quote) | absent | absent | both-absent | reject | low | no | Arbitrary compile-time code → non-determinism + external crates; ship closed derive instead. |
| Decorator-as-transform (Python/TS `@decorator`) | absent | absent | both-absent | reject | low | no | Runtime wrapper/replace; conflicts with static/immutable model; no PHP target. |
| Python `typing.Annotated[T, meta]` | ≈ attrs | absent | php-only | reject | low | partial | Specific form has no PHP type-position target; concept covered by passthrough. |
| TS decorators + reflect-metadata | runtime wrappers + polyfill | absent | both-absent | reject | low | partial | Runtime-wrapper half breaks the static model; metadata half duplicates passthrough. |
| Scala 3 macros | metaprogramming runtime | absent | both-absent | reject | low | partial | Over-scoped for std-only; derive covers 90%. |
| `@-bindings` (bind whole value while destructuring) | absent | absent | both-absent | adopt | medium | partial | `x @ 1..=5` — small front-end desugar; companion to match guards. |

### Track 5 — Beyond PHP, High-ROI

| Feature | In PHP? — how | In Phorge? — how | Relationship | Verdict | ROI | Transpile | Notes |
|---|---|---|---|---|---|---|---|
| Match guards (if-conditions in patterns) | absent | absent | both-absent | adopt | high | yes | Top pick. Lowers to existing branch ops; no new Op; a guarded arm is non-covering for exhaustiveness. |
| `Result<T,E>` + `?` propagation | absent (exceptions) | clean faults + optionals, no Result | both-absent | adopt | high | yes/partial | `?` over optionals ships now; full `Result` deferred behind generic enums. The no-exceptions answer. |
| Structural destructuring in match arms (nested ADT/class fields) | partial (list()) | binds single payload | different | adopt | high | yes | `Wrap(Point{x,y})` → field reads + binds; obvious next step after S4. |
| Or-patterns `A(x) \| B(x) =>` | absent | single-pattern arms | both-absent | adopt | high | yes | Lowers to ‖-joined tests; binding-consistency is a front-end check. |
| Opaque types / nominal newtypes | absent | transparent `type` aliases only | different | adopt | high | yes | `type UserId = opaque int;` checked nominally, ERASED to PHP int — exactly the `Core.Html` discipline. |
| Refinement / newtype-with-smart-constructor | absent | absent | both-absent | adopt | high/medium | yes | Opaque type + `make(s)->T?` validator (fits no-exceptions). Flow-sensitive refinement deferred (research-grade). |
| let-else / guard-let | absent | if-let exists (S2) | different | adopt | high | yes | The dual of S1.4 if-let; lowers to `if(!cond){<diverge>}`; no new Op. while-let deferred (needs mutation/iterator). |
| Gleam-style `use` (callback-flattening) | absent | lambdas + first-class fns (S3) | both-absent | adopt | high/medium | yes | Parse-time CPS rewrite → nested closures; restrict to non-this contexts (E-LAMBDA-THIS). Best with Result. |
| Default + named arguments (together) | defaults + named (8.0) | neither | php-only | adopt | high | yes | Table-stakes ergonomics; compile-time reorder/fill; often replaces overloading. |
| Inspect / debug-print (var_dump/print_r/var_export) | yes | only `Console.println` | php-only | adopt | high/medium | yes/partial | A Phorge-canonical `Core.Debug.dump` on both backends (NOT mirroring var_export) sidesteps format-match; cycles impossible (acyclic heap). |
| Derive-style codegen (`#[derive]`) | absent (attrs don't codegen) | absent | both-absent | adopt | high/medium | yes | Expand-before-backends discipline; deterministic, std-only. (Cross-listed with attributes track.) |
| Sealed / closed type hierarchies | absent | exhaustive over enums + unions | both-absent | adopt | high/medium | yes | Reuses S1 `Op::IsInstance`; PHP `instanceof` cascade; mostly sugar over S4. |
| Range patterns `0..=9 =>` | absent | range expressions exist | different | adopt | medium | yes | Lowers to a `lo<=x && x<=hi` guard; subsumed by match guards. |
| List / slice patterns `[first, ..rest]` | partial (list()) | absent | different | adopt | medium | yes | Length check + indexed reads (reuses `Op::Index`); pairs with destructuring. |
| `const fn` / CTFE (minimal const-fold) | partial (const-expr) | absent | different | adopt | medium | partial | Fold pure const-expressions to PHP literals; evaluator = pure interpreter subset. Adopt minimal; defer arbitrary const fn. |
| Design by Contract (requires/ensures) | absent (assert only) | clean-fault target | both-absent | adopt | medium | yes | `requires(cond)` → fault-emitting guard; fits the fault model now. Invariants need mutation → defer. |
| Pipeline-first / data-last stdlib | absent | has `\|>` but data-FIRST natives | different | adopt | medium | yes | **Latent footgun:** `xs \|> List.map(f)` is wrong-arity today. Reshape Core.List/Map signatures data-last; PHP emission unaffected. |
| Labelled tuples / structural records | absent (arrays) | nominal only | both-absent | adopt/defer | medium | yes | `{x:int,y:int}` lowers to a PHP array; **tension** with the nominal spine — keep lean (anonymous records) or defer. |
| Immutable-by-default | absent (readonly opt-in) | core invariant | phorge-only | phorge-already-better | high | yes | Foundational win; flip side blocks while-let/TCO/loop-accumulators. |
| String-interpolation typing (type-directed holes) | untyped | `html"…"` + checked `"{e}"` | phorge-only | phorge-already-better | medium/low | yes | Already shipped (Core.Html); ahead of PHP and most langs. |
| Exhaustive-everything (compile-time) | runtime UnhandledMatchError | compile-time exhaustive over enums/unions/optionals | phorge-only | phorge-already-better | low | yes | Already on the compile-time side. |
| Exhaustive ADTs with associated data | backed/pure enums only | payload ADTs | phorge-only | phorge-already-better | low | yes | Strictly more expressive than PHP enums. |
| Better error messages (Elm-style) | terse | caret spans + did-you-mean + `phg explain` | phorge-only | phorge-already-better | high/low | yes | Already Elm/Rust-grade; remaining work is coverage polish. |
| `while-let` | absent | absent | both-absent | defer | medium | partial | Needs a mutating source (iterator/mutation); over a range degenerates to for..in. |
| do-notation / monadic sugar | absent | partial (`?.`/`??`/`opt!`/`\|>`) | both-absent | defer | medium | partial | Presumes a `Result` type; unblock after Result lands. |
| Effect / exception tracking in types | absent | absent | both-absent | defer | low | partial | Lightweight throws-marker subset feasible (erased); full algebraic effects → reject (continuations). |
| Pattern binding everywhere (var/params/for) | partial (list()) | partial | different | defer | medium | yes | Blocked on tuples/records. |
| Opaque return types (impl Trait) | absent | interface return covers ~80% | both-absent | defer/reject | low | yes | Low marginal value; no monomorphization where it pays off. |
| Type classes / trait-bounds | absent | absent (overloading is the locked path) | both-absent | defer | medium | yes | The principled alternative to overloading; large design (coherence). Recorded so the lock is informed. |
| Bounded / constrained generics (`where T: I`) | absent | erased generics, no bounds | both-absent | adopt | high | yes | Checked then erased (same S7a discipline); zero backend change; PHP stays `mixed`. |
| Immutable persistent collections (HAMT/RRB) | absent (flat COW) | flat `Rc<Vec>` | different | defer | low/medium | yes | Internal perf only, invisible to the spine; premature — no functional-update op to optimize yet; std-only HAMT is real effort. |
| General operator overloading (Add trait) | BcMath\Number only (8.4) | absent | different | defer | medium | yes/partial | `a+b → a.add(b)`; interacts with the CTy-operand trap; in tension with explicit-over-implicit. Revisit after method overloading. |
| Tail-call optimization guarantee | absent | MAX_CALL_DEPTH guard | both-absent | reject | low | no | PHP has no TCO → transpiled PHP overflows where Phorge succeeds → breaks the spine. A `@tailrec` lint is the safe sliver. |
| Structured concurrency / async-await | Fibers (8.1) | single-threaded forced | both-absent | reject | low | no | Async scheduling non-deterministic → breaks the spine by construction; M6 green-threads is the roadmapped contract-preserving path. |
| Structural records (shape-based typing) | nominal | nominal | both-absent | reject | low | no | Fights the nominal spine; no clean PHP target. A nominal record with derived structural equality is the compromise. |
| Ownership-lite / borrow hints | absent | absent | both-absent | reject | low | no | Non-problem under the immutable heap; no PHP target. |
| Polymorphic (structural) variants | absent | nominal enums/unions | both-absent | reject | low | no/partial | No idiomatic PHP target; cuts against the nominal model. |
| Gradual / optional typing (any/mixed) | native dynamic | absent by design | php-only | reject | low | partial | Punches a hole in the static + byte-identity story; PHP already IS the gradual target. |
| Declarative / hygienic macros (`macro_rules!`) | absent | absent | both-absent | defer | low/medium | yes | Expand-then-transpile fits the existing discipline; large investment; derive covers the common need. |
| Capability-passing / effect handlers as values | absent (ambient superglobals) | "nothing in the wind" aligned | both-absent | defer | low/medium | partial | Capability-as-value transpiles (a parameter); runtime ENFORCEMENT needs effect typing. |

---

## Track-overlap note (regex, sprintf, date/time, enum-API)

Several high-value features recur across tracks (PHP-core Pass-4, operators Pass-3, beyond). They are consolidated here so the verdict is unmistakable:

- **Regex/PCRE** — `adopt`/high; tier-1 oracle-safe; **the std-only Rust matcher is the gating cost** (one row grades it `defer`/high precisely because the `regex` crate is forbidden — reconcile to *adopt with a hand-rolled subset, full PCRE deferred*).
- **sprintf/`Core.Text.format`** — `adopt`/high; gated on variadics; pin `%F`/`LC_NUMERIC=C` for float determinism.
- **String builders** (`str_pad`/`str_repeat`/`substr`) + **`number_format`** — `adopt`; tier-1, byte-based (no mbstring); 4-arg explicit `number_format` only (reject the locale-default 1-arg).
- **Sorting** (`sort`/`sortBy`) — `adopt`/high; stable, reuses higher-order machinery.
- **Array/Map breadth** (slice/merge/unique/flip/column/chunk/find/any/all) — `adopt`; additive natives.
- **Math breadth** — integer/exact ops `adopt`; transcendentals `adopt` but example-gated to exact values (irrational-float Rust-vs-PHP-14-digit divergence, KNOWN_ISSUES); RNG `reject`.
- **Date/Time** — *pure* arithmetic/format over an explicit (UTC/`gmdate`) timestamp `adopt`/`defer`; **clock reads `reject`** (non-deterministic — the URL/network deferral logic).
- **JSON** (`core.json`) — `defer`/high; needs a dynamic `Json`/`Any` type.
- **Enum behavioral API** — the blanket "enums phorge-already-better" verdict hid real gaps: **methods on enum cases**, **enums implementing interfaces**, **`cases()`/`tryFrom()`** are all `adopt` (backed-enum form is the prerequisite for `from`/`tryFrom`).

---

## Decision Log

> Batched ask-human verdicts recorded here. Each entry: `[YYYY-MM-DD] DECIDED: <feature/cluster> → <verdict> (rationale / sequencing note)`.

### Batch 1 — directional forks (2026-06-21)
- `[2026-06-21] DECIDED: ship-now ergonomics "Wave A" → sequence BEFORE / interleaved with method overloading` (insert as an "S5.5 ergonomics" slice; default+named args lands before overloading is finalized so the overloading design accounts for it — overloading may shrink/reshape as a result, but stays on the roadmap).
- `[2026-06-21] DECIDED: Regex/PCRE → adopt, reduced std-only subset FIRST, full PCRE later` (hand-rolled matcher covering literals/classes/anchors/quantifiers/groups as `Core.Regex`; full PCRE deferred; `regex` crate stays forbidden).
- `[2026-06-21] DECIDED: recoverable errors → BOTH eventually (Result + exceptions)` (build `Result<T,E>` + `?` for in-language flow AND try/catch/throw for PHP interop; sequencing of the two decided later; `?`-over-optionals can ship now, full `Result` waits on generic enums).

### Batch 2 — Wave A core (2026-06-21)
- `[2026-06-21] DECIDED: function ergonomics → adopt as ONE slice` (variadics + default+named args + call-site spread + list-destructuring together; first ergonomics slice).
- `[2026-06-21] DECIDED: sprintf / Core.Text.format → adopt, immediately after variadics` (highest-ROI stdlib win once the enabler exists; pin `%F`/`LC_NUMERIC=C`).
- `[2026-06-21] DECIDED: let-else AND break/continue → adopt NOW` (both; let-else completes S2 null-safety; break/continue over the existing `for..in` — while/do-while loops remain mutation-gated and deferred).
- `[2026-06-21] DECIDED: constants → adopt in Wave A` (module-level `const` AND class constants; compile-time fold → PHP `const`; unblocks typed class constants later).

### Batch 3 — patterns, operators, literals (2026-06-21)
- `[2026-06-21] DECIDED: pattern cluster → adopt as ONE slice` (match guards + payload/struct destructuring + or-patterns + range patterns + list/slice patterns + @-bindings; reuses S4 IsInstance+branch ops, NO new Op; guarded-arm-non-covering exhaustiveness rule).
- `[2026-06-21] DECIDED: operators → adopt ALL THREE` (spaceship `<=>` + bitwise `& | ^ ~ << >>` + exponent `**`; parser disambiguates bitwise from union/intersection type positions).
- `[2026-06-21] DECIDED: literal forms → adopt the lexer batch` (hex/binary/octal `0o`/numeric separators/unicode escapes + compile-time magic constants; REJECT legacy bare-0 octal footgun).

### Batch 4 — collections & stdlib breadth (2026-06-21)
- `[2026-06-21] DECIDED: collections breadth → adopt as a stdlib-breadth wave` (sort/sortBy + array/Map breadth slice/merge/unique/flip/chunk/find/any/all + first/last→`T?`; all on the existing `NativeEval::HigherOrder` path; bundle with the formatting wave).
- `[2026-06-21] DECIDED: foreach over Map/Set → adopt NOW` (the insertion-ordered Map/Set rep makes iteration deterministic + byte-identical; lifts the R1 deferral).
- `[2026-06-21] DECIDED: pipeline-first stdlib reshape → DO IT NOW` (reshape Core.List/Map natives data-LAST to fix the `xs |> List.map(f)` wrong-arity footgun while the stdlib is small; PHP emission unaffected; will touch existing call sites/examples).
- `[2026-06-21] DECIDED: JSON → PRIORITIZE the dynamic Json/Any type design as a near-term milestone` (so `core.json` AND `derive(Json)` unblock sooner; this is an upgrade from "defer indefinitely". Serialize-only could be an interim partial step).

### Batch 5 — attributes & metaprogramming (2026-06-21)
- `[2026-06-21] DECIDED: attributes → FULL PHP-PARITY runtime reflection (incl. dynamic class scanning)` (CORRECTION of the initial inert-passthrough/derive-only proposal, which the developer rightly rejected: "the big ROI of attributes is the runtime effect, to decorate classes"). The runtime-consumed decorate-and-read pattern (Route/ORM/Validate/DI) is the headline. Feasibility: attributes on statically-known types are compile-time-fixed → deterministic → byte-identity-safe; transpiles to PHP `ReflectionClass::getAttributes()->newInstance()`. **Determinism discipline:** Phorge programs are closed (no eval/dynamic require) so the class set is fully known at compile time — even "all classes bearing `@X`" is deterministic, requiring only a CANONICAL iteration order (declaration-order or sorted-FQN, same discipline as the existing sorted `class_implements`) applied identically in both backends + emitted PHP; attribute construction args must be const-expressions. The ONE rejected sliver: `new $runtimeString()` from non-deterministic input. Inert passthrough + closed derive come along as the cheap sub-channels.
- `[2026-06-21] DECIDED: attributes milestone timing → AFTER the OOP slices (S6 extends / S8 traits)` (attributes decorate classes/methods/properties — richest once inheritance + traits exist; passthrough + derive available earlier).
- `[2026-06-21] DECIDED: derive set → ALL FOUR together (Eq/PartialEq, Show/Display, Ord/Compare, Default)` (expand-before-backends codegen → ordinary deterministic PHP; Show doubles as the structured-debug answer).
- `[2026-06-21] DECIDED: structured debug → adopt Core.Debug.dump (Phorge-canonical format)` (identical across run/runvm; NOT mirroring PHP var_export byte-for-byte; cycles impossible on the acyclic heap; pairs with derive(Show)).

### Batch 6 — beyond-PHP differentiators (2026-06-21)
- `[2026-06-21] DECIDED: opaque newtypes + flow refinement → adopt BOTH (full story)` (`type UserId = opaque int;` nominal + erased like Core.Html; `make(s)->T?` smart-constructors; AND flow-sensitive refinement narrowing — the larger design accepted, not just the opaque-wrapper subset).
- `[2026-06-21] DECIDED: sealed hierarchies + bounded generics → adopt BOTH` (sealed = exhaustive match over a named hierarchy, reuses S1 IsInstance; bounds `where T: I` checked-then-erased per the S7a discipline, zero backend change, PHP stays `mixed`).
- `[2026-06-21] DECIDED: Gleam-style use + do-notation → adopt BOTH, after Result lands` (callback-flattening CPS sugar restricted to non-this contexts; do-notation presumes Result; sequence both post-Result).

### Batch 7 — reject re-categorization + familiarity principle (2026-06-21)
- `[2026-06-21] DECIDED: milestone-bound defers → ACCEPTED` (mutation+GC, exceptions, concurrency buckets stay tied to their milestones; not pulled forward; Result+? remains the primary recoverable-error channel ahead of exceptions).
- `[2026-06-21] PRINCIPLE: "familiar to PHP developers; KEEP or UPGRADE unless removal has a solid, specific reason."` (Developer overruled the original ~56-item reject bucket: "I don't see a valid reason to reject — this should be familiar with PHP, only with upgrades and reasonable removal with solid reason.") The reject bucket is re-categorized into three groups (below); **this Decision Log supersedes the inline matrix "reject" verdicts** wherever they conflict — the matrix rows will be reconciled to match.
- `[2026-06-21] PRINCIPLE: MAXIMAL FAMILIARITY` (Developer: "I want maximum familiarity with the new things we will propose in Phorge — like multi-inheritance and overloading etc."). Where a safe form exists, KEEP the familiar PHP syntax rather than only a renamed function — e.g. evaluate keeping the `(int)x` cast *syntax* mapped to a checked conversion. The new Phorge-original features (overloading, inheritance/composition) must also be made to feel PHP-familiar.
- `[2026-06-21] FLAG (revisit at the traits slice): multi-inheritance` — developer named it explicitly. PHP's familiar multi-inheritance substitute is **traits/mixins** (the prior D-L3 decision rejected raw MI in favor of traits+interfaces). Re-open at the S8 traits slice whether traits should provide ergonomic, PHP-familiar multiple-inheritance-like composition.

#### Reject re-categorization (the three groups)

**Group 1 — KEPT, upgraded (familiar capability stays; only the unsafe form changes — NOT a removal):**
`==`/`!=` (kept; only PHP's loose juggling dropped — `0=="foo"` footgun; `===` semantics become the default `==`), type conversions (kept via `Int.parse(s)->int?`; under MAXIMAL-FAMILIARITY also evaluate keeping the `(int)x` *syntax* mapped to a checked conversion), `echo`/`print`→`Console.println`, `require`/`include`→static `import`, `__invoke`→first-class functions, array-`+`-union→explicit merge native.

**Group 2 — DEFER to a milestone (reclassified out of "reject"):**
- *Mutation+GC milestone:* `global`, `static` locals, `unset`, `WeakMap`, static mutable properties.
- *Reflection/attributes milestone* (consistency fix — Batch 5 adopted FULL reflection): `get_class`/`method_exists`/Reflection API, `$obj::class`, the byte-safe subset of dynamic const fetch → become that milestone's compile-time-enumerated, canonically-ordered, deterministic queries.
- *Extension-policy tier-3:* `mb_*`/`intl` unicode/grapheme, distinct `char` type (absent only under the `php -n` oracle).
- *Future std-only natives:* `Core.BigInt` (arbitrary precision), `Core.Process` (shell — a native, never a backtick operator), seedable `Core.Random` / injected `Core.Time` (deterministic behind a seam).
- *Reconsider toward adopt:* anonymous classes (familiar + useful for one-off impls) → defer, not reject.

**Group 3 — GENUINELY REMOVED (~12 items, each documented WHY + the kept capability):**

| Removed | Why (solid reason) | Capability preserved via |
|---|---|---|
| `eval()` | runtime string execution → no static checking, no determinism, security hole | (none — intentionally absent) |
| `&` references / aliasing | breaks the immutable + acyclic heap that the no-tracing-GC design rests on | immutable values + `clone`-with (later) |
| `__destruct` | destruction timing non-deterministic under `Rc`/`Drop` → breaks the byte-identity spine | — |
| `__get`/`__set`/`__call`/`__callStatic` magic | untyped dynamic shapes defeat the static type checker (Phorge's core promise) | typed **property hooks** (PHP 8.4, roadmapped) |
| `@` error suppression | silently hides errors — directly conflicts with the clean-fault / no-hidden-failure model | explicit error handling / `Result` |
| `empty()` | defined purely by multi-type coercion (`0`/`'0'`/`''`/`[]`/`null`/`false`) — the ambiguity *is* the bug | explicit comparisons |
| word-form `and`/`or`/`xor` | documented precedence footgun (`$a = true and false`) | `&&` / `\|\|` |
| `break N` / `continue N` | multi-level numeric jump is a maintenance footgun | **labelled** break/continue (adopted) |
| `goto` / labels | unstructured control flow, fights the analyzable IR (PHP discourages it too) | structured control flow |
| `define()` (dynamic string name) | runtime string-named constant is statically unverifiable | `const` (and `Core.*` registries) |
| `endif`/`endforeach`/`:` template syntax | template-era PHP-ism, zero value in a brace language | braces |
| legacy bare-`0` octal (`0755`) | PHP's own footgun, already superseded | `0o` prefix |

> Net effect: the genuine-removal list shrank from ~56 to ~12; the corpus verdict counts shift accordingly (many former `reject` rows become `phorge-already-better`/Group-1, `defer`/Group-2, or `adopt`). The matrix tables will be reconciled to these groups; until then, **this Decision Log is authoritative.**

---

## Proposed slice re-sequencing (recommendation only)

The locked M-RT order is **method overloading → S6 `extends` (final-by-default) → S8 traits**, with the broader roadmap holding **exceptions**, **mutation+tracing-GC**, **concurrency (M6 green-threads)**, and **attributes** as separate milestones. Given the adopt-high candidates, here is where each best fits. This is a recommendation; nothing is decided until the Decision Log records it.

**A. Ship-now front-end wave (no prerequisite, deterministic, std-only) — insert as a "M-RT S5.5 ergonomics" slice *before or interleaved with* method overloading:**

1. **Default + named arguments** — [Inferred] this should land *before* overloading is finalized: one function with optional/named params eliminates many overload pairs, so the overloading design should be made with this on the table (the dataset repeatedly notes "defaults often REPLACE the need for overloading"). Pure compile-time reorder/fill.
2. **Variadics → then sprintf/`Core.Text.format`** — [Verified] variadics is the stated enabler; sprintf is the highest-ROI stdlib gap that becomes trivial once it exists.
3. **Pattern cluster: match guards + payload/struct destructuring + or-patterns (+ range/list patterns)** — [Verified] all reuse S4's `IsInstance`+branch ops, **no new `Op`**, and are the natural continuation of the just-shipped S4 type patterns. Land as one slice. Guarded-arm-non-covering exhaustiveness rule is the one subtlety.
4. **Numeric literal forms + `1_000_000` separators + `\u{}`** — [Verified] cheapest wins in the corpus; pure lexer; batch them.
5. **`let-else`** — [Verified] completes the S2 null-safety story; lowers to existing if-let + diverge.
6. **Module-level `const` + class constants** — [Verified] compile-time fold; natural for a function-heavy namespaced language.
7. **Pipeline-first stdlib reshape** — [Inferred] fix the latent `|>` data-first footgun while the higher-order stdlib is still small; pure stdlib reshape, PHP emission unaffected.

**B. Regex** — [Verified] the single biggest gap, but the **std-only Rust matcher is real effort**; sequence it as its own focused slice (a reduced literal/glob matcher could land first, full PCRE later). Independent of A and of the locked OOP order.

**C. Sorting + array/Map/Math breadth** — [Verified] rides the existing `NativeEval::HigherOrder` re-entrant-VM path; can land anytime, ideally bundled with the formatting wave for a coherent "stdlib breadth" release.

**D. Attributes milestone** — [Verified] adopt the **inert passthrough + closed-derive** model (reject runtime reflection). Sequence the **derive set (`Eq`/`Show`/`Ord`/`Default`)** early (it's the expand-before-backends discipline Phorge already runs) and the **passthrough channel** (Route/Validation/Deprecated) alongside or after — both are deterministic and unblock real PHP-framework interop. `derive(Json)` waits on core.json.

**E. Tied to the locked OOP slices:**
- With **method overloading**: revisit `E-INTERSECT-SIG`; weigh **default args** (A1) and **type-classes/trait-bounds** (the principled alternative) as recorded context. [Inferred]
- With **S6 `extends`**: unblock `abstract`, `final`, late-static-binding, `static::`, covariance, `#[\Override]`, `static` return type. [Verified — all dataset-tagged as gated on S6]
- With **S8 traits**: unblock trait constants, `final` trait methods, `__TRAIT__`. [Verified]

**F. Tied to deferred milestones (do NOT pull forward):**
- **Mutation+tracing-GC**: compound assigns, `++`/`--`, `??=`, `while`/`do-while`/C-for, static/global state, `clone`-with, property set-hooks, while-let, persistent collections. [Verified]
- **Exceptions**: `try`/`catch`/`finally`, `throw`, Throwable hierarchy, multi-catch — but **`Result<T,E>` + `?` is the recommended primary recoverable-error channel** (Result-first; try/catch as a thin PHP-interop bridge). [Verified — the fork is explicitly resolved Result-first across multiple rows]
- **Generics-as-values / `Json`/`Any` type**: `core.json`, `json_validate`, `derive(Json)`, `serialize`. [Verified]
- **Concurrency (M6 green-threads)**: Fibers, generators, async — reject async-as-language-feature; the green-thread runtime preserves the byte-identical contract. [Verified]

**G. Opaque newtypes / refinement** — [Inferred] high-leverage and a genuine beyond-PHP differentiator that fits the erasure discipline (erases like `Core.Html`); could slot into wave A as an upgrade of transparent `type` aliases to nominal opaque wrappers, with flow-sensitive refinement explicitly deferred.

**Recommended overall sequence:** wave A (ergonomics, ship-now) ∥ C (stdlib breadth) → B (regex, focused) → D (attributes: derive then passthrough) → keep E/F bound to their milestones, with **method overloading still next per the lock, but informed by default-args (A1)**.
