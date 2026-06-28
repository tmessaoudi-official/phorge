# Prior-Art Sweep — Go stdlib lens for Phorj native modules

**Lens:** Go's standard library (`net/url`, `net/http`, `database/sql`, `fmt %#v`/`spew`, `crypto/*`,
`encoding/*`, `math/rand`, `time`, `regexp`, `strconv`, `sort`, `unicode`).
**Goal:** enumerate the std-library / well-known-library *capabilities* that are candidates for a Phorj
`Core.*` native module, classify each on the **determinism partition** (Tier A = pure / byte-identity-gateable
vs Tier B = impure / quarantined behind the M6 `Transport` trait), and call out determinism traps and the
exact PHP transpile targets (under `php -n`, PHP 8.5 floor).

Confidence is graded per capability. "Std Rust API" = the `std` surface the Phorj native body would lean
on (no external crates — `[dependencies]` is empty).

---

## 0. What Phorj already ships (baseline, so we don't propose duplicates)

Registry modules present today (`src/native/`): `Core.Console`, `Core.Bytes`, `Core.Convert`,
`Core.Decimal`, `Core.Env`, `Core.File`, `Core.Html`, `Core.Json`, `Core.List`, `Core.Map`, `Core.Math`,
`Core.Process`, `Core.Reflect`, `Core.Set`, `Core.Text`.

So: **JSON, math, text, list/map/set, bytes, html-escaping, reflect, decimal, file-read, env/process,
type-convert are DONE.** The gaps the Go lens surfaces are the encoding/hashing/url/random/regex/csv/time/
http/sql cluster. Those are what this sweep enumerates.

---

## 1. Pretty-print / structured dump — `fmt %#v` / `%+v` / `spew`

**Go shape:** `fmt.Sprintf("%#v", x)` (Go-syntax repr), `%+v` (field names), `spew.Sdump` (deep, with
pointers/addresses). PHP analogues: `var_dump`, `var_export`, `print_r`.

