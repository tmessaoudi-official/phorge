# Prior-Art Sweep — Rust std + ecosystem, through the Phorj determinism lens

Stage 1. Lens: **what Rust's standard library gives for free vs. what requires a crate**, and for
each candidate native module — **API shape, purity (Tier A pure/gateable vs Tier B impure/quarantined),
and porting notes** for a Phorj `Core.*` native. Confidence graded per item.

The hard frame: Phorj is **std-only, zero external crates** (`[dependencies]` is empty; the only
exception is wasm-bindgen in the isolated `playground/` workspace member). So the question for every
capability is not "is there a crate?" but **"can std do it, or must it be hand-rolled in safe std
Rust, or is it simply impossible without a crate?"** A Tier-A native must additionally be
**deterministic w.r.t. the program text** to ride the byte-identity differential
(`run == runvm == real-PHP-8.5 under \`php -n\``).

---

## 0. What Rust std gives free vs. needs a crate (the baseline)

| Capability | std support | Verdict for Phorj |
|---|---|---|
| `Debug`/`Display` formatting (`std::fmt`) | YES — `{:?}`/`{:#?}` pretty-printers | Reuse the *idea*, not the output: Phorj must define its OWN deterministic var-dump format (no Rust type names / no addresses). |
| Integer/float formatting, parsing | YES — `format!`, `str::parse`, `f64::to_string` (shortest round-trip / Ryū-equivalent since Rust 1.x) | Already leaned on (`__phorj_float`). |
| Hashing for hashmaps (`std::hash::Hasher`, `DefaultHasher`/SipHash) | YES, but **NOT a cryptographic or stable-across-versions digest** | Cannot be exposed as a content hash (non-portable to PHP, version-unstable). Crypto digests must be hand-rolled. |
| Time — wall clock (`SystemTime::now`), monotonic (`Instant`) | YES | **Impure** (clock). `now()` is Tier B. Date *arithmetic* on a supplied epoch is Tier A. |
| Filesystem (`std::fs`) | YES | **Impure** (already: `Core.File` is `pure:false`-style quarantined; reads a committed fixture in examples). |
| Process / env (`std::env`, `std::process`) | YES | **Impure** (already: `Core.Process`/env quarantined, `pure:false`). |
| TCP/UDP sockets (`std::net`) | YES — raw sockets only | **No TLS, no HTTP** in std. HTTP client = Tier B + needs hand-rolled HTTP/1.1 over `TcpStream` (plaintext only) — see §HTTP. |
| TLS | **NO** — not in std (needs `rustls`/`native-tls`) | HTTPS is impossible std-only. Hard stop; defer to M6 `Transport` or PHP-only. |
| Regex | **NO** — not in std (needs `regex`) | Must hand-roll a small engine OR transpile to PCRE only (the oracle has PCRE core, NOT mbstring). See §Regex. |
| Random | **NO** RNG in std (no `rand`); `RandomState` seeds are process-random, not reproducible | Must hand-roll a **seeded** PRNG for Tier A; OS entropy (`getrandom`) is Tier B and not in std anyway. |
| Base64 / hex | **NO** base64 in std; hex via `format!("{:02x}")` is trivial | Hand-roll base64 (tiny, pure); hex is a few lines. Tier A. |
| URL parsing | **NO** in std (needs `url`) | Hand-roll a std-only parser (RFC 3986 subset). Pure → Tier A. |
| JSON | **NO** in std (needs `serde_json`) | Already shipped hand-rolled (`Core.Json`). Proof that the hand-roll model works. |
| CSV | **NO** in std (needs `csv`) | Hand-roll (small state machine). Pure → Tier A. |
| SQL driver | **NO** in std | Connection = Tier B. A query **builder** (string assembly + escaping) is Tier A. |

**Takeaway:** the determinism partition and the std-only constraint are *orthogonal but co-decisive*.
A capability is shippable as a Core.* native now iff it is **(Tier A pure) AND (std-only-implementable
in safe Rust) AND (faithfully transpilable to PHP 8.5 core under `php -n`)**. Many high-value modules
clear all three; the ones that don't are explicitly named below as deferrals.

---

## 1. var-dump / pretty-print  — Tier A — confidence HIGH

