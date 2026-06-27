# `as` → primitives matrix (Option 2, item a) — plan

> Design-locked 2026-06-27 (developer, via the decisions-review). Extends the checked `as` operator
> (currently class/interface/union only, `docs/specs/2026-06-26-m4-casting-conversion-design.md`) to
> **primitive targets**, using the **Unified, fallibility-typed** model. Byte-identity-gated
> (run≡runvm≡real PHP 8.5), incremental commits, no new `Op`, no `Value` change.

## Locked semantics (developer)
`x as T` for a PRIMITIVE T: result type tracks fallibility.
- **lossless / infallible → total `T`**
- **lossy or fallible → `T?`** (null, never a silent wrong value)
- **`as` is honest/loud — it does NOT inherit PHP's loose coercion** (diverges in 6 cells; the
  divergence is the value). `Convert.truncate` stays the named tool for explicit truncation.

### The matrix (source → target ⇒ result)
| source ＼ target | int | float | string | bool | decimal |
|---|---|---|---|---|---|
| **int**     | int *(id)* | float (widen) | string (toString) | bool (0=false) | decimal (widen) |
| **float**   | **int?** (exact-or-null) | float *(id)* | string | bool (0.0=false) | **decimal?** (shortest-str) |
| **string**  | **int?** (parseInt) | **float?** (parseFloat) | string *(id)* | **bool?** (strict "true"/"false") | **decimal?** (parse) |
| **bool**    | int (1/0) | float (1.0/0.0) | string ("true"/"false") | bool *(id)* | decimal (1/0) |
| **decimal** | **int?** (exact-or-null) | float (decimalToFloat) | string | bool (zero=false) | decimal *(id)* |
| **union/erased of primitives** | int? *(assert)* | float? | string (total toString) | bool? | decimal? |

*(id)* = identity ⇒ total `T` + `W-REDUNDANT-CAST` lint.

### The 8 remedies (vs PHP's surprising `(type)` cast)
1. `string as bool` → strict `bool?` ("true"/"false" only; **no** PHP truthiness, "false" is NOT true).
2. `int/float/decimal as bool` → total, explicit `!= 0` rule (documented, not hidden).
3. `bool as string` → `"true"/"false"` (Convert.toString), **not** PHP `(string)false == ""`.
4. `float/decimal as int` → **exact-or-null** (3.9→null), never silent truncate (use `Convert.truncate`).
5. `float as decimal` → shortest round-trip display string → `decimal?` (null on NaN/∞/overflow).
6. `string as int/float/decimal` → strict parse → `T?` (reject trailing junk; not PHP leading-number).
7. Blast radius: **single-source every cell** — reuse existing Convert/Text kernels where semantics
   match; only ~4 new kernels (exact-int, float→decimal, string→decimal, bool cells, string→bool).
8. **No new `Op`**: lower via a checker **span-keyed rewrite** to a leaf-qualified native call
   (`Member{Ident(q), name}`), resolved by `index_of_by_leaf` without an import — same mechanism as
   UFCS (`rewrite_ufcs`). Conversions reuse Convert/Text natives; assertions + new cells get new
   natives. Backends already execute native calls ⇒ run≡runvm by construction; transpiler emits the
   native's `php`.

### Boolean-context audit (developer asked; all Verified, no truthiness anywhere)
`if`/`else if`/`while`/`do-while`/`for(;c;)`/expr-`if`/`&&`/`||`/`!`/`match` guard/`assert`/
higher-order `(T)->bool` predicates all require a real `bool`; no C-ternary exists (expr-`if` only).
`for x in coll`, if-let, `??`/`?.`/`opt!` are correctly NOT boolean contexts. Nothing to change.

## Slices (each: TDD, 3-way byte-identity, guide example, commit green, no push)
- **S1 — concrete-primitive CONVERSIONS + identity lint.** Reuse `Convert.toFloat`/`intToDecimal`/
  `toString`, `Text.parseInt`/`parseFloat`; add `Convert.floatToIntExact`/`decimalToIntExact` (int?).
  Checker: primitive target no longer rejected — picks the cell, records the rewrite, types the result.
  `W-REDUNDANT-CAST` on `T as T`. Rewrite pass `rewrite_cast`. Cells: int↔float, int↔decimal,
  float→int?, decimal→int?, string→int?/float?, any→string. (Defer bool, decimal-from-float/string,
  assertions.) Example `examples/guide/as-primitives.phg`.
- **S2 — ASSERTION cells** (primitive-union / erased source → `T?`): new internal type-test natives
  (value-or-null by runtime variant). Smart-cast `if (var i = x as int)`.
- **S3 — bool cells** (numeric↔bool total, bool→string, **string→bool? strict**).
- **S4 — decimal extras** (`float as decimal?` shortest-string, `string as decimal?` parse) + close.

## Status
- [x] **S1 conversions + identity lint — DONE.** Cells: int→{float,decimal,string}, float→int?,
  decimal→{int?,float}, string→{int?,float?}, identity (`W-REDUNDANT-CAST`). New kernels
  `value::float_to_int_exact`/`decimal_to_int_exact` + natives `Convert.floatToIntExact`/
  `decimalToIntExact` (+ PHP helpers). Lowering = checker span-keyed rewrite (`cast_resolutions`)
  → leaf-qualified native call, applied by `rewrite_ufcs`'s `Cast` arm; identity stays `Expr::Cast`,
  each backend emits the value. Transpiler resolves the un-imported `Convert`/`Text` cast leaves via
  an `index_of_by_leaf` fallback (guarded: only those 2 leaves + not a user class — safe because the
  checker rejects user-written un-imported stdlib calls). Example `examples/guide/as-primitives.phg`.
  No new `Op`/`Value`; byte-identical run≡runvm≡real PHP 8.5.
- [x] **S2 assertions — DONE.** Primitive-union source → `T?` runtime assertion via internal natives
  `Convert.asInt`/`asFloat`/`asBool` (return value-or-null by runtime variant; arrow-IIFE PHP =
  single-eval). `as string` on a union stays total `toString`. **`as decimal` assertion deferred**
  (decimal's PHP carrier is a string — indistinguishable from a `string` union member; `is_*` can't
  tell them apart). Erased-generic sources also deferred. Example extended; if-let smart-cast works.
- [ ] S3 bool cells (numeric↔bool, bool→string, string→bool? strict)
- [ ] S4 decimal extras (`float as decimal?`, `string as decimal?`)
