# Probe: cannot instantiate an abstract class

**Rule under test:** `new` on an `abstract class` must be rejected.
**Verdict: ENFORCED — not a gap.**
**Severity: none.**

## Probe binary
`/stack/projects/phorj/target/release/phg` (pre-built release, not rebuilt).

## Source location of the enforcing check
`src/checker/calls.rs:619-624` raises `E-ABSTRACT-INSTANTIATE` ("cannot instantiate
abstract class `{name}`") when the `new` target class is `abstract`. The check is
keyed on the class's `is_abstract` flag (set by the parser at
`src/parser/items.rs:18-23,47`), so it fires even for an abstract class that carries
NO abstract methods (the seed-bug-shaped empty-modifier edge).

## Evidence

### Case 1 — empty abstract class (`abstract class A {}`, no abstract methods), `new A()`
The interesting edge: a modifier with nothing forcing it to matter. Rejected anyway.

```
$ phg check abstract-new.phg
type error at 7:13: cannot instantiate abstract class `A`
    var a = new A();
            ^
  [E-ABSTRACT-INSTANTIATE]
  hint: `A` is `abstract`; instantiate a concrete subclass that implements its abstract methods
exit=1
$ phg run abstract-new.phg
type error at 7:13: cannot instantiate abstract class `A`
    ...
  [E-ABSTRACT-INSTANTIATE]
exit=1
```

Program:
```phorj
package Main;
import Core.Console;
abstract class A {}
function main(): int {
    var a = new A();
    Console.println("made an abstract instance");
    return 0;
}
```

### Case 2 — abstract class with an abstract method, `new Shape()`
```
$ phg check abstract-new2.phg
type error at 9:13: cannot instantiate abstract class `Shape`
    var s = new Shape();
            ^
  [E-ABSTRACT-INSTANTIATE]
  hint: `Shape` is `abstract`; instantiate a concrete subclass that implements its abstract methods
exit=1
```

Program:
```phorj
abstract class Shape { abstract function area(): int; }
function main(): int { var s = new Shape(); return s.area(); }
```

### Case 3 (control) — concrete subclass `new Square(4)` IS instantiable
Proves the rejection is targeted at the `abstract` modifier, not a blanket `new` failure.
```
$ phg check abstract-new3.phg
OK (type-checks clean)
exit=0
$ phg run abstract-new3.phg
16
exit=0
```

Program:
```phorj
abstract class Shape { abstract function area(): int; }
class Square extends Shape {
    constructor(public int side) {}
    function area(): int { return this.side * this.side; }
}
function main(): int { var s = new Square(4); Console.println("{s.area()}"); return 0; }
```

## Conclusion
The `abstract` modifier is fully load-bearing for instantiation: both the empty-abstract
edge and the abstract-with-methods case are rejected with the correct, specific
`E-ABSTRACT-INSTANTIATE` diagnostic, while a concrete subclass instantiates and runs.
The bad program is rejected for the RIGHT reason. No gap; no fix needed.
