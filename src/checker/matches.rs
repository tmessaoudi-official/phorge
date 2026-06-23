//! `impl Checker` — matches cluster (M-Decomp W2). See checker/mod.rs for the struct + entry points.

use super::*;

impl Checker {
    pub(super) fn check_match(
        &mut self,
        scrutinee: &crate::ast::Expr,
        arms: &[crate::ast::MatchArm],
        span: Span,
    ) -> Ty {
        use crate::ast::Pattern;
        let scrut = self.check_expr(scrutinee);

        let mut result: Option<Ty> = None;
        let mut covered: Vec<String> = Vec::new();
        // Type-pattern coverage for match-over-union exhaustiveness (M-RT S4): the class/interface
        // names matched by `Circle c =>` arms.
        let mut covered_types: Vec<String> = Vec::new();
        let mut has_catch_all = false;
        // Once a `null` arm has matched, a later catch-all binding over a `T?` scrutinee sees only
        // the non-null inner — the smart-cast that makes `match opt { null => …, v => … }` bind
        // `v: T` (M3 S2.6 / S1.4). Tracks whether a prior arm covered `null`.
        let mut null_seen = false;
        // Reachability lints (W-MATCH-UNREACHABLE): an arm after a catch-all, or a duplicate
        // literal/variant/type arm, can never match. `catch_all_seen`/`seen_keys` reflect *prior*
        // arms only (updated at the end of each iteration), so the offending arm — not the first
        // good one — is flagged.
        let mut catch_all_seen = false;
        let mut seen_keys: Vec<String> = Vec::new();

        for arm in arms {
            if catch_all_seen {
                self.warn_coded(
                    arm.span,
                    "unreachable match arm: a previous arm already matches everything",
                    "W-MATCH-UNREACHABLE",
                    Some(
                        "a `_` or bare-identifier arm is a catch-all; arms after it never run"
                            .into(),
                    ),
                );
            } else if let Some(key) = match_arm_key(&arm.pattern) {
                if seen_keys.contains(&key) {
                    self.warn_coded(
                        arm.span,
                        "unreachable match arm: a previous arm already covers this pattern",
                        "W-MATCH-UNREACHABLE",
                        Some("remove the duplicate arm".into()),
                    );
                } else {
                    seen_keys.push(key);
                }
            }
            if matches!(arm.pattern, Pattern::Wildcard(_) | Pattern::Binding { .. }) {
                has_catch_all = true;
                catch_all_seen = true;
            }
            if let Pattern::Variant { name, .. } = &arm.pattern {
                covered.push(name.clone());
            }
            if let Pattern::Type { type_name, .. } = &arm.pattern {
                covered_types.push(type_name.clone());
            }
            // The type a catch-all binding sees: narrowed to the inner `T` when a preceding `null`
            // arm already handled absence; otherwise the scrutinee type unchanged.
            let arm_scrut = match (&scrut, null_seen) {
                (Ty::Optional(inner), true) => (**inner).clone(),
                _ => scrut.clone(),
            };
            // each arm gets its own scope for pattern bindings
            self.push_scope();
            self.check_pattern(&arm.pattern, &arm_scrut);
            let body_ty = self.check_expr(&arm.body);
            self.pop_scope();
            if matches!(arm.pattern, Pattern::Null(_)) {
                null_seen = true;
            }

            match &result {
                None => result = Some(body_ty),
                Some(first) => {
                    if !self.ty_assignable(&body_ty, first) && !self.ty_assignable(first, &body_ty)
                    {
                        self.err(
                            span,
                            format!(
                                "match arms must share one type; found `{first}` and `{body_ty}`"
                            ),
                        );
                    }
                }
            }
        }

        // exhaustiveness
        if !has_catch_all {
            match &scrut {
                Ty::Named(enum_name, _) if self.enums.contains_key(enum_name) => {
                    let all: Vec<String> = self.enums[enum_name].variants.keys().cloned().collect();
                    let mut missing: Vec<String> =
                        all.into_iter().filter(|v| !covered.contains(v)).collect();
                    // `variants` is a HashMap, so `keys()` order is nondeterministic — sort the
                    // missing list so the error message is stable across runs (otherwise it's an
                    // intermittent test/diff hazard).
                    missing.sort();
                    if !missing.is_empty() {
                        self.err(
                            span,
                            format!("non-exhaustive match: missing {}", missing.join(", ")),
                        );
                    }
                }
                // Match-over-union exhaustiveness (M-RT S4): every nominal member must be covered by a
                // type pattern naming it OR a covering supertype/interface. A primitive member can't be
                // type-matched, so a union containing one always needs a `_` (reported as missing).
                Ty::Union(members) => {
                    let mut missing: Vec<String> = members
                        .iter()
                        .filter(|m| match m {
                            Ty::Named(n, _) => !covered_types
                                .iter()
                                .any(|t| t == n || self.is_subtype(n, t)),
                            _ => true,
                        })
                        .map(std::string::ToString::to_string)
                        .collect();
                    missing.sort();
                    if !missing.is_empty() {
                        self.err(
                            span,
                            format!("non-exhaustive match: missing {}", missing.join(", ")),
                        );
                    }
                }
                Ty::Error => {}
                _ => {
                    self.err(
                        span,
                        "non-exhaustive match: add a `_` wildcard arm for non-enum scrutinees",
                    );
                }
            }
        }

        result.unwrap_or(Ty::Error)
    }

