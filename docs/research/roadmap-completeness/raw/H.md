# Track H — Correctness & safety guarantees

## Track summary

Phorj's headline promise is "if it type-checks, whole classes of runtime errors are gone" (VISION
§1) plus EV-7 ("never panics on input"). The EV-7 / no-panic / no-`unsafe` pillar is in excellent
shape: integer arithmetic is fully checked and single-sourced (`value.rs`: `int_add/sub/mul/div/rem/
neg → Result`), `i64::MIN/-1` and `-i64::MIN` are clean faults, literal overflow is a lex error,
range materialization is capped, the whole pipeline runs on a bounded 256 MB worker with explicit
depth limits, malformed binaries are read with checked arithmetic, and `#![forbid(unsafe_code)]` +
deny-warnings hold the line. Backend parity (`run ≡ runvm ≡ real PHP`) is enforced by the
differential harness. **Where the track genuinely leaks is the *provably-correct* half — the
type-system completeness / totality side**, and the leaks are real:

- **No return-path totality check.** A function declared `-> int` that falls off the end without
  returning type-checks clean and silently yields `unit`. `f(-5) + 1` then type-checks (the checker
  reifies the call as `int`) but faults at runtime on *both* backends with *different* fault bodies
  (`cannot apply Add to unit and int` vs `expected int, found unit`) — a soundness leak straight
  through the headline promise. [Verified: `/tmp/unitleak.phg` — `check` OK, `run`/`runvm`/PHP all
  fault.] This is the single most important finding in the track.
- **Exhaustiveness is checked for `match` (enums + unions) but coverage is otherwise narrow:** no
  flow-narrowing on the else-branch of `instanceof`, no exhaustiveness/totality for `if`/`else`
  chains, and `match` is restricted to return / initializer position (totality-relevant
  expressiveness gap). Several "provable safety" wins are left on the table (const div-by-zero / const
  OOB folding, a `never`/non-returning type, `assert`/contract diagnostics, sealed-hierarchy
  exhaustiveness for `extends`). These are PHP pain points (PHP only catches missing returns via
  PHPStan L2 / Psalm; PHP 8.1 added `never` exactly for this) — textbook TypeScript-over-JavaScript
  upgrades that a PHP dev instantly understands.

The no-panic guarantees should be **adopted as standing invariants and CI-fuzzed**, not re-listed as
features; the type-completeness items below are the actual roadmap gaps.

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| H-return-totality | Return-on-all-paths (missing-return) check | port | strong | adopt | M-RT (next) | M |
| H-never-type | `never` / non-returning return type | port | strong | adopt | M-RT | S |
| H-instanceof-else-narrow | Negative / else-branch flow narrowing | port | strong | adopt | M-RT (with S6 `extends`) | M |
| H-sealed-exhaustive | Sealed-hierarchy exhaustive `match` (post-`extends`) | new | ok | defer | M-RT S6+ | M |
| H-const-fold-diagnostics | Compile-time const div-by-zero / OOB / map-key diagnostics | new | ok | defer | M-RT or M9 | M |
| H-match-position | `match` in arbitrary expression position | port | strong | adopt | M-RT | M |
| H-assert-contracts | `assert(cond)` + lightweight pre/post contracts | port | ok | defer | M11 / new milestone | M |
| H-overflow-policy-doc | Document & test arithmetic-overflow *policy* as an invariant | map | strong | adopt | M9 | S |
| H-ev7-fuzz | Fuzz / property-test the EV-7 no-panic guarantee in CI | map | strong | adopt | M8/M9 | M |
| H-checker-invariant-audit | Audit every `unreachable!` for checker-completeness coupling | map | strong | adopt | M9 | M |
| H-faultkind-parity-totality | Make the unit-leak fault the *same* `FaultKind` on both backends | port | strong | adopt | M-RT (folds into H-return-totality) | S |
| H-result-error-model | Catchable error model (`try/catch` / `Result<T,E>`) — totality of error handling | port | strong | defer | Error-handling slice 2 | L |
| H-decimal-money | `decimal` / exact money type (no float rounding surprise) | port | ok | defer | M11 / future | L |
| H-sized-int-overflow | Sized integers with explicit wrap/checked/saturate ops | new | weak | defer | v2 | L |
| H-totality-purity | Totality / purity annotations (`pure`, total fns) | new | weak | reject | — | L |
| H-refinement-types | Refinement / dependent types (`int where x>0`) | new | weak | reject | — | L |
| H-exhaustive-bool-int | Exhaustiveness for `bool` / small-int `match` without `_` | new | ok | defer | M-RT | S |

