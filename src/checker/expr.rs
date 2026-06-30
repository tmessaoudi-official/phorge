//! `impl Checker` — expr cluster (M-Decomp W2). See checker/mod.rs for the struct + entry points.

use super::*;

impl Checker {
    // ---- expressions ----
    /// Depth-guarded entry to expression checking. Every recursive descent (`check_binary`,
    /// `check_call`, … all call back through here) is bounded by [`MAX_EXPR_DEPTH`], so a
    /// pathologically deep AST faults cleanly instead of overflowing the walker's stack. `depth`
    /// is balanced on every path (the result is captured before the decrement).
    pub(super) fn check_expr(&mut self, expr: &crate::ast::Expr) -> Ty {
        self.depth += 1;
        let ty = if self.depth > MAX_EXPR_DEPTH {
            self.err(
                Self::expr_span(expr),
                format!("expression nests too deeply (limit {MAX_EXPR_DEPTH})"),
            )
        } else {
            self.check_expr_inner(expr)
        };
        // S2.1-broad: record the reified operand type of a call / method-call / field-read / index whose
        // result is concrete, so the VM compiler can specialize `box.get() + 1`, `box.value + 1`,
        // `xs[i] + 1` as the operand the checker proved (the static shape may erase to `mixed`). Keyed by
        // the node's own span; only `Call`/`Member`/`Index` (the shapes `ctype` can fail to specialize)
        // and only when the type carries operand information (not `Error`/`Void`/a type parameter), so
        // the map stays small. Non-operand types (e.g. a function-returning call) are dropped at the
        // compile boundary by `ty_to_cty`, so this never overrides `ctype`'s fn-value/class resolution.
        if matches!(
            expr,
            crate::ast::Expr::Call { .. }
                | crate::ast::Expr::Member { .. }
                | crate::ast::Expr::Index { .. }
        ) && Self::is_reifiable_operand(&ty)
        {
            self.reified_operands
                .insert(Self::expr_span(expr).start, ty.clone());
        }
        self.depth -= 1;
        ty
    }

    /// Whether `ty` carries operand information worth recording for the VM compiler's `ctype` (S2.1-broad)
    /// — a concrete scalar/container/instance whose erased static shape would otherwise collapse to
    /// `CTy::Other`. Excludes `Error`/`Void`/`Unit`/`Never`/`Null`/a bare type parameter (`Ty::Param`),
    /// which carry no useful operand type.
    fn is_reifiable_operand(ty: &Ty) -> bool {
        matches!(
            ty,
            Ty::Int
                | Ty::Float
                | Ty::Decimal
                | Ty::String
                | Ty::Bool
                | Ty::List(_)
                | Ty::Map(_, _)
                | Ty::Named(_, _)
                | Ty::Optional(_)
        )
    }

