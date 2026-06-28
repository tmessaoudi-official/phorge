# Prior-Art Sweep — Python stdlib lens (for Phorj `Core.*` native modules)

**Lens:** Python standard library (`urllib`, `http.client`, `sqlite3`/DB-API 2.0, `pprint`/`reprlib`,
`hashlib`, `secrets`/`random`, `csv`, `datetime`, `re`, plus high-value adjacents: `base64`,
`json`, `hmac`, `uuid`, `textwrap`, `string`, `decimal`, `statistics`, `ipaddress`, `collections`).

**Framing first (the determinism partition decides feasibility before usefulness):**
- **Tier A (pure / deterministic → byte-identity-gateable, ships now):** anything whose output is a
  pure function of its inputs with no clock / address / RNG-entropy / locale / set-iteration-order
  dependence. These can be `Core.*` natives gated by `tests/differential.rs` (run == runvm == PHP 8.5).
- **Tier B (impure → quarantined behind M6 `Transport`, fixture-tested OUTSIDE differential.rs):**
  network, DB connection, filesystem writes, `time.time()`/`datetime.now()`, OS entropy.

Phorj already ships the relevant skeleton: `src/native/{math,text,convert,decimal,bytes,file,json,
list,map,set,html,reflect,process}.rs`, each a `(module,name)`-keyed table with `eval`
(shared interpreter+VM), a `php` transpile closure, and a `pure: bool`. New pure modules are purely
additive (no new `Op`, `Op::CallNative` path). The gated runtime-helper mechanism
(`__phorj_*` + `uses_*` flags + `emit_runtime_helpers`) is the escape hatch when a single PHP builtin
won't match Phorj semantics byte-for-byte — and it is the *dominant* tool for this batch, because
Python and PHP disagree constantly on edge cases (hex casing, base64 padding, urlencode space, date
parsing). **The safe default for every capability below is: define Phorj's exact semantics in the Rust
`eval`, then emit a `__phorj_*` PHP helper that reproduces it — never trust a bare PHP builtin to
match the Rust kernel unless verified.**

---

## CAPABILITY-BY-CAPABILITY

### 1. Pretty-print / var-dump (`pprint`, `reprlib`, `repr`) — **Tier A, HIGH value**
**Python shape:** `pprint.pformat(obj, indent, width)`, `repr(obj)`, `reprlib.repr` (depth/length
caps). Produces a stable, human-readable, deterministic-by-design rendering of nested structures.

