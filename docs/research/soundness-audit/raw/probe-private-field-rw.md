# Probe: private field not readable/writable cross-class

**Rule under test:** A `private` field must be inaccessible (read AND write) from outside the
declaring class — including from `main` and from a different class.

**Verdict: ENFORCED** (not a gap). All bad accesses are rejected for the RIGHT reason
(`E-FIELD-VISIBILITY`) by the type-checker, in both `check` and `run`. Severity: **none**
(one P3 cosmetic note on caret position).

Probes live under
`/tmp/claude-1000/-stack-projects-phorj/36037d06-83af-463e-be0a-ffeae057a42d/scratchpad/audit/`.
Binary: `/stack/projects/phorj/target/release/phg` (pre-built, not rebuilt).

---

## Probe 1 — external READ from `main` → REJECTED (correct)

```phorj
package Main;
import Core.Console;
class Account { constructor(private int balance) {} }
function main(): void {
    Account a = new Account(100);
    Console.println("{a.balance}");   // reading a private field from main
}
```

```
$ phg run private-field-read.phg
type error at 1:2: `balance` is a private field of `Account`
package Main;
 ^
  [E-FIELD-VISIBILITY]
  hint: it is accessible only inside `Account`
[run exit=1]
```
(`check` produces the identical diagnostic.) Rejected for the right reason — `E-FIELD-VISIBILITY`,
correct field name, correct hint. NOTE (P3 cosmetic): the caret points to `1:2` (the `package`
line, an interpolation-span artifact) rather than the actual `a.balance` site; the message itself
is correct and actionable.

## Probe 2 — external WRITE from `main` → REJECTED (correct)

```phorj
class Account { constructor(private mutable int balance) {} }
function main(): void {
    Account a = new Account(100);
    a.balance = 999;                 // writing a private field from main
    Console.println("{a.balance}");
}
```

```
$ phg run private-field-write.phg
type error at 11:5: `balance` is a private field of `Account`
    a.balance = 999;
    ^
  [E-FIELD-VISIBILITY]
  hint: it is accessible only inside `Account`
type error at 1:2: `balance` is a private field of `Account`   (the read in the next line)
  [E-FIELD-VISIBILITY]
[run exit=1]
```
The WRITE site (`11:5`) is flagged with a correctly-positioned caret. Even though the field is
declared `mutable`, the visibility check fires — mutability and visibility are orthogonal and both
enforced.

## Probe 3 — cross-class READ (a different class reads another's private) → REJECTED (correct)

```phorj
class Account { constructor(private int balance) {} }
class Thief { function steal(Account a): int { return a.balance; } }
function main(): void { ... Console.println("{t.steal(a)}"); }
```

```
$ phg run private-field-cross.phg
type error at 10:46: `balance` is a private field of `Account`
    function steal(Account a): int { return a.balance; }
                                             ^
  [E-FIELD-VISIBILITY]
  hint: it is accessible only inside `Account`
[run exit=1]
```
Cross-class access from an unrelated class `Thief` is rejected with a correctly-positioned caret.

## Control — same-class access must SUCCEED (rules out a blanket false-positive)

```phorj
class Account {
    constructor(private mutable int balance) {}
    function get(): int { return this.balance; }
    function set(int v): void { this.balance = v; }
}
function main(): void { Account a = new Account(100); a.set(7); Console.println("{a.get()}"); }
```

```
$ phg run private-field-control.phg
7
[run exit=0]
```
Same-class read AND write through `this.balance` succeed → the rejections in Probes 1–3 are
precise, not a blanket "private fields are always inaccessible" over-rejection.

---

## Conclusion

Unlike the seed bug (private/protected on a constructor parsed-and-dropped), private FIELDS are
fully enforced for external read, external write, and cross-class read, in both the checker and at
run. The enforcement lives in the checker's `enforce_member_vis` chokepoint
(per memory [[member-visibility-six-access-sites]]: six external access surfaces all route through
it). This stage finds **no soundness gap**.

**Severity: none.** One P3 cosmetic only: the interpolation-embedded read (`"{a.balance}"`) reports
its caret at `1:2` (the `package` line) instead of the field site — the diagnostic text/code/hint
are all correct, so it does not affect soundness or enforcement. (See the known
interpolation-span-reset behavior; runtime faults inside `"{…}"` also report line 1.)

**Fix recommendation (cosmetic, optional):** in the diagnostics path for member-visibility errors
raised from inside an interpolation segment, propagate the sub-expression's absolute span (the
`StrSeg::Interp(_, offset)` absolute offset already exists per memory
[[ufcs-and-interpolation-span-fix]]) so the caret lands on `a.balance` rather than the package line.
File: the checker's member-access/interpolation span handling in `src/checker/` (the
`enforce_member_vis` caller for `Member` reads within interpolation segments). Non-load-bearing.
