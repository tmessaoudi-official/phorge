//! Recursive-descent + Pratt parser: turns the lexer's token stream into the AST.

use crate::ast::{
    BinaryOp, ClassDecl, ClassMember, CtorParam, EnumDecl, EnumVariant, Expr, FunctionDecl, Item,
    LambdaBody, MatchArm, Modifier, Param, Pattern, Program, Stmt, StrPart, Type, UnaryOp,
    Visibility,
};
use crate::diagnostic::{Diagnostic, Stage};
use crate::limits::MAX_NEST_DEPTH;
use crate::token::{Span, Token, TokenKind};

/// Set the declaration-level visibility on a freshly parsed top-level item. Only the four declaration
/// kinds carry visibility; any other item is returned unchanged (imports/type aliases are guarded
/// against a visibility prefix in `parse_item` before this is reached).
fn stamp_visibility(item: Item, vis: Visibility) -> Item {
    match item {
        Item::Function(mut f) => {
            f.vis = vis;
            Item::Function(f)
        }
        Item::Class(mut c) => {
            c.vis = vis;
            Item::Class(c)
        }
        Item::Enum(mut e) => {
            e.vis = vis;
            Item::Enum(e)
        }
        Item::Interface(mut i) => {
            i.vis = vis;
            Item::Interface(i)
        }
        other => other,
    }
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Live expression-nesting depth, checked against [`MAX_NEST_DEPTH`] in `parse_unary` — the
    /// one function every nesting vector (parens, unary chains, index/list/arg re-entry) passes
    /// through exactly once per level.
    depth: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        // The lexer always terminates the stream with Eof, so `tokens` is non-empty.
        Parser {
            tokens,
            pos: 0,
            depth: 0,
        }
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

    /// Consume a token of the expected kind or produce a Diagnostic.
    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<Token, Diagnostic> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(self.error(what))
        }
    }

    /// Build a Diagnostic at the current position.
    fn error(&self, what: &str) -> Diagnostic {
        let sp = self.peek_span();
        Diagnostic::new(
            Stage::Parse,
            format!("expected {}, found {:?}", what, self.peek()),
            sp.line,
            sp.col,
        )
    }

    /// Entry point: parse a full expression (lowest precedence).
    pub fn parse_expr(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_range()
    }

    /// Ranges bind looser than every binary operator: `a..b` reads `a` and `b` as full
    /// (binary) sub-expressions, so `0..n + 1` is `0..(n + 1)`. Non-chaining (no `a..b..c`); a
    /// single optional `..`/`..=` follows the first operand. Used mainly as `for (int i in 0..n)`.
    fn parse_range(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.parse_binary(0)?;
        let inclusive = match self.peek() {
            TokenKind::DotDot => false,
            TokenKind::DotDotEq => true,
            _ => return Ok(start),
        };
        let sp = self.peek_span();
        self.advance(); // consume `..` / `..=`
        let end = self.parse_binary(0)?;
        Ok(Expr::Range {
            start: Box::new(start),
            end: Box::new(end),
            inclusive,
            span: sp,
        })
    }

    /// Left binding power for an infix operator token, plus its `BinaryOp`.
    /// Returns None if the token is not an infix operator. Higher binds tighter.
    fn infix_op(kind: &TokenKind) -> Option<(u8, BinaryOp)> {
        use TokenKind as T;
        Some(match kind {
            T::Pipe => (1, BinaryOp::Pipe),
            T::QuestionQuestion => (2, BinaryOp::Coalesce),
            T::OrOr => (3, BinaryOp::Or),
            T::AndAnd => (4, BinaryOp::And),
            T::EqEq => (5, BinaryOp::Eq),
            T::NotEq => (5, BinaryOp::NotEq),
            T::Lt => (6, BinaryOp::Lt),
            T::Gt => (6, BinaryOp::Gt),
            T::Le => (6, BinaryOp::Le),
            T::Ge => (6, BinaryOp::Ge),
            T::Plus => (7, BinaryOp::Add),
            T::Minus => (7, BinaryOp::Sub),
            T::Star => (8, BinaryOp::Mul),
            T::Slash => (8, BinaryOp::Div),
            T::Percent => (8, BinaryOp::Rem),
            _ => return None,
        })
    }

    /// Precedence-climbing: parse a unary, then fold infix operators whose
    /// binding power is >= `min_bp`. All our binary operators are left-associative,
    /// so the right operand is parsed with `bp + 1`.
    fn parse_binary(&mut self, min_bp: u8) -> Result<Expr, Diagnostic> {
        let mut lhs = self.parse_unary()?;
        loop {
            // `instanceof` is a type test at precedence 5 (like `==`), but its right operand is a
            // *type name*, not an expression — so it is parsed here rather than via `infix_op`. The
            // left operand and result type (`bool`) are validated by the checker (M-RT S1).
            if matches!(self.peek(), TokenKind::Instanceof) && 5 >= min_bp {
                let sp = self.peek_span();
                self.advance(); // consume `instanceof`
                let type_name = match self.peek().clone() {
                    TokenKind::Ident(n) => {
                        self.advance();
                        n
                    }
                    _ => return Err(self.error("a class name after `instanceof`")),
                };
                lhs = Expr::InstanceOf {
                    value: Box::new(lhs),
                    type_name,
                    span: sp,
                };
                continue;
            }
            let Some((bp, op)) = Self::infix_op(self.peek()) else {
                break;
            };
            if bp < min_bp {
                break;
            }
            let sp = self.peek_span();
            self.advance(); // consume the operator
            let rhs = self.parse_binary(bp + 1)?;
            lhs = if matches!(op, BinaryOp::Pipe) {
                // `lhs |> rhs` is syntactic sugar for `rhs(lhs)` — lower to a Call in the
                // parser so all four backends see an ordinary function call. `BinaryOp::Pipe`
                // is never placed in an `Expr::Binary` node; the precedence-table entry at
                // `infix_op` is kept to drive the precedence-climbing loop.
                Expr::Call {
                    callee: Box::new(rhs),
                    args: vec![lhs],
                    span: sp,
                }
            } else {
                Expr::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span: sp,
                }
            };
        }
        Ok(lhs)
    }

    /// Prefix unary operators: `-expr`, `!expr`. Right-associative by recursion.
    ///
    /// Every nesting vector — parens (`parse_primary` → `parse_expr`), unary chains (self-recursion
    /// here), and index/list/arg re-entry — routes through this function exactly once per level, so
    /// the depth guard here bounds all of them with a single counter. Past [`MAX_NEST_DEPTH`] it
    /// faults cleanly rather than overflowing the native stack. `depth` is balanced on both the `Ok`
    /// and `Err` paths (the result is captured before the decrement); the over-limit path aborts the
    /// whole parse, so leaving `depth` incremented there is harmless.
    fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        self.depth += 1;
        if self.depth > MAX_NEST_DEPTH {
            let sp = self.peek_span();
            return Err(Diagnostic::new(
                Stage::Parse,
                format!("expression nests too deeply (limit {MAX_NEST_DEPTH})"),
                sp.line,
                sp.col,
            ));
        }
        let sp = self.peek_span();
        let op = match self.peek() {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Bang => Some(UnaryOp::Not),
            _ => None,
        };
        let result = if let Some(op) = op {
            self.advance();
            self.parse_unary().map(|expr| Expr::Unary {
                op,
                expr: Box::new(expr),
                span: sp,
            })
        } else {
            self.parse_postfix()
        };
        self.depth -= 1;
        result
    }

    /// Parse a primary, then apply any chain of postfix operators.
    fn parse_postfix(&mut self) -> Result<Expr, Diagnostic> {
        let mut e = self.parse_primary()?;
        loop {
            let sp = self.peek_span();
            match self.peek() {
                TokenKind::Dot | TokenKind::QuestionDot => {
                    let safe = matches!(self.peek(), TokenKind::QuestionDot);
                    self.advance();
                    let name = match self.peek().clone() {
                        TokenKind::Ident(n) => {
                            self.advance();
                            n
                        }
                        _ => return Err(self.error("a field or method name after '.' or '?.'")),
                    };
                    e = Expr::Member {
                        object: Box::new(e),
                        name,
                        safe,
                        span: sp,
                    };
                }
                TokenKind::LParen => {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(&TokenKind::RParen, "')' to close arguments")?;
                    e = Expr::Call {
                        callee: Box::new(e),
                        args,
                        span: sp,
                    };
                }
                TokenKind::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket, "']' to close index")?;
                    e = Expr::Index {
                        object: Box::new(e),
                        index: Box::new(index),
                        span: sp,
                    };
                }
                // Postfix `!` is the force-unwrap (M3 S2.5). It can only appear here, after a
                // primary/postfix expr; prefix `!x` (logical not) is handled in `parse_unary`, and
                // `!=` lexes as a single `NotEq`, so there is no ambiguity.
                TokenKind::Bang => {
                    self.advance();
                    e = Expr::Force {
                        inner: Box::new(e),
                        span: sp,
                    };
                }
                // Postfix `?` is error propagation (M-faults Slice 2a). The lexer munches `??`/`?.`
                // into `QuestionQuestion`/`QuestionDot`, so a lone `Question` here is unambiguous.
                TokenKind::Question => {
                    self.advance();
                    e = Expr::Propagate {
                        inner: Box::new(e),
                        span: sp,
                    };
                }
                // `obj with { f = e, … }` — functional update (M-mut.4a). Postfix, so it binds to the
                // immediately-preceding expression; the brace block is unambiguous in expr position.
                TokenKind::With => {
                    self.advance();
                    self.expect(&TokenKind::LBrace, "'{' after 'with'")?;
                    let mut fields = Vec::new();
                    while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
                        let name = self.expect_ident("a field name in `with { … }`")?;
                        self.expect(&TokenKind::Eq, "'=' after a `with` field name")?;
                        let value = self.parse_expr()?;
                        fields.push((name, value));
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RBrace, "'}' to close `with { … }`")?;
                    e = Expr::CloneWith {
                        object: Box::new(e),
                        fields,
                        span: sp,
                    };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    /// Comma-separated expressions until the closing delimiter (caller consumes the closer).
    /// Allows zero args; allows a trailing comma.
    fn parse_arg_list(&mut self) -> Result<Vec<Expr>, Diagnostic> {
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
    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
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
            TokenKind::Bytes(b) => {
                self.advance();
                Ok(Expr::Bytes(b, sp))
            }
            TokenKind::Html(body) => {
                self.advance();
                // Reuse the exact `{expr}` splitter as plain strings; the type-directed desugar
                // into `html.concat([…])` kernel calls happens in the checker (which has types).
                let parts = self.split_interpolation(&body, sp)?;
                Ok(Expr::Html(parts, sp))
            }
            TokenKind::Match => self.parse_match(sp),
            TokenKind::If => self.parse_if_expr(sp),
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "')'")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                self.advance();
                // `[]` is the empty *list* (an empty map literal is deferred — it needs a builder).
                if self.check(&TokenKind::RBracket) {
                    self.advance();
                    Ok(Expr::List(Vec::new(), sp))
                } else {
                    // Parse the first element, then disambiguate: a following `=>` makes this a map
                    // literal (`[k => v, …]`); otherwise it's a list (`[a, b, …]`). A lambda element
                    // (`fn(x) => x`) consumes its own `=>` inside `parse_expr`, so it never trips the
                    // map peek. Once chosen, a mismatched separator errors cleanly at `expect`.
                    let first = self.parse_expr()?;
                    if self.eat(&TokenKind::FatArrow) {
                        let val = self.parse_expr()?;
                        let mut pairs = vec![(first, val)];
                        while self.eat(&TokenKind::Comma) {
                            if self.check(&TokenKind::RBracket) {
                                break; // trailing comma
                            }
                            let k = self.parse_expr()?;
                            self.expect(&TokenKind::FatArrow, "'=>' in map literal")?;
                            let v = self.parse_expr()?;
                            pairs.push((k, v));
                        }
                        self.expect(&TokenKind::RBracket, "']' to close map literal")?;
                        Ok(Expr::Map(pairs, sp))
                    } else {
                        let mut items = vec![first];
                        while self.eat(&TokenKind::Comma) {
                            if self.check(&TokenKind::RBracket) {
                                break; // trailing comma
                            }
                            items.push(self.parse_expr()?);
                        }
                        self.expect(&TokenKind::RBracket, "']' to close list literal")?;
                        Ok(Expr::List(items, sp))
                    }
                }
            }
            // Lambda expression: `fn(int x, int y) -> int => x + y` (expression body only;
            // statement-body lambdas land in S3 Task 6).
            TokenKind::Fn => {
                self.advance(); // consume 'fn'
                self.expect(&TokenKind::LParen, "'(' after 'fn'")?;
                let params = self.parse_params()?;
                self.expect(&TokenKind::RParen, "')' to close lambda parameters")?;
                // Optional return-type annotation before `=>`.
                let ret = if self.eat(&TokenKind::Arrow) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                let body = if self.eat(&TokenKind::FatArrow) {
                    LambdaBody::Expr(Box::new(self.parse_expr()?))
                } else if self.check(&TokenKind::LBrace) {
                    LambdaBody::Block(self.parse_block()?)
                } else {
                    return Err(self.error("'=>' (expression body) or '{' (statement body)"));
                };
                Ok(Expr::Lambda {
                    params,
                    ret,
                    body,
                    span: sp,
                })
            }
            _ => Err(self.error("an expression")),
        }
    }

    /// Parse a type annotation: `Name`, `Name<T, U>`, `T?`, `(T, U) -> R`, or a union `A | B | C`
    /// (M-RT S4). A single atom is returned unchanged (so a non-union program's AST is byte-identical);
    /// `?` binds to its immediate member (`A | B?` ≡ `A | (B?)`).
    pub fn parse_type(&mut self) -> Result<Type, Diagnostic> {
        let sp = self.peek_span();
        let first = self.parse_type_intersection()?;
        if !self.check(&TokenKind::Bar) {
            return Ok(first);
        }
        let mut members = vec![first];
        while self.eat(&TokenKind::Bar) {
            members.push(self.parse_type_intersection()?);
        }
        Ok(Type::Union(members, sp))
    }

    /// Parse an intersection level `A & B & C` (M-RT S5), which binds **tighter than** `|` — so
    /// `A | B & C` ≡ `A | (B & C)`. A single atom is returned unchanged (so a non-intersection
    /// program's AST is byte-identical). Sits between [`Self::parse_type`] (union) and
    /// [`Self::parse_type_atom`].
    fn parse_type_intersection(&mut self) -> Result<Type, Diagnostic> {
        let sp = self.peek_span();
        let first = self.parse_type_atom()?;
        if !self.check(&TokenKind::Amp) {
            return Ok(first);
        }
        let mut members = vec![first];
        while self.eat(&TokenKind::Amp) {
            members.push(self.parse_type_atom()?);
        }
        Ok(Type::Intersection(members, sp))
    }

    /// Parse a single (non-union, non-intersection) type: `Name`, `Name<T, U>`, `T?`, or `(T, U) -> R`. Type arguments
    /// and function params recurse through [`Self::parse_type`], so a union nests inside them
    /// (`List<A | B>`, `(A | B) -> C`).
    fn parse_type_atom(&mut self) -> Result<Type, Diagnostic> {
        let sp = self.peek_span();
        // Leading `(` introduces a function type: `(int, string) -> bool`.
        if self.eat(&TokenKind::LParen) {
            let mut params = Vec::new();
            if !self.check(&TokenKind::RParen) {
                params.push(self.parse_type()?);
                while self.eat(&TokenKind::Comma) {
                    params.push(self.parse_type()?);
                }
            }
            self.expect(&TokenKind::RParen, "')' to close function-type parameters")?;
            self.expect(&TokenKind::Arrow, "'->' in a function type")?;
            let ret = Box::new(self.parse_type()?);
            let mut t = Type::Function {
                params,
                ret,
                span: sp,
            };
            while self.eat(&TokenKind::Question) {
                t = Type::Optional {
                    inner: Box::new(t),
                    span: sp,
                };
            }
            return Ok(t);
        }
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
        let mut t = Type::Named {
            name,
            args,
            span: sp,
        };
        // trailing `?` makes it optional; allow stacking (`T??` -> Optional(Optional))
        while self.eat(&TokenKind::Question) {
            t = Type::Optional {
                inner: Box::new(t),
                span: sp,
            };
        }
        Ok(t)
    }

    /// Split a string body into literal runs and `{expr}` interpolations.
    /// Each interpolation is re-lexed + re-parsed as a standalone expression.
    /// M1 limitation: literal braces (`{{`) are not supported.
    fn split_interpolation(&self, body: &str, sp: Span) -> Result<Vec<StrPart>, Diagnostic> {
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
                        return Err(Diagnostic::new(
                            Stage::Parse,
                            "unterminated interpolation '{' in string",
                            sp.line,
                            sp.col,
                        ));
                    }
                    let sub_tokens = crate::lexer::lex(&inner).map_err(|e| {
                        Diagnostic::new(
                            Stage::Parse,
                            format!("in interpolation: {}", e.message),
                            sp.line,
                            sp.col,
                        )
                    })?;
                    let mut sub = Parser::new(sub_tokens);
                    let e = sub.parse_expr()?;
                    sub.expect(&TokenKind::Eof, "end of interpolation expression")?;
                    parts.push(StrPart::Expr(Box::new(e)));
                }
                '}' => {
                    return Err(Diagnostic::new(
                        Stage::Parse,
                        "unexpected '}' in string (no matching '{')",
                        sp.line,
                        sp.col,
                    ));
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
    pub fn parse_pattern(&mut self) -> Result<Pattern, Diagnostic> {
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
                    Ok(Pattern::Variant {
                        name,
                        fields,
                        span: sp,
                    })
                } else if let TokenKind::Ident(binder) = self.peek().clone() {
                    // A second identifier in pattern position makes this a **type pattern** for
                    // match-over-union (`Circle c`, M-RT S4): `name` is the type, `binder` the bound
                    // variable (`_` binds nothing). A lone `name =>` keeps the catch-all `Binding`.
                    self.advance();
                    let binding = if binder == "_" { None } else { Some(binder) };
                    Ok(Pattern::Type {
                        type_name: name,
                        binding,
                        span: sp,
                    })
                } else {
                    Ok(Pattern::Binding { name, span: sp })
                }
            }
            _ => Err(self.error("a pattern")),
        }
    }

    /// `match EXPR { PAT => EXPR, ... }` — assumes the current token is `match`.
    fn parse_match(&mut self, sp: Span) -> Result<Expr, Diagnostic> {
        self.expect(&TokenKind::Match, "'match'")?;
        let scrutinee = self.parse_expr()?;
        self.expect(&TokenKind::LBrace, "'{' to open match arms")?;
        let mut arms = Vec::new();
        while !self.check(&TokenKind::RBrace) {
            let arm_sp = self.peek_span();
            let pattern = self.parse_pattern()?;
            self.expect(&TokenKind::FatArrow, "'=>' after match pattern")?;
            let body = self.parse_expr()?;
            arms.push(MatchArm {
                pattern,
                body,
                span: arm_sp,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace, "'}' to close match")?;
        Ok(Expr::Match {
            scrutinee: Box::new(scrutinee),
            arms,
            span: sp,
        })
    }

    /// `if (cond) { e } else { e }` in **expression** position — parens and a single-expression
    /// body per arm, with a mandatory `else` (the value must come from somewhere). Reached only via
    /// `parse_primary`; a top-level `if` statement is matched first by `parse_stmt`, so the two
    /// never collide. Mirrors statement-`if`'s `if (cond)` shape for intra-language consistency.
    fn parse_if_expr(&mut self, sp: Span) -> Result<Expr, Diagnostic> {
        self.expect(&TokenKind::If, "'if'")?;
        self.expect(&TokenKind::LParen, "'(' after 'if'")?;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "')' after if condition")?;
        self.expect(&TokenKind::LBrace, "'{' to open the then-branch")?;
        let then_expr = self.parse_expr()?;
        self.expect(&TokenKind::RBrace, "'}' to close the then-branch")?;
        self.expect(
            &TokenKind::Else,
            "'else' (an expression `if` must have an else branch)",
        )?;
        self.expect(&TokenKind::LBrace, "'{' to open the else-branch")?;
        let else_expr = self.parse_expr()?;
        self.expect(&TokenKind::RBrace, "'}' to close the else-branch")?;
        Ok(Expr::If {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
            span: sp,
        })
    }

    /// Consume an identifier token, returning its name, or error with `what`.
    fn expect_ident(&mut self, what: &str) -> Result<String, Diagnostic> {
        match self.peek().clone() {
            TokenKind::Ident(n) => {
                self.advance();
                Ok(n)
            }
            _ => Err(self.error(what)),
        }
    }

    /// Parse one statement.
    pub fn parse_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        match self.peek() {
            TokenKind::Return => self.parse_return(),
            TokenKind::If => self.parse_if(),
            TokenKind::For => self.parse_for(),
            TokenKind::While => self.parse_while(),
            TokenKind::Do => self.parse_do_while(),
            TokenKind::Break => {
                let sp = self.peek_span();
                self.advance();
                self.expect(&TokenKind::Semicolon, "';' after 'break'")?;
                Ok(Stmt::Break(sp))
            }
            TokenKind::Continue => {
                let sp = self.peek_span();
                self.advance();
                self.expect(&TokenKind::Semicolon, "';' after 'continue'")?;
                Ok(Stmt::Continue(sp))
            }
            TokenKind::LBrace => {
                let sp = self.peek_span();
                let body = self.parse_block()?;
                Ok(Stmt::Block(body, sp))
            }
            TokenKind::Var => self.parse_var_inferred(false),
            TokenKind::Mutable => self.parse_mutable_var_decl(),
            TokenKind::Throw => self.parse_throw(),
            TokenKind::Try => self.parse_try(),
            _ => self.parse_var_decl_or_expr_stmt(),
        }
    }

    /// `throw expr;` (M-faults 2b).
    fn parse_throw(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::Throw, "'throw'")?;
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "';' after 'throw <expr>'")?;
        Ok(Stmt::Throw { value, span: sp })
    }

    /// `try { .. } catch (Type name) { .. } [catch …] [finally { .. }]` (M-faults 2b). Requires at
    /// least one `catch` **or** a `finally` (a bare `try {}` is a parse error). A catch type may be a
    /// union (`catch (A | B e)`), parsed by the shared `parse_type`.
    fn parse_try(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::Try, "'try'")?;
        let body = self.parse_block()?;
        let mut catches = Vec::new();
        while self.check(&TokenKind::Catch) {
            let csp = self.peek_span();
            self.advance(); // 'catch'
            self.expect(&TokenKind::LParen, "'(' after 'catch'")?;
            let ty = self.parse_type()?;
            let name = self.expect_ident("a binding name in the catch clause")?;
            self.expect(&TokenKind::RParen, "')' to close the catch clause")?;
            let cbody = self.parse_block()?;
            catches.push(crate::ast::CatchClause {
                ty,
                name,
                body: cbody,
                span: csp,
            });
        }
        let finally_block = if self.eat(&TokenKind::Finally) {
            Some(self.parse_block()?)
        } else {
            None
        };
        if catches.is_empty() && finally_block.is_none() {
            return Err(self.error("'catch' or 'finally' after the try block"));
        }
        Ok(Stmt::Try {
            body,
            catches,
            finally_block,
            span: sp,
        })
    }

    /// `var name = expr;` — the binding type is inferred from `expr` by the checker. `mutable` is
    /// `true` when this was reached via `mutable var name = …` (M-mut.1).
    fn parse_var_inferred(&mut self, mutable: bool) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::Var, "'var'")?;
        let name = self.expect_ident("a variable name after 'var'")?;
        self.expect(&TokenKind::Eq, "'=' after 'var <name>'")?;
        let init = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "';' after variable declaration")?;
        Ok(Stmt::VarDecl {
            ty: Type::Infer(sp),
            name,
            init,
            mutable,
            span: sp,
        })
    }

    /// `mutable var name = expr;` or `mutable Type name = expr;` (M-mut.1). `mutable` only ever
    /// precedes a binding declaration, so the typed form is committed (no speculative rewind).
    fn parse_mutable_var_decl(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::Mutable, "'mutable'")?;
        if self.check(&TokenKind::Var) {
            return self.parse_var_inferred(true);
        }
        let ty = self.parse_type()?;
        let name = self.expect_ident("a variable name after 'mutable <type>'")?;
        self.expect(&TokenKind::Eq, "'=' after 'mutable <type> <name>'")?;
        let init = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "';' after variable declaration")?;
        Ok(Stmt::VarDecl {
            ty,
            name,
            init,
            mutable: true,
            span: sp,
        })
    }

    /// `{ stmt* }` — consumes both braces, returns the inner statements.
    fn parse_block(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut stmts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(&TokenKind::RBrace, "'}' to close block")?;
        Ok(stmts)
    }

    /// `return;` or `return expr;`
    fn parse_return(&mut self) -> Result<Stmt, Diagnostic> {
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

    /// `if (cond) BLOCK [else BLOCK | else if …]`
    fn parse_if(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::If, "'if'")?;
        self.expect(&TokenKind::LParen, "'(' after 'if'")?;
        // `if (var name = scrutinee)` binds the non-null inner of an optional scrutinee inside the
        // then-block (M3 S2.4). `var` is a keyword that cannot begin a normal condition expression,
        // so seeing it right after `(` unambiguously selects the if-let form.
        let bind = if self.eat(&TokenKind::Var) {
            let name = self.expect_ident("a binding name after 'var'")?;
            self.expect(&TokenKind::Eq, "'=' in 'if (var name = …)'")?;
            Some(name)
        } else {
            None
        };
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
        Ok(Stmt::If {
            cond,
            bind,
            then_block,
            else_block,
            span: sp,
        })
    }

    /// `for (Type name in iter) BLOCK` (for-in) **or** C-style `for (init; cond; step) BLOCK`. The
    /// two are disambiguated by scanning the header at paren/bracket-depth 0: whichever of `in` /
    /// `;` appears first decides (a for-in header has no `;`; a C-for header has no top-level `in`).
    fn parse_for(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::For, "'for'")?;
        self.expect(&TokenKind::LParen, "'(' after 'for'")?;
        if self.for_header_is_classic() {
            return self.parse_cfor_rest(sp);
        }
        let ty = self.parse_type()?;
        let name = self.expect_ident("a loop variable name")?;
        self.expect(&TokenKind::In, "'in' in for-loop header")?;
        let iter = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "')' after for-loop header")?;
        let body = self.parse_block()?;
        Ok(Stmt::For {
            ty,
            name,
            iter,
            body,
            span: sp,
        })
    }

    /// Scan the for-header tokens (from just after the opening `(`) at paren/bracket depth 0: a
    /// top-level `;` means a C-`for`, a top-level `in` means a for-`in`. Neither `;` nor `in`
    /// appears inside balanced `()`/`[]` of a well-formed header, so depth tracking is exact.
    fn for_header_is_classic(&self) -> bool {
        let mut depth: i32 = 0;
        let mut i = self.pos;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::LParen | TokenKind::LBracket => depth += 1,
                TokenKind::RParen | TokenKind::RBracket => {
                    if depth == 0 {
                        return false; // header's closing `)` — no `;`/`in` seen → treat as for-in
                    }
                    depth -= 1;
                }
                TokenKind::Semicolon if depth == 0 => return true,
                TokenKind::In if depth == 0 => return false,
                TokenKind::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Parse the rest of a C-`for` header (the opening `(` already consumed) and its body:
    /// `init; cond; step) BLOCK`. Each clause is optional. `init`/`step` are clause-statements
    /// (decl / assignment / expression, no trailing `;`); `cond` is an expression.
    fn parse_cfor_rest(&mut self, sp: Span) -> Result<Stmt, Diagnostic> {
        let init = if self.check(&TokenKind::Semicolon) {
            None
        } else {
            Some(Box::new(self.parse_for_clause_stmt()?))
        };
        self.expect(&TokenKind::Semicolon, "';' after for-loop init")?;
        let cond = if self.check(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(&TokenKind::Semicolon, "';' after for-loop condition")?;
        let step = if self.check(&TokenKind::RParen) {
            None
        } else {
            Some(Box::new(self.parse_for_clause_stmt()?))
        };
        self.expect(&TokenKind::RParen, "')' after for-loop step")?;
        let body = self.parse_block()?;
        Ok(Stmt::CFor {
            init,
            cond,
            step,
            body,
            span: sp,
        })
    }

    /// A C-`for` init/step clause: a `[mutable] [var|Type] name = expr` declaration, an
    /// assignment / compound-assignment / `++`/`--`, or a bare expression — **without** a trailing
    /// `;` (the header separator is consumed by the caller).
    fn parse_for_clause_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        if self.eat(&TokenKind::Mutable) {
            let (ty, name) = if self.eat(&TokenKind::Var) {
                (
                    Type::Infer(sp),
                    self.expect_ident("a variable name after 'mutable var'")?,
                )
            } else {
                let ty = self.parse_type()?;
                (
                    ty,
                    self.expect_ident("a variable name after 'mutable <type>'")?,
                )
            };
            self.expect(&TokenKind::Eq, "'=' in for-loop init")?;
            let init = self.parse_expr()?;
            return Ok(Stmt::VarDecl {
                ty,
                name,
                init,
                mutable: true,
                span: sp,
            });
        }
        if self.eat(&TokenKind::Var) {
            let name = self.expect_ident("a variable name after 'var'")?;
            self.expect(&TokenKind::Eq, "'=' after 'var <name>'")?;
            let init = self.parse_expr()?;
            return Ok(Stmt::VarDecl {
                ty: Type::Infer(sp),
                name,
                init,
                mutable: false,
                span: sp,
            });
        }
        if let Some((ty, name)) = self.try_var_decl_header() {
            let init = self.parse_expr()?;
            return Ok(Stmt::VarDecl {
                ty,
                name,
                init,
                mutable: false,
                span: sp,
            });
        }
        let expr = self.parse_expr()?;
        self.finish_assign_or_expr(expr, sp)
    }

    /// `while (cond) BLOCK` or while-let `while (var name = opt) BLOCK`. The while-let form is
    /// desugared here into `while (true) { if (var name = opt) { BODY } else { break; } }`, reusing
    /// the if-let lowering and `break` — so no backend learns a while-let-specific shape (M-mut.3).
    fn parse_while(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::While, "'while'")?;
        self.expect(&TokenKind::LParen, "'(' after 'while'")?;
        if self.eat(&TokenKind::Var) {
            let name = self.expect_ident("a binding name after 'var'")?;
            self.expect(&TokenKind::Eq, "'=' in 'while (var name = …)'")?;
            let cond = self.parse_expr()?;
            self.expect(&TokenKind::RParen, "')' after while condition")?;
            let body = self.parse_block()?;
            let if_let = Stmt::If {
                cond,
                bind: Some(name),
                then_block: body,
                else_block: Some(vec![Stmt::Break(sp)]),
                span: sp,
            };
            return Ok(Stmt::While {
                cond: Expr::Bool(true, sp),
                body: vec![if_let],
                post_cond: false,
                span: sp,
            });
        }
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "')' after while condition")?;
        let body = self.parse_block()?;
        Ok(Stmt::While {
            cond,
            body,
            post_cond: false,
            span: sp,
        })
    }

    /// `do BLOCK while (cond);` — the body runs once before the first test. No while-let form.
    fn parse_do_while(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::Do, "'do'")?;
        let body = self.parse_block()?;
        self.expect(&TokenKind::While, "'while' after 'do { … }'")?;
        self.expect(&TokenKind::LParen, "'(' after 'while'")?;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "')' after do-while condition")?;
        self.expect(&TokenKind::Semicolon, "';' after 'do { … } while (…)'")?;
        Ok(Stmt::While {
            cond,
            body,
            post_cond: true,
            span: sp,
        })
    }

    /// Disambiguate `Type name = expr;` (var-decl) from `expr;` (expression statement).
    /// A var-decl is committed only after a type, a name, and `=` parse successfully;
    /// anything short of that rewinds the cursor and re-parses as an expression.
    fn parse_var_decl_or_expr_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        if let Some((ty, name)) = self.try_var_decl_header() {
            let init = self.parse_expr()?;
            self.expect(&TokenKind::Semicolon, "';' after variable declaration")?;
            return Ok(Stmt::VarDecl {
                ty,
                name,
                init,
                mutable: false,
                span: sp,
            });
        }
        let expr = self.parse_expr()?;
        let stmt = self.finish_assign_or_expr(expr, sp)?;
        self.expect(&TokenKind::Semicolon, "';' after statement")?;
        Ok(stmt)
    }

    /// Given an already-parsed lvalue/expression, parse an optional assignment tail and return the
    /// resulting statement — a plain reassignment (`= e`), a compound assignment (`op= e` / `??=`,
    /// desugared to `x = x op e`, M-mut.2), a statement increment/decrement (`++`/`--`), or a bare
    /// `Stmt::Expr` if no tail follows. Does **not** consume a terminator, so it is shared by the
    /// statement parser (which then expects `;`) and the C-`for` clause parser (terminated by `;`
    /// or `)`). `/=`/`%=` inherit `__phorge_div`/`__phorge_rem` via `BinaryOp::Div`/`Rem` (F7).
    fn finish_assign_or_expr(&mut self, expr: Expr, sp: Span) -> Result<Stmt, Diagnostic> {
        if self.eat(&TokenKind::Eq) {
            let value = self.parse_expr()?;
            return Ok(Stmt::Assign {
                target: expr,
                value,
                span: sp,
            });
        }
        if let Some(op) = compound_op(self.peek()) {
            self.advance();
            let rhs = self.parse_expr()?;
            let value = Expr::Binary {
                op,
                lhs: Box::new(expr.clone()),
                rhs: Box::new(rhs),
                span: sp,
            };
            return Ok(Stmt::Assign {
                target: expr,
                value,
                span: sp,
            });
        }
        if matches!(self.peek(), TokenKind::PlusPlus | TokenKind::MinusMinus) {
            let op = if matches!(self.peek(), TokenKind::PlusPlus) {
                BinaryOp::Add
            } else {
                BinaryOp::Sub
            };
            self.advance();
            let value = Expr::Binary {
                op,
                lhs: Box::new(expr.clone()),
                rhs: Box::new(Expr::Int(1, sp)),
                span: sp,
            };
            return Ok(Stmt::Assign {
                target: expr,
                value,
                span: sp,
            });
        }
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

    /// Parse one top-level item: an optional visibility prefix (`public`/`internal`/`private`)
    /// followed by `import` / `function` / `enum` / `class` / `interface` / `type`. The prefix is
    /// stamped onto the declaration by the free `stamp_visibility`.
    pub fn parse_item(&mut self) -> Result<Item, Diagnostic> {
        let sp = self.peek_span();
        // Optional leading declaration visibility (visibility modifiers): at most one of
        // public/internal/private. Absent ⇒ the default `Visibility::Public`.
        let vis = self.parse_decl_visibility()?;
        // Optional `open`/`abstract` class prefixes (M-RT S6/S6b), in any order. Both apply only to a
        // class; `abstract` implies extensibility (an abstract class exists to be subclassed), so it
        // also marks the class `open`.
        let mut is_open = false;
        let mut is_abstract = false;
        loop {
            if self.eat(&TokenKind::Open) {
                is_open = true;
            } else if self.eat(&TokenKind::Abstract) {
                is_abstract = true;
            } else {
                break;
            }
        }
        if (is_open || is_abstract) && !self.check(&TokenKind::Class) {
            return Err(self.error("only a class can be declared `open` or `abstract`"));
        }
        let item = match self.peek() {
            TokenKind::Import => {
                if vis != Visibility::Public {
                    return Err(self.error("an import cannot carry a visibility modifier"));
                }
                return self.parse_import(sp);
            }
            TokenKind::TypeKw => {
                if vis != Visibility::Public {
                    return Err(self.error("a type alias cannot carry a visibility modifier yet"));
                }
                return self.parse_type_alias(sp);
            }
            TokenKind::Function => Item::Function(self.parse_function(Vec::new(), sp)?),
            TokenKind::Enum => Item::Enum(self.parse_enum(sp)?),
            TokenKind::Class => {
                Item::Class(self.parse_class(sp, is_open || is_abstract, is_abstract)?)
            }
            TokenKind::Interface => Item::Interface(self.parse_interface(sp)?),
            TokenKind::Trait => {
                if vis != Visibility::Public {
                    return Err(self.error("a trait cannot carry a visibility modifier yet"));
                }
                return Ok(Item::Trait(self.parse_trait(sp)?));
            }
            TokenKind::Package => {
                return Err(self.error(
                    "'package' must be the first declaration, before any import or definition",
                ))
            }
            _ => {
                return Err(self
                    .error("a top-level item (import, function, enum, class, interface, or type)"))
            }
        };
        Ok(stamp_visibility(item, vis))
    }

    /// Read an optional single leading declaration-visibility keyword. Two visibility keywords in a
    /// row (`public private`) is an error; absent ⇒ the default `Visibility::Public`.
    fn parse_decl_visibility(&mut self) -> Result<Visibility, Diagnostic> {
        let first = match self.peek() {
            TokenKind::Public => Visibility::Public,
            TokenKind::Internal => Visibility::Internal,
            TokenKind::Private => Visibility::Private,
            _ => return Ok(Visibility::Public),
        };
        self.advance();
        if matches!(
            self.peek(),
            TokenKind::Public | TokenKind::Internal | TokenKind::Private
        ) {
            return Err(self.error("a single visibility (public, internal, or private), not two"));
        }
        Ok(first)
    }

    /// Entry point: parse a whole program — an optional leading `package …;` (M5: required by the
    /// checker, but parsed optionally so its absence is a typed `E-NO-PACKAGE`, not a parse error)
    /// followed by zero or more top-level items until EOF.
    pub fn parse_program(&mut self) -> Result<Program, Diagnostic> {
        let sp = self.peek_span();
        let package = if self.check(&TokenKind::Package) {
            self.parse_package()?
        } else {
            Vec::new()
        };
        let mut items = Vec::new();
        while !self.check(&TokenKind::Eof) {
            items.push(self.parse_item()?);
        }
        Ok(Program {
            package,
            items,
            span: sp,
        })
    }

    /// `package a.b.c;` — dotted package path at the file top. Assumes current token is `package`.
    fn parse_package(&mut self) -> Result<Vec<String>, Diagnostic> {
        self.expect(&TokenKind::Package, "'package'")?;
        let mut path = vec![self.expect_ident("a package path segment")?];
        while self.eat(&TokenKind::Dot) {
            path.push(self.expect_ident("a package path segment after '.'")?);
        }
        self.expect(&TokenKind::Semicolon, "';' after package")?;
        Ok(path)
    }

    /// `import a.b.c;` / `import a.b.c as leaf;` — a module import (Go-qualified `c.fn()` calls).
    /// `import type a.b.C;` / `import type a.b.C as D;` — a *terminal type* import: the leaf `C` is a
    /// user/library type, bound bare (or as `D`). `type` and `as` are **contextual** keywords
    /// (recognized only here), so they stay valid identifiers elsewhere. Assumes current token is
    /// `import`.
    fn parse_import(&mut self, sp: Span) -> Result<Item, Diagnostic> {
        self.expect(&TokenKind::Import, "'import'")?;
        let type_only = self.eat(&TokenKind::TypeKw); // contextual `import type …`
        let mut path = vec![self.expect_ident("a module path segment")?];
        while self.eat(&TokenKind::Dot) {
            path.push(self.expect_ident("a module path segment after '.'")?);
        }
        let alias = if matches!(self.peek(), TokenKind::Ident(s) if s == "as") {
            self.advance(); // consume `as`
            Some(self.expect_ident("an alias after 'as'")?)
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon, "';' after import")?;
        Ok(Item::Import {
            path,
            alias,
            type_only,
            span: sp,
        })
    }

    /// `type Name = Type;` — a top-level alias. Assumes the current token is `type`.
    fn parse_type_alias(&mut self, sp: Span) -> Result<Item, Diagnostic> {
        self.expect(&TokenKind::TypeKw, "'type'")?;
        let name = self.expect_ident("an alias name after 'type'")?;
        self.expect(&TokenKind::Eq, "'=' in type alias")?;
        let ty = self.parse_type()?;
        self.expect(&TokenKind::Semicolon, "';' after type alias")?;
        Ok(Item::TypeAlias { name, ty, span: sp })
    }

    /// `function name(params) [-> RetType] BLOCK`. `modifiers` are pre-parsed by the caller
    /// (empty for a free function; populated for a method).
    fn parse_function(
        &mut self,
        modifiers: Vec<Modifier>,
        sp: Span,
    ) -> Result<FunctionDecl, Diagnostic> {
        self.expect(&TokenKind::Function, "'function'")?;
        let name = self.expect_ident("a function name")?;
        let type_params = self.parse_type_params()?;
        self.expect(&TokenKind::LParen, "'(' after function name")?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen, "')' to close parameters")?;
        let ret = if self.eat(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        // `throws T (| T)*` (M-faults 2b). `parse_type` consumes a `A | B` union natively, so one
        // call captures the whole declared set as a single (possibly `Union`) `Type`; the checker
        // flattens it. Empty when the clause is absent.
        let throws = if self.eat(&TokenKind::Throws) {
            vec![self.parse_type()?]
        } else {
            Vec::new()
        };
        // M-RT S6b: an `abstract` method is a bodyless signature terminated by `;` (a concrete
        // subclass supplies the body). Every other method/function parses a block.
        let body = if modifiers.contains(&Modifier::Abstract) {
            self.expect(
                &TokenKind::Semicolon,
                "';' after an abstract method signature",
            )?;
            Vec::new()
        } else {
            self.parse_block()?
        };
        Ok(FunctionDecl {
            modifiers,
            vis: Visibility::Public,
            name,
            type_params,
            params,
            ret,
            throws,
            body,
            span: sp,
        })
    }

    /// Optional generic parameter list `<T, U>` immediately after a function name (M-RT S7).
    /// Absent ⇒ empty vec. Both free functions and methods may be generic (M-RT generics-all);
    /// generic *interface* methods are still a non-parse because interface methods build their
    /// `FunctionDecl` directly with an empty `type_params` (no `<…>` is consumed there).
    fn parse_type_params(&mut self) -> Result<Vec<String>, Diagnostic> {
        if !self.check(&TokenKind::Lt) {
            return Ok(Vec::new());
        }
        self.advance(); // consume '<'
        let mut params = vec![self.expect_ident("a type parameter name")?];
        while self.eat(&TokenKind::Comma) {
            params.push(self.expect_ident("a type parameter name")?);
        }
        self.expect(&TokenKind::Gt, "'>' to close type parameters")?;
        Ok(params)
    }

    /// Comma-separated `Type name` parameters up to (not including) `)`.
    /// Allows zero params; allows a trailing comma.
    fn parse_params(&mut self) -> Result<Vec<Param>, Diagnostic> {
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

    /// `enum Name { Variant[(Type field, …)], … }` — assumes current token is `enum`.
    fn parse_enum(&mut self, sp: Span) -> Result<EnumDecl, Diagnostic> {
        self.expect(&TokenKind::Enum, "'enum'")?;
        let name = self.expect_ident("an enum name")?;
        // Optional generic parameter list `<T, E>` immediately after the enum name (M-RT generic
        // enums) — `enum Result<T, E> { Ok(T value), Err(E error) }`.
        let type_params = self.parse_type_params()?;
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
            variants.push(EnumVariant {
                name: vname,
                fields,
                span: vsp,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace, "'}' to close enum")?;
        Ok(EnumDecl {
            vis: Visibility::Public,
            name,
            type_params,
            variants,
            span: sp,
        })
    }

    /// `[open] class Name<T> [extends A, B] [implements I1, I2] { member* }` — assumes current token
    /// is `class`. The `open` flag is parsed at the item level (`parse_item`) and threaded in.
    fn parse_class(
        &mut self,
        sp: Span,
        open: bool,
        is_abstract: bool,
    ) -> Result<ClassDecl, Diagnostic> {
        self.expect(&TokenKind::Class, "'class'")?;
        let name = self.expect_ident("a class name")?;
        // Optional generic parameter list `<T, U>` immediately after the class name (M-RT
        // generics-all), before `extends`/`implements` — `class Box<T> extends … implements … { … }`.
        let type_params = self.parse_type_params()?;
        // Optional `extends A, B` parent-class list (M-RT S6) — before `implements`.
        let extends = if self.eat(&TokenKind::Extends) {
            self.parse_name_list("a class name after 'extends'")?
        } else {
            Vec::new()
        };
        let implements = if self.eat(&TokenKind::Implements) {
            self.parse_name_list("an interface name after 'implements'")?
        } else {
            Vec::new()
        };
        self.expect(&TokenKind::LBrace, "'{' to open class body")?;
        let mut members = Vec::new();
        let mut resolutions = Vec::new();
        let mut uses = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            // A leading contextual `use`/`rename`/`exclude` (lexed as identifiers, never reserved)
            // introduces a clause rather than a member. Types are PascalCase, so these lowercase
            // leaders are unambiguous in member position. M-RT S8 dot-lookahead: `use P.m` (a `.`
            // after the name) is an S6b resolution clause; `use T;` / `use A, B;` is trait composition.
            let leader = if let TokenKind::Ident(kw) = self.peek() {
                Some(kw.clone())
            } else {
                None
            };
            if let Some(kw) = leader {
                match kw.as_str() {
                    "use" => {
                        let is_resolution = matches!(
                            self.tokens.get(self.pos + 2).map(|t| &t.kind),
                            Some(&TokenKind::Dot)
                        );
                        if is_resolution {
                            resolutions.push(self.parse_resolution()?);
                        } else {
                            uses.extend(self.parse_use_traits()?);
                        }
                        continue;
                    }
                    "rename" | "exclude" => {
                        resolutions.push(self.parse_resolution()?);
                        continue;
                    }
                    _ => {}
                }
            }
            members.push(self.parse_class_member()?);
        }
        self.expect(&TokenKind::RBrace, "'}' to close class")?;
        Ok(ClassDecl {
            vis: Visibility::Public,
            name,
            type_params,
            extends,
            implements,
            open,
            is_abstract,
            resolutions,
            uses,
            members,
            span: sp,
        })
    }

    /// M-RT S8 trait composition: `use Name [, Name]* ;` → one or more [`crate::ast::UseTrait`].
    /// Assumes the current token is the contextual `use` keyword and the name is NOT dot-qualified
    /// (the caller disambiguated this from an S6b `use P.m` resolution clause via dot-lookahead).
    fn parse_use_traits(&mut self) -> Result<Vec<crate::ast::UseTrait>, Diagnostic> {
        self.expect_ident("'use'")?; // consume the contextual `use`
        let mut out = Vec::new();
        loop {
            let sp = self.peek_span();
            let name = self.expect_ident("a trait name after 'use'")?;
            out.push(crate::ast::UseTrait { name, span: sp });
            if self.eat(&TokenKind::Comma) {
                continue;
            }
            break;
        }
        self.expect(&TokenKind::Semicolon, "';' after a trait `use` clause")?;
        Ok(out)
    }

    /// `trait Name { members }` (M-RT S8) — assumes the current token is `trait`. Members use the exact
    /// class-member grammar (methods, fields, const, static, hooks, constructor, abstract requirements).
    /// A trait has no `extends`/`implements`/generics this slice.
    fn parse_trait(&mut self, sp: Span) -> Result<crate::ast::TraitDecl, Diagnostic> {
        self.expect(&TokenKind::Trait, "'trait'")?;
        let name = self.expect_ident("a trait name")?;
        self.expect(&TokenKind::LBrace, "'{' to open trait body")?;
        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            members.push(self.parse_class_member()?);
        }
        self.expect(&TokenKind::RBrace, "'}' to close trait")?;
        Ok(crate::ast::TraitDecl {
            name,
            members,
            span: sp,
        })
    }

    /// A multi-inheritance resolution clause (M-RT S6b): `use P.m` | `rename P.m as n` | `exclude P.m`,
    /// with an optional trailing `;`. Assumes the current token is the contextual keyword.
    fn parse_resolution(&mut self) -> Result<crate::ast::Resolution, Diagnostic> {
        let sp = self.peek_span();
        let kw = self.expect_ident("a resolution clause keyword")?;
        let parent = self.expect_ident("a parent class name")?;
        self.expect(&TokenKind::Dot, "'.' between the parent and method")?;
        let method = self.expect_ident("a method name")?;
        let res = match kw.as_str() {
            "use" => crate::ast::Resolution::Use {
                parent,
                method,
                span: sp,
            },
            "exclude" => crate::ast::Resolution::Exclude {
                parent,
                method,
                span: sp,
            },
            "rename" => {
                let as_kw = self.expect_ident("'as' in a rename clause")?;
                if as_kw != "as" {
                    return Err(self.error("'as' after 'rename P.m'"));
                }
                let as_name = self.expect_ident("the new method name after 'as'")?;
                crate::ast::Resolution::Rename {
                    parent,
                    method,
                    as_name,
                    span: sp,
                }
            }
            _ => unreachable!("caller gated the keyword"),
        };
        // Optional terminator.
        if self.check(&TokenKind::Semicolon) {
            self.advance();
        }
        Ok(res)
    }

    /// `interface Name [extends A, B] { (function sig;)* }` — assumes current token is `interface`.
    /// Each member is a method *signature*: `function name(params) [-> Ret];` with no body, stored as
    /// a `FunctionDecl` whose body is empty (M-RT S2).
    fn parse_interface(&mut self, sp: Span) -> Result<crate::ast::InterfaceDecl, Diagnostic> {
        self.expect(&TokenKind::Interface, "'interface'")?;
        let name = self.expect_ident("an interface name")?;
        let extends = if self.eat(&TokenKind::Extends) {
            self.parse_name_list("an interface name after 'extends'")?
        } else {
            Vec::new()
        };
        self.expect(&TokenKind::LBrace, "'{' to open interface body")?;
        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let msp = self.peek_span();
            self.expect(
                &TokenKind::Function,
                "'function' for an interface method signature",
            )?;
            let mname = self.expect_ident("a method name")?;
            self.expect(&TokenKind::LParen, "'(' after method name")?;
            let params = self.parse_params()?;
            self.expect(&TokenKind::RParen, "')' to close parameters")?;
            let ret = if self.eat(&TokenKind::Arrow) {
                Some(self.parse_type()?)
            } else {
                None
            };
            let throws = if self.eat(&TokenKind::Throws) {
                vec![self.parse_type()?]
            } else {
                Vec::new()
            };
            self.expect(
                &TokenKind::Semicolon,
                "';' after an interface method signature",
            )?;
            methods.push(FunctionDecl {
                modifiers: Vec::new(),
                vis: Visibility::Public,
                name: mname,
                type_params: Vec::new(),
                params,
                ret,
                throws,
                body: Vec::new(),
                span: msp,
            });
        }
        self.expect(&TokenKind::RBrace, "'}' to close interface")?;
        Ok(crate::ast::InterfaceDecl {
            vis: Visibility::Public,
            name,
            extends,
            methods,
            span: sp,
        })
    }

    /// A comma-separated list of one-or-more identifiers (no trailing comma), used for a class's
    /// `implements` list and an interface's `extends` list.
    fn parse_name_list(&mut self, what: &str) -> Result<Vec<String>, Diagnostic> {
        let mut names = vec![self.expect_ident(what)?];
        while self.eat(&TokenKind::Comma) {
            names.push(self.expect_ident(what)?);
        }
        Ok(names)
    }

    /// One class member: a field, a constructor, or a method. Modifiers preceding
    /// `constructor` are consumed and dropped (M1: constructors are implicitly public).
    fn parse_class_member(&mut self) -> Result<ClassMember, Diagnostic> {
        let sp = self.peek_span();
        let modifiers = self.parse_modifiers();
        match self.peek() {
            TokenKind::Constructor => {
                self.advance();
                self.expect(&TokenKind::LParen, "'(' after 'constructor'")?;
                let params = self.parse_ctor_params()?;
                self.expect(&TokenKind::RParen, "')' to close constructor parameters")?;
                let body = self.parse_block()?;
                Ok(ClassMember::Constructor {
                    params,
                    body,
                    span: sp,
                })
            }
            TokenKind::Function => Ok(ClassMember::Method(self.parse_function(modifiers, sp)?)),
            _ => {
                // field or property hook: [modifiers] Type name …
                let ty = self.parse_type()?;
                let name = self.expect_ident("a field name")?;
                // A `{` after the name opens a **property hook** body (M-mut.7b):
                // `Type name { get => expr; set(Type v) { stmts } }`. Anything else is a field. A
                // hook is virtual behavior, not storage, so it carries no modifiers (`mutable`/
                // `static` would describe a backing slot it doesn't have).
                if self.check(&TokenKind::LBrace) {
                    if !modifiers.is_empty() {
                        return Err(self.error("a property hook to carry no modifiers"));
                    }
                    return self.parse_property_hook(ty, name, sp);
                }
                // field: [modifiers] Type name [= init] ;
                // An optional field-level initializer (`static mutable int total = 0;`). The checker
                // requires it for `static` fields and forbids it on instance fields (M-mut.7).
                let init = if self.check(&TokenKind::Eq) {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(&TokenKind::Semicolon, "';' after field declaration")?;
                Ok(ClassMember::Field {
                    modifiers,
                    ty,
                    name,
                    init,
                    span: sp,
                })
            }
        }
    }

    /// A property hook body (M-mut.7b): `{ get => expr; [set(Type v) { stmts }] }` — clauses in
    /// either order, each at most once, at least one required. Assumes the current token is `{`.
    fn parse_property_hook(
        &mut self,
        ty: Type,
        name: String,
        sp: Span,
    ) -> Result<ClassMember, Diagnostic> {
        self.expect(&TokenKind::LBrace, "'{' to open a property hook body")?;
        let mut get: Option<Expr> = None;
        let mut set: Option<(Param, Vec<Stmt>)> = None;
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let clause = self.expect_ident("`get` or `set`")?;
            match clause.as_str() {
                "get" => {
                    if get.is_some() {
                        return Err(self.error("a single `get` clause"));
                    }
                    // `get => expr ;`
                    self.expect(&TokenKind::FatArrow, "'=>' after `get`")?;
                    let body = self.parse_expr()?;
                    self.expect(&TokenKind::Semicolon, "';' after the `get` expression")?;
                    get = Some(body);
                }
                "set" => {
                    if set.is_some() {
                        return Err(self.error("a single `set` clause"));
                    }
                    // `set(Type v) { stmts }`
                    self.expect(&TokenKind::LParen, "'(' after `set`")?;
                    let params = self.parse_params()?;
                    self.expect(&TokenKind::RParen, "')' to close the `set` parameter")?;
                    if params.len() != 1 {
                        return Err(self.error("exactly one `set` parameter `set(Type v)`"));
                    }
                    let body = self.parse_block()?;
                    set = Some((params.into_iter().next().unwrap(), body));
                }
                _ => return Err(self.error("`get` or `set` in a property hook")),
            }
        }
        self.expect(&TokenKind::RBrace, "'}' to close the property hook body")?;
        if get.is_none() && set.is_none() {
            return Err(self.error("at least a `get` or `set` clause in the property hook"));
        }
        Ok(ClassMember::Hook {
            ty,
            name,
            get,
            set,
            span: sp,
        })
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
                // `open` method — opts into override (M-RT S6); final-by-default otherwise.
                TokenKind::Open => Modifier::Open,
                // `mutable` field / promoted ctor param (M-mut.6); immutable by default.
                TokenKind::Mutable => Modifier::Mutable,
                // `static` class field (M-mut.7) — class-level state.
                TokenKind::Static => Modifier::Static,
                // `abstract` method (M-RT S6b) — bodyless, implicitly `open`.
                TokenKind::Abstract => Modifier::Abstract,
                _ => break,
            };
            self.advance();
            mods.push(m);
        }
        mods
    }

    /// Constructor parameters: like normal params, but each may carry promotion modifiers
    /// (`constructor(private string name)`). Allows zero; allows a trailing comma.
    fn parse_ctor_params(&mut self) -> Result<Vec<CtorParam>, Diagnostic> {
        let mut params = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok(params);
        }
        loop {
            let sp = self.peek_span();
            let modifiers = self.parse_modifiers();
            let ty = self.parse_type()?;
            let name = self.expect_ident("a parameter name")?;
            params.push(CtorParam {
                modifiers,
                ty,
                name,
                span: sp,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
            if self.check(&TokenKind::RParen) {
                break; // trailing comma
            }
        }
        Ok(params)
    }
}

/// Map a compound-assignment operator token to the `BinaryOp` it desugars to (M-mut.2).
/// `x op= e` lowers to `x = x op e`; `??=` lowers to `x = x ?? e` (`Coalesce`). Returns `None`
/// for any non-compound token so the caller falls through to a plain expression statement.
fn compound_op(k: &TokenKind) -> Option<BinaryOp> {
    Some(match k {
        TokenKind::PlusEq => BinaryOp::Add,
        TokenKind::MinusEq => BinaryOp::Sub,
        TokenKind::StarEq => BinaryOp::Mul,
        TokenKind::SlashEq => BinaryOp::Div,
        TokenKind::PercentEq => BinaryOp::Rem,
        TokenKind::QuestionQuestionEq => BinaryOp::Coalesce,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
