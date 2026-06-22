# S6 — Multiple Inheritance (`extends`) Design

**Milestone:** M-RT (Rich Types), slice S6.
**Status:** Designed — not yet implemented.
**Supersedes the roadmap's S6 stub** ("`extends`(final-default)/`abstract`/LSB"): this spec replaces
the single-inheritance framing with **explicit-resolution multiple inheritance**, the model the research
(below) found to be the only one both spine-safe and idiomatic on a PHP target.

Phorge is a statically-typed PHP-inspired language in Rust with three backends that must stay
**byte-identical** — tree-walking interpreter (`run`), stack bytecode VM (`runvm`), and a Phorge→PHP
transpiler whose output runs under real PHP 8.4. That byte-identical spine is the non-negotiable
constraint every decision below is measured against.

---

## Research basis

A 5-agent research sweep (raw findings in `docs/research/s6-mi/raw/`: `cpp.md`, `python-ruby.md`,
`scala-eiffel.md`, `php-traits.md`, `prior-art.md`) studied how multiple inheritance is lowered onto
single-inheritance targets. Unanimous conclusions:

- **Nobody faithfully compiles *true* MI to PHP.** Haxe, Hack/HHVM, and every other PHP-targeting system
  use traits + interfaces instead, and the field treats that substitution as sufficient. — [Verified:
  prior-art agent]
- **PHP confirms the lowering works** (an agent ran PHP 8.4.22): `class C implements IA, IB { use TA, TB; }`
  makes `$c instanceof IA` **and** `$c instanceof IB` both true and carries fields + static fields +
  constants + methods from both parents. — [Verified: php-traits agent, executed]
- **The Schärli/Ducasse "Traits" model (ECOOP 2003) — flat composition + explicit conflict resolution —
  is academically *preferred* to linearized MI**, not merely a PHP-forced compromise. It maps almost 1:1
  onto PHP `insteadof`/`as`. — [Verified: prior-art agent]
- **C3 linearization + cooperative `super` is disqualified for the spine**: PHP has no MRO and no
  `call-next-method`, so it would require synthesized non-idiomatic forwarding scaffolding, a likely new
  `Op`, and the MRO+super algorithm reproduced identically across three backends — maximal byte-identity
  drift surface. — [Verified: python-ruby + scala-eiffel + prior-art agents all ranked it last]

---

## Decisions Log

- [2026-06-22] AGREED (developer): **pursue multiple inheritance** in S6 (not single `extends` + traits).
  Developer rejected the "single now, traits at S8" framing twice and asked for real research first.
- [2026-06-22] AGREED: **Model 1 — explicit-resolution MI** ships in S6. `class C extends A, B`; compose
  all members from every parent; a cross-parent name collision is a compile error unless `C` resolves it
  (override / `use B.foo` / `rename A.foo as g` / exclude). Lowers to PHP `implements IA,IB { use TA,TB
  { …insteadof/as… } }`. Front-end-only, **no new `Op`, no `Value` change**, byte-identical by
  construction (same discipline as `erase_generics`/`expand_aliases`).
