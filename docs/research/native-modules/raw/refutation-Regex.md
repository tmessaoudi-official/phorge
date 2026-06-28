# Stage 2b — Adversarial Byte-Identity Refutation — Core.Regex

**Verdict: determinism_holds = FALSE for the spike *as written*.** The spike's headline strategy
("one algorithm ported verbatim to both legs → byte-identical by construction") is *directionally
correct* and is the only sane approach — but the spike treats byte-identity as an achieved design
property when it is in fact an **unbuilt, unproven hypothesis**, and it contains at least one
**structural contradiction** that makes the proposed `List<string>` API unrepresentable for the very
"raw byte" inputs it promises to support. The 55% is honest about the parity-budget risk but the spike
then asserts identity "by construction" in §2/§4/§7 as if it were already true. Those two postures
contradict each other; the second is the one that fails review.

The refutations below are concrete byte-level divergence vectors. R1 and R2 are hard blockers as the
API is sketched; R3–R6 are real parity traps the "verbatim port" framing does not actually neutralize.

---

## R1 — STRUCTURAL CONTRADICTION: `Value::Str` is UTF-8; "raw byte" sub-matches are unrepresentable
**Grade: Verified (read `src/value.rs:28`).**

`src/value.rs:28` declares `Str(String)` — a Rust `String`, which is **statically guaranteed valid
UTF-8**. The spike (§3, §7 Notes) promises: *"All inputs/patterns are byte/ASCII; a non-ASCII byte is
matched as a raw byte (documented), never Unicode-folded."* and returns matches as `List<string>`
(`Value::Str`).

These are mutually exclusive. If a pattern matches a substring of a subject that contains an invalid
UTF-8 byte sequence (e.g. a lone `\xff`, which §"substr" probe confirms PHP stores fine as a 1-byte
string), the Rust engine **cannot** construct a `Value::Str` holding it — `String::from_utf8` errors,
`from_utf8_lossy` would silently substitute U+FFFD (3 bytes `EF BF BD`). Meanwhile the PHP helper,
operating on PHP's byte-string (`strlen("\xff")==1`, verified), happily returns the raw `\xff`. The
differential then compares the Rust leg's lossy/erroring output against PHP's raw-byte stdout →
**guaranteed divergence on any non-ASCII-byte capture**, OR an outright panic/fault on the Rust leg
that PHP does not produce (itself a parity break under the fault-classification rules).

The spike already ships a `Value::Bytes(Rc<Vec<u8>>)` variant (`src/value.rs:31`) precisely because
`Str` can't hold arbitrary bytes — yet the regex API is sketched entirely in `string`, and the spike
never reconciles this. **Either** the subset must be narrowed to *reject non-ASCII bytes in subject
and pattern at the boundary* (making the "matched as a raw byte" promise false and shrinking the
module), **or** the API must return `bytes`/`List<bytes>` (changing the whole §7 sketch and its PHP
mapping). The spike commits to neither. As written, the `string` API is a silent-divergence generator
the moment a byte ≥ 0x80 enters.

---

## R2 — The 55% is for an UNBUILT engine; "byte-identical by construction" is asserted, not proven
**Grade: Verified (spike §10.1 names this THE risk; §240 admits "can only be proven by building").**

The spike's own §10 lists "engine-semantics disagreement … our Rust NFA vs our PHP helper diverging on
leftmost-longest tie-breaking, empty-match handling … counted-quantifier bounds, or capture-group
sub-match selection" as **THE risk**, and §11 confidence note concedes parity "can only be proven by
building the engine and running the corpus." That is the definition of `determinism_holds = unproven`.
"Identical by construction" (§2 line 66, §4.1) is only true if a *single* implementation runs on all
three legs. It does **not**: there are demonstrably **two** implementations — a Rust NFA and a
hand-ported PHP helper in a *different language with different integer/string/array semantics*. "Ported
verbatim" across Rust→PHP is not "the same code"; it is two codebases that must be kept in lockstep by
test discipline. Every other Core.* native gets byte-identity because the PHP leg erases to a
**single PHP-core builtin** whose behavior is fixed; here the PHP leg is *bespoke Phorj-authored PHP*
that can drift from the Rust engine on any of the four sub-behaviors §10 enumerates. The claim under
review ("can stay byte-identical") is therefore **not established by the spike** — it is deferred to
future build work with a 45% chance of needing the fallback narrowing.

