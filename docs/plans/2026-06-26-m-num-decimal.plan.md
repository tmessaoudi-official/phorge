# M-NUM (decimal / money) Plan

## Decisions Log
- [2026-06-26] AGREED: Next milestone after M4 = **M-NUM (decimal/money)**, chosen over M-TIME / M8 hardening (developer-confirmed; rationale: on-philosophy correctness win, determinism-clean/fully byte-identity-gateable, self-contained, highest everyday PHP-parity demand).
- [2026-06-26] AGREED: **Transpile target = BCMath**, NOT `brick/math`. [Verified: `php -n -m` + `function_exists("bcadd")` on the 8.5.7 floor AND the 8.6-dev canary both return BCMath present under `-n`; `brick/math` is a Composer package needing autoload, which `php -n` cannot load.] The SSOT's "maps to brick/math" is corrected here.
- [2026-06-26] AGREED: **Representation = i128 fixed-point** `{ unscaled: i128, scale: u8 }`. Std-only, legible, fast; covers all realistic money (~10^36 at scale 2). **Overflow = a clean byte-identical fault** mirrored by a bounds-check in the emitted PHP helper. **True arbitrary-precision decimal is DEFERRED to M-NUM-2**, where it shares the hand-rolled bignum core with the already-deferred `BigInt`.
- [2026-06-26] AGREED: **Literal syntax = `1.50d` suffix** (lex the literal TEXT so scale is preserved: `1.50` ⇒ scale 2), **plus `Decimal.of(string) -> decimal?`** for dynamic/string input. Both ship.
- [2026-06-26] LOCKED (SSOT, not re-opened): `decimal` is a **primitive** (`Ty::Decimal` / `Value::Decimal` + operator support across all backends), not a stdlib class.
- [2026-06-26] AGREED (S2): **bare `decimal / decimal` is a compile error** (`E-DECIMAL-DIV`, hint → `Decimal.div`). Division goes through **`Decimal.div(decimal a, decimal b, int scale, RoundingMode mode) -> decimal`** + **`Decimal.round(decimal d, int scale, RoundingMode mode) -> decimal`**. Rationale: removes the silent-precision surprise (on-philosophy), and is *more* PHP-familiar (devs already pass scale to `bcdiv`). Accepted trade-off: breaks `+ - * /` symmetry.
- [2026-06-26] AGREED (S3): **numeric conversions live in a dedicated `Core.Convert` module** (matches SSOT L-convert; groups cross-type conversions; consistent with namespace-everything). **`NaN`/`Infinity` + float predicates are `Core.Math` functions** (`nan()`/`infinity()`/`negInfinity()`, `isNan`/`isFinite`/`isInfinite`), not keywords/literals. `Convert.toInt(float) -> int?` returns **null** on NaN/Inf/out-of-i64-range (avoids PHP `(int)`'s quirky 0+warning — verified — and I control both sides for byte-identity). `Convert.decimalToInt -> int?` truncates toward zero (compose with `Decimal.round(d,0,mode)` for explicit rounding — keeps Convert decoupled from the RoundingMode injection).
- [2026-06-26] AGREED (S2): **full 7-mode `RoundingMode` set** — `HalfUp`, `HalfDown`, `HalfEven` (banker's), `Up` (away from zero), `Down` (truncate toward zero, = raw `bcdiv`), `Ceiling` (toward +∞), `Floor` (toward −∞). A `RoundingMode` enum injected when `Core.Decimal` is imported (the [[core-json-and-injected-types]] injected-type pattern). [Verified byte-identity enabler: BCMath `bcdiv`/`bcmod` truncate toward zero, `bcmod` takes the dividend sign — identical to Rust i128 `/`/`%` — so a (quotient, remainder)-based rounding algorithm matches across all three backends.]

## Scope (from SSOT `docs/specs/2026-06-21-php-parity-and-beyond.md`)
In M-NUM: N-decimal (headline), N-decimal-rounding (explicit modes), N-float-predicates
(`isNan`/`isFinite`/`isInfinite` + `NaN`/`Infinity`), N-intdiv, N-int-conv (`toFloat`/`toInt`),
N-int-width (pin `int`=i64, document), N-float-exponent (`1e6` — already lexed, verify), G-math-breadth
(`round`/`sign`/`clamp`/`gcd`/`log`/`exp`/trig/`PI`/`E`), M-number-format (non-locale `number_format`).
Deferred to **M-NUM-2**: `BigInt`, arbitrary-precision decimal, composite `Money`+currency.

## Slice plan (each ships green + byte-identical run≡runvm≡real-PHP + a guide example, same change)
- **S1 — decimal primitive core (headline): ✅ COMPLETE.** `Ty::Decimal`/`Value::Decimal{i128,u8}`,
  `19.99d` literal (text-parsed scale, `1e3d`/overflow → `E-DECIMAL-LITERAL`), `Decimal.of(string) ->
  decimal?` (`Core.Decimal`), exact `+ - *` (single-sourced `value::decimal_add/sub/mul`, mixed
  `decimal⊕int` widen, scale rules, `"decimal overflow"` fault), numeric scale-insensitive
  compare/eq, unary `-`, scale-padded render. Three VM ops `AddD/SubD/MulD`; `NumTy::Decimal`/
  `CTy::Decimal`; BCMath transpile via gated `__phorge_dec_add/_sub/_mul/_of` (i128-bounds-checked).
  `E-DECIMAL-FLOAT-MIX` rejects a float mix. `examples/guide/decimals.phg` byte-identical
  run≡runvm≡real PHP 8.5; 1151 workspace tests green, clippy + fmt clean.
- **S1 — decimal primitive core (original spec):** `Ty::Decimal`/`Value::Decimal{i128,u8}`; `1.50d` literal lex
  + `Decimal.of(string)`; exact `+ - *`; comparison/equality; scale-aligning rules; `toString`; mixed
  `decimal`/`int` coercion rule; transpile to BCMath (`bcadd`/`bcsub`/`bcmul`/`bccomp`). Overflow fault.
- **S2 — division + rounding: ✅ COMPLETE (`1a2774d`).** Bare `decimal /`/`%` → `E-DECIMAL-DIV`;
  `Decimal.div(a,b,scale,mode)` + `Decimal.round(d,scale,mode)` natives; injected 7-mode `RoundingMode`
  enum; single-sourced `value::round_div` (trunc-toward-zero q/rem, all 7 modes, checked i128); BCMath
  `__phorge_dec_div`/`_round` (PHP-8.4 builtin-`RoundingMode` collision dodged via `RoundingMode_`
  mangle); div-by-zero / scale-out-of-range / overflow faults. `examples/guide/decimal-div.phg`
  byte-identical run≡runvm≡PHP 8.5. **CI fix (`63ba3f7`):** decimal transpile emits BCMath, which the
  `php -n` oracle disables (shared ext) → CI red on S1; harness now loads it via `-d extension=bcmath`
  (`php_n_args`, probe-gated) + `setup-php extensions: bcmath`. See [[transpile-no-ini-extensions]].
- **S3 — float predicates + numeric conversions:** `isNan`/`isFinite`/`isInfinite`, `NaN`/`Infinity`,
  `toFloat`/`toInt`/`Decimal.of`, `intdiv`; document `int`=i64.
- **S4 — math breadth + number_format:** `Core.Math` round/sign/clamp/gcd/log/exp/trig/PI/E (float
  abs/min/max already exist); non-locale `number_format`.

## Formal Plan — S1 (decimal primitive core)
Integration map (file:line) in the session; design in `docs/specs/2026-06-26-m-num-s1-decimal-core-design.md`.
Build order (follow the compiler's non-exhaustive errors after step 1):
1. **Data model:** `Value::Decimal{i128,u8}` (value.rs), `Ty::Decimal` (types.rs), `Expr::Decimal`/
   `Pattern::Decimal`/`Type` reuse `Named("decimal")` (ast), `TokenKind::Decimal` (token.rs). Build → fix
   every flagged exhaustive match (walk.rs, casing.rs, rewrite_* comments, value const_literal, type_name).
2. **Kernels (value.rs):** `decimal_add/sub/mul` (scale rules: add/sub=max, mul=sum; `checked_*` → overflow
   fault), `fmt_decimal`, `eq_val_rec` + `compare_ord` Decimal arms (numeric/scale-insensitive), neg.
3. **Ops:** `AddD/SubD/MulD` — chunk.rs `Op`+`stack_effect`, vm/exec.rs `exec_op`, compiler emit (3 matches).
4. **Lexer:** `scan_number` `d`-suffix → `TokenKind::Decimal`, text-parsed scale; reject `1e3d`; overflow→err.
5. **Parser:** `TokenKind::Decimal` → `Expr::Decimal` (exprs.rs) + `Pattern::Decimal` (patterns.rs).
6. **Checker:** `Expr::Decimal`→`Ty::Decimal` (expr.rs); binary-op arm (decimal⊕decimal, decimal⊕int⇒decimal,
   decimal⊕float⇒`E-DECIMAL-FLOAT-MIX`); unary neg; interpolation; resolve `"decimal"`; casing; is_builtin.
7. **Compiler:** `NumTy::Decimal`, `CTy::Decimal`, `resolve_cty`, `ctype`, `num_ty`, literal `emit_const`,
   binary-op emit chooses `AddD/SubD/MulD` when either operand is decimal.
8. **Interpreter:** literal eval + arithmetic dispatch calling the value.rs kernels (mixed decimal/int).
9. **Transpiler:** literal→PHP string; `emit_type(decimal)`→`string`; arithmetic→gated `__phorge_dec_*`
   helpers (runtime scale derivation + BCMath + i128 bounds-check fault); `Decimal.of`→`__phorge_dec_of`.
10. **Native:** `Decimal.of(string)->decimal?` in the registry.
11. **Tests/example:** failing checker+parser tests first (TDD); make `examples/guide/decimals.phg`
    byte-identical run≡runvm≡PHP-8.5; KNOWN_ISSUES overflow-fault note; README + CHANGELOG; commit green.

## Byte-identity strategy (the spine)
i128-fixed-point Rust ops and BCMath agree on exact decimal arithmetic while in range. The ONLY
divergence risk is i128 overflow (Rust faults; BCMath would keep going) — closed by emitting a
bounds-checked PHP helper that faults identically. Same discipline as `__phorge_div`/`__phorge_rem`.
Every S* example is gated by `tests/differential.rs` (run≡runvm≡real PHP 8.5).
