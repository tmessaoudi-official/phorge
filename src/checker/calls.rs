//! `impl Checker` — calls cluster (M-Decomp W2). See checker/mod.rs for the struct + entry points.

use super::*;

/// A resolved method-overload signature for call checking (Batch C): `(params, ret, throws)`, with
/// the class type-argument substitution already applied. The `throws` set is discharged at the call
/// site exactly like a free-function call.
pub(super) type MethodSig = (Vec<Ty>, Ty, Vec<Ty>);

/// How a UFCS member call was navigated (Slice 6 / F-002): a plain `.`, a `?.` on a genuinely nullable
/// receiver (needs the null-safe `match` desugar), or a `?.` on a non-null receiver (rare — a plain call
/// with an optional-typed result, matching `?.`-on-non-optional elsewhere).
#[derive(Clone, Copy)]
pub(super) enum UfcsNav {
    Plain,
    SafeNullable,
    SafeNonNull,
}

/// The resolved UFCS call site handed to [`Checker::finish_ufcs`] — bundled so the finalizer stays
/// within the argument-count budget. `leaf = None` ⇒ a free-function call; `Some(q)` ⇒ a `q.name(…)`
/// native module call.
pub(super) struct UfcsSite<'a> {
    span: Span,
    leaf: Option<&'a str>,
    name: &'a str,
    object: &'a crate::ast::Expr,
    args: &'a [crate::ast::Expr],
    nav: UfcsNav,
}

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
                let ty = self.check_named_call(name, args, span);
                self.record_pending_fill(callee, args, span);
                ty
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
                            // `Reflect.typeName(x)` is resolved from `x`'s STATIC type and erased
                            // before any backend (Core.Reflect, Tier 3) — never the generic-native
                            // path. `q` is reused for the synthesized `className`/`kind` calls.
                            let n = &crate::native::registry()[idx];
                            if n.module == "Core.Reflect" && n.name == "typeName" {
                                return self.check_reflect_type_name(q, args, span);
                            }
                            let ty = self.check_native_call(idx, args, span);
                            self.record_pending_fill(callee, args, span);
                            return ty;
                        }
                    }
                }
                // Static method call `ClassName.method(args)` (slice B0): the head is a class *name*
                // (not a value binding), resolved after the native path (an explicit import wins a
                // name collision with a class) but before instance-method dispatch. Mirrors the
                // static-field read in `check_member`.
                if !*safe {
                    if let Expr::Ident(cls, _) = &**object {
                        if self.lookup_binding(cls).is_none() && self.classes.contains_key(cls) {
                            return self.check_static_method_call(cls, name, args, span);
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
                // M4: defaulted-arity check (a non-default function has all-`None` defaults, so this
                // is exactly the old exact-arity `check_args`).
                self.check_args_defaulted(name, &sig.params, &sig.defaults, args, span);
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
        applied: &[MethodSig],
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        // Batch C: a method call discharges its declared checked exceptions exactly like a free-fn
        // call (finding #3 — previously dropped, letting a `throws E` escape uncaught). The `?`
        // suppression flag is honored for symmetry, though method-`?` propagation is still the
        // documented `free_call_throws` deferral (a method throw must be caught in a `try`).
        let skip_throws = std::mem::take(&mut self.skip_throws_discharge);
        if applied.len() == 1 {
            let (params, ret, throws) = &applied[0];
            if !skip_throws {
                for e in throws {
                    self.discharge_call_throw(name, e, span);
                }
            }
            return if params.iter().any(ty_has_param) || ty_has_param(ret) {
                self.check_generic_call(name, params, ret, args, span)
            } else {
                self.check_args(name, params, args, span);
                ret.clone()
            };
        }
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.check_expr(a)).collect();
        // Discharge the union of every statically-matching overload's throws — runtime dispatch may
        // pick any of them (mirrors `check_overload_call`).
        if !skip_throws {
            let mut discharged: Vec<Ty> = Vec::new();
            for (params, _, throws) in applied {
                let matches = params.len() == arg_tys.len()
                    && params
                        .iter()
                        .zip(&arg_tys)
                        .all(|(p, a)| self.ty_assignable(a, p));
                if matches {
                    for e in throws {
                        if !discharged.contains(e) {
                            discharged.push(e.clone());
                            self.discharge_call_throw(name, e, span);
                        }
                    }
                }
            }
        }
        let matched = applied.iter().find(|(params, _, _)| {
            params.len() == arg_tys.len()
                && params
                    .iter()
                    .zip(&arg_tys)
                    .all(|(p, a)| self.ty_assignable(a, p))
        });
        match matched {
            Some((_, ret, _)) => ret.clone(),
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
        // W-SECRET (Fork B): a freshly-`expose()`d `Secret` flowing *directly* into a sink is almost
        // certainly a leak (the plaintext gets logged / persisted). Syntactic on the direct argument —
        // a value laundered through a local is not flagged (full taint analysis is out of scope; the
        // type-system non-printability of `Secret` is the real guarantee). Sinks: Console.println/print,
        // File.write. `n` borrows the `'static` registry, so the `&mut self` lint call does not alias.
        if matches!(
            (n.module, n.name),
            ("Core.Console", "println") | ("Core.Console", "print") | ("Core.File", "write")
        ) {
            for a in args {
                if self.arg_is_secret_expose(a) {
                    self.warn_coded(
                        span,
                        "exposing a Secret directly into a sink — the plaintext will be logged or persisted",
                        "W-SECRET",
                        Some("bind the exposed value and use it deliberately, or avoid sending a secret's plaintext to a sink".into()),
                    );
                }
            }
        }
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
            // M4: a native may declare defaults for trailing params (e.g. `parseFloat(string, bool
            // permissive = false)`). Build the parallel `Option<Expr>` list from `native_defaults` —
            // `None` for the leading required params, then the trailing default literals — and run the
            // defaulted-arity check (a native with no defaults gets all-`None` ⇒ exact arity).
            let nd = crate::native::native_defaults(n.module, n.name);
            let ret = n.ret.clone();
            let params = n.params.clone();
            let mut defaults: Vec<Option<crate::ast::Expr>> = vec![None; params.len()];
            for (off, d) in nd.iter().enumerate() {
                let idx = params.len() - nd.len() + off;
                defaults[idx] = Some(native_default_expr(*d, span));
            }
            self.check_args_defaulted(&label, &params, &defaults, args, span);
            ret
        }
    }

    /// W-SECRET helper (Fork B): is `arg` syntactically `<recv>.expose()` where `recv: Secret<_>`?
    /// Matches the method-call shape (`Call` whose callee is a `Member` named `expose`, no args) and
    /// confirms the receiver's type. Re-checking the receiver is side-effect-free for the common case
    /// (a local var read) and span-keyed records are idempotent, so the later full arg-check is safe.
    fn arg_is_secret_expose(&mut self, arg: &crate::ast::Expr) -> bool {
        if let crate::ast::Expr::Call { callee, args, .. } = arg {
            if args.is_empty() {
                if let crate::ast::Expr::Member { object, name, .. } = callee.as_ref() {
                    if name == "expose" {
                        return matches!(self.check_expr(object), Ty::Named(n, _) if n == "Secret");
                    }
                }
            }
        }
        false
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

    /// M4 default parameters: consume a `pending_fill` (set by `check_named_call`/`check_native_call`
    /// when a call legally omitted trailing defaulted args) and record the full replacement `Call`
    /// (provided args + appended default literals) in `default_fills`, keyed by the call span. The
    /// replacement is applied by `rewrite_ufcs` like any other call rewrite, so every backend sees a
    /// full-arity call (byte-identity-safe — the default literal is identical on all three).
    pub(super) fn record_pending_fill(
        &mut self,
        callee: &crate::ast::Expr,
        args: &[crate::ast::Expr],
        span: Span,
    ) {
        if let Some(defaults) = self.pending_fill.take() {
            let mut full: Vec<crate::ast::Expr> = args.to_vec();
            full.extend(defaults);
            self.default_fills.insert(
                span.start,
                crate::ast::Expr::Call {
                    callee: Box::new(callee.clone()),
                    args: full,
                    span,
                },
            );
        }
    }

    /// M4 default parameters: type-check a call that may omit trailing defaulted arguments. `defaults`
    /// is parallel to `params` (`Some(literal)` for a defaulted param). The required arity is the count
    /// of leading non-default params. A call with `args.len()` in `[required, params.len()]` is valid:
    /// the provided args are checked against their params, and if any trailing params were omitted the
    /// caller is told (via `pending_fill`) to append their default literals. Outside that range, the
    /// usual "expects N argument(s)" error fires. Returns nothing (the caller owns the return type).
    pub(super) fn check_args_defaulted(
        &mut self,
        name: &str,
        params: &[Ty],
        defaults: &[Option<crate::ast::Expr>],
        args: &[crate::ast::Expr],
        span: Span,
    ) {
        let required = defaults.iter().take_while(|d| d.is_none()).count();
        if args.len() < required || args.len() > params.len() {
            let detail = if required == params.len() {
                format!("{}", params.len())
            } else {
                format!("{required} to {}", params.len())
            };
            self.err(
                span,
                format!(
                    "`{name}` expects {detail} argument(s), found {}",
                    args.len()
                ),
            );
            for a in args {
                self.check_expr(a);
            }
            return;
        }
        // Type-check the provided args against their params (the omitted defaults were validated at
        // the signature, so they need no re-check here).
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
        // Record the trailing defaults to append (only when some were omitted).
        if args.len() < params.len() {
            let fill: Vec<crate::ast::Expr> = defaults[args.len()..]
                .iter()
                .map(|d| d.clone().expect("trailing params are defaulted"))
                .collect();
            self.pending_fill = Some(fill);
        }
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
        // How a UFCS fallback (if reached) was navigated — plain `.`, `?.` on a nullable receiver (the
        // null-safe `match` desugar), or `?.` on a non-null receiver (F-002).
        let ufcs_nav = if !safe {
            UfcsNav::Plain
        } else if matches!(obj, Ty::Optional(_) | Ty::Null) {
            UfcsNav::SafeNullable
        } else {
            UfcsNav::SafeNonNull
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
                            .map(|s| (s.params.clone(), s.ret.clone(), s.throws.clone()))
                            .collect::<Vec<_>>()
                    })
                    .or_else(|| {
                        if self.interfaces.contains_key(&cls) {
                            // Interface-method `throws` via an interface-typed receiver is a
                            // documented follow-up (the flattened form drops `throws`); the concrete
                            // implementer's class-method call still discharges. Emit no throws here.
                            self.iface_flat_methods(&cls)
                                .into_iter()
                                .find(|(m, _)| m == name)
                                .map(|(_, sig)| vec![(sig.0, sig.1, Vec::new())])
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
                        // Wave 1.1: a `private`/`protected` method called from outside its scope is
                        // rejected (interface methods have no `method_vis` entry ⇒ public ⇒ no-op).
                        let v = self
                            .classes
                            .get(&cls)
                            .and_then(|i| i.method_vis.get(name).cloned());
                        self.enforce_member_vis(v, name, span, false);
                        let applied: Vec<MethodSig> = sigs
                            .iter()
                            .map(|(ps, r, th)| {
                                (
                                    ps.iter().map(|p| apply_subst(p, &theta)).collect(),
                                    apply_subst(r, &theta),
                                    th.iter().map(|t| apply_subst(t, &theta)).collect(),
                                )
                            })
                            .collect();
                        self.check_method_sigs(name, &applied, args, span)
                    }
                    None => {
                        // UFCS fallback (Slice 6): `inst.f(args)` with no method `f` may be the free
                        // function / imported native `f(inst, args)`. `?.` desugars to a null-safe
                        // `match` (F-002).
                        if let Some(ret) = self.try_ufcs(
                            object,
                            &Ty::Named(cls.clone(), cargs.clone()),
                            name,
                            args,
                            span,
                            ufcs_nav,
                        ) {
                            return ret;
                        }
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
                let mut found: Option<Vec<MethodSig>> = None;
                for m in &members {
                    if let Ty::Named(mn, margs) = m {
                        let sig = self
                            .classes
                            .get(mn)
                            .and_then(|info| info.methods.get(name))
                            .map(|v| {
                                v.iter()
                                    .map(|s| (s.params.clone(), s.ret.clone(), s.throws.clone()))
                                    .collect::<Vec<_>>()
                            })
                            .or_else(|| {
                                if self.interfaces.contains_key(mn) {
                                    self.iface_flat_methods(mn)
                                        .into_iter()
                                        .find(|(mm, _)| mm == name)
                                        .map(|(_, sig)| vec![(sig.0, sig.1, Vec::new())])
                                } else {
                                    None
                                }
                            });
                        if let Some(sigs) = sig {
                            let theta = self.class_subst(mn, margs);
                            found = Some(
                                sigs.iter()
                                    .map(|(ps, r, th)| {
                                        (
                                            ps.iter().map(|p| apply_subst(p, &theta)).collect(),
                                            apply_subst(r, &theta),
                                            th.iter().map(|t| apply_subst(t, &theta)).collect(),
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
                        if let Some(ret) = self.try_ufcs(
                            object,
                            &Ty::Intersection(members.clone()),
                            name,
                            args,
                            span,
                            ufcs_nav,
                        ) {
                            return ret;
                        }
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
                // UFCS fallback (Slice 6): a member call on a primitive/container receiver (`xs.map(g)`,
                // `s.upper()`) is `f(receiver, args)` — a free function or imported native. A `?.` call
                // desugars to a null-safe `match` (F-002).
                if let Some(ret) = self.try_ufcs(object, &other, name, args, span, ufcs_nav) {
                    return ret;
                }
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

    /// `ClassName.method(args)` — a **static** method call (slice B0). The class is known (the caller
    /// verified `cls` is a class name, not a value binding). The method must be declared `static`;
    /// calling an instance method this way is `E-STATIC-CALL`. Arg/overload/throws checking reuses
    /// [`check_method_sigs`] (no receiver, so no class-type-arg substitution — a static method that
    /// uses the class's own type parameter is out of scope this slice).
    pub(super) fn check_static_method_call(
        &mut self,
        cls: &str,
        name: &str,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        let sigs: Option<Vec<MethodSig>> = self
            .classes
            .get(cls)
            .and_then(|i| i.methods.get(name))
            .map(|v| {
                v.iter()
                    .map(|s| (s.params.clone(), s.ret.clone(), s.throws.clone()))
                    .collect()
            });
        let Some(sigs) = sigs else {
            for a in args {
                self.check_expr(a);
            }
            return self.err(span, format!("class `{cls}` has no static method `{name}`"));
        };
        if !self.classes[cls].static_methods.contains(name) {
            for a in args {
                self.check_expr(a);
            }
            return self.err_coded(
                span,
                format!("`{name}` is an instance method of `{cls}`, not a static one"),
                "E-STATIC-CALL",
                Some(format!(
                    "`ClassName.{name}(…)` calls a `static` method — make `{name}` static, or call it on an instance (`x.{name}(…)`)"
                )),
            );
        }
        // v1 scope: a static call lowers to a *direct* call to one function index, so an overloaded
        // static method (multiple signatures) is not yet dispatchable this way. Reject it cleanly
        // rather than silently calling one overload (a `run`↔`runvm` divergence risk).
        if sigs.len() > 1 {
            for a in args {
                self.check_expr(a);
            }
            return self.err_coded(
                span,
                format!("`{cls}.{name}` is overloaded — a static call to an overloaded method is not yet supported"),
                "E-STATIC-CALL",
                Some("call it on an instance, or give the static method a single signature".into()),
            );
        }
        // Visibility, mirroring the instance method-call site (Wave 1.1).
        let v = self
            .classes
            .get(cls)
            .and_then(|i| i.method_vis.get(name).cloned());
        self.enforce_member_vis(v, name, span, false);
        self.check_method_sigs(name, &sigs, args, span)
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
                    // non-generic class (M-RT generics-all). Wave 1.1: a `private`/`protected` field
                    // read from outside its scope is rejected here (closing the run↔PHP hole).
                    Some(t) => {
                        let v = self
                            .classes
                            .get(&cls)
                            .and_then(|i| i.field_vis.get(name).cloned());
                        self.enforce_member_vis(v, name, span, true);
                        apply_subst(&t, &self.class_subst(&cls, &cargs))
                    }
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

    /// Enforce member visibility (Wave 1.1) at an instance-member access site. `entry` is the
    /// member's `(visibility, declaring-owner)` (cloned out of the receiver class's `field_vis` /
    /// `method_vis`); `None` ⇒ no recorded visibility ⇒ public by construction (e.g. an interface
    /// method) ⇒ no-op. `private` is reachable only from inside the owner; `protected` from the owner
    /// and its subclasses (`cur_class` is the enclosing class, `None` in a free function). Mirrors the
    /// `const` check (`E-CONST-VISIBILITY`) so `run ≡ runvm ≡ transpiled PHP` all reject the same
    /// out-of-scope access — closing the documented byte-identity hole. `is_field` picks the code.
    pub(super) fn enforce_member_vis(
        &mut self,
        entry: Option<(MemberVis, String)>,
        name: &str,
        span: Span,
        is_field: bool,
    ) {
        let Some((vis, owner)) = entry else { return };
        if vis == MemberVis::Public {
            return;
        }
        let cur = self.cur_class.clone();
        let visible = match vis {
            MemberVis::Public => true,
            MemberVis::Private => cur.as_deref() == Some(owner.as_str()),
            MemberVis::Protected => cur.as_deref().is_some_and(|c| self.is_subtype(c, &owner)),
        };
        if visible {
            return;
        }
        let (kindword, code) = if is_field {
            ("field", "E-FIELD-VISIBILITY")
        } else {
            ("method", "E-METHOD-VISIBILITY")
        };
        let (visword, scope) = if vis == MemberVis::Private {
            ("private", format!("inside `{owner}`"))
        } else {
            ("protected", format!("inside `{owner}` and its subclasses"))
        };
        self.err_coded(
            span,
            format!("`{name}` is a {visword} {kindword} of `{owner}`"),
            code,
            Some(format!("it is accessible only {scope}")),
        );
    }

    /// Enforce a constructor's visibility at a `new C(...)` site (Soundness Batch A — the 7th
    /// member-visibility access site). A `private` ctor is constructible only inside its declaring
    /// class (`cur_class == owner`); a `protected` ctor inside the declaring class or a subclass. The
    /// in-scope cases are the factory/singleton patterns (a static factory method or a static field
    /// initializer, both running in the class's scope). Public (the default) is always allowed.
    pub(super) fn enforce_ctor_vis(&mut self, class_name: &str, span: Span) {
        let Some(info) = self.classes.get(class_name) else {
            return;
        };
        let vis = info.ctor_vis;
        if vis == MemberVis::Public {
            return;
        }
        let owner = info.ctor_owner.clone();
        let cur = self.cur_class.clone();
        let visible = match vis {
            MemberVis::Public => true,
            MemberVis::Private => cur.as_deref() == Some(owner.as_str()),
            MemberVis::Protected => cur.as_deref().is_some_and(|c| self.is_subtype(c, &owner)),
        };
        if visible {
            return;
        }
        let (visword, scope) = if vis == MemberVis::Private {
            ("private", format!("inside `{owner}`"))
        } else {
            ("protected", format!("inside `{owner}` and its subclasses"))
        };
        self.err_coded(
            span,
            format!("the constructor of `{class_name}` is {visword}"),
            "E-CTOR-VISIBILITY",
            Some(format!(
                "construct it only {scope} — e.g. a static factory method or a static field initializer"
            )),
        );
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

    /// UFCS fallback (Slice 6, `docs/plans/2026-06-25-overnight-design-forks-review.plan.md` F-001):
    /// a member call `object.name(args)` that did **not** resolve to a method is re-resolved as the
    /// free/native call `name(object, args)`, **method-first** having already failed. A candidate is,
    /// in priority order: (1) a user free function `name`, or (2) any *imported* `Core.*` native
    /// `name`, whose **first parameter accepts the receiver type** (`unify`, so a generic native like
    /// `map: (List<T>,(T)->U)` matches a `List<int>` receiver). Returns `Some(ret)` once a candidate is
    /// chosen (recording the desugared call in `ufcs_resolutions` for [`rewrite_ufcs`], which the
    /// backends consume verbatim — no new `Op`), or `None` when no callable named `name` fits at all
    /// (the caller then emits the original "no method" error). The receiver `recv_ty` is the
    /// already-checked, optional-peeled type — the receiver expression is *not* re-checked here, so a
    /// throwing-call receiver discharges exactly once.
    pub(super) fn try_ufcs(
        &mut self,
        object: &crate::ast::Expr,
        recv_ty: &Ty,
        name: &str,
        args: &[crate::ast::Expr],
        call_span: Span,
        nav: UfcsNav,
    ) -> Option<Ty> {
        // (1) A user free function wins over any stdlib native of the same name. Single-overload only
        // this slice (overload-set + multi-package UFCS deferred — F-004); a non-fitting arity/first
        // param falls through to the native search rather than committing to an error.
        if let Some(sigs) = self.funcs.get(name).cloned() {
            if sigs.len() == 1
                && sigs[0].params.len() == args.len() + 1
                && self.ufcs_first_accepts(&sigs[0].params[0], recv_ty)
            {
                let sig = &sigs[0];
                let ret =
                    self.check_ufcs_call(name, &sig.params, &sig.ret, recv_ty, args, call_span);
                return Some(self.finish_ufcs(
                    UfcsSite {
                        span: call_span,
                        leaf: None,
                        name,
                        object,
                        args,
                        nav,
                    },
                    ret,
                ));
            }
        }
        // (2) An imported native `name` whose first parameter accepts the receiver. A native is
        // eligible only when its module's *leaf* is the imported qualifier (`import Core.List` ⇒
        // `imports["List"] == "Core.List"`), so the leaf we emit resolves identically on every backend
        // (`index_of_by_leaf` on the interpreter/compiler, the import map on the transpiler). An
        // aliased-only core import is skipped (call it explicitly). Two distinct matches ⇒ ambiguous.
        let mut matched: Option<(usize, &'static str)> = None;
        let mut ambiguous = false;
        for (i, n) in crate::native::registry().iter().enumerate() {
            if n.name != name || n.params.len() != args.len() + 1 {
                continue;
            }
            // `Reflect.typeName` is resolved from its argument's static type and erased before any
            // backend; a UFCS-produced raw `typeName(x)` call would instead reach the backend (where
            // its PHP erasure is only coarse) and diverge. Exclude it — call it qualified. `kind` /
            // `className` are plain natives (byte-identical), so they stay UFCS-eligible.
            if n.module == "Core.Reflect" && n.name == "typeName" {
                continue;
            }
            let leaf = n.module.rsplit('.').next().unwrap_or(n.module);
            if self.imports.get(leaf).map(String::as_str) == Some(n.module)
                && self.ufcs_first_accepts(&n.params[0], recv_ty)
            {
                if matched.is_some() {
                    ambiguous = true;
                } else {
                    matched = Some((i, leaf));
                }
            }
        }
        if ambiguous {
            self.err_coded(
                call_span,
                format!(
                    "UFCS call `.{name}(…)` is ambiguous — more than one imported native matches"
                ),
                "E-UFCS-AMBIGUOUS",
                Some(format!(
                    "call it explicitly, e.g. `Module.{name}(receiver, …)`"
                )),
            );
            return Some(Ty::Error);
        }
        if let Some((idx, leaf)) = matched {
            let n = &crate::native::registry()[idx];
            let label = format!("{leaf}.{name}");
            let ret = self.check_ufcs_call(&label, &n.params, &n.ret, recv_ty, args, call_span);
            return Some(self.finish_ufcs(
                UfcsSite {
                    span: call_span,
                    leaf: Some(leaf),
                    name,
                    object,
                    args,
                    nav,
                },
                ret,
            ));
        }
        None
    }

    /// Does a candidate's first parameter type accept the UFCS receiver? Uses [`Self::unify`] against a
    /// throwaway substitution so a generic first parameter (`List<T>`) matches a concrete receiver
    /// (`List<int>`); for a non-generic parameter this reduces to ordinary assignability (receiver →
    /// parameter), so subtyping is honored.
    fn ufcs_first_accepts(&self, param0: &Ty, recv_ty: &Ty) -> bool {
        let mut theta: HashMap<String, Ty> = HashMap::new();
        self.unify(param0, recv_ty, &mut theta)
    }

    /// Type-check a chosen UFCS candidate as `name(receiver, args)`: the receiver fills the first
    /// parameter (already shown to fit by [`Self::ufcs_first_accepts`]), the call's `args` fill the
    /// rest. A generic candidate (its signature mentions `Ty::Param`) infers the substitution from the
    /// receiver *and* the arguments and applies it to the return type — exactly [`Self::check_generic_call`]
    /// with a prepended receiver; a monomorphic candidate validates each argument by assignability.
    /// The caller guarantees `params.len() == args.len() + 1`.
    fn check_ufcs_call(
        &mut self,
        label: &str,
        params: &[Ty],
        ret: &Ty,
        recv_ty: &Ty,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        if params.iter().any(ty_has_param) || ty_has_param(ret) {
            let mut theta: HashMap<String, Ty> = HashMap::new();
            // Seed θ from the receiver (the first parameter), then each remaining argument.
            self.unify(&params[0], recv_ty, &mut theta);
            for (param, arg) in params[1..].iter().zip(args) {
                let at = self.check_arg(arg, param);
                if !self.unify(param, &at, &mut theta) {
                    let want = apply_subst(param, &theta);
                    self.err(
                        span,
                        format!("`{label}` argument expects `{want}`, found `{at}`"),
                    );
                }
            }
            apply_subst(ret, &theta)
        } else {
            for (param, arg) in params[1..].iter().zip(args) {
                let at = self.check_arg(arg, param);
                if !self.ty_assignable(&at, param) {
                    self.err(
                        span,
                        format!("`{label}` argument expects `{param}`, found `{at}`"),
                    );
                }
            }
            ret.clone()
        }
    }

    /// Record the desugared UFCS call for [`rewrite_ufcs`] (keyed by the enclosing `Call` node's
    /// `Span.start` — each call site's `(` is at a unique byte offset) and return the call site's type.
    /// `leaf = None` ⇒ a free-function call `name(object, args)`; `leaf = Some(q)` ⇒ a native module
    /// call `q.name(object, args)` (the AST shape the user would hand-write). The receiver and arguments
    /// are carried verbatim so `rewrite_ufcs` re-walks them for nested UFCS (`xs.filter(p).map(g)`).
    ///
    /// For a **null-safe** UFCS on a nullable receiver (`x?.f(a)`, F-002), the plain call would lose the
    /// null short-circuit, so the substitution is instead `match x { null => null, r => f(r, a) }` — the
    /// receiver is evaluated once, a `null` short-circuits to `null`, otherwise the unwrapped value is
    /// passed. This reuses match-over-optional (no new `Op`; runs/transpiles on every backend), and the
    /// call site's type is `opt_wrap(ret)`. A `?.` on a non-nullable receiver (rare) is a plain call
    /// with an `opt_wrap`ped type, matching the existing `?.`-on-non-optional behavior.
    fn finish_ufcs(&mut self, site: UfcsSite, ret: Ty) -> Ty {
        use crate::ast::{Expr, MatchArm, Pattern};
        let UfcsSite {
            span: call_span,
            leaf,
            name,
            object,
            args,
            nav,
        } = site;
        let build_call = |receiver: Expr| -> Expr {
            let mut new_args = Vec::with_capacity(args.len() + 1);
            new_args.push(receiver);
            new_args.extend(args.iter().cloned());
            let callee = match leaf {
                None => Expr::Ident(name.to_string(), call_span),
                Some(q) => Expr::Member {
                    object: Box::new(Expr::Ident(q.to_string(), call_span)),
                    name: name.to_string(),
                    safe: false,
                    span: call_span,
                },
            };
            Expr::Call {
                callee: Box::new(callee),
                args: new_args,
                span: call_span,
            }
        };
        let repl = if matches!(nav, UfcsNav::SafeNullable) {
            let recv = Expr::Ident("__ufcs_recv".to_string(), call_span);
            Expr::Match {
                scrutinee: Box::new(object.clone()),
                arms: vec![
                    MatchArm {
                        pattern: Pattern::Null(call_span),
                        guard: None,
                        body: Expr::Null(call_span),
                        span: call_span,
                    },
                    MatchArm {
                        pattern: Pattern::Binding {
                            name: "__ufcs_recv".to_string(),
                            span: call_span,
                        },
                        guard: None,
                        body: build_call(recv),
                        span: call_span,
                    },
                ],
                span: call_span,
            }
        } else {
            build_call(object.clone())
        };
        self.ufcs_resolutions.insert(call_span.start, repl);
        if matches!(nav, UfcsNav::SafeNullable | UfcsNav::SafeNonNull) {
            Self::opt_wrap(ret)
        } else {
            ret
        }
    }
}

/// Convert a native parameter's [`NativeDefault`] literal to the equivalent `Expr` literal, used when
/// filling an omitted trailing native argument (M4 default parameters). The span is the call site's
/// (the synthesized literal needs *a* span; it is never re-matched by the call-rewrite pass since its
/// offset differs from the call key only conceptually — it carries no nested sugar regardless).
fn native_default_expr(d: crate::native::NativeDefault, span: Span) -> crate::ast::Expr {
    use crate::ast::{Expr, StrPart};
    use crate::native::NativeDefault as D;
    match d {
        D::Bool(b) => Expr::Bool(b, span),
        D::Int(n) => Expr::Int(n, span),
        D::Float(f) => Expr::Float(f, span),
        D::Str(s) => Expr::Str(vec![StrPart::Literal(s.to_string())], span),
        D::Null => Expr::Null(span),
    }
}