---

## R3 — Empty-match advancement in `findAll`/`split`/`replace` is a known PHP-specific rule, not a
free "POSIX answer"
**Grade: Inferred (PCRE/PHP empty-match semantics are well-documented; not re-run here because the
helper is hand-rolled, but the divergence vector is real).**

The spike waves at "leftmost-longest POSIX semantics = single well-defined answer." That is true for a
*single* match position. It is **not** true for the *iteration* primitives `findAll`, `split`, and
`replace`, where the engine must decide what to do when a pattern matches the **empty string** at a
position (e.g. `Regex.findAll("a*", "baab")`, or `Regex.split("", "abc")`, or `Regex.replace("x*", "-",
"ab")`). PHP/PCRE has a *specific, surprising* advancement rule (advance by one code unit after an
empty match; `preg_split` with `PREG_SPLIT_NO_EMPTY` differs again; PHP 7.3+ changed empty-match
behavior). POSIX-NFA libraries (and the `regex` crate, see R4) have *different* empty-match iteration
conventions. There is no canonical "single answer" here — there are at least three (PCRE, POSIX,
Rust-`regex`), and the Phorj Rust engine and the Phorj PHP helper must pick the **same** one and
encode it identically in two languages. The spike lists this in §10.1 as a sub-bullet of the risk but
its §3 "single well-defined answer" framing **understates it**: leftmost-longest disambiguates
*overlapping alternatives at one position*, not *how the cursor steps after a zero-width match*. This
is exactly the "partial agreement on 95%, silent break on 5%" trap the spike warns about elsewhere —
and the warned-about trap is *inside its own supported surface*, not only in the rejected PCRE features.

---

## R4 — The proposed CI oracle (`regex` crate as dev-dep) does NOT define POSIX leftmost-longest
**Grade: Verified (the `regex` crate's documented semantics are leftmost-FIRST / Perl-like for
captures, not POSIX leftmost-longest).**

§4.3 / §10.1 lean on "a property test against the `regex` crate as a dev-dependency reference" as the
mitigation that "catches engine bugs the small example corpus misses." But the `regex` crate
deliberately implements **leftmost-first (Perl/PCRE-style) match and capture semantics by default, NOT
POSIX leftmost-longest** — it only offers leftmost-longest behavior in restricted ways and explicitly
documents the difference for alternation and sub-capture. The spike *chose* POSIX leftmost-longest
(§3, §"single well-defined answer") precisely to avoid backtracking ambiguity. So the reference engine
the spike proposes to verify against **answers a different question** for any pattern where the two
disambiguation policies differ (e.g. `(a|ab)(c|bcd)` over `abcd`: POSIX-longest vs leftmost-first pick
different sub-captures). A property test against `regex` would either (a) flag false divergences on
every such pattern, drowning real bugs, or (b) be quietly restricted to patterns where the two agree —
in which case it does **not** verify the semantics actually shipped. The single named external
oracle in the strategy is the wrong oracle for the chosen semantics. This undercuts the primary R2
mitigation.

---

## R5 — Counted-quantifier bounds + integer width: Rust `usize`/`i64` vs PHP `int` (`{n,m}` and large `n`)
**Grade: Verified (PHP `PHP_INT_SIZE==8`, signed 64-bit; verified `php -n`).**

