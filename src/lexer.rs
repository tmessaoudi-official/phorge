//! Hand-written lexer: source `&str` → `Vec<Token>`. Iterative (no recursion), so unlike the
//! parser/checker it never contributes to the recursion-depth budget those stages guard. Faults
//! surface as a unified `diagnostic::Diagnostic` (`Stage::Lex`) carrying line/col.

use crate::diagnostic::{Diagnostic, Stage};
use crate::token::{Span, Token, TokenKind};

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer {
            src: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.src.get(self.pos + 1).copied()
    }

    fn peek3(&self) -> Option<u8> {
        self.src.get(self.pos + 2).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn scan_number(&mut self, start: usize, line: u32, col: u32) -> Result<Token, Diagnostic> {
        while matches!(self.peek(), Some(b) if b.is_ascii_digit()) {
            self.bump();
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') && matches!(self.peek2(), Some(d) if d.is_ascii_digit()) {
            is_float = true;
            self.bump(); // consume '.'
            while matches!(self.peek(), Some(b) if b.is_ascii_digit()) {
                self.bump();
            }
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        let kind = if is_float {
            let f: f64 = text.parse().map_err(|_| {
                Diagnostic::new(Stage::Lex, "float literal out of range", line, col)
            })?;
            if !f.is_finite() {
                return Err(Diagnostic::new(
                    Stage::Lex,
                    "float literal out of range",
                    line,
                    col,
                ));
            }
            TokenKind::Float(f)
        } else {
            let i: i64 = text.parse().map_err(|_| {
                Diagnostic::new(Stage::Lex, "integer literal out of range", line, col)
            })?;
            TokenKind::Int(i)
        };
        Ok(Token {
            kind,
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        })
    }

    fn skip_line_comment(&mut self) {
        while let Some(b) = self.peek() {
            if b == b'\n' {
                break;
            }
            self.bump();
        }
    }

    fn skip_block_comment(&mut self) -> Result<(), Diagnostic> {
        let (sl, sc) = (self.line, self.col);
        self.bump();
        self.bump(); // consume /*
        loop {
            match self.peek() {
                None => {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "unterminated block comment",
                        sl,
                        sc,
                    ))
                }
                Some(b'*') if self.peek2() == Some(b'/') => {
                    self.bump();
                    self.bump();
                    return Ok(());
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn scan_string(&mut self, start: usize, line: u32, col: u32) -> Result<Token, Diagnostic> {
        self.bump(); // opening quote
                     // Accumulate the body as raw bytes: literal bytes (including multi-byte UTF-8
                     // sequences) are copied verbatim, escapes expand to their ASCII byte. The source
                     // is already valid UTF-8, so the final from_utf8 cannot fail.
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            // Snapshot the position of this unit before consuming, so an invalid escape
            // can report the column of the offending backslash.
            let (el, ec) = (self.line, self.col);
            match self.bump() {
                None => {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "unterminated string",
                        line,
                        col,
                    ))
                }
                Some(b'"') => break,
                Some(b'\\') => match self.bump() {
                    Some(b'n') => bytes.push(b'\n'),
                    Some(b't') => bytes.push(b'\t'),
                    Some(b'r') => bytes.push(b'\r'),
                    Some(b'\\') => bytes.push(b'\\'),
                    Some(b'"') => bytes.push(b'"'),
                    Some(other) => {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            format!("invalid escape \\{}", other as char),
                            el,
                            ec,
                        ))
                    }
                    None => {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            "unterminated string",
                            line,
                            col,
                        ))
                    }
                },
                Some(other) => bytes.push(other),
            }
        }
        let value = String::from_utf8(bytes).expect("source string body is valid UTF-8");
        Ok(Token {
            kind: TokenKind::Str(value),
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        })
    }

    /// Scan an `html"…"` literal (the `html` prefix is already consumed). The body is captured
    /// exactly like [`Self::scan_string`] — same escapes (`\n \t \r \\ \"`), multi-byte UTF-8 and
    /// raw newlines copied verbatim, so an `html"…"` literal spans lines for free — and `{`/`}` are
    /// preserved verbatim: the interpolation split *and* the desugar into `Core.Html` kernel calls
    /// happen in the parser/checker, not here. The only difference from `scan_string` is the token
    /// kind, which routes the body to the html desugarer instead of the plain-string one.
    fn scan_html(&mut self, start: usize, line: u32, col: u32) -> Result<Token, Diagnostic> {
        self.bump(); // opening quote
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let (el, ec) = (self.line, self.col);
            match self.bump() {
                None => {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "unterminated html literal",
                        line,
                        col,
                    ))
                }
                Some(b'"') => break,
                Some(b'\\') => match self.bump() {
                    Some(b'n') => bytes.push(b'\n'),
                    Some(b't') => bytes.push(b'\t'),
                    Some(b'r') => bytes.push(b'\r'),
                    Some(b'\\') => bytes.push(b'\\'),
                    Some(b'"') => bytes.push(b'"'),
                    Some(other) => {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            format!("invalid escape \\{}", other as char),
                            el,
                            ec,
                        ))
                    }
                    None => {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            "unterminated html literal",
                            line,
                            col,
                        ))
                    }
                },
                Some(other) => bytes.push(other),
            }
        }
        let value = String::from_utf8(bytes).expect("source html body is valid UTF-8");
        Ok(Token {
            kind: TokenKind::Html(value),
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        })
    }

    /// Scan a `b"…"` byte-string literal (the `b` prefix is already consumed). Unlike `scan_string`
    /// there is NO interpolation — `{`/`}` are literal bytes. Escapes are `\n \t \r \\ \"` plus
    /// `\xHH` (two hex digits → one arbitrary octet), so a literal can hold non-UTF-8 Bytes.
    fn scan_bytes(&mut self, start: usize, line: u32, col: u32) -> Result<Token, Diagnostic> {
        self.bump(); // opening quote
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let (el, ec) = (self.line, self.col);
            match self.bump() {
                None => {
                    return Err(Diagnostic::new(
                        Stage::Lex,
                        "unterminated byte string",
                        line,
                        col,
                    ))
                }
                Some(b'"') => break,
                Some(b'\\') => match self.bump() {
                    Some(b'n') => bytes.push(b'\n'),
                    Some(b't') => bytes.push(b'\t'),
                    Some(b'r') => bytes.push(b'\r'),
                    Some(b'\\') => bytes.push(b'\\'),
                    Some(b'"') => bytes.push(b'"'),
                    Some(b'x') => {
                        let hi = self.hex_digit(el, ec)?;
                        let lo = self.hex_digit(el, ec)?;
                        bytes.push(hi << 4 | lo);
                    }
                    Some(other) => {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            format!("invalid escape \\{}", other as char),
                            el,
                            ec,
                        ))
                    }
                    None => {
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            "unterminated byte string",
                            line,
                            col,
                        ))
                    }
                },
                Some(other) => bytes.push(other),
            }
        }
        Ok(Token {
            kind: TokenKind::Bytes(bytes),
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        })
    }

    /// Consume one hex digit for a `\xHH` byte escape, or error at the offending position.
    fn hex_digit(&mut self, el: u32, ec: u32) -> Result<u8, Diagnostic> {
        match self.bump() {
            Some(c) if c.is_ascii_hexdigit() => Ok((c as char).to_digit(16).unwrap() as u8),
            _ => Err(Diagnostic::new(
                Stage::Lex,
                "invalid \\xHH byte escape (expected two hex digits)",
                el,
                ec,
            )),
        }
    }

    // NOTE: identifiers are ASCII-only by design for v0.1 (scan_ident uses
    // is_ascii_alphabetic / is_ascii_alphanumeric). Unicode identifiers are out of scope.
    fn scan_ident(&mut self, start: usize, line: u32, col: u32) -> Token {
        while matches!(self.peek(), Some(b) if b == b'_' || b.is_ascii_alphanumeric()) {
            self.bump();
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        let kind = keyword(text).unwrap_or_else(|| TokenKind::Ident(text.to_string()));
        Token {
            kind,
            span: Span {
                start,
                len: self.pos - start,
                line,
                col,
            },
        }
    }

    /// Decode the full UTF-8 char beginning at the current position. The source is always
    /// valid UTF-8 (it came from `&str`), so a char boundary is guaranteed at `self.pos`.
    /// Used only on the error path so diagnostics show the real char, not a mojibake byte.
    fn current_char(&self) -> char {
        std::str::from_utf8(&self.src[self.pos..])
            .ok()
            .and_then(|s| s.chars().next())
            .unwrap_or(char::REPLACEMENT_CHARACTER)
    }
}

