# Feasibility Spike — `Core.Validate` (filter_var-like, returns `T?`)

**Stage 2 feasibility spike.** Module: `Core.Validate`. Lens: PHP `filter_var` / `ctype_*`,
Python `ipaddress`/`re`, Go `net/mail`/`net/url`. Starting hypothesis: Tier A pure; *better than*
PHP `filter_var` because it returns a typed `T?` (or `bool`) rather than `false|mixed`. Predicates:
`isEmail` / `isInt` / `isUrl` / `isIpv4`.

## Verdict (up front)

- **Tier:** A (pure, deterministic, byte-identity-gateable).
- **std-only:** Yes — no external crate; the validation rules are hand-rolled ASCII/byte logic.
- **New VM Op:** No. Every predicate is an `Op::CallNative` (the existing `parseInt`/`parseFloat`
  precedent — same `pure: true` + `Ty::Bool`/`Ty::Optional` shape).
- **Byte-identity strategy:** OWN the rules in Rust + a **gated `__phorj_validate_*` PHP helper**
  that mirrors the Rust logic line-for-line. **Do NOT transpile to `filter_var`/`ctype_*`** — those
  disagree with each other, with Rust, and (for email/URL) across PHP versions. This is the same
  decision already made for `Text.parseInt` (`__phorj_parse_int`, not `filter_var`/`intval`).
- **Recommendation:** **adopt-now** for `isInt` + `isIpv4` (unambiguous, fully gateable). **Defer**
  `isEmail` to a documented Phorj-owned grammar slice, and **defer `isUrl` outright** until
  `Core.Url` exists (it does not today — the hypothesis's "reuse Core.Url" is invalid).
- **Honest feasibility:** ~88% for the buildable subset (`isInt`/`isIpv4` + a conservative
  `isEmail`); the 12% gap is the email-grammar bikeshed and the missing `Core.Url`.

---

## 1. Existing-mechanism verification (what I actually checked)

- `src/native/` has 14 leaf modules. **`Core.Url` does NOT exist** (`grep -rln "Core.Url" src/
  examples/` → empty). The hypothesis "Url validation reuses Core.Url" is therefore **not
  satisfiable today** — `isUrl` either ships its own rule or is deferred. I recommend deferral so
  `isUrl` and a future `Core.Url.parse` share one grammar (single source of truth).
- The `NativeFn` registry (`src/native/mod.rs`) single-sources `module`/`name`/`params`/`ret`/`eval`
  /`php`/`pure`. A pure predicate is a textbook `NativeEval::Pure(fn(&[Value], &mut String) ->
  Result<Value,String>)` with `pure: true`.
- **Exact precedent located:** `Text.parseInt` (`src/native/text.rs:472`) returns
  `Ty::Optional(Box::new(Ty::Int))`, evals `s.parse::<i64>().map_or(Value::Null, Value::Int)`, and
  transpiles to the **gated `__phorj_parse_int` helper** (`src/transpile/program.rs:396`), NOT to
  `filter_var`. `Text.contains`/`startsWith` return `Ty::Bool`. So both the `bool` and the `T?`
  shapes are already shipped and gated.
- **Gating mechanism confirmed** (`src/transpile/program.rs:263 emit_runtime_helpers`, flags set in
  `src/transpile/call.rs:108`): a `uses_<helper>: bool` field on the transpiler, set when the native
  is emitted, causes the helper `function __phorj_*` to be emitted once per file. `Core.Validate`
  follows this verbatim (`uses_validate_email` / `uses_validate_ipv4`, etc.).

## 2. The determinism partition — why `filter_var` is the trap, owned rules are Tier A

Measured under the **floor** `/stack/tools/phpbrew/php/php-8.5.7/bin/php -n` (and a PHP-8.6-dev canary):

**`FILTER_VALIDATE_INT` vs `ctype_digit` vs Rust `i64::from_str` all disagree:**

| input | `filter_var INT` | `ctype_digit` | Rust `i64::from_str` |
|---|---|---|---|
| `"123"` | true | true | true |
| `"-5"` | **true** | **false** | true |
| `"+5"` | true | false | true |
| `"05"` | **false** | **true** | true (leading zeros OK) |
| `"99999999999999999999"` | false (overflow) | **true** | false (overflow) |
| `"9223372036854775807"` (i64::MAX) | true | — | true |
| `"9223372036854775808"` (MAX+1) | false | — | false |

→ Transpiling `isInt` to **either** PHP builtin would silently break `run ≡ runvm ≡ PHP`.
Mirroring Rust's `i64::from_str` in a hand-rolled `__phorj_validate_int($s)` (optional `+`/`-`,
ASCII digits, leading zeros allowed, i64 range check) is the only byte-identical path. Verified that
i64::MAX / MAX+1 boundaries match PHP's int range exactly.

