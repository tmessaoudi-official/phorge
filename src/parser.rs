//! Recursive-descent + Pratt parser: turns the lexer's token stream into the AST.

use crate::ast::{BinaryOp, Expr, MatchArm, Pattern, StrPart, Type, UnaryOp};
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

    /// Entry point: parse a full expression (lowest precedence).
    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_binary(0)
    }

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
            if bp < min_bp {
                break;
            }
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
            self.parse_postfix()
        }
    }

    /// Parse a primary, then apply any chain of postfix operators.
    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.parse_primary()?;
        loop {
            let sp = self.peek_span();
            match self.peek() {
                TokenKind::Dot => {
                    self.advance();
                    let name = match self.peek().clone() {
                        TokenKind::Ident(n) => {
                            self.advance();
                            n
                        }
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
            if !self.eat(&TokenKind::Comma) {
                break;
            }
            if self.check(&TokenKind::RParen) {
                break; // trailing comma
            }
        }
        Ok(args)
    }

    /// Lowest-level expression: a literal, identifier, `this`, string, list, match, or `( expr )`.
    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let sp = self.peek_span();
        match self.peek().clone() {
            TokenKind::Int(n) => {
                self.advance();
                Ok(Expr::Int(n, sp))
            }
            TokenKind::Float(f) => {
                self.advance();
                Ok(Expr::Float(f, sp))
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::Bool(true, sp))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::Bool(false, sp))
            }
            TokenKind::Null => {
                self.advance();
                Ok(Expr::Null(sp))
            }
            TokenKind::This => {
                self.advance();
                Ok(Expr::This(sp))
            }
            TokenKind::Ident(name) => {
                self.advance();
                Ok(Expr::Ident(name, sp))
            }
            TokenKind::Str(body) => {
                self.advance();
                let parts = self.split_interpolation(&body, sp)?;
                Ok(Expr::Str(parts, sp))
            }
            TokenKind::Match => self.parse_match(sp),
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "')'")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        items.push(self.parse_expr()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                        if self.check(&TokenKind::RBracket) {
                            break; // trailing comma
                        }
                    }
                }
                self.expect(&TokenKind::RBracket, "']' to close list literal")?;
                Ok(Expr::List(items, sp))
            }
            _ => Err(self.error("an expression")),
        }
    }

    /// Parse a type annotation: `Name`, `Name<T, U>`, or `T?`.
    pub fn parse_type(&mut self) -> Result<Type, ParseError> {
        let sp = self.peek_span();
        let name = match self.peek().clone() {
            TokenKind::Ident(n) => {
                self.advance();
                n
            }
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
        // trailing `?` makes it optional; allow stacking (`T??` -> Optional(Optional))
        while self.eat(&TokenKind::Question) {
            t = Type::Optional { inner: Box::new(t), span: sp };
        }
        Ok(t)
    }

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
                        if ic == '}' {
                            closed = true;
                            break;
                        }
                        inner.push(ic);
                    }
                    if !closed {
                        return Err(ParseError {
                            message: "unterminated interpolation '{' in string".into(),
                            line: sp.line,
                            col: sp.col,
                        });
                    }
                    let sub_tokens = crate::lexer::lex(&inner).map_err(|e| ParseError {
                        message: format!("in interpolation: {}", e.message),
                        line: sp.line,
                        col: sp.col,
                    })?;
                    let mut sub = Parser::new(sub_tokens);
                    let e = sub.parse_expr()?;
                    sub.expect(&TokenKind::Eof, "end of interpolation expression")?;
                    parts.push(StrPart::Expr(Box::new(e)));
                }
                '}' => {
                    return Err(ParseError {
                        message: "unexpected '}' in string (no matching '{')".into(),
                        line: sp.line,
                        col: sp.col,
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

    /// Parse a single pattern (used in `match` arms).
    pub fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let sp = self.peek_span();
        match self.peek().clone() {
            TokenKind::Int(n) => {
                self.advance();
                Ok(Pattern::Int(n, sp))
            }
            TokenKind::Float(f) => {
                self.advance();
                Ok(Pattern::Float(f, sp))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Pattern::Str(s, sp))
            }
            TokenKind::True => {
                self.advance();
                Ok(Pattern::Bool(true, sp))
            }
            TokenKind::False => {
                self.advance();
                Ok(Pattern::Bool(false, sp))
            }
            TokenKind::Null => {
                self.advance();
                Ok(Pattern::Null(sp))
            }
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
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                            if self.check(&TokenKind::RParen) {
                                break; // trailing comma
                            }
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
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace, "'}' to close match")?;
        Ok(Expr::Match { scrutinee: Box::new(scrutinee), arms, span: sp })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expr, Pattern, StrPart, Type};
    use crate::lexer::lex;

    /// Helper: lex `src` and build a parser over the tokens.
    fn parser(src: &str) -> Parser {
        Parser::new(lex(src).expect("lex ok"))
    }

    /// Helper: parse `src` as a single expression.
    fn expr(src: &str) -> Expr {
        parser(src).parse_expr().expect("parse ok")
    }

    fn ty(src: &str) -> Type {
        parser(src).parse_type().expect("parse ok")
    }

    fn pat(src: &str) -> Pattern {
        parser(src).parse_pattern().expect("parse ok")
    }

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
                let o = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                };
                format!("({o} {})", sexpr(expr))
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                let o = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Rem => "%",
                    BinaryOp::Eq => "==",
                    BinaryOp::NotEq => "!=",
                    BinaryOp::Is => "is",
                    BinaryOp::Lt => "<",
                    BinaryOp::Gt => ">",
                    BinaryOp::Le => "<=",
                    BinaryOp::Ge => ">=",
                    BinaryOp::And => "&&",
                    BinaryOp::Or => "||",
                    BinaryOp::Pipe => "|>",
                };
                format!("({o} {} {})", sexpr(lhs), sexpr(rhs))
            }
            Expr::Member { object, name, .. } => format!("{}.{}", sexpr(object), name),
            Expr::Call { callee, args, .. } => {
                let a: Vec<String> = args.iter().map(sexpr).collect();
                format!("{}({})", sexpr(callee), a.join(", "))
            }
            Expr::Index { object, index, .. } => format!("{}[{}]", sexpr(object), sexpr(index)),
            other => format!("{other:?}"),
        }
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

    #[test]
    fn parses_types() {
        match ty("int") {
            Type::Named { name, args, .. } => {
                assert_eq!(name, "int");
                assert!(args.is_empty());
            }
            other => panic!("got {other:?}"),
        }
        match ty("List<Shape>") {
            Type::Named { name, args, .. } => {
                assert_eq!(name, "List");
                assert_eq!(args.len(), 1);
            }
            other => panic!("got {other:?}"),
        }
        match ty("Map<string, int>") {
            Type::Named { name, args, .. } => {
                assert_eq!(name, "Map");
                assert_eq!(args.len(), 2);
            }
            other => panic!("got {other:?}"),
        }
        assert!(matches!(ty("int?"), Type::Optional { .. }));
        // nested generics
        match ty("List<Map<string, int>>") {
            Type::Named { name, args, .. } => {
                assert_eq!(name, "List");
                assert_eq!(args.len(), 1);
            }
            other => panic!("got {other:?}"),
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

    #[test]
    fn parses_postfix_chains() {
        // member access
        match expr("a.b") {
            Expr::Member { object, name, .. } => {
                assert!(matches!(*object, Expr::Ident(ref s, _) if s == "a"));
                assert_eq!(name, "b");
            }
            other => panic!("got {other:?}"),
        }
        // call with args (also covers constructor calls like Circle(2.0))
        match expr("f(1, 2)") {
            Expr::Call { callee, args, .. } => {
                assert!(matches!(*callee, Expr::Ident(ref s, _) if s == "f"));
                assert_eq!(args.len(), 2);
            }
            other => panic!("got {other:?}"),
        }
        match expr("Circle(2.0)") {
            Expr::Call { callee, args, .. } => {
                assert!(matches!(*callee, Expr::Ident(ref s, _) if s == "Circle"));
                assert_eq!(args.len(), 1);
            }
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
            Expr::Str(parts, _) => {
                assert_eq!(parts.len(), 1);
                assert!(matches!(&parts[0], StrPart::Expr(_)));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn unterminated_interpolation_errors() {
        let mut p = parser("\"Hello {name\"");
        assert!(p.parse_expr().is_err());
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
            Pattern::Variant { name, fields, .. } => {
                assert_eq!(name, "Rect");
                assert_eq!(fields.len(), 2);
            }
            other => panic!("got {other:?}"),
        }
        // nested variant patterns
        match pat("Wrap(Circle(r))") {
            Pattern::Variant { fields, .. } => assert!(matches!(&fields[0], Pattern::Variant { .. })),
            other => panic!("got {other:?}"),
        }
    }

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
}
