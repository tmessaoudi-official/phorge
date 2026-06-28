# M-NUM S1 — `decimal` primitive core (design)

> Status: design-locked (forks resolved with developer 2026-06-26). Slice S1 of M-NUM.
> Plan + Decisions Log: `docs/plans/2026-06-26-m-num-decimal.plan.md`.

## Goal
A statically-typed `decimal` **primitive** for exact money/fixed-point math — making
float-for-currency a *compile choice*, not a silent bug. Headline ergonomics:

```phorj
package Main;
import Core.Console;
function main() {
    decimal price = 19.99d;
    int qty = 3;
    decimal total = price * qty;          // 59.97d  (decimal * int ⇒ decimal)
    Console.println("total = {total}");   // total = 59.97
}
```

## Representation (LOCKED: i128 fixed-point)
`Value::Decimal { unscaled: i128, scale: u8 }` — value = `unscaled × 10^(-scale)`.
- `19.99d` ⇒ `{ unscaled: 1999, scale: 2 }`.
- Std-only, native i128, legible. Covers all realistic money (~10^36 at scale 2).
- **Overflow ⇒ a clean fault** (`"decimal overflow"`, a `FaultKind`), byte-identical across both
  Rust backends AND the emitted PHP (the helper bounds-checks against i128 range and faults).
- True arbitrary-precision decimal is **M-NUM-2** (shares the hand-rolled bignum core with `BigInt`).

## Literal grammar (LOCKED: `1.50d` suffix, text-parsed)
- Lexer recognises a numeric literal immediately followed by `d` (no space): `19.99d`, `0d`, `100d`,
  `1.500d`. **Parse the literal TEXT** so trailing zeros set the scale: `1.50d` ⇒ scale 2,
  `1.500d` ⇒ scale 3, `100d` ⇒ scale 0.
- New `Expr::Decimal { unscaled: i128, scale: u8, span }` and `Type::Decimal` annotation;
  `Ty::Decimal`.
- A literal whose digits overflow i128 ⇒ a parse/lex error (not a runtime fault — known at compile time).
- `e`-exponent on a `d` literal is **rejected this slice** (`1e3d` → error); plain-int/float exponent
  unaffected. (Exponent-to-decimal is a later nicety, not core.)

## Construction from string (LOCKED: ships in S1)
`Decimal.of(string) -> decimal?` — a native (module `Core`, leaf reused as a built-in static like
`Int.parse`). Parses the same grammar as the literal at runtime; returns `null` on malformed input or
i128 overflow (composes with S2 `??`). PHP: a gated `__phorj_dec_of($s)` helper validating the grammar
(PCRE, tier-1) and normalising; on failure returns the null sentinel.

## Type rules
- `Ty::Decimal` is its own primitive. **No implicit decimal↔float coercion** (the whole point — mixing
  float into money is the bug we prevent). Cross-type assignment `float ↔ decimal` ⇒ `E-TYPE`.
- **`decimal` op `int` ⇒ `decimal`** (int widens to decimal scale 0). This is the one ergonomic
  coercion (qty/count math). `int` op `decimal` symmetric. `decimal` op `float` ⇒ **error**
  (`E-DECIMAL-FLOAT-MIX`, hint: convert explicitly).
- `assignable_with`: `decimal` assignable only to `decimal` (and the int-widening is op-level, not
  assignment-level — an `int` is NOT assignable to a `decimal` *slot* without `19d`/`Decimal.of`; keeps
  the type wall honest. Re-evaluate if too strict during S1 build.)

## Operator semantics (S1: `+ - *` + comparison/equality; `/` is S2)
Single-sourced kernels in `value.rs`; both backends call them; the PHP helper mirrors them.
- **add / sub:** result scale = `max(s_a, s_b)`. Align both unscaled to max scale (`× 10^(Δ)`), then
  `checked_add`/`checked_sub`; overflow ⇒ fault. (Matches `bcadd($a,$b,max)`.)
- **mul:** result scale = `s_a + s_b`. `unscaled_a checked_mul unscaled_b`; overflow ⇒ fault.
  (Matches `bcmul($a,$b,s_a+s_b)` — exact, no truncation.)
- **compare / `==` / `!=` / `< > <= >=`:** **numeric, scale-insensitive** — align to `max` scale, compare
  unscaled. So `1.50d == 1.5d` is `true`. (Matches `bccomp`.) Equality returns `bool`.
- **unary `-`:** negate unscaled (no negative zero: `-(0d)` renders `0`).
- Scale alignment itself can overflow i128 (aligning a near-max value up) ⇒ same fault.

