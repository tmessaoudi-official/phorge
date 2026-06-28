# Overnight Design-Forks Review — 2026-06-25

> **Purpose.** During the autonomous overnight session of 2026-06-25, every *genuine design fork*
> (a decision the active master plan did NOT already resolve) is logged here instead of being decided
> silently. For each fork I make a **provisional** best-judgment call so work continues, but it is
> marked `⏳ AWAITING CONFIRMATION` until the developer reviews it in the morning.
>
> **How to use this in the morning:** walk the forks top-to-bottom. For each, either ✅ confirm my call
> or ✏️ change it. If you change one, I revisit the affected commit(s) — every fork entry names the
> exact commit(s) it touched so a reversal is mechanical.

## Decision rule I applied to every fork

Per [[philosophy-of-phorj]] and the project thesis (Phorj : PHP :: TypeScript : JavaScript): I picked
the **most PHP-familiar, most legible, most pragmatic** option that preserves the byte-identity spine
(`run ≡ runvm ≡ real PHP 8.5`) and adds no new `Op`/`Value` unless genuinely required. When two options
were close, I favored the one that removes a surprise without removing capability.

## Status legend
- `⏳ AWAITING CONFIRMATION` — provisional call shipped; needs your review.
- `✅ CONFIRMED` — you approved (fill in the morning).
- `✏️ CHANGED` — you redirected; follow-up commit noted.

---

## ✅ RESOLVED 2026-06-25 (developer, one-by-one via ask-human)

| Fork | Decision | Action |
|---|---|---|
| **F-001** UFCS fallback mechanism | ✅ **Confirm as shipped** | none — stays as `0dc071c` |
| **F-002** `?.` (safe-nav) UFCS | ✏️ **CHANGED → build now** | ✅ **BUILT** — `x?.f(a)` lowers to `match x { null => null, r => f(r,a) }` (no new `Op`); byte-identical run≡runvm≡PHP 8.5 |
| **F-003** number-receiver UFCS | ✅ **Confirm enabled** | none — works via F-001 |
| **F-004** cross-package UFCS→user-fn | ✅ **Confirm deferral** | none — qualified calls stay the cross-package form |
| **F-005** Slice 7 `Text.charAt`/`substring` | ✅ **Confirm deferral → M4/M-text** | none |
| **F-006** Core.Reflect | ✅ **A — dedicated design pass** | ✅ **SPEC WRITTEN** → `docs/specs/2026-06-25-core-reflect-design.md` (Q1–Q5 await your answers) |
| **F-007** Process I/O | ✅ **Design pass first** | ✅ **SPEC WRITTEN** → `docs/specs/2026-06-25-process-io-quarantine-seam-design.md` (Q1–Q5 await your answers) |

**Build/write queue from these decisions:** (1) implement **F-002** `?.` UFCS (byte-identity-safe,
extends `0dc071c`); (2) write the **F-006** Core.Reflect design spec; (3) write the **F-007** Process I/O
seam design spec. Then await developer review of the two specs before building Reflect / Process I/O.

## Forks log

### F-001 — UFCS fallback also resolves stdlib natives (not just free functions)
- **Slice / context:** Slice 6 (UFCS). The master plan's locked decision says UFCS is *"general (any
  free function), method-first … else free-function fallback."* But the plan's own motivating examples —
  `xs.length()`, `xs.filter(p).map(g)` — are **`Core.List` natives** (`List.length`/`map`/`filter`),
  addressed by `(module, name)`, NOT user free functions. So "free-function fallback" alone would make
  the flagship examples fail to resolve.
- **The fork:** Should `x.f(args)` fall back only to user free functions, or also to stdlib natives?
- **Options considered:**
  - A) Free functions only (literal plan text). Flagship examples (`xs.map`, `xs.filter`) would NOT
    work via UFCS — the guide example would have to define user free fns, gutting the feature's value.
  - B) Free functions **and** natives, mapping the receiver's type to its stdlib module
    (`List→Core.List`, `Map→Core.Map`, `Set→Core.Set`, `string→Core.Text`, `bytes→Core.Bytes`),
    requiring that module to be imported (consistency with "explicit import required").
- **My provisional call:** **B, via a general "callable by first-param" mechanism (no type→module
  map).** Resolution order: **method → user free fn → any *imported* native**, where a candidate is
  *any* callable named `f` whose **first parameter accepts the receiver type** (tested with the
  existing `unify`, so generic natives like `map: (List<T>,(T)->U)` match a `List<int>` receiver).
  `xs.map(g) ≡ List.map(xs, g)`, `s.upper() ≡ Text.upper(s)`, and even `xs.join(", ") ≡ Text.join(xs,
  ", ")` (note: `join` lives in `Text` but takes a `List` first — a rigid receiver-type→module map
  would have missed it; the by-first-param search finds it). Native candidates must be **imported**
  (preserves the namespace invariant); a user free fn wins over a native of the same name; two distinct
  imported natives both matching ⇒ `E-UFCS-AMBIGUOUS` (rare).
  *Rationale for dropping the type→module map:* it's simpler, strictly more general, matches the plan's
  "general — any free function" wording, and avoids the `join`-style mismatch.