- **API (Phorj):**
  `Core.Debug.dump(value) -> string` (returns the formatted text — keeps it pure, no implicit stdout);
  optionally `Core.Debug.print(value)` thin wrapper that `Console.print`s it.
- **Purity:** PURE *iff the format is Phorj-defined and contains NO addresses / object-ids / hashmap
  iteration order surprises.* Phorj `Value` already gives a closed, ordered model: List/Map/Set are
  insertion-ordered `Rc<Vec<…>>` (Map discipline R1), so iteration order is deterministic. Int/Float/
  Bool/Str/Bytes/Decimal/Null/Enum/Instance all have stable shapes.
- **std reliance:** `std::fmt::Write` into a `String`; recursion with the **existing
  `value::eq_val_rec` visited-set** pattern reused for **circular-reference detection** (Instance is
  shared-mutable post-mutation milestone → cycles ARE possible; print `*RECURSION*` like PHP's
  `var_dump`/`print_r` do — PHP prints `*RECURSION*`).
- **PHP transpile target:** `var_export($v, true)` is the closest *pure* PHP builtin (returns a
  string, no addresses). BUT `var_export` output differs structurally from any Phorj-defined format,
  so the honest target is a **gated runtime helper** `__phorj_dump($v)` that reproduces the
  Phorj format byte-for-byte (recursive walk, same indentation, same `*RECURSION*` sentinel).
  Avoid `var_dump` (prints to stdout + includes types/refcounts) and `print_r` (locale-free but its
  format is fixed and PHP-specific).
- **Determinism traps:** (a) float rendering — MUST use the shortest-round-trip formatter already in
  the codebase (`__phorj_float`), NOT `{:e}` and NOT PHP's default `echo` 14-digit; (b) Set order is
  now insertion-order (good) — never a `HashSet` iteration; (c) NO object ids / spl_object_id /
  pointer values — those are the canonical impurity that makes a naive dumper non-gateable; (d)
  recursion sentinel must be identical on all three backends.
- **Notable:** This is the single most reusable module — it composes with examples and tests. The
  visited-set kernel already exists; the work is the format spec + the PHP helper.

---

## 2. URL parse + build + query  — Tier A — confidence HIGH

- **API (Phorj):**
  `Core.Url.parse(string) -> Url?` (an injected struct/enum: scheme/host/port/path/query/fragment);
  `Core.Url.build(Url) -> string`;
  `Core.Url.encodeComponent(string) -> string` / `decodeComponent(string) -> string?` (percent-enc);
  `Core.Url.parseQuery(string) -> Map<string,string>` / `buildQuery(Map<string,string>) -> string`.
- **Purity:** PURE — pure string→struct→string transformation, no I/O, no DNS resolution (resolution
  would be Tier B; parsing is not).
- **std reliance:** only `str`/`char`/`u8` byte work + `format!`. Percent-encoding is a byte loop
  (`%{:02X}`). No std URL type exists, so hand-roll an RFC-3986 subset (scheme `://` authority path
  `?` query `#` fragment). Decode is fallible (`%ZZ` → `None`), honoring EV-7.
- **PHP transpile target:** `parse_url()` (core, pure), `http_build_query()` (core),
  `urlencode`/`urldecode` and `rawurlencode`/`rawurldecode` (core). **Trap:** PHP has TWO encodings —
  `urlencode` encodes space as `+`, `rawurlencode` as `%20`; query strings use `+`, path components
  use `%20`. Pick **rawurlencode for components, http_build_query for queries** and match the Rust
  side exactly. `parse_url` returns an associative array with optional keys — the Phorj `Url?` must
  map missing parts to `Null` consistently with the Rust parser's `Optional` fields.
- **Determinism traps:** query-string **key ordering** — `Map` is insertion-ordered, so `buildQuery`
  must emit in insertion order and `http_build_query` does preserve array order → matches. Percent-
  encoding case (`%2F` vs `%2f`) must be pinned to UPPERCASE on both sides (PHP emits uppercase).
- **Notable:** very high-value (web examples, M6 router). Entirely std-implementable.

---

## 3. Base64 / Hex encoding  — Tier A — confidence HIGH

