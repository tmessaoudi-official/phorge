# Track G — Real-world batteries (runtime libs)

## Track summary

Phorj today ships a deliberately thin, std-only stdlib: `Core.Console` (`println` only),
`Core.Math`, `Core.Text`, `Core.File` (`read`/`exists`/`write`), `Core.Bytes`, `Core.Html`,
`Core.List`, `Core.Map`, `Core.Set`. The HTTP **server** value model (`handle(Request) -> Response`)
is in flight under M6. Everything else a "real app" needs — an HTTP **client**, a database access
layer, env/config loading, structured logging, CLI argument parsing, richer file/dir ops, process
spawning, datetime, crypto/hashing, UUID, randomness, base64/hex, compression, and regular
expressions — is **absent**. This is the largest open surface in the whole roadmap and the one most
directly felt by anyone trying to write something more than a demo. Each candidate must map to
idiomatic PHP and erase cleanly, and several break Phorj's two hard constraints: the
**zero-dependency** core (no external crates) and the **byte-identity / determinism spine**
(`run ≡ runvm ≡ real PHP`). Where they do, the M6 quarantine model is the precedent: keep the
non-deterministic / impure surface out of `tests/differential.rs` and behind a Rust-side seam, and
ship it as a native whose `eval` is *not* exercised by the byte-identical example harness (faults
and pure paths can still be tested). The PHP target is the friend here — PHP already has
`hash()`, `random_bytes()`, `base64_encode()`, `preg_*`, `DateTimeImmutable`, `PDO`, `getenv()`,
`json_encode()` etc., so most of these are "wrap a tier-1 PHP builtin + a std-only Rust kernel,"
which is exactly the existing native pattern. The honest verdict: a focused **batteries milestone**
(call it M11+ / a dedicated `Core.*` expansion) should adopt the deterministic, high-leverage
modules now (JSON, regex, datetime-as-value, base64/hex, env, richer Text/File/List, CLI args,
hashing-of-known-input), and **defer the impure ones** (random, UUID, process, HTTP client,
DB, logging-to-stderr) behind the M6-style quarantine where determinism forbids the spine.

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|----|-------|------|-----|-----|-----------|--------|
| G-json | `Core.Json` encode/decode | port | strong | adopt | M11 | L |
| G-regex | `Core.Regex` (PCRE-backed) | port | strong | adopt | M11 | L |
| G-datetime | `Core.Time` immutable datetime/duration value | port | strong | adopt | new M-Batteries | L |
| G-base64hex | `Core.Encoding` base64 / hex | port | strong | adopt | M11 | S |
| G-hash | `Core.Hash` deterministic digests (sha256/md5/crc32) | port | strong | adopt | M11 | M |
| G-env | `Core.Env` env var + dotenv config loading | port | ok | adopt | new M-Batteries | M |
| G-args | `Core.Args` CLI argument parsing | port | strong | adopt | new M-Batteries | M |
| G-text-more | `Core.Text` breadth (`startsWith`/`pad`/`slice`/`indexOf`/`repeat`/`format`) | port | strong | adopt | M11 | M |
| G-list-more | `Core.List` breadth (`sort`/`contains`/`slice`/`flatten`/`zip`/`unique`/`indexOf`) | port | strong | adopt | M11 | M |
| G-file-more | `Core.File` breadth (`append`/`delete`/`copy`/`lines`/`tempFile`) | port | ok | adopt | new M-Batteries | M |
| G-dir | `Core.Dir` directory ops (`list`/`make`/`exists`/`glob`) | port | ok | adopt | new M-Batteries | M |
| G-path | `Core.Path` path manipulation (`join`/`base`/`ext`/`dir`) | port | strong | adopt | M11 | S |
| G-console-io | `Core.Console` breadth: `print`, `eprintln`, `readLine`, `exit(code)` | port | strong | adopt | M11 | S |
| G-random | `Core.Random` PRNG + `random_bytes` | port | weak | defer | new M-Batteries | M |
| G-uuid | `Core.Uuid` v4/v7 generation | port | weak | defer | new M-Batteries | S |
| G-http-client | `Core.Http` outbound HTTP client | new | weak | defer | M6+ | L |
| G-db | `Core.Db` PDO-equivalent database access | port | ok | defer | M6+ / dedicated | L |
| G-process | `Core.Process` spawn / exec external programs | port | weak | defer | new M-Batteries | M |
| G-log | `Core.Log` structured logging (PSR-3 shape) | port | ok | defer | new M-Batteries | M |
| G-compress | `Core.Compress` gzip/zlib | port | weak | defer | new M-Batteries | M |
| G-crypto-strong | `Core.Crypto` HMAC / password hashing / constant-time compare | port | ok | defer | new M-Batteries | M |
| G-stdlib-namespace | A formal `Core.*` stdlib charter (determinism tiers, quarantine policy) | map | strong | adopt | M9/M11 | S |

