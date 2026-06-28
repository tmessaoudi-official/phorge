# `phg test` — self-hosted test suite

A small, runnable showcase of Phorj's built-in test runner (M-Test). These files are exercised by
CI (`tests/mtest.rs`), so they are guaranteed to stay green.

## Running

```console
$ phg test selftest/
selftest/arithmetic.phg :: addition ... ok
selftest/arithmetic.phg :: doubling ... ok
selftest/arithmetic.phg :: booleans and null ... ok
selftest/faults.phg :: indexing past the end faults ... ok
selftest/faults.phg :: a missing map key faults ... ok
selftest/faults.phg :: code that should NOT fault ... ok

6 passed, 0 failed, 6 tests in 2 files
```

Exit code is `0` iff every test passes, else `1` — so `phg test` drops straight into CI.

With no path, `phg test` discovers every `*.phg` under the project's `tests/` directory (the project
root is the nearest ancestor holding a `phorj.toml`, else the current directory). You can also point
it at a single file or a directory: `phg test selftest/arithmetic.phg`, `phg test selftest/`.

## Writing tests

A test file is a **normal Phorj program** — it can declare functions, classes, and imports
alongside its `test` blocks. `test` is a contextual keyword (special only at the start of a top-level
item, before a string name), so it stays usable as an ordinary identifier everywhere else.

```phorj
package Main;
import Core.Test;

function add(int a, int b): int {
    return a + b;
}

test "addition" {
    Test.assertEquals(add(2, 3), 5);
    Test.assertTrue(1 < 2);
}
```

A `test` block is checked like a `-> void` body (no `this`, no return value). It is valid **only**
in a file run by `phg test`; a `test` block in a normal build (`run`/`runvm`/`transpile`) is the
error `E-TEST-OUTSIDE-TESTS` (`phg explain E-TEST-OUTSIDE-TESTS`).

## The `Core.Test` assertions

| Assertion | Passes when |
|---|---|
| `Test.assert(bool, string)` | the condition is true (the message is shown on failure) |
| `Test.assertTrue(bool)` / `Test.assertFalse(bool)` | the condition is true / false |
| `Test.assertEquals(T, T)` / `Test.assertNotEquals(T, T)` | the two values are (not) equal (shared `==` kernel; both sides must be the same type) |
| `Test.assertNull(T)` / `Test.assertNotNull(T)` | the value is (not) `null` |
| `Test.assertFaults(() -> T)` | the closure faults (the way to test the error surface) |

A failing assertion raises a fault with a clear message; the runner catches it per-test, records the
failure with its line and stack trace, and continues to the next test. A test that faults *outside*
an assertion (a real bug in the code under test) is a failure too — not a runner crash.

```phorj
test "indexing past the end faults" {
    var xs = [10, 20, 30];
    Test.assertFaults(fn() => xs[5]);   // out-of-range read faults → the test passes
}
```

## Not yet (additive follow-ups)

Fixtures / setup-teardown, parameterized tests, `--vm` cross-run (run each test on the bytecode VM
too, for a free parity check), TAP/JUnit output, and a PHPUnit-emitting bridge — each lands on top
of this core runner without changing it.
