# C++ Multiple Inheritance — Research for Phorge S6

> Research target: extract the C++ MI object model, the diamond problem, name-resolution
> semantics, constructor ordering, and the footguns — then synthesise what is worth stealing
> for Phorge, whose transpile target (PHP 8.4) is **single-inheritance + interfaces + traits**.
> Sources: C++ standard semantics (`[class.derived]`, `[class.mi]`, `[class.member.lookup]`,
> `[class.base.init]`), the Itanium C++ ABI (the de-facto layout/vtable spec on Linux/macOS),
> and Stroustrup's design rationale (*The Design and Evolution of C++*, §12).

---

## 1. Object model — how a multiply-inheriting object is laid out

### 1.1 Non-virtual (ordinary) multiple inheritance

```cpp
struct A { int a; void fa(); };
struct B { int b; virtual void fb(); };
struct C : A, B { int c; };
```

A `C` object is laid out as a concatenation of **base subobjects** in declaration order, then the
derived members:

```
offset 0:  [ A subobject ]   { a }
offset N:  [ B subobject ]   { vptr_B, b }   <-- B has virtuals → its own vptr
offset M:  [ C's own members ] { c }
```

Key facts:

- Each base subobject occupies a **contiguous range** inside the derived object and has its own
  **offset** (`A` at 0, `B` at some non-zero offset).
- A base that has virtual functions carries a **vtable pointer (vptr)**. With MI, an object can hold
  **multiple vptrs** — one per polymorphic base that does not share layout with the primary base.
  The *primary base* (first polymorphic base, usually the first declared) shares the derived class's
  vptr at offset 0; secondary polymorphic bases get their own vptr at their subobject offset.
- **Pointer adjustment (`this`-adjustment) is the defining cost of MI.** Converting
  `C*` → `B*` is **not** a no-op: the compiler adds the constant offset of the `B` subobject.
  Converting `C*` → `A*` *is* a no-op (offset 0). Casting back (`B*` → `C*` via `static_cast` or a
  base-to-derived path) subtracts the offset. A `nullptr` is special-cased (adjustment is skipped so
  null stays null).
- **Thunks**: when you call a virtual function through a `B*` that is actually overridden in `C`,
  the `B`-subobject vtable slot does not point directly at `C::f` — it points at a compiler-generated
  **thunk** that first subtracts the `B`-offset to recover the real `C*` (`this`), then jumps to
  `C::f`. This is the "non-virtual thunk". (Virtual bases use a "virtual thunk" that reads the offset
  from the object at runtime — see §1.2.)

So the MI object model is: **N base subobjects at static offsets, up to N vptrs, and pointer
conversions that add/subtract a compile-time constant.** Single inheritance keeps everything at
offset 0, so up-casts are no-ops and there is exactly one vptr — that simplicity is exactly what MI
sacrifices.

### 1.2 Virtual base classes — the diamond fix

```cpp
struct Base   { int x; };
struct Left   : virtual Base { int l; };
struct Right  : virtual Base { int r; };
struct Diamond: Left, Right    { int d; };
```

With `virtual` inheritance, **all paths to `Base` share a single `Base` subobject**. But a shared
subobject can no longer sit at a fixed offset relative to every intermediate class (because `Left`
alone, `Right` alone, and `Diamond` each place `Base` at a *different* relative position). So the
compiler cannot bake the `Base` offset into `Left`'s or `Right`'s code as a constant.

The Itanium ABI solution: each subobject that has virtual bases carries, in its vtable, a
**vbase offset** — the runtime distance from "here" to the shared virtual-base subobject. Accessing
a virtual base member compiles to: *load vptr → load the vbase-offset entry → add it to `this`*.
A typical `Diamond` layout:

```
[ Left subobject:  vptr_Left,  l ]
[ Right subobject: vptr_Right, r ]
[ Diamond's own:   d ]
[ Base subobject (shared, placed last): x ]
```

The vtables for `Left` and `Right` (as embedded in `Diamond`) contain a **vbase-offset slot** whose
value points at that single trailing `Base`. **Virtual thunks** for overridden virtuals reachable
through a virtual base read this runtime offset to adjust `this` rather than using a constant.