## Rationale per ADOPT item

**G-json — `Core.Json`.** JSON is the lingua franca of every web/CLI app and PHP has
`json_encode`/`json_decode` built in (tier-1, no ini extension). It is fully deterministic, so it
sits on the byte-identity spine cleanly. The only real work is the type story: decode needs a
dynamic `Json`/`Any` sum type (the project already names this as the blocker that defers
`core.json` — `Ty` has no type variable / `Any`), which is exactly why it should land *with* the
generics/`Any` machinery in M11. Encode of a statically-typed value is easier and could ship first.
This is the single highest-leverage battery for a PHP-familiar developer.

**G-regex — `Core.Regex`.** Regular expressions are table-stakes for text processing and a PHP dev
reaches for `preg_match`/`preg_replace` reflexively. Deterministic, so spine-safe. Two design
constraints make it a large item: (1) the transpile target must use **PCRE (`preg_*`), not
mbstring** — the oracle runs `php -n`, so mbstring is absent (see the project's extension-policy
note); (2) std-only Rust has no regex engine, so the *interpreter/VM* side needs a hand-rolled
matcher (or the natives are transpile-only / quarantined). A pragmatic first cut: a small,
documented PCRE *subset* implemented in std-only Rust whose semantics match `preg_*` on that subset,
byte-identity-gated only over that subset. High effort but high value.

