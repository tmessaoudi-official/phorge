# M-NUM S3 — float predicates + numeric conversions (design)

> Status: design-locked (forks resolved with developer 2026-06-26). Slice S3 of M-NUM.
> Builds on S1 (`d5d161d`) + S2 (`1a2774d`). Plan: `docs/plans/2026-06-26-m-num-decimal.plan.md`.

## Goal
Round out the numeric surface: detect float special values, and convert explicitly between
`int`/`float`/`decimal`. All additive natives — **no new `Op`, no new `Value`** (reuses the native
registry + S2's `Value::Null`/optionals). Every S3 primitive is PHP **core** (verified available under
`php -n`: `is_nan`/`is_finite`/`is_infinite`/`intdiv`/`floor`, `NAN`/`INF`) — **no extension** (unlike
S1/S2's BCMath; nothing to load).

## `Core.Math` additions (float predicates + special values + intdiv)
- `Math.isNan(float) -> bool` → PHP `is_nan`
- `Math.isFinite(float) -> bool` → PHP `is_finite`
- `Math.isInfinite(float) -> bool` → PHP `is_infinite`
- `Math.nan() -> float` → PHP `NAN`; `Math.infinity() -> float` → `INF`; `Math.negInfinity() -> float` → `-INF`
- `Math.intdiv(int, int) -> int` → PHP `intdiv`. **Fault** on divisor 0 (`"division by zero"`) and on
  `intdiv(i64::MIN, -1)` overflow (`"integer overflow"`) — Rust `checked_div` → fault; PHP `intdiv`
  throws `DivisionByZeroError`/`ArithmeticError`. Fault is run≡runvm (FaultKind), PHP helper/`intdiv`
  throws the same class — not a runnable example (KNOWN_ISSUES).
- **Display caveat (existing):** float rendering can diverge between Rust `{}` and PHP `echo` for
  non-exactly-representable values (KNOWN_ISSUES) — the guide example keeps printed floats to
  exactly-representable values; `nan()`/`infinity()` are exercised through the *predicates* (`isNan`),
  never printed (PHP prints `NAN`/`INF`, Rust prints `NaN`/`inf` — would diverge; so don't print them).

## `Core.Convert` module (new — the conversions home)
- `Convert.toFloat(int) -> float` → `(float)$i`. Total. (i64→f64 round-to-nearest, identical both sides.)
- `Convert.toInt(float) -> int?` → **null** on NaN / ±Inf / out-of-i64-range; else truncate toward zero.
  Rust: `if x.is_finite() && (i64::MIN as f64..=i64::MAX as f64).contains(&x.trunc()) { Some(x.trunc() as i64) } else { None }`.
  PHP helper `__phorge_float_to_int`: `is_finite($f) && $f >= -9.2233720368547758E18 && $f < 9.2233720368547758E18 ? (int)$f : null`
  (the upper bound is exclusive because i64::MAX isn't exactly representable as f64 — pick the bound that
  makes Rust and PHP agree; gate it with a probe value near the edge). **Avoids** PHP `(int)NAN`=0+warning.
- `Convert.intToDecimal(int) -> decimal` → the decimal `{unscaled: i, scale: 0}`; PHP `(string)$i` (decimal carrier).
- `Convert.decimalToFloat(decimal) -> float` → parse the decimal's rendered string to f64; PHP `(float)$decstr`.
  Lossy by nature; printed results in the example must be exactly-representable.
- `Convert.decimalToInt(decimal) -> int?` → truncate toward zero (drop the fraction); **null** if the
  integer part is out of i64 range. For *rounded* decimal→int, compose `Decimal.round(d, 0, mode)` then
  `decimalToInt` — keeps `Core.Convert` decoupled from the `RoundingMode` injection (which is gated on
  `import Core.Decimal`). PHP: integer part via BCMath (`bcdiv`/the decimal is a string) → range-check → (int)/null.
  NOTE: `decimalToInt` touches the decimal string → if it needs BCMath, it inherits S1's bcmath loading;
  prefer a pure string-split of the integer part (before `.`) + `intval` with range check to avoid bcmath here.

## Documentation task (N-int-width)
Document that Phorge `int` is a **64-bit signed integer** (i64), pinned (not PHP's platform-width `int`):
add to `FEATURES.md`/`docs/INVARIANTS.md` (whichever holds the type model) + a `KNOWN_ISSUES` note that
`int` arithmetic overflow is a checked fault (already true). One-paragraph doc, no code.

## Byte-identity strategy
All S3 conversions are deterministic and (except float display) exactly representable. The two risk
points: (1) **float→int edge** (NaN/Inf/range) — closed by returning `int?` null with identical
guards on both sides; gate with an example covering a normal value, a fractional truncation, and a
null case (NaN via `Math.nan()`, an out-of-range via a huge float). (2) **float display** — never print
a non-representable float; print `int`/`decimal`/`bool` results instead. `examples/guide/numeric-convert.phg`
(or extend an existing one) byte-identical run≡runvm≡real PHP 8.5; faults (intdiv/0) → KNOWN_ISSUES.

## New `Op`? — NO. New `Value`? — NO.
Pure native-registry additions (`Op::CallNative`), reusing `Value::Null` (S2) for the `int?` returns and
`Value::Decimal` (S1). `Core.Convert` is a new module key in the registry; `Core.Math` gains entries.

## Diagnostics
Reuse native arg-type errors. No new codes expected (conversions are well-typed natives).

## Out of scope (S4 / later)
`Core.Math` breadth (`round`/`sign`/`clamp`/`gcd`/`log`/`exp`/trig/`PI`/`E`) + `number_format` = S4.
`floatToDecimal` deliberately omitted (float→decimal is lossy/surprising — use `Decimal.of(string)`).
