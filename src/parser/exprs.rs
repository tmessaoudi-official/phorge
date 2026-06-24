//! Recursive-descent parser — exprs (M-Decomp W3.1). See parser/mod.rs for the struct + token-stream primitives.

use super::*;

impl Parser {
    /// Entry point: parse a full expression (lowest precedence).
    pub fn parse_expr(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_range()
    }

    /// Ranges bind looser than every binary operator: `a..b` reads `a` and `b` as full
    /// (binary) sub-expressions, so `0..n + 1` is `0..(n + 1)`. Non-chaining (no `a..b..c`); a
    /// single optional `..`/`..=` follows the first operand. Used mainly as `for (int i in 0..n)`.
    pub(super) fn parse_range(&mut self) -> Result<Expr, Diagnostic> {
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
    pub(super) fn infix_op(kind: &TokenKind) -> Option<(u8, BinaryOp)> {
        use TokenKind as T;
        // Precedence follows PHP (higher binds tighter): `|>` `??` `||` `&&` then bitwise
        // `|` `^` `&`, then `==`/`!=`, comparison, shifts, `+ -`, `* / %`. Shift-right `>>` is not a
        // token (two `Gt`); it is handled at level 10 directly in `parse_binary`.
        Some(match kind {
            T::Pipe => (1, BinaryOp::Pipe),
            T::QuestionQuestion => (2, BinaryOp::Coalesce),
            T::OrOr => (3, BinaryOp::Or),
            T::AndAnd => (4, BinaryOp::And),
            T::Bar => (5, BinaryOp::BitOr),
            T::Caret => (6, BinaryOp::BitXor),
            T::Amp => (7, BinaryOp::BitAnd),
            T::EqEq => (8, BinaryOp::Eq),
            T::NotEq => (8, BinaryOp::NotEq),
            T::Lt => (9, BinaryOp::Lt),
            T::Gt => (9, BinaryOp::Gt),
            T::Le => (9, BinaryOp::Le),
            T::Ge => (9, BinaryOp::Ge),
            T::Shl => (10, BinaryOp::Shl),
            T::Plus => (11, BinaryOp::Add),
            T::Minus => (11, BinaryOp::Sub),
            T::Star => (12, BinaryOp::Mul),
            T::Slash => (12, BinaryOp::Div),
            T::Percent => (12, BinaryOp::Rem),
            // `**` power binds tighter than `* / %` and is **right-associative** (PHP-identical):
            // `2 ** 3 ** 2` is `2 ** (3 ** 2)`. Right-assoc is applied in `parse_binary`.
            T::StarStar => (13, BinaryOp::Pow),
            _ => return None,
        })
    }

    /// Precedence-climbing: parse a unary, then fold infix operators whose
    /// binding power is >= `min_bp`. All our binary operators are left-associative,
    /// so the right operand is parsed with `bp + 1`.
    pub(super) fn parse_binary(&mut self, min_bp: u8) -> Result<Expr, Diagnostic> {
        let mut lhs = self.parse_unary()?;
        loop {
            // `instanceof` is a type test at precedence 8 (like `==`), but its right operand is a
            // *type name*, not an expression — so it is parsed here rather than via `infix_op`. The
            // left operand and result type (`bool`) are validated by the checker (M-RT S1).
            if matches!(self.peek(), TokenKind::Instanceof) && 8 >= min_bp {
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
            // Shift-right `>>` is two adjacent `Gt` tokens (never a single token — that protects
            // nested generics `List<List<int>>`). In expression position two consecutive `Gt` can
            // only be `>>`; a single `>` falls through to `infix_op` as comparison. Level 10.
            if matches!(self.peek(), TokenKind::Gt)
                && matches!(self.peek2(), TokenKind::Gt)
                && 10 >= min_bp
            {
                let sp = self.peek_span();
                self.advance(); // first `>`
                self.advance(); // second `>`
                let rhs = self.parse_binary(10 + 1)?;
                lhs = Expr::Binary {
                    op: BinaryOp::Shr,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
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
                            // All binary operators are left-associative (`bp + 1`) except `**`, which is
                            // right-associative (`bp`): `2 ** 3 ** 2` parses as `2 ** (3 ** 2)`, PHP-identical.
            let right_bp = if matches!(op, BinaryOp::Pow) {
                bp
            } else {
                bp + 1
            };
            let rhs = self.parse_binary(right_bp)?;
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
    pub(super) fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
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
            TokenKind::Tilde => Some(UnaryOp::BitNot),
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
    pub(super) fn parse_postfix(&mut self) -> Result<Expr, Diagnostic> {
        // Feature C: `new <Name>(<args>)` — the mandatory construction keyword. Parse exactly the
        // construction call (a primary callee + its argument list) and wrap it in `Expr::New`; the
        // postfix loop below then applies any `.`/`[]`/`!`/`?`/`with` to the constructed value (so
        // `new C().m()` is `(new C()).m()`). A bare `new` not followed by a call is a parse error.
        let mut e = if matches!(self.peek(), TokenKind::New) {
            let sp = self.peek_span();
            self.advance();
            let callee = self.parse_primary()?;
            self.expect(
                &TokenKind::LParen,
                "'(' — `new` must be followed by a constructor call, e.g. `new Counter()`",
            )?;
            let args = self.parse_arg_list()?;
            self.expect(&TokenKind::RParen, "')' to close arguments")?;
            let call = Expr::Call {
                callee: Box::new(callee),
                args,
                span: sp,
            };
            Expr::New(Box::new(call), sp)
        } else {
            self.parse_primary()?
        };
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
    pub(super) fn parse_arg_list(&mut self) -> Result<Vec<Expr>, Diagnostic> {
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
    pub(super) fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
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
            TokenKind::Str(segs) => {
                self.advance();
                let parts = self.segments_to_parts(segs, sp)?;
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

    /// Turn the lexer's pre-split string segments into `StrPart`s: a `Lit` becomes a literal run
    /// (escapes, incl. `\{`, already expanded by the lexer); an `Interp` carries raw expression
    /// source that is re-lexed + parsed here. An all-empty input yields a single empty literal. This
    /// is the `Str` path; `html"…"` still uses [`Self::split_interpolation`] on its flat body.
    pub(super) fn segments_to_parts(
        &self,
        segs: Vec<crate::token::StrSeg>,
        sp: Span,
    ) -> Result<Vec<StrPart>, Diagnostic> {
        use crate::token::StrSeg;
        let mut parts = Vec::new();
        for seg in segs {
            match seg {
                StrSeg::Lit(s) => parts.push(StrPart::Literal(s)),
                StrSeg::Interp(src, base) => {
                    let mut sub_tokens = crate::lexer::lex(&src).map_err(|e| {
                        Diagnostic::new(
                            Stage::Parse,
                            format!("in interpolation: {}", e.message),
                            sp.line,
                            sp.col,
                        )
                    })?;
                    // The sub-lexer restarts spans at 0; shift every token's `start` to its absolute
                    // position in the original source so interpolated expressions carry globally-unique
                    // offsets (a span-keyed rewrite like UFCS keys on `start`). `line`/`col` keep the
                    // sub-lexer's values, so interpolation diagnostics are unchanged.
                    for t in &mut sub_tokens {
                        t.span.start += base;
                    }
                    let mut sub = Parser::new(sub_tokens);
                    let e = sub.parse_expr()?;
                    sub.expect(&TokenKind::Eof, "end of interpolation expression")?;
                    parts.push(StrPart::Expr(Box::new(e)));
                }
            }
        }
        if parts.is_empty() {
            parts.push(StrPart::Literal(String::new()));
        }
        Ok(parts)
    }

    /// Split a string body into literal runs and `{expr}` interpolations.
    /// Each interpolation is re-lexed + re-parsed as a standalone expression.
    /// Used by `html"…"` (whose body is still a flat string).
    pub(super) fn split_interpolation(
        &self,
        body: &str,
        sp: Span,
    ) -> Result<Vec<StrPart>, Diagnostic> {
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

    /// `match EXPR { PAT => EXPR, ... }` — assumes the current token is `match`.
    pub(super) fn parse_match(&mut self, sp: Span) -> Result<Expr, Diagnostic> {
        self.expect(&TokenKind::Match, "'match'")?;
        let scrutinee = self.parse_expr()?;
        self.expect(&TokenKind::LBrace, "'{' to open match arms")?;
        let mut arms = Vec::new();
        while !self.check(&TokenKind::RBrace) {
            let arm_sp = self.peek_span();
            // Or-pattern `p1 | p2 | … => body` (Phase 1 operators slice): collect `|`-separated
            // alternatives. A single pattern (no `|`) is the common case and behaves exactly as
            // before. `|` is unambiguous here — a pattern is followed only by `|`, `when`, or `=>`.
            let mut alts = vec![self.parse_pattern()?];
            while self.eat(&TokenKind::Bar) {
                alts.push(self.parse_pattern()?);
            }
            // Optional arm guard: a contextual `when <cond>` between the pattern and `=>`.
            // `when` is recognized only here (and in if/while-let) — a normal identifier elsewhere.
            let guard = if matches!(self.peek(), TokenKind::Ident(k) if k.as_str() == "when") {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(&TokenKind::FatArrow, "'=>' after match pattern")?;
            let body = self.parse_expr()?;
            if alts.len() == 1 {
                arms.push(MatchArm {
                    pattern: alts.pop().expect("one alternative"),
                    guard,
                    body,
                    span: arm_sp,
                });
            } else {
                // Desugar the or-pattern to one arm per alternative, each sharing the (cloned)
                // body + guard. Every backend then sees ordinary arms — zero backend change, and
                // exhaustiveness / duplicate-arm (`W-MATCH-UNREACHABLE`) / flow-narrowing all work
                // unchanged. Alternatives must be binding-free: a bare binding, `_`, or any
                // variable-binding sub-pattern is rejected, since the shared body cannot depend on
                // which alternative matched (`Some(_) | None()` is fine; `Some(n) | None()` is not).
                for pat in &alts {
                    if Self::or_alt_invalid(pat) {
                        return Err(Diagnostic::new(
                            Stage::Parse,
                            "an or-pattern `a | b` alternative must be a concrete pattern with no bindings (no `_`, no bare name, no variable-binding sub-pattern)",
                            arm_sp.line,
                            arm_sp.col,
                        )
                        .with_code("E-OR-PATTERN-BIND")
                        .with_hint("use literals/variants without binders, or split into separate arms if you need to bind"));
                    }
                }
                for pat in alts {
                    arms.push(MatchArm {
                        pattern: pat,
                        guard: guard.clone(),
                        body: body.clone(),
                        span: arm_sp,
                    });
                }
            }
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
    pub(super) fn parse_if_expr(&mut self, sp: Span) -> Result<Expr, Diagnostic> {
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
}
