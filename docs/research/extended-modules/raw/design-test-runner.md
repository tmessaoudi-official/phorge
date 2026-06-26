# Design — Testing: assertion library + `phg test` runner

**Stage 2 — DESIGN.** Status: design only, not implemented.
**Tier: A** (the assertion+runner core is fully deterministic and byte-identity-gateable; one
narrow Tier-B affordance — a *live* watch/parallel runner — is carved out and quarantined).
**New VM Op: NO. New `Value`: NO** (verified against the existing fault/native machinery below).

This file is the long-form design. The structured object returned to the caller is the summary.

---

## 0. The reframing, applied to a test runner

The task's correctness spine is the three-leg byte-identity (`run` ≡ `runvm` ≡ transpiled PHP under
`php -n`), broken only by (1) non-determinism, (2) backend asymmetry, (3) the TLS hard wall. A *test
runner* is the **purest possible Tier-A feature**: its entire job is to take deterministic Phorge
code, run it, and emit a deterministic report. There is no clock, no random, no socket, no scheduling
in the core. The only thing that could leak non-determinism into a runner is exactly the set of
things a *good* test runner must suppress anyway:

- **timing** ("ran in 0.3 ms") → non-deterministic → **omitted** (decision below)
- **memory** ("12 KB peak") → non-deterministic → **omitted**
- **discovery/iteration order** (filesystem/`HashMap` order) → non-deterministic → **sorted** (below)
- **process exit code** → deterministic function of pass/fail → **gated**

So the runner's determinism discipline is *identical* to the discipline that makes it a good runner.
That is the central insight: **a deterministic test runner and a byte-identical-across-backends test
runner are the same artifact.** This is why it is Tier A with high confidence.

---

## 1. What already exists (verified — do not rebuild)

I read the live code; these are the load-bearing facts the design reuses.

