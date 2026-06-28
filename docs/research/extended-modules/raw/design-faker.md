# Design — Seeded Faker (`Core.Faker`), Tier A

**Stage 2 design.** A seeded fake-data generator (name / email / address / lorem / number / date)
over **embedded corpora** + a **seeded, integer-only PRNG**, with a *byte-identical-by-construction*
three-leg guarantee. The whole feature is **Tier A** (gated in `tests/differential.rs`): same seed →
same stream → same data on `run`, `runvm`, and real `php -n` 8.5.

---

## 0. Verdict

| Axis | Decision |
|------|----------|
| **Tier** | **A (gated)** — admitted to the byte-identity differential. |
| **New `Op`** | **None.** Every native dispatches through the existing `Op::CallNative`. |
| **New `Value`** | **None** (recommended path); the RNG is a **pure-Phorj value object** built from existing `Value::Instance`, or a bare `int` seed in the functional variant. |
| **PHP target** | A small **hand-rolled, identical PRNG** emitted as a PHP function (`__phorj_faker_*`) over **integer-only** arithmetic (`intdiv`, `&`, `%`, `*` with the *same* wrapping rule), plus the corpora as a PHP array literal. **No `mt_rand`/`mt_srand`** (not bit-identical to any Rust PRNG). |
| **Feasibility** | **~82%** — the corpora + name/email/lorem/number are ~95%; the only genuine risk is the i64-wrapping-multiply ↔ PHP-int parity proof and the date formatting (see §6/§9). |
| **Confidence** | **medium** (the PRNG-parity argument is sound and grounded in the existing M-NUM i128/intdiv precedent, but it has not been built; the wrapping-multiply equivalence needs one fixture test to *prove* before the design is locked). |

The Faker is the **second consumer** of the seeded-PRNG kernel (`Core.Random`), which this design
specifies as its foundation. Ship `Core.Random` (the kernel) and `Core.Faker` (the corpora + field
generators) together, or `Core.Random` immediately before.

---

## 1. The byte-identity problem, precisely

A faker is "random". Randomness is the canonical determinism breaker (it is risk #1 in the project's
own framing). The escape is the locked decision: **seeded → deterministic → Tier A.** But "seeded"
is *not sufficient* on its own — it only holds if the **exact same number sequence is produced on all
three legs**. PHP's `mt_rand`/`mt_srand` (Mersenne Twister) is a specific algorithm that **no Rust
PRNG reproduces bit-for-bit**, and even if it did, `mt_rand`'s output is platform-/version-stable but
not something the Rust legs can replicate without re-implementing MT19937. So:

> **The PRNG algorithm itself must be hand-rolled identically in the Rust kernel and the emitted PHP.**
> Both legs run *our* PRNG, never the host language's. This is the single load-bearing rule of the
> whole feature, and it is exactly the rule already stated for `Core.Random` in
> `design-tierb-mechanism.md` §3.6 and §J ("PHP native RNG ≠ Rust RNG").

Given an identical PRNG, every downstream generator (pick a name by `rng.nextInt(0, len) `, build an
email from two picks, emit N lorem words) is a **pure deterministic function of the seed**, so the
three legs agree by construction — the same discipline that makes `Core.List.map` byte-identical.

### 1.1 The integer-model constraint (the real engineering)

PHP integers are **signed 64-bit** and **silently promote to float on overflow** (unlike Rust's
wrapping/panic). PHP has **no unsigned 64-bit type** and **no `u64` wrapping multiply**. Therefore the
PRNG must be chosen so that **every intermediate value stays within a range where signed-64 arithmetic
is identical on both sides and never triggers PHP's float promotion**. The stated `Core.Random`
constraints capture this: **constants < 2^63, intdiv-based, no PHP-float `/`.**

