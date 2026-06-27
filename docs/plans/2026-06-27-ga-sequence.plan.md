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
- [2026-06-27] AGREED (Option 2 build, item a — design fork resolved): `as`→primitives uses the
  **Unified, fallibility-typed** model. `x as T` (T primitive) result type tracks fallibility:
  **lossless/infallible → total `T`** (int→float, int→decimal, *→string, identity);
  **lossy or fallible → `T?`** (float/decimal→int = null unless integral; string→int/float = parse,
  null on non-numeric; primitive-union/erased member = assertion/narrow). **No silent lossy
  conversion** — lossy narrowing is always optional (loud null); `Convert.truncate` stays the named
  tool for "I want truncation". `T as T` = identity (W-redundant-cast lint).

### Tooling needle-mover (post Option-2 a/b)
- [2026-06-27] AGREED: next = **M-Test then phg fmt** (developer chose the tooling needle-mover). Both
  **design-specced first** (developer chose spec-first): `docs/specs/2026-06-27-m-test-design.md` +
  `docs/specs/2026-06-27-phg-fmt-design.md`. **All flagged forks approved as recommended** (developer:
  "build with all recommended defaults — M-Test first"): M-Test = `test "name" {}` items + catchable-
  fault failures + `Core.Test` asserts + `tests/**/*.phg` discovery + interpreter runner; phg fmt =
  comment side-channel + reattachment + gofmt-shaped CLI + tidy-no-reflow v1. **Finding:** phg fmt is
  NOT a printer reuse — the lexer discards comments, so it needs the trivia slice (F1–F5); M-Test is
  unblocked, hence first. Build order: M-Test T1→T5, then phg fmt F1→F5.
- [2026-06-27] DONE: **M-Test COMPLETE** (T1–T5, commits `fc0ea9f`/`6e657ff`/`e33eafa`/`195d186` + T5).
  No new `Op`/`Value`. Key implementation choices: (a) `test` is contextual, recognized before any
  modifier in `parse_item` so a leading modifier cleanly rejects it; (b) test-mode threaded via a
  `Checker.test_mode` flag + a `check_tests` entry (E-TEST-OUTSIDE-TESTS otherwise); (c) the runner
  **lowers each test body into a synthetic `main`** and reuses the ordinary check_and_expand→interpret
  pipeline, so every front-end pass processes the body with no test-specific backend path; (d) the
  self-hosted suite lives at top-level **`selftest/`** (outside `examples/`, so the byte-identity
  differential never touches it), gated by `tests/mtest.rs`. GA rock 2 30%→45%, total 49%→52%.
  **Next on the critical path: phg fmt (F1–F5).**
- [2026-06-27] AGREED: developer pushes the 5 M-Test commits themselves; I build **phg fmt next,
  autonomously, recommended defaults** (spec `docs/specs/2026-06-27-phg-fmt-design.md`): D1 comment
  side-channel + position reattachment, D2 gofmt-shaped CLI, D3 tidy-no-reflow v1, quotes left as
  written. Build order F1 (lexer comment capture) → F2 (comment-aware printer) → F3 (`phg fmt` CLI) →
  F4 (dogfood) → F5 (bonus: lift L5 comment fidelity).
