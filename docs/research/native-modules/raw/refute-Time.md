# Stage 2b — Adversarial Byte-Identity Review: `Core.Time` (M-TIME)

**Verdict: determinism does NOT hold as the spike claims.** The Tier-A/Tier-B split, UTC-only
`gmdate`/`gmmktime` strategy, locale-immunity, and no-new-Op claims are all **verified correct**.
But the spike's own implementation prescriptions contain **two confirmed silent run/runvm-vs-PHP
divergences** and **two under-specified surfaces** that will ship byte-different unless explicitly
nailed down. The headline 85% is too high for the strategy *as written*; it is achievable only after
the contradictions below are resolved. Recommend `revised_feasibility ≈ 72%`, tier still `mixed`.

All probes run against `/stack/tools/phpbrew/php/php-8.5.7/bin/php -n` (PHP 8.5.7 ZTS DEBUG) — the
exact oracle floor.

---

## CONFIRMED REFUTATION 1 — `diffDays`/`addDays` div sign: spike contradicts itself (CRITICAL, silent)

The spike says two incompatible things in the same document:
- §3 / §7.2: use `div_euclid`/`rem_euclid` (**floor toward −∞**) in the kernel to match `gmdate`.
- §4 / §3 (`diffDays`): transpile to **`intdiv((a)-(b), 86400)`** — and PHP `intdiv` **truncates
  toward zero**, NOT floor.

These disagree for any negative difference. Verified:
```
PHP   intdiv(-86401, 86400) = -1          # truncate toward zero
Rust  (-86401_i64).div_euclid(86400) = -2 # floor toward -inf
```
So if the Rust `diffDays` kernel uses `div_euclid` (as §3/§7.2 mandate for the *calendar* kernel) and
the transpiler emits `intdiv` (as §4 mandates), `Time.diffDays(earlier, later)` returns **−2 on the
interpreter/VM and −1 under real PHP** — a 1-byte+ stdout divergence that **only manifests when the
first argument is the earlier instant** (a<b). Every "later − earlier" example passes; the bug ships
green. The floor-division mitigation the spike is *proud of* is precisely what breaks `diffDays`,
because the civil-from-days kernel and the `diff` reducer have different correct rounding (the kernel
needs floor for the calendar; `diff` must match `intdiv`'s trunc). The spike conflates them.

**Refutes** the claim that "`diffDays` … with `div_euclid`" is byte-identical. It is not, against the
spike's own `intdiv` transpile target. Fix: `diffDays` must use the SAME rounding on both legs —
either emit a `__phorj_intdiv`-style trunc on the Rust side for `diff`, or floor on both (a PHP
helper, not bare `intdiv`). This is a real design decision the spike left contradictory, not a typo.

---

## CONFIRMED REFUTATION 2 — `gmmktime` 2-digit & small-year coercion (CRITICAL, silent)

The spike transpiles `Time.toUnix(y,m,d,h,mi,s)` directly to `gmmktime(h,mi,s,m,d,y)` and computes the
Rust side with a literal `days_from_civil(y, …)`. But PHP `gmmktime` applies **legacy 2-digit-year
coercion** that a literal Rust kernel will not:
```
in y=0   -> gmdate Y = 2000     (Rust days_from_civil(0)  => year 0000)
in y=50  -> 2050                 (Rust => 0050)
in y=69  -> 2069
in y=70  -> 1970                 (the 69/70 pivot!)
in y=99  -> 1999
in y=100 -> 2000                 (Rust => 0100)
in y=999 -> 0999                 (here they happen to agree)
```
So `Time.toUnix(70, 1, 1, 0,0,0)` ⇒ interpreter/VM produce a year-70 epoch, real PHP produces a
year-**1970** epoch. Total divergence for any caller passing a year < 100. The spike's leap-day example
`toUnix(2000,2,29,…)` uses a 4-digit year and **cannot catch this** — exactly the all-positive-example
blind spot it warns about elsewhere, reproduced in its own test plan. The differential MUST include a
`toUnix` with a 1- or 2-digit year, or this is invisible.

Fix options: reject `year < 100` in the native (documented edge), or replicate PHP's pivot in BOTH the
Rust kernel and a `__phorj_gmmktime` helper. Either is fine — but the spike's "direct
`gmmktime` map" is **not** byte-identical as written.

---

## CONFIRMED REFUTATION 3 — `gmmktime` out-of-range normalization (HIGH, silent)

PHP `gmmktime` silently **normalizes** out-of-range fields; a Rust `days_from_civil` kernel will either
reject or compute a different value:
```
gmmktime(0,0,0, 2,30, 2000) -> 2000-03-01   (Feb 30 rolls forward)
gmmktime(0,0,0,13, 1, 2000) -> 2001-01-01   (month 13 -> next year)
gmmktime(25,0,0, 1, 1, 2000) -> +1 day      (hour 25 normalizes)
```
The spike never states whether `Time.toUnix` rejects out-of-range fields (clean `E-*`/fault) or
normalizes like PHP. If the Rust kernel rejects (the natural EV-7 / `checked_*` choice) but PHP
normalizes, a program calling `toUnix(…, month=13, …)` faults on run/runvm and succeeds under PHP →
divergence. If the kernel normalizes, it must reproduce PHP's exact rollover (and that is no longer the
"well-known ~60-line integer algorithm" the effort estimate assumes). Under-specified = not yet
byte-identical.

