# Phorge M1 — Plan 1: Rust Scaffold + Lexer

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust project that tokenizes Phorge source into a typed token stream with source spans and clear lexing errors.

**Architecture:** A hand-written lexer (no lexer-generator) over a `&str`, producing `Vec<Token>`. Single pass, byte/char cursor with line/column tracking. String interpolation is recognized at the *string* level but the `{...}` split is **deferred to the parser** (Plan 2) — the lexer emits the raw template body. This keeps the lexer focused on lexical structure.

**Tech Stack:** Rust (stable, edition 2021), `cargo test` (built-in test harness). No external crates in Plan 1.

**Spec:** `/stack/projects/phorge/docs/specs/2026-06-15-phorge-language-design.md`

**This is Plan 1 of 5 for M1:** scaffold+lexer → parser → type-checker → evaluator → integration.

---

## File Structure

- `Cargo.toml` — crate manifest (lib + bin)
- `src/lib.rs` — crate root, re-exports `lexer`, `token`
- `src/token.rs` — `TokenKind`, `Token`, `Span` definitions
- `src/lexer.rs` — `Lexer`, `lex(src) -> Result<Vec<Token>, LexError>`
- `src/main.rs` — thin CLI: `phorge lex <file>` prints tokens (dev aid)
- `tests/lexer_integration.rs` — end-to-end: tokenize the spec's sample program

Each file has one responsibility: `token.rs` = data, `lexer.rs` = scanning logic, `main.rs` = I/O only.

---

### Task 1: Project scaffold

**Files:**
- Create: `Cargo.toml`, `src/lib.rs`, `src/main.rs`

- [ ] **Step 1: Create the cargo manifest**

`Cargo.toml`:
```toml
[package]
name = "phorge"
version = "0.0.1"
edition = "2021"

[lib]
name = "phorge"
path = "src/lib.rs"

[[bin]]
name = "phorge"
path = "src/main.rs"
```

- [ ] **Step 2: Create a minimal lib + bin so it compiles**

`src/lib.rs`:
```rust
pub mod token;
pub mod lexer;
```

`src/main.rs`:
```rust
fn main() {
    println!("phorge dev cli");
}
```

(Tasks 2–3 create `token.rs` and `lexer.rs`; until then, comment out the `pub mod` lines or create empty files. Create empty `src/token.rs` and `src/lexer.rs` now to keep it compiling.)

- [ ] **Step 3: Verify it builds**

Run: `cargo build`
Expected: compiles (empty modules are valid).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/
git commit -m "chore: rust project scaffold"
```

---

### Task 2: Token & Span types

**Files:**
- Modify: `src/token.rs`
- Test: inline `#[cfg(test)]` in `src/token.rs`

- [ ] **Step 1: Write the failing test**

In `src/token.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_records_position() {
        let t = Token { kind: TokenKind::Semicolon, span: Span { line: 3, col: 7, start: 42, len: 1 } };
        assert_eq!(t.span.line, 3);
        assert_eq!(t.span.col, 7);
        assert!(matches!(t.kind, TokenKind::Semicolon));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test span_records_position`
Expected: FAIL — `TokenKind`, `Token`, `Span` not defined.

- [ ] **Step 3: Implement the types**

At the top of `src/token.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize, // byte offset into source
    pub len: usize,
    pub line: u32,    // 1-based
    pub col: u32,     // 1-based
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // literals
    Int(i64),
    Float(f64),
    Str(String),        // processed string body (interpolation split deferred to parser)
    Ident(String),
    // keywords
    Function, Class, Enum, Constructor, Trait,
    Const, Final, Public, Private, Protected,
    Return, If, Else, For, In, Match, Import,
    This, True, False, Null, New,
    // punctuation / operators
    Dot, Semicolon, Comma, Colon, Question, Arrow, FatArrow, Pipe,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Lt, Gt, Le, Ge, EqEq, NotEq, Eq, Bang,
    Plus, Minus, Star, Slash, Percent, AndAnd, OrOr,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test span_records_position`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/token.rs
