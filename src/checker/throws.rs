//! `impl Checker` — throws cluster (M-Decomp W2). See checker/mod.rs for the struct + entry points.

use super::*;

impl Checker {
    /// Flatten any union member of a resolved `throws` list into its individual exception types, so
    /// `throws A | B` becomes the set `{A, B}` — discharge and validation operate per type, and a
    /// call/`?` that propagates a union must discharge every member (M-faults 2b).
    pub(super) fn flatten_throws(tys: Vec<Ty>) -> Vec<Ty> {
        let mut out = Vec::new();
        for t in tys {
            match t {
                Ty::Union(members) => out.extend(members),
                other => out.push(other),
            }
        }
        out
    }

    /// Whether `t` is a thrown/declared/caught exception type — a class or interface that is `<:` the
    /// built-in `Error` marker (the `Error` interface itself qualifies). Anything else (a primitive,
    /// an enum, a function, a poison `<error>`) is not throwable/catchable (`E-THROW-TYPE`/
    /// `E-CATCH-TYPE`). Poison (`Ty::Error`) returns `true` so a prior error doesn't cascade.
    pub(super) fn is_error_type(&self, t: &Ty) -> bool {
        match t {
            Ty::Error => true,
            Ty::Named(n, _) => self.is_subtype(n, "Error"),
            _ => false,
        }
    }

    /// Whether a thrown `e` is *declared* by the enclosing function — `<:` some member of
    /// [`Self::cur_throws`]. The propagate (`?`) path and a bare `throw e` both discharge this way.
    pub(super) fn throws_declared(&self, e: &Ty) -> bool {
        self.cur_throws.iter().any(|d| self.ty_assignable(e, d))
    }

    /// Whether a thrown `e` is *caught* by an enclosing `try` we are currently inside the body of —
    /// `<:` some catch-clause type in any active [`Self::try_catch_stack`] frame (innermost or outer).
    pub(super) fn covered_by_try(&self, e: &Ty) -> bool {
        self.try_catch_stack
            .iter()
            .any(|frame| frame.iter().any(|c| self.ty_assignable(e, c)))
    }

    /// If `inner` is a call to a free function declaring a non-empty `throws` set, return that set
    /// (owned). The one operand shape a throws-mode `?` recognises this slice — `f()?` where `f`
    /// declares `throws E`. Method/native/closure throwing calls are a documented deferral (their
    /// throws are still discharged at the call site, just not propagable with `?`).
    pub(super) fn free_call_throws(&self, inner: &crate::ast::Expr) -> Option<Vec<Ty>> {
        if let crate::ast::Expr::Call { callee, .. } = inner {
            if let crate::ast::Expr::Ident(name, _) = &**callee {
                // Throws-mode `?` on an overloaded function uses the first overload's throws (the
                // common case is a single overload); a fully overload-aware `?`-throws is a deferral.
                if let Some(sig) = self.funcs.get(name).and_then(|v| v.first()) {
                    if !sig.throws.is_empty() {
                        return Some(sig.throws.clone());
                    }
                }
            }
        }
        None
    }

    /// Throws-mode `?` (M-faults 2b): `f()?` where `f` declares `throws E`. Checks the inner call
    /// (suppressing its own call-site discharge), requires every thrown type to be *declared* by the
    /// enclosing `throws` (propagation — not a `try`), records the node for erasure, and returns the
    /// call's *normal* return type. Returns `None` when `inner` is not a free throwing call, so the
    /// caller falls back to Result-mode (`check_propagate`) or `E-PROPAGATE-POSITION`.
    pub(super) fn try_throws_propagate(
        &mut self,
        inner: &crate::ast::Expr,
        span: Span,
    ) -> Option<Ty> {
        let throws = self.free_call_throws(inner)?;
        // The wrapped call propagates instead of discharging locally — suppress its own check.
        self.skip_throws_discharge = true;
        let t = self.check_expr(inner);
        for e in &throws {
            if !self.throws_declared(e) {
                self.err_coded(
                    span,
                    format!(
                        "`?` propagates `{e}`, but the enclosing function does not declare `throws {e}`"
                    ),
                    "E-CALL-UNHANDLED",
                    Some(format!(
                        "add `throws {e}` to the enclosing function, or wrap the call in `try`/`catch`"
                    )),
                );
            }
        }
        // Record for erasure: a throws-mode `?` is a checker-only marker (the call's own throw
        // unwinds), so the backend-facing AST keeps just the inner call (see `resolve_html`).
        self.html_resolutions.insert(span.start, inner.clone());
        Some(t)
    }

    /// Validate a function's `throws` declaration (M-faults 2b): the entry `main` may not declare it
    /// (`E-UNCAUGHT-THROW`); every declared type must implement `Error` (`E-THROW-TYPE`); and naming
    /// the bare `Error` root is too broad (`E-THROWS-TOO-BROAD` — declare the specific subtype so
    /// callers know what to catch). `throws` is resolved into `resolved` in declaration order.
    pub(super) fn validate_throws_decl(&mut self, f: &crate::ast::FunctionDecl, resolved: &[Ty]) {
        if f.name == "main" && !f.throws.is_empty() {
            self.err_coded(
                f.span,
                "`main` is the program entry point and may not declare `throws`",
                "E-UNCAUGHT-THROW",
                Some(
                    "handle every error inside `main` with `try`/`catch` — nothing may escape the entry point"
                        .into(),
                ),
            );
        }
        for t in resolved {
            match t {
                Ty::Error => {} // poison from an earlier error — don't cascade
                Ty::Named(n, _) if n == "Error" => {
                    self.err_coded(
                        f.span,
                        "`throws Error` is too broad — declare the specific exception type(s) you throw",
                        "E-THROWS-TOO-BROAD",
                        Some("e.g. `throws BadInput` so callers know exactly what to catch".into()),
                    );
                }
                _ if !self.is_error_type(t) => {
                    self.err_coded(
                        f.span,
                        format!(
                            "`throws {t}` is not allowed — a thrown type must implement `Error`"
                        ),
                        "E-THROW-TYPE",
                        Some("declare the throwing type `class Foo implements Error { … }`".into()),
                    );
                }
                _ => {}
            }
        }
    }
}
