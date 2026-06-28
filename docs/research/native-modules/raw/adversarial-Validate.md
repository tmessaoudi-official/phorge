# Stage 2b — Adversarial Byte-Identity Review: `Core.Validate`

**Verdict: determinism_holds = FALSE as written.** The 88% Tier-A feasibility claim survives in
substance, but the spike's **`isEmail` PHP helper as written (`preg_match($pat, $s) === 1` with a
`$` anchor) breaks byte-identity** against a Rust end-of-input byte scanner. The break is real,
reproduced, and silent. With one concrete fix (`$` → `\z`) the claim becomes true. I therefore
**refute the helper as written** and require an amendment before the slice can claim run≡runvm≡PHP.

All tests run under the declared floor: `/stack/tools/phpbrew/php/php-8.5.7/bin/php -n`
(`PHP 8.5.7 (cli) ZTS DEBUG`), and Rust via `rustc -O`.

---

## R1 — PRIMARY REFUTATION: PCRE `$` matches before a trailing newline; a Rust byte scanner does not

PCRE's `$` (default, non-multiline) matches at end-of-subject **OR immediately before a trailing
`\n`**. The spike's email helper anchors with `$`:

```php
return preg_match('/^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$/', $s) === 1;  // spike §5
```

Measured (PHP 8.5.7 `-n`):

| input | spike PHP helper (`$`) | Rust byte scanner (end-of-input check) |
|---|---|---|
| `"a@b.com"` | 1 | true |
| `"a@b.com\n"` | **1** | **false** |
| `"a@b.co\nm"` | 0 | false |
| `"\na@b.com"` | 0 | false |

`"a@b.com\n"` → PHP says valid, a Rust scanner that requires `pos == input.len()` after the TLD says
invalid. **One byte differs → the differential PHP oracle fails.** This is exactly the byte-identity
spine the project guards. The spike's §2 measurement table never tested a trailing-newline input, so
it missed this — its "verified identical 8.5 and 8.6-dev" claim is true only for the inputs it chose,
all of which lacked a trailing `\n`.

**This is not theoretical for `Core.Validate`:** a validator's entire job is to be handed arbitrary,
untrusted, often line-delimited input. Trailing `\n` is the single most common contaminant in real
validation calls (file lines, form fields, stdin).

### Fix (verified): use `\z`, not `$`

`\z` matches **only** true end-of-subject. Measured:

```php
$pat = '/^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\z/';
"a@b.com"   => 1
"a@b.com\n" => 0   // now agrees with Rust
```

The Rust side must correspondingly check `position == bytes.len()` (or equivalently reject any byte
not in the grammar, including `\n`). With `\z` on the PHP side and a strict end-of-input check on the
Rust side, the email predicate is byte-identical. **This fix is mandatory and was absent from the
spike.**

## R2 — CORROBORATING EVIDENCE: the same `$`-anchor trap is already a latent bug in the SHIPPED `__phorj_parse_float` helper

The transpiler's existing `__phorj_parse_float` (src/transpile/program.rs) is
`preg_match($re, $s) === 1 ? (float)$s : null` with a `$`-anchored regex and **no round-trip guard**.
Measured:

| input | shipped parse_float PHP | Rust `f64::from_str` |
|---|---|---|
| `"1.5"` | `(float)` 1.5 | true |
| `"1.5\n"` | **`(float)` 1.5** | **false** |

