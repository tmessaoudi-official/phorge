# Feasibility Spike — `Core.Url` (parse / build / query-encode)

**Verdict (one line):** Adopt the **pure, deterministic subset** now — percent-encode/decode + query
build/parse — as a Tier A native module that erases to PHP core `rawurlencode`/`rawurldecode` and a
hand-owned query codec. **Split off `Url.parse(s) -> Url?`** into a Phorj-owned RFC-3986 parser
(NOT PHP `parse_url`) — feasible but materially harder and the determinism-risk locus. Build it as a
second slice. Feasibility overall **~80%**; the encode/query core is ~95%, the full `parse`+`build`
round-trip is ~65%.

---

## 1. std-only feasibility

**Yes, fully std-only.** No external crate is needed for any part:

- **Percent-encode/decode:** trivial byte-level loop over `&[u8]` using `u8::is_ascii_alphanumeric`
  and a fixed unreserved set; hex via `format!("%{:02X}", b)` and `u8::from_str_radix(_, 16)` for
  decode. Pure std (`core`/`alloc`).
- **Query build/parse:** string `split('&')` / `split_once('=')`, reusing the existing
  `Value::Map(Rc<Vec<(HKey,Value)>>)` insertion-ordered representation. Pure std.
- **URL component parse (`Url.parse`):** a hand-rolled RFC-3986 scanner (scheme `://`, authority
  `user:pass@host:port`, path, `?query`, `#fragment`). Pure std byte/char scanning — no regex engine
  needed (and none exists std-only, so a regex approach is off the table anyway). PHP has no
  std-only-equivalent constraint; the constraint is *byte-identity with `php -n`*.

Verified PHP-core availability under `php -n` (ran `php -n -r 'function_exists(...)'`): `rawurlencode`,
`rawurldecode`, `urlencode`, `parse_url`, `http_build_query`, `parse_str`, `ksort` **all present**.
So a transpile target exists for every candidate function — the question is *byte-identity*, not
*availability*.

std Rust APIs relied on: `str::as_bytes`, `u8::is_ascii_alphanumeric`, `u8::from_str_radix` (via
`from_str_radix` on the hex pair), `char`/`str` slicing, `String::push_str`, `str::split`,
`str::split_once`, `Vec`, `Rc`. All edition-2021 std.

---

## 2. Tier A vs Tier B

**Tier A (pure, deterministic, byte-identity-gateable) — every function in scope is Tier A.**
URL parsing/encoding is a pure function of its string input: no clock, no entropy, no filesystem, no
network, no locale, no map-iteration-order ambiguity *if we fix the query order*. `pure: true` for
every native. This module never touches the M6 `Transport` quarantine — it is the *pure builder half*
of the web story (the impure half is `Core.Http`, already Tier B / deferred).

The one ordering decision (below): `Url.buildQuery(Map)` must emit keys in the Map's **insertion
order** (Phorj `Value::Map` is an insertion-ordered `Rc<Vec>`), which both Rust and PHP
`http_build_query` honor identically — so **do NOT sort** (the prior-art "sorted keys" suggestion is
*wrong* for byte-identity against `http_build_query`; see §6).

---

## 3. Byte-identity strategy

The spine is `run == runvm == real-PHP-8.5`. Strategy differs per function group:

### Group 1 — percent codec (`Url.encode` / `Url.decode`)
- **Own the algorithm; map to PHP core `rawurlencode`/`rawurldecode`.** Verified
  (`php -n -r 'echo rawurlencode("a b/c+d");'` → `a%20b%2Fc%2Bd`; `rawurldecode("a%20b%2Fc")` → `a b/c`).
- **Pin the unreserved set to PHP `rawurlencode`'s set exactly:** `A-Z a-z 0-9 - _ . ~`. Verified PHP
  `rawurlencode("~-._abc")` → `~-._abc` (tilde NOT encoded since PHP 5.3, RFC-3986 unreserved). The Rust
  `eval` must leave exactly these four punctuation bytes (`- _ . ~`) plus alphanumerics unencoded and
  uppercase-hex everything else. **Trap:** `urlencode` (form style) encodes space as `+` and DOES
  encode `~` (`%7E`) — these are *different functions*. Pick `rawurlencode` semantics for `Url.encode`
  and document the `+`-vs-`%20` difference; offer `Url.encodeForm` only if a form variant is wanted
  (defer — keep slice 1 minimal).
