//! `impl Checker` — calls cluster (M-Decomp W2). See checker/mod.rs for the struct + entry points.

use super::*;

impl Checker {
    pub(super) fn check_call(
        &mut self,
        callee: &crate::ast::Expr,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        use crate::ast::Expr;
        match callee {
            Expr::Ident(name, _) => {
                // Built-in fault intrinsics (M-faults 2a): `panic`/`todo`/`unreachable` (→ `never`) and
                // `assert` (→ `unit`). Recognized here before any user-function lookup; the names are
                // reserved (`E-RESERVED-INTRINSIC`) so this can't be shadowed.
                if let Some(t) = self.check_intrinsic_call(name, args, span) {
                    return t;
                }
                // If the name is a local (or a `match`-arm binding) with function type, treat it
                // as a function-value call rather than a named-function call — the latter only
                // looks in `self.funcs` (top-level declarations) and would report "unknown
                // function `name`" for a lambda-typed local (M3 S3 Task 4).
                if let Some(Ty::Function(param_tys, ret_ty)) = self.lookup(name) {
                    self.check_args("<lambda>", &param_tys, args, span);
                    return *ret_ty;
                }
                self.check_named_call(name, args, span)
            }
            Expr::Member {
                object, name, safe, ..
            } => {
                // Namespaced native call: `console.println(x)` — head is an imported module
                // qualifier. The shadowing guard keeps an imported qualifier disjoint from every
                // value binding, so membership in the import map is decisive (no scope check).
                if !*safe {
                    if let Expr::Ident(q, _) = &**object {
                        if let Some(idx) = self
                            .imports
                            .get(q)
                            .and_then(|m| crate::native::index_of(m, name))
                        {
                            return self.check_native_call(idx, args, span);
                        }
                    }
                }
                self.check_method_call(object, name, args, *safe, span)
            }
            other => {
                // Evaluate the callee to see if it is a function value (closure or named-fn ref).
                let callee_ty = self.check_expr(other);
                match callee_ty {
                    Ty::Function(param_tys, ret_ty) => {
                        self.check_args("<lambda>", &param_tys, args, span);
                        *ret_ty
                    }
                    Ty::Optional(inner) if matches!(*inner, Ty::Function(..)) => {
                        for a in args {
                            self.check_expr(a);
                        }
                        self.err(
                            span,
                            "not callable — the function value is optional; unwrap it first with `??` or `if (var …)`",
                        )
                    }
                    Ty::Error => {
                        for a in args {
                            self.check_expr(a);
                        }
                        Ty::Error
                    }
                    _ => {
                        for a in args {
                            self.check_expr(a);
                        }
                        self.err(span, "expression is not callable")
                    }
                }
            }
        }
    }

    /// `name(args)` — a free function, enum-variant constructor (Task 5), or class
    /// constructor (Task 6). Free-function case here.
    pub(super) fn check_named_call(
        &mut self,
        name: &str,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        // Consume the throws-mode `?` suppression flag up front (a throwing call under `?` propagates
        // instead of discharging locally). Taken before the variant/ctor probe so it cannot leak —
        // the flag is only ever set for a free throwing function (`free_call_throws`), never a ctor.
        let skip_throws = std::mem::take(&mut self.skip_throws_discharge);
        // Feature C: take the `new`-prefix flag BEFORE checking args, so a bare construction *argument*
        // still requires its own `new`. A construction reached without `new` is `E-NEW-REQUIRED`.
        let was_new = std::mem::take(&mut self.under_new);
        if self.is_construction_name(name) && !was_new {
            self.err_coded(
                span,
                format!("construct `{name}` with `new {name}(…)`"),
                "E-NEW-REQUIRED",
                Some(format!("write `new {name}(…)`")),
            );
        }
        if let Some(t) = self.try_variant_or_class_call(name, args, span) {
            return t;
        }
        let sigs = match self.funcs.get(name) {
            Some(s) => s.clone(),
            None => {
                for a in args {
                    self.check_expr(a);
                }
                return self.err(span, format!("unknown function `{name}`"));
            }
        };
        // Single overload — the common case, identical to pre-overloading behaviour (incl. generics).
        if sigs.len() == 1 {
            let sig = &sigs[0];
            // Discharge each checked exception the callee declares: a bare call must catch it in an
            // enclosing `try` (M-faults 2b); the propagate (`?`) path used the suppression flag.
            if !skip_throws {
                for e in &sig.throws {
                    self.discharge_call_throw(name, e, span);
                }
            }
            return if sig.type_params.is_empty() {
                self.check_args(name, &sig.params, args, span);
                sig.ret.clone()
            } else {
                self.check_generic_call(name, &sig.params, &sig.ret, args, span)
            };
        }
        // Overload set (M-RT): generic members were rejected at collection, so every overload is
        // monomorphic. The call's result is the shared return type (`E-OVERLOAD-RETURN`); resolution
        // here is *static* (for typing) — the runtime dispatch is byte-identical by construction.
        self.check_overload_call(name, &sigs, args, span, skip_throws)
    }

