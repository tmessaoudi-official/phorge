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

## Post-LSP autonomous run — locked plan (2026-06-28, developer front-loaded all decisions)
> Developer will **push** the 6 commits themselves, then I run the below **fully autonomously** after a
> compaction. Persistent project autonomy bypass set (`~/.claude/projects/-stack-projects-phorge/state/
> autonomous-3c-bypass`). Recommended-defaults at every non-pre-decided fork; commit green slices;
> **never push**; only stop for a fundamental design fork or a risky/destructive action.
> **Run order: (1) overloaded statics → (2) LSP v2 → (3) rock 3.**
- [2026-06-28] AGREED: **option 4 = BOTH** (overloaded statics + LSP v2), then rock 3.
- [2026-06-28] AGREED: **overloaded statics (Item C Area B) = runtime VM dispatch** — add a VM
  static-overload dispatch (new `Op` or `CallOverload` variant that pushes the dummy receiver +
  selects by arg kinds via `dispatch::select_overload`, matching the interpreter) so `run≡runvm`.
  Remove the checker's `sigs.len()>1` rejection (route through `check_method_sigs`/`check_overload_call`).
  Op-coupling discipline (vm `exec_op` + chunk `validate` + compiler `stack_effect`) if a new Op. Add an
  overloaded-static example + tests; un-defer in KNOWN_ISSUES.
- [2026-06-28] AGREED: **LSP v2 = FULL** — locals/params resolution (precise go-to-def + hover for
  local bindings, not just top-level names), true end-position ranges (thread the diagnostic/token
  `Span.len` into LSP ranges), **completion**, and **document symbols** (outline). Needs a scope/symbol
  model over the AST; advertise the new capabilities; extend `tests/`-style coverage in `src/lsp/`.
- [2026-06-28] AGREED (autonomy): developer chose **all of options 1+2+3** — persistent project bypass
  ON, proceed with recommended defaults, AND pre-decide the rock-3 forks (below).

### Rock 3 (stability/conformance) — forks pre-decided (2026-06-28)
- **R1 conformance corpus = BOTH layers** (developer's "DDD multi-file" instinct + my challenge for
  localization, synthesized): (a) a **focused golden corpus** under `conformance/` — many small
  one-feature-each `.phg` programs + expected stdout, the precise regression net; AND (b) a **flagship
  multi-file/multi-package DDD program** (bounded contexts → packages, entities/value-objects/aggregates
  → classes, folder=path, exercising the M5 package model) as a composition-at-scale conformance
  **project**. Both byte-identity-gated (the differential is already project-aware + globs). Rationale:
  small programs localize regressions; the DDD program proves features compose at realistic scale.
- **R2 semver/BC = 0.x-may-break-documented, freeze at 1.0.** Write a SEMVER/stability policy: in 0.x,
  minor versions MAY break but each is documented (CHANGELOG + a BREAKING section); at 1.0 the public
  surface freezes under strict semver.
- **R3 deprecation = `W-DEPRECATED` lint + policy doc.** A deprecation-policy doc + a minimal
  `W-DEPRECATED` warning (rides the warning channel) when a stdlib symbol flagged deprecated is used,
  naming the replacement + removal version. No new *user* syntax (a `deprecated` modifier is later).
- **R4 stability surface = `STABILITY.md` with tiers** (stable / experimental / deprecated) enumerating
  the public surface (CLI commands, stdlib modules, language constructs); the conformance corpus
  enforces the `stable` tier.

### Locked autonomous execution order (post-compaction, fully autonomous)
1. **Overloaded statics** (runtime VM dispatch) — close Item C Area B; un-defer in KNOWN_ISSUES.
   ✅ DONE (2026-06-28, not pushed). New `Op::CallStaticOverload` (runtime-identical to `CallOverload`,
   shares exec + validate arms; distinct `stack_effect` since the compiler pushes a dummy receiver below
   the args). Checker: removed the static-call overload rejection → routes through `check_method_sigs`;
   added `E-OVERLOAD-STATIC-MIX` (overloads must agree on static-ness — closes the soundness hole the
   un-rejection would open). Interpreter already selected; compiler consults `method_overloads` at static
   call sites; transpiler emits a `static` dispatcher with `self::` targets.
   `examples/guide/overloaded-statics.phg` (incl. inherited `Swatch.of`) byte-identical run≡runvm≡real
   PHP 8.5; 1381 workspace tests green w/ oracle, clippy+fmt clean. KNOWN_ISSUES + CHANGELOG +
   examples/README + explain updated. **NEXT: (2) LSP v2 full.**
