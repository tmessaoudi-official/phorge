# Soundness probe — value-returning fn returns on all paths (totality)

**Verdict:** Free functions and methods are **ENFORCED**. **Statement-body lambdas are a GAP (P0,
unsound).** A `: T` (value-carrying) lambda that falls off the end type-checks clean and silently
yields `unit`/`Empty` at runtime on **both** backends.

Binary: `/stack/projects/phorj/target/release/phg` (prebuilt release, not rebuilt).
Probe dir: `/tmp/.../scratchpad/audit` (nothing written into the repo).

---

## Part A — the documented totality rule IS enforced (free fns / methods)

### P1 — `if` with no `else`, falls off the false branch

```
package Main;
import Core.Console;
function pick(int a, int b): int {
    if (a > b) { return a; }
}
function main(): void { Console.println("{pick(3, 8)}"); }
```

```
$ phg check return-all-paths.phg
type error at 6:1: function does not return `int` on all paths
function pick(int a, int b): int {
^
  [E-MISSING-RETURN]
  hint: add a `return` (or diverge) on every path — e.g. an `if` without an `else` leaves the false branch falling through
exit=1
$ phg run return-all-paths.phg   # same E-MISSING-RETURN, exit=1
```

Enforced — rejected for the RIGHT reason.

### P2 — empty `{}` body

```
function f(): int { }
```
```
$ phg check p2-empty.phg
type error at 3:1: function does not return `int` on all paths  [E-MISSING-RETURN]
exit=1
```
Enforced.

### P3 — `while (true)` with a reachable `break` (does NOT guarantee a return)

```
function f(int n): int {
    while (true) { if (n > 0) { break; } }
}
```
```
$ phg check p3-whiletrue-break.phg
type error at 3:1: function does not return `int` on all paths  [E-MISSING-RETURN]
exit=1
```
Enforced — the analysis correctly treats a `while(true)` containing a `break` as non-diverging
(it can fall through). Good — this is the subtle case naive analyses miss.

### P4 — a **method** (not a free fn) falling off the end

```
class C {
    constructor(public int x) {}
    function pick(int a): int { if (a > this.x) { return a; } }
}
```
```
$ phg check p4-method.phg
type error at 5:5: function does not return `int` on all paths  [E-MISSING-RETURN]
exit=1
```
Enforced for methods too. (A second `unknown type `let`` error in this probe is my own probe-syntax
mistake — local bindings use `var`, not `let` — irrelevant to the verdict; the method body was still
caught at 5:5.)

### P7 — `if`/`else` where the `else` does NOT return (only side-effects)

```
function f(int n): int {
    if (n > 0) { return n; } else { Console.println("neg"); }
}
```
```
$ phg check p7-elsefall.phg
type error at 3:1: function does not return `int` on all paths  [E-MISSING-RETURN]
exit=1
```
Enforced.

**Conclusion (Part A):** the `E-MISSING-RETURN` totality rule is correctly enforced for free functions
and class methods, including the tricky `while(true)+break` non-divergence case.

---

## Part B — GAP: statement-body lambdas are NOT totality-checked (P0, unsound)

### P6b — a `: int` statement-body lambda that falls off the end type-checks clean

```
package Main;
import Core.Console;
function main(): void {
    var g = fn(int x): int { if (x > 0) { return x; } };   // no return on x <= 0
    Console.println("{g(1)}");
}
```
```
$ phg check p6b-lambda.phg
OK (type-checks clean)
exit=0
```
A free function with this exact body is rejected (Part A, P1). The same body inside a lambda passes.

### P6c — the fall-off path is reachable and produces a **wrong-typed value** on BOTH backends

```
package Main;
import Core.Console;
function main(): void {
    var g = fn(int x): int { if (x > 0) { return x; } };
    var r = g(-5);                 // takes the un-returned path
    Console.println("r = {r}");
}
```
```
$ phg check p6c-lambda-fall.phg
OK (type-checks clean)
check exit=0

$ phg run   p6c-lambda-fall.phg     ->  r = unit     exit=0
$ phg runvm p6c-lambda-fall.phg     ->  r = unit     exit=0
```

