# Track M — i18n / text — Roadmap Gap Audit

## Track summary

Phorj's text model today is **byte-oriented ASCII by design**. `Core.Text.len` is `str.len()`
(UTF-8 *byte* count → PHP `strlen`); `upper`/`lower` are `to_ascii_uppercase`/`_lowercase` →
`strtoupper`/`strtolower` (C-locale ASCII); `contains`/`split`/`splitOnce`/`join`/`replace` are all
byte/substring operations → `str_contains`/`explode`/`implode`/`str_replace`. The `string` type is a
UTF-8-validated Rust `String` (so it *holds* Unicode), and `bytes` is the raw-octet escape hatch, but
**no operation in the language is Unicode-aware**: no grapheme/codepoint counting, no normalization,
no locale-aware case folding, no collation, no locale-formatted numbers/dates/currency, and no message
catalogs. This is a *coherent* state, not a bug — it is forced by two hard invariants: (1) the
correctness spine requires `run ≡ runvm ≡ real PHP` byte-identically, and (2) the PHP oracle runs
`php -n`, so **mbstring and ext-intl/ICU are absent** — the only Unicode the tier-1 PHP leg can do is
PCRE with the `/u` modifier. That means almost the entire classic i18n surface (the ICU-backed
`Normalizer`, `Collator`, `IntlDateFormatter`, `NumberFormatter`, `MessageFormatter`,
`Transliterator`, `grapheme_*`, `IntlBreakIterator`) cannot be transpiled to tier-1 PHP at all, and a
pure-Rust reimplementation would either pull in an external ICU-data dependency (violates
zero-dependency) or ship megabytes of Unicode tables std-only (a real but bounded effort).

The philosophy verdict colors the whole track. PHP's i18n is a genuine *pain point* a PHP dev feels
daily — the byte/char `strlen` trap, the mbstring-vs-intl split, locale-global `setlocale` state, and
the "is my string normalized?" question. Phorj's "remove surprises, provably-correct upgrade"
mission is *tailor-made* to fix the small, high-value end of this: codepoint-correct length/iteration,
a clear `string`-is-UTF-8 contract, and an explicit byte/char distinction (which Phorj already has
via `bytes`). The **large** end (full ICU collation, locale-data formatting, CLDR message catalogs)
is real capability but a poor philosophy fit *right now*: it is heavyweight, data-table-bound, and —
critically — **untranspilable to `php -n`**, so it would either fork the backends off the spine or
force a tier-3 extension policy. Recommendation pattern below: **adopt** the small codepoint-correct
core and the explicit-encoding contract (high value, modest effort, PHP-familiar); **defer** the
normalization/case-folding/segmentation layer to a dedicated Unicode-data milestone; **reject /
defer-hard** the ICU-locale-data features (collation, locale number/date/currency formatting) until a
tier-3 extension mechanism or a v2 ICU-data decision exists — they break the zero-dep + `php -n`
oracle today.

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| M-codepoint-len | Codepoint-aware length & indexing (`Text.chars`, char count) | port | strong | adopt | M-text S1 | M |
| M-encoding-contract | Explicit `string`=UTF-8 contract + byte↔char bridge docs/ops | new | strong | adopt | M-text S1 | S |
| M-grapheme | Grapheme-cluster correctness (user-perceived characters) | port | ok | defer | M-text S3 (Unicode-data) | L |
| M-unicode-case | Unicode-aware `upper`/`lower`/`fold` (locale-independent) | port | ok | defer | M-text S2 (Unicode-data) | L |
| M-normalization | Unicode normalization (NFC/NFD/NFKC/NFKD) | port | ok | defer | M-text S2 (Unicode-data) | L |
| M-collation | Locale-aware collation / sorting (`Collator`) | port | weak | reject | — (needs ICU data) | L |
| M-num-fmt | Locale-aware number/currency formatting (`NumberFormatter`) | port | weak | defer | M-locale (tier-3 ext) | L |
| M-date-fmt | Locale-aware date/time formatting (`IntlDateFormatter`) | port | weak | defer | M-locale (tier-3 ext) | L |
| M-msg-catalog | Message catalogs / translation (gettext / ICU MessageFormat) | port | weak | defer | M-locale (tier-3 ext) | L |
| M-segmentation | Text segmentation (word/sentence/line break) | port | weak | defer | M-text S3 (Unicode-data) | L |
| M-transliterate | Transliteration / ASCII-folding (`Transliterator`) | port | weak | reject | — (needs ICU data) | L |
| M-text-breadth | Tier-1 byte-string stdlib breadth (`startsWith`/`endsWith`/`indexOf`/`substring`/`repeat`/`padStart`/`padEnd`/`reverse`) | port | strong | adopt | M-text S1 | M |
| M-regex | Regular expressions (`Core.Regex`, PCRE `/u` Unicode mode) | port | strong | adopt | M-text S1 | M |
| M-string-fmt | `sprintf`-style typed formatting / template fns | map | ok | defer | M-text S2 | M |
| M-unicode-escape | Unicode escapes in string literals (`\u{1F600}`) | port | strong | adopt | M-text S1 | S |
| M-ascii-divergence | Document/guard byte-vs-char divergence in `Text.len`/case under non-ASCII | omit | strong | adopt | M-text S1 | S |

