# PHP Standard Library → Phorj Mapping (Strings · Arrays · Math · PCRE)

> Research date: 2026-06-18. Inventory of the four major PHP stdlib function families,
> mapped to Phorj with a 3-way honest bucket. Sources: php.net function references
> (cited at the end). Deprecation status confirmed against php.net + php.watch.

## Buckets

- **✅ core.\*** — Phorj already has it, OR it is a clean pure-Phorj native to build
  (no missing language feature). Gives the `core.<module>.<name>` target.
- **🔲 M3-blocked** — needs a language feature Phorj lacks today: **Map/Set** (assoc
  arrays — M3), **mutation** (in-place — Phorj is immutable, idiom is return-new),
  **closures/generics** (Track A — `map`/`filter`/`reduce`/`usort`). Blocker named per row.
- **❌ out-of-scope** — relies on dynamic typing, references (`&`), locale/intl C libraries,
  PHP-specific runtime (symbol table, internal array pointer), or has no static immutable
  equivalent. Deprecated/removed functions are also parked here (noted).

### Phorj today (existing natives, for reference)

- `core.text`: `len` `upper` `lower` `trim` `contains` `split` `split_once` `join` `replace`
- `core.math`: `sqrt` `pow` `floor` `ceil` `abs` `min` `max`
- `core.bytes`: `from_string` `to_string` `len` `find` `concat` `slice`
- `core.list`: **PLANNED** — `map`/`filter`/`reduce` (needs closures, Track A)

### Phorj constraints recap

- Statically typed; **immutable** (no in-place mutation — `array_push`/`sort`/`splice` have
  no mutable target; idiom = return a new value).
- `List<T>` only. **No associative arrays / Map / Set** yet (→ M3).
- **No closures** yet (→ Track A) ⇒ any callback-taking function is blocked.
- Strings are **UTF-8**; `bytes` is a separate primitive (`core.bytes`).
- **No regex engine** — PCRE would be a brand-new `core.regex` native, and Rust std has
  **no regex** (no `regex` crate allowed under the zero-dependency rule), so this is real
  pure-Rust implementation work, **not** a thin binding. Flagged as a design problem below.

---

## 1. STRINGS

