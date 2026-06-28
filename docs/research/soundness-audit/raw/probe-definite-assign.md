# Soundness Probe — Definite Assignment of Non-Optional Fields

**Rule under test:** Every non-optional instance field must be definitely initialized (either by a
field initializer or by the constructor on every path). A non-optional `T` field that is never set
must be a compile-time error.

**Verdict: GAP — P0 (unsound).** The checker accepts a class with a non-optional, non-initialized
field; the type system then *promises* a value of type `T` for that field (method return type, field
type) but the runtime instance has no such slot — the field read faults at runtime ("no field `x`").
This is a textbook type hole: `check` is clean, the program is provably wrong.

There is a **second, worse facet**: a bare non-`mutable` field (`int x;`) **cannot ever be
assigned** — even inside the constructor body — because `this.x = …` is rejected by
`E-ASSIGN-IMMUTABLE`. So a non-optional, non-`mutable`, non-initialized field is a *permanently
unusable* declaration that the checker nonetheless accepts and lets you read.

---

## Evidence

Binary: `/stack/projects/phorj/target/release/phg` (prebuilt release, not rebuilt).

### Probe 1 — non-optional field never initialized, read via method

`$TMP/definite-assign.phg`:
```phorj
package Main;
import Core.Console;

class Secret {
    int x;
    constructor(int y) {
        // x is NEVER initialized
    }
    function xOf(): int { return this.x; }
}

function main(): void {
    Secret s = new Secret(5);
    Console.println("x = {s.xOf()}");
}
```

```
$ phg check $TMP/definite-assign.phg
OK (type-checks clean)
exit=0

$ phg run $TMP/definite-assign.phg
runtime error at 9: no field `x` on `Secret`
    function xOf(): int { return this.x; }
stack trace (most recent call first):
  → Secret::xOf        line 9
    main               line 14
exit=1
```

`check` passes clean (exit 0). `xOf()` is declared `: int`, yet at runtime there is no field `x` —
the checker's `int` promise is unbacked. **GAP.**

### Probe 2 — same hole via external field read

```phorj
class Secret { int x; constructor(int y) {} }
... Console.println("x = {s.x}");
```
```
$ phg check $TMP/da2.phg   →  OK (type-checks clean)   exit=0
$ phg run   $TMP/da2.phg   →  runtime error at 9: no field `x` on `Secret`   exit=1
```

### Probe 3 — used as an arithmetic operand; both backends agree (so the differential spine does NOT catch it)

```phorj
class Secret { int x; constructor(int y) {} function calc(): int { return this.x + 1; } }
```
```
$ phg check  $TMP/da3.phg  →  OK (type-checks clean)   exit=0
$ phg run    $TMP/da3.phg  →  runtime error at 6: no field `x` on `Secret`   exit=1
$ phg runvm  $TMP/da3.phg  →  runtime error at 6: no field `x` on `Secret`   exit=1
```
Both `run` and `runvm` fault identically — this is a **front-end soundness gap**, invisible to the
byte-identity differential harness (both backends are equally wrong). Only `check` should have
caught it.

### Probe 4 — the compounding facet: a bare field cannot even be initialized in the ctor

```phorj
class Ok { int x; constructor(int y) { this.x = y; } function xOf(): int { return this.x; } }
```
```
$ phg check $TMP/da4.phg
type error at 5:26: field `x` of `Ok` is immutable and cannot be assigned
    constructor(int y) { this.x = y; }
                         ^
  [E-ASSIGN-IMMUTABLE]
  hint: declare it `mutable` (e.g. `mutable int x;`)
exit=1
```
A non-`mutable` declared field is unassignable everywhere, including the constructor. So the *only*
way to give a bare non-mutable field a value is a field initializer (`int x = …;`). A non-optional
bare field with no initializer is therefore **always** a latent runtime fault — and the checker
never says so.

### Probe 5 — `mutable` field, still never initialized → same GAP

```phorj
class C { mutable int x; constructor(int y) {} function xOf(): int { return this.x; } }
```
```
$ phg check $TMP/da5.phg  →  OK (type-checks clean)   exit=0
$ phg run   $TMP/da5.phg  →  runtime error at 6: no field `x` on `C`   exit=1
```
Making it `mutable` (so it *could* be assigned) does not help — the checker still never demands the
assignment.

### Controls (the cases that SHOULD pass, and do)

```
V6  mutable int x; ctor sets this.x = y;        → check OK, run "x = 5"      (exit 0)  ✓ correct
V7  mutable int? x; never set                    → check OK, run "ok"         (exit 0)  ✓ correct (optional defaults absent/null)
V8  int x = 7;  (field initializer, no ctor set) → check OK, run "x = 7"      (exit 0)  ✓ correct
```
These confirm the probes above fail for the *right* reason (missing definite assignment), not a
syntax error in the probe: when the field IS given a value (V6/V8) the identical program runs
cleanly, and an *optional* field (V7) is legitimately allowed to be unset.

---

## Root cause (source)

`src/checker/program.rs`, `check_type_body` (≈ lines 216–380):

- The constructor is handled by the `ClassMember::Constructor { params, body, .. }` arm, which only
  does `self.check_body(body)` — it **type-checks** the body but performs **no definite-assignment
  analysis** of the instance fields.
- Fields *with* an initializer (`ClassMember::Field { init: Some(e), .. }`) get Feature-B
  forward-reference + type checks and are added to the `available`/`instance_fields` tracking sets.
- Fields *without* an initializer fall into the final catch-all arm:
  ```rust
  ClassMember::Field { .. } => {}
  ```
  — a complete **no-op**. Nothing ever checks that such a field is non-optional, and nothing checks
  whether the constructor assigns it.

The machinery to do this is already half-present: `instance_fields` and `available` sets are built
in the same function. What is missing is a post-constructor pass that, for each instance field whose
type is **not** `Ty::Optional` and which has **no** initializer, requires it to be definitely
assigned (`this.<field> = …`) on every path of the constructor body.

## Recommended fix

In `src/checker/program.rs::check_type_body`, after checking the constructor body (and accounting
for field initializers), add a definite-assignment check:

1. Compute the set of **required** fields = instance fields that are (a) **not** `Ty::Optional` and
   (b) have **no** field initializer.
2. Walk the constructor body collecting fields definitely assigned on all paths via `this.f = …`
   (the conservative `block_terminates`/path engine already built for the totality cluster —
   `stmt_terminates` in the checker — is the right precedent for the "on every path" join: a field
   counts as assigned only if assigned in both arms of an `if`, etc.).
3. Any required field not in that set → a new diagnostic, e.g. `E-FIELD-UNINITIALIZED`
   (*"non-optional field `x` is never initialized — assign it in the constructor, give it an
   initializer `int x = …;`, or make it optional `int? x;`"*). If the class has **no** constructor
   at all, every required field is uninitialized.
4. Because a bare non-`mutable` field is unassignable in the ctor (Probe 4), the diagnostic's hint
   should steer the user to the only valid remedies: a field initializer, `mutable` + ctor
   assignment, or `int?`.

This is **front-end-only** (no new `Op`, no `Value` change, no backend touch) — the byte-identity
spine is untouched; it merely turns an existing latent runtime fault into a compile-time error,
which is exactly Phorj's "provably-correct upgrade of PHP" thesis.

**Severity: P0** — it is an unsoundness (a declared non-optional `T` slot that does not exist at
runtime; the checker hands out an `int` that is really "field missing"). It also masks a usability
trap (Probe 4: a bare field that can never be filled is silently accepted).
