# Prior-Art Sweep — PHP stdlib + ecosystem, candidates for Phorj native modules

**Lens:** PHP standard library + compiled-in extensions + canonical libraries
(symfony/var-dumper, Guzzle, PDO, Carbon, PCRE, `filter_var`, intl).
**Question per capability:** API shape → pure/impure (determinism) → notable for porting to Phorj.

**Ground truth (verified by reading the repo, not memory):**
- Existing `Core.*` modules: `Console, Math, Text, File, Bytes, Json, List, Map, Set, Convert,
  Decimal, Html, Reflect, Process, Env` (grep over `src/native/*.rs`).
- Native registry shape verified in `src/native/mod.rs`: each `NativeFn` single-sources
  `module/name/params/ret/eval (NativeEval::{Pure,HigherOrder,Reflective})/php/pure`.
- `pure: false` precedent verified in `src/native/process.rs` (Process/Env quarantined from the
  byte-identity differential, tested in `tests/process.rs`, walkthrough in `examples/process/`).
- The PHP oracle runs `php -n` (verified in CLAUDE.md + memory `transpile-no-ini-extensions`):
  **mbstring is ABSENT**, PCRE is core, BCMath is loaded via `-d`. This is the hardest single
  constraint on this sweep — any transpile target must be PHP CORE or a flag-loaded extension.

This sweep deliberately covers ONLY capabilities NOT already shipped, plus depth on the partial ones.

---

## TIER A — pure / deterministic → byte-identity-gateable, ships like any `Core.*` module

### A1. var-dump / pretty-print (symfony/var-dumper lens)  — HIGH value, HIGH confidence
- **PHP prior art:** `var_dump()` (types + values, but embeds object ids `#3` and is locale-ish on
  floats), `print_r()` (no object ids, recursion marker `*RECURSION*`), `var_export()` (valid-PHP
  re-parseable form), and symfony's `dump()`/`VarCloner` (depth/recursion-safe, casters).
- **API shape (Phorj):**
  ```
  import Core.Debug;
  Console.println(Debug.dump(value));        // Debug.dump(T) -> string
  Console.println(Debug.inspect(value, 2));  // depth-limited: Debug.inspect(T, int) -> string
  ```
- **Purity:** PURE — *only if Phorj defines its own deterministic format*. The native `Value` enum
  is closed (Int/Float/Bool/Str/Bytes/Decimal/Null/List/Map/Set/Instance/Enum/Closure), so a dumper
  can pattern-match it exhaustively. **No addresses, no object ids** (the whole point — see Determinism
  Traps). Insertion-ordered Map/Set reps already guarantee stable key order.
- **Reuse:** `value::eq_val_rec` already carries a **cyclic visited-set** — reuse the same
  pointer-identity tracking for `*RECURSION*` detection. `NativeEval::Reflective` gives the static
  field-name order via `ClassTables.fields` (sorted) for `Instance` dumping; the closed `Value`
  means the runtime walk needs no reflection for the value side.
- **Transpile target:** NOT PHP `var_dump`/`print_r` (their format differs + object ids leak). Emit a
  **gated runtime helper** `__phorj_dump($v)` (the `uses_*`/`emit_runtime_helpers` mechanism) that
  reproduces Phorj's chosen format byte-for-byte — this is the same discipline as `__phorj_str`.
- **Notable:** This is the single highest-leverage Tier-A module — debugging is a daily need and PHP's
  own `var_dump` is *not* directly portable (object ids). Phorj owning the format is an upgrade, not
  a port. Closures dump as an opaque `<closure>` token (no body, deterministic).

### A2. URL parse + build + query  — HIGH value, HIGH confidence
- **PHP prior art:** `parse_url()` (→ assoc array scheme/host/port/path/query/fragment),
  `http_build_query()`, `parse_str()`, `urlencode`/`rawurlencode` (RFC 3986 vs 1738).
