# Python C3 Linearization (MRO) & Ruby Mixins — research for Phorge S6 multiple inheritance

> Topic: the two leading dynamic-language linearization models, and whether Phorge can
> reproduce them across its three byte-identical backends (interpreter / VM / PHP transpile,
> target PHP 8.4: single-inheritance + traits, `insteadof`/`as` conflict resolution, **no MRO, no
> linearization-aware `super`**).
>
> All algorithmic claims below were verified empirically with `python3` 3.14.6 and `ruby` 4.0.5
> on this machine — outputs are quoted inline and marked **[Verified]**.

---

## 1. Python C3 MRO

### 1.1 The algorithm

The MRO ("method resolution order") of a class `C` is a single total order over `C` and **all**
its ancestors. Python computes it with the **C3 linearization** (Barrett et al. 1996, adopted in
Python 2.3). Definition:

```
L[C]  =  C  +  merge( L[P1], L[P2], …, L[Pn],  [P1, P2, …, Pn] )
```

where `P1…Pn` are `C`'s direct bases **in the order written in the class statement**, `L[Pi]` is
the already-computed linearization of base `Pi`, and the final `[P1…Pn]` is the literal list of
direct bases.

`merge` consumes the input lists head-by-head:

1. Look at the **head** of the first list (`L[P1][0]`).
2. A head is a **good head** if it does **not** appear in the **tail** (anything after position 0)
   of *any* remaining list.
3. If it's a good head: append it to the result, remove it from the front of every list it heads,
   restart from step 1.
4. If it's not a good head: try the head of the **next** list. (This is the only place precedence
   "looks ahead".)
5. If no list offers a good head and lists remain non-empty → **C3 failure** ("Cannot create a
   consistent method resolution order").

The two invariants C3 guarantees:

- **Local precedence order** — a class always precedes its own bases, and bases appear in the order
  they were listed (`class D(B, C)` ⇒ `B` before `C`).
- **Monotonicity** — if `X` precedes `Y` in the linearization of some class, `X` precedes `Y` in the
  linearization of every subclass. (No descendant may reorder an ancestor pair.) This is exactly what
  a naive depth-first or breadth-first walk fails to guarantee, which is why C3 replaced the old
  Python-2.2 DFS.

### 1.2 Worked diamond — D(B,C), B(A), C(A)  **[Verified]**

```python
class A:               def who(self): return "A"
class B(A):            def who(self): return "B->" + super().who()
class C(A):            def who(self): return "C->" + super().who()
class D(B, C):         def who(self): return "D->" + super().who()
```

Hand computation:

```
L[A] = [A, object]
L[B] = B + merge(L[A], [A]) = [B, A, object]
L[C] = C + merge(L[A], [A]) = [C, A, object]
L[D] = D + merge(L[B], L[C], [B, C])
     = D + merge([B,A,object], [C,A,object], [B,C])
       head B: not in any tail → take B
     = D, B + merge([A,object], [C,A,object], [C])
       head A: A IS in the tail of [C,A,object] → bad head; try next list
       head C: not in any tail → take C
     = D, B, C + merge([A,object], [A,object], [])
       head A: good → take A
     = D, B, C, A + merge([object],[object]) = D, B, C, A, object
```

Result and runtime behaviour (verified):

```
MRO(D): ['D', 'B', 'C', 'A', 'object']
D().who():  D->B->C->A
MRO(B): ['B', 'A', 'object']     # B standalone: super() in B -> A
```

**The crux to internalise:** `B.who` contains `super().who()`. When called on a `D` instance,
`super()` does **not** go to `B`'s static parent `A` — it goes to **the next class after `B` in
`D`'s MRO**, which is **`C`**. The same `B.who` source, called on a standalone `B`, sends `super()`
to `A`. *The meaning of `super` is not lexical; it is a function of the runtime instance's MRO.*

### 1.3 C3 failure  **[Verified]**

```python
class X: pass
class Y: pass
class K1(X, Y): pass    # demands X before Y
class K2(Y, X): pass    # demands Y before X
class Z(K1, K2): pass   # -> TypeError
```

```
C3 FAIL: Cannot create a consistent method resolution order (MRO) for bases X, Y
```

`K1` fixes `X≺Y`, `K2` fixes `Y≺X`; no total order satisfies both monotonically, so `merge`
deadlocks (every remaining head appears in some tail) and C3 raises. Inconsistent hierarchies are
**rejected at class-creation time**, before any instance exists — this is a static, total check.

### 1.4 Cooperative-super "runs each base once" property  **[Verified]**

```python
class Base:     def __init__(self):              self.log = ["Base"]
class M1(Base): def __init__(self): super().__init__(); self.log.append("M1")
class M2(Base): def __init__(self): super().__init__(); self.log.append("M2")
class Combined(M1, M2):
                def __init__(self): super().__init__(); self.log.append("Combined")
```

```
Combined MRO: ['Combined', 'M1', 'M2', 'Base', 'object']
init log:     ['Base', 'M2', 'M1', 'Combined']
```

`Base.__init__` runs **exactly once** despite being reachable through both `M1` and `M2`. This is
*the* feature C3+super buys you and the one PHP traits structurally cannot reproduce: a diamond
shared base is initialised once, in a deterministic order, with each mixin getting a chance to wrap.

---

## 2. Ruby — mixins, the ancestor chain, `super`

Ruby has **single class inheritance** but composes behaviour by mixing **modules** into a linear
**ancestor chain**. There is no separate "MRO algorithm name" — the chain is built by insertion
rules, and method lookup walks it left-to-right.

### 2.1 `include` vs `prepend`  **[Verified]**

```ruby
module M1; def who; "M1->" + (defined?(super) ? super : "end"); end; end
module M2; def who; "M2->" + (defined?(super) ? super : "end"); end; end
class Base; def who; "Base"; end; end
class C < Base
  include M1
  include M2          # LATER include = HIGHER precedence (inserted nearer the class)
  def who; "C->" + super; end
end
```

```
C.ancestors:  [C, M2, M1, Base, Object, Kernel, BasicObject]
C.new.who:    C->M2->M1->Base
```

- **`include M`** inserts `M` into the ancestor chain **immediately after the class itself** (between
  the class and its superclass). Multiple includes stack: the **most recently included module wins**
  (sits closer to the class), so include order is the precedence lever — opposite reflex from a
  parameter list. (`include M1; include M2` ⇒ `…, M2, M1, …`.)
- **`prepend M`** inserts `M` **before the class** in the chain:

```ruby
class D < Base
  prepend Logger      # Logger sits in FRONT of D
  def who; "D->" + super; end
end
```

```
D.ancestors:  [Logger, D, Base, Object, Kernel, BasicObject]
D.new.who:    Logger->D->Base
```

`prepend` lets a module intercept and wrap the class's *own* methods (decorator / around-advice
pattern) — `Logger#who` runs before `D#who` and `super` falls through to `D#who`.

