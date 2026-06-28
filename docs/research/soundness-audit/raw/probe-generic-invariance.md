# Soundness probe — generic type args are invariant at assignment

**Verdict: GAP (P0 — unsound type hole).** A `Box<string>` value is accepted into a `Box<int>`
slot. The checker reports OK, then the program runtime-faults when the wrongly-typed value is used
at its (false) static type. This is provably-wrong code passing the "provably-correct" checker.

Known limitation acknowledged in `CLAUDE.md` / KNOWN_ISSUES ("same-head generic types are *not*
actually invariant at an assignment boundary … the nominal assignability check short-circuits on the
reflexive name edge before the invariant arg compare; a real fix touches the shared subtype oracle
and is deferred"). This probe **confirms** it, **proves it is a genuine type hole** (not merely a
missing diagnostic), and **locates the exact line**.

## Binary
`/stack/projects/phorj/target/release/phg` (pre-built release, not rebuilt).

## Probe 1 — Box<string> into Box<int> slot

Program (`$TMP/generic-invariance.phg`):
```
package Main;
import Core.Console;
class Box<T> {
  constructor(private T value) {}
  function get(): T { return this.value; }
}
function main(): void {
  Box<int> b = new Box("phorj"); // Box<string> into a Box<int> slot — SHOULD be rejected
  var v = b.get();
  Console.println("got value");
}
```

```
$ phg check $TMP/generic-invariance.phg
OK (type-checks clean)
exit=0
$ phg run $TMP/generic-invariance.phg
got value
exit=0
```

The mismatched generic instantiation type-checks clean and runs. Should have been a type error.

## Probe 2 — proof it is a real hole (wrong type used as `int`)

```
function main(): void {
  Box<int> b = new Box("phorj"); // Box<string> bound to Box<int>: hole
  int x = b.get();                // checker thinks get() returns int -> x:int
  int y = x + 1;                  // arithmetic on a string at runtime
  Console.println("y = {y}");
}
```

```
$ phg check $TMP/generic-invariance2.phg
OK (type-checks clean)
exit=0
$ phg run $TMP/generic-invariance2.phg
runtime error at 15: cannot apply Add to string and int
  int y = x + 1;                  // arithmetic on a string at runtime
stack trace (most recent call first):
  → main               line 15
exit=1
```

The checker accepts `int x = b.get()` (because it believes `b: Box<int>` ⇒ `get(): int`), so
`x + 1` is statically a valid `int + int`. At runtime the value is a `string` and the add faults.
A statically-typed language whose pitch is *provable correctness* let a type error reach runtime.

## Control — the checker DOES reject a plain mismatch (so this is not a dead checker)

```
function main(): void { int x = "phorj"; ... }
```
```
$ phg check $TMP/control.phg
type error at 4:3: expected `int`, found `string`
  int x = "phorj"; // plain mismatch
  ^
exit=1
```
Plain `int = "phorj"` is correctly rejected — proving the hole is specific to same-head generic
argument invariance, not a globally-broken assignability check.

## Root cause (located)

`src/types.rs:228`, in `Ty::assignable_with`:
```rust
// Nominal types: a subtype edge … or the same head with **invariant** type arguments …
(Ty::Named(a, aa), Ty::Named(b, ba)) => subtype(a, b) || (a == b && aa == ba),
```

The comment *claims* invariant args, but `subtype(a, b)` is evaluated first and `||`-short-circuits.
The subtype oracle is reflexive — `src/checker/collect.rs:846`:
```rust
pub(super) fn is_subtype(&self, a: &str, b: &str) -> bool {
    if a == b { return true; }   // reflexive name edge
    ...
}
```
So for `Box<string> -> Box<int>`: `a == b == "Box"` ⇒ `subtype("Box","Box")` returns `true` ⇒
the whole `||` is `true` **before** `aa == ba` (`[string]` vs `[int]`) is ever compared. The
invariant arg check is dead code whenever the heads match.

`Ty::assignable_with` is the single shared subtype gate, reached via two callers:
- `Ty::assignable` (types.rs:112-115) passes the no-edge oracle `&|_, _| false`; `subtype` is always
  `false`, so it correctly falls through to `a == b && aa == ba`. **Not affected.**
- the checker's `ty_assignable` (`src/checker/collect.rs:893-896`) passes `is_subtype`, which is
  reflexive (`a == b ⇒ true`). **This is the affected path** — and it is the one the real type
  checker uses for assignment, so the hole is live in practice.

## Concrete fix

`src/types.rs:228` — split the same-head case from the genuine subtype edge so a matching head
ALWAYS requires invariant args, and the oracle is consulted only for a real `a != b` edge:

```rust
(Ty::Named(a, aa), Ty::Named(b, ba)) => {
    if a == b {
        aa == ba            // same head ⇒ invariant in type arguments (no oracle bypass)
    } else {
        subtype(a, b)       // genuine class→interface / class→parent edge (args empty in practice)
    }
}
```

This is safe: every legitimate nominal subtype edge in the in-file tests (`src/types.rs` ~440–520:
`A→Speaker`, `Dog→Drawable`, …) uses **distinct** names, so the `else` branch still serves them; the
reflexive `a == b` edge is the only one that must instead enforce arg invariance. Non-generic
nominals have empty args on both sides, so `aa == ba` reduces to `true` for them — no regression for
plain classes/interfaces.

A regression test should be added (probe 1 ⇒ `E-…`/type error; probe 2 likewise) once the fix lands.
Note the same line governs **generic enums** (`Option<string>` into `Option<int>` — KNOWN_ISSUES
flags them as sharing this hole), so the single fix closes both class and enum invariance.

## Severity
**P0** — unsound: provably-wrong code (a string used where the checker guarantees an int) passes the
static checker and reaches runtime. Worse than a missing feature for a correctness-pitched language.
