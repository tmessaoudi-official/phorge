//! `impl Checker` — resolve cluster (M-Decomp W2). See checker/mod.rs for the struct + entry points.

use super::*;

impl Checker {
    /// Resolve an AST type annotation to an internal `Ty`. Records and poisons on
    /// unknown / deferred types.
    pub(super) fn resolve_type(&mut self, ty: &crate::ast::Type) -> Ty {
        use crate::ast::Type;
        match ty {
            Type::Optional { inner, .. } => Ty::Optional(Box::new(self.resolve_type(inner))),
            Type::Union(members, span) => {
                // M-RT S4: resolve each member, validate its kind (classes/interfaces/primitives
                // only — enums/optionals/functions are rejected so the PHP `A|B` emission and the
                // instanceof-based match stay sound), then normalize. A degenerate union that
                // collapses to one member after dedupe is `E-UNION-ARITY`.
                let resolved: Vec<Ty> = members.iter().map(|m| self.resolve_type(m)).collect();
                for ty in &resolved {
                    let ok = match ty {
                        Ty::Int
                        | Ty::Float
                        | Ty::Bool
                        | Ty::String
                        | Ty::Bytes
                        | Ty::Html
                        | Ty::Attr
                        | Ty::Error => true,
                        Ty::Named(n, _) => {
                            self.classes.contains_key(n) || self.interfaces.contains_key(n)
                        }
                        _ => false,
                    };
                    if !ok {
                        self.err_coded(
                            *span,
                            format!(
                                "union member `{ty}` is not allowed — members must be classes, interfaces, or primitives"
                            ),
                            "E-UNION-MEMBER",
                            Some(
                                "enum, optional `T?`, and function members are not supported in a union this slice".into(),
                            ),
                        );
                    }
                }
                let norm = Ty::union_of(resolved);
                if !matches!(norm, Ty::Union(_) | Ty::Error) {
                    // ≥2 source members collapsed to one (`A | A`): a union needs ≥2 distinct types.
                    self.err_coded(
                        *span,
                        "a union needs two or more distinct types".to_string(),
                        "E-UNION-ARITY",
                        None,
                    );
                }
                norm
            }
            Type::Intersection(members, span) => {
                // M-RT S5: resolve each member, validate kinds (D1: interfaces, plus *at most one*
                // concrete class — a value has exactly one class, so two distinct classes are the
                // bottom type), then enforce shared-method signature agreement (D2: no overloading
                // yet, so two members whose shared method differs is uninhabited) and normalize.
                let resolved: Vec<Ty> = members.iter().map(|m| self.resolve_type(m)).collect();
                let mut class_count = 0;
                for ty in &resolved {
                    match ty {
                        Ty::Error => {}
                        Ty::Named(n, _) if self.interfaces.contains_key(n) => {}
                        Ty::Named(n, _) if self.classes.contains_key(n) => class_count += 1,
                        _ => {
                            self.err_coded(
                                *span,
                                format!(
                                    "intersection member `{ty}` is not allowed — members must be interfaces, with at most one concrete class"
                                ),
                                "E-INTERSECT-MEMBER",
                                Some("primitives, enums, optionals, and function types cannot be intersection members".into()),
                            );
                        }
                    }
                }
                if class_count >= 2 {
                    self.err_coded(
                        *span,
                        "an intersection may name at most one concrete class — no value can be two distinct classes at once".to_string(),
                        "E-INTERSECT-MULTI-CLASS",
                        Some("compose with interfaces instead; a second class becomes possible only when class `extends` lands (S6)".into()),
                    );
                }
                // D2: a method declared by two members with differing signatures can be satisfied by no
                // class (Phorge has no overloading — a class has exactly one `foo`), so the intersection
                // is uninhabited. Reject it here, where it is honest about *why*.
                let mut method_sigs: HashMap<String, (Vec<Ty>, Ty)> = HashMap::new();
                let mut sig_conflict: Option<String> = None;
                for ty in &resolved {
                    if let Ty::Named(n, _) = ty {
                        let methods: Vec<(String, (Vec<Ty>, Ty))> =
                            if self.interfaces.contains_key(n) {
                                self.iface_flat_methods(n)
                            } else if let Some(info) = self.classes.get(n) {
                                // Overloaded methods (M-RT): the intersection signature-agreement check
                                // uses the first overload as the representative — a full overload-aware
                                // intersection check is a documented follow-up.
                                info.methods
                                    .iter()
                                    .filter_map(|(m, s)| {
                                        s.first().map(|s0| {
                                            (m.clone(), (s0.params.clone(), s0.ret.clone()))
                                        })
                                    })
                                    .collect()
                            } else {
                                Vec::new()
                            };
                        for (m, sig) in methods {
                            match method_sigs.get(&m) {
                                Some(existing) if *existing != sig && sig_conflict.is_none() => {
                                    sig_conflict = Some(m.clone());
                                }
                                Some(_) => {}
                                None => {
                                    method_sigs.insert(m, sig);
                                }
                            }
                        }
                    }
                }
                if let Some(m) = sig_conflict {
                    self.err_coded(
                        *span,
                        format!(
                            "intersection members declare method `{m}` with conflicting signatures — no class could implement both"
                        ),
                        "E-INTERSECT-SIG",
                        Some("a method shared across intersection members must have identical parameter and return types (Phorge has no overloading)".into()),
                    );
                }
                let norm = Ty::intersection_of(resolved);
                if !matches!(norm, Ty::Intersection(_) | Ty::Error) {
                    // ≥2 source members collapsed to one (`A & A`): an intersection needs ≥2 distinct.
                    self.err_coded(
                        *span,
                        "an intersection needs two or more distinct types".to_string(),
                        "E-INTERSECT-ARITY",
                        None,
                    );
                }
                norm
            }
            Type::Function { params, ret, .. } => Ty::Function(
                params.iter().map(|p| self.resolve_type(p)).collect(),
                Box::new(self.resolve_type(ret)),
            ),
            // `var` is intercepted in `check_stmt`; reaching here means it was written somewhere it
            // is not allowed (a parameter, field, or return type).
            Type::Infer(span) => self.err(
                *span,
                "`var` type inference is only valid for a local variable declaration",
            ),
            // Defensive: `Type::Erased` is produced by `erase_generics` *after* a successful check,
            // so a normal pipeline never resolves it. Treat it as poison so a stray re-check of an
            // already-erased program can't cascade (M-RT S7).
            Type::Erased(_) => Ty::Error,
            Type::Named { name, args, span } => match name.as_str() {
                "int" => self.no_args(name, args, *span, Ty::Int),
                "float" => self.no_args(name, args, *span, Ty::Float),
                "bool" => self.no_args(name, args, *span, Ty::Bool),
                "string" => self.no_args(name, args, *span, Ty::String),
                "bytes" => self.no_args(name, args, *span, Ty::Bytes),
                // The bottom type (M-RT totality cluster): a `-> never` function never returns. Only
                // meaningful in return position, but resolvable anywhere a type name appears.
                "never" => self.no_args(name, args, *span, Ty::Never),
                "Html" => self.no_args(name, args, *span, Ty::Html),
                "Attr" => self.no_args(name, args, *span, Ty::Attr),
                "List" => Ty::List(Box::new(self.one_arg(name, args, *span))),
                "Set" => Ty::Set(Box::new(self.one_arg(name, args, *span))),
                "Map" => {
                    if args.len() != 2 {
                        return self.err(
                            *span,
                            format!("Map expects 2 type arguments, got {}", args.len()),
                        );
                    }
                    let k = self.resolve_type(&args[0]);
                    let v = self.resolve_type(&args[1]);
                    Ty::Map(Box::new(k), Box::new(v))
                }
                "decimal" | "double" | "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32"
                | "u64" => self.err(
                    *span,
                    format!("the numeric type `{name}` is not yet supported in M1"),
                ),
                other => {
                    if self.active_type_params.iter().any(|p| p == other) {
                        // A generic type parameter in scope (`T` in `function id<T>(T x)`) is an
                        // opaque `Ty::Param`, unified away at call sites and erased before backends.
                        // A type arg on it (`T<int>`) is meaningless — reject it.
                        if args.is_empty() {
                            Ty::Param(other.to_string())
                        } else {
                            self.err(
                                *span,
                                format!("type parameter `{other}` takes no type arguments"),
                            )
                        }
                    } else if self.aliases.contains_key(other) {
                        if self.alias_stack.iter().any(|n| n == other) {
                            return self.err(*span, format!("type alias cycle through `{other}`"));
                        }
                        let aliased = self.aliases.get(other).cloned().expect("alias present");
                        self.alias_stack.push(other.to_string());
                        let ty = self.resolve_type(&aliased);
                        self.alias_stack.pop();
                        ty
                    } else if self.interfaces.contains_key(other) {
                        // Interfaces take no type arguments this slice (generic interfaces deferred —
                        // M-RT generics-all).
                        self.no_args(other, args, *span, Ty::Named(other.to_string(), Vec::new()))
                    } else if self.traits.contains(other) {
                        // M-RT S8: a trait is collected into `classes` for member lookup, but it is
                        // **not a type** — a value can never be typed as a trait. Reject it here before
                        // the class branch would accept it.
                        self.err_coded(
                            *span,
                            format!("`{other}` is a trait, not a type"),
                            "E-USE-AS-TYPE",
                            Some(
                                "a trait is reuse, not a type — `use` it in a class; you cannot type a value as a trait"
                                    .into(),
                            ),
                        )
                    } else if let Some(arity) = self
                        .classes
                        .get(other)
                        .map(|c| c.type_params.len())
                        .or_else(|| self.enums.get(other).map(|e| e.type_params.len()))
                    {
                        // A class or enum. A generic one requires exactly its declared number of type
                        // arguments (`Box<int>`, `Option<int>`); a non-generic one takes none (M-RT
                        // generics-all / generic enums).
                        if args.len() != arity {
                            let plural = if arity == 1 { "" } else { "s" };
                            self.err(
                                *span,
                                format!(
                                    "type `{other}` expects {arity} type argument{plural}, got {}",
                                    args.len()
                                ),
                            )
                        } else {
                            let resolved = args.iter().map(|a| self.resolve_type(a)).collect();
                            Ty::Named(other.to_string(), resolved)
                        }
                    } else {
                        self.err_coded(
                            *span,
                            format!("unknown type `{other}`"),
                            "E-UNKNOWN-TYPE",
                            None,
                        )
                    }
                }
            },
        }
    }

    pub(super) fn no_args(
        &mut self,
        name: &str,
        args: &[crate::ast::Type],
        span: Span,
        ty: Ty,
    ) -> Ty {
        if args.is_empty() {
            ty
        } else {
            self.err(span, format!("type `{name}` takes no type arguments"))
        }
    }

    pub(super) fn one_arg(&mut self, name: &str, args: &[crate::ast::Type], span: Span) -> Ty {
        if args.len() != 1 {
            self.err(
                span,
                format!("{name} expects 1 type argument, got {}", args.len()),
            );
            Ty::Error
        } else {
            self.resolve_type(&args[0])
        }
    }
}