## Display / `toString`
Render `unscaled`/`scale` as a decimal string with **exactly `scale` fractional digits** (BCMath pads):
`{1999,2}` → `"19.99"`, `{15,4}` → `"0.0015"`, `{1500,3}` → `"1.500"`, `{100,0}` → `"100"`.
Negative: leading `-`; **never** `-0`. Single-sourced `fmt_decimal(unscaled, scale)` used by both
backends; PHP rendering already matches (BCMath strings render this way, and `(string)` of a bcmath
result is identical).

## Transpile (LOCKED: BCMath via gated runtime helpers)
[Verified: BCMath compiled into PHP 8.5.7 floor AND 8.6-dev canary, works under `php -n`; GMP/intl are
NOT under `-n`; `brick/math` needs Composer autoload → unusable in the oracle.]
- Literal `19.99d` ⇒ PHP string literal `"19.99"` (BCMath operates on strings).
- `a + b` (decimal) ⇒ `__phorj_dec_add($a, $b)`; `-`/`*`/compare similarly. Each helper derives operand
  scales at runtime (`scale = strlen(substr(strrchr($s,'.'),1))`, 0 if no dot), computes the result
  scale per the rules above, calls `bcadd/bcsub/bcmul/bccomp` with that scale, and **bounds-checks the
  result against i128 range** → throws the same `decimal overflow` fault as Rust.
- `decimal op int`: the int operand is stringified (`(string)$i`) before the helper; scale 0 ⇒ rules hold.
- Gated `uses_dec_add/_sub/_mul/_cmp/_of` bool fields on the Transpiler; emitted once in
  `emit_runtime_helpers`. `Decimal.of` ⇒ `__phorj_dec_of`.
- `emit_type(Ty::Decimal)` ⇒ PHP `string` (BCMath's carrier; PHP has no native decimal).

## Byte-identity strategy (the spine)
The i128 Rust kernel and the BCMath helper compute identical exact decimal results **while in range**.
The ONLY divergence is i128 overflow — closed by the helper's bounds-check faulting identically. The
S1 guide example bakes in a value that exercises mul-scale growth + decimal*int + render padding, and a
fault-case is captured in KNOWN_ISSUES (faults can't be a runnable example). `tests/differential.rs`
gates `examples/guide/decimals.phg` (run≡runvm≡real PHP 8.5).

## New `Op`? — YES: 3 (`AddD`, `SubD`, `MulD`).
[Verified via integration map: the VM uses **type-specialized** arithmetic ops — `AddI/SubI/MulI` +
`AddF/SubF/MulF` — chosen by the compiler from operand `NumTy`, NOT a value-polymorphic `Op::Add`. So
decimal `+ - *` need parallel `Op::AddD/SubD/MulD`.] Each rides the **three coupled matches in one
commit** (`chunk.rs` `Op` enum + `stack_effect`; `vm/exec.rs` `exec_op`; `compiler` emit) — the
`op-variant-match-coupling` invariant. `DivD`/`RemD` are deferred to S2 (division + rounding).

- **Literals** ride `Op::Const` (decimal joins the constant-pool value kinds, like float/bytes) — no op.
- **Comparison / `==` / `< >` …** stay **kernel-based**: extend `eq_val_rec` + `compare_ord` in
  `value.rs` with a `Decimal` arm; the existing `Op::Eq`/compare ops call them — **no new op**.
- **`NumTy`** (compiler) gains `Decimal`; **`CTy`** gains `Decimal` (so a decimal map-index/field-read
  result specialises on the VM — the CTy-operand invariant); `resolve_cty("decimal")`/`num_ty` wired.
- **`decimal op int`:** the compiler emits `AddD/SubD/MulD` when *either* operand is decimal; the shared
  kernel accepts mixed `(Value::Decimal, Value::Int)` and widens the int to scale 0. (Keeps both
  backends single-sourced; the VM pops two `Value`s and the kernel coerces — no separate widen op.)
- **`Decimal.of`** rides `Op::CallNative`.

## Diagnostics
`E-DECIMAL-FLOAT-MIX` (decimal⊕float), `E-DECIMAL-LITERAL` (literal overflows i128 / malformed), reuse
`E-TYPE` for assignment. All via `Diagnostic::new` + `phg explain`.

## Out of scope for S1 (later M-NUM slices / M-NUM-2)
`/` + rounding modes (S2); float predicates / `toFloat`/`toInt`/`intdiv` (S3); `Core.Math` breadth +
`number_format` (S4); arbitrary precision, `BigInt`, `Money`+currency (M-NUM-2).
