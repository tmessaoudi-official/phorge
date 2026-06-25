# Full Bidirectional PHP ↔ Phorge Support — Plan

> Umbrella plan: make **both** directions complete.
> **↓ Phorge→PHP** (`transpile`, byte-identity-verified) and **↑ PHP→Phorge** (`lift`, best-effort).
> Sub-plans: [`2026-06-25-transpile-modernization.plan.md`](2026-06-25-transpile-modernization.plan.md)
> (↓ modernization, **COMPLETE**) and [`2026-06-25-m-lift-php-to-phorge.plan.md`](2026-06-25-m-lift-php-to-phorge.plan.md)
> (↑ lift, L1 done). This file coordinates the remaining waves across both.

## Decisions Log
- [2026-06-25] AGREED (developer): pursue **full bidirectional support** — close every gap in both
  directions, then add the PHP-parity language features Phorge still lacks.
- [2026-06-25] AGREED: **scope = Both, sequenced** — Wave 1 (coverage + parity of already-shipped
  features) first, then Wave 2 (new PHP-parity language features).
- [2026-06-25] AGREED: **close the visibility parity hole in the checker now** — extend the existing
  `E-CONST-VISIBILITY` enforcement to fields/methods (`E-FIELD-VISIBILITY`/`E-METHOD-VISIBILITY`) so
  `run ≡ runvm ≡ real PHP` all reject an out-of-scope `private`/`protected` access. Front-end-only,
  no new `Op`/`Value`.
- [2026-06-25] AGREED: **lift reach = Tier-1 + Tier-2 (round-trip-gated) AND attempt Tier-3**
  (developer chose "Option 1 and Option 3"). **Reconciliation** (overrides the M-Lift plan's earlier
  "refuse Tier-3" verdict, which stands ONLY for the genuinely-untranslatable subset): Tier-3 is lifted
  **best-effort with a loud `// LIFTED TIER-3 (unsafe — verify): <reason>` annotation**, and the L5
  round-trip differential is the confidence proof (a Tier-3 lift that round-trips byte-identically is
  earned; one that diverges is flagged). The **hard-untranslatable** core — `eval`, variable-variables
  `$$x`, true runtime magic (`__get`/`__set`/`__call`), dynamic class names — still emits
  `// CANNOT LIFT: <reason>` and never guesses. 100%-confidence remains impossible; honesty is the
  contract.

## Answers to the developer's three questions (verified against code, 2026-06-25)
| Question | Status | Evidence |
|---|---|---|
| Static **function value** (`static (int)->int f = …;`) — "PHP doesn't support" | ✅ **Shipped.** PHP can't init a static prop with a closure, so transpile emits `public static \Closure $f;` + `__phorge_init_statics()` assigns it once before `main()`. Incl. `static mutable`. | `src/transpile/program.rs:839-858`, `:167-190` |
| `public`/`private`/`protected` member attributes | ⚠️ **Syntax/AST/transpile complete; runtime NOT enforced** (only `const` is). Parity hole → **Wave 1.1 fixes it.** | `src/checker/calls.rs:790-824`; KNOWN_ISSUES.md:521-527 |
| Initialize a field **with a function** | ✅ **Shipped** (instance + static field initializers accept lambdas/fn-values). Constraint: field-init lambda may not capture `this` (`E-LAMBDA-THIS`). | `src/checker/tests/field_init.rs:57-104` |

---

## WAVE 1 — Coverage + Parity (↓ Phorge→PHP completeness)
Small, high-value, fully verifiable; de-risks the rest. Each slice green + `run≡runvm≡real PHP 8.5`,
clippy+fmt clean, no new `Op`/`Value` unless noted, one guide example.

