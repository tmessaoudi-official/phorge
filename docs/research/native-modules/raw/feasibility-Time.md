# Feasibility Spike — `Core.Time` (date arithmetic + format + parse; `now()` is Tier B)

**Verdict (one line):** **Adopt now** a **Tier A pure** `Core.Time` covering UTC-only Unix-instant
arithmetic, `gmdate`-letter formatting, and explicit-format parsing. The whole pure surface erases to
PHP **`gmdate` / `gmmktime`** (TZ-independent core functions, verified present under `php -n`), so it is
fully byte-identity-gateable with **no new VM `Op`**. **`Time.now()` is Tier B** — quarantined with the
existing `pure: false` seam (Process/Env precedent), excluded from `differential.rs`. **Timezone/DST and
locale month/day *names* are explicitly out of scope for Tier A** — pin **UTC + hard-coded English
names + numeric ASCII** by construction. Overall feasibility **~85%**; the instant-arithmetic +
`gmdate`-format core is **~92%**, format-string *parse* is **~70%** (the determinism-and-effort locus),
`now()` is trivially Tier B.

---

## 0. Determinism partition — the framing that decides everything

The single fact that makes M-TIME feasible at all (verified, not recalled):

```
$ TZ=America/New_York php -n -r 'echo gmdate("Y-m-d H:i:s", 0)."\n";'
1970-01-01 00:00:00          # gmdate IGNORES $TZ — always UTC
$ php -n -r 'echo date("Y-m-d H:i:s", 0)."\n";'
1970-01-01 00:00:00          # date() reads date.default_timezone (UTC under -n) — TZ-SENSITIVE
```

- **Tier A (pure, gateable):** everything that is a pure function of an **explicit `int` Unix epoch
  (UTC seconds)** — `fromUnix`, `toUnix`, component extraction (`year`/`month`/`day`/`hour`/`minute`/
  `second`/`dayOfWeek`/`dayOfYear`), second-granular arithmetic (`addSeconds`/`addDays`/`diffSeconds`/
  `diffDays`), `format(epoch, fmt)` over a pinned letter subset, and `parse(s, fmt) -> int?`. All
  transpile to **`gmdate`/`gmmktime`** (or a `__phorj_time_*` helper) which are **TZ-env-independent**.
- **Tier B (impure, quarantined):** `Time.now() -> int` (reads the wall clock). One native, `pure:
  false`, excluded from the byte-identity differential exactly like `Core.Process`/`Core.Env`.

The "timezone/DST minefield" is **scoped out, not solved**: Tier A is **UTC-only**. No `DateTimeZone`,
no DST, no `add 1 month` calendar ambiguity in v1 (see §8). This is the same discipline that let
`Core.Url` ship its pure subset first.

---

## 1. std-only feasibility — YES, fully

No external crate is needed for any Tier A function. The entire civil-calendar ↔ epoch conversion is
integer arithmetic:

