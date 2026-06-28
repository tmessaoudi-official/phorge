# Transpile Modernization (Track 1) Plan

> Make the Phorj→PHP transpiler emit **idiomatic, modern PHP** — native `match` expressions,
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
`__phorj_float`/`_str`/`_add`/`_div`/`_rem` (PHP loose semantics ≠ Rust); enum→class-hierarchy (PHP
8.1 `enum` can't carry per-variant payloads). **Real gaps:**
1. **`match` → `if/elseif/else` chains** (both literal and type/union matches) — PHP's native `match`
   is unused entirely.
2. **Expression-position `match`/`if` → IIFEs** (`(function() use(...){ if(...) return …; })()`) where
   PHP has true `match`/ternary expressions. The ugliest output.
3. **`clone … with` → `__phorj_clone_with` helper** though the floor is now PHP 8.5, where native
   two-arg `clone($o, [...])` exists (the helper's comment still says "8.4" — stale since the bump).

## Slices (each green + byte-identity-gated)
| Slice | Work | Risk | Notes |
|---|---|---|---|
| **T1** | Literal/value `match` → PHP `match($x){ lit => …, _ => … }` | Med | PHP `match` is strict `===`; Phorj literal match is `==` on primitives — verify they agree for int/string/bool. Exhaustive Phorj matches → no `default` arm (the checker proved totality; PHP throws `\UnhandledMatchError` on the unreachable no-match, same as today). |
| **T2** | Type/guard `match` → `match(true){ $x instanceof T => …, cond => … }` | Med | A true expression → also kills the IIFE for these. Binding patterns: reference the scrutinee var directly. Struct-destructuring patterns can't be a `match` arm → keep the imperative/IIFE fallback for those. |
| **T3** | Expression-position `if` → ternary `?:` | Low | Replaces the IIFE for `if (c) { e } else { e }` in value position. |
| **T4** | `clone … with` → native `clone($o, [...])`; drop `__phorj_clone_with` | Low | Floor is 8.5; native two-arg `clone` available. Verify the bare-`clone` (no overrides) path too. |
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

## T6 — operand-type specialization (added 2026-06-25, developer-approved)
- [2026-06-25] AGREED: build T6 now — eliminate `__phorj_add`/`_div`/`_rem` and shrink
  `__phorj_str` (float-only) by resolving operand *types* in the transpiler (mirroring the
  bytecode compiler's proven `ctype`/`CTy`). Native emission: `string + string` → `.`,
  numeric `+` → `+`, int `/` → `intdiv`, float `/` → `/`, int `%` → `%`, float `%` → `fmod`;
  interpolation of a statically-typed string/int → direct, bool → inline ternary, float →
  `__phorj_float`. **Design: the runtime helper stays as a FALLBACK** for any operand whose
  type the resolver can't determine (`uses_*` flag set only on fallback) — so byte-identity is
  never at risk (the helper is the safety net; the native operator is the optimization). Fully
  gated by `run≡runvm≡real PHP 8.5`. Irreducible helpers (float Ryū, range, reflection,
  init_statics) stay.

## T6b — field / variant-payload type resolution (added 2026-06-25, developer-approved)
- [2026-06-25] AGREED: extend the T6 resolver to eliminate the remaining `__phorj_add`/`_str`
  fallbacks. Add `OpKind::Class(name)`; track class-typed locals/params/`this`; build
  class-field + variant-payload type maps; resolve `p.x`/`this.x` field reads, constructor
  results, and `Pass(s)` variant-payload match bindings → native operators. `__phorj_float`
  stays (irreducible Ryū). Oracle-gated.

## Status — COMPLETE (2026-06-25)
T1/T2/T3/T4/T5/T6/T6b/T6c all shipped (commits d158e6d, 06463fb, 4fe98e5, 1411883, 3b0560f,
c3a591e, 61d17be). Helpers **fully eliminated**: `__phorj_clone_with`, `__phorj_unwrap`,
`__phorj_div`, `__phorj_rem`. **Reduced to niche fallbacks**: `__phorj_add` (→3),
`__phorj_str` (→ list/map-index results, const/static reads, native-call results, catch-var
field reads). **Irreducible** (kept by design): `__phorj_float` (Ryū shortest-round-trip — the
hard floor), `__phorj_range`, reflection (`__phorj_kind`/`_class_name`/`_reflect_of`),
`__phorj_init_statics`. All gated `run≡runvm≡real PHP 8.5` byte-identical.

### Optional follow-up (T6d, not scheduled) — diminishing returns vs the `__phorj_float` floor
Resolve the last `__phorj_str`/`__phorj_add` fallbacks by adding: list/map element kinds
(`OpKind::List/Map`, mirroring the compiler's `CTy`), const/static read kinds, and native-call
return kinds (from the native registry's ret sig). Each is a smaller niche; `__phorj_float`
remains regardless wherever a float is displayed.

## T6d — finish helper niches (developer-approved 2026-06-25, then M-Lift)
- [2026-06-25] AGREED: do T6d (resolve the remaining __phorj_str/_add fallbacks) THEN start
  M-Lift. Developer pushes manually. T6d sub-pieces by census frequency: native-call return kinds
  (count/array_sum/strtoupper/implode…, biggest), list/map index element kinds, const/static read
  kinds, catch-var field reads. __phorj_float stays (irreducible floor).

## T6d — COMPLETE (2026-06-25, commit e3d4392)
Index element kinds (`OpKind::List/Map`), native-call return kinds (registry `Ty`→`OpKind`),
const/static reads (bare class-name ident → `Class`), catch-var typing. Final census across all
examples: `__phorj_float` 24 (irreducible floor — now the largest), `__phorj_str` 18,
`__phorj_range` 5, `__phorj_add` 3, reflection/init 1 each. Remaining `_str`/`_add` are genuinely
dynamic (closure-call results, `Reflect.className`, expr-position match/getenv) — the helper-as-safe-
fallback path. **Track 1 + T6/T6b/T6c/T6d fully COMPLETE.**
