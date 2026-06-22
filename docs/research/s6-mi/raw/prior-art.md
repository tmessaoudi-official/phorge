# S6 Multiple Inheritance — Prior Art Research

**Topic:** prior art for lowering multiple inheritance / rich object models onto PHP (or other
constrained single-inheritance targets), plus the academic conflict-resolution models.

**Phorge constraint (the spine):** three backends — tree-walking interpreter, stack bytecode VM,
Phorge→PHP transpiler — must stay **byte-identical**. PHP 8.4 target: single class inheritance, many
interfaces, traits with explicit `insteadof`/`as` conflict resolution. The transpiler can only emit
what PHP can express; the interp + VM must reproduce whatever semantics we pick, deterministically.

Date: 2026-06-22. All web claims dated June 2026 search.

---

## 1. Languages / compilers that target PHP

### Haxe (PHP target)
- **Single class inheritance only.** A Haxe class has exactly one parent; it may `implements`
  multiple interfaces. Interfaces themselves may `extends` multiple interfaces (MI *at the interface
  level* — pure signatures, no bodies in the core model).
- The PHP target (PHP 7.0+, `--php`, `-D php7`) maps this 1:1 onto PHP's own object model: one
  `extends`, many `implements`. **No translation gymnastics needed** because Haxe's source model is
  already the PHP model. [Inferred from Haxe manual: the constraint is at the *front end*, so the
  back end is trivial.]