| Function | Family | Phorj target | Bucket | Note |
|---|---|---|---|---|
| `strlen` | string | `core.text.len` | ✅ | EXISTS. PHP = byte length; Phorj `len` is UTF-8 chars — semantic difference, document it. Byte length = `core.bytes.len(bytes.from_string(s))`. |
| `substr` | string | `core.text.substr` | ✅ | Clean native: `substr(string, int, int?) -> string`. Negative offsets need a decision (PHP supports them). |
| `strpos` | string | `core.text.index_of` | ✅ | `index_of(string, string) -> int?` (returns null not `false`; aligns with S2 optionals). |
| `stripos` | string | `core.text.index_of_ci` | ✅ | Case-insensitive variant. ASCII-fold trivial; full-Unicode fold larger. |
| `strrpos` | string | `core.text.last_index_of` | ✅ | Last-occurrence variant. |
| `strripos` | string | `core.text.last_index_of_ci` | ✅ | CI last-occurrence. |
| `str_contains` (8.0) | string | `core.text.contains` | ✅ | **EXISTS.** |
| `str_starts_with` (8.0) | string | `core.text.starts_with` | ✅ | Clean native. |
| `str_ends_with` (8.0) | string | `core.text.ends_with` | ✅ | Clean native. |
| `str_replace` | string | `core.text.replace` | ✅ | **EXISTS** (single needle). PHP array-form needle/replacement needs Map or List pairs → that overload 🔲 (List/closures). |
| `str_ireplace` | string | `core.text.replace_ci` | ✅ | CI variant of replace. |
| `trim` | string | `core.text.trim` | ✅ | **EXISTS** (whitespace). PHP custom char-mask arg → add `trim(s, chars)` overload, clean native. |
| `ltrim` | string | `core.text.trim_start` | ✅ | Clean native. |
| `rtrim` / `chop` | string | `core.text.trim_end` | ✅ | Clean native. `chop` is an alias. |
| `strtoupper` | string | `core.text.upper` | ✅ | **EXISTS.** ASCII vs Unicode upper — pick & document. |
| `strtolower` | string | `core.text.lower` | ✅ | **EXISTS.** |
| `ucfirst` | string | `core.text.upper_first` | ✅ | Clean native. |
| `lcfirst` | string | `core.text.lower_first` | ✅ | Clean native. |
| `ucwords` | string | `core.text.title_case` | ✅ | Clean native (word-boundary upper). |
| `explode` | string | `core.text.split` | ✅ | **EXISTS** (returns `List<string>`). PHP `limit` arg → optional param, clean. |
| `implode` / `join` | string | `core.text.join` | ✅ | **EXISTS.** |
| `str_split` | string | `core.text.chars` / `core.text.chunks` | ✅ | `chars(s) -> List<string>`; chunk-size form `chunks(s, n)`. Clean native. |
| `str_repeat` | string | `core.text.repeat` | ✅ | Clean native: `repeat(string, int) -> string`. |
| `str_pad` | string | `core.text.pad_start` / `pad_end` | ✅ | Two clean natives (PHP `STR_PAD_*` flag → split into start/end/both). |
| `wordwrap` | string | `core.text.word_wrap` | ✅ | Clean native (pure string algo). |
| `strrev` | string | `core.text.reverse` | ✅ | Clean native (UTF-8-aware reverse). |
| `substr_count` | string | `core.text.count` | ✅ | `count(haystack, needle) -> int`. Clean native. |
| `str_word_count` | string | `core.text.word_count` | ✅ | Count form clean. PHP mode-2 (return words as array w/ offsets keys) needs Map 🔲. |
| `ord` | string | `core.text.ord` / `core.bytes` | ✅ | `ord(string) -> int` (first codepoint). Clean native. |
| `chr` | string | `core.text.chr` | ✅ | `chr(int) -> string`. Clean native. |
| `nl2br` | string | `core.text.nl2br` | ✅ | Clean native (newlines → `<br>`). |
| `number_format` | string | `core.text.number_format` / `core.math` | ✅ | Clean native (thousands/decimal sep). Locale-free version is fine; locale-aware → ❌. |
| `sprintf` | string | `core.text.format` | ✅ | Clean native, BUT variadic + dynamic `%` types are nontrivial under static typing — needs a typed format design (or a fixed small subset). Realistic but larger; flag as design work. |
| `printf` | string | `core.console.printf` | ✅ | = `console.print(text.format(...))`. Clean once `format` exists. |
| `vsprintf` / `vprintf` | string | `core.text.format_list` | ✅ | Args-from-List form of `sprintf`. Clean once `format` + List exist. |
| `fprintf` / `vfprintf` | string | — | 🔲 | Writes to a stream/file handle resource — needs a file-handle type (M6 IO). |
| `sscanf` | string | `core.text.scan` | 🔲 | Parses by format string into multiple typed outputs — needs tuple/multi-return or out-refs; design-heavy. Park until generics/tuples. |
| `strcmp` | string | `core.text.compare` | ✅ | `compare(a,b) -> int` (-1/0/1). Clean native. |
| `strcasecmp` | string | `core.text.compare_ci` | ✅ | CI compare. |
| `strncmp` / `strncasecmp` | string | `core.text.compare_n` | ✅ | First-N-chars compare. Clean native. |
| `strnatcmp` / `strnatcasecmp` | string | `core.text.compare_natural` | ✅ | Natural-order compare (pure algo). Clean native. |
| `str_starts_with`/`ends_with` (dup) | string | (above) | ✅ | Covered. |
| `strstr` / `strchr` | string | `core.text.after` / `before` | ✅ | Return substring from/before first needle. Clean native (return `string?`). |
| `stristr` | string | `core.text.after_ci` | ✅ | CI variant. |
| `strrchr` | string | `core.text.after_last` | ✅ | From last occurrence of a char. Clean native. |
| `strpbrk` | string | `core.text.find_any` | ✅ | First occurrence of any char in a set → `string?`. Clean native. |
| `strtr` | string | `core.text.translate` | 🔲 | Two-arg form (from/to chars) clean; **array/pair form needs Map** 🔲. |
| `substr_replace` | string | `core.text.splice` | ✅ | Replace a substring range (returns new string — immutable-friendly). Clean native. |
| `substr_compare` | string | `core.text.compare_at` | ✅ | Compare substrings at offset. Clean native. |
| `strspn` / `strcspn` | string | `core.text.span` / `cspan` | ✅ | Length of initial segment (not) in a char set. Clean native. |
| `strtok` | string | — | ❌ | Stateful tokenizer (hidden internal pointer across calls) — no immutable equivalent; use `split` instead. |
| `chunk_split` | string | `core.text.chunk_split` | ✅ | Insert separator every N chars. Clean native. |
| `htmlspecialchars` | string | `core.text.escape_html` | ✅ | Clean native (escape `< > & " '`). The quote-flag set → optional param. |
| `htmlspecialchars_decode` | string | `core.text.unescape_html` | ✅ | Inverse of the above. Clean native. |
| `htmlentities` | string | `core.text.escape_html_entities` | ✅ | Full named-entity table (larger but pure). Clean native. |
| `html_entity_decode` | string | `core.text.decode_html_entities` | ✅ | Inverse; needs entity table. Clean native. |
| `get_html_translation_table` | string | — | 🔲 | Returns an assoc array (entity map) — **needs Map**. |
| `strip_tags` | string | `core.text.strip_tags` | ✅ | Naive tag-strip is a clean native (the allowed-tags arg is best-effort, document caveat). |
| `addslashes` | string | `core.text.add_slashes` | ✅ | Clean native. |
| `stripslashes` | string | `core.text.strip_slashes` | ✅ | Clean native. |
| `addcslashes` / `stripcslashes` | string | `core.text.add_cslashes` / `strip_cslashes` | ✅ | C-style escapes; pure algo. Clean native. |
| `quotemeta` | string | `core.text.quote_meta` | ✅ | Escapes regex meta-chars (`. \ + * ? [ ^ ] $ ( )`). Clean native (pairs naturally with `core.regex`). |
| `quoted_printable_encode`/`_decode` | string | `core.text.qp_encode`/`qp_decode` | ✅ | Pure codecs. Clean natives (lower priority). |
| `bin2hex` / `hex2bin` | string | `core.bytes.to_hex` / `from_hex` | ✅ | Belongs in `core.bytes` (byte ↔ hex). Clean native. |
| `count_chars` | string | `core.text.char_counts` | 🔲 | Returns a byte→count map — **needs Map** (modes 0/1). Mode-3 (distinct chars as string) is ✅. |
| `str_getcsv` | string | `core.text.parse_csv` | 🔲 | Returns `List<List<string>>` or a row `List<string>` — row form is ✅ once List nesting is fine; multi-row + assoc header → Map. |
| `str_rot13` | string | `core.text.rot13` | ✅ | Trivial pure native. |
| `str_shuffle` | string | — | ❌ | Needs RNG + is nondeterministic (breaks the byte-identical differential spine). Could ship as a seeded `core.random` later. |
| `similar_text` | string | `core.text.similar` | ✅ | Pure algo (% + common-char count). Clean native (multi-return for the percent-by-ref → return a struct/tuple). |
| `levenshtein` | string | `core.text.levenshtein` | ✅ | Pure DP algo → `int`. Clean native. |
| `soundex` | string | `core.text.soundex` | ✅ | Pure algo → `string`. Clean native (ASCII/English only — document). |
| `metaphone` | string | `core.text.metaphone` | ✅ | Pure algo → `string`. Clean native (larger ruleset). |
| `crc32` | string | `core.bytes.crc32` / `core.hash` | ✅ | Pure checksum (Phorj already has FNV/CRC machinery in `bundle`). Clean native. |
| `md5` / `sha1` / `crypt` | string | `core.hash.*` | ✅ | Pure hash algos implementable in std Rust (no crate). Real work but feasible; lower priority. `crypt` (DES/blowfish) is larger. |
| `md5_file` / `sha1_file` | string | `core.hash.*_file` | 🔲 | Needs file IO (M6). |
| `str_increment` / `str_decrement` (8.3) | string | `core.text.increment` / `decrement` | ✅ | Alphanumeric "odometer" increment. Pure algo. Clean native (niche). |
| `str_rot13` (dup) | string | (above) | ✅ | Covered. |
| `parse_str` | string | — | ❌ | Parses a query string into variables **by reference** into an assoc array — needs refs + Map. (A pure `parse_query -> Map<string,string>` form is the M3 replacement.) |
| `sscanf` (dup) | string | (above) | 🔲 | Covered. |
| `setlocale` / `localeconv` / `nl_langinfo` | string | — | ❌ | Locale C library — out of scope (and breaks determinism). |
| `strcoll` | string | — | ❌ | Locale-based comparison — out of scope. |
| `money_format` | string | — | ❌ | **REMOVED in PHP 8.0** (was deprecated 7.4). Use `number_format` / intl. |
| `convert_cyr_string` | string | — | ❌ | **REMOVED in PHP 8.0.** |
| `hebrev` / `hebrevc` | string | — | ❌ | `hebrevc` removed 8.0; `hebrev` deprecated 8.3 — out of scope. |
| `utf8_encode` / `utf8_decode` | string | — | ❌ | **DEPRECATED 8.2, REMOVED in 9.0** (misleadingly named ISO-8859-1↔UTF-8). Out of scope. |
| `convert_uuencode` / `convert_uudecode` | string | `core.bytes.uuencode` / `uudecode` | ✅ | Pure codec (niche). Clean native if ever needed. |
| `crypt` (dup) | string | (above) | ✅ | Covered. |
| `echo` / `print` | string | `core.console.println` / `print` | ✅ | **EXISTS** (`console.println`). Language-level output. |
| `hebrev` (dup) | string | (above) | ❌ | Covered. |
| `mb_*` cluster (mbstring) | string | `core.text.*` (multibyte by default) | ✅/🔲 | **Important:** Phorj `core.text` is **UTF-8-native already**, so `mb_strlen`/`mb_substr`/`mb_strtoupper`/`mb_strpos`/`mb_str_split` are simply the **default behavior** of the `core.text` rows above — Phorj does not need a separate `mb_*` namespace. `mb_convert_encoding` / `mb_detect_encoding` (arbitrary encodings) → 🔲/❌ (encoding tables; non-UTF-8 → `core.bytes`). `mb_convert_case` = `upper`/`lower`/`title_case`. This is a **structural win**: the PHP `str_*` vs `mb_*` split collapses into one correct UTF-8 family. |