## Rationale per ADOPT item

**H-return-totality (adopt, M-RT, M).** This is the headline correctness leak. A `-> T` function that
can fall off the end is accepted and leaks a `unit` value where a `T` was promised, which then
detonates downstream at runtime — exactly the "runtime error the type system promised to remove."
PHP itself doesn't catch this (implicit `null` return); PHPStan L2 / Psalm do, and PHP 8.1's `never`
type exists for the proven-non-returning case. A PHP dev reads "missing return" as an obvious, welcome
diagnostic. Implementation is a standard reachability/terminator analysis over the function body
(every path ends in `return`/`throw`/an exhaustive `match`/a `never` call), front-end-only, no
backend or `Op` change, no parity risk. New code e.g. `E-MISSING-RETURN`. Should land before
overloading/`extends`, since those add more paths to reason about. Folds in **H-faultkind-parity-totality**
(the divergent fault bodies disappear once the leak is a compile error).

**H-never-type (adopt, M-RT, S).** The natural companion: a `never` (or `Never`/`!`) bottom type for
functions that always throw/abort/loop, so the totality checker can treat a call to one as a
terminator (and reject any `return` inside one). Maps 1:1 to PHP 8.1 `never`. Tiny, legible, and it
makes H-return-totality precise rather than conservative.

**H-instanceof-else-narrow (adopt, M-RT alongside S6 `extends`, M).** Today `if (x instanceof Circle)`
narrows the then-branch but the else-branch does not narrow a union to its remaining members
(KNOWN_ISSUES). For a union `A|B`, the else of `instanceof A` should narrow to `B` — this is what
makes union code *total* without a redundant final arm, and it's the same smart-cast machinery already
shipped, extended to the negative position. Strongly PHP-familiar (it mirrors how a PHP dev mentally
reasons after a type check) and directly serves the provable-correctness pillar.

**H-match-position (adopt, M-RT, M).** `match` is currently only legal in return / var-initializer
position; arbitrary expression position is rejected (KNOWN_ISSUES). Since `match` is the *exhaustive*
construct, restricting where it can appear pushes users toward non-exhaustive `if`/`else` ladders —
i.e. the restriction works *against* the correctness pillar. Lifting it (lowering an
expression-position `match` the way expression-`if` already lowers) makes the safe construct usable
everywhere. No new `Op` (reuses the branch lowering); front-end + compiler emit only.

