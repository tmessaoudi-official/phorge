//! Recursive-descent parser — patterns (M-Decomp W3.1). See parser/mod.rs for the struct + token-stream primitives.

use super::*;

impl Parser {
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
            TokenKind::Str(segs) => {
                self.advance();
                // A string pattern must be a plain literal — interpolation makes no sense in a
                // pattern. The lexer pre-split the literal; require exactly one (or zero, the empty
                // string) literal segment.
                use crate::token::StrSeg;
                match segs.as_slice() {
                    [] => Ok(Pattern::Str(String::new(), sp)),
                    [StrSeg::Lit(s)] => Ok(Pattern::Str(s.clone(), sp)),
                    _ => Err(self.error("a string pattern cannot contain interpolation `{…}`")),
                }
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
                } else if self.check(&TokenKind::LBrace)
                    && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                {
                    // Struct pattern `Point { x, y }` (S5.2). Gated on a PascalCase head so a bare
                    // lowercase binding can never steal a following `{`. Each entry is `field` with an
                    // optional `: <sub-pattern>`; shorthand `x` desugars to `x: x` (a `Binding`).
                    self.advance(); // `{`
                    let mut fields = Vec::new();
                    if !self.check(&TokenKind::RBrace) {
                        loop {
                            let fsp = self.peek_span();
                            let field = match self.peek().clone() {
                                TokenKind::Ident(f) => {
                                    self.advance();
                                    f
                                }
                                _ => return Err(self.error("a field name in a struct pattern")),
                            };
                            let pat = if self.eat(&TokenKind::Colon) {
                                self.parse_pattern()?
                            } else {
                                Pattern::Binding {
                                    name: field.clone(),
                                    span: fsp,
                                }
                            };
                            fields.push(FieldPat { field, pat });
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                            if self.check(&TokenKind::RBrace) {
                                break; // trailing comma
                            }
                        }
                    }
                    self.expect(&TokenKind::RBrace, "'}' to close struct pattern")?;
                    Ok(Pattern::Struct {
                        type_name: name,
                        fields,
                        span: sp,
                    })
                } else if let TokenKind::Ident(binder) = self.peek().clone() {
                    // The contextual guard keyword `when` is never a type-pattern binder — leave it
                    // for the enclosing match arm so `Circle when c => …` reads as a guard, not a
                    // binding named `when`. (A bare `Circle` stays a catch-all `Binding`, the
                    // documented footgun; use `Circle c` / `Circle _` for the type test.)
                    if binder == "when" {
                        return Ok(Pattern::Binding { name, span: sp });
                    }
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

    /// Is `p` an invalid alternative of an or-pattern (`a | b`)? An alternative may not be a
    /// top-level catch-all (`_` / a bare binding) nor introduce any named binding (a `Some(n)`
    /// payload binder, a `Circle c` type binder, a struct-field binder), because the shared arm
    /// body cannot know which alternative matched. Concrete patterns and `_` *sub*-patterns are
    /// fine (`Some(_) | None()`). Drives the `E-OR-PATTERN-BIND` parse error.
    pub(super) fn or_alt_invalid(p: &Pattern) -> bool {
        matches!(p, Pattern::Wildcard(_) | Pattern::Binding { .. })
            || Self::pattern_names_binding(p)
    }

    /// Does `p` (or any sub-pattern) introduce a *named* binding? A `_` wildcard binds nothing, so
    /// a wildcard sub-pattern is not a binding.
    fn pattern_names_binding(p: &Pattern) -> bool {
        match p {
            Pattern::Binding { .. } => true,
            Pattern::Type { binding, .. } => binding.is_some(),
            Pattern::Variant { fields, .. } => fields.iter().any(Self::pattern_names_binding),
            Pattern::Struct { fields, .. } => {
                fields.iter().any(|f| Self::pattern_names_binding(&f.pat))
            }
            _ => false,
        }
    }
}
