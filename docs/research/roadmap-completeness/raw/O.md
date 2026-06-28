# Track O — Testing & quality story

## Track summary

Phorj today ships **zero first-class testing capability for end-user code**. The robust testing
machinery that exists — `tests/differential.rs` (the `run ≡ runvm ≡ real PHP` oracle), the PHP
oracle, `phg bench`, `phg disasm` — is *internal to Phorj's own Rust development*; a developer
writing a `.phg` program gets nothing. There is no `phg test` runner, no assertion library, no test
discovery, no fakes/mocks, no property testing, no coverage, no snapshot testing, no seedable RNG /
injectable clock for deterministic tests. This is the single largest "ecosystem table-stakes" gap in
the whole roadmap: a statically-typed language that markets itself as *provably correct* but offers
no way to write and run tests is incomplete on its own terms. The PHP-familiar anchor is **PHPUnit**
(plus Pest's terse style, Infection for mutation testing); the beyond-PHP anchors are Rust's built-in
`#[test]`/`cargo test`/doctests, Go's `go test` + table-driven tests + `testing/quick`, Swift Testing,
Elixir's `ExUnit` + `doctest`, and Hypothesis/QuickCheck/`proptest` for property-based testing. The
recommended core is a **built-in `phg test` runner + a typed `Core.Test` assertion stdlib**, both of
which transpile cleanly (Phorj tests can even run under PHPUnit as generated PHP). Higher-power items
(property testing, mutation testing, coverage, snapshot) layer on top and are mostly deferable, but a
few — a **seedable `Core.Random` / injectable `Core.Time`** seam — are prerequisites that unblock
*deterministic* testing and should land early because the byte-identical spine forbids ambient
nondeterminism anyway.

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| O-test-runner | Built-in `phg test` runner + test discovery | port | strong | adopt | new milestone M-Test | L |
| O-assert-lib | `Core.Test` typed assertion library | port | strong | adopt | new milestone M-Test | M |
| O-deterministic-seam | Seedable `Core.Random` + injectable `Core.Time` (test seam) | new | strong | adopt | M-Test (prereq) | M |
| O-table-driven | Table-driven / parameterized tests (data providers) | port | strong | adopt | M-Test | M |
| O-doctest | Doctests — runnable assertions in doc comments | new | ok | defer | M-Test+1 | M |
| O-snapshot | Snapshot / golden-file testing | new | ok | defer | M-Test+1 | M |
| O-property | Property-based testing (`Core.Test.forAll`) | new | ok | defer | M-Test+1 | L |
| O-fakes-traits | Fakes/stubs via interfaces (no reflection mocking) | map | strong | adopt | M-Test | S |
| O-mock-reflection | Reflection-based mock framework (Mockery/PHPUnit style) | omit | weak | reject | — | — |
| O-coverage | Code coverage instrumentation + report | new | ok | defer | M-Test+2 | L |
| O-fuzz | `phg fuzz` — coverage-guided fuzzing of user code | new | weak | defer | v2 | L |
| O-mutation-testing | Mutation testing (Infection-style) | new | weak | defer | v2 | L |
| O-bench-user | User-facing micro-benchmark harness (`#[bench]`) | new | ok | defer | M-Test+2 | M |
| O-phpunit-bridge | PHPUnit interop — transpiled tests runnable under PHPUnit | map | ok | adopt | M-Test | S |
| O-contracts | Design-by-contract `requires`/`ensures` as testable guards | port | strong | defer | M3 (own slice) | M |
| O-assert-stmt | `assert(cond)` statement (debug-time invariant check) | port | ok | defer | M-Test | S |

## Rationale for ADOPT items

**O-test-runner — built-in `phg test` runner + discovery.** This is the keystone. Rust (`cargo
test`), Go (`go test`), Swift, Elixir all ship a runner in the box; PHP needs PHPUnit but it is the
universal de-facto standard a PHP dev expects. The most PHP-familiar *and* most legible form is a
convention-based discovery: any `package` file (or a `tests/` tree / `*_test.phg`) whose free
functions are annotated (e.g. `@test` or a `test` keyword form) is collected and run, with pass/fail
counts and a clean diagnostic on failure — exactly the ergonomics of `cargo test`. It fits the
philosophy perfectly: it removes a *surprise* (PHP's "bring your own runner + config + autoloader")
without adding capability beyond what every modern language has. Transpiles to a generated PHPUnit
suite (or runs natively on the VM). Effort is L because it needs CLI wiring, discovery, a result
reporter, and exit-code semantics — but it has no new `Op` and no spine risk (tests run, assertions
fault on mismatch like any other fault).

**O-assert-lib — `Core.Test` typed assertions.** A runner is useless without assertions. The
PHP-familiar shape is `assertEqual(a, b)`, `assertTrue(c)`, `assertThrows(...)` — but Phorj can do
*better*: assertions are statically typed (`assertEqual<T>(T, T)` rejects comparing an `int` to a
`string` at compile time, a class of PHPUnit footgun gone) and report a byte-identical diff using the
same diagnostic surface as the compiler. Erases to PHPUnit `$this->assertSame(...)` on transpile.
Strong fit, additive, no spine risk (a failed assertion is a clean fault).

**O-deterministic-seam — seedable `Core.Random` + injectable `Core.Time`.** Already flagged in the
parity spec (line 453) as a future native. It is a *prerequisite* for testing, not a luxury: the
byte-identical `run ≡ runvm ≡ real PHP` spine structurally forbids ambient nondeterminism, so any
randomness/time must already flow through an explicit, seedable seam. That same seam is exactly what
makes user tests deterministic and reproducible. Building it as part of the testing milestone kills
two birds. Maps to PHP `mt_srand($seed)` / an injected clock; strong philosophical fit (determinism
is a stated design principle).

**O-table-driven — parameterized / data-provider tests.** PHPUnit `#[DataProvider]` and Go's
table-driven idiom are the single most-used test-scaling pattern. The legible Phorj form is a test
function taking a `List<Case>` (a typed struct of inputs+expected) and the runner iterating it — far
cleaner than PHPUnit's stringly-typed reflection providers, and it leverages Phorj's existing typed
records and `for…in`. Adopt within the testing milestone.

**O-fakes-traits — fakes/stubs via interfaces.** Phorj already has interfaces + nominal subtyping
(M-RT S2). The idiomatic, legible way to substitute a dependency in a test is to implement the
interface with a hand-written fake — exactly Go's and modern-PHP's preferred style (constructor
injection + a test double class). This is a *map* gap: it is already expressible, but the testing
docs/guide must establish it as the blessed pattern. Cheap (S) — mostly documentation + an example —
and it lets Phorj **reject** the reflection-based magic-mock approach (O-mock-reflection) cleanly.

**O-phpunit-bridge — transpiled tests run under PHPUnit.** Because Phorj transpiles to PHP, a
Phorj test suite can emit a real PHPUnit test class, letting teams adopting Phorj incrementally run
Phorj tests inside their existing PHP CI. This is the migration-bridge philosophy applied to testing
and is nearly free once O-test-runner + O-assert-lib exist (it is a second emission target of the same
machinery). Small effort, high adoption value.

## Critic pass

### Mis-listings (already-shipped or wrongly-scoped)

Re-checked every listed item against `FEATURES.md`, `KNOWN_ISSUES.md`, `docs/MILESTONES.md`, and the
project CLAUDE.md milestone log. **No listed item is already shipped** — the track summary is correct
that end-user testing capability is completely absent (the only testing machinery, `tests/differential.rs`
/ the PHP oracle / `phg bench`, is Rust-internal). One scoping note (not a removal):

- **O-assert-stmt** overlaps **O-contracts** *and* the assertion half of **O-assert-lib**, and the
  rationale already says "fold into whichever lands first." It is a genuine candidate but its identity
  is thin — a bare `assert(cond)` is the debug-invariant subset of `requires`. Kept (the researcher's
  own "defer + fold" verdict is right), flagged here so it isn't double-counted as a distinct feature.

`removed_mislisted = 0`.

### Newly-found gaps (missed long tail)

The original list nails the headline items (runner, assertions, determinism seam, table-driven,
property, fakes-vs-mocks, coverage). It misses the **runner-internal ergonomics** that every PHPUnit /
`cargo test` / `go test` user reaches for daily — these are not optional polish, they are what makes a
runner usable, and a PHP dev would be surprised by their absence:

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| O-fixtures | Setup/teardown fixtures (`setUp`/`tearDown` / `beforeEach`/`afterEach`) | port | strong | adopt | M-Test | M |
| O-test-selection | Test selection & filtering (`phg test --filter` / tags) | port | strong | adopt | M-Test | S |
| O-skip-focus | Skip / focus / pending tests (`@skip`, `@only`, `markIncomplete`) | port | strong | adopt | M-Test | S |
| O-assert-fault | `assertFaults` / `assertThrows` — fault-assertion as a first-class assertion | port | strong | adopt | M-Test | M |
| O-ci-report | Machine-readable test report (JUnit-XML / TAP) for CI | port | ok | adopt | M-Test | S |
| O-test-isolation | Per-test fresh state / no cross-test leakage guarantee | new | strong | adopt | M-Test | S |

**O-fixtures — setup/teardown.** PHPUnit `setUp`/`tearDown` (and Jest/Vitest `beforeEach`/`afterEach`,
ExUnit `setup`) are the single most-used runner feature after the assertion itself: a per-test
construction of the system-under-test. Without it every test hand-rolls its own arrange step — the
exact boilerplate a runner exists to remove. The most PHP-familiar, legible form is a convention
(`setUp()`/`tearDown()` free functions or annotated functions discovered per test file) that the runner
calls around each test. No spine risk (just ordered calls); maps directly to PHPUnit's lifecycle on
transpile. Strong fit — removes a surprise, adds no capability. [Inferred: PHPUnit `setUp`/`tearDown`
are core lifecycle hooks — stable, well-known PHP.]

**O-test-selection — `--filter` / tags.** Every runner lets you run a *subset*: `cargo test <name>`,
`go test -run <regex>`, PHPUnit `--filter` / `@group`, Pest `--group`. On any suite past trivial size
this is non-negotiable for the inner dev loop (run the one failing test). Cheap: it's CLI-flag + a
predicate over discovered names/tags, no new language surface. Folds naturally into O-test-runner but
is worth listing because a runner shipped *without* selection is half a runner. Strong fit. [Inferred:
`--filter`/`-run`/`@group` are universal across PHPUnit/cargo/go.]

**O-skip-focus — skip / focus / pending.** `#[ignore]` (Rust), `t.Skip()` (Go), `markTestSkipped` /
`markTestIncomplete` (PHPUnit), `.only`/`.skip` (Jest), `@tag :skip` (ExUnit). A WIP or
environment-gated test must be expressible as skipped-with-reason rather than deleted or commented out,
and "run only this one" (focus) is the debugging counterpart of selection. Small, convention-driven,
reports in the summary. Strong fit — directly serves the legible-iteration loop. [Inferred: skip/only
are present in every mainstream runner.]

**O-assert-fault — fault-assertion as first-class.** The list's O-assert-lib mentions `assertThrows`
*in passing*, but for Phorj this is load-bearing enough to call out: Phorj has **no exceptions yet**
(try/catch is M3-deferred), so the only failure mode a user's code has is a **clean fault** (div-by-zero,
index-OOB, `opt!`-on-null, a future `requires`). Asserting "this input *faults*" (and ideally with a
`FaultKind` / message match — exactly what the internal `agree_err`/`FaultKind` harness already does
internally) is the primary negative-path test and currently impossible to express. It needs the runner
to *catch* a fault instead of aborting the process — a real mechanism, not just a function — so it's M
not S, and it should be designed alongside the eventual catchable-error slice (memory:
error-handling-and-whole-project-validation). PHPUnit anchor: `expectException`. Strong fit — it's the
test surface for Phorj's distinctive fault model. [Verified: KNOWN_ISSUES documents faults as the
universal failure mode and that `try`/`catch` is deferred; the internal harness already classifies by
`FaultKind`.]

