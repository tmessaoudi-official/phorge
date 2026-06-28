# PHP Parity & Beyond — The Definitive Roadmap-Completeness Audit

**Date:** 2026-06-21 (consolidated 2026-06-22) · **Status:** Designed — synthesis of a 20-track
multi-agent gap review. Nothing here is implemented by this document; it is the SSOT the developer
feeds into `ROADMAP.md` / `docs/MILESTONES.md` / `docs/specs/` so gaps stop being discovered ad hoc.

## 1. Purpose & the philosophy lens

This is the **definitive gap audit** of Phorj against (a) PHP 8.0–8.4 parity, (b) beyond-PHP
"upgrade" capability, (c) developer-experience / tooling / ecosystem maturity expected of a 1.0
language, and (d) the cross-cutting correctness, security, stdlib, numerics, i18n, testing, perf,
build/deploy, observability, docs, governance, and competitive-positioning surfaces. It replaces the
earlier Track-A/B-only `php-parity-and-beyond.md` with the merged output of **20 research tracks (A–S,
V)**, each independently completeness-critic'd against the shipped state (`FEATURES.md`,
`KNOWN_ISSUES.md`, `docs/MILESTONES.md`, `ROADMAP.md`, `src/`).

Every candidate is judged by the **Phorj philosophy** — *a pragmatic, legible, provably-correct
upgrade of PHP; the relationship TypeScript has to JavaScript.* Familiarity-first IS the adoption
strategy. Phorj removes **surprises**, never **capability**. Every feature must map to idiomatic PHP
(PHP-absent features are compile-time-only and erased before the backends, preserving the
`run ≡ runvm ≡ real PHP` byte-identity spine). The filter is **"what is the most PHP-familiar, legible,
pragmatic form of this?"** — not "what is the most powerful?". PL-theory maximalism that doesn't earn
its surprise budget is rejected; a great gap is one a PHP dev immediately understands and that makes
their code **provably safer or clearer**.

**Verdict vocabulary.** `kind`: `port` (a PHP feature we lack) / `new` (beyond-PHP) / `map` (concept
maps to a shipped feature or is a transpile-emission/doc refinement) / `omit` (PHP capability
deliberately reshaped). `rec`: **adopt** / **defer** (real, sequenced later) / **reject** (would add
surprise, break the spine, or is PL-theory vanity). `fit`: strong / ok / weak.

## 2. Master triage table (deduplicated across all 20 tracks)

Items that surfaced in multiple tracks are **merged into a single canonical row** with the
cross-listing noted. The canonical ID is kept; the duplicate IDs from other tracks are listed in
*Cross-listed* so nothing is double-counted.

### 2.1 Error handling, control flow, totality

> **DECIDED 2026-06-22 (developer, locked).** The error model is **three tiers**, one enforced-failure
> principle: **(1) `throws E`** — an enforced, *typed* exception declaration (the fix to PHP's
> unchecked `@throws` docblock), checker-enforced at the call site, `?`-propagable, **specific error
> type required** (no bare `throws Exception` swallow), transpiles to **idiomatic PHP exceptions**;
> this is the PHP-familiar *default* surface. **(2) `Result<T, E>`** — error-as-value (functional,
> `match`/`?`), transpiles to a PHP value; for data-flow / `?`-chain code. **(3) unchecked faults /
> panics** — programmer bugs / invariant violations (index-OOB, force-unwrap-null) that *crash* with a
> stack trace (Slice 1), never declared up the call chain (this is the explicit fix to Java's
> "everything is checked" mistake). Both checked tiers are typed + checker-enforced + `?`-composable;
> `throws` erases before the backends (front-end-only ⇒ byte-identity-safe, no new `Op`). `try/catch`
> handles the `throws` surface and the imported-PHP interop bridge. Supersedes the bare "Result-first"
> framing of `B-result`/`A-exceptions` below — they are now the value/exception surfaces of one model.

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| A-exceptions | A | try/catch/finally/throw + exception types | port | strong | adopt | M3 error-slice-2 | L | D-faults-catch, H-result-error-model, E-trycatch-bridge, O-assert-fault(rel) |
| B-result | B,V | First-class `Result<T,E>` + `?` propagation (Result-first; try/catch as PHP-interop bridge) | new | strong | adopt | M3 error-slice-2 | L | A-result-type, V-result-error-model, L-result-type |
| B-qmark-opt | B | `?` propagation over optionals (ships today, no prereq) | new | strong | adopt | M-RT (now) | S | — |
| H-return-totality | H | Return-on-all-paths (missing-return) check — the #1 soundness leak | port | strong | adopt | M-RT (next) | M | H-faultkind-parity-totality |
| H-never-type | H | `never` / non-returning return type | port | strong | adopt | M-RT | S | — |
| H-unreachable-after-return | H | Dead-code-after-terminator diagnostic | port | strong | adopt | M-RT (w/ totality) | S | C-unused-local(rel) |
| H-match-arm-overlap | H | Duplicate / unreachable `match` arm diagnostic | new | ok | adopt | M-RT | S | — |
| D-match-position | D,H | `match` in arbitrary expression position | port | strong | **SHIPPED** (M11) | — | — | H-match-position |
| B-intrinsics | B | `assert`/`unreachable`/`todo`/`panic` correctness intrinsics | new | ok | adopt | M3 (front-end) | S | L-assert-panic, Q-assert, O-assert-stmt |
| B-labeled-break | B,A,V | `break`/`continue` (bare shipped; optionally-labeled `break N`) | port | strong | adopt | M3 (control-flow) | S | A-labeled-loop, A-goto(legit part), V-swift-guard-ergonomics(rel) |
| B-let-else | B,V | let-else / bind-or-diverge (`guard let`) | new | strong | adopt | M-RT (w/ null-safety) | S | V-swift-guard-ergonomics |
| O-contracts | O,B | Design-by-contract `requires`/`ensures`/invariant | new | ok | defer | post-GA / contract slice | M | B-contracts |
| A-fault-cause-chain | D | Fault cause chain (needs error model) | port | ok | defer | M11 (w/ slice-2) | M | D-fault-cause-chain |

### 2.2 OO, classes, types (M-RT track)

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| D-overloading | A,D | Method/ctor overloading → one dispatching PHP method | port | strong | adopt | M-RT (next slice) | M | — |
| D-extends | A,D | Class `extends` (single inheritance, final-by-default) | port | strong | adopt | M-RT S6 | M | A-final-default |
| A-abstract | A | `abstract` classes & methods | port | strong | adopt | M-RT S6 | M | — |
| A-lsb | A | Late static binding (`static::`, `new static`) | port | ok | adopt | M-RT S6 | M | — |
| A-override-attr | A | `#[\Override]` correctness marker | port | strong | adopt | M-RT S6 | S | — |
| D-traits | A,D | Traits / mixins | port | strong | adopt | M-RT S8 | L | — |
| A-class-const | A | Class constants (typed, interface consts, final) | port | strong | adopt | M-RT | M | A-const-expr (shared evaluator) |
| A-const-expr | A | Top-level `const` + constant expressions | port | strong | adopt | M-RT / M11 | S | A-class-const |
| A-magic-stringable | A | `__toString` (Stringable) | port | strong | adopt | M-RT | S | A-arrayaccess(rel), J-string-coerce-interp |
| A-magic-invoke | A | `__invoke` (callable objects) | port | strong | adopt | M-RT | S | — |
| A-magic-clone | A | `__clone` hook for `clone`/`with` | port | ok | adopt | M-mut follow-up | S | — |
| A-readonly | A,D | `readonly` properties & classes (transpile-emit) | map | ok | adopt | M-RT | S | D-readonly-final-emit |
| A-asym-vis | A | Asymmetric member visibility `private(set)` | port | ok | adopt | M-RT | M | D-member-visibility |
| A-backed-enums | A | Backed enums + `from`/`tryFrom`/`cases` | port | strong | adopt | M-RT | M | — |
| A-enum-methods | A | Enum methods + enum-implements-interface + enum consts | port | strong | adopt | M-RT | M | — |
| B-genenums | B,D | Generic enums `enum Result<T,E>`/`Option<T>` | port | strong | adopt | M-RT (generics follow-up) | M | D-generic-enums |
| B-sealed | B,H | Sealed/closed hierarchies → exhaustive match over subclasses | new | strong | adopt | M-RT (post-S6) | M | H-sealed-exhaustive |
| B-newtype | B,K,V | Opaque newtypes / refinement-with-smart-constructor | new | strong | adopt | M-RT or dedicated slice | M | K-secrets-type(⊂), V (refinement) |
| A-iterators | A,B,J | Iterator/IteratorAggregate + `foreach` over user types | port | strong | adopt | M11 | M | B-iter-protocol, J-iter-protocol, L-iteration-protocol |
| A-arrayaccess | A | ArrayAccess / Countable SPL interfaces | port | ok | adopt | M11 | M | — |
| A-named-tuples | A,B | `list()` / array destructuring + minimal tuple type | port | ok | adopt | M3 | M | B-tuples, B-list-destr |
| D-generic-iface-methods | D | Generic interface methods | port | ok | defer | M-RT generics follow-up | M | — |
| D-generic-crosspkg-types | D | Cross-package generic library types | port | ok | defer | M5 follow-up | M | — |
| D-generic-fn-value | D | Generic fn as a first-class value | port | ok | defer | M-RT generics follow-up | M | — |
| B-bounds | B | Generic bounds `<T: Comparable>` | new | ok | defer | post-M-RT | M | D-bounds-variance(part) |
| A-anon-class | A | Anonymous classes `new class { … }` | omit | ok | defer | post-M-RT | M | — |
| A-attributes | A | User attributes `#[Attr]` + reflection read | port | ok | defer | post-M-RT | L | — |
| A-magic-dynamic | A | `__get`/`__set`/`__call`/`__callStatic` | omit | weak | **reject** | — | M | — |
| A-destruct | A | `__destruct` destructor (no deterministic finalization) | omit | weak | **reject** | — | M | — |
| A-references | A | Reference params/assignment `&$x` | omit | weak | **reject** | — | M | — |
| B-variance | B | Declared variance (in/out) | new | weak | **reject** | — | M | D-fn-type-variance, D-bounds-variance(part) |
| D-vis-on-alias-import | D | Visibility keyword on alias / import re-export | port | weak | **reject** | — | S | — |

