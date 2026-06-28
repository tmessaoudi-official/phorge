//! Core.Reflect — the precise `typeName` static-type pass (Tier 3 of
//! `docs/specs/2026-06-25-core-reflect-design.md`).
//!
//! `Reflect.typeName(x)` cannot be both precise (distinguish `List`/`Map`/`Set`/`bytes`/enum) AND a
//! plain runtime native (PHP erases those distinctions — a `Map` is just a PHP `array`). The locked
//! resolution: compute the answer from `x`'s **static type** at compile time, then erase the call to
//! that answer before any backend — so all three backends emit the *same* result and PHP's erased
//! runtime is never consulted. A value type → a baked string literal; an object → the runtime
//! `Reflect.className(x)` (`get_class`, byte-identical); an optional → a single-eval `match`
//! null-branch; an erased generic → the coarse `Reflect.kind(x)`.
//!
//! This is the same "erase front-end sugar before any backend" discipline as `type` aliases /
//! generics / `html"…"` / UFCS: the checker records a span-keyed substitution; [`rewrite_ufcs`]
//! (fed the merged call-rewrite map) applies it. No new `Op`/`Value`; byte-identical by construction.

use super::*;
use crate::ast::{Expr, MatchArm, Pattern, StrPart};
use crate::token::Span;
use crate::types::Ty;

/// A span that can never collide with a real call-site rewrite key: a `Span.start` is a byte offset
/// into the source, and `usize::MAX` is never one. Every node `typeName` *synthesizes* uses it, so
/// the combined call-rewrite walker never re-matches a synthesized node's root as a rewrite key
/// (which would loop), while the embedded original argument keeps its real span and is still
/// re-resolved for nested sugar (UFCS / nested `typeName`).
const fn synth_span() -> Span {
    Span {
        start: usize::MAX,
        len: 0,
        line: 0,
        col: 0,
    }
}

impl Checker {
    /// Intercept `q.typeName(x)` (before the generic-native path): check the argument, record a
    /// span-keyed precise replacement built from its STATIC type, and yield `string`. `qual` is the
    /// qualifier the call used (`Reflect`, or a user alias) — reused for the synthesized
    /// `className`/`kind` calls so the transpiler's import-map resolution agrees with the
    /// interpreter/compiler's leaf resolution.
    pub(super) fn check_reflect_type_name(&mut self, qual: &str, args: &[Expr], span: Span) -> Ty {
        if args.len() != 1 {
            for a in args {
                self.check_expr(a);
            }
            return self.err(span, "Reflect.typeName expects exactly one argument");
        }
        let arg_ty = self.check_expr(&args[0]);
        // The argument already failed to type — it is reported; still surface `string` so a downstream
        // use of the result isn't a second, cascading error.
        if matches!(arg_ty, Ty::Error) {
            return Ty::String;
        }
        let repl = self.type_name_replacement(&arg_ty, &args[0], qual);
        self.reflect_resolutions.insert(span.start, repl);
        Ty::String
    }

    /// Build the precise `typeName` replacement for a value of static type `ty`, applying runtime ops
    /// to `value` (the expression to evaluate — the original argument at top level, the bound non-null
    /// value inside an optional's match arm). `qual` is the Reflect qualifier to emit on synthesized
    /// calls.
    fn type_name_replacement(&self, ty: &Ty, value: &Expr, qual: &str) -> Expr {
        match ty {
            Ty::Int => lit("int"),
            Ty::Float => lit("float"),
            Ty::Bool => lit("bool"),
            Ty::String => lit("string"),
            Ty::Bytes => lit("bytes"),
            Ty::Html => lit("Html"),
            Ty::Attr => lit("Attr"),
            // A fixed-length list is still a `List` at runtime.
            Ty::List(_) | Ty::FixedList(..) => lit("List"),
            Ty::Map(..) => lit("Map"),
            Ty::Set(_) => lit("Set"),
            Ty::Function(..) => lit("function"),
            Ty::Null => lit("null"),
            // A nominal: an enum is named by the ENUM (the static type — variants aren't types), a
            // class/interface by its RUNTIME class (`className` ≡ `get_class`, byte-identical).
            Ty::Named(name, _) => {
                if self.enums.contains_key(name) {
                    lit(name)
                } else {
                    reflect_call(qual, "className", value.clone())
                }
            }
            // A union/intersection value is always a concrete instance → its runtime class.
            Ty::Union(_) | Ty::Intersection(_) => reflect_call(qual, "className", value.clone()),
            // Optional: evaluate `value` ONCE via `match`; null → "null", else the inner rule on the
            // bound (non-null) value. The scrutinee keeps `value`'s real span (re-resolved for nested
            // sugar); everything synthesized uses `synth_span()`. Single-eval — no double evaluation
            // of a side-effecting argument (the reason this isn't a `value === null ? … : …` ternary).
            Ty::Optional(inner) => {
                let binder = "__phorj_tn".to_string();
                let bound = Expr::Ident(binder.clone(), synth_span());
                let inner_repl = self.type_name_replacement(inner, &bound, qual);
                Expr::Match {
                    scrutinee: Box::new(value.clone()),
                    arms: vec![
                        MatchArm {
                            pattern: Pattern::Null(synth_span()),
                            guard: None,
                            body: lit("null"),
                            span: synth_span(),
                        },
                        MatchArm {
                            pattern: Pattern::Binding {
                                name: binder,
                                span: synth_span(),
                            },
                            guard: None,
                            body: inner_repl,
                            span: synth_span(),
                        },
                    ],
                    span: synth_span(),
                }
            }
            // Erased generic / unbound type parameter / anything not statically known → coarse `kind`.
            _ => reflect_call(qual, "kind", value.clone()),
        }
    }
}

/// A baked string-literal `Expr` (one literal part) — the byte-identical case: all three backends
/// emit the same string, so PHP's erasure is irrelevant.
fn lit(s: &str) -> Expr {
    Expr::Str(vec![StrPart::Literal(s.to_string())], synth_span())
}

/// `<qual>.<name>(value)` — a synthesized Reflect native call (`className`/`kind`). `qual` is the
/// qualifier the user imported `Reflect` as, so every backend resolves it identically (the transpiler
/// keys the import map by the written qualifier; the interpreter/compiler resolve by module leaf).
fn reflect_call(qual: &str, name: &str, value: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Member {
            object: Box::new(Expr::Ident(qual.to_string(), synth_span())),
            name: name.to_string(),
            safe: false,
            span: synth_span(),
        }),
        args: vec![value],
        span: synth_span(),
    }
}
