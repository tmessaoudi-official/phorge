# Feasibility Spike — Core.Regex (PCRE / preg_*)

**Stage 2 feasibility spike.** Module: regular-expression match / replace / split / capture.
Verdict up front: **adopt-later as a DOCUMENTED, NARROW, REGULAR-ONLY subset (Tier A) — reject the
"full PCRE" framing outright.** Feasibility of the *honest narrow subset* is **~55%**; feasibility of
*PCRE byte-parity* is **~3%** and must be rejected. This is a MILESTONE, not a quick native.

---

## 1. The decisive constraint (read this first)

Phorj has **three legs that must produce byte-identical stdout**: the tree-walking interpreter
(`run`), the bytecode VM (`runvm`), and the PHP transpile output run under real `php -n`
(`tests/differential.rs`). For every other Core.* module the Rust legs share a single value-kernel
(`NativeEval::Pure`) and the PHP leg erases to a core builtin — and the two implementations are
*independently* correct against the same spec, so byte-identity is a design property, not luck.

**Regex breaks that pattern.** There is **no regex engine in Rust std** (verified: `grep -rn 'regex'`
over `src/` and an empty `[dependencies]` — only an incidental lexer comment and the bundle reader's
`nfat`). The zero-external-crate rule forbids pulling in `regex`/`pcre2`. So a Core.Regex native cannot
delegate the Rust side to anything — **we must hand-write an engine in Rust** that runs in *both* Rust
legs (they already share one body via `NativeEval::Pure`, so that half is automatically consistent),
**and that engine must agree with PHP's PCRE byte-for-byte on every input the language accepts.**

That last clause is the whole problem. PCRE (PHP's `preg_*`, libpcre2) is a backtracking engine with
backreferences, lookaround, atomic groups, possessive quantifiers, Unicode properties, `\b` word
boundaries with PCRE's locale rules, named groups, conditionals, recursion, ~50 inline-flag forms, and
documented-but-surprising edge behavior (empty-match advancement in `preg_match_all`, `$` matching
before a trailing `\n` unless `D`, UTF-8 mode under `u`, etc.). **Matching all of that by hand is
effectively infeasible** and would be a permanent silent-divergence generator: the trap is not *total*
disagreement (a fuzzer would catch that), it is **partial agreement on 95% of inputs and a silent break
on the 5%** that uses a construct we implemented slightly differently than libpcre2. Every prior-art
sweep (PHP, Python, Go) independently flagged this same wall:

- PHP digest: *"a hand-rolled engine that disagrees with PCRE on ANY construct is a silent break — the
  trap is PARTIAL agreement on a subset."*
- Python digest: *"No std-only Rust regex engine; matching PHP PCRE byte-for-byte is effectively
  infeasible."*
- Go digest: *"matching PCRE (preg_*) bit-for-bit by hand is infeasible (backrefs/lookaround/Unicode/
  greedy edges); mapping to preg_* leaves the Rust legs with no engine → breaks run==runvm; solve only
  via a shared restricted-dialect NFA across all three legs."*

The Go digest names the **only viable strategy**: do NOT transpile to `preg_*` at all. Define a
**Phorj-owned regular-language dialect** (strictly regular — no backtracking-only features), implement
it once in Rust (the shared NFA kernel, used by both Rust legs), and **emit the SAME hand-rolled
matcher as a `__phorj_regex_*` runtime helper in PHP** — *not* `preg_match`. The PHP leg then runs OUR
algorithm, byte-for-byte identical to the Rust algorithm by construction, instead of libpcre2. PCRE
becomes irrelevant to correctness; it is at most a *non-gated convenience escape hatch* (see §6).

This inverts the naive design. The starting hypothesis ("PHP target = preg_*") is **the wrong target**
for any byte-identity-gated surface. preg_* is only safe for a Tier-B, fixture-tested-outside-
differential escape hatch.

---

## 2. Determinism partition: Tier A (subset) vs the PCRE mirage