`{n,m}` bounds are parsed integers. A hand-ported PHP helper parsing `{0,9999999999999999999}` (or a
catastrophic `{2147483648}`) must clamp/overflow **identically** to the Rust engine. Rust will use
`usize`/`u32` for repeat counts (overflow → debug panic, release wrap — itself a determinism hazard per
the project's checked-arithmetic invariant EV-7); PHP `int` is signed 64-bit and silently coerces huge
numeric strings via `intval` semantics or float-promotion. The spike's §3 lists `{n,m}` as supported
but never specifies the bound's integer domain or the over-large-bound behavior. Two independently
hand-written parsers in two languages with different integer models will not agree on the boundary
cases unless the bound domain is pinned and *both* implementations reject out-of-range bounds at compile
time with the identical rule. Not addressed.

---

## R6 — Compile-time vs run-time fault placement diverges run/runvm from PHP
**Grade: Inferred (from the spike's own §4.4 claim that pattern validation is front-end/compile-time).**

§4.4: *"invalid pattern → a clean compile-time `E-REGEX-*` diagnostic (front-end, no runtime fault)."*
This is only possible when the pattern is a **compile-time-constant string literal**. The API (§7) takes
`pattern` as an ordinary `string` parameter — nothing stops `Regex.match(userInput, subject)` where the
pattern is computed/dynamic. For a dynamic invalid pattern, the Rust legs would fault at runtime (engine
returns an error / panics), while the PHP helper would fault *its* way (or, worse, `preg_*`-style return
`false`) — and the spike has declared **no new `FaultKind`** (§4.4) for this path. A regex module that
only accepts literal patterns is far narrower than §7 implies; one that accepts dynamic patterns
re-introduces a runtime fault surface the spike explicitly says it doesn't have, and that fault must be
byte-identical across three legs (the differential's `agree_err` classifies by FaultKind — there is none
reserved). Either way the spike's fault story is incomplete and a divergence vector.

---

## What the spike got RIGHT (so the refutation is fair)
- The core inversion — **do NOT transpile to `preg_*`; emit a Phorj-authored PHP helper** — is correct
  and is the only path that *could* hold. Verified: `preg_*` is core under `php -n` (so the temptation
  is real), and `mb_*` is absent — the §10.2 mbstring-leakage guard is a genuine, correct concern.
- `Op::CallNative` suffices, no new VM Op (Verified against the established multi-arg native path; the
  `Text.split → List<string>` precedent shows list-of-string round-trips fine *for valid UTF-8*).
- The `E-REGEX-UNSUPPORTED` "loud frontier" is the right discipline for rejected constructs.
- Rejecting PCRE byte-parity (~3%) is correct and well-argued.

None of those rescue the byte-identity *claim*: a correct strategy that is unbuilt and contains an
unrepresentable-value contradiction (R1) does not yet "stay byte-identical."

---

## Revised assessment
- **Tier: mixed** (unchanged — the honest narrow subset is Tier-A-*intended*; the PCRE escape hatch is
  Tier B). But "intended Tier A" ≠ "proven Tier A."
- **determinism_holds: FALSE** — for the spike as written. R1 (UTF-8 vs raw-byte) is a hard
  contradiction at the API surface; R3/R4 show the "single POSIX answer" mitigation is weaker than
  claimed and its named external oracle is the wrong one; R2 establishes the whole identity claim is an
  unbuilt hypothesis, not a property.
- **Revised feasibility: 35%** (down from 55%). The engine is textbook (that part is sound), but the
  spike under-counts the parity surface: two-language verbatim porting of empty-match cursor
  advancement + counted-bound integer domains + capture sub-match selection, *plus* a forced API
  redesign to handle the raw-byte contradiction, *plus* the absence of a correct external oracle. The
  fallback (drop captures → bool `isMatch` on ASCII-only, reject non-ASCII bytes at the boundary) is
  genuinely byte-identity-safe and I'd put **that** narrower surface at ~70% — but the spike's
  advertised `List<string>` capture-returning subset is the thing under review, and it sits at ~35%.

To make determinism actually hold, the spike must: (1) resolve R1 by either rejecting bytes ≥0x80 at
the boundary or returning `bytes`; (2) pin and co-specify empty-match advancement + counted-bound
integer domain as part of the *spec*, not the engine; (3) replace the `regex`-crate oracle with a
POSIX-leftmost-longest reference (or a second independent hand-impl), since `regex` is leftmost-first;
(4) define the dynamic-invalid-pattern fault path with a reserved FaultKind or restrict patterns to
literals. None of these is impossible — but until they exist, byte-identity is asserted, not held.