- **Uppercase hex** (`%20` not `%2f`) — PHP `rawurlencode` emits uppercase; Rust `format!("{:02X}")`
  matches. Pin this in a test.
- **`Url.decode(s) -> string?`** returns `None` on malformed input (a `%` not followed by two hex
  digits, or producing invalid UTF-8). PHP `rawurldecode` is *lenient* (passes a bad `%` through), so a
  strict Phorj decoder would diverge from a raw PHP `rawurldecode` mapping. **Resolution:** emit a
  gated `__phorj_url_decode` helper that mirrors the Rust strictness (return `null` on a bad `%xx`),
  not a bare `rawurldecode`. This is the same "own-the-rules, gated helper" precedent as
  `__phorj_parse_int` / `__phorj_parse_float`.

### Group 2 — query codec (`Url.buildQuery` / `Url.parseQuery`)
- **`buildQuery(Map<string,string>) -> string`:** join `rawurlencode(k)=rawurlencode(v)` with `&`,
  in Map insertion order. Map to PHP `http_build_query($m, '', '&', PHP_QUERY_RFC3986)` — the RFC3986
  flag makes PHP use `rawurlencode` (space→`%20`, matching our codec) instead of the default
  `urlencode` (space→`+`). **Verified `http_build_query` preserves insertion order, does NOT sort**
  (`http_build_query(["b"=>"2","a"=>"1"])` → `b=2&a=1`). So Phorj must also preserve insertion order —
  the prior-art "sorted-by-key" guidance contradicts the PHP oracle and would break byte-identity.
- **`parseQuery(string) -> Map<string,string>`:** split on `&`, then `split_once('=')`, rawurldecode
  each side. **Trap — repeated keys:** PHP `parse_str("a=1&a=2", $r)` keeps **last wins** (`a=2`); a
  Phorj insertion-ordered Map with last-write-wins on duplicate keys matches *only if* `build_map`
  overwrites. Verify `value::build_map` dedup semantics; if it keeps first or appends, own a helper and
  emit `__phorj_parse_query` (do NOT map to bare `parse_str`, which also does PHP array-bracket magic
  `a[]=1` that we must NOT replicate). **Recommendation: own `parseQuery` with a gated helper**, since
  `parse_str`'s bracket/`.`→`_` mangling is a divergence minefield.

### Group 3 — component parse/build (`Url.parse` / `Url.build`)
- **Do NOT transpile to `parse_url`.** `parse_url` is a C parser with version-specific edge behavior:
  it returns `false` (not an array) on "seriously malformed" input (verified `parse_url("http://:80")`
  → `bool(false)`), omits absent keys entirely, returns `port` as an **int** while others are strings,
  and its definition of malformed has shifted across PHP releases. Mapping a Phorj `Url?` struct onto
  this is a latent cross-version break (same class as the `filter_var` version-drift trap).
- **Strategy:** own a Phorj RFC-3986 scanner in the `eval`, returning an injected `Url` struct (or
  `None`), and **emit a gated `__phorj_url_parse` PHP helper that re-implements the same scanner** —
  NOT a `parse_url` call. The helper is pure string ops (`strpos`/`substr`/`explode`), all PHP core,
  all `php -n`-safe. This makes the parser *Phorj-owned on all three legs* — the only way to guarantee
  byte-identity for a non-trivial parser (the same conclusion the prior art reaches for regex and
  `filter_var`).

---

## 4. Exact PHP transpile targets

