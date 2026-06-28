# Feasibility Spike — Core.Encoding (base64, hex, urlencode/decode)

**Stage 2 feasibility spike.** Verdict up front: **adopt-now, Tier A, feasibility ~96%, confidence high.**
This is the single most clearly-feasible module in the whole stdlib sweep — it is structurally a clone
of the already-shipped `Core.Bytes` module, with the same `bytes`/`bytes?` value plumbing already in
place and four PHP-core transpile targets verified to survive `php -n`.

## 1. Determinism partition — Tier A (pure), by construction

Every operation in this module is a **pure, total function of its argument bytes** with a fixed,
algorithm-defined output:

- base64 encode/decode — RFC 4648 alphabet, fixed `=` padding, no clock/RNG/locale/iteration-order.
- hex encode/decode — fixed lowercase alphabet (pinned), purely positional.
- urlencode/urldecode (both percent-encoding `rawurlencode` and form `+`-encoding `urlencode`) —
  fixed RFC 3986 unreserved set, fixed uppercase hex in `%XX`.

None of the eight named determinism traps apply except two that are trivially pinned (hex case, base64
alphabet/padding) — covered in §6. **No clock, no RNG, no map iteration, no locale, no float.** This is
the canonical Tier A module and is byte-identity-gateable in `tests/differential.rs` with zero quarantine.

## 2. std-only feasibility — YES, but actually we don't even hand-roll

The Rust `eval` bodies are pure std (`Vec<u8>` slicing, byte arithmetic, a 64-entry base64 table and a
16-entry hex table — all `const`). Rust std has **no** `base64`/`hex` in std (those are external crates,
which are forbidden), so we **hand-roll the codecs in ~40 lines each**. This is well within the std-only
constraint and matches the prior-art guidance (base64/hex are explicitly listed as "hand-rolled" Tier A,
unlike crypto/TLS which must NOT be hand-rolled). Hand-rolling base64/hex/url-percent is *not* a crypto
boundary — these are reversible encodings, not secrets — so the "don't hand-roll crypto" rule does not
bite here.

std APIs relied on: `Vec<u8>`, slice `chunks(3)`/`windows`, `u8` bit ops, `char::from_digit`/manual
nibble tables, `std::rc::Rc` (to wrap `Value::Bytes` exactly as `Core.Bytes` does). No `unsafe`, no I/O.

## 3. Existing mechanisms reused — this is a `Core.Bytes` clone

Verified by reading `src/native/bytes.rs` and `src/native/mod.rs`:

- **Value plumbing already exists.** `Value::Bytes(Rc<Vec<u8>>)`, `Ty::Bytes`, `Ty::String`,
  `Ty::Optional(Box<Ty>)` are all present. `Core.Bytes.toString` already demonstrates the exact
  `bytes -> string?` decode-failure idiom (`Ok(...) | Value::Null`) — our `*Decode -> bytes?` and
  `urlDecode -> string?` reuse it verbatim.
- **No new VM Op.** Every native dispatches through the existing `Op::CallNative(index, argc)`. No
  `chunk.rs`/`vm/exec.rs`/`compiler.rs` triple-match change. (This is the strongly-preferred path.)
- **`NativeEval::Pure(fn(&[Value], &mut String) -> Result<Value,String>)`** — every entry is `Pure`;
  none touch the output buffer, none are `HigherOrder`/`Reflective`. `pure: true` on all entries → fully
  inside the byte-identity differential, no `tests/process.rs`-style quarantine.
- **One new file `src/native/encoding.rs`** with an `encoding_natives() -> Vec<NativeFn>` builder, plus
  `mod encoding;` + a call to `encoding_natives()` in `mod.rs build()`. No god-file (matches the per-leaf
  module discipline). One new `src/native/encoding_tests.rs` mirroring `bytes`'s unit tests.
- **`parg(a, i)`** helper already exists for PHP arg emission.

## 4. Exact PHP transpile targets — all verified core under `php -n`

