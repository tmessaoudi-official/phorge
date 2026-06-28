//! Recursive-descent + Pratt parser: turns the lexer's token stream into the AST.

use crate::ast::{
    Attribute, BinaryOp, ClassDecl, ClassMember, CtorParam, EnumDecl, EnumVariant, Expr, FieldPat,
    FunctionDecl, Item, LambdaBody, MatchArm, Modifier, Param, Pattern, Program, Stmt, StrPart,
    Type, UnaryOp, Visibility,
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
    /// Restriction flag suppressing the `as`-cast fold at the *top level* of a `foreach` iterable
    /// (M4 casting). `as` is contextual — both the `foreach (EXPR as NAME)` separator and the cast
    /// operator — so while reading the iterable we must not let `parse_binary` greedily consume the
    /// separator `as` as a cast. It is set only by [`parse_foreach`] and **reset by every
    /// [`parse_expr`]** (parens/call-args/index/list-map all re-enter there), so a cast *inside* the
    /// iterable still parses; a top-level cast needs explicit parens (and is meaningless anyway — a
    /// cast yields `T?`, not an iterable). Mirrors Rust's no-struct-literal restriction in `if cond`.
    no_as_cast: bool,
}

// impl-cluster cohesion split (M-Decomp W3.1): one `impl Parser` block per cluster file.
mod exprs;
mod items;
mod patterns;
mod stmts;
mod types;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        // The lexer always terminates the stream with Eof, so `tokens` is non-empty.
        Parser {
            tokens,
            pos: 0,
            depth: 0,
            no_as_cast: false,
        }
    }

    /// The kind of the current token. At/after the end, this is `Eof`.
    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos.min(self.tokens.len() - 1)].kind
    }

    /// The kind of the next-but-one token (one beyond `peek`). `Eof` at/after the end. Used to
    /// recognize shift-right `>>` as two adjacent `Gt` tokens in expression position (primitives P2).
    fn peek2(&self) -> &TokenKind {
        &self.tokens[(self.pos + 1).min(self.tokens.len() - 1)].kind
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

    /// True if the current token is the contextual keyword `kw` (an identifier with that exact text).
    /// Phorj's contextual keywords (`var`, `foreach`, `as`, `when`) live as ordinary identifiers in
    /// the token stream and are recognized only in the positions where they are meaningful, so the
    /// same word stays usable as a value / field / parameter name everywhere else.
    fn at_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), TokenKind::Ident(s) if s == kw)
    }

    /// True when a leading `var` opens a declaration/binding rather than naming a value: `var IDENT`
    /// (an inferred binding, or the head of a `var Type { … }` struct destructure) or `var [` (a list
    /// destructure). `var` followed by anything else (`=`, `.`, `(`, an operator, `;`) is an ordinary
    /// identifier — a reassignment or expression — so the same word is a usable value name.
    fn at_var_decl(&self) -> bool {
        self.at_kw("var") && matches!(self.peek2(), TokenKind::Ident(_) | TokenKind::LBracket)
    }

    /// `discard` is a contextual statement keyword (M-must-use): it opens `discard <expr>;` only when
    /// it leads a statement *and* the next token begins a discardable expression — an identifier (a
    /// call / method-call / qualified call) or `new`. Every real discard target starts that way, and
    /// the gate never misfires on `discard` used as a value: `discard = e`, `discard.f`, `discard(…)`,
    /// `discard[i]` all have a non-`Ident`/`New` follower and fall through to the expression path.
    fn at_discard(&self) -> bool {
        self.at_kw("discard") && matches!(self.peek2(), TokenKind::Ident(_) | TokenKind::New)
    }

    /// Consume the contextual keyword `kw` (its presence already established by the caller) or error.
    fn eat_kw(&mut self, kw: &str, what: &str) -> Result<(), Diagnostic> {
        if self.at_kw(kw) {
            self.advance();
            Ok(())
        } else {
            Err(self.error(what))
        }
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