- [2026-06-27] CHALLENGED + REDECIDED (F2 engine): the spec's recommended option B ("comment-aware AST
  printer reusing the printer that already produces canonical layout") rested on a **false premise** —
  `src/lift/printer.rs` covers only the Tier-1 lift subset (it `Err`s on interfaces/traits/type-aliases/
  generics/unions/intersections/lambdas/try-throw/html/bytes/destructuring/property-hooks), so a fmt
  built on it would error on nearly every real file. Surfaced both real options (token reformatter vs a
  new full AST printer). **Developer chose B' — a full, exhaustive, comment-aware AST printer** in a NEW
  `src/fmt/` module (lift printer untouched). Rationale: a formatter's one hard rule is meaning-
  preservation; an AST printer gives `parse(fmt(x)) ≡ parse(x)` and, with exhaustive matches, compiler-
  proven completeness (can never silently mis-handle/error a parseable file) — a token reformatter can
  only guess at `<`/unary-`-`/`>>`/interpolation spacing. Gate: round-trip `parse(fmt(x))≡parse(x)` +
  idempotence `fmt(fmt(x))==fmt(x)`. Build slice-by-slice: items → stmts → exprs → types/patterns →
  comment interleaving (F1 channel). F1 `cd38064` DONE.

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
1. **CSV empty → `[]`** ✅ DONE `ea6bc96`.
2. **Division-by-zero cluster** ✅ DONE (float `/0`/`%0` now fault — `value::float_div`/`float_rem`
   → `Result`, wired through both backends + `__phorge_rem` PHP guard; int/0 + decimal-div/0 already
   faulted). `Math.fdiv` for explicit IEEE inf = deferred (add only if requested).
3. **Decimal `%` un-reject** ✅ DONE — exact remainder operator (`Op::RemD` → `value::decimal_rem` →
   `bcmod`; zero divisor faults; result scale = max). Checker allows `%`, keeps `/` rejected.
4. **Decimal `/` exact-or-fault** ✅ DONE — `Op::DivD` → `value::decimal_div_exact` (reduce fraction,
   strip 2s/5s, fault if non-terminating, minimal-form result). Transpiles to `__phorge_dec_div_exact`
   (bcdiv + exactness check + strip) byte-identical under PHP 8.5. `Decimal.div` (rounded) unchanged.
5. **numberFormat digit-based rounding** ✅ DONE — both legs digit-round the shortest-round-trip
   string (`__phorge_float`) by carry, not float-scaling. `0.285→0.29` byte-identical; `.5` divergence gone.
6. **Random → byte-identical parity** ✅ DONE — `pure: true`; transpiler hand-rolls the same xorshift64
   (`__phorge_rng_*`, logical-`>>` mask + signed `GOLDEN`); dice.phg now oracle-gated, seq identical 3-way.
7. **Overload erasure reject** ✅ DONE — `E-OVERLOAD-ERASE` at declaration via a `php_erasure_key`
   (string/bytes→string, List/Map/Set→array, Optional recursive); explain + checker test.
8. **Lambda bare-field fix** ✅ DONE — resolved by the bigger decision: **require `this.field`
   everywhere** (`E-BARE-FIELD`, PHP-faithful; `53dc203`). Migrated 16 examples + tests + Http prelude.
   Additive bonus ✅ DONE (`04ebe63`): the optional `fn(x): int => e` lambda return annotation was
   already built+parser-tested (parser `:`/`->`; checker assignability check); added the missing
   checker tests (match/mismatch, non-vacuous) + showcased it in `guide/lambdas-pipe.phg`
   (byte-identity-gated run≡runvm≡PHP) + README note. Backends ignore the annotation (checker-only).
9. **opt!-on-null PHP message** ✅ DONE — verified the body ALREADY matches across all backends
   (`"force-unwrap of null"`); only the source *location* differs (inherent to PHP exceptions,
   fault-domain). No code change; KNOWN_ISSUES note corrected (it overstated the difference).

**All 9 decision-fixes COMPLETE.** Additive bonus ✅ DONE (`04ebe63` — `fn(x): int => e` lambda
return annotation, coverage+example).

**Option 2 design-first items (each brainstorm + AskUserQuestion on the API before building):**
- (a) **`as`→primitives ✅ COMPLETE** (plan `docs/plans/2026-06-27-as-primitives-matrix.plan.md`;
  `fc60682` S1 + `85c569e` S2 + `bcb6ea7` S3+S4). Unified, fallibility-typed cast over the full
  primitive matrix + union assertion; no new `Op`/`Value`; byte-identical run≡runvm≡PHP 8.5.
  Design forks resolved with the developer (full matrix; honest/loud, not PHP coercion; bool
  conditions already strict everywhere — verified). Deferred edges in KNOWN_ISSUES.
