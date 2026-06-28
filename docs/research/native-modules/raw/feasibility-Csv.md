# Feasibility Spike — `Core.Csv` (parse/format, RFC 4180)

**Stage 2 feasibility spike** · 2026-06-26 · subject: a `Core.Csv` stdlib leaf doing
RFC-4180-style CSV parsing and formatting at the string level.

**Verdict up front:** **ADOPT-NOW. Tier A (pure). std-only. No new VM Op. Feasibility ~92%.**
The single non-trivial decision is the transpile target: **do NOT map to PHP `str_getcsv`/`fputcsv`**
(version-drifting escape semantics + a hard deprecation warning under PHP 8.5 + stream ceremony for
formatting) — emit **gated hand-rolled `__phorj_csv_*` helpers** instead, exactly as `Core.Text.parseInt`
already does with `__phorj_parse_int`. With that decision the byte-identity spine is safe by
construction.

---

## 1. Determinism partition — Tier A, unconditionally

CSV parse/format is a **pure total function of its string/list argument**: no clock, no RNG, no I/O,
no object identity, no map iteration. It produces `List<List<string>>` (parse) or `string` (format)
deterministically. It belongs in the byte-identity differential like `Core.Text` and `Core.Json`.
`pure: true`.

There is exactly **one latent determinism trap and it is avoidable**: PHP's *native* CSV functions are
the non-deterministic surface, not the algorithm. See §4.

---

## 2. std-only Rust feasibility — trivially yes

The whole module is byte/`char` scanning of `&str`/`String`. APIs relied on:

- `str::chars()` / `char_indices()` — RFC-4180 is defined over ASCII delimiters/quotes; we scan
  bytes-as-chars. Embedded UTF-8 inside a field is copied through verbatim (never case-folded or
  re-encoded), so mbstring's absence under `php -n` is irrelevant — there is no multibyte *operation*,
  only passthrough.
