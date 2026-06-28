# Stage 2b — Adversarial byte-identity review of Core.Hash (crc32/md5/sha1/sha256)

**Verdict: the feasibility claim SURVIVES.** `determinism_holds = true`. tier=A confirmed.
Revised feasibility ~93% (down 2pts from 95% only for the example-typing defect below, which is a
spec bug not a determinism bug). I could not produce a real byte-identity divergence across
run/runvm/real-PHP-8.5 for the four content digests. Every load-bearing claim was independently
re-verified by running the floor binary (`/stack/tools/phpbrew/php/php-8.5.7/bin/php`, PHP 8.5.7)
under `-n` and by reading the actual native/transpile/value source.

## What I tried to break, and why it held

### 1. crc32 signedness / type (the spike's PRIMARY named risk) — HELD
- Independently re-ran every spike vector: `crc32("")==0`, `crc32("a")==3904355907`,
  `crc32("The quick brown fox jumps over the lazy dog")==1095738169` — all reproduce exactly.
- Pushed the high-bit frontier: `crc32("\xff\xff\xff\xff")` → `int(4294967295)`, `gettype` =
  `integer` (NOT float, NOT negative). Swept `chr(255)×1..5`: every result is a positive `integer`
  ≤ 4294967295. `PHP_INT_SIZE == 8` on the floor + both CI legs.
- `Value::Int(crc as u32 as i64)` is therefore correct and lossless: max 4294967295 fits in i64,
  renders as plain decimal. `Value::as_display` for `Int(n)` is `n.to_string()` (src/value.rs:241)
  — byte-identical to PHP `echo`/interpolation of the same int. **No divergence.**
- The sign-extension trap (`crc as i32 as i64`) is real but the implementer is explicitly warned and
  a high-bit differential vector catches it. The 64-bit-only contract is enforced in the harness
  (`PHP_INT_SIZE` is 8 on floor + CI shivammathur/setup-php 8.5 and 8.6).

### 2. ext-hash / sha256 under `php -n` (the spike's named LOW risk) — HELD, and STRONGER than the spike claims
- Re-ran: `php -n -m | grep hash` → present; `php -n -r 'echo hash("sha256","hello")'` works.
- Went deeper than the spike: `ReflectionFunction("hash")->getExtensionName()` = **`hash`**;
  `crc32`/`md5`/`sha1` = **`standard`**. The floor build is configured `--disable-all` yet still
  shows `hash support => enabled` in `php -n -i`. Since PHP 7.4 the `--disable-hash` configure flag
  was REMOVED — ext-hash is a mandatory, non-disableable core extension, not an optional shared `.so`.
  So sha256's reliance on `hash('sha256', …)` is as portable as crc32/md5/sha1; **no `-d` flag and no
  harness probe is needed** (unlike bcmath, which IS an optional shared ext the harness loads via
  `-d extension=bcmath`). The spike's "could break on a minimal build" caveat overstates the risk:
  a build without ext-hash is not a mainstream PHP. CI's `shivammathur/setup-php@v2` ships it by default.

### 3. Bytes literal → PHP string-literal escaping (NOT examined by the spike) — HELD
- This is the gap the spike hand-waved ("the arg passes through directly"). The real path:
  `b"\xff\xff"` lexes to `TokenKind::Bytes(vec![0xff,0xff])` (src/lexer/mod.rs:672), becomes
  `Expr::Bytes(Vec<u8>, Span)` (src/ast/mod.rs:202), transpiles via
  `Expr::Bytes(b,_) => format!("\"{}\"", php_escape_bytes(b))` (src/transpile/expr.rs:185).
- `php_escape_bytes` (src/transpile/mod.rs:766) emits 0x20..=0x7E literally and **every other byte as
  `\xHH`**, escaping `\ " $`. So `b"\xff\xff"` → PHP `"\xff\xff"`. In a PHP double-quoted string,
  `\xff` is exactly one 0xFF byte → 2 bytes, byte-identical to the Rust `Value::Bytes` content.
  The hash then sees the same byte sequence on both sides. **High-bit byte transpile is sound.**
