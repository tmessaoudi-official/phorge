//! Resolved (internal) type representation, distinct from the AST's `Type`.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    String,
    /// A sequence of raw octets (not UTF-8). Converts to/from `string` only via `core.bytes`.
    Bytes,
    /// Escaped, render-ready HTML. A distinct *nominal* type — like `bytes`, it erases to PHP
    /// `string` at transpile and rides `Value::Str` at runtime, but the checker keeps it separate
    /// from `string`: untrusted text cannot reach `Html` except through `core.html.text` (escape) or
    /// the audited `core.html.raw`. That non-interchangeability is the whole XSS-safety property, and
    /// it falls out for free from `assignable`'s `from == to` final arm (no coercion added).
    Html,
    /// A single rendered HTML attribute — e.g. ` href="…"` (note the leading space) — produced by
    /// `core.html.attr` / `bool_attr` and consumed by `core.html.el` / `void_el`. Like `Html`, a
    /// distinct nominal type that erases to PHP `string` and rides `Value::Str` at runtime; kept
    /// separate so an attribute fragment can't be spliced where element content (`Html`) is expected
    /// and vice versa. core.html Wave 2 (the builders).
    Attr,
    Unit,
    /// A nominal enum or class type, by name.
    Named(String),
    List(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Set(Box<Ty>),
    /// `T?` — an optional: holds a `T` or `null`. The non-null guarantee lives in
    /// `assignable` (a non-optional `T` can never hold `null`).
    Optional(Box<Ty>),
    /// The type of the bare `null` literal: assignable to any `T?` and to nothing else. Lets
    /// `null` flow into an optional with no element type, while `var x = null;` stays an error.
    Null,
    /// Poison type: a failed sub-expression yields this. Assignable both ways so a
    /// single error does not cascade into many.
    Error,
    /// A function type: `(int, string) -> bool`. Exact match only — no subtyping variance (A6).
    Function(Vec<Ty>, Box<Ty>),
}

impl Ty {
    /// `from` may be used where `to` is expected. `Error` unifies with anything to
    /// suppress cascade errors. No numeric widening (spec §3: no implicit coercion).
    /// Optionals are covariant and non-null-disciplined: a non-optional `T` widens to
    /// `T?` (and `U?` -> `T?` when `U` -> `T`), but a `T?` never widens to a
    /// non-optional `T` — it must be unwrapped (`??`/`?.`/`if (var …)`/`!`).
    pub fn assignable(from: &Ty, to: &Ty) -> bool {
        if *from == Ty::Error || *to == Ty::Error {
            return true;
        }
        match (from, to) {
            // A bare `null` fits any optional (and itself); nothing else accepts it.
            (Ty::Null, Ty::Optional(_) | Ty::Null) => true,
            (Ty::Null, _) => false,
            // `U? -> T?` when `U -> T`; a non-optional `T -> T?` (covariant widening).
            (Ty::Optional(f), Ty::Optional(t)) => Ty::assignable(f, t),
            (other, Ty::Optional(t)) => Ty::assignable(other, t),
            // Function types are exact-match only — no co/contra-variance (spec A6).
            (Ty::Function(fp, fr), Ty::Function(tp, tr)) => {
                fp.len() == tp.len() && fp.iter().zip(tp.iter()).all(|(a, b)| a == b) && fr == tr
            }
            // A `T?` never widens to a non-optional `T` — it must be unwrapped.
            _ => from == to,
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Float => write!(f, "float"),
            Ty::Bool => write!(f, "bool"),
            Ty::String => write!(f, "string"),
            Ty::Bytes => write!(f, "bytes"),
            Ty::Html => write!(f, "Html"),
            Ty::Attr => write!(f, "Attr"),
            Ty::Unit => write!(f, "unit"),
            Ty::Named(n) => write!(f, "{n}"),
            Ty::List(e) => write!(f, "List<{e}>"),
            Ty::Map(k, v) => write!(f, "Map<{k}, {v}>"),
            Ty::Set(e) => write!(f, "Set<{e}>"),
            Ty::Optional(e) => write!(f, "{e}?"),
            Ty::Null => write!(f, "null"),
            Ty::Error => write!(f, "<error>"),
            Ty::Function(params, ret) => {
                let ps = params
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "({ps}) -> {ret}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignable_is_equality_plus_error() {
        assert!(Ty::assignable(&Ty::Int, &Ty::Int));
        assert!(!Ty::assignable(&Ty::Int, &Ty::Float)); // no widening
        assert!(Ty::assignable(&Ty::Error, &Ty::Int)); // poison unifies
        assert!(Ty::assignable(&Ty::Int, &Ty::Error));
        assert!(Ty::assignable(
            &Ty::List(Box::new(Ty::Int)),
            &Ty::List(Box::new(Ty::Int))
        ));
        assert!(!Ty::assignable(
            &Ty::List(Box::new(Ty::Int)),
            &Ty::List(Box::new(Ty::Float))
        ));
    }

    #[test]
    fn html_is_not_interchangeable_with_string() {
        // The XSS-safety wall: a raw `string` cannot stand in for `Html`, and vice versa. The only
        // bridges are `core.html.text`/`raw` (string -> Html) and `core.html.render` (Html -> string).
        assert!(Ty::assignable(&Ty::Html, &Ty::Html));
        assert!(!Ty::assignable(&Ty::String, &Ty::Html)); // untrusted text can't become HTML
        assert!(!Ty::assignable(&Ty::Html, &Ty::String)); // and HTML must be explicitly rendered out
        assert_eq!(Ty::Html.to_string(), "Html");
    }

    #[test]
    fn optional_assignability() {
        let int_opt = Ty::Optional(Box::new(Ty::Int));
        assert!(Ty::assignable(&Ty::Int, &int_opt)); // T -> T? (widen)
        assert!(!Ty::assignable(&int_opt, &Ty::Int)); // T? -/-> T (must unwrap)
        assert!(Ty::assignable(&int_opt, &int_opt)); // T? -> T?
        assert!(!Ty::assignable(
            &Ty::Optional(Box::new(Ty::Int)),
            &Ty::Optional(Box::new(Ty::Float))
        ));
        assert_eq!(int_opt.to_string(), "int?"); // Display
                                                 // the bare-`null` type fits any optional and nothing else
        assert!(Ty::assignable(&Ty::Null, &int_opt)); // null -> int?
        assert!(!Ty::assignable(&Ty::Null, &Ty::Int)); // null -/-> int
        assert_eq!(Ty::Null.to_string(), "null");
    }

    #[test]
    fn display_renders_generics() {
        assert_eq!(
            Ty::List(Box::new(Ty::Named("Shape".into()))).to_string(),
            "List<Shape>"
        );
    }

    #[test]
    fn function_type_assignability_is_exact() {
        let int_to_int = Ty::Function(vec![Ty::Int], Box::new(Ty::Int));
        let int_to_int2 = Ty::Function(vec![Ty::Int], Box::new(Ty::Int));
        let int_to_float = Ty::Function(vec![Ty::Int], Box::new(Ty::Float));
        assert!(Ty::assignable(&int_to_int, &int_to_int2));
        assert!(!Ty::assignable(&int_to_int, &int_to_float)); // no variance (A6)
        assert!(!Ty::assignable(&Ty::Int, &int_to_int)); // int is not a function
        assert_eq!(format!("{int_to_int}"), "(int) -> int");
    }
}
