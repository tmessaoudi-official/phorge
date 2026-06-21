# Track 1 — Mutation Semantics: value vs reference, and the cycle/GC consequence

> Research deliverable for the Phorge mutation + GC milestone. Question: when Phorge gains
> mutation, should a mutable value have **value semantics** (copy-on-write / place-based, like PHP
> arrays and Rust structs) or **reference semantics** (shared-mutable aliasing, like PHP objects and
> Java)? The crucial downstream consequence is which choice forces a **tracing GC** (cycles possible)
> versus keeps `Rc`/`Drop` sufficient (cycles impossible).
>
> Grounding read directly: `src/value.rs`, `src/ast.rs`, `src/vm.rs`, `src/chunk.rs`,
> `src/compiler.rs`, `src/interpreter.rs`, `docs/INVARIANTS.md`, `docs/ARCHITECTURE.md`,
> `docs/specs/2026-06-21-php-parity-and-beyond.md`, `docs/plans/2026-06-21-ga-direction-and-autonomy.plan.md`.
> Every claim graded inline.

---

## TL;DR — the recommendation

**Adopt mutable VALUE semantics for the whole heap — place-based mutation of the binding's storage,
copy-on-write under the hood — and explicitly KEEP reference semantics OUT of the language.** This is
PHP-array semantics generalized to *every* heap value (`List`/`Map`/`Set` AND `Instance`/`Enum`),
not PHP-object semantics. The transpile target is idiomatic PHP: arrays mutate as PHP arrays already
do, and object "mutation" lowers to PHP 8.5 **`clone $o with [...]`** (functional update) for
immutable fields, or direct property writes where a mutable-object escape hatch is justified.

The decisive consequence: **value semantics with no first-class references makes reference cycles
structurally impossible, so `Rc`/`Drop` keeps reclaiming the whole heap and NO tracing GC is ever
needed.** The "mutation+tracing-GC" milestone can drop the *tracing-GC* half entirely. This is not a
shortcut — it is the same design Hylo/"mutable value semantics" research proves sound
[Verified: arxiv 2106.12678; see Evidence §A], and it is the *only* choice that preserves Invariant
#1 (byte-identical `run≡runvm≡real PHP`) without a GC determinism hazard.

`mutable` stays a **binding modifier** (reassignment of a place), composed with — not conflated with —
in-place value update. Deep mutation through an alias is impossible *because there are no aliases*.

---

## 1. What the codebase forces (ground truth, all [Verified] by reading the source)

| Fact | Evidence | Consequence for this decision |
|---|---|---|
| Every heap `Value` is `Rc<T>` with an **immutable interior** — no `RefCell`, no `Cell`. | `src/value.rs:12-44`: `List(Rc<Vec<Value>>)`, `Map(Rc<Vec<(HKey,Value)>>)`, `Set(Rc<Vec<HKey>>)`, `Instance(Rc<Instance>)`, `Enum(Rc<EnumVal>)`. | Cloning a `Value` is a refcount bump. Mutation needs *somewhere* to write; today there is nowhere. |
| `Drop` reclaims the entire heap; the module header states the heap is **immutable + acyclic**, which is *why* no tracing GC exists. | `src/value.rs:1-6`: "no cycle can leak, so no tracing collector is needed (deferred to M3, when mutation could create cycles)". | The GC question is entirely downstream of whether mutation can create a cycle. A cycle needs aliasing. |
| There is **no assignment statement** in the AST. | `src/ast.rs:511-546` `Stmt` = `VarDecl`/`Return`/`If`/`For`/`Block`/`Expr` only. No `Assign`. | Mutation is a genuinely new construct; nothing to retrofit. Clean slate. |
| The VM **already has place-based local storage**. | `src/chunk.rs:94-95` `Op::GetLocal(slot)`/`Op::SetLocal(slot)` (set-and-pop into a stack slot); `src/vm.rs:199-203` `SetLocal` does `self.stack[idx] = v`. | Local *reassignment* (the `mutable` binding) is one new `Stmt::Assign` lowering to the **existing** `Op::SetLocal` — no new `Op`. |
| The interpreter stores locals in `HashMap<String, Value>` per scope, looked up by name. | `src/interpreter.rs:55-77` `CallScopes { scopes: Vec<HashMap<String,Value>> }`, `insert`/`lookup`. | Interpreter reassignment is a re-`insert` on the innermost scope — also no new machinery. |
| `Instance.fields` is `HashMap<String, Value>` (owned inside the `Rc`). | `src/value.rs:69-73`. | A field write needs `Rc::make_mut(&mut inst)` then `fields.insert(...)` — copy-on-write at the `Rc` boundary. The container is already shaped for it. |
| `&` references / aliasing are **already rejected by design**. | parity spec Group 3: "`&` references / aliasing — breaks the immutable + acyclic heap that the no-tracing-GC design rests on; preserved via immutable values + `clone`-with (later)". | The cycle-free property is a *standing project commitment*, not a new constraint I am inventing. |
| Backend parity (`run≡runvm≡real PHP`, byte-identical incl. failure) is the spine. | `docs/INVARIANTS.md` #1–#2. | Any mutation model must keep all three byte-identical. A GC with non-deterministic collection timing (e.g. observable via `__destruct`) would break this — and `__destruct` is *already removed* for exactly this reason (parity spec Group 3). |

