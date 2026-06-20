# S4 — Union Types `A | B` — Design

> Status: **DESIGN — not implemented** (developer chose "design first, then review"). The next M-RT
> slice after generics-all (CLOSED, `169c259`). Depends on S1 (`instanceof` + narrowing) and S2
> (interfaces + nominal subtyping). Transpile target: PHP 8.0 **native union types** (`A|B`).

## 1. Goal

A union type `A | B` is a value that is *one of* several nominal/primitive types — the open
composition counterpart to the closed, owned `enum`. `function describe(Circle | Square s)` accepts
either; inside, `if (s instanceof Circle) { … }` narrows it (S1/S2 machinery). Maps 1:1 to PHP 8.0's
native `A|B`. **No new `Op`** — it is a type-only feature (like interfaces/generics): the backends
run on the narrowed/erased shape, the union annotation only gates the checker and the PHP signature.

## 2. The central scoping decision (recommended split)

The plan groups "union types" with "match-over-union exhaustiveness." Grounding the code shows these
are **very different sizes**:

- **Union types + `instanceof` narrowing** — type-only, reuses S1/S2 narrowing verbatim, transpiles
  to PHP `A|B`. A clean, shippable, zero-`Op`, front-end-mostly slice.
- **match-over-union** — Phorge's `match` has **no type pattern** today (patterns are wildcard /
  binding / literal / null / enum-variant only). Matching `match s { Circle c => …, Square sq => … }`
  needs a brand-new `Pattern::Type` variant threaded through the parser, checker (binding + narrowing +
  exhaustiveness over the union's member set), **and all four backends** (interpreter, VM, transpiler)
  — the VM/transpiler must emit a runtime type test per arm. That is its own slice-sized surface.

**Recommendation: ship S4 as "S4a = union types + `instanceof` narrowing" now; defer match-over-union
type patterns to "S4b."** Rationale: unions are genuinely useful with `instanceof` alone (the dominant
use), S4a is byte-identity-trivial (type-only), and bolting a new pattern kind on in the same commit
multiplies risk across every backend. S4b also unlocks the `W-INSTANCEOF-CHAIN` lint (it can only
"nudge toward `match`" once match-over-union exists). The rest of this spec details **S4a**; §8
sketches S4b.

## 3. Syntax & lexing

- **New token `TokenKind::Bar`** for a lone `|`. The lexer already special-cases `|>` → `Pipe` and
  `||` → `OrOr`; add the fallthrough `(b'|', _) => Bar`. Unambiguous: `A|B` lexes `Bar`, `x |> f`
  lexes `Pipe`, `a || b` lexes `OrOr`. (A lone `|` is currently a lex error, so this only *adds*
  acceptance — no existing program changes.)
- **`parse_type`**: after parsing a single type (the existing `Named`/`Function` + trailing `?`
  logic), loop `while self.eat(&Bar) { members.push(parse one type) }`. With ≥1 `|`, wrap the
  collected members in `Type::Union(members, span)`; otherwise return the single type unchanged (so a
  non-union program's AST is byte-for-byte identical). `?` binds to its immediate member
  (`A | B?` ≡ `A | (B?)`); whole-union optional `(A | B)?` is **not** expressible this slice (§6).

## 4. AST & resolved type

- `ast::Type::Union(Vec<Type>, Span)` — parser output, members in source order.
- `types::Ty::Union(Vec<Ty>)` — **normalized**: flatten nested unions, dedupe, and sort into a
  canonical order (by `Display` string) so `A|B` and `B|A` are the *same* `Ty` and equality/assign
  are order-independent. A union that collapses to a single member (after dedupe) **is** that member
  (so `A | A` ≡ `A`). `Display`: `A | B | C` (canonical order).

## 5. Checker

- **`resolve_type`** (`Type::Union` arm): resolve each member; reject (clean diagnostics) —
  `E-UNION-ARITY` (< 2 distinct members after dedupe — "a union needs two or more distinct types"),
  `E-UNION-MEMBER` (a member is itself optional `T?` or a function type — keep v1 members to
  nominal/primitive; see §6), then normalize → `Ty::Union`.
- **Member kinds (recommended):** allow classes, interfaces, enums, **and primitives**
  (`int | string`) — all are valid PHP 8.0 union members and TS-idiomatic. (Open decision D1 below if
  you'd rather restrict to nominal types for the enum-vs-union coherence rule.)
- **`assignable_with`** (thread the existing subtype oracle):
  - `to = Union(ts)`: `from` fits iff — `from = Union(fs)` → every `f` fits some `t`; else `from`
    (non-union) fits some `t`. (member-in / subset-in)
  - `from = Union(fs)`, `to` non-union: every `f` must fit `to` (so `A|B → I` holds when both
    implement interface `I`). (all-members-out)
  - `Error` still unifies both ways (poison).
- **`instanceof`**: extend the left-operand check (today `Ty::Named(..) => ok`) to accept
  `Ty::Union(..)` too. Narrowing is **unchanged** — `if (x instanceof Circle)` already declares `x :
  Circle` in the then-branch (S1/S2), which is exactly right for a `Circle | Square` operand. (Else-
  branch / negative narrowing — removing `Circle` to leave `Square` — is flow-narrowing, deferred.)
- **Direct member access on a raw union** (`(A|B).foo()` without narrowing) is **rejected** this slice
  (`type 'A | B' has no method/field 'foo' — narrow with 'instanceof' first`). Common-member access
  (allowed when *every* member has a compatible `foo`) is a deferred refinement; if A and B share a
  method you would normally type the slot as their interface (S2), not a union.
- **`erase_generics` / `expand_aliases` / loader `resolve_type`**: add a `Type::Union` arm that maps
  over members (so a type alias, a generic param, or a cross-package type name *inside* a union
  resolves/erases like anywhere else — mirrors the existing exhaustive `Type` walks).

## 6. Deferred corners (→ KNOWN_ISSUES), kept out of v1 by clean rejection

- **`(A | B)?` (whole-union optional)** — not expressible (`?` is postfix on a single type). Use a
  member that is itself optional, or model nullability separately. `A | B?` parses as `A | (B?)`.
- **`T | null`** — `null` is not a type name; the optional form is `T?`. A `| null` member is an
  unknown-type error. (Coherence: `T?` is THE nullable form.)
- **Union members that are optional or function types** — rejected (`E-UNION-MEMBER`) to keep the PHP
  emission simple (PHP forbids nullable/`mixed` inside unions in several positions).
- **No flow-negative narrowing**, no common-member access on a raw union (above).

## 7. Backends (all unchanged at the `Op` level)

- **Compiler `resolve_cty`**: `Type::Union(..)` → `CTy::Other` (a union value is not a specialized
  arithmetic operand). Note the *same* `CTy`-operand boundary as interfaces/generics: after
  `instanceof` narrowing the checker knows `x : Circle`, but the compiler's local `CTy` for `x` stays
  the declared union (`Other`), so `x.radius + 1` inside the narrowed branch type-checks yet the VM
  rejects it (bind to a typed local first). Pre-existing behavior for interface narrowing — document,
  do not "fix" here. **No new `CTy` variant.**
- **Transpiler `emit_type`**: `Type::Union(members)` → `members.map(emit_type).join("|")` in canonical
  order, each member via the existing `php_type_ref` (so cross-package members emit their FQN). PHP 8.0
  parses `Circle|Square`, `int|string`, `\Acme\Geo\A|\Acme\Geo\B`. Dedup already guarantees no
  `int|int`. **No new `Op`, no `Value` change.**
- **Interpreter / VM**: never see a union as a *value* shape (a value is always a concrete instance);
  the union annotation is checker + PHP-signature only. Zero changes.

## 8. S4b sketch (match-over-union — deferred, for visibility)

A new `Pattern::Type { type_name, binding, span }` (`Circle c => …`): parser (a PascalCase head in
pattern position with a lowercase binder), checker (binds `c : Circle`, narrows, and computes
exhaustiveness over the union's *member set* — like enum-variant exhaustiveness today), and the
backends emit a per-arm runtime `instanceof` chain (interpreter/VM reuse the S1 `Op::IsInstance`;
transpiler emits PHP `match(true) { $x instanceof Circle => …, … }` or an if-chain). This is where
`W-INSTANCEOF-CHAIN` (warn on a ≥3-type `if/instanceof` chain) earns its place. Sized as its own
slice.

## 9. Example + gate (S4a)

`examples/guide/unions.phg` — a `Circle | Square` (two classes, each with `area()` — or just distinct
fields), a function taking the union, `instanceof` narrowing in `if` to reach a member, a value of each
type flowing in. Output deterministic (integer/string fields, no irrational floats). Byte-identical
`run ≡ runvm ≡ real PHP`; auto-gated by the `examples/**/*.phg` glob. Checker unit tests: member-in
assignability, all-members-out to a shared interface, arity/member rejections, `instanceof` narrowing
on a union, raw-union member-access rejection. **No new `Op`** → no bytecode-surface risk.

## 10. Open decisions for the developer

- **D1 — primitive union members.** Recommended: **allow** (`int | string`, PHP/TS-idiomatic).
  Alternative: nominal-only (classes/interfaces/enums) to sharpen the enum-vs-union coherence rule.
- **D2 — the S4a/S4b split (§2).** Recommended: **ship S4a now, defer match-over-union (S4b).**
  Alternative: one big S4 including the new `Pattern::Type` across all backends.
- **D3 — pace.** Autonomous (design→implement→commit) once these are settled, or gated per phase.
