# Feasibility Spike — `Core.Random` (SEEDED, deterministic PRNG)

**Verdict: ADOPT-NOW. Tier A (pure), std-only, no new VM Op. Feasibility ~88%.**
The starting hypothesis ("Tier A pure IF seeded, xorshift/PCG hand-rolled, must transpile to a PHP RNG
with identical sequence") is **confirmed with one sharp correction**: the algorithm MUST be
**shift/xor only (xorshift64 / xoshiro256**)** — a **multiply-based PRNG (LCG, PCG, splitmix64,
xorshift\*) is REJECTED** because PHP integer multiplication of two large ints silently promotes to
`float` and loses precision, breaking the sequence on the very first step. All claims below are
verified against the real PHP 8.5.7 oracle (`/stack/tools/phpbrew/php/php-8.5.7/bin/php -n`) and a
matched Rust `rustc -O` program.

---

## 1. The determinism partition — this is Tier A, not Tier B

A SEEDED PRNG with a Phorj-owned algorithm is a **pure function of (seed, call sequence)**. It reads
no clock, no OS entropy, no environment. It therefore belongs in the byte-identity spine exactly like
`Core.Hash` (hand-rolled digests) and is gated by `tests/differential.rs` like every other example.

- `pure: true` on every `NativeFn` entry (NOT the `Process`/`Env` quarantine path).
- It is the *opposite* of the prior-art "Core.Random (true/secure) — TIER B + POLICY-BLOCKED" line.
  That blocked item is `random_bytes`/CSPRNG; **this** spike is the seeded sibling, which is the canonical
  Tier-A example named in the partition brief.
- Crypto boundary respected: a seeded non-crypto PRNG is fine to hand-roll; we are NOT hand-rolling
  secure RNG / HMAC / password hashing (those stay Tier B → PHP `random_int`/`hash_hmac`).

---

## 2. THE BLOCKER, proven and avoided: PHP `*` promotes to float

The single thing that decides the algorithm. Verified on the oracle:

```php
php -n -r '$a=6364136223846793005; $b=1442695040888963407; var_dump($a*$b);'
// float(9.181507769685582E+36)   <-- precision LOST. Not a 64-bit wrap.
```

Rust `i64::wrapping_mul` would instead give an exact wrapped 64-bit result. **They diverge on step 1.**
PHP has 64-bit signed ints (`PHP_INT_SIZE` = 8, verified) but **no native u64 and no wrapping multiply**;
`*` overflows to `float`. Emulating a 64-bit wrapping multiply in pure PHP core (no GMP/BCMath in the hot
path — BCMath is string-based and would still need careful masking, and is slow) is possible but ugly and
fragile. **Decision: forbid multiply in the PRNG core.** This eliminates LCG, PCG, splitmix64, and
xorshift\* (the star is a final multiply). It leaves the shift/xor family, which is sufficient and
high-quality (xoshiro256\*\* uses a multiply only in its *output function* — use xoshiro256\*\* with a
**rotate-based** output, or plain xorshift64, or xoshiro256\*\*'s non-multiply variant; recommend
**xoshiro256++** which uses only rotate+add+xor in the output, no multiply).

> Recommended core: **xoshiro256++** (4×i64 state, output = rotl(s0+s3, 23) + s0). `+` is addition — see
> §3, addition wraps cleanly in BOTH languages when masked, OR use **xorshift128+**/**xorshift64** which
> are shift/xor only. For maximum byte-safety with zero arithmetic-overflow surface, **xorshift64**
> (single i64 state, `^`/`<<`/`>>` only) is the safest first ship; xoshiro256++ is the quality upgrade
> once the `+`-wrap helper (§3) is proven.

---

## 3. Byte-identity strategy — VERIFIED line by line

The PRNG core runs in raw Rust inside the native body (NOT through the checked value kernels, so I
control overflow semantics directly), and transpiles to PHP operators that match those semantics
exactly. Four operations, each verified Rust == PHP 8.5.7:

### 3.1 XOR `^` — trivially identical (bit-for-bit, no overflow).

### 3.2 Left shift `<<` — Rust `wrapping_shl` == PHP `<<` (both truncate to 64-bit signed)
```
PHP:  0x7FFFFFFFFFFFFFFF << 3  =>  int(-8)
Rust: 0x7FFFFFFFFFFFFFFF_i64.wrapping_shl(3)  =>  -8     ✓ identical
```
PHP `<<` wraps into signed 64-bit on overflow; Rust `i64::wrapping_shl` matches. **Rust MUST use
`wrapping_shl`** — a plain `<<` panics in debug on overflow (the recursion-guard build runs under
`forbid(unsafe)` + deny-warnings, debug-assertions on in tests). Transpiles to bare PHP `<<`.

### 3.3 Right shift `>>` — both ARITHMETIC (sign-extending), identical
```
PHP:  -1 >> 1  =>  int(-1)            (arithmetic)
Rust: -1_i64 >> 1  =>  -1             (arithmetic)   ✓ identical
```
xorshift needs a **logical** right shift on some lines; emulate it identically in both legs by masking
after the arithmetic shift: `(s >> 7) & 0x01FF_FFFF_FFFF_FFFF`. Verified the full xorshift64 step
produces identical sequences:
```
seed 88172645463325252, 3 steps:
PHP : 8748534153485358512 / 3040900993826735515 / 3453997556048239312
Rust: 8748534153485358512 / 3040900993826735515 / 3453997556048239312   ✓ byte-identical
```

### 3.4 The whole step, end to end — VERIFIED identical (see §3.3 numbers).

**Conclusion:** the sequence is byte-identical across interpreter / VM / real-PHP-8.5 by construction,
provided (a) no multiply, (b) Rust uses `wrapping_shl` for `<<`, (c) logical shifts are mask-emulated
identically on both legs, (d) `+` (if xoshiro is chosen) is wrapped via `& PHP_INT_MAX`-style masking
or `intval` discipline — verify before shipping the `+` variant; xorshift64 needs no `+` at all.

---

## 4. The API-shaping ops: `nextInt(lo,hi)`, `nextFloat()`, `shuffle` — all verified

### 4.1 `nextInt(lo, hi) -> int` — fully byte-identical (integers never diverge)
Map a raw i64 into `[lo, hi]`. The raw can be negative; clear the sign bit then modulo:
```
raw=-123456789012345, lo=10, hi=20:
  nonneg = raw & i64::MAX;  r = lo + (nonneg % range)
PHP : 10        Rust: 10      ✓ identical  ($raw & PHP_INT_MAX  ==  raw & i64::MAX)
```
`& PHP_INT_MAX` (=`0x7FFF...FF`) and `%` are byte-identical core ops. (Production: add rejection
sampling to remove modulo bias — `nonneg >= (i64::MAX/range)*range` reject and re-draw; the rejection
loop is itself deterministic and identical on both legs.)

### 4.2 `nextFloat() -> float in [0,1)` — value identical; output via the Ryū helper (NOT bare echo)
Standard `(raw_logical_shift_53) / 2^53`. The 53-bit mantissa and `2^53` are both exact, and the
division rounds identically under IEEE-754 round-to-nearest (both Rust f64 and PHP float):
```
raw=-123456789012345:  u53 = (raw>>11) & 0x1FFFFFFFFFFFFF;  f = u53 / 9007199254740992.0
PHP : float(0.9999933073940572)   Rust: 0.9999933073940572   ✓ identical VALUE
```
**The one trap** (the documented float KNOWN_ISSUE): PHP's *native `echo`* truncates to 14 digits
(`echo 0.9999933073940572` → `0.99999330739406`), while Rust `{}` is shortest-round-trip. This is
**already solved in Phorj**: every float that reaches output flows through the `__phorj_float` (Ryū)
helper, NOT bare PHP echo. So `nextFloat()`'s result prints identically *as long as it goes through the
normal Phorj float-output path* — which it does (interpolation / `Console.println` of a float both
route through `__phorj_str`→`__phorj_float`). No special handling needed; the existing helper covers it.
Guide example should still prefer printing `nextInt` results or comparing floats, to keep the showcase
robust.

### 4.3 `shuffle(List<T>) -> List<T>` — feasible, generic, no new mechanism
Deterministic Fisher–Yates driven by the same PRNG. `Core.List.reverse` already proves a
`List<T> -> List<T>` generic Pure native (uses `Ty::Param("T")`, erases pre-backend). Shuffle is the
same shape plus index draws. Transpiles to a PHP helper doing the identical Fisher–Yates loop (NOT
PHP's `shuffle()`, which uses an internal Mersenne Twister and is non-portable / mt-seed dependent —
must hand-emit the loop). Index draws use the §4.1 path. Byte-identical by construction.

---

## 5. State threading — the real design decision (no tuples in Phorj)

A PRNG is stateful, but determinism requires the state to be **explicit and value-typed** (Phorj has
no mutable-by-default and no tuple/multi-return — verified: no `Ty::Tuple`). Two shapes:

**Option A — functional state-threading (state IS an i64), recommended for the core:**
```
fn roll(int seed) -> int {
    int s = Random.seed(seed);          // s : Rng  (value type, wraps an i64)
    Pair<Rng,int> r = Random.nextInt(s, 1, 6);   // ...but no Pair/tuple type!
}
```
Blocked by no-tuples. So the clean functional form needs an **injected value type** that carries both
the new state and the drawn value — i.e. the API is naturally object-shaped.

**Option B — injected `Rng` value type (RECOMMENDED), mirrors the Json/RoundingMode injected-type pattern:**
`cli::inject_random_prelude` injects (gated on `import Core.Random;`) a small Phorj type whose state is
an `int`. Because Phorj instances are now **shared-mutable** (M-mut: `Value::Instance` is shared,
List/Map/Set are COW), a method `rng.nextInt(lo,hi)` can mutate its own `int` state field and return the
draw — and this is STILL deterministic (mutation of an in-program object is pure w.r.t. the seed). This
is the most ergonomic, most PHP/`Random\Randomizer`-familiar surface and needs **zero new runtime
machinery**: the injected type's methods are ordinary Phorj methods that call the pure stateless core
natives `Random.__step(state) -> int` / bounded helpers. The injected prelude flows through `check()` as
an ordinary class; backends see a normal instance; the transpiler emits a normal PHP class. The PRNG
*arithmetic* lives in the native(s); the *state object* lives in injected Phorj code.

> **Recommended:** Option B (injected `Rng` class) for the public surface + a tiny stateless pure native
> core (`Random.__step`, maybe `Random.__bound`). Mirrors `Core.Json`'s injected-enum + native-core split
> exactly. The class fields and the `int` state are byte-identical across legs (it's just an int and a
> method call); the arithmetic identity is the native core proven in §3–§4.

### Phorj API sketch (Option B)
```phorj
import Core.Random;

Rng rng = Random.seeded(42);          // injected type; state = 42 (or a splitmix-free scramble)
int d  = rng.nextInt(1, 6);           // mutates rng.state, returns draw in [1,6]
float u = rng.nextFloat();            // [0,1)
List<int> deck = rng.shuffle([1,2,3,4,5]);   // generic, deterministic Fisher-Yates
```
Stateless-functional alternative also offerable for purists:
`Rng next = Random.step(rng); int v = Random.peekInt(next, 1, 6);` (no mutation; thread the value).

---

## 6. Exact PHP transpile targets

NO PHP RNG builtin is used (PHP `mt_rand`/`shuffle` use Mersenne Twister with a different seed regime —
**non-portable, would diverge**, and `mt_srand` state is global+process-scoped). Everything transpiles to
**Phorj-owned arithmetic + a gated runtime helper**, the established `__phorj_*` pattern:

- `Random.__step` → inline PHP `^`/`<<`/`>>`/`&` matching §3, OR a gated helper
  `__phorj_rng_step($s)` (recommend a helper, like `__phorj_div`/`__phorj_str`, so the masking
  discipline is single-sourced and grep-auditable). Uses only PHP core (`-n` safe — no extension).
- `nextInt` → `__phorj_rng_bound($s,$lo,$hi)` (sign-clear + modulo/rejection, §4.1).
- `nextFloat` → `(($raw_shift) / 9007199254740992.0)` then the existing `__phorj_float` on output.
- `shuffle` → `__phorj_rng_shuffle($arr,$state)` hand-emitted Fisher–Yates (NOT `shuffle()`).

All targets are PHP **core** functions/operators that survive `php -n` (no `hash`/`mbstring`/`intl`/`bcmath`
dependency). Verified: `& | ^ << >> %` and float `/` are all core.

---

## 7. New VM Op? — NO.

Every operation is a `Op::CallNative` into the registry (the proven generic path: multi-arg, typed,
value-returning, `List<T>` generic via `reverse`'s precedent). The injected `Rng` type is ordinary
Phorj classes/methods — no Op. **Strongly preferred "no new Op" outcome holds.** The three coupled
matches (`chunk.rs`/`vm/exec.rs`/`compiler`) are untouched.

---

## 8. std Rust APIs relied on (zero external crates)

- `i64::wrapping_shl`, `i64::wrapping_add` (only if xoshiro `+` variant), `>>` (arithmetic), `^`, `&`, `%`
  — all core `std`/intrinsics, no crate.
- `f64` IEEE-754 division — core.
- `Rc<Vec<Value>>` for `List` (existing).
- No `rand`, no `getrandom`, no entropy source — by design (seeded only). Zero-dep invariant intact.

---

## 9. Named determinism risks

1. **Multiply promotion (THE blocker, NEUTRALIZED by design)** — forbid `*` in the core; pick a
   shift/xor algorithm. If anyone later "optimizes" to PCG/splitmix, it WILL break on step 1. Pin this in
   a code comment + KNOWN_ISSUES.
2. **`<<` debug panic** — Rust plain `<<` panics on overflow under debug-assertions (test build). MUST use
   `wrapping_shl`. (EV-7 discipline / recursion-guard build.)
3. **Logical vs arithmetic `>>`** — xorshift assumes unsigned shifts; emulate logical `>>` with an
   identical post-mask on BOTH legs. A mismatch is a silent sequence divergence. Verified mask values
   match.
4. **`+` overflow (only if xoshiro/xorshift+ chosen)** — PHP `+` of two large ints also promotes to
   float. The `+` in xoshiro256++'s output function needs masking emulation; verify before shipping the
   `+` variant. **xorshift64 avoids this entirely** (recommend shipping it first).
5. **nextFloat echo truncation** — bare PHP echo gives 14 digits; Phorj's `__phorj_float` (Ryū) path
   fixes it. Risk only if a future code path prints a float bypassing the helper. Keep the guide example
   on integer draws / float comparisons, not raw float echo.
6. **PHP `shuffle()`/`mt_rand()` temptation** — never transpile to them (Mersenne Twister, process-global
   seed, non-portable). Always hand-emit the loop. Grep transpiled examples for `mt_rand`/`shuffle(`
   /`random_int` as an oracle-hygiene check.
7. **Modulo bias** — `nextInt` via bare `%` is slightly biased; add deterministic rejection sampling.
   Bias is not a *divergence* risk (both legs biased identically) but a quality risk; the rejection loop
   stays byte-identical.
8. **Seed scrambling** — feeding a raw small seed (e.g. 0 or 1) to xorshift gives a poor first few
   outputs (xorshift64 with all-zero state is a fixed point → must reject seed 0 / scramble it). Scramble
   the seed deterministically (a fixed constant XOR + a few warm-up steps) identically on both legs;
   forbid the all-zero state.

---

## 10. Effort & recommendation

- **Effort: MEDIUM.** One `src/native/random.rs` leaf (core stateless natives) + an injected
  `Rng` prelude in `cli::inject_random_prelude` (mirrors `inject_json_prelude` / `inject_rounding_mode_prelude`)
  + gated `__phorj_rng_*` helpers in `transpile/program.rs` + one guide example
  (`examples/guide/random.phg`, byte-identity-gated) + registry tests. No backend/Op changes, no Value
  changes. The injected-type wiring is the only non-trivial part, and it has two existing precedents.
- **Recommendation: ADOPT-NOW.** It is a canonical Tier-A module, fully std-only, no new Op, and the
  hardest unknown (Rust↔PHP sequence identity) is **verified byte-identical** for the shift/xor family.
  The only "no" inside it (multiply-based PRNGs) is a clean algorithmic constraint, not a feasibility wall.
- **Feasibility: ~88%** (high confidence). The 12% residual is: the injected-type ergonomics + the
  `+`-wrap discipline if xoshiro is chosen (xorshift64-first removes most of it), plus the usual guide
  example float-output care. Confidence **high** on the core sequence identity (directly measured),
  **medium** on the final API ergonomics decision (functional-vs-object — both work, Option B recommended).