So virtual inheritance costs: an extra vtable slot per polymorphic-virtual-base relationship, an
extra indirection on every virtual-base member access, and slower up-casts (`Diamond*` → `Base*`
becomes a runtime offset lookup, not a constant add). This is the runtime price of the diamond fix.

---

## 2. The diamond problem

### 2.1 What goes wrong without `virtual`

```cpp
struct Base   { int x; };
struct Left   : Base {};   // non-virtual
struct Right  : Base {};   // non-virtual
struct Diamond: Left, Right {};
```

`Diamond` now contains **two distinct `Base` subobjects** — `Left::Base` and `Right::Base`. State is
**duplicated**: there are two independent `x` fields. Consequences:

- `d.x` is **ambiguous** → compile error. You must write `d.Left::x` or `d.Right::x`.
- `Base* p = &d;` is **ambiguous** → compile error (which `Base` subobject?).
- Worse than the diagnostics: it is usually a **semantic bug**. Most diamonds model "is-a" where the
  shared base is meant to be *one* thing (one `Object` identity, one `iostream` buffer). Two copies
  silently break invariants the programmer assumed were singular.

### 2.2 How `virtual` fixes it, and the residual cost

`virtual` inheritance collapses the two `Base` subobjects into one (§1.2). Now `d.x` is unambiguous
(one field), and `Base* p = &d;` is well-defined. Cost recap:

- Layout: shared virtual base relocated (typically to the end), reachable only via runtime offset.
- Access: every virtual-base member touch costs an extra load+add.
- Up-cast to a virtual base: runtime offset lookup, not a constant.
- Construction: the **most-derived class** is responsible for constructing the shared virtual base
  (see §4) — a non-obvious rule that surprises everyone.

The deep lesson: MI's "correctness" hinges on the programmer choosing `virtual` vs non-virtual
*per base* — a low-level decision with global, hard-to-reverse consequences.

---

## 3. Name resolution / ambiguity — the model worth stealing

C++ **does not silently pick a winner** when a member name exists in two bases. The rule
(`[class.member.lookup]`) is roughly:

1. Build the lookup set by searching the inheritance DAG.
2. If the name resolves to **declarations in two different, unrelated subobjects**, the lookup is
   **ambiguous** — a **compile-time error**. No precedence, no MRO, no "first base wins".
3. The programmer resolves it **explicitly** with qualified-id syntax: `obj.Base::member`,
   `Base::method()`, or a *using-declaration* (`using Left::f;`) that hoists one base's member into
   the derived class to make it the unambiguous choice.

```cpp
struct A { void f(); };
struct B { void f(); };
struct C : A, B {};

C c;
c.f();        // ERROR: ambiguous
c.A::f();     // OK — explicit disambiguation
c.B::f();     // OK
```

