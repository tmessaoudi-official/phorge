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
        self.depth -= 1;
        ty
    }

    pub(super) fn check_expr_inner(&mut self, expr: &crate::ast::Expr) -> Ty {
        use crate::ast::Expr;
        match expr {
            Expr::Int(_, _) => Ty::Int,
            Expr::Float(_, _) => Ty::Float,
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
            Expr::Call { callee, args, span } => self.check_call(callee, args, *span), // Task 4
            Expr::New(inner, span) => self.check_new(inner, *span),
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
            UnaryOp::Neg if t == Ty::Int || t == Ty::Float => t,
            UnaryOp::Neg => self.err(
                span,
                format!("unary `-` requires int or float, found `{t}`"),
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
                if (l == Ty::Int && r == Ty::Int) || (l == Ty::Float && r == Ty::Float) {
                    Ty::Bool
                } else {
                    self.err(span, format!("comparison requires matching int or float operands, found `{l}` and `{r}`"));
                    Ty::Bool
                }
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                if l != r {
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

    // ---- stubs replaced in later tasks ----
    pub(super) fn check_str(&mut self, parts: &[crate::ast::StrPart]) -> Ty {
        use crate::ast::StrPart;
        for part in parts {
            if let StrPart::Expr(e) = part {
                let t = self.check_expr(e);
                let ok = matches!(t, Ty::Int | Ty::Float | Ty::Bool | Ty::String | Ty::Error);
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
            Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::InstanceOf { span, .. }
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
            | Expr::New(_, span)
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
    /// The checker rejects a lambda that references `this` (F8 / `E-LAMBDA-THIS`): capturing
    /// `this` would create a run↔runvm divergence (the interpreter's `this` vs. the VM's slot 0).
    /// Workaround: `var self = this;` before the lambda captures the value explicitly.
    pub(super) fn check_lambda(
        &mut self,
        params: &[crate::ast::Param],
        ret: &Option<crate::ast::Type>,
        body: &crate::ast::LambdaBody,
        span: Span,
    ) -> Ty {
        use crate::ast::LambdaBody;
        // F8: reject any lambda that directly references `this` inside its body.
        if lambda_uses_this(body) {
            self.err_coded(
                span,
                "a lambda cannot reference `this` yet",
                "E-LAMBDA-THIS",
                Some("bind `var self = this;` before the lambda and capture `self` instead".into()),
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
                        for s in stmts {
                            self.check_stmt(s);
                        }
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