### 2.2 `super` semantics

Identical *mechanism* to Python: `super` (bare or with args) calls the **next entry in the ancestor
chain after the class where the currently-executing method was found** — not the lexical superclass.
So a module's `super` resolves into whatever sits below it in the *including* class's chain, which is
unknown when the module is written. Bare `super` forwards the current arguments; `super()` passes
none. A diamond is impossible to form by accident because Ruby has single class inheritance and a
module included twice is inserted only once (deduplicated), so the chain stays linear and acyclic.

### 2.3 How Ruby differs from C3

Ruby does **not** run C3. It builds the chain by a simple, deterministic insertion rule (class →
prepended modules in front, included modules behind, recursing into each module's own includes, with
de-duplication keeping the first occurrence). Because class inheritance is single, the monotonicity
crises that motivate C3 essentially can't arise — the only ordering freedom is module-insertion
order, which the programmer controls explicitly. Net: a simpler model that achieves the same
*outcome* (a single linear, acyclic ancestor list + cooperative `super`) without needing C3's merge.

---

## 3. The cooperative-super pattern — why it's powerful, why it needs a runtime list

C3 (or Ruby's chain) produces a **single total order over all ancestors**. Cooperative `super` then
means: *each method body calls the **next** body in that order.* This composes mixins into a chain
where every participant runs once, in a predictable sequence, and any one of them can wrap, short-
circuit, or augment the rest (`before`/`after`/`around` advice falls out for free). A shared diamond
base runs exactly once (§1.4). This is what makes Python/Ruby mixins genuinely *composable* rather
than merely *copied-in*.

The hard dependency: **`super` is resolved against the instance's runtime ordered ancestor list, not
the lexical class graph.** "Next after the class where this method was found" is only answerable if
(a) there exists one canonical linear order, and (b) the dispatch machinery can locate the *current*
position in it and step forward. Python carries `__mro__` on the type and `super` captures
`(class-where-defined, instance-type)` to index into it; Ruby walks `ancestors`. Remove the runtime
ordered list and "next" has no referent. **This is the entire difficulty of lowering it to PHP.**

---

## 4. Synthesis for Phorge

Phorge has an advantage the dynamic languages lack: a **fully static class graph** known at compile
time. The whole linearization is computable ahead of time. The question is what to *do* with it, and
the two sub-cases diverge sharply.

### 4.1 Can Phorge compute C3 at compile time and flatten?

**Yes for the order; the consequence depends on whether `super` is cooperative.**

C3 is a pure function of the class graph — Phorge can run the identical merge in the checker over
`ClassDecl` parents, reject inconsistent hierarchies with a clean diagnostic (the `K1/K2` case →
`E-MRO-INCONSISTENT`), and store the resulting `Vec<ClassId>` linearization on each class. This is a
front-end-only computation, **no new `Op`, no `Value` change** — exactly the discipline the project
already uses for `class_implements`, `erase_generics`, alias expansion. All three backends can be
handed the *same* precomputed `Vec` so they agree by construction. That part is cheap and safe.

What you do with the order splits into (a) and (b).

### 4.2 Case (a): C3 to pick the winner, NO cooperative super  — **EASY, RECOMMENDED**

If `super` is *not* linearization-aware (it means the lexical/declared parent only, as in PHP/Java),
then C3 is used purely as a **conflict-resolution tie-breaker**: when a method name is defined by
several ancestors, the one appearing **earliest in the C3 order wins**, and that single body is the
method.

- **Interpreter / VM:** for each `(class, method-name)`, resolve once at compile time to the winning
  body via the precomputed order; build a flat per-class method table. Dispatch is an ordinary table
  lookup — *no runtime MRO walk at all.* The two Rust backends consume the same table ⇒ byte-identical
  by construction.
- **Transpiler → PHP:** trivial. Each Phorge class becomes a PHP class whose method set is the
  C3-resolved flat set (inline the bodies, or model each mixin as a PHP `trait` and let
  `insteadof`/`as` encode the C3 winner — PHP's trait conflict resolution is *manual* but here Phorge
  generates the resolution from the computed order, so it's deterministic). No forwarding methods, no
  `super` emulation. Shared-base "diamond" state is just fields merged into the one PHP class.
- **`super`/`parent::`** keeps PHP's familiar single-parent meaning. No surprise for a PHP audience.

This is the **Phorge-philosophy-aligned** choice: it removes the surprise (ambiguous MI) without
adding a runtime mechanism PHP doesn't have, and it lowers to idiomatic PHP. **Verdict: adopt.**

### 4.3 Case (b): C3 + cooperative super — **POWERFUL BUT HARD TO LOWER; defer/avoid for the PHP target**

If `super` must mean "next body in the C3 linearization" (the Python/Ruby semantics), the Rust
backends can do it cleanly — but PHP cannot, and the transpiler emulation is fragile.

- **Interpreter + VM (the Rust spine): feasible and they CAN agree byte-for-byte.** Give every method
  body an implicit "MRO cursor": a `super` call dispatches to the next entry after the
  defining-class's position in the *instance's* precomputed linearization. Both backends walk the
  **same** precomputed `Vec<ClassId>` with the same cursor rule, so `run≡runvm` holds. This is the
  same parity discipline already proven for re-entrant higher-order natives
  (`call_closure_value`/`run_until`): a shared kernel driving shared control flow. So the spine itself
  is reproducible.

- **The PHP transpiler is the wall.** PHP has *no* MRO and *no* linearization-aware `super`:
  `parent::m()` is the single declared parent, full stop. To emulate cooperative super you must
  **synthesize forwarding plumbing** per class, e.g.:
    - flatten each ancestor's method body into a uniquely-renamed PHP method
      (`__phorge_mro_B_who`, `__phorge_mro_C_who`, …) on the final concrete class;
    - rewrite every `super.who()` inside those bodies into an explicit call to the *next* renamed
      method **for this specific concrete class's linearization** — i.e. the same source method gets a
      *different* forwarding target per concrete subclass it ends up in (because the MRO differs), so
      you cannot emit one shared body; you must **monomorphize each mixin body per concrete MRO
      context** and thread an explicit cursor;
    - reproduce the "shared base runs once" diamond guarantee by routing through the synthesized chain
      rather than letting two paths both reach the base.

  This is real codegen complexity with multiple fragility points: per-concrete-class body duplication
  (code-size blow-up), name-mangling collisions with PHP builtins (the project already hit
  `serialize`→`serialize_response`), `private`/visibility mismatches (PHP enforces what the Phorge
  backends don't — the project already logged this), and constructor-chain ordering for promoted
  fields. It is *buildable* but it is exactly the kind of "emit a runtime PHP doesn't have" that the
  project has consistently chosen to avoid, and every divergence is a `run`↔transpile byte-identity
  risk that only the real-PHP oracle would catch.

- **Verdict on (b):** the *Rust* half is reproducible across interpreter+VM; the *PHP* half is the
  fragile part. Do **not** ship cooperative super in the first S6 slice. If it's ever wanted, scope it
  as its own milestone with the PHP forwarding-codegen as the explicit risk, and gate it on the PHP
  oracle from day one. For S6, prefer (a).

### 4.4 Bottom line for the S6 design

1. **Compute C3 at compile time** over the static class graph; reject inconsistent hierarchies with a
   diagnostic (mirror Python's class-creation-time failure). Front-end-only, no new `Op`.
2. **Use the order only to pick the winning method** (case a). Flatten to one resolved body per
   `(class, method)`; build flat method tables for the Rust backends; emit a flat PHP class (or
   generated `insteadof`/`as` trait resolution) for the transpiler. Byte-identity is safe by
   construction; lowering is idiomatic PHP.
3. **State/diamond:** merge fields of all ancestors into the one resolved layout; a shared base
   contributes its fields once. No runtime walk needed in case (a).
4. **`super`/`parent` keeps PHP's lexical-parent meaning** in case (a) — no surprise for PHP devs,
   nothing to emulate.
5. **Cooperative super (case b) is explicitly out of scope** for the first slice — powerful, the Rust
   spine can reproduce it, but the PHP forwarding-method synthesis is fragile and against the
   project's "don't emit what PHP lacks" grain.
