# Phorge M1 — Type-Checker Design

- **Date:** 2026-06-15
- **Status:** Design approved — ready for implementation planning (Plan 4)
- **Scope:** M1 Plan 4 of the 6-plan roadmap (lexer ✓ → parser-expr ✓ → parser-stmt/decl ✓ → **type-checker** → evaluator → integration)
- **Parent spec:** `docs/specs/2026-06-15-phorge-language-design.md` (§3 type system is the contract)

---

## 1. Goal

A sound static type-checker that walks the existing AST (`Program`) and rejects ill-typed
programs **before** the Plan 5 evaluator runs. It adds **no new syntax** — it only checks
what the Plan 1–3 lexer/parser already produce. The frozen §6 sample program must
type-check with zero errors; deliberately broken variants must produce precise errors.

## 2. Scope decision — "sample-faithful core"

The §6 sample exercises only a subset of the frozen §3 type system. M1 implements the
subset that makes the sample **and close variants** type-check soundly, and emits clean
*"not yet supported in M1"* errors for the rest. Nothing unchecked is ever silently
accepted (soundness is preserved).

**In scope (M1):** enums + variant constructors; exhaustive `match` + destructuring +
binding; classes with fields, methods, constructor calls; built-in generic types
(`List`/`Map`/`Set`) with element-type checking; function/method calls with
arity + exact-type checking; `for`-`in` over `List`; string interpolation typing;
strict `bool` (no truthiness); `int`/`float`; structural `==`/identity `is`; block scoping.

**Deferred (clean M1 error, returns `Ty::Error`):** `T?` optionals + `null` literal;
the `decimal`/`i8..u64`/`double` numeric tower; cross-type equality *conversion*;
the `|>` pipe operator; function **overloading** (duplicate names); `Map` indexing.

**Unparseable, therefore out of scope entirely:** user-defined generics (`function f<T>`,
`class Box<T>` — no declaration syntax exists), traits, value-types/structs, operator-overload
declarations, property accessors. The parser cannot produce these, so the checker never sees them.

> **Note on "monomorphization":** the parent spec says generics are monomorphized. In a
> tree-walking interpreter there is no codegen, so monomorphization has no M1 meaning. It
> reduces to *tracking the type arguments of built-in generic types and checking element
> types* (e.g. `List<Shape>` rejects a `List<int>` value). Real monomorphization is an
> M2 (bytecode) concern.

## 3. Architecture — one pass, two sub-phases

`check(&Program) -> Result<(), Vec<TypeError>>`:

1. **Collect (hoist):** build a global symbol table — functions, enums, classes — plus a
   prelude. Declaration order is irrelevant (forward references work).
2. **Check (walk bodies):** type each statement/expression in every function/method body
   against the symbol table, threading a **block-scoped** lexical environment of local types.

The checker is a **validation gate**: it returns unit-or-errors and does **not** annotate or
transform the AST. The Plan 5 evaluator runs the original AST against runtime values.
(Upgradeable to a typed AST later if a real need appears — YAGNI for M1.)

**Error mode:** collect-all. Walk the whole program, accumulate every `TypeError` with its
span, return them all. The evaluator only runs when the list is empty.

## 4. Files

| File | Responsibility |
|---|---|
| `src/types.rs` | internal resolved `Ty` enum, `Display`, `assignable()` |
| `src/checker.rs` | symbol table, scope stack, `TypeError`, the pass, `pub fn check` |
| `src/lib.rs` | add `pub mod types; pub mod checker;` |

## 5. `Ty` and assignability

```rust
pub enum Ty {
    Int, Float, Bool, String, Unit,
    Named(String),                  // enum or class, nominal
    List(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Set(Box<Ty>),
    Error,                          // poison — suppresses cascade errors
}
```

- `Unit` is the type of statements / a function with no declared return / `println`.
- `assignable(from, to) = from == to || from == Error || to == Error`.
- **No numeric widening** — `int` and `float` never auto-convert (spec §3: no implicit coercion).
- `Error` is the poison type: a failed sub-expression yields `Ty::Error`, which is assignable
  both ways, so a single mistake does not spray follow-on errors.

