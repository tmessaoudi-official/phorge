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
use crate::ast::{Expr, Item, Program, Type};

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
                _ => {
                    self.check_expr(call);
                    return self.err_coded(
                        select_span,
                        format!("`<{sel}>` is a return-overload selector and applies only to a direct function call"),
                        "E-OVERLOAD-SELECT-UNKNOWN",
                        Some("method-call and indirect selectors are not supported — call a return-overloaded free function directly".into()),
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
        let members = match self.return_overload_sets.get(&name) {
            Some(m) => m.clone(),
            None => {
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
        };
        // Arity + assignability against the shared parameter signature (all members share it).
        let params = self.funcs[&name][0].params.clone();
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
        let exact: Vec<&(Ty, String)> = members.iter().filter(|(r, _)| *r == sel).collect();
        let chosen = if exact.len() == 1 {
            exact[0].clone()
        } else {
            let assignable: Vec<&(Ty, String)> = members
                .iter()
                .filter(|(r, _)| self.ty_assignable(r, &sel))
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
                        select_span,
                        format!("`{name}` has no overload returning `{sel}` (returns: {have})"),
                        "E-OVERLOAD-SELECT-UNKNOWN",
                        Some("name a return type one of the overloads actually has".into()),
                    );
                }
                _ => {
                    return self.err_coded(
                        select_span,
                        format!("`<{sel}>` matches more than one overload of `{name}`"),
                        "E-OVERLOAD-AMBIGUOUS-RETURN",
                        Some("name the exact return type of the overload you mean".into()),
                    );
                }
            }
        };
        // Discharge the chosen member's checked exceptions (M-faults 2b) unless under `?`-propagation.
        if !skip_throws {
            if let Some(sig) = self.funcs[&name].iter().find(|s| s.ret == chosen.0) {
                for e in sig.throws.clone() {
                    self.discharge_call_throw(&name, &e, call_span);
                }
            }
        }
        // Record the resolved rewrite: the selector wrapper becomes a plain call to the mangled name.
        // Keyed by the selector node's span (disjoint from the inner call's own span → no rewrite loop).
        self.overload_resolutions.insert(
            select_span.start,
            Expr::Call {
                callee: Box::new(Expr::Ident(chosen.1.clone(), call_span)),
                args,
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
            // Methods are never return-overloaded in C1; classes (and every other item) are returned
            // untouched. (A future return-overloaded-method slice would extend this pass.)
            other => other,
        })
        .collect();
    Program {
        package: program.package,
        items,
        span: program.span,
    }
}