---

## UNDER-SPECIFIED SURFACE 4 — unpinned `gmdate` letters leak literally (MEDIUM)

The spike says "any letter outside [the pinned set] is rejected by the checker/native." But PHP
`gmdate` passes unknown letters through *literally* and has live letters the spike's table omits:
```
gmdate("Q", 0) = "Q"     # unknown -> literal passthrough
gmdate("S", 0) = "st"     # S = ordinal suffix, a REAL gmdate letter, NOT in the pinned table
```
For a *literal* format string the compile-time letter check catches this. But the spike *recommends*
the **dynamic-format `__phorj_gmdate` runtime helper** (§8) "for ergonomics." A runtime helper that
validates against the pinned set will **reject** `"S"` while a naive `gmdate($f,$e)` transpile would
**accept** it — and worse, if the helper ever falls back to bare `gmdate` for an unrecognized letter,
the Rust renderer (which has no `S`/`Q` arm) and PHP diverge. The dynamic-format path is the 70%-risk
locus and the spike waves it through. Pin: v1 should be **literal-format-only** (compile-time check),
not the recommended dynamic helper, to keep the byte-identity guarantee airtight.

---

## VERIFIED-SAFE (the spike's correct claims — granting these)

- `gmdate` ignores `$TZ`/`date.default_timezone`: ✅ `TZ=America/New_York … gmdate(…,0)` = UTC.
- Negative epoch floor calendar: ✅ `gmdate("Y-m-d H:i:s", -1)` = `1969-12-31 23:59:59` (so the
  *civil-from-days* kernel genuinely needs `div_euclid` — correct, just don't reuse it for `diff`).
- `setlocale(LC_TIME,"fr_FR.UTF-8")` has NO effect on `gmdate("D l F M")`: ✅ still `Thu Thursday
  January Jan`. Hard-coded English tables are safe.
- `Y` width: `gmdate("Y")` zero-pads to 4 for years < 1000 (`0500`, `0001`, `0999`) ⇒ Rust
  `format!("{:04}", y)` AGREES. ✅
- No new `Op`/`Value` (epochs `Int`, output `Str`/`Int`): ✅ structurally sound; `CallNative` path.
- Tier B `now()` quarantine via `pure: false`: ✅ correct, matches Process/Env precedent.
- No float in Tier A ⇒ Ryū divergence cannot bite: ✅ confirmed integer-only surface.
- `(int)"08"` / `(int)"010"` in the parse helper: base-10, no octal trap: ✅ safe.

---

## Net assessment

determinism_holds = **false** as written. The strategy is *fundamentally sound* (UTC/gmdate/no-Op/
integer-only are the right pins and all verified), but the spike ships **two confirmed silent
divergences in its own transpile prescriptions** (`intdiv`-vs-`div_euclid` for `diffDays`; literal
`gmmktime` map ignoring PHP's 2-digit-year pivot) plus an unaddressed normalization gap and an
over-eager dynamic-format recommendation. None are fatal to the milestone — each has a concrete fix —
but each is a real byte-break that the spike's *proposed examples cannot catch* (all use 4-digit years
and later−earlier diffs). Feasibility is real but the 85% over-credits an implementation plan that is
internally contradictory on rounding. Revised: ~72%, tier mixed, conditional on the four fixes +
mandatory differential cases: negative `diffDays` (a<b), `toUnix` with a 2-digit year, an out-of-range
`toUnix` field, and literal-only format strings in v1.

Confidence: **high** (all four refutations directly reproduced under `php -n` 8.5.7).
