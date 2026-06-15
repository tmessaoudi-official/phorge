# M1 Plan 3 — Statements, Declarations & Whole-Program Parse — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **NOTE (this project):** spawned subagents deadlock on the ask-human PreToolUse gate, so this plan is executed **inline** in the parent session. Each task: write test(s) → run (fail) → implement → run (pass) → `cargo clippy` → commit. After all tasks: full code-read + throwaway panic-probe.

**Goal:** Extend the Pratt expression parser (Plan 2) into a full parser that consumes statements, top-level declarations (`import` / `function` / `enum` / `class`), and a whole program — making the spec §6 sample program parse end-to-end.

**Architecture:** Recursive descent layered on top of the existing `Parser` (cursor over `Vec<Token>`). New AST node families in `ast.rs` (`Stmt`, `Item`, decl structs). New parser methods in `parser.rs`. The one non-trivial decision is **var-decl vs expression-statement disambiguation**, solved by speculative parse + cursor rewind (the parser already owns `pos`, so backtracking is free).

**Tech Stack:** Rust (edition 2021, stable 1.96), no external crates. `cargo test` / `cargo clippy --all-targets`. `cargo` at `/stack/tools/cargo/bin/cargo`.

---

## Scope

**In scope (frozen syntax, spec §6):**
- Statements: var-decl `Type name = expr;`, `return [expr];`, `if (cond) {…} [else {…}|else if]`, `for (Type name in expr) {…}`, block `{…}`, expression-statement `expr;`.
- Declarations: `import a.b.c;`, `function name(params) [-> Ret] {…}`, `enum Name { Variant[(Type field,…)], … }`, `class Name { fields, constructor, methods }` with visibility/binding modifiers (`public`/`private`/`protected`/`const`/`final`) and constructor parameter promotion.
- `parse_program()` — top-level item loop to EOF.

**Out of scope (deliberate M1 limitations — documented, not bugs):**
- **Reassignment** (`x = 5;`): `=` is not an infix operator and there is no assignment statement; only var-decl introduces `=`. Deferred (the spec sample never reassigns). A bare `x = 5;` is a parse error in M1.
- **Field initializers** (`private int n = 0;`): fields are `[mods] Type name;` only. Deferred.
- **Constructor modifiers** (`private constructor(…)`): modifiers preceding `constructor` are consumed and dropped (constructors are implicitly public in M1).
- `while`/`break`/`continue`, generic *declaration* params (`function f<T>`), traits, value-types, operator-overload & property-accessor decls — later plans / unfrozen syntax.

---

## File Structure

- `src/ast.rs` — **modify**: add `Stmt`, `Param`, `Modifier`, `CtorParam`, `FunctionDecl`, `EnumVariant`, `EnumDecl`, `ClassMember`, `ClassDecl`, `Item`, `Program`. (Existing `Expr`/`Type`/`Pattern` unchanged.)
- `src/parser.rs` — **modify**: add statement + declaration + program parsing methods and `expect_ident` helper; extend the `use crate::ast::…` import; add inline unit tests.
- `tests/program_integration.rs` — **create**: parse the full spec §6 sample program and assert its top-level shape.

---

### Task 1: AST — statement & declaration node types

**Files:**
- Modify: `src/ast.rs` (append after the `Expr` enum, before `#[cfg(test)]`)
- Test: inline `#[cfg(test)] mod tests` in `src/ast.rs`

- [ ] **Step 1: Write the failing test** (add to the existing `tests` module in `ast.rs`)

```rust
#[test]
fn builds_var_decl_stmt() {
    let s = Stmt::VarDecl {
        ty: Type::Named { name: "int".into(), args: vec![], span: sp() },
        name: "n".into(),
        init: Expr::Int(5, sp()),
        span: sp(),
    };
    match s {
        Stmt::VarDecl { name, .. } => assert_eq!(name, "n"),
        _ => panic!("expected VarDecl"),
    }
}

#[test]
fn builds_function_item() {
    let f = FunctionDecl {
        modifiers: vec![Modifier::Private],
        name: "area".into(),
        params: vec![Param {
            ty: Type::Named { name: "Shape".into(), args: vec![], span: sp() },
            name: "s".into(),
            span: sp(),
        }],
        ret: Some(Type::Named { name: "float".into(), args: vec![], span: sp() }),
        body: vec![],
        span: sp(),
    };
    let item = Item::Function(f);
    match item {
        Item::Function(d) => {
            assert_eq!(d.name, "area");
            assert_eq!(d.params.len(), 1);
            assert!(d.ret.is_some());
        }
        _ => panic!("expected Function item"),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib ast 2>&1 | grep -E 'error|test result'`