**G-datetime — `Core.Time`.** Dates/times are needed by nearly every real app. The PHP-idiomatic,
craftsmanship-respecting form is an **immutable** value type (`DateTimeImmutable` is PHP's own
modern answer to the mutable-`DateTime` footgun — a perfect "remove the surprise, keep the
capability" upgrade). The pure parts — parse/format/arithmetic on an explicit timestamp, durations,
comparisons — are deterministic and spine-safe. Only `now()` is non-deterministic and must be
quarantined (M6 model: not exercised by the byte-identical harness). Ship the value type + pure ops
on the spine; gate `now()` outside it.

**G-base64hex — `Core.Encoding`.** `base64_encode`/`base64_decode`/`bin2hex`/`hex2bin` are tier-1
PHP builtins, fully deterministic, trivially std-only in Rust, and compose naturally with the
already-shipped `bytes` primitive (encode a `bytes` to a `string` and back). Small effort, immediate
real-world payoff (tokens, data URIs, binary interchange). Pure win.

**G-hash — `Core.Hash`.** Deterministic digests over a *known input* (`sha256`/`sha1`/`md5`/`crc32`)
are spine-safe (same input → same digest on all three backends; PHP `hash()`/`crc32` are tier-1).
std-only Rust must hand-roll the digest kernels (no crate), which is the bulk of the effort, but
these are well-specified and finite. Distinguish sharply from G-crypto-strong (HMAC / password
hashing with a *random* salt) which is impure and deferred — plain content digests are pure and
should ship.

**G-base64hex / G-hash note on bytes:** both lean on the shipped `bytes`/`Core.Bytes` surface, so
they are additive natives with no new `Op`/`Value` — the cheapest possible integration.

**G-env — `Core.Env`.** Reading configuration from the environment (`getenv`/`$_ENV`) and a
`.env`-style loader is how every PHP app is configured (`vlucas/phpdotenv` is near-universal). It is
*impure* (the environment is ambient), so `Env.get(name)` must be quarantined like M6 network — not
exercised on the byte-identical spine. But it is high-leverage and PHP-idiomatic, so adopt it behind
the quarantine in a dedicated batteries milestone. Pure parsing of a dotenv *string* can be
spine-tested.

**G-args — `Core.Args`.** Phorj has a strong CLI story (`phg build`, standalone executables) yet a
*built program cannot read argv* (KNOWN_ISSUES: "built binaries ignore argv"). A typed CLI
arg-parser (positional + flags, PHP's `getopt` is the floor; a typed builder is the craftsmanship
upgrade) is what turns Phorj from "scripting toy" into "real CLI tool language." Argv is impure
(quarantine the *source* of args) but parsing a given `List<string>` is pure and spine-testable.
This also unblocks the standalone-executable story end-to-end.

**G-text-more / G-list-more — stdlib breadth.** The shipped `Core.Text` and `Core.List` cover only a
handful of operations; a PHP dev expects `startsWith`/`endsWith`/`pad`/`slice`/`indexOf`/`repeat`,
`sprintf`-style formatting, and `sort`/`contains`/`slice`/`unique`/`flatten`/`zip`/`indexOf`. All
deterministic, all map to tier-1 PHP string/array builtins, all additive natives on the existing
generic + higher-order path (the `NativeEval` machinery already supports closure args for `sort` by
key). These are the "obvious missing verbs" that make everyday code legible — the project itself
lists `Set` union/intersection and map iteration as pending on this same path. Medium effort, broad
daily payoff.

**G-file-more / G-dir / G-path — filesystem breadth.** `Core.File` today is read/exists/write only.
Real apps need `append`/`delete`/`copy`/read-as-`lines`, directory listing/creation/glob, and pure
path manipulation (`join`/`basename`/`extension`/`dirname`). Path manipulation is *pure* and
spine-safe (string-in, string-out — `Core.Path` is a clean adopt with small effort). File and
directory *I/O* is impure/non-deterministic and quarantined like the existing `File.read` (which
already reads a committed fixture for determinism in examples) — adopt behind that same discipline.

**G-console-io — `Core.Console` breadth.** Console today is `println` only. A real program needs
`print` (no newline), `eprintln` (stderr — keeps diagnostics off stdout, the byte-identical
channel), `readLine` (stdin), and `exit(code)`. `print`/`eprintln` map to PHP `echo`/`fwrite(STDERR)`;
`eprintln` writing to stderr is *helpful* for the spine because stderr is outside the compared
stdout. `readLine`/`exit` are impure and quarantined. Small effort, foundational for any interactive
or scripted program.

**G-stdlib-namespace — a `Core.*` stdlib charter.** Before piling on modules, the project needs a
written policy: which `Core.*` modules exist, the **determinism tier** of each native (pure /
ambient-input / non-deterministic), and the rule for what may sit on the byte-identity spine vs.
what must be quarantined like M6. This already exists implicitly (the extension-policy spec, the
File-reads-a-fixture trick, the M6 quarantine) but is not consolidated. Cheap to write, and it is
the thing that lets the developer "stop finding gaps ad hoc" in this domain — it makes the batteries
roadmap a closed, tiered list instead of an open-ended grab-bag. Map kind (the discipline exists;
this formalizes it).

### Deferred (with reason)

- **G-random / G-uuid** — non-deterministic by nature; break the byte-identity spine. Defer behind
  the M6 quarantine. UUID v7 is desirable (sortable) but waits on a seeded/quarantined PRNG.
  Deterministic-seed variants could be offered for testing, but the default is impure.
- **G-http-client** — outbound HTTP needs a network stack: std-only Rust has **no HTTP/TLS client**
  (breaks zero-dependency), *and* network is non-deterministic (breaks the spine) — the exact two
  reasons M6 deferred URL/network. Defer to M6+ behind the `Transport`-style quarantine; the PHP
  target (`curl`/streams) exists, so it's transpile-feasible but cannot sit on the spine.
- **G-db** — a PDO-equivalent is the keystone of "real PHP app," but it is impure (external DB
  state), needs a driver (Postgres is already on the M6 roadmap), and the std-only Rust side has no
  client. Defer to a dedicated milestone after M6 concurrency/servers; transpiles to PHP `PDO`.
- **G-process** — spawning external programs is impure and a security/portability surface; std Rust
  *can* do it (`std::process`), so zero-dep holds, but determinism does not. Defer behind quarantine.
- **G-log** — structured logging (PSR-3 shape) writes to stderr/files (impure) and is partly
  subsumed by G-console-io `eprintln`. Worth a real PSR-3-shaped `Core.Log` later; defer.
- **G-compress** — gzip/zlib is deterministic in principle but std-only Rust has no DEFLATE
  implementation (would need a hand-rolled kernel, large), and demand is lower than the items above.
  Defer.
- **G-crypto-strong** — HMAC is deterministic (could adopt), but password hashing (`password_hash`)
  and secure random salts are impure, and constant-time compare is a security-correctness surface
  that deserves its own design. Defer the bundle; HMAC-over-known-input could be pulled into G-hash
  later.

## Critic pass

Verified the shipped native surface directly from `src/native.rs` (full `(module, name)` inventory):
`Core.Console` = `println` only; `Core.Math` = `sqrt`/`pow`/`floor`/`ceil`/`abs`/`min`/`max`;
`Core.Text` = `len`/`upper`/`lower`/`trim`/`contains`/`split`/`splitOnce`/`join`/`replace`;
`Core.File` = `read`/`exists`/`write`; `Core.Bytes` = `fromString`/`toString`/`len`/`concat`/`slice`/
`find`; `Core.List` = `map`/`filter`/`reduce`/`reverse`/`sum`; `Core.Map` = `keys`/`values`/`has`/
`size`; `Core.Set` = `of`/`contains`/`size`; `Core.Html` = full builder set. **No mis-listings:**
every ADOPT item's named verbs are genuinely absent (the original list correctly excludes the shipped
`reverse`/`sum`/`map`/`filter`/`reduce`, `keys`/`values`/`has`/`size`, `splitOnce`, etc.). **0 removed.**