**O-ci-report — JUnit-XML / TAP output.** A test runner that only prints to a terminal is invisible to
CI dashboards. PHPUnit `--log-junit`, `cargo test`'s libtest JSON, Go's `-json`, TAP — every mature
runner emits a machine-readable report so CI can show per-test pass/fail and trend it. Phorj already
has the JSON-diagnostic precedent (`phg check --json`), so a `phg test --format=junit|tap|json` is the
same discipline. Small once the runner has a structured result model. Ok/strong fit — table-stakes for
the CI-mature teams the migration-bridge story targets. [Inferred: JUnit-XML is the de-facto CI test
format; PHPUnit ships `--log-junit`.]

**O-test-isolation — per-test fresh state.** Phorj's immutable-by-default + acyclic-heap + no-ambient-
globals model means tests are **already** far more isolated than PHP's (no superglobal/static bleed
between tests — a structural superpower, like the whole-project-validation one). This is mostly a *map*
+ a stated guarantee rather than new machinery, but it deserves an explicit line: the runner should
document and lean on "each test runs against fresh state; there is no shared mutable global to reset" as
a *removed PHP surprise* (PHPUnit's `@backupGlobals`, `@runInSeparateProcess`, `static`-reset dances all
evaporate). Cheap; strong fit; reinforces the determinism story. Note: once mutation + `static mutable`
class fields ship (M-mut.7a, already landed), a test *can* mutate a static — so the guarantee needs a
documented caveat there (reset statics between tests, or forbid in tests). [Verified: `static mutable`
class fields shipped (M-mut.7a per CLAUDE.md/KNOWN_ISSUES) — the one place cross-test state can leak.]

