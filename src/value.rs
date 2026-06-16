//! Runtime values for the M1 tree-walking evaluator. Owned + `Clone` (no `Rc`):
//! M1 has no reassignment or post-construction mutation (Plan 3), so shared
//! mutability is unneeded. See design spec EV-1.

use std::collections::{HashMap, HashSet};

/// Maximum call-frame depth, enforced **identically by both backends** — the interpreter's
/// `run_call` depth counter and the VM's `frames` cap. Exceeding it is a clean `"stack overflow"`
/// runtime error (exit 1), never an abort. A *single shared* limit is what keeps `run` ≡ `runvm`
/// in the fault path: separate limits would let one backend succeed where the other errors.
///
/// The value is far below what the VM's heap-allocated frames could hold (it formerly capped at
/// `64*1024`) because the interpreter recurses on the *native* Rust stack (~14 KB/frame in debug,
/// so ~875 frames fit a default 12.2 MB stack). `cli::cmd_run`/`cmd_runvm` run the whole pipeline
/// on a dedicated 256 MB-stack thread so this limit is reachable with >4× native margin.
/// Centralised into a `Limits` module by roadmap Task 2.2.
pub const MAX_CALL_DEPTH: usize = 4096;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    List(Vec<Value>),
    /// Constructible in principle; the M1 sample never builds or indexes one.
    Map(HashMap<HKey, Value>),
    Set(HashSet<HKey>),
    Instance(Box<Instance>),
    Enum(Box<EnumVal>),
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub class: String,
    pub fields: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct EnumVal {
    pub ty: String,
    pub variant: String,
    pub payload: Vec<Value>,
}

/// Hashable key subset for `Map`/`Set` (`Value` can't derive `Hash`/`Eq`: it
/// holds `f64`). Unused by the M1 sample but required by the value-type signatures.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HKey {
    Int(i64),
    Bool(bool),
    Str(String),
}

impl Value {
    /// Short name for diagnostics. Composite types fold to a constant so the
    /// return can stay `&'static str`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::Str(_) => "string",
            Value::Unit => "unit",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Set(_) => "set",
            Value::Instance(_) => "instance",
            Value::Enum(_) => "enum",
        }
    }

    /// Render a *primitive* value for interpolation / `println`. `None` for a
    /// composite value (the caller turns that into a `RuntimeError`). Floats use
    /// Rust `{}` formatting (EV-6): `12.0` -> `"12"`.
    pub fn as_display(&self) -> Option<String> {
        match self {
            Value::Int(n) => Some(n.to_string()),
            Value::Float(x) => Some(format!("{x}")),
            Value::Bool(b) => Some(b.to_string()),
            Value::Str(s) => Some(s.clone()),
            Value::Unit => Some("unit".to_string()),
            _ => None,
        }
    }

    /// Structural value equality for `==` / `!=` / `is`.
    #[allow(clippy::float_cmp)] // intentional: language-level float equality
    pub fn eq_val(&self, other: &Value) -> bool {
        use Value::*;
        match (self, other) {
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (Bool(a), Bool(b)) => a == b,
            (Str(a), Str(b)) => a == b,
            (Unit, Unit) => true,
            (List(a), List(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_val(y)),
            (Enum(a), Enum(b)) => {
                a.ty == b.ty
                    && a.variant == b.variant
                    && a.payload.len() == b.payload.len()
                    && a.payload.iter().zip(&b.payload).all(|(x, y)| x.eq_val(y))
            }
            (Instance(a), Instance(b)) => {
                a.class == b.class
                    && a.fields.len() == b.fields.len()
                    && a.fields
                        .iter()
                        .all(|(k, v)| b.fields.get(k).is_some_and(|bv| v.eq_val(bv)))
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_display_renders_primitives() {
        assert_eq!(Value::Int(42).as_display().as_deref(), Some("42"));
        assert_eq!(Value::Float(12.0).as_display().as_deref(), Some("12"));
        assert_eq!(
            Value::Float(12.56636).as_display().as_deref(),
            Some("12.56636")
        );
        assert_eq!(Value::Bool(true).as_display().as_deref(), Some("true"));
        assert_eq!(Value::Str("hi".into()).as_display().as_deref(), Some("hi"));
    }

    #[test]
    fn as_display_is_none_for_composite() {
        let inst = Value::Instance(Box::new(Instance {
            class: "Greeter".into(),
            fields: HashMap::new(),
        }));
        assert!(inst.as_display().is_none());
    }

    #[test]
    fn eq_val_matches_by_value() {
        assert!(Value::Int(1).eq_val(&Value::Int(1)));
        assert!(!Value::Int(1).eq_val(&Value::Int(2)));
        assert!(!Value::Int(1).eq_val(&Value::Float(1.0))); // no cross-type eq
        let a = Value::Enum(Box::new(EnumVal {
            ty: "Shape".into(),
            variant: "Circle".into(),
            payload: vec![Value::Float(2.0)],
        }));
        let b = a.clone();
        assert!(a.eq_val(&b));
    }

    #[test]
    fn type_name_is_stable() {
        assert_eq!(Value::Unit.type_name(), "unit");
        assert_eq!(Value::List(vec![]).type_name(), "list");
    }
}
