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
        // `[T; N]` — a fixed-length list type (Phase 1 types slice). `[` is unambiguous in type
        // position (lists are `List<T>`, maps `Map<K, V>`), so a leading `[` always opens this form.
        // The length `N` is a non-negative integer literal.
        if self.eat(&TokenKind::LBracket) {
            let elem = Box::new(self.parse_type()?);
            self.expect(
                &TokenKind::Semicolon,
                "';' between the element type and length in `[T; N]`",
            )?;
            let len = match self.peek().clone() {
                TokenKind::Int(n) if n >= 0 => {
                    self.advance();
                    n as usize
                }
                _ => return Err(self.error("a non-negative integer length `N` in `[T; N]`")),
            };
            self.expect(
                &TokenKind::RBracket,
                "']' to close a fixed-length list type `[T; N]`",
            )?;
            let mut t = Type::FixedList {
                elem,
                len,
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
        // A leading `(` is either a function-type parameter list (`(int, string) -> bool`) or a
        // **grouped** type (`(T)` ≡ `T`) — disambiguated by whether a `->` follows the `)`. The
        // grouped form is what lets a function type appear, parenthesized, in return position:
        // `() -> ((int) -> bool)` (slice 3 / spec #8) — without it the inner `(` was always read as a
        // param list demanding its own `->`. The parens-free right-assoc form `() -> (int) -> bool`
        // already worked and parses to the same type.
        if self.eat(&TokenKind::LParen) {
            let mut params = Vec::new();
            if !self.check(&TokenKind::RParen) {
                params.push(self.parse_type()?);
                while self.eat(&TokenKind::Comma) {
                    params.push(self.parse_type()?);
                }
            }
            self.expect(
                &TokenKind::RParen,
                "')' to close a function-type parameter list or a grouped type",
            )?;
            let mut t = if self.eat(&TokenKind::Arrow) {
                // `( … ) -> R` — a function type with the parsed parameter list.
                Type::Function {
                    params,
                    ret: Box::new(self.parse_type()?),
                    span: sp,
                }
            } else {
                // No `->`: the parens were grouping, not a parameter list. Exactly one inner type is
                // `(T)` ≡ `T`; `()` / `(A, B)` without a `->` are invalid (Phorge has no unit-paren
                // or tuple types — a multi-element list must be a function-type parameter list).
                match params.len() {
                    1 => params.pop().expect("one grouped type"),
                    0 => {
                        return Err(self
                            .error("a `->` return type after `()` (an empty `()` is a function-type parameter list)"))
                    }
                    _ => {
                        return Err(self.error(
                            "a `->` return type (Phorge has no tuple types — `(A, B)` is a function-type parameter list and needs `-> R`)",
                        ))
                    }
                }
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
