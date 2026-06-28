# M-TIME — dates, time, durations (design)

Status: **design-locked** (2026-06-28). Milestone 1 of the "all four, in order" marathon (plan
`docs/plans/2026-06-27-ga-sequence.plan.md`, NEXT MARATHON section).

## Goal

A typed, deterministic, byte-identical time library: instants, durations, civil dates, and date-times,
mapping to idiomatic PHP — without ever touching PHP's locale/timezone-divergent `DateTimeImmutable`
machinery.

## Core decisions

1. **Pure-Phorj prelude + one native clock seam.** Everything except *reading the wall clock* is
   expressed as ordinary Phorj classes/functions in an **injected prelude** (mirrors
   `inject_http_prelude` / `inject_regex_prelude`). Because the prelude is run through the *same*
   transpiler as user code, its calendar + formatting math becomes PHP automatically and is
   byte-identical on `run`/`runvm`/real-PHP **by construction** — zero hand-rolled-PHP divergence risk.
   The only native is the clock (`Core.Time.nowMillis` and the freeze controls), hand-rolled identically
   in PHP exactly like `Core.Random` (decision: never delegate to PHP stdlib — `Core.Random` lesson).

2. **UTC-only, no timezones.** Timezones are environment-dependent (non-deterministic) and break the
   byte-identity spine. Civil breakdown is always UTC. (A `ZonedDateTime` is explicitly deferred.)

3. **Determinism via a freezable clock.** `Instant.now()` reads `Core.Time.nowMillis()`. Unfrozen, that
   reads the real wall clock → non-deterministic → **cannot** be a gated example (documented in
   KNOWN_ISSUES, like an unseeded note). `Time.freeze(millis)` pins it (process-global `RwLock<Option<i64>>`,
   same shape as `Core.Random`'s state) so every shipped example/conformance program is deterministic and
   byte-identical. `Time.unfreeze()` restores real-clock behavior.

4. **Integer epoch-millis is the canonical representation.** `Instant` wraps `int millis`; `Duration`
   wraps `int millis`. Calendar math uses days-since-epoch via Hinnant's truncating-division-safe
   `days_from_civil` / `civil_from_days` (validated: Phorj int `/` = truncate-toward-zero = PHP
   `intdiv`, so the algorithm ports verbatim). No floats anywhere → no rounding divergence.

5. **No new `Op`, no new `Value`.** The prelude is classes (`Value::Instance`); the clock is
   `Op::CallNative`. Front-end + registry only.

## Public API (final)

`import Core.Time;` injects bare classes `Instant`, `Duration`, `Date`, `DateTime` and makes the
`Core.Time` native module callable.

### `Duration` (a span; wraps `int millis`)
- static `seconds(int)`, `minutes(int)`, `hours(int)`, `days(int)`, `millis(int)` → `Duration`
- `toMillis() -> int`, `toSeconds() -> int`, `toMinutes() -> int`, `toHours() -> int`, `toDays() -> int`
- `plus(Duration) -> Duration`, `minus(Duration) -> Duration`, `negate() -> Duration`
- `isZero() -> bool`, `isNegative() -> bool`

### `Instant` (a point in time; wraps `int millis` since 1970-01-01T00:00:00Z)
- static `now() -> Instant` (clock seam), `ofEpochMillis(int)`, `ofEpochSeconds(int)`
- `epochMillis() -> int`, `epochSeconds() -> int`
- `plus(Duration) -> Instant`, `minus(Duration) -> Instant`
- `durationSince(Instant) -> Duration` (self - other)
- `isBefore(Instant) -> bool`, `isAfter(Instant) -> bool`, `compareTo(Instant) -> int`
- `toDateTime() -> DateTime` (UTC civil breakdown)

### `Date` (a civil date, UTC; wraps `int epochDay`)
- static `of(int year, int month, int day) -> Date`, `ofEpochDay(int) -> Date`
- `year() -> int`, `month() -> int`, `day() -> int`, `epochDay() -> int`
- `addDays(int) -> Date`, `addMonths(int) -> Date`, `plusDays`(alias), `minusDays`
- `daysUntil(Date) -> int`, `dayOfWeek() -> int` (1=Mon … 7=Sun, ISO-8601)
- `isLeapYear() -> bool`, `isBefore(Date)`, `isAfter(Date)`, `compareTo(Date) -> int`
- `toString() -> string` → `YYYY-MM-DD` (zero-padded)

### Civil (wall-time) view — **folded onto `Instant`** (not a separate class)

The original plan had a `DateTime` class. That was dropped during S3: the name **collides with PHP's
built-in `DateTime`** (a `package Main` class emits to the global PHP namespace → `Cannot redeclare
class`), and `Instant` already *is* the point in time. So the civil fields live on `Instant`:
- static `Instant.ofCivil(y, mo, d, h, mi, s) -> Instant` (UTC fields → instant)
- accessors `year`/`month`/`day`/`dayOfWeek`/`hour`/`minute`/`second`/`millis`/`millisOfDay` (UTC)
- `toIso() -> string` → `YYYY-MM-DDTHH:MM:SSZ` (zero-padded, always `Z`, second resolution)
- a printf-style `format(pattern)` was **dropped**: Phorj has first-class string interpolation, so
  custom layouts are just `"{i.day()}/{i.month()}/{i.year()}"` — no pattern mini-language needed.

## Native registry (`Core.Time`, new `src/native/time.rs`)

| name | sig | eval | php |
|------|-----|------|-----|
| `nowMillis` | `() -> int` | read frozen-or-wall clock | `__phorj_now_millis()` |
| `freeze` | `(int) -> void` | set frozen millis | `__phorj_now_freeze($0)` |
| `unfreeze` | `() -> void` | clear frozen | `__phorj_now_unfreeze()` |

`pure: false` (the unfrozen result depends on wall-clock). The PHP helpers are emitted via the existing
runtime-helper injection path (same mechanism as `__phorj_rng_*`). A `static $__phorj_clock` holds the
frozen value.

## Slices (each ships green + byte-identical + guide example + conformance + docs)

- **S1 — Instant + Duration + clock seam.** `time.rs` native, prelude with `Instant`/`Duration`,
  `examples/guide/time.phg` (freeze the clock, arithmetic, comparisons), conformance program.
- **S2 — Date (civil calendar).** Add `Date` to the prelude (Hinnant algorithm, leap year, day-of-week,
  ISO `toString`). `examples/guide/dates.phg`, conformance.
- **S3 — DateTime + ISO formatting.** Add `DateTime` (Instant↔civil, `toIso`, `format`). Fold into the
  time/date examples or a `datetimes.phg`, conformance.

## Verification

Per slice: `cargo test --workspace`, `PHORJ_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php
PHORJ_REQUIRE_PHP=1 cargo test --workspace` (3-way oracle), `cargo clippy --all-targets`,
`cargo fmt --check`. The example glob in `tests/differential.rs` gates each new `examples/guide/*.phg`.

## Deferred (KNOWN_ISSUES)

Timezones / `ZonedDateTime`; locale-aware formatting; sub-millisecond precision; parsing arbitrary
formats (only fixed ISO parse if added); unfrozen `Instant.now()` is non-deterministic (freeze for
reproducible output).