    pub(super) fn check_expr_inner(&mut self, expr: &crate::ast::Expr) -> Ty {
        use crate::ast::Expr;
        match expr {
            Expr::Int(_, _) => Ty::Int,
            Expr::Float(_, _) => Ty::Float,
            Expr::Decimal { .. } => Ty::Decimal,
            Expr::Bool(_, _) => Ty::Bool,
            Expr::Null(_) => Ty::Null,
            Expr::Str(parts, _) => self.check_str(parts), // Task 7
            Expr::Bytes(_, _) => Ty::Bytes,
            Expr::Ident(name, span) => match self.lookup(name) {
                Some(t) => t,
                None => {
                    // A4: bare named-function reference in value position — `fn_name` where
                    // `fn_name` is a top-level function, not a local. Return its function type so
                    // it can be passed as a first-class argument or stored in a variable.
                    if let Some(sigs) = self.funcs.get(name) {
                        // An overloaded function has no single first-class value (which overload?).
                        if sigs.len() > 1 {
                            return self.err_coded(
                                *span,
                                format!("`{name}` is overloaded — an overloaded function has no single first-class value"),
                                "E-OVERLOAD-FN-VALUE",
                                Some("call it directly, or wrap the intended overload in a lambda".into()),
                            );
                        }
                        let sig = &sigs[0];
                        let param_tys = sig.params.clone();
                        let ret_ty = sig.ret.clone();
                        return Ty::Function(param_tys, Box::new(ret_ty));
                    }
                    // A bare instance-field reference. Phorj requires `this.field` everywhere (like
                    // PHP's `$this->field`; no bare field access) — so this is always an error, with a
                    // distinct code per context: in a static method there is no instance at all
                    // (`E-STATIC-THIS`), otherwise the fix is to qualify it (`E-BARE-FIELD`).
                    if self.is_cur_field(name) {
                        if self.in_static_method {
                            return self.err_coded(
                                *span,
                                format!("instance field `{name}` is not accessible in a static method"),
                                "E-STATIC-THIS",
                                Some("a static method has no instance — pass the value as a parameter, use a static field, or make the method non-static".into()),
                            );
                        }
                        return self.err_coded(
                            *span,
                            format!("bare field reference `{name}` — write `this.{name}`"),
                            "E-BARE-FIELD",
                            Some(format!("Phorj has no bare field access (like PHP's `$this->`): qualify it as `this.{name}`")),
                        );
                    }
                    let cands = self.in_scope_names();
                    let hint = self
                        .nearest_name(name, &cands)
                        .map(|c| format!("did you mean `{c}`?"));
                    self.err_coded(
                        *span,
                        format!("unknown identifier `{name}`"),
                        "E-UNKNOWN-IDENT",
                        hint,
                    )
                }
            },
            Expr::This(span) if self.in_static_init => {
                self.err(*span, "`this` is not available in a static field initializer")
            }
            Expr::This(span) if self.in_static_method => self.err_coded(
                *span,
                "`this` is not available in a static method".to_string(),
                "E-STATIC-THIS",
                Some("a static method has no instance — access static members as `Class.member`, or make the method non-static".into()),
            ),
            Expr::This(span) => match &self.cur_class {
                // Inside a generic class body, `this` carries the class's own type parameters as
                // opaque `Ty::Param`s, so `this.value` (a `T` field) types as `T` and member access
                // substitutes identically (M-RT generics-all). Empty args for a non-generic class.
                Some(c) => {
                    let args = self
                        .cur_class_type_params
                        .iter()
                        .map(|p| Ty::Param(p.clone()))
                        .collect();
                    Ty::Named(c.clone(), args)
                }
                None => self.err(*span, "`this` is only valid inside a method"),
            },
            Expr::List(elems, span) => self.check_list(elems, *span), // Task 5
            Expr::Map(pairs, span) => self.check_map(pairs, *span),   // M-RT S3
            Expr::Unary { op, expr, span } => self.check_unary(*op, expr, *span),
            Expr::Binary { op, lhs, rhs, span } => self.check_binary(*op, lhs, rhs, *span),
            Expr::InstanceOf {
                value,
                type_name,
                span,
            } => self.check_instanceof(value, type_name, *span),
            Expr::Cast {
                value,
                type_name,
                span,
            } => self.check_cast(value, type_name, *span),
            Expr::Call { callee, args, span } => self.check_call(callee, args, *span), // Task 4
            // `<Type>f(args)` — a return-type overload selector (M-RT Slice C1).
            Expr::OverloadSelect { ty, call, span } => self.check_overload_select(ty, call, *span),
            // `parent.m(args)` / `parent(A).m(args)` — super/parent dispatch (M-RT super/parent).
            Expr::ParentCall {
                ancestor,
                method,
                args,
                span,
            } => self.check_parent_call(ancestor.as_deref(), method, args, *span),
            Expr::New(inner, span) => self.check_new(inner, *span),
            Expr::Spawn { call, span } => self.check_spawn(call, *span),
            Expr::Member {
                object,
                name,
                safe,
                span,
            } => self.check_member(object, name, *safe, *span),
            Expr::Index {
                object,
                index,
                span,
            } => self.check_index(object, index, *span), // Task 5
            Expr::Force { inner, span } => self.check_force(inner, *span),
            // A `?` in general expression position. Throws-mode `?` (a throwing call) is valid here
            // and returns the call's normal type (M-faults 2b). Result-mode `?` is *not* — it is
            // restricted to a let-initializer (the one position with a clean PHP hoist; M-faults 2a):
            // flag it, but still check the inner to avoid a cascade.
            Expr::Propagate { inner, span } => self.try_throws_propagate(inner, *span).unwrap_or_else(
                || {
                    self.check_expr(inner);
                    self.err_coded(
                        *span,
                        "Result-mode `?` propagation is only allowed as the whole initializer of a `var`/typed binding",
                        "E-PROPAGATE-POSITION",
                        Some("bind the call's result to a local first (`var r = f(); …`), then handle it".into()),
                    )
                },
            ),
            Expr::CloneWith {
                object,
                fields,
                span,
            } => self.check_clone_with(object, fields, *span),
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => self.check_match(scrutinee, arms, *span), // Task 8
            Expr::Range {
                start, end, span, ..
            } => self.check_range(start, end, *span),
            Expr::If {
                cond,
                then_expr,
                else_expr,
                span,
            } => self.check_if_expr(cond, then_expr, else_expr, *span),
            Expr::Lambda {
                params,
                ret,
                body,
                span,
            } => self.check_lambda(params, ret, body, *span),
            Expr::Html(parts, span) => self.check_html(parts, *span),
        }
    }

    /// `new <call>` (Feature C). Validates the inner is a class/enum-variant construction
    /// (`E-NEW-ON-NONCONSTRUCT` otherwise) and, when it is, type-checks it with the `under_new` flag so
    /// the construction does not also fire `E-NEW-REQUIRED`. Returns the construction's type (or the
    /// inner's type on the error path, to avoid a cascade). The node is later stripped by `unwrap_new`.
    pub(super) fn check_new(&mut self, inner: &crate::ast::Expr, span: Span) -> Ty {
        use crate::ast::Expr;
        match inner {
            Expr::Call { callee, .. } if self.is_construction_callee(callee) => {
                // Batch A: a class constructor's visibility is enforced here (the 7th access site).
                // Enum-variant construction has no visibility, so gate only real class names.
                if let Expr::Ident(cname, _) = &**callee {
                    if self.classes.contains_key(cname) {
                        self.enforce_ctor_vis(cname, span);
                    }
                }
                self.under_new = true;
                let t = self.check_expr(inner);
                self.under_new = false; // defensive — the construction call already took it
                t
            }
            _ => {
                let t = self.check_expr(inner); // check normally to avoid a cascade
                self.err_coded(
                    span,
                    "`new` is only for constructing a class or enum variant".to_string(),
                    "E-NEW-ON-NONCONSTRUCT",
                    Some("call a function without `new`; `new` precedes a class/variant construction".into()),
                );
                t
            }
        }
    }

    /// Whether `callee` names a class or enum variant *constructor* (Feature C) — an unshadowed
    /// identifier that is a known class name or enum-variant name. A local binding of the same name
    /// shadows it (then it is a value, never a construction).
    pub(super) fn is_construction_callee(&self, callee: &crate::ast::Expr) -> bool {
        match callee {
            crate::ast::Expr::Ident(name, _) => self.is_construction_name(name),
            _ => false,
        }
    }

    /// Whether `name` is a class or enum-variant constructor not shadowed by a local binding.
    pub(super) fn is_construction_name(&self, name: &str) -> bool {
        self.lookup(name).is_none()
            && (self.classes.contains_key(name)
                || self
                    .enums
                    .values()
                    .any(|info| info.variants.contains_key(name)))
    }

