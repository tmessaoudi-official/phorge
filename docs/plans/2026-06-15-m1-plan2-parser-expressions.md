# Phorge M1 — Plan 2: Parser Core (AST + Expressions + Types + Patterns)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the AST data model and a recursive-descent + Pratt expression parser that turns the lexer's `Vec<Token>` into typed expression, type, and pattern trees — including `match` and string interpolation.

**Architecture:** A hand-written recursive-descent parser with Pratt (precedence-climbing) expression parsing over the `Vec<Token>` produced by `phorge::lexer::lex`. The AST lives in its own module (`ast.rs`, pure data); the parser (`parser.rs`) owns the token cursor and all grammar productions. Generics are parsed only in **type position** (`List<Shape>`), which removes `<`/`>` ambiguity. String interpolation (`"Hello {name}"`) is split here — the lexer preserved the raw body — by re-lexing and re-parsing each `{...}` segment as a sub-expression.

**Tech Stack:** Rust (stable, edition 2021), `cargo test`. No external crates.

**Spec:** `/stack/projects/phorge/docs/specs/2026-06-15-phorge-language-design.md`

**Depends on:** Plan 1 (lexer) — complete. Consumes `lex() -> Result<Vec<Token>, LexError>`, `Token{kind,span}`, `TokenKind`, `Span{start,len,line,col}`.

**This is Plan 2 of 6 for M1:** lexer ✓ → **parser-core (this)** → parser-decls → type-checker → evaluator → integration.

---

## Scope

**In scope (Plan 2):**
- AST types for expressions, types, patterns, and the small ops enums.
- Parser scaffold (cursor, `ParseError`, lookahead helpers).
- Primary expressions: `int`, `float`, `true`/`false`, `null`, strings (with interpolation), identifiers, `this`, parenthesized expressions, list literals `[..]`.
- Operators with correct precedence/associativity: unary `-`/`!`; binary `* / %`, `+ -`, comparison `< > <= >=`, equality `== != is`, logical `&& ||`, pipe `|>`.
- Postfix: call `f(args)`, member `obj.name`, index `obj[i]` — left-associative, chainable.
- Type annotations: named (`int`), generic-in-type-position (`List<Shape>`, `Map<string, int>`), optional (`T?`).
- Patterns for `match` arms: wildcard `_`, binding `x`, literals, variant destructure `Circle(r)` / `Rect(w, h)`.
- `match` expression with arms `pattern => expr`.

**Out of scope (later plans / needs design):**
- Statements, blocks, and declarations (`function`/`class`/`enum`/`import`) → **Plan 3**.
- `trait` and value-type/`struct` declaration syntax, operator-overload & property-accessor declarations, user-defined generic declaration params (`class Box<T>`), null-safe navigation — **syntax not frozen; needs a mini-brainstorm before any plan implements them.**
- Literal-brace escaping inside interpolation (`{{`) — documented as unsupported in M1.

---

## File Structure

- `src/ast.rs` — AST data: `Expr`, `Type`, `Pattern`, `StrPart`, `MatchArm`, `UnaryOp`, `BinaryOp`. Pure data, no logic.
- `src/parser.rs` — `Parser`, `ParseError`, cursor helpers, all grammar productions, `parse_expr` / `parse_type` / `parse_pattern` entry points.
- `src/lib.rs` — add `pub mod ast;` and `pub mod parser;`.
- `src/token.rs` — add the `Is` keyword variant (Task 1).
- `src/lexer.rs` — register the `is` keyword (Task 1).
- `tests/parser_integration.rs` — end-to-end: lex+parse the spec sample's `match` expression into an AST.

`ast.rs` = data, `parser.rs` = grammar. They change together but split by responsibility (data vs logic), matching the lexer's `token.rs`/`lexer.rs` split.

---

### Task 1: Add the `is` keyword to the lexer

The spec uses `is` for identity equality (`==` is value equality). Plan 1 omitted it, so `is` currently lexes as `Ident("is")`. Add it as a keyword token before the parser needs it.

**Files:**
- Modify: `src/token.rs`, `src/lexer.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` mod in `src/lexer.rs`:
```rust
#[test]
fn is_keyword_is_recognized() {
    use TokenKind::*;
    assert_eq!(kinds("is"), vec![Is, Eof]);
    // still an ident when part of a longer word
    assert_eq!(kinds("island"), vec![Ident("island".into()), Eof]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test is_keyword_is_recognized`
Expected: FAIL — `Is` not a variant of `TokenKind` (compile error), and `is` currently lexes to `Ident`.

- [ ] **Step 3: Add the `Is` variant and register the keyword**

In `src/token.rs`, add `Is` to the keyword group of `TokenKind` (the line with `This, True, False, Null, New,`):
```rust
    This, True, False, Null, New, Is,
```

In `src/lexer.rs`, add to the `keyword` function's match (next to the other keywords):
```rust
        "is" => Is,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test is_keyword_is_recognized` then `cargo test`
Expected: PASS (all). `cargo clippy --all-targets` still exit 0.

- [ ] **Step 5: Commit**

```bash
git add src/token.rs src/lexer.rs
git commit -m "feat(lexer): add 'is' identity keyword"
```

---

### Task 2: AST data types

**Files:**
- Create: `src/ast.rs`
- Modify: `src/lib.rs`
- Test: inline `#[cfg(test)]` in `src/ast.rs`

