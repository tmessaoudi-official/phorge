# Stage 2b — Adversarial Byte-Identity Review: `Core.Csv`

**Subject:** the spike claim that `Core.Csv` (parse/format, RFC 4180) stays byte-identical across
`run` (interpreter) ≡ `runvm` (VM) ≡ real PHP 8.5 (`php -n`), tier A, feasibility 92%.

**Verdict: the determinism/byte-identity claim HOLDS.** I attempted to refute it and could not. The
spike's strategy (hand-rolled grammar mirrored in a gated `__phorj_csv_*` PHP helper, no
`str_getcsv`/`fputcsv`) is sound, and the empirical evidence below confirms the two hand-rolled legs
agree byte-for-byte on every edge case I could construct — including the genuinely dangerous ones the
spike did not test. `determinism_holds = true`. Feasibility revised modestly **up** to ~94% (the spike's
residual 8% was roundtrip-bikeshedding; my testing closed several of those concretely).

This is a *qualified* hold: it depends on two pre-existing invariants of the rest of the language (the
type checker forbidding non-strings into `List<List<String>>`, and the native-arg `Value::Str` guard).
Those are NOT new risks introduced by CSV; they are the same trust boundary every typed native relies
on. I flag them as residual, not as refutations.

---

## What I tested (all against the floor binary `php-8.5.7/bin/php -n`)

I ported the spike's exact PHP `__phorj_csv_parse`/`__phorj_csv_format` helpers AND wrote the
equivalent Rust char-scanning scanner (the `Pure` body the spike describes), then ran identical inputs
through both and diffed the outputs.

### Parse — 21 cases, all byte-identical between PHP byte-scan and Rust char-scan

Basic, embedded comma (`"x,y"`), quote doubling (`"he said ""hi"""`), embedded newline in quotes,
empty input → `[]`, empty fields (`a,,b` → 3 fields), `x\n`, multibyte field (`café`), 4-byte emoji
(`😀`) adjacent to a delimiter and inside quotes, lone `\r` → `[]`, `a\rb` → `[["ab"]]`, CRLF rows,
`\r\n` alone → `[[""]]`, unclosed quote (`"unclosed` → `[["unclosed"]]`), bare quote mid-unquoted-field
(`a"b,c\n` → `[["ab,c\n"]]`, a degenerate-but-consistent swallow), `\n` → `[[""]]`, `,\n` → `[["",""]]`,
`a,\n` → `[["a",""]]`, `""\n` → `[[""]]`, `a\n\n` → `[["a"],[""]]`.

### Format — 9 cases, all byte-identical

minimal-quoting triggers (`,` `"` `\n` `\r`), quote doubling, empty fields, empty rows list → `""`,
single empty row `[[]]` → `"\n"`, multibyte passthrough. Trailing-`\n` terminator confirmed on both.

---

## Refutation attempts and why each FAILED to break byte-identity

### A1 — Byte-scan (PHP `$s[$i]`) vs char-scan (Rust `chars()`) — the strongest candidate. CLOSED.
The PHP helper indexes **bytes**; the Rust body the spike specifies scans **chars** (Unicode scalars).
This is a real representational difference. It does NOT diverge because UTF-8 guarantees every byte of a
multibyte sequence is ≥ 0x80, so an ASCII delimiter/quote/CR/LF byte (`, " \r \n`, all < 0x80) can never
appear *inside* a multibyte char. Both scanners therefore split at exactly the same offsets and copy
multibyte content through verbatim. **Verified** with `café` and a 4-byte emoji `😀` placed adjacent to
`,` and inside `"…"`: outputs identical (`[["café","b"],["😀\"x"],["z"]]` on both). This was the spike's
weakest-justified claim ("byte-level scan + UTF-8 passthrough") and it survives — but the spike should
note explicitly that PHP scans bytes while Rust scans chars, and that the equivalence is *load-bearing
on the ASCII-delimiter invariant*. If a future config arg ever allowed a multibyte delimiter, this would
instantly break — a real future-proofing landmine worth pinning in the design.