**H-overflow-policy-doc (adopt, M9, S).** The fault-on-overflow behavior is correct and tested, but
it's a *policy* (Phorj chose fault-not-wrap, unlike PHP's silent float promotion in `array_sum`, and
unlike Rust release-mode wrap). It deserves a one-line entry in `docs/INVARIANTS.md` ("integer ops
fault on overflow; never wrap, never promote to float") plus an explicit differential test asserting
the policy at each op boundary, so a future "make it wrap for perf" change is a conscious invariant
break, not a silent drift. Cheap, high-leverage documentation of an existing guarantee.

**H-ev7-fuzz (adopt, M8/M9, M).** EV-7 ("never panics on input") is asserted and spot-tested but not
*fuzzed*. A small `cargo`-driven fuzz/property harness over the lexer→parser→checker→both backends
(random `.phg` and random bytes into the object-section readers) would turn EV-7 from "we believe it"
into "we continuously prove it" — the strongest possible backing for the no-crash half of the pillar,
and exactly the rigor VISION §2 promises. Std-only-friendly (can be a `#[test]`-driven random-input
loop; no external fuzzer crate needed to start).

**H-checker-invariant-audit (adopt, M9, M).** The backends contain ~6 `unreachable!("checker
restricts …")` sites (interpreter/compiler/vm). Each is a load-bearing assumption that the checker
fully rules out a shape. The return-totality leak proves these assumptions can be incomplete. A
one-time audit — for each `unreachable!`, write a test that the checker *actually* rejects the input
it claims to — converts implicit coupling into verified coupling and is the natural follow-on to
H-return-totality.

Sources:
- [`never` return type — PHP 8.1 · PHP.Watch](https://php.watch/versions/8.1/never-return-type)
- [PHP 8.1 RFC: never type](https://php.watch/rfcs/noreturn_type)
- [PHPStan: Function should return X but return statement is missing (L2)](https://phpstan.org/error-identifiers/missingType.return)

## Critic pass

Verified the shipped state against `src/`, `FEATURES.md`, `KNOWN_ISSUES.md`, and the project
CLAUDE.md log before judging.

**Confirmed leaks / non-leaks (sanity check):**
- Return-totality leak is **real and unaddressed** — `grep` for `E-MISSING-RETURN` /
  reachability / terminator in `src/`,`docs/` returns nothing. H-return-totality stands as the
  top finding. [Verified: no such code.]
- `match` exhaustiveness for **enums + unions** is shipped (`checker.rs` ~3465–3509,
  `non-exhaustive match: missing …`); non-enum/non-union scrutinees still require a `_` arm, so
  H-exhaustive-bool-int (bool/finite exhaustiveness without `_`) is a genuine gap. Correctly listed.
- `match`-in-arbitrary-expression-position **is** restricted (KNOWN_ISSUES line 65). H-match-position
  is correctly listed, not mis-listed.
- The `W-FORCE-UNWRAP` lint and the non-gating warning channel already ship (M-RT S2) — so the
  "warning" delivery vehicle the new items below rely on **exists**, lowering their cost.

**Non-gaps found while auditing (do NOT add — close them off):**
- **Cross-type equality is already rejected** (`checker.rs:1947` — `cross-type comparison requires
  explicit conversion`; test `equality_requires_same_type`). PHP's loose-`==` footgun is already
  fixed; no gap. [Verified.]
- **Definite-assignment / use-before-init is structurally impossible** — a binding is introduced by
  `var x = init` / typed-decl-with-initializer, always initialized at its declaration site; there is
  no declare-then-assign-later form (mutation reassigns an already-bound local). No gap.

**Mis-listings:** none. Every listed item is genuinely unshipped. (H-faultkind-parity-totality is a
*dependent* of H-return-totality, not a mis-listing — it correctly folds in.)

**Newly-found gaps (missed by the first pass):**

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| H-unreachable-after-return | Dead-code-after-terminator diagnostic (statement after `return`/exhaustive `match`/`never` call) | port | strong | adopt | M-RT (with H-return-totality) | S |
| H-unused-binding-warn | Unused local / unused import warning (non-gating) | port | ok | adopt | M9 | S |
| H-match-arm-overlap | Duplicate / unreachable `match` arm diagnostic (repeated literal, arm after catch-all) | new | ok | adopt | M-RT | S |

- **H-unreachable-after-return (adopt, M-RT, S).** The exact reachability analysis H-return-totality
  introduces also yields, for free, a diagnostic on a statement that follows a terminator (TypeScript
  `allowUnreachableCode`, Rust `unreachable_code`, Kotlin warn). PHP catches this only via PHPStan;
  a PHP dev reads it instantly. Front-end-only, no `Op`/backend/parity change, shares the totality
  pass's machinery. Should ship in the same slice as H-return-totality (same analysis, opposite edge).
- **H-unused-binding-warn (adopt, M9, S).** A `var x = …` never read, or an `import` never used, is a
  legibility-and-correctness smell every PHP dev expects from PHPStan/Psalm. The non-gating **warning
  channel already exists** (M-RT S2, `check()` returns `Ok(warnings)`), so this is a small additive
  pass that never gates the build — exactly on-philosophy (removes a surprise, costs no capability).
  Front-end-only; no parity risk (warnings are stderr-only, never affect output).
- **H-match-arm-overlap (adopt, M-RT, S).** The exhaustiveness checker already enumerates the arm set;
  flagging a **repeated literal arm** or any arm **after a catch-all `_`** (statically dead) is a cheap
  extension that closes a correctness footgun — and is the natural companion to the zero-payload-enum
  `V()` catch-all trap documented in KNOWN_ISSUES (a stray `V =>` silently shadows later arms; an
  overlap/dead-arm diagnostic would surface it). Front-end-only, no `Op`/parity change.

**Net:** 3 newly-found, 0 mis-listed/removed. Merged track total: 21 items.