- **API shape:**
  ```
  import Core.Url;
  Url.parse(s) -> Url?                  // a Phorj struct: scheme/host/port?/path/query/fragment
  Url.encode(s) -> string               // percent-encode a component (rawurlencode)
  Url.decode(s) -> string?              // null on malformed %-escape
  Url.buildQuery(Map<string,string>) -> string   // sorted-by-key for determinism
  Url.parseQuery(s) -> Map<string,string>
  ```
- **Purity:** PURE — pure string transformation, no I/O. Use the **injected-type pattern**
  (`cli::inject_*_prelude`) to introduce the `Url` struct AST, exactly like `Core.Json`'s injected
  `Json` enum.
- **Reuse:** std Rust only — manual percent-decode (`u8::from_str_radix(..,16)` on `%XX`), no `url`
  crate. `Core.Text.split`/`split_once` already exist for the query splitting.
- **Transpile target:** `rawurlencode`/`rawurldecode`/`parse_url` are **PHP core** (no extension) — but
  `parse_url` returns a different shape; safer to emit a runtime helper that mirrors Phorj's parse
  rules so edge cases (empty path, missing scheme) are byte-identical. `http_build_query` ordering is
  insertion order in PHP — Phorj sorts keys, so emit `ksort()` + manual build for parity.
- **Determinism trap:** query-string key ORDER. PHP arrays are insertion-ordered; force sorted-by-key
  in both backends and the PHP emission.

### A3. base64 / hex / URL-safe-base64 encoding  — HIGH value, HIGH confidence
- **PHP prior art:** `base64_encode`/`base64_decode` (core), `bin2hex`/`hex2bin` (core),
  `strtr($s,'+/','-_')` for URL-safe.
- **API shape:**
  ```
  import Core.Encoding;
  Encoding.base64Encode(bytes) -> string
  Encoding.base64Decode(string) -> bytes?     // null on invalid
  Encoding.hexEncode(bytes) -> string
  Encoding.hexDecode(string) -> bytes?
  Encoding.base64UrlEncode(bytes) -> string   // -_ , no padding
  ```
- **Purity:** PURE. Operates on the existing `Value::Bytes`. Hand-rolled base64 alphabet table —
  trivial, std-only.
- **Reuse:** `Core.Bytes` already exists (`from_string`/`to_string`/`len`/`concat`/`slice`); this is
  the natural sibling module operating on the same `bytes` primitive.
- **Transpile target:** `base64_encode`/`base64_decode`/`bin2hex`/`hex2bin` are all **PHP core** —
  direct mapping (no helper needed for the standard variants; URL-safe needs `strtr` + `rtrim`).
- **Determinism trap:** none for standard base64. `base64Decode` of invalid input → `null` (PHP returns
  `false`; map to Phorj `null` consistently in the helper).

### A4. hashing — non-crypto (crc32) + crypto digests (md5/sha1/sha256)  — MEDIUM value, MEDIUM confidence
- **PHP prior art:** `crc32()`, `md5()`, `sha1()`, `hash('sha256',$s)`, `hash('xxh3',…)`. All core via
  ext-hash (compiled in by default; **verify** it survives `php -n` — `crc32/md5/sha1` are core
  functions, `hash()` is ext-hash which is *usually* but not guaranteed under `-n`).
- **API shape:**
  ```
  import Core.Hash;
  Hash.crc32(bytes) -> int
  Hash.md5(bytes) -> string      // lowercase hex
  Hash.sha1(bytes) -> string
  Hash.sha256(bytes) -> string
  ```
- **Purity:** PURE (a hash is a deterministic function of input bytes).
- **CONSTRAINT CONFLICT (flagged):** the project rule is *"Crypto/TLS/secure-RNG must NOT be
  hand-rolled."* md5/sha1/sha256 are cryptographic hash *constructions*. Two readings:
  1. **Hand-rolling sha256 is forbidden** → defer crypto digests, ship only `crc32` (non-crypto,
     trivially hand-rolled) now. **(Recommended — safest.)**
  2. If the rule means "don't roll your own *cipher/secure-RNG*" and a well-known reference
     implementation of sha256 is acceptable, it can ship — but the transpile target `hash('sha256',…)`
     must be verified present under `php -n`. **Decision needed from the human; I default to (1).**