- **API (Phorj):**
  `Core.Encoding.base64Encode(bytes) -> string` / `base64Decode(string) -> bytes?`;
  `Core.Encoding.hexEncode(bytes) -> string` / `hexDecode(string) -> bytes?`.
  (Operates on the existing `bytes` primitive — already have `Core.Bytes`.)
- **Purity:** PURE — total functions over byte arrays.
- **std reliance:** none beyond slice/`u8` arithmetic. Base64 is ~30 lines (alphabet table + 3-byte→
  4-char with padding); decode validates alphabet + padding (EV-7 `None` on bad input). Hex is a
  trivial `{:02x}` / `from_str_radix(.., 16)` loop.
- **PHP transpile target:** `base64_encode`/`base64_decode` (core; `base64_decode($s, true)` strict
  mode returns `false` on bad input → maps to `null`); `bin2hex`/`hex2bin` (core). All present under
  `php -n` (core, no extension needed). **Trap:** PHP `base64_decode` in lenient mode silently skips
  bad chars — use the **strict** 2nd-arg form so it matches the Rust decoder's `None`-on-bad-input.
- **Determinism traps:** none — fully deterministic. Watch the padding policy (always emit `=`
  padding to match PHP).
- **Notable:** trivial, pure, immediately gateable. Good first slice of an `Encoding` module.

---

## 4. Hashing (digests)  — Tier A (the algorithms) — confidence HIGH for crc32/md5/sha1/sha256

- **API (Phorj):**
  `Core.Hash.crc32(bytes) -> int`;
  `Core.Hash.md5(bytes) -> string` (hex);
  `Core.Hash.sha1(bytes) -> string` (hex);
  `Core.Hash.sha256(bytes) -> string` (hex).
- **Purity:** PURE — deterministic digest of input bytes.
- **std reliance:** **hand-rolled in safe std Rust** — std's `DefaultHasher` is SipHash, NOT a stable
  cross-platform/cross-version digest, and is explicitly documented as unstable, so it CANNOT back a
  content hash. CRC32 = a table + loop. MD5/SHA-1/SHA-256 are well-specified bit-twiddling over
  `u32`/`u64` (`wrapping_add`, rotates) — implementable in pure safe Rust with no `unsafe`. Must use
  `wrapping_*` ops (these algorithms are defined modulo 2^32/2^64; checked arithmetic would falsely
  fault).
- **PHP transpile target:** `crc32()` (core), `md5()`/`sha1()` (core), `hash('sha256', $s)` (the
  `hash` extension — **VERIFY it's compiled-in under `php -n`**; `hash` is usually bundled but this is
  the one risk, grade MEDIUM for sha256 specifically). md5/sha1/crc32 are guaranteed core.
- **Determinism traps:** **CRYPTO BOUNDARY — do NOT hand-roll anything used for security** (per the
  task constraint: "Crypto/TLS/secure-RNG must NOT be hand-rolled"). md5/sha1 are CRYPTOGRAPHICALLY
  BROKEN; expose them ONLY as **checksums/content-addressing**, never present them as security
  primitives. sha256 hand-rolled is fine as a *checksum* but must NOT be marketed for password
  hashing / HMAC / signatures (those need constant-time + a vetted impl → defer / PHP-only via
  `password_hash`, which is Tier B-ish and not portable anyway). Document the non-security framing.
- **Determinism:** sha256 hand-roll must match PHP's `hash('sha256', …)` byte-for-byte — test against
  the oracle. crc32 has a PHP-vs-others polynomial/reflection gotcha: PHP's `crc32()` uses the
  standard CRC-32 (IEEE 802.3, reflected) — match that exact variant.

---

## 5. Random (seeded)  — Tier A iff SEEDED — confidence HIGH

- **API (Phorj):**
  `Core.Random.seeded(int seed) -> Rng` (an injected handle/struct carrying state) with methods
  `nextInt(Rng, int bound) -> int`, `nextFloat(Rng) -> float`, OR a functional form
  `Core.Random.next(int state) -> (int, int)` returning (value, nextState) to stay value-native and
  avoid mutable handle plumbing. A pure functional PRNG fits Phorj's immutable model best.