`new_found = 6`.

### Recommendation sanity-check (philosophy)

All original verdicts hold up against the philosophy lens. Spot-checks:

- **O-mock-reflection — reject** is *correct and well-reasoned*: runtime-reflection mocking is exactly
  the dynamic surprise Phorj removes, and O-fakes-traits is the strictly-better PHP-familiar
  alternative (constructor injection + a hand-written interface fake). Keep the reject.
- **O-fuzz / O-mutation-testing — defer to v2** is right: low PHP-dev demand, heavy machinery; property
  testing covers most of the value sooner. (Mutation testing's `philosophy_fit: weak` is slightly
  harsh — for a *provably-correct* language, test-suite-quality measurement is on-mission — but the
  sequencing verdict stands; weak-fit reads as "not now," which is correct.)
- **O-deterministic-seam — adopt as prereq** is well-placed and corroborated by the parity spec's
  "Future std-only natives: seedable `Core.Random` / injected `Core.Time` (deterministic behind a
  seam)" line — the byte-identical spine *structurally forbids* ambient nondeterminism, so this seam is
  a forcing function, not a luxury. Strong fit confirmed. [Verified: parity-spec Group-2 line read above.]
- **O-doctest — defer** is right and resonates with the project's own "examples ship with features"
  discipline; it's the natural mechanization of that rule, but second-order behind the core runner.

No verdict needs flipping. The track's spine (M-Test = runner + assertions + determinism-seam +
table-driven + fakes + phpunit-bridge, now plus fixtures/selection/skip/fault-assert/ci-report/isolation;
property/doctest/snapshot/coverage/bench layered after; fuzz/mutation at v2; mock rejected) is sound and
PHP-familiar throughout.
