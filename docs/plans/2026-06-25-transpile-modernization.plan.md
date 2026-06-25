# Transpile Modernization (Track 1) Plan

> Make the Phorge→PHP transpiler emit **idiomatic, modern PHP** — native `match` expressions,
> ternaries, and PHP 8.5 `clone`-with — instead of verbose `if/elseif` chains and IIFE closures.
> Self-contained; every slice gated by the existing `run≡runvm≡real PHP 8.5` differential.

## Decisions Log
- [2026-06-25] AGREED: Build **Track 1 before M-Lift** — smaller, self-contained, immediately visible,
  and it de-risks M-Lift (a clean native-PHP printer makes the lift round-trip far easier to validate).
- [2026-06-25] AGREED: Sequencing — finish in-flight work (Process I/O ✓, Reflect Tier-2 ✓), write
  these plan files, developer compacts, then build Track 1 slice by slice.

## Audit evidence (transpiled output today, 2026-06-25)
Already idiomatic (no work): higher-order natives → `array_map`/`array_filter`/`array_reduce`/
`array_sum` (+ arrow fns); constructor promotion; `final class`; `??`; first-class callables `f(...)`;
`mixed`; `\UnhandledMatchError`. **Justified, NOT gaps (byte-identity necessities — leave them):**
`__phorge_float`/`_str`/`_add`/`_div`/`_rem` (PHP loose semantics ≠ Rust); enum→class-hierarchy (PHP
8.1 `enum` can't carry per-variant payloads). **Real gaps:**
1. **`match` → `if/elseif/else` chains** (both literal and type/union matches) — PHP's native `match`
   is unused entirely.
2. **Expression-position `match`/`if` → IIFEs** (`(function() use(...){ if(...) return …; })()`) where
   PHP has true `match`/ternary expressions. The ugliest output.
3. **`clone … with` → `__phorge_clone_with` helper** though the floor is now PHP 8.5, where native
   two-arg `clone($o, [...])` exists (the helper's comment still says "8.4" — stale since the bump).

## Slices (each green + byte-identity-gated)
| Slice | Work | Risk | Notes |
|---|---|---|---|
| **T1** | Literal/value `match` → PHP `match($x){ lit => …, _ => … }` | Med | PHP `match` is strict `===`; Phorge literal match is `==` on primitives — verify they agree for int/string/bool. Exhaustive Phorge matches → no `default` arm (the checker proved totality; PHP throws `\UnhandledMatchError` on the unreachable no-match, same as today). |
| **T2** | Type/guard `match` → `match(true){ $x instanceof T => …, cond => … }` | Med | A true expression → also kills the IIFE for these. Binding patterns: reference the scrutinee var directly. Struct-destructuring patterns can't be a `match` arm → keep the imperative/IIFE fallback for those. |
| **T3** | Expression-position `if` → ternary `?:` | Low | Replaces the IIFE for `if (c) { e } else { e }` in value position. |
| **T4** | `clone … with` → native `clone($o, [...])`; drop `__phorge_clone_with` | Low | Floor is 8.5; native two-arg `clone` available. Verify the bare-`clone` (no overrides) path too. |
| **T5** | Byte-identity sweep + retire dead helper(s); transpile-quality audit of remaining examples | Med | Re-transpile all examples; confirm no regression; document any deliberately-kept helper. |

## Acceptance
- Every example transpiles to PHP with **no IIFE** except where a binding/destructuring pattern
  genuinely needs one (documented).
- `match`/ternary used where PHP supports them; `clone($o,[...])` for clone-with.
- Full `run≡runvm≡real PHP 8.5` gate green; `clippy`/`fmt` clean; no new `Op`/`Value`.

## Files (expected)
- `src/transpile/matches.rs` — match lowering (the bulk: T1/T2).
- `src/transpile/expr.rs` — expression-`if`→ternary (T3); expr-match call into matches.rs.
- `src/transpile/program.rs` + `src/transpile/expr.rs` — clone-with native emission (T4); drop the
  `uses_clone_with` helper.
- `tests/differential.rs` — the gate already covers it (glob); add focused PHP-shape assertions if useful.

## Effort
~5–7 gated slices ≈ one focused modernization milestone. The match-lowering (T1/T2) is the bulk.