## 6. Symbol table & prelude

- **functions:** `name → FnSig{ params: Vec<Ty>, ret: Ty }`. A duplicate name is the
  deferred-corner error `"function overloading is not yet supported in M1"`.
- **enums:** each variant registers as a callable returning the enum's `Named` type
  (`Circle: (float) -> Shape`), and the variant's field types are stored for pattern checking.
- **classes:** `fields` (from **explicit field declarations only**), `methods`, and the
  constructor's parameter types for `Greeter(args)` calls.
  **Constructor promotion is NOT modeled** — no field is synthesized from constructor params.
  This matches the Plan 3 parser limitation and avoids a duplicate-`name` field in the §6
  `Greeter`, which declares `private string name;` explicitly *and* has `constructor(private string name)`.
- **prelude:** primitives `int/float/bool/string`; built-in generics `List/Map/Set` (arity-checked);
  `println(string) -> Unit`. `import std.io;` is accepted syntactically; real module resolution
  is deferred (the prelude always provides `println`).

## 7. Type resolution (AST `Type` → `Ty`)

- `Named{"int"|"float"|"bool"|"string"}` → the primitive `Ty`.
- `Named{"List", [T]}` → `List(resolve T)`; `Map`/`Set` likewise, with arity check.
- `Named{enum-or-class-name}` → `Named(name)` (must exist in the symbol table, else `"unknown type"`).
- `Named{"decimal"|"i8".."u64"|"double"}` → deferred-corner error → `Ty::Error`.
- `Optional{inner}` → deferred-corner `"optional types are not yet supported in M1"` → `Ty::Error`.

## 8. Statement checking (threads scope, accumulates errors)

| Stmt | Rule |
|---|---|
| `VarDecl{ty,name,init}` | resolve `ty`; `init` assignable to `ty`; bind `name:ty` in current (block) scope |
| `Return{value}` | `value` assignable to enclosing fn `ret` (or `Unit` if `None`) |
| `If{cond,then,else}` | `cond` must be `Bool` (strict — no truthiness); check blocks in child scopes |
| `For{ty,name,iter,body}` | `iter` must be `List<E>`; `ty == E`; bind `name:ty`; body in child scope |
| `Block` | child scope; check stmts |
| `Expr(e)` | check `e`, discard its type |

## 9. Expression checking → `Ty`

- **literals:** `Int→Int`, `Float→Float`, `Bool→Bool`; `Null` → deferred-corner error.
- **`Str(parts)`:** each embedded expression must be a **primitive** (`int/float/bool/string`)
  — primitives auto-stringify in interpolation (the sample needs `float`); objects/enums →
  error. Whole literal is `String`. (This interpolation-stringification is intentional and
  distinct from the no-implicit-coercion rule, which governs arithmetic/comparison.)
- **`Ident`:** scope chain — locals → params → **class fields when inside a method** (so bare
  `name` in `greet` resolves to the field); else `"unknown identifier"`.
- **`This`:** the enclosing class type; error outside a method.
- **`List(elems)`:** non-empty → all elements must share one `Ty E` → `List(E)`; empty `[]` →
  takes its element type from the `VarDecl` expected type (bidirectional); empty with no
  expected type → `"cannot infer empty list element type"`.
- **`Unary`:** `Neg` on `int`/`float`; `Not` on `bool`.
- **`Binary`:** arithmetic `+ - * / %` require both `Int` or both `Float` → that type;
  comparison `< > <= >=` require both `Int` or both `Float` → `Bool`; `==`/`!=` require both
  the **same** type → `Bool` (cross-type → `"cross-type comparison requires explicit conversion"`);
  `&&`/`||` require both `Bool` → `Bool`; `is` → `Bool`; `|>` (pipe) → deferred-corner error.
- **`Call{callee, args}`:**
  - `callee = Ident(name)` → resolve as function, enum-variant constructor, or class
    constructor; check arity + each arg assignable to the parameter type → return type.
  - `callee = Member{obj, method}` → `obj:Named(class)`; look up method; check args → ret.
  - otherwise `"expression is not callable"`.