- **Transpile target:** `crc32` is PHP core. md5/sha1 are PHP core. `hash('sha256',$s)` is ext-hash.
- **Determinism trap:** none (hashes are deterministic); the trap is the *policy* above, not behavior.

### A5. SEEDED random (deterministic PRNG)  — MEDIUM value, HIGH confidence
- **PHP prior art:** `mt_srand($seed)` + `mt_rand()`, and PHP 8.2's `\Random\Randomizer` with an
  explicit seedable engine (`Mt19937`/`Xoshiro256StarStar`/`PcgOneseq128XslRr64`).
- **API shape:**
  ```
  import Core.Random;
  var rng = Random.seeded(42);     // returns a Rng instance (struct holding state)
  rng.nextInt(0, 100) -> int
  rng.nextFloat() -> float
  rng.shuffle(List<T>) -> List<T>
  ```
- **Purity:** PURE **iff seeded** — a seeded PRNG is a deterministic function of seed + call count.
  **Unseeded/true-random is Tier B** (see B-list). The seeded form is byte-identity-gateable.
- **CONSTRAINT NOTE:** This is a *non-crypto* PRNG (e.g. a small xorshift/PCG), explicitly NOT a secure
  RNG — so it does NOT violate the "no hand-rolled secure-RNG" rule. Must be documented as
  non-cryptographic.
- **Reuse:** the Rng is a Phorj `Instance` carrying its state (mutation milestone gives
  shared-mutable instances) OR a value-threaded functional form `(newState, value) = Random.next(state)`.
  Functional form is cleaner for byte-identity (no hidden mutation order issues).
- **Transpile target:** **THE HARD PART.** PHP's `Mt19937` and a hand-picked xorshift will NOT produce
  identical sequences. Two options: (a) implement the *same* algorithm both sides and transpile to a
  `__phorj_rng_*` runtime helper that reimplements it in PHP (byte-identical by construction — the
  ONLY safe path); (b) pick `Mt19937` and emit `\Random\Randomizer` (risky: engine details/version).
  **Recommend (a): own the algorithm, emit a helper.**
- **Determinism trap:** seed MUST be explicit; the algorithm must be byte-identical across all three
  legs (own it, don't borrow PHP's).

### A6. SQL query BUILDER (escaping/binding, NOT connection)  — MEDIUM value, MEDIUM confidence
- **PHP prior art:** Doctrine DBAL QueryBuilder, Laravel's fluent builder, PDO `quote()`/prepared
  statements. The *builder* (string assembly + parameter binding) is pure; the *execution* is Tier B.
- **API shape:**
  ```
  import Core.Sql;
  var q = Sql.select(["id","name"]).from("users").where("age > ?", [18]).build();
  q.sql -> string          // "SELECT id, name FROM users WHERE age > ?"
  q.params -> List<...>    // bound parameter values, in order
  ```
- **Purity:** PURE — produces a `(sql_string, params_list)` pair, no DB contact. The escaping/binding
  is deterministic string assembly.
- **Reuse:** builder is a Phorj class with chained methods (method-chaining already works); the
  output is just strings + a params list.
- **Transpile target:** pure PHP — the builder emits to a Phorj class that transpiles to a normal PHP
  class; no PDO needed at build time. **Execution** (`->execute($pdo)`) is Tier B, deferred to M6.
- **Determinism trap:** none (string assembly). Identifier quoting style must be fixed (always
  backtick or always nothing) so the output is stable.
- **Note:** lower priority than A1–A3 — useful but niche until DB execution (Tier B) lands; the builder
  alone has limited standalone value. Defer-able.