| Slice | Work | Notes |
|---|---|---|
| **W1.1 ✅** | **Member visibility enforcement** in the checker — **COMPLETE.** `ClassInfo` gains `field_vis`/`method_vis` (name → (vis, owner)), populated at collection (fields, promoted ctor params, methods), merged through inheritance (owner preserved for `extends`, re-owned to the using class for trait `use`). A shared `enforce_member_vis` helper (Private→owner, Protected→owner+subtypes) is wired into **six** external-access sites: instance-field read (`check_member`), field write (`check_field_assign`), **clone-with `obj with {…}`**, **let-destructuring** (`stmt.rs`), **match struct-patterns** (`matches.rs`), and method call (`check_method_call`). Codes `E-FIELD-VISIBILITY`/`E-METHOD-VISIBILITY` (+ `phg explain`). Example `examples/guide/member-visibility.phg` (legal accesses; rejected cases in README). 15 visibility tests + 933 gate green, byte-identical run≡runvm≡PHP 8.5. | Front-end-only, no new `Op`/`Value`. Phase-0 scan found NO example reads a private member externally (they use accessors); fixed two test fixtures that relied on the hole. **Verified (PHP 8.5):** `clone($o,[…])` AND `$obj->field` destructuring both throw on a private field — hence the clone-with + destructuring siblings. **Remaining narrow corners (documented in KNOWN_ISSUES, not yet enforced):** `private` *static* fields (`ClassName.field`) and intersection-typed-receiver members. |
| **W1.2 ✅** | **MI-ancestor type references** — **ALREADY SHIPPED (S6c.3), no work needed.** Phase-0 empirical check found `class C extends A, B` already transpiles `c instanceof A` → `$c instanceof IA` and ancestor-typed bindings, byte-identical 3-way (`guide/inheritance-lattice.phg`). The KNOWN_ISSUES "deferral (1)" was **stale** (written at S6b, not updated when S6c.3 landed) — corrected. *(Lesson: verify state against code, not docs — Rule 11.)* |
| **W1.3 ✅** | **Trait conflict resolution emission** — **COMPLETE.** A trait-vs-trait collision resolved by `use P.m`/`rename`/`exclude` now lowers to a combined PHP `use P, Q { P::m insteadof Q; P::m as n; }` block (new `build_use_trait_clauses`, the trait-composition analogue of the proven MI `build_trait_clauses`; `emit_class` threads `program`). Was a real gap (verified: PHP Fatal `Trait method ... not applied ... collision` without `insteadof`). Example `guide/trait-conflicts.phg`; all three forms (use/rename/exclude) byte-identical run≡runvm≡PHP 8.5. Transpile-only, no new `Op`. | KNOWN_ISSUES trait-deferral (4) closed. Narrow remaining edge (collision via a trait's own nested `use`) documented + oracle-guarded. |
| **W1.4 ✅** | **Coverage audit + triage** — **COMPLETE.** Swept the transpiler for unhandled-construct markers; **found + fixed a real cross-backend gap**: a general function-valued callee (`adder()(41)`, `fns[i](x)`, `(if … )(x)`) type-checked + ran on the interpreter but the **VM compiler AND transpiler both rejected it** ("unsupported call target"). Fixed both via the existing `CallValue` / `(<expr>)(args)` path (mirrors the interpreter); byte-identical 3-way, showcased in `guide/lambdas-pipe.phg`. Triage of the rest below. | Closes the ↓ direction. The `unreachable!`/`call.rs:135` markers are guaranteed invariants, not gaps. |

### W1.4 transpile-completeness triage (the ↓ direction)
**Conclusion: every shipped, example-covered Phorge feature has a working transpile path** — proven by the differential PHP oracle gating all **88** `examples/**/*.phg` byte-identical `run≡runvm≡real PHP 8.5`. The remaining KNOWN_ISSUES entries are NOT "feature exists but won't transpile" gaps; they fall into:
- **Fixed this wave:** member visibility (W1.1), trait-vs-trait conflicts (W1.3), MI-ancestor refs (W1.2/S6c.3, already shipped), general callable-expression callee (W1.4).
- **Inherent fault-domain divergences** (kept, documented): float ÷0 → PHP `DivisionByZeroError` vs Phorge `inf`/`NaN`; `opt!` message has no PHP source location. The differential excludes faults by design; no example produces them.
- **Unbuilt language features** (no transpile path needed yet): generic traits, cross-package traits, sized ints, `decimal`, etc. — later milestones; the PHP-parity subset (variadics/defaults/named args/attributes) is **Wave 2**.
- **Narrow checker corners** (not transpile gaps): `private` statics + intersection-member visibility (W1.1 follow-ups); a generic-typed result not a VM arithmetic operand (run↔runvm, pre-existing, workaround = bind to a typed local).
No transpiler marker is a reachable valid-Phorge gap after W1.4.

## WAVE 2 — New PHP-parity language features (bidirectional per feature)
Each lands the **full pipeline in one slice**: lexer → parser → AST → checker → interpreter → VM →
transpiler → (lift path once L2 exists) → guide example. Byte-identity-gated. Ordered easiest→hardest.

| Slice | Feature | Sketch |
|---|---|---|
| **W2.1** | **Default arguments** `function f(int x = 0)` | Param gains `default: Option<Expr>`; checker validates const-or-expr + trailing-only; backends fill missing args; transpile → PHP default param. |
| **W2.2** | **Variadic params** `function f(int... xs)` | Param `variadic: bool`; collects trailing args into a `List<T>`; one new lowering, likely no new `Op` (build a list). Transpile → PHP `...$xs`. |
| **W2.3** | **Named arguments** `f(x: 1, y: 2)` | Call-site arg labels; checker reorders against the sig; backends reorder at the call. Transpile → PHP named args (8.0). |
| **W2.4** | **Attributes** `#[Route("/x")]` | New `Item`/member annotation node; checker stores; transpile → PHP `#[...]`. Decision needed: are Phorge attributes *inert metadata* (emit + reflect only) or do any drive behavior? Default: inert, reflectable via `Core.Reflect`. |

## ↑ DIRECTION — M-Lift (PHP→Phorge), build-out
Continues [`2026-06-25-m-lift-php-to-phorge.plan.md`](2026-06-25-m-lift-php-to-phorge.plan.md). L1 (lexer) done.

| Slice | Work | Tier reach |
|---|---|---|
| **L2** | Tier-1 PHP **parser** (`src/lift/parser.rs`): typed fn sigs, classes + typed props + ctor promotion, `enum`, `match`, `if`/`for`/`foreach`/`while`, exprs, array literals → a PHP AST. The dominant slice. | Tier-1 |
| **L3** | Phorge AST → `.phg` **pretty-printer** (new; transpiler prints PHP, not Phorge). Reusable later for `phg fmt`. | — |
| **L4** | **Lifter** PHP-AST → Phorge-AST: Tier-1 1:1; Tier-2 infer `List`/`Map`/`Set` from `array` usage, `?T`→`T?`, `??`/`?->`; **Tier-3 best-effort + `// LIFTED TIER-3 (unsafe — verify)`**; hard-untranslatable → `// CANNOT LIFT`. | Tier-1+2+3 |
| **L5** | **Round-trip differential gate**: lift PHP→Phorge, transpile back→PHP, run both under real PHP, compare stdout. Match = behavior preserved. Annotate `// lifted (verify)`. The Tier-3 confidence proof. | gate |
| **L6** | `phg lift` CLI + **playground "paste PHP → see Phorge"** demo. | tooling |

## Proposed sequence (adjustable)
1. **Wave 1** (W1.1→W1.4) — quick parity wins, closes ↓ direction.
2. **M-Lift L2 + L3 + L4-core** — stand up the ↑ direction to a working Tier-1 lift.
3. **Wave 2** (W2.1→W2.4) — now each new feature lands BOTH a transpile path and a lift path in one slice (the L2 parser exists).
4. **M-Lift L5 + L6 + Tier-2/Tier-3 extension** — round-trip gate, CLI, playground, deeper inference.

## Invariants (all slices)
- `run ≡ runvm ≡ real PHP 8.5` byte-identical (gate: `PHORGE_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORGE_REQUIRE_PHP=1 cargo test --lib --test differential`).
- No new `Op`/`Value` unless a slice explicitly justifies one (then the 3 coupled matches in the same commit).
- Each shipped feature → a runnable byte-identity-gated `examples/` guide program + README entry.
- `cargo clippy --all-targets` + `cargo fmt --check` clean. TDD: failing test first.
- The lift front-end (`src/lift/`) is wholly separate from the Phorge pipeline → unit-tested, not on the byte-identity oracle (except L5's round-trip).

## Status
- [2026-06-25] Plan written + committed (`f3c3bc2`).
- [2026-06-25] AGREED (developer): **proceed — Wave 1.1 (visibility enforcement) first** (my
  recommendation: the one real byte-identity hole, cheap, de-risks the rest).
- [2026-06-25] **W1.1 COMPLETE** — member visibility enforced across all six external-access sites;
  three sibling holes (clone-with, let-destructuring, match struct-patterns) found by the blast-radius
  convergence pass and closed. 933 gate green, clippy+fmt clean.
- [2026-06-25] **W1.2 = no-op** — MI-ancestor type refs were already shipped (S6c.3); only the stale
  KNOWN_ISSUES doc needed correcting. (Phase-0 empirical verification, not doc-trust.)
- [2026-06-25] **W1.3 COMPLETE** — trait-vs-trait conflict resolution now transpiles to PHP
  `insteadof`/`as`; `guide/trait-conflicts.phg` byte-identical 3-way.
- [2026-06-25] **W1.4 COMPLETE — WAVE 1 (↓ Phorge→PHP) CLOSED.** Audit found + fixed a real
  cross-backend gap (general callable-expression callee rejected by VM+transpiler); triage confirms
  every example-covered feature transpiles byte-identically (88 oracle-gated examples). Remaining
  KNOWN_ISSUES are inherent fault-domain / unbuilt-feature / narrow-corner — none a reachable transpile
  gap. **NEXT = Wave 2 (new PHP-parity features) and/or M-Lift L2 (↑ direction).**
- [2026-06-25] PRINCIPLE (developer): **PHP is the floor, not the ceiling.** Adopt PHP's well-thought
  features; *fix* what violates best practice / craftsmanship — both directions. In transpile, hide
  PHP's awkward mechanics behind a cleaner Phorge surface (e.g. `use P.m` → PHP `insteadof`); in lift,
  emit idiomatic best-practice Phorge, never mirror PHP warts. Applies to Wave 2 (new features) + M-Lift.
