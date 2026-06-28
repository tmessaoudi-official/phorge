# Stage 2b — Adversarial Byte-Identity Review: `Core.Random` (SEEDED)

**Verdict: the core feasibility claim SURVIVES, but the 88% is OPTIMISTIC and three under-specified
traps must be promoted to hard, pinned constraints before the spec is trusted. `determinism_holds = true`
for the *recommended xorshift64 core, integer/float draws*, conditioned on the constraints below.
It is `false` the moment the algorithm or the rejection-sampling code is written the way the spike's
own prose describes them.**

All probes run against the real oracle `/stack/tools/phpbrew/php/php-8.5.7/bin/php -n` (PHP 8.5.7 ZTS DEBUG)
and a matched `rustc -O` program.

---

## What I CONFIRMED byte-identical (the spike is right here)

| Op | PHP 8.5.7 `-n` | Rust `-O` | Match |
|----|----------------|-----------|-------|
| `*` of two large ints → **float** (the blocker) | `float(9.18…E+36)` | `wrapping_mul` exact | spike correct: forbid `*` |
| `<<` (fixed amts 13/17) | `0x7FFF… << 3 = -8` | `wrapping_shl(3) = -8` | identical |
| `>>` arithmetic | `-1 >> 1 = -1` | `-1 >> 1 = -1` | identical |
| full xorshift64 step ×3 (seed 88172645463325252) | `8748534153485358512 / 3040900993826735515 / 3453997556048239312` | same | **byte-identical** |
| `& PHP_INT_MAX` == `& i64::MAX` | `9223248580065763463` | same | identical |
| `nextInt` sign-clear + `%` | `10` | `10` | identical |
| `nextFloat` value (raw −123456789012345) | `u53=9007138973105732`, `f=0.9999933073940572` | same | **value identical** |
| 53-bit / `2^53` IEEE division rounding | `%.17g = 0.99999330739405723` | same | identical |

The xorshift64 sequence-identity claim — the single hardest unknown — is **directly reproduced and holds.**
The float-output trap is genuinely already solved: `src/transpile/expr.rs:471-485` + `program.rs:298-307`
route every statically-float value through `__phorj_float` (Ryū shortest-round-trip, tier-1 only), so the
bare-`echo` 14-digit truncation (`0.99999330739406`) never reaches output. That mechanism exists and is tested
(`transpile/tests.rs:255`). Not a new risk.

---

## REFUTATIONS — concrete divergences the spike under-specifies

### R1 (HIGH) — PHP `/` is ALWAYS float; the rejection-sampling bound `i64::MAX / range` breaks
The spike's own §4.1 / §9.7 *recommends* rejection sampling and writes the bound in Rust prose as
`(i64::MAX/range)*range`. Transpiled literally to PHP that is **doubly broken**:
- `PHP_INT_MAX / $range` returns `float(1.5372286728091292E+18)` — verified — NOT the integer
  `1537228672809129301` that Rust `/` gives. Must emit `intdiv()`.
- The `* range` that follows is the EXACT multiply the spike forbade in §2. For `range ≥ 2` it happens to
  stay ≤ `PHP_INT_MAX` (verified: `intdiv(MAX,3)*3 = 9223372036854775806`, still int) so it does not promote
  — but this is luck, not design, and a `range` of 1 or a different bound formula would tip it to float.

**The rejection loop the spike calls "deterministic and identical on both legs" is only identical if it is
written with `intdiv` and a multiply-free bound. As described in the spike it is a silent sequence divergence.**
This is the same class of trap as the headline multiply blocker, sitting unguarded inside the feature the spike
marked "verified."

### R2 (HIGH) — any 64-bit constant ≥ 2^63 becomes a PHP `float` (decimal OR hex)
Verified: `9223372036854775808` → `float(9.22…E+18)`; `0x8000000000000000` → `float(9.22…E+18)`. PHP has no
u64 and no unsigned literal. xorshift64's masks (`0x01FFFFFFFFFFFFFF`, `0x1FFFFFFFFFFFFF`) are all `< 2^63`
so they stay int (verified) — **the recommended-first algorithm is safe.** But the moment anyone takes the
spike's own "quality upgrade" path (xoshiro256++ `rotl`+constants, or a splitmix seed-scramble using
`0x9E3779B97F4A7C15` = float in PHP), a top-half-of-u64 constant in the transpiled PHP silently becomes a
float and the sequence diverges. The spike names xoshiro/splitmix as upgrade options without flagging that
their constants are un-representable as PHP ints. Pin: **every PRNG constant must be `< 2^63`, asserted at
transpile time.**

