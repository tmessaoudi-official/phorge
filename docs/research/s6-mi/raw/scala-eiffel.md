# S6 Multiple-Inheritance research — Scala traits (→JVM) & Eiffel MI

> Topic owner research for Phorj S6 (`extends`/abstract/traits). Two precedents:
> **(1) Scala traits compiled to the JVM** — the closest real-world analogue to Phorj→PHP
> (MI-of-implementation lowered onto a single-inheritance target), and **(2) Eiffel MI** —
> the gold standard for *explicit, feature-level* conflict resolution.
>
> Phorj target is **PHP 8.4**: single class inheritance, many interfaces, traits
> (`use A, B;` carrying state + methods; collisions are a *fatal error* unless resolved with
> `insteadof` / `as`; **no** runtime MRO, **no** super-through-linearization).
>
> All "how PHP does X" claims [Verified] against php.net trait docs; all Scala/Eiffel claims
> [Verified] against the cited sources unless flagged [Inferred].

---

## PART 1 — Scala traits compiled to the JVM

### 1.1 What a Scala trait *is* semantically

A Scala trait is **multiple inheritance of implementation**: it can carry concrete method
bodies *and* state (`val`/`var`), and a class may mix in many traits (`class C extends B with T1 with T2`).
This is exactly the capability PHP traits provide — and exactly the capability Phorj S6 wants —
but Scala layers a **deterministic linearization** on top so the diamond never produces ambiguity
or double-execution. [Verified: scala-lang linearization sources]

### 1.2 The lowering target: a single-inheritance JVM

The JVM has **single class inheritance** + **multiple interface implementation**, and (pre-Java-8)
interfaces could hold **no method bodies and no instance fields**. Scala therefore had to *encode*
trait MI onto that constrained target — the identical structural problem Phorj faces lowering onto
PHP. The encoding evolved across three eras:

#### Era A — pre-2.12 (Scala 2.11.x and earlier): interface + `T$class` + virtual forwarders
- Each trait `T` compiled to **two** classfiles:
  1. an **interface** `T` declaring the trait's methods as *abstract* (signatures only), and
  2. a synthetic **`T$class`** holding every concrete method body as a **`static`** method whose
     first parameter is an explicit `$this: T` receiver (e.g. `T$class.foo($this, args)`).
- Every concrete class `C` that mixed in `T` got, for each concrete trait method, a **virtual
  forwarder**: `C.foo(args) { return T$class.foo(this, args); }`.
- Rationale: the interface gives `C` the *type* (`C` is-a `T`); the static `$class` method gives it
  the *implementation*; the per-class forwarder wires the two together because the interface itself
  could not carry a body. [Verified: scala-lang trait-method-performance blog]

#### Era B — 2.12.0-M4 (interim): default methods + `invokespecial` forwarders
- Java 8 lifted the "interfaces can't have bodies" restriction (`default` methods). Bodies moved
  into **default methods on the interface**; subclasses got a virtual method overriding the default
  and forwarding to it via **`invokespecial`**. [Verified: same blog]

#### Era C — 2.12.0-M5 onward (final, current): static body + default-method shim
- Method bodies are emitted as **`static` methods *inside the interface classfile*** (the interface
  may carry static methods since Java 8).
- The interface's **`default` method forwards to that static method**.
- In most cases **no per-subclass forwarder is generated** — the JVM resolves the inherited default
  method directly. Forwarders are re-introduced **selectively** in classes that inherit concrete
  trait methods, purely to recover JVM **startup performance** (default-method resolution was slower
  to warm up), at the cost of larger bytecode. [Verified: scala-lang blog; scala-dev#35]

**Takeaway for Phorj:** the durable trick across all three eras is *separation of TYPE from
IMPLEMENTATION* — emit the trait's contract as an interface (the type), emit the bodies somewhere a
single-inheritance target can reach (static methods / default methods), and **wire them per concrete
class**. PHP already gives us this split for free: `interface` (type) + `trait` (impl) are first-class.

### 1.3 How trait FIELDS / state are handled — the crucial part

A JVM interface **cannot declare instance fields**. Scala's solution (the exact pattern Phorj must
mirror onto PHP):

