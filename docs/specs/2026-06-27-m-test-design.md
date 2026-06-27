# M-Test — `phg test` runner + `Core.Test` assertions (design)

> Status: **COMPLETE** (2026-06-27, T1–T5) — all recommended defaults adopted (`test "name" {}` items,
> catchable-fault assertions, `tests/`-discovery, interpreter runner). Shipped: contextual `test` item +
> `E-TEST-OUTSIDE-TESTS`; `Core.Test` (`assert`/`assertTrue`/`assertFalse`/`assertEquals`/
> `assertNotEquals`/`assertNull`/`assertNotNull`/`assertFaults`); the `phg test [path…]` runner; and the
> `selftest/` showcase. Deferred follow-ups (fixtures, parameterized, `--vm`, TAP/JUnit, PHPUnit bridge)
> tracked in KNOWN_ISSUES. Goal achieved: a first-class testing story so Phorge can dogfood itself.

## Goals
- Write tests *in Phorge*, run them with one command, get a clear pass/fail report + non-zero exit on
  failure (CI-usable).
- Assertions are ordinary Phorge calls (`Core.Test`), so a test file is a normal program.
- Cover the fault surface (`assertFaults`) — Phorge's error model is a first-class feature to test.

## Key decisions (recommendation + rationale; **confirm before building**)

### D1 — how a test is declared  → **recommend: a `test "name" { … }` top-level item**
```phorge
package Main;
import Core.Test;

test "addition wraps at the boundary" {
    Test.assertEquals(2 + 2, 4);
    Test.assertTrue(1 < 2);
}
```
- `test` is a **contextual keyword** (only special at item position — like `package`/`type`), so it
  stays usable as an identifier elsewhere. A new `Item::Test { name: String, body: Vec<Stmt> }`.
- Rejected alternatives: (a) functions named `test*` (Go/PHPUnit convention — implicit, less
  readable, collides with a real function named `testX`); (b) a `@test` annotation (Phorge has no
  annotation system — would be a bigger prerequisite).
- A `test` block body is checked like a `-> void` function body (no return value; may call asserts).
  It captures no `this`. Lives only in test files (see D3); an `Item::Test` in a non-test build is an
  `E-TEST-OUTSIDE-TESTS` error so production code can't smuggle test blocks.

### D2 — the assertion API  → **recommend: `Core.Test`, failure = a catchable fault**
A failing assertion raises a distinguished **fault** (`FaultKind`-tagged "assertion failed: …"); the
runner catches it per-`test`, records a failure, and continues to the next test. This reuses the
existing fault machinery (no new control-flow concept) and gives a precise message + the Slice-1 stack
trace for free.

Initial surface (subject-first, charter-compliant):
- `Test.assert(bool, string message)` — base; fault carries `message`.
- `Test.assertTrue(bool)` / `Test.assertFalse(bool)`
- `Test.assertEquals(T a, T b)` — value equality via the shared `eq` kernel; the message renders both
  sides (`expected 4, got 5`). Generic `T` (the S7 native-generic path).
- `Test.assertNotEquals(T, T)`
- `Test.assertNull(T?)` / `Test.assertNotNull(T?)`
- `Test.assertFaults(() -> T)` — runs the closure; **passes iff it faults** (a HigherOrder native +
  the re-entrant VM `call_closure_value`, like `List.map`; catches the fault, returns unit). The dual
  of "must not fault" is just calling the code directly (an uncaught fault fails the test).
- (Deferred: `assertFaultsWith(msg)`, `assertContains`, fixtures/setup, parameterized tests.)

Open sub-decision **D2a**: are `Core.Test` natives `pure`? They are deterministic, but they only make
sense inside a `test` block run by `phg test`, never in a byte-identity example. **Recommend `pure:
true`** (deterministic; the runner, not the oracle, exercises them) but **exclude `test` files from the
example differential glob** (they live under `tests/`, not `examples/`).

### D3 — discovery + the runner  → **recommend: `phg test [path]` over `*.phg` under `tests/`**
- `phg test` with no arg: discover every `*.phg` under a `tests/` directory (project-aware: walk up to
  `phorge.toml`; loose mode: `./tests/`). `phg test <file|dir>`: run exactly that.
- Each test file is loaded through the **normal loader/checker** (so tests get packages, imports,
  cross-package types — real programs). Then every `Item::Test` is executed.
- **Backend**: run each test on the **interpreter** (`run`) for speed and clear stack traces.
  (Optional `--vm` to also run on the bytecode VM — a parity check for free. Deferred to a follow-up.)
- **Output**: a concise report — per failing test: name, message, file:line; a summary line
  `N passed, M failed, K tests in F files`. Exit `0` iff all pass, else `1`. `--format=tap` later.
- A test that faults *outside* an assertion (a real bug in the code under test) is a failure with its
  stack trace — not a runner crash.

### D4 — PHPUnit bridge → **defer.** A `--emit-phpunit` that transpiles `test` blocks to PHPUnit
methods is possible (the transpiler already emits PHP), but it is not needed for dogfooding and adds
surface. Revisit post-GA.

## Slices
- **T1 — `test` item**: lexer contextual `test`, parser `Item::Test`, checker (`check_body`,
  `E-TEST-OUTSIDE-TESTS`), interpreter/VM execute a test block as a void body. No assertions yet.
- **T2 — `Core.Test` core asserts**: `assert`/`assertTrue`/`assertFalse`/`assertEquals`/`assertNull`
  (+ Not variants) as natives; failure = a tagged fault. Unit + a `tests/`-dir example.
- **T3 — the `phg test` runner**: discovery, per-test execute + catch, report, exit code. `cli::cmd_test`.
- **T4 — `assertFaults`** (HigherOrder native + re-entrant VM) — closes the error-surface coverage.
- **T5 — docs**: README + `phg test --help`; a self-hosted `tests/` example suite.

## Non-goals (this milestone)
Fixtures/setup-teardown, parameterized/table tests, mocking, coverage, `--vm` cross-run, TAP/JUnit
output, the PHPUnit bridge. Each is an additive follow-up once the core runner exists.

## Open questions for the developer (confirm before T1)
1. **D1** test-declaration syntax: `test "name" { }` block? (vs `test*` functions / annotation)
2. **D2** assertion-failure model: catchable fault (recommended) vs a runner-injected callback?
3. **D3** discovery root: `tests/**/*.phg` (recommended) vs a manifest list vs `*_test.phg` anywhere?
4. Run on interpreter only for v1, or interpreter **and** VM (parity bonus, more work)?
