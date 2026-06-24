//! `impl Checker` — program cluster (M-Decomp W2). See checker/mod.rs for the struct + entry points.

use super::*;

impl Checker {
    /// Phase 2 — check every function/method body.
    pub(super) fn check_program(&mut self, program: &Program) {
        use crate::ast::Item;
        // Reshape slice 2a: identifier casing is a hard, front-end-only rule. Run it first so its
        // diagnostics surface regardless of body-level errors (it is purely declaration-shaped).
        self.check_casing(program);
        // M5 S1: every file is packaged, never inferred. Empty ⇒ no declaration; a `core` root is
        // reserved for the standard library. (Strict folder=path and loose-mode `main`-only land
        // with the project model in S2 — `docs/specs/2026-06-18-m5-project-model-design.md`.)
        if program.package.is_empty() {
            self.err_coded(
                program.span,
                "every file must declare a package (e.g. `package Main;`) as its first line",
                "E-NO-PACKAGE",
                Some("add `package Main;` at the top of the file".into()),
            );
        } else if program.package[0] == "Core" {
            self.err_coded(
                program.span,
                "`Core` is a reserved package root (the standard library)",
                "E-RESERVED-PACKAGE",
                Some("use a different root, e.g. `package App;`".into()),
            );
        }
        // Reshape slice 2b: package + import path segments are PascalCase (`E-PKG-CASE`) — a 1:1
        // mapping to PHP namespaces with no casing transform. Front-end-only, so it cannot affect
        // byte-identity (every backend sees the same AST; the rule only gates which programs reach
        // them). The reserved `Main`/`Core` roots are already PascalCase. An empty package is left to
        // `E-NO-PACKAGE` above (the loop is empty), so the two never double-report.
        for seg in &program.package {
            if !is_pascal(seg) {
                self.err_coded(
                    program.span,
                    format!("package segment `{seg}` must be PascalCase"),
                    "E-PKG-CASE",
                    Some(format!("did you mean `package {}`?", to_pascal(seg))),
                );
            }
        }
        for item in &program.items {
            if let Item::Import {
                path, alias, span, ..
            } = item
            {
                for seg in path {
                    if !is_pascal(seg) {
                        self.err_coded(
                            *span,
                            format!("import segment `{seg}` must be PascalCase"),
                            "E-PKG-CASE",
                            Some(format!("did you mean `{}`?", to_pascal(seg))),
                        );
                    }
                }
                // An alias renames the call-site qualifier (`import A.B as C;`), so it occupies a
                // package-leaf position and follows the same PascalCase rule.
                if let Some(a) = alias {
                    if !is_pascal(a) {
                        self.err_coded(
                            *span,
                            format!("import alias `{a}` must be PascalCase"),
                            "E-PKG-CASE",
                            Some(format!("did you mean `as {}`?", to_pascal(a))),
                        );
                    }
                }
            }
        }
        for item in &program.items {
            match item {
                Item::Function(f) => self.check_function(f),
                Item::Class(c) => self.check_type_body(&c.name, &c.type_params, &c.members),
                // M-RT S8: a trait's method/ctor/hook bodies are checked once, in trait context
                // (correct spans, no double-reporting), with the trait's own collected members as
                // `this`. A trait has no type parameters this slice.
                Item::Trait(t) => self.check_type_body(&t.name, &[], &t.members),
                // Interface method signatures have no body to check (the conformance/graph
                // validation ran in `collect`); enums/imports/aliases have nothing here.
                Item::Enum(_)
                | Item::Interface(_)
                | Item::Import { .. }
                | Item::TypeAlias { .. } => {}
            }
        }
    }