git commit -m "feat(lexer): token and span types"
```

---

### Task 3: Lexer skeleton — whitespace, newlines, EOF

**Files:**
- Modify: `src/lexer.rs`

- [ ] **Step 1: Write the failing test**

In `src/lexer.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::TokenKind;

    fn kinds(src: &str) -> Vec<TokenKind> {
        lex(src).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn empty_and_whitespace_yield_eof_only() {
        assert_eq!(kinds(""), vec![TokenKind::Eof]);
        assert_eq!(kinds("   \n\t \r\n"), vec![TokenKind::Eof]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test empty_and_whitespace_yield_eof_only`
Expected: FAIL — `lex` not defined.

- [ ] **Step 3: Implement the lexer skeleton**

At the top of `src/lexer.rs`:
```rust
use crate::token::{Span, Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer { src: src.as_bytes(), pos: 0, line: 1, col: 1 }
    }

    fn peek(&self) -> Option<u8> { self.src.get(self.pos).copied() }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        if b == b'\n' { self.line += 1; self.col = 1; } else { self.col += 1; }
        Some(b)
    }

    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' { self.bump(); } else { break; }
        }
    }
}

pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    let mut lx = Lexer::new(src);
    let mut out = Vec::new();
    loop {
        lx.skip_whitespace();
        let line = lx.line; let col = lx.col; let start = lx.pos;
        match lx.peek() {
            None => {
                out.push(Token { kind: TokenKind::Eof, span: Span { start, len: 0, line, col } });
                return Ok(out);
            }
            Some(_) => {
                // Subsequent tasks fill in real scanning here.
                return Err(LexError { message: "unexpected character".into(), line, col });
            }
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test empty_and_whitespace_yield_eof_only`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lexer.rs
git commit -m "feat(lexer): skeleton with whitespace and EOF"
```

---

### Task 4: Single-character punctuation & operators

**Files:**
- Modify: `src/lexer.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` mod in `src/lexer.rs`:
```rust
#[test]
fn single_char_tokens() {
    use TokenKind::*;
    assert_eq!(
        kinds(". ; , : ? ( ) { } [ ] < > = ! + - * / %"),
        vec![Dot, Semicolon, Comma, Colon, Question, LParen, RParen,
             LBrace, RBrace, LBracket, RBracket, Lt, Gt, Eq, Bang,
             Plus, Minus, Star, Slash, Percent, Eof]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test single_char_tokens`
Expected: FAIL — currently returns `Err` on first non-space char.

- [ ] **Step 3: Implement single-char scanning**

Replace the `Some(_)` arm in `lex` with a dispatch. Introduce a helper that pushes a token and a macro-free match:
```rust
            Some(b) => {
                let single = |k: TokenKind| Token { kind: k, span: Span { start, len: 1, line, col } };
                let kind = match b {
                    b'.' => Some(TokenKind::Dot),
                    b';' => Some(TokenKind::Semicolon),
                    b',' => Some(TokenKind::Comma),
                    b':' => Some(TokenKind::Colon),
                    b'?' => Some(TokenKind::Question),
                    b'(' => Some(TokenKind::LParen),
                    b')' => Some(TokenKind::RParen),
                    b'{' => Some(TokenKind::LBrace),
                    b'}' => Some(TokenKind::RBrace),
                    b'[' => Some(TokenKind::LBracket),
                    b']' => Some(TokenKind::RBracket),
                    b'+' => Some(TokenKind::Plus),
                    b'-' => Some(TokenKind::Minus),
                    b'*' => Some(TokenKind::Star),
                    b'/' => Some(TokenKind::Slash),
                    b'%' => Some(TokenKind::Percent),
                    b'<' => Some(TokenKind::Lt),
                    b'>' => Some(TokenKind::Gt),
                    b'=' => Some(TokenKind::Eq),
                    b'!' => Some(TokenKind::Bang),
                    _ => None,
                };
                match kind {
                    Some(k) => { lx.bump(); out.push(single(k)); }
                    None => return Err(LexError { message: format!("unexpected character {:?}", b as char), line, col }),
                }
            }
```

> NOTE: Tasks 5–8 will *intercept before this arm* (multi-char ops, numbers, idents, strings, comments). They take priority; this single-char arm is the fallback. The `<`,`>`,`=`,`!`,`/`,`-` cases here are provisional — Task 5 upgrades them to handle `<=`,`>=`,`==`,`!=`,`//`,`->`,`|>` first.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test single_char_tokens`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lexer.rs
git commit -m "feat(lexer): single-char punctuation and operators"
```

---

### Task 5: Multi-character operators

**Files:**
- Modify: `src/lexer.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn multi_char_operators() {
    use TokenKind::*;
    assert_eq!(
        kinds("== != <= >= -> => |> && ||"),
        vec![EqEq, NotEq, Le, Ge, Arrow, FatArrow, Pipe, AndAnd, OrOr, Eof]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test multi_char_operators`
Expected: FAIL — e.g. `==` lexes as two `Eq`.

- [ ] **Step 3: Implement two-char lookahead BEFORE the single-char arm**

Add a `peek2` helper to `impl Lexer`:
```rust
    fn peek2(&self) -> Option<u8> { self.src.get(self.pos + 1).copied() }
```

Insert this block at the start of the `Some(b)` arm, before the single-char match:
```rust
                // two-char operators take priority
                let two = |k: TokenKind| Token { kind: k, span: Span { start, len: 2, line, col } };
                let p2 = lx.peek2();
                let matched_two = match (b, p2) {
                    (b'=', Some(b'=')) => Some(TokenKind::EqEq),
                    (b'!', Some(b'=')) => Some(TokenKind::NotEq),
                    (b'<', Some(b'=')) => Some(TokenKind::Le),
                    (b'>', Some(b'=')) => Some(TokenKind::Ge),
                    (b'-', Some(b'>')) => Some(TokenKind::Arrow),
                    (b'=', Some(b'>')) => Some(TokenKind::FatArrow),
                    (b'|', Some(b'>')) => Some(TokenKind::Pipe),
                    (b'&', Some(b'&')) => Some(TokenKind::AndAnd),
                    (b'|', Some(b'|')) => Some(TokenKind::OrOr),
                    _ => None,
                };
                if let Some(k) = matched_two {
                    lx.bump(); lx.bump();
                    out.push(two(k));
                    continue;
                }
```

(The enclosing `loop` makes `continue` valid.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test multi_char_operators`
Expected: PASS. Also run `cargo test` — `single_char_tokens` must still pass.

- [ ] **Step 5: Commit**

```bash
git add src/lexer.rs
git commit -m "feat(lexer): multi-char operators (==, !=, ->, =>, |>, &&, ||)"
```

---

### Task 6: Integer & float literals

**Files:**
- Modify: `src/lexer.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn number_literals() {
    use TokenKind::*;
    assert_eq!(kinds("0 42 1000"), vec![Int(0), Int(42), Int(1000), Eof]);
    assert_eq!(kinds("3.14 0.5"), vec![Float(3.14), Float(0.5), Eof]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test number_literals`
Expected: FAIL — digits hit the unexpected-character error.

- [ ] **Step 3: Implement number scanning before two-char operators**

Add a method to `impl Lexer`:
```rust
    fn scan_number(&mut self, start: usize, line: u32, col: u32) -> Token {
        while matches!(self.peek(), Some(b) if b.is_ascii_digit()) { self.bump(); }
        let mut is_float = false;
        if self.peek() == Some(b'.') && matches!(self.peek2(), Some(d) if d.is_ascii_digit()) {
            is_float = true;
            self.bump(); // consume '.'
            while matches!(self.peek(), Some(b) if b.is_ascii_digit()) { self.bump(); }
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        let kind = if is_float {
            TokenKind::Float(text.parse().unwrap())
        } else {
            TokenKind::Int(text.parse().unwrap())
        };
        Token { kind, span: Span { start, len: self.pos - start, line, col } }
    }
```

Insert at the very top of the `Some(b)` arm (before two-char ops):
```rust
                if b.is_ascii_digit() {
                    let t = lx.scan_number(start, line, col);
                    out.push(t);
                    continue;
                }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test number_literals` then `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add src/lexer.rs
git commit -m "feat(lexer): integer and float literals"
```

---

### Task 7: Identifiers & keywords

**Files:**
- Modify: `src/lexer.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn identifiers_and_keywords() {
    use TokenKind::*;
    assert_eq!(
        kinds("function class enum constructor return match this true false null"),
        vec![Function, Class, Enum, Constructor, Return, Match, This, True, False, Null, Eof]
    );
    assert_eq!(kinds("age myVar User _x"),
        vec![Ident("age".into()), Ident("myVar".into()), Ident("User".into()), Ident("_x".into()), Eof]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test identifiers_and_keywords`
Expected: FAIL — letters hit unexpected-character error.

- [ ] **Step 3: Implement identifier scanning + keyword table**

Add to `impl Lexer`:
```rust
    fn scan_ident(&mut self, start: usize, line: u32, col: u32) -> Token {
        while matches!(self.peek(), Some(b) if b == b'_' || b.is_ascii_alphanumeric()) { self.bump(); }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        let kind = keyword(text).unwrap_or_else(|| TokenKind::Ident(text.to_string()));
        Token { kind, span: Span { start, len: self.pos - start, line, col } }
    }
```

Add a free function in `src/lexer.rs`:
```rust
fn keyword(s: &str) -> Option<TokenKind> {
    use TokenKind::*;
    Some(match s {
        "function" => Function, "class" => Class, "enum" => Enum,
        "constructor" => Constructor, "trait" => Trait,
        "const" => Const, "final" => Final,
        "public" => Public, "private" => Private, "protected" => Protected,
        "return" => Return, "if" => If, "else" => Else, "for" => For, "in" => In,
        "match" => Match, "import" => Import, "this" => This,
        "true" => True, "false" => False, "null" => Null, "new" => New,
        _ => return None,
    })
}
```

Insert at the top of the `Some(b)` arm (after the digit check, before two-char ops):
```rust
                if b == b'_' || b.is_ascii_alphabetic() {
                    let t = lx.scan_ident(start, line, col);
                    out.push(t);
                    continue;
                }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test identifiers_and_keywords` then `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add src/lexer.rs
git commit -m "feat(lexer): identifiers and keywords"
```

---

### Task 8: String literals (escapes; interpolation body preserved)

**Files:**
- Modify: `src/lexer.rs`

> Interpolation `{name}` is NOT split here. The lexer stores the raw body (after escape
> processing) in `TokenKind::Str`. Plan 2 (parser) splits literal/interpolation segments.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn string_literals() {
    use TokenKind::*;
    assert_eq!(kinds("\"hello\""), vec![Str("hello".into()), Eof]);
    // escapes
    assert_eq!(kinds("\"a\\nb\\t\\\"c\""), vec![Str("a\nb\t\"c".into()), Eof]);
    // interpolation body preserved verbatim (split happens in the parser)
    assert_eq!(kinds("\"Hello {name}\""), vec![Str("Hello {name}".into()), Eof]);
}

#[test]
fn unterminated_string_errors() {
    let err = lex("\"oops").unwrap_err();
    assert!(err.message.contains("unterminated string"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test string_literals`
Expected: FAIL — `"` hits unexpected-character error.

- [ ] **Step 3: Implement string scanning**

Add to `impl Lexer`:
```rust
    fn scan_string(&mut self, start: usize, line: u32, col: u32) -> Result<Token, LexError> {
        self.bump(); // opening quote
        let mut value = String::new();
        loop {
            match self.bump() {
                None => return Err(LexError { message: "unterminated string".into(), line, col }),
                Some(b'"') => break,
                Some(b'\\') => {
                    match self.bump() {
                        Some(b'n') => value.push('\n'),
                        Some(b't') => value.push('\t'),
                        Some(b'r') => value.push('\r'),
                        Some(b'\\') => value.push('\\'),
                        Some(b'"') => value.push('"'),
                        Some(other) => return Err(LexError {
                            message: format!("invalid escape \\{}", other as char), line: self.line, col: self.col }),
                        None => return Err(LexError { message: "unterminated string".into(), line, col }),
                    }
                }
                Some(other) => value.push(other as char),
            }
        }
        Ok(Token { kind: TokenKind::Str(value), span: Span { start, len: self.pos - start, line, col } })
    }
```

Insert at the top of the `Some(b)` arm (before digit/ident checks):
```rust
                if b == b'"' {
                    let t = lx.scan_string(start, line, col)?;
                    out.push(t);
                    continue;
                }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test string_literals` and `cargo test unterminated_string_errors`, then `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add src/lexer.rs
git commit -m "feat(lexer): string literals with escapes"
```

---

### Task 9: Comments

**Files:**
- Modify: `src/lexer.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn comments_are_skipped() {
    use TokenKind::*;
    assert_eq!(kinds("1 // line comment\n2"), vec![Int(1), Int(2), Eof]);
    assert_eq!(kinds("1 /* block\ncomment */ 2"), vec![Int(1), Int(2), Eof]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test comments_are_skipped`
Expected: FAIL — `/` lexes as `Slash`.

- [ ] **Step 3: Implement comment skipping**

Add to `impl Lexer`:
```rust
    fn skip_line_comment(&mut self) {
        while let Some(b) = self.peek() { if b == b'\n' { break; } self.bump(); }
    }

    fn skip_block_comment(&mut self) -> Result<(), LexError> {
        let (sl, sc) = (self.line, self.col);
        self.bump(); self.bump(); // consume /*
        loop {
            match self.peek() {
                None => return Err(LexError { message: "unterminated block comment".into(), line: sl, col: sc }),
                Some(b'*') if self.peek2() == Some(b'/') => { self.bump(); self.bump(); return Ok(()); }
                _ => { self.bump(); }
            }
        }
    }
```

Insert at the top of the `Some(b)` arm (before string/digit/ident checks):
```rust
                if b == b'/' && lx.peek2() == Some(b'/') { lx.skip_line_comment(); continue; }
                if b == b'/' && lx.peek2() == Some(b'*') { lx.skip_block_comment()?; continue; }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test comments_are_skipped` then `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add src/lexer.rs
git commit -m "feat(lexer): line and block comments"
```

---

### Task 10: CLI dev aid + integration test on the sample program

**Files:**
- Modify: `src/main.rs`
- Create: `tests/lexer_integration.rs`, `examples/hello.phg`

- [ ] **Step 1: Write the integration test**

Create `examples/hello.phg` (the spec sample, trimmed):
```phorge
function area(Shape s) -> float {
    return match s {
        Circle(r)  => 3.14159 * r * r,
        Rect(w, h) => w * h,
    };
}
```

Create `tests/lexer_integration.rs`:
```rust
use phorge::lexer::lex;
use phorge::token::TokenKind;

#[test]
fn tokenizes_sample_without_error() {
    let src = std::fs::read_to_string("examples/hello.phg").unwrap();
    let toks = lex(&src).expect("sample must lex cleanly");
    // last token is always Eof
    assert!(matches!(toks.last().unwrap().kind, TokenKind::Eof));
    // sanity: contains the function keyword and the fat-arrow match syntax
    assert!(toks.iter().any(|t| t.kind == TokenKind::Function));
    assert!(toks.iter().any(|t| t.kind == TokenKind::FatArrow));
    assert!(toks.iter().any(|t| t.kind == TokenKind::Match));
}
```

- [ ] **Step 2: Run the test (acceptance smoke test)**

Run: `cargo test --test lexer_integration`
Note: unlike the TDD unit tests, this is an end-of-pipeline acceptance test — it should **PASS** immediately if Tasks 1–9 are complete. If it FAILS, that's a real gap in the lexer (a token the sample uses isn't handled) — fix the lexer before continuing. It will not compile until Step 3 wires nothing in `main` (the test only uses the library), so run it as-is first.

- [ ] **Step 3: Wire the CLI to lex a file**

`src/main.rs`:
```rust
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("lex") => {
            let path = args.get(2).unwrap_or_else(|| { eprintln!("usage: phorge lex <file>"); exit(2); });
            let src = std::fs::read_to_string(path).unwrap_or_else(|e| { eprintln!("read error: {e}"); exit(1); });
            match phorge::lexer::lex(&src) {
                Ok(toks) => for t in toks { println!("{:?} @ {}:{}", t.kind, t.span.line, t.span.col); }
                Err(e) => { eprintln!("lex error at {}:{}: {}", e.line, e.col, e.message); exit(1); }
            }
        }
        _ => { eprintln!("usage: phorge lex <file>"); exit(2); }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test` (unit + integration)
Expected: PASS (all). Also manually: `cargo run -- lex examples/hello.phg` prints the token stream.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs tests/lexer_integration.rs examples/hello.phg
git commit -m "feat(lexer): cli lex command + sample integration test"
```

---

## Acceptance Criteria (Plan 1 done when all true)

- [ ] `cargo build` and `cargo test` both pass with zero warnings (`cargo build` clean).
- [ ] Lexer tokenizes: punctuation, single + multi-char operators, int/float, identifiers, all keywords, strings (with escapes), comments.
- [ ] Lexing errors (unterminated string/comment, invalid escape, unexpected char) return `LexError` with line/col — never panic.
- [ ] `cargo run -- lex examples/hello.phg` prints a token stream ending in `Eof`.
- [ ] Every token carries an accurate `Span` (line/col/byte-offset/len).

## Deferred to later plans (explicitly NOT in Plan 1)

- String interpolation `{...}` splitting → Plan 2 (parser).
- Sized-int / `decimal` literal suffixes (e.g. `42u8`, `1.5d`) → add when the type-checker needs them (Plan 3).
- `<` / `>` disambiguation for generics vs comparison → parser concern (Plan 2).