Newly-found gaps (the long tail the first pass missed — all map to tier-1 `php -n` builtins, all pure
& deterministic ⇒ sit straight on the byte-identity spine, all additive natives with no new
`Op`/`Value`):

| id | title | kind | fit | rec | milestone | effort |
|----|-------|------|-----|-----|-----------|--------|
| G-numfmt | `Core.Num` int/float parse + format (`parseInt`/`parseFloat`/`format`/`round`/`intDiv`/`mod`) | port | strong | adopt | M11 | M |
| G-math-more | `Core.Math` breadth (`round`/`trunc`/`log`/`exp`/`sin`/`cos`/`tan`/`PI`/`E`) | port | strong | adopt | M11 | S |
| G-list-predicates | `Core.List` higher-order predicates (`find`/`any`/`all`/`indexOfBy`) | port | strong | adopt | M11 | S |
| G-url | `Core.Url` urlencode/decode + query-string + `parseUrl` | port | strong | adopt | M6+/M11 | M |
| G-csv | `Core.Csv` parse/format rows | port | ok | adopt | new M-Batteries | M |

**G-numfmt — string↔number conversion + numeric formatting.** The most glaring miss. Phorj ships
**no** `string→int`/`string→float` parse and **no** `number_format`-style output — a PHP dev uses
`intval`/`floatval`/`(int)`/`number_format`/`intdiv`/`%` constantly. All pure & deterministic;
`parseInt` returns `int?` (clean failure on bad input, composes with S2 `??`). `intDiv`/`mod` map to
PHP `intdiv`/`%` (and Phorj already has checked-arith kernels to reuse). This is table-stakes for any
program that reads numbers from text (args, files, JSON later). Strong adopt; arguably should precede
the I/O batteries since it is pure and unblocks parsing argv/env/file content into numbers.

