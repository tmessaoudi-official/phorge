# GA Sequence — charter → DX → test → text → breadth-gaps → numerics → lift → release

> Multi-batch autonomous run chosen 2026-06-27. Move GA% / Global% via the highest-leverage
> remaining chunks, in dependency order. Each slice ships byte-identity-gated (run≡runvm≡real
> PHP 8.5) + a guide example, per the standing rules. Commit green; **never push**.

## Decisions Log
- [2026-06-27] AGREED: do **all four candidate batches in sequence** (developer: "do them in
  sequence"), in the **reordered** order below — NOT the M-Test-first framing I led with.
- [2026-06-27] AGREED: **charter-first reorder** (developer chose "Charter-first, as recommended").
  Rationale: M-Test/M-text/breadth all add stdlib surface; minting them before the conventions
  charter risks an API codemod later (the PascalCase-reshape pain). Charter governs all new stdlib.
- [2026-06-27] AGREED: at genuine design forks (Core.Test assertion API, Core.Regex API, Secret<T>
  model) **stop and ask** via AskUserQuestion before committing the public surface (developer choice).
- [2026-06-27] NOTE: roadmap docs were stale — **error model Slice 2 (throws/Result/try-catch) is
  BUILT** (`Op::Throw/PushHandler/PopHandler`, lexer keywords) and **`phg lift` CLI ships**
  (`cmd_lift`, full `src/lift/`). M4 stdlib **breadth is largely built** (sort/map/list/text/set/
  as-cast/parseFloat). So the remaining work is lighter than the milestone titles imply.

## Decision review — autonomous decisions re-confirmed/changed by the developer (2026-06-27)
> Developer asked to review decisions made in prior autonomous sessions, keep-or-change, one by one.
- [2026-06-27] CSV backslash escape → **KEEP** (RFC-4180, no backslash escape). Confirmed.
- [2026-06-27] **CHANGE** Core.Csv.parse empty input `[""]` → **`[]`** (zero fields; matches Python/Rust,
  honest, round-trips). Was: one empty field. **TODO: implement.**
- [2026-06-27] **CHANGE** Core.Random quarantine → **byte-identical parity**: hand-roll xorshift64 in
  emitted PHP (logical vs arithmetic `>>` masking), Random rejoins the oracle, `pure: true`. **TODO.**
- [2026-06-27] **CHANGE** Decimal `/`: E-DECIMAL-DIV compile error → **exact-or-fault** — bare `/` keeps
  the exact value when the quotient terminates, **faults** at runtime when non-terminating or i128
  overflow. `Decimal.div(a,b,scale,mode)` stays for explicit rounded division. **TODO.**
- [2026-06-27] **CHANGE** Decimal `%`: was wrongly lumped with `/` (rejected). **Un-reject** — `%` is
  exact/closed on fixed-point (no rounding), a bare operator like `+ - *`. Developer confirmed Option 1.
  Open follow-up: add named `Math.rem`/`mod`(+`fmod`?) for symmetry with `Math.intdiv`. **TODO.**

### Batch 2 (scope & API)
- [2026-06-27] Math remainder → **operator-only**, no named `Math.rem`/`fmod` (`%` is exact, needs no
  rounding; the operator already covers int/float, decimal being added). Confirmed.
- [2026-06-27] **CHANGE** Core.Hash: digests-only → **add password hashing** (bcrypt/argon2). Non-
  deterministic (random salt) ⇒ must be **quarantined** + a **security design pass** (own module, e.g.
  `Core.Crypto`/`Core.Password`). **TODO: design first.**
- [2026-06-27] Static calls → **KEEP narrow scope** (own-class, non-overloaded) for now, **AND** schedule
  a **research + brainstorm pass** to cover statics comprehensively (inherited, overloaded, late static
  binding). **TODO: research milestone item.**
- [2026-06-27] **CHANGE** `as` operator → **support all types incl. primitives** (`x as int`). Needs a
  cast-vs-convert reconciliation design (don't reintroduce the C-cast surprise; unify with Core.Convert
  semantics — total vs optional). **TODO: design first.**

### Batch 4 (minor / technical-constraint items)
- [2026-06-27] **CHANGE** float `/0` → **clean fault** (general principle: ANY division by zero throws —
  int/float/decimal `/` and `%`). Was: `1.0/0.0`→`inf` (IEEE), diverging from PHP DivisionByZeroError.
  Add `Math.fdiv` for explicit IEEE inf if ever wanted. Verify int/0 + decimal/0 already fault. **TODO.**
- [2026-06-27] **CHANGE** lambda bare-field `fn() => v` → fix the silent runtime failure (brainstorm:
  clear `E-LAMBDA-BARE-FIELD` vs auto-capture as `this.v`). **TODO: brainstorm form.**
- [2026-06-27] **CHANGE** overload erasure ambiguity → **reject** at declaration (`E-OVERLOAD-ERASE`)
  when two overloads differ only by string-vs-bytes or only among List/Map/Set. **TODO.**
- [2026-06-27] Map numeric-string-key coercion under PHP → **KEEP documented** (use non-numeric string
  keys when transpiling; run≡runvm always identical). No action.

### Batch 4b
- [2026-06-27] **CHANGE** `opt!`-on-null transpiled message → align emitted PHP message to the Rust
  backends' "force-unwrap of null" text. Cosmetic (fault domain). **TODO.**
- [2026-06-27] Transcendental last-ULP (Rust vs PHP libm) + `gcd(i64::MIN)` overflow-fault → **ACCEPT
  as-is** (physics / correct safety). No action.

- [2026-06-27] **CHANGE** numberFormat → **digit-based rounding on the shortest-round-trip decimal
  string** (same algorithm Rust + emitted PHP; no float×10^n scaling error; matches PHP's intended
  decimal). Closes the common-case money divergence. **TODO.**

## Decision-driven fixes — execution order (Option 1: do these, then resume GA sequence)
Each its own commit, TDD, byte-identity-gated (run≡runvm≡real PHP 8.5), + example where user-visible.
1. **CSV empty → `[]`** (trivial).
2. **Division-by-zero cluster**: float `/0` → clean fault; verify int/0 + decimal-div/0 fault; general
   "any division by zero throws" (+ `Math.fdiv` for explicit IEEE inf if wanted).
3. **Decimal `%` un-reject** (exact bare operator: 3 backends + PHP + example).
4. **Decimal `/` exact-or-fault** (terminating → exact; non-terminating/overflow → fault; keep
   `Decimal.div` for rounded).
5. **numberFormat digit-based rounding** (shortest-string, byte-identical).
6. **Random → byte-identical parity** (hand-roll xorshift64 in PHP; un-quarantine; rejoin oracle).
7. **Overload erasure reject** (`E-OVERLOAD-ERASE`: string-vs-bytes / List-Map-Set-only pairs).
8. **Lambda bare-field fix** (sub-brainstorm: clear `E-LAMBDA-BARE-FIELD` vs auto-capture as `this.v`).
9. **opt!-on-null PHP message alignment** (cosmetic).

**Then design-first items** (each: brainstorm + AskUserQuestion on the API before building), slotted
into the GA sequence: `as`→primitives (cast/convert reconciliation) · password hashing (quarantined
`Core.Crypto`) · statics research/brainstorm (inherited/overloaded/LSB).

## Sequence (dependency order)
1. **M4 charter** — codify the *de-facto* conventions from the ~18 shipped native modules into a
   one-page conventions doc + minimal enforcement. Governs items 3–6. (No API rework: descriptive.)
2. **`phg fmt` + lints** — `fmt` reuses the existing `src/lift/printer.rs`; add unused-import /
   unused-local lints on the warning channel. Near-free; speeds all later authoring.
3. **M-Test** — `phg test` runner + `Core.Test` assertions + `assertFaults` + fixtures/selection/skip
   (+ PHPUnit bridge if cheap). Determinism seam (seedable Random + quarantine) already built. **FORK.**
4. **M-text** — `Core.Regex` (PCRE `/u`), codepoint-aware string ops, `\u{…}` escapes, `number_format`.
   **FORK** (regex API surface).
5. **Breadth gaps** — only what `m4-stdlib-breadth.plan.md` left open (most is ✅); `core.json`
   safe-parse hardening, path/log/sprintf if not present.
6. **Close M-NUM S4** — Math breadth + `number_format` (shared with M-text). Flips M-NUM to ✅.
7. **lift L5** — PHP→Phorge→PHP round-trip oracle gate. Flips lift to ✅.
8. **Release-readiness** — M8 security hardening (injection guards, `Secret<T>` **FORK**, `write_atomic`)
   → GA governance docs (semver/BC/conformance corpus/security model) → M2.5 Phase 3 (CI stub registry
   + `--sign`). Docs last: they describe a stable surface.

## Status
- [ ] 1. M4 charter — IN PROGRESS
- [ ] 2. phg fmt + lints
- [ ] 3. M-Test
- [ ] 4. M-text
- [ ] 5. breadth gaps
- [ ] 6. M-NUM S4
- [ ] 7. lift L5
- [ ] 8. release-readiness
