# Pseudo-random numbers (`Core.Random`)

A **seeded** pseudo-random generator. The same seed always replays the same stream, so a program is
reproducible — handy for tests, simulations, and shuffles you want to repeat.

```phorj
import Core.Random;

Random.seed(n: int) -> void                 // reset the generator to a deterministic state
Random.next() -> int                         // advance; return the raw non-negative value
Random.intBetween(lo: int, hi: int) -> int   // advance; return a value in [lo, hi] (inclusive)
```

Run [`dice.phg`](dice.phg):

```console
$ phg run examples/random/dice.phg
Five d6 rolls:
  roll 0: 1
  roll 1: 6
  roll 2: 4
  roll 3: 5
  roll 4: 3
raw next: 4299401598188713652
```

Because the seed is fixed (`2026`), those rolls are the same on every run and on both Rust backends.

## Byte-identical across all three backends

Like every other example, `dice.phg` is **byte-identity-gated** (2026-06-27): the interpreter, the
bytecode VM, **and the transpiled PHP** print exactly the same bytes (`tests/differential.rs`). The
transpiler hand-rolls the **same** `xorshift64` as the Rust kernel (`__phorj_rng_*` helpers) instead
of PHP's Mersenne-Twister, so a seeded sequence reproduces identically everywhere — reproducibility
survives transpile. (Earlier this module was quarantined because it emitted `mt_srand`/`mt_rand`, whose
sequence couldn't match; that divergence is gone.)

## Notes

- **Deterministic by design.** There is no entropy source — seeding is explicit, and an unseeded
  program still starts from a fixed state (`GOLDEN`). (An entropy-seeded constructor would be an impure
  add-on, and would re-introduce quarantine.)
- **The kernel.** A `xorshift64` generator (XOR + shifts only, no overflow, no float), with every
  value masked to a non-negative `int` (`< 2^63`). The PHP emission masks the `>> 7` (PHP `>>` is
  arithmetic, Rust's `u64 >>` is logical) and writes `GOLDEN` as its signed-i64 reinterpretation.
- **Bounds.** `intBetween(lo, hi)` is inclusive on both ends; `hi < lo` is a fault.
- **Transpiled PHP.** `seed` → `__phorj_rng_seed`; `next` → `__phorj_rng_next()`; `intBetween` →
  `__phorj_rng_int_between(lo, hi)` — all over a shared by-reference function-static state.
