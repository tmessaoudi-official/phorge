# Probe: private method not callable cross-class

**Rule under test:** A `private` method must NOT be callable from outside its declaring class.
**Verdict:** ✅ ENFORCED — not a gap. Severity: none.
**Code emitted:** `E-METHOD-VISIBILITY` (correct, with accurate span + hint).

Binary: `/stack/projects/phorj/target/release/phg` (pre-built, not rebuilt).

---

## Probe 1 — external private call from `main` (SHOULD be rejected)

`private-method.phg`:

```phorj
package Main;

import Core.Console;

class Secret {
    constructor(public int x) {}
    private function hidden(): int { return this.x * 2; }
    function visible(): int { return this.hidden(); }
}

function main(): void {
    Secret s = new Secret(21);
    // external call to a private method — SHOULD be rejected
    Console.println("{s.hidden()}");
}
```

```
$ phg check private-method.phg
type error at 1:9: `hidden` is a private method of `Secret`
package Main;
        ^
  [E-METHOD-VISIBILITY]
  hint: it is accessible only inside `Secret`
exit=1

$ phg run private-method.phg
type error at 1:9: `hidden` is a private method of `Secret`
package Main;
        ^
  [E-METHOD-VISIBILITY]
  hint: it is accessible only inside `Secret`
exit=1
```

Rejected for the RIGHT reason (`E-METHOD-VISIBILITY`), at both `check` and `run`. (The reported
span column is slightly off — points at line 1 `package Main;` — a cosmetic span-mapping nit, not a
soundness issue; the diagnostic text correctly names `hidden`/`Secret`.)

## Probe 2 — positive control: internal call via public wrapper (SHOULD run)

`private-method-ok.phg` (identical class; `main` calls `s.visible()` which internally calls
`this.hidden()`):

```
$ phg run private-method-ok.phg
42
exit=0
```

Confirms the rule is not over-rejecting: a private method IS callable from inside its own class
(`visible()` → `this.hidden()`), and the value (21*2=42) is correct.

## Probe 3 — cross-class: class B calls A's private method (SHOULD be rejected)

`private-method-crossclass.phg`:

```phorj
class A {
    constructor(public int x) {}
    private function hidden(): int { return this.x; }
}
class B {
    function peek(A a): int { return a.hidden(); }
}
```

```
$ phg check private-method-crossclass.phg
type error at 11:46: `hidden` is a private method of `A`
    function peek(A a): int { return a.hidden(); }
                                             ^
  [E-METHOD-VISIBILITY]
  hint: it is accessible only inside `A`
exit=1
```

A sibling class B cannot reach A's private method — rejected with a precise span (caret on
`a.hidden()`) and the correct code.

---

## Conclusion

The `private`-method-not-callable-cross-class rule is **ENFORCED** on all three axes:
external-from-`main`, cross-class, and the positive-control internal call works. No soundness gap.

Enforcement site: `src/checker/calls.rs:1096` (`("method", "E-METHOD-VISIBILITY")`), backed by
`enforce_member_vis` and regression-tested in `src/checker/tests/visibility.rs:64-78`.

**Severity:** none (rule works). One P3-cosmetic observation: Probe 1's reported span column
(`1:9`) does not point at the offending call site (it does in Probe 3, `11:46`) — a span-mapping
inconsistency in the interpolation/`main`-context path, not a correctness defect. Recommended
follow-up (optional, out of scope for this audit): align the span computation in `calls.rs`
visibility-error construction for calls inside string interpolation so the caret lands on the call,
matching Probe 3's behavior.
