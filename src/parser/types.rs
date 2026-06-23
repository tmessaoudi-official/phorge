//! Recursive-descent parser — types (M-Decomp W3.1). See parser/mod.rs for the struct + token-stream primitives.

use super::*;

impl Parser {
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
    pub(super) fn parse_type_intersection(&mut self) -> Result<Type, Diagnostic> {
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
    pub(super) fn parse_type_atom(&mut self) -> Result<Type, Diagnostic> {
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

    /// Optional generic parameter list `<T, U>` immediately after a function name (M-RT S7).
    /// Absent ⇒ empty vec. Both free functions and methods may be generic (M-RT generics-all);
    /// generic *interface* methods are still a non-parse because interface methods build their
    /// `FunctionDecl` directly with an empty `type_params` (no `<…>` is consumed there).
    pub(super) fn parse_type_params(&mut self) -> Result<Vec<String>, Diagnostic> {
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
}
