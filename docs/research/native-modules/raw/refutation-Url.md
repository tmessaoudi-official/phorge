# Stage 2b — Adversarial Byte-Identity Refutation: `Core.Url`

**Target claim:** `Core.Url` (parse/build/query-encode) is Tier A, ~80% feasible, "100% Tier A — pure
function of string input," byte-identical across `run` / `runvm` / real-PHP-8.5 (`php -n`).

**Verdict:** The claim **largely survives** adversarial review. `determinism_holds = true` for the
codec/query slice. No hidden non-determinism (no clock, entropy, addresses, object-ids, float
formatting, locale) exists in this module — those classes are genuinely absent because every output
is a pure function of a string and a Phorj `Value::Map` (insertion-ordered `Rc<Vec<(HKey,Value)>>`).
Empirical re-verification against **PHP 8.5.7 under `php -n`** confirms the spike's load-bearing PHP
claims. However, I found **three under-specified landmines** that are byte-divergence risks *if the
implementation follows the transpile table literally* rather than the "own the helper" discipline. None
are fatal; all are avoidable; but each is a concrete one-byte-divergence trap, so I lower confidence on
the parse/build slice and flag the decode contract specifically.

---

## What I re-verified empirically (PHP 8.5.7, `php -n`) — claim holds

| Spike claim | Re-verification | Holds? |
|---|---|---|
| `rawurlencode` unreserved = `A-Za-z0-9-_.~`, space→`%20`, `~` kept, uppercase hex | `rawurlencode("~-._ /+&=A z9")` → `~-._%20%2F%2B%26%3DA%20z9`; `rawurlencode("/?#[]")` → `%2F%3F%23%5B%5D` (UPPER) | ✅ |
| `http_build_query(...,PHP_QUERY_RFC3986)` does NOT sort, preserves insertion order | `["b"=>"2","a"=>"1"]` → `b=2&a=1` | ✅ (prior-art "sorted keys" is indeed WRONG) |
| RFC3986 flag → `rawurlencode` semantics (space→`%20`, `~` kept) | `["k"=>"a b","t"=>"~"]` → `k=a%20b&t=~` | ✅ |
| `rawurldecode` is lenient (passes bad `%` through) | `rawurldecode("a%2")`→`"a%2"`, `rawurldecode("%ZZ")`→`"%ZZ"`, `rawurldecode("a+b")`→`"a+b"` (plus stays plus) | ✅ — a strict Phorj decoder genuinely diverges from bare `rawurldecode`; the gated helper is mandatory, not optional |
| `parse_url` returns `bool(false)` on malformed; do not transpile to it | (accepted from spike; the leniency class is real and version-drifting) | ✅ |
| `parse_str` bracket / `.`→`_` magic — must not replicate | (accepted; own `parseQuery`) | ✅ |
| Byte-level encoding (operate on `&[u8]`, not `chars()`) | `rawurlencode("é")`→`%C3%A9` (per-UTF-8-byte) | ✅ — matches a Rust `&[u8]` loop exactly |
| `build_map` last-wins on dup keys (needed for `parseQuery`) | `src/value.rs:148-152` keeps **first position, last value**; test `build_map_dedups_first_position_last_value` (value.rs:1333) | ✅ — and PHP arrays/`parse_str` do the **same** (first slot, last value), so it matches |
| No new `Op`, reuses `Op::CallNative`; `Value::Map` rep is insertion-ordered `Rc<Vec<(HKey,Value)>>` | `src/value.rs:43` confirms rep | ✅ |

The four "big" determinism traps (clock, entropy, filesystem, float-format) are correctly identified
as **absent**. I could not manufacture any object-id, hash-ordering, or address leak: the Map is an
ordered `Vec`, not a `HashMap`, so iteration order is deterministic and shared by both Rust backends.

---

## Landmine 1 (NAMED, medium) — strict `decode → string?` invalid-UTF-8 detection under `php -n`

The spike's contract: `Url.decode(s) -> string?` returns `None` on a bad `%xx` **or on invalid UTF-8**.
The Rust side gets invalid-UTF-8 rejection for free: `Value::Str` wraps a Rust `String` (UTF-8
invariant), so `String::from_utf8(decoded_bytes)` failing → `None` is natural.