| Surface | Tier | Why |
|---|---|---|
| Hand-rolled **regular-only** dialect (Thompson NFA, leftmost-longest POSIX semantics), engine shared Rust↔Rust, **and emitted as a PHP helper** | **A** | Pure, deterministic, the SAME algorithm on all three legs → byte-identical by construction. Gateable in `differential.rs`. |
| Transpile to PCRE `preg_*` | **B (reject for gated code)** | The Rust legs have no engine to match it; partial-agreement silent breaks; PCRE version drift across PHP 8.5/8.6. Only acceptable as a non-gated, fixture-tested convenience, like `Process`/`Env` (`pure:false`). |

There is **no clock, no RNG, no I/O, no locale, no mbstring** in a pure ASCII-byte regex matcher, so the
Tier-A subset has *no ambient determinism hazard* — the ONLY determinism risk is **engine-semantics
disagreement between our Rust impl and our PHP impl**, which we eliminate by emitting the same algorithm
on both sides rather than two independent implementations. That is the single most important design
decision in this spike.

---

## 3. The documented subset (what we CAN ship)

A **strictly regular** language — implementable as a Thompson NFA with deterministic
**leftmost-longest (POSIX)** match semantics, which has *no backtracking ambiguity* and therefore a
single well-defined answer that both legs reproduce:

