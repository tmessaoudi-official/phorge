//! `impl Checker` — assign cluster (M-Decomp W2). See checker/mod.rs for the struct + entry points.

use super::*;

impl Checker {
    /// `name = value` local reassignment (M-mut.1): the binding must exist and be `mutable`, and the
    /// value assignable to its type.
    pub(super) fn check_local_reassign(
        &mut self,
        name: &str,
        vty: &Ty,
        target: &crate::ast::Expr,
        value: &crate::ast::Expr,
    ) {
        match self.lookup_binding(name) {
            None => {
                self.err_coded(
                    Self::expr_span(target),
                    format!("cannot assign to unknown variable `{name}`"),
                    "E-ASSIGN-UNKNOWN",
                    None,
                );
            }
            Some((bty, false)) => {
                self.err_coded(
                    Self::expr_span(target),
                    format!("`{name}` is immutable and cannot be reassigned"),
                    "E-ASSIGN-IMMUTABLE",
                    Some(format!(
                        "declare it `mutable` (e.g. `mutable {bty} {name} = …;`)"
                    )),
                );
            }
            Some((bty, true)) => {
                if !self.ty_assignable(vty, &bty) {
                    self.err_coded(
                        Self::expr_span(value),
                        format!("cannot assign `{vty}` to `{name}: {bty}`"),
                        "E-ASSIGN-TYPE",
                        None,
                    );
                }
            }
        }
    }

    /// `container[index] = value` value-type element set (M-mut.5). The container must be a `mutable`
    /// local `List<T>` or `Map<K, V>` (nested places `a[i][j]`/`this.f[i]` are a later slice →
    /// `E-ASSIGN-TARGET`). For a list the index must be `int` and the value a `T`; for a map the
    /// index must be the key type `K` and the value a `V`.
    pub(super) fn check_index_assign(
        &mut self,
        object: &crate::ast::Expr,
        index: &crate::ast::Expr,
        vty: &Ty,
        value: &crate::ast::Expr,
        span: Span,
    ) {
        let ity = self.check_expr(index);
        let name = match object {
            crate::ast::Expr::Ident(n, _) => n.clone(),
            _ => {
                self.err_coded(
                    Self::expr_span(object),
                    "the container of an element assignment must be a simple variable",
                    "E-ASSIGN-TARGET",
                    Some("nested element/field assignment (`a[i][j]`, `this.f[i]`) lands in a later slice".into()),
                );
                return;
            }
        };
        let (cty, mutable) = match self.lookup_binding(&name) {
            Some(b) => b,
            None => {
                self.err_coded(
                    Self::expr_span(object),
                    format!("cannot assign into unknown variable `{name}`"),
                    "E-ASSIGN-UNKNOWN",
                    None,
                );
                return;
            }
        };
        if !mutable {
            self.err_coded(
                Self::expr_span(object),
                format!("`{name}` is immutable; its elements cannot be set"),
                "E-ASSIGN-IMMUTABLE",
                Some(format!(
                    "declare it `mutable` (e.g. `mutable {cty} {name} = …;`)"
                )),
            );
            return;
        }
        match cty {
            Ty::List(elem) => {
                if !self.ty_assignable(&ity, &Ty::Int) {
                    self.err(span, format!("list index must be `int`, found `{ity}`"));
                }
                if !self.ty_assignable(vty, &elem) {
                    self.err_coded(
                        Self::expr_span(value),
                        format!("cannot set a `{vty}` element into `{name}: List<{elem}>`"),
                        "E-ASSIGN-TYPE",
                        None,
                    );
                }
            }
            Ty::Map(k, v) => {
                if !self.ty_assignable(&ity, &k) {
                    self.err(span, format!("map key must be `{k}`, found `{ity}`"));
                }
                if !self.ty_assignable(vty, &v) {
                    self.err_coded(
                        Self::expr_span(value),
                        format!("cannot set a `{vty}` value into `{name}: Map<{k}, {v}>`"),
                        "E-ASSIGN-TYPE",
                        None,
                    );
                }
            }
            other => {
                self.err_coded(
                    Self::expr_span(object),
                    format!("`{name}: {other}` is not indexable for assignment"),
                    "E-ASSIGN-TARGET",
                    Some("only `List<T>` and `Map<K, V>` support `container[i] = e`".into()),
                );
            }
        }
    }