    /// Resolve a multi-overload free-function call (M-RT). Evaluates the argument types, selects the
    /// statically-matching overloads (arity + assignability), reports `E-OVERLOAD-NO-MATCH` when none
    /// match, discharges the union of the matching overloads' checked exceptions, and returns the
    /// shared return type (all overloads share it by `E-OVERLOAD-RETURN`).
    pub(super) fn check_overload_call(
        &mut self,
        name: &str,
        sigs: &[FnSig],
        args: &[crate::ast::Expr],
        span: Span,
        skip_throws: bool,
    ) -> Ty {
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.check_expr(a)).collect();
        let matches: Vec<&FnSig> = sigs
            .iter()
            .filter(|s| {
                s.params.len() == arg_tys.len()
                    && s.params
                        .iter()
                        .zip(&arg_tys)
                        .all(|(p, a)| self.ty_assignable(a, p))
            })
            .collect();
        if matches.is_empty() {
            let got = arg_tys
                .iter()
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return self.err_coded(
                span,
                format!("no overload of `{name}` accepts arguments `({got})`"),
                "E-OVERLOAD-NO-MATCH",
                Some("the argument types must match one overload's parameter types".into()),
            );
        }
        if !skip_throws {
            let mut discharged: Vec<Ty> = Vec::new();
            for m in &matches {
                for e in &m.throws {
                    if !discharged.contains(e) {
                        discharged.push(e.clone());
                        self.discharge_call_throw(name, e, span);
                    }
                }
            }
        }
        matches[0].ret.clone()
    }

    /// Resolve a *method* call against its overload set `applied` (class type-parameters already
    /// substituted in each `(params, ret)`). One overload → the pre-overloading path, including a
    /// method-level generic (`check_generic_call`). Multiple → a static match by arity +
    /// assignability, returning the shared return type (`E-OVERLOAD-RETURN`); none → `E-OVERLOAD-NO-MATCH`.
    pub(super) fn check_method_sigs(
        &mut self,
        name: &str,
        applied: &[(Vec<Ty>, Ty)],
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        if applied.len() == 1 {
            let (params, ret) = &applied[0];
            return if params.iter().any(ty_has_param) || ty_has_param(ret) {
                self.check_generic_call(name, params, ret, args, span)
            } else {
                self.check_args(name, params, args, span);
                ret.clone()
            };
        }
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.check_expr(a)).collect();
        let matched = applied.iter().find(|(params, _)| {
            params.len() == arg_tys.len()
                && params
                    .iter()
                    .zip(&arg_tys)
                    .all(|(p, a)| self.ty_assignable(a, p))
        });
        match matched {
            Some((_, ret)) => ret.clone(),
            None => {
                let got = arg_tys
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.err_coded(
                    span,
                    format!("no overload of method `{name}` accepts arguments `({got})`"),
                    "E-OVERLOAD-NO-MATCH",
                    Some("the argument types must match one overload's parameter types".into()),
                )
            }
        }
    }

    /// Discharge one checked exception `e` a called function `name` may throw at a *bare* (non-`?`)
    /// call site: it must be caught by an enclosing `try`, else `E-CALL-UNHANDLED`. Propagation is
    /// the `?` path ([`Self::try_throws_propagate`]) — a bare call may not silently propagate.
    pub(super) fn discharge_call_throw(&mut self, name: &str, e: &Ty, span: Span) {
        if self.covered_by_try(e) {
            return;
        }
        self.err_coded(
            span,
            format!("call to `{name}` can throw `{e}`, which is not handled here"),
            "E-CALL-UNHANDLED",
            Some(format!(
                "wrap the call in `try {{ … }} catch ({e} e) {{ … }}`, or propagate it with `?` and declare `throws {e}`"
            )),
        );
    }

    /// Check a call to a *generic* function (M-RT S7). Unifies each declared parameter type (which
    /// contains `Ty::Param` occurrences) against the inferred argument type to build a substitution
    /// `θ`, then applies `θ` to the declared return type. First-binding-wins, structural; `θ` lives
    /// only here and never touches the AST (the function's type params are erased separately, before
    /// any backend). A unification failure is a normal argument-type error.
    pub(super) fn check_generic_call(
        &mut self,
        name: &str,
        params: &[Ty],
        ret: &Ty,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        if params.len() != args.len() {
            self.err(
                span,
                format!(
                    "`{name}` expects {} argument(s), found {}",
                    params.len(),
                    args.len()
                ),
            );
            for a in args {
                self.check_expr(a);
            }
            return Ty::Error;
        }
        let mut theta: HashMap<String, Ty> = HashMap::new();
        let mut ok = true;
        for (i, (param, arg)) in params.iter().zip(args).enumerate() {
            let at = self.check_arg(arg, param);
            if !self.unify(param, &at, &mut theta) {
                ok = false;
                let want = apply_subst(param, &theta);
                self.err(
                    span,
                    format!("`{name}` argument {} expects `{want}`, found `{at}`", i + 1),
                );
            }
        }
        if !ok {
            return Ty::Error;
        }
        apply_subst(ret, &theta)
    }

    /// Structural unification of a declared type (possibly containing `Ty::Param`) against a concrete
    /// argument type, accumulating bindings in `θ`. Returns false on a mismatch. A parameter binds
    /// the first concrete type it meets; a later occurrence must be *consistent* (assignable either
    /// way, so subtyping is tolerated). A non-parameter position falls back to ordinary
    /// assignability. `Ty::Error` (poison) unifies with anything (M-RT S7).
    pub(super) fn unify(
        &self,
        declared: &Ty,
        actual: &Ty,
        theta: &mut HashMap<String, Ty>,
    ) -> bool {
        if matches!(declared, Ty::Error) || matches!(actual, Ty::Error) {
            return true;
        }
        match (declared, actual) {
            (Ty::Param(p), a) => match theta.get(p) {
                None => {
                    theta.insert(p.clone(), a.clone());
                    true
                }
                Some(bound) => self.ty_assignable(a, bound) || self.ty_assignable(bound, a),
            },
            (Ty::List(d), Ty::List(a)) | (Ty::Set(d), Ty::Set(a)) => self.unify(d, a, theta),
            (Ty::Optional(d), Ty::Optional(a)) => self.unify(d, a, theta),
            (Ty::Map(dk, dv), Ty::Map(ak, av)) => {
                self.unify(dk, ak, theta) && self.unify(dv, av, theta)
            }
            (Ty::Function(dp, dr), Ty::Function(ap, ar)) => {
                dp.len() == ap.len()
                    && dp.iter().zip(ap).all(|(d, a)| self.unify(d, a, theta))
                    && self.unify(dr, ar, theta)
            }
            // Two generic class instances with the same head — unify their arguments so a generic
            // function over a generic class (`function unwrap<T>(Box<T> b) -> T`) binds `T` from a
            // `Box<int>` argument (M-RT generics-all). Different heads fall through to assignability.
            (Ty::Named(dn, da), Ty::Named(an, aa)) if dn == an && da.len() == aa.len() => {
                da.iter().zip(aa).all(|(d, a)| self.unify(d, a, theta))
            }
            // No type parameter at this position — ordinary assignability (actual → declared).
            (d, a) => self.ty_assignable(a, d),
        }
    }

    /// `console.println(args)` — a namespaced native call resolved through the import map (M3
    /// Wave 1). The native single-sources its signature, so checking is the same arg/arity pass as a
    /// free function; the leaf-qualified label (`console.println`) drives the error messages.
    pub(super) fn check_native_call(
        &mut self,
        idx: usize,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        let n = &crate::native::registry()[idx];
        let leaf = n.module.rsplit('.').next().unwrap_or(n.module);
        let label = format!("{leaf}.{}", n.name);
        // A native whose stored signature carries a type parameter (`Map.keys(Map<K,V>) -> List<K>`,
        // `List.reverse(List<T>) -> List<T>`) is checked exactly like a generic free function: unify
        // the declared params against the argument types, then substitute into the return (M-RT S7b).
        // `θ` lives only in `check_generic_call`; the native's `Ty::Param` is registry-only and never
        // reaches a backend (the compiler types a native call by expression shape → `CTy::Other`, and
        // the transpiler emits via the `php` closure). `n` borrows the `'static` registry, so passing
        // `&n.params`/`&n.ret` alongside `&mut self` does not alias.
        if n.params.iter().any(ty_has_param) || ty_has_param(&n.ret) {
            self.check_generic_call(&label, &n.params, &n.ret, args, span)
        } else {
            self.check_args(&label, &n.params, args, span);
            n.ret.clone()
        }
    }

    /// Check a single call argument against its expected parameter type. Identical to `check_expr`
    /// except that an **empty list literal** `[]` — which has no element to infer a type from —
    /// adopts the expected `List<T>` element type instead of erroring with "cannot infer element
    /// type". This is the one place an expected type is threaded into expression checking
    /// (bidirectional, call-argument-only by design); an empty `[]` in any other position (a
    /// declaration initializer, a `return`) still requires a non-empty literal. It lets the
    /// zero-attribute / zero-child HTML builders read naturally — `el("p", [], [text("hi")])`.
    pub(super) fn check_arg(&mut self, arg: &crate::ast::Expr, expected: &Ty) -> Ty {
        if let crate::ast::Expr::List(elems, _) = arg {
            if elems.is_empty() {
                if let Ty::List(inner) = expected {
                    return Ty::List(inner.clone());
                }
            }
        }
        self.check_expr(arg)
    }

    /// Check call arguments against expected parameter types.
    pub(super) fn check_args(
        &mut self,
        name: &str,
        params: &[Ty],
        args: &[crate::ast::Expr],
        span: Span,
    ) {
        if params.len() != args.len() {
            self.err(
                span,
                format!(
                    "`{name}` expects {} argument(s), found {}",
                    params.len(),
                    args.len()
                ),
            );
            for a in args {
                self.check_expr(a);
            }
            return;
        }
        for (i, (param, arg)) in params.iter().zip(args).enumerate() {
            let at = self.check_arg(arg, param);
            if !self.ty_assignable(&at, param) {
                self.err(
                    span,
                    format!(
                        "`{name}` argument {} expects `{param}`, found `{at}`",
                        i + 1
                    ),
                );
            }
        }
    }

    /// Returns `Some(ret)` if `name` is an enum variant or class constructor.
    pub(super) fn try_variant_or_class_call(
        &mut self,
        name: &str,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Option<Ty> {
        // enum variant constructor: find the (unique) enum that owns this variant name
        let owner = self
            .enums
            .iter()
            .find(|(_, info)| info.variants.contains_key(name))
            .map(|(enum_name, info)| {
                (
                    enum_name.clone(),
                    info.variants[name].clone(),
                    info.type_params.clone(),
                )
            });
        if let Some((enum_name, fields, type_params)) = owner {
            if type_params.is_empty() {
                self.check_args(name, &fields, args, span);
                return Some(Ty::Named(enum_name, Vec::new()));
            }
            // A generic enum (`Option<T>`/`Result<T, E>`): infer its type arguments from the variant
            // constructor call (M-RT generic enums), the same first-binding-wins unifier as a generic
            // class constructor. A type parameter a variant does not mention (`None` for `Option<T>`,
            // `Ok(T)` w.r.t. `E`) stays un-inferred and defaults to `Ty::Error` (permissive — the call
            // site annotates the binding to fix it, e.g. `Option<int> n = None();`).
            if fields.len() != args.len() {
                self.err(
                    span,
                    format!(
                        "variant `{name}` expects {} argument(s), found {}",
                        fields.len(),
                        args.len()
                    ),
                );
                for a in args {
                    self.check_expr(a);
                }
                return Some(Ty::Named(enum_name, vec![Ty::Error; type_params.len()]));
            }
            let mut theta: HashMap<String, Ty> = HashMap::new();
            for (field, arg) in fields.iter().zip(args) {
                let at = self.check_arg(arg, field);
                if !self.unify(field, &at, &mut theta) {
                    let want = apply_subst(field, &theta);
                    self.err(
                        span,
                        format!("variant `{name}` expects `{want}`, found `{at}`"),
                    );
                }
            }
            let inst_args = type_params
                .iter()
                .map(|p| theta.get(p).cloned().unwrap_or(Ty::Error))
                .collect();
            return Some(Ty::Named(enum_name, inst_args));
        }
        // class constructor: `ClassName(args)`
        if let Some(info) = self.classes.get(name) {
            let ctor = info.ctor.clone();
            let type_params = info.type_params.clone();
            let is_abstract = info.is_abstract;
            // M-RT S6b: an abstract class has unimplemented methods and cannot be instantiated.
            if is_abstract {
                self.err_coded(
                    span,
                    format!("cannot instantiate abstract class `{name}`"),
                    "E-ABSTRACT-INSTANTIATE",
                    Some(format!(
                        "`{name}` is `abstract`; instantiate a concrete subclass that implements its \
                         abstract methods"
                    )),
                );
            }
            if type_params.is_empty() {
                self.check_args(name, &ctor, args, span);
                return Some(Ty::Named(name.to_string(), Vec::new()));
            }
            // A generic class: infer its type arguments from the constructor call (M-RT generics-all),
            // the same first-binding-wins unifier as a generic function. A parameter the constructor
            // does not mention stays un-inferred and defaults to `Ty::Error` (permissive).
            if ctor.len() != args.len() {
                self.err(
                    span,
                    format!(
                        "`{name}` expects {} argument(s), found {}",
                        ctor.len(),
                        args.len()
                    ),
                );
                for a in args {
                    self.check_expr(a);
                }
                return Some(Ty::Named(
                    name.to_string(),
                    vec![Ty::Error; type_params.len()],
                ));
            }
            let mut theta: HashMap<String, Ty> = HashMap::new();
            for (param, arg) in ctor.iter().zip(args) {
                let at = self.check_arg(arg, param);
                if !self.unify(param, &at, &mut theta) {
                    let want = apply_subst(param, &theta);
                    self.err(
                        span,
                        format!("`{name}` constructor expects `{want}`, found `{at}`"),
                    );
                }
            }
            let inst_args = type_params
                .iter()
                .map(|p| theta.get(p).cloned().unwrap_or(Ty::Error))
                .collect();
            return Some(Ty::Named(name.to_string(), inst_args));
        }
        None
    }

    pub(super) fn check_method_call(
        &mut self,
        object: &crate::ast::Expr,
        name: &str,
        args: &[crate::ast::Expr],
        safe: bool,
        span: Span,
    ) -> Ty {
        let obj = self.check_expr(object);
        // Peel an optional/null receiver, enforcing the non-null discipline: a plain `.m()` on a
        // `T?` is `E-OPT-USE`; `?.m()` unwraps and re-wraps the result as optional (M3 S2.3).
        let base = match &obj {
            Ty::Error => {
                for a in args {
                    self.check_expr(a);
                }
                return Ty::Error;
            }
            Ty::Null if safe => {
                for a in args {
                    self.check_expr(a);
                }
                return Ty::Null; // `null?.m()` short-circuits to null
            }
            Ty::Optional(_) | Ty::Null if !safe => {
                for a in args {
                    self.check_expr(a);
                }
                return self.err_opt_use(span, name, &obj, "call method");
            }
            Ty::Optional(inner) => (**inner).clone(),
            other => other.clone(),
        };
        let ret = match base {
            Ty::Named(cls, cargs) => {
                // A class method, or — when `cls` is an interface (M-RT S2) — an interface method
                // from its flattened (own + `extends`) signature set. Interface-typed receivers
                // dispatch polymorphically at runtime through the concrete class, so only the static
                // signature is needed here.
                // The method's overload set (M-RT): one or more signatures sharing a return type. An
                // interface method (no overloading) contributes a single signature.
                let sigs = self
                    .classes
                    .get(&cls)
                    .and_then(|info| info.methods.get(name))
                    .map(|v| {
                        v.iter()
                            .map(|s| (s.params.clone(), s.ret.clone()))
                            .collect::<Vec<_>>()
                    })
                    .or_else(|| {
                        if self.interfaces.contains_key(&cls) {
                            self.iface_flat_methods(&cls)
                                .into_iter()
                                .find(|(m, _)| m == name)
                                .map(|(_, sig)| vec![sig])
                        } else {
                            None
                        }
                    });
                // Substitute the *class* type parameters with this instance's type arguments
                // (`Box<int>` ⇒ `{T → int}`), so a method returning/taking `T` is checked at the
                // concrete type (M-RT generics-all). Empty for a non-generic class/interface, so this
                // is the identity in the common case. Any *method-level* `<U>` that survives is then
                // inferred from the call's arguments below.
                let theta = self.class_subst(&cls, &cargs);
                match sigs {
                    Some(sigs) => {
                        let applied: Vec<(Vec<Ty>, Ty)> = sigs
                            .iter()
                            .map(|(ps, r)| {
                                (
                                    ps.iter().map(|p| apply_subst(p, &theta)).collect(),
                                    apply_subst(r, &theta),
                                )
                            })
                            .collect();
                        self.check_method_sigs(name, &applied, args, span)
                    }
                    None => {
                        for a in args {
                            self.check_expr(a);
                        }
                        self.err(span, format!("type `{cls}` has no method `{name}`"))
                    }
                }
            }
            Ty::Intersection(members) => {
                // Member access over an intersection (M-RT S5): search each member (an interface, or
                // the lone class) for `name`, resolving from the *first* member that declares it; a
                // method present in two members agrees on its signature (E-INTERSECT-SIG at the type
                // site), so first-found is unambiguous. None → E-INTERSECT-NO-MEMBER. The value is a
                // concrete instance underneath, so dispatch is polymorphic at runtime — no Op change.
                let mut found: Option<Vec<(Vec<Ty>, Ty)>> = None;
                for m in &members {
                    if let Ty::Named(mn, margs) = m {
                        let sig = self
                            .classes
                            .get(mn)
                            .and_then(|info| info.methods.get(name))
                            .map(|v| {
                                v.iter()
                                    .map(|s| (s.params.clone(), s.ret.clone()))
                                    .collect::<Vec<_>>()
                            })
                            .or_else(|| {
                                if self.interfaces.contains_key(mn) {
                                    self.iface_flat_methods(mn)
                                        .into_iter()
                                        .find(|(mm, _)| mm == name)
                                        .map(|(_, sig)| vec![sig])
                                } else {
                                    None
                                }
                            });
                        if let Some(sigs) = sig {
                            let theta = self.class_subst(mn, margs);
                            found = Some(
                                sigs.iter()
                                    .map(|(ps, r)| {
                                        (
                                            ps.iter().map(|p| apply_subst(p, &theta)).collect(),
                                            apply_subst(r, &theta),
                                        )
                                    })
                                    .collect(),
                            );
                            break;
                        }
                    }
                }
                match found {
                    Some(applied) => self.check_method_sigs(name, &applied, args, span),
                    None => {
                        for a in args {
                            self.check_expr(a);
                        }
                        self.err_coded(
                            span,
                            format!(
                                "no member of `{}` has method `{name}`",
                                Ty::Intersection(members)
                            ),
                            "E-INTERSECT-NO-MEMBER",
                            None,
                        )
                    }
                }
            }
            Ty::Error => Ty::Error,
            other => {
                for a in args {
                    self.check_expr(a);
                }
                self.err(span, format!("type `{other}` has no method `{name}`"))
            }
        };
        if safe {
            Self::opt_wrap(ret)
        } else {
            ret
        }
    }

    pub(super) fn check_member(
        &mut self,
        object: &crate::ast::Expr,
        name: &str,
        safe: bool,
        span: Span,
    ) -> Ty {
        // Static field read `ClassName.field` (M-mut.7): the head is a class *name* not shadowed by a
        // local (locals-first), and `?.` makes no sense on a class. Resolved before `check_expr`,
        // which would otherwise reject the bare class name as an unknown variable.
        if !safe {
            if let crate::ast::Expr::Ident(cls, _) = object {
                if self.lookup_binding(cls).is_none() && self.classes.contains_key(cls) {
                    // A `const` class constant (Feature A) is resolved before a static field — it is
                    // class-name-only and visibility-checked. `consts` already carries inherited
                    // entries (merge_inherited), so `Sub.MAX` resolves an inherited `MAX`.
                    if let Some(entry) = self.classes[cls].consts.get(name).cloned() {
                        let visible = match entry.vis {
                            MemberVis::Public => true,
                            MemberVis::Private => {
                                self.cur_class.as_deref() == Some(entry.owner.as_str())
                            }
                            MemberVis::Protected => self
                                .cur_class
                                .as_deref()
                                .is_some_and(|c| self.is_subtype(c, &entry.owner)),
                        };
                        if !visible {
                            let kind = if entry.vis == MemberVis::Private {
                                "private"
                            } else {
                                "protected"
                            };
                            self.err_coded(
                                span,
                                format!("`{name}` is a {kind} constant of `{}`", entry.owner),
                                "E-CONST-VISIBILITY",
                                Some(format!(
                                    "it is readable only {}",
                                    if entry.vis == MemberVis::Private {
                                        format!("inside `{}`", entry.owner)
                                    } else {
                                        format!("inside `{}` and its subclasses", entry.owner)
                                    }
                                )),
                            );
                        }
                        return entry.ty;
                    }
                    return match self.classes[cls].statics.get(name).cloned() {
                        Some(t) => t,
                        None => self.err_coded(
                            span,
                            format!("`{cls}` has no static field `{name}`"),
                            "E-STATIC-UNKNOWN",
                            Some(
                                "static fields are declared `static …` and read as `Class.field`"
                                    .into(),
                            ),
                        ),
                    };
                }
            }
        }
        let obj = self.check_expr(object);
        // Peel an optional/null receiver, enforcing the non-null discipline: a plain `.field` on a
        // `T?` is `E-OPT-USE`; `?.field` unwraps and re-wraps the result as optional (M3 S2.3).
        let base = match &obj {
            Ty::Error => return Ty::Error,
            Ty::Null if safe => return Ty::Null, // `null?.field` short-circuits to null
            Ty::Optional(_) | Ty::Null if !safe => {
                return self.err_opt_use(span, name, &obj, "read field");
            }
            Ty::Optional(inner) => (**inner).clone(),
            other => other.clone(),
        };
        let field_ty = match base {
            Ty::Named(cls, cargs) => {
                // A property hook (M-mut.7b) is resolved before a stored field: `o.name` runs its
                // `get`. Reading a hook with no `get` (write-only) is `E-HOOK-NO-GET`. A hook is not
                // generic (`package Main` only), so no substitution applies to its type.
                if let Some(h) = self.classes.get(&cls).and_then(|info| info.hooks.get(name)) {
                    let (hty, has_get) = (h.ty.clone(), h.has_get);
                    if !has_get {
                        return self.err_coded(
                            span,
                            format!("property `{name}` of `{cls}` is write-only (no `get`)"),
                            "E-HOOK-NO-GET",
                            Some("add a `get => …;` clause to read it".into()),
                        );
                    }
                    return if safe { Self::opt_wrap(hty) } else { hty };
                }
                let found = self
                    .classes
                    .get(&cls)
                    .and_then(|info| info.fields.get(name).cloned());
                match found {
                    // Substitute the class type parameters with the instance's type arguments, so a
                    // `T` field reads at the concrete type (`Box<int>().value : int`) — identity for a
                    // non-generic class (M-RT generics-all).
                    Some(t) => apply_subst(&t, &self.class_subst(&cls, &cargs)),
                    // A `const` is class-name-only: reading it through an instance (`c.MAX`) is an
                    // error, with a hint pointing at the correct `ClassName.MAX` form (Feature A).
                    None if self
                        .classes
                        .get(&cls)
                        .is_some_and(|info| info.consts.contains_key(name)) =>
                    {
                        self.err_coded(
                            span,
                            format!("`{name}` is a constant of `{cls}` — read it as `{cls}.{name}`, not through an instance"),
                            "E-CONST-INSTANCE-ACCESS",
                            Some(format!("write `{cls}.{name}`")),
                        )
                    }
                    None => self.err(span, format!("type `{cls}` has no field `{name}`")),
                }
            }
            Ty::Intersection(members) => {
                // Only the lone class member can carry fields (interfaces have none, M-RT S5). Search
                // for the field on the class member; none → E-INTERSECT-NO-MEMBER.
                let mut found: Option<Ty> = None;
                for m in &members {
                    if let Ty::Named(mn, margs) = m {
                        if let Some(t) = self
                            .classes
                            .get(mn)
                            .and_then(|info| info.fields.get(name).cloned())
                        {
                            found = Some(apply_subst(&t, &self.class_subst(mn, margs)));
                            break;
                        }
                    }
                }
                match found {
                    Some(t) => t,
                    None => self.err_coded(
                        span,
                        format!(
                            "no member of `{}` has field `{name}`",
                            Ty::Intersection(members)
                        ),
                        "E-INTERSECT-NO-MEMBER",
                        None,
                    ),
                }
            }
            Ty::Error => Ty::Error,
            other => self.err(span, format!("type `{other}` has no field `{name}`")),
        };
        if safe {
            Self::opt_wrap(field_ty)
        } else {
            field_ty
        }
    }

    /// Build the substitution mapping a generic class's type parameters to a concrete instance's type
    /// arguments — `{T → int}` for a `Box<int>` receiver (M-RT generics-all). Empty (the identity
    /// substitution) for a non-generic class or any non-class name, so member/method access on a
    /// non-generic type is unchanged. `zip` tolerates an arity mismatch defensively.
    pub(super) fn class_subst(&self, cls: &str, cargs: &[Ty]) -> HashMap<String, Ty> {
        match self.classes.get(cls) {
            Some(info) => info
                .type_params
                .iter()
                .cloned()
                .zip(cargs.iter().cloned())
                .collect(),
            None => HashMap::new(),
        }
    }

    /// The substitution mapping a generic enum's type parameters to a scrutinee's type arguments
    /// (`Option<int>` ⇒ `{T → int}`), so a `match` binds a variant payload at the concrete type
    /// (`Some(n)` ⇒ `n: int`). Empty for a non-generic enum, so it is the identity in the common case
    /// (M-RT generic enums). Mirror of [`class_subst`].
    pub(super) fn enum_subst(&self, enum_name: &str, eargs: &[Ty]) -> HashMap<String, Ty> {
        match self.enums.get(enum_name) {
            Some(info) => info
                .type_params
                .iter()
                .cloned()
                .zip(eargs.iter().cloned())
                .collect(),
            None => HashMap::new(),
        }
    }
}