Supported:
- Literals (ASCII bytes), `.` (any byte except `\n` — pin this), escaped metas `\. \* \+ \? \( \) \[ \] \{ \} \| \\ \^ \$ \/`.
- Character classes `[abc]`, ranges `[a-z]`, negation `[^…]`, and a *fixed, Phorj-defined* set of
  shorthands: `\d` = `[0-9]`, `\w` = `[0-9A-Za-z_]`, `\s` = `[ \t\n\r\f\v]`, plus `\D \W \S`. **These
  are pinned to ASCII** (NOT PCRE's Unicode-aware versions) and documented as such.
- Quantifiers `*` `+` `?` and counted `{n}` `{n,}` `{n,m}` — **greedy only** (no lazy `*?`, no
  possessive `*+`: those need backtracking semantics that POSIX-NFA does not express).
- Alternation `|`, grouping `( … )`, capturing groups (numbered, leftmost-longest sub-match per POSIX).
- Anchors `^` `$` (string start/end; pin multiline OFF; pin `$` = absolute end, no trailing-newline
  special-case → simpler than PCRE).

**Explicitly rejected** (parse error at compile time, `E-REGEX-UNSUPPORTED`, so the gap is loud, never
silent): backreferences `\1`, lookahead/lookbehind `(?=…)` `(?<=…)`, atomic groups `(?>…)`, lazy/
possessive quantifiers, named groups `(?P<n>…)`, inline flags `(?i)`, Unicode properties `\p{…}`, `\b`
word boundary (PCRE's locale/Unicode `\b` is a parity minefield — defer), recursion/conditionals. A
construct we don't support **fails to compile** rather than matching differently than PCRE.

This subset is genuinely useful (validation, tokenizing, simple find/replace) and — critically — is the
subset where leftmost-longest NFA semantics give ONE answer, so the run≡runvm≡PHP-helper identity holds
by construction.

---

## 4. Byte-identity strategy (the crux, concretely)

1. **One Rust engine** in `src/native/regex/` (compile pattern→NFA, then a Thompson/Pike VM producing
   leftmost-longest captures). Lives behind `NativeEval::Pure`, so the interpreter and VM call the
   *same* function — `run≡runvm` is automatic (the value-kernel discipline, already proven for every
   other native). No new VM `Op` needed (it is `Op::CallNative`).
2. **Port the SAME algorithm to a PHP runtime helper** `__phorj_regex_match/_replace/_split`, gated by
   `uses_regex` exactly like `__phorj_div` / `__phorj_float` (`emit_runtime_helpers`,
   `src/transpile/program.rs:263`). The `php:` closure for the native emits `__phorj_regex_match(...)`,
   **NOT** `preg_match(...)`. The PHP helper is pure PHP-core string/array ops (no PCRE, no mbstring) so
   it survives `php -n`.
3. **Differential gate**: an `examples/guide/regex.phg` exercising each supported construct on
   exactly-representable ASCII inputs is globbed by `tests/differential.rs` and asserts run≡runvm≡PHP.
   Plus a dedicated `tests/regex_parity.rs` running a corpus of (pattern, input) pairs through all three
   legs. **Crucially, also a Rust-side differential against `regex`-the-crate in a `dev-dependency`
   ONLY** (never shipped): a property test that our NFA agrees with the reference engine on the
   supported subset, run in CI, catching engine bugs the small example corpus misses. (Dev-dep is
   allowed; the shipped binary stays zero-dep.)
4. **Fault parity**: invalid pattern → a clean compile-time `E-REGEX-*` diagnostic (front-end, no
   runtime fault). Runtime has no fault path for the regular subset (a match either succeeds or returns
   `None`/empty) — so it adds **no new `FaultKind`**.

If, after building the engine, we cannot make the Rust NFA and the PHP helper agree on the full subset
within budget, the fallback is to **narrow the subset further** (drop counted quantifiers, drop capture
groups → bool `isMatch` only) until parity holds — a smaller-but-honest module beats a broad-but-lying
one.

---

## 5. PHP transpile target (exact)

- **NOT `preg_*`.** Target = gated `__phorj_regex_*(string $pattern, string $subject, …)` PHP helpers
  emitting our own matcher (PHP-core only: `strlen`, `substr`, array ops; ASCII byte indexing; no
  `mb_*`, no `preg_*`). Gated by a `uses_regex` flag mirroring `uses_div`/`uses_str`
  (`src/transpile/program.rs`). Confirmed available under `php -n`: only PHP-core functions used.
- For the *convenience* escape hatch (§6) a `pure:false` `Core.Pcre` could emit real `preg_*` (PCRE is
  **verified core under `php -n`**: `preg_match`/`preg_match_all`/`preg_replace`/`preg_split`/
  `preg_quote` all `function_exists()==true` on `php-8.5.7 -n`) — but that surface is quarantined from
  the differential exactly like `Core.Process`.

---

## 6. Optional Tier-B escape hatch (separate, later)

A `Core.Pcre` module (`pure:false`) could expose `Pcre.match(pattern, subject)` transpiling directly to
`preg_match`, fixture-tested OUTSIDE `differential.rs` (the `Process`/`Env` precedent,
`docs/specs/2026-06-25-process-io-quarantine-seam-design.md`). It gives users *full* PCRE on the PHP leg
but the Rust legs would need to run our limited engine — so `run`/`runvm` could DIFFER from `php` for
any construct beyond the subset. **This is dangerous and should be gated behind an explicit opt-in / a
"PHP-target-only" annotation, or deferred entirely.** Not recommended for v1.

---

## 7. Phorj API sketch (Tier A subset)

```phorj
import Core.Regex;

// bool — does the pattern match anywhere in the subject?
bool ok = Regex.isMatch("\\d{3}-\\d{4}", "tel 555-1234");        // true

// first match + capture groups, or None
List<string>? m = Regex.match("(\\d+)-(\\d+)", "ab 12-34 cd");   // Some(["12-34","12","34"])

// all non-overlapping whole matches (group 0 only, keeps it regular + simple)
List<string> all = Regex.findAll("\\d+", "a1b22c333");           // ["1","22","333"]

// replace every match with a literal replacement (no $1 backrefs in v1 — regular only)
string r = Regex.replace("\\d", "#", "a1b2");                    // "a#b#"

// split on the pattern
List<string> parts = Regex.split(",", "a,b,c");                  // ["a","b","c"]
```

Notes: replacement is a **literal** in v1 (no `$1`/`\1` backref interpolation — that is a separate
feature with its own parity surface). `match` returns `List<string>?` (group 0 then captures) consistent
with PHP `preg_match`'s `$matches`; missing optional groups → empty string (pin this, matches PHP). All
inputs/patterns are byte/ASCII; a non-ASCII byte is matched as a raw byte (documented), never
Unicode-folded.

Native registry shape (one `NativeFn` per fn in `src/native/regex.rs`, mirroring `text.rs`): `params`
typed `[String, String]`, `ret` `Bool`/`Optional(List<String>)`/`List<String>`, `pure: true`,
`eval: NativeEval::Pure(regex_*)`, `php:` emitting the gated `__phorj_regex_*` helper.

---

## 8. New VM Op needed?

**No.** Every entry is `Op::CallNative(index, argc)` — the established generic, multi-arg, typed,
value-returning call path (the same path `Text.split` → `List<string>` uses). No `chunk.rs` /
`vm/exec.rs` / `compiler` match changes. The only "new code" is the engine module + a gated PHP helper.

---

## 9. std Rust APIs relied on

Pure `core`/`alloc`/`std::collections` only: `Vec`, `String`, `&[u8]` byte indexing, `HashMap`/`BTreeMap`
for the NFA state sets, `Rc` for the returned `Value::List`. No `std::process`, no clock, no RNG, no
filesystem. The engine is a textbook Thompson-construction + Pike-VM (≈400–700 lines Rust) — entirely
within std-only.

---

## 10. Named determinism risks

1. **Engine-semantics disagreement (THE risk)** — our Rust NFA vs our PHP helper diverging on
   leftmost-longest tie-breaking, empty-match handling in `findAll`, counted-quantifier bounds, or
   capture-group sub-match selection. *Mitigation*: ONE algorithm ported verbatim to both legs +
   property-test against the `regex` crate as a dev-dep reference + a broad parity corpus.
2. **Accidental Unicode/mbstring leakage** — if any string op in the PHP helper uses `mb_*` or PCRE's
   Unicode classes, it fails under `php -n` or diverges from byte-level Rust. *Mitigation*: pin `\d\w\s`
   to ASCII, byte-index everywhere, grep the transpiled output for `mb_`/`preg_` ([[transpile-no-ini-extensions]]).
3. **The preg_* temptation** — any future PR that "simplifies" the PHP helper to `preg_match` silently
   re-introduces PCRE-vs-our-engine divergence. *Mitigation*: a guard comment + a parity test that would
   immediately fail on a non-subset construct; document loudly in INVARIANTS.
4. **Scope creep into backtracking features** — adding `\b`, backrefs, or lazy quantifiers later breaks
   the "single POSIX answer" property the whole strategy rests on. *Mitigation*: `E-REGEX-UNSUPPORTED`
   keeps the gap loud; each new construct is its own parity spike.
5. **PCRE version drift** (only relevant to the §6 escape hatch) — `preg_*` semantics shifted across PHP
   versions; irrelevant to the Tier-A helper because it never calls PCRE.

---

## 11. Effort & recommendation

- **Effort: milestone.** This is the largest single Core.* surface — a regex *engine*, not a native
  binding. Realistically a dedicated slice (parser → NFA compiler → Pike VM → PHP helper port → parity
  corpus → example), comparable to a small VM feature, not an afternoon native. The prior-art sweeps
  (Go, Python) all explicitly reclassify it from "native" to "milestone."
- **Recommendation: adopt-later (as the narrow regular-only subset; reject full PCRE).** It is the
  single biggest stdlib gap and high user value, but it is gated on building + parity-proving a shared
  engine, which is real milestone work and should not block the cheaper pure natives (Encoding, Hash,
  Url, Csv) that ship now. Sequence it AFTER those, as its own milestone, and ship the *honest narrow*
  subset with a loud `E-REGEX-UNSUPPORTED` frontier — never a PCRE-parity promise.
- **Feasibility: ~55%** for the narrow subset shipped with full byte-identity (the engine is well-
  understood textbook work; the risk is the parity-proving budget and resisting scope creep).
  **~3%** for PCRE byte-parity — reject that framing.
- **Confidence: medium.** High confidence on the *constraint analysis* (verified: no Rust regex, PCRE
  core under `php -n`, native call path needs no Op, helper-gating mechanism exists). Medium on the
  *parity-achievability of the subset* — that can only be proven by building the engine and running the
  corpus; the % reflects that genuine unknown.
