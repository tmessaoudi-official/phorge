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
            | Stmt::Discard(_, span)
            | Stmt::Block(_, span)
            | Stmt::Throw { span, .. }
            | Stmt::Try { span, .. }
            | Stmt::Destructure { span, .. } => *span,
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
                        // `[T; N] p = [e0, e1, …]`: a list literal carries a known length, so the
                        // fixed-length is checked *here* (the literal is the one place the length is
                        // statically known) — `List` itself is length-erased, so this is the only
                        // path that introduces a `[T; N]` value (Phase 1 types slice).
                        if let (Ty::FixedList(elem, n), crate::ast::Expr::List(elems, _)) =
                            (&declared, init)
                        {
                            if elems.len() != *n {
                                self.err_coded(
                                    *span,
                                    format!(
                                        "expected `[{elem}; {n}]` (length {n}), found a list literal of length {}",
                                        elems.len()
                                    ),
                                    "E-FIXEDLIST-LEN",
                                    None,
                                );
                            }
                            // Element-type compatibility (List is invariant, so this checks `elem`).
                            if !self.ty_assignable(&actual, &Ty::List(elem.clone())) {
                                self.err_assign(*span, &actual, &declared);
                            }
                        } else if !self.ty_assignable(&actual, &declared) {
                            self.err_assign(*span, &actual, &declared);
                        }
                        declared
                    }
                };
                // S0a: a `void` value is uncapturable. Binding one into a variable is an error —
                // *unless* the declared type is the holdable `Empty` (`Empty x = noop();`), the
                // explicit escape hatch (`void <: Empty`). This catches both `var x = noop()`
                // (inferred `declared` = `Void`) and `void x = noop()` (declared = `Void`).
                let declared = if actual == Ty::Void && declared != Ty::Empty {
                    self.err_coded(
                        *span,
                        "a `void` value cannot be captured — the expression produces nothing",
                        "E-VOID-CAPTURE",
                        Some(
                            "drop the binding and call it as a statement; or, to hold the empty value, annotate it `Empty` (e.g. `Empty x = …;`)"
                                .into(),
                        ),
                    )
                } else {
                    declared
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
                    None => Ty::Void,
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
                    // are installed in a child scope and dropped after the block. The else-block sees
                    // the *false*-polarity narrowing (T2: the remaining union members, the null arm…).
                    let then_narrow = self.narrow_from_condition(cond, true);
                    self.check_block_narrowed(then_block, &then_narrow, *span);
                    if let Some(eb) = else_block {
                        let else_narrow = self.narrow_from_condition(cond, false);
                        self.check_block_narrowed(eb, &else_narrow, *span);
                    }
                    return;
                }
                // The if-let (`bind`) path: its else-block sees neither the binding nor any narrowing.
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
                // M-must-use Slice A: a non-`void`/`Empty` result used as a bare statement would be
                // dropped silently — forbid it (`E-UNUSED-VALUE`). `discard <expr>;` is the escape hatch.
                let t = self.check_expr(e);
                if !matches!(t, Ty::Void | Ty::Empty | Ty::Error | Ty::Never) {
                    self.err_coded(
                        Self::expr_span(e),
                        format!("unused `{t}` value"),
                        "E-UNUSED-VALUE",
                        Some(
                            "a non-`void`/`Empty` result must be used; bind it or prefix `discard`"
                                .into(),
                        ),
                    );
                }
            }
            Stmt::Discard(e, _) => {
                // Must-use escape hatch: type-check the expression; any result type may be dropped.
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
                    // never run (PHP is silent here; Phorj lints — see the totality cluster).
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
            Stmt::Destructure {
                pat,
                init,
                else_block,
                span,
            } => self.check_destructure(pat, init, else_block.as_deref(), *span),
        }
    }

    /// Let-destructuring (Phase 1 slice 5): type the initializer, decide refutability, enforce the
    /// `else` rules, then bind every binder into the **current** scope at its resolved element/field
    /// type. The `else` block (refutable list only) is checked in a scope *without* the binders and
    /// must diverge (Swift `guard let`); a present `else` on an irrefutable pattern is an error.
    pub(super) fn check_destructure(
        &mut self,
        pat: &crate::ast::DestructurePat,
        init: &crate::ast::Expr,
        else_block: Option<&[crate::ast::Stmt]>,
        span: Span,
    ) {
        use crate::ast::DestructurePat;
        let init_ty = self.check_expr(init);
        // (binding name, span, resolved type) for each binder, filled per pattern kind below.
        let mut binds: Vec<(String, Span, Ty)> = Vec::new();
        // Whether the pattern is refutable (a present `else` is then required and must diverge).
        let mut refutable = false;
        match pat {
            DestructurePat::Struct {
                type_name, fields, ..
            } => {
                // The head names a concrete class; the init must BE that class (irrefutable). A generic
                // instance must match the head exactly so its type args resolve the fields; a plain
                // subtype is accepted only for a non-generic class (no args to recover).
                let class_args: Option<Vec<Ty>> = match &init_ty {
                    Ty::Error => Some(vec![]), // poison: emit no further errors
                    Ty::Named(cls, cargs) if cls == type_name => Some(cargs.clone()),
                    Ty::Named(cls, _)
                        if self.is_subtype(cls, type_name)
                            && self
                                .classes
                                .get(type_name)
                                .is_some_and(|i| i.type_params.is_empty()) =>
                    {
                        Some(vec![])
                    }
                    other => {
                        self.err_coded(
                            span,
                            format!("cannot destructure `{other}` as `{type_name}`"),
                            "E-DESTRUCTURE-TYPE",
                            Some(format!(
                                "the value must be a `{type_name}` (or a subtype) — destructure it at its own type"
                            )),
                        );
                        None
                    }
                };
                if !matches!(init_ty, Ty::Error) && !self.classes.contains_key(type_name) {
                    self.err_coded(
                        span,
                        format!("`{type_name}` is not a class — only classes can be struct-destructured"),
                        "E-DESTRUCTURE-NOT-CLASS",
                        Some("list values destructure with `var [a, b] = …`".into()),
                    );
                }
                if let Some(cargs) = class_args {
                    let subst = self.class_subst(type_name, &cargs);
                    for f in fields {
                        let fty = self
                            .classes
                            .get(type_name)
                            .and_then(|i| i.fields.get(&f.field).cloned());
                        let resolved = match fty {
                            Some(t) => {
                                // Wave 1.1: destructuring reads the field (→ PHP `$obj->field`), so an
                                // out-of-scope `private`/`protected` field is rejected here too.
                                let v = self
                                    .classes
                                    .get(type_name)
                                    .and_then(|i| i.field_vis.get(&f.field).cloned());
                                self.enforce_member_vis(v, &f.field, f.span, true);
                                apply_subst(&t, &subst)
                            }
                            // Only emit "no field" when the class is real and not already poisoned
                            // (avoids double-reporting against an upstream error).
                            None => {
                                if self.classes.contains_key(type_name)
                                    && !matches!(init_ty, Ty::Error)
                                {
                                    self.err_coded(
                                        f.span,
                                        format!("type `{type_name}` has no field `{}`", f.field),
                                        "E-DESTRUCTURE-FIELD-UNKNOWN",
                                        None,
                                    );
                                }
                                Ty::Error
                            }
                        };
                        binds.push((f.binding.clone(), f.span, resolved));
                    }
                }
            }
            DestructurePat::List { binders, .. } => {
                let arity = binders.len();
                let elem = match &init_ty {
                    Ty::Error => Ty::Error,
                    // A `List<T>` carries no static length → refutable, `else` mandatory.
                    Ty::List(e) => {
                        refutable = true;
                        (**e).clone()
                    }
                    // A `[T; N]` is irrefutable iff its length matches the pattern arity (slice-3 payoff).
                    Ty::FixedList(e, n) => {
                        if *n != arity {
                            self.err_coded(
                                span,
                                format!(
                                    "destructuring binds {arity} element(s) but the value is `[{e}; {n}]` (length {n})"
                                ),
                                "E-FIXEDLIST-DESTRUCTURE-LEN",
                                Some(format!("bind exactly {n} element(s), or destructure a `List<{e}>` with an `else`")),
                            );
                        }
                        (**e).clone()
                    }
                    other => {
                        self.err_coded(
                            span,
                            format!("cannot list-destructure `{other}` — expected a list"),
                            "E-DESTRUCTURE-NOT-LIST",
                            Some("struct values destructure with `var Type { … } = …`".into()),
                        );
                        Ty::Error
                    }
                };
                for (name, bsp) in binders {
                    binds.push((name.clone(), *bsp, elem.clone()));
                }
            }
        }
        // `else` rules: required iff refutable; forbidden otherwise; and a present `else` must diverge.
        match (refutable, else_block) {
            (true, None) => {
                self.err_coded(
                    span,
                    "this destructuring can fail at runtime and needs an `else` that bails out",
                    "E-DESTRUCTURE-NEEDS-ELSE",
                    Some("add `else { … }` that returns/throws/breaks (a `List` has no static length)".into()),
                );
            }
            (false, Some(_)) => {
                self.err_coded(
                    span,
                    "this destructuring always succeeds, so it cannot have an `else`",
                    "E-DESTRUCTURE-ELSE-IRREFUTABLE",
                    Some(
                        "remove the `else` — an irrefutable destructuring binds unconditionally"
                            .into(),
                    ),
                );
            }
            _ => {}
        }
        if let Some(eb) = else_block {
            // The else block sees none of the binders (it runs only on the destructure-failed path).
            self.push_scope();
            self.check_block(eb);
            self.pop_scope();
            if !self.block_terminates(eb) {
                self.err_coded(
                    span,
                    "the destructuring `else` must not fall through — it has to bail out",
                    "E-DESTRUCTURE-ELSE-FALLTHROUGH",
                    Some(
                        "end every path of the `else` with `return`/`throw`/`break`/`continue`"
                            .into(),
                    ),
                );
            }
        }
        // Bind every binder into the current (enclosing) scope, immutable. A `void`/optional element
        // type is impossible here (init is a real List/class), so no E-VOID-CAPTURE guard is needed.
        // A duplicate binder would silently alias one slot on the VM (the SetLocal target collides) —
        // reject it up front (`var [a, a]` / `var P { x, x }`).
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (name, bsp, _) in &binds {
            if !seen.insert(name.as_str()) {
                self.err_coded(
                    *bsp,
                    format!("`{name}` is bound twice in this destructuring"),
                    "E-DESTRUCTURE-DUP-BIND",
                    Some("each binder must be distinct — rename one (e.g. `y: y2`)".into()),
                );
            }
        }
        for (name, bsp, ty) in binds {
            self.declare_binding(&name, ty, false, bsp);
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
    /// query (installation is the caller's job). Sources: `x instanceof T` (true ⇒ `T`; false ⇒ the
    /// remaining union members), `x == null` / `x != null` over a `T?` (both polarities), `!c` (flips
    /// polarity), and `a && b` (true side) / `a || b` (false side, De Morgan).
    pub(super) fn narrow_from_condition(
        &self,
        cond: &crate::ast::Expr,
        polarity: bool,
    ) -> Vec<(String, Ty)> {
        use crate::ast::{BinaryOp, Expr, UnaryOp};
        let mut out = Vec::new();
        match cond {
            Expr::InstanceOf {
                value, type_name, ..
            } => {
                if let Expr::Ident(name, _) = &**value {
                    let known = self.classes.contains_key(type_name)
                        || self.interfaces.contains_key(type_name);
                    if !known {
                        return out;
                    }
                    if polarity {
                        // then-branch: narrow to the tested type. `instanceof` carries no type
                        // arguments at runtime (`instanceof Box<int>` ≡ `instanceof Box`), so a
                        // generic class narrows with erased (poison) args — its generic members read
                        // as `mixed` (M-RT generics-all).
                        let arity = self
                            .classes
                            .get(type_name)
                            .map_or(0, |c| c.type_params.len());
                        out.push((
                            name.clone(),
                            Ty::Named(type_name.clone(), vec![Ty::Error; arity]),
                        ));
                    } else if let Some((Ty::Union(members), _)) = self.lookup_binding(name) {
                        // else-branch: drop the tested member (and any subtype of it) from the union.
                        let orig = members.len();
                        let rest: Vec<Ty> = members
                            .into_iter()
                            .filter(|m| {
                                !matches!(m, Ty::Named(n, _)
                                    if n == type_name || self.is_subtype(n, type_name))
                            })
                            .collect();
                        if !rest.is_empty() && rest.len() < orig {
                            out.push((name.clone(), Ty::union_of(rest)));
                        }
                    }
                }
            }
            // (Phorj has no `x == null` / `x != null` comparison — the checker rejects comparing a
            // `T?` to the null literal; optionals are tested via if-let / `??` / match-over-optional,
            // so there is no null-equality narrowing source here.)
            // `a && b` narrows the conjunction on its true side; `a || b` narrows on its false side
            // (De Morgan: `!(a || b)` ≡ `!a && !b`). The other polarity yields a disjunction — no
            // single narrowing — so it contributes nothing.
            Expr::Binary {
                op: BinaryOp::And,
                lhs,
                rhs,
                ..
            } if polarity => {
                out.extend(self.narrow_from_condition(lhs, true));
                out.extend(self.narrow_from_condition(rhs, true));
            }
            Expr::Binary {
                op: BinaryOp::Or,
                lhs,
                rhs,
                ..
            } if !polarity => {
                out.extend(self.narrow_from_condition(lhs, false));
                out.extend(self.narrow_from_condition(rhs, false));
            }
            // `!c` flips the polarity.
            Expr::Unary {
                op: UnaryOp::Not,
                expr,
                ..
            } => out.extend(self.narrow_from_condition(expr, !polarity)),
            _ => {}
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
            // A-6: an inferred binding (`foreach (xs as x)` desugars to `for (var x in xs)`) takes the
            // element type directly; an explicit `for (T x in xs)` validates `T` against it.
            let declared = if matches!(ty, crate::ast::Type::Infer(_)) {
                elem.clone()
            } else {
                let d = self.resolve_type(ty);
                if !self.ty_assignable(&elem, &d) {
                    self.err(
                        *span,
                        format!("loop variable `{name}` declared `{d}` but iterating `{elem}`"),
                    );
                }
                d
            };
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