- [ ] **Step 1: Add the module declaration**

In `src/lib.rs`, add after `pub mod lexer;`:
```rust
pub mod ast;
pub mod parser;
```
(Create an empty `src/parser.rs` now so the crate compiles; Task 3 fills it.)

- [ ] **Step 2: Write the failing test**

Create `src/ast.rs` with the test first:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Span;

    fn sp() -> Span { Span { start: 0, len: 1, line: 1, col: 1 } }

    #[test]
    fn builds_binary_expr() {
        let e = Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Int(1, sp())),
            rhs: Box::new(Expr::Int(2, sp())),
            span: sp(),
        };
        match e {
            Expr::Binary { op, .. } => assert_eq!(op, BinaryOp::Add),
            _ => panic!("expected Binary"),
        }
    }

    #[test]
    fn builds_variant_pattern() {
        let p = Pattern::Variant { name: "Circle".into(), fields: vec![Pattern::Binding { name: "r".into(), span: sp() }], span: sp() };
        match p {
            Pattern::Variant { name, fields, .. } => { assert_eq!(name, "Circle"); assert_eq!(fields.len(), 1); }
            _ => panic!("expected Variant"),
        }
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib builds_binary_expr`
Expected: FAIL — `Expr`, `BinaryOp`, `Pattern` not defined.

- [ ] **Step 4: Implement the AST types**

At the top of `src/ast.rs`:
```rust
use crate::token::Span;

/// Type annotations (e.g. `int`, `List<Shape>`, `T?`).
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// `int`, `List<Shape>`, `Map<string, int>` — `args` empty for non-generic.
    Named { name: String, args: Vec<Type>, span: Span },
    /// `T?`
    Optional { inner: Box<Type>, span: Span },
}

/// Patterns in `match` arms.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// `_`
    Wildcard(Span),
    /// bare identifier — binds the scrutinee (catch-all)
    Binding { name: String, span: Span },
    Int(i64, Span),
    Float(f64, Span),
    Str(String, Span),
    Bool(bool, Span),
    Null(Span),
    /// `Circle(r)`, `Rect(w, h)` — destructure an enum variant
    Variant { name: String, fields: Vec<Pattern>, span: Span },
}

/// One segment of a (possibly interpolated) string literal.
#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Literal(String),
    Expr(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp { Neg, Not }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add, Sub, Mul, Div, Rem,
    Eq, NotEq, Is,
    Lt, Gt, Le, Ge,
    And, Or,
    Pipe,
}

/// Expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64, Span),
    Float(f64, Span),
    Bool(bool, Span),
    Null(Span),
    /// String literal as interpolation parts; a plain string is a single `Literal` part.
    Str(Vec<StrPart>, Span),
    Ident(String, Span),
    This(Span),
    /// `[a, b, c]`
    List(Vec<Expr>, Span),
    Unary { op: UnaryOp, expr: Box<Expr>, span: Span },
    Binary { op: BinaryOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    /// `callee(args)` — also covers `Circle(2.0)` constructor calls
    Call { callee: Box<Expr>, args: Vec<Expr>, span: Span },
    /// `object.name`
    Member { object: Box<Expr>, name: String, span: Span },
    /// `object[index]`
    Index { object: Box<Expr>, index: Box<Expr>, span: Span },
    Match { scrutinee: Box<Expr>, arms: Vec<MatchArm>, span: Span },
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib builds_binary_expr builds_variant_pattern` then `cargo test`
Expected: PASS (all).

- [ ] **Step 6: Commit**

```bash
git add src/ast.rs src/lib.rs src/parser.rs
git commit -m "feat(ast): expression, type, and pattern AST types"
```

---

### Task 3: Parser scaffold — cursor, ParseError, helpers

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Write the failing test**

In `src/parser.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    /// Helper: lex `src` and build a parser over the tokens.
    fn parser(src: &str) -> Parser {
        Parser::new(lex(src).expect("lex ok"))
    }

    #[test]
    fn peek_and_advance_walk_tokens() {
        use crate::token::TokenKind::*;
        let mut p = parser("+ -");
        assert_eq!(*p.peek(), Plus);
        assert_eq!(p.advance().kind, Plus);
        assert_eq!(*p.peek(), Minus);
        assert_eq!(p.advance().kind, Minus);
        assert_eq!(*p.peek(), Eof);
        // advancing at EOF stays at EOF (does not panic)
        assert_eq!(p.advance().kind, Eof);
        assert_eq!(*p.peek(), Eof);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib peek_and_advance_walk_tokens`
Expected: FAIL — `Parser` not defined.

- [ ] **Step 3: Implement the scaffold**

At the top of `src/parser.rs`:
```rust
use crate::token::{Span, Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        // The lexer always terminates the stream with Eof, so `tokens` is non-empty.
        Parser { tokens, pos: 0 }
    }

    /// The kind of the current token. At/after the end, this is `Eof`.
    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos.min(self.tokens.len() - 1)].kind
    }

    /// Span of the current token (or the final Eof's span at the end).
    fn peek_span(&self) -> Span {
        self.tokens[self.pos.min(self.tokens.len() - 1)].span
    }

    /// Consume and return the current token; clamps at the final Eof.
    fn advance(&mut self) -> Token {
        let i = self.pos.min(self.tokens.len() - 1);
        let tok = self.tokens[i].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    /// Is the current token the given kind? Compares by variant, ignoring payload.
    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(kind)
    }

    /// If the current token matches `kind`, consume it and return true.
    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Consume a token of the expected kind or produce a ParseError.
    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<Token, ParseError> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(self.error(what))
        }
    }

    /// Build a ParseError at the current position.
    fn error(&self, what: &str) -> ParseError {
        let sp = self.peek_span();
        ParseError {
            message: format!("expected {}, found {:?}", what, self.peek()),
            line: sp.line,
            col: sp.col,
        }
    }
}
```

> NOTE on `check` with payload variants: `std::mem::discriminant` matches `Ident("x")` against any `Ident(_)`, which is what we want. For payloadless kinds (`Plus`, `LParen`, …) build the bare variant to compare against, e.g. `self.check(&TokenKind::LParen)`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib peek_and_advance_walk_tokens`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): cursor scaffold and ParseError"
```

---

### Task 4: Primary expressions — literals, ident, this, parens

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` mod in `src/parser.rs`:
```rust
use crate::ast::Expr;

/// Helper: parse `src` as a single expression.
fn expr(src: &str) -> Expr {
    parser(src).parse_expr().expect("parse ok")
}

#[test]
fn parses_literals_ident_this() {
    assert!(matches!(expr("42"), Expr::Int(42, _)));
    assert!(matches!(expr("3.5"), Expr::Float(f, _) if (f - 3.5).abs() < 1e-9));
    assert!(matches!(expr("true"), Expr::Bool(true, _)));
    assert!(matches!(expr("false"), Expr::Bool(false, _)));
    assert!(matches!(expr("null"), Expr::Null(_)));
    assert!(matches!(expr("this"), Expr::This(_)));
    match expr("foo") {
        Expr::Ident(name, _) => assert_eq!(name, "foo"),
        other => panic!("expected Ident, got {other:?}"),
    }
}

#[test]
fn parses_parenthesized() {
    // parens are grouping only — the inner expression is returned directly
    assert!(matches!(expr("(7)"), Expr::Int(7, _)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib parses_literals_ident_this`