## Rationale per ADOPT item

### M-codepoint-len — Codepoint-aware length & iteration
The single highest-value, most-PHP-familiar fix in the track. PHP's `strlen("é")` returning 2 (bytes)
when the user means 1 (character) is the canonical i18n footgun; PHP devs reach for `mb_strlen`
precisely to escape it. Phorj can offer a *named, unambiguous* pair — keep `Text.len` as the byte
length (it already is, and that maps cleanly to `strlen`), and add `Text.charCount` / `Text.chars`
(→ `List<string>` of single codepoints) for codepoint semantics. The Rust side is trivial
(`s.chars().count()`, `s.chars()`); the **transpile** is the catch — codepoint length under `php -n`
is `mb_strlen`-absent, so it must lower to a tier-1 PCRE form: `preg_match_all('/./u', $s)` returns the
codepoint count, and `preg_split('//u', $s, -1, PREG_SPLIT_NO_EMPTY)` splits into codepoints — both
work under `php -n` (PCRE is tier-1). This keeps the byte-identity spine intact *and* fixes the
language's biggest text surprise. Adopt in the first text slice; it unblocks correct interpolation
width, truncation, and any "first N characters" logic.

### M-encoding-contract — Explicit UTF-8 contract + byte↔char bridge
Phorj already has the right *bones*: `string` is UTF-8-validated and `bytes` is the raw-octet type
with `Core.Bytes.fromString`/`toString` (the `toString` even returns `string?` for invalid UTF-8).
The gap is that this contract is implicit and under-documented, so a dev can't reason about when an
operation is byte-wise vs char-wise. The adopt is small and mostly *specification + a couple of bridge
ops*: state in `docs/INVARIANTS.md`/FEATURES that `string` is always-valid-UTF-8, that `Core.Text.len`
is bytes, and provide the explicit conversions so a dev *chooses* the axis rather than being surprised
by it. This is pure philosophy-fit ("remove surprises, never capability") and rides on existing
machinery — no new `Op`, no backend change, mostly natives + docs. Low effort, high clarity payoff.

### M-text-breadth — Tier-1 byte-string stdlib breadth
`Core.Text` is missing the everyday byte-safe string operations a PHP dev expects: `startsWith`/
`endsWith` (→ PHP 8.0 `str_starts_with`/`str_ends_with`), `indexOf` (→ `strpos`, returns `int?`),
`substring` (→ `substr`), `repeat` (→ `str_repeat`), `padStart`/`padEnd` (→ `str_pad`), `reverse`
(→ `strrev`). All are tier-1 PHP builtins, all byte-wise (so byte-identical with the Rust `&str`/byte
ops), all camelCase-named per Phorj convention. This is purely additive on the already-generic
native call path (no plumbing change, mirroring the Wave-2 `Core.Text` landing) and closes the most
glaring "I can't do basic string work" gap. Strong fit, modest mechanical effort (each fn = one
registry entry + one Rust kernel + one PHP mapping + a byte-identity example case). Caveat to encode:
the *char-indexed* variants (substring by codepoint) belong to M-codepoint-len, not here — this slice
is the honest byte layer.

### M-regex — Regular expressions with PCRE `/u` Unicode mode
Regex is the one place Phorj can get *real Unicode behavior on the `php -n` oracle today*, because
PCRE (with the `/u` modifier and `\p{...}` property classes) is tier-1 — it needs no mbstring/intl.
A `Core.Regex` module (`match`/`matches`/`find`/`findAll`/`replace`/`split`) transpiling to
`preg_match`/`preg_match_all`/`preg_replace`/`preg_split` gives Phorj codepoint-aware search,
property-class matching, and Unicode-correct splitting — the practical 80% of i18n text work — without
violating zero-dependency (Rust std has no regex engine, so the *interpreter/VM* side needs a small
std-only matcher OR this is gated as a transpile-first native with a hand-rolled minimal engine for the
Rust backends). The byte-identity risk is real and must be designed carefully (PCRE vs a Rust matcher
must agree on the supported subset), so the slice should ship a **deliberately restricted, fully-spec'd
regex subset** where both legs provably agree, growing the subset only as parity is proven. High value,
strong PHP familiarity (`preg_*` is daily PHP); effort is M-to-L because of the dual-engine parity
burden — flagged here as the design's central risk.

