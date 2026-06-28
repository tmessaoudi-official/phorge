# Native Modules — Research SSOT

> Single source of truth for the "rich native modules" initiative (Stage 5 synthesis).
> Consolidates: prior-art sweep (`priorart-{php,python,go,rust}.md`), 12 per-module feasibility spikes
> (`feasibility-*.md`), the PHP-can/Phorj-can't upgrade-lens sweep (`phpgap-{strings-arrays,types-runtime}.md`),
> two designs (`design-dump.md`, `design-sql.md`), and the adversarial byte-identity review
> (`refutation-*.md`, `refute-*.md`, `adversarial-Validate.md`, `stage2b-refutation-Csv.md`).
> Plan + Decisions Log: `docs/plans/2026-06-26-native-modules-research.plan.md`.
> Language-level parity SSOT (companion, not superseded): `docs/specs/2026-06-21-php-parity-and-beyond.md`.

---

## 1. Executive summary + the Determinism Partition

The initiative asked: which "rich" PHP/ecosystem capabilities (var-dumper, URL, hashing, HTTP, DB,
regex, …) can Phorj ship as `Core.*` native modules — and how? The answer is decided **before**
usefulness by one framing:

**The Determinism Partition.** Phorj's correctness spine is three backends (`run` tree-walker,
`runvm` stack VM, Phorj→PHP transpiler under real `php -n` 8.5) that must produce **byte-identical
stdout** (`tests/differential.rs`). A module is feasible *as a first-class native* only if its output is
a **pure, deterministic function** of its inputs:

- **Tier A — pure/deterministic** → byte-identity-gateable, std-only, ships like any `Core.*` module
  with a guide example globbed by `differential.rs`. *Eligible for the spine.*
- **Tier B — impure/non-deterministic** (clock, true-RNG, sockets, DB connections, filesystem writes)
  → **cannot** be in the byte-identity spine. Quarantined behind the M6 `Transport` trait, fixture-tested
  *outside* `differential.rs`, `pure: false` (the shipped `Core.Process`/`Core.Env`/`Core.File` precedent),
  transpiled to PHP.

**The verdict of the adversarial pass: tier is the right axis, but "Tier A" is not a clean bill of
health.** Six of the eight modules judged Tier A by the feasibility spikes carry a *silent byte-identity
divergence* the spike missed — almost always at the **PHP leg**, where Phorj's rich, statically-tagged
`Value` enum (Map/Set/Bytes/Decimal/Enum) collapses onto PHP's untyped array/string runtime, or where a
PHP-core builtin's semantics differ from a hand-rolled Rust kernel (PCRE `$` anchor, `intdiv` vs
`div_euclid`, `gmmktime` legacy-year pivot, loose `array_unique`/`array_diff` comparison). **These are
fixable** — every one resolves to a *static-type tag*, a *gated `__phorj_*` helper*, an *anchor/operator
pin*, or a *narrowed example surface* — but they move the module from "free by construction" to "free
*after* a named, design-time mitigation." The partition stands; the per-module honesty bar is higher
than the spikes claimed.

**Three hard constraints drive everything** (all Verified against the repo): std-only Rust
(`[dependencies]` empty — **no TLS, no regex engine, no http/serde crate**); the byte-identity spine;
and the PHP 8.5 transpile floor run under `php -n` (so **mbstring and most ext are ABSENT**; BCMath is
loaded via `-d`, PCRE and `hash` are core). Crypto/TLS/secure-RNG must NOT be hand-rolled.

---

## 2. Per-module ADOPT / DEFER table

Tier shown is the **revised** tier after the adversarial pass. `determinism_holds=false` means the
feasibility spike's "Tier A, free by construction" claim was **overturned** — the module is still
shippable but only with the named mitigation (§3).