Expected: FAIL — `parse_expr` not defined.

- [ ] **Step 3: Implement `parse_expr` (delegating to primary for now) and `parse_primary`**

Add to `impl Parser`:
```rust
    /// Entry point: parse a full expression.
    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_primary()
    }

    /// Lowest-level expression: a literal, identifier, `this`, or `( expr )`.
    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let sp = self.peek_span();
        match self.peek().clone() {
            TokenKind::Int(n) => { self.advance(); Ok(Expr::Int(n, sp)) }
            TokenKind::Float(f) => { self.advance(); Ok(Expr::Float(f, sp)) }
            TokenKind::True => { self.advance(); Ok(Expr::Bool(true, sp)) }
            TokenKind::False => { self.advance(); Ok(Expr::Bool(false, sp)) }
            TokenKind::Null => { self.advance(); Ok(Expr::Null(sp)) }
            TokenKind::This => { self.advance(); Ok(Expr::This(sp)) }
            TokenKind::Ident(name) => { self.advance(); Ok(Expr::Ident(name, sp)) }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "')'")?;
                Ok(inner)
            }
            _ => Err(self.error("an expression")),
        }
    }
```

Add the import at the top of `src/parser.rs` (below the existing `use`):
```rust
use crate::ast::Expr;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib parses_literals_ident_this parses_parenthesized` then `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): primary expressions (literals, ident, this, parens)"
```

---

### Task 5: Type annotations — named, generic, optional

Types are parsed only where the grammar expects a type (so `<` here is unambiguously generic, never comparison). Plan 3 calls `parse_type` from declarations; here we test it directly.

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Write the failing test**

```rust
use crate::ast::Type;

fn ty(src: &str) -> Type {
    parser(src).parse_type().expect("parse ok")
}

#[test]
fn parses_types() {
    match ty("int") {
        Type::Named { name, args, .. } => { assert_eq!(name, "int"); assert!(args.is_empty()); }
        other => panic!("got {other:?}"),
    }
    match ty("List<Shape>") {
        Type::Named { name, args, .. } => { assert_eq!(name, "List"); assert_eq!(args.len(), 1); }
        other => panic!("got {other:?}"),
    }
    match ty("Map<string, int>") {
        Type::Named { name, args, .. } => { assert_eq!(name, "Map"); assert_eq!(args.len(), 2); }
        other => panic!("got {other:?}"),
    }
    assert!(matches!(ty("int?"), Type::Optional { .. }));
    // nested generics
    match ty("List<Map<string, int>>") {
        Type::Named { name, args, .. } => { assert_eq!(name, "List"); assert_eq!(args.len(), 1); }
        other => panic!("got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib parses_types`
Expected: FAIL — `parse_type` not defined.

- [ ] **Step 3: Implement `parse_type`**

Add the import at the top of `src/parser.rs`:
```rust
use crate::ast::Type;
```