    /// `o.f = e` / `this.f = e` shared-mutable instance field set (M-mut.6). The object must resolve
    /// to a concrete class (`this`, a local, or any field path `a.b` whose type is a class — handle
    /// semantics make a write through any binding visible everywhere); the field must exist
    /// (`E-ASSIGN-UNKNOWN`), be declared `mutable` (`E-ASSIGN-IMMUTABLE`), and the value must be
    /// assignable to its (generics-substituted) type (`E-ASSIGN-TYPE`). A `?.` target is rejected
    /// (`E-ASSIGN-TARGET`); nested index-into-field (`this.f[i] = e`) stays deferred to a later slice.
    pub(super) fn check_field_assign(
        &mut self,
        object: &crate::ast::Expr,
        name: &str,
        safe: bool,
        vty: &Ty,
        value: &crate::ast::Expr,
        span: Span,
    ) {
        if safe {
            self.err_coded(
                span,
                "cannot assign through a `?.` safe-access target",
                "E-ASSIGN-TARGET",
                Some("write `o.f = e` on a non-optional receiver".into()),
            );
            return;
        }
        // Static field write `ClassName.field = e` (M-mut.7): the head is a class name (not a local).
        // The field must be a `static mutable` of that class.
        if let crate::ast::Expr::Ident(cls, _) = object {
            if self.lookup_binding(cls).is_none() && self.classes.contains_key(cls) {
                let info = &self.classes[cls];
                match info.statics.get(name).cloned() {
                    None => {
                        self.err_coded(
                            span,
                            format!("`{cls}` has no static field `{name}` to assign"),
                            "E-ASSIGN-UNKNOWN",
                            None,
                        );
                    }
                    Some(fty) => {
                        let is_mut = info.static_mut.contains(name);
                        if !is_mut {
                            self.err_coded(
                                span,
                                format!("static field `{name}` of `{cls}` is immutable and cannot be assigned"),
                                "E-ASSIGN-IMMUTABLE",
                                Some(format!("declare it `static mutable {fty} {name} = …;`")),
                            );
                        } else if !self.ty_assignable(vty, &fty) {
                            self.err_coded(
                                Self::expr_span(value),
                                format!("cannot assign `{vty}` to static field `{name}: {fty}`"),
                                "E-ASSIGN-TYPE",
                                None,
                            );
                        }
                    }
                }
                return;
            }
        }
        let obj_ty = self.check_expr(object);
        let (class, cargs) = match &obj_ty {
            Ty::Error => return,
            Ty::Named(n, cargs) if self.classes.contains_key(n) => (n.clone(), cargs.clone()),
            other => {
                self.err_coded(
                    Self::expr_span(object),
                    format!("cannot set field `{name}` on non-class `{other}`"),
                    "E-ASSIGN-TARGET",
                    Some("field assignment requires a class instance".into()),
                );
                return;
            }
        };
        // A property hook (M-mut.7b) is resolved before a stored field: `o.name = e` runs its
        // `set`. Writing a hook with no `set` (read-only computed) is `E-HOOK-NO-SET`; otherwise the
        // value must be assignable to the hook's type. A hook is never `mutable`-gated (it has no
        // storage); the set body decides what to mutate.
        if let Some(h) = self
            .classes
            .get(&class)
            .and_then(|info| info.hooks.get(name))
        {
            let (hty, has_set) = (h.ty.clone(), h.has_set);
            if !has_set {
                self.err_coded(
                    span,
                    format!("property `{name}` of `{class}` is read-only (no `set`)"),
                    "E-HOOK-NO-SET",
                    Some("add a `set(T v) { … }` clause to assign it".into()),
                );
            } else if !self.ty_assignable(vty, &hty) {
                self.err_coded(
                    Self::expr_span(value),
                    format!("cannot assign `{vty}` to property `{name}: {hty}`"),
                    "E-ASSIGN-TYPE",
                    None,
                );
            }
            return;
        }
        let fty = match self.classes[&class].fields.get(name).cloned() {
            Some(t) => apply_subst(&t, &self.class_subst(&class, &cargs)),
            None => {
                self.err_coded(
                    span,
                    format!("`{class}` has no field `{name}` to assign"),
                    "E-ASSIGN-UNKNOWN",
                    None,
                );
                return;
            }
        };
        if !self.classes[&class].mutable_fields.contains(name) {
            self.err_coded(
                span,
                format!("field `{name}` of `{class}` is immutable and cannot be assigned"),
                "E-ASSIGN-IMMUTABLE",
                Some(format!(
                    "declare it `mutable` (e.g. `mutable {fty} {name};`)"
                )),
            );
            return;
        }
        if !self.ty_assignable(vty, &fty) {
            self.err_coded(
                Self::expr_span(value),
                format!("cannot assign `{vty}` to field `{name}: {fty}`"),
                "E-ASSIGN-TYPE",
                None,
            );
        }
    }