- The example uses `Bytes.fromString("hello")` whose `php` is identity (`"hello"`), so the digest
  call is `md5("hello")` etc. — also fine.

### 4. Output rendering of digest results — HELD
- Digests return `Value::Str` (lowercase hex). `Value::as_display(Str)` = `s.clone()`; PHP
  `md5`/`sha1`/`hash('sha256',…)` already return lowercase hex. Re-verified all empty-string vectors
  (`md5("")==d41d8cd9…`, `sha1("")==da39a3ee…`, `sha256("")==e3b0c442…`) — identical.
- Hex-casing trap (`{:02x}` vs `{:02X}`) is real but front-end-caught by a digest differential vector.

### 5. The non-determinism checklist — all NEGATIVE (correctly)
- No clock, no entropy, no filesystem, no locale, no map/set iteration order, no object ids/addresses,
  no float formatting (Ryū `__phorj_float` is never reached — digests are int/str only). The
  gzip-timestamp class of trap does not apply (no compression). mbstring absence is irrelevant: every
  PHP target (`crc32`/`md5`/`sha1`/`hash`) is a byte-level `standard`/`hash` function, not mbstring.

## The ONE real defect I found (spec/correctness, NOT determinism)

**The spike's API sketch (§4) does not typecheck.** It writes `Console.println(sum)` where
`sum : int` (crc32 result) and `Console.println(m)` where `m : string`. But `Console.println` is
registered with `params: vec![Ty::String]` (src/native/mod.rs:253) — it accepts ONLY a string.
Every shipped example prints non-strings via interpolation (`"sum={sum}"`) or `Convert.toString`
(verified: examples/guide/maps.phg interpolates ints; examples/guide/bytes.phg wraps with `?? "?"`).
So the guide example as written would fail the checker on `Console.println(sum)` (the int) — it must
either interpolate (`Console.println("crc={sum}")`) or convert. **This is a real bug in the spike's
example, but it does NOT threaten byte-identity** — interpolating an int is itself byte-identical
(`Value::as_display(Int)` == PHP int interpolation, verified). Cost: the example must be rewritten;
hence I shave feasibility 95%→~93% (implementation effort, not risk). The digest *strings* print
fine via `println` directly.

## New-Op / Value / plumbing claims — CONFIRMED
- No new `Op`: each digest is a single-value `Op::CallNative(idx, argc)` — confirmed the path exists
  and `NativeFn.php: fn(&[String]) -> String` (src/native/mod.rs:54) + `NativeEval::Pure` is the exact
  shape `Core.Bytes` uses (src/native/bytes.rs). No `chunk.rs`/`vm/exec.rs`/`compiler` three-match
  coupling. No `Value` change (Int/Str exist).
- The PHP mappings are pure static-typed closures: `crc32({0})`, `md5({0})`, `sha1({0})`,
  `hash('sha256', {0})` — no gated runtime helper needed (digests are byte functions; the bytes arg
  is already a PHP string).

## Residual risks (all LOW, all front-end-catchable)
- R1 endianness crossover in the hand-rolled md5(LE)/sha1+sha256(BE) — the most likely *impl* bug;
  caught instantly by the published empty + known-answer vectors in both `hash_tests.rs` and the
  differential. Not a backend-divergence risk (run==runvm share the same Rust kernel; PHP is the oracle).
- R2 hex-casing `{:02X}` typo — caught by a digest differential vector.
- R3 example must be re-typed to print the int via interpolation/Convert (the defect above).
- R4 (theoretical) a non-mainstream PHP built without ext-hash would lose sha256 — not a real target;
  CI guarantees it. Optionally add an `extension_loaded('hash')` probe mirroring the bcmath one for
  defense-in-depth, but it is not required (ext-hash is non-disableable since 7.4).

## Bottom line
The determinism partition is correct: Core.Hash is Tier A, pure, byte-identity-gateable. I found no
genuine cross-backend divergence. The only correction is a typecheck error in the spike's example
(println of an int), which is cosmetic to feasibility and itself byte-identical once fixed.
Confidence: HIGH (every claim re-run on the floor PHP 8.5.7 + source-verified).