- **`Member{object, name}`:** `object:Named(class)` → the field's type. (Method-as-value is
  unsupported; methods are only valid in `Call` position.)
- **`Index{object, index}`:** `List<E>` + `index:Int` → `E`. `Map` indexing → deferred-corner.
- **`Match`** — see §10.

## 10. Match + exhaustiveness (crown jewel)

`match scrutinee { arm* }`: scrutinee type `S`. Each arm's pattern is checked against `S`,
its bindings injected into that arm body's scope; **all arm body types must unify to one
type `T`** → the match expression's type is `T`. Exhaustiveness:

- **enum scrutinee:** collect covered variant names. A `Wildcard` or `Binding` arm ⇒
  exhaustive. Otherwise **every** variant of the enum must appear, else
  `"non-exhaustive match: missing <Variant>"`.
- **`Variant{name, fields}` pattern:** the variant must exist on the enum; the field-pattern
  count must equal the variant arity; recurse each field pattern against the variant's field
  type (binds e.g. `r:float`).
- **primitive scrutinee** with literal patterns: infinite domain ⇒ require a wildcard/binding
  arm, else non-exhaustive.

### Pattern checking (`pattern`, scrutinee `Ty`) → bindings
- `Wildcard` → none. `Binding{name}` → `name: scrutinee Ty`.
- `Int/Float/Str/Bool` → literal must match the scrutinee's primitive type.
- `Null` → deferred-corner error.
- `Variant{name, fields}` → as above.

## 11. Method body scope

When checking a method body, seed the scope with `this: Named(class)`, then all class fields
by name, then the parameters. This is why bare `name` in `greet()` resolves to the field.

## 12. Error reporting

```rust
pub struct TypeError { pub message: String, pub line: u32, pub col: u32 } // mirrors ParseError
```
`check(&Program) -> Result<(), Vec<TypeError>>` — collect-all; spans come from the AST nodes.

## 13. Testing (TDD) & acceptance

**Unit tests** (`src/checker.rs`), good + bad per rule: assignability; `if`-cond-must-be-bool;
arithmetic typing + no-mixing; same-type `==`; cross-type compare error; call arity/type;
unknown ident/type; each deferred corner (optional, `null`, decimal, sized int, pipe,
overloading); list element unification; `for`-in element type; match exhaustiveness
(missing-variant error + wildcard-ok); variant-pattern binding; method field scope;
interpolation primitive-ok / object-error.

**Integration** (`tests/typecheck_integration.rs`): the **verbatim §6 sample**
lexes + parses + **type-checks with zero errors**; several broken variants assert the exact
`TypeError`.

**Acceptance:**
- §6 sample ⇒ `check()` returns `Ok`.
- Broken variants ⇒ correct, specific `TypeError`s.
- `cargo test` all green; `cargo clippy --all-targets` clean; zero build warnings.
- Throwaway panic-probe: malformed-but-parseable programs ⇒ errors, no panic.

## 14. Decisions Log

| # | Decision | Choice | Rationale |
|---|---|---|---|
| TC-1 | Scope appetite | Sample-faithful core | Smallest correct Plan 4; gets to a runnable evaluator (Plan 5) sooner; stays sound |
| TC-2 | Generics | Built-in `List`/`Map`/`Set` element checking only | User-defined generics are unparseable; tree-walker has no monomorphization codegen |
| TC-3 | Deferred corners | Clean *"not yet in M1"* errors → `Ty::Error` | Sound (nothing unchecked accepted) + message signals deferred-by-design vs typo |
| TC-4 | Error mode | Collect all → `Result<(), Vec<TypeError>>` | Matches real compilers; better UX; evaluator gates on empty |
| TC-5 | Interpolation typing | Primitives auto-stringify; objects error | §6 sample interpolates `float`; distinct from no-coercion rule |
| TC-6 | Output shape | Validation gate, no AST annotation | Tree-walker runs untyped AST on runtime values; YAGNI for M1 |
| TC-7 | Constructor promotion | Not modeled in M1 | Matches Plan 3 parser limitation; avoids duplicate field in §6 `Greeter` |
| TC-8 | Assignability | Nominal equality, no numeric widening | Spec §3: no implicit coercion |
