# Soundness probe — cannot mutate an immutable field (direct + via alias)

**Rule under test:** A non-`mutable` instance field must not be assignable. Assigning to it —
directly (`obj.f = v`), through an aliased binding (`alias.f = v`), or as a reassignment via
`this.f = v` in a method — must be rejected by the checker.

**Verdict: ENFORCED (not a gap).** Every assignment surface to an immutable field is rejected at
check time with the correct diagnostic `E-ASSIGN-IMMUTABLE`. `run` shares the same front-end so it
fails identically (no runtime bypass).

**Severity: none.**

---

## Probe 1 — direct assignment `b.x = 99` (promoted `public int x`)

File `$TMP/immutable-field-direct.phg`:
```phorj
package Main;
import Core.Console;

class Box {
    constructor(public int x) {}
}

function main(): void {
    Box b = new Box(1);
    b.x = 99;
    Console.println("b.x = {b.x}");
}
```

```
$ /stack/projects/phorj/target/release/phg check $TMP/immutable-field-direct.phg
type error at 10:5: field `x` of `Box` is immutable and cannot be assigned
    b.x = 99;
    ^
  [E-ASSIGN-IMMUTABLE]
  hint: declare it `mutable` (e.g. `mutable int x;`)
exit=1

$ /stack/projects/phorj/target/release/phg run $TMP/immutable-field-direct.phg
type error at 10:5: field `x` of `Box` is immutable and cannot be assigned
    b.x = 99;
    ^
  [E-ASSIGN-IMMUTABLE]
  hint: declare it `mutable` (e.g. `mutable int x;`)
exit=1
```
Rejected for the RIGHT reason. `run` does not bypass (same front-end gate).

## Probe 2 — assignment through an alias `alias.x = 99`

File `$TMP/immutable-field-alias.phg` (same class; `Box alias = b; alias.x = 99;`):
```
$ /stack/projects/phorj/target/release/phg check $TMP/immutable-field-alias.phg
type error at 11:5: field `x` of `Box` is immutable and cannot be assigned
    alias.x = 99;
    ^
  [E-ASSIGN-IMMUTABLE]
  hint: declare it `mutable` (e.g. `mutable int x;`)
exit=1

$ /stack/projects/phorj/target/release/phg run $TMP/immutable-field-alias.phg
type error at 11:5: field `x` of `Box` is immutable and cannot be assigned
    alias.x = 99;
    ^
  [E-ASSIGN-IMMUTABLE]
  hint: declare it `mutable` (e.g. `mutable int x;`)
exit=1
```
Mutability is a property of the field declaration, not the binding name — the alias path is
correctly caught.

## Probe 3 (extra) — declared (non-promoted) immutable field, external set

File `$TMP/immutable-field-declared.phg` (`int x;` set in ctor body, then `b.x = 99` externally):
```
$ /stack/projects/phorj/target/release/phg check $TMP/immutable-field-declared.phg
type error at 7:9: field `x` of `Box` is immutable and cannot be assigned
        this.x = v;
        ^
  [E-ASSIGN-IMMUTABLE] ...
type error at 13:5: field `x` of `Box` is immutable and cannot be assigned
    b.x = 99;
    ^
  [E-ASSIGN-IMMUTABLE] ...
exit=1
```
Note: the checker even rejects the ctor-body init `this.x = v` on a declared immutable field
(it expects promotion-style initialization). Stricter than required — not a soundness gap.

## Probe 4 (extra) — `this.x = v` reassignment inside a non-ctor method

File `$TMP/immutable-field-this-method.phg` (`function set(int v): void { this.x = v; }`):
```
$ /stack/projects/phorj/target/release/phg check $TMP/immutable-field-this-method.phg
type error at 7:9: field `x` of `Box` is immutable and cannot be assigned
        this.x = v;
        ^
  [E-ASSIGN-IMMUTABLE]
  hint: declare it `mutable` (e.g. `mutable int x;`)
exit=1
```
The `this.f =` write surface is covered too.

---

## Enforcement location

`src/checker/assign.rs` — the immutable-field check emits `E-ASSIGN-IMMUTABLE`:
- instance field: line ~281 (`field \`{name}\` of \`{class}\` is immutable and cannot be assigned`)
- static field: line ~202
- local binding: lines ~28 / ~88

All field/member assignment lowers through `checker::assign`, so direct, alias, and `this.f`
paths funnel into the same mutability gate. No separate runtime check is needed (and none could be
bypassed) because the assignment never type-checks.

## Recommendation

No fix needed. The rule is fully enforced across direct, alias, and `this.f` assignment surfaces.
Optional (P3, not required): the differential/regression suite could add an explicit
`E-ASSIGN-IMMUTABLE`-via-alias case if not already present, to lock the alias path against future
regressions.
