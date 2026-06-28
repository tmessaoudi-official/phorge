# Probe: cannot extend a final (non-`open`) class — final-by-default (M-RT S6)

**Rule:** Phorj is final-by-default. A class must be declared `open` to be extended;
`class B extends A {}` where `A` is not `open` MUST be rejected.

**Verdict: ENFORCED — not a gap.**

## Probe 1 (negative): extend a non-`open` class — MUST be rejected

`$TMP/final-extend.phg`:

```phorj
package Main;

import Core.Console;

class A {
    function who(): string {
        return "A";
    }
}

class B extends A {
    function who2(): string {
        return "B";
    }
}

function main(): void {
    B b = new B();
    Console.println(b.who());
}
```

Command + output:

```
$ /stack/projects/phorj/target/release/phg check $TMP/final-extend.phg
type error at 11:1: class `B` cannot extend `A`, which is not `open`
class B extends A {
^
  [E-EXTEND-FINAL]
  hint: mark the parent `open class A` to allow extension
exit=1

$ /stack/projects/phorj/target/release/phg run $TMP/final-extend.phg
type error at 11:1: class `B` cannot extend `A`, which is not `open`
class B extends A {
^
  [E-EXTEND-FINAL]
  hint: mark the parent `open class A` to allow extension
exit=1
```

Rejected for the RIGHT reason: `E-EXTEND-FINAL`, pointing at the `extends` clause, with an
accurate hint. Both `check` and `run` fail (the checker runs ahead of both backends).

## Probe 2 (positive control): extend an `open` class — MUST be accepted

`$TMP/final-extend-ok.phg` (same as Probe 1 but `open class A`, plus `b.who2()`):

```
$ /stack/projects/phorj/target/release/phg check $TMP/final-extend-ok.phg
OK (type-checks clean)
exit=0

$ /stack/projects/phorj/target/release/phg run $TMP/final-extend-ok.phg
A
B
exit=0
```

The rejection in Probe 1 is therefore not a blanket "extends is disabled" failure — it is
specifically gated on the parent's `open` flag.

## Where it's enforced (for reference)

`src/checker/collect.rs`:
- L164–168: builds a `class_open` map (`Item::Class(c) -> c.open`).
- L195: `E-EXTEND-UNKNOWN` when the parent name is undefined.
- L201–210: `else if !class_open.get(parent).copied().unwrap_or(false)` → emits
  `E-EXTEND-FINAL` ("class `{}` cannot extend `{parent}`, which is not `open`",
  hint: "mark the parent `open class {parent}` to allow extension").

The sibling override rule (`E-OVERRIDE-FINAL`, L251–296) is enforced the same way (a
non-`open` method cannot be overridden) — not probed here but co-located and structurally
identical.

## Severity / fix

No defect. Severity: none. No fix required.
