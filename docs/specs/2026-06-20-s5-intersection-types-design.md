# S5 — Intersection Types `A & B` — Design

> Status: **IMPLEMENTED — S5 COMPLETE.** Front-end-only, **zero new `Op`/`Value`** (the dual of S4
> unions). Hard-depends on S2 (interfaces + nominal subtyping + `class_implements`); composes with S4.
> Transpile target: PHP 8.1 pure intersection types `A&B` (oracle runs php-master 8.6).
>
> **As-built decisions (developer-resolved, supersede §10's recommendations):**
> - **D1 = ≤1 concrete class + N interfaces** (NOT interface-only). The developer challenged the
>   interface-only line: nothing *forbids* a class member; under nominal single-class-per-value typing
>   `C & D` (two classes) is provably the bottom type ∅, but `C & I & J` (one class + interfaces) is
>   inhabitable (and becomes load-bearing once S6 `extends` lands). So members are interfaces plus **at
>   most one** class; two or more classes → `E-INTERSECT-MULTI-CLASS`; a primitive/enum/optional/function
>   member → `E-INTERSECT-MEMBER`.
> - **D2 = require-agreement, `E-INTERSECT-SIG`** (NOT first-member-wins). Two members declaring a shared
>   method with differing signatures cannot both be satisfied by one class (Phorge has no overloading
>   *yet*), so the intersection is uninhabited and is rejected at the type site. **Overloading is now
>   confirmed for M-RT, sequenced immediately after S5** (lowers to one dispatching PHP method); when it
>   lands, this rule is revisited.
> - **D3 = autonomous** (design → implement → commit in one green byte-identical slice).
>
> The rest of this document is the original design; where §2/§5/§6/§10 say "interfaces only," read
> "interfaces plus at most one class" per D1 above.

## 1. Goal

An intersection type `A & B` is a value that satisfies *both* `A` and `B` simultaneously — the
**dual** of a union. Where a union widens (`Circle` → `Circle | Square`), an intersection narrows: a
parameter `function render(Drawable & Serializable x)` accepts only a value whose class implements
**both** interfaces, and inside, *both* interfaces' methods are callable on `x`. Maps 1:1 to PHP 8.1's
pure intersection type `A&B`. Like unions/interfaces/generics, it is a **type-only** feature: the
backends run on the concrete instance; the annotation gates the checker and the PHP signature only.
**No new `Op`, no `Value` change.**

## 2. The interface-only constraint (the central semantic decision)

A value's runtime identity in Phorge today is its class plus the interfaces that class implements
(there is no class `extends` until S6, so no class is a subtype of another class). Therefore:

- `I & J` (two interfaces) is **inhabitable** — a class can `implement I, J`.
- `C & I` (a concrete class and an interface) is inhabitable **only if `C` itself implements `I`** — in
  which case `C & I` ≡ `C` (no new information). Allowing it adds redundancy, not expressiveness.
- `C & D` (two distinct concrete classes) is **uninhabited** — no value can be both, since a class is a
  subtype of no other class. PHP 8.1 likewise forbids a value being two unrelated concrete classes.

**Recommendation (D1): restrict intersection members to interfaces** for S5. This matches PHP's pure
intersection types (which are interface-oriented), keeps the inhabited-ness rule trivial ("members are
distinct interfaces"), and avoids the `C & I` ≡ `C` redundancy and the `C & D` uninhabited trap. A
non-interface member is a clean `E-INTERSECT-MEMBER`. (Alternative in §10.)

This is the mirror of S4's coherence boundary (unions = classes/interfaces/primitives; intersections =
interfaces). It also makes S5 strictly smaller than S4: **no new pattern kind** (an intersection is not
a sum, so there is no match-over-intersection), and **no instanceof change required** for v1 (testing
`x instanceof (A & B)` is deferred — see §6).

## 3. Syntax & lexing

- **New token `TokenKind::Amp`** for a lone `&`. The lexer's two-char dispatch already claims `&&`
  (`AndAnd`); add the single-char fallthrough `b'&' => Amp` (exactly as S4 added `Bar` for `|`). A lone
  `&` is currently a lex error, so this only *adds* acceptance.
- **Precedence: `&` binds tighter than `|`** (the TypeScript/PHP convention): `A | B & C` ≡
  `A | (B & C)`. Restructure `parse_type` into three levels:
  - `parse_type_atom` — a single `Named`/`Function` with trailing `?` (today's atom; unchanged).
  - `parse_type_intersection` — `parse_type_atom` then a loop on `Amp`, wrapping ≥2 members in
    `Type::Intersection`.
  - `parse_type` (the union level) — `parse_type_intersection` then a loop on `Bar`, wrapping ≥2 in
    `Type::Union`.
  A single atom returns unchanged at each level, so a non-intersection/non-union program's AST is
  byte-for-byte identical. (S4 already inserted the union loop; S5 slots the intersection loop *under*
  it.)
- `?` binds to its immediate atom: `A & B?` ≡ `A & (B?)` — but an optional intersection member is
  rejected (§6), so this parses then fails the checker, mirroring S4's `A | B?` handling.

## 4. AST & resolved type

- `ast::Type::Intersection(Vec<Type>, Span)` — parser output, members in source order.
- `types::Ty::Intersection(Vec<Ty>)` — **normalized** by a new `Ty::intersection_of` (the exact mirror
  of S4's `Ty::union_of`): flatten nested intersections, dedupe, canonical-sort by `Display`; a
  1-member collapse *is* that member. `Display`: `A & B & C` (canonical order). The single shared
  normalizer guarantees `A & B` and `B & A` are the same `Ty`.

## 5. Checker

- **`resolve_type`** (`Type::Intersection` arm): resolve each member; require each to be a **declared
  interface** (`E-INTERSECT-MEMBER` otherwise — naming a class, enum, primitive, optional, or function);
  then normalize → `Ty::Intersection`. If the normalized result collapses to a single member (≥2 source
  members were duplicates), that is `E-INTERSECT-ARITY` ("an intersection needs two or more distinct
  types") — the mirror of `E-UNION-ARITY`.
- **`assignable_with`** (thread the existing subtype oracle) — the **dual** of the S4 union arms,
  inserted right after them:
  - `to = Intersection(ts)`: `from` fits iff it fits **every** member — `ts.iter().all(|t| assignable(from, t))`.
    So a concrete `Dog` flows into `Drawable & Serializable` iff `Dog` implements both (each via the
    `is_subtype` oracle). (all-members-required-in)
  - `from = Intersection(fs)`, `to` non-intersection: `from` fits iff **some** member fits `to` —
    `fs.iter().any(|f| assignable(f, to))`. So `A & B → A` and `A & B → B` both hold (an `A & B` value
    has all of `A`'s capabilities). (some-member-out)
  - `from = Intersection(fs)`, `to = Intersection(ts)`: every `to`-member must be satisfied by some
    `from`-member — `ts.iter().all(|t| fs.iter().any(|f| assignable(f, t)))` (so `A & B & C → A & B`).
  - `Error` still unifies both ways.
  Ordering caution: place the intersection arms so they compose with the S4 union arms — a
  `Union ↔ Intersection` mix is handled by the recursion (e.g. `A → (B|C) & D` checks `A → B|C` AND
  `A → D`). Add focused tests for at least one union∩intersection cross-case.
- **Member access on an intersection** (the one genuinely new mechanism vs. S4). `check_method_call`
  and `check_member` gain a `Ty::Intersection(members)` arm in their `base`/receiver match: search each
  member interface's flattened method set (`iface_flat_methods`, already used for an interface-typed
  receiver in the `Ty::Named`-is-interface branch) and resolve the method/field from the **first member
  that has it**; if none do, `E-INTERSECT-NO-MEMBER` ("no member of `A & B` has method `m`"). A method
  present in two members with *different* signatures is an ambiguity — for v1, **first-member-wins**
  (document it; a stricter "must agree" check is a follow-up). Because every member is an interface and
  interface dispatch is already polymorphic-through-the-concrete-class, no runtime change is needed.
- **`erase_generics` / `expand_aliases` / loader `resolve_type`**: add a `Type::Intersection` arm that
  maps over members (mirrors the `Type::Union` arms S4 added) — so an alias, a generic param, or a
  cross-package interface name *inside* an intersection resolves/erases like anywhere else.
- **Casing**: members are interface names, validated at their declaration; no new casing rule.

## 6. Deferred corners (→ KNOWN_ISSUES), kept out of v1 by clean rejection

- **Non-interface members** (`C & I`, `C & D`, `int & X`) — `E-INTERSECT-MEMBER`. (`C & I` redundancy
  and `C & D` uninhabited-ness are the reasons; revisit when class `extends` lands in S6.)
- **`instanceof` against an intersection** (`x instanceof (A & B)`) — deferred. The S1 `Op::IsInstance`
  carries a single name; an intersection test would need either a new op or a lowering to
  `x instanceof A && x instanceof B`. Out of scope for v1 (KNOWN_ISSUES already lists "instanceof
  against an intersection" as pending). No match-over-intersection either (not a sum type).
- **Optional/function intersection members** and **whole-intersection optional `(A & B)?`** — rejected,
  mirroring S4.
- **Signature-conflict diagnostic** for a method shared by two members with differing signatures —
  first-member-wins for now (a `E-INTERSECT-SIG` refinement is a follow-up).

## 7. Backends (all unchanged at the `Op` level)

- **Compiler `resolve_cty`**: `Type::Intersection(..)` → `CTy::Other` (not a specialized arithmetic
  operand) — the same one-line arm S4 added for `Type::Union`.
- **Transpiler `emit_type`**: `Type::Intersection(members)` → `members.map(emit_type).join("&")` in
  canonical order, each via the existing `php_type_ref` (cross-package members emit their FQN). PHP 8.1
  parses `Drawable&Serializable`, `\Acme\A&\Acme\B`. Dedup defensively (the checker already guarantees
  ≥2 distinct interface members). **No new `Op`, no `Value` change.**
- **Interpreter / VM**: never see an intersection as a *value* shape (a value is always a concrete
  instance); the annotation is checker + PHP-signature only. **Zero changes** — member calls dispatch
  through the concrete instance's class exactly as an interface-typed receiver does today.

## 8. Example + gate

`examples/guide/intersections.phg` — two interfaces (e.g. `Drawable { draw() -> string }` and
`Named { name() -> string }`), a class implementing **both**, a function
`function describe(Drawable & Named x) -> string` that calls a method from *each* interface on `x`
(showing both are in scope), and a value of the implementing class flowing in. Output deterministic
(strings/ints, no irrational floats). Byte-identical `run ≡ runvm ≡ real PHP`; auto-gated by the
`examples/**/*.phg` glob. Checker unit tests: all-members-required-in assignability (a class
implementing both fits; a class implementing one does not), some-member-out (`A & B → A`),
arity/member rejections (`E-INTERSECT-MEMBER`/`E-INTERSECT-ARITY`), member access resolving a method
from each member, and a union∩intersection cross-assignability case. **No new `Op`** → no
bytecode-surface risk. `phg explain` entries for every new code.

## 9. Why this is smaller than S4

| Aspect | S4 unions | S5 intersections |
|---|---|---|
| New token | `Bar` (`\|`) | `Amp` (`&`) |
| New `Ty`/`Type` variant | `Union` | `Intersection` |
| New pattern kind | `Pattern::Type` (match-over-union) | **none** (not a sum) |
| `instanceof` change | accept union operand | **none** (intersection-instanceof deferred) |
| Member access | n/a (narrow first) | **new** — search all member interfaces |
| New `Op` | none | none |
| Normalizer | `union_of` | `intersection_of` (mirror) |

The only genuinely new logic is member-access-over-an-intersection (§5) and the dual assignability arms;
everything else mirrors S4 mechanically.

## 10. Open decisions for the developer

- **D1 — member kinds.** Recommended: **interfaces only** (matches PHP pure intersections; inhabited
  by construction; no `C & I` ≡ `C` redundancy). Alternative: also allow a single concrete class plus
  interfaces (`C & I & J`), rejecting only the two-concrete-classes case — closer to PHP's literal
  grammar but adds the redundancy/uninhabited edge cases with little expressive gain pre-`extends`.
- **D2 — method-signature conflict.** Recommended: **first-member-wins** silently for v1 (document it),
  add a strict `E-INTERSECT-SIG` later. Alternative: require all members declaring a shared method to
  agree on its signature now.
- **D3 — pace.** Autonomous (design→implement→commit in one green byte-identical slice, as S4 ran), or
  gated per phase.
