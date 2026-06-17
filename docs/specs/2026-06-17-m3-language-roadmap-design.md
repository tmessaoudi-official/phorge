# Phorge M3 — Language Enrichment Roadmap & Design Decisions

> Brainstorm output, 2026-06-17. Principle: **fix PHP's real pains; adopt the best of modern
> languages; keep Phorge's wins.** Captures the reject-spine, PHP 8.x verdicts, cross-language
> high-ROI additions, the ROI-ordered slice roadmap, and locked decisions. Each slice ships as its
> own implementation spec + plan, byte-identical on both backends. Living doc — extend as ideas land.
> Refines the M3 section of `ROADMAP.md`. **Draft — under review.**

## 1. Guiding principle

**ROI = (PHP / developer pain addressed) × (ease).** "Ease" is real: every language feature touches
`lexer → parser → checker → interpreter → VM → transpiler` + the differential harness, and an
`Op`-coupled feature hits three exhaustive matches (`vm.rs`, `chunk.rs`, `compiler.rs`). Keep Phorge's
existing wins: static types, immutable-by-default, checked arithmetic, two byte-identical backends,
std-only, `#![forbid(unsafe_code)]`. Reject anything that reintroduces a PHP footgun.

**The transpile contract (D-L9).** Phorge is to PHP what **TypeScript is to JavaScript**: a
statically-typed *superset* that transpiles to **idiomatic PHP**. Every feature maps onto a PHP
mechanism (traits, enums, `match`, nullable, attributes, closures); the few things PHP lacks
(generics) are **compile-time-only and erased** in the output (optionally emitting PHPStan
`@template` docblocks). Prior art: **Hack** (Meta) did exactly this — but required its own VM (HHVM)
and drifted off PHP until it broke compatibility and faded. Phorge's deliberate edge is the bridge
Hack burned: **transpile *to* PHP, require no custom VM, stay runnable as plain PHP forever.**

## 2. Reject spine — PHP misfeatures Phorge will NOT copy

| PHP misfeature | Phorge's better way |
|---|---|
| `==` type-juggling; `strict_types` *opt-in* | always-strict static types, zero coercion — ✅ already |
| `array` = list+map+stack+everything | typed `List`/`Map`/`Set`/tuples (S4) |
| `mixed` / raw dynamic escape hatch | generics + optionals + checked unions; **no raw `any`** (D-L2) |
| `null` returns, `?->`-on-everything (billion-$ mistake) | non-null by default + optionals `T?` (S2, D-L1) |
| `@` suppression; warnings vs errors vs exceptions soup | clean runtime errors + `Result`/`Option` (S6) |
| inconsistent stdlib (`strlen` vs `count`; needle/haystack order) | one consistent, well-named stdlib (S7) |
| magic methods (`__get`/`__call`/`__toString`) — defeat static analysis | explicit methods + traits; no runtime magic |
| `&` references, by-ref `foreach` | immutable-by-default — ✅ already |
| superglobals, `global`, `goto`, `$$var`, variable-variables | none — explicit scope |

## 3. Phorge already beats PHP

Static types (no `==` juggling) · immutable-by-default (no reference bugs) · checked arithmetic (no
silent int→float overflow) · exhaustive enum `match` (vs value-only `switch`) · constructor promotion ·
two byte-identical backends. This is a strong base — these are *closed*.

## 4. PHP 8.x inventory → verdict (condensed, version-annotated)

