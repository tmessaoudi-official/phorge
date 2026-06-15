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
    This, True, False, Null, New, Is,
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