    /// Check a pattern against the scrutinee type, declaring its bindings into the
    /// current scope.
    pub(super) fn check_pattern(&mut self, pat: &crate::ast::Pattern, scrut: &Ty) {
        use crate::ast::Pattern;
        match pat {
            Pattern::Wildcard(_) => {}
            Pattern::Binding { name, span } => self.declare(name, scrut.clone(), *span),
            Pattern::Int(_, span) => self.expect_prim(scrut, &Ty::Int, *span),
            Pattern::Float(_, span) => self.expect_prim(scrut, &Ty::Float, *span),
            Pattern::Str(_, span) => self.expect_prim(scrut, &Ty::String, *span),
            Pattern::Bool(_, span) => self.expect_prim(scrut, &Ty::Bool, *span),
            Pattern::Null(span) => {
                // A `null` pattern is only meaningful against an optional scrutinee (M3 S2.6).
                if !matches!(scrut, Ty::Optional(_) | Ty::Null | Ty::Error) {
                    self.err(
                        *span,
                        format!(
                            "`null` pattern requires an optional `T?` scrutinee, found `{scrut}`"
                        ),
                    );
                }
            }
            Pattern::Variant { name, fields, span } => {
                let (enum_name, eargs) = match scrut {
                    Ty::Named(n, eargs) if self.enums.contains_key(n) => (n.clone(), eargs.clone()),
                    Ty::Error => return,
                    other => {
                        self.err(*span, format!("variant pattern `{name}` requires an enum scrutinee, found `{other}`"));
                        return;
                    }
                };
                let field_tys: Vec<Ty> = match self.enums[&enum_name].variants.get(name) {
                    // Substitute the enum's type parameters with the scrutinee's type arguments
                    // (`Option<int>` ⇒ `{T → int}`) so a generic variant's payload binds at the
                    // concrete type (M-RT generic enums); identity for a non-generic enum.
                    Some(f) => {
                        let theta = self.enum_subst(&enum_name, &eargs);
                        f.iter().map(|t| apply_subst(t, &theta)).collect()
                    }
                    None => {
                        self.err(*span, format!("enum `{enum_name}` has no variant `{name}`"));
                        return;
                    }
                };
                if field_tys.len() != fields.len() {
                    self.err(
                        *span,
                        format!(
                            "variant `{name}` expects {} field(s), found {}",
                            field_tys.len(),
                            fields.len()
                        ),
                    );
                    return;
                }
                for (fp, ft) in fields.iter().zip(field_tys) {
                    // A type pattern nested in a variant field (`Wrapper(Circle c)`) is rejected this
                    // slice (M-RT S4): the transpiler only emits simple variable bindings for variant
                    // payloads, so allowing it would diverge from `run`/`runvm`. A clean rejection
                    // keeps all three backends agreeing (the byte-identity spine). Type patterns are
                    // top-level-only — that is the match-over-union surface.
                    if let Pattern::Type {
                        type_name, span, ..
                    } = fp
                    {
                        self.err_coded(
                            *span,
                            format!(
                                "type pattern `{type_name}` is only allowed at the top level of a match arm, not inside a variant pattern"
                            ),
                            "E-MATCH-TYPE",
                            Some("match the variant first, then `instanceof`/`match` its payload".into()),
                        );
                    }
                    self.check_pattern(fp, &ft);
                }
            }
            Pattern::Type {
                type_name,
                binding,
                span,
            } => {
                // M-RT S4 type pattern: the type must be a class or interface (the runtime test is
                // `instanceof`, which is class/interface-only — an enum value is never an instance).
                let known =
                    self.classes.contains_key(type_name) || self.interfaces.contains_key(type_name);
                if !known && !matches!(scrut, Ty::Error) {
                    let hint = if self.enums.contains_key(type_name) {
                        "an enum is a closed sum — match its variants directly, not via a type pattern"
                    } else {
                        "a type pattern matches a class or interface (as in a union scrutinee)"
                    };
                    self.err_coded(
                        *span,
                        format!("type pattern `{type_name}` must name a class or interface"),
                        "E-MATCH-TYPE",
                        Some(hint.into()),
                    );
                }
                // Bind the narrowed value as the named type. A generic class carries erased (poison)
                // arguments — `instanceof` keeps no type arguments at runtime, so its generic members
                // read as `mixed`, mirroring the if/instanceof smart-cast (M-RT generics-all).
                if let Some(name) = binding {
                    let arity = self
                        .classes
                        .get(type_name)
                        .map_or(0, |c| c.type_params.len());
                    let args = vec![Ty::Error; arity];
                    self.declare(name, Ty::Named(type_name.clone(), args), *span);
                }
            }
        }
    }

    pub(super) fn expect_prim(&mut self, scrut: &Ty, want: &Ty, span: Span) {
        // A literal pattern matches when the scrutinee *is* that primitive, or is a union that
        // *contains* it (M-RT S4): `match code { 0 => …, "ok" => … }` over `int | string` is well
        // typed (the runtime/transpiler match by value, so a non-member literal simply never fires).
        let ok = *scrut == Ty::Error
            || scrut == want
            || matches!(scrut, Ty::Union(members) if members.contains(want));
        if !ok {
            self.err(
                span,
                format!("pattern of type `{want}` cannot match scrutinee of type `{scrut}`"),
            );
        }
    }
}