    /// Check the method/constructor/hook bodies of a class or trait (M-RT S8 shares this between the
    /// two). `this` resolves to `type_name`'s already-collected [`ClassInfo`]; `type_params` are in
    /// scope across every body (empty for a trait this slice).
    pub(super) fn check_type_body(
        &mut self,
        type_name: &str,
        type_params: &[String],
        members: &[crate::ast::ClassMember],
    ) {
        use crate::ast::ClassMember;
        let prev = self.cur_class.replace(type_name.to_string());
        let prev_tp = std::mem::replace(&mut self.cur_class_type_params, type_params.to_vec());
        for m in members {
            match m {
                ClassMember::Method(f) => self.check_function(f),
                ClassMember::Constructor { params, body, .. } => {
                    let prev_ret = std::mem::replace(&mut self.cur_ret, Ty::Void);
                    // type params in scope for any `T` annotation in the body
                    self.active_type_params = type_params.to_vec();
                    self.push_scope();
                    // constructor params are in scope inside its body
                    let ctor = self
                        .classes
                        .get(type_name)
                        .map(|info| info.ctor.clone())
                        .unwrap_or_default();
                    for (p, t) in params.iter().zip(ctor) {
                        self.declare(&p.name, t, p.span);
                    }
                    self.check_body(body);
                    self.pop_scope();
                    self.active_type_params.clear();
                    self.cur_ret = prev_ret;
                }
                // A property hook (M-mut.7b) — type-check the `get` expression against the hook's
                // declared type and the `set` block with the assigned value `v` bound to that type,
                // both with `this` + the field scope live.
                ClassMember::Hook {
                    ty, name, get, set, ..
                } => {
                    self.active_type_params = type_params.to_vec();
                    let hook_ty = self.resolve_type(ty);
                    if let Some(e) = get {
                        self.push_scope();
                        let ety = self.check_expr(e);
                        if !self.ty_assignable(&ety, &hook_ty) {
                            self.err_coded(
                                Self::expr_span(e),
                                format!("`get` of `{name}` yields `{ety}`, expected `{hook_ty}`"),
                                "E-HOOK-TYPE",
                                None,
                            );
                        }
                        self.pop_scope();
                    }
                    if let Some((p, body)) = set {
                        let prev_ret = std::mem::replace(&mut self.cur_ret, Ty::Void);
                        self.push_scope();
                        let pty = self.resolve_type(&p.ty);
                        if !(self.ty_assignable(&pty, &hook_ty)
                            && self.ty_assignable(&hook_ty, &pty))
                        {
                            self.err_coded(
                                p.span,
                                format!(
                                    "`set` parameter of `{name}` is `{pty}`, expected the hook type `{hook_ty}`"
                                ),
                                "E-HOOK-TYPE",
                                Some(format!("declare it `set({hook_ty} {})`", p.name)),
                            );
                        }
                        // Bind `v` at the hook's type so the body checks consistently even when the
                        // declared parameter type mismatched.
                        self.declare(&p.name, hook_ty.clone(), p.span);
                        self.check_body(body);
                        self.pop_scope();
                        self.cur_ret = prev_ret;
                    }
                    self.active_type_params.clear();
                }
                ClassMember::Field { .. } => {}
            }
        }
        self.cur_class_type_params = prev_tp;
        self.cur_class = prev;
    }

    /// Check one free function or method body. Seeds a fresh scope with params. A generic function's
    /// type parameters are made active for the whole body so `T`-typed params/locals resolve to
    /// `Ty::Param` (M-RT S7). Functions never nest, so a flat set + clear is sufficient.
    pub(super) fn check_function(&mut self, f: &crate::ast::FunctionDecl) {
        // S0b: every function and method declares its return type — no exemptions where a return
        // slot exists (constructors and property hooks are separate `ClassMember` variants, so they
        // never reach here; expression-body lambdas infer and are not `FunctionDecl`s). Even `main`
        // must be annotated. Falling off the end of a value-carrying function was the soundness leak
        // the totality cluster closed; mandating the annotation makes every signature self-describing.
        if f.ret.is_none() {
            self.err_coded(
                f.span,
                format!("`{}` must declare a return type", f.name),
                "E-MISSING-RETURN-TYPE",
                Some(
                    "every function and method declares its return type — add `-> void` for a side-effecting function (or `-> Empty` to return the holdable empty value)"
                        .into(),
                ),
            );
        }
        // A method of a generic class sees both the class's type parameters and its own (M-RT
        // generics-all); `cur_class_type_params` is empty for free functions and non-generic classes.
        let mut active = self.cur_class_type_params.clone();
        active.extend(f.type_params.iter().cloned());
        self.active_type_params = active;
        let ret = match &f.ret {
            Some(t) => self.resolve_type(t),
            None => Ty::Void,
        };
        // Resolve + validate the declared throws set (type params still in scope), then make it the
        // active discharge context for the body (M-faults 2b).
        let throws = Self::flatten_throws(f.throws.iter().map(|t| self.resolve_type(t)).collect());
        self.validate_throws_decl(f, &throws);
        let prev_ret = std::mem::replace(&mut self.cur_ret, ret.clone());
        let prev_throws = std::mem::replace(&mut self.cur_throws, throws);
        let prev_main = std::mem::replace(&mut self.cur_is_main, f.name == "main");
        self.push_scope();
        for p in &f.params {
            let pty = self.resolve_type(&p.ty);
            self.declare(&p.name, pty, p.span);
        }
        self.check_body(&f.body);
        self.pop_scope();
        self.cur_ret = prev_ret;
        self.cur_throws = prev_throws;
        self.cur_is_main = prev_main;
        self.active_type_params.clear();
        // Totality: a non-`unit` function must return (or diverge) on every path (M-RT totality
        // cluster). Run after the body walk so all signatures are visible to the divergence analysis.
        // An `abstract` method (M-RT S6b) is a bodyless signature — exempt, like an interface method.
        if !f.modifiers.contains(&crate::ast::Modifier::Abstract) {
            self.check_return_totality(&ret, &f.body, f.span);
        }
    }

