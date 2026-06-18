# Phorge M3 S3 — Lambdas + Pipe: Design

> Brainstorm output, 2026-06-18. Track A of M3 (`docs/specs/2026-06-17-m3-language-roadmap-design.md`
> slice **S3 — lambdas + pipeline**). First-class anonymous functions (closures) + the pipe operator
> `|>`. Ships byte-identical on both backends and round-tripped through real PHP, per the differential
> spine (invariant #1) and the transpile contract (D-L9). **Approved 2026-06-18; under spec review.**

## 1. Guiding principle

Lambdas turn functions into values: you can bind one to a `var`, pass it to another function, and
call it. The pipe `|>` is reverse application — `x |> f ≡ f(x)` — for left-to-right data flow. Both
are the keystone that unblocks `core.list` (`map`/`filter`/`reduce`), the M6 router's
closure-route/middleware layer, and `core.json`. They must obey every existing invariant: `run` ≡
`runvm` byte-identical (#1), value kernels single-sourced (#3), a new `Op` extends three exhaustive
matches in lockstep (#5), the AST stays untyped and backends re-derive operand types (#9), and every
feature maps to idiomatic PHP (D-L9).

**The collapsing insight.** Because `|>` is pure reverse application, the parser **lowers**
`a |> b` to `Call{callee: b, args: [a]}` immediately. No backend then needs any pipe-specific code:
they already handle `Call`, and once they can call a *function value* (which lambdas require anyway),
the pipe works for free — byte-identically, with the correct precedence the parser already gives it
(`a + b |> f` ⟹ `f(a + b)`; `x |> f |> g` ⟹ `g(f(x))`). The real work of this slice is therefore
**lambdas as first-class values**; the pipe falls out of it.

## 2. Locked decisions

| # | Decision | Rationale |
|---|----------|-----------|
| **A1** | **Syntax:** `fn(int x) => expr` (expression body) **and** `fn(int x) { stmts; return e; }` (statement body). Zero params: `fn() => 42`. | `fn` is unused today (declarations use `function`); `=>` already tokenizes (match arms). Mirrors PHP arrow fn `fn($x)=>…` and `function($x) use(){…}` 1:1 (D-L9). |
| **A2** | **Params explicitly typed.** **Expression-body return is inferred** (the body expression's type); **statement-body requires an explicit `-> T`** this slice. | Phorge is always-explicit-typed; param inference needs bidirectional checking (deferred). Expr-body inference reuses the `var`/`Type::Infer` path; inferring a block body's return by unifying all `return` statements is deferred (keeps the rule simple + sound). |
| **A3** | **`x \|> rhs ≡ rhs(lhs)`**, lowered to `Call` in the parser. | PHP 8.5 native `\|>` semantics (D-L5); zero backend pipe-code; precedence already correct. Extra args via a lambda: `x \|> fn(int v) => add(v, 10)`. |
| **A4** | **A bare named-function identifier is a value** (a zero-capture closure). | `var f = myFn;`, `5 \|> myFn`. PHP 8.1 first-class callable `myFn(...)`. Uniform with lambda values at the VM level. |
| **A5** | **Capture by value at creation time.** No GC needed. | Immutable heap; a binding is **not** in scope inside its own initializer ⇒ no self-capture ⇒ acyclic ⇒ `Rc`/`Drop` reclaims fully (consistent with the M2 P5a "no tracing GC" position). |
| **A6** | **Function-type assignability = exact structural equality** (params equal, return equal); variance deferred. | Matches Phorge's no-subtyping rule (the only widening is `T → T?` and `Error`). Sound and conservative; variance can relax it later without breaking programs. |

## 3. Scope

**In this slice:**
- Anonymous lambdas, expression and statement bodies (A1).
- Calling a function *value* (a lambda variable, an inline lambda, or any expression of function type).
- Bare named-function references as values (A4).
- The pipe `|>` (parser-lowered to a call).
- One runnable, byte-identity-gated example + a guide example (the developer "examples ship with
  features" rule) + `FEATURES.md`/`CHANGELOG.md`/`examples/README.md` updates.

**Deferred (stated explicitly, not a regression):**
- **`core.list` `map`/`filter`/`reduce`** — a native signature `(List<T>, (T) -> U) -> List<U>` needs
  type variables `Ty` does not have (blocked on **S4.5 user generics**). A monomorphic `_int` variant
  is not worth the surface. The motivating consumer for *this* slice is **user-defined** higher-order
  functions, which need no generics.
- **Parameter-type inference** (`fn(x) => …`) — needs expected-type threading (A2).
- **Function-type variance** (A6).
- **Partial application** (PHP 8.6 `?`) — roadmap "consider after lambdas".

## 4. Surfaces (mapped to current source)

All line references are [Verified: read 2026-06-18].

### 4.1 Types — `src/types.rs`
- Add `Ty::Function(Vec<Ty>, Box<Ty>)` to the `Ty` enum (`types.rs:6–28`).
- `Display`: render as `(int, int) -> int` (params comma-joined in parens, ` -> `, return).
- `assignable(from, to)` (`types.rs:36–51`): add a match arm — two `Ty::Function` are assignable iff
  params are pairwise **equal** and returns **equal** (A6). `Error` still unifies (existing first
  guard). No optional/null interaction beyond `T → T?` wrapping a whole function type.

### 4.2 AST — `src/ast.rs`
- Add to `Expr` (`ast.rs:90–164`):
  ```rust
  Lambda { params: Vec<Param>, ret: Option<Type>, body: LambdaBody, span: Span }
  ```
  reusing the existing `Param { ty, name, span }` (`ast.rs`).
- Add `pub enum LambdaBody { Expr(Box<Expr>), Block(Vec<Stmt>) }`.
- **F15 — function-type annotation syntax (essential, was missing).** The syntactic `Type` enum
  (ast.rs:10 — today `Named`/`Optional`/`Infer`) gains `Type::Function { params: Vec<Type>, ret:
  Box<Type>, span }`. Without it, the explicit-param rule (A2) cannot express a higher-order
  parameter — the core example `function twice(int x, (int) -> int f) -> int => f(f(x))` is
  unwritable. `parse_type` (parser.rs:346, currently requires a leading `TokenKind::Ident`) gains a
  leading-`(` branch: `(` `(` Type (`,` Type)* `)`? `)` `Arrow` Type ⟹ `Type::Function` (right-assoc;
  zero params `() -> int`; reuses the `Arrow` token). A trailing `?` still wraps the whole thing
  optional (`((int)->int)?`). This is the one surface the research's checker/type map did not flag
  (it focused on the resolved `Ty`, not the syntactic `Type`).
- A bare named-fn reference needs **no** new node — it is an `Expr::Ident(name, span)` resolved as a
  value (§4.4).
- `BinaryOp::Pipe` (already declared, `ast.rs:68–86`) becomes **unused** after parser lowering; left
  in place harmlessly (removing it is out of scope).

### 4.3 Lexer / Parser — `src/token.rs`, `src/lexer.rs`, `src/parser.rs`
- **Lexer: reserve `fn` as a keyword** (F-kw). Add `TokenKind::Fn` (token.rs, beside `Function`) and
  `"fn" => Fn` to `keyword(s)` (lexer.rs:298). A *contextual* `fn` is unsafe: `var fn = g; fn(3)`
  would mis-parse as a lambda. Reserving it (consistent with `function` already being reserved) makes
  the lambda start unambiguous. **Task-1 guard:** a lexer-level keyword fires in
  *every* position, so grep `examples/` + all test programs for `fn` used as an identifier in **any**
  position — variable, field/member (`obj.fn`), param, method, or variant name — and migrate any (none
  found in `examples/` as of 2026-06-18). `=>` (`FatArrow`),
  `->` (`Arrow`), `|>` (`Pipe`) all already tokenize (`lexer.rs:413–443`).
- **`parse_primary`** — when the next token is `fn` immediately followed by `(`: parse a lambda.
  Grammar: `fn` `(` (Param (`,` Param)*)? `)` ( `=>` Expr | Block ). Params parse exactly like
  function-declaration params (typed). Mirrors `parse_if_expr` (`parser.rs:532–557`) as the precedent
  for a new primary expression form. A statement `Block` reuses the existing block parser.
- **Pipe lowering** — in the binary-parse path (`parse_binary`, `parser.rs:93–156`), when the operator
  is `Pipe` (precedence entry `parser.rs:116`), build `Expr::Call { callee: Box::new(rhs), args:
  vec![lhs], span }` instead of `Expr::Binary`. `Expr::Binary{op: Pipe}` is then **never constructed**.
  `BinaryOp::Pipe` stays in the enum (it is the precedence-table marker + the sexpr formatter
  `parser.rs:1046`). Update the precedence tests (`parser.rs:1268–1269`) to assert the lowered `Call`
  shape.
- **F4 — retire the dead Pipe stubs.** Four backends currently reject `BinaryOp::Pipe`: checker.rs:771,
  compiler.rs:1105, interpreter.rs:481, transpile.rs:524 (guard) + 790 (`unreachable!`). Because the
  variant is no longer constructed, these arms become unreachable; convert each to
  `unreachable!("`|>` is lowered to a call in the parser")` (matching transpile.rs:790's existing
  pattern) — truly unreachable, so EV-7-safe (no input can reach it). **Grep all `#[test]` fns for any
  `agree_err`/assertion expecting "`|>` not supported"** and flip them to `agree` with the working
  pipe semantics. [Verified: stub locations grepped 2026-06-18.]
- **Lambda-body greediness note:** a lambda's expression body is parsed with `parse_expr`, so it
  extends as far right as precedence allows (e.g. `data |> fn(int v) => v + 1`’s body is `v + 1`).
  Parentheses disambiguate. Documented, not a defect.

### 4.4 Checker — `src/checker.rs`
- **`check_lambda`** (new, ~60–90 lines): push a scope; `declare` each param (typed); save/restore
  `cur_ret` to the lambda's return type while checking the body (so a `return` in a `Block` body
  checks against the **lambda's** return, not the enclosing function's); infer the return type
  (expression body → the body's type via `check_expr`; **block body → an explicit `-> T` is required**
  this slice (A2) — `return` statements check against it; block-return inference is deferred); build
  and return `Ty::Function(param_tys, ret)`. A lambda with a block body and no `-> T` is a clean
  checker error. Free
  variables resolve naturally through the existing scope stack (`lookup`, `checker.rs:447–488`) — no
  capture computation needed in the checker.
- **`check_call`** (`checker.rs:896–932`): add a path — if the callee expression has type
  `Ty::Function(params, ret)` (a lambda var, inline lambda, or any function-typed expression), run the
  shared `check_args` against `params` and yield `ret`. Keep the existing named/native/method/ctor
  paths; the new path is the `_ =>` fallback that today errors "not callable".
- **F1 — calling an optional function is rejected.** A callee of type `Ty::Optional(Function)` does
  **not** match the function-call path — it falls through to a "not callable" error, requiring the
  programmer to unwrap first (`f!()`, `if (var g = f) { g(x) }`, or `match`). This is the existing
  non-null discipline ([[runtime-null-optional-types]]); state it so it is intentional, not an
  accidental gap. (There is no `?.()` call form in this slice.)
- **Bare named-fn ref (A4, same-package unqualified only)** — in `check_expr` for `Expr::Ident(name)`:
  if `name` is not a local but **is** a known same-package function (`self.funcs`), return
  `Ty::Function(sig.params, sig.ret)` instead of "unknown variable". (Calls still resolve via the named
  path first; this only fires in value position.) **F9 — qualified/cross-package value refs are out of
  scope:** `var f = util.compute;` (a `Member` in value position) is **not** a function value this
  slice — it collides with M5 S2c loader name-mangling. Piping into a qualified function still works
  because `x |> util.compute` lowers to the *call* `util.compute(x)` (handled by the existing
  qualified/native-call path), not a value ref.
- **F8 — reject `this` inside a lambda body** (this slice). `Expr::This` is bound to the enclosing
  method's receiver slot (compiler.rs:905) / `self.this` (interpreter.rs:304); a lambda is a separate
  frame with no receiver, so capturing `this` would be a silent `run`↔`runvm` divergence. When
  checking a lambda body, if any `Expr::This` appears, emit a clear error
  (`"a lambda cannot reference `this` yet"`, stable code e.g. `E-LAMBDA-THIS`). `this`-capture is
  deferred (it needs the closure to carry the receiver). Workaround: bind `var self = this;` before
  the lambda and capture `self`.
- **F15 — `resolve_type`** (checker.rs:159–166) maps `Type::Function { params, ret }` →
  `Ty::Function(resolved_params, Box::new(resolved_ret))`, recursing through nested function types.
  This makes a higher-order parameter (`(int) -> int f`), a function-returning return type, and a
  `var f: (int) -> int` annotation all resolve.
- **F17 — `expand_aliases`** (checker.rs:1421, the recursive `Type` walker at 1440–1455) gains a
  `Type::Function { params, ret }` arm that recursively expands aliases inside the params and return.
  Required so `type Mapper = (int) -> int;` and a function type containing an alias (`(MyAlias) ->
  int`) are dealiased before the backends run (the backends require an alias-free AST,
  [[type-sugar-expand-before-backends]]). The full set of `Type`-matching sites needing a
  `Type::Function` arm is therefore: `resolve_type`, `expand_aliases` (checker), `resolve_cty`,
  `emit_type` (compiler/transpiler).
- **F14 — first-class named refs vs. the M5 loader.** The S2c loader mangles cross-package + qualified
  *calls* but a value-position `Ident` (`var f = myFn;`) is not a call. In `package main` (the example
  + all inline tests) names are unmangled, so A4 works directly. **Task-time check:** confirm a
  same-package bare named-fn ref inside a *library* (non-`main`) package resolves correctly after the
  loader pass; if the loader doesn't rewrite value-position idents, restrict A4 first-class refs to
  `package main` this slice (record in KNOWN_ISSUES). Piping into a named fn is unaffected (it lowers
  to a call).
- **Pipe** — no checker code (it is a `Call` after parsing).

### 4.5 Value — `src/value.rs`
- Add `Value::Closure(Rc<ClosureData>)` to `Value` (currently 12 variants, `value.rs:12–46`):
  ```rust
  pub enum ClosureData {
      Tree { params: Vec<Param>, ret: Option<Type>, body: LambdaBody, env: Vec<(String, Value)> }, // interpreter
      Named(String),                                                                                // interpreter: bare named-fn ref
      Byte { func: usize, captures: Vec<Value> },                                                   // VM
  }
  ```
  The interpreter and the VM never share a `Value` instance (`phg run` and `phg runvm` execute
  separately; the differential harness compares only stdout), so each backend constructs and consumes
  **only** its own variant(s). `type_name()` → `"function"`. `as_display`/equality on a closure are
  unreachable in well-typed programs (the checker forbids printing/comparing functions); guard with a
  clean error, never a panic (EV-7).
- **Acyclicity (A5):** no self-capture ⇒ no cycles ⇒ `Rc`/`Drop` reclaims completely. No GC.
- **Derive burden (F7, verified):** `Value` derives only `Debug, Clone` (it explicitly cannot derive
  `Hash`/`Eq`, value.rs:47). `ClosureData`'s fields are all `Debug + Clone`, so
  `Value::Closure(Rc<ClosureData>)` adds **no** derive burden. Map/Set keys are the separate `HKey`
  enum, so a closure is simply not `HKey`-convertible (the `Value`→`HKey` conversion gets a
  clean-error arm — §4.10).

### 4.6 Bytecode + VM — `src/chunk.rs`, `src/compiler.rs`, `src/vm.rs`
**Two new `Op`s** (invariant #5):
- **`Op::MakeClosure(usize)`** — the operand is a function-table index. Pops
  `functions[idx].n_captures` values (the captured environment, in the compiler's sorted free-var
  order), pushes `Value::Closure(Rc::new(ClosureData::Byte { func: idx, captures }))`.
  - `exec_op` arm (vm.rs); `validate` arm (`chunk.rs` — `MakeClosure(idx) if idx >= nfns`);
    `stack_effect` arm (compiler.rs — `1 - functions[idx].n_captures`).
- **`Op::CallValue(usize)`** — the operand is `argc` (not a table index ⇒ **no** `validate` arm
  needed; the closure comes from a runtime value). Stack on entry: `[…, closure, arg0, … arg(argc-1)]`.
  Pop the `argc` args, pop the closure, push `closure.captures` then the args back, set
  `slot_base` at the captures’ start, push `Frame { func: closure.func, ip: 0, slot_base }` (after the
  `MAX_CALL_DEPTH` guard, like `Op::Call`/`Op::CallMethod`, vm.rs:275–384).
  - `exec_op` arm (vm.rs); `stack_effect` arm (compiler.rs — `-(argc as isize)`: pops `argc`+closure,
    pushes one result). No `validate` arm.

**`Function` struct** (`chunk.rs`) gains `n_captures: usize` (0 for named functions, ctors, methods).

**Lambda compilation** (compiler.rs): compiling `Expr::Lambda`:
1. Compute `free_vars` (§4.8) — the sorted list of captured enclosing locals.
2. Spin up a sub-compiler whose locals are seeded `[captures…, params…]` (captures first, then
   params), mirroring how methods seed slot 0 = receiver (`compile_method`, compiler.rs:460–503).
   Compile the body to a `Chunk` (expression body ⟹ value then `Return`; block body ⟹ statements with
   an implicit `Unit; Return` epilogue, exactly like named functions).
3. Append `Function { name: "<lambda@LINE>", arity: n_captures + n_params, n_captures, chunk }` to the
   shared `functions` Vec (lazy append; `validate` runs after the whole program is built); record its
   index `idx`.
4. In the **enclosing** function, emit `Op::GetLocal(slot)` for each captured var (sorted order), then
   `Op::MakeClosure(idx)`.

**Bare named-fn ref** (compiler.rs): an `Expr::Ident(name)` in value position where `name` is a known
function ⟹ `Op::MakeClosure(named_idx)` (that function already has `n_captures == 0`). Calling it then
goes through `CallValue` uniformly.

**Call compilation** (`compile_call`, compiler.rs:1111–1188): add a branch — if the callee is **not**
a known named function / variant / class / native (the existing static paths) but is a function-typed
expression (a lambda var, inline lambda, or `getFn()`), compile the callee (pushes the closure),
compile the args, emit `Op::CallValue(argc)`. The existing `Op::Call(idx)` static path is unchanged
for named calls — including `x |> namedFn` (lowered to `namedFn(x)`), which stays a direct
`Op::Call`, **needing no closure at all**.

**`CTy`** (compiler.rs:29–56): add `CTy::Fn { params: Vec<CTy>, ret: Box<CTy> }`. Two derivation paths
must both produce it (F16): `ctype(Expr::Lambda)` → `CTy::Fn` and `ctype` of a bare named-fn ident →
`CTy::Fn` (the **inferred** path); **`resolve_cty(Type::Function)` → `CTy::Fn`** (compiler.rs:514–528,
the **annotated** path, for `var f: (int)->int = …` / a function-typed param). Then `ctype(Call)` where
the callee is `CTy::Fn` → `*ret`, so `f(3) + 1` specializes to `AddI` (invariant #9).

### 4.7 Interpreter — `src/interpreter.rs`
- `Expr::Lambda` ⟹ build `Value::Closure(Rc::new(ClosureData::Tree { params, ret, body,
  env }))` where `env` is the captured free-var set snapshotted **by value** from the current scope
  (same sorted `free_vars` set as the compiler, for tidiness — over-capturing would be invisible but
  we keep the sets identical).
- Bare named-fn ref ⟹ `Value::Closure(Rc::new(ClosureData::Named(name)))`.
- Calling a closure value: `Tree` → walk `body` with a fresh scope seeded from `env` + the bound params
  (reuse `run_call`-style logic; save/restore the active scope chain); `Named(n)` → look up
  `funcs[n]` and call it like a named function (no env).
- Pipe needs nothing (it is a `Call`).

### 4.8 Shared analysis — `free_vars`
A single helper in **`src/ast.rs`** (Q2 resolved — it is a pure AST traversal that belongs with the
nodes; keeping it here avoids a new module and an `ARCHITECTURE.md` module-map change, F19) used by the
compiler and the transpiler (and the interpreter, for parity): `free_vars(params: &[Param], body:
&LambdaBody) -> Vec<String>`. It walks the body collecting identifiers, subtracting (a) the lambda's own params,
(b) locals declared inside the body (`var`/`if (var …)`/`for`/match bindings), and (c) global function
names + imported module qualifiers (those are not captures). Nested lambdas compose (their params
shadow). The result is **sorted** (invariant #8 — deterministic capture order regardless of source
order), which is the single source of truth for VM capture-slot order, the interpreter env, and the
PHP `use()` list.

### 4.9 Transpiler — `src/transpile.rs`
- **Expression-body lambda** ⟹ PHP arrow fn `fn($a, $b) => <expr>` (auto by-value capture — no `use`).
- **Statement-body lambda** ⟹ `function($a, $b) use ($cap1, $cap2) { <stmts> }` — the `use` list is
  `free_vars` (sorted), each captured **by value** (no `&`).
- **Bare named-fn ref** ⟹ PHP first-class callable `myFn(...)` (PHP 8.1); for a namespaced function the
  existing FQN logic applies (`\Vendor\Pkg\myFn(...)`).
- **F15 — `emit_type(Type::Function)`** (the transpiler's type emitter, transpile.rs `emit_type`/
  `ret_hint`) ⟹ PHP `\Closure` (a function-typed param/return/var renders as `\Closure`; PHP has no
  structural function type, and `\Closure` is the idiomatic hint for both arrow fns and `function(){}`
  closures).
- **Pipe** ⟹ nothing special — it is already a `Call`, emitted as `rhs(lhs)` by the existing
  `emit_call` (transpile.rs:623–641). (PHP 8.5 native `|>` emission is a possible future mode; nested
  call keeps the output runnable on older PHP, matching the "runnable as plain PHP forever" goal.)

### 4.10 Exhaustive-match blast radius (the lockstep surfaces)
Three new enum variants each open a family of exhaustive `match` sites that must all be extended in
the same change (the invariant #5 discipline, generalized):

| New variant | Match sites to extend |
|-------------|-----------------------|
| `Op::MakeClosure(idx)`, `Op::CallValue(argc)` | `vm::exec_op` (both); `chunk::validate` (`MakeClosure` index-bound only); `compiler::stack_effect` (both). `phg disasm` auto-handles them via `Op`'s `Debug` + the `_`-fall-through annotator — **no** disasm match to drift. |
| `CTy::Fn { params, ret }` | `compiler::num_ty` (the `CTy::Class\|Other\|List => None` arm, compiler.rs:774 — `CTy::Fn` is non-numeric ⇒ `None`); `resolve_cty`; any other `match cty`/`match self.ctype(..)`. [Verified: num_ty arm at compiler.rs:774.] |
| `Value::Closure(Rc<ClosureData>)` | every exhaustive `match` on `Value` in `value.rs` + both backends: `as_display`, `type_name`, equality (`Eq`/`Ne` kernels), `compare_ord`, truthiness, and `Value`→`HKey` conversion (a closure is **not** hashable ⇒ clean error, never a `Map`/`Set` key). All closure arms are unreachable in well-typed programs (the checker forbids printing/comparing/keying functions) but must exist for exhaustiveness + EV-7 safety. |
| `Type::Function { params, ret }` (syntactic) | `checker::resolve_type` (F15), `checker::expand_aliases` (F17), `compiler::resolve_cty` (F16), transpiler `emit_type`/`ret_hint` (F15). |
| `Expr::Lambda { … }` | `checker::check_expr`, `compiler::expr` + `ctype`, `interpreter` eval, `transpile::emit_expr`, **and the parser `sexpr`/Debug formatter** used by `phg parse` + parser tests (F18). |
| `BinaryOp::Pipe` (now never constructed) | the 4 dead stubs → `unreachable!` (F4): checker.rs:771, compiler.rs:1105, interpreter.rs:481, transpile.rs:524/790. `sexpr` (parser.rs:1046) keeps its arm (still referenced by the precedence table). |

Task 1 of the plan enumerates the exact sites with `grep` before editing, so none is missed (the
build breaks loudly on a missing arm — the compiler's exhaustiveness check is the backstop). This
table is intended to be **complete**: the new variants are `Op` (×2), `Value`, `CTy`, `Type`, `Expr`.

## 5. Testing strategy

- **Differential** (`tests/differential.rs`): `agree(src)` for every new form — lambda call, lambda
  passed to a user function, bare named-fn ref called, multi-stage pipe, lambda **capturing two
  enclosing vars** (the proven slot-ordering trigger — see §7), nested lambda, zero-param lambda,
  expr-body and stmt-body. `agree_err(src)` for: calling a non-function, arity mismatch on a lambda
  call, type mismatch on a piped value.
- **F13 — height-sensitive contexts (sharpens R1).** Because `MakeClosure`/`CallValue` `stack_effect`
  feeds the `match`-spill height, include `agree` cases that call a lambda **inside string
  interpolation** (`"{f(x)}"`) and **inside a `match` arm** — the contexts that caught the S2
  interpolation-height break. A statement-level call cannot expose a height-tracking bug.
- **Real PHP round-trip**: transpile the guide example and run it under PHP 8.6; assert byte-identical
  to `phg run`.
- **Example** (the "examples ship with features" rule): a runnable `examples/guide/lambdas-pipe.phg`
  (auto byte-identity-gated by the `examples/**/*.phg` glob) demonstrating higher-order functions +
  `|>` + named-fn refs, plus an `examples/README.md` index + coverage-matrix entry.
- **Quality gate**: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`, release build —
  all green (invariant #10).

## 6. Success criteria

1. `run` ≡ `runvm` byte-identical for all new forms (#1); `agree_err` parity for the fault cases.
2. The guide example round-trips through real PHP 8.6 byte-identically (D-L9).
3. Two new `Op`s, each extending its required matches in **one** commit (#5): `MakeClosure` (3
   matches), `CallValue` (2 matches).
4. No new panic path on adversarial input (EV-7) — closures never reach display/eq in well-typed code,
   and are guarded if they somehow do.
5. Quality gate green; the example set + `FEATURES.md` (add a *lambdas* row; flip the **pipe `|>`**
   row from `🔲 M3` to ✅) + `CHANGELOG.md` + `examples/README.md` updated in the same change as the
   feature.
6. **F11 — deferrals recorded in `KNOWN_ISSUES.md`:** `this`-capture, qualified/cross-package value
   refs, block-body return inference, function-type variance, and `core.list` map/filter/reduce.
7. **F12 — new diagnostic codes registered in `phg explain`:** `E-LAMBDA-THIS` (+ any "not callable"
   refinement), with a one-line explanation each (the S0 stable-codes dictionary).

## 7. Risks & mitigations

- **R1 — capture-slot ordering divergence (highest risk).** The VM's positional capture slots and the
  lambda body's slot resolution must agree, or it is a silent `run`↔`runvm` break (the
  [[null-op-scratch-slot]] class). **Mitigation:** one sorted `free_vars` order drives capture-emit,
  slot-assignment, env, and `use()`; the differential suite includes a **two-capture** lambda (a
  single capture cannot expose an ordering bug).
- **R2 — `return` in a block-body lambda targeting the wrong function's return type.** **Mitigation:**
  `check_lambda` saves/restores `cur_ret` (and the interpreter/compiler treat the lambda body as its
  own call frame, which they already do for functions).
- **R3 — `fn` keyword reservation (resolved in §4.3).** `fn` becomes a reserved keyword
  (`TokenKind::Fn` + `keyword()` entry); a contextual `fn` would mis-parse `var fn = g; fn(3)`.
  **Residual:** any existing identifier named `fn` breaks — Task 1 greps `examples/` + test programs
  and migrates (none in `examples/` as of 2026-06-18).
- **R6 — interpreter clones the lambda AST into the closure.** `ClosureData::Tree` owns `body` (a
  `Value` cannot borrow `&Expr`), so each closure creation clones its body subtree; a lambda created
  in a loop clones per iteration. This is **consistent with the interpreter's existing pattern**
  (named calls already clone the body from `self.funcs`) and is byte-identical — a perf note, not a
  correctness risk. Acyclic (A5) ⇒ `Rc`/`Drop` still reclaims fully.
- **R4 — over-broad `ClosureData` (fields one backend ignores).** Accepted: the enum-of-variants keeps
  each backend's construction/consumption to its own variant; the alternative (two `Value` enums)
  breaks the single shared kernel. Documented.
- **R5 — `free_vars` correctness with shadowing / nested lambdas / match & for bindings.** This is the
  subtlest logic. **Mitigation:** unit-test `free_vars` directly (not only through the backends) with
  shadowing, nested lambdas, and binding forms; the differential suite is the backstop.

## 8. Open questions (resolve during the plan / Task 1)

- **Q1 — RESOLVED (A2/§4.4):** block bodies **require** an explicit `-> T` this slice; only
  expression bodies infer the return. Block-return inference (unifying `return` statements) is a clean
  follow-up.
- **Q2 — RESOLVED (§4.8):** `free_vars` lives in `src/ast.rs` (pure AST traversal; no new module, no
  `ARCHITECTURE.md` change). If it ever grows enough to warrant extraction to `src/analysis.rs`, that
  extraction adds a module-map row.
- **Q3 — example shape.** Confirm the guide example builds entirely from existing primitives (no list
  construction needed): higher-order calls + `for`-print + pipelines. Lean: yes (verified feasible).

## 8.5. 3C convergence gate (2026-06-18)

Ran the 30/8 pre-implementation convergence gate against this spec: **converged at 8 consecutive clean
cycles (cycle 20 of 30)**, after 19 findings in the first 12 cycles, all folded in above. Notable
catches: **F15** (function-type annotation syntax `(int)->int` — `Type::Function` + `parse_type` +
`resolve_type`/`resolve_cty`/`emit_type`/`expand_aliases`, without which the core example is
unwritable); **F4** (retire four dead `BinaryOp::Pipe` backend stubs); **F8** (reject `this` in lambda
bodies — silent `run`↔`runvm` divergence otherwise); **F-kw** (reserve `fn` as a keyword — a lexer
change); **F1** (calling an optional function is rejected); **F13** (test lambda calls in
height-sensitive contexts); and the complete §4.10 match-family table (`Op`/`Value`/`CTy`/`Type`/`Expr`).

## 9. Decisions log

- [2026-06-18] AGREED: syntax `fn(int x) => expr` + statement-body `fn(int x) { … }` (A1).
- [2026-06-18] AGREED: explicit param types, inferred return (A2).
- [2026-06-18] AGREED: `x |> rhs ≡ rhs(lhs)`, lowered to `Call` in the parser (A3).
- [2026-06-18] AGREED: bare named-fn ident is a zero-capture closure value (A4).
- [2026-06-18] AGREED: by-value capture, acyclic ⇒ no GC (A5).
- [2026-06-18] AGREED: function-type assignability = exact structural equality, variance deferred (A6).
- [2026-06-18] AGREED: scope = lambdas + function-value calls + named-fn refs + pipe; `core.list`
  map/filter/reduce **deferred** to S4.5 generics.