- (b) **password hashing** — IN PROGRESS. **Decision (2026-06-27, after the developer challenged
  hard):** do NOT delegate to PHP and do NOT compromise security. Since secure password hashing
  requires a vetted impl ("never roll your own") and `std` has no crypto, the developer's rules
  *force* the first external crate. **Adopted RustCrypto `argon2`** (Argon2id) behind a written
  **dependency policy** (`docs/specs/2026-06-27-dependency-policy.md` — audited-crypto-only exception
  to `std`-only). `Core.Crypto.hashPassword`/`verifyPassword`/`needsRehash` implemented **natively in
  the Rust backends** (run/runvm), transpiling to PHP `password_hash(ARGON2ID)`/`password_verify` as a
  **peer emission target** (standard PHC `$argon2id$…` ⇒ Rust↔PHP cross-verify). `pure:false`,
  EXCLUDED from the byte-identity oracle (random salt); dedicated `tests/crypto.rs`; a **verify-only**
  example (committed PHC hash) IS gateable (deterministic). argon2 feature-gated OFF for the WASM
  playground. **Principle reaffirmed:** transpile/lift are migration+test bridges, never a runtime
  Phorge depends on — every native has a real Rust impl; PHP is only an emission target.
  **✅ COMPLETE** (`e345b85`): `argon2` crate adopted; `Core.Crypto.hashPassword`(impure)/
  `verifyPassword`(pure) native on Rust backends + PHP peer emission (PHC cross-verify proven);
  feature-gated off for the playground; `tests/crypto.rs` + verify-only gated example; 1112 tests green.
- (c) **statics research** — inherited/overloaded/LSB statics; research + brainstorm pass.

**Then design-first items** (each: brainstorm + AskUserQuestion on the API before building), slotted
into the GA sequence: `as`→primitives (cast/convert reconciliation) · password hashing (quarantined
`Core.Crypto`) · statics research/brainstorm (inherited/overloaded/LSB).

## Sequence (dependency order)
1. **M4 charter** — codify the *de-facto* conventions from the ~18 shipped native modules into a
   one-page conventions doc + minimal enforcement. Governs items 3–6. (No API rework: descriptive.)
2. **`phg fmt`** — **design-specced** `docs/specs/2026-06-27-phg-fmt-design.md`. NOT a printer reuse:
   the lexer discards comments, so a real formatter needs trivia preservation (comment side-channel +
   reattachment, F1–F5). Recommended scope v1 = "tidy + comment-safe, no reflow".
3. **M-Test** — `phg test` runner + `Core.Test` assertions + `assertFaults`. **Design-specced**
   `docs/specs/2026-06-27-m-test-design.md` (T1–T5). Recommended: `test "name" {}` items, failure =
   catchable fault, discover `tests/**/*.phg`, interpreter runner. **Build M-Test FIRST** (unblocked;
   fmt needs the trivia slice). Both specs have flagged forks awaiting developer confirmation.
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
- [x] 2. phg fmt — **COMPLETE** (F1–F4: comment side-channel + full-surface AST printer + gofmt-shaped CLI + dogfood). F5 (lift L5) deferred. GA 52% → 57%.
- [2026-06-28] AGREED (post M-Test + M-fmt): developer pushes the commits; next sequence = **(1) LSP
  — design-first then build** (minimal language server reusing the checker's `Diagnostic` surface;
  `phg check --json` already emits structured diagnostics — finishes GA rock 2), **then (2) rock 3
  stability/conformance** (conformance corpus + semver/BC + deprecation policy + frozen surface — the
  biggest remaining GA mover, ~17 pts). Build LSP design-spec first (the developer's spec-first
  preference), surface forks, then implement autonomously.
- [x] 3. M-Test — **COMPLETE** (T1–T5: `test` item + `Core.Test` + `assertFaults` + `phg test` runner + `selftest/` showcase). GA 49% → 52%.
- [ ] 4. M-text
- [ ] 5. breadth gaps
- [ ] 6. M-NUM S4
- [ ] 7. lift L5
- [ ] 8. release-readiness