So `Text.parseFloat("1.5\n")` already diverges run↔PHP today. This is *not* a Validate finding per se
(it's a pre-existing issue worth filing), but it **proves the `$`-anchor class of bug is real and
ships undetected** when the example set avoids trailing-newline inputs — exactly the blind spot the
Validate spike repeats. Recommend the Validate amendment also `\z`-harden parse_float, or at least
file it in KNOWN_ISSUES.

## R3 — WHY `isInt`/`toInt` SURVIVE (the spike's reuse decision is sound, for a non-obvious reason)

`__phorj_parse_int` uses the same `$`-anchored `preg_match('/^[+-]?[0-9]+$/', $s)`, so `"5\n"` DOES
pass its `preg_match`. But it is **rescued by its round-trip guard**:
`$digits = ltrim("5\n","+-") = "5\n"`, while `(string)(int)"5\n" = "5"`, and `"5" !== "5\n"` → returns
null. Measured: `__phorj_parse_int("5\n") = null`, matching Rust `"5\n".parse::<i64>() = Err`. So
**`isInt`/`toInt` aliasing `__phorj_parse_int` is byte-identical** — confirmed across `5`, `5\n`,
`\n5`, `05`, `-5`, `5\n6`. The spike's §5-note recommendation to define `isInt` as
`__phorj_parse_int($s) !== null` is correct and is the only int path that holds. The literal
`__phorj_validate_int` body in §5 is pseudocode (`... ; // (range guard, see note)` does not parse)
and must not be shipped; the note already redirects away from it.

`filter_var INT` would break (`"05"` int=false/Rust=true; `" 5"`/`"5 "`/`"5\n"` filter=true/Rust=false)
— the spike's "don't use filter_var" is independently re-confirmed.

## R4 — `isIpv4` SURVIVES (the explode/byte-scan helper rejects `\n` naturally)

The IPv4 helper is `explode('.')` + per-octet ASCII-digit scan. A trailing `\n` lands in the last
octet, fails the `$o[$k] < '0' || $o[$k] > '9'` digit check → false. Measured: `"1.2.3.4\n"` → 0,
matching a Rust rule that scans each octet byte-by-byte. Also confirmed `256.0.0.1`→0, `01.2.3.4`→0
(leading-zero reject). **No PCRE, no `$`-anchor, no divergence.** `isIpv4` is fully gateable as
claimed. (Note: the spike correctly avoids `std::net::Ipv4Addr`, whose parser accepts forms PHP
rejects — owning the rule is the right call.)

## R5 — Determinism trap list (cleared)

- **Object ids / addresses / hash-map order:** not reachable — every predicate returns
  `Bool`/`Int`/`Null`, no Map/Set/Instance construction, no iteration order exposed. ✅
- **Float formatting (Rust Ryū vs PHP):** not reachable — no float is ever *formatted* by Validate
  (`toInt` returns an int; there is no `toFloat` in the API). The parse_float trap in R2 is a
  *separate* shipped helper, not Validate's surface. ✅ for Validate as scoped.
- **Locale:** no `strcasecmp`/`ctype_*`/locale-sensitive compare; ASCII char-class literals only. ✅
- **Clock / RNG:** none. ✅
- **mbstring under `php -n`:** confirmed absent; every rule is single-byte ASCII (`strlen`, byte
  index, core PCRE). No `mb_*`. ✅
- **PCRE availability under `-n`:** core PCRE is compiled-in (the email `preg_match` runs). ✅

The ONLY determinism leak is R1's `$`-anchor, which is a Rust-vs-PHP **semantic** divergence, not an
impurity — fixable by `\z`.

## R6 — Example safety (passes)

`examples/guide/validate.phg` (spike §4) uses inputs `42,-5,05,4.2,192.168.1.1,256.0.0.1,a@b.com,
a@b,100` — none contain a trailing newline, all in the measured-agree set. The shipped example would
pass the oracle **even with the buggy `$` helper**, which is precisely how the bug would ship
undetected. **Mandate:** the example (or, better, a differential unit test) MUST include a
trailing-`\n` email input (`"a@b.com\n" => false`) so the `\z` fix is regression-gated. Without that
case the byte-identity harness gives false assurance.

---

## Disposition / required amendments before adopt

1. **MANDATORY:** email PHP helper uses `\z` (not `$`); Rust email scanner checks strict end-of-input.
   Add `"a@b.com\n" => false` to the differential test.
2. **RECOMMENDED:** define `isInt` = `__phorj_parse_int($s) !== null`; do not ship the §5 literal
   `__phorj_validate_int` pseudocode.
3. **ADVISORY (out of scope, file separately):** the shipped `__phorj_parse_float` has the same
   `$`-anchor divergence on `"1.5\n"` — `\z`-harden it or KNOWN_ISSUES it.

## Revised numbers

- **Tier: A** holds (no impurity found; the leak is a fixable semantic divergence, not a
  non-deterministic source).
- **determinism_holds: FALSE as written** (the `$`-anchored email helper breaks the spine on
  trailing-newline input).
- **Revised feasibility: 82%** (down from 88%): the buildable subset is still rock-solid, but the
  spike shipped a concretely-wrong PHP helper and a blind example set; the `\z` fix is easy but the
  miss lowers confidence in the "verified identical" methodology. With amendment #1 applied,
  feasibility returns to ~90%.

Confidence in this refutation: **HIGH** (R1 directly reproduced on the declared 8.5.7 floor; R2/R3/R4
cross-checked; the divergence is one byte and deterministic).