- **Byte-identity / Op impact:** none — UFCS is a type-directed post-check AST rewrite (`rewrite_ufcs`,
  mirroring `resolve_html`); the rewritten node is an ordinary free/native call the backends already
  handle. No new `Op`, no new `Value`.
- **Touched commit(s):** `0dc071c` (feat(lang): UFCS — Phase 1 slice 6)
- **Reversal cost if you change it:** low. Dropping native fallback = delete the native branch of
  `try_ufcs`; the free-fn path and the rewrite mechanism stay.
- **Status:** ⏳ AWAITING CONFIRMATION

### F-002 — `?.` (safe-nav) UFCS deferred
- **Slice / context:** Slice 6 (UFCS).
- **The fork:** Should `x?.f(a)` (safe-nav) also UFCS-desugar when `f` is not a method?
- **My provisional call:** **Defer.** Only plain `.` UFCS this slice. `x?.f(a)` would mean
  "if x is null → null, else f(x!, a) wrapped optional" — extra lowering with little demonstrated need.
  A `?.` call with no matching method keeps today's error. (KNOWN_ISSUES.)
- **Byte-identity / Op impact:** none.
- **Reversal cost if you change it:** low-medium (adds an optional-peel + re-wrap to the UFCS path).
- **Status:** ✅ **BUILT (developer chose "build now")** — `x?.f(a)` desugars to `match x { null =>
  null, __ufcs_recv => f(__ufcs_recv, a) }` (receiver evaluated once, null short-circuits to null). Reuses
  match-over-optional → **no new `Op`/`Value`**; byte-identical run≡runvm≡real PHP 8.5
  (`examples/guide/ufcs.phg` `safe-some`/`safe-null` rows + 2 checker unit tests).

### F-003 — number receivers (`int`/`float`) UFCS-to-`Core.Math` now *enabled* (consequence of F-001's general mechanism)
- **Slice / context:** Slice 6 (UFCS).
- **The fork:** Should `n.abs()` / `x.sqrt()` desugar to `Math.abs(n)` / `Math.sqrt(x)`?
- **My provisional call:** **Enabled (not deferred), as a free consequence of the by-first-param
  mechanism (F-001).** If `Core.Math` is imported, `Math.abs(int)`/`Math.sqrt(float)` have a numeric
  first param, so `n.abs()` resolves like any other native UFCS — no special-casing. Reads acceptably
  (`n.abs()`, `x.sqrt()`). If you'd rather forbid number receivers, that's a small guard to add.
- **Byte-identity / Op impact:** none.
- **Reversal cost if you change it:** low (add a guard rejecting primitive-number receivers in the
  native-UFCS search).
- **Status:** ⏳ AWAITING CONFIRMATION

### F-004 — multi-package UFCS-to-user-free-fn deferred (natives + single-package unaffected)
- **Slice / context:** Slice 6 (UFCS). The loader name-mangles non-`Main` package free fns to their
  PHP FQN and rewrites *qualified/ident* call sites — but a UFCS `x.f()` is a `Member` call the loader
  doesn't rewrite, so in a multi-package program `self.funcs` holds `f` under its mangled name and the
  bare-`f` UFCS lookup misses.
- **My provisional call:** **Defer** multi-package UFCS to a *user* free function. Single-package
  (`package Main`, all examples + the common case) works because there's no mangling; **native** UFCS
  works regardless (natives aren't mangled). (KNOWN_ISSUES.)
- **Byte-identity / Op impact:** none — it's a resolution-reach limitation, not a correctness risk
  (an unresolved UFCS is a clean compile error, never a silent divergence).
- **Reversal cost if you change it:** medium (loader would need to resolve UFCS heads cross-package).
- **Status:** ⏳ AWAITING CONFIRMATION

### F-005 — Slice 7 (`Text.charAt` / `Text.substring`) deferred to M4 / M-text
- **Slice / context:** Phase 1 Slice 7. The master plan lists it as *"`Text.charAt` / `Text.substring`
  natives (the safe alternative to `s[0]`; **→ M4**)"* — the `→ M4` annotation already routes it to the
  stdlib-charter milestone.
- **The fork:** Build byte-indexed `charAt`/`substring` now, or defer?
- **Options considered:**
  - A) Build now with **byte** semantics (matching PHP `substr`, byte-identical for ASCII; multibyte is
    a documented caveat like the existing `sqrt(2.0)` float case).
  - B) Defer to **M4 / M-text**, where byte-vs-codepoint is decided holistically (`s[0]` codepoint
    indexing is *explicitly* deferred there in the master plan).
- **My provisional call:** **B (defer).** Shipping byte semantics now and switching to codepoint in
  M-text would be a breaking change to a *shipped* stdlib function — exactly the entanglement M-text
  exists to resolve. The `→ M4` annotation already points this way. Phase 1's ergonomics perimeter is
  effectively **closed at Slice 6 (UFCS)**; I proceed to Phase 2 (introspection + process I/O).
