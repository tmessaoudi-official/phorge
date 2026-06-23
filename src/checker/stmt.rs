//! `impl Checker` — stmt cluster (M-Decomp W2). See checker/mod.rs for the struct + entry points.

use super::*;

impl Checker {
    pub(super) fn stmt_span(s: &crate::ast::Stmt) -> Span {
        use crate::ast::Stmt;
        match s {
            Stmt::VarDecl { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::If { span, .. }
            | Stmt::For { span, .. }
            | Stmt::While { span, .. }
            | Stmt::CFor { span, .. }
            | Stmt::Expr(_, span)
            | Stmt::Block(_, span)
            | Stmt::Throw { span, .. }
            | Stmt::Try { span, .. } => *span,
            Stmt::Break(span) | Stmt::Continue(span) => *span,
        }
    }

    pub(super) fn check_stmt(&mut self, stmt: &crate::ast::Stmt) {
        use crate::ast::Stmt;
        match stmt {
            Stmt::VarDecl {
                ty,
                name,
                init,
                mutable,
                span,
            } => {
                // A let-initializer is the one position where Result-mode `?` propagation is allowed
                // (M-faults 2a): detect it here and type it via `check_propagate` (the unwrapped `Ok`
                // payload). Throws-mode `?` (a throwing call) is allowed in *any* position and tried
                // first; it returns the call's normal type and erases the node (`try_throws_propagate`).
                let actual = match init {
                    crate::ast::Expr::Propagate { inner, span: psp } => self
                        .try_throws_propagate(inner, *psp)
                        .unwrap_or_else(|| self.check_propagate(inner, *psp)),
                    _ => self.check_expr(init),
                };
                let declared = match ty {
                    crate::ast::Type::Infer(infer_span) => {
                        // `var` binds the initializer's type — but a bare `null` (type `Ty::Null`)
                        // has no inferable element type and needs an explicit annotation, e.g.
                        // `int? x = null;` (S0.2 / S2).
                        if matches!(actual, Ty::Null) {
                            self.err_coded(
                                *infer_span,
                                "cannot infer a type from `null`",
                                "E-INFER-NULL",
                                Some("annotate the optional, e.g. `int? x = null;`".into()),
                            )
                        } else {
                            actual.clone()
                        }
                    }
                    _ => {
                        let declared = self.resolve_type(ty);
                        if !self.ty_assignable(&actual, &declared) {
                            self.err_assign(*span, &actual, &declared);
                        }
                        declared
                    }
                };
                self.declare_binding(name, declared, *mutable, *span);
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => {
                use crate::ast::Expr;
                // Always check the value (surfaces nested errors regardless of the target's fate).
                let vty = self.check_expr(value);
                match target {
                    Expr::Ident(name, _) => self.check_local_reassign(name, &vty, target, value),
                    // Value-type element set `xs[i] = e` / `m[k] = e` (M-mut.5).
                    Expr::Index { object, index, .. } => {
                        self.check_index_assign(object, index, &vty, value, *span)
                    }
                    // Shared-mutable instance field set `o.f = e` / `this.f = e` (M-mut.6).
                    Expr::Member {
                        object, name, safe, ..
                    } => self.check_field_assign(object, name, *safe, &vty, value, *span),
                    _ => {
                        self.err_coded(
                            *span,
                            "assignment target must be a variable, an indexed element, or a field",
                            "E-ASSIGN-TARGET",
                            Some(
                                "only `name = e;`, `container[i] = e;`, and `obj.field = e;` are supported; nested places (`a.b.c`, `this.f[i]`) land in a later slice"
                                    .into(),
                            ),
                        );
                    }
                }
            }
            Stmt::Return { value, span } => {
                let actual = match value {
                    Some(e) => self.check_expr(e),
                    None => Ty::Unit,
                };
                let want = self.cur_ret.clone();
                if !self.ty_assignable(&actual, &want) {
                    self.err_assign(*span, &actual, &want);
                }
            }
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                span,
            } => {
                let c = self.check_expr(cond);
                if let Some(name) = bind {
                    // `if (var name = cond)`: the scrutinee must be optional; inside the then-block
                    // `name` is smart-cast to the non-optional inner `T` (and only there). The else
                    // block sees neither `name` nor any narrowing.
                    let inner = match &c {
                        Ty::Optional(i) => (**i).clone(),
                        Ty::Error => Ty::Error,
                        other => self.err_coded(
                            *span,
                            format!("`if (var {name} = …)` requires an optional `T?` scrutinee, found `{other}`"),
                            "E-IF-LET-TYPE",
                            Some("if-let narrows an optional to its non-null inner; the scrutinee is already non-optional".into()),
                        ),
                    };
                    self.push_scope();
                    self.declare(name, inner, *span);
                    self.check_block(then_block);
                    self.pop_scope();
                } else {
                    if !self.ty_assignable(&c, &Ty::Bool) {
                        self.err(*span, format!("`if` condition must be `bool`, found `{c}`"));
                    }
                    // Flow-narrowing (S5.3): the then-block sees the variables the condition implies
                    // when *true* (e.g. `if (x instanceof T)` narrows `x` to `T`). The narrowed shadows
                    // are installed in a child scope and dropped after the block.
                    let narrowings = self.narrow_from_condition(cond, true);
                    self.check_block_narrowed(then_block, &narrowings, *span);
                }
                if let Some(eb) = else_block {
                    self.check_block(eb);
                }
            }
            Stmt::For { .. } => self.check_for(stmt), // implemented in Task 5
            Stmt::While {
                cond,
                body,
                post_cond,
                span,
            } => self.check_while(cond, body, *post_cond, *span),
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => self.check_cfor(init.as_deref(), cond.as_ref(), step.as_deref(), body),
            Stmt::Break(span) => {
                if self.loop_depth == 0 {
                    self.err_coded(
                        *span,
                        "`break` outside a loop",
                        "E-BREAK-OUTSIDE-LOOP",
                        Some(
                            "`break` may only appear inside a `for`/`while`/`do-while` loop".into(),
                        ),
                    );
                }
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    self.err_coded(
                        *span,
                        "`continue` outside a loop",
                        "E-CONTINUE-OUTSIDE-LOOP",
                        Some(
                            "`continue` may only appear inside a `for`/`while`/`do-while` loop"
                                .into(),
                        ),
                    );
                }
            }
            Stmt::Block(stmts, _) => self.check_block(stmts),
            Stmt::Expr(e, _) => {
                self.check_expr(e);
            }
            // M-faults 2b.3: `throw e` — the value must implement `Error` (`E-THROW-TYPE`), and the
            // exception must be *discharged* in context: caught by an enclosing `try` or declared in
            // the enclosing `throws` (`E-THROW-UNDECLARED`, or `E-UNCAUGHT-THROW` inside `main`).
            Stmt::Throw { value, span } => {
                let e = self.check_expr(value);
                if matches!(e, Ty::Error) {
                    // poison — an earlier error already reported
                } else if !self.is_error_type(&e) {
                    self.err_coded(
                        *span,
                        format!(
                            "can only `throw` a value whose type implements `Error`, found `{e}`"
                        ),
                        "E-THROW-TYPE",
                        Some("define the thrown type as `class Foo implements Error { … }`".into()),
                    );
                } else if !self.covered_by_try(&e) && !self.throws_declared(&e) {
                    if self.cur_is_main {
                        self.err_coded(
                            *span,
                            format!("`{e}` thrown in `main` escapes the program entry point"),
                            "E-UNCAUGHT-THROW",
                            Some("wrap it in `try { … } catch (… e) { … }` — `main` may not let an exception escape".into()),
                        );
                    } else {
                        self.err_coded(
                            *span,
                            format!("`{e}` is thrown here but neither caught nor declared"),
                            "E-THROW-UNDECLARED",
                            Some(format!("add `throws {e}` to the enclosing function, or wrap this in `try`/`catch`")),
                        );
                    }
                }
            }
            // M-faults 2b.3: a `try` — validate each catch type (`<: Error`, flag a shadowed clause
            // `W-CATCH-UNREACHABLE`), check the body with the catch set active so a throw inside is
            // discharged, then each catch body with its binding in scope, then `finally`.
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                // Resolve + validate catch types, building the active frame and the per-clause
                // binding types. A union catch `(A | B e)` contributes both members to the frame.
                let mut frame: Vec<Ty> = Vec::new();
                let mut seen: Vec<Ty> = Vec::new();
                let mut clause_tys: Vec<Ty> = Vec::with_capacity(catches.len());
                for c in catches {
                    let cty = self.resolve_type(&c.ty);
                    let members: Vec<Ty> = match &cty {
                        Ty::Union(ms) => ms.clone(),
                        other => vec![other.clone()],
                    };
                    for m in &members {
                        if !self.is_error_type(m) {
                            self.err_coded(
                                c.span,
                                format!("a `catch` type must implement `Error`, found `{m}`"),
                                "E-CATCH-TYPE",
                                Some("catch a type defined `class Foo implements Error { … }` (or the `Error` base itself)".into()),
                            );
                        }
                    }
                    // A clause every member of which is already covered by an earlier clause can
                    // never run (PHP is silent here; Phorge lints — see the totality cluster).
                    if !members.is_empty()
                        && members
                            .iter()
                            .all(|m| seen.iter().any(|s| self.ty_assignable(m, s)))
                    {
                        self.warn_coded(
                            c.span,
                            "unreachable `catch`: an earlier clause already catches this type",
                            "W-CATCH-UNREACHABLE",
                            Some(
                                "remove it, or reorder so the more specific clause comes first"
                                    .into(),
                            ),
                        );
                    }
                    seen.extend(members.iter().cloned());
                    frame.extend(members.iter().cloned());
                    clause_tys.push(cty);
                }
                // The catch set covers throws inside the *body* only (a throw in a catch/finally is
                // not caught by the same `try`): push for the body, pop before the clauses.
                self.try_catch_stack.push(frame);
                self.check_block(body);
                self.try_catch_stack.pop();
                for (c, cty) in catches.iter().zip(clause_tys) {
                    self.push_scope();
                    self.declare(&c.name, cty, c.span);
                    self.check_block(&c.body);
                    self.pop_scope();
                }
                if let Some(fb) = finally_block {
                    self.check_block(fb);
                }
            }
        }
    }

    /// Check `block` with the given flow-narrowings (`(var, narrowed-type)`) installed as shadows in
    /// a fresh child scope. Each narrowed shadow inherits its outer binding's mutability, so a
    /// `mutable` variable stays reassignable inside the narrowed block (reassignment is still checked
    /// against the narrowed type, keeping narrowing sound — the M-mut.1 smart-cast interaction). An
    /// empty narrowing list just checks the block in the current scope (no extra frame).
    pub(super) fn check_block_narrowed(
        &mut self,
        block: &[crate::ast::Stmt],
        narrowings: &[(String, Ty)],
        span: Span,
    ) {
        if narrowings.is_empty() {
            self.check_block(block);
            return;
        }
        self.push_scope();
        for (name, ty) in narrowings {
            let m = self.lookup_binding(name).map(|(_, m)| m).unwrap_or(false);
            self.declare_binding(name, ty.clone(), m, span);
        }
        self.check_block(block);
        self.pop_scope();
    }

    /// The variables a boolean condition narrows when it evaluates to `polarity` (`true` = then-branch,
    /// `false` = else-branch), as `(var, narrowed-type)` shadows. Flow-narrowing engine (S5.3); a `&self`
    /// query (installation is the caller's job). T1 recognizes the one pre-existing source — a bare
    /// `x instanceof T` at `polarity = true` — preserving the prior smart-cast behavior exactly; later
    /// tasks add the negative/else, equality and `&&`/`!` forms.
    pub(super) fn narrow_from_condition(
        &self,
        cond: &crate::ast::Expr,
        polarity: bool,
    ) -> Vec<(String, Ty)> {
        use crate::ast::Expr;
        let mut out = Vec::new();
        if let Expr::InstanceOf {
            value, type_name, ..
        } = cond
        {
            if polarity {
                if let Expr::Ident(name, _) = &**value {
                    if self.classes.contains_key(type_name)
                        || self.interfaces.contains_key(type_name)
                    {
                        // `instanceof` carries no type arguments at runtime (`instanceof Box<int>` ≡
                        // `instanceof Box`), so a narrowed generic class instance has erased (poison)
                        // type arguments — its generic members read as `mixed` (M-RT generics-all).
                        let arity = self
                            .classes
                            .get(type_name)
                            .map_or(0, |c| c.type_params.len());
                        let args = vec![Ty::Error; arity];
                        out.push((name.clone(), Ty::Named(type_name.clone(), args)));
                    }
                }
            }
        }
        out
    }

    pub(super) fn check_for(&mut self, stmt: &crate::ast::Stmt) {
        if let crate::ast::Stmt::For {
            ty,
            name,
            iter,
            body,
            span,
        } = stmt
        {
            let declared = self.resolve_type(ty);
            let iter_ty = self.check_expr(iter);
            let elem = match iter_ty {
                Ty::List(e) => *e,
                Ty::Error => Ty::Error,
                other => {
                    self.err(
                        *span,
                        format!("`for`-`in` requires a List, found `{other}`"),
                    );
                    Ty::Error
                }
            };
            if !self.ty_assignable(&elem, &declared) {
                self.err(
                    *span,
                    format!("loop variable `{name}` declared `{declared}` but iterating `{elem}`"),
                );
            }
            self.push_scope();
            self.declare(name, declared, *span);
            self.loop_depth += 1;
            for s in body {
                self.check_stmt(s);
            }
            self.loop_depth -= 1;
            self.pop_scope();
        }
    }

    /// `while (cond) { .. }` / `do { .. } while (cond);` (M-mut.3). The condition must be `bool` and
    /// is checked in the loop's *outer* scope (the body's own bindings are not visible to it — true
    /// for do-while too, matching the interpreter's scope-pop-before-retest).
    pub(super) fn check_while(
        &mut self,
        cond: &crate::ast::Expr,
        body: &[crate::ast::Stmt],
        _post_cond: bool,
        span: Span,
    ) {
        let ct = self.check_expr(cond);
        if !self.ty_assignable(&ct, &Ty::Bool) {
            self.err(span, format!("loop condition must be `bool`, found `{ct}`"));
        }
        self.push_scope();
        self.loop_depth += 1;
        for s in body {
            self.check_stmt(s);
        }
        self.loop_depth -= 1;
        self.pop_scope();
    }

    /// C-style `for (init; cond; step) { .. }` (M-mut.3). `init`'s binding lives in the loop's own
    /// scope and is visible to `cond`/`step`/`body`; `cond` (if present) must be `bool`.
    pub(super) fn check_cfor(
        &mut self,
        init: Option<&crate::ast::Stmt>,
        cond: Option<&crate::ast::Expr>,
        step: Option<&crate::ast::Stmt>,
        body: &[crate::ast::Stmt],
    ) {
        self.push_scope();
        if let Some(s) = init {
            self.check_stmt(s);
        }
        if let Some(c) = cond {
            let ct = self.check_expr(c);
            if !self.ty_assignable(&ct, &Ty::Bool) {
                self.err(
                    Self::expr_span(c),
                    format!("loop condition must be `bool`, found `{ct}`"),
                );
            }
        }
        // `step` runs each iteration (not the loop body) but is checked once; a bare `break`/
        // `continue` in `step` is nonsensical, so it is NOT inside the loop-depth bump.
        if let Some(s) = step {
            self.check_stmt(s);
        }
        self.loop_depth += 1;
        for s in body {
            self.check_stmt(s);
        }
        self.loop_depth -= 1;
        self.pop_scope();
    }
}