This is the single most important takeaway for Phorge. C++ refuses to invent a resolution order
(contrast Python's C3 MRO, which *does* linearise and silently pick). The C++ stance —
**ambiguity is an error, you disambiguate by naming the source explicitly** — is:

- **Deterministic** (no MRO algorithm to reimplement three times).
- **Local** (the error and the fix are at the conflict site).
- **Backend-agnostic** (it is a *front-end* decision — once resolved, the chosen member is a single
  concrete target; nothing about the choice is runtime-shaped).

### Parallel: PHP traits already do exactly this

PHP traits are the language's MI-of-implementation, and they enforce **the same model**:

```php
trait A { public function f() { /* ... */ } }
trait B { public function f() { /* ... */ } }

class C {
    use A, B {
        A::f insteadof B;   // pick A's f as C::f  (the "winner")
        B::f as g;          // and expose B's f under a new name g  (alias)
    }
}
```

- A trait method-name collision is a **fatal compile error** unless resolved.
- `insteadof` chooses which trait's member wins (explicit, like a C++ using-declaration).
- `as` **aliases** the other one under a new name (and may also change visibility).

So C++'s "ambiguity = error, disambiguate explicitly" maps **directly** onto PHP's
`insteadof` / `as`. That is the bridge for Phorge.

---

## 4. Constructor (initialization) ordering

C++'s initialization order (`[class.base.init]`) is fixed and **independent of the order written in
the member-initializer list** (a famous footgun — the list order is ignored, the declaration order
wins):

1. **Virtual base classes first**, constructed by the **most-derived object**, in the order they
   appear in a depth-first left-to-right traversal of the base DAG. (Intermediate classes' attempts
   to initialise a virtual base are *ignored* when they are not the most-derived class — only the
   final, complete object initialises each shared virtual base, exactly once.)
2. **Direct non-virtual base classes**, in **left-to-right declaration order** (`: A, B` → `A` then
   `B`), each recursively constructing its own bases first.
3. **Non-static data members**, in **declaration order**.
4. The constructor **body**.

Destruction is the **exact reverse**.

Two specific surprises:
- Initializer-list order is cosmetic; the compiler reorders to declaration order (warned by
  `-Wreorder`).
- For a virtual base, **only the most-derived constructor's initializer runs**; every intermediate
  class that "also" initialises it is silently skipped. So where a shared virtual base gets its
  arguments depends on the *complete* object type, not the local class — non-obvious and fragile.

For Phorge: the *principle* worth keeping is a **single, deterministic, documented init order**.
The *mechanism* (most-derived-constructs-virtual-base) is a diamond-state artifact Phorge should
avoid needing at all (see §6).

---

## 5. The footguns — why C++ MI is infamous

1. **Diamond state duplication** (§2.1) — silent two-copies-of-base bug unless you remember
   `virtual`; the decision is per-base and global.
2. **Per-base `virtual` choice is irreversible & viral** — making a base virtual changes layout,
   `this`-adjustment, who constructs it, and the cost of every access. Retrofitting it onto an
   existing hierarchy is a breaking ABI change.
3. **Initialization-order surprises** (§4) — declaration order vs init-list order; most-derived
   constructs virtual bases.
4. **Object slicing** — `Base b = derived;` copies only the `Base` subobject, silently dropping the
   rest (and any vptr fix-up), discarding polymorphism. MI multiplies the ways this bites.
5. **vtable / layout complexity** — multiple vptrs, thunks, vbase-offset indirections;
   `this`-pointer adjustment makes pointer identity subtle (`(void*)(A*)p != (void*)(B*)p` for the
   same object). `dynamic_cast` and `reinterpret_cast` interact badly with MI offsets.
6. **Dominance & lookup subtleties** — `[class.member.lookup]` has special "dominance" rules for
   virtual bases that even experts get wrong; ambiguity errors can appear/disappear when an
   unrelated base gains a member.
7. **Fragile base class** — adding a member to a base can introduce an ambiguity in a derived class
   far away; layout changes ripple through the ABI.

The cultural verdict (Stroustrup, Java's deliberate omission, Go's rejection, Rust's
trait-only model): **MI of *implementation/state* is rarely worth its cost; MI of *interface* is
both safe and useful.** This is the line nearly every modern language draws — and it is exactly the
line PHP draws (single class inheritance + many interfaces + traits for code reuse).

---

## 6. Synthesis for Phorge

### Phorge's constraints (the lens)

- Three backends must stay **byte-identical**: tree-walking interpreter, stack VM, Phorge→PHP
  transpiler. Anything chosen must reproduce **identically** across all three.
- Transpile target **PHP 8.4** is **strictly single-inheritance** for classes, with **interfaces**
  (many, interface-MI is allowed) and **traits** (`use A, B;`, conflict = fatal error unless
  `insteadof`/`as`).
- Phorge's heap is `Rc`-shared and (pre-mutation milestone) immutable + acyclic; object model is
  value-native. There is **no C++-style raw pointer / offset layout** — fields are looked up by
  name/slot, not by base-subobject offset.

### Which C++ MI idea is WORTH STEALING

1. **"Ambiguity is a compile error; disambiguate explicitly" (§3).** This is the crown jewel. It is:
   - **front-end-only** — resolution happens entirely in the checker; once a member is resolved to a
     single concrete target, the three backends see one unambiguous call. **No runtime divergence
     possible**, because the backends never observe the conflict — they observe the *resolution*.
   - **deterministic without an MRO** — Phorge does **not** need to implement (and triple-implement,
     and keep byte-identical) a C3 linearisation. "Error unless explicitly named" needs *zero*
     runtime algorithm.
   - **already PHP-native** — it lowers cleanly to PHP trait `insteadof` / `as` (§3 parallel).

2. **A single, documented, deterministic initialization order (§4).** Worth keeping the *principle*
   (one fixed order, reverse on teardown), not C++'s diamond-specific machinery.

3. **MI of *interface* is free and good.** Phorge already has this (S2 interfaces + `implements`,
   S5 intersections). Keep leaning on it; it transpiles 1:1 to PHP `interface`/`implements`.

### Which C++ MI idea is a TRAP (avoid)

1. **Shared mutable diamond state via `virtual` bases.** This requires runtime offset indirection
   and "most-derived constructs the base" — concepts with **no PHP target** and no clean mapping to
   Phorge's value-native, name/slot-addressed object model. Reproducing vbase-offset semantics
   identically across interp + VM + PHP is not feasible. **Do not model state-MI.**
2. **Per-base `virtual`/non-virtual choice.** A low-level, viral, layout-coupled knob. Phorge should
   not expose a "is this base shared or duplicated?" decision at all.
3. **`this`-pointer adjustment / multiple vptrs / thunks.** Pure C++-ABI artifacts; irrelevant and
   un-transpilable. Phorge resolves members by name, so there is nothing to adjust.
4. **Initializer-list-order-ignored surprise.** If Phorge ever has init lists, make written order =
   actual order (no silent reorder).

### Can the C++ model be reproduced identically across interp + VM + PHP?

**The state/layout half: no.** Multiple base subobjects, vptrs, `this`-adjustment, and virtual-base
offsets are C++-ABI machinery with no PHP analogue and no value-native mapping. Trying to mirror it
would create exactly the run↔runvm↔PHP divergence Phorge's spine forbids.

**The name-resolution half: yes, cleanly and completely.** Because it is a **front-end** decision:
the checker detects a name present in two trait/mixin sources, **errors** unless the program
disambiguates (`insteadof`-equivalent to pick a winner, `as`-equivalent to alias the other), and
**rewrites the AST to a single concrete target before any backend runs** — the identical discipline
Phorge already uses for `type` aliases, generic erasure, and `html"…"` lowering. After that rewrite:
- the **interpreter** dispatches to one method,
- the **VM** compiles one call target,
- the **transpiler** emits a PHP `use T { Winner::m insteadof Other; Other::m as alias; }` block.

All three observe the *resolved* program, never the conflict → byte-identity is **structural**, not
something to test for. This is the same reason interface-MI and intersection types were free.

### The concrete recommendation: `parent-as` / explicit-source qualification → PHP `as`

C++'s `obj.Base::member` (name the source) maps directly onto a Phorge syntax that names the
trait/mixin source and lowers to PHP trait aliasing. A sketch:

- **Conflict policy:** a method-name collision across two mixed-in sources is `E-MIXIN-CONFLICT`
  (fatal) — exactly C++/PHP behaviour, no silent winner.
- **Pick a winner:** `use A, B { A.f insteadof B; }` (or whatever surface syntax S6/S8 settles on)
  → PHP `A::f insteadof B`.
- **Alias the loser:** `use A, B { B.f as g; }` → PHP `B::f as g`. A call to the source-qualified
  name (`B.f` analog) lowers to the aliased PHP method `g`. This is the literal `parent-as.member`
  bridge the brief asks about.

This gives Phorge **C++'s explicit-disambiguation ergonomics** with **PHP traits as the lowering
target** and **zero runtime/spine risk**, because the whole mechanism is erased to a single concrete
PHP construct in the front end.

### Bottom line

Steal the **discipline** (ambiguity = error + explicit source qualification; deterministic init
order; interface-MI). Reject the **machinery** (virtual bases, shared duplicated state, vptr/offset
layout, per-base virtual choice). The stealable half is exactly the half that lives in the front end
and lowers to PHP traits — i.e. the half that is byte-identity-safe by construction. Phorge's MI
story should be **traits/mixins with explicit conflict resolution**, *not* C++-style state MI —
which is the same conclusion PHP, Java, Go, and Rust all reached.
