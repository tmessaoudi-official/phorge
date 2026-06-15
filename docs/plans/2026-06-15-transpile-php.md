# Phorge → PHP Transpiler Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement
> this plan task-by-task (subagent-driven deadlocks on the ask-human gate in this project —
> execute inline on `master`). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Emit runnable PHP 8.x from a type-checked Phorge program via `phorge transpile <file>`.

**Architecture:** New `src/transpile.rs` codegen module walks the untyped AST (same AST as
the evaluator), tracking global tables (funcs/classes/variants) and a per-function
local/field scope to resolve idents (`$name` vs `$this->field`) and dispatch calls exactly
as the evaluator does. `cli::cmd_transpile` gates on the type-checker then calls
`transpile::emit`. `main.rs` adds the `transpile` subcommand.

**Tech Stack:** Rust (std only), `cargo test`/`cargo clippy --all-targets`. Toolchain:
`export PATH=/stack/tools/cargo/bin:$PATH`. Spec: `docs/specs/2026-06-15-transpile-php-design.md`.

**Conventions (from project):** rtk tee swallows the `cargo test` success summary → grep
`test result:`/`running N tests` or trust exit code. `grep -c` 0-match exits 1 → guard
`|| echo 0`. Plain `rm` only. Run `cargo clippy --all-targets` (exit 0, zero warnings) and
commit after each task.

---

## File Structure

- **Create:** `src/transpile.rs` — the codegen module (`pub fn emit(&Program) -> Result<String,String>`
  + private `Transpiler` struct). Owns all PHP emission; one responsibility.
- **Modify:** `src/lib.rs` — add `pub mod transpile;`.
- **Modify:** `src/cli.rs` — add `pub fn cmd_transpile(src: &str) -> Result<String,String>`
  (reuse private `parse_checked`, then `transpile::emit`, mapping its error to `transpile error: …`).
- **Modify:** `src/main.rs` — add `transpile` to the subcommand match + USAGE string + dispatch.
- **Modify:** `tests/cli.rs` — add subprocess tests for `transpile`.
- **Create (if `php` on PATH):** round-trip assertions live in `tests/cli.rs` (guarded skip).

### `Transpiler` shape (defined once in Task 1, used throughout)

```rust
use crate::ast::*;
use std::collections::{HashMap, HashSet};

pub fn emit(program: &Program) -> Result<String, String> {
    let mut t = Transpiler::new();
    t.collect(program);
    t.emit_program(program)?;
    Ok(t.out)
}

struct Transpiler {
    funcs: HashSet<String>,
    classes: HashSet<String>,
    variants: HashSet<String>,                     // variant name -> dispatch `new`
    variant_fields: HashMap<String, Vec<String>>,  // variant -> ordered prop names (match binding)
    out: String,
    indent: usize,
    locals: Vec<HashSet<String>>,                  // per-fn scope stack of local/param names
    cur_class_fields: Option<HashSet<String>>,     // fields of class being emitted (for $this->)
}
```

---

### Task 1: Module scaffold — empty program emits the PHP prologue

**Files:**
- Create: `src/transpile.rs`
- Modify: `src/lib.rs`
- Test: `src/transpile.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test** (in `src/transpile.rs`)

```rust
#[cfg(test)]
mod tests {
    use crate::{lexer::Lexer, parser::Parser};
    use super::emit;

    fn php(src: &str) -> String {
        let toks = Lexer::new(src).tokenize().expect("lex");
        let prog = Parser::new(toks).parse_program().expect("parse");
        emit(&prog).expect("emit")
    }

    #[test]
    fn empty_program_emits_php_open_tag() {
        assert_eq!(php(""), "<?php\n");
    }
}
```
> Note: confirm the exact lexer/parser entrypoints first — `grep -n "pub fn tokenize\|pub fn parse_program\|pub fn new" src/lexer.rs src/parser.rs`. Adjust the helper to match (e.g. `Lexer::new(src).tokenize()` and `Parser::new(toks).parse_program()`).

- [ ] **Step 2: Run, verify it fails to compile** (`emit` undefined)

Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib transpile 2>&1 | grep -E "error\[|cannot find"`
Expected: compile error — `emit` not found.

- [ ] **Step 3: Implement the scaffold**

