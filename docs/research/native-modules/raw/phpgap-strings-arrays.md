# STAGE 3 — PHP string + array + iterable stdlib gaps, and the better Phorj port

Area: PHP string functions (`sprintf`, `str_*`, char-ops), array functions (`array_*`), iterable
helpers, sorting, and JSON edge cases — **stdlib/library capability gaps only** (language-level syntax
gaps are tracked in `docs/specs/2026-06-21-php-parity-and-beyond.md` and are out of scope here).

Lens applied throughout: **Phorj : PHP :: TypeScript : JS** — typed, deterministic, null-safe,
total. Every gap is classified Tier A (pure/deterministic → byte-identity-gateable, ships as a
`Core.*` native) or Tier B (impure → quarantined). Confidence graded high/medium/low.

---

## 0. Current Phorj inventory (verified by reading the source)

`src/native/text.rs` — **Core.Text**: `len`, `upper`, `lower`, `trim`, `contains`, `split`,
`splitOnce`, `join`, `replace`, `startsWith`, `endsWith`, `repeat`, `parseInt`, `parseFloat`,
`padLeft`, `padRight`, `indexOf`, `substring`. (All ASCII/byte semantics, no mbstring.)

`src/native/list.rs` — **Core.List**: `reverse`, `length`, `sum`, `contains`, `map`, `filter`,
`reduce`, `sort`, `sortWith`, `slice`, `indexOf`, `concat`, `first`, `last`.

`src/native/map.rs` — **Core.Map**: `keys`, `values`, `has`, `size`, `get`, `set`, `remove`.

`src/native/set.rs` — **Core.Set**: `of`, `contains`, `size`, `union`, `intersection`, `difference`.

`src/native/math.rs` — **Core.Math**: full numeric surface incl. `round`, `min`, `max`, `intdiv`.

`src/native/convert.rs` — **Core.Convert**: `toString`, `toFloat`, `toInt`, decimal conversions.

`src/native/json.rs` — **Core.Json**: `parse`, `stringify`, `stringifyPretty` (injected `Json` enum).

Mechanisms reused below (no reinvention): `(module,name)` registry entry per native;
`NativeEval::{Pure,HigherOrder}`; gated runtime helpers (`__phorj_*`, `uses_*` bool +
`emit_runtime_helpers`) when a PHP one-liner can't express the exact semantics; generic native path
(`Ty::Param`) erased before backends; `Op::CallNative` (no new Op needed for any gap below).

---

## TIER A — STRINGS

PHP's string library is large; the deterministic, byte-oriented core ports cleanly. The recurring
upgrade is **null-safe optional returns** (PHP returns `false`/`""`/`-1` sentinels) and **typed
checked formatting** (PHP `sprintf` is stringly-typed and silently coerces).

### A1. `sprintf`/`printf` — checked formatted output  **[GAP — high value]**
PHP: `sprintf("%05.2f %s", 3.1, "x")`. Phorj has only string interpolation (`"{x}"`) + the new
`number_format`-shaped need. **Phorj has NO equivalent for width/precision/zero-pad/radix/sign
formatting.**

Better port — a **checked, typed format native** rather than PHP's runtime-coerced varargs:
```
// Core.Text
Text.format(string template, List<FormatArg> args) -> string   // value-level varargs via a list
```
But the genuinely better, Phorj-idiomatic design is **typed per-kind formatters** that compose with
interpolation, avoiding a stringly format-string DSL entirely:
```
Int.toRadix(int n, int base) -> string          // base 2..36, lower-hex; PHP base_convert/dechex
Int.toHex(int n) -> string                       // PHP dechex
Float.fixed(float x, int decimals) -> string     // PHP number_format($x,$d,'.','')  (no thousands)
Float.fixedGrouped(float x, int decimals, string point, string sep) -> string  // PHP number_format full
Text.padLeft/padRight                            // ALREADY EXIST — cover %5s / %-5s / %05d via padLeft "0"
```
Why better than PHP `sprintf`: the format-string is the #1 PHP runtime-error source (wrong specifier
for the arg type silently mis-formats; `%d` on a string emits `0`). Typed formatters are
**compile-checked** (`Float.fixed` only accepts a float), total, and each is a trivial deterministic
native.
- **If a true `sprintf` is wanted** (muscle memory), ship a **checked subset** `Text.format` whose
  template is validated at runtime against the arg list arity/types — faulting (not silently
  coercing) on mismatch. Specifiers limited to the deterministic subset: `%s %d %x %X %o %b %f %e
  %05d %-10s %+d %%`. Locale specifiers (`%'`) excluded.