Add to `impl Parser`:
```rust
    /// Parse a type annotation: `Name`, `Name<T, U>`, or `T?`.
    pub fn parse_type(&mut self) -> Result<Type, ParseError> {
        let sp = self.peek_span();
        let name = match self.peek().clone() {
            TokenKind::Ident(n) => { self.advance(); n }
            _ => return Err(self.error("a type name")),
        };
        let mut args = Vec::new();
        if self.eat(&TokenKind::Lt) {
            // at least one type argument
            args.push(self.parse_type()?);
            while self.eat(&TokenKind::Comma) {
                args.push(self.parse_type()?);
            }
            self.expect(&TokenKind::Gt, "'>' to close type arguments")?;
        }
        let mut t = Type::Named { name, args, span: sp };
        // trailing `?` makes it optional; allow stacking just in case (`T??` -> Optional(Optional))
        while self.eat(&TokenKind::Question) {
            t = Type::Optional { inner: Box::new(t), span: sp };
        }
        Ok(t)
    }
```

> NOTE: nested generics like `List<Map<string, int>>` end with `>>`. The lexer tokenizes `>>` as two separate `Gt` tokens (there is no `>>` operator in `TokenKind`), so each `expect(Gt)` consumes exactly one — no special "split token" handling is needed. (Verified: `TokenKind` has `Gt` but no shift operator.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib parses_types` then `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): type annotations (named, generic, optional)"
```

---

### Task 6: Unary and binary operators with precedence (Pratt)

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Write the failing test**

```rust
use crate::ast::{BinaryOp, UnaryOp};

/// Render an expression to a fully-parenthesized string so precedence is visible.
fn sexpr(e: &Expr) -> String {
    match e {
        Expr::Int(n, _) => n.to_string(),
        Expr::Float(f, _) => format!("{f}"),
        Expr::Bool(b, _) => b.to_string(),
        Expr::Null(_) => "null".into(),
        Expr::Ident(s, _) => s.clone(),
        Expr::This(_) => "this".into(),
        Expr::Unary { op, expr, .. } => {
            let o = match op { UnaryOp::Neg => "-", UnaryOp::Not => "!" };
            format!("({o} {})", sexpr(expr))
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            let o = match op {
                BinaryOp::Add => "+", BinaryOp::Sub => "-", BinaryOp::Mul => "*",
                BinaryOp::Div => "/", BinaryOp::Rem => "%",
                BinaryOp::Eq => "==", BinaryOp::NotEq => "!=", BinaryOp::Is => "is",
                BinaryOp::Lt => "<", BinaryOp::Gt => ">", BinaryOp::Le => "<=", BinaryOp::Ge => ">=",
                BinaryOp::And => "&&", BinaryOp::Or => "||", BinaryOp::Pipe => "|>",
            };
            format!("({o} {} {})", sexpr(lhs), sexpr(rhs))
        }
        other => format!("{other:?}"),
    }
}

#[test]
fn precedence_and_associativity() {
    assert_eq!(sexpr(&expr("1 + 2 * 3")), "(+ 1 (* 2 3))");
    assert_eq!(sexpr(&expr("1 * 2 + 3")), "(+ (* 1 2) 3)");
    assert_eq!(sexpr(&expr("1 - 2 - 3")), "(- (- 1 2) 3)"); // left-assoc
    assert_eq!(sexpr(&expr("1 < 2 == true")), "(== (< 1 2) true)");
    assert_eq!(sexpr(&expr("a && b || c")), "(|| (&& a b) c)");
    assert_eq!(sexpr(&expr("-a + b")), "(+ (- a) b)");
    assert_eq!(sexpr(&expr("!a && b")), "(&& (! a) b)");
    assert_eq!(sexpr(&expr("x |> f")), "(|> x f)");
    // pipe is the lowest: `a + b |> f` == `(a + b) |> f`
    assert_eq!(sexpr(&expr("a + b |> f")), "(|> (+ a b) f)");
    assert_eq!(sexpr(&expr("a is b")), "(is a b)");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib precedence_and_associativity`
Expected: FAIL — `parse_expr` only handles primaries, so `1 + 2 * 3` returns just `1` (the rest is unconsumed) or errors.

- [ ] **Step 3: Implement Pratt parsing**

Replace the body of `parse_expr` and add the helpers. Change `parse_expr` to:
```rust
    /// Entry point: parse a full expression (lowest precedence).
    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_binary(0)
    }
```

Add to `impl Parser`:
```rust
    /// Left binding power for an infix operator token, plus its `BinaryOp`.
    /// Returns None if the token is not an infix operator. Higher binds tighter.
    fn infix_op(kind: &TokenKind) -> Option<(u8, BinaryOp)> {
        use TokenKind as T;
        Some(match kind {
            T::Pipe => (1, BinaryOp::Pipe),
            T::OrOr => (2, BinaryOp::Or),
            T::AndAnd => (3, BinaryOp::And),
            T::EqEq => (4, BinaryOp::Eq),
            T::NotEq => (4, BinaryOp::NotEq),
            T::Is => (4, BinaryOp::Is),
            T::Lt => (5, BinaryOp::Lt),
            T::Gt => (5, BinaryOp::Gt),
            T::Le => (5, BinaryOp::Le),
            T::Ge => (5, BinaryOp::Ge),
            T::Plus => (6, BinaryOp::Add),
            T::Minus => (6, BinaryOp::Sub),
            T::Star => (7, BinaryOp::Mul),
            T::Slash => (7, BinaryOp::Div),
            T::Percent => (7, BinaryOp::Rem),
            _ => return None,
        })
    }

    /// Precedence-climbing: parse a unary, then fold infix operators whose
    /// binding power is >= `min_bp`. All our binary operators are left-associative,
    /// so the right operand is parsed with `bp + 1`.
    fn parse_binary(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_unary()?;
        while let Some((bp, op)) = Self::infix_op(self.peek()) {
            if bp < min_bp { break; }
            let sp = self.peek_span();
            self.advance(); // consume the operator
            let rhs = self.parse_binary(bp + 1)?;
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span: sp };
        }
        Ok(lhs)
    }

    /// Prefix unary operators: `-expr`, `!expr`. Right-associative by recursion.
    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        let sp = self.peek_span();
        let op = match self.peek() {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Bang => Some(UnaryOp::Not),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let expr = self.parse_unary()?;
            Ok(Expr::Unary { op, expr: Box::new(expr), span: sp })
        } else {
            self.parse_primary()
        }
    }
