# Track N — Numerics & business-data — gap audit

## Track summary

Phorj's numeric tower today is intentionally minimal and **provably correct**: `int` (checked
`i64` — overflow and div/mod-by-zero are clean faults, never wraparound or panic), `float` (`f64`,
rendered byte-identically across all three backends via the `__phorj_float` helper), `bytes`
(`b"…"`), and a thin `Core.Math` (`sqrt`/`pow`/`floor`/`ceil` on float, `abs`/`min`/`max` on int).
There is **no decimal, no arbitrary-precision integer, no rational, no sized integers, no rounding
control, and — most strikingly — no date/time type whatsoever** (verified: zero `time`/`date`/
`duration`/`timezone` references in `src/`, ROADMAP, VISION, or specs). This is precisely the domain
where PHP burns its users: `0.1 + 0.2`-style float money bugs, `bcmath`'s stringly-typed API, the
sprawling mutable `DateTime` family with timezone/DST footguns, and integer width surprises. Because
Phorj's whole pitch is "a typed, surprise-free upgrade of PHP," the single highest-leverage business
feature it can ship is a **typed `Decimal` money/fixed-point type** that makes float-for-currency a
*compile error*, followed by a **correct, immutable date/time library** modelled on the good parts of
`DateTimeImmutable` but with the timezone made non-optional. Both map cleanly: `Decimal` → `brick/
math` `BigDecimal` (or `bcmath` strings) on the PHP leg; date/time → `DateTimeImmutable`/`DateInterval`/
`DateTimeZone`. The catch for the byte-identity spine is real and shapes every verdict below: a
std-only Rust core has **no `f128`/bigdecimal crate and no IANA tz database**, and wall-clock/`now()`
is **non-deterministic** — so the shippable subset is the *deterministic, pure-value* core (decimal
arithmetic on literals, duration math, explicit-offset timestamps), with `now()`/IANA-zone lookups
quarantined exactly as URL/network was (M6) and the float-irrational caveat already is.

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| N-decimal | Typed `decimal` / money fixed-point primitive | port | strong | adopt | new milestone M-NUM | L |
| N-decimal-rounding | Explicit rounding modes for `decimal` (banker's, half-up, …) | port | strong | adopt | M-NUM | M |
| N-datetime-core | Immutable `Instant`/`DateTime` value type (timezone-mandatory) | port | strong | adopt | new milestone M-TIME | L |
| N-duration | Typed `Duration` (durations + date arithmetic) | port | strong | adopt | M-TIME | M |
| N-date-civil | Civil `Date` / `Time` (no-zone calendar values) | port | strong | adopt | M-TIME | M |
| N-tz-iana | IANA timezone DB + DST-correct conversions | port | ok | defer | M-TIME-2 / M6 | L |
| N-now-clock | `now()` / wall-clock reads (non-deterministic) | map | weak | defer | M-TIME-2 / M6 | S |
| N-bigint | Arbitrary-precision `BigInt` | port | ok | defer | M-NUM-2 | L |
| N-sized-int | Sized integers (`i32`/`u8`/`i128`…) | port | ok | defer | v2 | L |
| N-int-width | Document/enforce `int` = `i64` vs PHP platform-width | omit | strong | adopt | M-NUM | S |
| N-rational | Rational / fraction type (`BigRational`) | port | weak | reject | — | L |
| N-numeric-parse | `Core.Num` string↔number parsing (`parseInt`/`parseFloat`/`parseDecimal`) | port | strong | adopt | M-NUM | M |
| N-num-format | Locale-aware number/money formatting (`format`, thousands, currency) | port | ok | defer | M-NUM-2 | M |
| N-math-breadth | Wider `Core.Math` (`round`/`sign`/`clamp`/`gcd`/trig/`log`/constants) | port | strong | adopt | M-NUM | M |
| N-int-conv | Explicit numeric conversions (`toFloat`/`toInt` truncation, `int↔decimal`) | port | strong | adopt | M-NUM | S |
| N-percent | Percentage / basis-point helper on `decimal` | new | weak | reject | — | M |
| N-overflow-policy | Opt-in wrapping/saturating int ops (`wrappingAdd`, `saturatingAdd`) | new | weak | defer | v2 | M |
| N-uuid-id | UUID / monotonic-ID value type | new | weak | defer | M4 stdlib | M |

## Rationale for ADOPT items

**N-decimal — typed `decimal` money/fixed-point primitive (the headline gap).** This is the single
feature a PHP/GRDF business dev would adopt instantly. PHP has *no* native decimal: developers either
misuse `float` (the classic `0.1 + 0.2` money bug that silently corrupts invoices) or fall back to
`bcmath`'s stringly-typed, error-prone API, or pull in `brick/math`. Phorj can make
`float`-for-currency a **compile error** by giving `decimal` its own `Ty` so it never auto-coerces with
`float`. The shippable, byte-identity-safe core is a **fixed-precision decimal as `(i128 mantissa,
scale)` or a small bignum on `Vec<u32>` — std-only, deterministic**, with checked arithmetic in the
EV-7 spirit (overflow/precision-loss = clean fault). It transpiles to `brick/math` `BigDecimal`
(vendored, the M5 dependency model already supports git deps) or `bcmath` strings on the PHP leg.
Literal syntax `1.50d` or `decimal "1.50"` keeps it legible. This earns its surprise budget many times
over — it removes the *largest* class of real-world PHP money bugs while mapping to an idiomatic PHP
target. Belongs in a dedicated **M-NUM** milestone because the type, the literal, the kernel, and the
PHP-leg helper are a cohesive unit.

**N-decimal-rounding — explicit rounding modes.** Money math is meaningless without controlled
rounding; `brick/math` ships a `RoundingMode` enum precisely because the rounding policy *is* the
business rule (banker's rounding for finance, half-up for retail). A Phorj `RoundingMode` enum
(`HalfUp`/`HalfEven`/`Floor`/`Ceil`/…) consumed by `decimal.round(scale, mode)` is a direct,
legible map to PHP and forces the developer to make the choice explicit rather than inheriting a
silent default. Ships with N-decimal.

**N-datetime-core — immutable timezone-aware `DateTime`/`Instant`.** PHP's date handling is its
second-most-notorious footgun: a sprawling mutable `DateTime` (mutation-aliasing bugs), an *optional*
timezone that silently defaults to a global ini setting, and DST surprises. The community consensus
(confirmed: "use `DateTimeImmutable`, always specify the timezone, store UTC") is exactly what Phorj
should bake into the *type*. A `DateTime` value that is **immutable by construction** (Phorj already
is) and where **the timezone/offset is a mandatory constructor argument** (no global-default footgun)
is a clean, strictly-safer mapping onto `DateTimeImmutable`. The deterministic, byte-identity-safe core
is **explicit-offset timestamps + arithmetic** (epoch seconds + fixed offset → no IANA DB, no `now()`):
constructing from components, formatting, comparing, adding durations. This is a genuine PHP-upgrade and
maps 1:1. Belongs in a dedicated **M-TIME** milestone.

**N-duration — typed `Duration`.** Date arithmetic without a type produces the classic "is this
seconds or milliseconds?" bug. A `Duration` type (PHP `DateInterval`, ISO-8601 `P…`/`PT…`) with
`dt.plus(Duration.days(3))` is legible, deterministic, and pure-value — no clock, no zone DB needed.
Ships with N-datetime-core as the arithmetic half of the same milestone.

**N-date-civil — civil `Date` / `Time`.** A huge fraction of business data is zone-less calendar data:
an invoice date, a birth date, a contract day. Conflating these with zoned instants is a real bug
source (the "date shifts by a day across timezones" classic). Separate `Date` (y-m-d) and `Time`
(h-m-s) value types — deterministic, no zone — are legible and strictly safer than PHP's everything-is-
`DateTime` approach. Maps to `DateTimeImmutable` constrained to midnight UTC, or a documented civil
convention. Ships in M-TIME.

**N-int-width — pin and document `int` = `i64`.** Phorj's `int` is a checked `i64`; PHP's `int` is
platform-width (64-bit on modern installs, but conceptually unbounded-via-float-promotion on overflow).
Phorj already *diverges safely* (it faults instead of promoting — see the `List.sum` KNOWN_ISSUES
caveat). This gap is cheap (S): make the i64 contract explicit in `docs/INVARIANTS.md` and FEATURES,
so the decimal/bigint deferrals have a documented rationale and the PHP-leg overflow divergence is a
stated contract, not a surprise. Adopt now, in M-NUM, because it's the documentation backbone the whole
track leans on.

**N-numeric-parse — `Core.Num` string↔number parsing.** Business apps parse numbers from CSV/JSON/form
input constantly; PHP's `intval`/`floatval`/`(int)` cast are silently lossy and locale-blind (the
`"12abc" → 12`, `"1,50" → 1` traps). A typed, **fault-on-malformed** `Core.Num.parseInt(string) ->
int?` / `parseDecimal(string) -> decimal?` returning an optional (the S2 machinery is already shipped)
is a direct legibility+safety upgrade with an obvious PHP mapping (`filter_var`/`BigDecimal::of` with
explicit failure). Adopt in M-NUM alongside decimal.

**N-math-breadth — wider `Core.Math`.** The shipped `Core.Math` is missing the everyday business/
math staples: `round` (with scale!), `sign`, `clamp`, `gcd`/`lcm`, the trig/`log`/`exp` family, and
constants (`PI`/`E`). These are all pure, deterministic, std-only `f64`/`i64` natives that map directly
to PHP's `round`/`abs`/`gmp_gcd`/`sin`/`M_PI`. Low surprise, high daily utility. The one caveat is the
already-documented irrational-`f64` PHP-render divergence (`sqrt(2.0)`) — new transcendental functions
inherit it and must be documented the same way (kept out of byte-identity examples). Adopt in M-NUM.

**N-int-conv — explicit numeric conversions.** Phorj has no `int`↔`float`↔`decimal` conversion surface
yet; PHP's implicit coercions are a surprise source the philosophy explicitly targets. Explicit,
named, lossy-by-intent conversions (`x.toFloat()`, `x.toInt()` = documented truncation, `d.toFloat()`
= documented precision loss) are legible and make every narrowing a deliberate, visible act. Small
effort; ships with the decimal type that creates the need. Adopt in M-NUM.

## DEFER / REJECT notes (brief)

- **N-tz-iana / N-now-clock — defer.** IANA tz DB (no std crate) and `now()` (non-deterministic) both
  break the byte-identity spine the same way URL/network did; quarantine them in a M-TIME-2 / M6 runtime
  layer behind a clear boundary, after the deterministic core lands.
- **N-bigint — defer to M-NUM-2.** Real need (factorials, crypto-ish, exact large sums) but a separate
  bignum implementation; sequence it after `decimal` (which shares the mantissa machinery). Maps to
  `gmp`/`brick BigInteger`.
- **N-sized-int — defer to v2.** Sized integers are explicitly a v2 (native/systems) concern in the
  existing roadmap; they're a performance/FFI feature, not a business-correctness one. PHP has no sized
  ints, so the surprise budget is high for low business value pre-1.0.
- **N-rational — reject.** `BigRational` exists in `brick/math` but is a niche scientific need; for
  money, `decimal` + explicit rounding is the correct, legible model. Pure PL-elegance, fails the
  surprise-budget test.
- **N-percent / N-overflow-policy / N-uuid-id — reject/defer.** A percentage helper is sugar best left
  to userland on top of `decimal` (reject). Wrapping/saturating int ops are a v2/systems concern
  (defer). UUID/ID generation is non-deterministic (clock/random) and belongs to a general M4 stdlib
  effort, not this track (defer).

---
Sources consulted: [brick/math](https://github.com/brick/math),
[PHP BCMath manual](https://www.php.net/manual/en/book.bc.php),
[PHP DateTimeImmutable manual](https://www.php.net/manual/en/class.datetimeimmutable.php).

## Critic pass

Re-verified shipped state against `FEATURES.md`, `KNOWN_ISSUES.md`, `src/native.rs` (Core.Math =
exactly `sqrt`/`pow`/`floor`/`ceil`/`abs`/`min`/`max`), `src/value.rs` (kernels: `int_add/sub/mul/
div/rem/neg`, `float_div`, `int_rem` ships → `%` exists; no bitwise kernels), and `src/lexer.rs`
`scan_number` (decimal-only; line 732 comment confirms **no exponent**). All 19 original items are
genuinely unshipped — **0 mis-listed**. The original audit covered the headline business types
(decimal, time, math breadth) well but **missed the entire "numeric literal ergonomics + integer
operator surface" layer** — the everyday PHP numeric syntax a dev types without thinking. Those are
the highest-value finds below: cheap, zero-surprise, pure front-end, and currently silent parse
errors a PHP dev would hit on line one.

### Newly-found items

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| N-numeric-literals | Hex/octal/binary int literals + digit separators (`0xFF`, `0o17`, `0b101`, `1_000_000`) | port | strong | adopt | M-NUM | S |
| N-float-exponent | Float exponent notation (`1e6`, `2.5e-3`) | port | strong | adopt | M-NUM | S |
| N-pow-operator | `**` exponentiation operator (sugar over `Core.Math.pow`) | port | ok | defer | M-NUM | S |
| N-bitwise-ops | Integer bitwise ops `&` `\|` `^` `<<` `>>` `~` | port | ok | defer | M-NUM-2 | M |
| N-intdiv | Integer-division semantics / `intdiv` + `divmod` | port | strong | adopt | M-NUM | S |
| N-float-predicates | `isNan`/`isFinite`/`isInfinite` + `NaN`/`Infinity` constants | port | strong | adopt | M-NUM | S |
| N-numeric-minmax-breadth | `min`/`max`/`abs` on float; `min`/`max`/`sum` over `List<T>` | port | strong | adopt | M-NUM | S |
| N-money-currency | Composite `Money` (decimal amount + currency code) value type | new | ok | defer | M-NUM-2 | M |
| N-random | Seeded/deterministic-or-quarantined RNG (`random_int`/`mt_rand`) | port | weak | defer | M4 stdlib | M |

**N-numeric-literals (adopt, S) — the biggest miss.** `scan_number` accepts decimal digits only:
`0xFF`, `0o17`, `0b1010`, and `1_000_000` are all **parse errors today**. PHP has had all four
forms for years (hex/octal/binary literals; `_` separators since PHP 7.4). This is pure lexer work,
front-end-only, byte-identity-safe (literals fold to the same `i64`), and **zero surprise budget** —
it removes a paper-cut, not adds a concept. Ships in M-NUM with the rest of the numeric front end.

**N-float-exponent (adopt, S).** The lexer's float grammar is literally `digits '.' digits` — the
test at `lexer.rs:732` notes "no exponent". So `1e6`, `6.022e23`, `2.5e-3` don't lex. PHP and every
mainstream language have scientific notation; business/scientific data routinely arrives this way
(JSON/CSV). Trivial lexer extension, same `f64` value, byte-identical. Adopt in M-NUM.

**N-pow-operator (defer, S).** PHP has `**` (right-assoc) *and* `pow()`; Phorj ships `Core.Math.pow`
but no operator. Legible sugar, but lower-value than the literal gaps and adds an operator-precedence
slot — defer to the M-NUM polish wave once `Core.Math` breadth lands. Maps 1:1 to PHP `**`.

**N-bitwise-ops (defer, M).** PHP integer bitwise `& | ^ << >> ~` are absent in Phorj — and there's
now a **syntactic collision the original audit didn't flag**: `&` is `TokenKind::Amp` (intersection
types, S5) and `|` is `TokenKind::Bar` (unions, S4), both *type-level*. A value-level bitwise `&`/`|`
is disambiguated by context (type position vs expression position) the same way TS/PHP do, but the
collision must be designed for, not assumed. Real PHP feature, but niche for business code (flags,
permissions); defer to M-NUM-2. Maps 1:1 to PHP operators.

**N-intdiv (adopt, S).** PHP distinguishes `/` (always float-ish, throws on div-by-zero) from
`intdiv()` (integer division) and `%`. Phorj's `int / int` semantics need an explicit, documented
contract (truncating-toward-zero `intdiv`, with the matching `%` already shipped) so `7 / 2` isn't a
silent surprise. Cheap, removes a coercion surprise the philosophy targets, maps to PHP `intdiv`/`%`.
Bundle with N-int-conv/N-int-width as the integer-semantics backbone of M-NUM.

**N-float-predicates (adopt, S).** KNOWN_ISSUES already documents that `1.0/0.0` yields `inf`/`NaN`
on the Rust backends (a valid `f64`, not a fault) — but there is **no way to test for it**. PHP has
`is_nan`/`is_finite`/`is_infinite` + `INF`/`NAN`. Without predicates, a non-finite float is an
undetectable silent corruptor — exactly the surprise Phorj exists to remove. Pure deterministic
natives (`Core.Math.isNan`, etc.), byte-identical (these compare structurally, not via irrational
rendering). Adopt in M-NUM. (Note: the non-finite *transpile* fault-domain divergence stands; the
predicates themselves are byte-identical on finite + non-finite inputs.)

**N-numeric-minmax-breadth (adopt, S).** FEATURES pins `abs`/`min`/`max` to **int only**, and
`Core.List` has `sum` but no `min`/`max`. A PHP dev expects `min`/`max` to work on floats and over a
list (PHP `min($arr)`/`max($arr)`). These are pure generic natives on the already-shipped S7b
`List<T>` path (the same machinery as `reverse`/`sum`), so the effort is small and additive. Adopt in
M-NUM. (`abs` on float maps to PHP `abs`; `min`/`max` over `List<T>` to PHP `min`/`max`.)

**N-money-currency (defer, M).** A `Money` = `(decimal amount, string currencyCode)` value type
prevents the "added EUR to USD" class of bug — strictly beyond `decimal` alone, and the natural
*business* capstone of the track. But it's a composite built **on top of** N-decimal, has currency
metadata (ISO-4217 minor-unit tables) that's data-heavy, and is well-served by userland once decimal
ships. Defer to M-NUM-2 after the decimal core proves out. Maps to `brick/money` (the sibling of
`brick/math`).

**N-random (defer, M).** PHP `random_int`/`mt_rand` are ubiquitous, but RNG is non-deterministic and
breaks the byte-identity spine the same way `now()`/UUID do — it belongs in the same quarantined M4
runtime-stdlib layer, not this deterministic numerics track. (A *seeded* deterministic PRNG could ship
on-spine, but that's a separate design call; flagging it so it isn't silently forgotten.)

### Mis-listings

None. All 19 original items verified unshipped against `src/native.rs`, `src/value.rs`, `src/lexer.rs`,
`FEATURES.md`, and `KNOWN_ISSUES.md` (which explicitly lists `decimal` and sized ints as not-yet-done).
