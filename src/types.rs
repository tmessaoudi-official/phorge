//! Resolved (internal) type representation, distinct from the AST's `Type`.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    String,
    Unit,
    /// A nominal enum or class type, by name.
    Named(String),
    List(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Set(Box<Ty>),
    /// Poison type: a failed sub-expression yields this. Assignable both ways so a
    /// single error does not cascade into many.
    Error,
}

impl Ty {
    /// `from` may be used where `to` is expected. `Error` unifies with anything to
    /// suppress cascade errors. No numeric widening (spec §3: no implicit coercion).
    pub fn assignable(from: &Ty, to: &Ty) -> bool {
        *from == Ty::Error || *to == Ty::Error || from == to
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Float => write!(f, "float"),
            Ty::Bool => write!(f, "bool"),
            Ty::String => write!(f, "string"),
            Ty::Unit => write!(f, "unit"),
            Ty::Named(n) => write!(f, "{n}"),
            Ty::List(e) => write!(f, "List<{e}>"),
            Ty::Map(k, v) => write!(f, "Map<{k}, {v}>"),
            Ty::Set(e) => write!(f, "Set<{e}>"),
            Ty::Error => write!(f, "<error>"),
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
    fn display_renders_generics() {
        assert_eq!(
            Ty::List(Box::new(Ty::Named("Shape".into()))).to_string(),
            "List<Shape>"
        );
    }
}