2. **LSP v2 full** — locals/params resolution, true end-ranges, completion, document symbols.
   ✅ DONE (2026-06-28, not pushed, `eccb9c0`). All front-end-only (off the byte-identity spine): new
   `src/lsp/scope.rs` (position↔offset, binding collection, enclosing-callable by source-ordering) +
   token-span/outline helpers in `symbols.rs`. True end-ranges re-derived from the buffer (the
   `Diagnostic` is span-less). Locals/params resolution via nearest-preceding binding in the enclosing
   callable; top-level wins first. Completion = top-level + in-scope locals + keywords. Document symbols
   = hierarchical outline (`[item..next_item)` ranges so children nest). 7 new tests + a wire smoke
   verified; 1176 lib + all suites green. README/CHANGELOG/spec updated. Deferred: member completion
   (needs resolved-type index), lambda/match-pattern binders. **NEXT: (3) rock 3.**
3. **Rock 3** — focused golden `conformance/` corpus + flagship DDD project + `SEMVER`/`STABILITY.md`
   policy docs + `W-DEPRECATED` lint + deprecation-policy doc.
   ✅ DONE (2026-06-28, not pushed). **R1** (`bba7785`): `conformance/` golden corpus — 19 single-feature
   programs (`tests/conformance.rs` asserts run≡runvm≡PHP≡golden, glob-discovered) + a flagship
   multi-package DDD project (`conformance/ddd/`, project-loaded golden). Golden pins the *value*
   (stronger than the example differential's agree-only). **R2+R4+R3** (next commit): `SEMVER.md`
   (0.x-may-break-documented → freeze at 1.0), `STABILITY.md` (stable/experimental/deprecated tiers over
   constructs+stdlib+CLI; corpus enforces the stable tier; + `conformance/types/unions.phg` &
   `intersections.phg` to back the stable type-system claim), `docs/DEPRECATION.md` + the `W-DEPRECATED`
   lint (`native::deprecation_of` side table — empty in release, `#[cfg(test)]` sample; checker emits on
   a deprecated native call; `phg explain W-DEPRECATED`). 1178 lib + conformance + all suites green;
   clippy+fmt clean. **Locked autonomous sequence (items 1–3) COMPLETE.**
Each: design-spec where non-trivial, TDD, byte-identity-gated (run≡runvm≡real PHP 8.5), commit green,
**NEVER push**, recommended-defaults at every remaining fork, only stop for a fundamental design fork or
a risky/destructive action. Rebuild the release binary after each feature; end every status with
`GA: ~X% · Global: ~Y%`.

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
- [2026-06-28] **REVISED ORDER** (developer chose "solve all the forks, then statics research, then
  LSP"): resolve the two standing design FORKs first — **(A) `Core.Regex` API** + **(B) `Secret<T>`
  model** — each brainstorm + AskUserQuestion + spec, then build; **then (C) statics research pass**
  (inherited/overloaded/LSB); **then (D) LSP design-first then build**. Statics is research-not-fork;
  LSP is last.
- [2026-06-28] FORK A **COMPLETE** (`159b296`) — `Core.Regex` shipped: `regex` crate (2nd vetted dep,
  policy amended), injected `Regex` class, 7 natives (compile/matches/find/findAll/findGroups/replace/
  split), gated `__phorge_regex_*` transpile helpers. 1354 tests green, clippy+fmt clean, playground
  builds without it, `examples/guide/regex.phg` byte-identical run≡runvm≡PHP 8.5. NOT pushed.
- [2026-06-28] FORK A RESOLVED — **`Core.Regex`**: (engine) **adopt the `regex` crate** as the 2nd
  vetted dependency (developer reframed the question to "best & most secure regardless of byte-identity
  /PHP" — `regex` is RE2-style, **ReDoS-immune by construction**, unlike PHP/PCRE backtracking; "never
  roll your own" applies to untrusted-input parsers too). **Amend `dependency-policy.md` clause 1**:
  generalize "crypto-only" → "security-critical primitive (crypto **and** untrusted-input parsers like
  regex) where std has none and rolling-your-own is the anti-pattern." Feature-gate off for the WASM
  playground (like `argon2`). Key insight: secure ≠ at odds with parity — `regex`'s restricted feature
  set (no backref/lookaround) is exactly the *regular* subset PHP `preg` matches identically, so
  byte-identity holds on the supported subset; backref/lookaround are **rejected at compile**
  (`E-REGEX-UNSUPPORTED`). (API) **compiled `Regex` value + named groups** — `Regex.compile(p) ->
  Regex` (validates once, reusable), `r.matches/find/findAll/replace/split`, named-group typed match;
  transpiles to `preg_*` with the compiled pattern + `/u`.
- [2026-06-28] ITEM D (LSP) **COMPLETE** (`f35d380` diagnostics core + `fe4d081` hover/go-to-def +
  VS Code client). `phg lsp` — hand-rolled JSON-RPC over stdio in `std` (`src/lsp/`: minimal JSON
  parser + framing + lifecycle + symbols index), diagnostics (reuses `phg check`), hover (decl
  signature, source-sliced), go-to-definition (top-level name → decl span). VS Code thin client in
  `editors/vscode/` + README "Editor support" (Neovim snippet). 14 LSP tests; 1377 green. No new
  `Op`/`Value`. v2 deferrals: locals/name-collision resolution, true end-position ranges, incremental
  sync. **ALL FOUR ITEMS of the "solve forks → statics → LSP" sequence are DONE. NOT pushed.** Next per
  the GA sequence = rock 3 (stability/conformance).
- [2026-06-28] ITEM D (LSP) design-first DELIVERED (`docs/specs/2026-06-28-lsp-design.md`) + scope
  locked. Hard constraint: an LSP server isn't security-critical → dependency policy excludes
  `tower-lsp`/`lsp-server`/`serde` → **hand-rolled JSON-RPC over stdio in std** (incl. a minimal
  internal request-JSON parser, off the byte-identity spine). Developer chose: **scope = diagnostics +
  hover + go-to-definition** (needs a new position→symbol/type index over the checker's resolved data);
  **editor client = VSCode thin client in-repo (`editors/vscode/`) + generic-registration docs**;
  **full document sync**. `phg lsp` subcommand. Build slices: JSON-RPC core → lifecycle/doc-store →
  diagnostics (publishDiagnostics, reuse checker, surface `phg explain`) → hover + go-to-def → VSCode
  client + docs; `tests/lsp.rs` drives framed requests (outside `differential.rs`).
- [2026-06-28] ITEM C **COMPLETE** — research delivered + **Area A (inherited/trait statics) shipped**
  (`7ce4ed7`): byte-identical run≡runvm≡PHP, no new `Op`/`Value`, `examples/guide/static-inheritance.phg`,
  3 checker tests, 1363 tests green. **Area B (overloaded statics) DEFERRED** (developer chose
  "defer B, move to LSP"): re-estimated bigger than the research said — the VM has no static-overload
  dispatch set, so B needs a runtime VM dispatch path matching the interpreter's `select_overload`
  (else run↔runvm divergence). Cleanly rejected as today; revisit as a dedicated VM slice. LSB stays a
  documented non-feature. **Next: Item D (LSP), design-first.**
- [2026-06-28] ITEM C (statics research) DELIVERED + scope locked — research
  `docs/specs/2026-06-28-statics-research-design.md`. Developer chose **A+B, defer C**: build
  **inherited statics (A)** + **overloaded statics (B)** (both compile-time, no new `Op`/`Value`, reuse
  inheritance-flattening + `check_overload_call`); **defer late static binding (C)** as a documented
  non-feature (runtime called-class cost + `self::`/`static::` footgun; explicit per-subclass override
  is the workaround).
- [2026-06-28] FORK B **COMPLETE** (`c8f4fea`) — `Secret<T>` shipped: injected generic class,
  non-printable (compile error) + private field (`.expose()` sole read path), `W-SECRET` lint on
  expose→sink, `final class Secret` + `#[\SensitiveParameter]` transpile. 1360 tests green, clippy+fmt
  clean, `examples/guide/secret.phg` byte-identical run≡runvm≡PHP 8.5. NOT pushed. **Both forks done →
  next: Item C (statics research), then Item D (LSP).**
- [2026-06-28] FORK B RESOLVED — **`Secret<T>` = Path 1 (opaque & non-printable)**, design
  `docs/specs/2026-06-28-secret-type-design.md`. An implementation discovery reopened the earlier
  "displays as `***`" wording: Phorge's `as_display` renders only primitives, so a class-typed `Secret`
  is **already unprintable** — `Console.println(s)`/`"{s}"` is a clean type error, the strongest+loudest
  guarantee, free from the type system. Chosen over Path 2 (runtime `***`, which needs a new `Value`
  variant + a *silent* `***`) — loud > silent, no new `Op`/`Value`. Model: an **injected generic class**
  `class Secret<T> { constructor(private T value){} function expose(): T {…} }` (gated on
  `import Core.Secret;`; reuses `Box<T>` machinery). `.expose()` sole read path (field private);
  `W-SECRET` lint flags `s.expose()` as a *direct* sink argument (syntactic; full taint deferred).
  Transpiles to `final class Secret` + `#[\SensitiveParameter]`. Byte-identical run≡runvm≡PHP.
- [x] 3. M-Test — **COMPLETE** (T1–T5: `test` item + `Core.Test` + `assertFaults` + `phg test` runner + `selftest/` showcase). GA 49% → 52%.
- [ ] 4. M-text
- [ ] 5. breadth gaps
- [ ] 6. M-NUM S4
- [ ] 7. lift L5
- [ ] 8. release-readiness

## Post-rock-3 locked sequence (2026-06-28, developer-chosen; fully autonomous)

> The 3 locked items (overloaded statics → LSP v2 → rock 3) are DONE (commits `027f0fc`…`c71fba8`,
> 6 ahead of origin). Developer will **compact + push** those, then I run the NEXT sequence fully
> autonomously (persistent project bypass ON). Run order: **(1) conformance breadth → (2) M6 W2
> router+attributes → (3) M2.5 Phase 3a**. Each: spec-first where non-trivial, TDD, byte-identity-gated
> (run≡runvm≡real PHP 8.5), commit green, **NEVER push**, recommended-defaults at remaining sub-forks,
> stop only for a *fundamental* design fork or a risky/destructive action. Rebuild the release binary
> after each feature; end every status with `GA: ~X% · Global: ~Y%`.

### Decisions Log (this round)
- [2026-06-28] IN PROGRESS: **M6 W3 — serve concurrency (spec-first)**. Research finding (VERIFIED by a
  compile-time `assert_send_sync::<ast::Program>()` probe): **`ast::Program` is `Send + Sync`**, and serve
  runs the *interpreter* over `&Program` with a **per-request `Value` heap** (Rc values never cross
  threads). ⇒ **bounded OS-thread-per-request is feasible** — `Arc<Program>` shared, each worker its own
  heap; real multi-core, std-only (`std::thread`), no `unsafe`, no `Value: Send`. This **revises** the
  long-documented "single-threaded by force → green-threads" plan: green-threads is *dominated* (hard
  std-only — no async runtime / generators unstable / stack-switching needs unsafe — and single-core).
  `BytecodeProgram` is NOT Send (Value constants), but serve uses the interpreter, so irrelevant. Spec:
  `docs/specs/2026-06-28-m6-w3-serve-concurrency-design.md`. **Bringing the model + CLI defaults decision
  back to the developer before building** (their explicit request).
- [2026-06-28] DONE: **M6 W2-ext slice 3 — `#[Route]` on class methods → W2-ext COMPLETE**. Attribute
  parser extended to class methods; a `#[Route]` method must be **static** (`E-ROUTE-METHOD-STATIC`);
  `Http.autoRouter()` lowers each static method to a `fn(req) => Class.method(req)` handler lambda
  (the `Route` tuple's handler is now an `Expr`, not a name). `examples/web/controller.phg` +
  conformance; byte-identical 3-way. **M6 W2 extensions (middleware → groups → constraints → method
  attrs) is now COMPLETE.** Natural point to push + tag (developer action) and validate the M2.5
  stub-registry CI; W3 serve/concurrency is the web-layer capstone after.
- [2026-06-28] DONE: **M6 W2-ext slice 2 — regex/typed route constraints**: `{name:regex}` whole-segment
  anchored match via Core.Regex; precedence literal > constrained > bare; prelude imports Core.Regex.
  `examples/web/route-constraints.phg` + conformance; byte-identical 3-way. **Gotcha:** strip only OUTER
  braces (`Text.substring(seg,1,-1)`) — a constraint may contain `{}` (`\d{4}`). **NEXT: slice 3
  (`#[Route]` on class methods).**
- [2026-06-28] DONE: **M6 W2-ext slice 1 — middleware + route groups** (`f01035e`): `router.use(mw)`
  (`(Request,next)->Response`, short-circuit or pass-through) + `router.group(prefix, build)`; pure
  Phorge, no new Op/Value, byte-identical 3-way. Also fixed two latent VM-compiler CTy gaps found while
  prototyping: a native-qualified call as an arithmetic operand (`e44bc29`) and a static-method call as
  a fn-value (folded into `f01035e`). **NEXT: slice 2 (regex/typed route constraints `{id:\d+}` via
  Core.Regex) → slice 3 (`#[Route]` on class methods).** Spec `docs/specs/2026-06-28-m6-w2-extensions-design.md`.
- [2026-06-28] AGREED: **after the 3 locked items (all ✅ DONE), next milestone = M6 W2 EXTENSIONS**,
  built in slices: **(1) middleware + route groups (closure-pipeline)** → (2) regex/typed route
  constraints (`{id:\d+}`, via Core.Regex) → (3) `#[Route]` attributes on class methods. Developer
  picked my recommended order (web spine before stepping to other parity tracks; W3 concurrency is the
  later capstone). `git push` + cutting a `v*` tag (to exercise `stub-registry.yml`) stays the
  developer's action. Run fully autonomous, spec-first, byte-identity-gated, never push.
- [2026-06-28] AGREED: sequence after push/compact = **(1) finish conformance breadth → (2) M6 W2
  router → (3) M2.5 Phase 3**. (Developer: "we do 2 then 1 then 3" = option2→option1→option3.)
- [2026-06-28] AGREED: **M2.5 = Phase 3a ONLY** (CI stub registry + download-and-cache + baked sha256
  manifest + release workflow — zero credentials). **Phase 3b DEFERRED** (`--sign` Authenticode/
  codesign/notarize + macOS stub — needs certs + a Mac SDK; can't provision autonomously).
- [2026-06-28] AGREED: **M6 W2 = Router + FULL path params + the attribute system, NOW** (developer
  overruled my "static-exact-match + design-attrs-next" rec; chose the full milestone). My challenges,
  resolved:
  - *Params now*: YES — routing is pure/deterministic (the determinism concern is W3's socket, not
    routing); params ride **PSR-15-style as Request attributes** so the locked `handle(Request) ->
    Response` contract is untouched. **Precedence: literal segment beats param segment**
    (most-specific-first); first-registered breaks a true tie; 404 fallback. `req.param("id") -> string?`.
  - *Attributes now*: built this milestone. Sub-forks pre-decided (recommended defaults, in the spec):
    **syntax = PHP-8 `#[Route("GET", "/users/{id}")]`** (new lexer/parser/AST surface; parse `#[Name(args)]`
    generally, wire only `Route` this milestone); **mechanism = COMPILE-TIME DESUGAR, not runtime
    reflection** — collect `#[Route]`-annotated free functions at load/check and lower
    `Http.autoRouter() -> Router` into explicit `new Router()` + `.route(method, path, handler)` per
    collected handler (expand-before-backends discipline ⇒ byte-identity trivial, no runtime attr
    machinery). [Verified 2026-06-28: Phorge has NO attribute syntax today — only Rust derives in the
    compiler — so this is a genuinely new surface.]
- [2026-06-28] AGREED: developer wants me **fully autonomous** for the whole sequence; asked all
  questions up front; no further blocking forks (sub-decisions defaulted + documented).

### Item 1 — conformance breadth (fork-free) — ✅ DONE (`86db429`)
8 golden programs added (loops, foreach, ranges, text-blocks, type-aliases, visibility, property-hooks,
fixed-lists); corpus 24→32 single-file. (lambdas+pipe + default-params were already in `functions.phg`.)

### Item 2 — M6 W2 router + attributes — ✅ DONE (`32d77f4` spec, `8c5d073` router, `986a9f0` attrs)
Core.Http `Router` + path params (`req.param`, literal>param precedence, 404) + `#[Route]` attribute
(new `#[` lexer token, parser, `FunctionDecl.attrs`, checker `E-UNKNOWN-ATTRIBUTE`/`E-ROUTE-*`/
`E-ATTR-TARGET`, `phg explain`) + `Http.autoRouter()` compile-time desugar (`src/checker/desugar_router.rs`,
pre-check injection). fmt printer emits attrs. Byte-identical run≡runvm≡real PHP 8.5; full gate green.

### Item 1 (original spec) — conformance breadth (fork-free)
Add the remaining stable-tier (`STABILITY.md`) constructs as golden conformance programs
(run≡runvm≡real PHP, glob-gated by `tests/conformance.rs`): mutation/loops (while/do/C-for,
compound-assign, ++/--), visibility (public/private/protected access), property hooks, lambdas+pipe,
foreach (+ `with i`), text-blocks (`"""…"""`) + raw strings, default parameters, ranges. ~8 programs.

### Item 2 — M6 W2 router + attributes (milestone, spec-first)
Spec → `docs/specs/2026-06-28-m6-w2-router-attributes-design.md`. Build order:
(a) **Router** class (pure Phorge, on the injected Core.Http Request/Response): `route(string method,
string pattern, handler)` + `handle(Request) -> Response`; segment matcher with `{name}` capture →
Request attributes (`req.param`); literal>param precedence; first-registered tie-break; 404 fallback.
Byte-identity-gated + a guide/conformance example.
(b) **`#[...]` attribute syntax**: lexer (`#[`), parser (item-level attribute list on free functions —
`#[Route("GET","/p")]`), AST (`Attribute{name, args}` on `FunctionDecl`). General parse; only `Route`
semantically recognized this milestone (others → a clean "unknown attribute" error or ignored — decide
in spec, recommend a soft `W-UNKNOWN-ATTRIBUTE` or hard `E-UNKNOWN-ATTRIBUTE`; lean hard-error for
safety).
(c) **Auto-registration**: `Http.autoRouter()` (injected) collected + lowered at the
`check_and_expand`/loader chokepoint into explicit registration of every `#[Route]` handler (handlers
referenced as first-class fn values). All three backends see explicit registration ⇒ byte-identical.
(d) Example (`examples/web/router.phg` or guide), conformance program, `phg explain` for new codes,
README/CHANGELOG/KNOWN_ISSUES. (W3 socket serve runtime stays the next slice; W2 is pure + gated.)

### Item 3 — M2.5 Phase 3a (CI stub registry; NO signing) — ✅ DONE (`3d6e7bc`)
Shipped `bundle/sha256.rs` (hand-rolled FIPS SHA-256, FIPS+cross-impl tested), `bundle/manifest.rs`
(tolerant parser + lookup + registry_base + `phg-stub-<triple>` asset names), `build.rs` (bakes
`PHORGE_BAKE_STUB_MANIFEST`, empty default → breaks the stub↔manifest circularity), the `bundle/cross.rs`
3-way branch (`download_stub`/`fetch`: verify-before-cache, curl/file:// transport), `Cargo.toml`
`repository`. CI workflow shipped as **`.github/workflows/stub-registry.yml`** (the spec's `release.yml`
name was taken → distinct file, complements it). Tests: `tests/registry.rs` (hermetic client +
tamper-rejection + cross-impl hash) + toolchain-gated `tests/build.rs` end-to-end (real musl stub →
download → verify → embed → run ≡ runvm). Phase 3b (signing/macOS) deferred. **ALL 3 LOCKED ITEMS DONE.**

### Item 3 (original spec) — M2.5 Phase 3a (CI stub registry; NO signing)
Spec already exists: `docs/specs/2026-06-17-m2.5-phase3a-stub-registry-design.md`. Build: `bundle/
sha256.rs` (std SHA-256), `bundle/manifest.rs` (parse + lookup + `registry_base` via Cargo.toml
`repository` / `PHORGE_STUB_REGISTRY` / `PHORGE_STUB_MANIFEST` overrides), `download_stub()` 3-way
branch in `bundle/cross.rs::build_stub` (cache→local-build→download), `build.rs` bakes the per-target
sha256 manifest into the released `x86_64-linux-gnu` primary, `.github/workflows/release.yml`
(build stubs → hash → bake → publish), `Cargo.toml` `repository`. Integrity gate (sha256 mismatch →
fault), EV-7 checked arithmetic. `tests/build.rs` fixture coverage (offline; a fake manifest+stub).
Phase 3b (signing/macOS) DEFERRED in KNOWN_ISSUES.