### A7. CSV read/write (string-level)  — MEDIUM value, HIGH confidence
- **PHP prior art:** `str_getcsv()`, `fgetcsv()`, `fputcsv()` (RFC 4180-ish, configurable delimiter/
  enclosure/escape).
- **API shape:**
  ```
  import Core.Csv;
  Csv.parse(string) -> List<List<string>>          // rows of fields
  Csv.parseRow(string) -> List<string>
  Csv.format(List<List<string>>) -> string
  ```
- **Purity:** PURE at the string level (parse a string → rows; format rows → string). The *file* form
  (`fgetcsv` on a handle) is Tier B, but `Core.File.read` already gives a string to feed `Csv.parse`.
- **Reuse:** composes with `Core.File.read -> string?` already shipped.
- **Transpile target:** `str_getcsv()` is PHP core (no extension). `fputcsv` writes to a handle — for
  the string form, a small runtime helper is cleaner for byte-identity (quoting rules edge cases:
  embedded quotes/newlines/delimiters).
- **Determinism trap:** quoting/escaping rules and line-ending normalization (CRLF vs LF) must be
  pinned; PHP's `str_getcsv` has subtle escape-char behavior — own the parser via a helper for parity.

### A8. date/time ARITHMETIC, format, parse (NOT `now()`)  — HIGH value, MEDIUM confidence
- **PHP prior art:** `DateTime`/`DateTimeImmutable`, `DateInterval`, `date()`/`strtotime()`,
  `IntlDateFormatter` (intl ext — ABSENT under `-n`), Carbon (`carbon/carbon`).
- **API shape:**
  ```
  import Core.Time;
  Time.fromUnix(int) -> DateTime              // a Phorj struct y/m/d/h/min/s
  dt.addDays(int) -> DateTime
  dt.format("Y-m-d") -> string
  Time.parse("2026-06-26", "Y-m-d") -> DateTime?
  Time.diffDays(a, b) -> int
  ```
- **Purity:** PURE for ARITHMETIC/format/parse on an *explicit* instant. `Time.now()` is Tier B (clock).
  The whole calendar algebra (leap years, day-of-week via Zeller, Unix-epoch arithmetic) is a pure
  deterministic function — no clock, no locale.
- **Reuse:** injected-type pattern for the `DateTime` struct; std Rust integer math only.
- **Transpile target:** **AVOID `DateTime`/intl** — `IntlDateFormatter` is ext-intl (absent under
  `-n`), and `DateTime` formatting can be timezone/locale-sensitive. Emit a `__phorj_time_*` runtime
  helper doing the same integer epoch math + a fixed-format printer. Format tokens are a *Phorj-owned*
  subset (not PHP's full `date()` alphabet) to keep parity tractable.
- **Determinism traps (MULTIPLE — the riskiest Tier-A module):**
  - **timezone:** ALL arithmetic must be UTC-only (no `date_default_timezone_*`); a TZ-aware version is
    Tier B / deferred.
  - **locale:** month/day NAMES are locale-dctated in `IntlDateFormatter` — ship only numeric formats
    first, or hard-code English names (Phorj-owned, not locale).
  - **leap seconds / DST:** ignore (use proleptic Gregorian + fixed 86400-second days), document it.

### A9. regex (PCRE)  — HIGH value, MEDIUM-LOW confidence
- **PHP prior art:** `preg_match`/`preg_match_all`/`preg_replace`/`preg_split` (ext-pcre, **core** —
  survives `php -n`, confirmed by memory `transpile-no-ini-extensions` which says PCRE is core).
- **API shape:**
  ```
  import Core.Regex;
  Regex.isMatch(pattern, subject) -> bool
  Regex.match(pattern, subject) -> List<string>?      // capture groups, null if no match
  Regex.replace(pattern, subject, repl) -> string
  Regex.split(pattern, subject) -> List<string>
  ```
- **Purity:** PURE (regex over a fixed pattern+input is deterministic).
- **THE BIG PROBLEM (flagged hard):** **Rust std has NO regex engine.** The project is ZERO external
  crates (no `regex` crate). So to run a regex on the *interpreter and VM* backends, Phorj would have
  to **hand-roll a regex engine in std-only Rust** — a Thompson-NFA/backtracking matcher. That is a
  large, error-prone undertaking, and matching PCRE's exact semantics (backreferences, lookaround,
  Unicode classes, greedy/lazy quantifiers) byte-for-byte against the transpiled `preg_*` is
  **extremely hard** — divergence risk is high.
