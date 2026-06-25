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
| **W1.1** | **Member visibility enforcement** in the checker. Extend `MemberVis::of` usage from `const`-only to field reads/writes + method calls: Private→owner class only, Protected→owner+subtypes, Public→anywhere. New codes `E-FIELD-VISIBILITY`/`E-METHOD-VISIBILITY` (+ `phg explain`). | Front-end-only; closes the byte-identity hole. Phase-0 must scan all examples/tests for now-illegal external `private` access and fix (likely the promoted-field reads KNOWN_ISSUES flags). |
| **W1.2** | **MI-ancestor type references** (S6c): when a multi-parent class lowers to interface+trait, rewrite a Phorge type binding / `instanceof` for that ancestor to the interface form (`ISwimmer` not `Swimmer`). | KNOWN_ISSUES.md:24-44. Loader/transpiler rewrite, mirrors existing decomposition. |
| **W1.3** | **Trait conflict resolution emission**: the checker already resolves `use P.m`/rename/exclude; emit PHP `insteadof`/`as` in the transpiled `use` block instead of a plain `use T;`. | KNOWN_ISSUES.md:46-59. Transpile-only. |
| **W1.4** | **Coverage audit + triage** of the 24 documented transpile limitations: fix the fixable (above), and for the *inherent* fault-domain ones (float ÷0, `opt!` location) confirm they stay documented (the differential excludes faults by design). Produce a final "every shipped feature → transpile path" matrix. | Closes the ↓ direction. |

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
- [2026-06-25] Plan written; **awaiting go** before Wave 1 implementation (Large task — explicit approval gate).