| Module | Tier | Feas% | Conf | New Op? | Effort | Recommendation | One-line rationale |
|---|---|---|---|---|---|---|---|
| **Core.Hash** | A | 95 | high | no | small | **adopt-now** | crc32/md5/sha1/sha256 hand-rolls; `hash` ext is non-disableable core; only impl-bug risks (endianness, hex case). |
| **Core.Encoding** | A | 96 | high | no | small | **adopt-now** | base64/hex/url codec; **encode** dir is clean — only **decode** semantics need pinning to PHP's lenient `base64_decode` skip-set. |
| **Core.Csv** | A | 92 | high | no | small | **adopt-now** | byte-scan == char-scan (ASCII delims safe in UTF-8); all helpers PHP-core under `-n`; no hidden non-determinism. |
| **Core.Validate** | A | 88 | high | no | small | **adopt-now** | `filter_var`-better typed `T?` predicates; **PCRE `$`→`\z` anchor fix** is mandatory (else trailing-`\n` diverges). |
| **Core.Random** | A | 88 | high | no | medium | **adopt-now** | *seeded* PRNG = deterministic; **PRNG constants must be `<2^63`, shifts `1..=63`, rejection loop must avoid PHP-float `/`/`*`**. |
| **Core.Url** | A | 80 | high | no | medium | **adopt-now** | codec+query slice high-conf; **parse/build RFC-3986 scanner ≈65%** (own-the-edge-matrix on three legs unproven). |
| **Core.Dump** | mixed | 88 | high | no | medium | **adopt-now** (w/ static-tag fix) | var-dumper; **Map key-type / empty-Map-vs-List / Bytes / Set / Enum lose their Phorj kind on the PHP leg** → static-type-tagged emission is the central fix. |
| **Core.Sql** | A | 92 | high | no | medium | **adopt-later** | typed injection-safe query *builder*; safe-by-construction (parameterize, never inline); blocked-API fixes are in `design-sql.md` (binds = `Json`). |
| **Core.Time** | mixed | 85 | high | no | medium | **adopt-now** (pure parts only) | date arithmetic/format/parse pure; `now()` Tier B; **`diffDays` floor-vs-trunc, `gmmktime` year-pivot + out-of-range normalization** are silent traps → v1 literal-format-only. |
| **Core.Http** | B | 55 | medium | no | medium | **defer** | no std TLS; **native faults are NOT byte-identical run≡runvm** (VM adds a line prefix), HTTPS asymmetry is a hidden run-divergence, the new `TcpStream` transport has *no* parity gate. |
| **Core.Regex** | mixed | 55 | medium | no | milestone | **adopt-later** | no std regex engine; **two independent impls (Rust NFA + hand-ported PHP) can drift**; `Value::Str` can't hold raw non-UTF-8 captures; empty-match advancement + `{n,m}` integer domain diverge. |
| **Core.Db** | B | 25→~0 | high | no | milestone | **adopt-later** | **empty `[dependencies]` + `forbid(unsafe)` ⇒ Rust legs cannot open a connection at all** → two legs produce nothing, byte-identity fails unconditionally; honest gateability ≈0%, PHP-only feature. |

Upgrade-lens (`phpgap-*`) modules — **stdlib breadth, all Tier A, no new Op** — summarized in §5; they
feed the existing **M4 stdlib-breadth** plan, not the 12-step initiative order.

---

## 3. Where the adversarial review OVERTURNED a feasibility claim

Six modules had `determinism_holds=false`. Each is shippable *after* the named mitigation.

### Core.Dump — `determinism_holds=false` (revised tier: **mixed**)
The PHP leg cannot recover Phorj's static `Value` kind at runtime — multiple refutations, one root cause:
- **P0 — Map key-type destroyed.** `HKey{Int,Bool,Str}` keys → bare PHP arrays; PHP coerces keys
  (Verified `php -n 8.5.7`: `true=>1`, `false=>0`, `"5"=>5`). `Map<bool,V>` and numeric-string-keyed maps
  silently diverge.
- **P0 — empty Map vs empty List indistinguishable.** Both emit `[]`; `array_is_list([])===true` (the
  proposed disambiguator *is* the predicate that collides them).
- **P1 — `Value::Enum` rendering entirely unanalyzed** by the spike's trap table (Option/Result/Json/
  RoundingMode/user enums are pervasive; an enum payload can itself carry a Map, inheriting both P0s).
- **P2 — float special-value tokens** (NaN/inf/-0) must route through `as_display`/`__phorj_float`, not
  a literal `format!("{x}")`.
