//! Return-type overloading (M-RT Slice C1). Free functions may overload on return type alone —
//! identical parameter signatures, differing returns (`parse(string): int` / `parse(string): bool`).
//! PHP has no overloading and cannot see the caller's expected type at runtime, so a return-overload
//! is resolved entirely by the checker (from an explicit `<Type>f(…)` selector in C1) and **mangled
//! per return type** before any backend: each member becomes a distinct single-overload function
//! (`f__ret_int`, `f__ret_bool`) and the resolved call site is rewritten to the mangled name. This is
//! the same "erase front-end sugar before any backend" discipline as generics / aliases / UFCS, so it
//! adds no `Op`/`Value` and keeps `run ≡ runvm ≡ real PHP`. Single-return names are never mangled, so
//! programs with no return-overloading are byte-identical.
//!
//! Scope (C1): free functions only; the selector is the only resolving context (C2 widens to the
//! shallow sinks). A return-overload set is a *pure* return-overload set — all members share one
//! parameter signature; mixing parameter- and return-overloading is rejected (`E-OVERLOAD-RETURN`).

use super::*;
use crate::ast::{ClassMember, Expr, Item, Program, Type};

/// One method declaration grouped by `(class, method)` for return-overload classification:
/// `(decl span, resolved params, resolved return)`. See [`Checker::finalize_method_overloads`].
type MethodOverloadDecl = (Span, Vec<Ty>, Ty);