1. **The trait declares only accessors in the interface — never the field.**
   - An abstract `val x: Int` → one abstract getter `int x();` in the interface.
   - An abstract `var x: Int` → getter `int x();` **and** setter `void x_$eq(int);` in the interface.
   - The Scala compiler **does not emit any field** for an abstract trait member — *only the
     accessor methods*. [Verified: alvinalexander def/val/var-in-traits]
2. **The actual backing field is injected into the concrete implementing class**, not the interface.
   A concrete `val x = 0` in a trait causes the implementing class `C` to get a `private final int x;`
   backing field (mutable / non-`final` for a `var`) plus the public getter/(setter) that satisfy the
   interface's abstract accessors. [Verified: alvinalexander; scala-lang spec 2.12]
3. **Field initialization runs through a static `$init$` on the trait, called from the class
   constructor.** For concrete trait fields the trait emits a static initializer, e.g.
   `public static void $init$(Foo $this) { $this.id_$eq(0); }`, and the implementing class's
   constructor **calls `T.$init$(this)`** to run the trait's field initializers (and any trait body
   side-effects) in linearization order. [Verified: alvinalexander; search result quoting `$init$`]

So a trait's *state* is **physically owned by the concrete class** and *logically* reached only
through accessor methods declared on the interface. The trait never holds storage; it holds a
*protocol* (accessors) + an *initializer* (`$init$`).

### 1.4 Linearization and the `super` ("stackable modifications") semantics

- **Linearization** flattens the class + all mixed-in traits + their ancestors into a single linear
  order. Rule: a **right-first, depth-first** traversal of the `extends … with …` list, then
  **keep only the *last* occurrence** of each type (dedupe-keeping-last). Mixins to the **right**
  (and their ancestors) come **earlier** than those to the left; shared ancestors appear once, as
  late as possible. [Verified: kkyr.io; scala-lang]
- Worked example: `class TA extends Person with Employee with Alumni` →
  `TA → Alumni → Student → Employee → Person → AnyRef → Any`. [Verified: kkyr.io]
- **`super` does NOT mean the lexical parent — it means the *next type in the linearization*.**
  A trait method marked `abstract override def m() { …; super.m(); … }` chains to whatever sits next
  in the *flattened* order, enabling **stackable modifications** (each trait wraps the next).
  Example: `new C with TimestampLogger with UppercaseLogger` runs `Uppercase → Timestamp → base`.
  [Verified: scala-lang stackable-modifications]
- This is encoded in bytecode by resolving each `super.m()` to a **specific, statically-known target**
  (the concrete next-in-line method), so there is **no runtime MRO walk** — the order is computed at
  compile time and burned into the call sites. [Inferred from the static-method/forwarder lowering;
  the call target is fixed at compile time, consistent with the era-C static-method encoding.]

**Diamond resolution:** because every type appears **exactly once** in the linear order, a diamond
(`D` ← `B`,`C` ← `A`) executes `A` **once**, not twice. Scala's answer to the diamond is *dedupe +
total order*, not Eiffel-style explicit selection. The cost: the programmer must *understand* the
linearization to predict which override wins, and silent re-ordering bugs are possible.

---

## PART 2 — Eiffel multiple inheritance (explicit feature-level resolution)

Eiffel is the **gold standard for explicit disambiguation**: there is *no* implicit linearization;
every conflict the compiler can't resolve must be resolved **by the programmer, at the inheritance
site**, with named subclauses. The five **Feature Adaptation** subclauses (all optional, attached to
each parent in the `inherit` clause): `rename`, `export`, `undefine`, `redefine`, `select`.
[Verified: eiffel.org ET-Inheritance]

### 2.1 `rename` — resolve a name clash by giving one parent's feature a new identity
When parents `A` and `B` both expose `foo`, you rename one so both survive under distinct names:
```eiffel
class D inherit
    A rename foo as a_foo end
    B   -- keeps foo
feature ... end
```
A renamed feature **sheds its old identity entirely** — to clients and descendants it is known only
by the new name. This is the most surgical conflict tool: **keep both, name both**. [Verified]

