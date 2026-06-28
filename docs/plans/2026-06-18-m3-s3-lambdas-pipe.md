# M3 S3 — Lambdas + Pipe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended)
> or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** Add first-class anonymous functions (`fn(int x) => expr` / `fn(int x) { … }`), function-value
calls, bare named-function references, and the pipe operator `|>` to Phorj — byte-identical on both
backends and round-tripped through real PHP.

**Architecture:** Lambdas become a `Value::Closure(Rc<ClosureData>)` (per-backend variant: interpreter
walks the captured AST; VM runs a compiled function with positional captures). Capture is **by value**
at creation (immutable, acyclic heap ⇒ no GC). The pipe is **lowered to a `Call` in the parser**
(`x |> f ≡ f(x)`), so no backend needs pipe-specific code. Two new `Op`s: `MakeClosure(func_idx)` and
`CallValue(argc)`.

**Tech Stack:** Rust 2021 (std-only, `#![forbid(unsafe_code)]`), the existing
lexer→parser→checker→{interpreter, bytecode-VM, PHP-transpiler} pipeline, `tests/differential.rs`
spine, PHP 8.6 round-trip.

**Source of truth:** `docs/specs/2026-06-18-m3-s3-lambdas-pipe-design.md` (decisions A1–A6, §4 surfaces,
§4.10 match-family table, the 19 3C findings F1–F19).

## Global Constraints

