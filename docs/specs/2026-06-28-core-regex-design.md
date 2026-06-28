# Core.Regex design (Fork A)

> Status: **design-locked** 2026-06-28 (developer-resolved forks; autonomous build to follow).
> Resolves GA-sequence Fork A. Companion: `docs/specs/2026-06-27-dependency-policy.md` (amended here).

## Resolved forks (developer, 2026-06-28)

1. **Engine = the `regex` crate** (RE2-style finite automaton). Chosen as the *best & most secure*
   option "regardless of byte-identity/PHP": **ReDoS-immune by construction** (linear-time, no
   catastrophic backtracking) — unlike PHP/PCRE, Perl, Python `re`, JS `RegExp`, which are all
   backtracking and ReDoS-vulnerable. "Never roll your own" applies to untrusted-input parsers, not
   only crypto, so hand-rolling an NFA was rejected (new security-sensitive code, far less vetting).
2. **API = compiled `Regex` value + named groups.**

## Why this does NOT break the byte-identity spine

The `regex` crate omits backreferences and lookaround *by design* (they force backtracking). That
omitted set is exactly the **non-regular** part of PCRE. On the **regular subset** the `regex` crate
accepts, PHP `preg_*` matches identically. So:

- A pattern the `regex` crate compiles ⇒ transpiled `preg_*` agrees ⇒ `run ≡ runvm ≡ real PHP` holds.
- A pattern using a backref/lookaround ⇒ **rejected at `Regex.compile` time** with a clean
  `E-REGEX-UNSUPPORTED`-class fault (the `regex` crate's own compile error, surfaced uniformly). It
  never reaches a backend, so no divergence is possible.

This is the SSOT's "restricted-subset dual-engine parity" (`php-parity-and-beyond.md:221`), achieved
*for free* by the crate's security-motivated feature set rather than a hand-curated subset.

## Dependency-policy amendment

`regex` becomes the **2nd** admitted dependency. Amend `dependency-policy.md` clause 1 from
"security-critical primitive **crypto**" to "security-critical primitive — **crypto, and
untrusted-input parsers (e.g. regex) where a ReDoS-/correctness-safe engine cannot be done safely in
`std`**". Add the table row. Feature-gate `regex` behind a new `regex` cargo feature (default on; OFF
for `phorj-playground`, exactly like `crypto`/`argon2`). `regex` is pure-Rust, no `unsafe` exposed,
fuzzed by the Rust project — clears clause 2 (vetted) and clause 3 (no `std` path, not PHP-delegated:
the engine runs natively on both Rust backends).

## Runtime model — zero new `Op`, zero new `Value`

Reuses the **injected-type** precedent (`Json`, `RoundingMode`, the Http types) + the established
**value-as-first-arg, module-qualified** stdlib call style (`List.map(xs,f)`, `Map.has(m,k)`,
`Set.contains(s,x)`).

- **`Regex` is an injected `final class`** with one private field `pattern: string` (the *bare*
  pattern, no delimiters). Injected by `cli::inject_regex_prelude` when a program imports
  `Core.Regex` (mirrors `inject_json_prelude`). Users construct it only via `Regex.compile`; the
  field is private so the value is opaque.
- A compiled `Regex` value is a `Value::Instance { class: "Regex", fields: { pattern } }`, built
  directly by the `Regex.compile` native (same hand-built-value technique as `jnode` for `Json`).
- **Compile cache**: a process-wide `thread_local!` `HashMap<String, Rc<regex::Regex>>` keyed by the
  bare pattern recovers "compile once, reuse" without a new `Value` variant. `Regex.compile`
  populates it (and faults on an invalid/unsupported pattern); query natives look up the cached
  engine (miss ⇒ recompile, which cannot fault because `compile` already validated). Cache is an
  optimization only — semantics are identical without it.

## Public surface (`import Core.Regex;`)

| Native | Signature | Behavior |
|--------|-----------|----------|
| `Regex.compile` | `(string) -> Regex` | Validate + cache; **faults** on invalid/unsupported pattern. |
| `Regex.matches` | `(Regex, string) -> bool` | Is there a match anywhere? |
| `Regex.find` | `(Regex, string) -> string?` | First whole match, else `null`. |
| `Regex.findAll` | `(Regex, string) -> List<string>` | All whole matches (empty list if none). |
| `Regex.findGroups` | `(Regex, string) -> Map<string, string>?` | Named captures of the first match, else `null`. |
| `Regex.replace` | `(Regex, string, string) -> string` | Replace all matches with the replacement. |
| `Regex.split` | `(Regex, string) -> List<string>` | Split on matches. |

Always Unicode (`/u`), case-sensitive. Case-insensitivity / inline flags deferred (note in
KNOWN_ISSUES) — keep v1 minimal, add `Regex.compileWith(pattern, ignoreCase)` later if asked.

**Named groups** reuse `Map<string,string>` rather than a new injected `Match` type (lowest surface,
consistent with `Core.Json`'s use of `Map`). Numbered-group access is deferred (named is the legible
choice; PHP `preg_match` with `$m` array covers both, but exposing only named keeps the API clean).

## Transpile (peer emission target — bridge only, per the dependency policy)

The injected PHP `final class Regex { private string $pattern; … }` stores the bare pattern. Query
natives emit `preg_*` using a delimiter that cannot collide: build the delimited pattern **at emit
time** with a `__phorj_regex_delim($pattern)` helper that picks a delimiter absent from the pattern
(`~`, then `#`, `%`, …) and appends `u`. Byte-identity is gated on the regular subset (above).
Mapping: `matches`→`preg_match`, `find`→`preg_match`+`$m[0]`, `findAll`→`preg_match_all`,
`findGroups`→`preg_match` + filter string-keyed (named) captures, `replace`→`preg_replace`,
`split`→`preg_split`. The oracle runs `php -n` (no extensions) — **PCRE is core, always present**, so
no extension dependency (unlike mbstring; see [[transpile-no-ini-extensions]]).

## Build slices

1. **Dep + policy**: add `regex` (feature-gated) to `Cargo.toml`; amend `dependency-policy.md`;
   `CHANGELOG.md` note; verify playground builds without it.
2. **Engine wrapper**: `src/native/regex.rs` — the thread-local cache, `compile_or_fault`, the seven
   `eval` bodies (shared by both backends), the `php` emitters, `regex_natives()`. Register in
   `native/mod.rs` (`#[cfg(feature = "regex")]`).
3. **Prelude injection**: `cli::inject_regex_prelude` + wire into `check_and_expand`.
4. **Transpile helper**: `__phorj_regex_delim` + the `Regex` PHP class emission.
5. **Example + tests**: `examples/guide/regex.phg` (byte-identity-gated run≡runvm≡real PHP 8.5);
   `src/native/regex_tests.rs`; a rejected-pattern case in the example README.

Gate each slice: `cargo test --workspace`, clippy, fmt; the PHP oracle at the 8.5 floor.