Expected: compile errors — `Stmt`, `Item`, `FunctionDecl`, `Param`, `Modifier` not found.

- [ ] **Step 3: Implement** — append to `src/ast.rs` (after the `Expr` enum, before the test module):

```rust
/// A function/method parameter: `Type name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub ty: Type,
    pub name: String,
    pub span: Span,
}

/// Visibility / binding modifiers on class members and promoted constructor params.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Public,
    Private,
    Protected,
    Const,
    Final,
}

/// A constructor parameter, which may carry promotion modifiers
/// (`constructor(private string name)`).
#[derive(Debug, Clone, PartialEq)]
pub struct CtorParam {
    pub modifiers: Vec<Modifier>,
    pub ty: Type,
    pub name: String,
    pub span: Span,
}

/// Statements — appear inside function/method bodies.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `Type name = expr;`
    VarDecl { ty: Type, name: String, init: Expr, span: Span },
    /// `return;` or `return expr;`
    Return { value: Option<Expr>, span: Span },
    /// `if (cond) { .. } [else { .. } | else if ..]` — else-branch is a block (an
    /// `else if` chain is stored as a single-statement block wrapping a nested `If`).
    If { cond: Expr, then_block: Vec<Stmt>, else_block: Option<Vec<Stmt>>, span: Span },
    /// `for (Type name in iter) { .. }`
    For { ty: Type, name: String, iter: Expr, body: Vec<Stmt>, span: Span },
    /// `{ .. }`
    Block(Vec<Stmt>, Span),
    /// `expr;`
    Expr(Expr, Span),
}

/// A function or method declaration. `modifiers` is empty for a free (top-level) function.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub modifiers: Vec<Modifier>,
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// One variant of an enum, with optional associated data fields (`Circle(float radius)`).
#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<Param>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