- **P2 — string/Bytes escape scheme asserted, not pinned** (char-oriented Rust vs byte-oriented PHP).
- **Mitigation (already in `design-dump.md`):** the dump native's `php` mapping is **static-type-tagged at
  the call site** — `__phorj_dump($x,'set'|'bytes'|'decimal'|'map'|'closure')` (mirrors `Reflect.kind`/
  `Convert.toInt`); a pinned 5-sequence escape scheme; floats via `__phorj_float`; Enum is the one kind
  needing an impl-time check against the actual enum lowering. **This is the design's central decision.**

### Core.Encoding — `determinism_holds=false` (revised tier: **A**)
- **`base64_decode($s,true)` is NOT strict** — it *skips* `{space,\t,\n,\r}`, *tolerates* missing padding
  (`'aGk'→'hi'`), and *masks* non-canonical trailing bits (Verified). A hand-rolled "strict" Rust decoder
  to the spike spec would **reject** ordinary line-wrapped MIME/PEM base64 → divergence. The spike stated
  the divergence backwards (Rust too strict, not PHP too lenient).
- **`base64_encode` never line-wraps** (pin "no wrapping"); **`hex2bin` rejects all whitespace** (opposite
  of base64's skip-set — shared decoder logic breaks one leg); **`rawurldecode` is byte-total** so a
  `string?` return needs a PCRE `//u` validity wrap to mirror Rust `from_utf8→null`.
- **Mitigation:** match PHP's *actual* lenient decode semantics (skip-set, padding tolerance) in the Rust
  kernel, not RFC-strict; per-decoder whitespace policy; PCRE `//u` validity check on decode-to-`string?`.
  Encode direction + Tier-A classification are Verified-correct.

### Core.Random — `determinism_holds=true` but **5 transpile traps** (revised tier: **A**)
The determinism *holds* (seeded), but the PHP transpile has live integer-model traps:
- **R1/R2 (HIGH):** PHP `/` is **always float** (`PHP_INT_MAX/6` → float); any 64-bit constant `≥2^63`
  becomes a PHP float. xorshift64 masks are safe (`<2^63`); the spike's xoshiro256++/splitmix *upgrade*
  constants (`0x9E3779B97F4A7C15`) are un-representable → silent divergence. A rejection bound needs
  `intdiv`, never `*range`.
- **R3 (MED):** shift-count `≥64` diverges (PHP `1<<64=0`, Rust masks mod 64). Pin shifts to `1..=63`.
- **R4 (LOW-MED):** a shared-mutable injected `Rng` makes draw-order load-bearing (the null-op-scratch-slot
  bug class) → differential MUST include a two-draws-in-one-expression case.
- **Mitigation:** assert all PRNG constants `<2^63` at transpile time; multiply-free, `intdiv`-based
  rejection; compile-time shift amounts; prefer the stateless-functional core.

### Core.Time — `determinism_holds=false` (revised tier: **mixed**)
- **`diffDays` sign divergence (CRITICAL, self-contradiction):** spike mandates `div_euclid` (floor) in the
  kernel but transpiles to `intdiv` (trunc). Verified: `intdiv(-86401,86400)=-1` vs `(-86401).div_euclid(86400)=-2`.
  Only manifests when the first arg is the earlier instant → every later-minus-earlier example passes green.
- **`gmmktime` year-pivot (CRITICAL):** PHP coerces 2-digit/small years (`70→1970`, `0→2000`); a literal
  `days_from_civil` kernel won't reproduce it.
- **`gmmktime` out-of-range normalization (HIGH):** PHP silently normalizes (`Feb 30→Mar 1`, `month 13→
  next Jan`); kernel must either reject (then diverges) or replicate rollover exactly.
- **Dynamic `gmdate` letter leak (MED):** PHP passes unknown letters literally (`gmdate('Q')='Q'`) and has
  real letters the pinned table omits (`'S'`→ordinal).
- **Mitigation:** **v1 = literal-format-only (compile-time check), NOT a dynamic helper**; `diff` must use
  `intdiv`-truncation semantics consistently across legs; pin/replicate the year-pivot + normalization, or
  restrict `toUnix` to 4-digit in-range inputs in v1.

### Core.Validate — `determinism_holds=false` (revised tier: **A**)
- **R1 PRIMARY:** PCRE `$` matches before a trailing `\n` — `preg_match('/…$/',"a@b.com\n")===1` on
  `php -n 8.5.7` but a Rust end-of-input scanner returns false. **Fix (Verified): replace `$` with `\z`.**
- **R2 CORROBORATING:** the *already-shipped* `__phorj_parse_float` helper has the identical `$`-anchor bug
  (`"1.5\n"`→1.5 on PHP, `Err` on Rust) — a real undetected bug class → file separately / KNOWN_ISSUES.
- **R6:** the spike's example set has no trailing-`\n` input → false assurance; a `"a@b.com\n"=>false`
  differential case must regression-gate the `\z` fix. `isInt`/`isIpv4` are Verified-safe.

### Core.Db — `determinism_holds=false` (revised tier: **B**, honest feas ≈0%)
- **MISSING-BACKEND (decisive):** empty `[dependencies]` + `#![forbid(unsafe_code)]` mean `run`/`runvm`
  **cannot open any connection** — no driver crate, no FFI, no hand-rolled MySQL crypto. Two legs produce
  *nothing* while PDO returns rows → byte-identity fails unconditionally, independent of DB
  non-determinism. Stronger than the Process/Env precedent (there all three legs produce real results).
- A closed `Value` enum (no `Resource` variant) means a live connection handle forces a core-invariant
  change. The 25% conflates *eventual PHP-only shipping* with *byte-identity gateability*; the honest
  gateable figure is **~0%**. **The pure, gateable half is `Core.Sql` (the builder) — ship that
  independently; `Core.Db` is PHP-only execution, deferred to a milestone.**

*(Core.Http `determinism_holds=false` is already Tier B — its refutations are about the quarantine being
incomplete, not about a mis-classified Tier A; see §2 row and the build-order note in §4.)*

---

## 4. Recommended BUILD ORDER (reconciled with the plan's 12-step order)

The plan's 12-step order (`…native-modules-research.plan.md` §"Recommended build order") is **largely
preserved** — its dependency logic (Encoding→Hash, Url→Validate, Sql→DB, HTTP-pattern→DB-pattern; Tier A
before Tier B; Regex last) survives the research. The research **adjusts** it on two axes: (a) **re-rank
within Tier A by *post-mitigation* risk**, promoting the modules whose adversarial pass came back cleanest;
(b) **down-grade Core.Db from "#11, ~0% as Tier A" to an explicitly PHP-only milestone feature**, and
flag that Core.Http's quarantine needs a harness edit the spike denied.

**Tier A (build first — establishes the modular native pattern):**

1. **Core.Hash** *(was #4)* — **promoted to #1.** Cleanest adversarial pass (95%, only impl-bug risks:
   endianness crossover, hex casing — both caught by published known-answer vectors, not backend
   divergence). Smallest, foundational (Hash needs hex from Encoding — see #2). *Rationale: highest
   value÷*post-mitigation*-risk; the `hash` ext is non-disableable core so no harness probe needed.*
2. **Core.Encoding** *(was #3)* — base64/hex/url codec, **a dependency of Hash's hex output and of Url**.
   Adopt-now *with* the lenient-decode mitigation (§3). *Rationale: foundational; encode dir is trivially
   correct, decode needs the one pin.*
   *(1↔2 ordering note: Encoding supplies Hash's hex; if hex is hand-rolled inside Hash they are
   independent. Either order works — build them as an adjacent pair.)*
3. **Core.Csv** *(was #8)* — **promoted.** 92%, the adversarial hunt came back fully clean (byte-scan ==
   char-scan proven, all helpers core under `-n`, no hidden non-determinism). Self-contained data chore.
4. **Core.Dump** *(was #1)* — **kept high but after the clean trio**, because its adversarial pass found
   the **central static-type-tag fix** (§3): highest DX value, but the *mixed* tier means it now carries a
   real design-time decision (per-kind PHP rendering), so it ships after the no-surprise modules prove the
   pattern. *Rationale: still the biggest DX payoff; `design-dump.md` is complete and de-risks it.*
5. **Core.Validate** *(was #7)* — needs the `\z`-anchor fix (§3) but otherwise small/clean. Pairs with the
   type system; pulls forward of Url because its only dependency on Url (email/url validation) is optional.
6. **Core.Random (seeded)** *(was #5)* — adopt-now with the integer-model mitigations (§3 R1–R5). Foundational
   for deterministic UUID/test fixtures. *Rationale: unlocks the M-Test deterministic seam.*
7. **Core.Url** *(was #2)* — **kept, but split-confidence:** ship the **codec+query slice** (high conf) first;
   the **parse/build RFC-3986 scanner is ~65%** (own-the-edge-matrix on three legs) → treat as a second
   sub-slice with an explicit edge differential corpus.
8. **Core.Sql** *(was #6)* — **adopt-later** (the builder). Materially-new piece = injecting a
   *method-bearing class pair* (no shipped prelude does this; Json/RoundingMode are bare enums). De-risk by
   writing `Query`/`Sql` as a normal user file, gate green, then lift into `SQL_PRELUDE`. Binds = `Json`
   enum (closes the heterogeneous-list compile-blocker). See `design-sql.md`.
9. **Core.Time (pure parts)** *(was #9)* — **v1 literal-format-only** (§3); `now()` is Tier B. Date
   arithmetic/format/parse are gateable but carry the `diffDays`/`gmmktime` traps — slot the pure parts into
   Tier A, quarantine `now()`.

**Tier B (build last — establishes the Transport pattern, lives outside the spine):**

10. **Core.Http** *(was #10)* — first Tier-B capability. **Caveat the research adds:** "ZERO harness edits"
    is **false** — native faults are not byte-identical (VM adds a source-line prefix), so `classify()` in
    `differential.rs` needs a new `FaultKind::Network` arm keyed on a single-sourced body substring; and the
    two **project**-globs never call `uses_impure_native`, so an Http example authored as a *project* would
    hit live network. **Keep Http examples single-file** and add the classify arm. The new `TcpStream`
    transport itself has **no parity gate** (only a canned mock) — gate it outside `differential.rs`.
11. **Core.Db** *(was #11)* — **reclassified PHP-only milestone feature, NOT a tri-backend native.** Rust
    legs cannot connect (§3). Consumes the #8 Sql builder; transpiles to PDO; fixture-tested outside the
    spine. Honest byte-identity gateability ≈0% → it is the deferred *execution half* of Sql, not a Tier-A
    candidate.
12. **Regex** *(was #12)* — **kept last; milestone-sized, ~55%, medium conf.** Two independent impls (Rust
    NFA + hand-ported PHP) *can drift*; `Value::Str` can't hold raw non-UTF-8 captures (use `Value::Bytes`);
    empty-match advancement and `{n,m}` integer domain diverge; the proposed `regex`-crate oracle implements
    leftmost-**first**, not the chosen POSIX leftmost-**longest** → wrong oracle. Subset-or-defer; likely its
    own milestone with a built-engine + corpus before any claim.

**Net change vs the plan:** within Tier A, re-rank to **Hash → Encoding → Csv → Dump → Validate → Random
→ Url → Sql → Time** (clean-pass modules first, design-heavy/split-confidence modules after); Tier B order
unchanged but **Http needs a harness edit** and **Db is PHP-only, not Tier-A-gateable**.

---

## 5. PHP-can / Phorj-can't + better-port findings (the upgrade lens)

From `phpgap-strings-arrays.md` + `phpgap-types-runtime.md`. **All Tier A, no new Op** — every gap is an
`Op::CallNative` native (Pure or HigherOrder) reusing the proven generic + closure-invoker path. These
feed the **M4 stdlib-breadth** plan (`docs/plans/2026-06-26-m4-stdlib-breadth.plan.md`), *not* the 12-step
initiative. Deduped against the language parity SSOT (which tracks *syntax* gaps); the items below are
*library capability* gaps only.

**ADOPT-NOW (high value, clear deterministic transpile):**
- **Strings:** `Int.toHex`/`toRadix`, `Float.fixed`, `number_format` (typed formatters beat stringly
  `sprintf`); `capitalize`/`uncapitalize`/`titleCase`; `reverse`/`count`/`wordCount`;
  `trimStart`/`trimEnd`/`trimChars`.
- **Lists:** `unique`/`uniqueBy`; `fill`/`pad`; `chunk`; `groupBy`/`indexBy` (genuine upgrade — PHP has no
  native `groupBy`); `diff`/`intersect`; `find`/`findIndex`/`any`/`all`/`count` (PHP has no `some`/`every`/
  `find`); `Map.mapValues`/`filter`/`merge`; `take`/`drop`/`takeWhile`/`dropWhile`/`flatten`/`flatMap`;
  `sortBy`/`sortDesc`.
- **`Core.Debug.export`** (the `var_export` sibling — re-parseable *Phorj* literal source) — one extra
  renderer arm on the Dump module.
- **JSON:** an edge-case conformance audit + flag-pinning (not new natives).

**ADOPT-LATER:** `Text.format` (checked-subset `sprintf`); `replaceAll`/`strtr` longest-match;
`Map.entries`/`fromEntries` + `List.zip` (gated on an injected **`Pair<K,V>`** type); `Map.flip`;
`List.mapIndexed`; **`Core.Serde`** (capability-free typed codec — the *better* `serialize`/`unserialize`:
no code-execution on decode, decimal/bytes survive, byte-stable; rides the Dump value-walk + needs
generic-enum `Result`); **`Reflect.entries -> Map<string,T>`** (value-returning, public-only, typed —
better `get_object_vars`); **`Core.Json.Serializable`** typed interface + `toJson()` (better
`JsonSerializable`); seeded `List.shuffle`.

**REJECT (from the spine, by design — each a documented PHP footgun the static/deterministic model closes):**
- `settype` (mutate-type in place); `Closure::bind`/`bindTo` (runtime scope-break, incompatible with
  capture-at-creation + single-threaded heap); `spl_object_id`/`spl_object_hash` (identity/address leak —
  the #1 determinism trap; the stable-content-hash need composes as `Hash.sha256(Serde.encode(v))`);
  unseeded `shuffle`/`array_rand`/`str_shuffle` (Tier B true-random); locale-dependent sorting/`natsort`/
  `strcoll` (Tier B locale); all `mb_*` multibyte ops (no `mb_*` under `php -n`).
- **Already-shipped (not a gap):** `gettype`/`get_debug_type` (= `Reflect.kind`/`typeName`, *better* —
  honest `"float"` not `"double"`); first-class callables.

**DEFER / language-gated (route to M6/M11/M-RT, not the stdlib charter):** generators/`yield` (M6);
Iterator/Traversable `foreach` + `List.fromIter` (M11); backed-enum `cases`/`from`/`tryFrom` (M-RT — reuses
the reflective-table mechanism this module owns).

**The three load-bearing cross-cutting findings:**
1. **PHP's loose-comparison defaults are the recurring trap** — `array_unique` (SORT_STRING), `array_diff`
   (`(string)` cast), `in_array` (loose `==`). Every list-membership/dedupe native MUST transpile to a gated
   `__phorj_*` helper using strict `===` to match Phorj's structural `eq_val`, **never the bare builtin.**
2. **An injected `Pair<K,V>` type is the unlock** for `Map.entries`/`zip`/`fromEntries` — one design decision
   opens a cluster of TS-idiomatic natives.
3. **`Text.trim`'s Unicode-vs-ASCII whitespace divergence is a likely *live* bug** — Rust `trim()` strips all
   Unicode WS; PHP `trim()` strips 6 ASCII chars. **Must-verify the current transpile and pin to the ASCII
   set on both sides** (graded must-check, P1 if confirmed). *(Plus the §3-R2 `__phorj_parse_float` `$`-anchor
   bug — a second confirmed live divergence to file.)*

**The value-walk cluster** (biggest leverage point in the types/runtime area): `Core.Dump`
(dump/inspect/export), `Core.Serde` (encode/decode), and `Reflect.entries` all walk the closed `Value`
enum, reuse `eq_val_rec`'s cyclic visited-set, and share the `ClassTables` field order + `__phorj_float`
float discipline. **Design and build them as one cluster** to single-source the walk skeleton.

---

## 6. Core.Dump format + Core.Sql builder designs (summaries)

### Core.Dump — deterministic value-dumper (`design-dump.md`, Tier A, no new Op)
A multi-line, 2-space-indented, type-annotated tree, identical on all three legs because **Phorj owns the
format** (no addresses/object-ids/resource handles — unlike PHP `var_dump`'s non-deterministic `#N` ids).
- **API:** `Dump.dump(value) -> string` (unbounded depth, cycle-safe), `Dump.inspect(value, depth) -> string`
  (depth-capped). `NativeEval::Reflective` (reads `&ClassTables` for sorted, inheritance-flattened field
  order), `pure: true`, `Op::CallNative`. One `src/native/dump.rs` leaf + a gated `__phorj_dump*` PHP helper
  block.
- **Per-kind format:** scalars single-line; compounds nest with trailing commas (`List` → `[ i => v, ]`,
  `Map` → `{ k => v, }`, `Set` → `Set { … }`, `Instance` → `ClassName { field: v, }`, `Enum` →
  `Type.Variant(payload…)`). Empty compounds one-line. Always-expand (no width heuristic — rejected as a
  divergence axis).
- **The central decision (resolving the §3 P0s):** **static-type-tagged emission** — the transpiler bakes a
  literal tag (`'set'`/`'bytes'`/`'decimal'`/`'map'`/`'closure'`) into the call based on the argument's
  *static* Phorj type (mirrors `Reflect.kind`/`Convert.toInt` call-site dispatch), because Set→list-array,
  Bytes→string, Decimal→numeric-string, empty-Map→empty-list-array are runtime-indistinguishable on the PHP
  leg. No tag reaches a backend value.
- **Determinism single-sources:** field order from `ClassTables.fields` (BTreeMap, never the `HashMap`);
  floats via `__phorj_float`; decimals via `fmt_decimal`; cycle detection via path-scoped `Rc::as_ptr` /
  `spl_object_id` (gates a `<circular>` token, **never printed**); pinned 5-sequence string escape; lowercase
  `\xHH` bytes; code-point truncation via PCRE `/./us` (no mbstring).
- **Highest-effort sub-part:** Enum PHP rendering (one impl-time check against the actual enum lowering).
- **Open cosmetic calls (all caught by `differential.rs`):** List `i => v` vs bare; `Set { }` spelling; enum
  inline-scalar-payload; truncation defaults (`MAX_STR=100`/`MAX_BYTES=64`); `Core.Dump` vs `Core.Debug` name.

### Core.Sql — typed, injection-safe query builder (`design-sql.md`, Tier A, no new Op)
The **pure half of DB**: a pure function from typed inputs to a `(sql: string, params: List<Json>)` pair.
- **Injection is impossible by construction:** PARAMETERIZE, NEVER INLINE. The SQL string is assembled only
  from developer-authored fragments/keywords + `?` placeholders the builder emits; user values go into a
  separate `List<Json>` handed to the driver's prepared-statement binder. **No `quoteString`/`escapeString`
  ever** (PDO::quote is absent under `php -n` and connection-charset-dependent → non-deterministic).
- **Keystone decision — binds are `Json`:** a SQL bind is the scalar subset of the *already-shipping* `Json`
  enum (`Int`/`Float`/`Bool`/`Str`/`Null`), so a params list is a **homogeneous `List<Json>`** that
  type-checks today — closing Stage-2b's P0 (heterogeneous bind list `[18, true]` is a checker error). Zero
  new injected type. Optional `Sql.int(...)`/`str`/`bool`/`float`/`null` sugar wraps `Json.*`.
- **API:** `Sql.select/insertInto/update/deleteFrom` factories → an immutable (clone-with) `Query` builder
  (`from`/`where`/`andWhere`/`orWhere`/`join`/`set`/`values`/`groupBy`/`having`/`orderBy`/`limit`/`offset`/
  `sql()`/`params()`). Identifiers are quoted by us (ANSI `"id"`, `"`→`""`); only `where`/`having`
  *conditions* are author-authored and carry values solely via the binds list; `orderBy` direction
  allowlisted to `ASC`/`DESC`. LIMIT/OFFSET inlined as `int` (drivers reject bound LIMIT; int render is
  deterministic).
- **Byte-identity free by construction:** build path emits only `string` + `List<Json>` (already-identical
  primitives); the PHP leg runs the *same* transpiled Phorj string code (`implode`/`str_replace`/`.` — all
  core under `-n`), so there is no second implementation to diverge. Float binds are *carried, never
  formatted* (Ryū-vs-PHP-14-digit never touches the build path).
- **The materially-new build piece:** injecting a **method-bearing class pair** (no shipped prelude does
  this) — generalize the prelude injector to carry `Item::Class` as well as `Item::Enum`. De-risk: write the
  classes as a normal user file, gate green, then lift verbatim into `SQL_PRELUDE`.
- **`Core.Db` (Tier B execution half) is out of scope** — consumes the pair via PDO `prepare`/`execute`,
  quarantined like Process/Env.

---

## 7. Open questions / decisions the developer must make before each build

**Cluster-wide (decide once):**
- **The value-walk cluster** — build `Core.Dump` + `Core.Serde` + `Reflect.entries` as one cluster
  (single-source the `Value`-walk + cycle guard + `__phorj_float`)? Or ship Dump alone first?
- **Inject `Pair<K,V>`** as a stdlib type now (unlocks `Map.entries`/`zip`/`fromEntries`/`List.zip`), or
  defer those natives until it lands?
- **Live-bug triage:** confirm + fix the two `$`-anchor / `trim`-whitespace divergences (`__phorj_parse_float`
  R2, `Text.trim` Unicode-vs-ASCII) as standalone P1 items before building dependent validators.

**Per-module gates before build:**
- **Core.Dump:** the 5 cosmetic calls (§6); pin the Enum PHP rendering against the actual enum lowering.
- **Core.Encoding:** confirm the Rust decoder matches PHP's *lenient* `base64_decode` skip-set + padding
  tolerance + non-canonical-bit masking (NOT RFC-strict); per-decoder whitespace policy.
- **Core.Validate:** ship the `\z`-anchor everywhere; add the `"a@b.com\n"=>false` regression case.
- **Core.Random:** pick stateless-functional vs shared-mutable injected `Rng` (Option B needs the
  two-draws-in-one-expression differential case); assert all PRNG constants `<2^63`; choose the PRNG family
  (xorshift64 is safe; xoshiro/splitmix upgrade constants are NOT representable as PHP ints).
- **Core.Url:** treat parse/build (~65%) as a separate sub-slice with an explicit RFC-3986 edge corpus
  (empty authority, `:port` no host, trailing `?` empty query); name the `php -n` UTF-8 validation mechanism
  (PCRE `//u`, not mbstring).
- **Core.Sql:** v1 with bare `Json.Int(...)` or v1.1 `Sql.*` sugar? Confirm the prelude-injector
  generalization to carry `Item::Class`. Dialect pins (ANSI quoting, `?` placeholder) are v1; named
  placeholders / backtick / T-SQL are v2 `Dialect` knobs.
- **Core.Time:** confirm **v1 = literal-format-only** (reject the dynamic `gmdate` helper); decide whether
  `toUnix` rejects out-of-range fields (then restrict examples to in-range 4-digit years) or replicates PHP's
  normalization + year-pivot exactly; make `diffDays` use consistent `intdiv`-truncation on all legs.
- **Core.Http:** add the `FaultKind::Network` `classify()` arm; **keep examples single-file** (the project
  globs don't quarantine impure natives); decide the HTTPS story (http-only Rust vs documented asymmetry) and
  enforce `Accept-Encoding: identity` *in the native* (gzip mtime non-determinism). The `TcpStream` transport
  needs a gate outside `differential.rs`.
- **Core.Db:** accept it as a **PHP-only milestone feature** (no Rust-leg gating); revisit only if a std-only
  in-memory backend or a sanctioned dependency exception is ever approved.
- **Core.Regex:** subset-or-defer decision; if built — `Value::Bytes` (not `Str`) for captures, a *single*
  engine ported faithfully to both legs (or a real corpus + a leftmost-**longest** oracle, not the `regex`
  crate), pinned empty-match advancement + `{n,m}` integer domain, and a reserved `FaultKind` for dynamic
  invalid patterns.

---

*Evidence grades used throughout: feasibility %/tier are [Verified] where the refutation cites a `php -n
8.5.7` run or a `src/…:line` read, [Inferred] where reasoned from the verified mechanism, [Speculative] for
cosmetic/ergonomic design calls. Per-claim grades live in the cited raw files.*
