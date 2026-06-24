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

/// One segment of a lexed string literal. The lexer splits interpolation (`{expr}`) from literal
/// runs because only the lexer knows whether a `{` is a real interpolation brace or a `\{` literal
/// escape — a parser-side split on a flat, escape-expanded value couldn't tell them apart (a literal
/// `\{` and a `\\{` collapse to the same bytes). Literal runs have their escapes already expanded
/// (`\n`, `\u{…}`, `\{`→`{`, …); interpolation segments carry the **raw** inner expression source,
/// re-lexed + parsed by the parser. A raw string (`r"…"`) is a single `Lit` with no escapes.
#[derive(Debug, Clone, PartialEq)]
pub enum StrSeg {
    /// An escape-expanded literal run.
    Lit(String),
    /// The raw source between `{` and `}` — a Phorge expression the parser re-lexes and parses.
    Interp(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // literals
    Int(i64),
    Float(f64),
    /// A string literal, pre-split into literal + interpolation segments (the lexer owns the split
    /// so `\{` literal braces are unambiguous). Empty vec = the empty string `""`.
    Str(Vec<StrSeg>),
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
    /// `open` — the extensibility opt-in (M-RT S6). A class is `open class` to allow `extends`; a
    /// method is `open function` to allow override. Final-by-default everywhere else, so the `final`
    /// keyword is retired (redundant). Mirrors the `mutable`/immutable-default house rule.
    Open,
    /// `abstract` — a class with unimplemented methods (M-RT S6b). An `abstract class` cannot be
    /// instantiated (`E-ABSTRACT-INSTANTIATE`) and may declare bodyless `abstract function` methods a
    /// concrete subclass must implement (`E-ABSTRACT-UNIMPL`); an abstract method is implicitly `open`.
    /// Abstract implies extensible (sets `open`).
    Abstract,
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
    /// In *expression* position `&` is bitwise-AND (primitives P2); type-vs-expr is decided by the
    /// parsing context, never the token.
    Amp,
    /// `^` — bitwise XOR (primitives P2); expression-only.
    Caret,
    /// `~` — unary bitwise NOT (primitives P2); expression-only.
    Tilde,
    /// `<<` — bitwise shift-left (primitives P2). Shift-*right* `>>` is intentionally NOT a token: it
    /// is two adjacent `Gt` handled in `parse_binary`, so nested generics (`List<List<int>>`) still
    /// close with two `>`.
    Shl,
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