### M-unicode-escape — Unicode escapes in string literals (`\u{1F600}`)
A tiny, pure-front-end win that is conspicuously absent: Phorj string literals support `\xHH` in
`bytes` literals but there's no `\u{...}` codepoint escape in `string` literals, so writing a non-ASCII
character requires pasting the raw UTF-8 bytes into source. Adding `\u{1F600}` / `é` lexing
(producing the UTF-8 bytes at lex time) is a lexer-only change, transpiles to the identical UTF-8 bytes
in the PHP string literal (byte-identical for free), needs no new `Op`, and matches the
TypeScript/Rust/modern-PHP (`"\u{...}"`) form a dev expects. Small effort, strong fit, removes a real
authoring papercut.

### M-ascii-divergence — Document & guard the byte-vs-char divergence
Right now `Text.len` (bytes) and `Text.upper`/`lower` (ASCII-only) silently do the "wrong" thing on
non-ASCII input — `upper("café")` leaves the `é` untouched, and `len("café")` is 5, not 4. Today this
is *byte-identical with `php -n`* (so the spine is green) but it is exactly the kind of silent surprise
the philosophy forbids. The adopt is the cheap, honest move: explicitly document these as
ASCII/byte-wise (a KNOWN_ISSUES entry + FEATURES note), and — where the codepoint-correct variant lands
in M-codepoint-len — keep the byte variant clearly named so the dev *chooses*. No code beyond naming
and docs; pairs with M-codepoint-len and M-encoding-contract to make the byte/char axis fully legible.
This is the "remove the surprise even if you don't yet add the capability" minimum.

## Notes on the DEFER / REJECT items (why they don't earn their surprise budget yet)

- **M-normalization, M-unicode-case, M-grapheme, M-segmentation** are all genuine PHP features
  (`Normalizer`, `mb_convert_case`, `grapheme_*`, `IntlBreakIterator`) and *good* fits philosophically
  (they remove real surprises), but every one needs **Unicode data tables** (case-folding maps,
  normalization decomposition data, grapheme/word-break property tables). Shipping those std-only is a
  bounded but real multi-megabyte effort, and **none of them transpile to `php -n`** (they all require
  mbstring/intl). They belong in a dedicated **M-text S2/S3 "Unicode-data" slice** that first decides
  the tier policy: either (a) embed generated Unicode tables in the Rust backends and accept that these
  functions are *Phorj-native-only* (the PHP leg would need tier-3 ext or be excluded from the oracle),
  or (b) wait for a tier-3 extension mechanism. Defer, don't reject — they're on-mission, just gated.

- **M-collation, M-num-fmt, M-date-fmt, M-msg-catalog, M-transliterate** are the **ICU-locale-data**
  features (`Collator`, `NumberFormatter`, `IntlDateFormatter`, `MessageFormatter`/gettext,
  `Transliterator`). These are weak fits *for now*: they are locale-data-bound (CLDR), inherently
  stateful/locale-global (the exact PHP pain Phorj wants to avoid replicating naively), and
  fundamentally untranspilable to `php -n` (ext-intl absent). Reproducing ICU in std-only Rust is out
  of proportion to a pre-1.0 language. They are **deferred to a future M-locale milestone gated on a
  tier-3 extension policy** (the `docs/specs/2026-06-19-extension-policy-design.md` mechanism), where
  the locale/ICU surface can live as an *opt-in extension* rather than core — preserving the
  zero-dependency core and the `php -n` spine. `M-collation` and `M-transliterate` are marked **reject**
  (not merely defer) for the core roadmap because, unlike formatting, they have no acceptable tier-1
  approximation and no near-term milestone — they should only re-enter if the tier-3 ICU decision is
  made.