- **Byte-identity / Op impact:** none (nothing built).
- **Reversal cost if you change it:** low — if you want them now, they're a small additive pair of
  `Core.Text` natives (the generic call path is already in place); say the word and I build them.
- **Status:** ⏳ AWAITING CONFIRMATION

### F-006 — Core.Reflect (Phase 2 Slice 1) is BLOCKED on a real byte-identity-vs-erasure tension — NOT shipped
- **Slice / context:** Phase 2 Slice 1 (introspection). The spec says the reflect natives "erase to PHP
  `get_class`/`class_implements`/`class_parents`/`class_uses`/`gettype`" and are byte-identity-gated.
- **The blocker (verified by reading the transpiler erasure model):** PHP **erases** Phorj's finer type
  distinctions, so several reflect natives **cannot** be byte-identical run≡runvm≡real PHP for arbitrary
  inputs — a spine break in *user* code, not just examples:
  - `typeName(x)`: `bytes` → PHP `string`; `List`/`Map`/`Set` all → PHP `array` (PHP cannot tell them
    apart to return `"List"`/`"Map"`/`"Set"`); enum variants → generated PHP classes (name ≠ Phorj enum
    name). So `typeName(aMap)` = `"Map"` on Rust backends, unreproducible in PHP.
  - `className(x)`: salvageable for **class instances** (`get_class` matches a `package Main` class name)
    + `null` for non-objects — but the **enum** case depends on enum PHP erasure (unverified) and the
    composite cases need care.
  - `implements`/`parents`/`traits`/`methodNames`/`fieldNames`: return `List<string>` whose **order** must
    match PHP's `class_*`/`get_class_methods` order — the same ordering hazard as `Map.keys` vs PHP. Plus
    they need a new `NativeEval::Reflective(&[Value], &ClassTables)` arm threaded into both backends
    (the VM's `BytecodeProgram` would carry a new `ClassTables` bundle).
- **Options:**
  - A) **Defer the whole module to a dedicated design pass** (my recommendation) — resolve the erasure
    tension explicitly: e.g. restrict `typeName`'s contract, define each list native to return a
    **sorted** list with a matching `sort()` PHP wrapper, verify enum erasure, decide the `ClassTables`
    plumbing. This is design work, not a mechanical slice.
  - B) Ship only the **byte-identity-safe subset** now — `className` restricted to class-instance/null —
    and defer `typeName` + the enumeration natives. Thin, and arguably should be designed as one module.
  - C) Make the reflect list natives **run/runvm-only** (drop them from the PHP oracle, like the
    quarantine seam) — a real departure from the "everything is byte-identical with PHP" thesis;
    needs your explicit blessing.
- **My provisional call:** **A — do NOT ship Core.Reflect autonomously tonight.** It needs a design
  decision about the reflection-vs-erasure tension that I shouldn't make silently while you sleep
  (shipping a `typeName` that diverges for `Map`/`Set`/`bytes`/enums would break the byte-identity spine
  in user programs — the one invariant I won't risk autonomously). Phase 2 Slice 1 is parked pending your
  call on A/B/C.
- **Byte-identity / Op impact:** none (nothing built).
- **Status:** ⏳ AWAITING CONFIRMATION — **needs your decision before I build it.**

### F-007 — Phase 2 Slice 3 (Process I/O) also needs a seam decision before autonomous build
- **Slice / context:** Phase 2 Slice 3 — `Core.Process.args()` / `Core.Env.get/all`. The spec marks it
  **non-deterministic → off the byte-identity differential**, on a new "impure native" **quarantine
  seam**, and calls it "the **M-Batteries kickoff**" (milestone-scale, not a quick slice).
- **The fork:** the quarantine seam has open mechanism decisions — how an impure native is *marked*
  (a `NativeEval` variant? a per-native flag?), how `differential.rs` *detects + skips* a program that
  uses one, and how CLI argv (`phg run f.phg -- args`) threads to the interpreter/VM/serve paths.
- **My provisional call:** **defer to a short design pass** rather than improvise a harness-affecting
  seam autonomously (a wrong quarantine could let a non-deterministic program silently onto the oracle
  and cause flaky CI). Low byte-identity risk by intent, but the seam design deserves your sign-off.
- **Byte-identity / Op impact:** none (nothing built); the seam exists precisely to keep these *off* the
  spine.
- **Status:** ⏳ AWAITING CONFIRMATION

<!-- TEMPLATE for each fork — copy below the line:

### F-NNN — <short title>
- **Slice / context:** <which slice, what I was building>
- **The fork:** <the genuine decision the plan didn't resolve, stated neutrally>
- **Options considered:**
  - A) <option> — <tradeoff>
  - B) <option> — <tradeoff>
- **My provisional call:** <which + why, tied to the decision rule above>
- **Byte-identity / Op impact:** <none / new Op / etc.>
- **Touched commit(s):** `<sha>` (`<subject>`)
- **Reversal cost if you change it:** <low/medium/high + what changes>
- **Status:** ⏳ AWAITING CONFIRMATION

-->
