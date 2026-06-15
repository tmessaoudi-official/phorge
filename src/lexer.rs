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

    fn peek2(&self) -> Option<u8> { self.src.get(self.pos + 1).copied() }

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

    fn scan_ident(&mut self, start: usize, line: u32, col: u32) -> Token {
        while matches!(self.peek(), Some(b) if b == b'_' || b.is_ascii_alphanumeric()) { self.bump(); }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        let kind = keyword(text).unwrap_or_else(|| TokenKind::Ident(text.to_string()));
        Token { kind, span: Span { start, len: self.pos - start, line, col } }
    }
}

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
            Some(b) => {
                if b == b'/' && lx.peek2() == Some(b'/') { lx.skip_line_comment(); continue; }
                if b == b'/' && lx.peek2() == Some(b'*') { lx.skip_block_comment()?; continue; }

                if b == b'"' {
                    let t = lx.scan_string(start, line, col)?;
                    out.push(t);
                    continue;
                }

                if b.is_ascii_digit() {
                    let t = lx.scan_number(start, line, col);
                    out.push(t);
                    continue;
                }

                if b == b'_' || b.is_ascii_alphabetic() {
                    let t = lx.scan_ident(start, line, col);
                    out.push(t);
                    continue;
                }

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
    fn single_char_tokens() {
        use TokenKind::*;
        assert_eq!(
            kinds(". ; , : ? ( ) { } [ ] < > = ! + - * / %"),
            vec![Dot, Semicolon, Comma, Colon, Question, LParen, RParen,
                 LBrace, RBrace, LBracket, RBracket, Lt, Gt, Eq, Bang,
                 Plus, Minus, Star, Slash, Percent, Eof]
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
    fn number_literals() {
        use TokenKind::*;
        assert_eq!(kinds("0 42 1000"), vec![Int(0), Int(42), Int(1000), Eof]);
        assert_eq!(kinds("3.14 0.5"), vec![Float(3.14), Float(0.5), Eof]);
    }

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

    #[test]
    fn comments_are_skipped() {
        use TokenKind::*;
        assert_eq!(kinds("1 // line comment\n2"), vec![Int(1), Int(2), Eof]);
        assert_eq!(kinds("1 /* block\ncomment */ 2"), vec![Int(1), Int(2), Eof]);
    }
}
