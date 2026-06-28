# Feasibility Spike — Core.Hash (crc32 / md5 / sha1 / sha256)

**Stage 2 feasibility spike.** Verdict: **Tier A, pure, ADOPT-NOW.** Feasibility ~95%.
All four functions are deterministic, std-only-implementable in Rust, and have an
**exactly byte-identical PHP transpile target that survives `php -n`** — including
sha256, despite the prior-art warning that "sha256 has NO core PHP function."

---

## 1. Determinism partition — Tier A (pure), no caveats

A content hash is a pure function of its input bytes. No clock, no entropy, no
filesystem, no locale, no map iteration. Output is either an integer (crc32) or a
fixed-length lowercase-hex digest string (md5/sha1/sha256). These belong on the
byte-identity spine exactly like `Core.Bytes`/`Core.Math` — `pure: true`,
gated by `tests/differential.rs`.

**Crypto-boundary policy check (passes):** the prior art flags that
HMAC / password-hashing / secure-RNG / TLS must NOT be hand-rolled. crc32/md5/sha1/sha256
are **content digests**, not secrets primitives — hashing for content-addressing,
checksums, ETags, cache keys. The policy explicitly permits "hand-rolling sha256/crc32
for content-addressing." HMAC is a deliberate **non-goal of this slice** (defer to a
later slice that transpiles to PHP `hash_hmac` rather than hand-rolling a MAC). md5/sha1
are cryptographically broken but remain legitimate non-secret checksums (Git uses sha1
for object IDs) — ship them, do not advertise them as secure.

---

## 2. std-only feasibility (Rust) — VERIFIED feasible, ~200 LOC total

All four algorithms are pure integer/bit arithmetic over `&[u8]`. Rust std provides
everything: `u32`/`u64` `wrapping_*` ops, `rotate_left`, `to_be_bytes`, slicing. **Zero
crates.** This matches the project's existing hand-rolled-primitive precedent
(`__phorj_float` Ryū, the ELF/PE readers in `bundle/`).

- **crc32 (IEEE 802.3, reflected, poly 0xEDB88320):** ~25 LOC. Build the 256-entry table
  at runtime with a `OnceLock<[u32;256]>` (or `const fn`), then a byte loop:
  `crc = (crc >> 8) ^ table[(crc ^ b) & 0xff]`, init/final XOR `0xFFFFFFFF`. This is the
  exact variant PHP's `crc32()` uses (zlib IEEE). Verified expected outputs below.
- **md5 (RFC 1321):** ~90 LOC. Four 16-step rounds, the `K`/`S` constant tables,
  little-endian length append, `wrapping_add` + `rotate_left`. Output 16 bytes → 32 hex.
- **sha1 (RFC 3174):** ~70 LOC. 80-round compression, big-endian length, `rotate_left`.
  Output 20 bytes → 40 hex.
- **sha256 (FIPS 180-4):** ~90 LOC. 64-round compression with the 64-entry `K` table and
  the eight `H` init constants, big-endian. Output 32 bytes → 64 hex.

Total ~275 LOC including the shared hex-encode helper and padding. Each is a textbook,
test-vector-anchored implementation — low risk of a subtle divergence because the
algorithms have universally-published reference vectors and the PHP oracle cross-checks
every call at test time.

**Hex output (shared helper):** Rust `format!("{:02x}", byte)` per byte, or a small
nibble-LUT loop, produces lowercase hex identical to PHP `bin2hex`/`md5`/`sha1`/`hash`.
Pin **lowercase** explicitly (PHP `dechex`/`%X` would be upper — not used here; the
digest functions are already lowercase).

---

## 3. PHP transpile target — VERIFIED byte-identical under `php -n`

This is the spike's most important finding. The prior art warned sha256 might not
survive `php -n` (ext-hash). **It does — verified on the project's floor PHP 8.5.7.**

