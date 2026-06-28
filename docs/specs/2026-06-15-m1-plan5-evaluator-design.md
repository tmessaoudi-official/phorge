# Phorj M1 — Plan 5: Tree-Walking Evaluator (Design)

> Status: **frozen** (2026-06-15). Inputs: the frozen language design
> (`2026-06-15-phorj-language-design.md`, §6 sample is the run target) and the
> type-checker design (`2026-06-15-m1-typechecker-design.md`).

## 1. Goal

Programs **run**. Walk the *untyped* AST produced by the parser against runtime
values and execute `main`. The §6 sample must print:

```
Hello Tak
area = 12.56636
area = 12
```

The type-checker (`phorj::checker::check`) remains a separate **gate**: the
evaluator assumes type-correct input and does not re-check types. But it must
never *panic* on the handful of faults types cannot prevent — those become
clean `RuntimeError`s.

## 2. Scope (sample-faithful core)

Implement exactly what makes the §6 sample run, plus the obvious neighbours that
share a code path (all arithmetic/comparison/logical operators, recursion,
if/for, every `match` pattern kind the parser emits). Deferred corners
(`T?`/`null` values, `decimal`/sized-ints/`double`, `|>` pipe, Map/Set indexing,
function overloading, **nullary enum variants referenced bare** — e.g. `B` with no
payload/parens, which parse as a plain identifier and are `undefined` to both the
checker and the evaluator) are **not** implemented; if execution somehow reaches one
(it shouldn't — the checker gates them), emit a `RuntimeError`
"… not yet supported in M1", never a panic.

## 3. Architecture

Mirror the checker's two-phase shape:

1. **collect** — hoist every top-level `Function`, `Enum`, `Class` into global
   tables (declaration-order-independent). Also build a `variant → (enum, arity)`
   index so a `Call` on a bare variant name resolves.
2. **interpret** — locate `main`, call it with no args, return captured stdout.

One recursive walker over `Stmt`/`Expr`. No AST transformation.

## 4. Files

- `src/value.rs` — `Value`, `Instance`, `EnumVal`, `HKey`, stringify/`Display`,
  unit tests.
- `src/interpreter.rs` — `Interp`, `Frame`, `Signal`, `RuntimeError`, the walker,
  unit tests.
- `src/lib.rs` — add `pub mod value;` and `pub mod interpreter;`.
- `tests/run_integration.rs` — run the verbatim §6 sample, assert exact output.

## 5. Runtime values (Decision 1 — owned + `Clone`, no `Rc`)

M1 has no reassignment and no post-construction field mutation (Plan 3 parser
limitation), so shared mutability is unnecessary. Values are owned and `Clone`;
objects are boxed for size, not sharing. Revisit `Rc`/`RefCell` only when a later
plan introduces mutation.

```rust
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    List(Vec<Value>),
    Map(HashMap<HKey, Value>),   // unused by the sample; constructible only
    Set(HashSet<HKey>),          // unused by the sample; constructible only
    Instance(Box<Instance>),
    Enum(Box<EnumVal>),
}

pub struct Instance { pub class: String, pub fields: HashMap<String, Value> }
pub struct EnumVal  { pub ty: String, pub variant: String, pub payload: Vec<Value> }
```

`HKey` is a hashable subset (Int/Bool/Str) used only so `Map`/`Set` can exist as
values; the sample never indexes them, so no key-coercion rules are needed in M1.

**Stringification** (for interpolation and `println`):
- `Int` → decimal; `Float` → Rust `{}` (so `12.0` renders `12`, `12.56636`
  renders `12.56636`) — documented M1 format, revisit if a plan needs trailing
  `.0`; `Bool` → `true`/`false`; `Str` → as-is; `Unit` → `unit`.
- `Instance`/`Enum`/`List`/`Map`/`Set` in a string hole → `RuntimeError`
  (the checker already blocks non-primitive interpolation; this is the safety net).

## 6. Environment & call stack (Decision 2 — flat frame + block-scope stack)

No closures in M1, so a call does not capture an enclosing environment.

```rust
struct Frame { scopes: Vec<HashMap<String, Value>> }  // block scopes within one call

pub struct Interp {
    funcs:   HashMap<String, FunctionDecl>,
    enums:   HashMap<String, EnumDecl>,
    classes: HashMap<String, ClassDecl>,
    variants: HashMap<String, (String, usize)>, // variant name -> (enum, arity)
    frame: Frame,
    this:  Option<Value>,   // Some(Instance) inside a method body
    out:   String,          // captured stdout
}
```

- **Variable lookup**: search `frame.scopes` top-down; on miss, if `this` is an
  `Instance`, fall back to its fields (mirrors the checker's method-scope seeding,
  so bare `name` inside `greet` resolves).
- **Calls** build a *fresh* `Frame`, bind params into its base scope, run the
  body, and yield the return value. The current frame/`this` are saved and
  restored around the call.
- **Functions/enums/classes** resolve from the global tables, never the scope
  chain.

## 7. Control flow & errors (Decision 3 — signal enum in `Result::Err`)

```rust
pub enum Signal { Return(Value), Runtime(RuntimeError) }
pub struct RuntimeError { pub message: String }
type EvalResult<T> = Result<T, Signal>;
```

- Statement/expression evaluation returns `EvalResult<…>`; `?` propagates both
  `Return` and `Runtime` naturally.
- `return e;` → `Err(Signal::Return(v))`. A function call site catches
  `Signal::Return(v)` → that is the call's value; it re-raises `Signal::Runtime`.
  A function that falls off the end yields `Value::Unit`.
- `for` propagates `Signal::Return` out of the loop (no `break`/`continue` in M1).
- Faults (division by zero, non-primitive interpolation, deferred-corner ops,
  runtime match fall-through) → `Signal::Runtime`. **No panics in normal
  operation.**
- Top-level `interpret` maps `Ok`/`Signal::Return` → `Ok(out)` and
  `Signal::Runtime(e)` → `Err(e)`.

## 8. Evaluation rules (mirror the checker)

**Expressions**
- `Int/Float/Bool/Str(literal)` → the obvious `Value`. `Null` → `RuntimeError`
  (no null values in M1).
- `Str(parts)` interpolation → concatenate: `Literal` verbatim, `Expr` evaluated
  then stringified per §5 (primitives only).
- `Ident(name)` → variable lookup (§6); unresolved → `RuntimeError` (checker
  prevents this).
- `This` → the current `this` (clone); outside a method → `RuntimeError`.
- `List(items)` → `Value::List` of evaluated items.
- `Unary{Neg}` on Int/Float; `Unary{Not}` on Bool.
- `Binary`: `Add/Sub/Mul/Div/Rem` on Int (Int result) or Float (Float result),
  no mixing (checker forbids it); `Div`/`Rem` by zero → `RuntimeError`. Integer
  ops are **checked** — overflow (incl. `i64::MIN` neg / `÷ -1`) → `RuntimeError`,
  never a panic (types can't catch overflow).
  `Eq/NotEq/Is` → `Bool` by value equality; `Lt/Gt/Le/Ge` on Int/Float → `Bool`;
  `And/Or` short-circuit on `Bool`. `Pipe` → `RuntimeError` (deferred corner).
- `Index` → `RuntimeError` (deferred corner; the sample never indexes).
- `Call{callee, args}` — resolve by `callee`:
  - `callee = Ident(n)`: if `n` is a **function** → call it (fresh frame, bind
    params, run body); else if `n` is an **enum variant** → build
    `Value::Enum{ty, variant:n, payload:args}`; else if `n` is a **class** →
    construct an instance (§9). Resolution order: function → variant → class.
  - `callee = Member{object, name}`: evaluate `object` to an `Instance`, look up
    method `name` on its class, call it with `this = object` (§9).
  - any other callee → `RuntimeError`.
- `Member{object, name}` (not called) → evaluate `object` to an `Instance`, read
  `fields[name]`; missing → `RuntimeError`.
- `Match{scrutinee, arms}` → §10.

**Statements**
- `VarDecl{name, init}` → evaluate `init`, declare `name` in the current scope.
- `Return{value}` → `Err(Signal::Return(value? .map(eval).unwrap_or(Unit)))`.
- `If{cond, then_block, else_block}` → eval `cond` (Bool); run the chosen block
  in a pushed scope.
- `For{name, iter, body}` → eval `iter` to a `List`; for each element, push a
  scope, bind `name`, run `body`, pop; propagate `Signal::Return`.
- `Block(stmts)` → push scope, run, pop.
- `Expr(e)` → evaluate, discard.

## 9. Object construction & methods (constructor promotion is runtime-critical)

The §6 `Greeter` constructor body is **empty** — the field `name` is populated
purely by **constructor promotion**. The checker did not model promotion (types
only), but the evaluator must, or the sample prints `Hello ` instead of
`Hello Tak`.

**Constructing `ClassName(args)`**:
1. Create `Instance { class, fields: {} }`.
2. Find the class `Constructor`. Bind each `CtorParam` to the corresponding arg.
3. **Promotion**: for every `CtorParam` carrying a visibility modifier
   (`Public`/`Private`/`Protected`), set `instance.fields[param.name] = arg`.
4. Run the constructor body with `this = instance` and the params in scope
   (body may assign further fields in a future plan; in M1 it is typically empty).
5. The constructed `Instance` is the call's value.

A class with no declared constructor → an instance with empty fields (none of the
M1 sample classes need this, but it must not panic).

**Calling a method `obj.method(args)`**:
1. Evaluate `obj` to an `Instance`; clone it as `this` (owned semantics).
2. Look up `method` among the class `Method`s; bind params in a fresh frame.
3. Run the body with `this` set; the returned value is the call's value.

## 10. Match evaluation

Evaluate the scrutinee once. Try arms top-to-bottom; the first whose pattern
matches wins:
- `Wildcard` / `Binding(n)` → always match; `Binding` binds the scrutinee to `n`.
- `Int/Float/Str/Bool` literal → match on value equality.
- `Null` → `RuntimeError` (no null in M1).
- `Variant{name, fields}` → scrutinee must be an `Enum` with the same `variant`;
  recursively match each field pattern against the corresponding payload element,
  binding as it goes (the sample uses `Binding` sub-patterns: `Circle(r)`).

Bindings live in a scope pushed for the arm body. If no arm matches (should be
impossible after exhaustiveness checking) → `RuntimeError` "non-exhaustive match
at runtime". Match is an expression: its value is the chosen arm body's value.

## 11. Public API

```rust
pub fn interpret(program: &Program) -> Result<String, RuntimeError>;
```

Collect → locate `main` (no params) → call → return the captured `out` buffer.
Missing `main` → `RuntimeError`. The Plan 6 CLI prints the returned buffer to
real stdout; keeping I/O in a buffer here makes the evaluator pure and testable.

## 12. Testing (TDD, per rule)

Unit tests in `value.rs` (stringification, equality) and `interpreter.rs`, added
test-first in this order: literals → arithmetic (incl. div-by-zero error) →
comparisons/logical short-circuit → variable scope → function call + recursion →
enum-variant construction → class construction **with promotion** → method call
(`this` + field fallback) → match arms (variant binding + wildcard) → for-loop →
string interpolation → `return` unwinding from nested blocks.

Integration (`tests/run_integration.rs`): run the verbatim §6 sample, assert the
captured output equals the three expected lines (§1). Plus: a non-`main` program
errors cleanly; a division-by-zero program returns `Err(RuntimeError)` not a
panic.

## 13. Decisions Log

- **EV-1** Owned `Value` + `Clone`, no `Rc`/`RefCell` (M1 has no mutation).
- **EV-2** Flat per-call `Frame` with a block-scope stack; globals table for
  funcs/enums/classes; no closures.
- **EV-3** `Signal` enum in `Result::Err` carries both `Return` unwinding and
  `RuntimeError`; no panics in normal operation.
- **EV-4** Constructor **promotion executed at runtime** (promoted params →
  fields) — required for the §6 sample's empty-body constructor to populate
  `name`. The checker does not model this; the evaluator must.
- **EV-5** `println` writes to an in-memory `out` buffer; real stdout deferred to
  the Plan 6 CLI.
- **EV-6** Float stringification uses Rust `{}` (no forced `.0`) for M1.
- **EV-7** Deferred corners (`|>`, indexing, `null`, Map/Set keys, overloading)
  emit `RuntimeError`, never panic — same sample-faithful boundary as the checker.
- **EV-8** Call resolution order for an `Ident` callee: function → enum variant →
  class constructor.