- **`assert`, `panic`, `todo`, `unreachable` are already intrinsics** with a single-sourced message.
  [Verified: `src/interpreter/call.rs:84-96` — `match name { "panic"|"todo"|"unreachable"|"assert" }`
  routes through `crate::chunk::FaultMsg::{Panic,Todo,Unreachable,Assert}.message()`, "single-sourced
  on `FaultMsg::message` so it is byte-identical to the VM's `Op::Fault`."] `assert(cond, msg?)`
  faults iff `cond` is false. **The runner's assertions build on this — they do not invent a new
  faulting mechanism.**
- **Faults are catchable** via the shipped `throws`/`try`/`catch`/`Signal::Throw` model.
  [Verified: `src/interpreter/mod.rs:28-41` — "matches it, or out of `main` (uncaught)"; `Signal::Throw`
  is catchable, a `Runtime` fault passes through; memory `[[error-model-slice2-progress]]` = M-faults
  Slice 2 CLOSED with `try`/`catch`/`finally`/`?` and 3 Ops Throw/PushHandler/PopHandler.] This is
  what `assertThrows` hangs on — **no new control-flow Op is needed.**
- **Native registry** keyed by `(module, name)`, one `src/native/<leaf>.rs` per module, each a
  `NativeFn { module, name, params, ret, eval: NativeEval::{Pure|HigherOrder|Reflective}, php, pure }`.
  [Verified: `src/native/mod.rs:40-93`.] `HigherOrder` natives can call a `Value::Closure` argument
  re-entrantly on either backend via the supplied `ClosureInvoker` [Verified: `mod.rs:64-69, 86-88`].
- **`ClassTables`** (sorted reflection tables) already threaded into both backends + emitted as a PHP
  static map by the transpiler. [Verified: `src/native/mod.rs:95-120`.] This is the seam the auto-mocker
  (Stage 4) reuses; for *this* slice (assertions + runner) it is only needed if we add mock support,
  which we defer.
- **Impure quarantine seam:** `tests/differential.rs:916` `uses_impure_native(src)` derives the impure
  module set **from the `NativeFn::pure` flag**, not a hardcode — "a future impure module is covered
  with no harness edit." [Verified.] The runner's *core* is pure; its *watch/live* affordance is the
  only `pure:false` part and is auto-dropped from the differential by this exact seam.
- **CLI dispatch** is a flat `match cmd` in `src/main.rs:68-69` over
  `run|runvm|check|parse|lex|transpile|lift|disasm|bench|build|vendor|serve|explain`, plus the
  `<file>`/`-`/`-e` source resolver `cli::resolve_source`. [Verified: `src/main.rs:68`,
  `src/cli/mod.rs:214`.] Adding `test` is one arm + one `cli::cmd_test`.
- **Project/loader model:** a project root = a dir with `phorge.toml`; `loader::load` assembles all
  `.phg` under the source root, mangles non-`main` defs, flat-merges. `tests/differential.rs` is
  project-aware (discovers roots, gates run≡runvm). [Verified: `src/cli/mod.rs:456` `run_program(&Unit)`,
  `tests/differential.rs:944-960` `collect_projects`.]
- **`package Main` fns become *global* PHP fns**, and a name colliding with a PHP builtin breaks the
  transpile leg. [Verified: CLAUDE.md M6 W1 gotcha — `serialize`→`serialize_response`.] **This
  constrains every name we emit** (see §6 naming).

---

## 2. Scope of THIS slice

In: (a) the **assertion library** (`Core.Test` natives + a small Phorge-level prelude type); (b) the
**`phg test` runner CLI**; (c) **deterministic reporting + exit codes**; (d) the **transpile target**
(plain-PHP if-checks + a hand-written reporter, NOT PHPUnit); (e) **how it composes with
`differential.rs`**.

Out (later sub-slices, explicitly): seeded Faker (Stage-3 of the parent research), the auto-mocker
(Stage-4, reuses `ClassTables`), parameterized/table tests, snapshot tests, a watch mode (the lone
Tier-B affordance — sketched in §9 but not built here).

---

## 3. The assertion library

### 3.1 Surface (Phorge syntax)

Assertions live in a new stdlib module **`Core.Test`** (PascalCase, consistent with
`Core.Console`/`Core.Math`). Each is a native: deterministic, faulting-on-failure, returning `Unit`
on success. A failing assertion **faults** through the existing `FaultMsg`/`Signal` machinery — so it
is byte-identical across backends by construction, and (importantly) it is *catchable* by the runner
(§4) exactly like any other fault.

```phorge
package Main;
import Core.Test;

function test_arithmetic() -> Empty {
  Test.assertEquals(4, 2 + 2);            // (expected, actual)
  Test.assertTrue(1 < 2);
  Test.assertFalse(2 < 1);
  Test.assertNull(noneOf());              // value must be null (optional)
  Test.assertNotNull(Some(3));
  Test.assertContains("ell", "hello");    // string-in-string / item-in-list (overloaded by type)
}

function test_faults() -> Empty {
  // assertThrows takes a zero-arg closure; passes iff the closure faults.
  Test.assertThrows(fn() => panic("boom"));
  // optional message-substring form: passes iff it faults AND the rendered fault contains the needle
  Test.assertThrowsMessage("force-unwrap", fn() => (noneOf())!);
}
```

Initial native set (each `(Core.Test, name)`, all `pure: true`):

| native | sig | faults when |
|---|---|---|
| `assertTrue` | `(bool) -> Empty` | arg is false |
| `assertFalse` | `(bool) -> Empty` | arg is true |
| `assertEquals<T>` | `(T, T) -> Empty` | `expected != actual` (structural `eq_val`) |
| `assertNotEquals<T>` | `(T, T) -> Empty` | equal |
| `assertNull<T>` | `(T?) -> Empty` | non-null |
| `assertNotNull<T>` | `(T?) -> Empty` | null |
| `assertContains` | `(string, string) -> Empty` (+ list overload, see note) | needle not in haystack |
| `assertThrows` | `((); Empty) -> Empty` i.e. `(() -> Empty) -> Empty` | closure does **not** fault |
| `assertThrowsMessage` | `(string, () -> Empty) -> Empty` | closure doesn't fault, or fault msg lacks the needle |
| `fail` | `(string) -> never` | always (`-> never`, totality-friendly) |

Notes:
- **Generic assertions** (`assertEquals<T>`, `assertNull<T>`) reuse the **S7b-1 generic-typed
  native-call path** [Verified: CLAUDE.md S7b-1 — `check_native_call` routes through
  `check_generic_call` when a native sig has a `Ty::Param`; the type var never reaches a backend].
  Equality is the **single-sourced `eq_val` value kernel** [Verified: memory
  `[[value-kernels-single-sourced]]`], so `run`/`runvm` compare identically and there is no second
  comparison surface to drift.
- `assertContains` over a *list* needs the same type-var trick or an overload; to avoid leaning on
  overloading (M-RT method-overloading exists but per-CLAUDE.md it lowers to one dispatching PHP
  method) the **string form ships first**; the `List<T>` form rides the generic-native path in a
  follow-up. Conservative: ship `assertContains(string,string)` + `assertListContains<T>(List<T>, T)`
  if a list form is wanted now — two names, zero overloading risk.
- `assertThrows` is a **`HigherOrder` native** [Verified mechanism: `NativeEval::HigherOrder`,
  `src/native/mod.rs:86`]: it receives the closure + a `ClosureInvoker`, calls the closure, and
  inspects the `Result<Value, String>`:
  - `Ok(_)` → the assertion itself faults ("expected a fault, none occurred").
  - `Err(body)` → success (the closure faulted as required). For `assertThrowsMessage`, success iff
    `body.contains(needle)`.
  This works **byte-identically on both Rust backends** because the VM's `call_closure_value` +
  `run_until` drive the *shared* `exec_op`, so a closure's fault is the same `String` on both
  [Verified: CLAUDE.md S7b-3 — "a closure's result AND any fault are byte-identical to the
  interpreter"]. The fault *body* (not raw text) is what `differential.rs` compares anyway via
  `FaultKind` [Verified: `tests/differential.rs:64,102`], so matching on a substring of the body is
  consistent with the existing parity contract.

### 3.2 Failure message format (must be deterministic)

A failed assertion's `FaultMsg` carries a fixed, value-only string — no addresses, no timing:

```
assertion failed: assertEquals — expected 4, got 5
```

The expected/actual rendering reuses the existing **`Value` display kernel** (the same one
`Console.println` uses), so the bytes are identical on `run`/`runvm`, and the PHP leg's `echo`/string
cast of the same values matches (this is the same constraint every other native already lives under —
see the `sqrt(2.0)` irrational-float KNOWN_ISSUE: tests must compare exactly-representable values).
**No float-equality assertion in v1** beyond exact equality; `assertApproxEquals(a, b, epsilon)` is a
follow-up that sidesteps the 14-digit PHP `echo` divergence by comparing `abs(a-b) < eps` (a bool) and
never echoing the floats.

### 3.3 PHP transpile target for assertions

Each assertion native supplies a `php: fn(&[String]) -> String` mapping, like every other native
[Verified: `src/native/mod.rs:71`, `tests.rs:php_emission_is_echo_with_newline`]. They erase to a
**gated helper** `__phorge_assert_*` (the established `uses_* + __phorge_*` pattern [Verified:
`src/transpile/call.rs:112-156`]) so the emitted PHP is small and the helper is only included when used:

```php
function __phorge_assert_eq($exp, $act) {
    if ($exp !== $act) {
        throw new \RuntimeException("assertion failed: assertEquals — expected "
            . __phorge_str($exp) . ", got " . __phorge_str($act));
    }
}
```

- `assertThrows(closure)` → `__phorge_assert_throws($closure)` which wraps `try { $closure(); }
  catch (\Throwable $e) { return; } throw new \RuntimeException("assertion failed: assertThrows — no
  fault");`. PHP `\Throwable` is core (no extension), works under `php -n`. The closure is a PHP
  `\Closure` (the existing lambda erasure [Verified: CLAUDE.md S3 — arrow fn / `function(){}use()`]).
- `===` (identity) vs `==` (loose): use **`===`** to match Phorge's structural `eq_val` for scalars;
  composite values (`List`/`Map`/instances) erase to PHP arrays/objects where `===` is too strict — so
  `assertEquals` over composites must emit a recursive `__phorge_eq($a,$b)` helper that mirrors
  `eq_val` (it already has to exist conceptually for `==` on composites in the transpiler; reuse it).
  [Inferred: the transpiler must already handle `==` over composites somewhere — confirm and reuse
  rather than add a parallel comparator. Open question Q3.]

**Byte-identity argument for the assertion library:** every assertion is a pure function of its
argument *values*; success returns `Unit` (no output); failure produces a fixed value-derived string
via the single-sourced `FaultMsg`/`eq_val`/`Value`-display kernels shared by both Rust backends, and
the PHP helper recomputes the same comparison and the same message from the same values. No clock, no
order, no identity. **Tier A, high confidence.**

---

## 4. The `phg test` runner

### 4.1 Discovery convention (deterministic)

Two layers, both deterministic:

1. **Function convention:** within the loaded program, every **free function whose name starts with
   `test_`** and whose signature is `() -> Empty` is a test case. [Chosen over an attribute/annotation
   because Phorge has no attribute syntax yet, and a name convention is what Go (`func TestXxx`) and
   pytest (`test_*`) use — familiarity-first per the Phorge philosophy memory.]
2. **Project layer:** in project mode (`phorge.toml` present), the runner loads the project via the
   **existing `loader::load`** [Verified: `src/cli/mod.rs:456`] and collects `test_*` functions from
   **all merged packages**. A dedicated `tests/` source subtree is *not* required in v1 — tests live
   beside code (Go model), or in their own package; the loader already flat-merges. A future
   `[test] paths = [...]` manifest key can scope discovery.

**Ordering — the determinism keystone:** discovered tests are **sorted by `(package, function name)`
lexicographically** before execution and reporting. This makes the run order a pure function of the
program text, independent of `HashMap`/filesystem iteration. [This mirrors `Core.Env.all`'s "sorted by
key (Q4)" precedent and `ClassTables`' sorted-list invariant — Verified both.]

### 4.2 Execution model (no new control flow)

The runner is itself **a generated `main()`** — conceptually the runner *lowers to* a Phorge program
that calls each `test_*` in sorted order, each wrapped in a `try`/`catch` (the shipped catchable-fault
model). This is the elegant move: **the runner reuses the language's own fault-catching to isolate
test failures**, so a faulting test does not abort the suite; it is recorded as a failure and the
next test runs.

Two viable implementations — recommend **(B)**:

- **(A) Native-driven:** a `Core.Test.run(tests: List<...>)` higher-order native that takes the test
  closures and drives them. Rejected: requires materializing test fns as first-class values + a list,
  and first-class cross-package fn *values* are a KNOWN_ISSUE deferral [Verified: CLAUDE.md S3
  deferrals].
- **(B) Lowering / synthesized entry (recommended):** `phg test` parses+checks+loads the program, then
  the CLI **synthesizes a runner `main`** that, for each sorted `test_*`, emits the equivalent of:
  ```phorge
  Console.print("test PkgName.test_arithmetic ... ");
  try { test_arithmetic(); Console.println("ok"); /* tally pass */ }
  catch (e) { Console.println("FAIL"); Console.println("  " + e.message); /* tally fail */ }
  ```
  This is **front-end-only** (build the runner AST, then run it on the chosen backend / transpile it).
  No new Op, no native even strictly required for the driver (the assertions are natives; the *driver*
  is synthesized Phorge). It composes perfectly with all three backends because it *is* ordinary
  Phorge.

  Subtlety: `catch (e)` needs the fault to be catchable. A *`throws`/`Signal::Throw`* (assertion) is
  catchable; a hard *`Runtime`* fault (index-OOB, `panic`) is currently **not** catchable
  [Verified: `src/interpreter/mod.rs:29` — "only a `Throw` is catchable; a `Runtime` fault … passes
  through"]. **Decision (Q1 — needs developer ratification):** for the runner to isolate *any* failing
  test (not just assertion failures), either (i) assertions throw a catchable `Test failure` exception
  (so `assert*` use `throw`, not the uncatchable `FaultMsg::Assert`), and a hard panic in a test
  legitimately aborts the suite (pytest-ish: an unexpected crash is fatal); or (ii) the runner gets a
  privileged catch-all. **Recommend (i):** assertions throw a catchable typed exception; a genuine
  `panic`/OOB aborting the run is *correct* (it's a bug in the test, surfaced loudly with the existing
  byte-identical stack trace [Verified: `[[error-handling-and-whole-project-validation]]`]). This keeps
  the fault taxonomy honest and needs **no new mechanism** — assertions just use `throw` instead of the
  intrinsic `assert`'s `FaultMsg`.

### 4.3 Deterministic report format

```
running 4 tests
test Main.test_arithmetic ... ok
test Main.test_contains ... ok
test Main.test_faults ... ok
test Math.test_sqrt ... FAIL
  assertion failed: assertEquals — expected 2, got 1.414…

result: FAILED. 3 passed; 1 failed
```

Hard rules (all enforced to keep it byte-identical):
- **No timing** ("0.03s"), **no memory**, **no PID**, **no absolute paths** in the default report.
- **Sorted test order** (§4.1).
- The summary counts are deterministic functions of pass/fail.
- A `--format=json` mode emits a sorted-key JSON object (`{"passed":3,"failed":1,"cases":[...]}`)
  reusing `Core.Json` stringify for stable key order [Verified: `Core.Json` shipped, memory
  `[[core-json-and-injected-types]]`] — handy for CI/editor integration and itself byte-identical.

### 4.4 Exit codes

- all pass → **0**
- any fail → **1**
- usage error (no tests found, bad args, compile error) → **2**

These match Go/cargo/pytest convention and are deterministic. The runner sets the process exit code in
the CLI (`src/main.rs`), not inside the program (the synthesized `main` returns a pass/fail tally; the
CLI maps it to the exit code).

### 4.5 New CLI surface

One new subcommand arm in `src/main.rs:68` `match cmd` and one `cli::cmd_test`:

```
phg test [<file>|<dir>|.]            # run tests (project mode if a phorge.toml is found by walk-up)
         [--backend=run|runvm]        # default: run (interpreter). runvm available for parity spot-check.
         [--filter=<substr>]          # only tests whose qualified name contains <substr>
         [--format=text|json]         # default: text
         [--list]                     # print discovered test names (sorted), run nothing
```

- Default backend is the interpreter (`run`); `--backend=runvm` lets a developer confirm a test suite
  is itself backend-identical. (CI can run both and diff — see §7.)
- `--filter` is deterministic (substring over the sorted qualified names).
- Reuses `cli::resolve_source` for the `<file>`/`.`/dir argument and `loader::load` for project mode —
  **no new source-resolution code.** [Verified seams: `src/cli/mod.rs:214,456`.]
- Per-command `--help` with a worked example, matching the M3 S0 `--help` convention [Verified:
  CLAUDE.md M3 S0].

### 4.6 Transpile target for the runner (`phg test --transpile` / `phg transpile`)

The synthesized runner `main` is **ordinary Phorge**, so it transpiles through the *existing*
transpiler with **zero new transpiler arms** — the `try`/`catch`, `Console.print`, and the assertion
helpers all already have emit paths. The emitted PHP is a single file: the gated `__phorge_assert_*`
helpers + the user's (namespaced) functions + a synthesized `\Main\__phorge_test_main()` that runs the
sorted cases under `try/catch (\Throwable)` and `echo`es the same report, then `exit(0|1)`.

This is the "NOT PHPUnit" requirement satisfied **for free**: PHPUnit is a Composer package, absent
under `php -n` [Verified: task constraint + memory `[[transpile-no-ini-extensions]]`]; our reporter is
hand-written `echo`/`if` PHP that needs only core. The PHP output runs identically to both Rust
backends → the runner is part of the byte-identity spine.

---

## 5. How it composes with `differential.rs`

This is the elegant payoff and the strongest Tier-A argument.

- **A `phg test` run is just a Phorge program** (the synthesized runner). So an example test suite can
  be dropped under `examples/` (e.g. `examples/test/calculator/` as a project, or a single
  `examples/guide/testing.phg`) and the **existing glob/project harness gates it byte-identically with
  no harness edit** [Verified: `tests/differential.rs:986` globs `examples/**/*.phg`; project-aware
  discovery at `:944`]. The runner's *own* output (the "running N tests … result: FAILED" report) is
  what gets compared across `run`/`runvm`/PHP — and because every rule in §4.3 is deterministic, it is
  byte-identical.
- **Subtlety — a failing test in an example:** an `examples/` program must produce identical *Ok*
  output on all backends [Verified: `tests/differential.rs:1000` "Every example must *run* (produce
  identical Ok output)"]. A test-suite example that *intentionally* contains a failing case would exit
  non-zero. So the *gated* example test suites must be **all-green** (their report ends `result: ok`);
  the *failing-path* behavior is demonstrated in a README walkthrough + covered by a Rust integration
  test (`tests/test_runner.rs`) that asserts the exact "FAILED" report bytes and exit code 1. This is
  the same split the `examples/process/` walkthroughs use (Verified pattern) and the same the
  `examples/errors/` faults use [Verified: CLAUDE.md "Faults can't be a runnable example … capture
  them in a README"].
- **The assertion library + runner do NOT need quarantining** — they are `pure: true`. `uses_impure_native`
  returns false for them, so they stay *inside* the differential. Good: we *want* them gated.
- **`tests/test_runner.rs` (new):** integration tests that (a) run the synthesized runner over a green
  fixture suite and assert the byte report on `run` and `runvm`; (b) run it over a red fixture and
  assert the FAIL report + exit 1; (c) under `PHORGE_REQUIRE_PHP=1`, transpile the same fixtures and
  assert the PHP report matches. This *is* the Coverage evidence per Rule 6/7.

---

## 6. Naming constraints (a real trap, verified)

`package Main` user functions become **global** PHP functions [Verified: CLAUDE.md M6 W1 gotcha]. So:
- The synthesized runner entry must be named to avoid PHP-builtin collision — use a `__phorge_`-prefixed
  internal name (`__phorge_test_main`) which can never collide.
- A user's `test_foo` becomes a global PHP `test_foo()` — `test_` is not a PHP reserved prefix, safe.
- **But** a user must not name a test fn `assert`, `print`, etc. — already guarded by the existing
  `E-RESERVED-NAME` / PHP-reserved-word work [Verified: memory `[[contextual-var-and-reserved-names]]`].
- `Core.Test` leaf `Test` must not be shadowable by a local named `test` — `E-SHADOW-IMPORT` already
  bites a lowercase user leaf [Verified: CLAUDE.md Wave 1 guard]; `Test` is PascalCase so the import
  qualifier is `Test`, and a local `test` is lowercase → safe, but document it (same as the
  `Core.Text`→don't-name-a-local-`text` gotcha).

---

## 7. CI / parity integration

- The project's CI already enforces the oracle + cross-build [Verified: memory `[[ga-roadmap-spec-m7-next]]`
  — `.github/workflows/ci.yml`]. Add a step `phg test examples/test/... --backend=run` and
  `--backend=runvm` and diff the two reports (must be identical) — a cheap second parity net beyond
  `differential.rs`.
- The runner's own correctness is gated by `tests/test_runner.rs` (Rust integration) + the
  byte-identity of any green example suite under `differential.rs`.

---

## 8. New VM Op / Value — NONE (verified)

- Assertions: `CallNative` (existing dispatch). `assertThrows` is `HigherOrder` (existing). [Verified:
  `src/native/mod.rs:86`, `Op::CallNative` is how all natives dispatch — memory
  `[[higher-order-natives-reentrant-vm]]`.]
- Runner driver: synthesized ordinary Phorge AST (`try`/`catch`/`Console.print`/calls) — all existing
  Ops. The `try`/`catch` Ops (Throw/PushHandler/PopHandler) already shipped [Verified: memory
  `[[error-model-slice2-progress]]`].
- Exit code: set in the CLI, not the VM.

So the whole slice is **front-end + native + CLI**, the cheapest possible shape, and the byte-identity
spine is safe by construction.

---

## 9. The lone Tier-B affordance (NOT built here — sketched for completeness)

A **watch mode** (`phg test --watch`) re-runs on file change. It reads the clock / FS events / runs in
a loop → non-deterministic, no `php -n` target. If ever built, it is `pure:false`-adjacent: it is a
*CLI loop around* the deterministic runner, never transpiled, never gated, fixture-tested only for the
"detect-change→rerun-once" mechanic in a dedicated `tests/test_watch.rs` (outside `differential.rs`),
exactly like `Core.Process`/`serve.rs`. **Recommend deferring** — `entr`/editor-watch already covers it
and it adds the project's only non-deterministic test-tooling surface.

Likewise **parallel test execution** is the §parent-research data-parallelism case: deterministic iff
the merge is sorted-order (which it already is). Ships *sequentially* now = trivially byte-identical;
physical parallelism is a later invisible optimization (tests are pure functions, so safe), gated only
by output-order preservation. Not in v1.

---

## 10. Effort & sequencing

- **S-a (assertions):** add `src/native/test.rs` (~8 natives + `php` mappings + gated helpers in
  `src/transpile/call.rs`), 1 day. Mechanical — every native is additive (the call path is already
  generic, multi-arg, typed, value-returning, higher-order-capable).
- **S-b (runner):** `cli::cmd_test` + synthesized-runner builder + `main.rs` arm + exit codes, 1–2 days.
- **S-c (tests + example):** `tests/test_runner.rs` + a green `examples/test/` suite + a README
  walkthrough for the FAIL path, 0.5 day.
- **Total: ~Medium** (a focused multi-file feature, no backend/Op surgery).

---

## 11. Open questions for the developer

- **Q1 (must answer first):** Should `assert*` use the **catchable `throw`** path (so the runner
  isolates assertion failures and a hard `panic` legitimately aborts the suite — recommended), or
  should the runner get a privileged catch-all over *all* faults? This decides the assertion
  mechanism (catchable typed exception vs the intrinsic `FaultMsg::Assert`).
- **Q2:** Test discovery convention — `test_*` free-function names (recommended, Go/pytest-familiar),
  or an explicit `@test`/attribute (needs attribute syntax Phorge lacks), or a reserved
  `package Test;` (Go-test-file analogue)? I recommend `test_*` now, `[test]` manifest scoping later.
- **Q3:** Does the transpiler already have a recursive composite-equality emitter (for `==` over
  `List`/`Map`/instances) that `assertEquals` can reuse, or must `__phorge_eq` be written fresh? (Reuse
  if it exists — single comparison surface.)
- **Q4:** Default runner backend — interpreter `run` (recommended, fastest startup) — and do we want CI
  to *always* run both backends and diff, or trust `differential.rs`?
- **Q5:** `assertContains` over lists — ship `assertListContains<T>` as a separate name now (no
  overloading risk), or wait for the generic-native list form?
- **Q6:** Scope of v1 assertions — is the 10-native set in §3.1 the right starting surface, or add
  `assertApproxEquals`/`assertEmpty`/`assertCount` immediately?

---

## 12. Determinism risks (named)

1. **Discovery order** — mitigated by mandatory `(package, fn-name)` sort.
2. **Float rendering in `assertEquals`** — mitigated by exact-equality-only v1 + a future
   `assertApproxEquals` that never echoes floats (the `sqrt(2.0)` KNOWN_ISSUE class).
3. **Fault message text drift between backends** — mitigated by single-sourced `FaultMsg`/`Value`-display
   kernels and the `FaultKind`-body comparison already used by `differential.rs`.
4. **Composite equality (`===` too strict / `==` too loose in PHP)** — mitigated by a single
   `__phorge_eq` mirroring `eq_val` (Q3).
5. **Timing/memory leaking into the report** — mitigated by hard exclusion (§4.3).
6. **Process exit code vs program output** — exit code set in the CLI as a pure function of the tally;
   the *program* output (the report) is what's gated.