- **Byte-identity spine (invariant #1):** every executable feature must pass `agree(src)` in
  `tests/differential.rs` (run ≡ runvm) and the guide example must round-trip through real PHP 8.6.
- **Value kernels single-sourced (invariant #3):** never re-inline arithmetic/compare in a backend.
- **`Op` lockstep (invariant #5):** a new `Op` extends `vm::exec_op` + `chunk::validate` (index-carrying
  only) + `compiler::stack_effect` in the **same commit**.
- **No panic on input (EV-7):** adversarial `.phg` exits 1 with a clean `Diagnostic`, never a panic.
- **Quality gate (invariant #10):** every commit is green — `cargo test`, `cargo clippy --all-targets`,
  `cargo fmt --check`, `cargo build --release` all clean. `PATH=/stack/tools/cargo/bin:$PATH`.
- **Capture by value (A5):** `free_vars` (sorted, invariant #8) is the single source of capture order
  for the VM slots, the interpreter env, and the PHP `use()` list.
- **Mandatory `package Main;` (M5 S1):** every program/example starts with `package Main;`; the
  differential harness `with_pkg` auto-prepends it for inline snippets.
- **Git autonomy (project rule):** commit each green task with a `feat:`/`test:`/`docs:` message, no
  `Co-Authored-By` line.
- **Examples ship with features:** the feature lands with a runnable `examples/guide/` program (auto
  byte-identity-gated) + `examples/README.md` entry, in the same change set.
- **PHP:** `/stack/tools/phpbrew/php/php-master/bin/php` (8.6.0-dev).

**Match-family backstop (spec §4.10):** the new variants `Op` (×2), `Value::Closure`, `CTy::Fn`,
`Type::Function`, `Expr::Lambda` each open a family of exhaustive `match` sites. Before editing in any
task that adds a variant, `grep` the family and add every arm in the same commit — the compiler's
exhaustiveness error is the loud backstop.

---

## File Structure

| File | Responsibility | Tasks |
|------|----------------|-------|
| `src/token.rs` | `TokenKind::Fn` | T1 |
| `src/lexer.rs` | `"fn"` keyword recognition | T1 |
| `src/types.rs` | `Ty::Function` + `Display` + `assignable` exact-eq | T2 |
| `src/ast.rs` | `Type::Function`, `Expr::Lambda`, `LambdaBody`, `free_vars` | T2, T3, T6 |
| `src/parser.rs` | `parse_type` `(…)->T`; lambda in `parse_primary`; pipe→`Call` lowering; `sexpr` arms | T2, T3, T6, T7 |
| `src/checker.rs` | `resolve_type`/`expand_aliases` (`Type::Function`); `check_lambda`; function-value `check_call`; named-ref value position; `E-LAMBDA-THIS` | T2, T3, T6 |
| `src/value.rs` | `Value::Closure(Rc<ClosureData>)` + the 6 `Value`-match arms | T3 |
| `src/interpreter.rs` | eval `Expr::Lambda`, named-ref, closure call; retire pipe stub | T3, T4, T7 |
| `src/chunk.rs` | `Op::MakeClosure`/`Op::CallValue`; `Function.n_captures`; `validate` arms | T4 |
| `src/compiler.rs` | `CTy::Fn` (+ `num_ty`/`resolve_cty`/`ctype`); compile lambda/closure-call/named-ref; `stack_effect`; retire pipe stub | T4, T6, T7 |
| `src/vm.rs` | exec `MakeClosure`/`CallValue`; `Value::Closure` match arms | T4 |
| `src/transpile.rs` | `emit_type(Function)`→`\Closure`; arrow-fn / `function(){} use()`; named-ref; retire pipe stub | T2, T5, T6, T7 |
| `tests/differential.rs` | `agree`/`agree_err` cases for all forms (incl. height-sensitive) | T4, T5, T6 |
| `examples/guide/lambdas-pipe.phg`, `examples/README.md` | runnable showcase | T8 |
| `FEATURES.md`, `CHANGELOG.md`, `KNOWN_ISSUES.md`, `phg explain` dict, `docs/MILESTONES.md`/`CLAUDE.md` | docs | T8 |

---

## Task 1: Reserve `fn` as a keyword

**Files:**
- Modify: `src/token.rs` (add `Fn` variant near `Function`)
- Modify: `src/lexer.rs:298` (`keyword()` map) and the multi-token test vector `src/lexer.rs:546`
- Test: inline in `src/lexer.rs` tests

**Interfaces:**
- Produces: `TokenKind::Fn` — the lambda-leading keyword consumed by `parse_primary` (T3).

- [ ] **Step 1: Guard — grep for any existing `fn` identifier (must be empty)**

Run:
```bash
cd /stack/projects/phorj
grep -rn "\bfn\b" examples/ && echo "FOUND — migrate before proceeding" || echo "clear in examples"
grep -rn "\"fn\"\|\bfn\b" src/*.rs | grep -iv "fn \|fn(" | grep -i "ident\|test prog\|agree(" || echo "no fn-as-ident in test programs"
```
Expected: `clear in examples`; no `fn`-as-identifier in any inline test program. If any found, migrate it first (rename to e.g. `func`).

- [ ] **Step 2: Write the failing test**

Add to the lexer tests in `src/lexer.rs`:
```rust
#[test]
fn fn_is_a_reserved_keyword() {
    use TokenKind::*;
    assert_eq!(kinds("fn"), vec![Fn, Eof]);
    // contextual sanity: `fn (` still lexes as the keyword then a paren
    assert_eq!(kinds("fn ("), vec![Fn, LParen, Eof]);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test fn_is_a_reserved_keyword`
Expected: FAIL — `TokenKind::Fn` does not exist / `kinds("fn")` yields `Ident("fn")`.

- [ ] **Step 4: Add the token + keyword mapping**

In `src/token.rs`, add `Fn,` to the `TokenKind` enum directly after `Function,`.
In `src/lexer.rs` `keyword()` (≈line 301), add a line beside `"function" => Function,`:
```rust
"fn" => Fn,
```
In the multi-token test vector at `src/lexer.rs:546`, no change needed (it lists operators, not `fn`).

- [ ] **Step 5: Run tests to verify they pass**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test fn_is_a_reserved_keyword && cargo test --quiet`
Expected: the new test PASSES; the full suite stays green (no program used `fn` as an identifier).

- [ ] **Step 6: Quality gate + commit**

```bash
PATH=/stack/tools/cargo/bin:$PATH cargo clippy --all-targets && cargo fmt --check
git add src/token.rs src/lexer.rs
git commit -m "feat(lang): reserve \`fn\` as a keyword (M3 S3, F-kw)"
```

---

## Task 2: Function-type annotations — `(int) -> int`, `Ty::Function`, type machinery

**Files:**
- Modify: `src/types.rs` (`Ty::Function`, `Display`, `assignable`)
- Modify: `src/ast.rs` (`Type::Function`)
- Modify: `src/parser.rs:346` (`parse_type` leading-`(`), `sexpr` type rendering
- Modify: `src/checker.rs:159` (`resolve_type`), `:1421` (`expand_aliases`)
- Modify: `src/compiler.rs:47` (`CTy::Fn`), `:514` (`resolve_cty`), `:771` (`num_ty`)
- Modify: `src/transpile.rs` (`emit_type`/`ret_hint`)
- Test: inline in `src/parser.rs`, `src/checker.rs`, `src/types.rs`

**Interfaces:**
- Produces:
  - `ast::Type::Function { params: Vec<Type>, ret: Box<Type>, span: Span }`
  - `types::Ty::Function(Vec<Ty>, Box<Ty>)`
  - `compiler::CTy::Fn { params: Vec<CTy>, ret: Box<CTy> }`
  - `Ty::assignable` treats two `Function`s as assignable iff params pairwise-equal and returns equal.

- [ ] **Step 1: Write the failing tests**

In `src/types.rs` tests:
```rust
#[test]
fn function_type_assignability_is_exact() {
    let int_to_int = Ty::Function(vec![Ty::Int], Box::new(Ty::Int));
    let int_to_int2 = Ty::Function(vec![Ty::Int], Box::new(Ty::Int));
    let int_to_float = Ty::Function(vec![Ty::Int], Box::new(Ty::Float));
    assert!(Ty::assignable(&int_to_int, &int_to_int2));
    assert!(!Ty::assignable(&int_to_int, &int_to_float)); // no variance (A6)
    assert!(!Ty::assignable(&Ty::Int, &int_to_int));      // int is not a function
    assert_eq!(format!("{int_to_int}"), "(int) -> int");
}
```
In `src/parser.rs` tests (mirroring existing `parse_type` tests):
```rust
#[test]
fn parses_function_type_annotation() {
    // a function-typed parameter must parse
    let item = parse_one_item("package Main; function apply(int x, (int) -> int f) -> int { return x; }");
    assert!(item.is_ok(), "function-typed param should parse: {item:?}");
    // nested + zero-arg
    assert!(parse_one_item("package Main; function f() -> () -> int { }").is_ok());
}
```
In `src/checker.rs` tests:
```rust
#[test]
fn function_typed_binding_rejects_non_function() {
    // var f : (int)->int = 5;  -> int not assignable to a function type
    let errs = check_errs("package Main; function main() { var f: (int) -> int = 5; }");
    assert!(errs.iter().any(|e| e.message.contains("(int) -> int")), "{errs:?}");
}
```
(Use whatever the existing parse/check test helpers are named — `parse_one_item`/`check_errs` are
placeholders for the real harness in those files; match the surrounding tests.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test function_type`
Expected: FAIL — `Ty::Function` and `Type::Function` do not exist; `parse_type` rejects `(`.

- [ ] **Step 3: Add `Ty::Function` + `Display` + `assignable`** (`src/types.rs`)

```rust
// in enum Ty
Function(Vec<Ty>, Box<Ty>),
```
In `Display`:
```rust
Ty::Function(params, ret) => {
    let ps = params.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
    write!(f, "({ps}) -> {ret}")
}
```
In `assignable`, add before the final `_ => from == to`:
```rust
(Ty::Function(fp, fr), Ty::Function(tp, tr)) => {
    fp.len() == tp.len() && fp.iter().zip(tp).all(|(a, b)| a == b) && fr == tr
}
```

- [ ] **Step 4: Add `Type::Function` AST + `parse_type` branch + `sexpr`** (`src/ast.rs`, `src/parser.rs`)

`src/ast.rs`, in `enum Type`:
```rust
/// `(int, string) -> bool`
Function { params: Vec<Type>, ret: Box<Type>, span: Span },
```
`src/parser.rs` `parse_type` (≈line 346): before the `Ident`-required match, handle a leading `(`:
```rust
if self.eat(&TokenKind::LParen) {
    let mut params = Vec::new();
    if !self.check(&TokenKind::RParen) {
        params.push(self.parse_type()?);
        while self.eat(&TokenKind::Comma) { params.push(self.parse_type()?); }
    }
    self.expect(&TokenKind::RParen, "')' to close function-type parameters")?;
    self.expect(&TokenKind::Arrow, "'->' in a function type")?;
    let ret = Box::new(self.parse_type()?);
    let mut t = Type::Function { params, ret, span: sp };
    while self.eat(&TokenKind::Question) { t = Type::Optional { inner: Box::new(t), span: sp }; }
    return Ok(t);
}
```
Add the `sexpr` rendering arm for `Type::Function` wherever types are rendered in parser tests (search
for the existing `Type::Named`/`Type::Optional` render arm and add `Function`).

- [ ] **Step 5: Add checker + compiler + transpiler type arms**

`src/checker.rs` `resolve_type` (≈line 159), add:
```rust
Type::Function { params, ret, .. } => Ty::Function(
    params.iter().map(|p| self.resolve_type(p)).collect(),
    Box::new(self.resolve_type(ret)),
),
```
`src/checker.rs` `expand_aliases` `Type` walker (≈line 1440), add:
```rust
Type::Function { params, ret, span } => Type::Function {
    params: params.iter().map(expand_ty).collect(),  // use the same recursive fn the arm uses
    ret: Box::new(expand_ty(ret)),
    span: *span,
},
```
(Match the exact name of the recursive expander used by the surrounding `Named`/`Optional` arms.)
`src/compiler.rs`: add `CTy::Fn { params: Vec<CTy>, ret: Box<CTy> }` to `enum CTy` (≈line 47);
in `resolve_cty` (≈line 514) add:
```rust
Type::Function { params, ret, .. } => CTy::Fn {
    params: params.iter().map(resolve_cty).collect(),
    ret: Box::new(resolve_cty(ret)),
},
```
in `num_ty`'s non-numeric arm (≈line 774) extend to include `CTy::Fn { .. }` → `None`.
`src/transpile.rs` `emit_type`/`ret_hint`: add a `Type::Function { .. } => "\\Closure".into()` arm
(a function-typed param/return renders as PHP `\Closure`).

- [ ] **Step 6: Run tests to verify they pass + full suite**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test function_type && cargo test --quiet`
Expected: the three new tests PASS; full suite green (build breaks loudly if any `CTy`/`Type` match arm
was missed — add it).

- [ ] **Step 7: Quality gate + commit**

```bash
PATH=/stack/tools/cargo/bin:$PATH cargo clippy --all-targets && cargo fmt --check
git add src/types.rs src/ast.rs src/parser.rs src/checker.rs src/compiler.rs src/transpile.rs
git commit -m "feat(lang): function-type annotations \`(T)->R\` + Ty::Function (M3 S3, F15/F16/F17)"
```

---

## Task 3: Expression-body lambdas on the interpreter

**Files:**
- Modify: `src/ast.rs` (`Expr::Lambda`, `LambdaBody`, `free_vars`)
- Modify: `src/parser.rs` (`parse_primary` lambda branch; `sexpr` `Expr::Lambda` arm)
- Modify: `src/checker.rs` (`check_lambda`, function-value `check_call`, named-ref value position, `E-LAMBDA-THIS`)
- Modify: `src/value.rs` (`Value::Closure(Rc<ClosureData>)` + the 6 `Value`-match arms)
- Modify: `src/interpreter.rs` (eval `Expr::Lambda`, named-ref, closure call)
- Test: inline `#[test]`s asserting `phg run` output (run-only — VM comes in Task 4)

**Interfaces:**
- Produces:
  - `ast::Expr::Lambda { params: Vec<Param>, ret: Option<Type>, body: LambdaBody, span: Span }`
  - `ast::LambdaBody { Expr(Box<Expr>), Block(Vec<Stmt>) }`  (only `Expr` constructed this task)
  - `ast::free_vars(params: &[Param], body: &LambdaBody) -> Vec<String>` (sorted)
  - `value::Value::Closure(Rc<ClosureData>)`, `value::ClosureData { Tree{params,ret,body,env}, Named(String), Byte{func,captures} }`
- Consumes: `Ty::Function` (T2), `TokenKind::Fn` (T1).

- [ ] **Step 1: Write the failing run-only tests** (helper that runs only the interpreter)

In `src/interpreter.rs` tests (use the existing run helper, here `run_ok`):
```rust
#[test]
fn lambda_value_call_interpreter() {
    let out = run_ok(r#"package Main;
import core.console;
function main() {
    var double = fn(int x) => x * 2;
    console.println("{double(5)}");          // 10
}"#);
    assert_eq!(out, "10\n");
}

#[test]
fn lambda_captures_two_vars_interpreter() {
    let out = run_ok(r#"package Main;
import core.console;
function main() {
    var a = 10;
    var b = 100;
    var f = fn(int x) => x + a + b;
    console.println("{f(1)}");               // 111
}"#);
    assert_eq!(out, "111\n");
}

#[test]
fn higher_order_user_function_interpreter() {
    let out = run_ok(r#"package Main;
import core.console;
function twice(int x, (int) -> int f) -> int { return f(f(x)); }
function main() {
    console.println("{twice(3, fn(int n) => n + 1)}");  // 5
}"#);
    assert_eq!(out, "5\n");
}
```
And a checker-rejection test:
```rust
#[test]
fn lambda_cannot_reference_this() {
    let errs = check_errs(r#"package Main;
class C { constructor(public int x) {}
  method() -> (int) -> int { return fn(int n) => n + this.x; } }
function main() { }"#);
    assert!(errs.iter().any(|e| e.message.contains("`this`")), "{errs:?}");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test lambda_ higher_order_user`
Expected: FAIL — `fn(...)` does not parse; `Expr::Lambda`/`Value::Closure` undefined.

- [ ] **Step 3: Add the AST nodes + `free_vars`** (`src/ast.rs`)

```rust
// in enum Expr
Lambda { params: Vec<Param>, ret: Option<Type>, body: LambdaBody, span: Span },

#[derive(Debug, Clone, PartialEq)]
pub enum LambdaBody {
    Expr(Box<Expr>),
    Block(Vec<Stmt>),
}

/// Sorted (invariant #8) free variables of a lambda: identifiers referenced in `body`
/// that are not the lambda's params, not locals bound inside the body, and not global
/// function names / imported qualifiers. Single source of capture order for all backends.
pub fn free_vars(params: &[Param], body: &LambdaBody) -> Vec<String> {
    let mut bound: std::collections::HashSet<String> =
        params.iter().map(|p| p.name.clone()).collect();
    let mut found: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    match body {
        LambdaBody::Expr(e) => collect_free_expr(e, &mut bound, &mut found),
        LambdaBody::Block(stmts) => collect_free_block(stmts, &mut bound, &mut found),
    }
    found.into_iter().collect() // BTreeSet ⇒ already sorted
}
```
Implement the two private walkers `collect_free_expr(&Expr, &mut HashSet, &mut BTreeSet)` and
`collect_free_block(&[Stmt], …)`: add `Ident(n)` to `found` iff not in `bound`; recurse all
sub-expressions; when entering a binding form (`var`, `if (var …)`, `for (T x in …)`, match arm
bindings, nested `Lambda` params) add the bound names to a cloned `bound` set for the inner scope.
Global-function-name exclusion is handled at the call sites (the compiler/checker skip names that
resolve to `funcs`/imports), so `free_vars` may over-report a global name; the consumers filter it.
(Document this in a doc-comment.)

- [ ] **Step 4: Parse lambdas in `parse_primary` + `sexpr` arm** (`src/parser.rs`)

In `parse_primary`, when `self.peek() == TokenKind::Fn`:
```rust
TokenKind::Fn => {
    let sp = self.peek_span();
    self.advance(); // 'fn'
    self.expect(&TokenKind::LParen, "'(' after 'fn'")?;
    let mut params = Vec::new();
    if !self.check(&TokenKind::RParen) {
        params.push(self.parse_param()?);          // reuse the fn-decl param parser (typed)
        while self.eat(&TokenKind::Comma) { params.push(self.parse_param()?); }
    }
    self.expect(&TokenKind::RParen, "')' to close lambda parameters")?;
    let ret = if self.eat(&TokenKind::Arrow) { Some(self.parse_type()?) } else { None };
    let body = if self.eat(&TokenKind::FatArrow) {
        LambdaBody::Expr(Box::new(self.parse_expr()?))      // expression body
    } else {
        // Block body: parsed in Task 6. For now require '=>'.
        return Err(self.error("'=>' and an expression (statement-body lambdas land in S3 Task 6)"));
    };
    return Ok(Expr::Lambda { params, ret, body, span: sp });
}
```
(Note: an optional `-> T` may appear before `=>`; for expression bodies it is allowed but the return is
inferred — if present it is checked for consistency in T3 Step 5.) Add the `sexpr` arm for
`Expr::Lambda` (render e.g. `(lambda (x) <body>)`) so parser/debug tests stay exhaustive.

- [ ] **Step 5: Checker — `check_lambda`, function-value calls, named refs, `this` rejection** (`src/checker.rs`)

Add `check_lambda` (called from `check_expr`'s new `Expr::Lambda` arm):
```rust
fn check_lambda(&mut self, params: &[Param], ret: &Option<Type>, body: &LambdaBody, span: Span) -> Ty {
    if lambda_uses_this(body) {            // small recursive scan for Expr::This
        self.err_coded(span, "a lambda cannot reference `this` yet", "E-LAMBDA-THIS",
            Some("bind `var self = this;` before the lambda and capture `self`".into()));
    }
    let param_tys: Vec<Ty> = params.iter().map(|p| self.resolve_type(&p.ty)).collect();
    self.push_scope();
    for p in params { self.declare(&p.name, self.resolve_type(&p.ty), p.span); }
    let saved_ret = std::mem::replace(&mut self.cur_ret, Ty::Error);
    let ret_ty = match body {
        LambdaBody::Expr(e) => {
            let t = self.check_expr(e);
            if let Some(rt) = ret { let declared = self.resolve_type(rt);
                if !Ty::assignable(&t, &declared) { self.err_assign(span, &t, &declared); }
                declared
            } else { t }
        }
        LambdaBody::Block(_) => {            // T6; require explicit -> T (A2/F10)
            let declared = match ret { Some(rt) => self.resolve_type(rt),
                None => self.err(span, "a statement-body lambda needs an explicit `-> T`") };
            self.cur_ret = declared.clone();
            // (T6 wires the block body here)
            declared
        }
    };
    self.cur_ret = saved_ret;
    self.pop_scope();
    Ty::Function(param_tys, Box::new(ret_ty))
}
```
In `check_call` (≈line 896): keep named/native/method/ctor paths; in the fallback, if
`self.check_expr(callee)` is `Ty::Function(params, ret)`, run `check_args("<lambda>", &params, args,
span)` and return `*ret`; if it is `Ty::Optional(Function)` emit "not callable — unwrap the optional
first" (F1); else the existing "not callable" error.
In `check_expr` for `Expr::Ident(name)`: if not a local but `self.funcs.contains_key(name)`, return
`Ty::Function(sig.params.clone(), Box::new(sig.ret.clone()))` (A4 bare named-fn ref).

- [ ] **Step 6: Add `Value::Closure` + the 6 Value-match arms** (`src/value.rs`)

```rust
// in enum Value
Closure(Rc<ClosureData>),

#[derive(Debug, Clone)]
pub enum ClosureData {
    Tree { params: Vec<crate::ast::Param>, ret: Option<crate::ast::Type>,
           body: crate::ast::LambdaBody, env: Vec<(String, Value)> },
    Named(String),
    Byte { func: usize, captures: Vec<Value> },   // constructed by the VM in Task 4
}
```
Add `Value::Closure` arms to every exhaustive `Value` match (grep `match .* Value` / the methods):
- `type_name` → `"function"`
- `as_display` → `Err` / unreachable-guard ("cannot display a function") — never panic
- equality kernels (`Eq`/`Ne`) → clean error ("cannot compare functions")
- `compare_ord` → clean error
- truthiness (if present) → clean error
- `Value`→`HKey` conversion → clean error ("a function is not a valid Map/Set key")
All are unreachable in well-typed programs (the checker forbids these) but required for exhaustiveness.

- [ ] **Step 7: Interpreter — construct + call closures** (`src/interpreter.rs`)

`Expr::Lambda` ⟹ snapshot env from the sorted `free_vars` (look each up in the current scope; skip
names that resolve to a global function), build `Value::Closure(Rc::new(ClosureData::Tree { params,
ret, body, env }))`. Bare named-fn ref (`Expr::Ident` resolving to a function, value position) ⟹
`Value::Closure(Rc::new(ClosureData::Named(name)))`. Calling a `Value::Closure`:
```rust
match &*closure {
    ClosureData::Tree { params, body, env, .. } => {
        self.push_call_scope();                       // fresh scope, this = None
        for (k, v) in env { self.declare_local(k, v.clone()); }
        for (p, a) in params.iter().zip(args) { self.declare_local(&p.name, a); }
        let rv = match body {
            LambdaBody::Expr(e) => self.eval(e)?,
            LambdaBody::Block(b) => self.run_block_as_call(b)?,   // T6
        };
        self.pop_call_scope();
        rv
    }
    ClosureData::Named(name) => self.call_named_fn(name, args)?,  // existing named-call path
    ClosureData::Byte { .. } => unreachable!("VM closure in the interpreter"),
}
```
(Adapt to the interpreter's actual scope API — `CallScopes`. The key invariant: a closure call runs
with `this = None` and the captured env + params in scope.)

- [ ] **Step 8: Run the run-only tests + full suite**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test lambda_ higher_order_user && cargo test --quiet`
Expected: the four new tests PASS; full suite green. (`phg runvm` on a lambda will still error
"unsupported" — that is Task 4; no `agree()` test exists yet, so the suite stays green.)

- [ ] **Step 9: Quality gate + commit**

```bash
PATH=/stack/tools/cargo/bin:$PATH cargo clippy --all-targets && cargo fmt --check
git add src/ast.rs src/parser.rs src/checker.rs src/value.rs src/interpreter.rs
git commit -m "feat(lang): expression-body lambdas on the interpreter (M3 S3, F8/F1)"
```

---

## Task 4: Expression-body lambdas on the VM — `agree()` byte-identity

**Files:**
- Modify: `src/chunk.rs` (`Op::MakeClosure`/`Op::CallValue`; `Function.n_captures`; `validate` arm)
- Modify: `src/compiler.rs` (compile `Expr::Lambda` → nested `Function` + `MakeClosure`; compile a
  function-value call → `CallValue`; named-ref → `MakeClosure(0-cap)`; `ctype(Lambda)`/`ctype(Call on Fn)`;
  `stack_effect` arms)
- Modify: `src/vm.rs` (exec `MakeClosure`/`CallValue`; `Value::Closure` arms in any VM `Value` match)
- Test: `tests/differential.rs` (`agree` for every interpreter test above + height-sensitive)

**Interfaces:**
- Consumes: `Value::ClosureData::Byte` (T3), `CTy::Fn` (T2).
- Produces: `Op::MakeClosure(usize)`, `Op::CallValue(usize)`, `chunk::Function.n_captures: usize`.

- [ ] **Step 1: Write the failing differential tests** (`tests/differential.rs`)

```rust
#[test]
fn lambdas_agree() {
    agree("import core.console; function main() { var d = fn(int x) => x*2; console.println(\"{d(5)}\"); }");
    agree("import core.console; function main() { var a=10; var b=100; var f=fn(int x)=>x+a+b; console.println(\"{f(1)}\"); }");
    agree("import core.console; function twice(int x,(int)->int f)->int{return f(f(x));} function main(){ console.println(\"{twice(3, fn(int n)=>n+1)}\"); }");
    // F13 — height-sensitive contexts: lambda call inside interpolation AND inside a match arm
    agree("import core.console; function main(){ var inc=fn(int x)=>x+1; console.println(\"{inc(1)} {inc(2)}\"); }");
    agree("import core.console; enum E{A(),B()} function pick(E e,(int)->int f)->int{ return match e { A()=>f(1), B()=>f(2) }; } function main(){ console.println(\"{pick(A(), fn(int x)=>x*10)}\"); }");
}

#[test]
fn lambda_call_errors_agree() {
    agree_err("import core.console; function main(){ var f=fn(int x)=>x; console.println(\"{f(1,2)}\"); }"); // arity
}
```
(The harness's `with_pkg` auto-prepends `package Main;`.)

- [ ] **Step 2: Run to verify they fail**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test lambdas_agree lambda_call_errors_agree`
Expected: FAIL — `runvm` errors "the `|>`/lambda … not supported" / backend mismatch.

- [ ] **Step 3: Add the two `Op`s + `Function.n_captures` + `validate`** (`src/chunk.rs`)

```rust
// in enum Op
MakeClosure(usize),   // operand = function-table index; pops functions[idx].n_captures values
CallValue(usize),     // operand = argc; pops [closure, arg0..arg(argc-1)]
```
Add `pub n_captures: usize` to `struct Function` (default 0 for named fns/ctors/methods — update all
constructors). In `validate` (the per-op match), add:
```rust
Op::MakeClosure(idx) if *idx >= nfns => Some(format!("closure target {idx} out of range")),
```
`CallValue` carries no table index ⇒ **no** `validate` arm (falls through `_ => None`).

- [ ] **Step 4: Compiler — compile lambdas, closure calls, named refs; `stack_effect`** (`src/compiler.rs`)

Compiling `Expr::Lambda`:
1. `let caps = ast::free_vars(params, body); let caps: Vec<_> = caps.into_iter().filter(|n| self.resolve_local(n).is_some()).collect();` (keep only enclosing locals — drops global fn names).
2. Build a sub-compiler seeded with locals `[caps…, params…]` (captures first); compile the body
   (expr body ⟹ `expr(e)` then `Op::Return`).
3. `let idx = self.push_function(Function { name: format!("<lambda@{}>", span.line), arity: caps.len()+params.len(), n_captures: caps.len(), chunk });`
4. In the enclosing chunk: for each cap name (sorted order) `self.emit(Op::GetLocal(self.resolve_local(name).unwrap()), line);` then `self.emit(Op::MakeClosure(idx), line);`.

Bare named-fn ref (`Expr::Ident(name)` in value position, `name` in `self.fns`):
`self.emit(Op::MakeClosure(named_idx), line)` (that function has `n_captures == 0`).

Calling a function value (in `compile_call`, when the callee is not a known named/native/variant/class
but is function-typed): `self.expr(callee)?;` (pushes the closure) then `for a in args { self.expr(a)?; }`
then `self.emit(Op::CallValue(args.len()), line);`.

`ctype`: `Expr::Lambda { .. }` → `CTy::Fn { … }` (params/ret from annotations); a call whose callee is
`CTy::Fn { ret, .. }` → `*ret`. `stack_effect`: `Op::MakeClosure(idx) => 1 - self.functions[*idx].n_captures as isize`; `Op::CallValue(argc) => -(*argc as isize)`.

- [ ] **Step 5: VM — exec the two Ops + `Value::Closure` match arms** (`src/vm.rs`)

```rust
Op::MakeClosure(idx) => {
    let n = self.program.functions[idx].n_captures;
    let captures = self.split_off(n);                 // top n values, in order
    self.stack.push(Value::Closure(Rc::new(ClosureData::Byte { func: idx, captures })));
}
Op::CallValue(argc) => {
    if self.frames.len() >= MAX_CALL_DEPTH { return Err("stack overflow".into()); }
    let args = self.split_off(argc);                  // pop args
    let closure = match self.stack.pop() {            // pop the closure beneath them
        Some(Value::Closure(c)) => c,
        other => return Err(format!("cannot call {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let (func, captures) = match &*closure {
        ClosureData::Byte { func, captures } => (*func, captures.clone()),
        _ => unreachable!("interpreter closure in the VM"),
    };
    let slot_base = self.stack.len();
    self.stack.extend(captures);                      // captures first…
    self.stack.extend(args);                          // …then args  ⇒ [caps.., params..]
    self.frames.push(Frame { func, ip: 0, slot_base });
}
```
Add `Value::Closure` arms to any `Value` match in `vm.rs` (e.g. a debug/`type_name` path) — clean error
/ unreachable, never panic. (`do_return` is unchanged — it truncates to `slot_base` and pushes the rv.)

- [ ] **Step 6: Run the differential tests + full suite**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test lambdas_agree lambda_call_errors_agree && cargo test --quiet`
Expected: PASS — run ≡ runvm byte-identical, including the two-capture, interpolation, and match-arm
cases (F13). Full suite green.

- [ ] **Step 7: Quality gate + commit**

```bash
PATH=/stack/tools/cargo/bin:$PATH cargo clippy --all-targets && cargo fmt --check
git add src/chunk.rs src/compiler.rs src/vm.rs tests/differential.rs
git commit -m "feat(vm): expression-body lambdas on the VM — MakeClosure/CallValue, byte-identical (M3 S3, F13)"
```

---

## Task 5: Expression-body lambdas — transpiler + real-PHP round-trip

**Files:**
- Modify: `src/transpile.rs` (`emit_expr` `Expr::Lambda` → PHP arrow fn; bare named-ref → `myFn(...)`)
- Test: inline transpile test + a manual PHP round-trip check folded into Task 8's example

**Interfaces:**
- Consumes: `Expr::Lambda` (T3), `free_vars` (unused for arrow fns — auto-capture).

- [ ] **Step 1: Write the failing transpile test** (`src/transpile.rs`)

```rust
#[test]
fn transpiles_expression_lambda_to_arrow_fn() {
    let php = transpile_ok("package Main; import core.console; function main(){ var d = fn(int x) => x*2; console.println(\"{d(5)}\"); }");
    assert!(php.contains("fn($x) => $x * 2"), "{php}");
}

#[test]
fn transpiles_named_fn_reference() {
    let php = transpile_ok("package Main; function inc(int x)->int{return x+1;} function apply(int x,(int)->int f)->int{return f(x);} function main(){ apply(1, inc); }");
    assert!(php.contains("inc(...)"), "first-class callable: {php}");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test transpiles_expression_lambda transpiles_named_fn`
Expected: FAIL — `emit_expr` has no `Expr::Lambda` arm.

- [ ] **Step 3: Implement the transpiler arms** (`src/transpile.rs`)

In `emit_expr`:
```rust
Expr::Lambda { params, body, .. } => {
    let ps = params.iter().map(|p| format!("${}", p.name)).collect::<Vec<_>>().join(", ");
    match body {
        LambdaBody::Expr(e) => Ok(format!("fn({ps}) => {}", self.emit_expr(e)?)),   // arrow fn: auto by-value capture
        LambdaBody::Block(_) => /* Task 6 */ unreachable!("statement bodies land in S3 Task 6"),
    }
}
```
Bare named-fn ref in value position (an `Expr::Ident(name)` the checker typed as a function and the
transpiler isn't emitting as a call): emit `format!("{name}(...)")` (PHP 8.1 first-class callable;
namespaced names keep the existing FQN logic).

- [ ] **Step 4: Run tests + manual PHP round-trip**

Run:
```bash
PATH=/stack/tools/cargo/bin:$PATH cargo test transpiles_expression_lambda transpiles_named_fn
cat > /tmp/lam.phg <<'EOF'
package Main;
import core.console;
function twice(int x, (int) -> int f) -> int { return f(f(x)); }
function main() { console.println("{twice(3, fn(int n) => n + 1)}"); }
EOF
PATH=/stack/tools/cargo/bin:$PATH cargo run --quiet -- run /tmp/lam.phg
PATH=/stack/tools/cargo/bin:$PATH cargo run --quiet -- transpile /tmp/lam.phg > /tmp/lam.php
/stack/tools/phpbrew/php/php-master/bin/php /tmp/lam.php
```
Expected: both print `5`. (`twice` is a statement-body *named* function — fine; the lambda passed in is
expression-body.) Clean up `/tmp/lam.*`.

- [ ] **Step 5: Quality gate + commit**

```bash
PATH=/stack/tools/cargo/bin:$PATH cargo clippy --all-targets && cargo fmt --check
git add src/transpile.rs
git commit -m "feat(transpile): expression lambdas → PHP arrow fn + named-fn refs (M3 S3)"
```

---

## Task 6: Statement-body lambdas (`fn(int x) -> int { … }`)

**Files:**
- Modify: `src/parser.rs` (block-body branch in the lambda parser)
- Modify: `src/checker.rs` (`check_lambda` block path — explicit `-> T` required, A2/F10)
- Modify: `src/compiler.rs` (compile a block-body lambda like a function body)
- Modify: `src/interpreter.rs` (`run_block_as_call` for a `LambdaBody::Block`)
- Modify: `src/transpile.rs` (`function($x) use ($caps) { … }` via `free_vars`)
- Test: `tests/differential.rs` (`agree`) + a transpile test

**Interfaces:**
- Consumes: `LambdaBody::Block` (declared T3), `free_vars` (T3).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn statement_body_lambda_agrees() {
    agree("import core.console; function main(){ var base=100; var f = fn(int x) -> int { var y = x*2; return y + base; }; console.println(\"{f(3)}\"); }"); // 106
}
#[test]
fn statement_body_lambda_needs_return_type() {
    let errs = check_errs("package Main; function main(){ var f = fn(int x) { return x; }; }");
    assert!(errs.iter().any(|e| e.message.contains("explicit `-> T`")), "{errs:?}");
}
#[test]
fn transpiles_statement_lambda_with_use_clause() {
    let php = transpile_ok("package Main; import core.console; function main(){ var base=100; var f = fn(int x) -> int { return x + base; }; console.println(\"{f(3)}\"); }");
    assert!(php.contains("function($x) use ($base)") && php.contains("return $x + $base"), "{php}");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test statement_body_lambda transpiles_statement_lambda`
Expected: FAIL — the parser rejects a block body (T3 Step 4 returned an error for non-`=>`).

- [ ] **Step 3: Parser — accept a block body**

Replace the T3 placeholder error with:
```rust
let body = if self.eat(&TokenKind::FatArrow) {
    LambdaBody::Expr(Box::new(self.parse_expr()?))
} else if self.check(&TokenKind::LBrace) {
    LambdaBody::Block(self.parse_block()?)        // reuse the existing block-statement parser
} else {
    return Err(self.error("'=>' (expression body) or '{' (statement body)"));
};
```

- [ ] **Step 4: Checker — wire the block path** (already stubbed in `check_lambda`, T3 Step 5)

In the `LambdaBody::Block(stmts)` arm: require `ret` (else the F10 error), set `self.cur_ret`, then
`for s in stmts { self.check_stmt(s); }` inside the lambda scope (so `return` checks against the
lambda's `-> T`).

- [ ] **Step 5: Compiler + interpreter — run a block body**

Compiler: in the lambda sub-compiler, when `LambdaBody::Block(stmts)`, compile the statements then the
implicit `Op::Const(Unit); Op::Return` epilogue (identical to a named-function body). Interpreter:
implement `run_block_as_call(&[Stmt]) -> Result<Value, Signal>` (walk the block; a `return` yields the
value; fall-through yields `Unit`) — reuse the existing function-body execution path.

- [ ] **Step 6: Transpiler — `function(){} use()`**

```rust
LambdaBody::Block(stmts) => {
    let caps = ast::free_vars(params, body)
        .into_iter().filter(|n| /* it is a captured local, not a global fn */ true)
        .map(|n| format!("${n}")).collect::<Vec<_>>();
    let use_clause = if caps.is_empty() { String::new() } else { format!(" use ({})", caps.join(", ")) };
    let ps = params.iter().map(|p| format!("${}", p.name)).collect::<Vec<_>>().join(", ");
    let body_php = /* emit the block statements with the existing stmt emitter */;
    Ok(format!("function({ps}){use_clause} {{ {body_php} }}"))
}
```
(Match the transpiler's statement-emission helper; ensure `use` names are by value — no `&`.)

- [ ] **Step 7: Run all tests + manual PHP round-trip of the statement-body case; full suite**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test statement_body_lambda transpiles_statement_lambda && cargo test --quiet`
Plus a `/tmp` PHP round-trip of the `agree` program (expect `106`).
Expected: all PASS, run ≡ runvm ≡ PHP.

- [ ] **Step 8: Quality gate + commit**

```bash
PATH=/stack/tools/cargo/bin:$PATH cargo clippy --all-targets && cargo fmt --check
git add src/parser.rs src/checker.rs src/compiler.rs src/interpreter.rs src/transpile.rs tests/differential.rs
git commit -m "feat(lang): statement-body lambdas + PHP function(){}use() (M3 S3, A2/F10)"
```

---

## Task 7: Pipe operator `|>` — parser lowering + retire dead stubs

**Files:**
- Modify: `src/parser.rs` (lower `Pipe` to `Call` in `parse_binary`; update precedence tests `1268–1269`)
- Modify: `src/checker.rs:771`, `src/compiler.rs:1105`, `src/interpreter.rs:481`,
  `src/transpile.rs:524/790` (the four dead `BinaryOp::Pipe` stubs → `unreachable!`)
- Test: `tests/differential.rs` (`agree`) + updated parser precedence tests

**Interfaces:**
- Consumes: function-value calls (T4) + named refs (T4/T5). Pipe is purely a `Call` after lowering.

- [ ] **Step 1: Write the failing tests** (`tests/differential.rs` + update `src/parser.rs` tests)

```rust
#[test]
fn pipe_agrees() {
    agree("import core.console; function dbl(int x)->int{return x*2;} function inc(int x)->int{return x+1;} function main(){ console.println(\"{5 |> dbl |> inc}\"); }"); // inc(dbl(5)) = 11
    agree("import core.console; function main(){ var add=fn(int a,int b)->int{return a+b;}; console.println(\"{3 |> fn(int v) => v + 10}\"); }"); // 13
    agree("import core.console; function dbl(int x)->int{return x*2;} function main(){ console.println(\"{1 + 2 |> dbl}\"); }"); // dbl(1+2) = 6
}
```
Update `src/parser.rs` precedence tests at 1268–1269 to assert the lowered `Call` sexpr, e.g.:
```rust
assert_eq!(sexpr(&expr("x |> f")), "(call f x)");
assert_eq!(sexpr(&expr("a + b |> f")), "(call f (+ a b))");
```

- [ ] **Step 2: Run to verify they fail**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test pipe_agrees`
Expected: FAIL — pipe still rejected by the backend stubs.

- [ ] **Step 3: Lower `Pipe` to `Call` in the parser** (`src/parser.rs`)

In `parse_binary`, where it currently builds `Expr::Binary { op, lhs, rhs, span }`, special-case `Pipe`:
```rust
let node = if matches!(op, BinaryOp::Pipe) {
    Expr::Call { callee: Box::new(rhs), args: vec![lhs], span }
} else {
    Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span }
};
```
(`Expr::Binary{op: Pipe}` is now never constructed; `BinaryOp::Pipe` remains in the precedence table
entry `parser.rs:116` and the `sexpr` map `parser.rs:1046`.)

- [ ] **Step 4: Retire the four dead backend stubs → `unreachable!`** (F4)

- `src/checker.rs:771` — `BinaryOp::Pipe => unreachable!("\`|>\` is lowered to a call in the parser"),`
- `src/compiler.rs:1105` — same.
- `src/interpreter.rs:481` — same.
- `src/transpile.rs:524` — drop `Pipe` from the `matches!(op, Is | Pipe)` guard (leave `Is`); the
  existing `unreachable!` at `:790` keeps a `Pipe` arm with the lowered-in-parser message.
Then `grep -rn "not supported\|not yet supported" src/ tests/ | grep -i pipe` and flip any test that
asserted the old "`|>` not supported" error to the working semantics.

- [ ] **Step 5: Run all pipe tests + full suite**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test pipe_ && cargo test --quiet`
Expected: PASS — `5 |> dbl |> inc` = `11`, precedence cases correct, run ≡ runvm.

- [ ] **Step 6: Quality gate + commit**

```bash
PATH=/stack/tools/cargo/bin:$PATH cargo clippy --all-targets && cargo fmt --check
git add src/parser.rs src/checker.rs src/compiler.rs src/interpreter.rs src/transpile.rs tests/differential.rs
git commit -m "feat(lang): pipe operator |> (parser-lowered to a call), retire dead stubs (M3 S3, A3/F4)"
```

---

## Task 8: Example + docs (F11/F12) + milestone status

**Files:**
- Create: `examples/guide/lambdas-pipe.phg`
- Modify: `examples/README.md` (index + coverage matrix), `FEATURES.md`, `CHANGELOG.md`,
  `KNOWN_ISSUES.md`, the `phg explain` dictionary (in `src/cli.rs`/wherever codes live),
  `docs/MILESTONES.md`/`CLAUDE.md` (mark S3 status)
- Test: the example is auto byte-identity-gated by `tests/differential.rs`’s `examples/**/*.phg` glob

**Interfaces:** none — consumes all prior tasks.

- [ ] **Step 1: Write the guide example** `examples/guide/lambdas-pipe.phg`

```
package Main;
import core.console;

function dbl(int x) -> int { return x * 2; }
function inc(int x) -> int { return x + 1; }

// higher-order: a lambda passed to a user function
function twice(int x, (int) -> int f) -> int { return f(f(x)); }

function main() {
    var add10 = fn(int x) => x + 10;          // expression-body lambda
    var scaled = fn(int x) -> int {           // statement-body lambda (captures none)
        var t = x * x;
        return t + 1;
    };
    console.println("{add10(5)}");            // 15
    console.println("{scaled(4)}");           // 17
    console.println("{twice(3, add10)}");     // 23
    console.println("{5 |> dbl |> inc}");     // inc(dbl(5)) = 11
    console.println("{3 |> fn(int v) => v + 100}"); // 103
    console.println("{twice(2, dbl)}");       // dbl(dbl(2)) = 8  (named-fn ref)
}
```

- [ ] **Step 2: Verify the example runs byte-identically on both backends + real PHP**

Run:
```bash
cd /stack/projects/phorj
PATH=/stack/tools/cargo/bin:$PATH cargo run --quiet -- run examples/guide/lambdas-pipe.phg
PATH=/stack/tools/cargo/bin:$PATH cargo run --quiet -- runvm examples/guide/lambdas-pipe.phg
PATH=/stack/tools/cargo/bin:$PATH cargo run --quiet -- transpile examples/guide/lambdas-pipe.phg | /stack/tools/phpbrew/php/php-master/bin/php
```
Expected: all three print identically:
```
15
17
23
11
103
8
```

- [ ] **Step 3: Run the differential suite (auto-gates the new example)**

Run: `PATH=/stack/tools/cargo/bin:$PATH cargo test --test differential`
Expected: PASS — the glob picks up `examples/guide/lambdas-pipe.phg` and gates run ≡ runvm.

- [ ] **Step 4: Update docs**

- `examples/README.md`: add an index row + a coverage-matrix row (lambdas + pipe).
- `FEATURES.md`: add a *Lambdas / closures* row (`✅`, `fn(x) => …`); flip the **Pipe operator `|>`**
  row from `🔲 M3` to `✅`; add a *First-class function refs* note.
- `CHANGELOG.md`: add an M3 S3 entry under `[Unreleased]`.
- `KNOWN_ISSUES.md` (F11): record the deferrals — `this`-capture, qualified/cross-package value refs,
  block-body return inference, function-type variance, `core.list` map/filter/reduce.
- `phg explain` dictionary (F12): add `E-LAMBDA-THIS` (+ any "not callable / unwrap the optional"
  refinement) with a one-line explanation; verify `phg explain E-LAMBDA-THIS` prints it.
- `docs/MILESTONES.md` / `CLAUDE.md`: mark M3 S3 (Track A) status.

- [ ] **Step 5: Full gate + commit**

```bash
PATH=/stack/tools/cargo/bin:$PATH cargo test --quiet && cargo clippy --all-targets && cargo fmt --check
git add examples/guide/lambdas-pipe.phg examples/README.md FEATURES.md CHANGELOG.md KNOWN_ISSUES.md src/ docs/MILESTONES.md CLAUDE.md
git commit -m "docs(lang): lambdas+pipe guide example + FEATURES/CHANGELOG/KNOWN_ISSUES/explain (M3 S3)"
```

- [ ] **Step 6: Task-time verification of F14 (loader interaction)**

Before closing S3, confirm a same-package bare named-fn ref inside a *library* (non-`main`) package
resolves after the M5 loader pass (write a tiny two-package fixture, or reason from the loader's
call-only rewrite). If it does **not** resolve, restrict A4 first-class refs to `package Main` this
slice and record the restriction in `KNOWN_ISSUES.md` (the guide example uses `package Main`, so it is
unaffected either way).

---

## Self-Review (writing-plans checklist)

**1. Spec coverage** — every spec §4 surface maps to a task:
- §4.1 `Ty::Function` → T2 · §4.2 `Type::Function`/`Expr::Lambda`/`LambdaBody`/`free_vars` → T2/T3 ·
  §4.3 `fn` keyword/parse_primary/pipe-lowering → T1/T3/T7 · §4.4 `check_lambda`/check_call/named-ref/
  `this`/resolve_type/expand_aliases → T2/T3 · §4.5 `Value::Closure` → T3 · §4.6 Ops/compiler/VM/CTy →
  T4 · §4.7 interpreter → T3 · §4.8 `free_vars` → T3 · §4.9 transpiler → T5/T6 · §4.10 match families →
  enforced per-task by grep+build · §5 testing (incl. F13 height) → T4 · §6 success criteria → T8 ·
  findings F1/F4/F8/F10/F11/F12/F13/F14/F15/F16/F17/F18/F-kw → mapped (see task tags).
**2. Placeholder scan** — implementation snippets that say "match the real harness/API name" are
deliberate adapters to existing-but-unverified helper names (the spec referenced line numbers, not
exact private fn names); every *behavioral* step has concrete code + a runnable command. No "TODO/TBD".
**3. Type consistency** — `Value::ClosureData::{Tree,Named,Byte}`, `Op::{MakeClosure(usize),
CallValue(usize)}`, `Function.n_captures`, `CTy::Fn{params,ret}`, `Type::Function{params,ret}`,
`free_vars(&[Param],&LambdaBody)->Vec<String>` are used identically across T2–T8.

**Note for the implementer:** several steps reference private helper names by intent (`run_ok`,
`check_errs`, `transpile_ok`, `parse_one_item`, `parse_param`, `parse_block`, `resolve_local`,
`split_off`, `push_function`, `declare_local`). Confirm each against the actual source in that file's
existing tests/methods before use — the spec cited line numbers, and the exact private identifiers
should be read from the code at implementation time (Rule 11).
