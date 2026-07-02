# Agent C — Full Decision Register

> Harvested 2026-07-02 from: 66 `docs/plans/*.md` (Decisions Log sections + inline markers), 81
> `docs/specs/*.md` (Decision/D-x/LOCKED markers), `/stack/projects/phorj/CLAUDE.md`, and the 100-file
> memory dir (`~/.claude/projects/-stack-projects-phorj/memory/`). Duplicated records (plan+spec+CLAUDE.md)
> are merged into one row with the primary source cited. The 555-row parity triage
> (`docs/specs/2026-06-21-php-parity-and-beyond.md`) is summarized by category (§ Parity SSOT), with only
> contested/major rows pulled out individually.
>
> **Mode legend:** ASKED = developer explicitly chose (AskUserQuestion / plan approval / recorded
> "developer chose/overruled"). AUTONOMOUS = decided in a `_AUTONOMOUS_3C` / bypass-sentinel session
> without a per-decision ask (incl. "locked at implementation" entries inside autonomous slices).
> RATIFIED = made autonomously, later reviewed & confirmed by the developer (counted with ASKED in
> totals, flagged separately). **Shipped:** ✅ in code · 📐 designed-only · ⬆ superseded · ◐ partial.

---

## 1. Foundational doctrine & process

| ID | Date | Decision | Alternatives rejected | Source | Mode | Shipped |
|----|------|----------|----------------------|--------|------|---------|
| DEC-001 | 06-15 | Three-backend model: tree-walking interpreter + bytecode VM + Phorj→PHP transpiler, gated by a byte-identity differential spine (`run ≡ runvm`) | single backend | specs/2026-06-15-phorj-language-design.md, m2-bytecode-vm-design.md | ASKED | ✅ |
| DEC-002 | 06-17/18 | Transpile contract **D-L9**: Phorj : PHP :: TypeScript : JavaScript — every feature maps to idiomatic PHP; PHP-absent features compile-time-only + erased | features w/o a PHP target | specs/2026-06-17-m3-language-roadmap-design.md | ASKED | ✅ |
| DEC-003 | 06-19 | M7 PHP oracle in the loop: transpiled PHP executed under real `php` must match interpreter stdout; `PHORJ_REQUIRE_PHP=1` fails-not-skips | skip-when-missing | plans/2026-06-19-m7-correctness-closure.plan.md; memory php-leg-outside-correctness-loop | ASKED | ✅ |
| DEC-004 | 06-21 | **Philosophy locked**: craftsmanship (SOLID/patterns/best practice) is the APEX filter — not familiarity, not purism; PHP is the floor, never the ceiling; additive power, never remove capability | familiarity-first; PL-theory purism (both explicitly corrected) | memory philosophy-of-phorge; parity SSOT §1 | ASKED (dev corrected Claude twice) | ✅ doctrine |
| DEC-005 | 06-27 | **Transpile is a bridge, not a runtime**: every feature/native implemented natively on Rust backends; PHP emission is a peer target, never the source of truth; never delegate a capability to PHP | PHP-only implementations (Claude proposed twice, rejected) | memory transpile-is-a-bridge-not-a-runtime | ASKED (hard feedback) | ✅ doctrine |
| DEC-006 | 06-24 | **Language config must be compile-time** (phorj.toml `[language]` / editions → M13); runtime knobs (env/.ini) architecturally rejected — transpiled PHP runs with no Phorj runtime, would silently break byte-identity in prod | runtime env/.ini flag | plans/2026-06-24-language-evolution-master.plan.md; memory config-must-be-compile-time | ASKED | ✅ doctrine (M13 📐) |
| DEC-007 | 06-26 | **Determinism Partition**: every capability is Tier A (pure/deterministic → byte-identity-gated) or Tier B (impure → quarantined via Transport model, fixture-tested); admission decided CASE-BY-CASE, no blanket Tier-B charter | blanket Tier-B charter (dev's own round-1 lean, withdrawn post-challenge) | plans/2026-06-26-native-modules-research.plan.md + extended-scope.plan.md | ASKED | ✅ |
| DEC-008 | 06-26 | Zero-dependency std-only core ("NO TLS, NO regex, NO http/serde crates") as locked framing | — | plans/2026-06-26-native-modules-research.plan.md | ASKED | ⬆ superseded by DEC-009 (see CONFLICTS C-3) |
| DEC-009 | 06-27→29 | **Dependency policy**: narrowly-scoped vetted external deps admitted per-domain — argon2 (crypto), regex (ReDoS-immune matching), ctrlc (signals), corosensei (coroutines); all optional/feature-gated, playground stays dep-free | hand-rolled crypto/regex/unsafe; general-purpose deps (tokio et al. stay disallowed) | specs/2026-06-27-dependency-policy.md; Cargo.toml comments | ASKED (each dep individually authorized) | ✅ |
| DEC-010 | 06-21 | **Autonomy contract**: TOTAL autonomy incl. big architecture, stop+ask only on genuine craftsmanship forks; auto-commit green slices; NEVER push | per-slice checkpoints | memory ga-direction-and-autonomy | ASKED | ✅ standing |
| DEC-011 | 06-17 | **Examples ship with features** (standing rule): every feature lands with a byte-identity-gated `examples/` program + README entry in the same change | retroactive examples | memory examples-ship-with-features; CLAUDE.md | ASKED | ✅ standing |
| DEC-012 | 06-25 | Overnight-session fork protocol: genuine forks logged with provisional call + `⏳ AWAITING CONFIRMATION`, never decided silently; walked next morning | silent autonomous decisions | plans/2026-06-25-overnight-autonomous-session.plan.md | ASKED | ✅ process |
| DEC-013 | 06-28 | **Rename Phorge → Phorj** (reads "forge"; Phorge = active Phabricator fork, SEO/legal collision); `phg` binary + `.phg` extension kept | Clarus/Hone/Hearth/… shortlist; fire-theme names (all collide); keep Phorge | memory name-collision-rename-decision | ASKED | ✅ code (`297229f`); GitHub repo rename + dir `mv` still manual |
| DEC-014 | 06-18 | CLI binary renamed `phorj` → `phg` (ripgrep model: package/lib/env-vars stay `phorj`… then-`phorge`) | — | CLAUDE.md (`70ea75d`); memory binary-renamed-to-phg | ASKED | ✅ |
| DEC-015 | 06-18 | Quality bar for every mapped PHP feature: BETTER / SAME+syntax / SAME / WORSE(reject) — never worse than PHP | — | plans/2026-06-18-m8-php-import-design.md | ASKED | ✅ doctrine |
| DEC-016 | 07-01 | Full-audit shape: audit-first NO code; every recorded decision adjudicated interactively (AskUserQuestion, batches of 4); "100% of the language" = everything ever mentioned, no cutline; CLAUDE.md full rewrite rules-only | — | plans/2026-07-01-full-audit-and-master-plan.plan.md | ASKED | in progress (this register) |

## 2. Namespace / module / package system

| ID | Date | Decision | Alternatives rejected | Source | Mode | Shipped |
|----|------|----------|----------------------|--------|------|---------|
| DEC-020 | 06-18 | **"Nothing in the wind"** — everything namespaced by default, no free-floating globals | globals-by-default | specs/2026-06-18-m3-namespace-system-design.md; memory namespace-system-decisions | ASKED | ✅ (intrinsics gap being closed, DEC-047) |
| DEC-021 | 06-18 | **Go-style module-qualified calls** (leaf-qualified: root in the import, leaf at the call site) | Java `System.out.println` object-path (no idiomatic PHP target, breaks D-L9); 3-segment full paths | same | ASKED | ✅ |
| DEC-022 | 06-18 | Reserved `core.` stdlib root; jargon-free leaves `console` (not io), `file` (not fs), `text` (not string — "avoids shadowing the `string` type") | io/fs/string names | same | ASKED | ⬆ leaves renamed twice (DEC-034, DEC-113; see CONFLICTS C-4) |
| DEC-023 | 06-18 | Bare global `println` **RETIRED**; `println` requires `import core.console;` | prelude/auto-import | same | ASKED | ✅ (name now `Output.printLine`) |
| DEC-024 | 06-18 | Explicit import required even for stdlib | prelude imports | same | ASKED | ✅ |
| DEC-025 | 06-18 | User code **mandatorily packaged**, `package` never inferred — even `-e`/stdin one-liners write `package Main;`; reserved `package Main` = runnable entry (Go model) | inferred packages; PHP/TS optional namespacing | plans/2026-06-18-m5-modules-packages.md | ASKED | ✅ |
| DEC-026 | 06-18 | Native registry keyed by `(module, name)`; one `Op::CallNative(idx, argc)`; `Op::Print` retired; shared `eval` = structural parity (one impl, two callers) | per-native Ops; two print mechanisms | plans/2026-06-18-trackB-stdlib-io-imports.md | ASKED | ✅ |
| DEC-027 | 06-18 | `E-SHADOW-IMPORT`: a value binding may not shadow an imported qualifier (keeps locals-first run-backends and import-map transpiler consistent) | — | same; memory namespace-system-decisions | AUTONOMOUS (impl detail) | ✅ |
| DEC-028 | 06-18 | Manifest = **Composer vocabulary in an honest TOML** (`phorj.toml`, `[require]`, `vendor/package` names); literal `composer.json` REJECTED (a file the composer tool can't process is a false promise); **exact-pin only**, no `^`/`~` ranges | composer.json; version ranges + resolver | plans/2026-06-18-m5-modules-packages.md | ASKED (dev's own kill-shot) | ✅ |
| DEC-029 | 06-18 | Directory=package, strict folder=path (`E-PKG-PATH`), enforcement path-aware in the **loader**, never in `check()`; flat AST merge | enforcement in checker | same | ASKED | ✅ |
| DEC-030 | 06-18 | Cross-package resolution = **loader-side name-mangling to PHP FQNs** before any backend (backends consume rewritten AST unchanged → run≡runvm structural) | backend-aware resolution | same (S2c) | ASKED | ✅ |
| DEC-031 | 06-18 | PHP emission = **single-file brace-namespace blocks** + `\Main\main()` bootstrap | PSR-4 dir tree + Composer autoload (can't autoload free functions; Phorj is function-heavy) | specs/2026-06-18-m5-project-model-design.md; selective-type-import spec | ASKED | ✅ |
| DEC-032 | 06-18 | Library packages export **functions only** (`E-PKG-TYPE`) — interim scope | — | plans (S2c) | ASKED | ⬆ lifted by DEC-036 (planned supersession) |
| DEC-033 | 06-18 | M5 S3: git deps + `phorj.lock` (SHA pin + FNV-1a-64 tree hash) + `phg vendor` = the ONLY network-touching command; run/check/transpile offline-only (`E-VENDOR-MISSING`); guards `E-DUP-DEF`, `E-VENDOR-MAIN` | live fetch on run | plans/2026-06-18-m5-modules-packages.md | ASKED (design 3C-converged) | ✅ (transitive deps deferred) |
| DEC-034 | 06-20 | Stdlib root + leaves become **PascalCase** (`Core.Console`, `Core.Text`…; fn names stay camelCase) | lowercase `core.*` | plans/2026-06-20-m-rt-rich-types.plan.md ("even native core should be PascalCase") | ASKED | ✅ (`c4479d6`) |
| DEC-035 | 06-20 | **Casing is a HARD ERROR for all**: package/folder segments PascalCase (`E-PKG-CASE`), types PascalCase, fns/vars camelCase; no `W-CASE` lint fallback; manifest key `name` → `module`; PascalCase enforced incl. vendor (PHP deps case-mapped at importer boundary) | warn-only lint | plans/2026-06-20-post-wave3-four-tracks.plan.md; parity write-back | ASKED | ✅ (`15a5745`+) |
| DEC-036 | 06-20 | E-PKG-TYPE **lifted**: library packages may declare class/enum/interface, consumed via terminal **`import type Pkg.Path.Type [as A];`**; all three kinds in one commit; codes `E-TYPE-IMPORT-*` | classes-first phasing; module-qualified `Geometry.Point` form (deferred) | specs/2026-06-20-epkgtype-lift-crosspackage-types-design.md | ASKED ("all three at once") | ✅ |
| DEC-037 | 06-20 | Selective type import applies to user/library types ONLY; built-ins stay import-free; **no wildcard** (PHP has no `use A\*`) | `import Core.List.List` | specs/2026-06-20-selective-type-import-design.md | ASKED | ✅ |
| DEC-047 | 07-01 | **No-wind closure** (design-locked, NOT implemented): fault intrinsics `panic/todo/unreachable/assert` move behind mandatory `import Core;`, called `Core.assert(...)` etc. (`E-UNIMPORTED`); deep imports `import Core.A.B.C` any depth binding bare leaf AND parent-qualified; aliasing extended to stdlib+deep; de-reserve `Attr`→Core.Html, `Error`→Core.Error, `Channel`/`Task`→**`Core.Async`** (dev rejected "Concurrent" as misnomer — tasks are cooperative, never parallel) | keep intrinsics in the wind; `Core.Concurrent` | specs/2026-07-01-no-wind-namespace-and-language-surface-design.md | ASKED | 📐 |
| DEC-048 | 07-01 | Import roots: PSR-4-style optional `[packages]` map in manifest; default root `src/` folder=path; first-party bare; `vendor:` prefix for deps | — | specs/2026-07-01-import-roots-psr4-design.md | ASKED | 📐 (spec committed `8fc85f2`) |
| DEC-049 | 07-01 | **Keyword-vs-import 3-way rule**: built-in types (`int float string bool bytes decimal void never`, `List Map Set`, `T?`, fn types, ranges) are keywords NEVER imported; user/library types `import type`; stdlib functions `import Core.X` | force-import of primitives; `Integer`/`Float` wrapper objects (Java-autoboxing anti-pattern) | plans/2026-07-01-m-dogfood-benchmark-marathon.plan.md | ASKED (rejected 2 proposals) | ✅ documented (INVARIANTS) |

## 3. Type system (M-RT) & generics

| ID | Date | Decision | Alternatives rejected | Source | Mode | Shipped |
|----|------|----------|----------------------|--------|------|---------|
| DEC-050 | 06-20 | M-RT scope = **maximal TS-grade type system** (interfaces, instanceof, unions, intersections, erased generics, inheritance, Map/Set, traits) | "coherent cluster only"; defer (Claude's recs, overruled — "put a real effort here") | plans/2026-06-20-m-rt-rich-types.plan.md | ASKED | ✅ (M-RT CLOSED 06-23) |
| DEC-051 | 06-20 | Keyword **`instanceof`** (lowercase, PHP-style); the broken `is` value-equality stub replaced by a real type test with smart-cast narrowing; `is` no longer a keyword | keep `is` as type-test keyword; Claude's dissent to RETIRE `is` entirely (recorded non-binding; dev chose Option 1) | same + plans/2026-06-20-post-wave3-four-tracks.plan.md | ASKED | ✅ |
| DEC-052 | 06-20 | Interfaces: nominal subtyping via one shared `ast::class_implements` consumed by checker+interpreter+VM; `package Main`-only that slice; exact sig match | per-backend duplication | m-rt plan (S2 design) | AUTONOMOUS (impl, inside approved slice) | ✅ |
| DEC-053 | 06-20 | Generics = **fully erased** (no monomorphization), reified-in-checker; call-site first-binding-wins `unify`; `Type::Erased` rewritten pre-backend at the `check_and_expand` chokepoint | monomorphization | m-rt plan (S7a) | ASKED (approach) / AUTONOMOUS (details) | ✅ |
| DEC-054 | 06-20 | **Generics reach = ALL** — free fns + methods + classes + (later) enums | free-functions-only | m-rt plan ("I want generics all options") | ASKED | ✅ |
| DEC-055 | 06-20 | Generic classes: inference-only construction (`Box(7)`, no `Box<int>(7)` turbofish), invariant, no bounds | explicit type-arg syntax | specs/2026-06-20-generic-types-classes-design.md | AUTONOMOUS | ✅ |
| DEC-056 | 06-20 | S4 unions: **D1 primitive members allowed** (`int\|string`); **D2 one big S4** (unions + match-over-union together); **D3 fully autonomous**; `Pattern::Type` reuses `Op::IsInstance`; lone `Circle =>` stays a catch-all binding (footgun deliberately preserved) | enum members (deferred); S4a-only split | specs/2026-06-20-s4-union-types-design.md | ASKED (D1–D3) / AUTONOMOUS (details incl. footgun) | ✅ |
| DEC-057 | 06-21 | S5 intersections: **D1 = ≤1 concrete class + N interfaces** (dev overruled Claude's interface-only rec — correctly); `E-INTERSECT-MULTI-CLASS` for ≥2 classes; **D2 = require-agreement `E-INTERSECT-SIG`** (revisit when overloading lands) | interface-only members; first-member-wins conflict rule | m-rt plan; specs/2026-06-20-s5-intersection-types-design.md | ASKED (2 challenge rounds) | ✅ (D2 revisit still open — see CONFLICTS C-8) |
| DEC-058 | 06-21/22 | **Method overloading confirmed** (dev explicitly rejected "stay PHP-aligned / don't add it": "this language should be equal or better than PHP"); lowers to ONE dispatching PHP method; compile-time unambiguous, most-specific-wins, `T?`≠`T` | no overloading (PHP parity) | m-rt plan; memory ga-direction-and-autonomy | ASKED | ✅ |
| DEC-059 | 06-28 | **Return-type overloading**: overloads may differ only in return type; resolved from a SHALLOW/direct sink set; `<type>f(...)` selector (distinct from `as` cast); `E-OVERLOAD-AMBIGUOUS-RETURN`/`-SELECT-CONFLICT`/`-NO-CONTEXT`; dev conceded `discard <int>f()` valid | — | plans/2026-06-28-ga-marathon-super-overloading.plan.md | ASKED | ✅ |
| DEC-060 | 06-22 | **Totality cluster**: return-on-all-paths `E-MISSING-RETURN` + `never` bottom type + `W-UNREACHABLE` + `W-MATCH-UNREACHABLE`, all front-end-only, sequenced FIRST in M-RT (before overloading) | — | specs/2026-06-22-totality-cluster-design.md; parity triage | ASKED (ordering) / AUTONOMOUS (execution) | ✅ |
| DEC-061 | 06-22 | Generic enums `enum Option<T>` / `Result<T,E>` mirroring Box machinery, zero backend change | — | plans/2026-06-22-generic-enums.plan.md | AUTONOMOUS | ✅ |
| DEC-062 | 06-22 | **S6 = multiple inheritance, Model 1 explicit-resolution** (`class C extends A, B`; cross-parent collision = compile error unless resolved); Model 3 (C3 + cooperative super) deferred to post-S8 gated milestone; `super`/`parent` under multiple parents = clean error `E-MI-SUPER-AMBIGUOUS` (forward-compat reservation) | single-`extends`-only + traits framing (dev rejected twice); C3 linearization now | specs/2026-06-22-s6-multiple-inheritance-design.md | ASKED | ✅ (reverses D-L3 — see CONFLICTS C-1) |
| DEC-063 | 06-22 | **Final-by-default + `open`** (Kotlin model); `final` keyword retired as redundant — internal consistency with immutable-by-default beats PHP-familiar open-by-default | PHP open-by-default | same spec | ASKED | ✅ |
| DEC-064 | 06-23 | **S8 traits**: reuse-only NOT a type (`use T`; `instanceof T` rejected); members carry visibility+mutability; **maximal D4** (ctors, static state, hooks, const, abstract requirements — all supported); every PHP-fatal/silent trait footgun becomes an ahead-of-time diagnostic (D5); trait-ctor shadowing warnings D6/D8 | trait-as-type; minimal trait subset | specs/2026-06-23-m-rt-s8-traits-design.md (D1–D8) | ASKED (challenge round + PHP 8.4 evidence) | ✅ |
| DEC-065 | 06-21 | **Mutation model**: immutable-by-default, keyword **`mutable`** (not `mut`); 4 orthogonal axes mutable/const/static/open; `final`/`readonly` eliminated as value modifiers; value/handle split — List/Map/Set/Bytes = deep-frozen COW values, Instance = shared-mutable handle; **no tracing GC** (Rc/Drop suffices; acyclic) | `mut`; readonly modifiers; tracing GC | specs/2026-06-21-mutation-milestone-design.md; memory mutation-milestone, ga-direction-and-autonomy | ASKED (Claude challenged, dev agreed) | ✅ |
| DEC-066 | 06-27 | **`this.field` everywhere** — bare field access is `E-BARE-FIELD` (BREAKING, PHP-faithful); `E-STATIC-THIS` in statics | implicit field resolution | memory decision-review-and-9-fixes (`53dc203`) | ASKED (decisions-review) | ✅ |
| DEC-067 | 06-21 | Visibility: public/private/protected enforced in the checker across six access surfaces; parity hole closed later with `E-FIELD-VISIBILITY`/`E-METHOD-VISIBILITY` | runtime-only enforcement | specs/2026-06-21-visibility-modifiers-design.md; plans/2026-06-25-full-bidirectional-php-support.plan.md | ASKED | ✅ |
| DEC-068 | 06-22 | **Error model = three tiers**: enforced typed `throws E` (PHP-familiar default; specific type required) + `Result<T,E>` value surface + unchecked faults/panics for bugs; `try/catch` discharges `throws` + PHP-interop bridge | Result-first-only (Claude's rec); Java checked-everything | parity SSOT §2.1; plans/2026-06-21-roadmap-completeness-review.plan.md | ASKED (dev extended the rec; reconciled via challenge) | ✅ (slice 2 closed; 3 new Ops) |
| DEC-069 | 06-28 | `super`/`parent` dispatch via `Op::CallParent`; must-use returns + `discard` contextual keyword (`E-UNUSED-VALUE`) | `void f()` C-style discard | plans/2026-06-28-ga-marathon-super-overloading.plan.md | ASKED (order + scope Option 1) | ✅ |
| DEC-070 | 06-29 | Soundness Batch B: same-head generic types made truly **invariant** at assignment (`Box<string>` rejected where `Box<int>` expected) — closing a known M-RT gap | — | CLAUDE.md; memory m-rt-progress | AUTONOMOUS (marathon) | ✅ |

## 4. Language surface & syntax (evolution decisions)

| ID | Date | Decision | Alternatives rejected | Source | Mode | Shipped |
|----|------|----------|----------------------|--------|------|---------|
| DEC-080 | 06-17 | S0 DX: `var` local inference; `type` aliases expanded out pre-backend; diagnostics with codes + `phg explain` | — | specs/2026-06-17-m3-slice1-s0-s1-s2-design.md | ASKED | ✅ |
| DEC-081 | 06-17 | S2 null-safety suite: `T?` optionals w/ compile-time non-null guarantee; `??`; `?.`; if-let + smart-cast; `opt!` + `W-FORCE-UNWRAP`; warning channel = **stderr, non-fatal, all commands** | separate `Ty::Null` variant | plans/2026-06-17-m3-s2-null-safety.md | ASKED (channel via AskUserQuestion) | ✅ |
| DEC-082 | 06-18 | S3 lambdas: expr-body infers, block-body explicit return; capture by value; pipe `x \|> f ≡ f(x)` lowered in parser (no new Op) | — | specs/2026-06-18-m3-s3-lambdas-pipe-design.md | ASKED | ✅ (later `fn`→`function`, DEC-113) |
| DEC-083 | 06-24 | **Mandatory `new` EVERYWHERE** — classes AND enum variants (`new Some(7)`); one-rule uniformity | `new` for classes only (Claude's rec, overruled) | specs/2026-06-24-mandatory-new-design.md | ASKED | ✅ (`5fb1259`) |
| DEC-084 | 06-24 | `const` class constants: literal-only v1, inherited, inlined on Rust backends → PHP typed const; SCREAMING_SNAKE; const-of-const + interface constants deferred | — | plans/2026-06-24-new-const-fieldinit.plan.md | ASKED (accepted all recs) | ✅ (`c6b1ac2`) |
| DEC-085 | 06-24 | Expression field initializers (instance + static); statics **EAGER once at program start, declaration order, before main**; may read `this` + earlier siblings (forward-ref = error); lazy `??=`-on-first-access rejected; runtime config rejected (→ DEC-006) | lazy init; runtime knob | plans/2026-06-24-language-evolution-master.plan.md; specs/2026-06-24-member-initializers-design.md | ASKED | ✅ |
| DEC-086 | 06-24 | No-value types: `void` (uncapturable) + `Empty` (holdable), `void <: Empty` | single unit type | language-evolution master plan | ASKED | ✅ then reshaped (`Empty`→`empty`, DEC-113) |
| DEC-087 | 06-24 | **UFCS general, method-first** (method → user free fn → any *imported* native by first-param unify) | rigid type→module map | plan + overnight fork F-001 | ASKED (adopt) / RATIFIED (mechanism F-001) | ✅ |
| DEC-088 | 06-24 | Return-type mandate: named fns + methods + statement-body lambdas annotated; **expression-body lambdas keep inferring** (dev's "Option 2?" instinct challenged and reversed — `=>` can't fall off the end) | annotate everything | language-evolution master plan | ASKED | ✅ |
| DEC-089 | 06-24 | Perimeter verdicts: string `+` ✅; `**` + `Math.ipow` both ✅; or-patterns instead of `switch` (reject); `s[0]` → defer M-text; single-quotes ❌; `<=>` ❌; PHP `.` concat ❌; tuples defer (classes now); let-destructuring full + `else`; fixed-length `[T; N]` adopt; `\u{}` pull forward; this-capture build; decimal/BigInt → M-NUM | — | specs/2026-06-24-language-ergonomics-perimeter-design.md | ASKED (item-by-item) | ✅ mostly (`[T;N]` see plan) |
| DEC-090 | 06-24 | **Ternary `? :` DEFERRED, not rejected** — postfix-`?` collision + third meaning of `?`; expression-`if` already covers the capability | adding it now (the same-day perimeter record said "✅ add" — superseded within the day; see CONFLICTS C-5) | language-evolution master plan | ASKED | not shipped [Verified: `? :` is a parse error in current `phg`] |
| DEC-091 | 06-24 | Literal braces: BOTH `\{`/`\}` escapes AND raw strings `r"…"`/`r#"…"#` (lexer-side interpolation split) | parser-side split (can't distinguish `\{`) | introspection-strings-process design | ASKED | ✅ |
| DEC-092 | 06-24 | Reflection: full name-level read-only introspection now (typeName/className/hierarchy/member names); dynamic-dispatch + attribute reflection rejected; **no ambient superglobals ever** (env/args → M-Batteries; request → M6 typed Request; `$_REQUEST` rejected) | deferred reflection; ambient superglobals | specs/2026-06-24-introspection-strings-process-design.md | ASKED (challenge upheld) | ✅ |
| DEC-093 | 06-25 | **A-1: `: T` return syntax; `->` fully retired**; typed lambdas TS-identical (`fn(int x): string => …`) | keep `->` | plans/2026-06-25-php-fidelity-and-divergence-audit.plan.md | ASKED | ✅ |
| DEC-094 | 06-25 | **A-6: `foreach (coll as BINDING)` adopted to REPLACE `for (x in coll)`**; one keyword `as`; 4 binding forms; optional `with int i` counter; `of`/`in` rejected as synonyms | keep `for in`; `of` keyword | same plan | ASKED | ◐ shipped **alongside** for-in, not replacing (see CONFLICTS C-2) |
| DEC-095 | 06-25 | **A-3: type-first params KEEP** (`(int name)` = PHP-minus-sigil) | TS name-first `name: int` | same plan | ASKED | ✅ |
| DEC-096 | 06-25 | **A-46: `++`/`--` allowed as EXPRESSIONS** (dev overruled Claude's statement-only KEEP after full hazard briefing); eval order pinned to PHP left-to-right; `W-SEQUENCE-MUTATION` lint sweetener | statement-only | same plan; specs/2026-06-26-m3-stream1-syntax-reshape-design.md | ASKED (overruled) | ✅ |
| DEC-097 | 06-25/26 | Strings: two modes `"…"` (interpolating) + `r"…"` (raw); PHP `'…'` rejected; **A-62 `"""…"""` auto-dedent text blocks adopted** (Java-style trailing-strip, interpolating, purely additive); `{w}` interpolation delimiter KEEP (A-7; `${w}`/`{$w}` rejected — reintroduce the sigil) | single quotes; `${}` | same plan | ASKED | ✅ |
| DEC-098 | 06-25 | **A-61: `instanceof` stays lowercase** — universal cross-language convention beats camelCase-consistency | `instanceOf` | same plan | ASKED | ✅ |
| DEC-099 | 06-25 | Transpile fidelity: B-1 per-hole native PHP `"{$…}"` interpolation with EXHAUSTIVE hole-kind classification (dev requirement); B-2 `println` → `echo X, "\n"` (`printf` rejected — literal `%` corruption risk); B-9 minimal `$` escaping | printf; blanket concat | same plan | ASKED | ✅ |
| DEC-100 | 06-26 | **Keep `var`, make it CONTEXTUAL** — all four declaration forms stay; the real bug was hard-reservation, not the spelling (supersedes the same-day "retire `var`" agreement after research on Hack/Haxe + philosophy re-read) | retire `var`; `let`=immutable; keyless synthesis; Go `:=` | plans/2026-06-26-retire-var-declaration-reshape.plan.md (two logs) | ASKED (reversal recorded) | ✅ |
| DEC-101 | 06-26 | Default parameters: `param: T = <literal>`, trailing-only, literal-only, front-end call-fill (no backend change) | — | plans/2026-06-26-default-parameters.plan.md | ASKED | ✅ |
| DEC-102 | 06-26 | Idea-backlog batch 1: no top-level execution in project files (A); optional `main(args: List<string>): int`, no `argc` (B); `handle(Request) -> Response` reserved web entry (C); **`length` for ordered / `size` for keyed** collections, hard rename no alias (D) | PHP-style top-level code; `argc` | plans/2026-06-26-developer-idea-backlog.plan.md | ASKED | ✅ |
| DEC-103 | 06-27 | **Class entry points: BOTH forms allowed** — top-level `main`/`handle` OR `static` class method (dev overruled Claude's "top-level only, Java-ism" challenge); `E-MULTIPLE-MAIN` on ambiguity | top-level only | specs/2026-06-27-class-entry-points-design.md | ASKED (overruled) | ✅ |
| DEC-104 | 06-27 | `as` operator → checked cast to primitives (`value as Type` ⇒ `Type?`); `as` contextual; casting system = mix (Core.Convert + `as` + UFCS), TS `<X>` assertion axis separated from value conversion | C-style `(int)x` cast (the PHP surprise) | plans/2026-06-26-m4-stdlib-breadth.plan.md; memory as-primitives-and-crypto-session | ASKED (spec-first) | ✅ |
| DEC-105 | 06-30 | B1 iteration protocol: for-in over string/Map (two-binding) + `List.enumerate`; `zip` deferred to B3 | — | memory session-naming-and-b1 | AUTONOMOUS (within approved marathon) | ✅ |
| DEC-106 | 07-01 | Dogfood W0/W2: empty-list literal init; comma-throws; nested-quote interpolation; list upcast | — | memory marathon-m-dogfood | AUTONOMOUS | ✅ |
| DEC-107 | 07-01 | **Q1 dynamic dispatch: NO string-instantiate/string-call primitive** (un-typeable/un-erasable); ADD method-references-as-values (`obj.method` → typed closure) + typed-registry guide | PHP `new $class`/`$obj->$m()` | no-wind spec §context; four-lane plan | ASKED | 📐 |

## 5. Naming & renames

| ID | Date | Decision | Alternatives rejected | Source | Mode | Shipped |
|----|------|----------|----------------------|--------|------|---------|
| DEC-110 | 06-20 | Stdlib API camelCase fn names (`split_once`→`splitOnce` etc.) with the casing hard-error slice | snake_case | post-wave3 plan | ASKED | ✅ |
| DEC-111 | 06-26 | Core.Json enum variants PHP-reserved-name mangling in transpiler only (`Int`→`Int_`…), API stays clean | `J`-prefixed API | plans/2026-06-26-autonomous-backlog.plan.md | ASKED | ✅ |
| DEC-112 | 06-29 | `Channel.new()` → `Channel.create()` (`new` became a keyword token); `Task`/`Channel` reserved forcing example `class Task`→`Parcel` rename | — | big-marathon plan | AUTONOMOUS (forced) | ✅ |
| DEC-113 | 06-30 | **Full naming overhaul (clarity / no-shortcut)**: lambda `fn`→**`function`**; `Empty`→lowercase **`empty`** (union-able; `void` rejected in unions → `E-VOID-IN-UNION`); Result `Ok`/`Err`→**`Success`/`Failure`**; `recv`→`receive`; CLI `fmt`→`format`, `bench`→`benchmark`, `disasm`→`disassemble`, `lex`→`tokenize`; packages `Console`→**`Output`**, `Text`→**`String`**, `Validate`→`Validation`, `Convert`→`Conversion`, `Reflect`→`Reflection`, `Crypto`→`Cryptography`; new `Core.Environment`; ~20 native renames (println→printLine, upper→uppercase, div→divide, args→arguments, next→nextInt, millis→milliseconds, url-encode family…); KEPT: math notation, acronyms, `of` factories, Task/Channel (Thread & Observable rejected) | Thread/Observable; Unit; Console/Out | specs/2026-06-30-naming-overhaul-design.md; memory naming-overhaul-decisions | ASKED (exhaustive review) | ✅ (unpushed); Lane-1 leftovers done 07-01 |
| DEC-114 | 06-28 | Name **Phorj** locked (see DEC-013) — this row records that the *prior* 06-21 decision was "keep Phorge for now, rename before GA" (superseded) | rename immediately (06-21); Phurnace | memory name-collision-rename-decision | ASKED | ⬆→✅ |

## 6. Runtime, VM, performance

| ID | Date | Decision | Alternatives rejected | Source | Mode | Shipped |
|----|------|----------|----------------------|--------|------|---------|
| DEC-120 | 06-16 | P4 object model **A — value-native** (reuse shared `Value::Instance`/`Enum`, clone-on-use); arena/handle deferred bench-gated | arena/handle model | plans/2026-06-16-m2-p4-classes-enums-match.md | ASKED | ✅ then evolved (P5a) |
| DEC-121 | 06-16 | P5a: `Rc`-share Instance/Enum/List (2.4×); **Phase B slot-indexed layout bench-gated, unopened**; slab-arena rejected (no locality evidence) | slab arena | plans/2026-06-16-m2-p5a-rc-shared-heap.md | ASKED | ✅ (slot-indexed later shipped in 06-28 marathon when evidence arrived) |
| DEC-122 | 06-16 | Wave 4 before P5 (correctness gap outranks bench-gated perf); class-aware `CTy` derived structurally from AST annotations | threading checker `Ty` into compiler | plans/2026-06-16-m2-wave4-compiler-types.md | ASKED | ✅ |
| DEC-123 | 06-17 | No tracing GC in M2 — Rc/Drop reclaims the immutable+acyclic heap fully; tracing deferred to a mutation milestone (then permanently mooted by COW value semantics) | mark-sweep GC (original M2 criterion, revised) | CLAUDE.md; memory mutation-milestone | ASKED | ✅ |
| DEC-124 | 06-18 | `Op` discipline: any new Op extends exactly three coupled matches (`exec_op`/`validate`/`stack_effect`) same commit; "no new Op" default for front-end features | — | docs/INVARIANTS.md; memory op-variant-match-coupling | ASKED (standing) | ✅ |
| DEC-125 | 06-20 | Higher-order natives = **`NativeEval` enum (Pure \| HigherOrder)** + backend-supplied closure invoker; VM gains re-entrant `run_until`/`call_closure_value`; no new Op | backend intrinsics; dedicated Ops | m-rt plan; memory higher-order-natives-reentrant-vm | ASKED | ✅ (later + `Reflective`) |
| DEC-126 | 06-20 | S3 Maps: insertion-ordered `Rc<Vec<(HKey,Value)>>`; `Op::MakeMap` + runtime-polymorphic `Op::Index` (no `IndexMap`); Set folded into generics slice (not shipped thin) | HashMap rep; separate IndexMap op; thin Set now | m-rt plan | ASKED (full gates for this slice) | ✅ |
| DEC-127 | 06-29/07-01 | Perf wins: FNV-1a string hashing; slot-indexed fields S1a/S1b + VM inline cache; COW index-assign in place (`Op::SetIndexLocal`, O(n²)→O(1)); reified-operand side-table | — | memory m4-text-and-mperf-fnv, marathon-perf-mustuse-superparent, cow-index-assign-inplace | AUTONOMOUS (marathons) | ✅ |
| DEC-128 | 07-01 | M-perf W2 (Rc-share `Value::Str`) DEFERRED — 164 call sites, ROI not demonstrated; CI perf-regression gate shipped instead (`scripts/perf-gate.sh`, ratio + best-of-N) | do the Str sharing now | memory session-2026-07-01-lane1-perfgate | AUTONOMOUS | ✅ gate / 📐 W2 |
| DEC-129 | 07-01 | M-DX build profiles Dev/Release **side-channels only** — byte-identical run≡runvm≡PHP preserved (the "keystone"); interpreter-only debugger (REPL + DAP) | profile-dependent semantics | plans/2026-07-01-m-dx-error-experience.plan.md; memory m-dx-error-experience | ASKED (milestone) / AUTONOMOUS (slices) | ✅ (unpushed) |

## 7. Concurrency (M6 W4 / green threads)

| ID | Date | Decision | Alternatives rejected | Source | Mode | Shipped |
|----|------|----------|----------------------|--------|------|---------|
| DEC-130 | 06-18 | **Single-threaded FORCED** by the Rc-shared heap (`Value` is `!Send`); OS-thread pools "off the table"; real concurrency = green threads later | OS threads | specs/2026-06-18-m6-web-design.md | ASKED (design-locked) | ✅ then ◐ (W3 shipped an OS-thread serve pool — see CONFLICTS C-6) |
| DEC-131 | 06-26 | Concurrency admission: cooperative async + pure data-parallelism + reactive over deterministic sources + a Tier-B live escape; **shared-state OS threads = HARD NO**; suspension-free subset first (D-Async-1) | shared-memory threading | native-modules-extended-scope plan | ASKED | ◐ (green threads shipped; parallel/reactive 📐) |
| DEC-132 | 06-29 | Green threads = **uniform stackful coroutines on BOTH backends + single-sourced deterministic scheduler kernel** (`green::sched`); dev chose Option A over Claude's VM-frame-swap simplification; corosensei admitted (4th dep) after a no-unsafe spike; wasm keeps eager (corosensei won't compile there) | VM frame-swap hybrid (B); OS-thread-per-task (Value !Send); literal "1+3" mix (rejected incoherent) | specs/2026-06-29-m6-w4-green-threads-design.md §4; big-marathon plan | ASKED | ✅ (A1 cutover complete, unpushed) |
| DEC-133 | 06-29 | Concurrency **quarantined from the PHP oracle** (`E-CONCURRENCY-NO-PHP` + harness skip) — transpile→sync-PHP rejected as spine-breaking; the spawn/channels example ships with no PHP equivalent (accepted exception to the 3-leg rule) | sync-PHP emission | big-marathon plan | ASKED | ✅ |
| DEC-134 | 06-29 | Interim step 2 shipped **synchronous-degenerate** (spawn eager, recv-on-empty faults) with 5 new Ops; developer then demanded the real cooperative cutover FIRST (litmus: `spawn consume(ch); send(42)` must not fault) | leaving eager semantics | big-marathon plan; memory session-playground-fix-and-cutover-foundation | ASKED | ⬆→✅ (A1 cutover) |
| DEC-135 | 07-01 | Real parallelism **ON HOLD** — models table (async-reactor / actor / data-parallel / shared-memory) recorded; actor model = best structural fit; deep M-Parallel plan delegated | committing to a model now | no-wind spec §5 | ASKED | 📐 |

## 8. Web (M6), stdlib & natives

| ID | Date | Decision | Alternatives rejected | Source | Mode | Shipped |
|----|------|----------|----------------------|--------|------|---------|
| DEC-140 | 06-18 | Portable web unit = **`handle(Request) -> Response` at the VALUE level** (PSR-7/15 insight); socket/superglobal bridge is runtime glue, never transpiled 1:1 | raw-bytes handler | m6-web-capabilities-research plan (3C 8/8) | ASKED | ✅ |
| DEC-141 | 06-18 | **Shape A** (pure-Phorj Request/Response classes) is the ONE public API; native header map = later invisible optimization | Shape B native map as 2nd API ("do both" resolved to one) | same + specs/2026-06-18-m6-w1-handler-design.md | ASKED | ✅ |
| DEC-142 | 06-18 | `bytes` primitive pulled forward as its own W0 slice; Transport trait seam quarantines the socket (`src/serve.rs`, tested outside differential.rs) | UTF-8-text-only v1 | m6 research plan §11 | ASKED | ✅ |
| DEC-143 | 06-18 | **URL/network deferred to M6** — determinism (not the dependency) gates examples; rich std-only stdlib NOW (L-2) | HTTP client via crate now | specs/2026-06-18-m3-next-intuitive-features-and-io-design.md (L-2) | ASKED (heard full challenge) | ✅ |
| DEC-144 | 06-18 | Wave-2 buildable subset only (`core.math`/`text`/`file`); `core.list`+`core.json` DEFERRED until generics/lambdas exist | force-typing with concrete sigs | trackB plan | ASKED | ✅ (both later shipped) |
| DEC-145 | 06-26 | Core.Json: number model `Int(int) + Float(float)` (PHP-faithful); `stringify` + `stringifyPretty` both; sealed `Json` ADT + explicit `mixed` escape hatch | `Num(float)`; J-prefix API | autonomous-backlog plan; ga-direction memory | ASKED | ✅ |
| DEC-146 | 06-26 | M4 sort API = `sort` + `sortWith` (mirrors PHP sort/usort); strings compare via strcmp (byte-lexicographic) never PHP numeric-string juggling; stable; returns NEW list | locale/numeric-string compare | m4-stdlib-breadth plan | ASKED | ✅ |
| DEC-147 | 06-26 | **M-NUM decimal**: primitive `decimal` (i128 fixed-point `{unscaled, scale}`); literal `1.50d`; transpile target **BCMath** (corrects the SSOT's brick/math — composer pkg can't load under `php -n`); bare `decimal/decimal` = `E-DECIMAL-DIV` → `Decimal.div(a,b,scale,mode)`; 7-mode RoundingMode; overflow = clean fault; arbitrary precision → M-NUM-2 | stdlib class; brick/math; silent division | m-num plan + specs (LOCKED) | ASKED | ✅ (later refined DEC-148) |
| DEC-148 | 06-27 | Decimal refinements from decisions-review: `%` exact remainder (dev caught mis-lumping with `/`); bare `/` = exact-or-fault (`FaultKind::DecimalInexact`); division by zero ALWAYS faults (incl. float — IEEE inf/NaN removed); numberFormat digit-string rounding | keeping E-DECIMAL-DIV for both ops; IEEE semantics | memory decision-review-and-9-fixes | ASKED | ✅ (pushed) |
| DEC-149 | 06-26 | NaN/Infinity are `Core.Math` **functions**, not keywords/literals; `Convert.toInt(float) -> int?` null on NaN/Inf/overflow (fixes PHP `(int)` quirk); conversions live in `Core.Convert` | keywords; PHP cast semantics | m-num plan (S3) | ASKED | ✅ |
| DEC-150 | 06-26/27 | `Core.Random` = seeded sub-2^63 shift-add PRNG, **pure:true** — transpiler hand-rolls identical xorshift in PHP (masked `>>`), never `mt_rand`; byte-identical across 3 legs | mt_rand mapping; quarantine | extended-scope plan (D-PRNG); decision-review memory | ASKED | ✅ |
| DEC-151 | 06-27 | **Core.Crypto = Argon2id via the first external dependency** (RustCrypto, audited) — rolling your own is the security anti-pattern; PHP-only delegation rejected (DEC-005) | hand-rolled; PHP-delegated | memory as-primitives-and-crypto-session; Cargo.toml | ASKED | ✅ |
| DEC-152 | 06-27 | Core.Http API = **Option 1: static/instance methods on injected types** (`Request.parse`, `resp.serialize()`, `Response.text`) — namespace-clean, pure Phorj | free functions; native impl | big-chunk plan | ASKED | ✅ |
| DEC-153 | 06-27 | M4 **stdlib charter FIRST** before any new stdlib surface (naming/shape conventions govern all future modules) — reorder over M-Test-first | mint modules then charter | ga-sequence plan | ASKED | ✅ |
| DEC-154 | 06-28 | Router: `Core.Http` Router + `#[Route]` attributes + middleware | — | specs/2026-06-28-m6-w2-router-attributes-design.md | ASKED | ✅ |
| DEC-155 | 06-21 | Stack traces identical across backends (interpreter gains a logical call-stack mirroring VM frames); traces on stderr only (FaultKind spine untouched); CLI + dev-mode web error page; **prod = bare 500, never leaks trace/source** | VM-only traces | error-handling plan | ASKED | ✅ |
| DEC-156 | 07-01 | Manual benchmarking (`Core.Runtime.memoryBytes`/Stopwatch) legal but **quarantined from the byte-identity example set** (`pure:false` model) | blocking manual timing entirely | m-dogfood plan | ASKED | ✅ |

## 9. Tooling, build, distribution, interop

| ID | Date | Decision | Alternatives rejected | Source | Mode | Shipped |
|----|------|----------|----------------------|--------|------|---------|
| DEC-160 | 06-16 | `phg build`: embed program SOURCE in a versioned CRC-guarded `.phorj` ELF/PE/Mach-O section; stub = the running phg binary; cross-compile via cargo-zigbuild; apple `--target` rejected (Phase 3) | bytecode embedding | m2.5 plans/specs | ASKED | ✅ (vendor-merge + Phase 3 deferred) |
| DEC-161 | 06-17 | Profiling lives in `bench` (one timing surface); memory measured COLD (warm glibc reads ~0); `phg disasm` ships | separate `--profile` flag | v0.4.0 plan | ASKED | ✅ |
| DEC-162 | 06-19 | GA road M7→M12; keep 3-backend model + Op descriptor table; **shared-IR rewrite deferred**; M7 correctness closure non-negotiably first; runtime PHP helpers (`__phorj_div`/`__phorj_rem`/`__phorj_str`) over a static transpiler type resolver | shared IR now; transpiler-side type resolver | ga-roadmap plan | ASKED | ✅ |
| DEC-163 | 06-23/24 | PHP transpile floor raised 8.4 → **8.5**; CI pins 8.5 + non-gating 8.6-dev canary; version *targeting* (`--php-target`) = separate post-S8 milestone | float to 8.6 | memory php-transpile-floor-84; traits spec version note | ASKED | ✅ |
| DEC-164 | 06-24 | **WASM playground**: Cargo workspace, isolated `playground/` crate (core stays dep-free); full 3-way with php-wasm from day one; CodeMirror 6; GitHub Pages | core-crate wasm deps | playground plan | ASKED | ✅ |
| DEC-165 | 06-25 | Transpile modernization Track 1 before M-Lift (native match/ternary/clone/`??throw` emission; `OpKind` operand resolver; `__phorj_float` Ryū helper irreducible) | lift first | transpile-modernization plan | ASKED | ✅ |
| DEC-166 | 06-25 | **M-Lift (PHP→Phorj)**: staged Tier-1 → Tier-2 (round-trip-gated) → **Tier-3 best-effort with loud `// LIFTED TIER-3 (unsafe — verify)`**; hard-untranslatable core (`eval`, `$$x`, runtime magic, dynamic class names) always `// CANNOT LIFT`, never guesses | demo-only reach; original blanket "refuse Tier-3" (superseded for the attemptable subset) | full-bidirectional + m-lift plans | ASKED | ✅ |
| DEC-167 | 06-25 | Lift verdicts: C-1 interpolation faithful-subset (silent wrong guess worse than loud rejection); C-45 void-or-reject; C-5/6 precedence-aware printer; C-46/47 instanceof + bitwise coverage | "try everything" | php-fidelity plan | ASKED | ✅ |
| DEC-168 | 06-27+ | LSP: ONE server reusing the checker `Diagnostic` surface + thin VSCode/JetBrains clients; cross-file support in the 06-28 marathon | per-editor logic | memory ide-tooling-extensions | ASKED | ✅ |
| DEC-169 | 06-27/28 | M-Test: `phg test` + `Core.Test` + `test"…"{}` blocks; seeded Faker; Reflect-based auto-mocker (full suite chosen) | minimal assertions-only | extended-scope plan; memory m-test-milestone | ASKED | ✅ |
| DEC-170 | 06-28 | `phg fmt`: canonical-form, comment-preserving (side-channel), meaning-preserving printer | reflow/opinionated formatter | memory phg-fmt-milestone | ASKED | ✅ (CLI now `format`) |
| DEC-171 | 06-28 | M8.5 interop: `declare` blocks / `.d.phg`-style typed PHP bindings | — | memory m8.5-interop-declare | ASKED | ✅ |
| DEC-172 | 06-27 | M2.5 Phase 3a stub registry: SHA-256 + manifest + verify-before-cache; 3b (`--sign`) deferred/parked | — | memory m25-phase3a-stub-registry | ASKED | ✅ 3a / 📐 3b |
| DEC-173 | 06-23 | M-Decomp: whale files split into `foo/mod.rs` cohesion clusters, byte-identity-gated; **HYBRID by-phase backbone + selective thin-dispatcher** (pure by-construct rejected) | by-construct split | specs/2026-06-23-decomposition-milestone-design.md | ASKED | ✅ |
| DEC-174 | 06-27 | `git push` NEVER autonomous (standing, survives every bypass); `git add`/`commit` autonomous when green (project override of global Rule 10, authorized 06-16) | — | CLAUDE.md | ASKED | ✅ standing |
| DEC-175 | 07-01 | Post-M-DX order: **Naming → M-perf → VM-debug-symbols → Stdlib-breadth**; + 5 folded ADD candidates (`phg repl`, `phg doc`, parser multi-error recovery, A2 generators, opportunistic wins) | other orders | four-lane plan | ASKED | in progress |
| DEC-176 | 07-01 | Post-dogfood: clarity workstream = **blanket `clippy::pedantic`, fix ALL** (dev overrode "selective lints only" rec) | selective lints | post-dogfood plan | ASKED (overruled) | in progress |

## 10. Parity SSOT (2026-06-21/22) — verdict summary

**One-shot 20-track (A–S+V), 41-agent review → 555 deduplicated candidates: 290 adopt / 187 defer / 81
reject.** SSOT: `docs/specs/2026-06-21-php-parity-and-beyond.md`. Verdict vocabulary: kind
port/new/map/omit × rec adopt/defer/reject. Category sections: 2.1 error handling/totality · 2.2 OO &
types · 2.3 pattern matching · 2.4 call convention/operators/syntax · 2.5 semantics/numerics · 2.6
mutation/build/packages · 2.7 stdlib & batteries · 2.8 concurrency/web/security · 2.9 tooling/testing/DX ·
2.10 performance · 2.11 interop & migration · 2.12 docs/governance/competitive.

Developer-locked batch decisions from the triage (already itemized above where major): three-tier error
model (DEC-068); totality-before-overloading reorder (DEC-060); nine new milestones approved (M4, M-NUM,
M-TIME, M-text, M-Test, M-perf, M-Batteries, M8.5, M13); full ROADMAP/MILESTONES write-back; PascalCase
incl. vendor (DEC-035). Representative REJECT bucket (81): single-quote strings, `<=>`, `.` concat,
`switch`, ambient superglobals, `eval`, variable-variables, runtime magic methods (`__get`/`__set`/`__call`),
loose `==` semantics, `@` suppression, PL-theory items that don't earn their surprise budget (typestate,
refinement types, comptime macros noted as vanity for this language's thesis). An earlier version of the
review had a ~56-item purist reject bucket that the developer **corrected** (philosophy recalibration —
see DEC-004); verdicts were re-graded under the craftsmanship-apex lens.

---

## CONFLICTS (contradictory records — adjudicate)

| # | Conflict | Trace | Status |
|---|----------|-------|--------|
| **C-1** | **D-L3 (06-18) REJECTED multiple inheritance** ("realized as traits/mixins + interfaces") — yet **S6 shipped real MI** (`class C extends A, B`) *and* S8 shipped traits. | Traced: 06-18 D-L3 reject (next-intuitive-features spec) → 06-21 dev: "multi-inheritance wanted, real game changer, WITHOUT removing traits" (ga-direction memory) → 06-22 dev rejected the single+traits framing **twice**, demanded research → S6 Model-1 explicit-resolution MI ASKED + shipped; Model-3 C3 deferred. So: a legitimate developer reversal, properly recorded each step — but **D-L3's text was never amended**, so the two specs still contradict. | Developer-driven supersession; needs doc reconciliation, not re-adjudication. |
| **C-2** | **A-6 (06-25) adopted `foreach (coll as …)` to REPLACE `for (x in coll)`** ("free `for` for C-style only") — but commit `0747385` (06-26) shipped foreach **"alongside the typed `for (T x in xs)` form"**; examples still use for-in everywhere; FEATURES.md lists `for … in` ✅ with no replacement note; B1 (06-30) *extended* for-in (string/Map two-binding). | The decided replacement was silently softened into an addition during an autonomous slice. Either the decision or the implementation is wrong. [Verified: both forms parse today.] | **Open — adjudicate** (keep both / execute the replacement / amend A-6). |
| **C-3** | **Zero-dep locked framing (06-26): "NO TLS, NO regex, NO http/serde crates, `[dependencies]` empty, verified"** — days later `regex` admitted as dep #2, plus argon2/ctrlc/corosensei (4 deps total). | Each dep individually developer-authorized under the 06-27 dependency policy; but the 06-26 "LOCKED FRAMING" text (native-modules-research plan) explicitly names regex as forbidden and was never updated. | Superseded-in-practice; framing doc stale. |
| **C-4** | **`text` leaf chosen 06-18 explicitly "not `string` (avoids shadowing the `string` type)"** — naming overhaul (06-30) renamed `Core.Text` → **`Core.String`**. | The original rationale (shadowing) is mooted by PascalCase (`String` ≠ `string`), but no record shows the old rationale being revisited when the rename was made. | Likely fine; confirm the shadowing concern was consciously dismissed. |
| **C-5** | **Ternary: two same-day records disagree (06-24)** — perimeter spec says "ternary ✅ add"; master plan says "DEFERRED, not rejected" (postfix-`?` collision + expression-if coverage). | [Verified: `? :` is a parse error in the current binary → DEFERRED won.] The perimeter spec was never corrected. | Resolved in practice; fix the stale record. |
| **C-6** | **M6 design (06-18): OS-thread pools "off the table"** (Rc heap) — yet **M6 W3 shipped an OS-thread pool for `phg serve`** (memory: m6-w3-serve-concurrency), later superseded by green threads. | The W3 pool isolated per-connection state so it didn't share `Value`s, but it contradicts the design's blanket statement; superseded anyway by DEC-132. | Historical; no action beyond doc note. |
| **C-7** | **CLAUDE.md/docs still document `phg bench`, `phg disasm`, `phg fmt`** while DEC-113 renamed the CLI verbs to `benchmark`/`disassemble`/`format`/`tokenize`. | Doc drift from the unpushed naming overhaul; e.g. project CLAUDE.md instructs `phg bench <file>`. | Doc reconciliation task. |
| **C-8** | **`E-INTERSECT-SIG` (require-agreement) was decided with "revisited when overloading lands"** — overloading landed (param + return-type); no record shows the revisit happening. | m-rt plan D2 note vs overloading completion. | **Open — adjudicate** (allow intersections whose shared method differs per overloading rules?). |
| **C-9** | **"Nothing in the wind" (06-18) vs shipped import-free intrinsics** — `panic`/`todo`/`unreachable`/`assert` shipped usable with no import, violating the standing principle for weeks. | Caught by the developer 07-01; fix designed (DEC-047: `import Core;`) but NOT implemented. | Designed fix pending implementation. |
| **C-10** | **Zero-payload enum-variant construction guidance is stale in older records** — pre-06-24 docs/memory said "construct with `V()`"; mandatory-`new` (DEC-083) made it `new V()`, while *match patterns* still use bare call form `V()` (bare `V =>` remains a silent catch-all footgun, deliberately preserved in DEC-056). | memory zero-payload-variant-call-form (already corrected 07-01) + S4 footgun preservation. | Mostly reconciled; the `V =>` catch-all footgun itself may deserve re-adjudication (it was preserved autonomously). |

## SUPERSEDED (decision → what replaced it)

| Original | Superseded by | When/Who |
|----------|--------------|----------|
| D-L3 reject MI → traits at S5 (06-18) | S6 explicit-resolution MI **and** S8 traits both shipped (DEC-062/064) | 06-21/22, developer (twice rejected the old framing) |
| lowercase `core.console` etc. (06-18) | PascalCase `Core.Console` (DEC-034) | 06-20, developer |
| `console` leaf (06-18) | `Core.Output` (DEC-113) | 06-30, developer |
| `text` leaf (06-18) | `Core.String` (DEC-113) | 06-30, developer |
| `fn` lambda keyword (S3, 06-18) + A-1 typed-lambda `fn(int x): string` (06-25) | lambda keyword `function` (DEC-113) | 06-30, developer |
| `->` return syntax (M1) | `: T` returns, `->` retired (DEC-093) | 06-25, developer |
| `Ok`/`Err` Result variants (error-model slice 2) | `Success`/`Failure` (DEC-113) | 06-30, developer |
| `Empty` PascalCase unit type (06-24) | lowercase `empty` keyword + `E-VOID-IN-UNION` (DEC-113) | 06-30, developer |
| `recv` (green threads) | `receive` (DEC-113) | 06-30, developer |
| CLI `fmt`/`bench`/`disasm`/`lex` | `format`/`benchmark`/`disassemble`/`tokenize` (DEC-113) | 06-30, developer |
| "retire `var`" agreement (06-26 AM) | keep `var`, contextual (DEC-100) | 06-26 same day, developer after research |
| `is` value-equality stub (M1) | real `instanceof`; `is` de-keyworded (DEC-051) | 06-20, developer (over Claude's retire-it dissent) |
| Bare construction `V()` / `Name()` | mandatory `new` everywhere (DEC-083) | 06-24, developer |
| E-PKG-TYPE functions-only libraries (06-18) | cross-package types + `import type` (DEC-036) | 06-20, developer (planned lift) |
| manifest key `name` (06-18) | `module` (DEC-035 slice 1, `ce588e3`) | 06-20, developer |
| `package main` lowercase (M5 S1) | `package Main` PascalCase reshape | 06-23, developer |
| PHP floor 8.4 | floor 8.5 + 8.6-dev canary (DEC-163) | 06-24, developer |
| "keep Phorge, rename pre-GA" (06-21) | rename NOW → Phorj (DEC-013) | 06-28, developer |
| Zero-dep absolute (06-26) | 4-dep vetted policy (DEC-009) | 06-27/29, developer per-dep |
| Tier-3 lift = refuse (M-Lift tier table) | Tier-3 best-effort + loud annotation (DEC-166) | 06-25, developer ("Option 1 and 3") |
| W3 OS-thread serve pool | green-thread runtime (DEC-132) | 06-29, developer |
| spawn-eager synchronous-degenerate (step 2) | cooperative cutover A1 (DEC-134) | 06-29/30, developer demanded litmus |
| `Value::Set` as `HashSet<HKey>` (S7b-2 initial) | insertion-ordered `Rc<Vec<HKey>>` | 06-20, autonomous realignment |
| `Channel.new()` | `Channel.create()` (DEC-112) | 06-29, forced by `new` keyword |
| `Op::MatchFail` | generalized `Op::Fault(FaultMsg)` | 06-17, agreed in-slice |
| M2 "mark-sweep GC" success criterion | Rc/Drop + COW; tracing GC permanently mooted (DEC-123/065) | 06-17→06-21, developer |
| Reflect/Convert/Validate/Crypto package names | Reflection/Conversion/Validation/Cryptography (DEC-113) | 06-30, developer |
| `Bytes.len`/`Text.len` | `.length` hard rename (DEC-102 D) | 06-26, developer |
| php-parity-review (narrow Track A/B) | 20-track roadmap-completeness review (§10) | 06-21, developer |
| flat 2-level imports only | deep imports + dual call form (DEC-047) | 07-01, developer — 📐 not implemented |
| `Attr`/`Error`/`Channel`/`Task` reserved built-ins | de-reserved → importable Core modules (DEC-047) | 07-01, developer — 📐 not implemented |

## AUTONOMOUS-HIGH-IMPACT (adjudicate first)

Ranked by user-visible blast radius (syntax/keywords/semantics). All were made in `_AUTONOMOUS_3C` /
bypass-sentinel sessions without a per-decision ask; some sit inside developer-approved *milestones* but
the specific user-visible call was Claude's.

1. **DEC-056(d) — the `Circle =>` catch-all footgun deliberately preserved** (S4, autonomous D3): a bare
   PascalCase ident in a match arm is a *binding*, silently catching everything; the type-pattern needs
   two idents. This is the same trap that already bit zero-payload enum variants. A one-line warning
   (`W-BINDING-SHADOWS-TYPE`) was possible and was not chosen. **Highest silent-bug surface.**
2. **DEC-094 execution drift — foreach shipped "alongside" instead of "replacing" for-in** (C-2): the
   language now permanently carries TWO iteration statements; every doc/example choice compounds it.
   Decided-ASKED, drifted-AUTONOMOUS.
3. **Totality cluster semantics (DEC-060)**: `E-MISSING-RETURN` hard error + `never` type + the exact
   divergence rules (`while(true)` with no `break` counts, etc.) — a breaking-ish soundness gate whose
   precise contours (what counts as terminating) were fixed autonomously.
4. **Pattern-cluster surface (06-23, fully autonomous)**: `when` guard keyword (contextual), struct
   destructuring forms (shorthand/rename/nesting), number-literal grammar (`0x`/`0b`/`0o`/`_`/`1e3`),
   bitwise operator set incl. `>>` lexed as two `Gt` — all permanent user-facing syntax chosen in one
   autonomous sweep.
5. **S7a generics details (autonomous)**: PascalCase-only type params (`E-TYPE-CASE`), first-binding-wins
   (non-backtracking) inference, inference-only construction with **no turbofish** (`Box<int>(7)` illegal
   forever unless revisited) — the no-explicit-type-arg call syntax is a notable permanent gap vs TS.
6. **Overnight F-001/F-003 (RATIFIED next morning but shipped first)**: UFCS resolves *any imported
   native by first-param unify* — including number receivers (`n.abs()`) — a broad implicit-resolution
   surface; ambiguity = `E-UFCS-AMBIGUOUS`, which later forced a native rename (`repeat`→`fill`).
7. **M-DX debugger surface (07-01, autonomous slices)**: `phg debug` REPL command set + DAP protocol
   choices + `--dump-on-fault` format — developer-facing tool UX fixed without a surface review.
8. **Dogfood W0/W2 grammar patches (07-01, autonomous)**: empty-list literal init rule, comma-throws,
   nested-quote interpolation semantics — small but permanent grammar decisions.
9. **DEC-070 invariance retrofit (autonomous)**: same-head generic assignability tightened (programs
   that previously type-checked now rejected) — a breaking soundness fix applied without an ask.
10. **DEC-127 `Op::SetIndexLocal` + COW in-place mutation model** (autonomous): observable only via
    performance, but it created a new Op + a subtle aliasing contract (`make_mut` at refcount 1) that
    future features must honor.

**Notable ASKED-but-thin decisions worth re-surfacing during adjudication** (recorded as developer
choices but decided rapidly inside marathons): DEC-133 (concurrency permanently outside the PHP oracle —
the single standing exception to the 3-leg identity claim); DEC-083 (`new` on enum variants — no other
language does this; dev overruled the rec); DEC-096 (`++`/`--` as expressions — overruled after hazard
briefing; `W-SEQUENCE-MUTATION` lint status unverified); DEC-057 D2 revisit (C-8).

---

*Register totals: 141 primary rows (DEC-001…DEC-176 numbering with gaps) + 555 triage rows summarized
by category (§10). Mode split over primary rows: ASKED ≈ 108 (incl. 2 RATIFIED overnight forks),
AUTONOMOUS ≈ 25, UNCLEAR ≈ 8 (mostly early-M1/M2 records with no explicit mode note). 10 conflicts,
33 supersessions traced.*