### R3 (MEDIUM) — shift-count divergence is latent, not absent
`1 << 64`: PHP → `int(0)`; Rust `wrapping_shl(64)` → `1` (count masked mod 64). `1 << 70`: PHP `0`, Rust `64`.
And `1 << -1`: PHP throws `ArithmeticError`, Rust panics/wraps. xorshift64 uses only fixed amounts 13/7/17,
all `< 64`, so this is **dormant for the recommended core** — but it is a live divergence for any
data-dependent or computed shift amount (a future rotate emulation `(x << k) | (x >> (64-k))` with `k=0`
gives `>> 64` → same trap). The spike says "Rust MUST use `wrapping_shl`" purely to avoid the debug panic; it
does NOT note that `wrapping_shl`'s count-masking is itself a Rust-vs-PHP divergence for counts ≥ 64. Pin:
**all shift amounts must be compile-time constants in `1..=63`.**

### R4 (LOW/MEDIUM) — Option B "shared-mutable injected `Rng`" adds an evaluation-order surface
The spike *recommends* Option B (a shared-mutable injected class whose method mutates `self.state`). For a
PRNG, draw order is the entire output, so any place the three backends evaluate sub-expressions in a different
order — e.g. two draws in one interpolation `"{r.nextInt(1,6)} {r.nextInt(1,6)}"`, or a draw inside a
function-argument list — would reorder the mutation and diverge. This is exactly the [[null-op-scratch-slot]]
class of bug (two stateful ops in one expression broke run↔runvm historically). The pure stateless-functional
core (`Random.step(state) -> (state', value)` threaded explicitly) has no such surface. The injected-class
ergonomics are fine ONLY if the guide example and the differential harness include the "two draws in one
expression" case. The spike's own guide-example advice stops at float-output care and misses this.

### R5 (LOW) — seed-0 fixed point + scramble must be done in multiply-free PHP-int-safe ops
`0 ^ (0<<13) = 0` verified (all-zero state is a fixed point). The spike says "scramble the seed
deterministically (fixed-constant XOR + warm-up steps)." Fine — **provided** the scramble constant is `< 2^63`
(R2) and uses no multiply (R1). A splitmix-style scramble (the obvious choice) violates both. Pin the scramble
to xor/shift/add-with-mask only.

---

## Net assessment

- **Tier A is correct.** Pure function of (seed, call sequence); `pure: true`; no clock/entropy; belongs in the
  byte-identity spine; no new `Op` (all `Op::CallNative`). All confirmed.
- **The xorshift64 core IS byte-identical** — directly measured, high confidence.
- **88% is too high for the spike as written**, because two of its own recommendations (rejection sampling via
  `(MAX/range)*range`, and the xoshiro/splitmix "upgrade") contain unflagged PHP-`/`-float and `≥2^63`-float
  traps — the very failure class the spike exists to prevent. Revised feasibility for the *constrained*
  module (xorshift64 only, `intdiv`+multiply-free bound, all constants `<2^63`, all shifts `1..=63`,
  stateless-functional or order-tested injected state) is **~82%**: the determinism holds, but it holds on a
  knife-edge that the spec must nail down rather than the spike's looser prose.

## Required hard constraints (promote from prose to pinned/asserted)
1. Algorithm = xorshift64 ONLY for v1. No multiply anywhere (core, bound, scramble). [R1, blocker]
2. Every PRNG constant `< 2^63` — assert at transpile emit time; reject xoshiro/splitmix constants. [R2]
3. All shift amounts are compile-time constants in `1..=63`. [R3]
4. Rejection bound emitted as `intdiv(PHP_INT_MAX, $range)` with a multiply-free reject test. [R1]
5. Logical-`>>` mask single-sourced in `__phorj_rng_step` helper (grep-auditable). [spike §3.3, agreed]
6. Differential harness must include: two draws in one expression; a draw in an argument list;
   seed 0 and a negative seed. [R4]
7. Never transpile to `mt_rand`/`shuffle()`/`random_int`; grep transpiled examples as oracle hygiene. [spike §6, agreed]
