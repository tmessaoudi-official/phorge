//! Token kinds + `Span` (byte range plus line/col), produced by the lexer and consumed by the
//! parser. `Span` is the single source of source-position truth threaded into
//! `diagnostic::Diagnostic` for every front-end stage.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize, // byte offset into source
    pub len: usize,
    pub line: u32, // 1-based
    pub col: u32,  // 1-based
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // literals
    Int(i64),
    Float(f64),
    Str(String),    // processed string body (interpolation split deferred to parser)
    Bytes(Vec<u8>), // `b"…"` raw byte-string literal (no interpolation)
    Html(String),   // `html"…"` literal body (interpolation split + desugar deferred to parser)
    Ident(String),
    // keywords
    Function,
    Fn,
    Class,
    Enum,
    Constructor,
    Trait,
    Const,
    Final,
    Public,
    Private,
    Protected,
    /// `internal` — package-level declaration visibility (visibility modifiers). Distinct from the
    /// member modifiers above; recognized as a top-level declaration prefix.
    Internal,
    Return,
    If,
    Else,
    For,
    While,
    Do,
    Break,
    Continue,
    In,
    Match,
    Import,
    Package,
    This,
    True,
    False,
    Null,
    New,
    Instanceof,
    Interface,
    Implements,
    Extends,
    Var,
    Mutable,
    Static,
    With,
    TypeKw,
    // M-faults Slice 2b — exception keywords.
    Throw,
    Try,
    Catch,
    Finally,
    /// `throws T (| T)*` clause on a function signature (M-faults 2b). Distinct from `throw` (the
    /// statement); the lexer matches the full word so `throws` never lexes as `throw` + `s`.
    Throws,
    // punctuation / operators
    Dot,
    DotDot,   // `..` exclusive range
    DotDotEq, // `..=` inclusive range
    Semicolon,
    Comma,
    Colon,
    Question,
    QuestionQuestion, // `??` null-coalesce
    QuestionDot,      // `?.` safe (nullsafe) access
    Arrow,
    FatArrow,
    Pipe,
    /// A lone `|` — the union-type separator `A | B` (M-RT S4). Distinct from `|>` (`Pipe`) and
    /// `||` (`OrOr`); the lexer's two-char dispatch claims those first, so a bare `|` falls through
    /// to this single-char token.
    Bar,
    /// A lone `&` — the intersection-type separator `A & B` (M-RT S5). Distinct from `&&` (`AndAnd`),
    /// which the lexer's two-char dispatch claims first, so a bare `&` falls through to this
    /// single-char token. Binds tighter than `|` in `parse_type` (`A | B & C` ≡ `A | (B & C)`).
    Amp,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Lt,
    Gt,
    Le,
    Ge,
    EqEq,
    NotEq,
    Eq,
    Bang,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    AndAnd,
    OrOr,
    // compound-assignment + increment/decrement (M-mut.2). Each desugars in the parser into the
    // `Stmt::Assign` from M-mut.1 (`x += e` ⟶ `x = x + e`), so no backend learns a new form.
    PlusEq,             // `+=`
    MinusEq,            // `-=`
    StarEq,             // `*=`
    SlashEq,            // `/=`  (routes through __phorge_div on transpile, via BinaryOp::Div)
    PercentEq,          // `%=`  (routes through __phorge_rem on transpile, via BinaryOp::Rem)
    PlusPlus,           // `++`  (statement form `x++` only)
    MinusMinus,         // `--`  (statement form `x--` only)
    QuestionQuestionEq, // `??=` null-coalesce-assign (three-char; longest-match ahead of `??`)
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
        let t = Token {
            kind: TokenKind::Semicolon,
            span: Span {
                line: 3,
                col: 7,
                start: 42,
                len: 1,
            },
        };
        assert_eq!(t.span.line, 3);
        assert_eq!(t.span.col, 7);
        assert!(matches!(t.kind, TokenKind::Semicolon));
    }
}
