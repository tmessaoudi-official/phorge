# Soundness Audit — Probe: interface method implemented with the declared signature

**Rule probed:** A class that `implements` an interface must provide each interface method
with a *matching* signature (arity, parameter types, return type). A wrong signature must
be rejected.

**Verdict: ENFORCED — no gap.** All signature-mismatch kinds (return type, arity, parameter
type), including methods inherited via interface `extends`, are rejected at check-time with
the dedicated code **`E-IFACE-SIG`**. A correct signature passes (control). The rejection is
for the right reason (the diagnostic names the class, method, and interface).

---

## Evidence

`BIN=/stack/projects/phorj/target/release/phg`

### Probe A — wrong return type (interface `string`, class returns `int`)

```
$ $BIN check iface-sig-rettype.phg
type error at 8:1: class `Robot` method `speak` does not match interface `Speaker`'s signature
class Robot implements Speaker {
^
  [E-IFACE-SIG]
  hint: the parameter types and return type must match the interface
exit=1
```
(`run` produces the identical error, exit=1 — check runs before execution.)

### Probe B — wrong arity (interface `greet(string)`, class `greet()`)

```
$ $BIN check iface-sig-arity.phg
type error at 8:1: class `Hello` method `greet` does not match interface `Greeter`'s signature
class Hello implements Greeter {
^
  [E-IFACE-SIG]
  hint: the parameter types and return type must match the interface
exit=1
```

### Probe C — wrong parameter type (interface `add(int)`, class `add(string)`)

```
$ $BIN check iface-sig-paramtype.phg
type error at 8:1: class `A` method `add` does not match interface `Adder`'s signature
class A implements Adder {
^
  [E-IFACE-SIG]
  hint: the parameter types and return type must match the interface
exit=1
```

### Inherited interface method (via `extends`) — wrong sig still caught

```
$ $BIN check iface-sig-extends.phg     # interface Pet extends Speaker; Dog.speak returns int
type error at 7:1: class `Dog` method `speak` does not match interface `Pet`'s signature
class Dog implements Pet {
^
  [E-IFACE-SIG]
  hint: the parameter types and return type must match the interface
exit=1
```

### Control — correct signature must pass (and does)

```
$ $BIN run iface-sig-ok.phg            # class A implements Adder { add(int x): int }
42
exit=0
```

---

## Where it lives (for reference)

- Check: `src/checker/collect.rs:575-578` — emits `E-IFACE-SIG` when a class method's
  signature diverges from the interface's.
- Doc: `src/cli/explain.rs:333` (`phg explain E-IFACE-SIG`).
- Test: `src/checker/tests/interfaces.rs:32` already asserts this code.

## Severity / fix

**severity: none** — the rule is enforced. No fix required. (Documented scope limit per
CLAUDE.md, not probed here: signature match is *exact*, no parameter contravariance / return
covariance — but that is a soundness-conservative choice, not a gap.)