This **rejects SplitMix64 / xoshiro / PCG-XSH** as-is: their multipliers
(`0x9E3779B97F4A7C15`, `0x2545F4914F6CDD1D`, …) exceed 2^63, and `a * b` over full 64-bit words
overflows PHP int → float, diverging from Rust's `wrapping_mul`. We need a generator whose state and
multiply fit a regime where Rust `i64` wrapping == PHP int arithmetic.

**Chosen kernel: a 63-bit LCG with a *masked* multiply** (a "Lehmer / MMIX-style" LCG constrained to
fit PHP int):

```
state : i64, always in [0, 2^62)          // top 2 bits always clear ⇒ never negative, never near overflow
MASK  = (1 << 62) - 1                       // 0x3FFF_FFFF_FFFF_FFFF, < 2^63 ✓
MUL   = 6364136223846793005 & MASK          // a *masked* well-known LCG multiplier, < 2^62 ✓ (see §6 note)
INC   = 1442695040888963407 & MASK          // (likewise masked) odd increment
next():
    // a*b can be up to (2^62-1)^2 ≈ 2^124 — too big for ONE i64 multiply.
    // So we do the multiply in two 31-bit halves with intdiv/% — pure integer, no float, < 2^63 each step.
    state = mul_mod(state, MUL, 2^62) + INC, then & MASK
    // emit the high bits (better-quality output than low bits of an LCG)
    return (state >> 30) & 0xFFFFFFFF        // 32-bit output word
```

The **load-bearing primitive** is a `mul_mod(a, b, m)` that multiplies two sub-2^62 numbers modulo
`m` **without ever exceeding 2^63**, using the schoolbook split that the M-NUM decimal i128 work and
PHP's BCMath-free integer math already established as a transpile pattern:

```
// Russian-peasant / shift-add mul_mod — every intermediate < 2*m < 2^63, no overflow, no float.
mul_mod(a, b, m):
    result = 0
    a = a % m
    while b > 0:
        if (b & 1) == 1: result = (result + a) % m
        a = (a * 2) % m          // a < m < 2^62 ⇒ a*2 < 2^63 ✓
        b = b >> 1
    return result
```

This is **provably identical** in Rust i64 and PHP int because **no intermediate ever exceeds 2^63**:
`a < m < 2^62`, so `a*2 < 2^63`; `result < m` and `a < m`, so `result + a < 2^63`. PHP never promotes
to float (promotion only happens *above* `PHP_INT_MAX = 2^63 - 1`), and Rust never wraps. `%` and `*2`
and `+` are bit-identical across the two languages within this regime. **This is the entire
byte-identity proof.** (Trade-off: `mul_mod` is O(62) per `next()` — ~62 loop iterations. For a faker
this is utterly negligible; for a hot Monte-Carlo `Core.Random` it is slow, so §9 flags a faster
"two-32-bit-limb" variant as a follow-up — but the shift-add version is the one whose parity is *trivial
to prove*, so ship it first.)

> **Why not just demand `php -n` has GMP?** GMP is **absent under `php -n`** (the oracle's environment;
> see the project's `transpile-no-ini-extensions` memory). The whole point of the int-model constraint
> is to stay inside PHP **core** integer arithmetic. BCMath *is* present under `php -n` and is the M-NUM
> decimal target — but BCMath is *string* decimal math, far heavier than needed and not bit-compatible
> with an i64 PRNG. Core integer ops are the right tool.

---

## 2. Public API (Phorj syntax)

Two layers. **Layer 1** is the seeded PRNG (`Core.Random`) — the kernel. **Layer 2** is `Core.Faker`,
the corpora + field generators built *on* the kernel. The Faker never re-implements randomness; it
only consumes `rng.nextInt(...)`.

### 2.1 The RNG value — recommended: a pure-Phorj object (no new `Value`)

The cleanest ergonomic surface is a **stateful object** with methods. To avoid a new `Value` variant,
the RNG is a **pure-Phorj class** whose single field is the `int` state, *injected* into the program
exactly like `Core.Json`'s injected `Json` enum (the "injected-type pattern" memory,
`cli::inject_json_prelude`). The class body is written in Phorj and prepended before checking when the
program imports `Core.Random`; its methods call **two tiny natives** (`__random_next`,
`__random_seed_norm`) that do the integer kernel. This means:

