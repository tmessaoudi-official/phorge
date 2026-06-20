//! Recursive-descent + Pratt parser: turns the lexer's token stream into the AST.

use crate::ast::{
    BinaryOp, ClassDecl, ClassMember, CtorParam, EnumDecl, EnumVariant, Expr, FunctionDecl, Item,
    LambdaBody, MatchArm, Modifier, Param, Pattern, Program, Stmt, StrPart, Type, UnaryOp,
};
use crate::diagnostic::{Diagnostic, Stage};
use crate::limits::MAX_NEST_DEPTH;
use crate::token::{Span, Token, TokenKind};

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

    /// Parse a type annotation: `Name`, `Name<T, U>`, `T?`, or `(T, U) -> R`.
    pub fn parse_type(&mut self) -> Result<Type, Diagnostic> {
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
            TokenKind::LBrace => {
                let sp = self.peek_span();
                let body = self.parse_block()?;
                Ok(Stmt::Block(body, sp))
            }
            TokenKind::Var => self.parse_var_inferred(),
            _ => self.parse_var_decl_or_expr_stmt(),
        }
    }

    /// `var name = expr;` — the binding type is inferred from `expr` by the checker.
    fn parse_var_inferred(&mut self) -> Result<Stmt, Diagnostic> {
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

    /// `for (Type name in iter) BLOCK`
    fn parse_for(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::For, "'for'")?;
        self.expect(&TokenKind::LParen, "'(' after 'for'")?;
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
                span: sp,
            });
        }
        let expr = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "';' after expression statement")?;
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

    /// Parse one top-level item: `import` / `function` / `enum` / `class`.
    pub fn parse_item(&mut self) -> Result<Item, Diagnostic> {
        let sp = self.peek_span();
        match self.peek() {
            TokenKind::Import => self.parse_import(sp),
            TokenKind::Function => Ok(Item::Function(self.parse_function(Vec::new(), sp)?)),
            TokenKind::Enum => Ok(Item::Enum(self.parse_enum(sp)?)),
            TokenKind::Class => Ok(Item::Class(self.parse_class(sp)?)),
            TokenKind::Interface => Ok(Item::Interface(self.parse_interface(sp)?)),
            TokenKind::TypeKw => self.parse_type_alias(sp),
            TokenKind::Package => Err(self
                .error("'package' must be the first declaration, before any import or definition")),
            _ => {
                Err(self
                    .error("a top-level item (import, function, enum, class, interface, or type)"))
            }
        }
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

    /// `import a.b.c;` or `import a.b.c as leaf;` — dotted module path with an optional alias. `as`
    /// is a **contextual** keyword (recognized only here), so it stays a valid identifier elsewhere.
    /// Assumes current token is `import`.
    fn parse_import(&mut self, sp: Span) -> Result<Item, Diagnostic> {
        self.expect(&TokenKind::Import, "'import'")?;
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
        self.expect(&TokenKind::LParen, "'(' after function name")?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen, "')' to close parameters")?;
        let ret = if self.eat(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        Ok(FunctionDecl {
            modifiers,
            name,
            params,
            ret,
            body,
            span: sp,
        })
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
            name,
            variants,
            span: sp,
        })
    }

    /// `class Name [implements A, B] { member* }` — assumes current token is `class`.
    fn parse_class(&mut self, sp: Span) -> Result<ClassDecl, Diagnostic> {
        self.expect(&TokenKind::Class, "'class'")?;
        let name = self.expect_ident("a class name")?;
        let implements = if self.eat(&TokenKind::Implements) {
            self.parse_name_list("an interface name after 'implements'")?
        } else {
            Vec::new()
        };
        self.expect(&TokenKind::LBrace, "'{' to open class body")?;
        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            members.push(self.parse_class_member()?);
        }
        self.expect(&TokenKind::RBrace, "'}' to close class")?;
        Ok(ClassDecl {
            name,
            implements,
            members,
            span: sp,
        })
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
            self.expect(
                &TokenKind::Semicolon,
                "';' after an interface method signature",
            )?;
            methods.push(FunctionDecl {
                modifiers: Vec::new(),
                name: mname,
                params,
                ret,
                body: Vec::new(),
                span: msp,
            });
        }
        self.expect(&TokenKind::RBrace, "'}' to close interface")?;
        Ok(crate::ast::InterfaceDecl {
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
                // field: [modifiers] Type name ;
                let ty = self.parse_type()?;
                let name = self.expect_ident("a field name")?;
                self.expect(&TokenKind::Semicolon, "';' after field declaration")?;
                Ok(ClassMember::Field {
                    modifiers,
                    ty,
                    name,
                    span: sp,
                })
            }
        }
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
                TokenKind::Final => Modifier::Final,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{ClassMember, Expr, Item, Modifier, Pattern, Stmt, StrPart, Type};
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

    /// Helper: parse `src` as a single statement.
    fn stmt(src: &str) -> Stmt {
        parser(src).parse_stmt().expect("parse ok")
    }

    /// Helper: parse `src` as a top-level item.
    fn item(src: &str) -> Item {
        parser(src).parse_item().expect("parse ok")
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
                    BinaryOp::Lt => "<",
                    BinaryOp::Gt => ">",
                    BinaryOp::Le => "<=",
                    BinaryOp::Ge => ">=",
                    BinaryOp::And => "&&",
                    BinaryOp::Or => "||",
                    BinaryOp::Pipe => "|>",
                    BinaryOp::Coalesce => "??",
                };
                format!("({o} {} {})", sexpr(lhs), sexpr(rhs))
            }
            Expr::Member {
                object, name, safe, ..
            } => format!(
                "{}{}{}",
                sexpr(object),
                if *safe { "?." } else { "." },
                name
            ),
            Expr::Call { callee, args, .. } => {
                let a: Vec<String> = args.iter().map(sexpr).collect();
                format!("{}({})", sexpr(callee), a.join(", "))
            }
            Expr::Index { object, index, .. } => format!("{}[{}]", sexpr(object), sexpr(index)),
            Expr::Lambda { params, body, .. } => {
                let ps: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
                let body_str = match body {
                    LambdaBody::Expr(e) => sexpr(e),
                    LambdaBody::Block(_) => "<block>".into(),
                };
                format!("(lambda ({}) {})", ps.join(" "), body_str)
            }
            Expr::InstanceOf {
                value, type_name, ..
            } => format!("(instanceof {} {type_name})", sexpr(value)),
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
        assert_eq!(sexpr(&expr("x |> f")), "f(x)");
        // pipe is the lowest: `a + b |> f` == `(a + b) |> f`
        assert_eq!(sexpr(&expr("a + b |> f")), "f((+ a b))");
        assert_eq!(sexpr(&expr("a instanceof Foo")), "(instanceof a Foo)");
        assert_eq!(sexpr(&expr("a ?? b")), "(?? a b)");
        // `??` binds looser than `||`: `a || b ?? c` is `(a || b) ?? c`
        assert_eq!(sexpr(&expr("a || b ?? c")), "(?? (|| a b) c)");
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
    fn parses_safe_member_access() {
        // `?.` parses as a *safe* Member; plain `.` stays unsafe. `sexpr` renders the distinction.
        assert_eq!(sexpr(&expr("a?.b")), "a?.b");
        assert_eq!(sexpr(&expr("a.b")), "a.b");
        // chained safe access stays right-extending
        assert_eq!(sexpr(&expr("a?.b?.c")), "a?.b?.c");
        // a safe method call is a `Call` whose callee is a safe `Member`
        assert_eq!(sexpr(&expr("a?.m(x)")), "a?.m(x)");
        match expr("a?.b") {
            Expr::Member { name, safe, .. } => {
                assert_eq!(name, "b");
                assert!(safe, "`?.` must set safe = true");
            }
            other => panic!("got {other:?}"),
        }
        match expr("a.b") {
            Expr::Member { safe, .. } => assert!(!safe, "`.` must set safe = false"),
            other => panic!("got {other:?}"),
        }
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
                assert!(
                    matches!(&parts[1], StrPart::Expr(b) if matches!(**b, Expr::Ident(ref n,_) if n == "name"))
                );
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
            Pattern::Variant { fields, .. } => {
                assert!(matches!(&fields[0], Pattern::Variant { .. }))
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_match_expression() {
        let e = expr("match s { Circle(r) => r, Rect(w, h) => w, _ => 0 }");
        match e {
            Expr::Match {
                scrutinee, arms, ..
            } => {
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

    #[test]
    fn parses_ranges() {
        match expr("0..3") {
            Expr::Range { inclusive, .. } => assert!(!inclusive),
            other => panic!("got {other:?}"),
        }
        match expr("1..=n") {
            Expr::Range { inclusive, .. } => assert!(inclusive),
            other => panic!("got {other:?}"),
        }
        // ranges bind looser than `+`: `0..n + 1` is `0..(n + 1)`
        match expr("0..n + 1") {
            Expr::Range { end, .. } => assert!(matches!(*end, Expr::Binary { .. })),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_expression_if() {
        match expr("if (true) { 1 } else { 2 }") {
            Expr::If { .. } => {}
            other => panic!("got {other:?}"),
        }
        // a missing else is a parse error in expression position
        let mut p = parser("if (true) { 1 }");
        assert!(p.parse_expr().is_err());
    }

    #[test]
    fn parses_return_stmt() {
        assert!(matches!(stmt("return;"), Stmt::Return { value: None, .. }));
        match stmt("return 1 + 2;") {
            Stmt::Return {
                value: Some(Expr::Binary { .. }),
                ..
            } => {}
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_expr_stmt() {
        match stmt("console.println(x);") {
            Stmt::Expr(Expr::Call { .. }, _) => {}
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_block_stmt() {
        match stmt("{ return; return 1; }") {
            Stmt::Block(body, _) => assert_eq!(body.len(), 2),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_var_decl_stmt() {
        match stmt("int n = 5;") {
            Stmt::VarDecl { ty, name, init, .. } => {
                assert!(matches!(ty, Type::Named { ref name, .. } if name == "int"));
                assert_eq!(name, "n");
                assert!(matches!(init, Expr::Int(5, _)));
            }
            other => panic!("got {other:?}"),
        }
        // generic-typed var-decl must not be mistaken for comparison
        match stmt("List<Shape> shapes = items;") {
            Stmt::VarDecl { name, .. } => assert_eq!(name, "shapes"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_if_else() {
        match stmt("if (a) { return 1; } else { return 2; }") {
            Stmt::If {
                then_block,
                else_block: Some(eb),
                ..
            } => {
                assert_eq!(then_block.len(), 1);
                assert_eq!(eb.len(), 1);
            }
            other => panic!("got {other:?}"),
        }
        match stmt("if (a) { return 1; }") {
            Stmt::If {
                else_block: None, ..
            } => {}
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_else_if_chain() {
        match stmt("if (a) { return 1; } else if (b) { return 2; }") {
            Stmt::If {
                else_block: Some(eb),
                ..
            } => {
                assert_eq!(eb.len(), 1);
                assert!(matches!(eb[0], Stmt::If { .. }));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_if_let_binding() {
        // `if (var x = e)` carries the bound name; the condition expr is the scrutinee.
        match stmt("if (var x = o) { return 1; } else { return 2; }") {
            Stmt::If {
                bind: Some(name),
                else_block: Some(eb),
                ..
            } => {
                assert_eq!(name, "x");
                assert_eq!(eb.len(), 1);
            }
            other => panic!("got {other:?}"),
        }
        // a plain condition has no binding
        match stmt("if (a) { return 1; }") {
            Stmt::If { bind: None, .. } => {}
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_force_unwrap() {
        // postfix `!` is a force-unwrap; prefix `!` stays a logical-not unary
        match expr("o!") {
            Expr::Force { .. } => {}
            other => panic!("got {other:?}"),
        }
        match expr("!b") {
            Expr::Unary {
                op: UnaryOp::Not, ..
            } => {}
            other => panic!("got {other:?}"),
        }
        // `a != b` must remain a single NotEq comparison, never `a` `!` `= b`
        match expr("a != b") {
            Expr::Binary {
                op: BinaryOp::NotEq,
                ..
            } => {}
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_for_in() {
        match stmt("for (Shape s in shapes) { console.println(s); }") {
            Stmt::For {
                ty,
                name,
                iter,
                body,
                ..
            } => {
                assert!(matches!(ty, Type::Named { ref name, .. } if name == "Shape"));
                assert_eq!(name, "s");
                assert!(matches!(iter, Expr::Ident(ref n, _) if n == "shapes"));
                assert_eq!(body.len(), 1);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_function_decl() {
        match item("function area(Shape s) -> float { return s; }") {
            Item::Function(f) => {
                assert_eq!(f.name, "area");
                assert_eq!(f.params.len(), 1);
                assert_eq!(f.params[0].name, "s");
                assert!(f.ret.is_some());
                assert_eq!(f.body.len(), 1);
                assert!(f.modifiers.is_empty());
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_function_no_ret_no_params() {
        match item("function main() { console.println(1); }") {
            Item::Function(f) => {
                assert_eq!(f.name, "main");
                assert!(f.params.is_empty());
                assert!(f.ret.is_none());
                assert_eq!(f.body.len(), 1);
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_enum_decl() {
        let src = "enum Shape { Circle(float radius), Rect(float w, float h), Unit, }";
        match item(src) {
            Item::Enum(e) => {
                assert_eq!(e.name, "Shape");
                assert_eq!(e.variants.len(), 3);
                assert_eq!(e.variants[0].name, "Circle");
                assert_eq!(e.variants[0].fields.len(), 1);
                assert_eq!(e.variants[1].fields.len(), 2);
                assert!(e.variants[2].fields.is_empty()); // bare variant
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_class_decl() {
        let src = "class Greeter { \
                     private string name; \
                     constructor(private string name) {} \
                     function greet() -> string { return name; } \
                   }";
        match item(src) {
            Item::Class(c) => {
                assert_eq!(c.name, "Greeter");
                assert_eq!(c.members.len(), 3);
                match &c.members[0] {
                    ClassMember::Field {
                        modifiers, name, ..
                    } => {
                        assert_eq!(name, "name");
                        assert_eq!(modifiers, &vec![Modifier::Private]);
                    }
                    other => panic!("member 0: {other:?}"),
                }
                match &c.members[1] {
                    ClassMember::Constructor { params, .. } => {
                        assert_eq!(params.len(), 1);
                        assert_eq!(params[0].modifiers, vec![Modifier::Private]);
                        assert_eq!(params[0].name, "name");
                    }
                    other => panic!("member 1: {other:?}"),
                }
                match &c.members[2] {
                    ClassMember::Method(f) => assert_eq!(f.name, "greet"),
                    other => panic!("member 2: {other:?}"),
                }
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_class_implements_list() {
        // M-RT S2: `implements A, B` is parsed into ClassDecl.implements.
        match item(
            "class Dog implements Speaker, Pet { function speak() -> string { return \"w\"; } }",
        ) {
            Item::Class(c) => {
                assert_eq!(c.name, "Dog");
                assert_eq!(c.implements, vec!["Speaker".to_string(), "Pet".to_string()]);
                assert_eq!(c.members.len(), 1);
            }
            other => panic!("got {other:?}"),
        }
        // No `implements` ⇒ empty list.
        match item("class Plain {}") {
            Item::Class(c) => assert!(c.implements.is_empty()),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_interface_decl() {
        // M-RT S2: an interface is method signatures (no bodies) + an optional `extends` list.
        match item("interface Pet extends Speaker, Named { function speak() -> string; function age() -> int; }") {
            Item::Interface(i) => {
                assert_eq!(i.name, "Pet");
                assert_eq!(i.extends, vec!["Speaker".to_string(), "Named".to_string()]);
                assert_eq!(i.methods.len(), 2);
                assert_eq!(i.methods[0].name, "speak");
                assert!(i.methods[0].body.is_empty(), "signature has no body");
                assert_eq!(i.methods[1].name, "age");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_import() {
        match item("import core.console;") {
            Item::Import { path, .. } => assert_eq!(path, vec!["core", "console"]),
            other => panic!("got {other:?}"),
        }
        match item("import a;") {
            Item::Import { path, .. } => assert_eq!(path, vec!["a"]),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_package_declaration() {
        // `package a.b;` is captured on the Program, not as an Item (M5 S1).
        let prog = parser("package app.util; function main() {}")
            .parse_program()
            .expect("parse ok");
        assert_eq!(prog.package, vec!["app".to_string(), "util".to_string()]);
        // A bare file parses with an empty package — the checker, not the parser, enforces presence.
        let bare = parser("function main() {}")
            .parse_program()
            .expect("parse ok");
        assert!(bare.package.is_empty());
        // `package` after another item is a parse error (it must be the first declaration).
        assert!(parser("function main() {} package app;")
            .parse_program()
            .is_err());
    }

    #[test]
    fn parses_program_multiple_items() {
        let src = "import core.console; enum E { A, } function main() { return; }";
        let prog = parser(src).parse_program().expect("parse ok");
        assert_eq!(prog.items.len(), 3);
        assert!(matches!(prog.items[0], Item::Import { .. }));
        assert!(matches!(prog.items[1], Item::Enum(_)));
        assert!(matches!(prog.items[2], Item::Function(_)));
    }

    #[test]
    fn empty_program_parses() {
        let prog = parser("").parse_program().expect("parse ok");
        assert!(prog.items.is_empty());
    }

    #[test]
    fn parses_function_type_annotation() {
        // a function-typed parameter must parse
        let result =
            parser("package main; function apply(int x, (int) -> int f) -> int { return x; }")
                .parse_program();
        assert!(
            result.is_ok(),
            "function-typed param should parse: {result:?}"
        );
        // nested + zero-arg
        let result2 = parser("package main; function f() -> () -> int { }").parse_program();
        assert!(
            result2.is_ok(),
            "zero-arg function type should parse: {result2:?}"
        );
        // direct type parsing
        match ty("(int) -> int") {
            Type::Function { params, ret, .. } => {
                assert_eq!(params.len(), 1);
                assert!(matches!(ret.as_ref(), Type::Named { name, .. } if name == "int"));
            }
            other => panic!("expected Type::Function, got {other:?}"),
        }
    }
}