impl Checker {
    /// Deterministic mangled name for one return-overload member: `f__ret_<slug(ret)>`. The slug keeps
    /// alphanumerics and `_`, mapping every other character to `_` (so `List<int>` → `List_int_`).
    /// Distinct member return types yield distinct slugs in practice; computed once here and shared by
    /// the call-site rewrite and the definition rename, so the two never disagree.
    pub(super) fn ret_overload_mangle(name: &str, ret: &Ty) -> String {
        let mut slug = String::new();
        for ch in ret.to_string().chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                slug.push(ch);
            } else {
                slug.push('_');
            }
        }
        format!("{name}__ret_{slug}")
    }

    /// Classify return-overload sets after collection (every signature is known) and before any body
    /// is checked (so a `<Type>` selector can resolve against the set). A free-function name is a *pure
    /// return-overload set* when it has ≥2 overloads, all sharing one parameter signature, with
    /// pairwise-distinct return types. For each, record the `(ret, mangled)` members (for call
    /// resolution) and the per-declaration span→mangled rename (applied before the backends). A set
    /// with a parameter-overload (distinct params) is a runtime-dispatched parameter-overload set and
    /// is left untouched; an illegal set (duplicate signatures — already an error) is never classified.
    pub(super) fn finalize_overloads(&mut self) {
        let names: Vec<String> = self
            .funcs
            .iter()
            .filter(|(_, sigs)| sigs.len() >= 2)
            .map(|(n, _)| n.clone())
            .collect();
        for name in names {
            let sigs = &self.funcs[&name];
            let first = &sigs[0].params;
            let all_same_params = sigs.iter().all(|s| &s.params == first);
            if !all_same_params {
                continue; // parameter-overload set — runtime dispatched, not return-overloaded
            }
            // Require pairwise-distinct return types (a repeated return is a duplicate — already an
            // error; do not classify it as a return-overload set).
            let rets: Vec<Ty> = sigs.iter().map(|s| s.ret.clone()).collect();
            let distinct = rets.iter().enumerate().all(|(i, r)| !rets[..i].contains(r));
            if !distinct {
                continue;
            }
            let members: Vec<(Ty, String)> = rets
                .iter()
                .map(|r| (r.clone(), Self::ret_overload_mangle(&name, r)))
                .collect();
            self.return_overload_sets.insert(name, members);
        }
        // Per-declaration renames, keyed by each member's declaration span (so the rename touches the
        // exact `FunctionDecl`, not every same-named function — though here all of them are members).
        let renames: Vec<(usize, String)> = self
            .free_fn_decls
            .iter()
            .filter(|(name, _, _, _)| self.return_overload_sets.contains_key(name))
            .map(|(name, span, _, ret)| (span.start, Self::ret_overload_mangle(name, ret)))
            .collect();
        for (key, mangled) in renames {
            self.overload_def_renames.insert(key, mangled);
        }
    }

    /// Classify return-overload **method** sets (M-RT S2.2), the method analog of
    /// [`Self::finalize_overloads`]. Grouped by `(class, method)`: a set with ≥2 overloads sharing one
    /// parameter signature and pairwise-distinct returns is a *pure return-overload method set* — its
    /// `(ret, mangled)` members are recorded for selector resolution and each declaration is queued for
    /// a per-decl mangled rename in the shared `overload_def_renames` map (method decl spans are
    /// disjoint from free-fn ones, so the two never collide). A different-parameter set is a normal
    /// runtime-dispatched parameter-overload set and is left untouched; a repeated return is a
    /// duplicate (already an error) and is never classified. Runs after [`Self::finalize_overloads`].
    pub(super) fn finalize_method_overloads(&mut self) {
        use std::collections::BTreeMap;
        let mut groups: BTreeMap<(String, String), Vec<MethodOverloadDecl>> = BTreeMap::new();
        for (cls, m, span, params, ret) in &self.method_fn_decls {
            groups.entry((cls.clone(), m.clone())).or_default().push((
                *span,
                params.clone(),
                ret.clone(),
            ));
        }
        for ((cls, m), decls) in groups {
            if decls.len() < 2 {
                continue;
            }
            let first = &decls[0].1;
            if !decls.iter().all(|(_, p, _)| p == first) {
                continue; // parameter-overload set — runtime dispatched, not return-overloaded
            }
            let rets: Vec<Ty> = decls.iter().map(|(_, _, r)| r.clone()).collect();
            if !rets.iter().enumerate().all(|(i, r)| !rets[..i].contains(r)) {
                continue; // a repeated return is a duplicate (already an error) — do not classify
            }
            let members: Vec<(Ty, String)> = rets
                .iter()
                .map(|r| (r.clone(), Self::ret_overload_mangle(&m, r)))
                .collect();
            self.return_overload_methods
                .insert((cls, m.clone()), members);
            for (span, _, ret) in &decls {
                self.overload_def_renames
                    .insert(span.start, Self::ret_overload_mangle(&m, ret));
            }
        }
    }

    /// Whether `(class, method)` is a return-overload method set (M-RT S2.2) — consulted by
    /// `check_method_call` so a *bare* (selector-less) call to such a method is `E-OVERLOAD-NO-CONTEXT`
    /// (C1 scope: methods need an explicit `<Type>` selector, like free functions without a sink).
    pub(super) fn is_return_overload_method(&self, cls: &str, method: &str) -> bool {
        self.return_overload_methods
            .contains_key(&(cls.to_string(), method.to_string()))
    }

    /// Resolve a method return-overload selector `<Type>obj.m(args)` (M-RT S2.2). Resolves the
    /// receiver's static class, picks the member of `(class, m)` whose (instance-substituted) return
    /// type the selector names, type-checks the arguments against the shared parameter signature,
    /// discharges the chosen member's checked exceptions, records a rewrite to a *method* call on the
    /// mangled name (`obj.m__ret_int(args)`, preserving the receiver), and returns the chosen return
    /// type. The receiver must be a class instance whose method is return-overloaded — anything else is
    /// `E-OVERLOAD-SELECT-UNKNOWN`.
    #[allow(clippy::too_many_arguments)]
    fn resolve_method_return_overload(
        &mut self,
        object: &Expr,
        method: &str,
        safe: bool,
        args: &[Expr],
        call_span: Span,
        rewrite_key: usize,
        sel: &Ty,
        skip_throws: bool,
    ) -> Ty {
        let obj_ty = self.check_expr(object);
        let (cls, cargs) = match &obj_ty {
            Ty::Named(c, a) => (c.clone(), a.clone()),
            Ty::Error => {
                for a in args {
                    self.check_expr(a);
                }
                return Ty::Error;
            }
            _ => {
                for a in args {
                    self.check_expr(a);
                }
                return self.err_coded(
                    call_span,
                    format!("`<{sel}>` selects a method return-overload, but the receiver `{obj_ty}` is not a class instance"),
                    "E-OVERLOAD-SELECT-UNKNOWN",
                    Some("a method overload selector applies to `<Type>instance.method(args)` where `method` is return-type-overloaded".into()),
                );
            }
        };
        let members = match self
            .return_overload_methods
            .get(&(cls.clone(), method.to_string()))
        {
            Some(m) => m.clone(),
            None => {
                for a in args {
                    self.check_expr(a);
                }
                return self.err_coded(
                    call_span,
                    format!("`<{sel}>` selects a return-overload, but method `{method}` on `{cls}` is not return-type-overloaded"),
                    "E-OVERLOAD-SELECT-UNKNOWN",
                    Some("a method overload selector applies only to a method declared with several return types over identical parameters".into()),
                );
            }
        };
        // A `private`/`protected` method called from outside its scope is rejected exactly as on the
        // bare-call path (the selector must not bypass visibility).
        let v = self
            .classes
            .get(&cls)
            .and_then(|i| i.method_vis.get(method).cloned());
        self.enforce_member_vis(v, method, call_span, false);
        // Arity + assignability against the shared parameter signature, substituting the instance's
        // class type arguments (`Box<int>` ⇒ `{T → int}`) — the identity for a non-generic class.
        let theta = self.class_subst(&cls, &cargs);
        let params: Vec<Ty> = self.classes[&cls].methods[method][0]
            .params
            .iter()
            .map(|p| apply_subst(p, &theta))
            .collect();
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.check_expr(a)).collect();
        let arity_ok = params.len() == arg_tys.len()
            && params
                .iter()
                .zip(&arg_tys)
                .all(|(p, a)| self.ty_assignable(a, p));
        if !arity_ok {
            let got = arg_tys
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            return self.err_coded(
                call_span,
                format!("no overload of method `{method}` accepts arguments `({got})`"),
                "E-OVERLOAD-NO-MATCH",
                Some("the argument types must match the overload's parameter types".into()),
            );
        }
        // Pick the member by the *substituted* return type (exact → unique assignable). The mangled
        // name stays keyed on the DECLARED return (matching `finalize_method_overloads`/the rename), so
        // carry the declared return alongside for throws discharge.
        let cands: Vec<(Ty, Ty, String)> = members
            .iter()
            .map(|(decl_ret, mangled)| {
                (
                    apply_subst(decl_ret, &theta),
                    decl_ret.clone(),
                    mangled.clone(),
                )
            })
            .collect();
        let exact: Vec<&(Ty, Ty, String)> = cands.iter().filter(|(s, _, _)| s == sel).collect();
        let chosen = if exact.len() == 1 {
            exact[0].clone()
        } else {
            let assignable: Vec<&(Ty, Ty, String)> = cands
                .iter()
                .filter(|(s, _, _)| self.ty_assignable(s, sel))
                .collect();
            match assignable.len() {
                1 => assignable[0].clone(),
                0 => {
                    let have = members
                        .iter()
                        .map(|(r, _)| r.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return self.err_coded(
                        call_span,
                        format!("method `{method}` on `{cls}` has no overload returning `{sel}` (returns: {have})"),
                        "E-OVERLOAD-SELECT-UNKNOWN",
                        Some("name a return type one of the overloads actually has".into()),
                    );
                }
                _ => {
                    let have = members
                        .iter()
                        .map(|(r, _)| r.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return self.err_coded(
                        call_span,
                        format!("method `{method}` on `{cls}` has no unique overload whose return type is `{sel}` (returns: {have})"),
                        "E-OVERLOAD-AMBIGUOUS-RETURN",
                        Some("add an explicit `<Type>` selector naming the exact return type you want".into()),
                    );
                }
            }
        };
        // Discharge the chosen member's checked exceptions (the sig whose DECLARED return matches),
        // unless under `?`-propagation.
        if !skip_throws {
            if let Some(sig) = self.classes[&cls].methods[method]
                .iter()
                .find(|s| s.ret == chosen.1)
            {
                for e in sig.throws.clone() {
                    self.discharge_call_throw(method, &apply_subst(&e, &theta), call_span);
                }
            }
        }
        // Record the resolved rewrite: a method call on the mangled name, preserving the receiver and
        // null-safety. Keyed by `rewrite_key` (the selector node's span). `apply_repl` re-walks the
        // embedded receiver/args without re-matching the key.
        self.overload_resolutions.insert(
            rewrite_key,
            Expr::Call {
                callee: Box::new(Expr::Member {
                    object: Box::new(object.clone()),
                    name: chosen.2.clone(),
                    safe,
                    span: call_span,
                }),
                args: args.to_vec(),
                span: call_span,
            },
        );
        chosen.0
    }

    /// Resolve a `<Type>f(args)` overload selector (M-RT Slice C1). The selector picks the
    /// return-overload of the free function `f` whose return type the selector names, records the
    /// mangled call-site rewrite, and returns the chosen member's return type. The selector is valid
    /// only on a return-overloaded free-function call; anything else is `E-OVERLOAD-SELECT-UNKNOWN`.
    pub(super) fn check_overload_select(
        &mut self,
        ty: &Type,
        call: &Expr,
        select_span: Span,
    ) -> Ty {
        let skip_throws = std::mem::take(&mut self.skip_throws_discharge);
        let sel = self.resolve_type(ty);
        // The selector must prefix a direct free-function call `f(args)` (callee is a bare identifier).
        let (name, args, call_span) = match call {
            Expr::Call { callee, args, span } => match &**callee {
                Expr::Ident(n, _) => (n.clone(), args.clone(), *span),
                // M-RT S2.2: `<Type>obj.m(args)` — a method return-overload selector. Resolved against
                // the receiver's class (the free-fn arms below never see a method callee).
                Expr::Member {
                    object,
                    name: m,
                    safe,
                    ..
                } => {
                    return self.resolve_method_return_overload(
                        object,
                        m,
                        *safe,
                        args,
                        *span,
                        select_span.start,
                        &sel,
                        skip_throws,
                    );
                }
                _ => {
                    self.check_expr(call);
                    return self.err_coded(
                        select_span,
                        format!("`<{sel}>` is a return-overload selector and applies only to a direct function or method call"),
                        "E-OVERLOAD-SELECT-UNKNOWN",
                        Some("indirect selectors are not supported — call a return-overloaded free function or method directly".into()),
                    );
                }
            },
            _ => {
                self.check_expr(call);
                return self.err_coded(
                    select_span,
                    format!(
                        "`<{sel}>` is a return-overload selector and must prefix a function call"
                    ),
                    "E-OVERLOAD-SELECT-UNKNOWN",
                    Some(
                        "write `<Type>f(args)` where `f` is a return-type-overloaded function"
                            .into(),
                    ),
                );
            }
        };
        if !self.return_overload_sets.contains_key(&name) {
            for a in &args {
                self.check_expr(a);
            }
            return self.err_coded(
                select_span,
                format!("`<{sel}>` selects a return-overload, but `{name}` is not return-type-overloaded"),
                "E-OVERLOAD-SELECT-UNKNOWN",
                Some("an overload selector applies only to a function declared with several return types over identical parameters".into()),
            );
        }
        // Resolve against the selector type; key the rewrite by the selector node (its span is disjoint
        // from the inner call's, so the rewrite never re-matches itself).
        self.resolve_return_overload(
            &name,
            &args,
            call_span,
            select_span.start,
            &sel,
            skip_throws,
            true,
        )
    }

    /// Whether `e` is a *bare* call to a return-overloaded free function (no `<Type>` selector) — the
    /// shape a C2 sink can resolve against its expected type. Cheap; checked before resolving the sink's
    /// declared type so the common (non-overloaded) path is untouched.
    pub(super) fn is_return_overload_call(&self, e: &Expr) -> bool {
        matches!(e, Expr::Call { callee, .. }
            if matches!(&**callee, Expr::Ident(n, _) if self.return_overload_sets.contains_key(n)))
    }

    /// C2 sink: resolve a *bare* return-overloaded call `name(args)` against an `expected` type fixed by
    /// the surrounding context (a typed binding or `return`), with no explicit `<Type>` selector.
    /// Returns `Some(ret)` (recording the mangled rewrite keyed by the call's own span — the bare
    /// `Expr::Call` that `rewrite_ufcs` will replace) when `name` is a return-overload set; `None` when
    /// it is not (so the caller falls back to ordinary checking, e.g. a plain or parameter-overloaded
    /// call). A non-resolving expected type is a hard error inside (returns `Some(Ty::Error)`), so the
    /// sink never silently passes an unresolved selector-less call to a backend.
    pub(super) fn try_resolve_sink_overload(&mut self, call: &Expr, expected: &Ty) -> Option<Ty> {
        let (name, args, call_span) = match call {
            Expr::Call { callee, args, span } => match &**callee {
                Expr::Ident(n, _) if self.return_overload_sets.contains_key(n) => {
                    (n.clone(), args.clone(), *span)
                }
                _ => return None,
            },
            _ => return None,
        };
        let skip_throws = std::mem::take(&mut self.skip_throws_discharge);
        Some(self.resolve_return_overload(
            &name,
            &args,
            call_span,
            call_span.start,
            expected,
            skip_throws,
            false,
        ))
    }

    /// Shared return-overload resolution core (Slice C). Picks the member of `name` whose return type
    /// the `expected` type selects — exact equality → unique assignable → else error — type-checks the
    /// arguments against the (shared) parameter signature, discharges the chosen member's checked
    /// exceptions, records the mangled call rewrite keyed by `rewrite_key`, and returns the chosen
    /// return type. `from_selector` distinguishes the error flavour of the zero-candidate case
    /// (`E-OVERLOAD-SELECT-UNKNOWN` for an explicit selector vs `E-OVERLOAD-AMBIGUOUS-RETURN` for a
    /// sink). The caller guarantees `name` is a return-overload set.
    #[allow(clippy::too_many_arguments)]
    fn resolve_return_overload(
        &mut self,
        name: &str,
        args: &[Expr],
        call_span: Span,
        rewrite_key: usize,
        expected: &Ty,
        skip_throws: bool,
        from_selector: bool,
    ) -> Ty {
        // A poisoned expected type means the surrounding context already errored — resolve to poison
        // without a second diagnostic (and without a rewrite; the program will not reach a backend).
        if *expected == Ty::Error {
            for a in args {
                self.check_expr(a);
            }
            return Ty::Error;
        }
        let members = self.return_overload_sets[name].clone();
        // Arity + assignability against the shared parameter signature (all members share it).
        let params = self.funcs[name][0].params.clone();
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.check_expr(a)).collect();
        let arity_ok = params.len() == arg_tys.len()
            && params
                .iter()
                .zip(&arg_tys)
                .all(|(p, a)| self.ty_assignable(a, p));
        if !arity_ok {
            let got = arg_tys
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            return self.err_coded(
                call_span,
                format!("no overload of `{name}` accepts arguments `({got})`"),
                "E-OVERLOAD-NO-MATCH",
                Some("the argument types must match the overload's parameter types".into()),
            );
        }
        // Pick the member: exact return match → unique assignable → else unknown / ambiguous.
        let exact: Vec<&(Ty, String)> = members.iter().filter(|(r, _)| r == expected).collect();
        let chosen = if exact.len() == 1 {
            exact[0].clone()
        } else {
            let assignable: Vec<&(Ty, String)> = members
                .iter()
                .filter(|(r, _)| self.ty_assignable(r, expected))
                .collect();
            match assignable.len() {
                1 => assignable[0].clone(),
                0 if from_selector => {
                    let have = members
                        .iter()
                        .map(|(r, _)| r.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return self.err_coded(
                        call_span,
                        format!(
                            "`{name}` has no overload returning `{expected}` (returns: {have})"
                        ),
                        "E-OVERLOAD-SELECT-UNKNOWN",
                        Some("name a return type one of the overloads actually has".into()),
                    );
                }
                _ => {
                    let have = members
                        .iter()
                        .map(|(r, _)| r.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return self.err_coded(
                        call_span,
                        format!(
                            "`{name}` has no unique overload whose return type is `{expected}` (returns: {have})"
                        ),
                        "E-OVERLOAD-AMBIGUOUS-RETURN",
                        Some("add an explicit `<Type>` selector naming the exact return type you want".into()),
                    );
                }
            }
        };
        // Discharge the chosen member's checked exceptions (M-faults 2b) unless under `?`-propagation.
        if !skip_throws {
            if let Some(sig) = self.funcs[name].iter().find(|s| s.ret == chosen.0) {
                for e in sig.throws.clone() {
                    self.discharge_call_throw(name, &e, call_span);
                }
            }
        }
        // Record the resolved rewrite: a plain call to the mangled name, keyed by `rewrite_key` (the
        // selector node for a selector, or the bare call's own span for a sink). The replacement's own
        // span is the inner call's, re-walked by `apply_repl` without re-matching the key.
        self.overload_resolutions.insert(
            rewrite_key,
            Expr::Call {
                callee: Box::new(Expr::Ident(chosen.1.clone(), call_span)),
                args: args.to_vec(),
                span: call_span,
            },
        );
        chosen.0
    }
}

/// Rename each return-overload member's *definition* to its mangled name (M-RT Slice C1), keyed by the
/// `FunctionDecl`'s span. The resolved call sites were already rewritten to the same mangled names by
/// [`super::rewrite_ufcs`]; renaming the definitions makes the backends see distinct, single-overload
/// functions (so no ambiguous identical-`ParamKind` dispatch table is ever built, and the transpiler
/// emits each as a plain PHP function). A no-op when `renames` is empty — so a program with no
/// return-overloading is byte-for-byte the pre-Slice-C AST. Free functions only; methods are not
/// return-overloadable in C1, so class members are returned untouched.
pub fn rename_overload_defs(program: Program, renames: &HashMap<usize, String>) -> Program {
    if renames.is_empty() {
        return program;
    }
    let items = program
        .items
        .into_iter()
        .map(|item| match item {
            Item::Function(mut f) => {
                if let Some(mangled) = renames.get(&f.span.start) {
                    f.name = mangled.clone();
                }
                Item::Function(f)
            }
            // M-RT S2.2: a class may hold return-overloaded *methods*; rename each method member whose
            // declaration span is in the map to its mangled name (`m__ret_int`). The resolved selector
            // call sites were rewritten to the same mangled names by `rewrite_ufcs`, so dispatch on the
            // mangled `(class, name)` stays consistent across all backends. A no-op for a class with no
            // return-overloaded method.
            Item::Class(mut c) => {
                for member in &mut c.members {
                    if let ClassMember::Method(f) = member {
                        if let Some(mangled) = renames.get(&f.span.start) {
                            f.name = mangled.clone();
                        }
                    }
                }
                Item::Class(c)
            }
            // Every other item (enum, interface, trait, type alias, …) is returned untouched.
            other => other,
        })
        .collect();
    Program {
        package: program.package,
        items,
        span: program.span,
    }
}