- [2026-06-22] AGREED: **Model 3 — C3 + cooperative `super` — is deferred to a future gated milestone**,
  to be researched/designed *after* traits (S8), because cooperative super over multiple parents is the
  same machinery as trait linearization (Scala's model). Honest expectation set: the future evaluation
  may still conclude the spine cost isn't worth it. Keeping the door open costs nothing.
- [2026-06-22] AGREED (forward-compat guarantee): **`super`/`parent` under multiple parents is a clean
  compile error in S6** (`E-MI-SUPER-AMBIGUOUS`). This reserves the syntax so a future Model-3 milestone
  is *purely additive* (only ever grants meaning to what was an error) — no breaking change to shipped
  code. Single-inheritance `parent`/`super` keeps its normal PHP meaning.
- [2026-06-22] AGREED: **final-by-default + `open`** (Kotlin's model; the GA-direction modifier decision,
  unpaused). Classes and methods are **final-by-default**; `open` is the single opt-in for
  extensibility/overridability. The `final` keyword is **retired** (redundant with the default). Rationale:
  *consistency* with the already-accepted immutable-by-default house rule ("safe by default, opt into
  power with one keyword") outweighs raw PHP-familiarity; learning one rule once and applying it
  everywhere is itself a removal of surprise. Counterweight acknowledged (B = PHP-familiar open-by-default
  is more first-contact-friendly for PHP devs); A chosen for internal consistency.
- [2026-06-22] CLARIFIED (not a new decision — confirms shipped M-mut semantics): `open`/`final`
  (extensibility) and `mutable` (mutability) are **orthogonal axes that never interact**. `open`/`final`
  applies to **classes + methods only**; **fields** use the `mutable` axis (immutable by default,
  `mutable` opt-in); **free functions** are untouched by `open`/`final` (no override surface). Immutable
  **value-type collections** (`List`/`Map`/`Set`/`Bytes`, COW) are *deeply* frozen — neither reassignable
  nor element-settable — strictly stronger than PHP/JS `readonly`/`const`. Immutable **instance** bindings
  are shallower (handle semantics): can't rebind, but `mutable` fields still mutate through the binding.

---

## Syntax

```
open class Animal {
  name: string
  open function describe() -> string => "an animal named {this.name}"
}
open class Swimmer extends Animal { function move() -> string => "swims" }
open class Flyer   extends Animal { function move() -> string => "flies" }

class Duck extends Swimmer, Flyer {
  use Swimmer.move          // explicit resolution of the move() collision
  // alternatives: `rename Flyer.move as glide`  |  `exclude Flyer.move`  |  override `function move()…`
}
```

- `ClassDecl` gains `extends: Vec<String>` (reuses the existing `Extends` token — today only interfaces
  use it).
- A parent must be `open` (else `E-EXTEND-FINAL`).
- Resolution clauses (`use`/`rename … as`/`exclude`) appear in the class body, before/among members.
- `open` is a class- and method-level modifier; `final` keyword retired.

## Semantics

1. **Composition.** `C` inherits all fields, static fields, constants, and methods from every parent and
   their transitive ancestors.
2. **Diamond shared base.** A member reached through two arms **auto-merges only when both arms resolve to
   a byte-identical member** (e.g. `Animal.describe` reached via both `Swimmer` and `Flyer`). Any real
   divergence is a collision. — [Verified: php-traits agent — PHP auto-dedups only byte-identical
   flattened members]
3. **Method collision** (same name from ≥2 parents, not byte-identical) → `E-MI-CONFLICT` until `C`
   resolves it via `use P.m` (pick), `rename P.m as n` (alias/keep both), `exclude P.m` (drop), or by
   redefining `m` in `C` (override — requires the parent `m` to be `open`).
4. **Field collision** (same-named field from ≥2 parents) → `E-MI-FIELD-CONFLICT`. PHP has **no
   `insteadof` for properties** and any divergent property is fatal, so S6 resolves field collisions in
   the checker (rename in a parent, or `C` redeclares the field). The Scala accessor-lowering trick
   (abstract accessors in the interface + one backing field in the concrete class, turning a property
   collision into a resolvable *method* collision) is an optional later refinement; **start strict.**
   — [Verified: php-traits + scala-eiffel agents]
5. **Constructors.** PHP never chains two trait constructors. Each parent constructor lowers to a
   uniquely-named init method; `C` gets a **synthesized orchestrating constructor** that calls each
   parent's init in `extends`-list order, then runs `C`'s own constructor body. — [Verified: php-traits
   agent ran this pattern]
6. **Subtyping / `instanceof`.** `C` is a subtype of every parent and their ancestors/interfaces. The
   existing `ast::class_implements` interface-flattening oracle generalizes to a **`class_supertypes`**
   transitive closure (cycle-checked → `E-MI-CYCLE`), threaded through `Ty::assignable_with`. `instanceof`
   accepts any ancestor with smart-cast narrowing (S1/S4 machinery).
7. **`super`/`parent` under MI** → `E-MI-SUPER-AMBIGUOUS` (reserved; see Decisions Log). Single-parent
   `extends` keeps normal `parent`/`super`.
8. **`open`/`final`.** A non-`open` class extended → `E-EXTEND-FINAL`. A non-`open` (final-by-default)
   method overridden in a subclass → `E-OVERRIDE-FINAL`. An `abstract` method (S6b) is implicitly `open`.
   `open` on a `static` method → error (statics are not virtual). These last two confirmed in S6b.

## Lowering to PHP

- **Single-parent `extends`** (one parent) → plain PHP `class C extends A` — **no trait machinery**, so
  ordinary single inheritance stays byte-identical to a hand-written class.
- **Multi-parent** → each parent emits as **interface `I<Name>` (type side) + trait `T<Name>` (impl
  side)**; `class C extends A, B` → `class C implements IA, IB { use TA, TB { …insteadof/as… } }`.
  Resolution clauses emit the corresponding `insteadof`/`as`. The synthesized constructor orchestrates
  parent inits.
- A non-`open` class → PHP `final class`; an `open` class → plain `class`. Non-`open` method → PHP
  `final` method (when not under MI-trait lowering, where `final` in a trait is still legal).

## Implementation discipline

All composition, collision detection, resolution, and flattening happen in the **checker / loader,
before any backend runs** — the program each backend sees is already disambiguated to a single concrete
target per `(class, member)`. Therefore:

- **No new `Op` variant, no `Value` change.** Backends consume a flat, resolved method/field table — the
  interpreter's `call_method`, the VM's `Op::CallMethod` + `method_overloads` table, and the transpiler
  all operate on resolved members exactly as today.
- Byte-identity is **structural, not tested-for** (same property that made interface-MI and intersections
  free in S2/S4/S5).
- The compiler **pre-flattens inherited methods** into the bytecode `methods`/`method_overloads` tables
  (zero-cost dispatch); the interpreter resolves via the parent chain on the resolved AST.

## Diagnostics (each self-documents via `phg explain`)

`E-EXTEND-FINAL` · `E-OVERRIDE-FINAL` · `E-MI-CONFLICT` · `E-MI-FIELD-CONFLICT` · `E-MI-CYCLE` ·
`E-MI-SUPER-AMBIGUOUS` · (S6b) `E-ABSTRACT-INSTANTIATE` / `E-ABSTRACT-UNIMPL`.

## Sub-slices (each a green, byte-identical commit)

- **S6a — single `extends` + override + the `open`/`final` model.** `ClassDecl.extends`; parser; checker
  `class_supertypes` closure + subtyping through `assignable_with`; method override + `E-OVERRIDE-FINAL`;
  `open` modifier + `E-EXTEND-FINAL`; retire the `final` keyword; constructor chaining for the single
  parent (`super(...)` allowed here); interpreter parent-chain lookup; compiler pre-flatten; transpiler
  `extends Parent` / `final class`. Guide example: single inheritance + override.
- **S6b — multi-parent compose + resolution + `abstract`.** `extends A, B, …`; `E-MI-CONFLICT` +
  `use`/`rename`/`exclude` clauses; `abstract` classes/methods (`E-ABSTRACT-INSTANTIATE`/`-UNIMPL`);
  `E-MI-SUPER-AMBIGUOUS`. Transpiler interface+trait decomposition + `insteadof`/`as`. Guide example: the
  diamond with an explicitly-resolved method collision.
- **S6c — field/ctor composition + diamond + full subtyping.** `E-MI-FIELD-CONFLICT`; synthesized
  orchestrating constructor; diamond auto-merge of byte-identical members; `instanceof`/assignability
  against every ancestor with smart-cast. Guide example: multi-parent with state + `instanceof`.

## Scope (S6) and deferrals (→ KNOWN_ISSUES)

**In scope:** `package main` classes; single-parent fast path; multi-parent compose with explicit
resolution; `open`/`final`; `abstract`; field/ctor/diamond composition; subtyping/`instanceof`.

**Deferred:**
- Cooperative `super` / C3 linearization (Model 3) — future gated milestone, after S8 traits.
- Cross-package MI parents (S6 is `package main`-only, mirroring earlier slices).
- The Scala accessor-lowering trick for field collisions (start strict instead).
- Generic-class MI parents (`class C<T> extends Box<T>`), generic-method override variance.
- Method-signature **variance** on override (S6 requires exact-match or covariant-return; contravariant
  params deferred).
- Visibility × inheritance narrowing rules beyond the existing checks.

## Acceptance

For each sub-slice: byte-identical `run ≡ runvm ≡ real PHP 8.4` for its guide example
(`examples/guide/inheritance*.phg`, glob-gated by `tests/differential.rs`); full lib + PHP-oracle
differential + integration suite green on the PHP-8.4 floor; clippy + fmt clean; **no new `Op`**; every
new diagnostic code documented by `phg explain`.

## Rollback

Each sub-slice is an isolated commit; revert the offending commit. S6a is the broad refactor (subtyping
oracle generalization + `open`/`final`); if it destabilizes, `git revert` restores the
interface-only oracle and the `final`-keyword surface.