```

Add the import at the top of `src/parser.rs`:
```rust
use crate::ast::{BinaryOp, UnaryOp};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib precedence_and_associativity` then `cargo test`
Expected: PASS (all). Earlier tests (`parses_literals_ident_this`, `parses_parenthesized`) still pass — primaries flow through `parse_unary`.

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): unary and precedence-climbing binary operators"
```

---

### Task 7: Postfix — call, member access, index

Postfix operators bind tighter than unary, so they attach to the primary before any prefix `-`/`!`. They chain left-to-right (`a.b(c)[d]`).

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parses_postfix_chains() {
    // member access
    match expr("a.b") {
        Expr::Member { object, name, .. } => { assert!(matches!(*object, Expr::Ident(ref s,_) if s=="a")); assert_eq!(name, "b"); }
        other => panic!("got {other:?}"),
    }
    // call with args (also covers constructor calls like Circle(2.0))
    match expr("f(1, 2)") {
        Expr::Call { callee, args, .. } => { assert!(matches!(*callee, Expr::Ident(ref s,_) if s=="f")); assert_eq!(args.len(), 2); }
        other => panic!("got {other:?}"),
    }
    match expr("Circle(2.0)") {
        Expr::Call { callee, args, .. } => { assert!(matches!(*callee, Expr::Ident(ref s,_) if s=="Circle")); assert_eq!(args.len(), 1); }
        other => panic!("got {other:?}"),
    }
    // index
    assert!(matches!(expr("a[0]"), Expr::Index { .. }));
    // empty-arg call
    match expr("g()") {
        Expr::Call { args, .. } => assert!(args.is_empty()),
        other => panic!("got {other:?}"),
    }
    // chaining: obj.method(x).field — outermost is Member "field"
    match expr("obj.method(x).field") {
        Expr::Member { name, .. } => assert_eq!(name, "field"),
        other => panic!("got {other:?}"),
    }
    // postfix binds tighter than unary: -a.b  ==  -(a.b)
    assert_eq!(sexpr(&expr("-a.b")), "(- a.b)");
}
```

(Extend `sexpr` to render `Member`, `Call`, `Index` — add these arms before the catch-all `other` arm:)
```rust
        Expr::Member { object, name, .. } => format!("{}.{}", sexpr(object), name),
        Expr::Call { callee, args, .. } => {
            let a: Vec<String> = args.iter().map(sexpr).collect();
            format!("{}({})", sexpr(callee), a.join(", "))
        }
        Expr::Index { object, index, .. } => format!("{}[{}]", sexpr(object), sexpr(index)),
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib parses_postfix_chains`
Expected: FAIL — `a.b` currently leaves `.b` unconsumed (and `sexpr`'s new arms reference fields that already exist, so it compiles but the parse is wrong).

- [ ] **Step 3: Implement postfix parsing**

Add a `parse_postfix` method and call it from `parse_unary` (replace `self.parse_primary()` at the end of `parse_unary` with `self.parse_postfix()`):
```rust
    /// Parse a primary, then apply any chain of postfix operators.
    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.parse_primary()?;
        loop {
            let sp = self.peek_span();
            match self.peek() {
                TokenKind::Dot => {
                    self.advance();
                    let name = match self.peek().clone() {
                        TokenKind::Ident(n) => { self.advance(); n }
                        _ => return Err(self.error("a field or method name after '.'")),
                    };
                    e = Expr::Member { object: Box::new(e), name, span: sp };
                }
                TokenKind::LParen => {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(&TokenKind::RParen, "')' to close arguments")?;
                    e = Expr::Call { callee: Box::new(e), args, span: sp };
                }
                TokenKind::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket, "']' to close index")?;
                    e = Expr::Index { object: Box::new(e), index: Box::new(index), span: sp };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    /// Comma-separated expressions until the closing delimiter (caller consumes the closer).
    /// Allows zero args; allows a trailing comma.
    fn parse_arg_list(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr()?);
            if !self.eat(&TokenKind::Comma) { break; }
            if self.check(&TokenKind::RParen) { break; } // trailing comma
        }
        Ok(args)
    }