| Phorj native | PHP transpile target (`php -n`-safe) | Notes |
|---|---|---|
| `Url.encode(s)` | `rawurlencode({s})` | core; unreserved `A-Za-z0-9-_.~`, uppercase hex, space→`%20` |
| `Url.decode(s)` | `__phorj_url_decode({s})` (gated helper) | strict: `null` on bad `%xx` / invalid UTF-8 (NOT bare `rawurldecode`, which is lenient) |
| `Url.buildQuery(m)` | `http_build_query({m}, '', '&', PHP_QUERY_RFC3986)` | RFC3986 flag = rawurlencode semantics; insertion order preserved (no sort) |
| `Url.parseQuery(s)` | `__phorj_parse_query({s})` (gated helper) | own it — avoid `parse_str` bracket/`.` mangling; last-wins on dup keys |
| `Url.parse(s)` | `__phorj_url_parse({s})` (gated helper) | own RFC-3986 scanner; NOT `parse_url` (version-drift + `false`-return trap) |
| `Url.build(url)` | `__phorj_url_build({url})` (gated helper) | reassemble `scheme://user:pass@host:port/path?query#frag`, omitting absent parts |

Gated helpers follow the existing `uses_* bool + emit_runtime_helpers` mechanism (precedent
`__phorj_parse_int`, `__phorj_text_index_of`, `__phorj_json_*`). The Rust `eval` is the source of
truth; each helper is written to match it byte-for-byte and is covered by the example differential.

---

## 5. Phorj API sketch

```phorj
import Core.Url;

// --- percent codec (slice 1) ---
string e = Url.encode("a b/c");          // "a%20b%2Fc"
string? d = Url.decode("a%20b%2Fc");     // Some("a b/c"); None on bad %xx

// --- query codec (slice 1) ---
Map<string, string> q = ["name" => "Ada", "lang" => "phorj"];
string qs = Url.buildQuery(q);           // "name=Ada&lang=phorj"  (insertion order)
Map<string, string> back = Url.parseQuery("a=1&b=&c=2");  // {a:"1", b:"", c:"2"}

// --- component parse/build (slice 2, injected Url struct) ---
Url? u = Url.parse("https://host.example:8080/a/b?x=1#frag");
match u {
    Some(url) => Console.println(url.host),   // "host.example"
    None      => Console.println("malformed"),
}
string rebuilt = Url.build(url);              // round-trips
```

**`Url` struct shape** (injected-type pattern, like `Json`/`RoundingMode`): inject an enum/struct AST
when `Core.Url` is imported. Recommended as a **single-variant struct-enum** (Phorj has no free-standing
struct type, but `enum`/class works):

```phorj
class Url {
    public string scheme;
    public string host;
    public int? port;          // PHP parse_url returns int port — but we own it; int? is clean
    public string path;
    public string query;       // raw query string (parseQuery splits it)
    public string fragment;
}
```

Absent components → empty string (`""`) rather than `null`, except `port` which is genuinely optional
(`int?`). This avoids six `string?` fields and the `??` ceremony at every use; it diverges from
`parse_url`'s "omit the key" model, but since we own the parser on all legs that is a *clean* choice,
not a divergence. (Open design question for slice 2: empty-string-vs-`string?` for `host`/`scheme` —
lean empty-string for ergonomics.)

---

## 6. New VM Op needed?

**No.** Every function is a `Op::CallNative(idx, argc)` — the existing generic, typed, multi-arg,
value-returning native call path (the same path `Core.Text`/`Core.Json` use). No new `Op`, no
`Value` change (reuses `Value::Str`, `Value::Map`, `Value::Null`, and — for slice 2 — `Value::Enum`/
`Instance` via the injected `Url` type). This is purely additive, like every Wave-2 stdlib module.

For slice 2, the injected-type prelude (`cli::inject_url_prelude`, gated on `import Core.Url`) mirrors
`inject_json_prelude` exactly — a no-op `Cow::Borrowed` unless imported and not already declared.

---

## 7. Named determinism risks

1. **`urlencode` vs `rawurlencode` (`+` vs `%20`, `~` vs `%7E`)** — *the* most likely accidental break.
   Pin `rawurlencode` semantics; a test must assert space→`%20` and `~` unencoded on all three legs.
   [Verified: `urlencode("~-._abc")`→`%7E-._abc`, `rawurlencode(...)`→`~-._abc`.]