- **Purity:** PURE **iff seeded** by an explicit argument. **Impure** (`Tier B`) iff it pulls OS
  entropy — `getrandom` isn't in std anyway, and `RandomState`'s seed is process-random → NOT
  reproducible → NOT gateable. So the *only* gateable random is a hand-rolled deterministic PRNG.
- **std reliance:** hand-roll a small, well-defined algorithm — **xorshift / PCG / SplitMix64** — all
  pure `u64` `wrapping_*` + shifts. SplitMix64 is ~4 lines and ideal for a functional `next(state)`.
- **PHP transpile target:** **THE HARD PART.** PHP's `mt_rand`/`mt_srand` use Mersenne Twister with a
  PHP-specific seeding; `rand` is platform-dependent. To stay byte-identical, do NOT transpile to
  PHP's RNG — instead **transpile the hand-rolled SplitMix64 to a `__phorj_rng_next` PHP helper**
  that reproduces the exact same `u64` arithmetic. CAUTION: PHP ints are 64-bit signed and PHP has no
  native u64 — the helper must mask with `& 0xFFFFFFFFFFFFFFFF` semantics via careful `int`
  arithmetic or GMP-free bit ops; this is the determinism risk → grade the *transpile* MEDIUM, the
  Rust side HIGH. Alternatively expose only `nextInt(bound)` results that are small enough to dodge
  the u64-signedness mismatch.
- **Determinism traps:** seeding must be explicit (no `time()` default seed — that's the classic
  impurity). u64 overflow semantics differ between Rust (`wrapping`) and PHP (`int` is signed,
  overflow → float in older PHP, but 64-bit since 7) — pin with explicit masking.

---

## 6. SQL — connection (Tier B) vs query BUILDER (Tier A)  — confidence HIGH on the split

- **API (Phorj), Tier A builder:**
  `Core.Sql.select(...).from(...).where(...).build() -> string` OR a simpler
  `Core.Sql.quote(string) -> string` (escape), `Core.Sql.placeholders(int n) -> string` (`?,?,?`),
  and a fluent builder producing a parameterized statement `(sql: string, params: List<...>)`.
- **Purity:** the **query builder is PURE** (string assembly + escaping/binding — no connection); the
  **connection/execute is Tier B** (network/file I/O, non-deterministic results) → quarantined behind
  the M6 `Transport`-style seam, fixture-tested outside `differential.rs`.
- **std reliance:** builder = pure string work. No std SQL driver exists; a real driver needs sockets
  (`std::net`, plaintext only — no TLS) → Tier B and out of scope for byte-identity.
- **PHP transpile target:** builder → plain string concatenation in PHP; escaping → **do NOT use
  `mysqli_real_escape_string`/`PDO::quote`** (those need a live connection / the mysqli|pdo extension,
  ABSENT under `php -n`). For a *pure, gateable* escaper, define a deterministic Phorj-level escape
  (e.g. ANSI-SQL single-quote doubling `'` → `''`) and transpile to a `__phorj_sql_quote` helper —
  but FLAG LOUDLY: a hand-rolled escaper is **not** a substitute for parameterized queries against a
  real driver; the gateable surface is the *builder + binding placeholders*, with actual binding done
  by the (Tier B) driver. Lean on **`?` placeholders + a params list**, NOT string interpolation, as
  the public API — escaping-by-concatenation invites SQL injection if misused.
- **Determinism traps:** the moment a real DB is involved (row order without ORDER BY, auto-increment
  ids, NOW()) determinism is gone — that's the Tier-B boundary. Keep the gateable module to building
  the *statement text* only.

---

## 7. CSV  — Tier A — confidence HIGH

- **API (Phorj):**
  `Core.Csv.parse(string) -> List<List<string>>` (or `List<Map<string,string>>` with a header row);
  `Core.Csv.format(List<List<string>>) -> string`.
- **Purity:** PURE — string↔table transformation.
- **std reliance:** hand-rolled state machine (RFC 4180): handle quoted fields, embedded commas,
  embedded newlines, `""` escaping. ~60 lines of safe `char`/`String` work. No std CSV.