    /// `obj with { f = e, … }` (M-mut.4a): `obj` must be a concrete class; each overridden name must
    /// be one of its fields and each value assignable to that field's type. The result type is the
    /// class itself (a fresh instance). Codes `E-WITH-NONCLASS`/`E-WITH-FIELD`/`E-WITH-TYPE`.
    pub(super) fn check_clone_with(
        &mut self,
        object: &crate::ast::Expr,
        fields: &[(String, crate::ast::Expr)],
        span: Span,
    ) -> Ty {
        let obj_ty = self.check_expr(object);
        // Always check the override value expressions (surface nested errors regardless).
        let value_tys: Vec<Ty> = fields.iter().map(|(_, e)| self.check_expr(e)).collect();
        let class = match &obj_ty {
            Ty::Error => return Ty::Error,
            Ty::Named(name, _) if self.classes.contains_key(name) => name.clone(),
            other => {
                return self.err_coded(
                    span,
                    format!("`with` requires a class instance, found `{other}`"),
                    "E-WITH-NONCLASS",
                    Some(
                        "`with` produces a copy of a class instance with some fields replaced"
                            .into(),
                    ),
                );
            }
        };
        // Snapshot the class's field types (clone to drop the borrow before `err_coded` needs &mut).
        let field_tys = self.classes[&class].fields.clone();
        for ((name, _), vty) in fields.iter().zip(value_tys.iter()) {
            match field_tys.get(name) {
                None => {
                    self.err_coded(
                        Self::expr_span(object),
                        format!("`{class}` has no field `{name}` to set in `with`"),
                        "E-WITH-FIELD",
                        None,
                    );
                }
                Some(fty) => {
                    if !self.ty_assignable(vty, fty) {
                        self.err_coded(
                            span,
                            format!("cannot set `{name}: {fty}` to `{vty}` in `with`"),
                            "E-WITH-TYPE",
                            None,
                        );
                    }
                }
            }
        }
        obj_ty
    }

    /// `opt!` checked force-unwrap (M3 S2.5): `T?` → `T`. Every use is linted (`W-FORCE-UNWRAP`) to
    /// nudge toward `??`/`?.`/if-let; force-unwrapping a non-optional is `E-OPT-UNWRAP`.
    /// Is `name` a `Result`-shaped enum — exactly an `Ok` variant (arity 1) and an `Err` variant
    /// (arity 1)? `?` propagation is defined only over this shape (M-faults 2a).
    pub(super) fn is_result_enum(&self, name: &str) -> bool {
        self.enums.get(name).is_some_and(|e| {
            e.variants.get("Ok").is_some_and(|f| f.len() == 1)
                && e.variants.get("Err").is_some_and(|f| f.len() == 1)
        })
    }

    /// The type of variant `v`'s single payload field on enum `name`, with the enum's type parameters
    /// substituted by `args` (`Ok` payload of `Result<int, _>` ⇒ `int`).
    pub(super) fn result_payload(&self, name: &str, v: &str, args: &[Ty]) -> Ty {
        let theta = self.enum_subst(name, args);
        self.enums[name].variants[v]
            .first()
            .map_or(Ty::Error, |t| apply_subst(t, &theta))
    }

    /// `expr?` — Result-error propagation (M-faults 2a). Unwraps an `Ok` payload to its value, or
    /// early-returns the `Err` from the enclosing function. Requires the operand to be a `Result`-shaped
    /// enum AND the enclosing function (`cur_ret`) to return that *same* enum, with the operand's `Err`
    /// payload assignable to the function's (`E-PROPAGATE-CONTEXT`/`E-PROPAGATE-ERR`). Returns the
    /// unwrapped `Ok` payload type. Called only from a let-initializer; any other position is rejected
    /// by the `Expr::Propagate` arm in `check_expr` (`E-PROPAGATE-POSITION`).
    pub(super) fn check_propagate(&mut self, inner: &crate::ast::Expr, span: Span) -> Ty {
        let t = self.check_expr(inner);
        let (name, args) = match &t {
            Ty::Error => return Ty::Error,
            Ty::Named(n, a) if self.is_result_enum(n) => (n.clone(), a.clone()),
            other => {
                return self.err_coded(
                    span,
                    format!("`?` requires a `Result`-shaped operand (an enum with `Ok`/`Err` variants), found `{other}`"),
                    "E-PROPAGATE-CONTEXT",
                    Some("`?` unwraps `Ok` or early-returns `Err`".into()),
                );
            }
        };
        match self.cur_ret.clone() {
            Ty::Named(rn, rargs) if rn == name => {
                let err_in = self.result_payload(&name, "Err", &args);
                let err_ret = self.result_payload(&rn, "Err", &rargs);
                if !self.ty_assignable(&err_in, &err_ret) {
                    self.err_coded(
                        span,
                        format!("`?` propagates an `Err({err_in})` but the enclosing `{name}` carries `Err({err_ret})`"),
                        "E-PROPAGATE-ERR",
                        None,
                    );
                }
                self.result_payload(&name, "Ok", &args)
            }
            other => self.err_coded(
                span,
                format!("`?` early-returns the `Err`, so the enclosing function must return `{name}<…>`, but it returns `{other}`"),
                "E-PROPAGATE-CONTEXT",
                Some(format!("declare the function to return `{name}<…>`")),
            ),
        }
    }