`g` is declared to return `int`, but on the `x <= 0` path it returns `unit` (the `Empty`/void value).
The checker accepts it; both the interpreter and the VM run it and bind a non-`int` value into an
`int`-typed local. This is a true type hole — a value of the wrong type flows through a statically
"`int`" slot — exactly the "declared-but-not-enforced soundness property" class this audit hunts. It is
*output-consistent* across backends (so the differential spine stays green and never caught it), which
makes it more insidious, not less.

---

## Root cause (verified by reading the checker)

`check_return_totality` (`src/checker/program.rs:542`) is the engine; `block_terminates`
(`program.rs:441`) is the per-path divergence analysis. The **only** call site is
`src/checker/program.rs:435`, inside the free-function/method checker:

```
435:            self.check_return_totality(&ret, &f.body, f.span);
```

The lambda checker `check_lambda` (`src/checker/expr.rs:855`) handles `LambdaBody::Block`
(`expr.rs:899–915`) by setting `self.cur_ret = declared` and walking the statements with `check_stmt`,
then returning the *declared* type as the lambda's result type — but it **never calls
`check_return_totality`** on the lambda body:

```
899:  LambdaBody::Block(stmts) => {
901:      match ret {
902:          Some(rt) => {
903:              let declared = self.resolve_type(rt);
904:              self.cur_ret = declared.clone();
905:              for s in stmts { self.check_stmt(s); }   // <-- no totality check
908:              declared                                   // <-- declared type returned regardless
```

So a value-carrying statement-body lambda is exempt from the return-on-all-paths guarantee that free
functions and methods enforce.

---

## Severity: **P0 (unsound)**

A statically-`int`-typed lambda can fall off the end and bind a void value into a typed slot, with no
diagnostic, on both backends. It lets provably-wrong code through and looks like it works — the worst
category for a "provably-correct upgrade of PHP".

## Concrete fix

In `src/checker/expr.rs`, `check_lambda`, the `LambdaBody::Block` / `Some(rt)` arm (≈expr.rs:902–908):
after walking the statements, call the existing engine on the lambda body before returning `declared`:

```rust
Some(rt) => {
    let declared = self.resolve_type(rt);
    self.cur_ret = declared.clone();
    for s in stmts { self.check_stmt(s); }
    self.check_return_totality(&declared, stmts, span);   // <-- ADD: same rule as fns/methods
    declared
}
```

This reuses `check_return_totality` (which already handles `void`/`Empty`/`never` exemptions and the
`never` divergence requirement), so a `: void`/`: Empty` statement-body lambda stays legal and only
value-carrying ones are required to return on all paths — matching free-function behavior exactly. No
new `Op`, no backend change, front-end-only (preserves the byte-identity spine).

Recommended companion: also route the lambda block through `check_body` (`program.rs:580`) instead of
the bare `for s in stmts { self.check_stmt(s); }` loop, so the `W-UNREACHABLE` dead-code lint applies
inside lambdas too (currently it does not — a secondary, non-unsound consistency gap). Add a totality
regression test alongside the existing `src/checker/tests/totality.rs` cases (a `fn(int x): int { if
(x>0){return x;} }` should now produce `E-MISSING-RETURN`), plus a guide/example update per the
"examples ship with features" rule.

## Notes on probe hygiene

- P5/P5b/P6 initial runs failed for **unrelated probe-syntax** reasons (match arms are expressions, not
  `{ ... }` statement blocks; local binding is `var` not `let`; zero-arg enum variants need `new A()`).
  These were syntax mistakes in my probes, NOT enforcement signals — I re-ran with corrected syntax.
  The Part A verdicts (P1–P4, P7) and the Part B GAP (P6b/P6c) all rest on clean, relevant output.
