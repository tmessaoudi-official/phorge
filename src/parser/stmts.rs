//! Recursive-descent parser — stmts (M-Decomp W3.1). See parser/mod.rs for the struct + token-stream primitives.

use super::*;

impl Parser {
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
            TokenKind::Mutable => self.parse_mutable_var_decl(),
            TokenKind::Throw => self.parse_throw(),
            TokenKind::Try => self.parse_try(),
            // `var` is a contextual keyword: it opens an inferred binding / destructure ONLY when it
            // leads a declaration (`var IDENT` / `var [`). `var` used as a value (`var = e`, `var.f`,
            // `var(…)`) falls through to the expression/assignment path below.
            TokenKind::Ident(s) if s == "var" && self.at_var_decl() => {
                self.parse_var_or_destructure()
            }
            // A-6: `foreach` is a contextual keyword (like `as`/`when`) — only the `foreach (`
            // statement-leading form is the loop; a bare `foreach` ident elsewhere is unaffected.
            TokenKind::Ident(s) if s == "foreach" => self.parse_foreach(),
            // M-must-use: `discard <expr>;` — the escape hatch for the must-use rule. Contextual
            // (see `at_discard`); a bare `discard` value-use falls through to the expression path.
            TokenKind::Ident(s) if s == "discard" && self.at_discard() => self.parse_discard(),
            _ => self.parse_var_decl_or_expr_stmt(),
        }
    }

    /// `discard expr;` (M-must-use Slice A) — evaluate `expr` for effect and drop a non-`void`/`Empty`
    /// result, the explicit escape hatch for the must-use rule (`E-UNUSED-VALUE`).
    pub(super) fn parse_discard(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.eat_kw("discard", "'discard'")?;
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "';' after 'discard <expr>'")?;
        Ok(Stmt::Discard(value, sp))
    }

    /// `throw expr;` (M-faults 2b).
    pub(super) fn parse_throw(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::Throw, "'throw'")?;
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "';' after 'throw <expr>'")?;
        Ok(Stmt::Throw { value, span: sp })
    }

    /// `try { .. } catch (Type name) { .. } [catch …] [finally { .. }]` (M-faults 2b). Requires at
    /// least one `catch` **or** a `finally` (a bare `try {}` is a parse error). A catch type may be a
    /// union (`catch (A | B e)`), parsed by the shared `parse_type`.
    pub(super) fn parse_try(&mut self) -> Result<Stmt, Diagnostic> {
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

    /// Dispatch the three `var`-led statement forms (Phase 1 slice 5): a list destructure (`var [a, b]
    /// = …`), a struct destructure (`var Type { … } = …`), or a plain inferred binding (`var name =
    /// …`). The destructure forms are reached only through this (the bare `var` path); `mutable var` is
    /// always a scalar binding, so `parse_mutable_var_decl` never routes here.
    pub(super) fn parse_var_or_destructure(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.eat_kw("var", "'var'")?;
        if self.check(&TokenKind::LBracket) {
            return self.parse_list_destructure(sp);
        }
        if matches!(self.peek(), TokenKind::Ident(_)) && matches!(self.peek2(), TokenKind::LBrace) {
            return self.parse_struct_destructure(sp);
        }
        // Plain `var name = expr;` (the `Var` token is already consumed).
        let name = self.expect_ident("a variable name after 'var'")?;
        self.expect(&TokenKind::Eq, "'=' after 'var <name>'")?;
        let init = self.parse_expr()?;
        self.expect(&TokenKind::Semicolon, "';' after variable declaration")?;
        Ok(Stmt::VarDecl {
            ty: Type::Infer(sp),
            name,
            init,
            mutable: false,
            span: sp,
        })
    }

    /// `[a, b] = expr [else { … }];` — the `var` and `[` have been peeked but not consumed. The
    /// trailing form is either `else { block }` (refutable, no `;`) or `;` (irrefutable `[T; N]`).
    fn parse_list_destructure(&mut self, sp: Span) -> Result<Stmt, Diagnostic> {
        self.expect(&TokenKind::LBracket, "'[' to open a list destructuring")?;
        let mut binders = Vec::new();
        loop {
            let bsp = self.peek_span();
            let name = self.expect_ident("a binding name in the list destructuring")?;
            binders.push((name, bsp));
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBracket, "']' to close the list destructuring")?;
        let pat = crate::ast::DestructurePat::List { binders, span: sp };
        self.finish_destructure(pat, sp)
    }

    /// `Type { field [: binding], … } = expr [else { … }];` — the `var`, type ident, and `{` have been
    /// peeked but not consumed.
    fn parse_struct_destructure(&mut self, sp: Span) -> Result<Stmt, Diagnostic> {
        let type_name = self.expect_ident("a type name to destructure")?;
        self.expect(&TokenKind::LBrace, "'{' to open a struct destructuring")?;
        let mut fields = Vec::new();
        loop {
            let fsp = self.peek_span();
            let field = self.expect_ident("a field name in the struct destructuring")?;
            // `field: binding` renames; bare `field` binds to its own name.
            let binding = if self.eat(&TokenKind::Colon) {
                self.expect_ident("a binding name after ':'")?
            } else {
                field.clone()
            };
            fields.push(crate::ast::DestructureField {
                field,
                binding,
                span: fsp,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace, "'}' to close the struct destructuring")?;
        let pat = crate::ast::DestructurePat::Struct {
            type_name,
            fields,
            span: sp,
        };
        self.finish_destructure(pat, sp)
    }

    /// Shared tail of both destructure forms: `= expr`, then either `else { block }` (no `;`) or `;`.
    /// The checker enforces which form each pattern requires (refutable list ⇒ `else`; everything else
    /// ⇒ no `else`).
    fn finish_destructure(
        &mut self,
        pat: crate::ast::DestructurePat,
        sp: Span,
    ) -> Result<Stmt, Diagnostic> {
        self.expect(&TokenKind::Eq, "'=' after the destructuring pattern")?;
        let init = self.parse_expr()?;
        let else_block = if self.eat(&TokenKind::Else) {
            Some(self.parse_block()?)
        } else {
            self.expect(
                &TokenKind::Semicolon,
                "';' or 'else { … }' after the destructuring",
            )?;
            None
        };
        Ok(Stmt::Destructure {
            pat,
            init,
            else_block,
            span: sp,
        })
    }

    /// `var name = expr;` — the binding type is inferred from `expr` by the checker. `mutable` is
    /// `true` when this was reached via `mutable var name = …` (M-mut.1).
    pub(super) fn parse_var_inferred(&mut self, mutable: bool) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.eat_kw("var", "'var'")?;
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
    pub(super) fn parse_mutable_var_decl(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::Mutable, "'mutable'")?;
        if self.at_kw("var") {
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
    pub(super) fn parse_block(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut stmts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(&TokenKind::RBrace, "'}' to close block")?;
        Ok(stmts)
    }

    /// `return;` or `return expr;`
    pub(super) fn parse_return(&mut self) -> Result<Stmt, Diagnostic> {
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
    pub(super) fn parse_if(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::If, "'if'")?;
        self.expect(&TokenKind::LParen, "'(' after 'if'")?;
        // `if (var name = scrutinee)` binds the non-null inner of an optional scrutinee inside the
        // then-block (M3 S2.4). With `var` now contextual, a plain `var` value could open a normal
        // condition, so the if-let form is selected only on `var IDENT` (a binding head); `if (var)`,
        // `if (var == x)`, etc. parse `var` as an ordinary value.
        let bind = if self.at_kw("var") && matches!(self.peek2(), TokenKind::Ident(_)) {
            self.advance(); // contextual `var`
            let name = self.expect_ident("a binding name after 'var'")?;
            self.expect(&TokenKind::Eq, "'=' in 'if (var name = …)'")?;
            Some(name)
        } else {
            None
        };
        let cond = self.parse_expr()?;
        // An if-let `when` guard (pattern cluster S5.3): `if (var x = e when <cond>)`. Contextual
        // `when`, recognized only in the if-let form (after the binding). It is desugared below into a
        // nested `if (<guard>)` inside the bind's then-block (where `x` is in scope), so the AST gains
        // no `Stmt::If.guard` field and every backend is untouched. A `when` in a plain `if` is not
        // recognized (it would fail the `)` expectation) — use `&&` for a plain compound condition.
        let guard = if bind.is_some()
            && matches!(self.peek(), TokenKind::Ident(k) if k.as_str() == "when")
        {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
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
        if let Some(g) = guard {
            // if-let-guard desugar: `if (var x = e when g) THEN [else ELSE]` becomes
            // `if (var x = e) { if (g) THEN [else ELSE] } [else ELSE]`. The guard `g` is checked in
            // the then-scope (where `x` is the narrowed non-null binding); the else branch runs when
            // the bind fails OR the guard is false (the else block is shared by both, hence cloned).
            let inner = Stmt::If {
                cond: g,
                bind: None,
                then_block,
                else_block: else_block.clone(),
                span: sp,
            };
            return Ok(Stmt::If {
                cond,
                bind,
                then_block: vec![inner],
                else_block,
                span: sp,
            });
        }
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
    pub(super) fn parse_for(&mut self) -> Result<Stmt, Diagnostic> {
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

    /// A-6: `foreach (EXPR as NAME [with int COUNTER]) BLOCK` — PHP-familiar iteration, kept
    /// alongside the typed `for (T x in xs)` form. Desugars entirely to the existing for-in (with an
    /// **inferred** element type, resolved by the checker) so the interpreter/VM/transpiler are
    /// untouched; the for-in already emits idiomatic PHP `foreach`. An optional `with int i` counter
    /// becomes a 0-based induction variable in an enclosing block, incremented at the end of each
    /// iteration. (Key/value `as k => v` and destructure bindings are a documented follow-up — they
    /// need iteration-model changes the value form does not.)
    pub(super) fn parse_foreach(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.advance(); // `foreach` (contextual)
        self.expect(&TokenKind::LParen, "'(' after 'foreach'")?;
        // Read the iterable with the `as`-cast fold suppressed at the top level, so the upcoming `as`
        // is recognized as the foreach separator (not a cast). `parse_range` keeps the flag; any
        // bracketed sub-expression re-enters `parse_expr`, which re-enables casts (M4 casting).
        self.no_as_cast = true;
        let iter = self.parse_range();
        self.no_as_cast = false;
        let iter = iter?;
        if !matches!(self.peek(), TokenKind::Ident(s) if s == "as") {
            return Err(self.error("'as' after the foreach iterable (e.g. `foreach (xs as x)`)"));
        }
        self.advance(); // `as`
        if matches!(self.peek(), TokenKind::LBracket) || self.peek2_is_fat_arrow_binding() {
            return Err(self.error(
                "foreach key/value (`as k => v`) and destructure bindings are not supported yet — \
                 use `foreach (xs as x)` (value form) or the typed `for (T x in xs)`",
            ));
        }
        let name = self.expect_ident("a binding name after 'as'")?;
        // Optional `with int COUNTER` — a 0-based auto-incrementing position counter.
        let counter = if matches!(self.peek(), TokenKind::With) {
            self.advance(); // `with`
            let cty = self.parse_type()?;
            if !matches!(&cty, Type::Named { name, args, .. } if name == "int" && args.is_empty()) {
                return Err(
                    self.error("the foreach counter must be typed `int` (e.g. `with int i`)")
                );
            }
            Some(self.expect_ident("a counter name after 'with int'")?)
        } else {
            None
        };
        self.expect(&TokenKind::RParen, "')' after the foreach header")?;
        let mut body = self.parse_block()?;
        // With a counter, append `c = c + 1;` to the loop body and declare `c` in an enclosing block.
        if let Some(c) = &counter {
            body.push(Stmt::Assign {
                target: Expr::Ident(c.clone(), sp),
                value: Expr::Binary {
                    op: BinaryOp::Add,
                    lhs: Box::new(Expr::Ident(c.clone(), sp)),
                    rhs: Box::new(Expr::Int(1, sp)),
                    span: sp,
                },
                span: sp,
            });
        }
        let loop_stmt = Stmt::For {
            ty: Type::Infer(sp),
            name,
            iter,
            body,
            span: sp,
        };
        match counter {
            None => Ok(loop_stmt),
            Some(c) => Ok(Stmt::Block(
                vec![
                    Stmt::VarDecl {
                        ty: Type::Named {
                            name: "int".to_string(),
                            args: Vec::new(),
                            span: sp,
                        },
                        name: c,
                        init: Expr::Int(0, sp),
                        mutable: true,
                        span: sp,
                    },
                    loop_stmt,
                ],
                sp,
            )),
        }
    }

    /// True if the tokens just after `as` look like a key/value binding `NAME =>` (so we can reject
    /// it with a helpful message rather than misparsing). Peeks `Ident` then `=>`.
    fn peek2_is_fat_arrow_binding(&self) -> bool {
        matches!(self.peek(), TokenKind::Ident(_)) && matches!(self.peek2(), TokenKind::FatArrow)
    }

    /// Scan the for-header tokens (from just after the opening `(`) at paren/bracket depth 0: a
    /// top-level `;` means a C-`for`, a top-level `in` means a for-`in`. Neither `;` nor `in`
    /// appears inside balanced `()`/`[]` of a well-formed header, so depth tracking is exact.
    pub(super) fn for_header_is_classic(&self) -> bool {
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
    pub(super) fn parse_cfor_rest(&mut self, sp: Span) -> Result<Stmt, Diagnostic> {
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
    pub(super) fn parse_for_clause_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        if self.eat(&TokenKind::Mutable) {
            // After `mutable` a binding is mandatory, so `var` here is always the inference keyword.
            let (ty, name) = if self.at_kw("var") {
                self.advance(); // contextual `var`
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
        // `for (var i = 0; …)` — the inference keyword only on `var IDENT`; `for (var = 0; …)`
        // reassigns a variable named `var` and falls through to the assignment/expression path.
        if self.at_kw("var") && matches!(self.peek2(), TokenKind::Ident(_)) {
            self.advance(); // contextual `var`
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
    pub(super) fn parse_while(&mut self) -> Result<Stmt, Diagnostic> {
        let sp = self.peek_span();
        self.expect(&TokenKind::While, "'while'")?;
        self.expect(&TokenKind::LParen, "'(' after 'while'")?;
        // while-let, selected only on `var IDENT` (a binding head) now that `var` is contextual.
        if self.at_kw("var") && matches!(self.peek2(), TokenKind::Ident(_)) {
            self.advance(); // contextual `var`
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
    pub(super) fn parse_do_while(&mut self) -> Result<Stmt, Diagnostic> {
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
    pub(super) fn parse_var_decl_or_expr_stmt(&mut self) -> Result<Stmt, Diagnostic> {
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
    /// or `)`). `/=`/`%=` inherit `__phorj_div`/`__phorj_rem` via `BinaryOp::Div`/`Rem` (F7).
    pub(super) fn finish_assign_or_expr(
        &mut self,
        expr: Expr,
        sp: Span,
    ) -> Result<Stmt, Diagnostic> {
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
    pub(super) fn try_var_decl_header(&mut self) -> Option<(Type, String)> {
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
}