```
$ php -n -m | grep -i hash       → hash        (compiled-in, NOT ini-loaded)
$ php -n -r 'echo hash("sha256","hello");'  → 2cf24dba...9824   (works)
$ php -n -r 'echo md5("hello");'            → 5d41402a...c592
$ php -n -r 'echo sha1("hello");'           → aaf4c61d...434d
$ php -n -r 'echo crc32("hello");'          → 907060870
```

`crc32`, `md5`, `sha1` are PHP **core** functions (no extension). `hash()` is provided
by ext-hash, which is **compiled-in by default** (appeared in `php -n -m`), so it needs
**no `-d` flag** — unlike BCMath, which the differential harness must explicitly load.
Hash is therefore a clean fit for the existing oracle with zero harness changes.

Exact `php` mapping closures (the `NativeFn.php` field, `parg(a,0)` = first arg expr):

| Native | Rust output type | PHP transpile target |
|---|---|---|
| `Hash.crc32(bytes) -> int` | `Value::Int` | `crc32({0})` |
| `Hash.md5(bytes) -> string` | `Value::Str` (32 hex) | `md5({0})` |
| `Hash.sha1(bytes) -> string` | `Value::Str` (40 hex) | `sha1({0})` |
| `Hash.sha256(bytes) -> string` | `Value::Str` (64 hex) | `hash('sha256', {0})` |

Because Phorj `bytes` erases to a PHP `string` (PHP strings ARE byte arrays — see
`Core.Bytes` identity mappings), the arg passes through directly. No runtime helper
needed; static-typed mappings suffice.

### crc32 sign trap — RESOLVED (no trap on 64-bit)

crc32 yields a full unsigned 32-bit value. On a 32-bit PHP build, `crc32()` returns a
*negative* int for high-bit results; on **64-bit** PHP (the only build the floor/CI use —
`PHP_INT_SIZE == 8` verified) it returns the **positive** full value
(`crc32("\xff\xff") == 4294901760`, fits in i64). The Rust side must therefore return the
**unsigned** value widened to i64: `Value::Int(crc as u32 as i64)` — never `crc as i32 as
i64`. Verified vectors: `crc32("")==0`, `crc32("a")==3904355907`,
`crc32("The quick brown fox jumps over the lazy dog")==1095738169`. All positive, all
representable in i64. (If a 32-bit PHP target ever mattered it would diverge, but the
project is 64-bit-only by its own CI contract.)

---

## 4. Phorj API sketch

```phorj
import Core.Hash;
import Core.Bytes;

package Main;

function main() -> void {
    var data = Bytes.fromString("hello");

    var sum  = Hash.crc32(data);    // int    → 907060870
    var m    = Hash.md5(data);      // string → "5d41402abc4b2a76b9719d911017c592"
    var s1   = Hash.sha1(data);     // string → "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
    var s256 = Hash.sha256(data);   // string → "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"

    Console.println(sum);
    Console.println(m);
    Console.println(s1);
    Console.println(s256);
}
```