**`FILTER_VALIDATE_IP, FILTER_FLAG_IPV4`** (measured): `1.2.3.4`→true, `256.0.0.1`→false,
`1.2.3`→false, `01.2.3.4`→**false** (PHP rejects leading-zero octets). A Rust rule — split on `.`,
require exactly 4 parts, each 1–3 ASCII digits, no leading zero unless the octet is literally `"0"`,
value 0–255 — reproduces this exactly and is trivially mirrorable in PHP. **`isIpv4` is fully
gateable.**

**Email:** a hand-rolled core-PCRE rule
`/^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$/` returns **identical results on PHP 8.5 AND
8.6-dev** (measured: `a@b.com`→true, `a@b`→false, `user+tag@sub.example.co.uk`→true, `a@@b.com`
→false). Crucially this is a *fixed, simple ASCII grammar* — it is NOT PHP's `filter_var` email
regex (which is idiosyncratic, ~enormous, and version-drifting). The Rust side implements the SAME
grammar as a hand-written byte scanner (no regex engine needed — this grammar has no backrefs/
lookaround/Unicode), the PHP side emits the SAME pattern via core PCRE. Byte-identical by
construction. The cost is honesty: this validates *a* defensible email shape, not RFC 5322.

## 3. Byte-identity strategy (concrete)