- **PHP transpile target:** PHP's `str_getcsv`/`fputcsv`/`fgetcsv` — **`str_getcsv` is core** (string
  parsing); writing is trickier (`fputcsv` needs a stream). For pure round-tripping prefer a
  `__phorj_csv_*` helper that matches the Rust state machine exactly, rather than relying on
  `str_getcsv`'s edge-case quirks (its escape-char handling has historically differed across PHP
  versions — a determinism risk against the 8.5 floor). Grade MEDIUM if leaning on `str_getcsv`;
  HIGH with a dedicated helper.
- **Determinism traps:** line-ending normalization (`\r\n` vs `\n`), trailing newline policy, quoting
  policy (quote-always vs quote-when-needed) — pin all three and match the helper.

---

## 8. Date / Time  — split: arithmetic/format/parse (Tier A) vs now() (Tier B)  — confidence HIGH

- **API (Phorj), Tier A:**
  `Core.Time.fromEpoch(int seconds) -> DateTime` (injected struct);
  `Core.Time.format(DateTime, string fmt) -> string`;
  `Core.Time.parse(string, string fmt) -> DateTime?`;
  `Core.Time.addDays(DateTime, int) -> DateTime`, `diff(DateTime, DateTime) -> int`, etc.
- **Purity:** arithmetic/format/parse on an **explicitly-supplied epoch** is PURE → Tier A.
  **`now()` / `SystemTime::now()` is Tier B** (clock) — quarantined.
- **std reliance:** std has `SystemTime`/`Duration` but NO calendar (no Gregorian decomposition, no
  formatting) — that's what `chrono`/`time` crates provide and they're banned. So **hand-roll the
  civil-date algorithms** (days-from-epoch ↔ y/m/d via Howard Hinnant's well-known integer formulas,
  pure `i64` arithmetic, leap-year correct). Formatting is a `strftime`-subset string builder.
- **PHP transpile target:** `gmdate($fmt, $epoch)` / `date($fmt, $epoch)` (core). **CRITICAL TRAP:
  use `gmdate` (UTC), NOT `date`** — `date()` depends on the process timezone (`date_default_timezone`),
  which is **locale/config-dependent and non-deterministic across machines**. Pin everything to UTC.
  Parsing: `DateTime::createFromFormat` needs the `date` extension (present in core) but again is
  timezone-sensitive → prefer a `__phorj_time_parse` helper matching the Rust parser.
- **Determinism traps (the big ones):** (a) **timezone** — UTC only, never local; (b) **locale** —
  month/day names must be a fixed English table, never `setlocale`-dependent; (c) leap seconds — POSIX
  ignores them, match that; (d) the format-spec dialect (`strftime` vs PHP `date` letters differ —
  `Y-m-d` PHP vs `%Y-%m-%d` C) must be ONE Phorj-defined dialect mapped to both sides. This is the
  module with the most determinism traps — getting UTC + locale-free is the whole game.

---

## 9. HTTP client  — Tier B, and BLOCKED std-only by no-TLS  — confidence HIGH (on infeasibility)

- **Purity:** IMPURE (network, non-deterministic) → Tier B, cannot be in the byte-identity spine.
- **std reliance:** std has `TcpStream` (plaintext) but **NO TLS** → HTTPS is impossible without a
  crate. A plaintext HTTP/1.1 client is hand-rollable over `TcpStream` (write request lines, read
  response), but it's plaintext-only (useless for the modern HTTPS web) AND impure.
- **PHP transpile target:** PHP core has `file_get_contents` with stream wrappers / `fopen('http://')`
  but **the `curl` extension and openssl are ABSENT under `php -n`**, and HTTPS needs openssl. So even
  the PHP leg can't do HTTPS in the test oracle.
- **Verdict:** **DEFER to M6** behind the `Transport` trait, fixture-tested outside `differential.rs`,
  transpiled to PHP `curl`/streams for *production* runtime (not the oracle). Per the existing M6
  decision (`docs/specs/2026-06-18-m6-web-design.md`): URL/network deferred; determinism (not the
  dependency) is the gate. Do NOT attempt a std-only HTTPS client.

---

## 10. Validation / Filtering  — Tier A — confidence HIGH