**Phorj proposal — `Core.Debug`:**
```
import Core.Debug;
Console.println(Debug.dump(value));      // dump(T) -> string   (deterministic, Phorj-defined format)
Console.println(Debug.inspect(value));   // alias / compact one-line form
```
API sketch — a generic `dump<T>(T x) -> string` (erased generic, like `id<T>`), returning a
**Phorj-defined deterministic textual format** — NOT PHP's `var_dump` (which leaks `#object-id` and is
not stable across runs). Recurse over the closed `Value` enum: `Int`→`5`, `Float`→Ryū-canonical (reuse
`__phorj_float`), `Str`→quoted+escaped, `Bool`, `Null`→`null`, `List`→`[a, b]`, `Map`→`{k: v}`
(insertion order — already preserved in `Value::Map`), `Set`→`{a, b}`, `Instance`→`ClassName { field: v }`
(fields in declaration order via the checker's class table, NOT runtime hash order), `Enum`→`Variant(payload)`,
`Bytes`→`b"\xNN…"`, `Decimal`→`1.50d`, `Closure`→`<closure>` (no address!).

**Purity: PURE** — *if and only if* the format is Phorj-defined and address-free. This is the single
most important design call: Go's `%p`/`spew` print pointer addresses (non-deterministic). Phorj must
**never** print an address or object id. With that rule it is fully byte-identity-gateable.

**Determinism traps (named):**
- **Object identity / addresses** — must be omitted entirely (no `Closure@0x…`, no `#1` object ids).
- **Map/Set iteration order** — already solved: Phorj's `Value::Map`/`Set` are insertion-ordered
  `Rc<Vec<…>>` (Map discipline R1). Use that order, never a `HashMap` walk.
- **Struct field order** — use the *declaration order* from the checker's class table, not runtime order.
- **Float formatting** — reuse the existing Ryū `__phorj_float` helper so Rust backends and PHP agree
  (PHP `echo`'s 14-digit default would diverge — this is the documented irrational-float issue).
- **Circular references** — `Value::eq_val_rec` already maintains a visited-set; reuse the same pattern
  in the dumper so a self-referential `Instance` (possible since M-mut introduced shared-mutable
  instances) prints `<cycle>` instead of recursing forever.

**Std Rust API:** none beyond `String` formatting; pure walk over `Value`.

**PHP transpile target:** **not** `var_dump` (non-deterministic, wrong format). Emit a gated runtime
helper `__phorj_dump($x)` (the `uses_dump` bool + `emit_runtime_helpers` pattern) that reproduces the
exact Phorj format in PHP — a recursive switch on `gettype`/`is_*` with `instanceof` for objects and
`(array)$obj` for field extraction in declaration order. This is the correct call because no PHP builtin
matches the Phorj format byte-for-byte.

**Confidence: HIGH** (mechanism fully understood; the only design risk is nailing the format spec, which
is a one-time decision). **Recommend: SHIP EARLY — highest value-per-effort, no new Op, pure.**

---

## 2. URL parse + build + query — Go `net/url`

**Go shape:** `url.Parse(s) (*URL, error)`, fields `Scheme/Host/Path/RawQuery/Fragment`,
`url.Values` (map) with `.Get/.Set/.Encode()`, `url.QueryEscape`/`QueryUnescape`,
`url.PathEscape`/`PathUnescape`.

**Phorj proposal — `Core.Url`:**
```
import Core.Url;
Url Url.parse(string s) -> Url?              // null on malformed
string Url.build(Url u) -> string
string Url.encode(string s) -> string        // percent-encode (query component)
string Url.decode(string s) -> string?       // null on bad %XX
Map<string,string> Url.parseQuery(string q)  // a=1&b=2 -> {a:1, b:2}
string Url.buildQuery(Map<string,string> m)  // sorted-by-key for determinism
```
`Url` is an **injected type** (`cli::inject_*_prelude` pattern, like `Core.Json`'s `Json` enum) — a
Phorj class with `scheme/host/port?/path/query/fragment` fields, gated on `import Core.Url`. `parse`
returns `Url?`.

**Purity: PURE.** Parsing/encoding is a deterministic string transform.

**Determinism traps:**
- **Query map ordering** — `buildQuery` must **sort by key** (Go's `url.Values.Encode()` does exactly
  this: "sorted by key"). Phorj `Map` is insertion-ordered, but a *built* query from a map literal
  should canonicalize so two equal maps produce identical bytes. Decision needed: sort-by-key (Go) vs
  preserve-insertion (Phorj Map default). **Recommend sort-by-key** to match Go and guarantee stability.
- **Percent-encoding case** — `%2F` vs `%2f`: pick uppercase hex (RFC 3986 / Go convention) and pin it,
  because PHP's `rawurlencode` already produces uppercase — they must agree.
- **Reserved-char set** — `urlencode` (PHP, `+` for space, form style) vs `rawurlencode` (PHP, `%20`,
  RFC 3986). Go's `QueryEscape` uses `+` for space; `PathEscape` uses `%20`. **Pick one and document.**
  Recommend `rawurlencode`/`%20` (RFC, unambiguous) for `Url.encode`, and a separate form-encode if needed.

**Std Rust API:** pure string scanning; no `std::net` needed (parsing only, no resolution). Hand-roll the
percent codec (trivial — hex of bytes).

**PHP transpile target:** `Url.encode` → `rawurlencode($s)`; `decode` → `rawurldecode` (but bad-`%XX`
detection needs a helper to return null — PHP's `rawurldecode` is lenient, so a `__phorj_urldecode`
gated helper validating `%[0-9A-Fa-f]{2}` is safer for the `string?` contract). `parseQuery` →
`parse_str($q, $out)` (but `parse_str` mangles keys with `[]`/dots — a `__phorj_parse_query` helper
splitting on `&`/`=` is more faithful and deterministic). `buildQuery` → `http_build_query` with
`ksort` first (or a helper) — note `http_build_query` uses `urlencode` (+) form by default, so a helper
is again the byte-safe path. `parse`/`build` → `parse_url` is **partial** (no rebuild, drops empty
components) → emit a `__phorj_url_*` helper set.

**Confidence: HIGH** on the codec, **MEDIUM** on the build/parse-url round-trip (PHP's `parse_url` quirks
mean a helper is mandatory, not a thin map). **Recommend: SHIP — pure, high-value, no new Op.**

---

## 3. base64 / hex encoding — Go `encoding/base64`, `encoding/hex`

**Go shape:** `base64.StdEncoding.EncodeToString(b)`, `.DecodeString(s) ([]byte, error)`,
`base64.URLEncoding`, `base64.RawStdEncoding` (no padding); `hex.EncodeToString`, `hex.DecodeString`.

**Phorj proposal — `Core.Encoding`:**
```
import Core.Encoding;
string Encoding.base64Encode(bytes b) -> string
bytes  Encoding.base64Decode(string s) -> bytes?      // null on invalid
string Encoding.base64UrlEncode(bytes b) -> string    // URL-safe alphabet, no pad
bytes  Encoding.base64UrlDecode(string s) -> bytes?
string Encoding.hexEncode(bytes b) -> string
bytes  Encoding.hexDecode(string s) -> bytes?
```
Operates on `bytes` (M6 W0 already shipped the `bytes` primitive + `Core.Bytes`). Natural fit.

**Purity: PURE.** Pure byte transform.

**Determinism traps:**
- **Hex case** — lowercase (Go `hex` default, PHP `bin2hex` default). Pin lowercase.
- **base64 alphabet + padding** — standard vs URL-safe (`+/` vs `-_`), padding `=` vs raw. Two explicit
  variants (Std + URL-no-pad) as above; do NOT auto-detect on decode.
- **Whitespace tolerance on decode** — Go is strict; PHP `base64_decode($s, true)` (strict mode) rejects
  invalid. Use strict on both sides so `bytes?` null-on-invalid is byte-identical.

**Std Rust API:** none — hand-roll (base64 and hex are ~30 lines each; the prompt explicitly allows
hand-rolling encodings, only crypto/TLS/secure-RNG are off-limits).

**PHP transpile target:** `base64Encode` → `base64_encode($b)`; `base64Decode` →
`(($t=base64_decode($s,true))===false?null:$t)` (strict). URL-safe → `strtr` over the standard result
(`+/`↔`-_`, strip `=`) + reverse on decode, via a small helper. `hexEncode` → `bin2hex($b)`;
`hexDecode` → `(ctype_xdigit($s)&&strlen($s)%2===0 ? hex2bin($s) : null)` (hex2bin warns on odd/invalid;
guard it for the `bytes?` contract). All in PHP **core** (no ext) — safe under `php -n`.

**Confidence: HIGH.** **Recommend: SHIP — pure, no new Op, composes with the existing `bytes` primitive.**

---

## 4. Hashing — Go `crypto/md5`, `crypto/sha1`, `crypto/sha256`, `hash/crc32`, `hash/fnv`

**Go shape:** `sha256.Sum256(b) [32]byte`, `md5.Sum`, `sha1.Sum`, `crc32.ChecksumIEEE(b) uint32`,
`fnv.New64a()`. PHP: `hash('sha256', $s)`, `md5`, `sha1`, `crc32`.

**Phorj proposal — `Core.Hash`:**
```
import Core.Hash;
string Hash.sha256(bytes b) -> string     // 64-hex lowercase
string Hash.sha1(bytes b) -> string       // 40-hex
string Hash.md5(bytes b) -> string        // 32-hex
int    Hash.crc32(bytes b) -> int         // IEEE, u32 -> i64
int    Hash.fnv1a64(bytes b) -> int       // already have fnv1a_64 in bundle::cross!
```

**Purity: PURE.** Hashes are deterministic functions of input bytes — perfect byte-identity candidates
(the prompt explicitly lists "hashing (crc32/md5/sha1/sha256 hand-rolled)" as Tier A feasible).

**CRITICAL constraint (named):** these are *non-cryptographic-use* hashes here (checksums, content
addressing). The prompt forbids hand-rolling **crypto/TLS/secure-RNG**. md5/sha1/sha256 *algorithms* are
publicly specified and deterministic, so hand-rolling the **digest** is fine for *checksum/content-hash*
use — but they MUST NOT be marketed as security primitives (no HMAC-for-auth, no password hashing — that
is M-Batteries/M6 Tier B territory needing a real audited impl). **Recommend: ship sha256/crc32/fnv1a64
for content-addressing; document "not for security" prominently;** defer HMAC/bcrypt/argon2 to a Tier-B
crypto module that transpiles to PHP `password_hash`/`hash_hmac` and is fixture-tested, never hand-rolled.

**Determinism traps:** none for the digest itself (deterministic by definition). Trap is *endianness* in
crc32/fnv output→int conversion — pin the byte order. crc32 is u32; map to `i64` (Phorj has no unsigned).

**Std Rust API:** none — hand-roll the digest loops (sha256/crc32 are standard public algorithms).
`fnv1a_64` already exists in `src/bundle/cross.rs` — **reuse it**.

**PHP transpile target:** `sha256` → `hash('sha256',$b)`; `sha1`→`sha1($b)`; `md5`→`md5($b)`;
`crc32`→`crc32($b)` (returns int). These are PHP **core** (the `hash` extension is compiled-in by default,
but verify it survives `php -n` — `hash()` is bundled since 7.2 and is *not* a loadable ext anymore, so
it's present; sha1/md5/crc32 are always core). fnv1a64 → a small helper (no PHP builtin).
**Determinism-trap on the oracle:** confirm `hash()` is available under `-n` (it is core since 7.2, but
this needs a one-line verification test like the BCMath-under-`-n` lesson).

**Confidence: HIGH** on crc32/fnv/sha256-as-checksum; **MEDIUM** on the security-boundary messaging.
**Recommend: SHIP content hashes; carve crypto-auth into Tier B.**

---

## 5. Random — Go `math/rand` (seeded) vs `crypto/rand` (secure)

**Go shape:** `math/rand` — `rand.New(rand.NewSource(seed))`, `.Intn(n)`, `.Float64()`, `.Shuffle`.
`crypto/rand.Read` — secure, non-deterministic. PHP: `mt_rand`/`mt_srand` (seedable, deterministic
sequence), `random_int` (CSPRNG, non-deterministic).

**Phorj proposal — `Core.Random` (SEEDED only, Tier A):**
```
import Core.Random;
Rng Random.seeded(int seed) -> Rng           // injected Rng type, holds state
int Rng.nextInt(Rng r, int n) -> int         // [0, n)  — but Rng is immutable in Phorj!
```
**This is the hard one.** Phorj's heap is immutable+acyclic by default; a PRNG is inherently *stateful*.
Two viable shapes:
- **(a) Explicit state-threading (functional):** `Random.next(int state) -> (int, int)` returning
  `(value, newState)` — pure, byte-identity-gateable, but awkward ergonomics.
- **(b) Mutable Rng instance:** since M-mut shipped shared-mutable `Instance`, an `Rng` class with a
  mutable `state` field and `r.nextInt(n)` mutating it is now *possible* — and deterministic given a seed.

**Purity: PURE iff seeded** (the prompt lists "SEEDED random" as Tier A feasible). A seeded LCG/xorshift
produces an identical sequence on every backend → byte-identity-gateable. **Unseeded / secure random is
Tier B** (non-deterministic → quarantined, transpiles to PHP `random_int`, fixture-tested only).

**Determinism traps (the central risk):**
- **The PRNG algorithm must be byte-identical to its PHP transpile.** This is the killer: PHP's `mt_rand`
  is a *specific* Mersenne-Twister variant; matching it bit-for-bit in Rust is fragile. **Recommend:
  Phorj defines its OWN documented PRNG** (a simple xorshift64 or PCG), hand-rolled identically in the
  Rust backends AND emitted as a `__phorj_rng_*` PHP helper — do NOT delegate to PHP's `mt_rand`
  (different algorithm → divergence). Then `seed → sequence` is identical across all three legs.
- **Float generation** — `nextFloat` must use the same mantissa-construction on both sides (reuse the
  bit-pattern approach, not a divide that rounds differently). Or restrict to int output initially.
- **`shuffle`/`sample`** — deterministic given seed+algorithm; same PRNG-must-match constraint.

**Std Rust API:** none — define the PRNG with plain integer arithmetic (`wrapping_mul`, `^`, `>>`).

**PHP transpile target:** a gated `__phorj_rng_next($state)` helper implementing the *same* xorshift —
NOT `mt_rand`. Seeded-random is the textbook case where the runtime-helper approach (not a builtin map)
is mandatory.

**Confidence: MEDIUM** (feasible and Tier A, but only with a *Phorj-defined* PRNG; the temptation to map
to `mt_rand` is the trap). **Recommend: ship a documented xorshift `Core.Random` with explicit seed;
defer secure random to Tier B.**

---

## 6. SQL access + query builder — Go `database/sql`

**Go shape:** `sql.Open(driver, dsn)`, `db.Query`, `db.Exec`, `rows.Scan` (impure — real DB);
query *building* is usually a third-party builder (squirrel) — pure string assembly with placeholder
binding.

**Phorj — SPLIT cleanly on the partition:**
- **Tier A — `Core.Sql` query BUILDER (pure):** the prompt explicitly lists "a typed SQL query BUILDER
  (escaping/binding)" as Tier A feasible.
  ```
  import Core.Sql;
  Query Sql.select(List<string> cols) -> Query
  Query Query.from(Query q, string table) -> Query
  Query Query.where(Query q, string col, string op, Value v) -> Query   // parameterized
  (string, List<Value>) Query.build(Query q)   // -> ("SELECT … WHERE x = ?", [binds])
  string Sql.quoteIdent(string id) -> string   // backtick/escape an identifier
  string Sql.escapeString(string s) -> string  // for display/logging, NOT for injection-safety
  ```
  Produces a parameterized SQL string + a bind list. **Pure** — pure string assembly, byte-identity-gateable.
  **Determinism traps:** identifier-quoting dialect (MySQL backtick vs Postgres double-quote vs ANSI) —
  pick a default dialect and make it explicit; placeholder style (`?` positional vs `$1` numbered vs
  `:named`) — pick one and document. Sorting of WHERE/columns must preserve insertion order (it's a
  builder, order is semantic).
- **Tier B — actual DB connection/execution:** non-deterministic (network, server state, result ordering
  unless `ORDER BY`). **Quarantined** behind the M6 `Transport`-style trait, transpiles to PDO
  (`new PDO($dsn)`, `$stmt->execute($binds)`), fixture-tested OUTSIDE `differential.rs`. PDO is PHP core
  but the *drivers* (pdo_mysql) are extensions → absent under `php -n` → cannot be in the oracle anyway,
  reinforcing Tier B.

**Std Rust API:** Tier A needs none (string building). Tier B would need `std::net::TcpStream` + a
hand-rolled wire protocol — **out of scope / deferred** (huge, and the byte-identity spine forbids it).

**PHP transpile target:** builder → pure string/array assembly (emit the SQL + binds as PHP). Tier B →
PDO calls.

**Confidence: HIGH** on the builder (pure, clear value), **N/A** on live DB (correctly Tier B, deferred).
**Recommend: SHIP the query builder; explicitly defer live DB to M6/M-Batteries Tier B.**

---

## 7. CSV — Go `encoding/csv`

**Go shape:** `csv.NewReader(r).ReadAll() ([][]string, error)`, `csv.NewWriter(w).WriteAll(rows)`.
Pure transform between text and `[][]string`.

**Phorj proposal — `Core.Csv`:**
```
import Core.Csv;
List<List<string>> Csv.parse(string s) -> List<List<string>>?   // null on malformed quoting
string Csv.format(List<List<string>> rows) -> string
```

**Purity: PURE** (prompt lists CSV as Tier A feasible). Text↔rows transform.

**Determinism traps:**
- **Quoting / escaping** — RFC 4180: fields with `,`/`"`/newline get quoted, `"`→`""`. Pin RFC 4180.
- **Line terminator** — `\r\n` (RFC 4180) vs `\n`. **Pick `\n` for determinism** (CRLF is the spec but
  `\n` avoids platform drift; document the choice).
- **Trailing newline** — emit (or not) consistently.
- **Delimiter** — comma default; if configurable, a param, but default-fixed.

**Std Rust API:** none — hand-roll a small state machine (in-quote / escaped-quote / field-end).

**PHP transpile target:** PHP's `str_getcsv`/`fputcsv` work on file handles and have locale/escape quirks
(the `escape` parameter is deprecated-ish in 8.4+ and changes behavior). **Recommend a `__phorj_csv_*`
helper** implementing the exact RFC-4180 state machine in PHP rather than `str_getcsv`, to guarantee
byte-identity (PHP's CSV functions have well-known edge-case divergences). This is a runtime-helper case.

**Confidence: HIGH** on feasibility, **MEDIUM** on matching a PHP builtin (helper mandatory).
**Recommend: SHIP with a runtime helper, not a builtin map.**

---

## 8. Date / time — Go `time`

**Go shape:** `time.Now()` (impure!), `time.Parse(layout, s)`, `t.Format(layout)`, `t.Add(d)`,
`t.Sub`, `time.Duration`, `t.Unix()`. PHP: `DateTime`, `date()`, `strtotime`, `DateInterval`.

**Phorj — SPLIT on the partition (the prompt lists "date ARITHMETIC/format/parse" as Tier A, but `now()`
is the canonical Tier B example):**
- **Tier A — `Core.Time` arithmetic/format/parse (PURE):**
  ```
  import Core.Time;
  Instant Time.fromUnix(int seconds) -> Instant          // injected Instant type (epoch-based, UTC)
  Instant Time.parse(string fmt, string s) -> Instant?
  string  Time.format(Instant t, string fmt) -> string
  Instant Time.addSeconds(Instant t, int n) -> Instant
  int     Time.toUnix(Instant t) -> int
  int     Time.diffSeconds(Instant a, Instant b) -> int
  ```
  All deterministic given an explicit `Instant` (epoch seconds, **UTC only** to start). Byte-identity-gateable.
- **Tier B — `Time.now()`:** **the canonical determinism trap** (clock). Quarantined, transpiles to PHP
  `time()`, fixture/seam-tested only — exactly like `Core.Process`/`Core.Env` (the `pure:false` precedent
  already exists; `now()` is its natural next member).

**Determinism traps (named, and this module is FULL of them):**
- **Clock (`now`)** — Tier B, non-negotiable.
- **Timezone / locale** — `date()` in PHP respects `date.default_timezone_set` and the runtime TZ; month
  names respect locale. **Recommend: UTC + a fixed C-locale / numeric format ONLY for the Tier A surface.**
  No locale-dependent month/day names in the pure path (those would diverge between Rust and PHP unless
  hardcoded). If named months are wanted, hardcode an English table in both legs.
- **DST / calendar arithmetic** — adding "1 month" is ambiguous (Jan 31 + 1 month?). Restrict Tier A to
  **second-based arithmetic** (`addSeconds`, `diffSeconds`); defer calendar-aware `addMonths` (it needs a
  documented convention matched in PHP — doable but a later slice).
- **Leap seconds** — ignore (Unix-time convention; both legs agree).
- **Format-string dialect** — Go uses reference-time layouts (`2006-01-02`), PHP uses `Y-m-d`, strftime
  uses `%Y`. **Pick ONE** (recommend a small explicit subset, or adopt PHP's `Y-m-d H:i:s` letters since
  the transpile target is PHP — least translation). Parsing must be the inverse and equally pinned.

**Std Rust API:** `std::time::{SystemTime, UNIX_EPOCH, Duration}` for the *Tier B* `now()` only; the Tier A
calendar math is pure integer arithmetic on epoch seconds (civil-from-days / days-from-civil algorithms —
hand-rolled, well-known public formulas). **Do NOT use `std::time` for the pure path** — it gives no
calendar breakdown; implement the date math directly.

**PHP transpile target (Tier A):** `format`/`parse` → `gmdate($fmt, $unix)` / a `DateTime::createFromFormat`
in UTC, but locale/TZ quirks mean a `__phorj_time_*` helper computing civil-from-epoch in pure PHP
integer math is the byte-safe choice. `addSeconds`/`diff` → plain integer arithmetic (trivially identical).
`now()` (Tier B) → `time()`.

**Confidence: MEDIUM** (feasible Tier A, but it's the trap-richest module — TZ/locale/DST/format-dialect
all must be pinned; the *epoch-integer-arithmetic* subset is HIGH confidence, the *formatting* subset is
MEDIUM and helper-dependent). **Recommend: ship the epoch-arithmetic + numeric-format subset first; defer
locale names and calendar-month arithmetic; `now()` is Tier B alongside Process/Env.**

---

## 9. HTTP client — Go `net/http`

**Go shape:** `http.Get(url)`, `http.Client.Do(req)`, `resp.Body`. PHP: `curl_*`, `file_get_contents`
on a URL, `Guzzle`.

**Purity: IMPURE — Tier B, full stop.** Two independent disqualifiers (both named in the prompt):
1. **No TLS in Rust std** — `std` has `TcpStream` but no HTTPS; the zero-dep constraint forbids `rustls`/
   `native-tls`. Plain-HTTP-only would be a crippled, insecure client.
2. **Non-deterministic** — network responses break the byte-identity spine.

**Recommend: DEFER to M6 entirely.** When it lands it is quarantined behind the `Transport` trait,
transpiles to PHP `curl`/`file_get_contents`, and is fixture-tested OUTSIDE `differential.rs`. The portable
unit per the M6 design is `handle(Request)->Response` at the value level — the client is the *outbound*
dual and belongs in the same quarantine. **Do not attempt a hand-rolled HTTPS client** (security + zero-dep
both forbid it). Note: even plain HTTP needs `std::net::TcpStream` (blocking) — fine for a fixture-tested
Tier B, but never in the spine.

**Confidence: HIGH (that it is Tier B and deferred).** **Recommend: explicitly OUT of native-module scope;
M6 Transport-quarantined.**

---

## 10. Validation / filtering — Go (mostly third-party `validator`) / PHP `filter_var`

**Go shape:** no stdlib validator; PHP `filter_var($v, FILTER_VALIDATE_EMAIL|URL|INT|IP)`.

**Phorj proposal — `Core.Validate`:**
```
import Core.Validate;
bool Validate.isEmail(string s) -> bool
bool Validate.isUrl(string s) -> bool
bool Validate.isInt(string s) -> bool
bool Validate.isIpv4(string s) -> bool
int? Validate.toInt(string s) -> int?     // parse-or-null
```

**Purity: PURE** (deterministic string predicates).

**Determinism traps:**
- **The validation *definition* must match PHP's `filter_var` exactly**, or the transpile diverges. PHP's
  email/URL validators are notoriously idiosyncratic (their own grammar, not strict RFC). **This is the
  trap:** a hand-rolled Rust validator and PHP's `filter_var` will disagree on edge cases. **Recommend:
  ship only the *unambiguous* predicates** (`isInt`, `isIpv4` — well-defined) and **defer email/URL**
  validation, OR define them via a `__phorj_validate_*` helper that does NOT delegate to `filter_var`
  (so both legs run the same explicit grammar). Email/URL "validation" is a swamp; the integer/IP subset
  is clean.
- **No regex engine** — without `Core.Regex` (see §11), complex validators are hand-written state machines.

**Std Rust API:** none (string predicates).

**PHP transpile target:** `isInt` → a `__phorj_is_int` helper (PHP `is_numeric`/`ctype_digit` have sign
quirks); `isIpv4` → `filter_var($s, FILTER_VALIDATE_IP, FILTER_FLAG_IPV4) !== false` (well-defined, safe).
Email/URL deferred.

**Confidence: MEDIUM** (clean for int/IP, swampy for email/URL). **Recommend: ship int/IP predicates; defer
email/URL until a pinned grammar is agreed.**

---

## 11. Regex — Go `regexp` (RE2)

**Go shape:** `regexp.MustCompile(pat)`, `re.MatchString`, `re.FindAllStringSubmatch`, `re.ReplaceAllString`.
PHP: `preg_match`/`preg_replace` (PCRE — **core**, survives `php -n`).

**Phorj proposal — `Core.Regex`:**
```
import Core.Regex;
bool        Regex.matches(string pat, string s) -> bool
List<string> Regex.find(string pat, string s) -> List<string>?    // first match + groups
string      Regex.replace(string pat, string repl, string s) -> string
List<string> Regex.split(string pat, string s) -> List<string>
```

**Purity: PURE** (a regex match is a deterministic function of pattern+input).

**THE central trap (and a hard decision):** **Phorj has no regex engine and zero deps.** Two paths:
- **(a) Hand-roll a regex engine** — large (a real NFA/backtracker is a milestone of its own), and matching
  PCRE semantics bit-for-bit (the transpile target) is *extremely* hard (backreferences, lookaround,
  Unicode classes, greedy/lazy edge cases). **High risk of run↔PHP divergence.** Effectively infeasible to
  match PCRE exactly by hand.
- **(b) Define a *restricted, Phorj-specified* regex dialect** (a documented subset — literals, `.`, `*`,
  `+`, `?`, `[]`, `|`, anchors, groups; NO backrefs/lookaround) implemented identically as a small NFA in
  Rust AND as a `__phorj_regex_*` helper in PHP (the helper itself an NFA in PHP, NOT `preg_*`). Then both
  legs run the *same* engine → byte-identical. This is the only byte-identity-safe route, but it's a
  substantial build.
- **(c) Map directly to PCRE (`preg_*`)** — easy transpile, but then the Rust backends have nothing to run
  (no engine), breaking run≡runvm. **Rejected.**

**Recommend: regex is a MILESTONE, not a quick native.** The byte-identity spine makes "just call PCRE"
impossible (the Rust legs need a matching engine). The honest options are (a) a full hand-rolled
PCRE-compatible engine (very large, risky) or (b) a documented restricted dialect with a shared NFA across
all three legs. **Defer to a dedicated `M-Regex` slice; do not slot it into a stdlib wave.** This is the
single most important "looks like a native but isn't" finding.

**Std Rust API:** none (would hand-roll an NFA).

**Confidence: HIGH (that it's a milestone-sized trap, not a thin native).** **Recommend: DEFER, design a
restricted dialect with a shared engine; flag the PCRE-divergence trap explicitly.**

---

## 12. strconv / parsing — Go `strconv`

Already mostly covered by `Core.Text.parseInt/parseFloat` and `Core.Convert`. Go `strconv.Quote` /
`Atoi` / `FormatFloat` map to existing Phorj surface. **Gap spotted:** number *formatting with options*
(thousands separators, fixed precision) — Go `strconv.FormatFloat(f,'f',2,64)`, PHP `number_format`. This
is already flagged in M-NUM-S4 (`number_format`). **Pure**, no new module needed — extend `Core.Math`/
`Core.Text`. Determinism trap: `number_format`'s locale (thousands/decimal separator) — pin to `.`/`,`
explicitly, no locale. **Confidence: HIGH. Recommend: fold into M-NUM-S4 (already planned).**

---

## 13. sort / collections — Go `sort`, `container/*`

Largely covered by `Core.List` (map/filter/reduce shipped). **Gaps:** `List.sort` /
`List.sortBy(cmp)` (higher-order, like the shipped reduce), `List.unique`, `List.contains`, `List.indexOf`.
**Pure.** Determinism trap: **sort stability + comparator determinism** — use a *stable* sort (Rust
`sort_by` is stable; PHP `usort` is stable since 8.0 — both stable on the floor, good), and require a total-
order comparator. **Std Rust API:** `slice::sort_by` (stable). **PHP:** `usort` (stable ≥8.0). **Confidence:
HIGH. Recommend: extend `Core.List` with sort/unique/indexOf — reuses the HigherOrder native machinery.**

---

## 14. encoding/json — already DONE (`Core.Json`), noted for completeness

Go `encoding/json` ≈ Phorj `Core.Json` (parse/stringify, injected `Json` enum). Float-extreme divergence
from native `json_encode` already in KNOWN_ISSUES. No action.

---

## Determinism-partition summary table

| Capability            | Module           | Tier | Pure? | New Op? | PHP target (under `php -n`)            | Confidence |
|-----------------------|------------------|------|-------|---------|----------------------------------------|------------|
| pretty-print/dump     | `Core.Debug`     | A    | yes   | no      | `__phorj_dump` helper (NOT var_dump)  | HIGH       |
| URL parse/build/query | `Core.Url`       | A    | yes   | no      | rawurlencode + `__phorj_url_*` helpers| HIGH/MED   |
| base64/hex            | `Core.Encoding`  | A    | yes   | no      | base64_encode/bin2hex (+strict guards) | HIGH       |
| content hashing       | `Core.Hash`      | A    | yes   | no      | hash()/sha1/md5/crc32 (core)           | HIGH       |
| seeded random         | `Core.Random`    | A    | yes*  | no      | `__phorj_rng` helper (NOT mt_rand)    | MEDIUM     |
| SQL query builder     | `Core.Sql`       | A    | yes   | no      | pure string/array assembly             | HIGH       |
| CSV                   | `Core.Csv`       | A    | yes   | no      | `__phorj_csv_*` helper (NOT str_getcsv)| HIGH/MED  |
| time arithmetic/fmt   | `Core.Time`      | A    | yes   | no      | `__phorj_time_*` integer helpers (UTC)| MEDIUM     |
| number_format         | extend `Core.Math`| A   | yes   | no      | number_format (pinned separators)      | HIGH       |
| list sort/unique      | extend `Core.List`| A   | yes   | no      | usort (stable ≥8.0)                     | HIGH       |
| int/IP validate       | `Core.Validate`  | A    | yes   | no      | FILTER_VALIDATE_IP / helper            | MEDIUM     |
| **regex**             | `M-Regex`        | A**  | yes   | no      | shared NFA helper (NOT preg_*)         | DEFER      |
| **secure random**     | (M6/Batteries)   | B    | no    | no      | random_int — fixture-tested            | DEFER      |
| **email/url validate**| (later)          | A    | yes   | no      | pinned grammar helper                  | DEFER      |
| **HTTP client**       | (M6 Transport)   | B    | no    | no      | curl — fixture-tested                  | DEFER      |
| **live SQL/DB**       | (M6 Transport)   | B    | no    | no      | PDO — fixture-tested                   | DEFER      |
| **time.now()**        | `Core.Time` (B)  | B    | no    | no      | time() — fixture-tested (like Env)     | DEFER/B    |

\* seeded-random is pure ONLY with a Phorj-defined PRNG (xorshift), never PHP `mt_rand`.
\** regex is pure but engine-sized; the trap is matching PCRE — solve with a shared restricted-dialect NFA.

## Cross-cutting findings (the load-bearing insights)

1. **The byte-identity spine inverts the usual "map to a builtin" instinct.** For dump, url-query, csv,
   seeded-random, time-format, and regex, the *easy* transpile (var_dump / parse_str / str_getcsv /
   mt_rand / date / preg_*) is exactly the WRONG choice — PHP's builtins have their own (often
   locale/version-dependent) semantics that won't byte-match a Rust impl. The correct pattern for all of
   these is the **gated runtime-helper** (`uses_* bool` + `emit_runtime_helpers`), where Phorj *defines*
   the format and emits the same algorithm into PHP. This is the dominant design conclusion of the sweep.

2. **Most candidates need NO new Op** — they're `Op::CallNative` natives or injected types
   (`cli::inject_*_prelude`). The Op set is safe.

3. **The partition cleanly sorts everything:** pure transforms (encode/hash/dump/url/csv/sql-build/
   date-math/sort/seeded-rng) are Tier A and shippable now; anything touching clock/network/DB/secure-RNG
   is Tier B behind the M6 `Transport` trait. The `pure: false` precedent (`Core.Process`/`Core.Env`) is
   the template for the few Tier-B members (`time.now`).

4. **Two "trap" capabilities masquerade as natives:** **regex** (engine-sized; can't map to PCRE without
   breaking run≡runvm) and **email/URL validation** (PHP's `filter_var` grammar is unmatchable by hand).
   Both should be explicitly deferred, not slotted into a quick wave.

5. **Crypto boundary:** content-hashes (sha256/crc32/fnv) are fine to hand-roll for content-addressing,
   but HMAC/password-hashing/secure-RNG/TLS must NOT be — they go Tier B and transpile to audited PHP
   builtins (`password_hash`, `hash_hmac`, `random_int`), fixture-tested.

6. **Oracle hygiene:** verify each PHP target survives `php -n` (the BCMath-under-`-n` lesson). `hash()`,
   `base64_*`, `bin2hex`, `rawurlencode`, `usort`, `number_format`, PCRE (`preg_*`) are all **core** and
   survive `-n`; `mbstring`/PDO-drivers do NOT — another reason those routes are avoided/deferred.

## Recommended ship order (Tier A, value-per-effort)

1. `Core.Debug.dump` — highest value, pure, no new Op, fully understood.
2. `Core.Encoding` (base64/hex) — trivial, composes with the existing `bytes` primitive.
3. `Core.Hash` (sha256/crc32/fnv) — content addressing; reuse `bundle::cross::fnv1a_64`.
4. `Core.Url` — parse/build/query (helper-based codec).
5. `Core.Sql` query builder — pure, high developer value.
6. `Core.Csv` — pure, helper-based state machine.
7. extend `Core.List` (sort/unique) + `Core.Math` (number_format, folds into M-NUM-S4).
8. `Core.Time` (epoch-arithmetic + numeric-format subset; `now()` Tier B later).
9. `Core.Random` (seeded xorshift) — after the PRNG dialect is pinned.
10. `Core.Validate` (int/IP only).

Deferred (design milestones, not waves): **`M-Regex`** (shared restricted-dialect NFA), email/URL
validation, and all **Tier B** (HTTP client, live DB, secure crypto, `time.now`) behind the M6 `Transport`
quarantine.