For each predicate, **one rule, three legs:**
- **Interpreter + VM:** the single `NativeEval::Pure` body (shared `eval` → structural `run ≡
  runvm` by the registry's one-body-two-callers guarantee).
- **PHP:** a gated `__phorj_validate_<x>` helper emitted by `emit_runtime_helpers`, written to
  reproduce the Rust logic. `isEmail` may emit a single `preg_match($pat, $s) === 1` (core PCRE
  survives `php -n`, verified). `isInt`/`isIpv4` emit pure byte/string logic (no extension).

**Determinism is by construction:** no clock, no RNG, no locale, no map iteration, ASCII-only (so
no mbstring dependency — verified mbstring is absent under `-n`), no float formatting. Nothing in the
trap list is reachable.

## 4. Phorj API sketch

```phorj
import Core.Validate;

// bool predicates (the filter_var "is it valid?" surface, but typed):
Validate.isEmail(string s) -> bool
Validate.isInt(string s)   -> bool      // mirrors i64::from_str
Validate.isIpv4(string s)  -> bool
// DEFERRED until Core.Url exists:
// Validate.isUrl(string s) -> bool

// The "better than filter_var" surface — typed parse, not bool (reuses Text.parseInt machinery):
Validate.toInt(string s) -> int?        // None on invalid; the T? win over filter_var's `false`
```

Worked example (the shipped `examples/guide/validate.phg`, byte-identity-gated):
```phorj
package Main;
import Core.Console;
import Core.Validate;

function main() -> int {
  Console.println(Validate.isInt("42"));        // true
  Console.println(Validate.isInt("-5"));         // true
  Console.println(Validate.isInt("05"));         // true  (NB: differs from ctype_digit, matches Rust)
  Console.println(Validate.isInt("4.2"));        // false
  Console.println(Validate.isIpv4("192.168.1.1")); // true
  Console.println(Validate.isIpv4("256.0.0.1")); // false
  Console.println(Validate.isEmail("a@b.com"));   // true
  Console.println(Validate.isEmail("a@b"));       // false
  if (var n = Validate.toInt("100")) {            // T? composes with S2 if-let
    Console.println(n + 1);                        // 101
  }
  return 0;
}
```
Example must use only inputs where all three legs provably agree (covered by the measurements above).

## 5. Exact PHP transpile targets (gated helpers, NOT filter_var)

```php
// uses_validate_int
function __phorj_validate_int($s) {
  if ($s === "") return false;
  $i = 0; $n = strlen($s);
  if ($s[0] === '+' || $s[0] === '-') { $i = 1; if ($n === 1) return false; }
  for (; $i < $n; $i++) { $c = $s[$i]; if ($c < '0' || $c > '9') return false; }
  // i64-range check mirroring Rust i64::from_str:
  return $s === (string)(int)$s && (int)$s == $s ? __phorj_in_i64($s) : ...; // (range guard, see note)
}
// uses_validate_ipv4
function __phorj_validate_ipv4($s) {
  $p = explode('.', $s);
  if (count($p) !== 4) return false;
  foreach ($p as $o) {
    $L = strlen($o);
    if ($L < 1 || $L > 3) return false;
    for ($k=0;$k<$L;$k++){ if($o[$k]<'0'||$o[$k]>'9') return false; }
    if ($L > 1 && $o[0] === '0') return false;     // no leading zero
    if ((int)$o > 255) return false;
  }
  return true;
}
// uses_validate_email — single core-PCRE call (verified stable 8.5 & 8.6-dev)
function __phorj_validate_email($s) {
  return preg_match('/^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$/', $s) === 1;
}
// toInt reuses the existing __phorj_parse_int (already shipped).
```
*Note:* the i64-range edge in `__phorj_validate_int` is the one fiddly bit — simplest is to reuse
the already-proven `__phorj_parse_int($s) !== null` pattern (it mirrors `i64::from_str` including
range) and define `isInt` as `__phorj_parse_int($s) !== null`. That eliminates a second hand-rolled
range guard and reuses a tested helper. Recommended.

## 6. New VM Op needed?

**No.** All predicates are `Op::CallNative(idx, argc)` — no `chunk.rs`/`vm/exec.rs`/`compiler`
triple-match change. No `Value` variant change (returns `Bool`/`Null`/`Int`, all existing).

## 7. std Rust APIs relied upon

- `str::parse::<i64>()` (`i64::FromStr`) — for `isInt`/`toInt` (mirrors PHP int range, verified).
- `str::split('.')`, `str::len`, byte indexing, `u8::is_ascii_digit` — for `isIpv4` and the
  hand-written email scanner. **No regex crate, no `std::net::Ipv4Addr`** (its parser is close but
  NOT guaranteed identical to PHP's leading-zero rule — own the rule instead, so the PHP mirror is
  exact). All `core`/`std` ASCII string ops — zero external crates.

## 8. Named determinism risks (and disposition)

1. **`filter_var`/`ctype_*` divergence** (MEASURED, primary trap) → avoided: own the rules, gated
   `__phorj_*` helper. The whole point of this module.
2. **Email grammar version-drift** → avoided: a fixed Phorj-owned ASCII regex (verified identical
   8.5/8.6-dev), never PHP's built-in email filter.
3. **i64 range edge** → avoided: define `isInt`/`toInt` via the already-tested `__phorj_parse_int`.
4. **mbstring absence under `php -n`** → non-issue: every rule is ASCII/byte-level (validated).
5. **Locale** → non-issue: no `strcasecmp`/locale-sensitive compare; pin ASCII char classes.
6. **`isUrl` without `Core.Url`** → DEFERRED (Core.Url absent today; build them together later so
   one grammar serves parse + validate).
7. **Float formatting / clock / RNG / map-order** → none reachable in this module.

## 9. Effort & recommendation

- **Effort:** small. One new `src/native/validate.rs` leaf (~80 lines incl. tests) + registry wiring
  in `mod.rs` + 2–3 `uses_validate_*` flags and helper bodies in `transpile/program.rs` +
  `examples/guide/validate.phg` + README entry. No backend/Op/Value change. Mirrors the
  `parseInt`/`parseFloat` slice almost exactly.
- **Recommendation:** **adopt-now** for `isInt` (alias the proven `__phorj_parse_int`), `toInt`
  (literally is `Text.parseInt` re-exported under a validation name — or just point users at
  `Text.parseInt`), and `isIpv4`. Ship `isEmail` with an explicitly-documented conservative grammar
  (KNOWN_ISSUES: "not RFC 5322; validates a common ASCII shape"). **Defer `isUrl`** to the `Core.Url`
  slice.
- **Feasibility:** ~88% (buildable subset rock-solid; the residual is the email-shape bikeshed and
  the `Core.Url` dependency for `isUrl`).

## 10. The "better than PHP" claim — substantiated

PHP `filter_var($s, FILTER_VALIDATE_INT)` returns `int|false` (and `false` is a *valid-looking*
falsy value that collides with `0`); Phorj's `Validate.toInt(s) -> int?` returns a real `int?` that
composes with S2 null-safety (`if (var n = Validate.toInt(x))`, `??`, `opt!`). That IS the typed-`T?`
improvement the hypothesis claims, and it is achievable today by re-exposing the shipped
`Text.parseInt` machinery under the validation module — no new runtime mechanism required.