```

In `parse_unary`, change the non-operator branch from `self.parse_primary()` to:
```rust
            self.parse_postfix()
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib parses_postfix_chains` then `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): postfix call, member access, and index"
```

---

### Task 8: List literals

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parses_list_literals() {
    match expr("[1, 2, 3]") {
        Expr::List(items, _) => assert_eq!(items.len(), 3),
        other => panic!("got {other:?}"),
    }
    match expr("[]") {
        Expr::List(items, _) => assert!(items.is_empty()),
        other => panic!("got {other:?}"),
    }
    // trailing comma allowed
    match expr("[1, 2,]") {
        Expr::List(items, _) => assert_eq!(items.len(), 2),
        other => panic!("got {other:?}"),
    }
    // nested + constructor-call elements (the spec sample: [Circle(2.0), Rect(3.0, 4.0)])
    match expr("[Circle(2.0), Rect(3.0, 4.0)]") {
        Expr::List(items, _) => assert_eq!(items.len(), 2),
        other => panic!("got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib parses_list_literals`
Expected: FAIL — `[` hits the `parse_primary` catch-all error.

- [ ] **Step 3: Implement list-literal parsing**

Add an arm to the `match` in `parse_primary`, before the final `_ =>` arm:
```rust
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        items.push(self.parse_expr()?);
                        if !self.eat(&TokenKind::Comma) { break; }
                        if self.check(&TokenKind::RBracket) { break; } // trailing comma
                    }
                }
                self.expect(&TokenKind::RBracket, "']' to close list literal")?;
                Ok(Expr::List(items, sp))
            }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib parses_list_literals` then `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): list literals"
```

---

### Task 9: String interpolation splitting

The lexer stored the raw body (after escape processing) in `TokenKind::Str`. Here we split it into `StrPart`s: literal runs and `{expr}` embedded expressions. Each embedded expression is re-lexed and re-parsed.

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Write the failing test**

```rust
use crate::ast::StrPart;

#[test]
fn parses_string_interpolation() {
    // plain string -> a single literal part
    match expr("\"hello\"") {
        Expr::Str(parts, _) => {
            assert_eq!(parts.len(), 1);
            assert!(matches!(&parts[0], StrPart::Literal(s) if s == "hello"));
        }
        other => panic!("got {other:?}"),
    }
    // interpolation: "Hello {name}" -> [Literal("Hello "), Expr(name)]
    match expr("\"Hello {name}\"") {
        Expr::Str(parts, _) => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[0], StrPart::Literal(s) if s == "Hello "));
            assert!(matches!(&parts[1], StrPart::Expr(b) if matches!(**b, Expr::Ident(ref n,_) if n == "name")));
        }
        other => panic!("got {other:?}"),
    }
    // embedded call expression: "area = {area(s)}"
    match expr("\"area = {area(s)}\"") {
        Expr::Str(parts, _) => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[1], StrPart::Expr(b) if matches!(**b, Expr::Call { .. })));
        }
        other => panic!("got {other:?}"),
    }
    // no parts before/after braces -> single Expr part
    match expr("\"{x}\"") {
        Expr::Str(parts, _) => { assert_eq!(parts.len(), 1); assert!(matches!(&parts[0], StrPart::Expr(_))); }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn unterminated_interpolation_errors() {
    let mut p = parser("\"Hello {name\"");
    assert!(p.parse_expr().is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib parses_string_interpolation`
Expected: FAIL — `Str` is not handled in `parse_primary`.

- [ ] **Step 3: Implement interpolation splitting**

Add an arm to `parse_primary`, before the final `_ =>` arm:
```rust
            TokenKind::Str(body) => {
                self.advance();
                let parts = self.split_interpolation(&body, sp)?;
                Ok(Expr::Str(parts, sp))
            }
```

