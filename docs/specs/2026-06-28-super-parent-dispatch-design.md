# Super/Parent Dispatch — Design

- **Date:** 2026-06-28
- **Status:** Designed — pending user review, then plan.
- **Milestone:** GA marathon (step 1 of the locked order; see
  `docs/plans/2026-06-28-ga-marathon-super-overloading.plan.md`).
- **Backends:** front-end + both runtime backends + transpiler; byte-identical
  `run ≡ runvm ≡ real PHP 8.5`. All PHP-emission strategies below are **verified against real PHP
  8.5** (`-n`, no deprecation warnings) — see "Verification" at the end.

## Motivation

Phorj has inheritance (single + multiple, `final`-by-default, `abstract`) and method **override**
(an `open` parent method, a child redeclaring it), but **no way to invoke the implementation an
override shadows** — there is no `super`/`parent` construct (today inherited methods are reached only
via `this.m()`, which dispatches to the *resolved* method, never "the one above me"). This blocks two
real things:

1. A child override cannot reuse + extend its parent's behaviour (`return parent.describe() + …`).
2. **A class that declares its own constructor under inheritance cannot initialize inherited parent
   state** — the standing KNOWN_ISSUE (the deferred "`super`-replacement").

This slice adds `parent` dispatch for **methods and constructors**, across single and multiple
inheritance.

## Syntax

`parent` is a new **contextual keyword**, valid only as a call head inside an instance method or
constructor body (it is not reserved — existing identifiers named `parent` are unaffected elsewhere).

- **Immediate:** `parent.m(args)` / `parent.constructor(args)` — the nearest ancestor's `m` (resp.
  the immediate parent's constructor).
- **Qualified (any ancestor):** `parent(A).m(args)` / `parent(A).constructor(args)` — the named
  ancestor `A`'s `m` (resp. constructor). `parent(A)` is the call-style ancestor selector (chosen
  over `parent[A]`, which collides visually with indexing, and `parent.A.m()`, which reads as member
  access).

`parent` dispatches **methods and constructors only**. Field access via `parent` is out of scope
(a redeclared field shadows; fields are not virtual).

## Semantics

### Resolution

- **`parent.m()`** resolves to the **nearest ancestor that declares `m`**, walking up the MRO
  (`ast::class_mro`) **from above the current class** (so an override calling `parent.m()` reaches
  the version it shadows, never itself). Ancestors that do not declare `m` are skipped (nothing to
  run there) — that is resolution, not chaining.
- **`parent(A).m()`** resolves to `A`'s `m` (the `m` that an instance of `A` would run, i.e. `A`'s
  own or its nearest-ancestor's per `A`'s MRO). `A` may be **any** transitive ancestor — a deep
  descendant may jump straight to a grandparent (C++-style), explicitly skipping intermediate
  overrides.

### Chaining is always explicit