fn keyword(s: &str) -> Option<TokenKind> {
    use TokenKind::*;
    Some(match s {
        "function" => Function,
        "fn" => Fn,
        "class" => Class,
        "enum" => Enum,
        "constructor" => Constructor,
        "trait" => Trait,
        "const" => Const,
        "final" => Final,
        "public" => Public,
        "private" => Private,
        "protected" => Protected,
        "internal" => Internal,
        "return" => Return,
        "if" => If,
        "else" => Else,
        "for" => For,
        "while" => While,
        "do" => Do,
        "break" => Break,
        "continue" => Continue,
        "in" => In,
        "match" => Match,
        "import" => Import,
        "package" => Package,
        "this" => This,
        "true" => True,
        "false" => False,
        "null" => Null,
        "new" => New,
        "instanceof" => Instanceof,
        "interface" => Interface,
        "implements" => Implements,
        "extends" => Extends,
        "var" => Var,
        "mutable" => Mutable,
        "static" => Static,
        "with" => With,
        "type" => TypeKw,
        _ => return None,
    })
}

pub fn lex(src: &str) -> Result<Vec<Token>, Diagnostic> {
    let mut lx = Lexer::new(src);
    let mut out = Vec::new();
    loop {
        lx.skip_whitespace();
        let line = lx.line;
        let col = lx.col;
        let start = lx.pos;
        match lx.peek() {
            None => {
                out.push(Token {
                    kind: TokenKind::Eof,
                    span: Span {
                        start,
                        len: 0,
                        line,
                        col,
                    },
                });
                return Ok(out);
            }
            Some(b) => {
                if b == b'/' && lx.peek2() == Some(b'/') {
                    lx.skip_line_comment();
                    continue;
                }
                if b == b'/' && lx.peek2() == Some(b'*') {
                    lx.skip_block_comment()?;
                    continue;
                }

                // `html"…"` literal — must precede the identifier scan (a bare `html` is a valid
                // identifier, and the module qualifier in `html.text(…)`). Only the exact `html"`
                // sequence triggers it: `Html.` / `htmlx` / a bare `html` are ordinary idents.
                if b == b'h' && lx.src[lx.pos..].starts_with(b"html\"") {
                    for _ in 0..4 {
                        lx.bump(); // consume the `html` prefix
                    }
                    let t = lx.scan_html(start, line, col)?;
                    out.push(t);
                    continue;
                }

                // `b"…"` byte-string literal — must precede the identifier scan (a bare `b` is a
                // valid identifier start). Only the exact `b"` digraph triggers it.
                if b == b'b' && lx.peek2() == Some(b'"') {
                    lx.bump(); // consume the `b` prefix
                    let t = lx.scan_bytes(start, line, col)?;
                    out.push(t);
                    continue;
                }

                if b == b'"' {
                    let t = lx.scan_string(start, line, col)?;
                    out.push(t);
                    continue;
                }

                if b.is_ascii_digit() {
                    let t = lx.scan_number(start, line, col)?;
                    out.push(t);
                    continue;
                }

                if b == b'_' || b.is_ascii_alphabetic() {
                    let t = lx.scan_ident(start, line, col);
                    out.push(t);
                    continue;
                }

                // Range operators: longest-match `..=` (3) and `..` (2) ahead of `.` (1). A number
                // like `0..3` already lexes `0` as `Int(0)` — `scan_number`'s float branch needs a
                // *digit* after the dot, and here the next char is another `.`.
                if b == b'.' && lx.peek2() == Some(b'.') {
                    let (kind, len) = if lx.peek3() == Some(b'=') {
                        (TokenKind::DotDotEq, 3)
                    } else {
                        (TokenKind::DotDot, 2)
                    };
                    for _ in 0..len {
                        lx.bump();
                    }
                    out.push(Token {
                        kind,
                        span: Span {
                            start,
                            len,
                            line,
                            col,
                        },
                    });
                    continue;
                }

                // `??=` (3) null-coalesce-assign — longest-match ahead of the two-char `??`,
                // mirroring the `..=`/`..` range block above (M-mut.2).
                if b == b'?' && lx.peek2() == Some(b'?') && lx.peek3() == Some(b'=') {
                    for _ in 0..3 {
                        lx.bump();
                    }
                    out.push(Token {
                        kind: TokenKind::QuestionQuestionEq,
                        span: Span {
                            start,
                            len: 3,
                            line,
                            col,
                        },
                    });
                    continue;
                }

                // two-char operators take priority
                let two = |k: TokenKind| Token {
                    kind: k,
                    span: Span {
                        start,
                        len: 2,
                        line,
                        col,
                    },
                };
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
                    (b'?', Some(b'?')) => Some(TokenKind::QuestionQuestion),
                    (b'?', Some(b'.')) => Some(TokenKind::QuestionDot),
                    // compound-assign + increment/decrement (M-mut.2). `-=`/`--`/`->` and
                    // `/=` (not a `//`/`/*` comment, handled earlier) all reach here distinctly.
                    (b'+', Some(b'=')) => Some(TokenKind::PlusEq),
                    (b'-', Some(b'=')) => Some(TokenKind::MinusEq),
                    (b'*', Some(b'=')) => Some(TokenKind::StarEq),
                    (b'/', Some(b'=')) => Some(TokenKind::SlashEq),
                    (b'%', Some(b'=')) => Some(TokenKind::PercentEq),
                    (b'+', Some(b'+')) => Some(TokenKind::PlusPlus),
                    (b'-', Some(b'-')) => Some(TokenKind::MinusMinus),
                    _ => None,
                };
                if let Some(k) = matched_two {
                    lx.bump();
                    lx.bump();
                    out.push(two(k));
                    continue;
                }

                let single = |k: TokenKind| Token {
                    kind: k,
                    span: Span {
                        start,
                        len: 1,
                        line,
                        col,
                    },
                };
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
                    // A lone `|` is the union-type separator (`A | B`, M-RT S4). `|>` and `||` are
                    // claimed by the two-char dispatch above, so reaching here means a single `|`.
                    b'|' => Some(TokenKind::Bar),
                    // A lone `&` is the intersection-type separator (`A & B`, M-RT S5). `&&` is
                    // claimed by the two-char dispatch above, so reaching here means a single `&`.
                    b'&' => Some(TokenKind::Amp),
                    _ => None,
                };
                match kind {
                    Some(k) => {
                        lx.bump();
                        out.push(single(k));
                    }
                    None => {
                        // Decode the full char (handles multi-byte UTF-8) for the message.
                        let ch = lx.current_char();
                        return Err(Diagnostic::new(
                            Stage::Lex,
                            format!("unexpected character {ch:?}"),
                            line,
                            col,
                        ));
                    }
                }
            }
        }
    }
}

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

    #[test]
    fn span_tracks_across_newlines() {
        // "a\nbb": ident "a" on line 1, ident "bb" on line 2 at col 1.
        let toks = lex("a\nbb").unwrap();
        // toks[0] = a, toks[1] = bb, toks[2] = Eof
        assert_eq!(toks[0].span.line, 1);
        assert_eq!(toks[0].span.col, 1);
        assert_eq!(toks[0].span.start, 0);
        assert_eq!(toks[0].span.len, 1);

        assert_eq!(toks[1].span.line, 2);
        assert_eq!(toks[1].span.col, 1);
        assert_eq!(toks[1].span.start, 2); // byte 0='a', 1='\n', 2='b'
        assert_eq!(toks[1].span.len, 2);
    }

    #[test]
    fn single_char_tokens() {
        use TokenKind::*;
        assert_eq!(
            kinds(". ; , : ? ( ) { } [ ] < > = ! + - * / %"),
            vec![
                Dot, Semicolon, Comma, Colon, Question, LParen, RParen, LBrace, RBrace, LBracket,
                RBracket, Lt, Gt, Eq, Bang, Plus, Minus, Star, Slash, Percent, Eof
            ]
        );
    }

    #[test]
    fn multi_char_operators() {
        use TokenKind::*;
        assert_eq!(
            kinds("== != <= >= -> => |> && ||"),
            vec![EqEq, NotEq, Le, Ge, Arrow, FatArrow, Pipe, AndAnd, OrOr, Eof]
        );
    }

    #[test]
    fn compound_assign_and_incdec_operators() {
        use TokenKind::*;
        // M-mut.2: the five `op=`, `??=`, and `++`/`--`.
        assert_eq!(
            kinds("+= -= *= /= %= ??= ++ --"),
            vec![
                PlusEq,
                MinusEq,
                StarEq,
                SlashEq,
                PercentEq,
                QuestionQuestionEq,
                PlusPlus,
                MinusMinus,
                Eof
            ]
        );
        // `??=` (3) is longest-match ahead of `??` (2): `??` alone still lexes as QuestionQuestion.
        assert_eq!(kinds("??"), vec![QuestionQuestion, Eof]);
        // `-=` / `--` / `->` coexist (distinct second byte).
        assert_eq!(kinds("-> -- -="), vec![Arrow, MinusMinus, MinusEq, Eof]);
        // `/=` is not a comment start (`//`, `/*`): it lexes as SlashEq.
        assert_eq!(
            kinds("a /= 2"),
            vec![Ident("a".into()), SlashEq, Int(2), Eof]
        );
    }

    #[test]
    fn range_operators_lex_longest_match() {
        use TokenKind::*;
        // `..=` (3) beats `..` (2) beats `.` (1); `0` stays an Int (no digit after the dot).
        assert_eq!(kinds("0..3"), vec![Int(0), DotDot, Int(3), Eof]);
        assert_eq!(kinds("0..=3"), vec![Int(0), DotDotEq, Int(3), Eof]);
        // a lone `.` is still a member-access Dot — `..` handling doesn't swallow it
        assert_eq!(
            kinds("a.b"),
            vec![Ident("a".into()), Dot, Ident("b".into()), Eof]
        );
    }

    #[test]
    fn number_literals() {
        use TokenKind::*;
        assert_eq!(kinds("0 42 1000"), vec![Int(0), Int(42), Int(1000), Eof]);
        assert_eq!(kinds("3.5 0.5"), vec![Float(3.5), Float(0.5), Eof]);
    }

    #[test]
    fn leading_zero_int_collapses() {
        // M1: leading zeros are absorbed by i64 parsing — `007` lexes to Int(7).
        assert_eq!(kinds("007"), vec![TokenKind::Int(7), TokenKind::Eof]);
    }

    #[test]
    fn integer_overflow_is_error_not_panic() {
        // 26-digit literal exceeds i64::MAX; must yield Diagnostic, never panic.
        let err = lex("99999999999999999999999999").unwrap_err();
        assert!(err.message.contains("out of range"), "got: {}", err.message);
        assert_eq!(err.line, 1);
        assert_eq!(err.col, 1);
    }

    #[test]
    fn float_overflow_is_error_not_panic() {
        // The lexer's float grammar is digits '.' digits (no exponent), so we use a
        // literal whose integer part exceeds f64::MAX (~1.8e308) to force inf.
        let huge = format!("{}.0", "9".repeat(320));
        let err = lex(&huge).unwrap_err();
        assert!(err.message.contains("out of range"), "got: {}", err.message);
    }

    #[test]
    fn identifiers_and_keywords() {
        use TokenKind::*;
        assert_eq!(
            kinds("function class enum constructor return match this true false null"),
            vec![
                Function,
                Class,
                Enum,
                Constructor,
                Return,
                Match,
                This,
                True,
                False,
                Null,
                Eof
            ]
        );
        assert_eq!(
            kinds("age myVar User _x"),
            vec![
                Ident("age".into()),
                Ident("myVar".into()),
                Ident("User".into()),
                Ident("_x".into()),
                Eof
            ]
        );
    }

    #[test]
    fn instanceof_keyword_is_recognized() {
        use TokenKind::*;
        // `instanceof` is the type-test keyword (M-RT S1); the retired `is` is now a plain ident.
        assert_eq!(kinds("instanceof"), vec![Instanceof, Eof]);
        assert_eq!(kinds("is"), vec![Ident("is".into()), Eof]);
        // still an ident when part of a longer word
        assert_eq!(kinds("island"), vec![Ident("island".into()), Eof]);
    }

    #[test]
    fn fn_is_a_reserved_keyword() {
        use TokenKind::*;
        assert_eq!(kinds("fn"), vec![Fn, Eof]);
        // contextual sanity: `fn (` still lexes as the keyword then a paren
        assert_eq!(kinds("fn ("), vec![Fn, LParen, Eof]);
    }

    #[test]
    fn interface_keywords_are_recognized() {
        use TokenKind::*;
        // M-RT S2 keywords: `interface`, `implements`, `extends`.
        assert_eq!(
            kinds("interface implements extends"),
            vec![Interface, Implements, Extends, Eof]
        );
        // still idents when embedded in a longer word
        assert_eq!(kinds("interfaces"), vec![Ident("interfaces".into()), Eof]);
    }

    #[test]
    fn with_keyword_is_recognized() {
        use TokenKind::*;
        // M-mut.4a `clone with` operator keyword.
        assert_eq!(kinds("with"), vec![With, Eof]);
        // still an ident embedded in a longer word.
        assert_eq!(kinds("within"), vec![Ident("within".into()), Eof]);
        assert_eq!(kinds("withdraw"), vec![Ident("withdraw".into()), Eof]);
    }

    #[test]
    fn loop_keywords_are_recognized() {
        use TokenKind::*;
        // M-mut.3 condition-loop keywords.
        assert_eq!(
            kinds("while do break continue"),
            vec![While, Do, Break, Continue, Eof]
        );
        // still idents when embedded in a longer word
        assert_eq!(kinds("breakfast"), vec![Ident("breakfast".into()), Eof]);
        assert_eq!(kinds("doer"), vec![Ident("doer".into()), Eof]);
    }

    #[test]
    fn mutable_keyword_is_recognized() {
        use TokenKind::*;
        // M-mut.1: `mutable` binding modifier.
        assert_eq!(
            kinds("mutable int x"),
            vec![Mutable, Ident("int".into()), Ident("x".into()), Eof]
        );
        // still an ident when embedded in a longer word
        assert_eq!(kinds("mutableness"), vec![Ident("mutableness".into()), Eof]);
    }

    #[test]
    fn string_literals() {
        use TokenKind::*;
        assert_eq!(kinds("\"hello\""), vec![Str("hello".into()), Eof]);
        // escapes
        assert_eq!(
            kinds("\"a\\nb\\t\\\"c\""),
            vec![Str("a\nb\t\"c".into()), Eof]
        );
        // interpolation body preserved verbatim (split happens in the parser)
        assert_eq!(
            kinds("\"Hello {name}\""),
            vec![Str("Hello {name}".into()), Eof]
        );
    }

    #[test]
    fn utf8_string_body_preserved() {
        use TokenKind::*;
        assert_eq!(kinds("\"café\""), vec![Str("café".into()), Eof]);
        assert_eq!(kinds("\"a 🎉 b\""), vec![Str("a 🎉 b".into()), Eof]);
    }

    #[test]
    fn unterminated_string_errors() {
        let err = lex("\"oops").unwrap_err();
        assert!(err.message.contains("unterminated string"));
    }

    #[test]
    fn byte_string_literals() {
        use TokenKind::*;
        assert_eq!(kinds("b\"Hi\""), vec![Bytes(vec![b'H', b'i']), Eof]);
        // \xHH escapes to arbitrary octets (incl. non-UTF-8).
        assert_eq!(
            kinds("b\"\\x48\\xff\\x00\""),
            vec![Bytes(vec![0x48, 0xff, 0x00]), Eof]
        );
        // ordinary escapes still work.
        assert_eq!(
            kinds("b\"a\\nb\""),
            vec![Bytes(vec![b'a', b'\n', b'b']), Eof]
        );
        // NO interpolation — braces are literal bytes.
        assert_eq!(
            kinds("b\"x{y}\""),
            vec![Bytes(vec![b'x', b'{', b'y', b'}']), Eof]
        );
        // a bare `b` is still an identifier; only `b"` triggers a byte literal.
        assert_eq!(kinds("b"), vec![Ident("b".into()), Eof]);
    }

    #[test]
    fn html_literals() {
        use TokenKind::*;
        assert_eq!(
            kinds("html\"<h1>Hi</h1>\""),
            vec![Html("<h1>Hi</h1>".into()), Eof]
        );
        // interpolation body preserved verbatim (split + desugar happen later).
        assert_eq!(
            kinds("html\"<h1>{name}</h1>\""),
            vec![Html("<h1>{name}</h1>".into()), Eof]
        );
        // ordinary escapes work, including `\"` for an attribute quote.
        assert_eq!(
            kinds("html\"<a href=\\\"x\\\">a\\nb</a>\""),
            vec![Html("<a href=\"x\">a\nb</a>".into()), Eof]
        );
        // multi-line for free (raw newline copied verbatim, like a plain string).
        assert_eq!(
            kinds("html\"<ul>\n  <li>x</li>\n</ul>\""),
            vec![Html("<ul>\n  <li>x</li>\n</ul>".into()), Eof]
        );
        // only the exact `html"` sequence triggers it: a bare `html`, `Html.`, `htmlx` are idents.
        assert_eq!(kinds("html"), vec![Ident("html".into()), Eof]);
        assert_eq!(
            kinds("html.text"),
            vec![Ident("html".into()), Dot, Ident("text".into()), Eof]
        );
        assert_eq!(kinds("htmlx"), vec![Ident("htmlx".into()), Eof]);
    }

    #[test]
    fn html_literal_errors() {
        assert!(lex("html\"oops")
            .unwrap_err()
            .message
            .contains("unterminated html literal"));
        assert!(lex("html\"\\q\"")
            .unwrap_err()
            .message
            .contains("invalid escape"));
    }

    #[test]
    fn byte_string_errors() {
        assert!(lex("b\"oops")
            .unwrap_err()
            .message
            .contains("unterminated byte string"));
        assert!(lex("b\"\\xZZ\"").unwrap_err().message.contains("\\xHH"));
        assert!(lex("b\"\\q\"")
            .unwrap_err()
            .message
            .contains("invalid escape"));
    }

    #[test]
    fn error_positions_are_accurate() {
        // unterminated string: points at the opening quote
        let e = lex("\"oops").unwrap_err();
        assert!(e.message.contains("unterminated string"));
        assert_eq!((e.line, e.col), (1, 1));

        // invalid escape: points at the offending backslash, not one past it
        let e = lex("\"ab\\q\"").unwrap_err();
        assert!(e.message.contains("invalid escape"));
        assert_eq!((e.line, e.col), (1, 4)); // " a b \  -> backslash at col 4

        // invalid escape on a later line reports the right line/col
        let e = lex("\"x\ny\\q\"").unwrap_err();
        assert!(e.message.contains("invalid escape"));
        assert_eq!((e.line, e.col), (2, 2)); // line 2: y(\)q  -> backslash at col 2

        // unterminated block comment: points at the comment start
        let e = lex("/* never ends").unwrap_err();
        assert!(e.message.contains("unterminated block comment"));
        assert_eq!((e.line, e.col), (1, 1));

        // unexpected char
        let e = lex("  @").unwrap_err();
        assert!(e.message.contains("unexpected character"));
        assert_eq!((e.line, e.col), (1, 3));
    }

    #[test]
    fn non_ascii_outside_string_reports_decoded_char() {
        // Identifiers are ASCII-only by design (v0.1), so a stray non-ASCII char is an
        // error — but the message must show the real char, not a mojibake lead byte.
        let e = lex("é").unwrap_err();
        assert!(e.message.contains("unexpected character"));
        assert!(e.message.contains('é'), "got: {}", e.message);
        assert_eq!((e.line, e.col), (1, 1));

        // Column must count one per char, not per byte: after the 2-byte "é",
        // the '@' is at column 2.
        let e = lex("é@").unwrap_err();
        assert!(e.message.contains('é'), "got: {}", e.message);
        assert_eq!((e.line, e.col), (1, 1));
    }

    #[test]
    fn comments_are_skipped() {
        use TokenKind::*;
        assert_eq!(kinds("1 // line comment\n2"), vec![Int(1), Int(2), Eof]);
        assert_eq!(kinds("1 /* block\ncomment */ 2"), vec![Int(1), Int(2), Eof]);
    }
}