### 2.3 Pattern matching & narrowing

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| B-guards | B | Match guards (`Circle c if c.r > 0 =>`) | new | strong | adopt | M-RT (post-S4) | S | — |
| B-orpat | B | Or-patterns (`A \| B =>`) | new | strong | adopt | M-RT (post-S4) | S | — |
| B-payload-destr | B | Enum/variant payload destructuring in arms | new | strong | adopt | M-RT (post-S4) | M | — |
| B-struct-destr | B | Structural destructuring (nested fields) | new | strong | adopt | M-RT (post-S4) | M | — |
| B-range-pat | B | Range/literal patterns (`1..=5 =>`) | new | strong | adopt | M-RT (post-S4) | S | — |
| B-at-bind | B | `@`-bindings (bind whole value while destructuring) | new | ok | adopt | M-RT (w/ guards) | S | — |
| B-flow-narrow | B,H,V | Negative/else-branch flow narrowing + union exhaustiveness | new | strong | adopt | M-RT (S4 follow-up) | M | H-instanceof-else-narrow, D-union-flow-narrow |
| V-equality-refinement | V | Equality `==`/`!=` narrowing (TS discriminated narrowing) | new | strong | adopt | M-RT | M | — |
| D-instanceof-intersect-rhs | D | `instanceof (A & B)` right side (lower to `&&`) | port | strong | adopt | M-RT S5 follow-up | S | — |
| H-exhaustive-bool-int | H | Exhaustiveness for bool/finite match without `_` | new | ok | defer | M-RT | S | — |
| D-type-pattern-nested | D | Type pattern nested in a variant payload | port | ok | defer | M-RT unions follow-up | M | — |
| D-union-common-member | D | Common-member access on a raw union | port | ok | defer | M-RT unions follow-up | M | — |
| V-discriminated-unions | V | Literal-tag field on a union of classes | map | ok | defer | M-RT / post | M | — |
| D-whole-union-optional | D | `(A\|B)?` / `(A & B)?` whole-union/intersection optional | port | weak | **reject** | — | M | — |
| B-active-pat | B | Active/view patterns (F#-style) | new | weak | **reject** | — | M | — |

### 2.4 Call convention, operators, syntax ergonomics

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| A-named-args | A,E | Named arguments `f(name: val)` | port | strong | adopt | M3 / M8-prereq | M | E-named-args |
| A-default-args | A | Default parameter values | port | strong | adopt | M3 | S | — |
| A-variadics | A,E | Variadics `...$xs` + spread `...` | port | strong | adopt | M3 / M8-prereq | M | E-variadics |
| A-new-in-init | A | `new` in default-arg / const initializers | port | ok | defer | with A-default-args (M3) | S | — |
| C-numsep | C,A,N | Numeric separators `1_000_000` (+ inside hex/bin) | port | strong | adopt | M3 ergonomics | S | A-numeric-sep, N-numeric-literals, C-numsep-bases |
| C-int-base | C,N | Integer base literals `0x1F`/`0b1010`/`0o17` | port | strong | adopt | M3 ergonomics | S | N-numeric-literals |
| N-float-exponent | N | Float exponent notation (`1e6`, `2.5e-3`) | port | strong | adopt | M-NUM | S | — |
| M-unicode-escape | M | `\u{1F600}` codepoint escape in string literals | port | strong | adopt | M-text S1 | S | — |
| M-string-escapes | M | Complete `\0`/`\e`/`\f`/`\v`/octal escape set | port | ok | defer | M-text S2 | S | — |
| J-string-concat | J | String concatenation operator (PHP `.`) | port | strong | adopt | M-RT | S | — |
| J-spaceship | J,A | Spaceship `<=>` three-way compare | port | strong | adopt | M-RT | S | A-spaceship |
| N-intdiv | N | Integer-division `intdiv`/`divmod` semantics + doc | port | strong | adopt | M-NUM | S | — |
| N-int-conv | N | Explicit numeric conversions (`toFloat`/`toInt`) | port | strong | adopt | M-NUM | S | — |
| A-heredoc | A,E | Heredoc / nowdoc multi-line strings | map | ok | adopt | M3 | S | E-heredoc-nowdoc-import |
| J-pow-operator | J,N | Exponentiation operator `**` | port | ok | defer | M11 / M-NUM | S | N-pow-operator |
| J-bitwise-ops | J,N | Bitwise/shift `& \| ^ << >> ~` (token-collision w/ type ops) | port | weak | defer | M11 / M-NUM-2 | M | N-bitwise-ops |
| J-compound-assign-types | J | Compound-assign result-type rules (`+=`/`??=`/`++`) | port | ok | defer | M-RT | S | — |
| A-strict-types | A | `declare(strict_types=1)` (already strict; emit it) | map | strong | defer | transpile-emit only | S | — |
| A-cast-ops | A | `(int)`/`(string)` cast operators | map | ok | **reject** | — (named conv fns) | S | — |
| A-switch | A | C-style `switch`/`case` (fall-through footgun) | map | strong | **reject** | — (`match` covers) | S | — |
| A-isset-empty | A | `isset`/`empty`/`unset` dynamic predicates | map | weak | **reject** | — (`?`/`??`/if-let) | S | — |
| A-ternary-elvis | A | Ternary `c?a:b` + Elvis `a?:b` | map | ok | defer | — (expr-if/`??` cover) | S | — |
| A-nullsafe-chain-call | A | Nullsafe method-chain (shipped as `?.`) | map | ok | defer | — (`?.` shipped) | S | — |
| A-goto | A | `goto` / unstructured control flow | omit | weak | **reject** | — | S | — |
| A-func-static | A | Function-`static` locals + `global` | omit | weak | **reject** | — | S | — |
| A-compact-extract | A | `compact`/`extract`/variable-variables `$$x` | omit | weak | **reject** | — | S | — |
| J-op-overload | A,B,J,D | Operator overloading on user types | new | weak | **reject** | — | M/L | B-op-overload-derive, D-operator-overload |
| B-derive | B | Derive-style attributes `#[derive(Eq/Show/Ord/Default)]` | new | strong | adopt | M11 / derive slice | L | — |
| B-derive-json | B | `#[derive(Json)]` serialize | new | ok | defer | M11 (after core.json) | M | — |
| V-elixir-pipe | V | Pipe into any arg position (`x \|> f(_, y)`) | new | ok | defer | M-RT / post | M | — |

### 2.5 Semantics, numerics, business data

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| N-decimal | N | Typed `decimal` / money fixed-point primitive (headline) | port | strong | adopt | new M-NUM | L | H-decimal-money |
| N-decimal-rounding | N | Explicit rounding modes for `decimal` | port | strong | adopt | M-NUM | M | — |
| N-datetime-core | N,G | Immutable timezone-mandatory `DateTime`/`Instant` | port | strong | adopt | new M-TIME | L | G-datetime |
| N-duration | N | Typed `Duration` | port | strong | adopt | M-TIME | M | — |
| N-date-civil | N | Civil `Date`/`Time` (no-zone) | port | strong | adopt | M-TIME | M | — |
| J-numeric-tower | J | int↔float coercion rule (auto-widen, documented) | port | strong | adopt | M-RT | M | — |
| J-ordering-rules | J | Total ordering: string `<`, cross-type, enum/bool | port | strong | adopt | M-RT | M | J-eq-asymmetry |
| J-sort-stability | J | Sort/`usort` semantics + stability + comparator contract | port | strong | adopt | M11 | M | J-nan-ordering |
| J-float-eq-lint | J | Float `==` exactness lint (`W-FLOAT-EQ`) | new | strong | adopt | M-RT | S | — |
| J-bool-coercion | J | Truthiness rule — enforced (doc the shipped `E-COND-NOT-BOOL`) | map | strong | adopt | M-RT | S | — |
| J-unicode-model | J,M | `string`=UTF-8 byte-model contract + byte↔char bridge | port | strong | adopt | M11 / M-text S1 | M | M-encoding-contract, M-ascii-divergence |
| N-int-width | N,J | Pin & document `int`=i64 vs PHP platform-width | omit | strong | adopt | M-NUM | S | J-int-width(doc), H-overflow-policy-doc |
| N-float-predicates | N | `isNan`/`isFinite`/`isInfinite` + `NaN`/`Infinity` | port | strong | adopt | M-NUM | S | — |
| N-bigint | N | Arbitrary-precision `BigInt` | port | ok | defer | M-NUM-2 | L | — |
| N-money-currency | N | Composite `Money` (decimal + currency) | new | ok | defer | M-NUM-2 | M | — |
| A-sized-int | A,I,N,H | Sized integers (`i8`…`i64`/`u*`) | new | ok | defer | v2 | L | I-sized-ints, N-sized-int, H-sized-int-overflow |
| N-rational | N | Rational / fraction type | port | weak | **reject** | — | L | — |
| N-percent | N | Percentage helper (userland) | new | weak | **reject** | — | M | — |
| N-overflow-policy | N | Opt-in wrapping/saturating int ops | new | weak | defer | v2 | M | — |
| D-identity-eq | J,D | Identity `===` (`Rc::ptr_eq`) | port | ok | defer | M-mut follow-up | S | J-identity-eq |
| J-hash-contract | J | User-defined hash/key contract for Map/Set keys | port | ok | defer | M11 | M | — |
| D-float-key | D | `float` map keys | omit | weak | **reject** | — | S | — |
| D-map-bool-int-key-coerce | D | Map bool/int-string key coercion vs PHP | defer | weak | **reject** | — (caveat) | S | — |

### 2.6 Mutation, build, packages (follow-ups to shipped milestones)

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| D-nested-place-store | D | Nested place-stores (`this.f[i]=e`, indexed field target) | port | strong | adopt | M-mut follow-up | M | D-field-set-intersection |
| D-property-accessors-backed | D | Backed property hooks + static/iface/abstract hooks | port | ok | defer | M-mut follow-up | M | — |
| D-cycle-collector | D,I | Cycle collector (mutation-created cycles) | port | ok | defer | M11-GC / v2 | L | — |
| P-build-vendor | P,D | `phg build` merges `vendor/` + multi-package projects | port | strong | adopt | M2.5 P3 / M5 | M | D-build-vendor-merge |
| P-build-argv | P,D,G | Built binaries pass argv + exit codes | port | strong | adopt | M2.5 P3 | S | D-build-argv, G-args(rel) |
| P-stub-registry | P | M2.5 Phase 3a prebuilt cross-stub registry | port | strong | adopt | M2.5 P3a | L | — |
| P-strip-meta | P | `--strip`/`--release`/`--debug` + size report | port | strong | adopt | M2.5 P3 | S | — |
| D-build-transitive-deps | D | Transitive dependency resolution (`phg vendor`) | port | strong | adopt | M5 follow-up | M | — |
| D-lambda-lib-pkg | D | Lambdas / fn-values in library packages | port | strong | adopt | M5 follow-up | M | D-crosspkg-fn-value |
| D-transpile-php-builtin | D,E | `package Main` fn-name vs PHP-builtin collision lint | port | strong | adopt | M8 | S | E-transpile-hazard-lint, D-transpile-private-field |
| D-module-qualified-type | D | Module-qualified type form (`Geometry.Point`) | port | ok | defer | M5 follow-up | M | — |
| NS-pascalcase-reshape | (pre-locked, audit-missed) | PascalCase package/folder reshape — `package Main`, `E-PKG-CASE`, manifest `name → module`, lift `E-PKG-TYPE`; **enforced incl. vendor** (PHP/Composer deps case-mapped at the importer boundary, not by exception); maps 1:1 to PHP PSR-4 namespaces | port | strong | adopt | new milestone (breaking codemod) | L | spec `2026-06-20-package-namespace-reshape-design.md` |
| P-codesign | P | Code signing (Authenticode + macOS notarize) | port | ok | defer | M2.5 P3b | L | — |
| P-macos-stub | P | macOS signed stub production | port | ok | defer | M2.5 P3b | M | D-build-macos-stub |
| D-lambda-this | D | Lambda referencing `this` (`E-LAMBDA-THIS`) | port | ok | defer | M-RT / M3 follow-up | M | — |
| D-lambda-block-infer | D | Statement-body lambda return inference | port | ok | defer | M3 follow-up | S | — |
| P-build-bytecode | P | Bytecode (not source) payload in built binaries | defer | ok | defer | v2 | M | — |
| P-pkg-registry | F,P | Hosted package registry (Packagist analogue) | new | weak | **reject** | v2+ | L | F-registry(defer) |
| P-faas | P | Serverless/FaaS deploy adapters | map | weak | **reject** | — | L | — |

### 2.7 Stdlib breadth & batteries

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| L-stdlib-charter | L,G,V | Written stdlib design charter (naming, arg-order, error-vs-optional, tiers, native-vs-`.phg`) | new | strong | adopt | M4 / M9 | S | G-stdlib-namespace, L-stdlib-impl-strategy, L-naming-fix, V-typed-stdlib-product |
| L-list-breadth | A,G,L,N | `Core.List` breadth: query/slice/sort/HO-extras (`sort`/`contains`/`indexOf`/`slice`/`unique`/`find`/`any`/`all`/`zip`/`min`/`max`) | port | strong | adopt | M11 / M4 | M | A-corelist-breadth, G-list-more, G-list-predicates, L-list-*, N-numeric-minmax-breadth |
| L-map-breadth | L,D | `Core.Map`: safe `get`/`getOr`, empty/builder, `insert`/`remove`/`merge`, `map`/`filter`, iteration | port | strong | adopt | M11 / M4 | M | D-empty-map-literal, D-map-iteration, L-map-access, L-map-transform |
| L-set-algebra | L,D | `Core.Set`: union/intersection/difference/isSubset/add/remove | port | strong | adopt | M11 / M4 | M/S | D-set-union-intersect |
| G-json | A,G,L | `Core.Json` encode (now, statically-typed) + decode (needs `Any`) | port | strong | adopt(encode)/defer(decode) | M11 | L | A-json-validate, L-json, L-json-encode, K-deser-safe, B-derive-json |
| G-regex | G,L,M | `Core.Regex` (PCRE `/u`, restricted-subset dual-engine parity) | port | strong | adopt | M11 / M-text | L | L-regex, M-regex, K-redos-constraint |
| L-convert | G,L,N,M | `Core.Convert`/parse: `Int.parse->int?`, `Float.parse->float?`, `toString` | port | strong | adopt | M11 / M4 / M-NUM | M | G-numfmt, N-numeric-parse |
| L-text-breadth | A,G,L,M | `Core.Text` breadth: startsWith/endsWith/indexOf/substring/repeat/pad/reverse | port | strong | adopt | M11 / M4 / M-text S1 | M | G-text-more, M-text-breadth |
| M-codepoint-len | M | Codepoint-aware length & iteration (`chars`/`charCount`) | port | strong | adopt | M-text S1 | M | M-codepoint-int, L-char-ops |
| M-number-format | M,N,A | Non-locale `number_format` (thousands + fixed decimals) | port | strong | adopt | M-text S1 / M-NUM | S | N-num-format(tier1 part), L-numeric-format, A-sprintf(rel) |
| M-ci-compare | M | ASCII case-insensitive compare/search | port | strong | adopt | M-text S1 | S | — |
| A-sprintf | A,G,M | `sprintf`/`printf` checked-subset formatted output | port | strong | adopt | M11 | M | M-string-fmt(defer), L-numeric-format |
| G-math-breadth | G,L,N | `Core.Math` breadth: `round`/`sign`/`clamp`/`gcd`/`log`/`exp`/trig/`PI`/`E`; float `abs`/`min`/`max` | port | strong | adopt | M11 / M-NUM | M/S | G-math-more, L-math-breadth |
| G-console-io | A,G,L | `Core.Console` breadth: `print`/`eprintln`(stderr)/`readLine`/`exit` | port | strong | adopt | M11 / M4 | S | A-print-nonewline, L-console-io |
| A-printf-debug | A,Q | `var_dump`/`var_export`-style structured dump (`Console.debug`/`inspect`) | new | ok | adopt | M11 | S | Q-debug-dump |
| G-base64hex | G | `Core.Encoding` base64/hex (composes with `bytes`) | port | strong | adopt | M11 | S | — |
| G-hash | G,K | `Core.Hash` deterministic digests (sha256/md5/crc32) | port | strong | adopt | M11 | M | K-crypto-stdlib(digest subset) |
| G-path | G | `Core.Path` pure path manipulation (join/base/ext/dir) | port | strong | adopt | M11 | S | K-shell-path-safe(rel) |
| G-url | G | `Core.Url` urlencode/decode + query-string + parseUrl | port | strong | adopt | M6+/M11 | M | — |
| G-csv | G | `Core.Csv` parse/format rows | port | ok | adopt | new M-Batteries | M | — |
| G-file-more | G | `Core.File` breadth: append/delete/copy/lines/tempFile/readBytes | port | ok | adopt | new M-Batteries | M | L-bytes-breadth |
| G-dir | G | `Core.Dir` directory ops (list/make/exists/glob) | port | ok | adopt | new M-Batteries | M | — |
| L-option-combinators | L | `Option`-style combinators over `T?` (no exceptions) | new | ok | adopt | M4 | S | — |
| L-natives-introspect | L,P,F | `phg natives` / `--list-natives` discoverable stdlib | new | strong | adopt | M5 | S | — |
| G-env | G,K,Q | `Core.Env` env/dotenv config (quarantined) | port | ok | adopt | new M-Batteries / M8 | M/S | K-env-config, Q-env-introspect |
| G-args | G | `Core.Args` typed CLI arg parsing | port | strong | adopt | new M-Batteries | M | — |
| G-random | G,K,O | `Core.Random` seedable CSPRNG (deterministic-under-test seam) | port | weak/strong | defer/adopt-seam | new M-Batteries / M8 | M | K-csprng, O-deterministic-seam |
| G-datetime-now | N,G,Q | `Core.Time` clock `now()` (non-deterministic, quarantined) | map | weak | defer | M-TIME-2 / M6 | S | N-now-clock, Q-coretime |
| N-tz-iana | N | IANA tz DB + DST conversions | port | ok | defer | M-TIME-2 / M6 | L | — |
| G-uuid | G | `Core.Uuid` v4/v7 | port | weak | defer | new M-Batteries | S | — |
| G-http-client | G | `Core.Http` outbound client (no std HTTP/TLS, non-det) | new | weak | defer | M6+ | L | — |
| G-db | G,K | `Core.Db` PDO-equivalent (parameterized-only) | port | ok | defer | M6+ / `Core.Sql` | L | K-sql-prepared |
| G-process | G,K | `Core.Process` spawn/exec (argv-array only) | port | weak | defer | new M-Batteries | M | K-shell-path-safe |
| G-log | G,Q | `Core.Log` PSR-3 structured logging | port | ok | adopt | M11 | M | Q-corelog, Q-loglevel |
| G-compress | G | `Core.Compress` gzip/zlib (no std DEFLATE) | port | weak | defer | new M-Batteries | M | — |
| G-crypto-strong | G,K | password hashing / HMAC / constant-time compare | port | ok | adopt(subset)/defer | M8+M11 | M | K-crypto-stdlib, K-timing-safe-eq |
| A-spl-ds | A | SPL data structures (map onto generics) | map | weak | defer | M11 | M | — |
| A-streams | A | Stream wrappers / `fopen` resources | omit | weak | **reject** | — (M6 IO) | L | — |
| L-lazy-seq | L | Lazy iterators / `Seq<T>` generator protocol | new | weak | **reject** | — | L | A-generators(defer), A-fibers(reject) |
| A-generators | A | Generators / `yield` / `yield from` | port | ok | defer | M6 | L | — |
| A-fibers | A | Fibers (stackful coroutines) | omit | weak | **reject** | — (M6 spawn) | L | — |

### 2.8 Concurrency, web, security

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| B-concurrency | B,D | Structured concurrency: `spawn` + channels (green threads) | port | strong | defer | M6 | L | D-concurrency |
| P-frontcontroller-gen | P | Generated PHP front-controller (`phg serve --emit-php`) | port | strong | adopt | M6 W4 | M | — |
| P-phar | P | PHAR output target (`phg package --phar`) | port | strong | adopt | M6/M12 | M | — |
| Q-serve-reqlog | Q | Structured request/access logging on `phg serve` | new | strong | adopt | M6 W4 | M | — |
| Q-serve-health | Q | Health/readiness route helper for `serve` | new | strong | adopt | M6 W4 | S | — |
| K-html-context-escape | K,D | Context-aware escaping (URL/JS/CSS) beyond text+attr | port | strong | adopt | M11 | M | D-html-url-css-script |
| K-header-injection | K | `phg serve` response header-injection / smuggling guard | port | strong | adopt | M8 | S | — |
| K-secrets-type | K | `#[SensitiveParameter]` + `Secret<T>` (trace redaction) | port | strong | adopt | M8 | S | (⊂ B-newtype) |
| K-supply-chain-vendor-min | K | Vendor copy symlink-refusal (mostly shipped; residual) | port | strong | adopt | M8 (P2-#36) | S | — |
| K-security-doc | K | First-class application-developer "Security model" doc | port | strong | adopt | M12 | S | K-int-overflow-story, K-hashdos-immunity, K-artifact-integrity |
| K-fuzz-harness | K,H | Continuous fuzzing of lexer/parser/binary-readers (CI) | new | strong | adopt | M9 (planned M12) | M | H-ev7-fuzz |
| K-sql-prepared | K,G | Parameterized-only SQL (no string-built SQL) | new | strong | defer | M6/M11 `Core.Sql` | L | G-db |
| K-auth-csrf-session | K | Auth/CSRF/session helpers (web layer) | new | ok | defer | post-1.0 (M6) | L | K-csp-headers, Q-serve-metrics-ep(reject) |
| K-redos-constraint | K | Lock ReDoS-safe constraint for future `Core.Regex` | new | ok | defer | M11 / post-1.0 | M | — |
| K-file-capability | K | Root-jailed `Core.File` capability model | new | ok | defer | post-1.0 | L | — |
| K-serve-handler-budget | K | Per-request wall-clock/step budget (busy-loop DoS) | new | ok | defer | post-1.0 (M6) | M | — |
| K-dep-provenance | K | `phg vendor` SBOM-lite provenance (SHA + license) | new | ok | defer | M12 (w/ audit) | S | — |
| K-audit-cmd | K | `phg audit` advisory check of vendored deps | new | strong | defer | M12 | M | — |
| K-taint-tracking | K | Taint tracking (untrusted-string flow) | new | weak | **reject** | — | L | — |
| B-async-await | B,V | async/await (colored functions) | new | weak | **reject** | — | L | V-async-await |
| B-actors | B | Actor model / message-passing isolates | new | weak | **reject** | — | L | — |
| B-effects | B,V | Algebraic effects / Roc-style platform-effects | new | weak | **reject** | — | L | V-roc-platform-effects |
| B-reactive | B | Reactive primitives / signals | new | weak | **reject** | — | L | — |
| Q-tracing-spans | Q | Distributed tracing / OTel spans | new | weak | **reject** | — | L | Q-metrics(defer), Q-serve-metrics-ep |
| Q-panic-shutdown | Q | Crash capture: shutdown/uncaught-fault hook | port | ok | defer | post error-model | M | — |
| Q-reflection | Q | Runtime reflection / introspection API | port | weak | defer | M-RT follow-up / v2 | L | Q-debug-trace |

### 2.9 Tooling, testing, DX

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| C-fmt | C,F,V | `phg fmt` canonical formatter (gofmt model, no options) | new | strong | adopt | M7 / M12 | L | F-fmt, V-go-onboarding-tooling |
| C-fmt-check | C | `phg fmt --check`/`--diff` CI gate | new | strong | adopt | M7 (w/ fmt) | S | — |
| C-lsp | C,F | Language server (`phg lsp` on `check --json` seam) | new | strong | defer | M7 (planned M12) | L | F-lsp |
| C-editor-clients | C,F | VSCode + PhpStorm thin clients | new | ok | defer | M7 (after lsp) | M | F-vscode, F-jetbrains |
| O-test-runner | O,F | Built-in `phg test` runner + discovery | port | strong | adopt | new M-Test / M11 | L | F-test |
| O-assert-lib | O | `Core.Test` typed assertion library | port | strong | adopt | M-Test | M | — |
| O-deterministic-seam | O,G,K | Seedable `Core.Random` + injectable `Core.Time` (test seam) | new | strong | adopt | M-Test (prereq) | M | (= G-random/K-csprng seam) |
| O-table-driven | O | Table-driven / parameterized tests | port | strong | adopt | M-Test | M | — |
| O-fakes-traits | O | Fakes/stubs via interfaces (blessed pattern, doc) | map | strong | adopt | M-Test | S | — |
| O-phpunit-bridge | O,E | Transpiled tests runnable under PHPUnit | map | ok | adopt | M-Test | S | E-phpunit-bridge |
| O-fixtures | O | Setup/teardown fixtures (`setUp`/`tearDown`) | port | strong | adopt | M-Test | M | — |
| O-test-selection | O | Test selection/filtering (`--filter`/tags) | port | strong | adopt | M-Test | S | — |
| O-skip-focus | O | Skip/focus/pending tests | port | strong | adopt | M-Test | S | — |
| O-assert-fault | O | `assertFaults`/`assertThrows` (runner catches fault) | port | strong | adopt | M-Test | M | — |
| O-ci-report | O | Machine-readable test report (JUnit-XML/TAP) | port | ok | adopt | M-Test | S | — |
| O-test-isolation | O | Per-test fresh-state guarantee (doc; static-mut caveat) | new | strong | adopt | M-Test | S | — |
| C-new | C,F | `phg new` project scaffolder | port | strong | adopt | M5/M7 / M11 | S | F-scaffold |
| C-init-config | C,F | `phg init` (manifest + .gitignore in-place) | port | ok | adopt | M5/M7 | S | F-scaffold |
| F-add | F | `phg add` dependency add/resolve (consume side of vendor) | port | strong | adopt | M11 | M | — |
| F-toolchain-pin | F,S | `phorj` version field in `phorj.toml` | port | strong | adopt | M11 | S | S-msrv-policy(rel) |
| C-unused-import | C,H,F | Unused-import lint (`W-UNUSED-IMPORT`) | new | strong | adopt | M3/M8 warning channel | S | H-unused-binding-warn, F-lint |
| C-unused-local | C,H,F | Unused-local / unreachable-code lint | new | strong | adopt | M3/M8 | M | H-unused-binding-warn, F-lint |
| C-api-didyoumean | C | "did you mean" for stdlib/native APIs | port | strong | adopt | M3/M8 | S | — |
| C-explain-fuzzy | C | `phg explain <typo>` did-you-mean for codes | port | strong | adopt | M3 | S | — |
| C-explain-list | C,R | `phg explain --list` browse all codes | port | ok | adopt | M3 / M12 | S | R-error-index |
| C-interp-line | C,D | Fix line=1 reporting inside `"{…}"` interpolation (bug) | map | strong | adopt | M3/M8 | M | D-interp-line1 |
| D-trace-method-fileline | D,Q | Method/ctor/closure frames `file:line` (trace follow-up) | port | strong | adopt | M-faults Slice 1.1 | M | Q-trace-frames |
| C-repl | C,F | `phg repl` interactive shell | new | ok | adopt | M7 (planned M12) | M | F-repl |
| C-fix-it | C,F | Machine-applicable fix-its (`--fix`) | new | ok | defer | M7 (after fmt/lsp) | M | F-lint(--fix) |
| C-doc-gen | C,F,R | `phg doc` API doc generator + doc-comments | new | ok | defer/adopt | M7 / M11 | L/M | F-doc, R-doc-comments, R-stdlib-apidoc |
| C-doctest | C,O,R | Doctests (runnable `///` examples) | new | ok | defer | M7/M9 | M | O-doctest, R-doctest |
| C-watch | C,F | `phg watch` / `check --watch` (std-only polling) | new | ok | defer/adopt | M7 / M12 | S | F-watch |
| C-completions | C,F,P | Shell completions `phg completions <shell>` | new | ok | adopt | M7 / M12 | S | F-completions, P-shell-completion |
| F-playground | F | Web playground (WASM, run+disasm+transpiled-PHP side-by-side) | new | strong | adopt | M12 | M | R-interactive-playground(defer) |
| F-docsite | F,R | Rendered documentation site | new | ok | adopt | M12 | M | R-website(defer) |
| F-citemplates | F | `phg`-aware CI templates (setup-phg action) | new | ok | adopt | M12 | S | — |
| I-regress-gate | I | Perf-regression gate in CI (baseline + ratchet) | new | strong | adopt | M9 | M | I-bench-suite |
| F-installer | F,P | One-line installer / `phgup` version manager | new | ok | defer | v1.1 | M | P-install-script, P-self-update |
| F-debugger | F | Step debugger (Xdebug/DAP analogue) | port | ok | defer | v1.1+ | L | — |
| F-profiler | F | Standalone profiler / flame output | port | weak | defer | v1.1+ | M | I-disasm-cost |
| O-coverage | O,F | Code coverage instrumentation | new | ok | defer | M-Test+2 / M12 | L | F-coverage |
| O-property | O | Property-based testing (`forAll`) | new | ok | defer | M-Test+1 | L | — |
| O-snapshot | O | Snapshot / golden-file testing | new | ok | defer | M-Test+1 | M | — |
| O-mock-reflection | O | Reflection-based mock framework | omit | weak | **reject** | — | — | — |
| O-fuzz | O | `phg fuzz` user-code fuzzing | new | weak | defer | v2 | L | — |
| O-mutation-testing | O | Mutation testing (Infection-style) | new | weak | defer | v2 | L | — |

### 2.10 Performance (mostly invisible, spine-gated)

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| I-str-rc | I | `Rc`-share `Value::Str` (kill string deep-clone) | port | strong | adopt | M-perf | S | — |
| I-isinstance-interned | I | Intern `Op::IsInstance(String)` to an index | port | strong | adopt | M-perf | S | — |
| I-dispatch | I | Faster dispatch (no per-op `Op::clone`) | new | strong | adopt | M-perf | S | I-op-shrink |
| I-constfold | I | Constant-folding compiler pass | new | strong | adopt | M-perf | M | — |
| I-peephole | I | Peephole / dead-code-after-return elimination | new | strong | adopt | M-perf | M | — |
| I-range-lazy | I | Lazy `for`-loop range (don't materialize `0..n`) | new | strong | adopt | M-perf | S | — |
| I-cargo-profile | I | Release-profile tuning (`lto=fat`, `codegen-units=1`) | new | strong | adopt | M9 | S | — |
| I-superinstr | I | Superinstructions (`GetLocal0`, `AddIConst`, fused cmp-jump) | new | strong | defer | M-perf | M | — |
| I-inline-cache | I | Inline caches for method/field/native resolution | new | ok | defer | M-perf | L | — |
| I-intern-symbols | I | Intern field/method/native names to symbol IDs | new | strong | defer | M-perf | M | — |
| I-alloc-stack | I | Allocation reduction (stack reuse, small-vec, string arena) | new | ok | defer | M-perf | M | I-vm-stack-precap |
| I-threaded-dispatch | I | Threaded/computed-goto dispatch (TCO-gated) | new | ok | defer | M-perf | M | — |
| I-fn-inline | I | Function inlining (small/leaf) | new | ok | defer | v2 | L | — |
| I-aot | I | Native AOT compilation | new | weak | defer | v2 | L | — |
| I-ownership-nogc | I | Ownership model removing the GC (narrow the v2 goal, don't build) | new | weak | **reject** | — | L | — |
| B-tco | B,I | Guaranteed TCO (breaks PHP-leg spine) | new | weak | **reject** | — | M | I-tco |
| I-bench-php-real | I | `--vs-php` against OPcache+JIT release build | map | ok | defer | M9 | S | — |

### 2.11 Interop & migration

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| E-importer-stageA | E,F | PHP→Phorj importer Stage A (round-trip own emitted PHP) | port | strong | adopt | M8 | L | F-migrate |
| E-importer-stageB | E | PHP→Phorj importer Stage B (idiomatic typed PHP 8) | port | strong | adopt | M8 | L | — |
| E-decl-files | E | Declaration-file equivalent (`.d.phg`/stub) for untyped deps | new | strong | adopt | new M8.5 (interop) | L | — |
| E-call-composer | E | Call a Composer/PHP library (transpile-time only) | new | strong | adopt | M8.5 | M | — |
| E-migration-report | E | Importer migration report (BETTER/SAME/REJECT verdicts) | new | strong | adopt | M8 | M | — |
| E-incremental-codemod | E,V | Directory-at-a-time codemod CLI (`phg import ./legacy`) | new | strong | adopt | M8 | M | V-incremental-adoption, V-kotlin-interop-posture |
| E-namespace-fqn-interop | E | Map PHP namespace/use ↔ Phorj package | map | strong | adopt | M8 | M | E-psr4-autoload-bridge(defer) |
| E-strict-types-gate | E | `declare(strict_types=1)` as import-eligibility gate | map | strong | adopt | M8 | S | — |
| E-phpdoc-harvest | E | Harvest PHPDoc `@param`/`@return`/`@template` on import | port | strong | adopt | M8 Stage B | M | — |
| E-firstclass-callable-import | E | Map PHP first-class callable → Phorj fn value | map | strong | adopt | M8 Stage B | S | — |
| E-php-version-target | E | `--php-target=8.1\|8.2\|8.3\|8.4` transpile floor | port | strong | adopt | M9/M12 | S | — |
| E-mixed-project | E | Mixed `.php`+`.phg` in one project (`allowJs` analogue) | new | ok | defer | M8.5 | L | — |
| E-raw-php-escape | E | Raw-PHP escape hatch (`php"…"`, transpile-only) | new | ok | defer | M8.5 | M | — |
| E-union-to-enum | E | PHP `T\|U` import mapping (revisit now unions ship) | map | strong | defer | M8 | M | — |
| E-composer-manifest-bridge | E | `phorj.toml` ↔ `composer.json` interop | map | ok | defer | M8.5 | M | — |
| E-stub-distribution | E | Stub ecosystem repo + `phg stub get` (DefinitelyTyped) | new | ok | defer | post-M8.5 | L | — |
| E-importer-stageC | E | Importer of dynamic PHP (eval/var-vars/`__call`) | omit | weak | **reject** | — | — | — |
| E-php-ffi | E | Live PHP-engine FFI / embed PHP in the VM | omit | weak | **reject** | — | — | — |
| E-gradual-checkjs | E | Per-file gradual/optional typing (`mixed` hole) | map | weak | **reject** | — | — | V-error-suppression-stance(inverse) |

### 2.12 Docs, governance, competitive positioning

| ID | Track | Title | Kind | Fit | Rec | Milestone | Effort | Cross-listed |
|----|-------|-------|------|-----|-----|-----------|--------|--------------|
| R-langref | R | Formal language reference doc | port | strong | adopt | M12 | L | — |
| R-tour | R | Guided tour / "the book" | new | strong | adopt | M12 | L | V-onboarding-first-hour |
| R-migration | R,V | PHP→Phorj migration guide (human, concept-mapping) | new | strong | adopt | M12 | M | V-incremental-adoption(doc) |
| R-transpile-contract-doc | R | "How Phorj maps to PHP" per-construct reference | new | strong | adopt | M12 | M | — |
| R-explain-coverage | R | `phg explain` completeness + enforcement test | port | strong | adopt | M9 | S | — |
| R-stdlib-apidoc | R,L | Generated `Core.*` API reference (from registry) | new | strong | adopt | M11 | M | L-stdlib-apidoc |
| R-doc-comments | R | Phorj doc-comment syntax (`/** */`) + checker awareness | port | strong | adopt | M11 | M | — |
| R-getting-started | R,V | 5-minute getting-started page | new | strong | adopt | M12 | S | V-onboarding-first-hour |
| R-grammar-ref | R,S | Published formal grammar (EBNF) | port | ok | adopt | M12 | M | S-frozen-grammar |
| R-cheatsheet | R | One-page syntax cheat sheet | new | strong | adopt | M12 | S | — |
| R-faq-troubleshooting | R | FAQ / troubleshooting (CTy-trap, `V()`, shadow-import) | new | ok | adopt | M12 | S | — |
| S-semver-policy | S,V | Documented semver + stability policy (lang + stdlib) | port | strong | adopt | GA-M12 | S | V-semver-deprecation-policy, R-stability-policy |
| S-breaking-change-def | S | Explicit breaking-change definition (BC contract) | new | strong | adopt | GA-M12 | S | — |
| S-deprecation-policy | S,V | Deprecation policy + `@deprecated`/`W-DEPRECATED` lint | port | strong | adopt | GA-M12 | M | V-semver-deprecation-policy |
| S-frozen-grammar | S | Frozen versioned 1.0 grammar SSOT | port | strong | adopt | GA-M12 | M | R-grammar-ref |
| S-release-automation | S,F,P | Release automation: tags + SHA-256 checksums | port | ok | adopt | GA-M12 | M | F-release, P-release-cmd, P-repro-builds |
| S-changelog-discipline | S | Keep-a-Changelog → versioned release notes | map | strong | adopt | GA-M12 | S | S-upgrading-guide |
| S-conformance-corpus | S | Frozen 1.0 conformance corpus (executable BC guardrail) | new | strong | adopt | GA-M12 | M | — |
| S-diagnostic-code-stability | S | Diagnostic codes declared a stable API | new | strong | adopt | GA-M12 | S | — |
| S-version-provenance | S | Embed git SHA + build metadata in `phg --version` | port | ok | adopt | GA-M12 | S | — |
| S-zerodep-promise | S | Zero-runtime-dependency framed as a stability promise | map | strong | adopt | GA-M12 | S | — |
| S-msrv-policy | S | Documented MSRV + bump policy | port | ok | adopt | GA-M12 | S | F-toolchain-pin |
| S-version-binary-contract | S | `.phorj` container/bytecode compatibility contract | port | ok | adopt | GA-M12 | S | — |
| V-differentiation-vs-php8 | V | Differentiation thesis vs PHP 8.x (+ perf-honesty clause) | new | strong | adopt | GA | S | V-perf-honesty-vs-php |
| V-killer-app-domain | V | Named flagship domain (typed web backends) | new | strong | adopt | GA | M | — |
| V-error-suppression-stance | V | Explicit "no `@`/`any`/authored-`mixed`" stance | new | strong | adopt | GA | S | — |
| V-gleam-error-quality | V | Error-quality as a measured commitment (golden corpus) | new | strong | adopt | M12 | M | — |
| V-naming-branding | V | Resolve the "Phorj" name collision (defer to pre-GA) | new | strong | defer | pre-GA | S | — |
| S-editions | S,V | Rust-style editions mechanism (policy now, build post-1.0) | new | strong | defer | new M13 (post-1.0) | L | V-editions-stability |
| S-rfc-process | S,V | Lightweight RFC / governance-evolution process | port | ok | defer | post-1.0 | M | V-community-governance, S-governance-evolution |
| S-stdlib-stability-tiers | S | Per-API stability tiers (stable/experimental/internal) | new | ok | defer | M11 | M | — |
| V-llm-codegen-affinity | V | Optimise surface for LLM-assisted authoring | new | ok | defer | post-GA | M | — |
| V-corpus-driven-priorities | V | Real-corpus feature prioritisation | new | ok | defer | post-GA | M | V-benchmark-game-presence |
| R-rustdoc-internal | R | Rustdoc on the compiler crate | new | weak | defer | v2 | M | — |
| S-lts-backport | S | LTS / multi-line backport policy | port | weak | **reject** | — | M | — |
| R-versioned-docs | R | Per-release doc snapshots | new | weak | **reject** | — | M | — |
| R-i18n-docs | R | Localized / translated docs | omit | weak | **reject** | — | L | — |
| R-video-tutorials | R | Video / screencast tutorials | new | weak | **reject** | — | M | — |
| V-shapes-vs-records | V | Structural record/"shape" types (vs nominal classes) | new | weak | **reject** | — | L | — |

## 3. Rollup by proposed milestone

This is the section that feeds `ROADMAP.md` / `docs/MILESTONES.md`. Items are listed under their
*primary* proposed milestone; cross-milestone items appear under the earliest.

### M-RT (Rich Types — active) — the immediate language work
- **Next slice:** D-overloading (confirmed next), then S6 (D-extends + A-abstract + A-lsb +
  A-override-attr + A-final-default + D-const-final-enforce), then S8 (D-traits).
- **Totality cluster (land before/with overloading):** H-return-totality, H-never-type,
  H-unreachable-after-return, H-match-arm-overlap.
- **Generics follow-up:** B-genenums → B-result + B-qmark-opt; D-generic-iface-methods,
  D-generic-fn-value, D-generic-enums (= B-genenums).
- **Pattern cluster (post-S4):** B-guards, B-orpat, B-payload-destr, B-struct-destr, B-range-pat,
  B-at-bind, B-flow-narrow (+ V-equality-refinement), D-instanceof-intersect-rhs, B-sealed (post-S6).
- **Class surface:** A-class-const + A-const-expr, A-magic-stringable, A-magic-invoke, A-backed-enums,
  A-enum-methods, A-readonly, A-asym-vis, B-newtype, A-iterators (also M11).
- **Semantics:** J-numeric-tower, J-ordering-rules, J-spaceship, J-float-eq-lint, J-bool-coercion(doc),
  J-string-concat.
- **Ergonomics (M3):** A-named-args, A-default-args, A-variadics, A-named-tuples, C-numsep, C-int-base,
  A-heredoc, B-labeled-break, B-let-else, B-intrinsics.

### M-faults (error handling)
- **Slice 1.1:** D-trace-method-fileline, C-interp-line / D-interp-line1.
- **Slice 2 (the big one, ~L):** A-exceptions **and** B-result decided together — *Result-first,
  try/catch as the PHP-interop bridge*; A-fault-cause-chain folds in.

### M5 follow-ups
- D-build-transitive-deps, D-lambda-lib-pkg / D-crosspkg-fn-value, D-module-qualified-type,
  C-new / C-init-config.

### M2.5 Phase 3 (build/deploy)
- P-stub-registry (3a, fully spec'd), P-build-vendor, P-build-argv, P-strip-meta;
  P-codesign / P-macos-stub (3b, credential-gated, defer).

### M6 (web/concurrency)
- B-concurrency (spawn+channels), P-frontcontroller-gen, P-phar, Q-serve-reqlog, Q-serve-health,
  G-url. Deferred web-security: K-auth-csrf-session, K-serve-handler-budget.

### M8 / M8.5 (interop & migration)
- **M8:** E-importer-stageA/B, E-migration-report, E-incremental-codemod, E-namespace-fqn-interop,
  E-strict-types-gate, E-phpdoc-harvest, E-firstclass-callable-import, D-transpile-php-builtin
  (W-PHP-BUILTIN-NAME), K-header-injection, K-secrets-type, K-supply-chain-vendor-min, G-crypto digests.
- **New M8.5 (interop):** E-decl-files (headline), E-call-composer; defer E-mixed-project,
  E-raw-php-escape, E-composer-manifest-bridge.

### M9 (engineering hygiene / CI)
- I-regress-gate + I-bench-suite, I-cargo-profile, K-fuzz-harness / H-ev7-fuzz, R-explain-coverage,
  H-overflow-policy-doc, H-checker-invariant-audit, C-unused-import / C-unused-local (warning channel),
  E-php-version-target, R-doctest discipline.

### M11 (stdlib breadth) + new M4 (stdlib charter)
- **Charter first (M4/M9):** L-stdlib-charter (naming/arg-order/error-discipline/native-vs-`.phg`/tiers).
- **Collections:** L-list-breadth, L-map-breadth, L-set-algebra, A-iterators / J-iter-protocol,
  L-option-combinators, A-printf-debug.
- **Modules:** G-json (encode now / decode w/ `Any`), G-regex, L-convert, A-sprintf, G-hash,
  G-base64hex, G-path, G-console-io, G-log / Q-corelog, G-math-breadth.
- **API docs / discoverability:** R-stdlib-apidoc, R-doc-comments, L-natives-introspect, F-add,
  F-toolchain-pin. (Note: D-match-position already shipped — sync KNOWN_ISSUES line 65.)

### M7 / M12 (tooling & docs) — decompose the single "Editor/LSP, formatter" bullet
- **Sequence:** M7.1 `phg fmt` (+ `--check`) → M7.2 `phg repl`/`phg new`/`phg init`/`phg completions`
  → M7.3 LSP core (on `check --json`) → M7.4 editor clients → M7.5 `phg doc`/doctests.
- **M12 release/docs:** R-langref, R-tour, R-migration, R-transpile-contract-doc, R-cheatsheet,
  R-getting-started, R-faq-troubleshooting, R-grammar-ref/S-frozen-grammar, F-playground, F-docsite,
  F-citemplates, S-release-automation, K-audit-cmd, K-dep-provenance.

### GA-M12 (governance/stability — all cheap docs)
- S-semver-policy, S-breaking-change-def, S-deprecation-policy, S-changelog-discipline,
  S-conformance-corpus, S-diagnostic-code-stability, S-version-provenance, S-zerodep-promise,
  S-msrv-policy, S-version-binary-contract, V-differentiation-vs-php8 (+ perf-honesty),
  V-error-suppression-stance, V-killer-app-domain, V-gleam-error-quality, K-security-doc.

### New dedicated milestones to create
- **M-NUM** (numerics): N-decimal + N-decimal-rounding, N-numeric-parse, N-math-breadth, N-int-conv,
  N-int-width, N-float-predicates, N-numeric-literals, N-float-exponent, N-intdiv. Defers: N-bigint,
  N-money-currency (M-NUM-2).
- **M-TIME** (date/time): N-datetime-core, N-duration, N-date-civil. Defers: N-tz-iana, N-now-clock (M-TIME-2/M6).
- **M-text** (i18n core): M-codepoint-len, M-encoding-contract/J-unicode-model, M-text-breadth, M-regex,
  M-unicode-escape, M-ascii-divergence, M-number-format, M-codepoint-int, M-ci-compare.
  Defers (Unicode-data, S2/S3): M-normalization, M-unicode-case, M-grapheme, M-segmentation.
- **M-Test** (testing): O-test-runner, O-assert-lib, O-deterministic-seam, O-table-driven,
  O-fakes-traits, O-phpunit-bridge, O-fixtures, O-test-selection, O-skip-focus, O-assert-fault,
  O-ci-report, O-test-isolation. Defers: O-property/O-snapshot/O-coverage; v2: O-fuzz/O-mutation.
- **M-perf** (optimization, behind I-regress-gate): I-str-rc, I-isinstance-interned, I-dispatch,
  I-constfold, I-peephole, I-range-lazy. Deferred: I-superinstr, I-inline-cache, I-intern-symbols,
  I-alloc-stack, I-op-shrink, I-threaded-dispatch.
- **M-Batteries** (impure stdlib, quarantined): G-env, G-args, G-file-more, G-dir, G-csv, G-random,
  G-uuid (defer). Could be folded into M11 if the charter's quarantine tier is clear.

### v2 (native / systems)
- A-sized-int, N-sized-int, I-aot, I-fn-inline, D-cycle-collector (or M11-GC), N-overflow-policy,
  P-build-bytecode. Narrow (do not build): I-ownership-nogc.

### Post-1.0 / new M13 (governance evolution)
- S-editions / V-editions-stability (policy at GA, build at M13), S-rfc-process, S-feature-gating,
  S-stdlib-stability-tiers, V-llm-codegen-affinity, V-corpus-driven-priorities, F-installer,
  F-debugger, F-profiler.

## 4. Top adopt candidates (highest-value, strong-fit, low-regret)

The shortlist the developer should treat as the immediate spine. All are strong-fit, map cleanly to
idiomatic PHP, and most are front-end-only (zero byte-identity risk).

1. **H-return-totality + H-never-type** — closes the *single most important soundness leak* found in
   the whole audit (a `-> T` function can fall off the end, leak `unit`, and detonate at runtime with
   *different* fault messages on each backend). Front-end-only, M effort, the headline correctness win.
   Land before overloading (more paths to reason about).
2. **B-genenums → B-result + B-qmark-opt** — the single highest-leverage *capability* unlock: generic
   enums convert `Result`, true `Option<T>`, and generic ADT containers from three deferred rows into
   shipped features, riding `erase_generics` with no new `Op`. `?` over optionals ships *today*.
3. **A-exceptions / B-result error model (Result-first, try/catch as bridge)** — the largest
   user-visible PHP-parity hole, already roadmapped as fault-slice-2. The competitive evidence (Rust,
   Gleam, Swift) and the philosophy both favour Result-primary with try/catch as the interop concession.
4. **D-overloading → D-extends/A-abstract/A-lsb → D-traits** — the owed M-RT OO slices; daily PHP OO
   building blocks, each lowering to idiomatic PHP (overloading → one dispatching method).
5. **Pattern cluster: B-guards, B-payload-destr, B-flow-narrow (+ V-equality-refinement)** — makes
   ADTs *ergonomic* and the union/narrowing story *total*; the defining TS/Rust capability a PHP-from-TS
   migrant expects. All front-end, no new `Op`.
6. **N-decimal (+ rounding)** — the headline *business* feature: makes float-for-currency a compile
   error, fixing the largest class of real-world PHP money bugs; maps to `brick/math` BigDecimal.
7. **L-list-breadth / L-map-breadth / L-set-algebra (+ L-stdlib-charter first)** — the everyday
   `array_*` muscle-memory surface; reuses the proven generic + HigherOrder-native path, no new `Op`,
   gated by a written charter so the stdlib reads as designed, not accreted.
8. **C-fmt (gofmt model) + C-numsep/C-int-base + C-unused-import/local lints** — the DX trio a
   PHP/TS/Go dev expects on day one; the formatter ends bikeshedding before a community exists, the
   literal/separator lexer wins are pure legibility, the lints ride the existing warning channel.
9. **G-json (encode now) + L-convert (`Int.parse->int?`) + G-console-io (`print`/`eprintln`)** — the
   batteries that turn demos into real programs; encode needs no `Any`, parse gives the *safe* number
   conversion PHP never had.
10. **The GA governance doc-bundle** (S-semver-policy, S-breaking-change-def, S-conformance-corpus,
    S-diagnostic-code-stability, V-differentiation-vs-php8 + perf-honesty, V-error-suppression-stance,
    K-security-doc) — near-zero effort, GA-blocking, and the answer to "why not just use PHP 8.4?".

## 5. Recommended reject / omit (with reasons)

Each fails the philosophy: it adds a *surprise*, breaks the byte-identity/determinism spine, has no
idiomatic PHP target, or is PL-theory maximalism that doesn't earn its budget.

**Dynamic-PHP footguns (defeat static checking — the exact surprise Phorj removes):**
A-magic-dynamic (`__get`/`__set`/`__call`), A-compact-extract (`compact`/`extract`/`$$x`),
A-func-static (function-`static` + `global`), A-isset-empty (truthiness predicates),
A-references (`&$x` aliasing — contradicts the value/handle split), A-cast-ops (`(int)` coercion),
A-switch (fall-through footgun — `match` covers it).

**No deterministic PHP target / breaks the spine:**
J-op-overload / B-op-overload-derive (hidden `$a->__add($b)` action-at-a-distance — derived
`equals`/`compareTo` methods cover the pragmatic slice), B-tco / I-tco (PHP has no TCO → a recursive
program that succeeds under TCO fails under transpiled PHP), B-async-await (coloured functions
contradict uncoloured `spawn`), B-effects / V-roc-platform-effects (continuation capture, no PHP
lowering), B-reactive (hidden mutation graphs), A-destruct (`Rc`/`Drop` has no deterministic
finalization — cycles leak until exit).

**Cannot honor zero-dep + `php -n` oracle:**
M-collation, M-transliterate (need ICU data; no tier-1 PHP approximation). Other ICU-locale features
(M-num-fmt/M-date-fmt/M-msg-catalog) *defer* to a tier-3 extension policy rather than reject.

**PL-theory maximalism (overruns the surprise budget for a PHP audience):**
B-refinement (solver-backed liquid types — newtypes cover the pragmatic slice), B-units (niche;
newtypes cover it), B-typestate (linear/affine typing), B-gadts / B-variance (HKT/declared variance —
erased generics are invariant by design), B-macros (open proc-macros break std-only + the spine —
the *closed* `B-derive` channel is the answer), L-lazy-seq / A-fibers (generators/coroutines fight the
eager-array transpile target), I-ownership-nogc (Rust-style borrow checker — narrow the v2 goal to "a
cycle collector if needed", don't build), V-shapes-vs-records (structural types clash with nominal
identity), O-mock-reflection (reflection mocking — interface fakes are the legible answer),
K-taint-tracking (flow analysis strictly dominated by by-construction `Secret`/`Html`/parameterized-SQL),
Q-tracing-spans / Q-serve-metrics-ep (heavyweight ecosystem machinery, not PHP-core idioms).

**Reverses a deliberate decision / over-scoped for single-dev pre-1.0:**
P-pkg-registry (M5 chose git+vendor+offline — ADR-0005), P-faas (vendor glue), S-lts-backport
(multi-line maintenance contradicts "latest only"), E-php-ffi (drags the dynamic PHP runtime in),
E-importer-stageC (dynamic PHP is un-importable into a closed no-`eval` language),
E-gradual-checkjs (gradual typing punches a hole in the static spine — decl-files + import is the
Phorj answer), R-versioned-docs / R-i18n-docs / R-video-tutorials (premature for a pre-1.0 single-dev
project).

**Clean-rejected corners (parser/purism for a form already expressible):**
D-whole-union-optional (`(A|B)?` — `T?` covers it), D-float-key / D-map-bool-int-key-coerce (Phorj's
distinct-keys behaviour is *more* correct), N-rational / N-percent (userland on `decimal`),
D-vis-on-alias-import (aliases are file-local + erased), B-active-pat (obscure for a PHP audience —
guards deliver the practical subset).

## 6. Cross-track themes (the big recurring ideas, merged across tracks)

These are the patterns that surfaced in 3+ tracks and should be treated as *programmes*, not
scattered rows:

1. **The error model is the keystone fork (A, B, D, E, H, L, O, V).** `try/catch` vs `Result<T,E>` is
   the single most-cross-referenced decision. Synthesis verdict: **Result-first** (the legibility apex
   — errors visible in the type, riding generic enums), with **try/catch as the PHP-interop bridge**
   (imported PHP throws). It unblocks A-fault-cause-chain, O-assert-fault, and the fault-trace work.
2. **Generics-all isn't done until enums are generic (B, D, L).** B-genenums is the lynchpin under
   `Result`/`Option`/`core.json`'s `Any`/typed-container stdlib; the `id(7)+1` operand gap
   (D-generic-result-operand) is the matching *backend* keystone (M10). Both ride `erase_generics`.
3. **Narrowing completeness is the "provably-correct upgrade" made concrete (B, H, J, V).** Else-branch
   flow narrowing + union exhaustiveness + equality refinement + sealed hierarchies are one coherent
   programme that turns Phorj's type system from "checks types" into "proves totality" — the defining
   TS capability a migrant expects. Pair with H-return-totality.
4. **The stdlib must become a *product*, not an accretion (A, G, L, R, V).** A written charter
   (naming, subject-first arg-order, optional-for-absence vs fault-for-programmer-error, determinism
   tiers, native-vs-`.phg` policy) precedes the breadth push; collection breadth + a generated API
   reference (from the registry, single-sourced) is the most-cross-referenced *legibility* win and the
   "Hack HSL was the killer feature" lesson.
5. **Determinism quarantine is the universal mechanism for impure batteries (G, K, N, O, Q).**
   Random, time/clock, env, network, process, logging-to-stderr all break the byte-identity spine the
   same way URL/network did — the M6 `Transport`/quarantine precedent (excluded from
   `differential.rs`, seedable/injectable seam) is the single design pattern that unblocks all of them.
   The same seam that quarantines them also makes user tests deterministic (O-deterministic-seam).
6. **Lexer-only ergonomics are free wins repeatedly missed (A, C, M, N).** Numeric separators, hex/bin/
   octal int literals, float exponent notation, `\u{…}` escapes — all front-end-only, byte-identical by
   construction, pure familiarity, currently *parse errors a PHP dev hits on line one*. Ship as one M3
   ergonomics slice.
7. **Tooling-as-adoption-lever, currently one undifferentiated ROADMAP bullet (C, F, O, P, R, V).**
   The "M7 — Editor/LSP, formatter" line collapses fmt → repl/scaffold/completions → LSP → editor
   clients → doc-gen into one box; it must be *sequenced* (fmt first, it de-risks the AST-printer the
   LSP rename/fix later needs). The `phg test` runner is the biggest missing ecosystem table-stake.
8. **Governance/stability is cheap docs, GA-blocking, and a genuine PHP *upgrade* (R, S, V).** Semver +
   breaking-change definition + a *frozen conformance corpus* (Phorj can state BC *provably* via the
   byte-identity spine — PHP can't) + stable diagnostic codes + a zero-dep promise + an honest
   differentiation-vs-PHP-8.4 statement (don't claim speed). Editions are the long-term jewel but the
   *policy* (not the implementation) is the GA sliver; build editions post-1.0.
9. **Incremental adoption is the whole thesis and is currently implicit (E, V).** The TypeScript-beat-
   Hack lesson: the import direction (`.d.phg` decl-files, codemod, migration report, mixed projects)
   and the Phorj→PHP deploy direction (generated front-controller, PHAR, `--php-target` floor) must be
   first-class, tested, documented workflows — not aspirational single bullets.
10. **A cluster of KNOWN_ISSUES "deferred corners" are really one mechanism each (D, H, J).** The union
    follow-ups (flow-narrow, common-member, nested-type-pattern), the mutation corners (nested
    place-store, intersection field-set), and the transpile hazards (builtin-name collision,
    external-private-field, non-finite-float) each root to a single shared fix — bundle them rather than
    track ~12 independent rows.

---

*Status note (per developer standard): GA ~72% · Global ~58% — this audit is the "stop finding gaps
ad hoc" deliverable; the needle moves most by locking the error-model fork (theme 1) and the M-RT
totality + generic-enums spine (themes 2–3) into the active plan. Both percentages are [Speculative].*