/// A member of a class.
#[derive(Debug, Clone, PartialEq)]
pub enum ClassMember {
    Field { modifiers: Vec<Modifier>, ty: Type, name: String, span: Span },
    Constructor { params: Vec<CtorParam>, body: Vec<Stmt>, span: Span },
    Method(FunctionDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl {
    pub name: String,
    pub members: Vec<ClassMember>,
    pub span: Span,
}

/// A top-level item in a program.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// `import a.b.c;`
    Import { path: Vec<String>, span: Span },
    Function(FunctionDecl),
    Enum(EnumDecl),
    Class(ClassDecl),
}

/// A whole parsed program.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
    pub span: Span,
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib ast 2>&1 | grep 'test result'`
Expected: `test result: ok.` (existing + 2 new tests pass)

- [ ] **Step 5: Commit**

```bash
git add src/ast.rs
git commit -m "feat(ast): statement, declaration, and program node types"
```

---

### Task 2: Parser — blocks, `return`, expression statements, `expect_ident`

**Files:**
- Modify: `src/parser.rs` — extend the `use crate::ast::…` line to include the new types; add methods inside `impl Parser`; add inline tests.

- [ ] **Step 1: Write the failing tests** (add to `parser.rs` `tests` module; add helper `stmt`)

```rust
/// Helper: parse `src` as a single statement.
fn stmt(src: &str) -> Stmt {
    parser(src).parse_stmt().expect("parse ok")
}

#[test]
fn parses_return_stmt() {
    assert!(matches!(stmt("return;"), Stmt::Return { value: None, .. }));
    match stmt("return 1 + 2;") {
        Stmt::Return { value: Some(Expr::Binary { .. }), .. } => {}
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_expr_stmt() {
    match stmt("println(x);") {
        Stmt::Expr(Expr::Call { .. }, _) => {}
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_block_stmt() {
    match stmt("{ return; return 1; }") {
        Stmt::Block(body, _) => assert_eq!(body.len(), 2),
        other => panic!("got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib parser 2>&1 | grep -E 'error|test result'`
Expected: compile errors — `parse_stmt`, `Stmt` unresolved.

- [ ] **Step 3: Implement**

First extend the import at the top of `parser.rs`:

```rust
use crate::ast::{
    BinaryOp, ClassDecl, ClassMember, CtorParam, EnumDecl, EnumVariant, Expr, FunctionDecl,
    Item, MatchArm, Modifier, Param, Pattern, Program, Stmt, StrPart, Type, UnaryOp,
};
```

Then add these methods inside `impl Parser` (place after `parse_match`, before the closing `}` of the impl):

```rust
/// Consume an identifier token, returning its name, or error with `what`.
fn expect_ident(&mut self, what: &str) -> Result<String, ParseError> {
    match self.peek().clone() {
        TokenKind::Ident(n) => {
            self.advance();
            Ok(n)
        }
        _ => Err(self.error(what)),
    }
}

/// Parse one statement.
pub fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
    match self.peek() {
        TokenKind::Return => self.parse_return(),
        TokenKind::If => self.parse_if(),
        TokenKind::For => self.parse_for(),
        TokenKind::LBrace => {
            let sp = self.peek_span();
            let body = self.parse_block()?;
            Ok(Stmt::Block(body, sp))
        }
        _ => self.parse_var_decl_or_expr_stmt(),
    }
}

/// `{ stmt* }` — consumes both braces, returns the inner statements.
fn parse_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
    self.expect(&TokenKind::LBrace, "'{'")?;
    let mut stmts = Vec::new();
    while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
        stmts.push(self.parse_stmt()?);
    }
    self.expect(&TokenKind::RBrace, "'}' to close block")?;
    Ok(stmts)
}

/// `return;` or `return expr;`
fn parse_return(&mut self) -> Result<Stmt, ParseError> {
    let sp = self.peek_span();
    self.expect(&TokenKind::Return, "'return'")?;
    let value = if self.check(&TokenKind::Semicolon) {
        None
    } else {
        Some(self.parse_expr()?)
    };
    self.expect(&TokenKind::Semicolon, "';' after return")?;
    Ok(Stmt::Return { value, span: sp })
}

/// Disambiguate `Type name = expr;` (var-decl) from `expr;` (expression statement).
/// A var-decl is committed only after a type, a name, and `=` parse successfully;
/// anything short of that rewinds the cursor and re-parses as an expression.
fn parse_var_decl_or_expr_stmt(&mut self) -> Result<Stmt, ParseError> {
    let sp = self.peek_span();
    if let Some((ty, name)) = self.try_var_decl_header() {
        let init = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "';' after variable declaration")?;
        return Ok(Stmt::VarDecl { ty, name, init, span: sp });
    }
    let expr = self.parse_expr()?;
    self.expect(&TokenKind::Semicolon, "';' after expression statement")?;
    Ok(Stmt::Expr(expr, sp))
}

/// Speculatively parse a var-decl header `Type name =`. Restores the cursor and
/// returns `None` on any failure so the caller can fall back to expression parsing.
fn try_var_decl_header(&mut self) -> Option<(Type, String)> {
    let start = self.pos;
    if let Ok(ty) = self.parse_type() {
        if let TokenKind::Ident(name) = self.peek().clone() {
            self.advance();
            if self.eat(&TokenKind::Eq) {
                return Some((ty, name));
            }
        }
    }
    self.pos = start;
    None
}
```

> `parse_if` and `parse_for` are referenced here but implemented in Tasks 3 & 4. To keep this task compiling, add temporary stubs that the next tasks replace:
> ```rust
> fn parse_if(&mut self) -> Result<Stmt, ParseError> { Err(self.error("if (Task 3)")) }
> fn parse_for(&mut self) -> Result<Stmt, ParseError> { Err(self.error("for (Task 4)")) }
> ```
> (These stubs are replaced — not appended to — in Tasks 3 and 4.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib parser 2>&1 | grep 'test result'`
Expected: `test result: ok.`

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): statements — block, return, expr-stmt, var-decl disambiguation"
```

---

### Task 3: Parser — `if` / `else` / `else if`

**Files:**
- Modify: `src/parser.rs` (replace the `parse_if` stub; add tests)

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn parses_if_else() {
    match stmt("if (a) { return 1; } else { return 2; }") {
        Stmt::If { then_block, else_block: Some(eb), .. } => {
            assert_eq!(then_block.len(), 1);
            assert_eq!(eb.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
    // no else
    match stmt("if (a) { return 1; }") {
        Stmt::If { else_block: None, .. } => {}
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_else_if_chain() {
    // `else if` nests as a single-statement else block wrapping another If
    match stmt("if (a) { return 1; } else if (b) { return 2; }") {
        Stmt::If { else_block: Some(eb), .. } => {
            assert_eq!(eb.len(), 1);
            assert!(matches!(eb[0], Stmt::If { .. }));
        }
        other => panic!("got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib parser::tests::parses_if 2>&1 | grep -E 'error|FAILED|test result'`
Expected: the stub returns Err → tests fail (`parse ok` panic).

- [ ] **Step 3: Implement** — replace the `parse_if` stub with:

```rust
/// `if (cond) BLOCK [else BLOCK | else if …]`
fn parse_if(&mut self) -> Result<Stmt, ParseError> {
    let sp = self.peek_span();
    self.expect(&TokenKind::If, "'if'")?;
    self.expect(&TokenKind::LParen, "'(' after 'if'")?;
    let cond = self.parse_expr()?;
    self.expect(&TokenKind::RParen, "')' after if condition")?;
    let then_block = self.parse_block()?;
    let else_block = if self.eat(&TokenKind::Else) {
        if self.check(&TokenKind::If) {
            // `else if …` — store the nested if as the sole statement of the else block
            Some(vec![self.parse_if()?])
        } else {
            Some(self.parse_block()?)
        }
    } else {
        None
    };
    Ok(Stmt::If { cond, then_block, else_block, span: sp })
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib parser 2>&1 | grep 'test result'`
Expected: `test result: ok.`

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): if/else and else-if chains"
```

---

### Task 4: Parser — `for (Type name in iter)`

**Files:**
- Modify: `src/parser.rs` (replace the `parse_for` stub; add test)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parses_for_in() {
    match stmt("for (Shape s in shapes) { println(s); }") {
        Stmt::For { ty, name, iter, body, .. } => {
            assert!(matches!(ty, Type::Named { ref name, .. } if name == "Shape"));
            assert_eq!(name, "s");
            assert!(matches!(iter, Expr::Ident(ref n, _) if n == "shapes"));
            assert_eq!(body.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib parser::tests::parses_for_in 2>&1 | grep -E 'error|FAILED|test result'`
Expected: fails (stub).

- [ ] **Step 3: Implement** — replace the `parse_for` stub with:

```rust
/// `for (Type name in iter) BLOCK`
fn parse_for(&mut self) -> Result<Stmt, ParseError> {
    let sp = self.peek_span();
    self.expect(&TokenKind::For, "'for'")?;
    self.expect(&TokenKind::LParen, "'(' after 'for'")?;
    let ty = self.parse_type()?;
    let name = self.expect_ident("a loop variable name")?;
    self.expect(&TokenKind::In, "'in' in for-loop header")?;
    let iter = self.parse_expr()?;
    self.expect(&TokenKind::RParen, "')' after for-loop header")?;
    let body = self.parse_block()?;
    Ok(Stmt::For { ty, name, iter, body, span: sp })
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib parser 2>&1 | grep 'test result'`
Expected: `test result: ok.`

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): for-in loop"
```

---

### Task 5: Parser — `function` declaration + parameter lists

**Files:**
- Modify: `src/parser.rs` (add `parse_function`, `parse_params`; add tests)

- [ ] **Step 1: Write the failing tests** (add helper `func`)

```rust
/// Helper: parse `src` as a top-level item.
fn item(src: &str) -> Item {
    parser(src).parse_item().expect("parse ok")
}

#[test]
fn parses_function_decl() {
    match item("function area(Shape s) -> float { return s; }") {
        Item::Function(f) => {
            assert_eq!(f.name, "area");
            assert_eq!(f.params.len(), 1);
            assert_eq!(f.params[0].name, "s");
            assert!(f.ret.is_some());
            assert_eq!(f.body.len(), 1);
            assert!(f.modifiers.is_empty());
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_function_no_ret_no_params() {
    match item("function main() { println(1); }") {
        Item::Function(f) => {
            assert_eq!(f.name, "main");
            assert!(f.params.is_empty());
            assert!(f.ret.is_none());
            assert_eq!(f.body.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib parser::tests::parses_function 2>&1 | grep -E 'error|test result'`
Expected: compile error — `parse_item`, `parse_function` not found.

- [ ] **Step 3: Implement** — add inside `impl Parser`:

```rust
/// `function name(params) [-> RetType] BLOCK`. `modifiers` are pre-parsed by the caller
/// (empty for a free function; populated for a method).
fn parse_function(&mut self, modifiers: Vec<Modifier>, sp: Span) -> Result<FunctionDecl, ParseError> {
    self.expect(&TokenKind::Function, "'function'")?;
    let name = self.expect_ident("a function name")?;
    self.expect(&TokenKind::LParen, "'(' after function name")?;
    let params = self.parse_params()?;
    self.expect(&TokenKind::RParen, "')' to close parameters")?;
    let ret = if self.eat(&TokenKind::Arrow) {
        Some(self.parse_type()?)
    } else {
        None
    };
    let body = self.parse_block()?;
    Ok(FunctionDecl { modifiers, name, params, ret, body, span: sp })
}

/// Comma-separated `Type name` parameters up to (not including) `)`.
/// Allows zero params; allows a trailing comma.
fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
    let mut params = Vec::new();
    if self.check(&TokenKind::RParen) {
        return Ok(params);
    }
    loop {
        let sp = self.peek_span();
        let ty = self.parse_type()?;
        let name = self.expect_ident("a parameter name")?;
        params.push(Param { ty, name, span: sp });
        if !self.eat(&TokenKind::Comma) {
            break;
        }
        if self.check(&TokenKind::RParen) {
            break; // trailing comma
        }
    }
    Ok(params)
}

/// Parse one top-level item: `import` / `function` / `enum` / `class`.
pub fn parse_item(&mut self) -> Result<Item, ParseError> {
    let sp = self.peek_span();
    match self.peek() {
        TokenKind::Import => self.parse_import(sp),
        TokenKind::Function => Ok(Item::Function(self.parse_function(Vec::new(), sp)?)),
        TokenKind::Enum => Ok(Item::Enum(self.parse_enum(sp)?)),
        TokenKind::Class => Ok(Item::Class(self.parse_class(sp)?)),
        _ => Err(self.error("a top-level item (import, function, enum, or class)")),
    }
}
```

> `parse_import`, `parse_enum`, `parse_class` are referenced by `parse_item` but implemented in Tasks 6-8. Add temporary stubs to compile:
> ```rust
> fn parse_import(&mut self, _sp: Span) -> Result<Item, ParseError> { Err(self.error("import (Task 8)")) }
> fn parse_enum(&mut self, _sp: Span) -> Result<EnumDecl, ParseError> { Err(self.error("enum (Task 6)")) }
> fn parse_class(&mut self, _sp: Span) -> Result<ClassDecl, ParseError> { Err(self.error("class (Task 7)")) }
> ```
> (Replaced — not appended to — in Tasks 6, 7, 8.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib parser 2>&1 | grep 'test result'`
Expected: `test result: ok.`

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): function declarations, parameter lists, parse_item dispatch"
```

---

### Task 6: Parser — `enum` declaration with variant data

**Files:**
- Modify: `src/parser.rs` (replace the `parse_enum` stub; add test)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parses_enum_decl() {
    let src = "enum Shape { Circle(float radius), Rect(float w, float h), Unit, }";
    match item(src) {
        Item::Enum(e) => {
            assert_eq!(e.name, "Shape");
            assert_eq!(e.variants.len(), 3);
            assert_eq!(e.variants[0].name, "Circle");
            assert_eq!(e.variants[0].fields.len(), 1);
            assert_eq!(e.variants[1].fields.len(), 2);
            assert!(e.variants[2].fields.is_empty()); // bare variant
        }
        other => panic!("got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib parser::tests::parses_enum 2>&1 | grep -E 'FAILED|error|test result'`
Expected: fails (stub).

- [ ] **Step 3: Implement** — replace the `parse_enum` stub with:

```rust
/// `enum Name { Variant[(Type field, …)], … }` — assumes current token is `enum`.
fn parse_enum(&mut self, sp: Span) -> Result<EnumDecl, ParseError> {
    self.expect(&TokenKind::Enum, "'enum'")?;
    let name = self.expect_ident("an enum name")?;
    self.expect(&TokenKind::LBrace, "'{' to open enum body")?;
    let mut variants = Vec::new();
    while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
        let vsp = self.peek_span();
        let vname = self.expect_ident("a variant name")?;
        let fields = if self.eat(&TokenKind::LParen) {
            let f = self.parse_params()?;
            self.expect(&TokenKind::RParen, "')' to close variant fields")?;
            f
        } else {
            Vec::new()
        };
        variants.push(EnumVariant { name: vname, fields, span: vsp });
        if !self.eat(&TokenKind::Comma) {
            break;
        }
    }
    self.expect(&TokenKind::RBrace, "'}' to close enum")?;
    Ok(EnumDecl { name, variants, span: sp })
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib parser 2>&1 | grep 'test result'`
Expected: `test result: ok.`

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): enum declarations with variant data"
```

---

### Task 7: Parser — `class` declaration (fields, constructor, methods, modifiers)

**Files:**
- Modify: `src/parser.rs` (replace the `parse_class` stub; add `parse_modifiers`, `parse_ctor_params`, `parse_class_member`; add tests)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parses_class_decl() {
    let src = "class Greeter { \
                 private string name; \
                 constructor(private string name) {} \
                 function greet() -> string { return name; } \
               }";
    match item(src) {
        Item::Class(c) => {
            assert_eq!(c.name, "Greeter");
            assert_eq!(c.members.len(), 3);
            match &c.members[0] {
                ClassMember::Field { modifiers, name, .. } => {
                    assert_eq!(name, "name");
                    assert_eq!(modifiers, &vec![Modifier::Private]);
                }
                other => panic!("member 0: {other:?}"),
            }
            match &c.members[1] {
                ClassMember::Constructor { params, .. } => {
                    assert_eq!(params.len(), 1);
                    assert_eq!(params[0].modifiers, vec![Modifier::Private]);
                    assert_eq!(params[0].name, "name");
                }
                other => panic!("member 1: {other:?}"),
            }
            match &c.members[2] {
                ClassMember::Method(f) => assert_eq!(f.name, "greet"),
                other => panic!("member 2: {other:?}"),
            }
        }
        other => panic!("got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib parser::tests::parses_class 2>&1 | grep -E 'FAILED|error|test result'`
Expected: fails (stub).

- [ ] **Step 3: Implement** — replace the `parse_class` stub and add the helpers:

```rust
/// `class Name { member* }` — assumes current token is `class`.
fn parse_class(&mut self, sp: Span) -> Result<ClassDecl, ParseError> {
    self.expect(&TokenKind::Class, "'class'")?;
    let name = self.expect_ident("a class name")?;
    self.expect(&TokenKind::LBrace, "'{' to open class body")?;
    let mut members = Vec::new();
    while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
        members.push(self.parse_class_member()?);
    }
    self.expect(&TokenKind::RBrace, "'}' to close class")?;
    Ok(ClassDecl { name, members, span: sp })
}

/// One class member: a field, a constructor, or a method.
/// Modifiers preceding `constructor` are consumed and dropped (M1: constructors are public).
fn parse_class_member(&mut self) -> Result<ClassMember, ParseError> {
    let sp = self.peek_span();
    let modifiers = self.parse_modifiers();
    match self.peek() {
        TokenKind::Constructor => {
            self.advance();
            self.expect(&TokenKind::LParen, "'(' after 'constructor'")?;
            let params = self.parse_ctor_params()?;
            self.expect(&TokenKind::RParen, "')' to close constructor parameters")?;
            let body = self.parse_block()?;
            Ok(ClassMember::Constructor { params, body, span: sp })
        }
        TokenKind::Function => Ok(ClassMember::Method(self.parse_function(modifiers, sp)?)),
        _ => {
            // field: [modifiers] Type name ;
            let ty = self.parse_type()?;
            let name = self.expect_ident("a field name")?;
            self.expect(&TokenKind::Semicolon, "';' after field declaration")?;
            Ok(ClassMember::Field { modifiers, ty, name, span: sp })
        }
    }
}

/// Consume any run of visibility/binding modifiers.
fn parse_modifiers(&mut self) -> Vec<Modifier> {
    let mut mods = Vec::new();
    loop {
        let m = match self.peek() {
            TokenKind::Public => Modifier::Public,
            TokenKind::Private => Modifier::Private,
            TokenKind::Protected => Modifier::Protected,
            TokenKind::Const => Modifier::Const,
            TokenKind::Final => Modifier::Final,
            _ => break,
        };
        self.advance();
        mods.push(m);
    }
    mods
}

/// Constructor parameters: like normal params, but each may carry promotion modifiers
/// (`constructor(private string name)`). Allows zero; allows a trailing comma.
fn parse_ctor_params(&mut self) -> Result<Vec<CtorParam>, ParseError> {
    let mut params = Vec::new();
    if self.check(&TokenKind::RParen) {
        return Ok(params);
    }
    loop {
        let sp = self.peek_span();
        let modifiers = self.parse_modifiers();
        let ty = self.parse_type()?;
        let name = self.expect_ident("a parameter name")?;
        params.push(CtorParam { modifiers, ty, name, span: sp });
        if !self.eat(&TokenKind::Comma) {
            break;
        }
        if self.check(&TokenKind::RParen) {
            break; // trailing comma
        }
    }
    Ok(params)
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib parser 2>&1 | grep 'test result'`
Expected: `test result: ok.`

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): class declarations — fields, constructor promotion, methods, modifiers"
```

---

### Task 8: Parser — `import` statement + `parse_program()`

**Files:**
- Modify: `src/parser.rs` (replace the `parse_import` stub; add public `parse_program`; add tests)

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn parses_import() {
    match item("import std.io;") {
        Item::Import { path, .. } => assert_eq!(path, vec!["std", "io"]),
        other => panic!("got {other:?}"),
    }
    match item("import a;") {
        Item::Import { path, .. } => assert_eq!(path, vec!["a"]),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_program_multiple_items() {
    let src = "import std.io; enum E { A, } function main() { return; }";
    let prog = parser(src).parse_program().expect("parse ok");
    assert_eq!(prog.items.len(), 3);
    assert!(matches!(prog.items[0], Item::Import { .. }));
    assert!(matches!(prog.items[1], Item::Enum(_)));
    assert!(matches!(prog.items[2], Item::Function(_)));
}

#[test]
fn empty_program_parses() {
    let prog = parser("").parse_program().expect("parse ok");
    assert!(prog.items.is_empty());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib parser::tests::parses_program 2>&1 | grep -E 'FAILED|error|test result'`
Expected: fails — `parse_program` not found / import stub errors.

- [ ] **Step 3: Implement** — replace the `parse_import` stub and add `parse_program`:

```rust
/// `import a.b.c;` — dotted module path. Assumes current token is `import`.
fn parse_import(&mut self, sp: Span) -> Result<Item, ParseError> {
    self.expect(&TokenKind::Import, "'import'")?;
    let mut path = vec![self.expect_ident("a module path segment")?];
    while self.eat(&TokenKind::Dot) {
        path.push(self.expect_ident("a module path segment after '.'")?);
    }
    self.expect(&TokenKind::Semicolon, "';' after import")?;
    Ok(Item::Import { path, span: sp })
}

/// Entry point: parse a whole program (zero or more top-level items) until EOF.
pub fn parse_program(&mut self) -> Result<Program, ParseError> {
    let sp = self.peek_span();
    let mut items = Vec::new();
    while !self.check(&TokenKind::Eof) {
        items.push(self.parse_item()?);
    }
    Ok(Program { items, span: sp })
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib parser 2>&1 | grep 'test result'`
Expected: `test result: ok.`

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): import statements and whole-program parse_program()"
```

---

### Task 9: Integration — parse the full spec §6 sample program

**Files:**
- Create: `tests/program_integration.rs`

- [ ] **Step 1: Write the test**

```rust
use phorge::ast::{ClassMember, Item};
use phorge::lexer::lex;
use phorge::parser::Parser;

/// The complete sample program from the design spec (§6), verbatim.
const SAMPLE: &str = r#"
import std.io;

enum Shape {
    Circle(float radius),
    Rect(float w, float h),
}

function area(Shape s) -> float {
    return match s {
        Circle(r)  => 3.14159 * r * r,
        Rect(w, h) => w * h,
    };
}

class Greeter {
    private string name;

    constructor(private string name) {}

    function greet() -> string {
        return "Hello {name}";
    }
}

function main() {
    Greeter g = Greeter("Tak");
    println(g.greet());

    List<Shape> shapes = [Circle(2.0), Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        println("area = {area(s)}");
    }
}
"#;

#[test]
fn parses_full_sample_program() {
    let tokens = lex(SAMPLE).expect("lex ok");
    let prog = Parser::new(tokens).parse_program().expect("parse ok");

    // import, enum, function area, class Greeter, function main
    assert_eq!(prog.items.len(), 5);

    assert!(matches!(prog.items[0], Item::Import { .. }));

    match &prog.items[1] {
        Item::Enum(e) => {
            assert_eq!(e.name, "Shape");
            assert_eq!(e.variants.len(), 2);
        }
        other => panic!("item 1: {other:?}"),
    }

    match &prog.items[2] {
        Item::Function(f) => {
            assert_eq!(f.name, "area");
            assert_eq!(f.params.len(), 1);
            assert!(f.ret.is_some());
            assert_eq!(f.body.len(), 1); // a single `return match …;`
        }
        other => panic!("item 2: {other:?}"),
    }

    match &prog.items[3] {
        Item::Class(c) => {
            assert_eq!(c.name, "Greeter");
            assert_eq!(c.members.len(), 3);
            assert!(matches!(c.members[0], ClassMember::Field { .. }));
            assert!(matches!(c.members[1], ClassMember::Constructor { .. }));
            assert!(matches!(c.members[2], ClassMember::Method(_)));
        }
        other => panic!("item 3: {other:?}"),
    }

    match &prog.items[4] {
        Item::Function(f) => {
            assert_eq!(f.name, "main");
            assert!(f.ret.is_none());
            // Greeter g = …;  println(…);  List<Shape> shapes = …;  for (…) {…}
            assert_eq!(f.body.len(), 4);
        }
        other => panic!("item 4: {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it fails (then passes)**

Run: `cargo test --test program_integration 2>&1 | grep -E 'error|test result'`
Expected: passes once Tasks 1-8 are in (`test result: ok.`). If it fails, the failure pinpoints a grammar gap.

- [ ] **Step 3: Commit**

```bash
git add tests/program_integration.rs
git commit -m "test(parser): parse the full spec sample program end-to-end"
```

---

## Final Verification (inline, after all tasks)

- [ ] **Full suite:** `cargo test 2>&1 | grep 'test result'` — every line `ok`, zero failures.
- [ ] **Clippy:** `cargo clippy --all-targets 2>&1 | grep -E 'warning|error' || echo CLEAN` — expect `CLEAN`.
- [ ] **Build warnings:** `cargo build 2>&1 | grep -c warning` — expect `0`.
- [ ] **Code-read pass:** re-read every new method for span correctness, infinite-loop guards (`!check(Eof)` in all `while` loops over members/variants/stmts), and that no stub remains.
- [ ] **Panic-probe (throwaway):** write `tests/_probe.rs` feeding ~30 malformed programs (unclosed braces, missing `;`, `import ;`, `class {`, `function (`, `enum X { , }`, deeply nested generics, `for (x) {}`, etc.) to `parse_program`, assert every one returns `Err` (never panics). Run it, confirm green, then delete it (`find tests/_probe.rs -delete`) — do **not** commit.

## Completion Gate (produce before declaring done)

| Dimension | Evidence |
|---|---|
| Coverage | `cargo test` output: new inline tests (Tasks 2-8) + `program_integration` all pass |
| Docs | This plan documents the grammar + the 3 deliberate M1 limitations; spec §6 is the contract |
| Config | Update `handoff.md` (Plan 3 complete, new HEAD, public API: `parse_stmt`/`parse_item`/`parse_program`) |
| Blast radius | `grep -rn 'parse_expr\|parse_type\|parse_pattern' src/ tests/` — confirm Plan 2 entry points still compile and are unchanged |

## Self-Review (writing-plans checklist)

- **Spec coverage:** import ✓ (T8), enum+data ✓ (T6), function+ret ✓ (T5), class field/ctor-promotion/method ✓ (T7), var-decl ✓ (T2), for-in ✓ (T4), return+match-expr ✓ (T2 + Plan 2), block ✓ (T2), program ✓ (T8), full sample ✓ (T9). `if` (T3) is beyond the sample but in scope per the handoff.
- **Placeholder scan:** none — every step has full code. The only intentional stubs (T2, T5) are explicitly flagged as replaced in later tasks.
- **Type consistency:** `Vec<Stmt>` used for all blocks (then/else/for-body/fn-body); `Param` reused for fn params + enum variant fields; `CtorParam` only for constructors; `parse_function(modifiers, sp)` signature consistent across free-fn (T5) and method (T7) call sites; `FunctionDecl.modifiers` empty for free functions.