**Phorj API sketch:**
```
import Core.Debug;
Console.println(Debug.dump(value));          // -> string, multi-line, indented
Console.println(Debug.inspect(value));        // -> string, single-line compact (repr-like)
```
**Determinism:** Tier A **iff** the format is *Phorj-defined* and contains **NO** addresses /
object-ids / pointer values (the #1 Python `repr` trap: default `object.__repr__` prints
`<Foo at 0x7f…>`). Phorj's Value enum is closed and ordered: `Map`/`Set` are already
**insertion-ordered `Rc<Vec<…>>`** (per the project's R1 decision — verified in `value.rs`), so
iteration order is stable and gateable, unlike Python `set`/pre-3.7 `dict`. Render: `Int`/`Float`
(Ryū via the existing `__phorj_float`), `Bool` (`true`/`false`), `Str` (quoted+escaped), `Bytes`
(`b"\xHH…"`), `Decimal` (`1.50d`), `Null` (`null`), `List`/`Map`/`Set`/`Instance`/`Enum` recursively.
**Reuse `value::eq_val_rec`'s `visited: Vec<(*const Instance,…)>` cycle-set pattern** for circular-ref
guard once mutation lands (M-mut shipped `Instance` shared-mutable → cycles ARE possible) — print
`<cycle>` instead of looping.
**Transpile target:** NOT `var_export`/`print_r` (PHP renders objects with class FQNs + `[private]`
markers + no Ryū float fidelity — guaranteed divergence). Emit a dedicated `__phorj_dump($v,$indent)`
recursive helper that mirrors the Rust kernel exactly (this is precisely the `__phorj_str` pattern,
one level up). **Risk:** float formatting must route through the SAME Ryū-equivalent helper both sides
(already solved — `__phorj_float`). **Confidence: HIGH.**

### 2. URL parse / build / query (`urllib.parse`) — **Tier A, HIGH value**
**Python shape:** `urlsplit(s) -> SplitResult(scheme,netloc,path,query,fragment)`, `urlunsplit`,
`parse_qs`/`parse_qsl` (query → dict/list), `urlencode(dict)`, `quote`/`unquote` (percent-encoding),
`quote_plus`/`unquote_plus` (space↔`+`).

**Phorj API sketch:**
```
import Core.Url;
Url url = Url.parse("https://h:8080/a/b?x=1&y=2#f");  // -> a Url struct/class
string s   = url.scheme;     // "https"
Map<string,string> q = Url.parseQuery(url.query);
string built = Url.build(scheme, host, port, path, query, fragment);
string enc   = Url.encode("a b/c");   // "a%20b%2Fc"
string plus  = Url.encodeForm("a b"); // "a+b"
```
**Determinism:** fully pure → Tier A. Best modeled with the **injected-type pattern**
(`cli::inject_*_prelude`) for a `Url` result type, OR return a `Map<string,string>` for the query and
plain fields for parse (simpler, no injected type needed for v1). `parseQuery` ordering: Phorj `Map`
is insertion-ordered → gateable; Python `parse_qs` returns lists for repeated keys (decide:
last-wins like PHP `parse_str`, or list — recommend **list of pairs** `List<(string,string)>` to be
lossless, but Phorj has no tuples yet → use `List<KeyVal>` injected type, or two parallel lists).
**Transpile traps (named):**
- PHP `urlencode` encodes space as `+`; `rawurlencode` as `%20`. Python `quote` → `%20`,
  `quote_plus` → `+`. Map `Url.encode`→`rawurlencode`, `Url.encodeForm`→`urlencode`. **Pin which.**
- PHP `parse_url` is lenient and returns `false`/missing keys inconsistently; Python `urlsplit`
  never fails. **Prefer a `__phorj_url_parse` helper** for byte-identity rather than `parse_url`.
- Percent-encoding case: Python uppercases hex (`%2F`); PHP `rawurlencode` also uppercases — verify,
  but pin via helper if any doubt. **Confidence: HIGH.**

### 3. Base64 / hex / base32 encoding (`base64`, `binascii`) — **Tier A, HIGH value**
**Python shape:** `base64.b64encode/b64decode(bytes)`, `urlsafe_b64encode` (`+/`→`-_`),
`binascii.hexlify`/`unhexlify`, `base64.b32encode`.

**Phorj API sketch (operates on the existing `bytes` primitive — strong fit):**
```
import Core.Encoding;
string b64 = Encoding.base64Encode(bytes);
bytes  raw = Encoding.base64Decode(str);        // -> bytes? (invalid input → null)
string hex = Encoding.hexEncode(bytes);          // lowercase, no separator
bytes  b   = Encoding.hexDecode(str);            // -> bytes?
string u   = Encoding.base64UrlEncode(bytes);
```
**Determinism:** fully pure → Tier A. Hand-rollable in Rust std trivially (this is NOT crypto — the
"don't hand-roll crypto/TLS" rule does **not** apply to base64/hex; they are pure encodings).
**Transpile traps (named):**
- **Hex casing:** PHP `bin2hex` → **lowercase**; Python `hexlify` → lowercase. Match → pin lowercase
  in the Rust kernel and emit `bin2hex`. `dechex`/`sprintf('%X')` would diverge — avoid.
- **base64 padding & urlsafe alphabet:** PHP `base64_encode` always pads with `=`; for urlsafe emit
  `strtr(base64_encode($b),'+/','-_')` and decide on padding (Python urlsafe keeps `=`). Pin behavior;
  if mismatch risk, use a `__phorj_b64` helper.
- **Invalid-input handling:** Phorj returns `bytes?` (`null`); PHP `base64_decode($s,true)` returns
  `false` on bad input → map `false`→`null`. **Confidence: HIGH.**

### 4. Hashing (`hashlib`) — **Tier A (digests), HIGH value**; HMAC = Tier A but care
**Python shape:** `hashlib.md5/sha1/sha256(bytes).hexdigest()`, `hmac.new(key,msg,sha256)`.

**Phorj API sketch:**
```
import Core.Hash;
string h = Hash.sha256Hex(bytes);   // 64 lowercase hex chars
string h = Hash.md5Hex(bytes);
string h = Hash.sha1Hex(bytes);
string c = Hash.crc32Hex(bytes);
string m = Hash.hmacSha256Hex(key, msg);
```
**Determinism:** digest of fixed input is fully deterministic → Tier A, byte-identity-gateable.
**Nuance on the "don't hand-roll crypto" rule:** md5/sha1/crc32 are *checksums*, not security
primitives — hand-rolling them in std-only Rust is fine and standard (the prompt explicitly lists
"crc32/md5/sha1/sha256 hand-rolled" as feasible Tier A). sha256/hmac-sha256 ARE security-relevant; the
rule says don't hand-roll *secure-RNG/TLS*, and hashing is a pure, well-specified, test-vector-checkable
algorithm — implementing sha256 from the FIPS spec with NIST test vectors is acceptable and the only
std-only option (no crate). **Mark sha256/hmac confidence MEDIUM** — must validate against published
test vectors in the differential set, and the implementation must be constant-time-irrelevant (these
are not used for password comparison in the language runtime). **Transpile target:** `hash('sha256',
$b)`, `md5($b)`, `sha1($b)`, `sprintf('%08x',crc32($b))`, `hash_hmac('sha256',$m,$k)` — **but `hash()`
needs the `hash` extension which is compiled-in by default (verify under `php -n`!).** If `hash` is
absent under `-n`, md5/sha1/crc32 have dedicated core functions (`md5`,`sha1`,`crc32`) but **sha256
has no core function** → either require the extension or ship a `__phorj_sha256` PHP helper (slow but
deterministic). **This is the single biggest transpile risk in this batch — `php -n` likely lacks a
sha256 builtin. Confidence: HIGH for md5/sha1/crc32, MEDIUM for sha256/hmac (pending `php -n` probe).**

### 5. Random / secrets (`random`, `secrets`) — **SPLIT: seeded=Tier A, system=Tier B**
**Python shape:** `random.Random(seed)` (Mersenne-Twister, reproducible), `random.random()`/
`randint`/`choice`/`shuffle` (module-global, seedable); `secrets.token_bytes` (OS entropy, NOT
reproducible).

**Phorj API sketch:**
```
import Core.Random;
Random rng = Random.seeded(42);           // explicit-seed PRNG (Tier A)
int n      = rng.int(0, 100);
float f    = rng.float();
T item     = rng.choice(list);
List<T> sh = rng.shuffle(list);
// Tier B (M6 Transport-quarantined):
bytes b = Random.systemBytes(16);          // OS entropy — NOT gateable
```
**Determinism:** **seeded PRNG with a Phorj-defined algorithm is Tier A** — but ONLY if the
algorithm is identical in Rust and PHP. Do **NOT** use `random.Random`/Mersenne or PHP `mt_rand`
(MT seeding differs across PHP versions and from any Rust impl → guaranteed divergence). **Define a
small, fully-specified PRNG (e.g. SplitMix64 or PCG32) in the Rust kernel and re-implement the exact
integer arithmetic in a `__phorj_rng_*` PHP helper** using 64-bit ops (PHP ints are 64-bit on the CI
platform — verify, and watch overflow: PHP has no wrapping u64, must mask with `& 0xFFFFFFFFFFFFFFFF`
and use `gmp`/manual — **this is a real trap**). `rng.float()` in `[0,1)` from the integer state must
use the SAME bit-extraction both sides. **System entropy is Tier B** — quarantine. **Confidence:
MEDIUM** (the u64-wraparound parity between Rust and PHP is the hard part; SplitMix64 chosen
specifically because it's all `*` and `xor`/`>>` on u64 and reproducible — but PHP u64 overflow is the
named risk).

### 6. SQL access + query builder (`sqlite3` / DB-API 2.0) — **SPLIT: builder=Tier A, connection=Tier B**
**Python shape:** `sqlite3.connect()` → `cursor.execute(sql, params)` (parameter binding `?`/`:name`).
DB-API 2.0 is the std interface.

**Phorj API sketch:**
```
import Core.Sql;
// Tier A — pure query BUILDER (escaping/binding, no I/O):
Query q = Sql.select(["id","name"]).from("users").where("age > ?", [18]).orderBy("name");
string sql = q.toSql();             // deterministic string
List<Value> binds = q.bindings();
string esc = Sql.quoteIdent("col"); // identifier quoting
string lit = Sql.quoteString("a'b"); // 'a''b' — SQL string escaping
// Tier B — actual connection (M6 Transport):
Connection c = Sql.connect(dsn);    // NOT gateable
```
**Determinism:** **the query BUILDER is pure → Tier A** (this is explicitly in the prompt's feasible
list — "a typed SQL query BUILDER (escaping/binding)"). It produces deterministic SQL strings + a
binding list; no DB needed. The **connection/execution is Tier B**, quarantined behind M6 `Transport`.
**Transpile target for the builder:** the builder is pure Phorj logic over strings/lists — it likely
needs NO natives at all beyond string ops Phorj already has; `quoteString`/`quoteIdent` could be small
natives (`__phorj_sql_quote`). **Trap:** SQL escaping is dialect-specific (`'`→`''` for strings is
ANSI; identifier quoting `"`/backtick/`[]` differs by engine) — pin a single dialect (recommend ANSI +
optional MySQL backtick mode) and gate it. **Confidence: HIGH for builder, the connection is out of
scope until M6.**

### 7. CSV (`csv`) — **Tier A, HIGH value**
**Python shape:** `csv.reader(lines, delimiter, quotechar)`, `csv.writer(...).writerow`. RFC-4180-ish
quoting (double-quote escaping, embedded newlines/commas).

**Phorj API sketch:**
```
import Core.Csv;
List<List<string>> rows = Csv.parse(text);                 // default ',' delim, '"' quote
string out              = Csv.write(rows);                  // RFC-4180 quoting
List<List<string>> rows2 = Csv.parseWith(text, ';', '"');
```
**Determinism:** fully pure → Tier A. Hand-roll the RFC-4180 state machine in Rust (quoted fields,
escaped `""`, embedded delimiters/newlines). **Transpile traps (named):**
- PHP `str_getcsv`/`fgetcsv` exist but have quirky escape semantics (the `escape` char `\` causes
  long-standing surprises, **deprecated/changed in PHP 8.4+** — Rule 9 deprecation flag!). **Do NOT
  rely on `str_getcsv` for byte-identity** — emit `__phorj_csv_parse`/`__phorj_csv_write` helpers
  that mirror the Rust state machine exactly. This avoids the PHP 8.4 escape-default change entirely.
- Line-ending normalization (CRLF vs LF) must be pinned identically both sides.
**Confidence: HIGH** (with the explicit decision to use helpers, not `str_getcsv`).

### 8. Date / time (`datetime`, `time`, `calendar`) — **SPLIT: arithmetic/format/parse=Tier A, now()=Tier B**
**Python shape:** `datetime(y,m,d,h,m,s)`, `timedelta`, `.strftime`/`strptime`, `date.today()`/
`datetime.now()` (clock), `time.time()` (epoch).

**Phorj API sketch:**
```
import Core.Time;
// Tier A — pure arithmetic/format/parse:
Instant t  = Time.fromUnix(1_700_000_000);     // explicit epoch seconds in
Instant t2 = t.addSeconds(3600);
string s   = Time.format(t, "%Y-%m-%d %H:%M:%S");   // UTC only (locale-free)
Instant p  = Time.parse("2026-06-26", "%Y-%m-%d");  // -> Instant?
int dow    = Time.dayOfWeek(t);
// Tier B (M6 Transport):
Instant n  = Time.now();                        // clock — NOT gateable
```
**Determinism:** **arithmetic/format/parse over an EXPLICIT instant is Tier A** (in prompt's feasible
list). The hard rule: **UTC only, locale-free, no `%Z`/`%a`/`%b` locale-dependent specifiers in the
gateable set** (those are the classic determinism traps — Python `strftime('%a')` depends on
`LC_TIME`). `now()`/`today()`/`time.time()` are Tier B. **Transpile traps (named & severe):**
- PHP `date()`/`DateTime` default to the **`date.default_timezone` ini setting** (non-deterministic
  across environments, and **`php -n` resets it** → may emit warnings / default to UTC). **MUST pass
  explicit UTC** (`gmdate()` not `date()`, or `DateTimeImmutable` with explicit `new DateTimeZone('UTC')`).
- Python and PHP strftime/`date()` use **different format-specifier alphabets** (`%Y` vs `Y`,
  `%m` vs `m`). Phorj should pick ONE (recommend `strftime`-style `%Y` since it's the prompt's lens)
  and **translate to PHP's `date()` letters in the transpile helper** (`__phorj_time_format`).
- Leap-year / month-length arithmetic: hand-roll in Rust (std has no date type), mirror in a PHP helper
  rather than trusting `DateTime` modify-string parsing. **Confidence: MEDIUM** (calendar arithmetic is
  fiddly; the timezone-default trap is the #1 named risk and is fully avoidable with `gmdate`/explicit UTC).

### 9. HTTP client (`urllib.request`, `http.client`, `requests`) — **Tier B, deferred to M6**
**Python shape:** `urllib.request.urlopen(url)`, `requests.get(url)` → response (status, headers, body).
**Determinism:** **CANNOT be Tier A** — network is non-deterministic AND **Rust std has no HTTP client
and no TLS** (the prompt's hard constraint: no TLS in std, breaks zero-dep). This is the canonical
Tier B capability: quarantined behind the M6 `Transport` trait, fixture-tested outside `differential.rs`,
transpiled to PHP (`file_get_contents`/cURL — but cURL ext may be absent under `php -n`). **The portable
unit per M6 design is `handle(Request)->Response` at the VALUE level; the socket bridge is runtime glue,
not transpiled 1:1.** The pure `Request`/`Response` *value* construction (M6 W1, already shipped) IS
gateable; the actual fetch is not. **Confidence: HIGH that it's Tier B / M6-deferred** (matches existing
M6 direction in CLAUDE.md).

### 10. Validation / filtering (`re`-based validators, PHP `filter_var` analog) — **Tier A, MEDIUM value**
**Python shape:** no single module — idioms via `re` (email/URL regex), `int()`/`float()` with
try/except, `ipaddress.ip_address(s)`. PHP's `filter_var` is the closer analog (the project is
PHP-targeted).
**Phorj API sketch:**
```
import Core.Validate;
bool ok    = Validate.isEmail(s);
int? n     = Validate.toInt(s);          // strict parse, null on failure
bool ip    = Validate.isIpv4(s);
string san = Validate.trimAll(s);
```
**Determinism:** pure → Tier A. **Transpile:** PHP `filter_var($s, FILTER_VALIDATE_EMAIL)` exists but
the `filter` extension may be absent under `php -n` (**named risk**) and its email regex is RFC-5322-ish
and notoriously divergent from any hand-rolled check → **prefer hand-rolled validators with a
`__phorj_validate_*` helper** to guarantee byte-identity. `toInt`/`toFloat` already partly exist
(`Text.parseInt`/`parseFloat`, `Core.Convert`). **Confidence: MEDIUM** (regex-equivalence is the trap —
avoid by defining simple, explicit grammars, not full RFC parsers).

### 11. Regular expressions (`re`) — **Tier A in principle, but HIGH-risk / recommend DEFER**
**Python shape:** `re.match`/`search`/`findall`/`sub`/`split`, capture groups, named groups, flags.
**Determinism:** a regex match is pure → Tier A *in principle*. **But:** Rust std has **NO regex
engine** (the `regex` crate is forbidden by zero-dep). Hand-rolling a regex engine that is
**byte-identical to PHP PCRE** (which IS core, available under `php -n`) is a massive undertaking and
PCRE has hundreds of edge cases (backreferences, lookahead, Unicode property classes, greedy/lazy
nuances) — **getting Rust to match PCRE exactly is effectively infeasible at reasonable cost.**
**Recommendation: DEFER regex** (or ship only a tiny, explicitly-non-PCRE glob/wildcard matcher with
Phorj-defined semantics so there's nothing to diverge from). If full regex is wanted later, the only
byte-identity-safe path is to make it **transpile-only / PHP-PCRE-backed and run-backend = a hand-rolled
subset**, which violates the spine. **Named trap: there is no std-only Rust regex; PCRE-parity is the
killer.** **Confidence: HIGH that regex should be deferred/scoped-down.**

### 12. UUID (`uuid`) — **SPLIT: v4=Tier B (entropy), v5/v3=Tier A (namespaced hash)**
**Python shape:** `uuid.uuid4()` (random), `uuid.uuid5(ns, name)` (sha1-based, deterministic).
**Determinism:** `uuid4` needs entropy → Tier B. **`uuid5`/`uuid3` are pure deterministic hashes →
Tier A** (depend on the sha1/md5 kernel from §4). **Phorj API:** `Uuid.v5(namespace, name) -> string`.
**Transpile:** compose from the hash helpers; format with the version/variant bit-twiddling in a
`__phorj_uuid5` helper. **Confidence: MEDIUM** (depends on hash kernel landing first; v4 deferred).

### 13. Text wrapping / templating (`textwrap`, `string.Template`) — **Tier A, LOW-MEDIUM value**
`textwrap.fill`/`wrap`/`dedent`/`indent`, `string.Template.substitute`. All pure → Tier A, hand-rollable.
Phorj already has `Core.Text` (`split`/`join`/`trim`/`repeat`/etc.) and `Core.Html`. Adds: `Text.wrap`,
`Text.dedent`, `Text.pad`. **Transpile:** `wordwrap()` (core), `str_pad()` (core). **Confidence: HIGH.**

### 14. Statistics (`statistics`) — **Tier A, LOW value**
`mean`/`median`/`stdev`. Pure but **float-determinism trap**: summation order and float rounding must be
identical Rust↔PHP (and PHP `echo`'s 14-digit float printing diverges from Ryū — already a logged
KNOWN_ISSUE). Recommend integer/decimal-only stats first, or restrict examples to exactly-representable
values. **Confidence: MEDIUM** (float-echo divergence is a pre-existing, documented trap).

---

## DETERMINISM TRAPS — consolidated (named, with the surface they hit)
1. **Object addresses / ids** — Python default `repr` prints `0x…`; any dumper MUST be Phorj-defined,
   address-free. (Hits: var-dump §1, uuid, anything reflecting identity.)
2. **Clock** — `time.time()`, `datetime.now()`, `date.today()` → Tier B. Only EXPLICIT instants are
   gateable. (Hits: time §8, uuid v1.)
3. **OS entropy / RNG** — `secrets`, `os.urandom`, `random` *without* explicit seed, `uuid4` → Tier B.
   Seeded PRNG is Tier A only if the algorithm is identical both sides. (Hits: random §5, uuid §12.)
4. **Set / dict iteration order** — Python `set` is unordered; pre-3.7 dict unordered. Phorj `Map`/`Set`
   are **insertion-ordered `Rc<Vec>`** (verified) → safe, but any dumper/CSV/JSON must rely on that order
   and never re-sort via a hashmap. (Hits: var-dump §1, csv §7, sql §6.)
5. **Locale** — `strftime('%a'/'%b'/'%A')`, `LC_TIME`, `LC_NUMERIC` (decimal comma!), `strtolower` on
   non-ASCII. Gateable set must be locale-free / UTC / ASCII-only. (Hits: time §8, text-case ops.)
6. **PHP `date.default_timezone` ini** — non-deterministic env setting; `php -n` resets it. Use `gmdate`/
   explicit `DateTimeZone('UTC')`. (Hits: time §8 — the single most dangerous one.)
7. **Float rendering divergence** — PHP `echo`/`json_encode` emit ~14 sig-digits, NOT Ryū → any
   irrational/non-exactly-representable float diverges from the Rust backends (already KNOWN_ISSUE).
   Route ALL float output through `__phorj_float` (Ryū). (Hits: stats §14, hash-of-float, json.)
8. **Hex casing** — `bin2hex`/`hexlify` lowercase vs `dechex`/`sprintf('%X')` upper. Pin lowercase.
9. **base64 padding & urlsafe alphabet** — `=` padding, `+/` vs `-_`. Pin explicitly. (§3.)
10. **u64 overflow in a seeded PRNG** — Rust wraps, PHP int overflow promotes to float / needs manual
    masking. The named blocker for byte-identical RNG. (§5.)
11. **gzip header timestamp** (prompt-flagged) — any compression module's container carries an mtime;
    must be zeroed for byte-identity. (Future `Core.Compress`.)
12. **`php -n` missing extensions** — `hash` (sha256!), `filter`, `mbstring`, `curl`, `intl`, possibly
    `ctype` are absent or uncertain under `-n`. Every transpile target MUST be a CORE function or a
    `__phorj_*` helper. **Probe `php -n` for `hash`/`filter`/`ctype` before relying on them.** (§4,§10.)

## STD-RUST APIs RELIED ON (all std-only, zero-crate — verified the dep list is empty)
- `str`/`String`/`char` (UTF-8 iteration), `[u8]`/`Vec<u8>` (bytes) — text/url/base64/csv/hash input.
- `u8`/`u32`/`u64`/`i64`/`i128` wrapping & checked arithmetic — hashing, PRNG, hex, base64, decimal.
- Manual byte/bit ops — base64 (6-bit groups), hex (nibbles), sha256/md5 (FIPS spec), crc32 (table).
- `core::fmt` / existing Ryū `__phorj_float` — pretty-print & stats float rendering.
- NO `std::time::SystemTime`, NO `std::net`, NO `getrandom`/`/dev/urandom` in the Tier-A set — those
  belong only to Tier B (M6 Transport).
- Reuse `value::eq_val_rec`'s `Vec<(*const Instance,…)>` cycle-set for the dumper's circular guard.

## RECOMMENDED BUILD ORDER (pure first, dependency-aware)
1. **`Core.Encoding`** (base64/hex) — trivial, no deps, unlocks hashing output & uuid. HIGH conf.
2. **`Core.Hash`** (md5/sha1/crc32 first → HIGH; sha256/hmac after `php -n` hash probe → MEDIUM).
3. **`Core.Url`** (parse/build/query) — HIGH conf, pure, high value.
4. **`Core.Csv`** — HIGH conf, helper-based (avoid PHP 8.4 `str_getcsv` change).
5. **`Core.Debug`** (var-dump/inspect) — HIGH conf, reuses Ryū + cycle-set.
6. **`Core.Sql`** (query builder only; connection → M6) — HIGH conf.
7. **`Core.Time`** (explicit-instant arithmetic/format/parse, UTC-only) — MEDIUM (timezone trap).
8. **`Core.Random`** (seeded PRNG, SplitMix64) — MEDIUM (u64 overflow parity).
9. **`Core.Validate`** — MEDIUM (avoid filter ext / regex equivalence).
10. **DEFER:** regex (no std engine, PCRE-parity infeasible), HTTP (Tier B/M6), uuid4/system-random
    (Tier B), full statistics (float-echo trap), compression (gzip mtime trap).

## NOTABLE-FOR-PHORJ SUMMARY
- The dominant porting tactic is **define-in-Rust-kernel + `__phorj_*` PHP helper**, NOT trusting PHP
  builtins — because Python↔PHP↔Rust disagree on exactly the edge cases (hex case, b64 padding, date
  specifiers, csv escaping, urlencode space) that byte-identity gating will catch.
- Most capabilities need **no new `Op`** (pure `Op::CallNative`), so they ship like existing modules.
- The `pure: bool` field + the Tier-A/Tier-B partition map 1:1 onto the differential gate: pure natives
  go in the glob-gated examples; impure ones get fixture tests outside `differential.rs`.
- `php -n` extension absence (esp. **sha256 has no core PHP function**) is the highest-leverage unknown
  to resolve before committing the hashing module's transpile strategy.