### 2.2 `redefine` / *join* — merge/override colliding features into one
- `redefine` replaces an inherited implementation with a new one in the heir
  (`A redefine deposit end` + a new `deposit` body; `Precursor` calls the parent version).
- **Join**: list several same-named, signature-compatible inherited features in a `redefine` clause
  and provide one new body — they **fuse into a single feature** with no clash. [Verified]

### 2.3 `undefine` — drop one of two colliding features (turn it deferred)
If you inherit `foo` from both `A` and `B`, keep one and **un-effect** the other:
```eiffel
class D inherit
    A
    B undefine foo end   -- B's foo becomes deferred → A's stays effective
feature ... end
```
`undefine` is the inverse of *effecting*; it converts a concrete feature back to deferred so the
other parent's version (or a redefinition) supplies the body. [Verified]

### 2.4 Repeated inheritance, the diamond, and `select`
**Repeated inheritance** = an heir inherits the same ancestor more than once (`D` ← `B`,`C` ← `A`).
Eiffel's default is **sharing**: a feature inherited unchanged along *both* paths is **one feature,
one copy** in `D`. But if a feature was **renamed / redefined differently along the two paths**, it
**replicates** — `D` ends up with *two distinct versions* of what began as one ancestor feature.

`select` exists **only** for the replicated-and-polymorphic case. With two versions present, a
**polymorphic** access (a `B`/`C`/`A`-typed reference attached to a `D` object) is ambiguous: which
version should dynamic binding pick? `select` names the authoritative one:
```eiffel
class AMPHIBIOUS inherit
    CAR
    PLANE select move end   -- polymorphic `move` dispatches to PLANE's version
feature ... end
```
Without `select`, the compiler **rejects** the class — it refuses to guess. [Verified: eiffel.org]

**Key contrast with Scala:** Eiffel forces the programmer to disambiguate *every* genuine conflict
explicitly and *locally* at the inheritance clause; there is no global order to reason about. The
cost is verbosity; the benefit is that the resolution is **written down, auditable, and
order-independent**.

---

## PART 3 — Synthesis for Phorj (interp + VM + transpiled PHP)

### 3.1 Phorj's actual target toolkit (PHP 8.4)
- **One** parent class (`extends`), **many** interfaces (`implements`), **many** traits (`use`).
- A trait carries **methods, abstract methods, static methods, AND properties (state)**.
- **Trait method collision is a FATAL compile error** unless resolved with:
  - `insteadof` — pick exactly one trait's method, exclude the others, and
  - `as` — alias a method to a second name (and/or change its visibility),
  e.g. `use A, B { A::say insteadof B; B::say as sayB; }`. [Verified: php.net traits]
- **PHP has no runtime MRO, no linearization, no trait `super`-chaining.** Trait methods are
  **flattened (copy-pasted) into the using class at compile time** — semantically as if written
  inline. There is no "next trait" to call.
- **Property collision rule (sharp edge):** two traits declaring the **same property** is allowed
  *only* if the declarations are *identical* (same visibility, same initial value); otherwise PHP
  **fatal-errors** (or, historically, warns) — and there is **no `insteadof` for properties**, only
  for methods. This is the spot where Scala's "inject every field into the class" does **not**
  transfer cleanly. [Verified: php.net traits — property conflict]

### 3.2 Does scalac's lowering map onto PHP's (interface + trait) toolkit?

| Scala-on-JVM trick | PHP equivalent | Fit |
|---|---|---|
| Trait *type* = interface | PHP `interface` | **Clean** — emit each Phorj trait/parent's *contract* as an `interface`. |
| Trait *impl* = static methods / default methods, wired per class | PHP `trait` (flattened into the class) | **Clean** — PHP traits *are* the "inject implementation into the concrete class" mechanism, done by the engine. We don't even need forwarders; PHP copies the bodies in. |
| Trait *state* = abstract accessors in interface + **backing field injected into the concrete class** + `$init$` from ctor | PHP `trait` properties are **also injected into the using class** | **Mostly clean, with one collision hazard** — see below. |
| `super` = next-in-linearization | *(no PHP analogue)* | **Does NOT map** — PHP has no trait `super`/MRO. |

