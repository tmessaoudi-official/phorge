# Probe: cannot reassign a `const` local / class constant

## Rule under test
Immutable bindings must not be reassignable. In Phorj this splits into two surfaces:
1. **Locals are immutable by default** (there is *no* `const` local keyword — immutability IS the
   default; only a `mutable`-declared local may be reassigned).
2. **Class `const` members** are compile-time constants and must never be reassigned.

## Verdict: ENFORCED (not a gap)

Both surfaces are rejected at `check` time (and therefore also at `run`, which front-runs the
checker) with dedicated, correct diagnostics. The `mutable` control program reassigns successfully,
proving the rejection is targeted at immutability — not a blanket reassignment ban or an unrelated
syntax error. Compound-assignment (`+=`) and `var`-inferred bypass vectors are all closed.

Enforcement site: `src/checker/assign.rs` (emits `E-ASSIGN-IMMUTABLE` and `E-CONST-REASSIGN`);
regression tests in `src/checker/tests/mutation.rs` and `src/checker/tests/constants.rs`.

---

## Evidence

### Probe 1 — reassign an immutable (default) local → REJECTED (right reason)
Program (`const-reassign-local.phg`):
```
package Main;
import Core.Console;

function main(): void {
    int x = 1;
    x = 2;
    Console.println("{x}");
}
```
Command + output (`check` and `run` identical):
```
$ phg check const-reassign-local.phg
type error at 6:5: `x` is immutable and cannot be reassigned
    x = 2;
    ^
  [E-ASSIGN-IMMUTABLE]
  hint: declare it `mutable` (e.g. `mutable int x = …;`)
exit=1

$ phg run const-reassign-local.phg
type error at 6:5: `x` is immutable and cannot be reassigned
    x = 2;
    ^
  [E-ASSIGN-IMMUTABLE]
  hint: declare it `mutable` (e.g. `mutable int x = …;`)
exit=1
```

### Probe 2 — reassign a class constant → REJECTED (right reason)
Program (`const-reassign-classconst.phg`):
```
package Main;
import Core.Console;

class Limits {
    const int MAX = 100;
}

function main(): void {
    Limits.MAX = 200;
    Console.println("{Limits.MAX}");
}
```
Command + output (`check` and `run` identical):
```
$ phg check const-reassign-classconst.phg
type error at 9:5: `MAX` is a constant of `Limits` and cannot be reassigned
    Limits.MAX = 200;
    ^
  [E-CONST-REASSIGN]
  hint: constants are fixed at declaration; use a `static mutable` field for class-level mutable state
exit=1

$ phg run const-reassign-classconst.phg
type error at 9:5: `MAX` is a constant of `Limits` and cannot be reassigned
    Limits.MAX = 200;
    ^
  [E-CONST-REASSIGN]
  hint: constants are fixed at declaration; use a `static mutable` field for class-level mutable state
exit=1
```

### Control — reassign a `mutable` local → ACCEPTED (proves the rejection is targeted)
Program (`mutable-ok.phg`):
```
package Main;
import Core.Console;

function main(): void {
    mutable int x = 1;
    x = 2;
    Console.println("{x}");
}
```
```
$ phg run mutable-ok.phg
2
exit=0
```

### Bypass vectors — all closed
Compound assign on immutable local:
```
$ phg check const-compound.phg      # x += 5; on `int x = 1;`
type error at 6:5: `x` is immutable and cannot be reassigned   [E-ASSIGN-IMMUTABLE]   exit=1
```
Compound assign on class constant:
```
$ phg check const-classconst-compound.phg   # Limits.MAX += 1;
type error at 7:5: `MAX` is a constant of `Limits` and cannot be reassigned   [E-CONST-REASSIGN]   exit=1
```
`var`-inferred immutable local reassign:
```
$ phg check const-var-infer.phg     # var x = 1; x = 2;
type error at 6:5: `x` is immutable and cannot be reassigned   [E-ASSIGN-IMMUTABLE]   exit=1
```

---

## Severity
none — the rule is fully enforced across direct assignment, compound assignment, and `var`-inferred
declarations, for both local bindings and class constants.

## Fix recommendation
No fix required. (For reference, enforcement already lives in `src/checker/assign.rs`.)