- **No language-level traits/mixins for classes.** When users want shared *behaviour* across an
  inheritance boundary, the idiomatic workaround (Haxe community thread "Writing externs: the problem
  of multiple inheritance") is **interfaces carrying default implementations via the `tink_lang`
  macro library** (`@:tink` interfaces). Extern classes `implements` several such interfaces and gain
  the bodies *at compile time*; at runtime the target stays single-inheritance. A PR (#5797) to allow
  MI on externs was **closed** — the community deliberately prefers the lightweight compile-time
  workaround over real MI.
- **Lesson for Phorge:** Haxe never tries to *simulate* MI. It pushes the whole problem to the front
  end (interfaces + macro-injected defaults resolved before codegen), so the PHP it emits is plain
  single-inheritance PHP. This is the same "expand/resolve before the backend" discipline Phorge
  already uses (`erase_generics`, `expand_aliases`, `core.html` holes).

### Peachpie (PHP → .NET / CIL)
- Peachpie compiles **PHP** onto .NET (CIL), which is *also* single-inheritance — so the interesting
  direction is the reverse of Phorge, but the technique is directly relevant.
- PHP **traits** were "quite a challenge" to compile to CIL because .NET has no trait construct.
  Peachpie's approach: the compiled class **declares all the trait's members itself**, but the trait
  is compiled **once** as a separate type (constructor shape `public .ctor(Context ctx, TSelf @this)`);
  a **private field holds a trait instance**, and the host class's generated members **delegate** to
  that instance. So trait code is *not* copied per-use — it is shared and forwarded to.
- **Lesson for Phorge:** there are two faithful lowerings of "horizontal reuse": (a) **copy/flatten**
  the members into the host (PHP's own trait model, and the original academic traits model), or
  (b) **store-and-delegate** (Peachpie). For a *transpiler to PHP*, (a) is free because PHP traits
  already do the flattening for us — we don't need Peachpie's delegation trick. But (b) is the model
  the interp + VM may find easier to keep byte-identical (a parent instance held in a field, method
  calls forwarded), if we ever step outside PHP's native trait flattening.

### Hack / HHVM (Facebook's PHP dialect)
- Hack keeps **single inheritance + multiple interfaces + traits** — it does **not** add real MI.
- Hack's notable addition is **trait/interface requirements** (`require extends C`,
  `require implements I`): a trait can declare that any class using it **must** be a subclass of `C`
  or implement `I`. This lets the type checker verify a trait that assumes a surrounding class
  hierarchy/API — "horizontally composable functionality" that "defines an external API it provides
  *and* an API it expects from the use site."
- Hack's stated framing: PHP traits "were originally designed to allow for a sort of multiple
  inheritance while avoiding state conflicts" and, *combined with interfaces*, give "a convenient way
  to achieve multiple inheritance as long as they avoid name collisions." Hack's contribution is
  making that **statically checkable**, not adding new runtime power.
- **Lesson for Phorge:** Hack validates the trait-as-MI-substitute thesis from a billion-line
  codebase, and its **requirements** mechanism is exactly the static-checker hook Phorge would want to
  make trait composition type-safe (Phorge already has a checker that runs before every backend).

### Other PHP front-ends (Pharen, etc.)
- Pharen (a Lisp that compiles to PHP) and similar PHP-targeting front-ends inherit PHP's object model
  wholesale; none is documented as faithfully compiling *true* MI to PHP. [Unverified: no source found
  showing Pharen does anything MI-specific; treat as "does not attempt MI."]

---

## 2. Traits as the academic answer to MI — "Traits: Composable Units of Behaviour"

Schärli, Ducasse, Nierstrasz, Black, **ECOOP 2003** (ECOOP Test-of-Time Award 2022; first implemented
in Squeak Smalltalk). The paper is the intellectual root of PHP/Hack/Scala/Rust traits.

**Central thesis:** traits give you MI's *reuse* without MI's *ambiguity*, by making composition
**flat** and conflicts **explicit**. The composing class — not a hidden linearization algorithm — is
in charge.

**Core rules (the paper's model):**
1. **A trait = a set of methods**, nothing else. It is the primitive unit of reuse.
2. **No state.** In the pure model traits **cannot declare instance variables and never access state
   directly** — they call accessor methods that the composing class must provide. This is what kills
   the *diamond state problem* (the hard part of MI): there is no duplicated/merged state to reconcile
   because traits carry none. (PHP traits **break this** — PHP traits *are* stateful, which
   re-introduces a weaker form of the conflict.)
3. **Composition is symmetric and order-independent.** Composing traits T1 and T2 yields the same
   result regardless of order. There is **no linearization, no implicit precedence** — unlike MI and
   unlike mixins (which *are* order-sensitive / linearized).
4. **Conflicts must be resolved explicitly by the composer.** If two composed traits provide the same
   method name, that is a **conflict**, and the composing class must resolve it with one of:
   - **override** (the class/glue defines its own method, which wins over all trait methods of that
     name),
   - **alias / `@` rename** (give a trait method a second name so both remain reachable),
   - **exclusion / `-` (minus)** (remove a method from a trait before composing, so the other trait's
     wins).
   An unresolved conflict is a **compile-time error**, not a silent pick.
5. **The flattening property.** A class built from traits is **semantically equivalent to the same
   class written *without* traits**, with all methods inlined directly. Traits add **no** semantic
   layer at runtime — you can always "flatten them away." Method lookup is *as if* the methods were
   declared in the class itself.
6. **Glue code.** The composing class provides the "glue": the state (instance variables), the
   required accessors, and the conflict resolutions. Traits provide only behaviour.

**Why this beats linearized MI for *reasoning* (the paper's argument):**
- With C++/Python MI you must mentally run the **linearization algorithm (MRO)** to know which method
  wins; precedence is **implicit** and changes when the hierarchy changes (the *fragile hierarchy*
  problem — adding a class can silently reorder which method a call resolves to).
- With traits, a conflict is **visible at the composition site and the composer chose**; there is no
  hidden ordering. Behaviour is **local and explicit**. The flattening property means you can read the
  composed class as a flat method set.
- vs **mixins**: mixins are applied in sequence and are *order-dependent* (later mixins override
  earlier) — they are essentially single-axis linearization. Traits are *symmetric*; the conflict is
  surfaced rather than silently won by application order.

**This is exactly the model Phorge is leaning toward** — "compose many parents, collisions are a
compile error unless the subclass resolves them." That *is* explicit-resolution flat composition.

---

## 3. CLOS / Dylan — multiple dispatch + MI (the linearization camp)

- CLOS (Common Lisp Object System) and **Dylan** are the canonical *full* MI systems. They resolve MI
  not by trait flattening but by **class precedence list / Method Resolution Order (MRO)** computed by
  **C3 linearization** (C3 was *developed for Dylan*, later adopted by Python 2.3+, Solidity, Raku).
- C3 produces a **deterministic, monotonic** linear order of all superclasses from the user's declared
  direct-superclass order. **Monotonicity:** if C1 precedes C2 in a class's linearization, C1 precedes
  C2 in every subclass's linearization too. It also preserves *local precedence order* (the order you
  wrote the parents in).
- **`call-next-method`** (CLOS) / `next-method` (Dylan) is **cooperative super**: a method body can
  invoke "the next most specific method in the MRO," letting a diamond's shared base run **exactly
  once** even though two paths reach it. This is the elegant part — it solves the diamond *call*
  problem (Python's `super()` is the same idea on the C3 MRO).
- **Cost:** the programmer must understand the MRO to predict behaviour; C3 can **fail to linearize**
  (raises an error) when parent orders are inconsistent; and behaviour is implicit/global rather than
  local/explicit. CLOS additionally has **multiple dispatch** (method chosen on *all* argument types,
  not just the receiver) — far beyond anything PHP can express, and irrelevant to a PHP target.
- **Relevance to Phorge:** this is the model the Traits paper is arguing *against* for
  reasoning-clarity, and — critically — **PHP cannot express it.** PHP has no MRO, no
  `call-next-method` across multiple parents, single `parent::`. To reproduce C3 + cooperative super
  on a PHP target you would have to **build the MRO yourself and emit explicit dispatch glue** in the
  transpiled PHP, then reproduce that *same* dispatch in the interp and VM. Heavy, and the emitted PHP
  would look nothing like idiomatic PHP (breaks Phorge's "transpiles to idiomatic PHP" contract).

---

## 4. Mixin-via-linearization in transpiled languages — TypeScript mixins

- TypeScript targets **single-inheritance JavaScript** (one `extends`). It fakes MI with the
  **mixin factory pattern**:
  ```ts
  type Constructor = new (...args: any[]) => {};
  const Flyable  = <T extends Constructor>(Base: T) => class extends Base { fly()  {} };
  const Swimmable = <T extends Constructor>(Base: T) => class extends Base { swim() {} };
  class AmphibiousPlane extends Flyable(Swimmable(Vehicle)) {}
  ```
- A mixin is a **function `Base => class extends Base`**. You **stack** them, producing a *real linear
  chain* `Flyable ⊃ Swimmable ⊃ Vehicle` at runtime. So TS mixins are **linearization**, not flat
  composition: order matters, later wraps earlier, the last-applied mixin's method wins on collision —
  and **collisions are silent** (no explicit-resolution requirement; TS only warns on type
  incompatibility, not on "two mixins both define `foo`").
- It compiles cleanly to single-inheritance JS *because* it builds an actual prototype chain. The cost
  is the silent-override / order-dependence the Traits paper warns about.
- **Relevance to Phorge:** TS mixins prove a transpiler *can* fake MI on a single-inheritance target —
  but via linearization with silent precedence, the exact footgun Phorge's "collision = compile error"
  stance is trying to avoid. Also, the chain trick has **no clean PHP analogue** (PHP can't
  `class X extends $dynamicBase` — `extends` needs a static name), so even the *mechanism* doesn't port
  to a PHP target. PHP **traits** are the right tool, not a synthesized parent chain.

---

## 5. Synthesis for Phorge

### (a) Has anyone *faithfully* compiled true multiple inheritance to PHP?
**No.** Surveyed: Haxe (single-inheritance front end → trivial PHP), Hack (single inheritance +
checkable traits, no MI), Peachpie (the *reverse* direction; lowers PHP traits to CIL via
store-and-delegate), TypeScript (fakes MI on JS via a *prototype-chain linearization* that has no PHP
analogue). **Every serious system either (i) refuses MI at the front end and emits plain
single-inheritance code, or (ii) uses traits/interfaces as the MI substitute.** Nobody emits a C3-MRO
+ cooperative-super runtime into PHP. The "instead" they chose — **traits + interfaces** — is widely
considered *good enough* in practice (Hack ships it at Facebook scale; PHP itself shipped it in 5.4).
The reuse you actually want from MI is covered; what you lose (a single object that *is-a* of two
unrelated concrete classes with merged state and C3 dispatch) is precisely the part that is unsound /
hard to reason about anyway.

### (b) Does the Traits paper support an "explicit-resolution MI" model over Python-C3-super?
**Yes, strongly, and it is the paper's central claim.** The paper argues flat symmetric composition +
*explicit* conflict resolution is **superior for reasoning** to linearized MI precisely because:
- precedence is **visible and chosen** at the composition site (vs implicit MRO you must compute),
- there is **no fragile-hierarchy reordering** (adding a parent can't silently change which method
  wins),
- the **flattening property** lets you read a composed class as a flat method set — which also happens
  to be **exactly how PHP traits behave** (the transpiler gets the semantics for free).
So Phorge choosing "compose many parents; collisions are a compile error unless the subclass resolves
them (via choose-one / alias / exclude)" is not a compromise forced by PHP — it is the *academically
preferred* model, and PHP's `insteadof`/`as` are a near-perfect lowering target for it.
**Caveat:** PHP traits are **stateful**, breaking the paper's pure-stateless rule. Phorge must decide
whether composed parents may carry fields. If they can, Phorge re-inherits a (weak) state-conflict
problem and must define a rule (PHP's default: a property collision across traits with *different*
initial values is a fatal error / deprecation; same value is allowed). Cleanest: **require the
composing class to resolve field collisions too**, mirroring method resolution.

### (c) Ranking — "transpiles faithfully to PHP AND reproducible byte-identically in interp+VM"

**Rank 1 — (a) explicit-resolution flat composition.** *Winner by a wide margin.*
- **PHP lowering:** direct — emit each parent as a PHP **trait**, `use A, B;` in the class, and emit
  the user's resolutions as `insteadof` (choose-one) and `as` (alias). Idiomatic, native PHP 8.4. For
  the "is-a both" typing, emit matching **interfaces** + `implements` (Haxe/Hack model).
- **Byte-identical interp+VM:** trivial because composition is **flattening** — resolve it in the
  *front end* (the checker), exactly like Phorge already flattens `erase_generics` / `expand_aliases`
  / cross-package mangling **before any backend**. After flattening, the class is an ordinary
  single-class method table; interp + VM consume an already-merged AST with **no new `Op`, no runtime
  MRO**. Conflicts are a compile error, so there is no runtime ambiguity to keep in sync. This is the
  *only* option where the three backends see identical, pre-resolved input.

**Rank 2 — (c) C++-style explicit qualification** (`Base::method()` to disambiguate at each call site).
- **PHP lowering:** *partial.* PHP can name `TraitName::method()` inside a class and has trait
  aliasing, so explicit qualification is expressible-ish — but PHP has **no general
  `Base::method()` virtual-dispatch through multiple concrete parents**, and `parent::` is
  single-target. You'd be emitting trait-qualified calls, which is really model (a) wearing a
  different syntax. As a *standalone* model it forces the programmer to qualify every ambiguous call
  (worse ergonomics, more error-prone) and still needs trait flattening underneath.
- **Byte-identical:** achievable (qualification is also front-end-resolvable to a concrete target),
  but it offers nothing over (a) except worse UX, and its runtime story collapses into (a) anyway.

**Rank 3 — (b) C3 linearization + cooperative super.** *Worst fit; effectively disqualified.*
- **PHP lowering:** PHP has **no MRO and no `call-next-method` across multiple parents.** To reproduce
  C3 + cooperative super you must **compute the MRO in the compiler and emit explicit dispatch
  scaffolding** into the PHP (synthetic ordered method tables, manual "next method" chaining). The
  emitted PHP would be **non-idiomatic generated glue** — a direct violation of Phorge's
  "every feature maps to *idiomatic* PHP" contract (the same reason the Java `System.out.println`
  object-path and real-MI were rejected before).
- **Byte-identical:** the *hardest* of the three. The MRO + cooperative-super semantics must be
  reproduced identically in the interpreter, the VM (likely a **new `Op`** or a runtime dispatch
  kernel), *and* the emitted PHP scaffolding — three independent implementations of a subtle algorithm
  that must agree on every diamond, every `super` hop, every linearization-failure error. This is the
  maximal-surface-area-for-drift option, against a spine whose entire discipline is *minimize runtime
  surface, resolve in the front end*.
- It is also the model the Traits paper argues is *worse for human reasoning* — so it loses on
  ergonomics, on PHP-faithfulness, **and** on byte-identity simultaneously.

**Justification of the ordering:** the byte-identity spine rewards *front-end-resolvable, runtime-free*
features (no new `Op`, identical pre-resolved AST to all three backends). (a) is fully
front-end-resolvable and lowers to native PHP traits/interfaces → top. (c) is also front-end-resolvable
but adds no value and reduces to (a) → middle. (b) is irreducibly *runtime* (MRO + cooperative super),
has no idiomatic PHP target, and triples the algorithm surface that must stay in lockstep → bottom.

### What Phorge should steal
- **The traits-paper model wholesale:** flat, symmetric, order-independent composition; **conflicts =
  compile-time error** unless resolved; resolution via **override (subclass wins) / alias (`as`) /
  exclusion**. This is both the academically-preferred model *and* the cheapest to keep byte-identical.
- **Resolve composition in the checker, before any backend** — the same "expand/erase/mangle before
  backends" discipline already proven by `erase_generics`, `expand_aliases`, `core.html` holes,
  cross-package mangling. After resolution the backends see a flat method table; **no new `Op`,
  no runtime MRO.**
- **PHP `insteadof` (choose-one) + `as` (alias) as the lowering target** — they are an almost
  one-to-one match for the trait paper's resolution operators; emit each parent as a PHP **trait** and
  the "is-a" typing as **interfaces + `implements`** (Haxe/Hack pattern).
- **Hack's trait *requirements* idea** — let a composed parent declare it `requires` a method/field/
  type from the host, statically checked in the checker (Phorge already type-checks before every
  backend). Makes composition safe and gives clean diagnostics (`E-MI-CONFLICT`, `E-MI-REQUIRE`, …).
- **Explicit field-collision resolution too** (close PHP's stateful-trait gap): require the composer
  to resolve clashing fields just like methods, restoring the paper's "no silent state merge" property.

### What to avoid
- **C3 / MRO / cooperative-super on a PHP target** — no idiomatic PHP form, triples the algorithm
  surface across interp/VM/PHP (drift magnet), and is the model the literature deems worse for
  reasoning. (Same class of rejection as the Java object-path call form.)
- **TypeScript-style prototype-chain mixins** — they linearize with **silent** precedence (footgun)
  and the `extends DynamicBase` mechanism **has no PHP analogue** (`extends` needs a static name).
- **Silent "last one wins" collision handling** of any kind — it breaks the explicit-resolution
  property and is unreproducible-by-design across backends without a hidden ordering rule each must
  share.
- **Carrying PHP's stateful-trait ambiguity unaddressed** — define the field-collision rule explicitly
  rather than inheriting PHP's fatal-vs-allowed-by-equal-default-value subtlety.
- **Runtime multiple *dispatch* (CLOS-style)** — selects on all argument types; PHP can't express it
  and it's far outside the S6 scope.

---

## Sources
- Haxe inheritance / interfaces / PHP target — https://haxe.org/manual/types-class-inheritance.html ,
  https://haxe.org/manual/types-interfaces.html , https://haxe.org/manual/target-php.html
- Haxe externs MI thread — https://community.haxe.org/t/writing-externs-the-problem-of-multiple-inheritance-traits/182
- Traits: Composable Units of Behaviour (ECOOP 2003) — https://www.cs.cmu.edu/~aldrich/courses/819/Scha03aTraits.pdf ,
  https://link.springer.com/chapter/10.1007/978-3-540-45070-2_12
- Hack trait/interface requirements — https://hhvm.com/blog/9581/trait-and-interface-requirements-in-hack ,
  https://docs.hhvm.com/hack/classes/trait-and-interface-requirements
- Peachpie compiled trait — https://docs.peachpie.io/api/assembly/compiled-trait/ , https://en.wikipedia.org/wiki/PeachPie
- C3 linearization / CLOS / Dylan MRO — https://handwiki.org/wiki/C3_linearization ,
  https://www.python.org/download/releases/2.3/mro/
- PHP traits + insteadof/as conflict resolution — https://www.php.net/manual/en/language.oop5.traits.php ,
  https://riptutorial.com/php/example/7271/conflict-resolution
- TypeScript mixins — https://www.typescriptlang.org/docs/handbook/mixins.html