Input is `bytes` (consistent with `Core.Bytes`; a `string` is converted via
`Bytes.fromString`). Digests return lowercase-hex `string`; crc32 returns `int`. This
matches the starting hypothesis exactly. The `Encoding`-for-hex dependency in the
hypothesis is **not required** — the hex encoding is internal to each digest native
(PHP's md5/sha1/hash already return hex), so Core.Hash ships independently of a future
Core.Encoding module. (Core.Encoding/hex does NOT yet exist — verified by grep; Core.Hash
does not block on it.)

`examples/guide/hashing.phg` (+ README entry) ships in the same change, byte-identity-gated
by the `examples/**/*.phg` glob. Use exactly-representable inputs — strings, not floats —
so no float-formatting path is touched.

---

## 5. New VM Op? — NO

Every function is a single-value-in / single-value-out native → `Op::CallNative(idx,
argc)`, the existing path. No new `Op`, no `Value` change (Int/Str already exist), no
three-match coupling. Purely additive in a new `src/native/hash.rs` plus one registration
line in `src/native/mod.rs` (the `eval` is `NativeEval::Pure`). This mirrors how
`Core.Bytes`/`Core.Math` landed.

---

## 6. Named determinism risks

1. **crc32 signedness (PRIMARY):** must return `crc as u32 as i64`, not sign-extended.
   Mitigated by the 64-bit-only contract + a high-bit test vector (`"\xff\xff"` → 4294901760)
   in the differential example. RESOLVED with evidence above.
2. **Hex casing:** pin lowercase. PHP digest fns are already lowercase; the Rust helper
   uses `{:02x}`. A `{:02X}` typo would silently diverge — add a digest vector to the test.
3. **ext-hash presence:** `hash('sha256')` relies on compiled-in ext-hash. Verified
   present under `php -n` on the floor build. If a future minimal PHP target dropped it,
   sha256 would break while md5/sha1/crc32 survive — document that sha256 needs ext-hash
   (the harness probe could assert it, like the bcmath probe). LOW risk (compiled-in
   default on every mainstream build).
4. **Empty-input correctness:** `md5("")`, `sha1("")`, `sha256("")`, `crc32("")==0` are
   classic off-by-one padding traps in hand-rolled digests. Anchor each with the
   published empty-string vector in `src/native/hash_tests.rs` AND the differential.
5. **Endianness:** md5 is little-endian length/words; sha1/sha256 are big-endian. A
   crossed `to_be_bytes`/`to_le_bytes` is the most likely implementation bug. Test vectors
   catch it immediately.
6. **No float / map / locale paths** — none of the known KNOWN_ISSUE traps (Ryū floats,
   map ordering, locale, mbstring) are reachable. mbstring absence is irrelevant: hashing
   is byte-level by definition and the PHP targets are all byte functions.

---

## 7. Effort & recommendation

- **Effort: small.** ~275 LOC of textbook algorithms + 4 registry entries + a guide
  example + a `hash_tests.rs` unit file with published vectors. No backend plumbing, no
  new Op, no harness change. Comparable to the `Core.Bytes` slice.
- **Recommendation: adopt-now.** Highest-confidence Tier-A candidate of the stdlib sweep:
  pure, std-only, exact PHP parity verified on the floor build, no new Op, immediately
  useful (content-addressing, ETags, cache keys, Git-style object IDs).
- **Confidence: high.** Every load-bearing claim verified by running the floor PHP 8.5.7
  under `-n` and reading the actual native-registration code.

**Deferred (out of this slice):** HMAC (transpile to `hash_hmac`, never hand-rolled),
sha512/sha224/other digests (trivial additions once the core four ship),
streaming/incremental hashing (a stateful `Hasher` object — needs a `Value` or a handle,
larger), Core.Encoding hex/base64 as a standalone module (independent slice).

---

## 8. Verification log (commands actually run)

```
php -n -m | grep -i hash                                  → hash
php -n -r 'echo hash("sha256","hello");'                  → 2cf24dba...9824
php -n -r 'echo md5("hello");'                            → 5d41402a...c592
php -n -r 'echo sha1("hello");'                           → aaf4c61d...434d
php -n -r 'echo crc32("hello");'                          → 907060870
php -n -r 'echo crc32(""), crc32("a");'                   → 0 / 3904355907
php -n -r '$x=crc32("\xff\xff"); echo $x; echo PHP_INT_SIZE;' → 4294901760 / 8
```
Floor binary: `/stack/tools/phpbrew/php/php-8.5.7/bin/php` (PHP 8.5.7).
Native shape confirmed in `src/native/bytes.rs` + `src/native/mod.rs` (`NativeFn`,
`pure`, `eval: NativeEval::Pure`, `php: fn(&[String]) -> String`).
Differential harness PHP invocation confirmed `-n` + bcmath probe in `tests/differential.rs`.