**STRINGS count: ~95 distinct functions surveyed (excl. duplicate listing rows).**
- ✅ core.* (have or clean native): **~62** (incl. the whole `mb_*` cluster folded into `core.text`)
- 🔲 M3-blocked: **~11** (Map-returning: `get_html_translation_table`, `count_chars`, `strtr` array-form, `str_word_count` mode-2, `str_getcsv` multi-row; closures/array-form: `str_replace` array overload; file IO: `fprintf`, `md5_file`, `sha1_file`; tuples/multi-return: `sscanf`)
- ❌ out-of-scope: **~14** (locale: `setlocale`/`localeconv`/`nl_langinfo`/`strcoll`; stateful: `strtok`; refs: `parse_str`; RNG/nondeterministic: `str_shuffle`; removed/deprecated: `money_format`, `convert_cyr_string`, `hebrev`/`hebrevc`, `utf8_encode`/`utf8_decode`)

---

## 2. ARRAYS

> **Two systemic blockers dominate this family:**
> 1. **Mutation** — `array_push`/`pop`/`shift`/`unshift`/`splice` + all in-place sorts mutate by
>    reference. Phorj is immutable: the idiom is **return a new List**. So these map to ✅
>    *return-new* natives where the element type is uniform (`List<T>`), but the PHP signature
>    (mutate `&$array`) does not. They are listed ✅ with a "(return-new)" note when the
>    operation is expressible on `List<T>`, and 🔲 when they also need closures or Map.
> 2. **Associative arrays** — PHP arrays are ordered maps. Anything keyed (`array_keys`,
>    `array_flip`, `ksort`, `array_column`, `array_combine`, `*_assoc`, `*_key`) **needs Map** (M3).

