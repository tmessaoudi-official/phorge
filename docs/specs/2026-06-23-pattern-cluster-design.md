# Pattern cluster — design (guards + destructuring + flow-narrowing)

> Status: **approved design**, 2026-06-23. The post-M-RT language-ergonomics milestone (GA spine #5).
> Front-end-only: **no new `Op`, no `Value` change**; byte-identical `run ≡ runvm ≡ real PHP 8.4`.
> Continuation of the closed M-RT type system (unions, intersections, generics, generic enums) — it
> makes that type surface *ergonomic*. Plan + Decisions Log: `docs/plans/2026-06-23-pattern-cluster.plan.md`.

## 1. Goal & scope

Close the pattern-matching ergonomics gap a TS/Rust migrant expects, across three axes. The scope is
maximal ("Everything" — developer-chosen):

1. **Guards** — an optional per-arm condition on `match` arms and on `if`-let bindings.
2. **Payload destructuring** — un-reject nested type-patterns inside variant payloads
   (`Wrapper(Circle c)`), **plus** new class/struct field destructuring `Point { x, y }` with rename and
   full nesting.
3. **Flow-narrowing** — a unified engine covering negative/else narrowing, early-return narrowing,
   post-exhaustive-match narrowing, and equality/null/literal refinement.

**Philosophy fit** ([[philosophy-of-phorge]]): every form maps to legible, idiomatic PHP; nothing adds a
runtime surprise. Uniform nesting (axis 2) is chosen specifically to *remove* the surprise of an
arbitrary "patterns nest here but not there" asymmetry.

### Non-goals / deferrals
- `||`-disjunction narrowing (the intersection of two branch narrowings is usually empty) — documented,
  not implemented; may land later.
- Common-member access on a *raw* (un-narrowed) union — still requires narrowing first (pre-existing).
- Or-patterns (`A | B =>` in a single arm) — not in scope.
- Range/slice patterns — not in scope.

## 2. Current surface (verified)

| Axis | Already shipped | Gap this slice closes |
|---|---|---|
| Guards | none | `MatchArm` has no guard slot |
| Destructuring | enum-variant positional + nestable (`Circle(r)`), `Variant{fields:Vec<Pattern>}` | (a) type-pattern nested in a payload `Wrapper(Circle c)` rejected `E-MATCH-TYPE`; (b) no class/field destructuring (`Pattern::Struct` absent) |
| Narrowing | `if (x instanceof T)` narrows then-block; `if (var x = opt)` if-let; smart-cast shadow | else/negative, early-return, post-match, equality/null/literal — all absent |

Verified against `ast/mod.rs` (Pattern enum, MatchArm), `parser/patterns.rs`, `checker/stmt.rs`
(narrowing), `transpile/matches.rs` (lowering), `KNOWN_ISSUES.md` (the deferred-corner rows).

## 3. Grammar, AST & parser

**AST (`ast/mod.rs`):**
- `MatchArm` gains `guard: Option<Expr>` (`None` = unguarded, unchanged behavior).
- New `Pattern::Struct { type_name: String, fields: Vec<StructFieldPat>, span: Span }`.
  `StructFieldPat { field: String, target: FieldTarget }`; `FieldTarget` is:
  - `Bind(String)` — shorthand `{ x }` ⇒ `Bind("x")`; rename `{ x: px }` ⇒ `Bind("px")`.
  - `Sub(Pattern)` — nested `{ from: Point { … } }` recurses.
- `Pattern::Variant.fields` is unchanged (`Vec<Pattern>`); the checker simply stops rejecting a
  `Pattern::Type` element inside it.

**Parser (`parser/patterns.rs`, `parser/stmts.rs`):**
- Struct pattern: in pattern position, `PascalCaseIdent` followed by `{` ⇒ `Pattern::Struct`
  (one-token lookahead, mirrors `Circle(` ⇒ `Variant`). A field RHS that is a bare ident ⇒ `Bind`; a
  field RHS that is itself a pattern ⇒ `Sub`. Bare `{ x }` ⇒ `Bind("x")`.
- Guard: after an arm's pattern, a **contextual** `when` (recognized only here) parses a guard `Expr` up
  to `=>`. `when` is a normal identifier everywhere else (no global reservation — same contextual
  treatment as `as` for import aliasing).
- if-let guard: `if (var x = <init> when <cond>)` — the `when`-clause parses after the initializer,
  before `)`. Same for `while (var x = … when …)`.
- Nesting recurses through the existing `parse_pattern`, so depth is free.

## 4. Checker

### 4.1 Binding & structural checks
- Each `Bind` (shorthand/rename) and each nested leaf introduces an arm-scoped local, in scope for both
  the guard expr and the arm body. Duplicate bind names within one pattern ⇒ `E-PATTERN-DUP-BIND`.
- Struct pattern: `type_name` must resolve to a class (`E-STRUCT-PAT-TYPE` otherwise); each `field` must
  exist on that class (`E-STRUCT-FIELD-UNKNOWN`); a `Sub` pattern is checked recursively against the
  field's declared type. Each `Bind` is typed from the field's declared type — **including its `CTy`**
  (see §5, parity trap 1).

### 4.2 Exhaustiveness with guards
A **guarded** arm does not discharge its shape (the guard may be false), so an unguarded arm covering
that shape is still required for the match to be exhaustive; otherwise `E-MATCH-GUARD-EXHAUST`. Struct
patterns are single-type tests (not sum types), so they do **not** change exhaustiveness obligations —
nesting adds no new coverage surface (only the union/enum payload nesting does, which already exists as
the variant-exhaustiveness rule).

### 4.3 Flow-narrowing engine
Factor today's inline `Stmt::If` narrowing into one reusable helper:

```
narrow_from_condition(cond: &Expr, polarity: bool) -> Vec<(VarName, NarrowedTy)>
```

Callers:
- **then-block** applies `polarity = true`.
- **else-block** applies `polarity = false` (negation).
- **early-return**: if the then-block diverges (`block_terminates`, from the totality cluster), apply
  `polarity = false` to the **remainder of the enclosing block**.
- **post-exhaustive-match**: after a `match` *statement* where every arm but one diverges, narrow the
  scrutinee to the surviving arm's type for the rest of the block.

Recognized sources (true / false forms):

| Condition | true-branch | false-branch |
|---|---|---|
| `x instanceof T` | `x : T` | `x :` union∖T (`Ty::union_of` minus the member, re-normalized; no-op when `x` isn't a union) |
| `x == null` | `x : null` | `x :` non-null inner |
| `x != null` | `x :` non-null inner | `x : null` |
| if-let `var x = opt` | `x :` non-null inner | — |
| `x == <literal>` on a primitive-union `x` | that member | — |
| `!cond` | recurse, flipped | recurse, flipped |
| `a && b` | conjoin both | — (`\|\|` → no narrowing, documented) |

A narrowed binding installs a shadow that inherits the outer binding's mutability (same rule as the
existing S1 instanceof narrowing — M-mut.1 interaction preserved).

**Byte-identity**: narrowing is *purely* a checker concern. It changes nothing the backends emit — it
only lets more correct code type-check — so `run ≡ runvm ≡ PHP` is structurally untouched.

## 5. Backends & PHP lowering

The transpiler **already lowers every `match` to a PHP `if/elseif/else` ladder** (verified in
`transpile/matches.rs::emit_match`; type patterns become `instanceof` conditions). Guards and struct
patterns slot into that ladder.

**Guards — interpreter/VM**: after a pattern matches and binds, evaluate the guard in the arm scope; if
false, fall through to the next arm. VM: `pattern-test → bind → <guard> → JumpIfFalse next-arm`. Reuses
existing branch ops — **no new `Op`** (the coupled `exec_op`/`validate`/`stack_effect` trio is untouched,
[[op-variant-match-coupling]]).

**Guards — transpiler**: fold the guard into the `elseif` condition. Binds must be live for the guard, so
emit each as an always-true assignment conjunct *before* the guard conjunct:

```php
elseif ($subj instanceof Circle && (($c = $subj) || true) && ($c->r > 0)) { /* body */ }
```

`(($c = $subj) || true)` assigns `$c` then forces true, so the bind is safe for any value (incl. falsy
`0`/`""`/`null`). One conjunct per bind; PHP's function-level scope keeps the binds visible in the body
(identical to the unguarded case, where binds are emitted inside the block).

**Struct / nested patterns — all backends**: `Point { x, y }` ⇒ `instanceof` test + field-read binds
(`$x = $subj->x`). Nested (`Line { from: Point {…} }`) and nested-in-variant (`Wrapper(Circle c)`)
recurse as conjoined `instanceof` + field tests. Interpreter/VM reuse `Op::IsInstance` (S1) + existing
field-read ops.

**Parity traps (both have bitten this codebase):**
1. **CTy for field binds** ([[cty-tracks-operand-types]]): if a destructured field is used as an
   arithmetic operand (`Point { x, y } when x + y > 0`), each bind's `CTy` must be registered from the
   class field type, or the VM rejects what the interpreter accepts → silent `run`↔`runvm` break. The
   differential must include an `expr + 1` operand case over a destructured field/payload.
2. **Assignment-in-condition idiom** ([[php-transpile-floor-84]]): verify against the **real PHP 8.4
   oracle** (`PHORGE_PHP=…php-8.4… PHORGE_REQUIRE_PHP=1`), not just the permissive local php-master.

## 6. Diagnostics (all `phg explain`-documented)
- `E-MATCH-GUARD-EXHAUST` — a shape covered only by guarded arms, no unguarded fallback.
- `E-PATTERN-DUP-BIND` — duplicate bind name within one pattern.
- `E-STRUCT-FIELD-UNKNOWN` — struct pattern names a field the class lacks.
- `E-STRUCT-PAT-TYPE` — struct pattern applied to a non-class type.
- `E-MATCH-TYPE` (the "type patterns are top-level-only" rule) is **removed** for payload nesting.

## 7. Testing
- **Differential** (`tests/differential.rs`): guard fall-through, multiple same-shape arms, struct
  shorthand/rename/nested, nested-in-variant, the CTy operand cases, and runtime flow-narrowing paths
  (else / early-return / post-match / `== null`).
- **Checker** (`checker/tests/matching.rs`, by feature): the five diagnostics + accept/reject cases per
  narrowing source (else *does* narrow; `||` does *not*).
- **Parser** (`parser/tests/patterns.rs`, by construct): struct pattern, contextual `when`, nested,
  if-let guard.
- **Example** (per "examples ship with features", [[examples-ship-with-features]]):
  `examples/guide/pattern-matching.phg` — guards + struct destructuring + nesting + a flow-narrowing
  idiom in one runnable program; auto byte-identity-gated by the example glob + round-tripped through
  real PHP 8.4; `examples/README.md` index/coverage row.
- **KNOWN_ISSUES**: remove the now-fixed rows (negative/flow narrowing, type-pattern-nested-in-payload,
  "no flow-typing beyond structural termination"); record any genuine deferral (§1 non-goals).

## 8. Build cadence (detail in writing-plans)
Three green, byte-identical sub-slices, each committed independently (M-RT/error-model rhythm):
- **S5.1 — guards** (match + if-let): AST `guard` slot, contextual `when` parse, exhaustiveness-with-guards,
  3-backend eval/lowering, example.
- **S5.2 — struct/nested destructuring**: `Pattern::Struct`, payload-nesting un-reject, 3-backend
  lowering, the CTy field-bind trap, example extension.
- **S5.3 — flow-narrowing engine**: the unified `narrow_from_condition`, negative/else, early-return
  (reusing `block_terminates`), post-match, equality/null/literal refinement (checker-only).

## 9. Byte-identity statement
No new `Op`, no `Value` change, no global keyword. Guards lower to existing branch ops; struct/nested
patterns to `IsInstance` + field reads; narrowing is checker-only. Every shipped form is gated
`run ≡ runvm ≡ real PHP 8.4` by the differential harness + the example glob.