**The single most important grounded fact:** the project has *already decided* aliasing is out and
the heap stays acyclic (parity spec Group 3, the `&` row). The mutation milestone's only open
question is therefore not "value vs reference" in the abstract — it is **"how do we add in-place
update while keeping the already-committed no-aliasing property?"** Value semantics is the answer that
honors that commitment; reference semantics would *reverse* it.

---

## 2. The two models, precisely

### Model V — mutable value semantics (RECOMMENDED)
- A binding names a **place**. `mutable x` may be reassigned (`x = e;`) and may be updated in place
  (`x.field = e;`, `xs[i] = e;`, `m[k] = e;`).
- **No two bindings ever share mutable state.** Assigning/passing a value gives the callee a logically
  independent copy. Under the hood this is copy-on-write: the `Rc` is shared until a write, at which
  point `Rc::make_mut` clones the *unique* path being mutated (structural sharing of the untouched
  parts is a later perf concern, not a semantic one).
- Mirrors: **PHP arrays** [Verified: zetcode/php COW], **Rust `struct`/`Vec` with `let mut`**
  [Verified: Rust book], **Swift `struct`/`Array`/`Dictionary`** [Verified: Swift COW is automatic for
  value types], **Kotlin `data class` `val` + `copy()` for the immutable spine**.
- **Cycles: IMPOSSIBLE.** A cycle requires two live handles to the same mutable cell; value semantics
  forbids exactly that. `Rc`/`Drop` reclaims fully. **No tracing GC.** [Verified: see §A].

### Model R — reference semantics (REJECTED)
- A binding names a **handle**. Two bindings can point at the same mutable object; a write through one
  is visible through the other (deep mutation through any alias).
- Mirrors: **PHP objects** [Verified], **Java/Kotlin class instances** [Verified: Kotlin `copy()` is a
  *shallow* copy that *shares mutable references* — search result], **Python/JS objects**.