There is **no auto-chaining**. `parent.m()` / `parent(A).m()` runs **one** method, once. Whether it
continues further up is entirely that method's own business (it forwards only if *it* contains a
`parent` call). To run a deep chain top-to-bottom, every level forwards explicitly — the universal
`super`/`parent::` model (PHP/Java/C#/Swift). The only implicit behaviour is MRO skipping
non-declaring ancestors.

### Constructors

A class declaring its own constructor initializes inherited state by **explicit** parent-ctor calls:

- Single inheritance: `parent.constructor(args)`.
- MI: `parent(P).constructor(args)` **once per parent** to initialize.

The existing **no-own-ctor** construction plan is unchanged (`ast::ctor_plan`: single inheritance
runs the inherited ctor; MI runs every parent's ctor via the synthesized orchestrator). `parent`
ctor calls are only for the **own-ctor** case (the KNOWN_ISSUE this closes).

### Ambiguity & errors

- `parent.m()` (or `parent.constructor()`) in a **multiple-inheritance** class where **≥2 ancestor
  arms** declare the method → `E-PARENT-AMBIGUOUS`; the fix is to qualify (`parent(A).m()`).
- `parent(A).…` where `A` is **not an ancestor** of the current class → `E-PARENT-NOT-ANCESTOR`.
- `parent(A).m()` where `A` neither declares nor inherits `m` → `E-PARENT-NO-METHOD`.
- `parent` used outside an instance method/constructor body (e.g. a free function, a `static`
  method) → `E-PARENT-OUTSIDE-METHOD`.
- `parent` in a class with **no** parents → `E-PARENT-NO-PARENT`.

All codes self-document via `phg explain`.

## Worked examples (current syntax)

**A — single inheritance: immediate vs ancestor-jump**
```phorj
open class Animal { open function describe(): string { return "animal"; } }
open class Dog extends Animal {
    open function describe(): string { return "{parent.describe()}/dog"; }   // -> "animal/dog"
}
class Puppy extends Dog {
    function describe(): string {
        string viaDog    = parent.describe();           // immediate Dog -> "animal/dog"
        string viaAnimal = parent(Animal).describe();   // jump past Dog -> "animal"
        return "{viaDog} | {viaAnimal} | puppy";        // "animal/dog | animal | puppy"
    }
}
```

**B — MI diamond: jump vs through, ambiguity**
```phorj
open class Base  { open function tag(): int { return 1; } }
open class Left  extends Base { open function tag(): int { return parent.tag() + 100; } }  // 101
open class Right extends Base { }                                                          // inherits Base.tag
class Both extends Left, Right {
    function tag(): int {
        int up   = parent(Base).tag();   // jump to grandparent Base -> 1
        int left = parent(Left).tag();   // through Left (which chains to Base) -> 101
        return up * 1000 + left;         // 1101
    }
}
// bare `parent.tag()` in Both -> E-PARENT-AMBIGUOUS
```

**C — constructor chaining (single + MI)**
```phorj
open class Shape  { constructor(public int sides) {} }
class Square extends Shape {
    constructor(public int size) { parent.constructor(4); }     // Shape.sides = 4
}

open class Engine  { constructor(public int hp) {} }
open class Chassis { constructor(public int wheels) {} }
class Car extends Engine, Chassis {
    constructor(public string name) {
        parent(Engine).constructor(200);     // Engine.hp = 200
        parent(Chassis).constructor(4);       // Chassis.wheels = 4
    }
}
```

## Backend strategy

Resolution is computed **once** in the front end from `ast::class_mro` + `class_method_origins` and
threaded to both backends as the concrete `(declaring_class, method)` target — the same
single-source discipline as ordinary dispatch, so `run ≡ runvm`. The interpreter calls the resolved
method with `this` bound to the current receiver; the VM dispatches to the resolved function index.
A `parent.constructor` call runs the resolved ctor body on the current instance.

Whether this needs a **new `Op`** (e.g. `Op::CallParent`) or can reuse the existing method-call op
with a pre-resolved target is a **plan-phase decision** — prefer reusing the existing dispatch with a
resolved target (no new `Op`) if the compiler can bake the target.

## Transpile strategy (verified)

- **Single inheritance** → native PHP: `parent.m()` ⇒ `parent::m()`; `parent(A).m()` (A a
  transitive ancestor) ⇒ `A::m()` (PHP forwards `$this`, **no deprecation** under 8.5);
  `parent.constructor(a)` ⇒ `parent::__construct(a)`.
- **Multiple inheritance** (lowered to traits/interfaces) → **trait aliasing**: for each
  `parent(X)`-targeted method/ctor, the class **directly `use`s `X`'s trait** with an alias
  (`use TX { TX::m as private __super_X_m; }`), and the call emits `$this->__super_X_m(args)`. Trait
  method collisions are resolved with `insteadof`. Verified for one-arm, multi-arm, diamond, and
  constructors (see Verification).

### Prerequisite — complete the MI "multi-of-multi" lowering

Full MI parent-dispatch requires the transpiler to emit **any class used as an MI-parent (and the
ancestors a `parent(X)` call targets) as a trait**, including **traits-that-use-traits** for deeper
ancestors. This **completes the currently-deferred multi-of-multi lowering** (KNOWN_ISSUES: a
multi-of-multi class "is not also emitted as a trait"). This foundation work lands **first/with** the
parent-dispatch emission. It is real transpiler work but verified-achievable with standard
`use`/`insteadof`/`as`.

## Out of scope / deferred

- `parent` field access (fields shadow, not virtual).
- `parent` in a `static` method (no receiver) — `E-PARENT-OUTSIDE-METHOD`.
- Auto-chaining (`parent.m()` running the whole ancestor chain) — explicitly rejected.

## Examples to ship (with the feature)

- `examples/guide/parent-dispatch.phg` — single-inheritance immediate + jump + ctor.
- `examples/guide/parent-dispatch-mi.phg` — MI qualified method calls, diamond, MI ctor.

Both byte-identical on `run`/`runvm` and round-tripped through real PHP 8.5.

## Test plan

- Checker: every `E-PARENT-*` code (positive + negative); ambiguity in MI; not-an-ancestor;
  outside-method; no-parent.
- Differential (`tests/differential.rs`): the two guide examples + targeted single/MI/diamond/ctor
  programs gating `run ≡ runvm` and the PHP-8.5 oracle (trait-aliased emission round-trips).
- Transpile: the multi-of-multi lowering completion (any MI-parent and `parent(X)`-target emitted as
  a trait-using-trait), with collisions resolved by `insteadof`.

## Verification (real PHP 8.5, `php -n`)

- One-arm MI parent-call via `TA::m as __super_A_m` → `11`. ✓
- Multi-arm qualified (`insteadof` + alias both) → `30`. ✓
- Diamond / trait-using-trait → `100`, shared base `A`. ✓
- Single-inheritance ancestor JUMP (`A::greet()` from grandchild, no deprecation) → `AB|A|C`. ✓
- MI grandparent jump (direct `use TA` alias + parents' traits) → `1101`, base `A`. ✓
- MI parent-constructor via trait-ctor aliasing → `zoe hp=200 wheels=4`. ✓