- **M-string-fmt** (`sprintf`-style formatting) is `map`/defer: Phorj already has type-safe string
  *interpolation* (`"{x}"`), which covers the common case more safely than `sprintf`; a positional
  width/precision formatter is a convenience that maps onto interpolation + a small `Core.Text.format`
  native (→ PHP `sprintf`) later, once the locale-aware number formatting question is settled (so the
  two don't ship a conflicting number-rendering story).

## Critic pass

**Verification of shipped state.** I read `FEATURES.md`, `KNOWN_ISSUES.md`, the project `CLAUDE.md`
milestone log, and `src/native.rs`. The confirmed `Core.Text` surface is **`len`, `upper`, `lower`,
`trim`, `contains`, `split`, `splitOnce`, `join`, `replace`** (native.rs:309–390). `Core.Bytes` has
`fromString`/`toString`/`len`/`concat`/`slice`/`find`. The lexer (lexer.rs:140–320) supports
`\n \t \r \\ \"` in `string` and `\xHH` in `bytes` only — **no `\u{…}`**. This corroborates the
researcher's "byte-oriented ASCII by design" summary.

**Mis-listings (already shipped):** none. Every item on the original list is genuinely absent. The
nearest call is `bytes.find` (shipped) vs the proposed text `indexOf` (M-text-breadth) — these are
distinct (byte-offset on `bytes` vs `int?` index on `string`), so M-text-breadth is **not** a
mis-listing. `removed_mislisted = 0`.

**Newly-found items (4):**

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| M-number-format | Non-locale `number_format` (thousands sep + fixed decimals) | port | strong | adopt | M-text S1 | S |
| M-codepoint-int | Codepoint ↔ int conversion (`Text.codepointAt` / `Text.fromCodepoint`) | port | ok | adopt | M-text S1 | S |
| M-ci-compare | ASCII case-insensitive compare / search (`equalsIgnoreCase` / `indexOfIgnoreCase`) | port | strong | adopt | M-text S1 | S |
| M-string-escapes | Complete the double-quoted escape set (`\0`, `\e`, octal/hex in `string`) | port | ok | defer | M-text S2 | S |

Rationale for each:

- **M-number-format** — the researcher folded *all* number formatting into the ICU `NumberFormatter`
  (M-num-fmt, deferred to tier-3). But PHP's plain **`number_format($n, $decimals, '.', ',')` is a
  tier-1 core builtin** — no ext-intl, runs under `php -n`. It is the single most-reached-for "show me
  `1,234.50`" function in everyday PHP and is fully transpilable + byte-identical (the Rust side is a
  trivial digit-grouping kernel). Distinct from the locale-aware (CLDR) story, which legitimately
  defers. Strong fit, S effort. Adopt in the same byte-tier slice as breadth.

- **M-codepoint-int** — PHP `ord`/`chr` are byte-level (tier-1) and `mb_ord`/`mb_chr` are intl. A
  codepoint↔int bridge (`Text.codepointAt(s, i) -> int?`, `Text.fromCodepoint(int) -> string?`) is the
  natural companion to M-codepoint-len: Rust side is `s.chars().nth(i)` / `char::from_u32`, and it
  transpiles to a small tier-1 PCRE/`IntlChar`-free helper (`mb_*` absent, so emit a `preg`-based or
  arithmetic UTF-8 encode/decode helper — codepoint→UTF-8 bytes is pure arithmetic, no tables). Lets a
  dev work with a single character as a number. `ok` fit (slightly lower-traffic than length), S effort.

- **M-ci-compare** — `equalsIgnoreCase`/`containsIgnoreCase`/`indexOfIgnoreCase` over ASCII map to
  `strcasecmp`/`stripos`/`str_ireplace` (all tier-1, byte/ASCII, byte-identical). Case-insensitive
  comparison is an extremely common everyday need the breadth list omitted (it lists only case-sensitive
  ops). Honest byte-ASCII layer, consistent with M-ascii-divergence's "clearly named byte/ASCII variant"
  discipline. Strong fit, S effort.

- **M-string-escapes** — the `string` escape set is just `\n \t \r \\ \"`. PHP double-quoted strings
  also accept `\0`, `\e`, `\f`, `\v`, octal `\NNN`, and `\xHH`. Adding the missing ones (lexer-only,
  emit the byte at lex time, byte-identical transpile to the same PHP escape) removes a small authoring
  papercut. `defer` (lower priority than `\u{…}`, which is the high-value one already on the list as
  M-unicode-escape) — but it is a genuine, separately-trackable gap. S effort.

**Recommendation sanity-check against philosophy.** All four newly-found items pass the
"most-PHP-familiar, legible, pragmatic" test: each maps to a named tier-1 PHP builtin a PHP dev already
knows, each is byte-identical on the `php -n` oracle, none introduces PL-theory surprise. The original
list's adopt/defer/reject split is sound — in particular the ICU/CLDR cluster (collation,
locale-num/date/currency, message catalogs, transliteration, segmentation) correctly defers/rejects
because it breaks zero-dep + the `php -n` spine. **One refinement to the original:** the researcher's
M-num-fmt rationale should explicitly carve out the tier-1 `number_format` case (now captured as
M-number-format) so the deferred ICU work isn't read as deferring *all* number display. No other
recommendation changes.

`new_found = 4`, `removed_mislisted = 0`.
