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
}