- **Feasibility verdict:** A *restricted* regex subset (literals, `.`, `*`, `+`, `?`, `[...]`, `|`,
  anchors, basic groups — NO backreferences/lookaround) implemented as a small NFA could be made
  byte-identical against a *correspondingly restricted* PCRE usage. Full PCRE parity is **NOT feasible
  std-only**. Recommend: design a "Phorj regex dialect" (documented subset) rather than claiming PCRE
  compatibility. Confidence LOW that full parity is achievable; MEDIUM that a useful subset is.
- **Transpile target:** `preg_match` etc. are PCRE/core — but only safe if the Phorj engine's subset
  semantics exactly match PCRE on that subset. Determinism trap: any feature where the two engines
  disagree (e.g. POSIX vs PCRE char classes, Unicode handling) is a silent byte-identity break.

### A10. validation / filtering (`filter_var` lens)  — MEDIUM value, HIGH confidence
- **PHP prior art:** `filter_var($v, FILTER_VALIDATE_EMAIL/URL/INT/IP/FLOAT)`, `ctype_*`, symfony
  Validator / Respect\Validation.
- **API shape:**
  ```
  import Core.Validate;
  Validate.isEmail(string) -> bool
  Validate.isInt(string) -> bool
  Validate.isUrl(string) -> bool
  Validate.isIpv4(string) -> bool
  ```
- **Purity:** PURE (predicate over a string).
- **THE CATCH:** `FILTER_VALIDATE_EMAIL`/`URL` are *regex-backed* in PHP — to be byte-identical, Phorj
  must define its OWN validation rules (a documented grammar) and NOT claim `filter_var` parity, OR
  depend on A9 (regex), which is itself hard. **Recommend:** ship the simple predicates (`isInt`,
  `isIpv4` — pure char/range checks, no regex) now; defer email/URL validation until either A9 lands or
  a Phorj-owned (non-regex) rule is specified.
- **Transpile target:** for the simple predicates, emit hand-written PHP (`ctype_digit` + range checks)
  — NOT `filter_var` (whose email/URL rules are version-specific and would drift). Determinism trap:
  `filter_var`'s rules changed across PHP versions — never transpile to it for parity-gated code.

### A11. string breadth still missing from `Core.Text`  — MEDIUM value, HIGH confidence
- Existing `Core.Text` has len/upper/lower/trim/contains/split/split_once/join/replace/startsWith/
  endsWith/repeat (verified). Missing high-value PHP `str_*`:
  - `padLeft`/`padRight` (`str_pad`), `substr` (Phorj has `slice` on bytes but a char-`substr` on
    `string`), `indexOf` (`strpos -> int?`), `count` (`substr_count`), `reverse` (`strrev`),
    `replaceFirst`, `chars -> List<string>`, `wordCount`.
- **Purity:** PURE.
- **CONSTRAINT (sharp):** `Core.Text.len`/`upper`/`lower` are currently **ASCII-only** (verified:
  `to_ascii_uppercase`, `s.len()` = byte count). mbstring is ABSENT under `php -n`, so **multibyte
  string ops are NOT portable** — any Unicode-aware `substr`/`strlen` would need `mb_*` (absent). So
  new `Core.Text` ops must stay **byte/ASCII-level** to transpile to PHP core (`substr`/`strpos`/
  `str_pad` are core and byte-oriented — perfect match). Document the ASCII/byte semantics loudly.