### A2 — Lone `\r` / old-Mac CR line endings — divergence I expected. CLOSED.
`\r` is unconditionally skipped (`$i++` / `i+=1`) in both legs, never treated as a record separator. So
`"a\rb"` → `[["ab"]]` (CR silently deleted, data corruption — but **identical** corruption on both
backends), `"\r"` → `[]`, `"a,b\rc,d"` → `[["a","bc","d"]]`. Both legs agree. This is a *correctness*
wart (CR-only CSV is silently mangled) but NOT a byte-identity break. Recommend the design document it as
a pinned non-feature.

### A3 — `(string)$f` PHP coercion vs Rust `Value::Str` strict match — the real residual. NOT a CSV bug.
PHP's `(string)$f` would coerce `null`→`""`, `true`→`"1"`, `42`→`"42"`, a float via PHP formatting; the
Rust `Pure` body (per the established `text.rs` pattern) pattern-matches `Value::Str` and **faults** on
anything else. If a non-string field ever reached `format`, PHP would silently coerce while Rust faults →
hard divergence. The defense is entirely upstream: `format` is typed `List<List<String>>`, and S2
null-safety guarantees a non-optional `String` is never `Null`. So this is closed *by the type checker*,
not by the CSV code. Two caveats the spike omitted:
  - The `(string)$f` cast in the PHP helper is therefore **dead defensive code** under a sound checker.
    It should arguably be a no-op or removed for honesty — leaving it in masks the divergence if the
    checker ever lets a `mixed` through.
  - The spike's own KNOWN_ISSUES note the erased-generic `mixed`/`CTy::Other` hole and the same-head
    generic-invariance hole. If CSV `format` is ever called with a value flowing through one of those
    holes, the coerce-vs-fault gap is live. This is a *transitive* risk inherited from generics, not
    introduced by CSV — but it is the one place the 92% should not round to 100%.

### A4 — `strpbrk` quoting-trigger vs Rust `contains` — CLOSED.
`strpbrk("", ",\"\n\r")` → `false` (empty field stays unquoted), matching Rust `"".contains(..)`. NUL
byte (`a\0b`) → no trigger, stays unquoted, passes through verbatim on both. Identical.

### A5 — Roundtrip identity — NOT claimed, correctly. CLOSED as a footgun, not a break.
`format([[]])` → `"\n"`, then `parse("\n")` → `[[""]]` — roundtrip is NOT identity for the empty-row
case (and `format` of a list whose last logical row was "no trailing newline" cannot be recovered). The
spike correctly does NOT claim roundtrip identity and flags trailing-newline/empty-row as pinned
decisions. Both backends agree on each *direction*, which is all byte-identity requires. No refutation.

### A6 — `php -n` missing extensions — CLOSED.
Every PHP function used (`strlen`, `[]` indexing, `str_replace`, `strpbrk`, `implode`, `count`) is PHP
core, present under `php -n` (verified: the helpers ran clean under `-n` with zero warnings). No
mbstring, no ext-hash, no `str_getcsv`/`fputcsv` (which the spike correctly avoids — I did not re-verify
the deprecation since the chosen path sidesteps it entirely). No float formatting → Ryū divergence N/A,
confirmed (no float ever enters the string-typed layer).

### A7 — Hidden non-determinism (object ids, hash ordering, clock, addresses, RNG) — CLOSED.
None present. Output is `List<List<string>>` / `string` built by sequential append; no map iteration
(`Value::Map` ordering is irrelevant — none used), no object identity, no clock, no RNG, no FS. `pure:
true` is correct.

---

## Residual risks (do not block adoption; pin in design)

1. **[medium]** The `(string)$f` cast is dead code under a sound checker but live divergence if a
   non-string ever reaches `format` via the erased-generic `mixed` hole. Recommend: keep the Rust `Err`
   strict, and treat any non-`Value::Str` field as a *checker bug to surface*, not silently coerce.
2. **[low]** PHP byte-scan vs Rust char-scan equivalence is load-bearing on ASCII-only delimiters. A
   future multibyte-delimiter config arg would break it. Pin "delimiter is a single ASCII byte" in the
   spec.
3. **[low]** CR-only (old-Mac) line endings are silently mangled identically on both legs — a
   correctness wart, not a byte-identity break. Document as a pinned non-feature.

## Confidence
**High.** Every claim was verified by running both the spike's exact PHP helper and an equivalent Rust
scanner against the floor `php-8.5.7 -n` binary across ~30 inputs, not recalled. The two non-CSV
residuals (A3, A1-future) are honestly transitive/conditional, not present defects.