| Function | Family | Phorj target | Bucket | Note |
|---|---|---|---|---|
| `count` / `sizeof` | array | `core.list.len` | ✅ | Clean native: `len(List<T>) -> int`. |
| `array_map` | array | `core.list.map` | 🔲 | **PLANNED.** Needs **closures** (Track A) + generics `<T,U>`. |
| `array_filter` | array | `core.list.filter` | 🔲 | **PLANNED.** Needs **closures** (Track A). |
| `array_reduce` | array | `core.list.reduce` | 🔲 | **PLANNED.** Needs **closures** (Track A). |
| `array_walk` / `array_walk_recursive` | array | — | ❌ | Mutates each element **by reference** via callback — refs + closures + mutation. No immutable equivalent (use `map`). |
| `array_find` (8.4) | array | `core.list.find` | 🔲 | Needs closures. Returns `T?` (fits S2 optionals). |
| `array_find_key` (8.4) | array | `core.list.find_index` | 🔲 | Needs closures; index form returns `int?`. |
| `array_any` (8.4) | array | `core.list.any` | 🔲 | Needs closures. |
| `array_all` (8.4) | array | `core.list.all` | 🔲 | Needs closures. |
| `array_first` (8.5) | array | `core.list.first` | ✅ | Clean native: `first(List<T>) -> T?`. No closure. |
| `array_last` (8.5) | array | `core.list.last` | ✅ | Clean native: `last(List<T>) -> T?`. No closure. |
| `in_array` | array | `core.list.contains` | ✅ | `contains(List<T>, T) -> bool`. Needs `T: Eq` — fine for scalars/strings. |
| `array_search` | array | `core.list.index_of` | ✅ | `index_of(List<T>, T) -> int?`. Scalar/string element. |
| `array_key_exists` / `key_exists` | array | — | 🔲 | **Needs Map** (key semantics). For List it degenerates to a 0..len bound check. |
| `array_keys` | array | — | 🔲 | **Needs Map.** (List keys are just `0..len` → `range`.) |
| `array_values` | array | `core.list.values` | ✅ | For a `List<T>` this is identity / reindex — trivial; meaningful only once Map exists (Map→List). Park value-extraction-from-Map as 🔲. |
| `array_flip` | array | — | 🔲 | **Needs Map.** |
| `array_reverse` | array | `core.list.reverse` | ✅ | Clean native (return-new). The `preserve_keys` flag → Map 🔲. |
| `array_merge` | array | `core.list.concat` | ✅ | For `List<T>`: concatenation (return-new). Assoc-key merge semantics → 🔲 Map. |
| `array_merge_recursive` / `array_replace` / `array_replace_recursive` | array | — | 🔲 | **Needs Map** (key-based merge/replace). |
| `array_combine` | array | — | 🔲 | **Needs Map** (keys-array + values-array → Map). |
| `array_slice` | array | `core.list.slice` | ✅ | Clean native (return-new). Negative offsets/length = a decision. |
| `array_splice` | array | `core.list.splice` | ✅ | Return-new form: `splice(List<T>, start, len, List<T>) -> List<T>` (vs PHP mutate-by-ref). Clean once we accept return-new semantics. |
| `array_push` | array | `core.list.push` | ✅ | Return-new: `push(List<T>, T) -> List<T>` (idiom shift from PHP's mutate `&$a`). Clean. |
| `array_pop` | array | `core.list.pop` | ✅ | Return-new: returns `(List<T>, T?)` — needs **tuple/multi-return** to give both the rest + popped value. Pure value form is ✅ once tuples exist; otherwise two natives (`init` + `last`). |
| `array_shift` | array | `core.list.shift` | ✅ | Same as pop (front). Tuple/multi-return or `(rest, first)` split. |
| `array_unshift` | array | `core.list.prepend` | ✅ | Return-new prepend. Clean. |
| `array_unique` | array | `core.list.unique` | ✅ | Return-new; scalar/string `T: Eq`. Clean native (the `SORT_*` flag → optional). |
| `array_diff` | array | `core.list.diff` | ✅ | `diff(List<T>, List<T>) -> List<T>`, `T: Eq`. Clean (value-compare form). |
| `array_intersect` | array | `core.list.intersect` | ✅ | `T: Eq`. Clean native. |
| `array_diff_*` / `array_intersect_*` (`_key`/`_assoc`/`u*`) | array | — | 🔲 | `_key`/`_assoc` **need Map**; `u*` callback forms **need closures**. |
| `array_column` | array | — | 🔲 | **Needs Map** (extract a column by key from a list of assoc rows). |
| `array_chunk` | array | `core.list.chunk` | ✅ | `chunk(List<T>, int) -> List<List<T>>` (return-new). The `preserve_keys` flag → Map 🔲. |
| `array_fill` | array | `core.list.fill` | ✅ | `fill(int count, T) -> List<T>`. Clean native. |
| `array_fill_keys` | array | — | 🔲 | **Needs Map** (keyed). |
| `array_pad` | array | `core.list.pad` | ✅ | Pad to length with a value (return-new). Clean native. |
| `array_sum` | array | `core.list.sum` | ✅ | `sum(List<int>) -> int` / `List<float> -> float`. Clean (needs numeric `T` — pre-generics, two typed natives or a numeric-only signature). |
| `array_product` | array | `core.list.product` | ✅ | Same shape as `sum`. Clean native. |
| `array_count_values` | array | — | 🔲 | Returns value→count **Map**. |
| `array_change_key_case` | array | — | 🔲 | **Needs Map** (key transform). |
| `array_is_list` | array | — | ❌ | In Phorj every `List<T>` is always a list — this PHP introspection is meaningless (always true). N/A. |
| `array_key_first` / `array_key_last` | array | `core.list` (≈ `0` / `len-1`) | 🔲 | For Map → 🔲; for List the "key" is just the index, so degenerate. |
| `array_rand` | array | — | ❌ | RNG + nondeterministic (breaks differential spine). Defer to a seeded `core.random`. |
| `array_multisort` | array | — | 🔲 | Multi-array sort by reference — needs mutation + closures + multi-array coupling. |
| `sort` / `rsort` | array | `core.list.sort` / `sort_desc` | ✅ | Return-new sort of `List<T>` where `T: Ord` (int/float/string). Clean native (PHP mutates by ref; Phorj returns new). |
| `asort` / `arsort` / `ksort` / `krsort` | array | — | 🔲 | Key-association-preserving sorts — **need Map**. |
| `usort` / `uasort` / `uksort` | array | `core.list.sort_by` | 🔲 | Custom comparator — **needs closures** (Track A). |
| `natsort` / `natcasesort` | array | `core.list.sort_natural` | ✅ | Natural-order value sort (return-new). Pure comparator → clean native. (PHP's key-preservation → Map 🔲 for that aspect.) |
| `shuffle` | array | — | ❌ | RNG + nondeterministic. Defer to seeded `core.random`. |
| `range` | array | (language) `a..b` / `a..=b` | ✅ | **EXISTS as a language feature** (M3 S1.2 — `0..n` materializes `List<int>`, transpiles to PHP `range()`). Float/char ranges → extend later. |
| `compact` | array | — | ❌ | Builds an assoc array from local variable **names** (symbol-table reflection) — no static equivalent. |
| `extract` | array | — | ❌ | Injects assoc-array entries as local variables (symbol-table mutation) — no static equivalent, actively unsafe. |
| `list()` (destructuring) | array | (language) | 🔲 | PHP's `[$a,$b] = $arr` destructuring — a **language feature** (pattern binding), candidate for an M3/Track-A ergonomics slice, not a native. |
| `current`/`next`/`prev`/`reset`/`end`/`key`/`pos`/`each` | array | — | ❌ | Internal-array-pointer iteration — stateful, by-reference, no immutable equivalent. `each` is **DEPRECATED**. Use `for (x in xs)` / indexing. |

**ARRAYS count: ~75 distinct functions surveyed.**
- ✅ core.* (clean return-new / value natives, mostly `core.list.*`): **~26**
  (`len` `first` `last` `contains` `index_of` `reverse` `concat` `slice` `splice` `push` `pop`
  `shift` `prepend` `unique` `diff` `intersect` `chunk` `fill` `pad` `sum` `product` `sort`
  `sort_desc` `sort_natural` `values` + `range`-as-language)
- 🔲 M3-blocked: **~32**
  - **Closures-needed (Track A) cluster:** `array_map`, `array_filter`, `array_reduce`,
    `array_find`, `array_find_key`, `array_any`, `array_all`, `usort`/`uasort`/`uksort`,
    `array_*_u*` callback diff/intersect variants.
  - **Map-needed (M3) cluster:** `array_keys`, `array_flip`, `array_combine`, `array_column`,
    `array_fill_keys`, `array_count_values`, `array_change_key_case`, `array_merge_recursive`,
    `array_replace[_recursive]`, `*_assoc`/`*_key` diff/intersect, `asort`/`arsort`/`ksort`/`krsort`,
    `array_key_first`/`last`, `array_key_exists`, `array_multisort`.
  - **Tuples/multi-return:** the both-values forms of `array_pop`/`array_shift`.
  - **Language ergonomics:** `list()` destructuring.
- ❌ out-of-scope: **~9** (`array_walk[_recursive]`, `array_is_list` (always-true), `array_rand`,
  `shuffle`, `compact`, `extract`, internal-pointer family incl. deprecated `each`)

---

## 3. MATH

> Math is the **best-aligned** family: nearly all are pure `float`/`int` → scalar functions
> with no Map/closure/mutation. Most are clean `core.math` natives. The only ❌ are the
> arbitrary-precision C extensions (GMP/BCMath) and RNG (nondeterminism breaks the spine).

| Function | Family | Phorj target | Bucket | Note |
|---|---|---|---|---|
| `abs` | math | `core.math.abs` | ✅ | **EXISTS** (int). Add float overload. |
| `ceil` | math | `core.math.ceil` | ✅ | **EXISTS.** |
| `floor` | math | `core.math.floor` | ✅ | **EXISTS.** |
| `round` | math | `core.math.round` | ✅ | Clean native (precision arg → optional param; PHP rounding modes → flag). |
| `sqrt` | math | `core.math.sqrt` | ✅ | **EXISTS.** (Irrational results differ from PHP's 14-digit echo — KNOWN_ISSUE; run↔runvm stays identical.) |
| `pow` / `**` | math | `core.math.pow` | ✅ | **EXISTS** (`pow`). `**` operator is a language-level candidate. |
| `fpow` (8.3) | math | `core.math.fpow` | ✅ | IEEE-754 float pow. Clean native. |
| `min` | math | `core.math.min` | ✅ | **EXISTS** (int, 2-arg). Variadic + float + List form → extend. |
| `max` | math | `core.math.max` | ✅ | **EXISTS.** |
| `clamp` (8.6) | math | `core.math.clamp` | ✅ | **NEW in PHP 8.6.** `clamp(value, min, max)`. Trivial pure native. (PHP throws ValueError if min>max / NaN — Phorj → fault.) |
| `intdiv` | math | `core.math.intdiv` | ✅ | Clean native (int division; div-by-zero → fault, parity with existing arith faults). |
| `fmod` | math | `core.math.fmod` | ✅ | Float remainder. Clean native. |
| `fdiv` (8.0) | math | `core.math.fdiv` | ✅ | IEEE-754 division (inf/nan instead of fault). Clean native. |
| `pi` / `M_PI` | math | `core.math.pi` | ✅ | Constant fn (or a `const`). Clean. |
| `sin` `cos` `tan` | math | `core.math.sin`/`cos`/`tan` | ✅ | Rust std `f64` — clean natives. |
| `asin` `acos` `atan` | math | `core.math.asin`/`acos`/`atan` | ✅ | Clean natives. |
| `atan2` | math | `core.math.atan2` | ✅ | Clean native. |
| `sinh` `cosh` `tanh` | math | `core.math.sinh`/`cosh`/`tanh` | ✅ | Clean natives. |
| `asinh` `acosh` `atanh` | math | `core.math.asinh`/`acosh`/`atanh` | ✅ | Clean natives. |
| `hypot` | math | `core.math.hypot` | ✅ | Clean native (`f64::hypot`). |
| `deg2rad` / `rad2deg` | math | `core.math.deg2rad`/`rad2deg` | ✅ | Clean natives. |
| `exp` | math | `core.math.exp` | ✅ | Clean native. |
| `expm1` | math | `core.math.expm1` | ✅ | `exp(x)-1` precise. Clean native. |
| `log` | math | `core.math.log` | ✅ | Natural + optional base arg. Clean native. |
| `log10` | math | `core.math.log10` | ✅ | Clean native. |
| `log1p` | math | `core.math.log1p` | ✅ | `log(1+x)` precise. Clean native. |
| `hexdec` / `dechex` | math | `core.math.hex_to_int`/`int_to_hex` | ✅ | Clean natives (int↔hex string). |
| `bindec` / `decbin` | math | `core.math.bin_to_int`/`int_to_bin` | ✅ | Clean natives. |
| `octdec` / `decoct` | math | `core.math.oct_to_int`/`int_to_oct` | ✅ | Clean natives. |
| `base_convert` | math | `core.math.base_convert` | ✅ | Arbitrary base 2–36 string↔string. Pure algo. Clean native. |
| `is_finite` / `is_infinite` / `is_nan` | math | `core.math.is_finite`/`is_infinite`/`is_nan` | ✅ | Float predicates. Clean natives. |
| `rand` / `mt_rand` | math | — | ❌ | RNG — **nondeterministic, breaks the byte-identical differential spine.** Defer to an explicitly-seeded `core.random` (out of the auto-gated example contract). |
| `random_int` / `mt_getrandmax` / `mt_srand` / `srand` / `random_bytes` | math | — | ❌ | Same RNG/CSPRNG concern; seeded `core.random` later. |
| `gmp_*` (GMP ext) | math | — | ❌ | **Arbitrary-precision C extension** (libgmp). Out of scope (a pure-Rust bignum could be a far-future `core.bignum`, but not a stdlib mapping). |
| `bcmath` (`bcadd`/`bcsub`/…) | math | — | ❌ | **Arbitrary-precision decimal C extension.** Out of scope (same far-future `core.bignum` note). |

**MATH count: ~50 distinct functions surveyed.**
- ✅ core.* (clean `core.math.*` natives, several already exist): **~42**
- 🔲 M3-blocked: **0** (math needs no Map/closures/mutation).
- ❌ out-of-scope: **~8** (RNG family: `rand`/`mt_rand`/`random_int`/`random_bytes`/`srand`/… ;
  arbitrary-precision: `gmp_*`, `bcmath`)

---

## 4. PCRE / REGEX

> **⚠ DESIGN PROBLEM, not a trivial native.** Rust's standard library has **no regex engine**
> (the `regex` crate is forbidden by Phorj's zero-external-crate rule). A `core.regex` module
> therefore requires implementing a regex engine in pure `std` Rust — realistically a
> backtracking or Thompson-NFA matcher over UTF-8 — which is **substantial, security-sensitive
> work** (ReDoS/catastrophic-backtracking must be bounded; Phorj already enforces depth limits
> via `MAX_*_DEPTH`, so a step-bounded NFA fits the existing safety posture). The PHP↔PCRE
> transpile mapping is clean (`core.regex.match` → `preg_match`), but the **engine is the cost**.
> Recommended design: a **Thompson NFA / Pike VM** (linear-time, no catastrophic backtracking)
> supporting a documented PCRE subset (literals, char classes, `* + ?`, `{m,n}`, alternation,
> anchors, groups, common escapes) — explicitly NOT full PCRE (no backreferences/lookaround in v1,
> since those force backtracking). All the functions below depend on this single engine.

| Function | Family | Phorj target | Bucket | Note |
|---|---|---|---|---|
| `preg_match` | pcre | `core.regex.match` | ✅* | *Bucket ✅ as a clean native **once the engine exists** — the engine is the gating work, not the binding. Returns capture groups as `List<string>?` (assoc named groups → Map 🔲). |
| `preg_match_all` | pcre | `core.regex.match_all` | ✅* | Returns `List<List<string>>`. Same engine dependency. |
| `preg_replace` | pcre | `core.regex.replace` | ✅* | Pattern + replacement string (`$1` backrefs in the replacement template are fine — not in the pattern). Engine-dependent. |
| `preg_replace_callback` | pcre | `core.regex.replace_with` | 🔲 | Needs **closures** (Track A) **and** the engine. |
| `preg_replace_callback_array` | pcre | — | 🔲 | Needs closures + **Map** (pattern→callback) + engine. |
| `preg_split` | pcre | `core.regex.split` | ✅* | Returns `List<string>`. Engine-dependent. |
| `preg_grep` | pcre | `core.regex.grep` | ✅* | Filter a `List<string>` by pattern → `List<string>`. (Note: arguably needs closures only if predicate-driven; the pattern form is engine-only.) |
| `preg_quote` | pcre | `core.regex.quote` | ✅ | **No engine needed** — pure meta-char escaper (= `core.text.quote_meta`). Shippable immediately, independent of the engine. |
| `preg_filter` | pcre | `core.regex.filter` | ✅* | `preg_replace` that returns only matched subjects. Engine-dependent. |
| `preg_last_error` / `preg_last_error_msg` | pcre | — | ❌ | Global last-error state (stateful) — replaced by Phorj faults / `Result`-style returns; no global error register. |

> **Deprecation note:** the PCRE **`/e` (eval) modifier** for `preg_replace` was **deprecated in
> PHP 5.5 and removed in PHP 7.0** — it executed the replacement as PHP code; `preg_replace_callback`
> is the replacement. Not applicable to Phorj (no eval). No current `preg_*` *function* is deprecated.

**PCRE count: 11 functions surveyed.**
- ✅ core.* (1 immediately shippable: `preg_quote`; 6 gated on the regex engine): **7**
- 🔲 M3-blocked: **2** (`preg_replace_callback`, `preg_replace_callback_array` — closures; the
  latter also Map)
- ❌ out-of-scope: **2** (`preg_last_error`, `preg_last_error_msg` — global error state)
- **All ✅* functions share a single hard dependency: a pure-Rust regex engine (no std regex).**

---

## GRAND TOTALS

| Family | Surveyed | ✅ core.* | 🔲 M3-blocked | ❌ out-of-scope |
|---|---|---|---|---|
| STRINGS | ~95 | ~62 | ~11 | ~14 |
| ARRAYS | ~75 | ~26 | ~32 | ~9 |
| MATH | ~50 | ~42 | 0 | ~8 |
| PCRE | 11 | 7 (6 engine-gated) | 2 | 2 |
| **TOTAL** | **~231** | **~137** | **~45** | **~33** |

### Key 🔲 M3-blocker clusters

1. **Closures (Track A) — the single biggest unlock.** Every callback-taking function:
   `array_map` / `array_filter` / `array_reduce` (the planned `core.list` trio),
   `array_find` / `array_find_key` / `array_any` / `array_all`,
   `usort` / `uasort` / `uksort`, the `array_*_u*` diff/intersect variants,
   `preg_replace_callback[_array]`. **This is what `core.list` is waiting on.**
2. **Map / associative arrays (M3).** Every keyed operation:
   `array_keys` / `array_values`(from Map) / `array_flip` / `array_combine` / `array_column` /
   `array_fill_keys` / `array_count_values` / `array_change_key_case` /
   `array_merge_recursive` / `array_replace[_recursive]` / `*_assoc` / `*_key` /
   `asort` / `arsort` / `ksort` / `krsort` / `array_key_first` / `array_key_last` /
   `array_key_exists`; and string-side Map returners
   (`get_html_translation_table`, `count_chars`, `strtr` array-form, `str_word_count` mode-2,
   `str_getcsv` multi-row).
3. **Tuples / multi-return.** Both-values forms of `array_pop` / `array_shift`; `sscanf`;
   `similar_text` percent-by-ref. (A small ergonomics feature, not a full milestone.)
4. **File IO (M6).** `fprintf` / `vfprintf`, `md5_file` / `sha1_file`.

### Functions Phorj ALREADY covers (today, no new work)

- `core.text`: `len` (`strlen`/`mb_strlen`), `upper` (`strtoupper`/`mb_strtoupper`),
  `lower` (`strtolower`), `trim` (`trim`), `contains` (`str_contains`), `split` (`explode`),
  `split_once`, `join` (`implode`), `replace` (`str_replace`, single needle)
- `core.math`: `sqrt`, `pow`, `floor`, `ceil`, `abs`, `min`, `max`
- `core.bytes`: `from_string`, `to_string`, `len`, `find`, `concat`, `slice`
  (covers `bin2hex`/`hex2bin` neighbors once added)
- **Language-level:** `range()` → `a..b` / `a..=b` (M3 S1.2)

### Structural wins worth flagging

- **The PHP `str_*` vs `mb_*` split disappears in Phorj.** `core.text` is UTF-8-native, so the
  ~15 `mb_*` multibyte twins are simply the default behavior of the single `core.text` family —
  no parallel namespace, no encoding-flag footguns. (Only non-UTF-8 conversion goes to `core.bytes`.)
- **Optionals replace PHP's `false`-on-failure sentinel.** `strpos`/`array_search` return `int?`,
  `array_find`/`first`/`last` return `T?` — aligning cleanly with M3 S2 (`??`, `?.`, `opt!`).
- **Immutability reframes the array family**, not blocks it: ~26 array ops are clean *return-new*
  natives; only the genuinely keyed/closured/stateful ones are blocked.
- **`core.regex` is the one family with a real engineering cost** — a pure-`std`-Rust matcher
  (no `regex` crate). `preg_quote` ships free; everything else waits on the engine.

---

## Sources

- PHP String Functions — https://www.php.net/manual/en/ref.strings.php
- PHP Array Functions — https://www.php.net/manual/en/ref.array.php
- PHP Math Functions — https://www.php.net/manual/en/ref.math.php
- PHP PCRE Functions — https://www.php.net/manual/en/ref.pcre.php
- `clamp()` (PHP 8.6) — https://php.watch/versions/8.6/clamp · https://wiki.php.net/rfc/clamp_v2
- `utf8_encode`/`utf8_decode` deprecation (8.2, removed 9.0) — https://php.watch/versions/8.2/utf8_encode-utf8_decode-deprecated · https://wiki.php.net/rfc/remove_utf8_decode_and_utf8_encode
- `array_find`/`array_any`/`array_all` (8.4), `array_first`/`array_last`/`array_all`/`array_any` (8.5) — per the array reference page above