- **Determinism:** all pure integer/float→string. Float formatting must route through the existing
  `__phorj_float` (Ryū) discipline OR PHP `number_format`/`sprintf("%.*f")` — **and the two must be
  pinned to agree**: PHP `sprintf("%f")` uses C `printf` rounding (round-half-to-even on most libc),
  Rust `format!("{:.2}")` also rounds half-to-even, so `%.Nf` IS byte-identical for finite values —
  *verify the half-way rounding on the 8.5 floor*. **`%g`/`%e` are a determinism trap** (PHP's `%e`
  uses a min-2-digit exponent `1.0e+1`, Rust `{:e}` uses `1e1`) → if `%e` ships, emit via a gated
  helper that normalizes to PHP's exponent form, never Rust's.
- **Transpile target:** `Int.toHex`→`dechex`, `toRadix`→`base_convert($n,10,$base)` (lowercase, PHP
  matches), `Float.fixed`→`number_format($x,$d,'.','')`, `Text.format`→a gated `__phorj_format`
  helper that re-implements the checked subset in PHP (NOT raw `sprintf`, whose coercion differs from
  the checked semantics).
- Tier A. **Recommend: adopt-now** for the typed formatters (`Int.toHex/toRadix`, `Float.fixed`,
  `number_format`); **adopt-later** for the full `Text.format` checked-subset DSL.
- Confidence: high (formatters), medium (the `%f` half-way + `%e` exponent byte-identity needs an
  oracle check on PHP 8.5).

