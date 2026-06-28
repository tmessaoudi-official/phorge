# Soundness probe — declared `throws` enforcement

**Rule under test:** a function declaring `throws E` may raise `E`; a *call* to such a function from a
context that does not handle `E` (no enclosing `try`/`catch`, no `?`-propagation into a declared
`throws`) MUST be rejected at compile time (`E-CALL-UNHANDLED`). This is the "fix to PHP's unenforced
`@throws`" — the headline promise of M-faults Slice 2.

**Verdict: ENFORCED for free-function calls; GAP (P0, unsound) for METHOD calls.**

A throwing **method** call is not checked at all — it type-checks clean and the exception escapes the
declared/handled surface, surfacing only as an uncaught runtime exception. The equivalent free-function
call is correctly rejected. This is a declared-rule-silently-ignored *and* unsound hole (provably-wrong
code — an unhandled checked exception — compiles clean).

---

## Evidence

Binary: `/stack/projects/phorj/target/release/phg` (prebuilt release, not rebuilt).

### 1. Baseline — free-function call, unhandled (correctly REJECTED)

`$TMP/uncaught-throws.phg`:

```phorj
package Main;
import Core.Console;
class NegativeInput implements Error { constructor(public string message) {} }
function validate(int n): int throws NegativeInput {
  if (n < 0) { throw new NegativeInput("must not be negative"); }
  return n;
}
function main(): void {
  var r = validate(0 - 5);          // no try/catch, no `?`; main declares no throws
  Console.println("escaped: {r}");
}
```

```
$ phg check uncaught-throws.phg
type error at 19:19: call to `validate` can throw `NegativeInput`, which is not handled here
  var r = validate(0 - 5);
                  ^
  [E-CALL-UNHANDLED]
  hint: wrap the call in `try { … } catch (NegativeInput e) { … }`, or propagate it with `?` and declare `throws NegativeInput`
[exit=1]
```

Enforced for the right reason. Companion positive/negative checks also pass:
- `?`-propagation into a non-covering `throws` set → `E-CALL-UNHANDLED` (rejected).
- a properly `try`/`catch`-handled call → `OK (type-checks clean)` and runs.
- under-declared `throws` set (throws `B`, declares `throws A` / nothing) → `E-THROW-UNDECLARED` (rejected).
- throwing a type that does not `implements Error` → `E-THROW-TYPE` (rejected).

So the free-function spine is sound.

### 2. THE GAP — throwing method call, unhandled (incorrectly ACCEPTED, escapes at runtime)

`$TMP/method-throw.phg`:

```phorj
package Main;
import Core.Console;
class A implements Error { constructor(public string message) {} }
class Svc {
  constructor() {}
  function go(int n): int throws A {
    if (n < 0) { throw new A("neg"); }
    return n;
  }
}
function main(): void {
  var s = new Svc();
  var r = s.go(0 - 1);              // unhandled throwing METHOD call; main declares no throws
  Console.println("escaped: {r}");
}
```

```
$ phg check method-throw.phg
OK (type-checks clean)
[exit=0]

$ phg run method-throw.phg
runtime error at 7: uncaught exception `A`
    if (n < 0) { throw new A("neg"); }
stack trace (most recent call first):
  → Svc::go            line 7
    main               line 13
[exit=1]
```

`check` reports **clean** for a program that the free-function rule would reject. The checked
exception escapes all the way to `main` uncaught — exactly the silent-`@throws` failure mode the
feature exists to prevent.

### 3. Control — identical body as a free function (REJECTED), confirming the path asymmetry

`$TMP/free-control.phg` (same `go`, but a top-level `function go` instead of a method):

```
$ phg check free-control.phg
type error at 9:13: call to `go` can throw `A`, which is not handled here
  var r = go(0 - 1);
            ^
  [E-CALL-UNHANDLED]
[exit=1]
```

The only difference between §2 (accepted) and §3 (rejected) is free-function vs. method dispatch ⇒
the gap is the method-call checking path, not the `throws` declaration or the thrower.

### 4. Secondary symptom — `?` on a method call is mis-typed

`$TMP/method-prop.phg` (`var r = s.go(0 - 1)?;`):

```
$ phg check method-prop.phg
type error at 13:22: `?` requires a `Result`-shaped operand (an enum with `Ok`/`Err` variants), found `int`
  [E-PROPAGATE-CONTEXT]
[exit=1]
```

On a *free* call `?` performs `throws`-propagation; on a *method* call it falls through to the
Result-`?` interpretation. Same root cause: the method-call path never consults the callee's `throws`
set, so neither the bare-call discharge nor the `?`-propagation specialization happens for methods.

---

## Root cause (verified by reading the checker)

`src/checker/calls.rs`:

- Free-function calls discharge `throws` correctly. `check_overload_call` (lines 213–223) iterates
  `m.throws` and calls `discharge_call_throw(name, e, span)` for each declared exception. Single-sig
  free calls go through the analogous path (line 158–159).
- **Method calls do NOT.** `check_method_sigs` (lines 231–270) receives the overload set as
  `applied: &[(Vec<Ty>, Ty)]` — a list of **`(params, ret)` pairs only**. The method signature's
  `throws` set is dropped at the point the `applied` tuples are built (the caller that constructs
  `applied` projects each method sig down to params + return type), so by the time
  `check_method_sigs` runs there is no `throws` information left to discharge. There is no
  `discharge_call_throw` / `try_throws_propagate` call anywhere in the method-call path.

In short: the `throws` set is structurally absent from the method-call type information, so the
checker physically cannot enforce it.

## Recommended fix

File: `src/checker/calls.rs` (with a small change at the `applied`-tuple construction site, likely in
`src/checker/calls.rs` / wherever method overloads are resolved, e.g. `check_method_call`).

1. Widen the method-overload tuple from `(Vec<Ty>, Ty)` to carry the per-overload `throws` set, e.g.
   `(Vec<Ty>, Ty, Vec<Ty>)` (params, ret, throws) — mirroring how `FnSig.throws` flows into
   `check_overload_call`.
2. In `check_method_sigs`, after selecting the matching overload(s), discharge their checked
   exceptions exactly as `check_overload_call` does (lines 213–223): for each `e` in the matched
   overload's `throws`, call `self.discharge_call_throw(name, e, span)` (de-duped across overloads).
   Add a `skip_throws` parameter for parity with `check_overload_call` if needed (the `?`-propagation
   path will want to suppress the bare discharge).
3. Wire the method-call `?` path (`Expr::Propagate` over a `Member`/method-call inner) into
   `try_throws_propagate` so `s.go(n)?` propagates the method's `throws` set into the enclosing
   function's declared `throws`, identically to a free call. Today it falls through to the Result-`?`
   branch (§4).
4. Regression tests: `src/checker/tests/throws.rs` currently covers only free-function calls (the
   `E-CALL-UNHANDLED` cases at lines ~158/182 are free calls). Add a method-call analogue for each:
   unhandled bare method call → `E-CALL-UNHANDLED`; method `?` into a covering/non-covering `throws`;
   handled method call → clean. These would have caught this gap.

## Severity

**P0 — unsound.** An unhandled checked exception (provably-wrong code by the feature's own contract)
type-checks clean whenever the throwing callee is a method. This breaks the central
"provably-correct upgrade of PHP" promise for the entire OO surface (any `class … function f() throws
E`), which is the idiomatic way most real Phorj code will raise errors.