```rust
//! Phorge → PHP transpiler. Walks the untyped AST and emits runnable PHP 8.x.
use crate::ast::*;
use std::collections::{HashMap, HashSet};

pub fn emit(program: &Program) -> Result<String, String> {
    let mut t = Transpiler::new();
    t.collect(program);
    t.emit_program(program)?;
    Ok(t.out)
}

struct Transpiler {
    funcs: HashSet<String>,
    classes: HashSet<String>,
    variants: HashSet<String>,
    variant_fields: HashMap<String, Vec<String>>,
    out: String,
    indent: usize,
    locals: Vec<HashSet<String>>,
    cur_class_fields: Option<HashSet<String>>,
}

impl Transpiler {
    fn new() -> Self {
        Transpiler {
            funcs: HashSet::new(),
            classes: HashSet::new(),
            variants: HashSet::new(),
            variant_fields: HashMap::new(),
            out: String::new(),
            indent: 0,
            locals: Vec::new(),
            cur_class_fields: None,
        }
    }

    fn collect(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Function(f) => { self.funcs.insert(f.name.clone()); }
                Item::Class(c) => { self.classes.insert(c.name.clone()); }
                Item::Enum(e) => {
                    for v in &e.variants {
                        self.variants.insert(v.name.clone());
                        self.variant_fields
                            .insert(v.name.clone(), v.fields.iter().map(|p| p.name.clone()).collect());
                    }
                }
                Item::Import { .. } => {}
            }
        }
    }

    fn emit_program(&mut self, _program: &Program) -> Result<(), String> {
        self.out.push_str("<?php\n");
        Ok(())
    }

    // indentation-aware line writer used by later tasks
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent { self.out.push_str("    "); }
        self.out.push_str(s);
        self.out.push('\n');
    }
}
```

Add to `src/lib.rs`: `pub mod transpile;`

- [ ] **Step 4: Run, verify pass**

Run: `cargo test --lib transpile 2>&1 | grep -E "test result|FAILED"`
Expected: `test result: ok. 1 passed`.

- [ ] **Step 5: Clippy + commit**

```bash
cargo clippy --all-targets 2>&1 | grep -E "warning|error" || true   # expect none
git add src/transpile.rs src/lib.rs
git commit -m "feat(transpile): module scaffold + PHP prologue (Task 1)"
```

---

### Task 2: Functions, types, locals, literals, arithmetic, unary, if/for

**Files:** Modify: `src/transpile.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn free_function_with_params_and_arithmetic() {
    let out = php("function add(int a, int b) -> int { int c = a + b; return c; }");
    assert!(out.contains("function add(int $a, int $b): int {"), "{out}");
    assert!(out.contains("$c = $a + $b;"), "{out}");
    assert!(out.contains("return $c;"), "{out}");
}

#[test]
fn no_return_type_is_void() {
    let out = php("function f() { return; }");
    assert!(out.contains("function f(): void {"), "{out}");
}

#[test]
fn if_and_for_and_unary() {
    let out = php(
        "function f(int n) -> int { \
           List<int> xs = [1, 2]; int t = 0; \
           for (int x in xs) { if (x > 0) { t = -x; } else { t = !true; } } return t; }",
    );
    assert!(out.contains("foreach ($xs as $x) {"), "{out}");
    assert!(out.contains("if ($x > 0) {"), "{out}");
    assert!(out.contains("} else {"), "{out}");
    assert!(out.contains("= -$x;") && out.contains("= !true;"), "{out}");
    assert!(out.contains("[1, 2]"), "{out}");
}
```

- [ ] **Step 2: Run, verify fail** — `cargo test --lib transpile 2>&1 | grep FAILED`

- [ ] **Step 3: Implement** emission for items/stmts/exprs/types. Add to `impl Transpiler`:

```rust
fn emit_program(&mut self, program: &Program) -> Result<(), String> {
    self.out.push_str("<?php\n");
    for item in &program.items {
        match item {
            Item::Import { .. } => {}
            Item::Function(f) => self.emit_function(f, false)?,
            Item::Enum(e) => self.emit_enum(e)?,
            Item::Class(c) => self.emit_class(c)?,
        }
    }
    Ok(())
}

fn emit_type(ty: &Type) -> String {
    match ty {
        Type::Named { name, .. } => match name.as_str() {
            "int" => "int".into(),
            "float" => "float".into(),
            "bool" => "bool".into(),
            "string" => "string".into(),
            "List" | "Map" | "Set" => "array".into(),
            other => other.to_string(), // enum/class name
        },
        // Optional types are a deferred corner; the checker already rejects them, so
        // a transpiled program never reaches here. Be defensive:
        _ => "mixed".into(),
    }
}

fn ret_hint(ret: &Option<Type>) -> String {
    match ret { Some(t) => Self::emit_type(t), None => "void".into() }
}

// `_is_method` is accepted for call-site symmetry but unused (PHP uses `function` for both
// free functions and methods). Prefix with `_` to avoid an unused-param clippy warning, or
// drop the parameter and update the two call sites.
fn emit_function(&mut self, f: &FunctionDecl, _is_method: bool) -> Result<(), String> {
    let params: Vec<String> = f.params.iter()
        .map(|p| format!("{} ${}", Self::emit_type(&p.ty), p.name))
        .collect();
    self.line(&format!("function {}({}): {} {{", f.name, params.join(", "), Self::ret_hint(&f.ret)));
    self.indent += 1;
    self.push_scope();
    for p in &f.params { self.declare(&p.name); }
    for s in &f.body { self.emit_stmt(s)?; }
    self.pop_scope();
    self.indent -= 1;
    self.line("}");
    Ok(())
}

fn push_scope(&mut self) { self.locals.push(HashSet::new()); }
fn pop_scope(&mut self) { self.locals.pop(); }
fn declare(&mut self, name: &str) {
    if let Some(s) = self.locals.last_mut() { s.insert(name.to_string()); }
}
fn is_local(&self, name: &str) -> bool {
    self.locals.iter().any(|s| s.contains(name))
}

fn emit_stmt(&mut self, s: &Stmt) -> Result<(), String> {
    match s {
        Stmt::VarDecl { name, init, .. } => {
            // match-in-init is handled specially in Task 6; for now treat as expression.
            let e = self.emit_expr(init)?;
            self.declare(name);
            self.line(&format!("${name} = {e};"));
        }
        Stmt::Return { value, .. } => match value {
            Some(e) => { let s = self.emit_expr(e)?; self.line(&format!("return {s};")); }
            None => self.line("return;"),
        },
        Stmt::If { cond, then_block, else_block, .. } => {
            let c = self.emit_expr(cond)?;
            self.line(&format!("if ({c}) {{"));
            self.indent += 1; self.push_scope();
            for st in then_block { self.emit_stmt(st)?; }
            self.pop_scope(); self.indent -= 1;
            if let Some(eb) = else_block {
                self.line("} else {");
                self.indent += 1; self.push_scope();
                for st in eb { self.emit_stmt(st)?; }
                self.pop_scope(); self.indent -= 1;
            }
            self.line("}");
        }
        Stmt::For { name, iter, body, .. } => {
            let it = self.emit_expr(iter)?;
            self.line(&format!("foreach ({it} as ${name}) {{"));
            self.indent += 1; self.push_scope(); self.declare(name);
            for st in body { self.emit_stmt(st)?; }
            self.pop_scope(); self.indent -= 1;
            self.line("}");
        }
        Stmt::Block(stmts, _) => {
            self.line("{");
            self.indent += 1; self.push_scope();
            for st in stmts { self.emit_stmt(st)?; }
            self.pop_scope(); self.indent -= 1;
            self.line("}");
        }
        Stmt::Expr(e, _) => { let s = self.emit_expr(e)?; self.line(&format!("{s};")); }
    }
    Ok(())
}
```

And `emit_expr` (literals, idents, binary, unary, list — calls/strings/member added in later tasks; for now route them to a stub that the later tasks replace). Reference the AST variant names exactly — verify with `grep -n "pub enum Expr" -A40 src/ast.rs` before writing:

```rust
fn emit_expr(&mut self, e: &Expr) -> Result<String, String> {
    match e {
        Expr::Int(n, _) => Ok(n.to_string()),
        Expr::Float(x, _) => Ok(format!("{x:?}")), // 12.0 -> "12.0"
        Expr::Bool(b, _) => Ok(if *b { "true".into() } else { "false".into() }),
        Expr::Ident(name, _) => Ok(self.resolve_ident(name)),
        Expr::This(_) => Ok("$this".into()),
        Expr::Unary { op, expr, .. } => {
            let inner = self.emit_expr(expr)?;
            let sym = match op { UnaryOp::Neg => "-", UnaryOp::Not => "!" };
            Ok(format!("{sym}{inner}"))
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            // Is (type test) and Pipe are deferred corners — never emit broken PHP.
            if matches!(op, BinaryOp::Is | BinaryOp::Pipe) {
                return Err("transpile error: `is`/`|>` operators are not yet supported".into());
            }
            let l = self.emit_expr(lhs)?;
            let r = self.emit_expr(rhs)?;
            Ok(format!("{l} {} {r}", Self::binop(op)))
        }
        Expr::List(items, _) => {
            let parts: Result<Vec<_>, _> = items.iter().map(|i| self.emit_expr(i)).collect();
            Ok(format!("[{}]", parts?.join(", ")))
        }
        Expr::Null(_) => Err("transpile error: null is not yet supported".into()),
        Expr::Index { .. } => Err("transpile error: indexing is not yet supported".into()),
        // Str / Call / Member / Match are implemented in Tasks 3–6 (replace these arms):
        Expr::Str(..) | Expr::Call { .. } | Expr::Member { .. } | Expr::Match { .. } =>
            Err("transpile error: implemented in a later task".into()),
    }
}

fn binop(op: &BinaryOp) -> &'static str {
    use BinaryOp::*;
    match op {
        Add => "+", Sub => "-", Mul => "*", Div => "/", Rem => "%",
        Eq => "==", NotEq => "!=", Lt => "<", Le => "<=", Gt => ">", Ge => ">=",
        And => "&&", Or => "||",
        Is | Pipe => unreachable!("Is/Pipe handled before binop()"),
    }
}

fn resolve_ident(&self, name: &str) -> String {
    if self.is_local(name) {
        format!("${name}")
    } else if self.cur_class_fields.as_ref().map_or(false, |f| f.contains(name)) {
        format!("$this->{name}")
    } else {
        format!("${name}") // best-effort; checker guarantees it resolves
    }
}
```
> Verify exact AST names: `Expr::Int/Float/Bool/Ident/Unary/Binary/List/Str/Call/Member/Match`, `BinaryOp`/`UnaryOp` variant spellings, and `Float`'s payload type. Adjust literally to match `src/ast.rs`. The `{x:?}` float trick must yield `12.0` not `12` — if it doesn't, format explicitly (append `.0` when fractional part is zero), mirroring the evaluator's float stringify but for PHP source (PHP needs `12.0` to stay float).

