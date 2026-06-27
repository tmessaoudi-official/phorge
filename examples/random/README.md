# Pseudo-random numbers (`Core.Random`)

A **seeded** pseudo-random generator. The same seed always replays the same stream, so a program is
reproducible — handy for tests, simulations, and shuffles you want to repeat.

```phorge
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

## Why this is a walkthrough, not a gated example

Every other example here is **byte-identity-gated**: the interpreter, the bytecode VM, and the
transpiled PHP must print exactly the same bytes (`tests/differential.rs`). That can't hold for
`Core.Random` — the transpiled code uses PHP's own Mersenne-Twister (`mt_srand`/`mt_rand`), whose
sequence intentionally differs from the Rust kernel.

So programs that import `Core.Random` are **quarantined** (detected via the `pure: bool` marker on each
native): the differential skips them, and they are tested instead in
[`tests/random.rs`](../../tests/random.rs). The `run ≡ runvm` half still holds — both Rust backends
share one process-global generator — so reproducibility and bounds are checked there; only the PHP
oracle is opted out.

## Notes

- **Deterministic by design.** There is no entropy source — seeding is explicit, and an unseeded
  program still starts from a fixed state. (An entropy-seeded constructor would be an impure add-on.)
- **The kernel.** A `xorshift64` generator (XOR + shifts only, no overflow, no float), with every
  value masked to a non-negative `int` (`< 2^63`).
- **Bounds.** `intBetween(lo, hi)` is inclusive on both ends; `hi < lo` is a fault.
- **Transpiled PHP.** `seed` → `mt_srand`; `next` → `mt_rand()`; `intBetween` → `mt_rand(lo, hi)`.