**G-math-more — `round` is the headline omission.** Shipped Math has `floor`/`ceil` but **not
`round`** (with precision) — the single most-used PHP math builtin after `abs`. Add `round`/`trunc`,
the transcendentals (`log`/`exp`/`sin`/`cos`/`tan`/`sqrt` is already there), and the `PI`/`E`
constants. Pure, deterministic, trivially std-only. Caveat (note, not blocker): irrational results
(`sin`, `log`) hit the existing float-precision divergence vs PHP's `echo` — keep guide examples to
exactly-representable values, same discipline `Core.Math.sqrt` already uses (KNOWN_ISSUES). Small,
high daily payoff.

**G-list-predicates — the PHP 8.4 `array_find`/`array_any`/`array_all` family.** [Verified via search:
PHP 8.4 added `array_find`/`array_find_key`/`array_any`/`array_all`.] These are closure-taking
predicates that sit *exactly* on the already-shipped `NativeEval::HigherOrder` + re-entrant
`call_closure_value` path (the same machinery as `map`/`filter`/`reduce`), so they are nearly free to
add and erase to the new PHP 8.4 builtins (or a tiny shim on older PHP). Distinct from G-list-more's
non-closure verbs; both are worth it. Strong adopt, small effort.

**G-url — url-encoding + query-string.** `urlencode`/`rawurlencode`/`http_build_query`/`parse_url` are
pure string-in/string-out, fully deterministic and spine-safe, and become near-term relevant the
moment the M6 `handle(Request) -> Response` work needs to read query parameters or build links. The
first pass covered the HTTP *client* (deferred, impure) but missed this *pure* web-string battery
entirely. Strong adopt; naturally lands alongside or just after M6's handler work.

**G-csv — CSV row parse/format.** `str_getcsv`/`fputcsv` are tier-1, and CSV is the second most common
data interchange after JSON for real CLI/data programs. Parsing/formatting a *given* string (or
`List<List<string>>`) is pure and deterministic; only reading a file is impure (reuse the File
quarantine). Lower priority than JSON but a genuine real-world battery the first pass omitted. Adopt
into the batteries milestone.

Refinements on existing rows (not new rows — flagged for the consolidator):
- **G-json should split encode from decode.** Encode of a *statically-typed* value needs no `Any`/`Json`
  type and is pure — it can ship **early**, ahead of the generics/`Any` work that only `decode` truly
  needs. The single-L-item framing over-couples the cheap half to the expensive half.
- **G-text-more `pad` must target `str_pad`, not `mb_str_pad`.** [Verified via search: `mb_str_pad` is
  PHP 8.3 mbstring.] The oracle runs `php -n` (mbstring absent — see the extension-policy note), so a
  Text `pad`/`len` that must round-trip through PHP uses the single-byte `str_pad`/`strlen` family;
  multi-byte width is a tier-3 (local-only) concern. Same trap already documented for the stdlib.
- **G-file-more should include bytes I/O.** Today `File.read` returns `string?`, so a *binary* file
  can't round-trip through the already-shipped `bytes` primitive. Add `readBytes -> bytes?` /
  `writeBytes(bytes)` under the same File quarantine — closes the loop between `Core.File` and
  `Core.Bytes`.

Philosophy sanity check: all five new items are "the most PHP-familiar, legible form" of a builtin a
PHP dev reaches for reflexively (`number_format`, `round`, `array_find`, `urlencode`, `str_getcsv`) —
no PL-theory novelty, each makes everyday code more legible and (for the parse family) provably safer
via `int?`/`float?`. None earns a surprise-budget cost. All fit the existing additive-native path.