Add the helper to `impl Parser`:
```rust
    /// Split a string body into literal runs and `{expr}` interpolations.
    /// Each interpolation is re-lexed + re-parsed as a standalone expression.
    /// M1 limitation: literal braces (`{{`) are not supported.
    fn split_interpolation(&self, body: &str, sp: Span) -> Result<Vec<StrPart>, ParseError> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut chars = body.chars();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    if !literal.is_empty() {
                        parts.push(StrPart::Literal(std::mem::take(&mut literal)));
                    }
                    // collect until the matching '}'
                    let mut inner = String::new();
                    let mut closed = false;
                    for ic in chars.by_ref() {
                        if ic == '}' { closed = true; break; }
                        inner.push(ic);
                    }
                    if !closed {
                        return Err(ParseError {
                            message: "unterminated interpolation '{' in string".into(),
                            line: sp.line, col: sp.col,
                        });
                    }
                    let sub_tokens = crate::lexer::lex(&inner).map_err(|e| ParseError {
                        message: format!("in interpolation: {}", e.message),
                        line: sp.line, col: sp.col,
                    })?;
                    let mut sub = Parser::new(sub_tokens);
                    let e = sub.parse_expr()?;
                    sub.expect(&TokenKind::Eof, "end of interpolation expression")?;
                    parts.push(StrPart::Expr(Box::new(e)));
                }
                '}' => {
                    return Err(ParseError {
                        message: "unexpected '}' in string (no matching '{')".into(),
                        line: sp.line, col: sp.col,
                    });
                }
                _ => literal.push(c),
            }
        }
        if !literal.is_empty() {
            parts.push(StrPart::Literal(literal));
        }
        // an empty string is a single empty literal part
        if parts.is_empty() {
            parts.push(StrPart::Literal(String::new()));
        }
        Ok(parts)
    }
```

Add the import at the top of `src/parser.rs`:
```rust
use crate::ast::StrPart;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib parses_string_interpolation unterminated_interpolation_errors` then `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): string interpolation splitting"
```

---

### Task 10: Patterns

Patterns appear in `match` arms (Task 11 uses them). A bare identifier is a binding (catch-all); `Name(subpatterns)` destructures a variant; literals match by value; `_` is wildcard.

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Write the failing test**

```rust
use crate::ast::Pattern;

fn pat(src: &str) -> Pattern {
    parser(src).parse_pattern().expect("parse ok")
}

#[test]
fn parses_patterns() {
    assert!(matches!(pat("_"), Pattern::Wildcard(_)));
    match pat("x") {
        Pattern::Binding { name, .. } => assert_eq!(name, "x"),
        other => panic!("got {other:?}"),
    }
    assert!(matches!(pat("42"), Pattern::Int(42, _)));
    assert!(matches!(pat("true"), Pattern::Bool(true, _)));
    assert!(matches!(pat("null"), Pattern::Null(_)));
    // variant destructure
    match pat("Circle(r)") {
        Pattern::Variant { name, fields, .. } => {
            assert_eq!(name, "Circle");
            assert_eq!(fields.len(), 1);
            assert!(matches!(&fields[0], Pattern::Binding { name, .. } if name == "r"));
        }
        other => panic!("got {other:?}"),
    }
    match pat("Rect(w, h)") {
        Pattern::Variant { name, fields, .. } => { assert_eq!(name, "Rect"); assert_eq!(fields.len(), 2); }
        other => panic!("got {other:?}"),
    }
    // nested variant patterns
    match pat("Wrap(Circle(r))") {
        Pattern::Variant { fields, .. } => assert!(matches!(&fields[0], Pattern::Variant { .. })),
        other => panic!("got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib parses_patterns`
Expected: FAIL — `parse_pattern` not defined.

- [ ] **Step 3: Implement `parse_pattern`**

Add the import at the top of `src/parser.rs`:
```rust
use crate::ast::Pattern;
```

Add to `impl Parser`:
```rust
    /// Parse a single pattern (used in `match` arms).
    pub fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let sp = self.peek_span();
        match self.peek().clone() {
            TokenKind::Int(n) => { self.advance(); Ok(Pattern::Int(n, sp)) }
            TokenKind::Float(f) => { self.advance(); Ok(Pattern::Float(f, sp)) }
            TokenKind::Str(s) => { self.advance(); Ok(Pattern::Str(s, sp)) }
            TokenKind::True => { self.advance(); Ok(Pattern::Bool(true, sp)) }
            TokenKind::False => { self.advance(); Ok(Pattern::Bool(false, sp)) }
            TokenKind::Null => { self.advance(); Ok(Pattern::Null(sp)) }
            TokenKind::Ident(name) => {
                self.advance();
                if name == "_" {
                    return Ok(Pattern::Wildcard(sp));
                }
                if self.eat(&TokenKind::LParen) {
                    let mut fields = Vec::new();
                    if !self.check(&TokenKind::RParen) {
                        loop {
                            fields.push(self.parse_pattern()?);
                            if !self.eat(&TokenKind::Comma) { break; }
                            if self.check(&TokenKind::RParen) { break; } // trailing comma
                        }
                    }
                    self.expect(&TokenKind::RParen, "')' to close variant pattern")?;
                    Ok(Pattern::Variant { name, fields, span: sp })
                } else {
                    Ok(Pattern::Binding { name, span: sp })
                }
            }
            _ => Err(self.error("a pattern")),
        }
    }
```

> NOTE: `_` lexes as `Ident("_")` (the lexer treats `_` as an identifier start), so wildcard is detected by name after consuming the identifier. A bare `Name` with no `(` is always a binding in M1; nullary variant patterns are out of scope (the spec sample's variants all carry data).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib parses_patterns` then `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): match patterns (wildcard, binding, literal, variant)"
```

---

### Task 11: `match` expression

`match` is a primary expression: `match SCRUTINEE { PATTERN => EXPR, ... }`. Arms are comma-separated; a trailing comma is allowed (the spec sample has one).

**Files:**
- Modify: `src/parser.rs`

- [ ] **Step 1: Write the failing test**

```rust
use crate::ast::MatchArm;

#[test]
fn parses_match_expression() {
    let e = expr("match s { Circle(r) => r, Rect(w, h) => w, _ => 0 }");
    match e {
        Expr::Match { scrutinee, arms, .. } => {
            assert!(matches!(*scrutinee, Expr::Ident(ref n, _) if n == "s"));
            assert_eq!(arms.len(), 3);
            assert!(matches!(arms[0].pattern, Pattern::Variant { .. }));
            assert!(matches!(arms[2].pattern, Pattern::Wildcard(_)));
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_match_with_trailing_comma_and_exprs() {
    // mirrors the spec sample body
    let e = expr("match s { Circle(r) => 3.14159 * r * r, Rect(w, h) => w * h, }");
    match e {
        Expr::Match { arms, .. } => {
            assert_eq!(arms.len(), 2);
            assert!(matches!(arms[0].body, Expr::Binary { .. }));
        }
        other => panic!("got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib parses_match_expression`
Expected: FAIL — `match` keyword hits the `parse_primary` catch-all error.

- [ ] **Step 3: Implement `match` parsing**

Add the import at the top of `src/parser.rs`:
```rust
use crate::ast::MatchArm;
```

Add an arm to `parse_primary`, before the final `_ =>` arm:
```rust
            TokenKind::Match => self.parse_match(sp),
```

Add to `impl Parser`:
```rust
    /// `match EXPR { PAT => EXPR, ... }` — assumes the current token is `match`.
    fn parse_match(&mut self, sp: Span) -> Result<Expr, ParseError> {
        self.expect(&TokenKind::Match, "'match'")?;
        let scrutinee = self.parse_expr()?;
        self.expect(&TokenKind::LBrace, "'{' to open match arms")?;
        let mut arms = Vec::new();
        while !self.check(&TokenKind::RBrace) {
            let arm_sp = self.peek_span();
            let pattern = self.parse_pattern()?;
            self.expect(&TokenKind::FatArrow, "'=>' after match pattern")?;
            let body = self.parse_expr()?;
            arms.push(MatchArm { pattern, body, span: arm_sp });
            if !self.eat(&TokenKind::Comma) { break; }
        }
        self.expect(&TokenKind::RBrace, "'}' to close match")?;
        Ok(Expr::Match { scrutinee: Box::new(scrutinee), arms, span: sp })
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib parses_match_expression parses_match_with_trailing_comma_and_exprs` then `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs
git commit -m "feat(parser): match expression with arms"
```

---

### Task 12: Integration test — parse the spec sample's match body

**Files:**
- Create: `tests/parser_integration.rs`

- [ ] **Step 1: Write the integration test (acceptance — should PASS once Tasks 1–11 are done)**

Create `tests/parser_integration.rs`:
```rust
use phorge::ast::{Expr, Pattern};
use phorge::lexer::lex;
use phorge::parser::Parser;

fn parse_expr(src: &str) -> Expr {
    let tokens = lex(src).expect("lex ok");
    let mut p = Parser::new(tokens);
    p.parse_expr().expect("parse ok")
}

#[test]
fn parses_spec_sample_match_body() {
    // The body of `area(Shape s)` from the spec's sample program.
    let src = "match s { Circle(r) => 3.14159 * r * r, Rect(w, h) => w * h, }";
    match parse_expr(src) {
        Expr::Match { scrutinee, arms, .. } => {
            assert!(matches!(*scrutinee, Expr::Ident(ref n, _) if n == "s"));
            assert_eq!(arms.len(), 2);
            // first arm: Circle(r) => 3.14159 * r * r
            match &arms[0].pattern {
                Pattern::Variant { name, fields, .. } => { assert_eq!(name, "Circle"); assert_eq!(fields.len(), 1); }
                other => panic!("arm 0 pattern: {other:?}"),
            }
            assert!(matches!(arms[0].body, Expr::Binary { .. }));
            // second arm: Rect(w, h) => w * h
            match &arms[1].pattern {
                Pattern::Variant { name, fields, .. } => { assert_eq!(name, "Rect"); assert_eq!(fields.len(), 2); }
                other => panic!("arm 1 pattern: {other:?}"),
            }
        }
        other => panic!("expected Match, got {other:?}"),
    }
}

#[test]
fn parses_interpolated_call_string() {
    // from the sample's loop body: "area = {area(s)}"
    match parse_expr("\"area = {area(s)}\"") {
        Expr::Str(parts, _) => assert_eq!(parts.len(), 2),
        other => panic!("expected Str, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test --test parser_integration`
Note: this is an acceptance test — it should **PASS** if Tasks 1–11 are complete. If it FAILS, that's a real parser gap — fix the parser, not the test.

- [ ] **Step 3: Run the full suite + lints**

Run: `cargo test` then `cargo clippy --all-targets`
Expected: all tests pass; clippy exit 0; `cargo build` zero warnings.

- [ ] **Step 4: Commit**

```bash
git add tests/parser_integration.rs
git commit -m "test(parser): integration parse of spec sample match + interpolation"
```

---

## Acceptance Criteria (Plan 2 done when all true)

- [ ] `cargo build` clean (zero warnings); `cargo test` all green; `cargo clippy --all-targets` exit 0.
- [ ] `is` is a lexer keyword (`TokenKind::Is`).
- [ ] Parser produces correct ASTs for: all literals, identifiers, `this`, parenthesized expressions, list literals, unary `-`/`!`, all binary operators at correct precedence/associativity, postfix call/member/index chains, type annotations (named/generic/optional), patterns, and `match`.
- [ ] String interpolation splits into `StrPart`s, re-parsing each `{expr}`.
- [ ] Parse errors return `ParseError` with line/col — the parser never panics on malformed input (e.g. `(1`, `[1,`, `"{x"`, `match s {`).
- [ ] The integration test parses the spec sample's `match` body and an interpolated call string.

## Deferred to later plans (explicitly NOT in Plan 2)

- Statements, blocks, declarations (`function`/`class`/`enum`/`import`), and whole-program parsing → **Plan 3**.
- `trait`/`struct` decls, operator-overload & property-accessor decl syntax, generic *declaration* params, null-safe navigation → need design before any plan.
- Literal-brace escaping in interpolation (`{{`/`}}`).
- Semantic checks (exhaustiveness, type correctness, name resolution) → type-checker (Plan 4).