### A2. `number_format` (non-locale) **[GAP — already roadmapped M-text S1 / M-NUM]**
PHP `number_format(1234567.891, 2, '.', ',')` → `"1,234,567.89"`. No Phorj equivalent.
Better port: `Float.fixedGrouped(float, int decimals, string point, string sep) -> string` (above) —
**explicit separators, no implicit locale** (PHP's locale-aware variant is non-deterministic across
environments; Phorj forces explicit args = deterministic). Tier A. Transpile:
`number_format($x,$d,$point,$sep)`. **Recommend: adopt-now.** Confidence high. *(Note: for money use
the `decimal` primitive's own formatter — float grouping is for display only.)*

### A3. Case helpers: `ucfirst`, `lcfirst`, `ucwords`, `strtolower`/`strtoupper` (have upper/lower)  **[GAP]**
PHP `ucfirst`/`ucwords`/`lcfirst`. Phorj has `upper`/`lower` only.
Better port (ASCII, documented like the rest of Core.Text):
```
Text.capitalize(string) -> string     // ucfirst (first byte to upper)
Text.uncapitalize(string) -> string   // lcfirst
Text.titleCase(string) -> string      // ucwords (capitalize each whitespace-delimited word)
```
Why better: explicit names, ASCII semantics documented up front (PHP `ucwords` is byte-based too →
byte-identical). Tier A. Transpile: `ucfirst`/`lcfirst`/`ucwords`. **adopt-now.** Confidence high.

### A4. `strrev` (reverse), `substr_count`, `str_word_count`  **[GAP]**
```
Text.reverse(string) -> string         // strrev — BYTE reverse (multibyte caveat documented)
Text.count(string, string sub) -> int  // substr_count (non-overlapping)
Text.wordCount(string) -> int          // str_word_count mode 0
```
Better: typed `int` returns (PHP `substr_count` is fine; `str_word_count` has a locale-ish mode
matrix → expose only the deterministic ASCII mode 0). `Text.reverse` byte-reverse can split a
multibyte char → **fault on resulting invalid UTF-8** (EV-7), like `substring`/`pad` already do.
Tier A. Transpile: `strrev`/`substr_count`/`str_word_count`. **adopt-now.** Confidence high
(`reverse`/`count`); medium (`wordCount` — confirm PHP mode-0 word boundary matches a simple
`split_whitespace().count()` for ASCII; punctuation handling differs, document or restrict).

### A5. `trim` variants: `ltrim`, `rtrim`, and char-set trimming  **[GAP — partial]**
Phorj `trim` strips ASCII+unicode whitespace both sides. PHP `ltrim`/`rtrim` and the optional
`$characters` charset are missing.
Better port:
```
Text.trimStart(string) -> string                  // ltrim
Text.trimEnd(string) -> string                    // rtrim
Text.trimChars(string, string chars) -> string    // trim($s, $chars) — strip any char in set
```
Determinism trap: **PHP `trim()` default charset is `" \t\n\r\0\x0B"` (6 ASCII chars), Rust
`str::trim()` strips ALL Unicode whitespace** — these already DIVERGE. The existing `Text.trim`
should be audited: if it transpiles to bare PHP `trim()`, a string with a Unicode NBSP trims on Rust
but not PHP. **Finding: verify `Text.trim`'s current transpile; if it emits `trim($s)` the semantics
are not byte-identical for non-ASCII whitespace.** Fix: trim explicitly to the ASCII whitespace set
on both sides (Rust `trim_matches(|c| matches)` ↔ PHP default `trim`). Tier A. **adopt-now** for
ltrim/rtrim/trimChars; **flag the existing `trim` divergence as a P1 correctness item.**
Confidence: high (the gap), medium→needs-verification (the existing-`trim` divergence — grade as a
must-check).

### A6. `str_replace` array form, `strtr`, `preg_*`-free replacement  **[GAP — partial]**
Phorj `replace(subject, from, to)` does single-pair replace. PHP `str_replace([..],[..],$s)` (paired
arrays) and `strtr($s, $map)` (longest-match, single-pass) are missing.
Better port:
```
Text.replaceAll(string subject, Map<string,string> replacements) -> string   // strtr-like single pass
Text.replaceN(string subject, string from, string to, int limit) -> string   // bounded replace
```
Why better than PHP: `strtr` with an array does **longest-key-first single-pass** (no cascade
surprise where output of one replace feeds the next); exposing it as a `Map` makes the contract
typed and explicit. **Determinism note:** PHP `strtr` iteration over the replacement array is
key-length-ordered, deterministic — but a `Map` literal in Phorj is insertion-ordered. Pin the
native to **longest-key-first** (sort keys by length desc, ties by byte order) and write the gated
PHP helper to match (don't trust raw `strtr` to honor the same tie-break across PHP versions — verify
on 8.5). Tier A. **adopt-later** (needs the longest-match contract pinned + oracle-verified).
Confidence: medium.

### A7. `wordwrap`, `chunk_split`, `nl2br`  **[GAP — low priority]**
`Text.wrap(string, int width, string break, bool cut) -> string` (wordwrap),
`Text.chunk(string, int len, string sep) -> string` (chunk_split). All byte-deterministic, pure.
Tier A, transpile to `wordwrap`/`chunk_split`. **defer** (`nl2br` is HTML-specific → belongs in
`Core.Html` if anywhere; wrap/chunk are niche). Confidence high (feasibility), low (priority).

### A8. `sprintf("%b")`-free: `Text.toBytes`/`fromBytes`, hex/base64  **[mostly covered elsewhere]**
`Core.Bytes` already has `from_string`/`to_string`/hex-ish. base64/hex encoding is the separate
**Encoding** feasibility study (`feasibility-Encoding.md`) — out of this area. No new finding.

---

## TIER A — ARRAYS / LISTS

PHP's `array_*` is the muscle-memory surface. The map/filter/reduce/sort/slice spine already exists.
The gaps are the **structural transforms** and **set-ish ops**, all reusing the generic +
HigherOrder native path with **no new Op**.

### L1. `array_unique` → `List.unique` / `List.uniqueBy`  **[GAP — high value]**
PHP `array_unique($xs)` (preserves first occurrence). Missing.
```
List.unique(List<T>) -> List<T>                          // structural eq_val dedupe, first-wins
List.uniqueBy(List<T>, (T) -> K) -> List<T>              // HigherOrder, key-projected dedupe
```
Better: `uniqueBy` (TS/Lodash `uniqBy`) has no PHP equivalent and is the common real need
(dedupe-by-id). Use the existing `HKey`/`eq_val` machinery; insertion-order preserved (matches PHP
`array_unique` and the Map/Set insertion-order rep). Tier A. Transpile: `unique`→
`array_values(array_unique($xs, SORT_REGULAR))` — **determinism trap: PHP `array_unique` default
flag `SORT_STRING` stringifies for comparison** (so `[1,"1"]` dedupes), `SORT_REGULAR` uses loose
`==`. Phorj equality is structural/strict. **Pin to a gated `__phorj_unique` helper using strict
`===` comparison** so it matches Phorj's `eq_val`, NOT raw `array_unique`. **adopt-now.**
Confidence: high (with the gated-helper note).

### L2. `array_flip`, `array_combine`, `array_fill`, `array_pad`, `array_fill_keys`  **[GAP]**
```
List.fill(int count, T value) -> List<T>                 // array_fill(0,$n,$v)
List.pad(List<T>, int size, T value) -> List<T>          // array_pad
Map.fromEntries(List<Pair<K,V>>) -> Map<K,V>             // see L7; combine/flip live here
Map.flip(Map<K,V>) -> Map<V,K>                           // array_flip (V must be hashable → HKey)
```
Better: `Map.flip` is typed so a non-hashable value flip is a **compile error** (PHP `array_flip`
warns + drops at runtime). Tier A. `fill`→`array_fill`, `pad`→`array_pad`, `flip`→a gated helper
(PHP `array_flip` only accepts int/string values; Phorj's HKey subset matches → bare `array_flip`
works for the valid subset). **adopt-now** for fill/pad; **adopt-later** for flip (gated on Map value
hashability check). Confidence high.

### L3. `array_chunk` → `List.chunk`  **[GAP — high value]**
PHP `array_chunk($xs, $n)` → `List<List<T>>`. Missing; common for batching/pagination.
```
List.chunk(List<T>, int size) -> List<List<T>>           // size>=1 or fault (PHP warns + returns)
```
Better: faults on `size <= 0` (PHP emits a warning and returns the whole array — a footgun). Tier A.
Transpile: `array_chunk($xs, $n)` (PHP returns sequential sub-arrays → byte-identical to nested
Vecs). **adopt-now.** Confidence high.

### L4. `array_column`, `array_key_by` (group/index)  **[GAP]**
PHP `array_column($rows, 'name', 'id')`. The TS/Lodash idioms `keyBy`/`groupBy` are the better port:
```
List.indexBy(List<T>, (T) -> K) -> Map<K,T>              // keyBy — last-wins (or first? pin it)
List.groupBy(List<T>, (T) -> K) -> Map<K, List<T>>       // groupBy — no PHP builtin!
List.pluck(List<Instance>, string field) -> List<F>      // array_column single-col — but needs field reflection
```
Better: `groupBy` is a genuine upgrade — PHP has no native (everyone hand-rolls a `foreach`).
`indexBy`/`groupBy` are HigherOrder natives over the existing closure invoker. `pluck`/`array_column`
needs runtime field access on instances → defer until a clean reflective story (Core.Reflect exists
but field-by-name read on an arbitrary instance is heavier). Tier A. `groupBy`/`indexBy`: gated
`__phorj_group_by` PHP helper (a `foreach` accumulator — PHP has no builtin). **adopt-now**
(groupBy/indexBy — high value), **defer** (pluck — needs reflection). Confidence high
(group/index), medium (pluck).

### L5. `array_diff`, `array_intersect` on LISTS (Set has them on sets)  **[GAP — partial]**
`Core.Set` has `union`/`intersection`/`difference`. Lists don't.
```
List.diff(List<T>, List<T>) -> List<T>                   // array_diff — elements of a not in b, order-preserving
List.intersect(List<T>, List<T>) -> List<T>              // array_intersect
```
Better: order-preserving (PHP `array_diff` preserves keys → `array_values` re-index). Tier A.
**Determinism trap:** PHP `array_diff` compares via **string cast** (`(string)$a === (string)$b`),
so `[1]` vs `["1"]` are "equal" — Phorj must use strict `eq_val`. Gated `__phorj_diff` helper using
`===`. **adopt-now.** Confidence high (with gated-helper note). *(If the user already has
`Core.Set`, list-diff is a convenience; Set is the typed answer for true set algebra.)*

### L6. `array_search`/`in_array` with predicate; `find`/`findIndex`/`any`/`all`  **[GAP — TS upgrade]**
Phorj has `contains`/`indexOf` (value equality). The predicate forms (TS `find`/`some`/`every`) are
the better port and have weak PHP equivalents (`array_filter`+head, or hand-rolled loops):
```
List.find(List<T>, (T) -> bool) -> T?                    // first match or null  (TS find)
List.findIndex(List<T>, (T) -> bool) -> int?             // TS findIndex
List.any(List<T>, (T) -> bool) -> bool                   // some  / PHP no builtin
List.all(List<T>, (T) -> bool) -> bool                   // every / PHP no builtin
List.count(List<T>, (T) -> bool) -> int                  // count matching
```
Better: PHP has NO native `some`/`every`/`find` — these are the daily TS idioms a migrant expects.
HigherOrder natives, optional/`bool` returns (null-safe). Tier A. Transpile: gated helpers (PHP
lacks builtins) — `__phorj_any`/`__phorj_all` (`foreach` short-circuit), `find`→`array_filter`+
head won't short-circuit, so a gated `foreach` helper is both more correct and faster. **adopt-now.**
Confidence high.

### L7. `Map.fromEntries` / `Map.entries` / `Map.map` / `Map.filter` / `Map.merge`  **[GAP]**
`Core.Map` has keys/values/get/set/remove/has/size but no transforms.
```
Map.entries(Map<K,V>) -> List<Pair<K,V>>                 // needs a Pair type, OR List<[K,V]>?  see note
Map.fromEntries(List<Pair<K,V>>) -> Map<K,V>
Map.mapValues(Map<K,V>, (V) -> W) -> Map<K,W>            // HigherOrder
Map.filter(Map<K,V>, (K,V) -> bool) -> Map<K,V>          // HigherOrder
Map.merge(Map<K,V>, Map<K,V>) -> Map<K,V>                // array_merge / + operator (right-wins)
```
**Design blocker (medium):** `entries` needs a 2-tuple. Phorj has no tuple primitive; options:
(a) inject a `Pair<A,B>` stdlib type (injected-type pattern like `Json`), or (b) return
`List<List<V>>` heterogeneous — rejected (loses typing). **Recommend the injected `Pair<K,V>`** —
reusable across `entries`/zip/etc. `mapValues`/`filter`/`merge` need no tuple and are immediately
buildable. Tier A. Transpile: `mapValues`→`array_map` over values, `merge`→`array_merge` (right-wins,
matches), `filter`→`array_filter(...,ARRAY_FILTER_USE_BOTH)`. **adopt-now** (mapValues/filter/merge);
**adopt-later** (entries/fromEntries — gated on `Pair`). Confidence high (transforms), medium
(Pair design decision).

### L8. `List.take`/`drop`/`takeWhile`/`dropWhile`/`zip`/`flatten`/`flatMap`  **[GAP — TS/FP upgrade]**
```
List.take(List<T>, int n) -> List<T>                     // slice(0,n) sugar — but clearer intent
List.drop(List<T>, int n) -> List<T>
List.takeWhile(List<T>, (T)->bool) -> List<T>            // HigherOrder; no PHP builtin
List.dropWhile(List<T>, (T)->bool) -> List<T>
List.zip(List<A>, List<B>) -> List<Pair<A,B>>            // needs Pair (see L7); no PHP builtin
List.flatten(List<List<T>>) -> List<T>                   // array_merge(...$xs)
List.flatMap(List<T>, (T)->List<U>) -> List<U>           // map + flatten; HigherOrder
```
Better: none of takeWhile/dropWhile/zip/flatMap exist in PHP — pure TS/FP wins. Tier A. `take`/`drop`/
`flatten`→`array_slice`/`array_merge` spread; the HigherOrder + zip ones → gated helpers. **adopt-now**
(take/drop/flatten/flatMap/takeWhile/dropWhile); **adopt-later** (zip — gated on `Pair`).
Confidence high.

### L9. Sorting breadth: `sortBy` (key projection), `sortDesc`, stable-by-key  **[GAP — partial]**
Phorj has `sort` (natural) + `sortWith` (comparator). The ergonomic gap:
```
List.sortBy(List<T>, (T) -> K) -> List<T>                // sort by projected key (Schwartzian); TS/Lodash
List.sortDesc(List<T>) -> List<T>                        // natural descending
```
Better: `sortBy` avoids the comparator boilerplate (PHP `usort` always needs the full `<=>` lambda).
HigherOrder, stable (matches the existing `sort`'s stability note). Tier A. `sortBy`→a gated helper
projecting then `usort` (or `array_multisort`); `sortDesc`→`__phorj_sort` + `array_reverse`.
**adopt-now.** Confidence high. *(Sort stability/NaN-ordering is already roadmapped J-sort-stability
M11 — this is the ergonomic layer above it.)*

### L10. `range()` for lists  **[mostly covered — language `a..b` exists]**
Phorj has `a..b`/`a..=b` integer ranges (S1). PHP `range('a','z')` (char range) and float-step range
are not covered, but are niche. `List.range(int start, int end, int step)` could fill the stepped
case. Tier A. **defer** (the `..` syntax covers the 95% case). Confidence high (low priority).

---

## TIER A — ITERABLE / JSON EDGE CASES

### J1. JSON edge cases — depth, big ints, float fidelity, key ordering  **[partial — Core.Json exists]**
`Core.Json` already does parse/stringify/stringifyPretty over an injected `Json` enum. The remaining
**edge-case parity gaps** vs PHP `json_encode`/`json_decode`:
- **Big integers:** PHP `json_decode` with `JSON_BIGINT_AS_STRING`; without it, ints > PHP_INT_MAX
  become floats. Phorj `Json.Int` is i64 → an input `1e400`-ish or `>i64` integer must fault or
  clamp deterministically (verify current behavior — likely already faults on parse; document).
- **Float rendering:** `json_encode(0.1)` → PHP uses `serialize_precision=-1` (shortest round-trip,
  Ryū-like since PHP 7.1). Phorj must pin its float→JSON to the **same shortest round-trip** (the
  `__phorj_float` Ryū path) — already noted in memory as a divergence risk for irrational floats.
  **Finding: confirm `Json.stringify(0.1)` is byte-identical to PHP 8.5 `json_encode(0.1)` on the
  oracle; the M-NUM memory flags float-extremes divergence from native `json_encode`.**
- **`JSON_UNESCAPED_SLASHES` / `JSON_UNESCAPED_UNICODE`:** PHP escapes `/` → `\/` and non-ASCII →
  `\uXXXX` by *default*. Phorj must pick a fixed escaping policy and pin the PHP flags to match
  (e.g. always pass `JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE` for predictable UTF-8, OR match
  PHP defaults exactly — **pick one and document; this is a determinism gate**).
- **Key ordering / duplicate keys:** PHP `json_decode` last-key-wins on duplicates; object→assoc
  array preserves insertion order. Phorj's insertion-ordered Map matches — verify dup-key policy.
- **NaN/Inf:** `json_encode(NAN)` errors in PHP. Phorj `Json` has no NaN node → already safe; ensure
  a float NaN passed to stringify faults cleanly (EV-7), not emits `nan`.
- Tier A. **Recommend: adopt-now an explicit JSON-edge-case conformance audit** (not new natives —
  pinning the existing ones' flags + a differential fixture set of edge inputs). Confidence: medium
  (needs oracle runs to confirm each pin).

### J2. `array_map` with index / `List.mapIndexed` / `forEach`-with-index  **[GAP — small]**
PHP `array_map` can take the key via a second array; TS `map((x,i)=>...)`. Phorj `List.map` is
value-only.
```
List.mapIndexed(List<T>, (int, T) -> U) -> List<U>       // HigherOrder, index passed
```
Better: explicit index (PHP requires `array_map(null, array_keys, $xs)` gymnastics). Tier A. Gated
helper (PHP `array_map` with `array_keys`). **adopt-later.** Confidence high.

---

## TIER B — IMPURE (quarantined, NOT in the byte-identity spine)

Within strings/arrays/iterables, almost everything is pure. The Tier B members are:
- **`shuffle`/`array_rand`/`str_shuffle`** — depend on RNG → only a **SEEDED** variant is Tier A
  (`List.shuffle(List<T>, int seed)` deterministic Fisher-Yates); the unseeded PHP form is Tier B
  (true randomness → quarantined, see `feasibility-Random.md`). **Recommend the seeded form (Tier A,
  adopt-later)**; reject the unseeded one from the spine.
- **`natsort`/`natcasesort` with locale**, locale-aware `sort` (`SORT_LOCALE_STRING`), locale
  `strcoll`, `setlocale`-dependent case folding — **Tier B** (locale is environment state →
  non-deterministic). Phorj should expose only the explicit byte/ASCII orderings (already does).
  **Reject locale-dependent sorting from the spine; document the ASCII/byte contract.**
- **`mb_*` (multibyte)** — not impure, but depends on the mbstring extension which `php -n` does NOT
  load (the oracle runs `php -n`). So a Unicode-aware `Text` layer **cannot transpile to `mb_*`** and
  would have to hand-roll UTF-8 in both Rust and emitted PHP. **Recommend: defer all multibyte string
  ops; keep the documented byte/ASCII contract** (this is already the project's stance per memory
  `transpile-no-ini-extensions`). A future `Core.Unicode` would need self-contained PHP UTF-8 helpers,
  not `mb_*`. Confidence high (this is a hard `php -n` constraint).

---

## Summary recommendation table (area roll-up)

ADOPT-NOW (Tier A, high value, no new Op, clear deterministic transpile):
- Strings: `Int.toHex/toRadix`, `Float.fixed`, `number_format` (A1/A2); `capitalize/uncapitalize/
  titleCase` (A3); `reverse/count/wordCount` (A4); `trimStart/trimEnd/trimChars` + **fix `trim`
  Unicode-vs-ASCII divergence** (A5).
- Lists: `unique/uniqueBy` (L1); `fill/pad` (L2); `chunk` (L3); `groupBy/indexBy` (L4); `diff/intersect`
  (L5); `find/findIndex/any/all/count` (L6); `Map.mapValues/filter/merge` (L7); `take/drop/takeWhile/
  dropWhile/flatten/flatMap` (L8); `sortBy/sortDesc` (L9).
- JSON: edge-case conformance audit + flag-pinning (J1).

ADOPT-LATER:
- `Text.format` checked-subset sprintf (A1); `replaceAll`/`strtr` longest-match (A6); `Map.entries/
  fromEntries` + `List.zip` (gated on an injected **`Pair<K,V>`** type — L7/L8); `Map.flip` (L2);
  `List.mapIndexed` (J2); seeded `List.shuffle` (Tier B-seeded).

DEFER:
- `wordwrap/chunk_split/nl2br` (A7); `List.range` stepped (L10); `pluck`/`array_column` (needs
  reflection — L4).

REJECT (from the byte-identity spine):
- Unseeded `shuffle`/`array_rand` (Tier B random); locale-dependent sorting/`natsort`/`strcoll`
  (Tier B locale); all `mb_*` multibyte ops (no `mb_*` under `php -n`).

## Cross-cutting findings (the most load-bearing)

1. **The recurring determinism trap is PHP's LOOSE comparison defaults** — `array_unique`
   (SORT_STRING), `array_diff` ((string) cast), `in_array` (loose `==`). Every list-membership/dedupe
   native MUST transpile to a **gated `__phorj_*` helper using strict `===`** to match Phorj's
   structural `eq_val`, never the bare PHP builtin. This is the single most important correctness note
   for this area.
2. **The `Pair<K,V>` injected type is the unlock** for `Map.entries`/`zip`/`fromEntries` — one
   injected-type decision (like `Json`) opens a cluster of TS-idiomatic natives. Recommend designing it
   once.
3. **`Text.trim`'s Unicode-vs-ASCII whitespace divergence is a likely live correctness bug** — Rust
   `trim()` strips all Unicode WS, PHP `trim()` strips 6 ASCII chars. Must verify the current transpile
   and pin to the ASCII set on both sides. (Graded: must-verify.)
4. **Typed formatters beat `sprintf`** — the Phorj upgrade is per-kind compile-checked formatters
   (`Int.toHex`, `Float.fixed`), not a stringly format-string DSL; ship those first, a checked-subset
   `Text.format` only if muscle-memory demand justifies it.
5. **No new `Op` anywhere in this area** — every gap is a `Op::CallNative` native (Pure or
   HigherOrder), reusing the proven generic-native + closure-invoker path. The only design decisions
   are (a) `Pair<K,V>`, (b) the strict-eq gated helpers, (c) JSON flag-pinning.

std Rust APIs relied on: `str::{find, split, replace, repeat, chars, trim_matches, split_whitespace}`,
`<[T]>::{chunks, windows}`, `Vec::{dedup_by_key, sort_by, sort_by_key}`, `f64::to_string`/Ryū path,
`i64::checked_*`, `String::from_utf8` (EV-7 fault on bad UTF-8). All std-only, zero external crates.