    /// A block diverges iff *any* of its statements diverges (everything after a diverging statement
    /// is dead, so the block as a whole never falls through).
    pub(super) fn block_terminates(&self, stmts: &[crate::ast::Stmt]) -> bool {
        stmts.iter().any(|s| self.stmt_terminates(s))
    }

    pub(super) fn stmt_terminates(&self, s: &crate::ast::Stmt) -> bool {
        use crate::ast::Stmt;
        match s {
            Stmt::Return { .. } => true,
            Stmt::Block(b, _) => self.block_terminates(b),
            Stmt::If {
                then_block,
                else_block: Some(eb),
                ..
            } => self.block_terminates(then_block) && self.block_terminates(eb),
            // An `if` with no `else` always has a path (the false branch) that falls through.
            Stmt::If {
                else_block: None, ..
            } => false,
            // A condition loop diverges only when it cannot exit: an always-true condition with no
            // `break` bound to it. A `do { … } while (…)` additionally diverges when its body diverges
            // on the guaranteed first iteration. A plain `while`/`for` body may run zero times, so a
            // diverging body alone does NOT make the loop diverge.
            Stmt::While {
                cond,
                body,
                post_cond,
                ..
            } => {
                (*post_cond && self.block_terminates(body))
                    || (is_true_lit(cond) && !breaks_this_loop(body))
            }
            Stmt::CFor { cond, body, .. } => {
                let cond_always = match cond {
                    None => true,
                    Some(c) => is_true_lit(c),
                };
                cond_always && !breaks_this_loop(body)
            }
            // `for (T x in iter)` always terminates over a finite list — never a divergence source.
            Stmt::Expr(e, _) => self.expr_is_never(e),
            // A `throw` always diverges (it unwinds out of the current frame; M-faults 2b).
            Stmt::Throw { .. } => true,
            // A `try` diverges iff control can never leave it normally: a `finally` that itself
            // diverges forces divergence; otherwise every exit edge must diverge — the body AND
            // every catch body. (An uncaught throw out of the body also diverges, so requiring the
            // body to terminate is sound — if it falls through normally, so does the `try`.)
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                if finally_block
                    .as_ref()
                    .is_some_and(|fb| self.block_terminates(fb))
                {
                    return true;
                }
                self.block_terminates(body)
                    && catches.iter().all(|c| self.block_terminates(&c.body))
            }
            _ => false,
        }
    }

    /// Whether an expression has the bottom type `never` — recognised *without* re-checking (emits no
    /// diagnostics): a call to a free function whose signature returns `never`, or an `if`/`match`
    /// expression every arm of which is itself `never`. Method/closure `never`-returns are deferred
    /// (need receiver typing) — see the design's KNOWN_ISSUES.
    pub(super) fn expr_is_never(&self, e: &crate::ast::Expr) -> bool {
        use crate::ast::Expr;
        match e {
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name, _) = &**callee {
                    // A `never`-typed fault intrinsic (`panic`/`todo`/`unreachable` — not `assert`,
                    // which is `unit`), or a user function declared `-> never` (M-faults 2a).
                    matches!(name.as_str(), "panic" | "todo" | "unreachable")
                        || self
                            .funcs
                            .get(name)
                            .and_then(|v| v.first())
                            .is_some_and(|s| s.ret == Ty::Never)
                } else {
                    false
                }
            }
            Expr::If {
                then_expr,
                else_expr,
                ..
            } => self.expr_is_never(then_expr) && self.expr_is_never(else_expr),
            Expr::Match { arms, .. } => {
                !arms.is_empty() && arms.iter().all(|a| self.expr_is_never(&a.body))
            }
            _ => false,
        }
    }

    /// Return-on-all-paths gate (M-RT totality cluster). A function whose declared return type carries
    /// a value must return (or diverge) on every path; `never` is the inverse — it must provably
    /// diverge. `void` (the no-annotation default), `Empty`, and `<error>` (poison) are exempt.
    pub(super) fn check_return_totality(
        &mut self,
        ret: &Ty,
        body: &[crate::ast::Stmt],
        span: Span,
    ) {
        match ret {
            // `void` (the common nothing, incl. the no-annotation default) and `Empty` (the holdable
            // nothing) are both value-less: a function may fall off the end (it implicitly produces
            // the empty value), so neither requires a `return` on all paths. `<error>` is poison.
            Ty::Void | Ty::Empty | Ty::Error => {}
            Ty::Never => {
                if !self.block_terminates(body) {
                    self.err_coded(
                        span,
                        "a `never` function must never return, but this body can fall through",
                        "E-NEVER-RETURN",
                        Some("a `-> never` function must diverge on every path (e.g. an infinite loop); drop the `never` return type if it can return normally".into()),
                    );
                }
            }
            _ => {
                if !self.block_terminates(body) {
                    self.err_coded(
                        span,
                        format!("function does not return `{ret}` on all paths"),
                        "E-MISSING-RETURN",
                        Some("add a `return` (or diverge) on every path — e.g. an `if` without an `else` leaves the false branch falling through".into()),
                    );
                }
            }
        }
    }

    /// Check a statement sequence in the *current* scope (no scope push), flagging the first
    /// unreachable statement after a diverging one (`W-UNREACHABLE`, once per dead region). Used for
    /// function, constructor, and `set`-hook bodies; `check_block` wraps it in a fresh scope for
    /// nested `{ … }` blocks.
    pub(super) fn check_body(&mut self, stmts: &[crate::ast::Stmt]) {
        let mut dead = false;
        let mut warned = false;
        for s in stmts {
            if dead && !warned {
                self.warn_coded(
                    Self::stmt_span(s),
                    "unreachable code: control never reaches this statement",
                    "W-UNREACHABLE",
                    Some(
                        "a preceding statement always returns or diverges; remove the dead code"
                            .into(),
                    ),
                );
                warned = true;
            }
            self.check_stmt(s);
            // Early-return narrowing (S5.3-T3): a guard `if (cond) { <diverges> }` means `cond` is
            // FALSE for every statement after it in this block — so install the false-polarity
            // narrowings into the current scope (they persist to the block's end, then pop with it).
            // Sound regardless of an `else`: reaching past a diverging then-block implies `cond` false.
            for (name, ty) in self.guard_if_narrowings(s) {
                let m = self.lookup_binding(&name).map(|(_, m)| m).unwrap_or(false);
                self.declare_binding(&name, ty, m, Self::stmt_span(s));
            }
            if self.stmt_terminates(s) {
                dead = true;
            }
        }
    }

    /// The narrowings a *guard* statement imposes on the rest of its block: an `if (cond) { … }` (no
    /// if-let binding) whose then-block diverges (`return`/`throw`/…) leaves `cond` false on the
    /// fall-through path, so the rest of the block sees the `polarity = false` narrowing (S5.3-T3).
    /// Empty for any other statement.
    fn guard_if_narrowings(&self, s: &crate::ast::Stmt) -> Vec<(String, Ty)> {
        use crate::ast::Stmt;
        if let Stmt::If {
            cond,
            bind: None,
            then_block,
            ..
        } = s
        {
            if self.block_terminates(then_block) {
                return self.narrow_from_condition(cond, false);
            }
        }
        Vec::new()
    }

    pub(super) fn check_block(&mut self, stmts: &[crate::ast::Stmt]) {
        self.push_scope();
        self.check_body(stmts);
        self.pop_scope();
    }
}