- **API (Phorj):**
  `Core.Validate.isEmail(string) -> bool`, `isInt(string) -> bool`, `isFloat(string) -> bool`,
  `isUrl(string) -> bool`, `inRange(int, int, int) -> bool`, `parseInt(string) -> int?`,
  `parseFloat(string) -> float?` (the last partly exists — `Text.parseFloat`).
- **Purity:** PURE — predicates / parsers over strings.
- **std reliance:** `str::parse`, char-class checks. No regex needed for the simple validators (email
  can use a pragmatic non-regex check; a full RFC-5322 email regex is famously a trap — use a
  "has exactly one @, non-empty local/domain, domain has a dot" heuristic and DOCUMENT it as
  pragmatic, matching the PHP `FILTER_VALIDATE_EMAIL` *spirit* not its exact algorithm).
- **PHP transpile target:** `filter_var($s, FILTER_VALIDATE_EMAIL|_INT|_FLOAT|_URL)` — **the `filter`
  extension is core and present under `php -n`** (grade MEDIUM — verify; `filter` is usually compiled
  in). **TRAP:** `FILTER_VALIDATE_EMAIL`'s exact acceptance set is a specific PHP algorithm; a
  hand-rolled Rust validator will NOT match it byte-for-byte on edge cases → byte-identity break. So
  EITHER (a) transpile to a `__phorj_validate_*` helper that mirrors the Rust heuristic exactly
  (recommended — keeps the spine), OR (b) restrict examples to inputs where both agree. Same risk as
  date parsing and CSV: hand-rolled-vs-PHP-builtin edge-case divergence.
- **Determinism traps:** locale-dependent float parsing (`,` vs `.` decimal) — pin to `.` always;
  `FILTER_VALIDATE_FLOAT` can accept locale separators → use the helper or restrict inputs.

---

## 11. Regex  — Tier A in principle, but a STRATEGIC FORK — confidence MEDIUM

- **Purity:** matching is PURE.
- **std reliance:** **NO regex in std** (the `regex` crate is banned). Two options:
  (A) **hand-roll a small engine** (Thompson NFA over a regex subset) — significant work, but pure and
  std-only and fully controllable for byte-identity;
  (B) **transpile to PCRE** (`preg_match`/`preg_replace`) — PCRE IS core under `php -n` (the task
  confirms "PCRE is core") — but then the Phorj backends (interpreter/VM) need a matching engine
  ANYWAY to run `run`/`runvm`, so option B alone doesn't satisfy the three-backend identity. You
  cannot transpile-only; the interpreter and VM must compute the same result.
- **Verdict:** regex requires a **hand-rolled engine in the Rust backends** whose semantics are pinned
  to a documented subset, transpiled to PCRE with the same subset. This is a *milestone-sized* effort,
  not a quick native. The big determinism trap: PCRE has many features (backreferences, lookaround,
  Unicode property classes) a hand-rolled subset won't have — the Phorj regex dialect must be a
  **strict subset of PCRE** so the transpile is sound, and inputs in examples must stay inside it.
  RECOMMEND deferring to its own milestone (M-text or a dedicated M-regex); do NOT bolt it on as a
  native module slice.

---

## 12. Other high-value capabilities spotted