2. **Hex case** — `%2F` (upper) not `%2f`. Rust `{:02X}` and PHP `rawurlencode` both upper; decode must
   accept both cases on input. Pin in a test.
3. **`http_build_query` does NOT sort** — preserves insertion order. Prior-art "sorted keys" is wrong;
   Phorj Map insertion order must match. [Verified: `["b"=>"2","a"=>"1"]`→`b=2&a=1`.]
4. **`parse_url` returns `false` on malformed + omits absent keys + int port** — do NOT transpile to it;
   own the parser. [Verified: `parse_url("http://:80")`→`bool(false)`.]
5. **`rawurldecode` is lenient; a strict Phorj decoder diverges** — own `Url.decode` with a gated
   helper that returns `null` on bad `%xx` (matches the Rust `eval`). [Verified: `rawurldecode` passes
   bad `%` through.]
6. **`parse_str` array-bracket / `.`→`_` mangling** — `a[]=1` and `a.b=1` get magic in PHP; we must NOT
   replicate. Own `parseQuery` with a flat last-wins Map. [Verified: `parse_str("a=1&a=2")`→`a=2`,
   last-wins; and PHP applies bracket magic that a flat Map must avoid.]
7. **Non-ASCII / mbstring absence under `php -n`** — encode/decode operate byte-level (UTF-8 bytes
   percent-encoded individually), matching PHP `rawurlencode` (also byte-level). A char-level Rust impl
   would diverge. Operate on `&[u8]`, not `chars()`.
8. **Map dedup semantics in `value::build_map`** — `parseQuery` last-wins requires `build_map` to
   overwrite on duplicate key. MUST verify the kernel before relying on it (flagged for slice-1
   implementation; if it keeps-first or appends, own the dedup in the helper + eval).
9. **`Url.decode` invalid-UTF-8 → fault vs `None`** — decide: `None` (optional) is the cleaner contract
   and matches `Text.parseInt`'s optional-return precedent; a fault would need `FaultKind` parity. Use
   `string?`.

No clock, entropy, filesystem, locale, or float-formatting risk in this module — the four big
determinism traps are all absent.

---

## 8. Effort

- **Slice 1 (encode/decode/buildQuery/parseQuery):** **small** — four natives in a new
  `src/native/url.rs`, ~4 gated PHP helpers (2 can map to core fns directly), one guide example
  (`examples/guide/url.phg`), registry wiring (one `registry.extend(url::url_natives())` line). Mirrors
  the `Core.Text` Wave-2 module almost exactly. No new Op, no injected type.
- **Slice 2 (parse/build + injected `Url` struct):** **medium** — the RFC-3986 scanner (Rust `eval` +
  matching PHP helper), `cli::inject_url_prelude`, the `Url` struct AST, struct-value construction in
  the `eval` (build a `Value::Instance`), round-trip example, more differential cases for malformed
  inputs. The parser-on-three-legs is the real work and the byte-identity locus.

Recommend shipping **slice 1 now** (adopt-now) and **slice 2 next** (adopt-later, same milestone) — the
encode/query half delivers most of the day-to-day value and carries near-zero determinism risk, while
the component parser deserves its own focused slice with the malformed-input differential matrix.

---

## 9. Recommendation

**adopt-now** for the pure codec subset; the component parser is a fast follow in the same module.
Rationale: the determinism partition is clean (100% Tier A), the byte-identity strategy is concrete and
mostly maps to verified PHP-core functions, no new Op or Value is required, and it is the pure builder
companion to the (deferred, Tier B) `Core.Http` — exactly the kind of std-only, deterministic module
the stdlib charter wants. The only real engineering is owning the parser/decoder rules instead of
leaning on `parse_url`/`rawurldecode`/`parse_str` (whose quirks are the documented traps), which is the
same own-the-rules discipline already applied to `parseInt`/`parseFloat`/`json`.

**Feasibility: ~80%** (codec slice ~95%, parse/build slice ~65%). **Confidence: high** for the codec
slice (verified PHP behavior, established native pattern), **medium** for the full parser round-trip
(the malformed-input edge matrix and the empty-string-vs-`string?` struct shape are unproven until
built).