    pub(super) fn check_unary(
        &mut self,
        op: crate::ast::UnaryOp,
        expr: &crate::ast::Expr,
        span: Span,
    ) -> Ty {
        use crate::ast::UnaryOp;
        let t = self.check_expr(expr);
        if t == Ty::Error {
            return Ty::Error;
        }
        match op {
            UnaryOp::Neg if t == Ty::Int || t == Ty::Float || t == Ty::Decimal => t,
            UnaryOp::Neg => self.err(
                span,
                format!("unary `-` requires int, float, or decimal, found `{t}`"),
            ),
            UnaryOp::Not if t == Ty::Bool => Ty::Bool,
            UnaryOp::Not => self.err(span, format!("unary `!` requires `bool`, found `{t}`")),
            UnaryOp::BitNot if t == Ty::Int => Ty::Int,
            UnaryOp::BitNot => self.err(span, format!("unary `~` requires `int`, found `{t}`")),
        }
    }

    pub(super) fn check_binary(
        &mut self,
        op: crate::ast::BinaryOp,
        lhs: &crate::ast::Expr,
        rhs: &crate::ast::Expr,
        span: Span,
    ) -> Ty {
        use crate::ast::BinaryOp;
        let l = self.check_expr(lhs);
        let r = self.check_expr(rhs);
        if l == Ty::Error || r == Ty::Error {
            return match op {
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::Le
                | BinaryOp::Ge
                | BinaryOp::And
                | BinaryOp::Or => Ty::Bool,
                _ => Ty::Error,
            };
        }
        match op {
            // `+` is overloaded for string concatenation (Phase 1 string slice): `string + string`
            // → `string`. It is type-directed with **no coercion** — `string + int` stays an error
            // (the typed system kills JS's `"1" + 1` footgun). Only `+` concatenates; `-`/`*`/`/`/`%`
            // remain numeric-only.
            BinaryOp::Add if l == Ty::String && r == Ty::String => Ty::String,
            // `decimal` arithmetic. All of `+ - * / %` over decimals yield `decimal`; `decimal ⊕ int`
            // (either order) widens the int and stays `decimal` — the one ergonomic coercion (qty
            // math). `/` is *exact-or-fault* at runtime (a non-terminating quotient like `1d/3d`
            // faults; use `Decimal.div(a, b, scale, mode)` for a rounded one) and `%` is the exact
            // remainder — both type as `decimal` here. A `decimal ⊕ float` mix is the bug this
            // primitive prevents (`E-DECIMAL-FLOAT-MIX`). Checked before the int/float arm so a decimal
            // operand never reaches the matching-int-or-float test.
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem
                if l == Ty::Decimal || r == Ty::Decimal =>
            {
                let other = if l == Ty::Decimal { &r } else { &l };
                match other {
                    Ty::Decimal | Ty::Int => Ty::Decimal,
                    Ty::Float => self.err_coded(
                        span,
                        "cannot mix `decimal` and `float` in arithmetic — they are distinct types"
                            .to_string(),
                        "E-DECIMAL-FLOAT-MIX",
                        Some(
                            "convert explicitly (a `float` literal `1.5` and a `decimal` `1.50d` are \
                             different); keep money math in `decimal`"
                                .into(),
                        ),
                    ),
                    _ => self.err(
                        span,
                        format!("`decimal` arithmetic requires a `decimal` or `int` operand, found `{l}` and `{r}`"),
                    ),
                }
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                if (l == Ty::Int && r == Ty::Int) || (l == Ty::Float && r == Ty::Float) {
                    l
                } else if op == BinaryOp::Add && (l == Ty::String || r == Ty::String) {
                    self.err(span, format!("`+` concatenates two `string`s or adds two numbers — no coercion; found `{l}` and `{r}`"))
                } else {
                    self.err(span, format!("arithmetic requires matching int or float operands, found `{l}` and `{r}`"))
                }
            }
            // `**` power is type-directed like the other arithmetic ops but never concatenates:
            // `int**int→int`, `float**float→float`, anything else is an error (no coercion).
            BinaryOp::Pow => {
                if (l == Ty::Int && r == Ty::Int) || (l == Ty::Float && r == Ty::Float) {
                    l
                } else {
                    self.err(
                        span,
                        format!(
                            "`**` requires matching int or float operands, found `{l}` and `{r}`"
                        ),
                    )
                }
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                // `decimal` compares against `decimal` or `int` (numeric, scale-insensitive); same
                // operand rule as decimal arithmetic, a `float` mix is `E-DECIMAL-FLOAT-MIX`.
                let dec_ok = (l == Ty::Decimal && (r == Ty::Decimal || r == Ty::Int))
                    || (r == Ty::Decimal && l == Ty::Int);
                if (l == Ty::Int && r == Ty::Int) || (l == Ty::Float && r == Ty::Float) || dec_ok {
                    Ty::Bool
                } else if l == Ty::Decimal || r == Ty::Decimal {
                    self.err_coded(
                        span,
                        format!(
                            "cannot compare `decimal` with `{}`",
                            if l == Ty::Decimal { &r } else { &l }
                        ),
                        "E-DECIMAL-FLOAT-MIX",
                        Some("compare a `decimal` only with another `decimal` or an `int`".into()),
                    );
                    Ty::Bool
                } else {
                    self.err(span, format!("comparison requires matching int or float operands, found `{l}` and `{r}`"));
                    Ty::Bool
                }
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                // `decimal == int` (either order) is numeric equality (the operator-level int-widen);
                // every other cross-type pairing still requires explicit conversion.
                let dec_int =
                    (l == Ty::Decimal && r == Ty::Int) || (l == Ty::Int && r == Ty::Decimal);
                if l != r && !dec_int {
                    self.err(
                        span,
                        format!(
                            "cross-type comparison requires explicit conversion (`{l}` vs `{r}`)"
                        ),
                    );
                }
                Ty::Bool
            }
            BinaryOp::And | BinaryOp::Or => {
                if l != Ty::Bool || r != Ty::Bool {
                    self.err(
                        span,
                        format!("`&&`/`||` require `bool` operands, found `{l}` and `{r}`"),
                    );
                }
                Ty::Bool
            }
            BinaryOp::Coalesce => {
                match &l {
                    Ty::Error => Ty::Error,
                    Ty::Null => r.clone(), // `null ?? b` is always `b`
                    Ty::Optional(inner) => {
                        let inner = (**inner).clone();
                        if self.ty_assignable(&r, &inner) {
                            inner // `a ?? b` yields the unwrapped `T` when the default is a `T`
                        } else {
                            if !self.ty_assignable(&r, &Ty::Optional(Box::new(inner.clone()))) {
                                self.err(
                                span,
                                format!("`??` default of type `{r}` is not compatible with `{inner}?`"),
                            );
                            }
                            Ty::Optional(Box::new(inner)) // both sides optional → stays `T?`
                        }
                    }
                    other => self.err(
                        span,
                        format!("left operand of `??` must be optional, found `{other}`"),
                    ),
                }
            }
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                if l == Ty::Int && r == Ty::Int {
                    Ty::Int
                } else {
                    self.err(
                        span,
                        format!("bitwise operators require `int` operands, found `{l}` and `{r}`"),
                    )
                }
            }
            BinaryOp::Pipe => unreachable!("`|>` is lowered to a call in the parser"),
        }
    }

    /// `value instanceof TypeName` (M-RT S1): a runtime type test that always yields `bool`. The
    /// right operand must name a known class **or interface** (M-RT S2); the left operand must be a
    /// class instance (a `Ty::Named`). The smart-cast that narrows the operand inside an `if`
    /// then-block lives in `check_stmt`'s `Stmt::If` arm (it needs the surrounding block), not here.
    pub(super) fn check_instanceof(
        &mut self,
        value: &crate::ast::Expr,
        type_name: &str,
        span: Span,
    ) -> Ty {
        let v = self.check_expr(value);
        // M-RT S8: a trait is reuse, not a type — it cannot be an `instanceof` target (it is collected
        // into `classes` for member lookup, so this explicit guard is needed before the class check).
        if self.traits.contains(type_name) {
            return self.err_coded(
                span,
                format!("`{type_name}` is a trait, not a type"),
                "E-INSTANCEOF-TYPE",
                Some("a trait is reuse, not a type; test against a class or interface".into()),
            );
        }
        if !self.classes.contains_key(type_name) && !self.interfaces.contains_key(type_name) {
            return self.err_coded(
                span,
                format!(
                    "`instanceof` requires a class or interface name on the right, found `{type_name}`"
                ),
                "E-INSTANCEOF-TYPE",
                Some("only a declared class or interface can be tested with `instanceof`".into()),
            );
        }
        match &v {
            // A poisoned operand already reported its own error; still type the test as `bool`.
            Ty::Error => {}
            // A class instance — or a union (M-RT S4) / intersection (M-RT S5) of them — is the
            // meaningful left operand.
            Ty::Named(..) | Ty::Union(..) | Ty::Intersection(..) => {}
            other => {
                self.err_coded(
                    span,
                    format!("`instanceof` left operand must be a class instance, found `{other}`"),
                    "E-INSTANCEOF-TYPE",
                    Some("`instanceof` tests whether a class instance is of a given class".into()),
                );
            }
        }
        Ty::Bool
    }

    /// `value as TypeName` — the checked downcast (M4 casting axis 2). Validates the same operands as
    /// `instanceof` (a class/union/intersection value on the left; a class or interface name on the
    /// right) but types the result `TypeName?` (the cast yields the value when it really is a
    /// `TypeName` at runtime, else `null`). A *primitive* `as` (e.g. `x as int`) is rejected with a
    /// hint toward `Core.Conversion`/`parse*` — value conversion is a different axis from type assertion.
    pub(super) fn check_cast(
        &mut self,
        value: &crate::ast::Expr,
        type_name: &str,
        span: Span,
    ) -> Ty {
        let v = self.check_expr(value);
        // M4 as-matrix: a primitive target is a value CONVERSION (or identity), not a class downcast.
        // `value as int/float/string/bool/decimal` is typed here (and conversions are rewritten to a
        // native call), so it never falls into the class/interface path below.
        if matches!(type_name, "int" | "float" | "string" | "bool" | "decimal") {
            return self.check_cast_primitive(value, &v, type_name, span);
        }
        // A trait is reuse, not a type — same guard as `instanceof`.
        if self.traits.contains(type_name) {
            return self.err_coded(
                span,
                format!("`{type_name}` is a trait, not a type"),
                "E-CAST-TYPE",
                Some("a trait is reuse, not a type; cast to a class or interface".into()),
            );
        }
        let is_class = self.classes.contains_key(type_name);
        if !is_class && !self.interfaces.contains_key(type_name) {
            // Tailor the hint for a primitive target: `as` is *assertion*, not conversion.
            let hint = if is_builtin_type_name(type_name) {
                "`as` is a checked downcast, not a value conversion — use `Core.Conversion` (e.g. \
                 `Conversion.toFloat`/`truncate`) or `Core.Text.parseInt`/`parseFloat` to change a value's type"
            } else {
                "only a declared class or interface can be a cast target"
            };
            return self.err_coded(
                span,
                format!(
                    "`as` requires a class or interface name on the right, found `{type_name}`"
                ),
                "E-CAST-TYPE",
                Some(hint.into()),
            );
        }
        match &v {
            // A poisoned operand already reported; still type the cast as `TypeName?`.
            Ty::Error => {}
            // A class instance — or a union (S4) / intersection (S5) of them — is the meaningful left
            // operand (same surface as `instanceof`).
            Ty::Named(..) | Ty::Union(..) | Ty::Intersection(..) => {}
            other => {
                self.err_coded(
                    span,
                    format!("`as` left operand must be a class instance, found `{other}`"),
                    "E-CAST-TYPE",
                    Some(
                        "`as` downcasts a class instance to a more specific class or interface"
                            .into(),
                    ),
                );
            }
        }
        // Result is `TypeName?`. A generic class carries erased (poison) args — `instanceof`/`as` see
        // no runtime type arguments (`x as Box` ≡ `x as Box<…erased…>`), mirroring the narrow logic.
        let arity = self
            .classes
            .get(type_name)
            .map_or(0, |c| c.type_params.len());
        Ty::Optional(Box::new(Ty::Named(
            type_name.to_string(),
            vec![Ty::Error; arity],
        )))
    }

    /// `value as <primitive>` (M4 as-matrix, S1 — concrete-primitive sources). Types the result per
    /// the **Unified, fallibility-typed** model (lossless → total `T`, lossy/fallible → `T?`) and, for
    /// a real conversion, records a span-keyed rewrite to a leaf-qualified native call
    /// (`Conversion.toFloat(v)` / `Text.parseInt(v)` …) that the backends resolve by `index_of_by_leaf`
    /// without an import. **Identity** (`T as T`) is total, fires `W-REDUNDANT-CAST`, and is NOT
    /// rewritten — the `Cast` node survives and each backend emits the value unchanged. Bool cells,
    /// `float as decimal`, `string as decimal`, and union/erased *assertion* sources land in later
    /// slices (rejected for now with a forward-looking hint). No new `Op`/`Value`.
    fn check_cast_primitive(
        &mut self,
        value: &crate::ast::Expr,
        v: &Ty,
        target: &str,
        span: Span,
    ) -> Ty {
        let target_ty = match target {
            "int" => Ty::Int,
            "float" => Ty::Float,
            "string" => Ty::String,
            "bool" => Ty::Bool,
            "decimal" => Ty::Decimal,
            _ => unreachable!("guarded by the caller"),
        };
        // Identity — already the target type. Total, no rewrite, redundant-cast lint.
        if *v == target_ty {
            self.warn_coded(
                span,
                format!("redundant cast: value is already `{target}`"),
                "W-REDUNDANT-CAST",
                Some("remove the `as` — the value already has this type".into()),
            );
            return target_ty;
        }
        // A poisoned operand already reported; type the cast as the (likely) target to avoid a cascade.
        if matches!(v, Ty::Error) {
            return target_ty;
        }
        // Union source (S2): the value is one-of, so `as P` is a runtime ASSERTION → `P?` (the value
        // when its runtime variant is `P`, else null) — except `as string`, which is the total
        // `toString` conversion (every value renders). `decimal` is deferred (its PHP carrier is a
        // string, indistinguishable from a `string` union member).
        if matches!(v, Ty::Union(_)) {
            let assert_cell: Option<&str> = match target {
                "int" => Some("asInt"),
                "float" => Some("asFloat"),
                "bool" => Some("asBool"),
                _ => None,
            };
            if let Some(name) = assert_cell {
                self.record_cast_call("Conversion", name, value, span);
                return Ty::Optional(Box::new(target_ty));
            }
            if target == "string" {
                self.record_cast_call("Conversion", "toString", value, span);
                return Ty::String;
            }
            // `as decimal` on a union — deferred (carrier conflation).
            self.err_coded(
                span,
                format!("`{v} as {target}` is not a supported conversion"),
                "E-CAST-TYPE",
                Some(
                    "`as decimal` on a union is deferred (decimal shares PHP's string carrier)"
                        .into(),
                ),
            );
            return Ty::Error;
        }
        // (source, target) → (qualifier leaf, native name, result type). `opt(t)` = `T?`.
        let opt = |t: Ty| Ty::Optional(Box::new(t));
        let cell: Option<(&str, &str, Ty)> = match (v, target) {
            (Ty::Int, "float") => Some(("Conversion", "toFloat", Ty::Float)),
            (Ty::Int, "decimal") => Some(("Conversion", "intToDecimal", Ty::Decimal)),
            (Ty::Float, "int") => Some(("Conversion", "floatToIntExact", opt(Ty::Int))),
            (Ty::Decimal, "int") => Some(("Conversion", "decimalToIntExact", opt(Ty::Int))),
            (Ty::Decimal, "float") => Some(("Conversion", "decimalToFloat", Ty::Float)),
            (Ty::String, "int") => Some(("Text", "parseInt", opt(Ty::Int))),
            (Ty::String, "float") => Some(("Text", "parseFloat", opt(Ty::Float))),
            // S4 decimal extras — float via the shortest-string parse; string via `Decimal.of`.
            (Ty::Float, "decimal") => Some(("Conversion", "floatToDecimal", opt(Ty::Decimal))),
            (Ty::String, "decimal") => Some(("Decimal", "of", opt(Ty::Decimal))),
            // S3 bool cells — total numeric↔bool (explicit `!= 0` / `1`/`0`), strict string parse.
            (Ty::Int, "bool") => Some(("Conversion", "intToBool", Ty::Bool)),
            (Ty::Float, "bool") => Some(("Conversion", "floatToBool", Ty::Bool)),
            (Ty::Decimal, "bool") => Some(("Conversion", "decimalToBool", Ty::Bool)),
            (Ty::Bool, "int") => Some(("Conversion", "boolToInt", Ty::Int)),
            (Ty::Bool, "float") => Some(("Conversion", "boolToFloat", Ty::Float)),
            (Ty::Bool, "decimal") => Some(("Conversion", "boolToDecimal", Ty::Decimal)),
            (Ty::String, "bool") => Some(("Text", "parseBool", opt(Ty::Bool))),
            // any primitive → string is total (Convert.toString is generic).
            (Ty::Int | Ty::Float | Ty::Decimal | Ty::Bool, "string") => {
                Some(("Conversion", "toString", Ty::String))
            }
            _ => None,
        };
        match cell {
            Some((leaf, name, ret)) => {
                self.record_cast_call(leaf, name, value, span);
                ret
            }
            None => {
                // Deferred cell (bool, float→decimal, string→decimal) or an impossible pair.
                self.err_coded(
                    span,
                    format!("`{v} as {target}` is not a supported conversion"),
                    "E-CAST-TYPE",
                    Some(
                        "convert via `Core.Conversion` / `Core.Text.parse*`; bool/decimal-from-float/string \
                         casts ship in a later slice"
                            .into(),
                    ),
                );
                Ty::Error
            }
        }
    }

    /// Record a primitive `as`-cast rewrite: `value as T` ⇒ `Leaf.name(value)` (a leaf-qualified
    /// native call the backends resolve by `index_of_by_leaf` without an import), keyed by the cast
    /// node's span. The synthetic call must be full-arity — the default-param fill pass does not see a
    /// post-check rewrite — so `Text.parseFloat` (which takes `(string, bool permissive=false)`) gets
    /// its `false` (strict) default supplied explicitly.
    fn record_cast_call(&mut self, leaf: &str, name: &str, value: &crate::ast::Expr, span: Span) {
        use crate::ast::Expr;
        let callee = Expr::Member {
            object: Box::new(Expr::Ident(leaf.to_string(), span)),
            name: name.to_string(),
            safe: false,
            span,
        };
        let mut call_args = vec![value.clone()];
        if name == "parseFloat" {
            call_args.push(Expr::Bool(false, span));
        }
        self.cast_resolutions.insert(
            span.start,
            Expr::Call {
                callee: Box::new(callee),
                args: call_args,
                span,
            },
        );
    }

    // ---- stubs replaced in later tasks ----
    pub(super) fn check_str(&mut self, parts: &[crate::ast::StrPart]) -> Ty {
        use crate::ast::StrPart;
        for part in parts {
            if let StrPart::Expr(e) = part {
                let t = self.check_expr(e);
                let ok = matches!(
                    t,
                    Ty::Int | Ty::Float | Ty::Decimal | Ty::Bool | Ty::String | Ty::Error
                );
                if !ok {
                    let sp = Self::expr_span(e);
                    self.err(sp, format!("type `{t}` cannot be interpolated into a string (only primitives auto-stringify in M1)"));
                }
            }
        }
        Ty::String
    }

    /// Check an `html"…"` literal (core.html Wave 3) and record its type-directed desugaring.
    ///
    /// Each literal chunk becomes `html.raw(chunk)` (author markup is trusted); each `{e}` hole is
    /// resolved **by `e`'s type**: an `Html` value embeds as-is (already safe — lets you nest
    /// builders / other `html"…"`); a `string` is wrapped in `html.text(e)` (auto-escaped — the safe
    /// default for raw data); an `int`/`float`/`bool` is stringified then escaped; anything else is a
    /// clean `E-HTML-HOLE`. The default hole behavior is **escape** — injecting trusted markup
    /// requires writing `{html.raw(x)}` explicitly (unsafe is long, safe is short). The pieces are
    /// concatenated with `html.concat([…])`; the whole tree uses only Wave-1/2 natives, which are
    /// already byte-identical across the three backends, so parity is inherited, not re-proved.
    ///
    /// The replacement is stored by the literal's `Span.start` and applied by [`resolve_html`] after
    /// checking — `check` itself never mutates the AST (it borrows it). Returns [`Ty::Html`].
    pub(super) fn check_html(&mut self, parts: &[crate::ast::StrPart], span: Span) -> Ty {
        use crate::ast::{Expr, StrPart};
        // `html"…"` desugars to `<leaf>.raw/.text/.concat` calls, so the program must import
        // core.html. Resolve whatever leaf maps to it (robust to `import core.html as h;`).
        let leaf = self
            .imports
            .iter()
            .find(|(_, full)| full.as_str() == "Core.Html")
            .map(|(leaf, _)| leaf.clone());
        let leaf = match leaf {
            Some(l) => l,
            None => {
                return self.err_coded(
                    span,
                    "`html\"…\"` requires the Core.Html module",
                    "E-HTML-IMPORT",
                    Some("add `import Core.Html;` (or `import Core.Html as h;`)".into()),
                );
            }
        };
        // Build `<leaf>.<name>(args)` as a plain `Member`-headed call (resolved like any namespaced
        // native by the backends, via the import map). All synthetic nodes carry the literal's span.
        let call = |name: &str, args: Vec<Expr>| -> Expr {
            Expr::Call {
                callee: Box::new(Expr::Member {
                    object: Box::new(Expr::Ident(leaf.clone(), span)),
                    name: name.to_string(),
                    safe: false,
                    span,
                }),
                args,
                span,
            }
        };
        let str_lit = |s: &str| Expr::Str(vec![StrPart::Literal(s.to_string())], span);

        let mut elems: Vec<Expr> = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                StrPart::Literal(chunk) => elems.push(call("raw", vec![str_lit(chunk)])),
                StrPart::Expr(e) => {
                    let t = self.check_expr(e);
                    match t {
                        // already an Html fragment — embed verbatim (no double-escape).
                        Ty::Html => elems.push((**e).clone()),
                        // raw text — escape it (the safe default).
                        Ty::String => elems.push(call("text", vec![(**e).clone()])),
                        // primitives stringify (via a one-hole string interp) then escape, for
                        // uniformity — numbers carry no markup but go through the same wall.
                        Ty::Int | Ty::Float | Ty::Bool => {
                            let stringified =
                                Expr::Str(vec![StrPart::Expr(Box::new((**e).clone()))], span);
                            elems.push(call("text", vec![stringified]));
                        }
                        // a poisoned hole already reported its own error; keep going without piling
                        // on, and emit *something* well-typed so the replacement stays buildable.
                        Ty::Error => elems.push(call("text", vec![str_lit("")])),
                        other => {
                            self.err_coded(
                                Self::expr_span(e),
                                format!(
                                    "cannot interpolate `{other}` into html; render it to a string or Html first"
                                ),
                                "E-HTML-HOLE",
                                Some(
                                    "wrap it with `Html.text(…)`/`Html.raw(…)`, or build it with the html builders"
                                        .into(),
                                ),
                            );
                            elems.push(call("text", vec![str_lit("")]));
                        }
                    }
                }
            }
        }

        let replacement = call("concat", vec![Expr::List(elems, span)]);
        self.html_resolutions.insert(span.start, replacement);
        Ty::Html
    }

    /// The source span of an expression (used to position errors precisely).
    pub(super) fn expr_span(e: &crate::ast::Expr) -> Span {
        use crate::ast::Expr;
        match e {
            Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Bool(_, s)
            | Expr::Str(_, s)
            | Expr::Bytes(_, s)
            | Expr::Ident(_, s)
            | Expr::List(_, s)
            | Expr::Map(_, s) => *s,
            Expr::Null(s) | Expr::This(s) => *s,
            Expr::Decimal { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::InstanceOf { span, .. }
            | Expr::Cast { span, .. }
            | Expr::Call { span, .. }
            | Expr::Member { span, .. }
            | Expr::Index { span, .. }
            | Expr::Force { span, .. }
            | Expr::Propagate { span, .. }
            | Expr::Match { span, .. }
            | Expr::Range { span, .. }
            | Expr::If { span, .. }
            | Expr::Lambda { span, .. }
            | Expr::CloneWith { span, .. }
            | Expr::OverloadSelect { span, .. }
            | Expr::ParentCall { span, .. }
            | Expr::New(_, span)
            | Expr::Spawn { span, .. }
            | Expr::Html(_, span) => *span,
        }
    }

    pub(super) fn check_list(&mut self, elems: &[crate::ast::Expr], span: Span) -> Ty {
        if elems.is_empty() {
            // empty list element type cannot be inferred without an expected type;
            // the §6 sample has no empty list (YAGNI to thread expected types now).
            return self.err(span, "cannot infer element type of empty list literal");
        }
        let first = self.check_expr(&elems[0]);
        for e in &elems[1..] {
            let t = self.check_expr(e);
            if !self.ty_assignable(&t, &first) && !self.ty_assignable(&first, &t) {
                self.err(
                    span,
                    format!("list elements must share one type; found `{first}` and `{t}`"),
                );
            }
        }
        Ty::List(Box::new(first))
    }

    /// `[k => v, …]` (M-RT S3): infer the key type `K` and value type `V`, unifying across pairs
    /// (each must share one type, like list elements). The parser guarantees ≥1 pair (an empty `[]`
    /// is the empty *list*). Keys must be the hashable subset — `int`/`bool`/`string` — else
    /// `E-MAP-KEY` (a `float`/instance/list key has no `HKey`). Result: `Ty::Map(K, V)`.
    pub(super) fn check_map(
        &mut self,
        pairs: &[(crate::ast::Expr, crate::ast::Expr)],
        span: Span,
    ) -> Ty {
        let (k0, v0) = &pairs[0];
        let key_ty = self.check_expr(k0);
        let val_ty = self.check_expr(v0);
        for (k, v) in &pairs[1..] {
            let kt = self.check_expr(k);
            if !self.ty_assignable(&kt, &key_ty) && !self.ty_assignable(&key_ty, &kt) {
                self.err(
                    span,
                    format!("map keys must share one type; found `{key_ty}` and `{kt}`"),
                );
            }
            let vt = self.check_expr(v);
            if !self.ty_assignable(&vt, &val_ty) && !self.ty_assignable(&val_ty, &vt) {
                self.err(
                    span,
                    format!("map values must share one type; found `{val_ty}` and `{vt}`"),
                );
            }
        }
        if !matches!(key_ty, Ty::Int | Ty::Bool | Ty::String | Ty::Error) {
            return self.err_coded(
                span,
                format!("map key type must be `int`, `bool`, or `string`, found `{key_ty}`"),
                "E-MAP-KEY",
                None,
            );
        }
        Ty::Map(Box::new(key_ty), Box::new(val_ty))
    }

    /// Static bounds check for a fixed-length list `[T; N]` (Phase 1 types slice). Only a *literal*
    /// index is known at compile time: a constant `< 0` or `>= len` is `E-FIXEDLIST-BOUNDS`. A
    /// non-literal index is left to the runtime bounds check (the same `Op::Index` as a list).
    pub(super) fn fixedlist_static_bounds(
        &mut self,
        index: &crate::ast::Expr,
        len: usize,
        elem: &Ty,
        span: Span,
    ) {
        if let crate::ast::Expr::Int(k, _) = index {
            if *k < 0 || (*k as usize) >= len {
                self.err_coded(
                    span,
                    format!("index {k} is out of bounds for `[{elem}; {len}]`"),
                    "E-FIXEDLIST-BOUNDS",
                    Some(format!("valid indices are 0..{len}")),
                );
            }
        }
    }

    pub(super) fn check_index(
        &mut self,
        object: &crate::ast::Expr,
        index: &crate::ast::Expr,
        span: Span,
    ) -> Ty {
        let obj = self.check_expr(object);
        let idx = self.check_expr(index);
        match obj {
            Ty::List(elem) => {
                if !self.ty_assignable(&idx, &Ty::Int) {
                    self.err(span, format!("list index must be `int`, found `{idx}`"));
                }
                *elem
            }
            // `pair[i]` on a `[T; N]`: like a list read, but a *literal* index is bounds-checked at
            // compile time (`pair[5]` on `[int; 2]` is `E-FIXEDLIST-BOUNDS`); a dynamic index falls
            // back to the runtime bounds check, same as a list (Phase 1 types slice).
            Ty::FixedList(elem, n) => {
                if !self.ty_assignable(&idx, &Ty::Int) {
                    self.err(span, format!("list index must be `int`, found `{idx}`"));
                }
                self.fixedlist_static_bounds(index, n, &elem, span);
                *elem
            }
            // `m[k]` (M-RT S3): the index must match the key type; the result is the value type. A
            // missing key faults at runtime (byte-identical present-key, like list-OOB the fault path
            // is excluded from differential gating).
            Ty::Map(k, v) => {
                if !self.ty_assignable(&idx, &k) {
                    self.err(span, format!("map index must be `{k}`, found `{idx}`"));
                }
                *v
            }
            Ty::Error => Ty::Error,
            other => self.err(span, format!("type `{other}` cannot be indexed")),
        }
    }

    /// `start..end` / `start..=end`: both bounds must be `int`; the range's type is `List<int>` (its
    /// only role this slice is `for … in`). A non-int bound is `E-RANGE-TYPE` (decision S1-R).
    pub(super) fn check_range(
        &mut self,
        start: &crate::ast::Expr,
        end: &crate::ast::Expr,
        span: Span,
    ) -> Ty {
        let s = self.check_expr(start);
        let e = self.check_expr(end);
        let ok = |t: &Ty| matches!(t, Ty::Int | Ty::Error);
        if !ok(&s) || !ok(&e) {
            return self.err_coded(
                span,
                format!("range bounds must be `int`, found `{s}` and `{e}`"),
                "E-RANGE-TYPE",
                None,
            );
        }
        Ty::List(Box::new(Ty::Int))
    }

    /// Expression `if`: the condition must be `bool` and both arms must share one type `T`, which is
    /// the expression's type. (`else` is mandatory at the parser, so there is no missing-else case
    /// here.) Mirrors `check_match`'s arm-unification rule (M3 S1.3).
    pub(super) fn check_if_expr(
        &mut self,
        cond: &crate::ast::Expr,
        then_e: &crate::ast::Expr,
        else_e: &crate::ast::Expr,
        span: Span,
    ) -> Ty {
        let c = self.check_expr(cond);
        if !self.ty_assignable(&c, &Ty::Bool) {
            self.err(span, format!("`if` condition must be `bool`, found `{c}`"));
        }
        let t = self.check_expr(then_e);
        let e = self.check_expr(else_e);
        if t != Ty::Error
            && e != Ty::Error
            && !self.ty_assignable(&e, &t)
            && !self.ty_assignable(&t, &e)
        {
            self.err(
                span,
                format!("`if` branches must share one type; found `{t}` and `{e}`"),
            );
        }
        if t == Ty::Error {
            e
        } else {
            t
        }
    }

    /// Type-check a lambda expression (M3 S3, Task 3). Returns `Ty::Function(params, ret)`.
    ///
    /// Type-checks a lambda. A method-body lambda **may** capture `this` (Phase 1 closures slice): it
    /// is captured by value (the `Rc` instance handle, so mutations stay live), `this` types as the
    /// enclosing class via `cur_class`, and the two backends + PHP all bind the same receiver. The one
    /// place it stays rejected is a **field/static initializer** (`in_field_init`): the instance is
    /// only partially built when an initializer runs, so capturing the receiver is the F8 footgun.
    pub(super) fn check_lambda(
        &mut self,
        params: &[crate::ast::Param],
        ret: &Option<crate::ast::Type>,
        body: &crate::ast::LambdaBody,
        span: Span,
    ) -> Ty {
        use crate::ast::LambdaBody;
        // A field-default lambda may not capture `this` (partially-built instance, F8).
        if self.in_field_init && crate::ast::lambda_uses_this(body) {
            self.err_coded(
                span,
                "a field-initializer lambda cannot capture `this` — the instance is not fully built yet",
                "E-LAMBDA-THIS",
                Some("move the closure into the constructor body, or capture a specific value (`var v = this.x;`) instead".into()),
            );
        }
        let param_tys: Vec<Ty> = params.iter().map(|p| self.resolve_type(&p.ty)).collect();
        // Save and replace the current return type (a lambda has its own return scope).
        let saved_ret = std::mem::replace(&mut self.cur_ret, Ty::Error);
        // A lambda is a separate callable: it declares no `throws`, and it does not see the lexical
        // `try` it is written inside (it may be invoked elsewhere — e.g. passed to a native). So a
        // `throw` in its body discharges against an empty context (M-faults 2b).
        let saved_throws = std::mem::take(&mut self.cur_throws);
        let saved_try = std::mem::take(&mut self.try_catch_stack);
        let saved_main = std::mem::replace(&mut self.cur_is_main, false);
        self.push_scope();
        for p in params {
            let pty = self.resolve_type(&p.ty);
            self.declare(&p.name, pty, p.span);
        }
        let ret_ty = match body {
            LambdaBody::Expr(e) => {
                let inferred = self.check_expr(e);
                if let Some(rt) = ret {
                    let declared = self.resolve_type(rt);
                    if !self.ty_assignable(&inferred, &declared) {
                        self.err_assign(span, &inferred, &declared);
                    }
                    declared
                } else {
                    inferred
                }
            }
            LambdaBody::Block(stmts) => {
                // A2/F10: an explicit `-> T` annotation is required for statement-body lambdas.
                match ret {
                    Some(rt) => {
                        let declared = self.resolve_type(rt);
                        self.cur_ret = declared.clone();
                        // Batch F (finding #6): a statement-body lambda must return on all paths just
                        // like a free fn/method — falling off the end of a `-> int` lambda bound `unit`
                        // into an `int` slot. Route through `check_body` (W-UNREACHABLE) + enforce
                        // return totality.
                        self.check_body(stmts);
                        self.check_return_totality(&declared, stmts, span);
                        declared
                    }
                    None => self.err(
                        span,
                        "a statement-body lambda requires an explicit `-> T` return type",
                    ),
                }
            }
        };
        self.pop_scope();
        self.cur_ret = saved_ret;
        self.cur_throws = saved_throws;
        self.try_catch_stack = saved_try;
        self.cur_is_main = saved_main;
        Ty::Function(param_tys, Box::new(ret_ty))
    }
}