- **epoch → (y,m,d,h,mi,s):** the standard **days-from-civil / civil-from-days** algorithm (Howard
  Hinnant's `civil_from_days`, public-domain integer math) — pure `i64` ops, no float, no lookup that
  can drift. Handles the proleptic Gregorian calendar including leap years and **negative epochs**
  (pre-1970) correctly with floor division.
- **(y,m,d,h,mi,s) → epoch:** the inverse `days_from_civil`, then `*86400 + h*3600 + mi*60 + s`.
- **formatting:** a `match` over format letters writing into a `String` with `write!`/`push_str` +
  zero-padding via `format!("{:02}", n)`.
- **parse:** a hand-rolled scanner walking the format string and the input in lockstep (`%Y` consumes
  4 digits, `-`/literal must match, etc.) — pure byte/char scanning.

**std APIs relied on:** `i64` arithmetic (`checked_add`/`checked_mul`/`rem_euclid`/`div_euclid` — the
last two are the floor-division primitives that match PHP's `gmdate` for negative epochs), `format!`,
`String::push_str`, `str::as_bytes`, `u8::is_ascii_digit`, `i64::from_str_radix`. All edition-2021 std.

**For `now()` (Tier B):** `std::time::SystemTime::now().duration_since(UNIX_EPOCH)` → `i64` seconds.
Pure std, no crate. Crypto/TLS/RNG are not involved.

**Verified PHP-core availability under `php -n`** (ran `php -n -r 'function_exists(...)'`):
`gmdate`, `gmmktime`, `mktime`, `date`, `checkdate`, `sprintf`, `intdiv`, `str_pad`, `explode`,
`strtotime` — **all present**. So a transpile target exists; the question is *byte-identity*, addressed
in §3.

---

## 2. Tier A vs Tier B

| Function | Tier | Rationale |
|---|---|---|
| `Time.fromUnix(int) -> DateTime` (struct) | A | pure decode of an explicit epoch |
| `Time.toUnix(DateTime) -> int` | A | pure encode |
| `dt.year()/month()/day()/hour()/minute()/second()` | A | component reads |
| `dt.dayOfWeek() -> int` (0=Sun, matches `gmdate("w")`) | A | pure modular arithmetic |
| `dt.dayOfYear() -> int` | A | pure |
| `dt.addSeconds(int)` / `dt.addDays(int)` | A | epoch + n; re-decode |
| `Time.diffSeconds(a,b)` / `Time.diffDays(a,b)` | A | epoch subtraction |
| `Time.format(int epoch, string fmt) -> string` | A | pinned-letter `gmdate` |
| `Time.parse(string, string fmt) -> int?` | A | explicit-format scan (no `strtotime`) |
| `Time.now() -> int` | **B** | wall clock — `pure: false`, quarantined |

**There is no Tier-A `now()`.** Any clock read poisons determinism; it matches the `Process`/`Env`
precedent exactly.

---

## 3. Byte-identity strategy (the load-bearing part)

**Pin three things by construction — all verified against `php -n` PHP 8.5:**

1. **UTC only → `gmdate`/`gmmktime`, never `date`/`mktime`.** `gmdate`/`gmmktime` ignore `$TZ` and
   `date.default_timezone` (verified above: `TZ=America/New_York` had no effect). So the PHP leg is
   stable regardless of the oracle subprocess's environment — the exact fragility that quarantines
   `Core.Env` does **not** bite here, because we never read ambient TZ state.

2. **English names are hard-coded and safe.** Verified: `setlocale(LC_TIME, "fr_FR.UTF-8")` does **NOT**
   change `gmdate("D l F")` — it still prints `Thu Thursday January`. PHP's `gmdate`/`date` letter-names
   are compiled-in English; only the (now-removed) `strftime` was `LC_TIME`-sensitive. So a Rust-side
   hard-coded `["Sunday","Monday",…]` / `["January",…]` table is byte-identical to `gmdate("l"/"F")`
   with zero locale risk. **Do not** transpile to or rely on `strftime`/`IntlDateFormatter` (absent
   under `-n` anyway).

3. **Negative-epoch floor-division.** Verified PHP behaves as floor toward `-∞` for the calendar
   (`gmdate("Y-m-d H:i:s", -1)` → `1969-12-31 23:59:59`). Rust `i64 % / /` truncate toward zero, which
   **diverges for negative epochs**. Fix: use `i64::div_euclid`/`rem_euclid` (floor semantics) in the
   civil-from-days kernel. This is the #1 silent-break trap — the differential must include a
   **negative-epoch example** (e.g. `1969-12-31`) to catch it, since all-positive examples would pass
   while the kernel is wrong.

**Format-letter pinning (the gateable subset — minimal, all numeric/English, all `gmdate`-faithful):**

| Letter | Meaning | Rust render | `gmdate` |
|---|---|---|---|
| `Y` | 4-digit year | `format!("{:04}", y)` | same |
| `m` | 2-digit month | `{:02}` | same |
| `d` | 2-digit day | `{:02}` | same |
| `H` | 2-digit 24h hour | `{:02}` | same |
| `i` | 2-digit minute | `{:02}` | same |
| `s` | 2-digit second | `{:02}` | same |
| `j` | day no leading zero | `{}` | same |
| `n` | month no leading zero | `{}` | same |
| `w` | day-of-week 0=Sun | int | same |
| `N` | day-of-week 1=Mon..7=Sun | int | same |
| `D` | short day name (English) | table `Thu` | same (verified) |
| `l` | full day name (English) | table `Thursday` | same (verified) |
| `F` | full month name (English) | table | same (verified) |
| `M` | short month name (English) | table | same (verified) |

Any letter outside this set is rejected by the checker/native (a documented edge), so we never emit a
`gmdate` letter whose Rust render we haven't pinned.

**No float anywhere in Tier A** → the Ryū/`__phorj_str` float-divergence KNOWN_ISSUE cannot bite. Every
output is an `int`, a zero-padded `string`, or an English name. This is a major reason the feasibility is
high: time is integer-native, unlike Math/CSV which carry floats.

**`diffDays` / `addDays`:** pure `epoch ± n*86400` and `(a-b)/86400` with `div_euclid` — second-based, so
no DST/calendar ambiguity (a "day" is exactly 86400 s in UTC, which is correct because we have no leap
seconds and no DST). Calendar-month arithmetic (`addMonths`) is deliberately deferred (§8).

---

## 4. Exact PHP transpile targets

Each native's `php` closure (single-sourced in the `NativeFn` registry, like `Core.Math`):

- `Time.fromUnix(e)` / component reads — the **`DateTime` struct is Phorj-defined** (an injected enum/
  class via the `inject_*_prelude` pattern, like `Core.Json`'s `Json` and `Core.Math`'s `RoundingMode`),
  so `fromUnix` builds the struct from `gmdate`-extracted fields and the transpiler emits a small
  constructor expression — OR, simpler and recommended, **make `DateTime` opaque: carry the `int` epoch
  only** and compute components on demand (see §5). With epoch-carrier, the transpile is direct:
  - `Time.format(e, f)` → `gmdate(<f>, <e>)` (for a literal pinned format string; a dynamic format string
    routes through a `__phorj_gmdate` helper that pre-validates the letter set, or is rejected at
    compile time if non-literal — see §8).
  - `dt.year()` → `(int)gmdate("Y", <e>)`, `dt.month()` → `(int)gmdate("n", <e>)`, etc.
  - `dt.dayOfWeek()` → `(int)gmdate("w", <e>)`.
  - `dt.addSeconds(n)` → `(<e>) + (<n>)`; `dt.addDays(n)` → `(<e>) + (<n>) * 86400`.
  - `Time.diffSeconds(a,b)` → `(<a>) - (<b>)`; `diffDays` → `intdiv((<a>) - (<b>), 86400)`.
  - `Time.toUnix(y,m,d,h,mi,s)` (or from struct) → `gmmktime(<h>,<mi>,<s>,<m>,<d>,<y>)`.
  - `Time.parse(s, fmt)` → a **`__phorj_time_parse($s,$fmt)` runtime helper** (gated `uses_time_parse`
    bool + `emit_runtime_helpers`), NOT `strtotime`/`DateTime::createFromFormat`. `strtotime` is
    heuristic/locale-ish/version-drifting (the `filter_var`-style trap); `createFromFormat` pulls
    timezone state. The helper is a small hand-written PHP scanner mirroring the Rust one, returning
    `?int` (epoch via `gmmktime` of the scanned fields, or `null` on mismatch). This guarantees
    byte-identity because **both legs run the same algorithm**, not two different library parsers.

**Why a helper for parse but a direct map for format/arithmetic:** format/arithmetic have an exact,
stable PHP-core twin (`gmdate`/`gmmktime`/`intdiv`); parse does not (every PHP parser is heuristic or
TZ-bearing), so we own the algorithm on both legs — the documented "prefer a runtime helper when static
types/builtins are insufficient" rule from the M7 PHP-leg memory.

---

## 5. Phorj API sketch

**Recommended representation: `DateTime` as an opaque epoch carrier** (a `package Main` injected class
with one `int` field, or — even lighter — Tier-A ops just take/return `int` epochs and the struct is
sugar). Carrying only the epoch keeps `fromUnix`/`toUnix` a no-op and sidesteps a wide struct value:

```phorj
package Main;
import Core.Time;
import Core.Console;

// Tier A — pure, byte-identity-gated
int epoch = 1700000000;                          // explicit UTC seconds (Time.now() is Tier B)
string s  = Time.format(epoch, "Y-m-d H:i:s");   // "2023-11-14 22:13:20"  (gmdate, UTC)
int  yr   = Time.year(epoch);                    // 2023
int  dow  = Time.dayOfWeek(epoch);               // 0=Sun..6=Sat (gmdate "w")
int  tmrw = Time.addDays(epoch, 1);
int  days = Time.diffDays(tmrw, epoch);          // 1
int  e2   = Time.toUnix(2000, 2, 29, 0, 0, 0);   // gmmktime — leap day OK
int? p    = Time.parse("2023-11-14", "Y-m-d");   // -> int? (null on mismatch)

Console.println(s);
Console.println(p ?? -1);                        // composes with S2 ?? / if-let
```

If a richer `DateTime` struct is wanted (`dt.year()` method syntax), it rides **UFCS** (`Time.year(e)`
≡ `e.year()` only if `e` is the carrier type) — but the simplest v1 is **free functions over `int`
epochs**, no new injected type at all, no `inject_*_prelude` plumbing. That is the lowest-risk slice and
what I recommend shipping first; the struct is a follow-up.

---

## 6. New VM Op needed? — NO

Every function is a `Core.Time.*` native → `Op::CallNative(idx, argc)` (already exists). `now()` is a
zero-arg `CallNative` with `pure: false`. No new `Op`, no `Value` variant (epochs are `Value::Int`;
formatted output is `Value::Str`; `parse` returns `Value::Int`/`Value::Null` for `int?`). If a struct
`DateTime` is later added it reuses the injected-type pattern (`Core.Json`/`RoundingMode` precedent) —
still no new `Op` or `Value`. **"No new Op" is satisfied.**

---

## 7. Named determinism risks

1. **`date`/`mktime` vs `gmdate`/`gmmktime` (CRITICAL).** Transpiling to the local-TZ variant makes the
   PHP leg read `date.default_timezone` → flaky across machines/CI. **Mitigation:** the registry `php`
   closures must emit **only** `gmdate`/`gmmktime`. Add a grep-guard test asserting no `Core.Time` emit
   contains `\bdate(` / `\bmktime(` (only `gmdate`/`gmmktime`). Verified: `gmdate` ignores `$TZ`.
2. **Negative-epoch truncation vs floor (CRITICAL, silent).** Rust `%`/`/` truncate toward zero; PHP
   `gmdate` is floor toward `-∞`. **Mitigation:** `div_euclid`/`rem_euclid` in the civil kernel + a
   **negative-epoch differential example** (pre-1970 date). Without that example the bug ships green.
3. **Locale month/day names.** *Resolved as a non-risk for `gmdate`/`date`* (verified `setlocale` has no
   effect on `gmdate("D l F")`). Stays safe **only** because we hard-code English and never touch
   `strftime`/`IntlDateFormatter` (the latter is absent under `-n` regardless). Documented invariant.
4. **`parse` via PHP library functions.** `strtotime`/`DateTime::createFromFormat` are heuristic /
   TZ-bearing / version-drifting (the `filter_var` trap). **Mitigation:** own the algorithm in a
   `__phorj_time_parse` helper mirrored on both legs; never call a PHP date-parsing builtin.
5. **i64 overflow on huge epochs / `addSeconds`.** `checked_add`/`checked_mul`; overflow → clean fault
   (EV-7), byte-identical via FaultKind. The PHP helper bounds-checks to match (or examples stay in a
   sane range).
6. **`gmmktime` argument-order footgun.** PHP `gmmktime(hour, minute, second, month, day, year)` — the
   reverse of intuition. The `php` closure must place args in that exact order (transpile-arg-order trap,
   same class as `array_map`/`array_reduce`). A round-trip example (`toUnix` then `format`) catches it.
7. **Float formatting.** *Not a risk* — Tier A is integer-only, no float path exists, so the Ryū
   KNOWN_ISSUE cannot manifest.
8. **`now()` non-determinism.** Handled by the `pure: false` quarantine; `tests/time.rs` (or the existing
   process-style test) asserts `now()` returns a plausible monotone-ish int with a controlled fixture;
   it never enters `differential.rs`. `examples/time/now/` is a README walkthrough, not a gated example.

---

## 8. Scope / non-goals (v1 Tier A)

- **UTC only.** No `DateTimeZone`, no DST, no offset arithmetic. (The whole point — TZ/DST is the
  minefield; UTC is the gateable island.) A future M-TIME-2 slice can add fixed-offset zones (still
  deterministic) and explicitly defer DST/IANA-tz (which would need a tzdata blob — non-std, non-deterministic across PHP versions).
- **Second-granular arithmetic only.** `addSeconds`/`addDays` are exact. **`addMonths`/`addYears`
  deferred** — calendar-month arithmetic has the "Jan 31 + 1 month = ?" ambiguity that PHP and a naive
  impl disagree on; ship it as a named, documented slice with an explicit rounding policy, not silently.
- **Explicit-format parse only.** No natural-language / `strtotime` parsing — heuristic, undeterministic,
  version-drifting. The format dialect is the **PHP `gmdate` letter subset** (§3), minimizing
  Rust↔PHP translation (one dialect, not Go `2006-01-02` + strftime `%Y` + PHP `Y`).
- **Dynamic (non-literal) format strings:** either route through a `__phorj_gmdate` helper that
  validates the letter set at runtime, or restrict v1 to literal format strings (compile-time letter
  check). Recommend the helper for ergonomics; both are byte-identical.
- **No fractional seconds / microseconds** in v1 (PHP `microtime` is a clock → Tier B anyway).

---

## 9. Effort

**Medium.** Comparable to `Core.Url` (which split a ~95% encode core from a ~65% parser). Breakdown:
- Tier A free-function core (`fromUnix`/`toUnix`/components/`addSeconds`/`addDays`/`diff*`/`format`):
  one `src/native/time.rs` + the civil-calendar kernel (well-known integer algorithm, ~60 lines) +
  registry entries + the `gmdate`/`gmmktime` php closures + `examples/guide/time.phg` (incl. a
  **negative-epoch** and a **leap-day** case). ~1 focused session.
- `Time.parse` + `__phorj_time_parse` helper (the harder ~70% piece — two mirrored scanners, fault
  parity): a second slice.
- `Time.now()` Tier B + quarantine test + README walkthrough: small, reuses the `pure: false` seam.

No backend plumbing changes (the four-backend native call path is already generic), no new `Op`, no new
`Value`. The risk budget is concentrated in the two CRITICAL determinism traps (§7.1, §7.2), both
guard-testable.

---

## 10. Recommendation

**Adopt now** — ship Tier A as two slices (core arithmetic+format first, `parse` second) plus the Tier B
`now()` behind the existing quarantine seam. The byte-identity story is **stronger than most candidate
modules** because time is integer-native (no float divergence) and PHP's `gmdate`/`gmmktime` are
genuinely TZ-independent core functions (verified). The historically-scary parts — timezone, DST, locale
names — are either *scoped out* (UTC-only) or *verified non-issues* (`gmdate` names are locale-immune).
The two real traps (gmdate-not-date, floor-division on negative epochs) are concrete and caught by two
specific differential examples. Feasibility **~85%** overall; core **~92%**, parse **~70%**.

Confidence: **high** for the Tier A/Tier B split, the `gmdate` byte-identity strategy, no-new-Op, and the
two critical traps (all directly verified against `php -n` PHP 8.5.7). Medium for the exact effort of the
`parse` helper.