    /// Recognize and check a fault intrinsic call (`panic`/`todo`/`unreachable`/`assert`); returns
    /// `Some(ty)` if `name` is one, else `None` (a normal call). Messages must be string *literals*
    /// (compile-time const) so both backends bake byte-identical fault text (M-faults 2a).
    pub(super) fn check_intrinsic_call(
        &mut self,
        name: &str,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Option<Ty> {
        match name {
            "panic" => {
                if args.len() != 1 {
                    self.err(span, "`panic` takes one string-literal message");
                } else {
                    self.require_str_literal(&args[0], span);
                }
                Some(Ty::Never)
            }
            "todo" | "unreachable" => {
                if !args.is_empty() {
                    self.err(span, format!("`{name}` takes no arguments"));
                }
                Some(Ty::Never)
            }
            "assert" => {
                if args.is_empty() || args.len() > 2 {
                    self.err(
                        span,
                        "`assert` takes a bool and an optional string-literal message",
                    );
                } else {
                    let c = self.check_expr(&args[0]);
                    if !self.ty_assignable(&c, &Ty::Bool) {
                        self.err(
                            span,
                            format!("`assert` condition must be `bool`, found `{c}`"),
                        );
                    }
                    if let Some(m) = args.get(1) {
                        self.require_str_literal(m, span);
                    }
                }
                Some(Ty::Void)
            }
            _ => None,
        }
    }

    /// Require `e` to be a string *literal* (one `StrPart::Literal`, no interpolation) — the fault
    /// intrinsics bake their message at compile time (M-faults 2a).
    pub(super) fn require_str_literal(&mut self, e: &crate::ast::Expr, span: Span) {
        if !matches!(e, crate::ast::Expr::Str(parts, _)
            if parts.len() == 1 && matches!(parts[0], crate::ast::StrPart::Literal(_)))
        {
            self.err_coded(
                span,
                "this intrinsic's message must be a plain string literal",
                "E-INTRINSIC-LITERAL",
                Some("interpolation/expressions aren't allowed here yet".into()),
            );
        }
        self.check_expr(e);
    }

    pub(super) fn check_force(&mut self, inner: &crate::ast::Expr, span: Span) -> Ty {
        let t = self.check_expr(inner);
        match t {
            Ty::Error => Ty::Error,
            Ty::Optional(inner_ty) => {
                self.warn_coded(
                    span,
                    "force-unwrap `!` asserts an optional is non-null and faults at runtime if it is null",
                    "W-FORCE-UNWRAP",
                    Some("prefer `??` (default), `?.` (safe access), or `if (var x = opt)` to handle null without a possible fault".into()),
                );
                *inner_ty
            }
            other => self.err_coded(
                span,
                format!("force-unwrap `!` requires an optional `T?`, found non-optional `{other}`"),
                "E-OPT-UNWRAP",
                Some("`!` unwraps a `T?` to `T`; a non-optional value is already non-null".into()),
            ),
        }
    }

    /// `E-OPT-USE`: a plain `.`/`.m()` was used on an optional (or `null`) receiver, which could
    /// dereference null. Steers the developer to `?.`, `??`, or a checked unwrap `!`.
    pub(super) fn err_opt_use(&mut self, span: Span, name: &str, recv: &Ty, verb: &str) -> Ty {
        self.err_coded(
            span,
            format!("cannot {verb} `{name}` of optional `{recv}`; use `?.` for null-safe access or unwrap with `!`"),
            "E-OPT-USE",
            Some(format!("`{name}` is only present when the receiver is non-null")),
        )
    }

    /// Wrap a member/method result in `Optional` for a `?.` access (a safe access yields a nullable
    /// result), without double-wrapping an already-optional member and leaving `Error` to cascade.
    pub(super) fn opt_wrap(t: Ty) -> Ty {
        match t {
            Ty::Error => Ty::Error,
            Ty::Optional(_) => t,
            other => Ty::Optional(Box::new(other)),
        }
    }
}