| PHP feature (version) | Verdict for Phorge | Slice |
|---|---|---|
| `match` expression (8.0) | ✅ have (over enums); extend to expression `if` too | S1 |
| named args + trailing commas (8.0) | adopt (ergonomics) | S1/later |
| nullsafe `?->` + `??` (8.0/7.0) | adopt as `?.` / `??` over optionals | S2 |
| `mixed` (8.0) | **reject** — checked types instead | — |
| enums (8.1, w/ methods + backed) | ✅ have payloaded enums; add enum methods | S5 |
| readonly props/classes (8.1/8.2) | superseded by immutable-by-default + records | S5 |
| first-class callable `f(...)` (8.1) | adopt with lambdas | S3 |
| fibers (8.1) | defer → concurrency | M6 |
| property hooks (8.4) | candidate — get/set without boilerplate (open fork) | S5 |
| asymmetric visibility (8.4) | candidate | S5 |
| **pipe `\|>` (8.5)** ✓verified | **adopt** (F#/OCaml origin) | S3 |
| **`clone with` (8.5)** ✓verified | adopt for record updates | S5 |
| **`#[\NoDiscard]` (8.5)** ✓verified | make `Result` must-use by default | S6 |
| `array_first/last` (8.5) | clean stdlib collection methods | S7 |
| **partial application `?` (8.6, accepted)** ✓verified | consider after lambdas | S3 |
| `clamp()` (8.6) | stdlib math | S7 |
| attributes `#[Attr]` (8.0) | defer (meta-programming) | later |

## 5. Cross-language high-ROI additions

| Feature | Origin | What it buys Phorge | Slice |
|---|---|---|---|
| Data classes / records (auto equality, `copy`, display) | Kotlin, C#, Scala | kills getter/setter/`equals` boilerplate | S5 |
| Extension functions | Kotlin, C# | add methods to any type without subclassing | S7 |
| Comprehensions `[f(x) for x in xs if p]` | Python | intuitive collection building | S3 |
| `guard` / `if let` binding | Swift, Rust | flatten optional/error handling | S2 |
| Smart casts / type narrowing | Kotlin, TS | after `is`/`match`, value auto-typed | S1/S5 |
| Sealed hierarchies | Kotlin, Rust | exhaustive non-enum class trees | S5 |
| `?` error propagation (`f()?`) | Rust | ergonomic `Result` chaining | S6 |
| Type inference (`val`/`:=`) | Kotlin, Go, Rust | concise but static | S0 |
| Tuples + multi-return | Go, Rust, Python | lightweight grouping | S4 |

## 6. ROI-ordered slice roadmap

| Slice | Features | Cost |
|---|---|---|
| **S0 — DX** | per-cmd `--help`+examples, `var` inference, `type` aliases, sharper diagnostics | low (CLI/checker only) |
| **S1 — core ergonomics** | indexing `xs[i]`, ranges `0..n`/`0..=n`, expression `if`/`match`, smart-cast narrowing | med |
| **S2 — null-safety** | optionals `T?`, `??`, `?.`, `if (var x = opt)`, `match`, checked `opt!` | high |
| **S3 — lambdas + pipeline** | first-class fns/lambdas, `.map/.filter/.reduce` and/or comprehensions, `\|>` | high |
| **S4 — typed collections** | `Map<K,V>`, `Set<T>`, tuples + destructuring | high |
| **S4.5 — user generics** | `class Box<T>`, `function first<T>(…)`, optional bounds `T: Comparable` | high |
| **S5 — OOP done right** | interfaces + traits/mixins, records, sealed, static, visibility, enum methods | high |
| **S6 — error handling** | `Result`/`Option` + `?` propagation, must-use returns | med |
| **S7 — stdlib + imports** | consistent `std.string/list/map/math`, single-item import, extension fns, more examples | med |
| **DX milestone** | REPL (`phorge repl`), `phorge fmt`, LSP | — (REPL could jump after S0) |

## 7. Locked decisions

- **D-L1 — null model = Option 3.** Non-null by default; `T?` ≡ `Option<T>`; ergonomics `??`, `?.`,
  `if (var x = opt)` binding, exhaustive `match { Some/None }`, and a **checked** force-unwrap `opt!`
  (clean runtime error on `None`, never UB — Phorge has none). Guardrails: `!` stays loud + greppable,
  a lint nudges `!` → `??`/`if`/`match`, and every `!` failure names the binding + line. The
  force-unwrap is the deliberate escape hatch, *not* the happy path.
- **D-L2 — no raw `any`/`mixed`.** Generics + optionals + checked unions cover the need without the
  footgun. (If ever needed, only a *checked* `Any` you must `match`/downcast out of.)
- **D-L3 — "multi-heritage" = traits/mixins + interfaces, not true multiple inheritance** (diamond
  problem). More reuse power than PHP, without the anti-pattern. **User-confirmed 2026-06-17: "true MI
  becomes traits."** Realized as PHP `trait` + `use` with `insteadof`/`as` conflict resolution
  (= Java default-method semantics); transpiles 1:1 to PHP traits.
- **D-L4 — user-defined generics** (classes + functions, optional bounds) — S4.5. **Erased** in PHP
  output (compile-time checked; optionally emit PHPStan `@template`/`@param` docblocks) — the
  TypeScript / "transpiled generics" model, since PHP has no native runtime generics.
- **D-L5 — adopt PHP 8.5's `|>`** for S3 (align + improve); consider 8.6 partial application.
- **D-L6 — `var` = inferred-but-static local binding** (S0); explicit types still allowed; bindings
  stay immutable.
- **D-L7 — records / data classes** with auto structural equality + display + `clone with` — S5.
- **D-L8 — indexing/range out-of-bounds = clean runtime error**, never PHP's silent null + warning.
- **D-L9 — the transpile contract: Phorge : PHP :: TypeScript : JavaScript** (see §1). Every feature
  transpiles to idiomatic PHP; PHP-absent features (generics) are erased. No custom VM required — the
  edge Hack lacked. This is now a *hard* design filter: a feature ships only with its PHP-mapping
  defined.
- **D-L10 — attributes / annotations** — adopt PHP-native attributes `#[Attr]` (PHP 8.0) for
  declaration metadata; maps 1:1 to PHP. Slotted as a meta feature (likely alongside S5).

## 8. Open forks (resolved per slice)

- **S3** — comprehensions vs `.map/.filter/.reduce` pipeline (or both)?
- **S2** — exact `if let`/`guard` surface; force-unwrap spelling (`!` vs `.force()`).
- **S5** — interfaces+traits surface; record syntax; adopt PHP 8.4 property hooks yes/no.
- **S7** — extension functions: part of the stdlib slice, or their own concern?

## 9. Build order & next step

Design + ship **S0 + S1 + S2** first as one combined implementation spec (they share the
parser/checker surface), then a bite-sized TDD plan, then build task-by-task — each commit green and
byte-identical on both backends. Subsequent slices follow in roadmap order, re-prioritizable by ROI as
real usage reveals what hurts most.
