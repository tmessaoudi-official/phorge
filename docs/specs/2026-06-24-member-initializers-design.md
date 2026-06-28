# Class Member Initializers — `const` + Expression Field Initializers — Design

**Date:** 2026-06-24
**Status:** Design (decided with the developer; spec for review before plan/impl).

Two related features that share the member-declaration surface
`[visibility] [const | static [mutable]] TYPE NAME = <initializer>;`:

1. **`const` class constants** — activate the vestigial `const` modifier as a real, compile-time,
   class-name-only constant with visibility.
2. **Expression field initializers** — lift PHP's constant-expression-only restriction: instance AND
   static fields may be initialized by an arbitrary expression (calls, closures, `this`/sibling reads),
   lowered to valid PHP.

Both are byte-identical `run ≡ runvm ≡ real PHP 8.5`. `const` adds **no** `Op`/`Value`; field
initializers add none either (they lower to existing constructor / assignment ops).

---

## Verified PHP baseline (why this is a Phorj-over-PHP win)

PHP property defaults must be **constant expressions** (probed on PHP 8.5):

| Initializer | PHP |
|---|---|
| literal, `const`-arithmetic (`self::A + self::B`), `new D()`, enum case | ✅ allowed |
| static-property read, function/method call, **closure**, `$this`, variable | ❌ "Constant expression contains invalid operations" |

So a computed/closure/stateful default forces you into the constructor in PHP. Phorj removes that
papercut by lowering arbitrary initializers to valid PHP (the TS-over-JS move).

---

## Feature 1 — `const` class constants

### Surface
```
[public|private|protected] const TYPE NAME = <literal-or-const-expr>;
const int MAX = 100;                 // public by default
private const string TAG = "x";
```

### Semantics (all developer-confirmed)
- **Type** explicit (Phorj mandates types). **Initializer** required; a compile-time **literal or
  const-expression** (literals, other in-scope consts, `+`/`*`/… on them — reuses/extends
  `value::const_literal`). `E-CONST-NO-INIT` / `E-CONST-NOT-LITERAL`.
- **Immutable always.** `const mutable` → `E-CONST-MUTABLE`. Reassigning `C.MAX = …` → `E-CONST-REASSIGN`.
- **Access: class-name-only** — `C.MAX`. Instance access `c.MAX` → `E-CONST-INSTANCE-ACCESS`
  (the same rule statics already enforce; also sharpen the static message to match).
- **Visibility:** member-level `public` (default) / `private` / `protected`, enforced by the existing
  member-visibility lattice.
- **Inherited:** a subclass accesses an inherited const via its **own** name (`Child.MAX` when `MAX`
  is on `Parent`) — matches PHP + Phorj's static/method inheritance.
- **Deferred (v1):** const-referencing-non-literal-const-expr beyond simple arithmetic; **interface
  constants** (classes only).

### Mechanism (no new `Op`/`Value`)
- Checker records each class's const name→value table (a literal `Value`), validates type/visibility/
  immutability, and resolves `C.MAX` accesses; **inlines the literal** at each access site on the Rust
  backends (interpreter + compiler both load the constant `Value` — byte-identical, no runtime store).
- **Transpiler:** emit a PHP **typed class constant** with visibility — `private const int MAX = 100;`
  (PHP 8.3+; floor 8.5 ✓) — and emit accesses as `C::MAX` (no `$`, distinct from a static field's
  `C::$s`). PHP resolves the constant to the same value → byte-identical output.

---

## Feature 2 — Expression field initializers (instance + static)

### Surface
```
class Task {
  int weight = compute(3) + 1;            // instance field: a call in the default
  (int) -> int scorer = fn(int x) => x*2; // instance field: a closure default
  int base = this.weight * 2;             // may read THIS + earlier-declared siblings
  static int SEED = compute(7);           // static field: runtime expression (once)
}
```
- Lifts the current restrictions: instance fields previously had **no** inline initializer (ctor-only);
  static fields were **literal-only**. Both now accept **any expression** (calls, method calls,
  closures, arithmetic, const/static reads).
- **`this` / sibling reads allowed, declaration-order** (developer's choice). An initializer may read
  `this` and **earlier-declared** fields; reading a **later** field → `E-FIELD-INIT-FORWARD-REF` (the
  forward-reference guard that tames the half-constructed-object footgun).
- Closures allowed; a field-default closure **may not capture `this`** in v1 (`E-FIELD-INIT-THIS-CAPTURE`)
  — defers to the closures slice (`this`-capture). It may still *read* `this.field` in a non-closure
  initializer.

### Evaluation model & lowering (the byte-identity-critical part)
- **Instance fields:** evaluated **per-instance at construction, in declaration order**, after promoted
  constructor params are bound. Lowered by the transpiler into a **constructor prelude**: if the class
  has a constructor, the field-init assignments are prepended (after promotion); if it has none, a
  constructor is synthesized. Interpreter + VM evaluate the initializers at construction in the same
  declaration order → identical field values.
- **Static fields:** evaluated **once** (first class use), in declaration order among statics. PHP has
  no runtime static-property default, so the transpiler emits a **one-time guarded initializer**
  (a static `__phorj_init` run once per class, or a `??=`-guarded lazy set) — the harder case the
  developer chose to include. Rust backends evaluate static initializers once at program start
  (extending the existing `static_inits` path, which today only handles literals).
- **Byte-identity:** guaranteed by a single shared declaration-order evaluation contract honored by all
  three backends; the differential gates it (a guide example + a `this`/sibling-order case).

### Errors
`E-FIELD-INIT-FORWARD-REF`, `E-FIELD-INIT-THIS-CAPTURE`; type mismatches reuse the assignment error.

### Open implementation note
The static one-time-init guard is the riskiest piece (PHP-side timing). The plan should build
**instance-field initializers first** (clean ctor lowering), then **static-field initializers** (the
guarded init) as a second step, each independently gated.

---

## Sequencing (developer: specs-first)

Specs for `new`, `const`, and field initializers land for review, then plans, then build. Suggested
implementation order once approved: **`const`** (additive, low-risk) → **field initializers**
(instance then static) → **`new`** (breaking codemod) — or as the developer directs.

## Test plan (both features)
- Checker: const visibility/immutability/instance-access/inheritance; field-init forward-ref +
  this-capture guards; clean cases.
- Differential: a `const` guide example (`C.MAX`, inheritance) and a field-init example (a computed
  default, a closure default, a `this`/sibling-order case, a static runtime init) — all byte-identical
  `run ≡ runvm ≡ real PHP 8.5`.
