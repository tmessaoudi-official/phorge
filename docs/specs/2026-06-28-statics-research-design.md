# Comprehensive statics — research + design (Item C)

> Status: **research delivered** 2026-06-28; scope fork awaiting the developer. Follows the
> GA-sequence decision to "schedule a research + brainstorm pass" on statics (Batch 2). Builds on the
> shipped narrow scope (slice B0: own-class, single-signature `ClassName.method(args)`).

## What ships today (B0)

`ClassName.method(args)` calls a `static` method — checked in `check_static_method_call`
(`src/checker/calls.rs:977`): it finds the signature in `classes[cls].methods` (which *is*
inheritance-flattened) but **gates on `classes[cls].static_methods`**, a per-class **own-only**
`HashSet` (`collect.rs`; the interpreter's `call_static_method` is "own members" too). A static call
**lowers to a single direct call** to one function. Three documented deferrals — the research targets:

## Area A — inherited static methods (`Child.parentStatic()`)

**Today:** rejected. `Child` doesn't own `parentStatic`, so the own-only `static_methods` gate fails
with `E-STATIC-CALL` ("instance method") even though the signature exists in the flattened `methods`.
**PHP:** `Child::parentStatic()` works — statics are inherited.

**Design:** flatten `static_methods` across ancestors exactly as `methods`/`ClassTables` already do
(reuse `class_supertypes`). Resolution + lowering must target the **declaring** class's static
function (the ancestor that owns it), found by walking `cls`→ancestors for the first owner. No runtime
concept needed (no LSB — see Area C): an inherited static is just a call to the ancestor's function.
- **Cost:** low. Front-end + a declaring-class lookup in the lowering. No new `Op`/`Value`.
- **Risk:** a static method *overridden* in a child (same name, static, in both) — resolution picks the
  most-derived owner walking from `cls` up. Clean, deterministic.
- **Recommendation: BUILD.** PHP-faithful, low cost, closes the most common gap.

## Area B — overloaded static methods

**Today:** rejected (`E-STATIC-CALL`, "overloaded … not yet supported") because a static call lowers
to one direct function index. **But** the checker already has full overload *resolution*
(`check_overload_call`, `calls.rs:197`) used by instance methods + free functions: it selects the
matching overload by arg arity/assignability.

**Design:** route a multi-signature static call through the *same* `check_overload_call`, then lower to
the **resolved** overload's static function (the lowering already mangles overloads for instance/free
calls — reuse that selection key at the static site). The byte-identity risk the B0 comment flagged
("silently calling one overload") is exactly what compile-time resolution removes: the *chosen*
overload is fixed at check time, identical on all backends.
- **Cost:** medium — depends on how the existing overload lowering keys the chosen function; if the
  static lowering can reuse that key, it's small.
- **Recommendation: BUILD** (after A), reusing the existing overload machinery. Falls out naturally.

## Area C — late static binding (`static::`, `new static()`)

**Today:** absent. PHP's LSB makes `static::` / `new static()` inside a static method resolve to the
**runtime called class**, not the declaring one — enabling a base-class `create()` that returns the
right subclass.

**Design tension (the real fork):**
- LSB is a genuine PHP idiom (ORMs, active-record factories) — *familiarity* argues for it.
- But it is **subtle/surprising** (the `self::` vs `static::` distinction is a classic PHP footgun) and
  it introduces a **runtime "called class" concept** threaded through static dispatch — the first
  static feature that isn't a pure compile-time resolution. That cuts against Phorj's "legible, no
  surprises, no new runtime machinery unless necessary."
- It also interacts with the type system: `new static()` has type "the called class" — expressible only
  as the enclosing class bound, an `F`-bounded-polymorphism shape Phorj doesn't have.

**Options:**
1. **Defer + reject cleanly** (recommended): no `static::`/`new static()`; document as a deliberate
   non-feature. The inherited+overloaded statics (A+B) cover the everyday cases; the factory-returns-
   subclass pattern is achievable by overriding the static in each subclass (explicit > magic).
2. **Build LSB**: thread the called-class through static dispatch (a runtime value), add `static`/`Self`
   as a type. High cost, real footgun surface, needs its own design slice.

- **Recommendation: DEFER (Option 1)** for this pass — ship A+B (the common, low-risk, compile-time
  cases), document LSB as a considered non-feature with the explicit-override workaround. Revisit LSB
  as its own milestone if a concrete need appears.

## Proposed scope for the build

**A (inherited) + B (overloaded), defer C (LSB).** Both A and B are compile-time, no new `Op`/`Value`,
byte-identity-safe by construction, and reuse existing machinery (inheritance flattening + overload
resolution). Each ships with checker tests, a guide example, and updated KNOWN_ISSUES. LSB stays a
documented non-feature pending a dedicated milestone.
