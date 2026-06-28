# M-NUM S2 — decimal division + rounding (design)

> Status: design-locked (forks resolved with developer 2026-06-26). Slice S2 of M-NUM.
> Builds on S1 (`d5d161d`). Plan + Decisions Log: `docs/plans/2026-06-26-m-num-decimal.plan.md`.

## Goal
Exact, explicitly-rounded decimal division. Bare `decimal / decimal` is a **compile error**
(`E-DECIMAL-DIV`, hint → `Decimal.div`); division and re-scaling go through two natives with an
explicit target scale + rounding mode:

```phorj
import Core.Decimal;
decimal unit = Decimal.div(10.00d, 3d, 2, RoundingMode.HalfEven);   // 3.33
decimal cents = Decimal.round(2.345d, 2, RoundingMode.HalfUp);      // 2.35
```

## API (natives in module `Core.Decimal`)
- `Decimal.div(decimal a, decimal b, int scale, RoundingMode mode) -> decimal`
- `Decimal.round(decimal d, int scale, RoundingMode mode) -> decimal`
- `b == 0` ⇒ clean fault `"decimal division by zero"` (new `FaultKind`, byte-identical run≡runvm; PHP
  helper throws the same). `scale < 0` ⇒ fault `"decimal scale out of range"`. Any i128 overflow in the
  intermediate (`N`/`D`/result) ⇒ the existing `"decimal overflow"` fault.

## `RoundingMode` — injected enum (the [[core-json-and-injected-types]] pattern)
Inject `enum RoundingMode { HalfUp, HalfDown, HalfEven, Up, Down, Ceiling, Floor }` into the AST when
`Core.Decimal` is imported (mirror `cli::inject_json_prelude` — gated on the import, flows as an ordinary
enum, zero backend machinery; transpiles to a normal PHP enum/class). **Reference syntax = whatever the
existing enum convention is** (mirror an existing guide enum example + the Json injected enum — do NOT
invent new syntax; zero-payload variants follow the project's `V()`-construct rule if applicable). The
natives read the mode from the `Value::Enum` variant name; the transpiler passes the enum's PHP form to
the helper, which switches on it.

## The rounding algorithm (single-sourced concept — Rust i128 AND PHP bcmath implement it identically)
[Verified byte-identity enabler: BCMath `bcdiv`/`bcmod` truncate toward zero and `bcmod` takes the
dividend's sign — identical to Rust i128 `/`/`%`. So the quotient/remainder below match across backends.]

**Core primitive `round_div(n, d, mode) -> i128`** — round the exact rational `n/d` to an integer.
1. Normalise sign: if `d < 0` then `n = -n; d = -d` (now `d > 0`; quotient sign unchanged). `d == 0` is
   the division-by-zero fault (checked by the caller).
2. `q = n / d` (Rust trunc-toward-zero; PHP `bcdiv(n,d,0)`). `rem = n % d` (sign of `n`; PHP `bcmod(n,d)`).
   `s = sign(n)` ∈ {−1,0,1}.
3. If `rem == 0` ⇒ `q` (exact). Else apply `mode` using the **half-comparison without doubling**
   (avoids `2*rem` overflow): compare `|rem|` against `d − |rem|` (`d − |rem|` is always ≥ 0, safe):
   - **Down** (toward 0): `q`
   - **Up** (away from 0): `q + s`
   - **Ceiling** (→ +∞): `if n > 0 { q + 1 } else { q }`
   - **Floor** (→ −∞): `if n < 0 { q − 1 } else { q }`
   - **HalfUp** (ties away): `|rem| >= d−|rem|` ⇒ `q + s`, else `q`
   - **HalfDown** (ties toward 0): `|rem| >  d−|rem|` ⇒ `q + s`, else `q`
   - **HalfEven** (ties to even): `>` ⇒ `q+s`; `<` ⇒ `q`; `==` ⇒ `if q is odd { q + s } else { q }`
   All `q ± 1`/`q + s` are `checked_*` ⇒ overflow fault. `|rem|` = `rem.unsigned_abs()` style (handle MIN).

**`Decimal.div(a=(ua,sa), b=(ub,sb), scale, mode)`** ⇒ `Value::Decimal { unscaled: round_div(N, D, mode), scale }`
where `N = ua * 10^(sb + scale)`, `D = ub * 10^sa` (both `checked_mul`/`checked_pow` ⇒ overflow fault;
`10^k` needs `k ≥ 0`, so `sb+scale` and `sa` fit). `ub == 0` ⇒ division-by-zero fault.

**`Decimal.round(d=(ud,sd), scale, mode)`**:
- `scale >= sd` ⇒ rescale up exactly: `ud * 10^(scale − sd)` (checked) at `scale` (no rounding).
- `scale <  sd` ⇒ `round_div(ud, 10^(sd − scale), mode)` at `scale`.

## Transpile (BCMath helpers)
`Decimal.div`/`Decimal.round` ⇒ gated `__phorj_dec_div`/`__phorj_dec_round` helpers. They compute `N`,
`D` with `bcmul`/(10^k via `bcpow` or string), then `round_div` via `bcdiv($N,$D,0)` (q, trunc-to-zero ✓)
+ `bcmod($N,$D)` (rem ✓) + `bccomp(|rem|, bcsub($d,|rem|))` for the half-decision, switching on `$mode`
exactly as the Rust table. Result i128-bounds-checked (reuse S1's `__phorj_dec_check`) ⇒ identical
overflow fault; explicit `"decimal division by zero"` / `"decimal scale out of range"` throws.

## Checker
- New binary-op arm: `decimal / decimal` (and `%`?) ⇒ `E-DECIMAL-DIV` (hint: use `Decimal.div`). `/` only —
  decimal `%` is also rejected with the same code (no decimal-modulo this slice).
- `Decimal.div`/`round` are ordinary natives (typed sig with the injected `RoundingMode` named type).

## New `Op`? — NO.
Division is a **native call** (`Op::CallNative`), not an operator — so no `DivD`/`RemD` op is needed
(S1's spec speculated them; S2 supersedes that — the explicit-call decision removes the operator path).
`RoundingMode` is an injected enum (rides existing enum ops). **Zero new `Op`, zero new `Value`.**

## Byte-identity strategy
`round_div` is integer arithmetic on `(N, D)`; Rust i128 and PHP bcmath agree on trunc-toward-zero
division + dividend-signed remainder (verified), so every mode matches. Overflow + div-by-zero faults are
run≡runvm (FaultKind) and the PHP helper throws the same message (faults stay out of the byte-identity
example set per the "examples are Ok-only" rule — captured in KNOWN_ISSUES). `examples/guide/decimal-div.phg`
exercises all 7 modes on a tie value (e.g. `2.345d` → scale 2) + a non-tie + `Decimal.div` + div-by-zero
note, byte-identical run≡runvm≡real PHP 8.5.

## Diagnostics
`E-DECIMAL-DIV` (bare `/` or `%` on decimals). Reuse existing for native arg-type errors. `phg explain`.

## Out of scope (later)
Decimal `%` operator/modulo native; `Decimal.div` with an inferred default scale (deliberately omitted —
explicit is the whole point). Float predicates/conversions = S3; math breadth = S4.