- [ ] **Step 4: Run, verify pass** — `cargo test --lib transpile 2>&1 | grep -E "test result|FAILED"`

- [ ] **Step 5: Clippy + commit**

```bash
cargo clippy --all-targets 2>&1 | grep -E "warning|error" || true
git add src/transpile.rs && git commit -m "feat(transpile): functions, stmts, literals, arithmetic (Task 2)"
```

---

### Task 3: String interpolation → concatenation, `println` → `echo`

**Files:** Modify: `src/transpile.rs`

> AST (confirmed): `Expr::Str(Vec<StrPart>, Span)` where
> `enum StrPart { Literal(String), Expr(Box<Expr>) }`. A plain string is a single
> `Literal` part.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn interpolation_emits_concatenation() {
    // bare identifier + a literal
    let out = php("function greet(string name) -> string { return \"Hello {name}\"; }");
    assert!(out.contains(r#"return "Hello " . $name;"#), "{out}");
}

#[test]
fn pure_string_literal_no_concat() {
    let out = php("function f() -> string { return \"hi\"; }");
    assert!(out.contains(r#"return "hi";"#), "{out}");
}

#[test]
fn println_becomes_echo() {
    let out = php("function main() { println(\"hi\"); }");
    assert!(out.contains(r#"echo "hi" . "\n";"#), "{out}");
}
```

- [ ] **Step 2: Run, verify fail.**

- [ ] **Step 3: Implement.** Add a `Str` arm to `emit_expr` building a concatenation, and
  intercept `println` in the call arm (Task 4 finishes calls; add the `println` case here):

```rust
// in emit_expr, replace the catch-all for Str:
Expr::Str(parts, _) => self.emit_string(parts),
```

```rust
// Build a PHP concatenation from interpolation parts. Adjust `Part` to the real AST enum.
fn emit_string(&mut self, parts: &[StrPart]) -> Result<String, String> {
    if parts.is_empty() {
        return Ok("\"\"".into());
    }
    let mut chunks: Vec<String> = Vec::new();
    for p in parts {
        match p {
            StrPart::Literal(s) => chunks.push(format!("\"{}\"", php_escape(s))),
            StrPart::Expr(e) => {
                let code = self.emit_expr(e)?;
                chunks.push(format!("({code})"));
            }
        }
    }
    Ok(chunks.join(" . "))
}

fn php_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$")
}
```

For `println`: handled in Task 4's call dispatch — but write its emission now so this task's
test passes, by special-casing the callee in a minimal `Call` arm:

```rust
Expr::Call { callee, args, .. } => self.emit_call(callee, args),
```
```rust
fn emit_call(&mut self, callee: &Expr, args: &[Expr]) -> Result<String, String> {
    if let Expr::Ident(name, _) = callee {
        if name == "println" {
            let a = if args.is_empty() { "\"\"".into() } else { self.emit_expr(&args[0])? };
            return Ok(format!(r#"echo {a} . "\n""#)); // ';' added by Stmt::Expr
        }
    }
    // full dispatch (free fn / new) lands in Task 4:
    Err("transpile error: call dispatch implemented in Task 4".into())
}
```
> `StrPart::{Literal,Expr}` confirmed against `src/ast.rs`. `println` is dispatched as
> `Expr::Call { callee: Expr::Ident("println"), .. }` (mirror the evaluator's
> `builtin_println` / `eval_call` dispatch).

- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Clippy + commit** — `git commit -m "feat(transpile): interpolation→concat, println→echo (Task 3)"`

---

### Task 4: Enums → abstract base + subclasses; call dispatch → `new`

**Files:** Modify: `src/transpile.rs`

- [ ] **Step 1: Failing tests**

```rust
const SHAPE: &str = "enum Shape { Circle(float radius), Rect(float w, float h), }";

#[test]
fn enum_emits_base_and_subclasses() {
    let out = php(SHAPE);
    assert!(out.contains("abstract class Shape {}"), "{out}");
    assert!(out.contains("final class Circle extends Shape {"), "{out}");
    assert!(out.contains("public function __construct(public float $radius) {}"), "{out}");
    assert!(out.contains("final class Rect extends Shape {"), "{out}");
    assert!(out.contains("public function __construct(public float $w, public float $h) {}"), "{out}");
}

#[test]
fn variant_and_class_construction_use_new() {
    let out = php(&format!(
        "{SHAPE} function f() -> Shape {{ return Circle(2.0); }}"
    ));
    assert!(out.contains("return new Circle(2.0);"), "{out}");
}

#[test]
fn free_function_call_no_new() {
    let out = php("function inc(int n) -> int { return n + 1; } \
                   function f() -> int { return inc(1); }");
    assert!(out.contains("return inc(1);"), "{out}");
}
```

- [ ] **Step 2: Run, verify fail.**

- [ ] **Step 3: Implement `emit_enum` + finish `emit_call` dispatch.**

```rust
fn emit_enum(&mut self, e: &EnumDecl) -> Result<(), String> {
    self.line(&format!("abstract class {} {{}}", e.name));
    for v in &e.variants {
        let props: Vec<String> = v.fields.iter()
            .map(|p| format!("public {} ${}", Self::emit_type(&p.ty), p.name))
            .collect();
        self.line(&format!("final class {} extends {} {{", v.name, e.name));
        self.indent += 1;
        if props.is_empty() {
            // nullary variant: no ctor needed
        } else {
            self.line(&format!("public function __construct({}) {{}}", props.join(", ")));
        }
        self.indent -= 1;
        self.line("}");
    }
    Ok(())
}
```
```rust
// replace emit_call's Task-3 stub tail with full dispatch:
fn emit_call(&mut self, callee: &Expr, args: &[Expr]) -> Result<String, String> {
    if let Expr::Ident(name, _) = callee {
        if name == "println" { /* (Task 3 body) */ }
        let argv = self.emit_args(args)?;
        if self.variants.contains(name) || self.classes.contains(name) {
            return Ok(format!("new {name}({argv})"));
        }
        return Ok(format!("{name}({argv})")); // free function
    }
    // method call: callee is a Member (Task 5)
    if let Expr::Member { .. } = callee {
        return self.emit_member_call(callee, args);
    }
    Err("transpile error: unsupported call target".into())
}

fn emit_args(&mut self, args: &[Expr]) -> Result<String, String> {
    let parts: Result<Vec<_>, _> = args.iter().map(|a| self.emit_expr(a)).collect();
    Ok(parts?.join(", "))
}
```
> Keep the `println` special-case at the top of `emit_call` (from Task 3). Verify `EnumDecl`/
> `EnumVariant` field names (`variants`, `name`, `fields`) against `src/ast.rs`.

- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Clippy + commit** — `git commit -m "feat(transpile): enums→classes, call dispatch→new (Task 4)"`

---

### Task 5: Classes — fields, promoted ctor, methods, member access

**Files:** Modify: `src/transpile.rs`

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn class_with_promotion_and_method() {
    let out = php(
        "class Greeter { constructor(private string name) {} \
           function greet() -> string { return \"Hello {name}\"; } }",
    );
    assert!(out.contains("class Greeter {"), "{out}");
    assert!(out.contains("function __construct(private string $name) {}"), "{out}");
    assert!(out.contains("function greet(): string {"), "{out}");
    // bare field ref inside a method resolves to $this->name
    assert!(out.contains(r#"return "Hello " . $this->name;"#), "{out}");
}

#[test]
fn explicit_field_decl_emitted() {
    let out = php("class C { private int total; constructor(private int total) {} }");
    assert!(out.contains("private int $total;"), "{out}");
}

#[test]
fn member_access_and_method_call() {
    let out = php(
        "class Greeter { constructor(private string name) {} \
           function greet() -> string { return name; } } \
         function main() { Greeter g = Greeter(\"Tak\"); println(g.greet()); }",
    );
    assert!(out.contains("$g = new Greeter(\"Tak\");"), "{out}");
    assert!(out.contains("$g->greet()"), "{out}");
}
```

- [ ] **Step 2: Run, verify fail.**

- [ ] **Step 3: Implement `emit_class`, member access, member-call.**

```rust
fn emit_class(&mut self, c: &ClassDecl) -> Result<(), String> {
    // gather field names (explicit decls + promoted ctor params) for $this-> resolution
    let mut fields: HashSet<String> = HashSet::new();
    for m in &c.members {
        match m {
            ClassMember::Field { name, .. } => { fields.insert(name.clone()); }
            ClassMember::Constructor { params, .. } => {
                for p in params {
                    if p.modifiers.iter().any(|m| matches!(m,
                        Modifier::Public | Modifier::Private | Modifier::Protected)) {
                        fields.insert(p.name.clone());
                    }
                }
            }
            ClassMember::Method(_) => {}
        }
    }
    self.line(&format!("class {} {{", c.name));
    self.indent += 1;
    let prev = self.cur_class_fields.replace(fields);
    for m in &c.members {
        match m {
            ClassMember::Field { modifiers, ty, name, .. } => {
                self.line(&format!("{} {} ${name};", vis(modifiers), Self::emit_type(ty)));
            }
            ClassMember::Constructor { params, body, .. } => {
                let ps: Vec<String> = params.iter().map(|p| {
                    let v = vis(&p.modifiers);
                    if v.is_empty() { format!("{} ${}", Self::emit_type(&p.ty), p.name) }
                    else { format!("{} {} ${}", v, Self::emit_type(&p.ty), p.name) }
                }).collect();
                self.line(&format!("function __construct({}) {{", ps.join(", ")));
                self.indent += 1; self.push_scope();
                for p in params { self.declare(&p.name); }
                for s in body { self.emit_stmt(s)?; }
                self.pop_scope(); self.indent -= 1;
                self.line("}");
            }
            ClassMember::Method(f) => self.emit_function(f, true)?,
        }
    }
    self.cur_class_fields = prev;
    self.indent -= 1;
    self.line("}");
    Ok(())
}
```
```rust
// free fn outside impl, or assoc fn:
fn vis(mods: &[Modifier]) -> &'static str {
    if mods.iter().any(|m| matches!(m, Modifier::Private)) { "private" }
    else if mods.iter().any(|m| matches!(m, Modifier::Protected)) { "protected" }
    else if mods.iter().any(|m| matches!(m, Modifier::Public)) { "public" }
    else { "" }
}
```
```rust
// in emit_expr: Member access (field read) — distinguish from member-CALL (handled in emit_call)
Expr::Member { object, name, .. } => {
    let o = self.emit_expr(object)?;
    Ok(format!("{o}->{name}"))
}
```
```rust
fn emit_member_call(&mut self, callee: &Expr, args: &[Expr]) -> Result<String, String> {
    if let Expr::Member { object, name, .. } = callee {
        let o = self.emit_expr(object)?;
        let a = self.emit_args(args)?;
        return Ok(format!("{o}->{name}({a})"));
    }
    Err("transpile error: bad member call".into())
}
```
> Confirmed against `src/ast.rs`: `Expr::Member { object, name }` (field accessor is `name`,
> NOT `field`); `ClassMember::{Field,Constructor,Method}`; `ClassDecl.members`;
> `Modifier::{Public,Private,Protected,Const,Final}`. The promotion field-set logic mirrors
> the checker fix in `collect_class`. Note: `emit_expr`'s `Expr::Member` arm (field read)
> and `emit_call`'s `Expr::Member` branch (method call) both replace the placeholder arms
> added in Task 2/4.

- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Clippy + commit** — `git commit -m "feat(transpile): classes, promotion, member access (Task 5)"`

---

### Task 6: `match` → `instanceof` chain (return + var-decl-init); deferred-position error

**Files:** Modify: `src/transpile.rs`

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn match_in_return_emits_instanceof_chain() {
    let out = php(&format!(
        "{SHAPE} function area(Shape s) -> float {{ \
           return match s {{ Circle(r) => 3.14159 * r * r, Rect(w, h) => w * h, }}; }}"
    ));
    assert!(out.contains("if ($s instanceof Circle) {"), "{out}");
    assert!(out.contains("$r = $s->radius;"), "{out}");      // positional bind: r <- field 0 (radius)
    assert!(out.contains("return 3.14159 * $r * $r;"), "{out}");
    assert!(out.contains("if ($s instanceof Rect) {"), "{out}");
    assert!(out.contains("$w = $s->w;") && out.contains("$h = $s->h;"), "{out}");
    assert!(out.contains("throw new \\UnhandledMatchError();"), "{out}");
}

#[test]
fn match_in_var_decl_assigns_in_each_arm() {
    let out = php(&format!(
        "{SHAPE} function f(Shape s) -> float {{ \
           float a = match s {{ Circle(r) => r, Rect(w, h) => w, }}; return a; }}"
    ));
    assert!(out.contains("if ($s instanceof Circle) { $r = $s->radius; $a = $r; }"), "{out}");
    assert!(out.contains("if ($s instanceof Rect) {"), "{out}");
}

#[test]
fn wildcard_arm_has_no_trailing_throw() {
    let out = php(&format!(
        "{SHAPE} function area(Shape s) -> float {{ \
           return match s {{ Circle(r) => r, _ => 0.0, }}; }}"
    ));
    assert!(!out.contains("UnhandledMatchError"), "{out}");
}

#[test]
fn match_in_expression_position_errors_cleanly() {
    let prog = parse_only(&format!(
        "{SHAPE} function f(Shape s) -> float {{ float a = (match s {{ Circle(r) => r, Rect(w,h) => w, }}) + 1.0; return a; }}"
    ));
    let err = emit(&prog).unwrap_err();
    assert!(err.contains("match in this position is not yet supported"), "{err}");
}
```
> Add a `parse_only` helper alongside `php` that returns the `Program` (skip emit), for the
> error test. If the parser rejects parenthesized match, swap the trigger to another nested
> position (e.g. `println(match …)`), whichever the parser accepts.

- [ ] **Step 2: Run, verify fail.**

- [ ] **Step 3: Implement.** Match is handled at *statement* granularity. Add detection in
  `emit_stmt` for `Return`/`VarDecl` whose payload is a `Match`; and make `emit_expr`'s
  `Match` arm return the deferred-position error.

```rust
// emit_expr: any match reaching expression context is a deferred position
Expr::Match { .. } => Err("transpile error: match in this position is not yet supported".into()),
```
```rust
// in emit_stmt, BEFORE the generic Return/VarDecl handling:
Stmt::Return { value: Some(Expr::Match { scrutinee, arms, .. }), .. } => {
    self.emit_match(scrutinee, arms, MatchTarget::Return)?;
}
Stmt::VarDecl { name, init: Expr::Match { scrutinee, arms, .. }, .. } => {
    self.declare(name);
    self.emit_match(scrutinee, arms, MatchTarget::Assign(name.clone()))?;
}
```
```rust
enum MatchTarget { Return, Assign(String) }

fn emit_match(&mut self, scrutinee: &Expr, arms: &[MatchArm], target: MatchTarget)
    -> Result<(), String>
{
    let subj = self.emit_expr(scrutinee)?;
    let mut has_catch_all = false;
    for arm in arms {
        let stmt = |t: &MatchTarget, body: &str| match t {
            MatchTarget::Return => format!("return {body};"),
            MatchTarget::Assign(v) => format!("${v} = {body};"),
        };
        match &arm.pattern {
            Pattern::Variant { name: vname, fields: pats, .. } => {
                let props = self.variant_fields.get(vname).cloned().unwrap_or_default();
                self.push_scope();
                let mut binds = String::new();
                for (i, fp) in pats.iter().enumerate() {
                    // M1 transpiler supports simple binding payload patterns only.
                    let bind_name = match fp {
                        Pattern::Binding { name, .. } => name,
                        _ => return Err(
                            "transpile error: only simple variable patterns are supported in match payloads".into()),
                    };
                    let prop = &props[i]; // positional: pattern var i <- variant field i
                    binds.push_str(&format!("${bind_name} = {subj}->{prop}; "));
                    self.declare(bind_name);
                }
                let body = self.emit_expr(&arm.body)?;
                self.line(&format!("if ({subj} instanceof {vname}) {{ {binds}{} }}", stmt(&target, &body)));
                self.pop_scope();
            }
            Pattern::Wildcard(_) => {
                has_catch_all = true;
                let body = self.emit_expr(&arm.body)?;
                self.line(&format!("{{ {} }}", stmt(&target, &body)));
            }
            Pattern::Binding { name, .. } => {
                // bare identifier arm: binds the whole scrutinee, catch-all
                has_catch_all = true;
                self.push_scope(); self.declare(name);
                let body = self.emit_expr(&arm.body)?;
                self.line(&format!("{{ ${name} = {subj}; {} }}", stmt(&target, &body)));
                self.pop_scope();
            }
            _ => return Err(
                "transpile error: literal patterns in match are not yet supported".into()),
        }
    }
    if !has_catch_all {
        self.line("throw new \\UnhandledMatchError();");
    }
    Ok(())
}
```
> Confirmed against `src/ast.rs`: `Expr::Match { scrutinee, arms }`, `MatchArm { pattern, body }`,
> `Pattern::{ Wildcard(Span), Binding { name }, Int, Float, Str, Bool, Null, Variant { name, fields: Vec<Pattern> } }`.
> Payload patterns (`fields`) are themselves `Pattern`s — for M1 only `Binding` payloads are
> supported (the §6 sample uses `Circle(r)`, `Rect(w, h)`). Literal/nested payload patterns
> and top-level literal patterns → clean transpile error. The closure `stmt` avoids repeating
> the Return/Assign formatting; if the borrow checker objects to capturing `target`, inline it.

- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Clippy + commit** — `git commit -m "feat(transpile): match→instanceof chain (Task 6)"`

---

### Task 7: CLI wiring — `cmd_transpile`, `main.rs` dispatch, subprocess tests

**Files:** Modify: `src/cli.rs`, `src/main.rs`, `tests/cli.rs`

- [ ] **Step 1: Failing tests** (in `src/cli.rs` tests + `tests/cli.rs`)

In `src/cli.rs`:
```rust
#[test]
fn cmd_transpile_emits_php_for_sample() {
    let php = cmd_transpile(SAMPLE).expect("transpile"); // SAMPLE = §6 program constant
    assert!(php.starts_with("<?php\n"), "{php}");
    assert!(php.contains("abstract class Shape {}"), "{php}");
    assert!(php.contains("function __construct(private string $name) {}"), "{php}");
}

#[test]
fn cmd_transpile_rejects_ill_typed() {
    let err = cmd_transpile("function main() { int x = \"no\"; }").unwrap_err();
    assert!(err.contains("type error"), "{err}");
}
```
> Reuse the existing `SAMPLE`/sample constant if `cli.rs` already has one; else inline the
> §6 program (it's in `tests/fixtures/sample.phg`).

In `tests/cli.rs`:
```rust
#[test]
fn transpile_sample_exits_0_with_php() {
    let out = Command::new(BIN).args(["transpile", "tests/fixtures/sample.phg"])
        .output().expect("spawn");
    assert!(out.status.success(), "exit {:?}", out.status.code());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("<?php"));
}
```

- [ ] **Step 2: Run, verify fail.**

- [ ] **Step 3: Implement.** In `src/cli.rs`:
```rust
pub fn cmd_transpile(src: &str) -> Result<String, String> {
    let prog = parse_checked(src)?;                 // existing helper; gates on the checker
    crate::transpile::emit(&prog)                   // Err already "transpile error: …"
}
```
In `src/main.rs`: add `"transpile"` to the validated subcommand set, the dispatch arm
`"transpile" => cli::cmd_transpile(&src),`, and update USAGE to
`"usage: phorge <run|check|parse|lex|transpile> <file>"`.

- [ ] **Step 4: Run, verify pass** — `cargo test 2>&1 | grep -E "test result|FAILED"` (all suites).

- [ ] **Step 5: Clippy + commit** — `git commit -m "feat(transpile): cli cmd_transpile + main dispatch (Task 7)"`

---

### Task 8: Round-trip verification — emitted PHP runs and matches `phorge run`

**Files:** Modify: `tests/cli.rs`

- [ ] **Step 1: Write the round-trip test** (guarded on `php` availability)

```rust
#[test]
fn transpiled_php_runs_and_matches_interpreter() {
    // Skip cleanly if no PHP runtime is available.
    if Command::new("php").arg("--version").output().map(|o| !o.status.success()).unwrap_or(true) {
        eprintln!("skipping round-trip: php not on PATH");
        return;
    }
    let php = Command::new(BIN).args(["transpile", "tests/fixtures/sample.phg"])
        .output().expect("spawn transpile");
    assert!(php.status.success());
    // write to a temp file and run it
    let dir = std::env::temp_dir().join("phorge_rt");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sample.php");
    std::fs::write(&path, &php.stdout).unwrap();
    let run = Command::new("php").arg(&path).output().expect("spawn php");
    assert!(run.status.success(), "php stderr: {}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "Hello Tak\narea = 12.56636\narea = 12\n");
    let _ = std::fs::remove_file(&path);
}
```
> [Verify at execution time] `command -v php` — `/stack` runs PHP containers (tier 03), but
> the host shell running `cargo test` may not have `php` on PATH. If absent, the test
> self-skips (prints a notice, passes). If a PHP version mismatch surfaces (e.g. promoted
> ctor needs PHP 8.0+, `match`/enums-as-classes need 8.0+), note the minimum in the test.
> If `php` is available, this is the strongest correctness signal — it proves the emitted
> PHP is semantically equivalent to the interpreter for the §6 program.

- [ ] **Step 2: Run** — `cargo test transpiled_php 2>&1 | grep -E "test result|skipping|FAILED"`
  Expected: pass (either ran against `php`, or self-skipped).

- [ ] **Step 3: Clippy + commit** — `git commit -m "test(transpile): php round-trip vs interpreter (Task 8)"`

---

## Acceptance Criteria

- `phorge transpile tests/fixtures/sample.phg` prints valid PHP beginning `<?php`, exit 0.
- Emitted PHP (if `php` available) prints exactly `Hello Tak\narea = 12.56636\narea = 12\n`.
- Ill-typed input → `type error …`, exit 1. Unsupported feature → `transpile error: …`, exit 1.
- Full `cargo test` green; `cargo clippy --all-targets` exit 0, zero warnings.
- No panics on any input (errors flow through `Result`).

## Rollback

Each task is a separate commit. `git revert <sha>` or `git reset --hard <prev>` (tree is
clean between tasks). `src/transpile.rs` is additive; reverting it + the `lib.rs`/`cli.rs`/
`main.rs` lines restores the pre-transpiler state with no behavior change to run/check/parse/lex.