- **Cycles: POSSIBLE** (`a.next = b; b.next = a`). `Rc`/`Drop` then leaks; you MUST add either a tracing
  GC or a `Weak`-reference discipline the user manages by hand. [Verified: Rust book ch15-06 — `Rc` +
  interior mutability leaks cycles; Swift ARC has the same hazard, "you have to avoid/break reference
  cycles manually, or you leak memory"].

---

## 3. Why Model V is the craftsmanship-apex choice (the filter: SOLID / legible / provably-correct)

1. **It keeps the no-tracing-GC invariant alive — provably, not hopefully.** Mutable value semantics
   is a *researched, named discipline* with a soundness proof: in its pure form "references are
   second-class… variables can never share mutable state" → no aliasing → no cycles
   [Verified: arxiv 2106.12678, "Native Implementation of Mutable Value Semantics"; scattered-thoughts
   summary]. Phorge's `Rc<immutable interior>` + copy-on-write at the write boundary IS the native
   implementation strategy that paper describes [Verified: JOT 2022 "Implementation Strategies for
   Mutable Value Semantics" enumerates exactly the COW-via-uniqueness-check approach Swift uses with
   `isKnownUniquelyReferenced`, which maps 1:1 to Rust `Rc::make_mut` / `Rc::get_mut`].
   - This turns the milestone name "mutation + **tracing-GC**" into "mutation + **(no GC needed)**" —
     a strict simplification with a citation behind it, not a gamble.

2. **It is the determinism-safe choice, and determinism IS the byte-identity spine.** A tracing GC
   collects at non-deterministic times. The moment collection timing is observable (finalizers /
   `__destruct`), `run` (tree-walker, Rust `Drop`), `runvm` (VM, Rust `Drop`), and real PHP (Zend GC)
   would diverge — breaking Invariant #1. The project already removed `__destruct` for precisely this
   reason (parity spec Group 3: "destruction timing non-deterministic under `Rc`/`Drop` → breaks the
   byte-identity spine"). Choosing Model R re-introduces the hazard the project already engineered out.
   [Inferred — from Invariant #1 + the documented `__destruct` removal rationale.]

3. **It matches the half of PHP that already has value semantics, and PHP 8.5 supplies the object
   transpile target.** PHP arrays are COW value types [Verified]. So Phorge `List`/`Map`/`Set`
   mutation transpiles **1:1** to PHP array mutation — byte-identical for free. For objects, PHP 8.5
   ships **`clone $o with ['field' => v]`** — a first-class functional-update construct that "respects
   all visibility rules and type constraints" [Verified: wiki.php.net/rfc/clone_with; PHP 8.5 stable].
   So Phorge object "mutation" of immutable fields lowers to idiomatic, *modern* PHP, not a hand-rolled
   wither-method emulation. The transpile contract (Phorge:PHP :: TS:JS) is satisfied *more* cleanly by
   value semantics than the project's own prior "clone-with later" note assumed.

4. **It composes with everything already shipped, additively.** Immutable-by-default + `mutable`
   (already ACCEPTED, GA plan) is *exactly* the Rust `let` / `let mut` and Swift `let` / `var` split.
   Optionals (S2), unions (S4), generics (S7), `Result+?` (planned primary error channel) all assume
   values don't alias-mutate underneath them — smart-casts and flow-narrowing are **only sound under
   value semantics** [Inferred: a smart-cast `if (x instanceof C) { … }` can be invalidated by a
   concurrent write through an alias; with no aliases it cannot]. Model R would silently weaken every
   narrowing guarantee the type system already makes.

5. **Reject-list consistency.** The project rejected `&`, `global`, `static` locals, `WeakMap`,
   `__get`/`__set`, `__destruct`, lazy objects — *every one* is an aliasing/shared-mutable/non-determinism
   feature. Model R is the umbrella those rejections live under. Choosing Model R would make ~8 prior
   decisions incoherent. [Verified: parity spec Group 2/3.]

---

## 4. The one honest tension — and why it still resolves to Model V

PHP objects ARE reference types. A PHP developer's mental model of `$a = $b;` on an object is "they
now share." Phorge giving objects *value* semantics is a **conceptual divergence from PHP** — and the
GA philosophy prizes familiarity-as-on-ramp.

**Resolution (per the philosophy's own ordering — craftsmanship is apex, familiarity is the on-ramp,
never a license to keep an unsound form):**
- Shared-mutable object aliasing is *the* canonical PHP footgun (the "I mutated it over there and it
  changed over here" bug). The philosophy explicitly removes surprises while preserving capability.
- The **capability** "I have an object and I change a field to get an updated object" is preserved —
  via `clone with` (immutable spine) or an explicit, opt-in mutable local. What's removed is the
  *spooky-action-at-a-distance*, not the capability.
- Familiarity is preserved *conceptually*: `mutable` reads like Rust/Swift/Kotlin's `var`, which the
  target audience increasingly knows; and the *array* half stays byte-for-byte PHP.
- This is the identical call the project already made for `&` references: capability preserved
  (immutable values + clone-with), unsound aliasing form removed. Model V is just that decision applied
  uniformly to objects too. [Inferred — direct analogy to the documented `&` decision.]

**If the developer wants a reference escape hatch** (genuine shared-mutable graphs — a doubly-linked
list, an observer registry): that is a *genuine fork* with no single craftsmanship-best answer, and
per the Autonomy Contract should STOP and ask. Recommended framing for that fork is in §7.

---

## 5. `mutable` as a binding modifier vs a type modifier

**Recommendation: `mutable` is a BINDING modifier (a property of the place), not a type modifier (a
property of the value).** [Speculative→Inferred, grounded in the Rust/Swift precedent below.]

- Rust: `let mut x` / `&mut T` — mutability is on the *binding* and the *borrow*, never baked into the
  type `T` itself. A `String` is just a `String`; whether you can mutate it depends on how you hold it.
  [Verified: Rust book.] This is the cleaner model because it avoids a `mutable T` / `T` type-pair
  explosion across the entire type lattice (it would interact combinatorially with `T?`, `A|B`, `A&B`,
  `List<T>`, generics — all already shipped).
- Swift: `let` / `var` — same; the binding decides, `struct` types are uniformly value types.
  [Verified.]
- Concretely for Phorge: `mutable` lives on `Stmt::VarDecl` (and later on a `mutable` ctor-param /
  field modifier — note `ast::Modifier` already has the *slot* for this, currently
  `Public/Private/Protected/Const/Final`, `src/ast.rs:491-498`). The *type* annotation
  (`Type::Named`, etc.) is untouched — Invariant #9 (AST untyped, backends re-derive) stays intact, and
  the compiler's `CTy` lattice (`src/compiler.rs:47`) needs **no new variant** for mutability.

This also means **field mutability is per-field, declared on the field/promoted-param**, exactly like
Rust struct fields being mutable-through-a-`mut`-binding and PHP 8.1 `readonly` being a per-property
modifier. Default immutable; `mutable` opts a field into in-place write. [Inferred.]

---

## 6. Place-based vs deep mutation; inout/ref params

- **Place-based, always.** `x.field = e` mutates the storage `x` denotes. Because values don't alias,
  "mutate the place" and "mutate through any alias" are the *same operation* — there is only ever one
  handle. Deep mutation "through an alias" is not a feature to design; it is a non-event under Model V.
  [Inferred — definitional under value semantics.]
- **Copy-on-write is the implementation, invisible to semantics.** `xs[i] = e` on a `Value::List`:
  `Rc::make_mut(&mut rc_vec)` (clones iff shared) then index-assign. Identical kernel in both backends
  (single-sourced, like the arith kernels in `value.rs` — Invariant #3), so `run≡runvm` is preserved by
  construction. [Inferred — mirrors the existing single-sourced-kernel discipline.]
- **`inout`/`ref` params: ADOPT `inout`, REJECT `&`.** This is the subtle, high-value distinction:
  - PHP's `&` param is *reference semantics* (an alias escapes into the function and persists) → cycles
    possible → rejected (already is).
  - Swift's `inout` is **copy-in / copy-out** ("call by value result"), NOT a reference
    [Verified: Swift search — "The in-out parameter doesn't use reference types… copy-in copy-out"]. It
    gives you "mutate my caller's variable" *ergonomics* with **value semantics** and **no aliasing**:
    the callee gets a copy, mutates it, and the copy is written back at return. No alias survives the
    call → no cycle. This is the craftsmanship-correct way to offer "pass something to be modified"
    without reopening the heap. Transpiles to PHP `&$param` *only as a lowering detail* (PHP `&` is the
    mechanical target for write-back), while Phorge-level semantics stay copy-in/copy-out.
    [Inferred — semantics from Swift, lowering from PHP's by-ref param.]
  - Net: offer `inout` (Swift-shaped, value-safe) and keep `&` rejected. The capability "function
    modifies its argument" is preserved; the aliasing footgun is not.

---

## 7. The genuine fork to surface (per Autonomy Contract: STOP-and-ask)

**Fork: is there a sanctioned reference-semantics escape hatch for genuinely-cyclic data structures?**
Most programs never need one (value semantics + `inout` + `Result` covers the 95%). But graph-shaped
domains (doubly-linked lists, observer/parent-pointer trees, ECS) genuinely want shared mutable
identity. Options:
- **(a) No escape hatch.** Purest; keeps GC permanently off the table; users model graphs with
  index/`Map<Id,Node>` indirection (the Rust-community idiom). [Verified: this is the standard Rust
  answer to "I want a graph" — arena + indices.]
- **(b) An opt-in `Shared<T>` / `ref` reference type, GC-quarantined.** Adds reference semantics for a
  *named, visible* wrapper only — but this re-admits cycles for that type, which forces *some*
  reclamation story (a `Weak`-discipline, or a tracing GC scoped to `Shared` values). Heavy; reopens
  the determinism hazard. [Inferred.]
- **(c) Defer the decision** — ship Model V now (it's a strict subset), revisit (b) only if a real
  program proves the need. [Inferred — YAGNI / cheapest-reversible.]

**My recommendation: (c) — ship pure Model V, defer (b).** It's additive-safe: if (b) is ever added it
*extends* the language without invalidating any Model-V program. But because (a)-vs-(b) is a real
capability question with no forced answer, it is exactly the kind of "genuine fork" the contract says
to STOP on. **Recommended: present (a)/(b)/(c) to the developer before the milestone's design freeze.**

---

## A. External evidence (cited)

- **Mutable value semantics is sound & cycle-free by construction.** "In the purest form of mutable
  value semantics, references are second-class: they are only created implicitly, at function
  boundaries, and cannot be stored in variables or object fields. Hence, variables can never share
  mutable state." → no aliasing → no cycles → no tracing GC.
  [Verified: *Native Implementation of Mutable Value Semantics*, arXiv 2106.12678; summarized at
  scattered-thoughts.net/writing/ruminating-about-mutable-value-semantics/]
- **COW implementation strategy maps 1:1 to Rust `Rc`.** Swift uses `isKnownUniquelyReferenced` to
  copy lazily only on a non-unique mutation; Rust's `Rc::make_mut`/`Rc::get_mut` is the identical
  uniqueness-gated clone. [Verified: Swift COW docs (medium/marcosantadev); *Implementation Strategies
  for Mutable Value Semantics*, JOT 2022 issue.]
- **Reference semantics ⇒ cycles ⇒ leak under refcounting.** Rust: `Rc<T>` + interior mutability can
  form cycles that never reach refcount 0 and leak; the fix is manual `Weak`. Swift ARC: "you have to
  avoid/break reference cycles manually, or you leak memory." [Verified: Rust book ch15-06; Hylo/Swift
  comparison on lobste.rs.]
- **PHP split is real and stable.** Scalars copy-by-value; arrays are COW value types; objects are
  reference handles; `&` makes an explicit alias. [Verified: zetcode.com/php/copy-value-reference;
  php.net/manual/en/language.references.php.]
- **PHP 8.5 `clone with` is the object functional-update target.** `$new = clone $o with ['x'=>v];`,
  respects visibility & type constraints, designed for immutable value objects with `readonly` props.
  [Verified: wiki.php.net/rfc/clone_with; PHP 8.5 stable release notes.]
- **Kotlin `copy()` is shallow / shares mutable references** — a cautionary data point for what Model R
  buys you (accidental aliasing). [Verified: kotlinlang.org/docs/data-classes; medium follow-ups.]

## B. Implementation sketch (what Model V costs — for the downstream design track)

- **New AST:** `Stmt::Assign { target: AssignTarget, value: Expr }` where `AssignTarget ∈ { Local(name),
  Field(obj, name), Index(obj, idx) }`. New `mutable` flag on `VarDecl` (+ later a `Modifier::Mutable`).
  Checker enforces: target binding/field is `mutable`; type of `value` assignable to the place.
- **New `Op`: at most ONE** — `Op::SetField(name_idx)` and `Op::SetIndex` *may* be needed for the VM
  (field/element write); local reassignment reuses the **existing** `Op::SetLocal`. Each new `Op`
  triggers the three coupled matches (`exec_op`, `validate`, `stack_effect`) per Invariant #5 — budget
  for 0–2 new ops, no more. [Inferred from the `Op` set in `chunk.rs`.]
- **Value kernel:** a single-sourced `value::set_index` / `value::set_field` doing `Rc::make_mut` +
  write, called by both backends (Invariant #3 discipline). COW is here and only here.
- **No GC subsystem.** `Drop` continues to reclaim. The milestone ships *mutation only*; the
  "tracing-GC" deliverable is **struck** with the citation in §A as justification.
- **Transpile:** array/list/map writes → PHP array writes (1:1); field write on a `mutable` field → PHP
  property write; functional update of an immutable object → PHP 8.5 `clone … with`.

---

*STATUS: Designed — research/recommendation only, not implemented. The value-vs-reference choice is
forced to Model V by the existing no-aliasing/acyclic-heap commitment + Invariant #1; the one genuine
fork (reference escape hatch (a)/(b)/(c)) is flagged for STOP-and-ask before design freeze.*