- **Transpile target:** all map to PHP core byte functions (`str_pad`/`substr`/`strpos`/`strrev`/
  `str_word_count`). Determinism trap: any multibyte assumption (don't make one).

---

## TIER B — impure / non-deterministic → quarantined behind M6 `Transport`, fixture-tested OUTSIDE `differential.rs`

These follow the `Core.Process`/`Core.Env` precedent (`pure: false`, quarantined, `tests/<x>.rs`,
`examples/<x>/` walkthrough — NOT a gated example).

### B1. HTTP client (Guzzle lens)  — HIGH value, but BLOCKED
- **PHP prior art:** Guzzle, `curl_*` (ext-curl), `file_get_contents($url)` (with `allow_url_fopen`).
- **BLOCKER:** **Rust std has NO HTTP client and NO TLS.** Per CLAUDE.md, URL/network is explicitly
  **deferred to M6** ("Rust std has no HTTP client → breaks zero-dep, *and* network is
  non-deterministic"). Plain HTTP (no TLS) over `std::net::TcpStream` is *possible* but HTTPS is not
  std-only. This is the canonical Tier-B item: socket quarantined behind the M6 `Transport` trait,
  transpiled to PHP `curl`/Guzzle, fixture-tested.
- **Purity:** IMPURE (network, non-deterministic).
- **Verdict:** correctly deferred to M6; not a near-term native module. Listed for completeness.

### B2. Database access / PDO  — HIGH value, Tier B
- **PHP prior art:** PDO, mysqli. The query *builder* is Tier A (A6); the *connection + execute* is
  Tier B (network/socket, non-deterministic result set).
- **Purity:** IMPURE. Quarantined; transpiles to PDO. Pairs with A6 (build pure, execute impure).

### B3. `now()` / wall clock  — Tier B
- `Time.now() -> DateTime` reads the system clock → non-deterministic. Pairs with A8 (arithmetic pure,
  `now()` impure). Transpiles to PHP `time()`/`new DateTimeImmutable()`. Quarantine + fixture.

### B4. true random / secure random  — Tier B + POLICY
- `random_bytes()`/`random_int()` (CSPRNG). **IMPURE** (non-deterministic) AND **policy-blocked**
  ("secure-RNG must NOT be hand-rolled"). For the secure form, the ONLY acceptable path is to transpile
  to PHP's `random_bytes`/`random_int` and source entropy from the OS (`/dev/urandom` via `std::fs` on
  the Rust side) — never hand-roll the CSPRNG. Quarantined regardless. (Contrast A5 seeded PRNG = Tier
  A, non-secure.)

### B5. filesystem WRITE / directory ops  — Tier B
- `Core.File.read`/`exists`/`write` already exist (write is the impure one). Broader fs (`scandir`,
  `mkdir`, `glob`, `stat` with mtimes) is IMPURE (mtimes/inode/ordering non-deterministic). Quarantine
  anything returning timestamps or OS-ordered directory listings (sort listings for any pure-ish view).

---

## DETERMINISM TRAPS (named, cross-cutting — the things that silently break the byte-identity spine)

1. **Object ids / addresses** — `var_dump`'s `#3`, `spl_object_id`, any pointer-derived value. A dumper
   (A1) MUST NOT emit these. Phorj's closed `Value` makes this avoidable by construction.
2. **Clock** — `time()`, `now()`, `microtime()`, file mtimes (B3/B5). Any reachable clock read poisons
   determinism → Tier B.
3. **True randomness** — unseeded `rand`/`mt_rand`/`random_*` (B4). Only *seeded* PRNG with a
   Phorj-owned algorithm is Tier A (A5).
4. **Iteration / map ordering** — PHP arrays are insertion-ordered; OS env iteration is unspecified
   (handled in `Core.Env` by sorting). Any new map-producing native must FIX an order (insertion or
   sorted) identically in all three legs. Phorj's `Value::Map`/`Set` are already insertion-ordered.
5. **Locale** — `IntlDateFormatter` month/day names, `setlocale`-sensitive number/`strcasecmp`
   behavior, locale-dependent float formatting. A8/A10 must hard-code rules (English/UTC/`.`-decimal),
   never read a locale.
6. **mbstring absence under `php -n`** — multibyte string length/case/substr is NOT available. All
   string natives must be byte/ASCII-level (A11). A Unicode-aware op would pass on the Rust backends
   and FAIL the PHP oracle. This is the most likely accidental break for string work.
7. **Float formatting divergence** — already a KNOWN_ISSUE: irrational floats (`sqrt(2.0)`) diverge
   between Rust backends and PHP's 14-digit `echo`. Any dumper/CSV/JSON path that prints a float must
   route through the existing `__phorj_str`/Ryū floor, and examples must use exactly-representable
   values. (Memory `transpile-no-ini-extensions` + CLAUDE.md.)
8. **gzip/compression headers** — gzip embeds a non-deterministic mtime in its header (called out in
   the prompt). Any compression module (`gzencode`/`gzcompress` = ext-zlib) must zero the timestamp or
   it breaks byte-identity. Likely defer compression entirely (ext-zlib may be absent under `-n` too).
9. **Regex engine semantics** — std Rust has no regex; a hand-rolled engine that disagrees with PCRE on
   ANY construct (char classes, greediness, Unicode) is a silent break (A9). The trap is *partial*
   agreement — a subset that works for simple patterns but diverges on an edge case neither side
   intended.
10. **`filter_var` version drift** — PHP's built-in email/URL validation regexes changed across
    versions; transpiling to `filter_var` for parity-gated code is a latent break across PHP 8.5/8.6
    (A10). Own the rules instead.
11. **Hash extension presence** — `crc32/md5/sha1` are core, but `hash('sha256',…)` is ext-hash which
    may not survive `php -n`; verify before relying on it (A4).

---

## RECOMMENDED BUILD ORDER (value × confidence × no-policy-conflict)

1. **A1 `Core.Debug` (dump/inspect)** — highest daily value, fully Tier A, reuses the cyclic
   visited-set, no new Op, owns its format. HIGH confidence.
2. **A3 `Core.Encoding` (base64/hex)** — trivial, all PHP-core transpile, operates on existing
   `Value::Bytes`. HIGH confidence.
3. **A2 `Core.Url`** — high value, injected-type pattern, std-only. HIGH confidence.
4. **A11 `Core.Text` breadth** (padLeft/substr/indexOf/reverse, ASCII-only) — purely additive, PHP-core
   byte functions. HIGH confidence.
5. **A7 `Core.Csv`** — composes with `Core.File`, str_getcsv-core. HIGH confidence.
6. **A5 `Core.Random` (seeded)** — needs a Phorj-owned algorithm + matching PHP helper (the real work
   is parity, not the PRNG). MEDIUM.
7. **A8 `Core.Time` (UTC arithmetic)** — high value but the most determinism traps; numeric-format-only
   first. MEDIUM.
8. **A4 `Core.Hash`** — crc32 only first (md5/sha256 pending the crypto-policy decision). MEDIUM.
9. **A6 `Core.Sql` builder** — useful but lower standalone value until Tier-B execution lands. Defer.
10. **A9 `Core.Regex` / A10 `Core.Validate` (regex-backed)** — only as a Phorj-owned subset; full PCRE
    parity NOT feasible std-only. Design-first, LOW confidence on parity.

**Decisions the human must make:** (1) crypto-hash policy for A4 (default: defer md5/sha256, ship
crc32); (2) regex strategy for A9/A10 (own-dialect subset vs defer); (3) Random functional-vs-mutable
API for A5.