Verified live against `php-8.5.7 -n` (and the `php -n` posture matters — these are all PHP *core*, none
are mbstring/ext-hash/intl, so they survive the oracle's `-n`):

| Phorj native | Rust eval | PHP `php` emission (the transpile target) |
|---|---|---|
| `Encoding.base64Encode(bytes) -> string` | hand-rolled std RFC4648 | `base64_encode({0})` |
| `Encoding.base64Decode(string) -> bytes?` | hand-rolled strict | `(($__b=base64_decode({0}, true))===false?null:$__b)` |
| `Encoding.base64UrlEncode(bytes) -> string` | std + `+/`→`-_`, strip `=` | `rtrim(strtr(base64_encode({0}),'+/','-_'),'=')` |
| `Encoding.base64UrlDecode(string) -> bytes?` | std + `-_`→`+/`, re-pad | IIFE: pad to %4, `strtr`, `base64_decode(...,true)`→null-on-false |
| `Encoding.hexEncode(bytes) -> string` | lowercase nibble table | `bin2hex({0})` |
| `Encoding.hexDecode(string) -> bytes?` | strict, even-len + 0-9a-fA-F | `(($__h=@hex2bin({0}))===false?null:$__h)` |
| `Encoding.urlEncode(string) -> string` | percent-encode RFC3986 | `rawurlencode({0})` |
| `Encoding.urlDecode(string) -> string?` | percent-decode | `rawurldecode({0})` (total; see §6 note) |
| `Encoding.formEncode(string) -> string` | space→`+` form variant | `urlencode({0})` |
| `Encoding.formDecode(string) -> string` | `+`→space form variant | `urldecode({0})` |

Verified outputs (`php -n`): `base64_encode("hi")="aGk="`, `base64_decode("aGk=",true)="hi"`,
`base64_decode("!!!!",true)=false`, `bin2hex("hi")="6869"`, `hex2bin("6869")="hi"`,
`@hex2bin("xyz")=false`, `@hex2bin("abc")=false` (odd len), `rawurlencode("a b/c")="a%20b%2Fc"`,
`rawurlencode("~-._")="~-._"` (unreserved untouched), `rawurlencode("*")="%2A"`, `urlencode("a b")="a+b"`,
`urldecode("a+b")="a b"`, `strtr` exists. The url-unsafe `base64_encode("\xff\xfe")="//4="` confirms the
`+`/`/` chars the URL-safe variant must remap.

**Byte-identity strategy:** the Rust `eval` must produce byte-for-byte what the PHP function produces.
Pin every degree of freedom identically on both legs: (a) base64 standard alphabet
`A-Za-z0-9+/` with `=` padding; (b) hex **lowercase** `0-9a-f` (matches `bin2hex`; encode must emit
lowercase, decode must accept both cases like `hex2bin`); (c) url percent-encoding to **uppercase** `%XX`
over exactly the `rawurlencode` reserved set (RFC 3986 unreserved `A-Za-z0-9-_.~` pass through — verified
`~-._` unchanged; everything else `%XX`); (d) strict base64/hex decode → `false`/`null` on any invalid
input. The unit test in `encoding_tests.rs` pins each Rust/PHP pair (the `Core.Bytes` test precedent), and
the `examples/guide/encoding.phg` differential round-trips all three legs.

## 5. New VM Op needed — NO

All ten natives ride `Op::CallNative`. No `Op` variant, no `Value` variant, no checker `Ty` addition
(reuses `Ty::Bytes`/`Ty::String`/`Ty::Optional`). Pure front-of-registry addition. This is why the effort
is *small*.

## 6. Named determinism risks (all mitigated)

1. **Hex case (lowercase vs uppercase).** `bin2hex` emits lowercase; `dechex`/`%X` emit upper. **Pin
   lowercase** on encode; accept both on decode (mirrors `hex2bin`). Risk: a Rust impl that emits
   uppercase silently diverges from `bin2hex`. *Caught by the unit pin.*
2. **base64 alphabet + padding.** Standard `+/=` vs URL-safe `-_` (no pad). Ship **two explicit
   variants** (`base64*` and `base64Url*`); never let one bleed into the other. URL-safe must `strtr`
   `+/`→`-_` AND strip `=` on encode, reverse + re-pad on decode (PHP `base64_decode` tolerates missing
   padding, but to be safe the IIFE re-pads to a multiple of 4 before decoding — pinned identically in
   Rust).
3. **Strict vs lenient decode.** `base64_decode($s, true)` (strict) is mandatory — the lenient default
   silently drops invalid chars and would diverge from a strict Rust decoder. **Always pass `true`.**
   Invalid → `null` (the `bytes?` absent case), never a fault.
4. **`hex2bin` odd-length / invalid → false + E_WARNING.** Suppress the warning with `@` (the warning
   goes to stderr and does not affect stdout byte-identity, but `@` keeps the oracle output clean) and
   map `false → null`. Rust decoder must reject odd-length and non-`[0-9a-fA-F]` identically.
5. **mbstring absence under `php -n`.** Non-risk by design: base64/hex/rawurlencode/urlencode are all PCRE-
   free CORE byte functions — no `mb_*`. (`Core.Bytes.toString` already had to dodge `mb_check_encoding`
   via PCRE `//u`; *this* module needs no such dance because none of its ops are encoding-aware.)
6. **`urlDecode` totality.** `rawurldecode` never fails (a stray `%` is left literal); it is total →
   `string`, NOT `string?`. But a `%XX` whose bytes are invalid UTF-8 would, if we typed it `string`,
   risk producing a non-UTF-8 `Value::Str` (Phorj `Str` is UTF-8). **Decision needed (low-risk):** either
   (a) type `urlDecode -> string?` and return null on non-UTF-8 result (mirrors `Bytes.toString`), or
   (b) define a `urlDecodeBytes(string) -> bytes` total form and keep `urlDecode -> string?`. Recommend
   shipping `urlDecode -> string?` (null on non-UTF-8) — consistent with the existing `bytes?`/`string?`
   discipline. This is the only genuine design fork; it does not threaten feasibility.
7. **No float, no clock, no RNG, no map ordering, no gzip mtime** — none of the high-severity traps in the
   prior-art digest are reachable from this module. (gzip/compression is a *different, deferred* module.)

## 7. API sketch (Phorj)

```phorj
import Core.Encoding;
import Core.Bytes;

main() {
    bytes data = Bytes.fromString("Hello, Phorj!");

    string b64  = Encoding.base64Encode(data);          // "SGVsbG8sIFBob3JnZSE="
    bytes? back = Encoding.base64Decode(b64);           // bytes? — null on invalid
    string url  = Encoding.base64UrlEncode(data);       // no '+', '/', no padding

    string hex  = Encoding.hexEncode(data);             // lowercase
    bytes? raw  = Encoding.hexDecode("48656c6c6f");      // null on odd/invalid

    string enc  = Encoding.urlEncode("a b/c?d=e");        // "a%20b%2Fc%3Fd%3De"
    string? dec = Encoding.urlDecode("a%20b%2Fc");        // string? (null on non-UTF8)
    string form = Encoding.formEncode("a b");             // "a+b"
}
```

(`base64Decode`/`hexDecode` return `bytes?` and compose naturally with S2 `??` / if-let, exactly like the
shipped `Core.File.read -> string?`.)

## 8. Effort & sequencing

- **Effort: small.** ~250 LOC total: one `encoding.rs` (10 natives, ~150 LOC incl. the two hand-rolled
  codecs), `encoding_tests.rs` (~80 LOC of Rust/PHP pin tests), 3 lines in `mod.rs`, one
  `examples/guide/encoding.phg` + `examples/README.md` row. Half a day with the gate.
- **Foundational:** unblocks `Core.Hash` (digests want hex output), `Core.Url` (percent-encoding shared),
  and the M6 HTTP/handler work (header/body encoding). Build this *first* in the stdlib-breadth wave.
- **Quality gate:** `cargo test --workspace`, then
  `PHORJ_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORJ_REQUIRE_PHP=1 cargo test --workspace`
  (8.5 floor), clippy + fmt. The new `examples/guide/encoding.phg` is auto byte-identity-gated by the
  `examples/**/*.phg` glob.

## 9. Honest confidence — high (~96%)

The 4% residual: the `urlDecode` UTF-8/`string?` fork (§6.6) is a minor design choice, not a blocker; and
the URL-safe base64 re-padding IIFE needs one careful byte-pin test (the only place a subtle Rust↔PHP
divergence could hide). Everything else is a verified, mechanical clone of a shipped module against
verified PHP-core targets.