* **No new `Value`** — `Rng` is an ordinary `Value::Instance` with a mutable `int` field (mutation
  already exists, M-mut milestone: `Instance` is shared-mutable).
* **No new `Op`** — methods compile to ordinary method calls + `Op::CallNative`.
* The PHP leg gets the same injected class, transpiled to a normal PHP class holding `$state`.

```phorj
package Main;
import Core.Random;
import Core.Console;

function main() -> void {
    mutable Rng rng = Random.seeded(42);     // deterministic stream from seed 42

    int a = rng.nextInt(1, 6);               // dice roll in [1, 6]
    float f = rng.nextFloat();               // in [0.0, 1.0)  — see §6 float note
    bool b = rng.nextBool();
    string s = rng.pick(["red", "green", "blue"]);   // uniform choice from a List<string>

    Console.println("roll={a} f={f} pick={s}");
}
```

`Random.seeded(int) -> Rng` is the one constructor. `Rng` carries `mutable int state`. Each method
*advances* the state (so the object is consumed in call order — the determinism is "same seed + same
call sequence → same outputs", the JS-event-loop-style *total order is a language rule*).

> **Functional alternative (no object, no injection — if injection is judged too heavy):** expose the
> kernel as pure `(seed) -> (value, nextSeed)` natives and make the user thread the seed. This is
> uglier (`let (a, s1) = Random.intFrom(s0, 1, 6);`) but needs **zero injected class** and is the most
> trivially-provable Tier-A form. **Recommendation: ship the object form** (matches Faker/PHP-dev
> expectations) and keep the functional natives as the *underlying* registry entries the injected class
> calls. Open question Q1.

### 2.2 `Core.Faker` — field generators

`Core.Faker` is itself a **pure-Phorj module class** (`Faker`) injected the same way, **constructed
from an `Rng`** so the faker stream is part of the same deterministic sequence:

```phorj
import Core.Random;
import Core.Faker;

mutable Rng rng = Random.seeded(1234);
mutable Faker f = Faker.from(rng);          // shares the rng's stream

string nm   = f.name();                     // "Olivia Martin"
string fn   = f.firstName();                // "Olivia"
string ln   = f.lastName();                 // "Martin"
string mail = f.email();                    // "olivia.martin@example.com"  (derived from a name pick)
string city = f.city();                     // "Springfield"
string addr = f.streetAddress();            // "47 Oak Street"
string para = f.lorem(12);                  // 12 lorem words, space-joined
int    n    = f.numberBetween(100, 999);
int    age  = f.numberBetween(18, 80);
string day  = f.date(2000, 2030);           // "2014-07-22"  — see §6 date note (string, ISO-8601)
bool   yes  = f.boolean();
string item = f.pick(["a", "b", "c"]);      // delegate to rng.pick

// Reproducibility: a fresh faker from the same seed yields the SAME sequence.
mutable Faker g = Faker.from(Random.seeded(1234));
// g.name() == "Olivia Martin"   (proven by the differential)
```

**Field surface for v1** (all derived from corpora + the kernel, all deterministic):

| Method | Returns | Source |
|--------|---------|--------|
| `name()` / `firstName()` / `lastName()` | `string` | first/last name corpora |
| `email()` | `string` | `{first}.{last}@{domain}` (lowercased, ASCII), domain corpus |
| `username()` | `string` | `{first}{lastInitial}{2-digit-number}` |
| `city()` / `country()` / `streetName()` | `string` | place corpora |
| `streetAddress()` | `string` | `{1..999} {streetName} {Street\|Ave\|Road\|...}` |
| `lorem(int words)` | `string` | lorem corpus, picks `words` words, space-joined |
| `sentence(int words)` | `string` | lorem, capitalized first word + period |
| `numberBetween(int, int)` | `int` | `rng.nextInt` |
| `boolean()` | `bool` | `rng.nextBool` |
| `date(int yearLo, int yearHi)` | `string` | ISO date from integer day-arithmetic (§6) |
| `pick<T>(List<T>)` | `T` | `rng.pick` |

`pick` is **generic** (rides the already-shipped erased-generics machinery — a `HigherOrder`-free
generic native exactly like `firstOr<T>`), so `f.pick([...])` works for any element type.

---

## 3. Corpora as embedded data

The corpora (first names, last names, cities, streets, lorem words, email domains, …) are **embedded
constant data**, single-sourced in the Rust kernel and **emitted verbatim into the PHP** so both legs
index the *same* array with the *same* index from the *same* PRNG draw. This is structurally identical
to how `ClassTables` is emitted as a PHP static table for `Core.Reflect` — one source of truth,
mechanically reproduced on the transpile leg, so the two cannot drift.

```rust
// src/native/faker_data.rs  — pure const data, no logic.
pub const FIRST_NAMES: &[&str] = &["Olivia", "Liam", "Emma", "Noah", /* ~200 entries */];
pub const LAST_NAMES:  &[&str] = &["Smith", "Martin", "Johnson", /* ~200 */];
pub const CITIES:      &[&str] = &["Springfield", "Riverside", /* ~100 */];
pub const STREETS:     &[&str] = &["Oak", "Maple", "Main", /* ~80 */];
pub const STREET_SUFX: &[&str] = &["Street", "Avenue", "Road", "Lane", "Boulevard"];
pub const EMAIL_DOMAINS: &[&str] = &["example.com", "example.org", "test.com"];
pub const LOREM: &[&str] = &["lorem", "ipsum", "dolor", "sit", "amet", /* ~180 */];
```

* **ASCII-only corpora** (no accents). Rationale: avoids any `mb_*` need (mbstring is **absent under
  `php -n`** — the project's `transpile-no-ini-extensions` invariant). `email()`'s lowercasing uses the
  ASCII subset only, so `strtolower` (core) is byte-identical to Rust `to_ascii_lowercase`.
* The corpora ship as **embedded Rust slices** and are emitted to PHP as `['Olivia','Liam',...]`
  literals inside the injected Faker class (or as `__phorj_faker_first_names()` returning the array).
  Both legs use **0-based indexing** with the PRNG draw `idx = rng.nextInt(0, len)` (half-open
  `[0,len)`), so `FIRST_NAMES[idx]` ≡ `$first_names[$idx]`.
* Size budget: ~1–2 KB of strings; embedded directly in the binary and the transpiled PHP. Acceptable.

> **Single-sourcing the corpora across Rust and PHP:** the Rust slice is the master; the transpiler
> emits the PHP array by iterating the same slice (`FIRST_NAMES.iter().map(php_quote).join(",")`).
> A unit test asserts the emitted PHP array length == the Rust slice length, so a corpus edit can't
> desync the two legs. This is the same "emit the table you already hold" guarantee as `ClassTables`.

---

## 4. How the three legs stay byte-identical (the proof, assembled)

1. **PRNG**: both legs run the *same* hand-rolled LCG + `mul_mod` (§1.1), all arithmetic < 2^63 ⇒ no
   PHP float promotion, no Rust wrap ⇒ identical 32-bit output words for identical seed + draw count.
2. **Bounded draw**: `nextInt(lo, hi)` = `lo + (word % (hi - lo))` (half-open; `nextInt(lo, hiInclusive)`
   variant adds 1) — pure integer modulo, identical both sides. (Modulo bias is *present but identical*
   on both legs, so it doesn't break byte-identity; §6 notes a rejection-sampling upgrade that **stays
   deterministic** if unbiased draws are wanted later.)
3. **Corpus pick**: `corpus[draw]` over the *same* embedded array with the *same* index ⇒ same string.
4. **Composition**: `email = lower(first) + "." + lower(last) + "@" + domain` — ASCII lower (core
   `strtolower` ≡ Rust `to_ascii_lowercase`) + string concat (already byte-identical across legs) ⇒
   same email.
5. **Call order is a total language rule** (each method advances the shared state in evaluation order),
   so "same seed + same program text" pins the entire stream — exactly the JS-event-loop determinism
   principle the concurrency designs adopt.

Therefore `Core.Faker` is admitted to `tests/differential.rs` like every other gated example, and
`examples/guide/faker.phg` runs byte-identically on `run` ≡ `runvm` ≡ real PHP 8.5.

---

## 5. Exact PHP transpile target

The injected `Rng`/`Faker` classes transpile to ordinary PHP classes (the injected-type pattern already
does this for `Json`). The **two kernel natives** map to emitted PHP functions:

```php
// emitted once when the program uses Core.Random / Core.Faker (gated helper, like __phorj_div):
function __phorj_rng_mulmod(int $a, int $b, int $m): int {   // identical to the Rust mul_mod
    $r = 0; $a %= $m;
    while ($b > 0) {
        if (($b & 1) === 1) { $r = ($r + $a) % $m; }
        $a = ($a * 2) % $m;
        $b = $b >> 1;
    }
    return $r;
}
function __phorj_rng_next(int $state): array {               // returns [newState, word]
    $MASK = (1 << 62) - 1;
    $MUL  = 6364136223846793005 & $MASK;
    $INC  = 1442695040888963407 & $MASK;
    $state = (__phorj_rng_mulmod($state, $MUL, 1 << 62) + $INC) & $MASK;
    return [$state, ($state >> 30) & 0xFFFFFFFF];
}
function __phorj_rng_seed_norm(int $seed): int {             // fold an arbitrary seed into [0, 2^62)
    return $seed & ((1 << 62) - 1);  // plus one scrambling round in real impl (see §6)
}
```

```php
class Rng {                  // the injected Phorj class, transpiled
    public int $state;
    public function nextWord(): int { [$this->state, $w] = __phorj_rng_next($this->state); return $w; }
    public function nextInt(int $lo, int $hi): int { return $lo + ($this->nextWord() % ($hi - $lo)); }
    // ...
}
```

* `mt_srand`/`mt_rand` are **never emitted** (they would diverge). All randomness is the hand-rolled
  function above.
* Corpora emit as PHP array literals (§3). `f.lorem(n)` → an integer loop building the array of picks
  then `implode(' ', ...)` (core).
* Everything used is **PHP core** (`%`, `&`, `>>`, `*`, `implode`, `strtolower` on ASCII) — clean under
  `php -n`.

The single per-program emission of the helpers rides the existing `uses_* + __phorj_*` gated-helper
mechanism (the project's `php-leg-outside-correctness-loop` memory: prefer a runtime helper over static
types).

---

## 6. Determinism risks (named)

1. **i64-wrapping-multiply ↔ PHP-int parity** *(the one real risk)* — the *entire* feature rests on
   `mul_mod` staying < 2^63 on both legs. **Mitigation:** the shift-add `mul_mod` proven in §1.1 keeps
   every intermediate < 2^63 by construction; a dedicated fixture test (Rust `mul_mod` vs the emitted
   PHP `__phorj_rng_mulmod` over a vector of `(a,b,m)` near the 2^62 boundary) **proves** it before
   the design is locked. *Status:* designed-not-proven → see Q4.
2. **The masked multiplier `MUL = K & MASK`** is **not** a tested full-period LCG multiplier — masking
   a known good 64-bit multiplier into 62 bits does **not** guarantee a full-period generator. **This
   is a statistical-quality risk, not a byte-identity risk** (both legs are still identical). For a
   *faker* the quality bar is "looks varied", which a 62-bit LCG meets. **Mitigation:** pick a
   *verified* LCG multiplier that is **natively < 2^62** (e.g. one of L'Ecuyer's tables for modulus
   2^62) rather than masking a 64-bit constant — removes the guesswork. *Open question Q3.*
3. **`nextFloat()` and PHP float `/`** — the constraint forbids "no PHP-float `/`". A float in `[0,1)`
   from a 32-bit word would normally be `word / 2^32`, which is a **float division** and risks the
   classic 14-digit `echo` divergence the project already documented (irrational floats differ between
   Rust and PHP). **Mitigation:** `nextFloat()` is defined as `word / 4294967296.0` **only if** the
   division is exactly representable; safer is to **scope v1 to integer + string fields and DEFER
   `nextFloat`** (KNOWN_ISSUES), since money/decimals already go through `Core.Decimal`. The faker's
   `numberBetween` is pure integer and safe. *Recommendation: defer `nextFloat` to a later slice.*
4. **`date()` formatting** — must be pure integer day-arithmetic, **not** PHP `DateTime`/`date()` (whose
   output can differ by locale/timezone and is non-deterministic in spirit). **Mitigation:** compute a
   day-offset integer in `[0, days-in-range)` with `nextInt`, convert to `(y, m, d)` with the proleptic
   Gregorian integer algorithm (the same fixed `intdiv`-based civil-from-days math `Core.Time` will use),
   and format `"%04d-%02d-%02d"` by hand (string concat + zero-pad, both legs identical). **No `DateTime`,
   no timezone, no clock.** This is Tier-A precisely because it never reads the real clock.
5. **Modulo bias** — `word % range` is biased but **identically biased on both legs**, so it does not
   break byte-identity; only statistical uniformity. Acceptable for a faker; a deterministic rejection
   loop is the unbiased upgrade if needed.
6. **Seed normalization** — an arbitrary user seed (negative, huge) must fold into `[0, 2^62)`
   identically. `seed & MASK` is identical both sides; a single scrambling round (one `next()`) before
   first use avoids a poor first draw. Deterministic.
7. **Corpus desync** — a corpus edited in Rust but not re-emitted to PHP. **Mitigation:** the corpora
   are emitted *from* the Rust slice (§3), and a unit test asserts length parity, so desync is
   structurally prevented.

---

## 7. New Op / Value — none

* **No new `Op`.** `Random.seeded`, `rng.nextInt`, `f.name()` etc. are method calls on injected Phorj
  classes whose leaves call `Op::CallNative` for the kernel — the existing dispatch.
* **No new `Value`.** `Rng`/`Faker` are `Value::Instance` (shared-mutable, M-mut). The corpora live in
  Rust `const` slices, not in any `Value`.
* The injected classes follow the **`cli::inject_json_prelude` pattern** (memory
  `core-json-and-injected-types`): inject the `Rng`/`Faker` AST before `check()`, gated on the import,
  so they flow as ordinary user types with zero backend machinery.
* The two kernel natives (`__random_next`, `__random_seed_norm`, plus maybe a `mulmod` helper) are
  ordinary `NativeEval::Pure` entries in a new `src/native/random.rs` (the Faker's corpora-picking can
  be expressed entirely in injected Phorj calling `rng.nextInt`, so `Core.Faker` may need **no Rust
  native at all** beyond the corpora data — open question Q2).

---

## 8. Effort

**Medium.** Breakdown:

| Piece | Effort | Note |
|-------|--------|------|
| `Core.Random` kernel (LCG + `mul_mod` natives, injected `Rng` class, PHP helpers) | medium | the load-bearing parity work; ~1 native module + 1 injected class + 3 emitted PHP fns |
| Corpora (`faker_data.rs` + PHP emission + length-parity test) | small | pure data |
| `Core.Faker` injected class (field generators in Phorj over `rng`) | small-medium | mostly Phorj code calling `rng`; `date()` integer math is the fiddly bit |
| `mul_mod` parity fixture test (Rust vs emitted PHP) | small | **must precede lock** (Q4) |
| `examples/guide/faker.phg` + README (byte-identity-gated) | small | the standing "examples ship with features" rule |
| Differential admission (it's `pure:true` ⇒ auto-gated) | trivial | no quarantine needed |

Depends on `Core.Random` landing first (or in the same change). Net: a **medium** slice, one milestone
sub-slice — call it **M-Test** foundation work (the Faker is a testing-suite pillar).

---

## 9. Honest feasibility

**~82%.** The name/email/lorem/number/city/address generators over embedded corpora are **~95%** — they
are pure index picks and string concat, the most byte-identity-safe shape there is. The two things that
pull the number down:

* The **PRNG i64/PHP-int parity** is *sound on paper* (the < 2^63 invariant is airtight) but **unproven
  in code** — until the `mul_mod` fixture test is green, it's [Inferred], not [Verified]. Risk that some
  edge (negative seed, the `& MASK` of a 64-bit literal in PHP where `1 << 62` interacts with sign) needs
  a tweak. ~90% on its own.
* **`nextFloat` and `date`** carry the float-divergence and calendar-math tails; v1 **defers `nextFloat`**
  and uses **hand-rolled integer date math** — both de-risk to Tier A, but `date` is the fiddliest code.

The performance of the shift-add `mul_mod` (O(62) per draw) is a *non-issue for a faker* but would be a
real concern for a hot `Core.Random` Monte-Carlo loop — a **two-32-bit-limb `mul_mod`** (4 partial
products, still < 2^63) is the follow-up optimization, and it must be re-proven for parity before
replacing the simple version. Flagged, not blocking.

---

## 10. Open questions for the developer

* **Q1 — RNG surface: object or functional?** Recommended: **object** (`Random.seeded(n).nextInt(...)`)
  via the injected-class pattern (matches PHP/dev expectations, no new `Value`). The functional
  `(seed)->(val,seed)` form is more trivially-provable but uglier. Confirm object.
* **Q2 — Is `Core.Faker` pure-Phorj (corpora-as-PHP, generators-in-Phorj) or does it need Rust
  natives?** Recommended: corpora as a Rust `const` emitted to PHP + Faker generators written in
  **injected Phorj** calling `rng.nextInt` — minimizes native surface and maximizes shared logic.
  The only thing that *must* be a native is the PRNG `next` and `mul_mod`.
* **Q3 — PRNG multiplier choice.** Recommended: a **verified < 2^62 LCG multiplier** (L'Ecuyer table
  for modulus 2^62) rather than masking a 64-bit constant — removes the period-quality guesswork while
  keeping the < 2^63 parity guarantee. Confirm we may pin a specific documented constant.
* **Q4 — Prove-before-lock.** The `mul_mod` Rust-vs-emitted-PHP fixture test is a **gate on locking this
  design**, not a later task. OK to treat the design as provisional until that test is green?
* **Q5 — `nextFloat` and `date` scope for v1.** Recommended: **defer `nextFloat`** (float-divergence
  tail; decimals go through `Core.Decimal`), **ship integer `numberBetween` + hand-rolled integer
  `date`**. Confirm v1 field set (§2.2 table).
* **Q6 — Corpus size / licensing.** ~200 first/last names, ~100 cities, ~80 streets, ~180 lorem words,
  3 email domains, all **ASCII**. Confirm scale and that hand-curated public-domain corpora are fine
  (no third-party data files — keeps zero-dep).
* **Q7 — Relationship to `Core.Random` shipping order.** Ship `Core.Random` (kernel) first or together?
  Recommended: **together**, since the Faker is the kernel's first real consumer and the differential
  example exercises both.