The PHP gated helper has **no such free invariant** — PHP strings are byte strings and hold invalid
UTF-8 happily (verified: `rawurldecode("%FF")` → a 1-byte string `0xFF`, `strlen`=1). So the helper
**must explicitly detect invalid UTF-8** to return `null` byte-identically. The spike's transpile table
(`__phorj_url_decode`) says "mirror Rust strictness" but never names the detection mechanism. Under
`php -n` the only *guaranteed-core* mechanism is **PCRE `preg_match('//u', $s)`** (PCRE is core — verified
`preg_match` present). `mb_check_encoding` happens to be present on *this* build but mbstring is NOT
guaranteed compiled-in on every `php -n` (KNOWN: the oracle assumes only PHP core + compiled-in ext;
mbstring is conventionally treated as absent per the project's own `transpile-no-ini-extensions` memory).
**If the helper author reaches for `mb_check_encoding`, it can break on a stricter `php -n` build.** The
fix is trivial (use `preg_match('//u', $s) === 1`), but it is a real, must-pin decision, not a free win.
→ **A differential case `Url.decode("%FF")` (must be `None`) is mandatory**, plus `Url.decode("%2")` (bad
`%xx` → `None`) and `Url.decode("%2f")` (lowercase hex must decode to `/` — verified `rawurldecode("%2f")`
→ `/`, so the strict helper must accept BOTH cases on input while emitting upper on encode).

## Landmine 2 (NAMED, low) — `buildQuery` value coercion vs the `Map<string,string>` scope guard

`http_build_query` silently **drops `null` values** and coerces `true`→`1`, `false`→`0` (verified:
`["n"=>5,"f"=>1.5,"b"=>true,"x"=>false,"z"=>null]` → `n=5&f=1.5&b=1&x=0` — `z` is GONE). The spike scopes
`buildQuery` to `Map<string,string>`, which sidesteps this entirely — *as long as the checker actually
enforces the `string` value type*. But Phorj `Value::Map` can hold any `Value`, and a generic-erased or
`Map<string, V>` caller could smuggle a non-string in. If a `Value::Null` ever reaches the transpiled
`http_build_query`, the Rust `eval` (which would need its own null-handling) and PHP **must agree** that
the pair is dropped — and a naive Rust `eval` that emits `k=` would diverge. → The native's checker
signature must pin the value type to `string` (no optional, no union), and a differential case proving
the type wall holds is cheap insurance. Not a divergence *if scoped as written*; a divergence the moment
scope creeps.

## Landmine 3 (NAMED, low/structural) — PHP array integer-key normalization (does NOT bite buildQuery, but bites any Map round-trip)

PHP arrays auto-cast numeric-string keys to ints: `["10"=>"x"]` has key `int(10)` (verified
`array_keys` → `int(10)`, `int(2)`, `"abc"`). Phorj `HKey::from_value` keeps `Str("10")` as a **string**
(verified `src/value.rs` `from_value`: `Value::Str(s) => HKey::Str(...)`, no numeric cast). For
`buildQuery` this is **harmless** — the emitted *text* is `10=x` either way and insertion order is
preserved on both sides (verified `["z","5","a"]` insertion order survives `http_build_query`). **But**
the spike's slice-2 `Url` struct and any future "return a Map" API (e.g. `parseQuery` returning a Map that
is then printed or compared) must never assume PHP-side and Phorj-side keys are the *same type* — a
`parseQuery("10=x")` result Map would have a `Str("10")` key in Phorj and, if the gated helper builds a
PHP array, an `int(10)` key in PHP. They only stay byte-identical because (a) `parseQuery` is owned by a
gated helper that should build the array with **string keys forced** (`$r["$k"] = $v` does NOT prevent the
cast — PHP *always* casts numeric-string array keys; the only safe rep is to NOT round-trip the Map
through PHP-observable form, i.e. keep the helper's output internal and never `var_dump`/`print_r` a
parsed-query Map in an example). → **No example may print a parsed-query Map directly**; examples must
project specific keys (`m["name"]`) whose *values* are strings. This is the same discipline as Map-print
generally, but the int-key cast makes it sharper here.

---

## Things I tried to break and could NOT

- **Empty inputs:** `http_build_query([])` → `""` (verified), single empty value `["a"=>""]` → `a=`
  (verified) — both trivially matchable by a Rust join.
- **Special chars in keys/values:** `["a=b"=>"c&d"]` → `a%3Db=c%26d` (verified, fully encoded) — a Rust
  `rawurlencode(k)=rawurlencode(v)` join matches.
- **Unicode:** byte-level on both sides (verified) — no mbstring dependency, no locale.
- **Hash ordering:** impossible — `Value::Map` is an ordered `Vec`, shared by interpreter + VM via the
  single `build_map` kernel; no `HashMap` iteration anywhere on the path.
- **Float formatting:** not reachable — module is string/int only (the one float risk class, Rust-vs-PHP
  `echo`, never appears).

---

## Effect on the feasibility number

The codec/query slice (encode/decode/buildQuery/parseQuery) is **solid**: I confirm ~95%, *conditioned on*
the decode helper using PCRE `//u` (not mbstring) for UTF-8 validation and the value-type wall being
enforced. The parse/build slice (own RFC-3986 scanner on three legs) carries the genuine unproven risk —
the malformed-input edge matrix is large and the "own it identically in Rust eval + PHP string-ops helper"
discipline is exactly where a one-byte divergence hides (e.g. how an empty authority, a `:port` with no
host, or a trailing `?` with empty query is rendered must be pinned identically). I keep the parse slice
at ~65%. Blended, I land at **~78%** (a hair below the spike's 80%, reflecting the three named landmines),
**tier A confirmed**.

**Confidence:** high for codec/query (empirically re-verified against the 8.5 floor), medium for
parse/build (unproven scanner edge matrix). `determinism_holds = true` — the module is genuinely pure;
the risks are PHP-vs-Rust *semantic* divergences in the gated helpers, all avoidable with the documented
own-the-rules discipline, not non-determinism.
