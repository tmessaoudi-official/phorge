# Adversarial Refutation — Core.Encoding (base64 / hex / urlencode)

**Verdict: determinism_holds = FALSE for the spike *as written*.** The Tier-A classification is
correct (no clock/RNG/map-order/float/locale/address non-determinism is reachable — that part of the
spike is sound). But the byte-identity claim rests on a **factually wrong characterization of PHP's
decode semantics**, and a hand-rolled Rust decoder built to the spike's spec will diverge from real
PHP 8.5 on multiple ordinary inputs. The risk is not non-determinism — it is **Rust↔PHP semantic
divergence inside the differential spine**. Feasibility is still high *if the implementation is pinned
to PHP's actual (counter-intuitive) decode behavior*, which the spike does NOT specify and in places
actively contradicts.

All evidence below is **[Verified]** against `php-8.5.7 -n` (the oracle floor), live.

---

## REFUTATION 1 — "STRICT base64 decode" is the central error (HIGH severity)

The spike (§4, §6.3, abstract) repeatedly claims `base64_decode($s, true)` is "STRICT" and that a
hand-rolled Rust "strict decoder" matches it. **This is false.** PHP's so-called strict mode is
*whitespace-skipping and non-canonical-bit-tolerant*. Verified on `php-8.5.7 -n`:

| Input | `base64_decode($s, true)` | A naive hand-rolled "strict" Rust decoder would |
|---|---|---|
| `"aGk"` (no padding) | `"hi"` | likely **reject** (spike §6.2 says re-pad to %4 only for url-safe, not std) |
| `"aG k="` (embedded space) | `"hi"` | **reject** (space not in alphabet) |
| `"aG\nk="` (embedded newline) | `"hi"` | **reject** |
| `"aGVs\tbG8="` (tab) | `"hello"` | **reject** |
| `"aGVs\rbG8="` (CR) | `"hello"` | **reject** |
| `"a G V s b G 8 ="` (spaces throughout) | `"hello"` | **reject** |
| `"YWJ="` (non-canonical: 3rd char's low bits set) | `"ab"` (silently masks low bits) | **reject** (a bit-checking strict decoder errors on nonzero trailing bits) |
| `"YR=="` (non-canonical 2-char group) | `"a"` (masks low bits of `R`) | **reject** |
| `"aGk=="` (over-padded) | `false` | reject ✓ (agrees) |
| `"aGVsbG8=\f"` (form-feed) | `false` | reject ✓ (agrees — but only because `\f`/vtab are NOT in the skip set, while space/`\t`/`\r`/`\n` ARE) |

The skip-set is **exactly** `{0x20 space, 0x09 \t, 0x0A \n, 0x0D \r}` and nothing else (`\f`=0x0C and
vtab=0x0B are rejected — verified). To stay byte-identical, the Rust `base64Decode` eval must:
1. **Strip exactly that four-char whitespace set** before decoding (not "any whitespace", not "none").
2. **Tolerate missing padding** (treat `"aGk"` ≡ `"aGk="`) — but **reject wrong-count padding**
   (`"YQ="` → false, `"aGk=="` → false).
3. **Mask, not validate, the trailing bits** of the final group (`"YWJ="` → `"ab"`), matching PHP's
   non-canonical tolerance — a bit-strict RFC4648 decoder is WRONG against this oracle.

The spike's §6.3 ("strict … the lenient default silently drops invalid chars and would diverge from a
strict Rust decoder. Always pass `true`") has the divergence **backwards**: it is the *Rust* side that
will be too strict, not too lenient. This single misunderstanding, shipped as written, produces a
`run`/`runvm` (hand-rolled) vs real-PHP-8.5 mismatch on the very first example program that decodes
whitespace-wrapped or unpadded base64 — and MIME/PEM base64 is line-wrapped, so this is not exotic.

**This is the refutation that flips determinism_holds to false:** the claim "produce byte-for-byte what
the PHP function produces" is not achievable by the decoder the spike describes; it is achievable only
by a decoder reverse-engineered to PHP's quirks, which the spike does not provide and partly contradicts.

## REFUTATION 2 — base64_encode never line-wraps, but the spike doesn't pin it (LOW, latent)

Verified: `base64_encode(str_repeat("x",100))` emits **one unbroken line** (no `\r\n` every 76 chars,
unlike MIME `chunk_split`). The spike never states this, and a hand-rolled encoder modeled on a
"textbook" base64 (some emit 76-char-wrapped output) would diverge. Must pin: **no line wrapping**.
Not fatal because the natural Rust impl also doesn't wrap, but it is an unstated degree of freedom.

## REFUTATION 3 — hex2bin rejects ALL whitespace (asymmetry with base64) (MEDIUM)

Verified: `@hex2bin("68 69")` → `false`, `@hex2bin("6  869")` → `false`. Unlike base64, hex2bin does
**not** skip whitespace. So the two decoders have *opposite* whitespace policies — the Rust `hexDecode`
must reject whitespace while `base64Decode` must skip a specific subset. The spike treats both as
generic "strict" and never flags this asymmetry; implementing them with shared "skip-whitespace" or
shared "reject-whitespace" logic breaks one leg. (`hex2bin` does accept both cases `ABCD`/`abcd` →
spike §6.1 is correct on that point.)

## REFUTATION 4 — urlDecode UTF-8 fork is real but UNDER-stated; rawurldecode is byte-total (MEDIUM)

Verified: `rawurldecode("%FF%FE")` → bytes `0xFF 0xFE` (invalid UTF-8), `rawurldecode("a%2")` → `"a%2"`
(stray percent left literal), `rawurldecode("a%ZZ")` → `"a%ZZ"`. So `rawurldecode` is a **total
bytes→bytes** function in PHP, but Phorj `Value::Str` MUST be valid UTF-8. The spike's §6.6 picks
"type `string?`, null on non-UTF-8" — that IS a sound resolution, but note the consequence the spike
omits: **the Rust eval must run a `std::str::from_utf8` validity check on the decoded bytes and return
`Value::Null` whenever PHP would have produced a non-UTF-8 string** — and the PHP emission therefore
can NOT be a bare `rawurldecode({0})` (which yields a PHP string of raw bytes, byte-identical on stdout
but representing a value Phorj would have nulled). The transpile target listed in §4
(`rawurldecode({0})` "total; see §6 note") is **wrong for the chosen `string?` typing**: PHP must wrap
it in a UTF-8 validity check (PCRE `//u` or `mb_check_encoding`-free equivalent) to mirror the null,
or the `string?` value diverges between the Rust backends (null) and PHP (a raw-byte string). Under
`php -n` mbstring is absent (the spike itself notes this for Bytes), so the only available UTF-8 check
is **PCRE `preg_match('//u', $s)===1 ? $s : null`** — which the spike does not specify. Without it,
`run`/`runvm` produce `null` and PHP produces a (printed) garbage string → **byte divergence the moment
the example prints the decoded value or its null-ness**.

## REFUTATION 5 — `@hex2bin` warning suppression and odd-length (LOW, agrees)

Verified `@hex2bin("6G")` → false, `@hex2bin("")` → `""`, odd-length → false. The `@` suppresses the
E_WARNING to stderr; stdout is unaffected, so byte-identity on stdout holds. Spike §6.4 is correct
here. (Minor: relying on `@` is fragile if PHP ever promotes the warning to an exception in a future
edition, but at the 8.5 floor it is fine — not a refutation, noted for completeness.)

## What the spike got RIGHT (steel-man)

- Tier A classification: **correct.** No clock, RNG, map iteration, float formatting, locale, or
  address/object-id leaks are reachable. The eight named non-determinism traps genuinely don't apply.
- `rawurlencode` reserved set: **verified correct** — `~-._` and `A-Za-z0-9` pass through; everything
  else → uppercase `%XX` (`rawurlencode(" !*'()")` → `%20%21%2A%27%28%29`). Encode direction is the
  easy, safe half.
- `bin2hex` lowercase pin (§6.1): correct.
- No new `Op`, `Op::CallNative`, `pure:true`, Bytes-clone plumbing: structurally sound.
- url-safe base64 `strtr('+/','-_')` + strip `=`: encode direction verified
  (`base64_encode("\xff\xfe")` = `//4=` → `__4`).

## Net assessment

The module IS byte-identity-feasible — but ONLY if the decode evals are written to **emulate PHP's
actual decoder quirks**, not RFC strictness:
- base64Decode: strip `{space,\t,\n,\r}`, tolerate missing padding, mask (don't validate) trailing
  non-canonical bits, reject wrong padding count and `\f`/vtab.
- hexDecode: reject ALL whitespace, accept mixed case, reject odd-length/non-hex.
- urlDecode: decode bytes, then `std::str::from_utf8` → null on failure, AND emit a PCRE `//u`-guarded
  PHP expression (NOT bare `rawurldecode`) so the `string?` null matches.

None of these three corrected behaviors appear in the spike; two are actively contradicted (§6.3
"strict", §4 bare `rawurldecode`). Therefore the feasibility claim **as written** does not hold its
byte-identity guarantee. Revised feasibility once these are pinned: ~88% (the residual is the
non-canonical-bit emulation, which needs a dedicated differential pin to lock down per PHP's exact
masking, plus the PCRE `//u` round-trip under `php -n`).

Confidence in this refutation: **HIGH** — every divergence above is reproduced live against
`php-8.5.7 -n`.