**Where field state collides.** Scala dodges field collisions because every trait field is reached
only through *accessors*, and the *class* owns one backing field per logical member — name clashes
surface as accessor-signature clashes, resolvable. PHP injects the **raw property** into the using
class, so **two traits with a same-named property are an unresolvable fatal** (no `insteadof` for
properties; identical-declaration is the only escape). Therefore:

- **If Phorj lowers trait state as raw PHP trait properties**, a diamond/collision on *state* is a
  hard PHP error with no resolution clause — worse than methods.
- **The Scala-faithful fix is to lower trait state the way scalac does:** declare an **abstract
  accessor pair in the emitted interface** (`function x(): T; function setX(T $v): void;`) and put a
  **single backing field + the concrete accessors into the *final concrete class*** (or into one
  canonical trait), so cross-trait state is reached through methods (which *do* have `insteadof`/`as`)
  and never as two colliding raw properties. This converts an un-resolvable property clash into a
  resolvable *method* clash. [Inferred — direct application of §1.3 onto PHP's property rule.]

### 3.3 Could Phorj adopt an Eiffel-style explicit clause that lowers to PHP `insteadof`/`as`?

Yes — and it is an *almost 1:1* structural match, which is the strongest finding here:

```
// Phorj surface (Eiffel-flavoured, PHP-lowerable)
class C extends Base implements I uses A, B {
    rename A.foo as aFoo;   // Eiffel `rename`  → PHP  A::foo as aFoo;
    use    B.foo;           // Eiffel `select`  → PHP  B::foo insteadof A;  (pick the winner)
}
```
- Eiffel **`rename A.foo as aFoo`** ⇒ PHP **`A::foo as aFoo;`** — *exact* semantic and almost-exact
  syntactic correspondence (keep both, name both).
- Eiffel **`select`** (which version a polymorphic call resolves to) ⇒ PHP **`insteadof`** (which
  trait supplies the canonical method) — same *intent* (designate the authoritative version),
  different trigger (Eiffel's is polymorphism-driven; PHP's is collision-driven). The mapping holds.
- Eiffel **`undefine`** ⇒ in PHP, *just don't `use` that trait's method* / `insteadof` it away.
- Eiffel **`redefine`/join** ⇒ override the method in `C` itself (PHP: a method declared in the
  class body **wins over** any trait method automatically — class > trait > inherited precedence).

**Assessment: strong fit.** An Eiffel-style explicit `rename`/`select` clause at the inheritance site
lowers **directly and statically** to PHP `as` / `insteadof`. There is no order to compute, no MRO to
reproduce — the resolution is a **local, named, compile-time rewrite**. This is *exactly* the kind of
front-end-only transform Phorj already uses (alias expansion, generic erasure): resolve at the
checker/loader, emit explicit `insteadof`/`as`, and **all three backends see an already-disambiguated
program**.

### 3.4 The verdict: Eiffel-style explicit > Scala-style linearized, for the byte-identical spine

**Pick Eiffel-style explicit `rename`/`select` resolution. Reject Scala-style linearized `super`.**

Reasoning, weighted by Phorj's *defining* constraint (byte-identical interp ≡ VM ≡ generated PHP):

1. **Reproducibility across three backends.** Explicit resolution is a **static rewrite** with a
   single, written-down answer per conflict. The interpreter, the VM, and the transpiler each see the
   *same already-resolved* call target — there is nothing to re-derive at runtime, so divergence is
   structurally impossible (the same reason alias-expansion and generic-erasure are spine-safe).
   Scala-style linearization requires every backend to **compute the identical linear order and the
   identical `super`-chain target** independently; any discrepancy between the interpreter's walk, the
   VM's encoding, and what PHP would do is a silent byte-identity break — and **PHP has no
   linearization to match against at all**, so the transpiled leg could never reproduce a Phorj MRO
   without emitting a hand-rolled dispatch shim (defeating "idiomatic PHP"). [Verified: PHP has no
   MRO/trait-super; §3.1.]

2. **No new runtime mechanism, no new `Op`.** Explicit resolution lives entirely in the front end
   (checker + loader name-mangling/rewrite pass, mirroring the existing cross-package mangle). The
   backends consume a flattened, conflict-free program — **zero `Op` additions, zero `Value`
   changes** — the cheapest possible landing and the one most consistent with every prior Phorj
   slice. Scala-style `super`-chaining would force a runtime "next-in-linearization" dispatch the VM
   and interpreter must both implement *and* the transpiler must fake in PHP.

3. **Maps onto PHP's real tool.** PHP *only* offers explicit (`insteadof`/`as`) conflict resolution —
   it has no implicit ordering to lean on. Eiffel's model lowers **directly** to that tool; Scala's
   does not lower at all without a synthetic dispatcher.

4. **Philosophy fit ("legible, no surprises, familiarity-first").** Eiffel's "you must disambiguate
   explicitly, at the site, by name" is *more legible* and *less surprising* than "memorise the
   right-to-left dedupe-keeping-last rule to predict which override wins" — and it is the model a PHP
   developer **already knows** (`insteadof`/`as`). Phorj's job is to be a provably-correct *upgrade*
   of PHP, not to import JVM-trait mental overhead.

**The one Scala idea worth keeping** is the **field-lowering technique** (§3.2): accessor-in-interface
+ backing-field-in-class. That is orthogonal to the resolution model and is the clean fix for PHP's
un-resolvable *property* collisions. Use Eiffel's resolution model **and** Scala's state-lowering
trick together.

---

## Quick-reference summary

- **Scala lowering (current, era C):** trait → interface (type) + static method bodies in the
  interface + default-method shims; selective per-class forwarders only for JVM startup perf.
- **Scala state:** abstract accessors in the interface; backing field + concrete accessors injected
  into the implementing class; `$init$` static run from the class ctor in linearization order.
- **Scala `super`:** resolves to the *next type in the linearization* (right-first DFS, dedupe-keep-
  last), enabling stackable modifications; diamond runs the shared ancestor exactly once.
- **Eiffel:** `rename` (keep both, name both) · `redefine`/join (merge/override) · `undefine` (drop
  one) · `select` (designate the polymorphic winner under replicated repeated inheritance). No
  implicit order — every real conflict is resolved explicitly at the inheritance clause.
- **PHP 8.4:** single `extends`, many `implements`, many `use` (traits flattened, methods + state);
  method collision = fatal unless `insteadof`/`as`; property collision = fatal unless identical
  declarations; **no MRO, no trait `super`**.
- **Phorj verdict:** adopt **Eiffel-style explicit `rename`/`select`** lowering to PHP `as`/`insteadof`
  (front-end-only, byte-identity-safe, no new `Op`); borrow **Scala's accessor+injected-backing-field**
  state lowering to dodge PHP's un-resolvable property collisions; **reject Scala linearized `super`**
  (no PHP analogue, requires per-backend MRO reproduction = silent spine break).

## Sources
- Performance of trait methods — https://www.scala-lang.org/blog/2016/07/08/trait-method-performance.html
- scala/scala-dev#35 (default methods in trait encoding) — https://github.com/scala/scala-dev/issues/35
- Scala 2.12.0 release notes — https://www.scala-lang.org/news/2.12.0/
- def/val/var fields in Scala traits, decompiled — https://alvinalexander.com/scala/all-wanted-to-know-about-def-val-var-fields-in-traits/
- Linearization in Scala — https://kkyr.io/blog/linearization-in-scala/
- Scala stackable modifications / abstract override — https://www.lorrin.org/blog/2012/08/09/scalas-abstract-override-stackable-traits-and-object-hierarchy-linearization/
- Scala 2.12 spec, classes & objects — https://www.scala-lang.org/files/archive/spec/2.12/05-classes-and-objects.html
- Eiffel ET: Inheritance — https://www.eiffel.org/doc/eiffel/ET-_Inheritance
- Eiffel Inheritance (solutions) — https://www.eiffel.org/doc/solutions/Inheritance
- Harnessing multiple inheritance (Meyer) — https://archive.eiffel.com/doc/manuals/technology/bmarticles/joop/multiple.html
- PHP: Traits manual — https://www.php.net/manual/en/language.oop5.traits.php