| Capability | Tier | std-only? | PHP target (`php -n`) | Note |
|---|---|---|---|---|
| **Bytes/string codecs** — UTF-8 validate/decode, byte↔string | A | YES (`str::from_utf8`) | `mb_*` ABSENT → use core `utf8_*`/manual | partly have `Core.Bytes`; UTF-8 validation is pure + valuable |
| **JSON5 / pretty JSON** | A | YES (extend `Core.Json`) | hand-roll helper | already have `Core.Json` incl. `stringifyPretty` |
| **String case / slug / pad / wrap** | A | YES | core `str_pad`/`wordwrap`/`ucfirst` | extend `Core.Text`; watch mbstring-absence for Unicode case → ASCII-only or helper |
| **Math breadth** (`log`, `sin/cos`, `gcd/lcm`, `clamp`) | A | YES (`f64` methods) | core `log`/`sin`/`pow` | extend `Core.Math`; **float transcendentals diverge** from PHP's 14-digit `echo` (known issue) → restrict examples to exactly-representable or use the float helper |
| **UUID** | A iff seeded/v5; B iff v4-random | hand-roll | no core uuid ext | v4 needs RNG (see §5 caveats); v5 (sha1 of namespace+name) is PURE and gateable — prefer v5 |
| **Levenshtein / similar_text** | A | YES (hand-roll) | core `levenshtein`/`similar_text` | pure; PHP `levenshtein` is core |
| **Number formatting** (thousands sep, money) | A | YES | core `number_format` | overlaps M-NUM; locale trap (sep char) → pin |
| **Glob / fnmatch (pattern match, not regex)** | A | YES (hand-roll) | core `fnmatch` (verify under -n) | simpler than regex; pure |
| **Compression (gzip/deflate)** | A-with-a-CATCH | **NO** in std (needs `flate2`) | core `gzencode`/`gzdeflate` (zlib ext — verify) | **DETERMINISM TRAP: the gzip header carries an mtime → non-deterministic.** Even if hand-rolled, gzip's header timestamp breaks byte-identity. Use **raw DEFLATE** (no header) if at all, and hand-rolling DEFLATE is large. DEFER. |

---

## Cross-cutting determinism traps (the named risk list)

1. **Object ids / addresses** (`spl_object_id`, pointer printing) — the canonical var-dump impurity. Never expose.
2. **Clock** (`now()`, `time()`, default-seeded RNG, gzip mtime) — Tier B; gzip's embedded mtime is a sneaky one.
3. **Locale** — float decimal separator, month/day names, case folding, `number_format` separators. Pin to C/UTF-8/English/`.`.
4. **Timezone** — `date()` vs `gmdate()`; ALWAYS UTC.
5. **Iteration order** — historically HashMap/HashSet; Phorj already neutralized this (insertion-ordered Map/Set, R1). Keep it.
6. **Float rendering** — shortest-round-trip (`__phorj_float`) vs PHP's 14-digit `echo` vs scientific. Transcendentals/irrationals diverge → restrict example inputs (existing KNOWN_ISSUE).
7. **Hand-rolled-vs-PHP-builtin edge cases** — the recurring pattern (CSV `str_getcsv`, email `FILTER_VALIDATE_EMAIL`, date `createFromFormat`, base64 lenient mode): a PHP builtin's edge-case behavior won't match a Rust hand-roll → **prefer a `__phorj_*` runtime helper that mirrors the Rust kernel** over leaning on the builtin, wherever edge cases matter.
8. **`php -n` extension absence** — mbstring, curl/openssl, pdo/mysqli are ABSENT. Verify `hash`, `filter`, `zlib`, `fnmatch` are compiled-in before relying on them; core (PCRE, base64, md5/sha1/crc32, json, str_getcsv, date, number_format, levenshtein) is safe.
9. **u64/signed-int mismatch** — Rust `wrapping` u64 vs PHP signed 64-bit int (PRNG, hashing). Mask explicitly; this is the PRNG transpile risk.
10. **Crypto boundary** — md5/sha1/sha256 only as checksums, never security; no hand-rolled TLS/secure-RNG/password-hash.

---

## Recommended first slices (clear all three gates, lowest risk, highest value)

1. **`Core.Debug.dump`** (var-dump) — reuses the visited-set, composes everywhere. HIGH.
2. **`Core.Encoding`** (base64 + hex) — trivial, pure, exact PHP core mapping. HIGH.
3. **`Core.Url`** (parse/build/query) — high web value, pure, std-only. HIGH.
4. **`Core.Hash`** (crc32/md5/sha1, sha256 pending `hash`-ext verify) — pure checksums. HIGH.
5. **`Core.Csv`** — pure, helper-backed for edge-case safety. HIGH.
6. **`Core.Time`** (epoch arithmetic/format, UTC-only) — high value, most traps but all nameable. HIGH for the pure subset.
7. **`Core.Random.seeded`** (SplitMix64) — pure iff seeded; transpile u64 risk MEDIUM.
8. **`Core.Validate`** — helper-backed. HIGH for the pure subset.

DEFER: HTTP/TLS (M6 Transport), real SQL driver (Tier B), regex (own milestone), compression (mtime trap + large DEFLATE), v4-UUID (random caveats — prefer v5).