- `String::push` / `push_str` — field/row accumulation.
- `Vec<Value>` / `std::rc::Rc::new` — building `Value::List(Rc<Vec<Value>>)`, the existing list rep
  (mirrors `text_split`'s `Ok(Value::List(Rc::new(parts)))`, verified at `src/native/text.rs:42`).

No external crate, no `unsafe`, no allocation tricks. This is strictly simpler than the already-shipped
`Core.Json` parser (which hand-rolls a recursive-descent number/string grammar in `src/native/json.rs`).

---

## 3. Byte-identity strategy (the load-bearing decision)

Three legs must agree byte-for-byte: interpreter `run`, VM `runvm`, and transpiled-PHP-under-`php -n`.

**run ≡ runvm** is free: a single `Pure(fn(&[Value], &mut String))` body is shared by both backends via
`Op::CallNative` (the structural-parity guarantee documented in `src/native/mod.rs`). One impl, two
callers — exactly like every other native.

**run ≡ PHP** is the real work, and the strategy is: **own the grammar in a `__phorj_csv_*` PHP helper,
do not call PHP's `str_getcsv`/`fputcsv`.** Rationale proven empirically below (§4). The helper is gated
by a `uses_csv_parse` / `uses_csv_format` bool on the transpiler struct (the established pattern —
`uses_div`/`uses_parse_int` at `src/transpile/mod.rs:174`, emitted once per file in
`emit_runtime_helpers`, `src/transpile/program.rs:263`). The Rust body and the PHP helper implement the
*same* pinned RFC-4180 dialect, so they cannot drift:

Pinned dialect (both legs identical):
- delimiter `,`; quote `"`; escape = doubled quote `""` (RFC 4180 §2.7), **not** backslash.
- a field is quoted on *output* iff it contains `,`, `"`, `\n`, or `\r`.
- embedded `\n`/`\r` inside a quoted field are preserved (parser is multi-row-aware: a record ends on a
  delimiter-level newline, never on a newline inside quotes).
- record terminator on **format** = `\n` (LF). (Pinned; `\r\n` is a configurable future arg, defaulted
  off — keeps the spec single and deterministic. PHP `fputcsv` defaults to `\n` too, but we don't use
  it.)
- **trailing newline on format**: the output ends with a `\n` after the last row (matches the natural
  "one line per row" mental model; pinned and documented).
- **no trailing-newline ambiguity on parse**: a final `\n` does *not* produce a trailing empty row
  (standard CSV reader behavior); an empty input string parses to `[]` (zero rows).

This is a *closed, fully specified* grammar — no PCRE, no locale, no float formatting, no Unicode
classification — so it has none of the engine-disagreement traps that sink `Core.Regex`. Floats never
enter (CSV is string-typed at this layer), so the Ryū float-divergence KNOWN_ISSUE does not apply.

---

## 4. The PHP-native trap (verified, this is why we hand-roll)

Tested against the floor `PHORJ_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php` (`php -n`):

1. **Deprecation warning poisons the oracle.** `str_getcsv("...")` with the default escape emits:
   `Deprecated: str_getcsv(): the $escape parameter must be provided as its default value will
   change` — on **stderr**, but any code path that surfaces it (or a future error_reporting change)
   breaks byte-identity. *Verified.*
2. **Backslash-escape is non-RFC4180 by default.** `str_getcsv("a\\b")` keeps `a\\b` but the legacy
   `$escape='\\'` default means a backslash before a quote suppresses RFC doubling — a silent
   divergence from a clean RFC parser. *Verified.*
3. **Explicit `escape=""` fixes both** (`str_getcsv($s, ",", "\"", "")` returns RFC-correct fields and
   silences the deprecation) — **but `str_getcsv` is single-row**: it cannot parse a multi-line string
   with embedded-newline records into multiple rows. Our `Csv.parse(string) -> List<List<string>>`
   needs whole-document, embedded-newline-aware parsing, which `str_getcsv` structurally cannot do.
   *Verified: single array returned.*
4. **`fputcsv` is a stream sink, not an expression.** It writes to a file handle and returns
   bytes-written; producing a *string* needs `fopen("php://memory","r+")` + `fputcsv(...,",", "\"", "")`
   + `rewind` + `stream_get_contents` ceremony, and under 8.5 also needs the explicit escape arg to
   avoid the same deprecation. Output `"a,b","he""q",c\n` is RFC-correct *with* explicit args.
   *Verified.*

Conclusion: every PHP-native route is either non-deterministic (deprecation), semantically wrong
(backslash escape), structurally incapable (single-row), or expression-hostile (stream). A
**~15-line hand-rolled PHP helper that mirrors the Rust scanner** is the only clean byte-identity
target — and it costs nothing extra because the gated-helper machinery already exists.

---

## 5. Exact PHP transpile target

Two gated helpers, emitted once per file when used (sketch — final lines authored against
`emit_runtime_helpers`):

```php
function __phorj_csv_parse($s) {
    $rows = []; $row = []; $field = ''; $inq = false; $started = false;
    $n = strlen($s); $i = 0;
    while ($i < $n) {
        $c = $s[$i];
        if ($inq) {
            if ($c === '"') {
                if ($i + 1 < $n && $s[$i+1] === '"') { $field .= '"'; $i += 2; continue; }
                $inq = false; $i++; continue;
            }
            $field .= $c; $i++; continue;
        }
        if ($c === '"') { $inq = true; $started = true; $i++; continue; }
        if ($c === ',') { $row[] = $field; $field = ''; $started = true; $i++; continue; }
        if ($c === "\r") { $i++; continue; }          // normalize CRLF → LF on record split
        if ($c === "\n") { $row[] = $field; $rows[] = $row; $row = []; $field = ''; $started = false; $i++; continue; }
        $field .= $c; $started = true; $i++;
    }
    if ($started || $field !== '' || count($row) > 0) { $row[] = $field; $rows[] = $row; }
    return $rows;
}

function __phorj_csv_format($rows) {
    $out = '';
    foreach ($rows as $row) {
        $cells = [];
        foreach ($row as $f) {
            $f = (string)$f;
            if (strpbrk($f, ",\"\n\r") !== false) {
                $f = '"' . str_replace('"', '""', $f) . '"';
            }
            $cells[] = $f;
        }
        $out .= implode(',', $cells) . "\n";
    }
    return $out;
}
```

All functions used (`strlen`, string indexing, `str_replace`, `strpbrk`, `implode`) are **PHP core**,
so they survive `php -n`. No `str_getcsv`/`fputcsv`, no mbstring, no ext-hash. The `php:` closures map:

- `Csv.parse(s)` → `format!("__phorj_csv_parse({})", parg(a,0))` + set `uses_csv_parse`.
- `Csv.format(rows)` → `format!("__phorj_csv_format({})", parg(a,0))` + set `uses_csv_format`.
- `Csv.parseRow(s)` → emit as `__phorj_csv_parse({}) [0] ?? []`-style, or give it its own thin helper;
  simplest is a dedicated `__phorj_csv_parse_row` that runs the scanner with newline-as-field-char.
  (Recommend: parseRow = parse the single line and return `rows[0]`, pinned to first record.)

The Rust `Pure` bodies implement the identical state machine over `s.chars()`, building
`Value::List(Rc::new(rows))` where each row is `Value::List(Rc::new(fields))`.

---

## 6. Phorj API sketch

```phorj
import Core.Csv;

// parse a whole document → rows of fields
List<List<string>> rows = Csv.parse("a,b,c\n\"x,y\",\"he said \"\"hi\"\"\",z\n");
// rows == [["a","b","c"], ["x,y", "he said \"hi\"", "z"]]

// parse a single line → one row
List<string> r = Csv.parseRow("a,\"b,c\",d");   // ["a", "b,c", "d"]

// format rows back to a string (RFC-4180, LF terminator, minimal quoting)
string text = Csv.format([["a","b"], ["needs,quote", "ok"]]);
// text == "a,b\n\"needs,quote\",ok\n"
```

Registry entries (mirroring `text_natives()` at `src/native/text.rs:265`):

| name      | params                                  | ret                              | eval   |
|-----------|-----------------------------------------|----------------------------------|--------|
| `parse`   | `[Ty::String]`                          | `List<List<String>>`             | `Pure` |
| `parseRow`| `[Ty::String]`                          | `List<String>`                   | `Pure` |
| `format`  | `[List<List<String>>]`                  | `Ty::String`                     | `Pure` |

`List<List<String>>` = `Ty::List(Box::new(Ty::List(Box::new(Ty::String))))` — nested `Ty::List` is
already used (`src/native/list.rs:246`), no type-system change.

Guide example `examples/guide/csv.phg` (round-trips parse→format, must use exactly-representable
string data — no floats — to stay byte-identical; auto byte-identity-gated by the `examples/**/*.phg`
glob).

---

## 7. New VM Op? — NO

Every entry is `Op::CallNative(idx, argc)`, the existing dispatch. No new `Op`, no `Value` variant
(reuses `Value::List`/`Value::Str`), no `chunk.rs`/`vm/exec.rs`/`compiler` match changes. This is a
purely additive native leaf + one new `src/native/csv.rs` registered in `mod.rs build()` (append after
the existing leaves; slot order only matters for the pinned `CONSOLE_PRINTLN=0` constant, which is
untouched).

---

## 8. Named determinism risks (and why each is closed)

1. **PHP `str_getcsv` deprecation / escape drift** — CLOSED by not using it (§4). The hand-rolled
   helper has no version-sensitive behavior.
2. **Embedded newline / multi-row** — CLOSED: the scanner is whole-document and quote-state-aware in
   both legs (the single most common CSV correctness bug; handled by construction).
3. **CRLF vs LF** — pinned: parser normalizes `\r` away at record splits; formatter emits LF only.
   A future `--crlf` arg would be a new pinned param, never a locale/OS read.
4. **Trailing newline / trailing empty row** — pinned: format always ends `\n`; parse does not emit a
   trailing empty row for a final `\n`. Both legs encode the identical rule. (This is the classic
   CSV-roundtrip footgun — call it out in the guide and a differential case.)
5. **Float formatting** — N/A: CSV is string-typed here; floats never enter the parse/format path.
   (A caller that wants numeric cells uses `Core.Text.parseFloat`/`Convert` *after* parse — that path
   already routes through `__phorj_float`.)
6. **mbstring absence under `php -n`** — N/A: byte-level scanning + passthrough only; no multibyte op.
7. **Empty-field vs absent-field** — pinned: `"a,,b"` → `["a","","b"]` (three fields); `""` (empty
   input) → `[]` (zero rows). Encode both in differential cases.

---

## 9. Effort & recommendation

- **Effort: small.** One new `src/native/csv.rs` (~3 natives + the shared Rust scanner, ~120 lines),
  two `uses_csv_*` flags + two helper bodies in `emit_runtime_helpers`, one `mod csv;` +
  `csv_natives()` call in `mod.rs::build()`, one guide example, and ~6 differential cases
  (basic, embedded-comma, embedded-quote-doubling, embedded-newline, empty-input/empty-field,
  roundtrip). No backend plumbing, no Op, no Value, no type-system change.
- **Recommendation: ADOPT-NOW.** It is a pure, std-only, no-new-Op Tier-A leaf that fills a genuine
  real-world gap (config/data interchange) and pairs naturally with the already-shipped
  `Core.Json`/`Core.Text`. The one design subtlety (hand-roll, don't map to `str_getcsv`) is *proven*
  and reuses an existing mechanism.

**Confidence: high** — every claim (str_getcsv deprecation/escape/single-row, fputcsv stream ceremony,
nested `Ty::List`, gated-helper pattern, shared `Pure` eval) was verified against the 8.5 floor binary
and the live source tree, not recalled.

**Feasibility: ~92%** (the residual 8% is the usual roundtrip-edge bikeshedding — trailing newline,
empty-row, CRLF — all design-decidable, none blocking).
