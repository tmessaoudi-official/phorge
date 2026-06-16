//! Runtime values for the M1 tree-walking evaluator. Owned + `Clone` (no `Rc`):
//! M1 has no reassignment or post-construction mutation (Plan 3), so shared
//! mutability is unneeded. See design spec EV-1.

use std::cmp::Ordering;
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

// --- Arithmetic & comparison kernels (single-sourced; both backends call these) ---
//
// The `Op::Neg` parity bug (M2 P3.5 Wave 0) was possible because integer arithmetic lived in two
// hand-kept-identical copies, one per backend. These kernels are the *one* implementation both the
// tree-walker (`interpreter::arith`/`eval_unary`/`compare`) and the VM (`vm.rs` arith arms +
// `compare`) dispatch into, so the two can no longer drift. They return the bare fault *body*
// (`String`); each backend wraps it in its own error type. Floats can't fault (NaN/inf are valid
// `f64`); only integer overflow and integer division/modulo by zero are faults. The op→bool / op→fn
// projection stays in each backend — their op enums (`BinaryOp` vs `Op`) differ, so only the
// arithmetic and the fault strings are shared, not the dispatch.

/// Canonical fault body for integer `x / 0`. Single-sourced so `run` ≡ `runvm` in the fault path.
pub const FAULT_DIV_ZERO: &str = "division by zero";
/// Canonical fault body for integer `x % 0`.
pub const FAULT_MOD_ZERO: &str = "modulo by zero";
/// Canonical fault body for any integer op whose result leaves `i64` range
/// (`MAX + 1`, `MIN - 1`, `MIN * -1`, `MIN / -1`, `MIN % -1`, `-MIN`).
pub const FAULT_INT_OVERFLOW: &str = "integer overflow";

/// Checked integer addition; overflow is a clean fault, never a panic (EV-7).
pub fn int_add(a: i64, b: i64) -> Result<i64, String> {
    a.checked_add(b)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// Checked integer subtraction.
pub fn int_sub(a: i64, b: i64) -> Result<i64, String> {
    a.checked_sub(b)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// Checked integer multiplication.
pub fn int_mul(a: i64, b: i64) -> Result<i64, String> {
    a.checked_mul(b)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// Checked integer division. `b == 0` is `FAULT_DIV_ZERO`; `i64::MIN / -1` overflows.
pub fn int_div(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 {
        return Err(FAULT_DIV_ZERO.to_string());
    }
    a.checked_div(b)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// Checked integer remainder. `b == 0` is `FAULT_MOD_ZERO`; `i64::MIN % -1` overflows.
pub fn int_rem(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 {
        return Err(FAULT_MOD_ZERO.to_string());
    }
    a.checked_rem(b)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// Checked integer negation. `-i64::MIN` overflows (the exact Wave 0 P0 case).
pub fn int_neg(n: i64) -> Result<i64, String> {
    n.checked_neg()
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}

/// Float addition. Floats never fault — NaN/inf are valid `f64`.
pub fn float_add(a: f64, b: f64) -> f64 {
    a + b
}
/// Float subtraction.
pub fn float_sub(a: f64, b: f64) -> f64 {
    a - b
}
/// Float multiplication.
pub fn float_mul(a: f64, b: f64) -> f64 {
    a * b
}
/// Float division (`b == 0.0` yields `inf`/`NaN`, not a fault).
pub fn float_div(a: f64, b: f64) -> f64 {
    a / b
}
/// Float remainder.
pub fn float_rem(a: f64, b: f64) -> f64 {
    a % b
}

/// Ordering probe for `< > <= >=`. `Ok(None)` is the NaN case (every ordered comparison of NaN is
/// `false`); `Err` is a non-comparable operand pairing. The op→bool projection stays backend-local
/// (the op enums differ); only the ordering and the comparability fault are shared.
pub fn compare_ord(a: &Value, b: &Value) -> Result<Option<Ordering>, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.partial_cmp(y)),
        (Value::Float(x), Value::Float(y)) => Ok(x.partial_cmp(y)),
        _ => Err(format!(
            "cannot compare {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_kernels_fault_and_overflow() {
        assert_eq!(int_add(2, 3), Ok(5));
        assert_eq!(int_add(i64::MAX, 1), Err(FAULT_INT_OVERFLOW.to_string()));
        assert_eq!(int_sub(i64::MIN, 1), Err(FAULT_INT_OVERFLOW.to_string()));
        assert_eq!(int_mul(i64::MAX, 2), Err(FAULT_INT_OVERFLOW.to_string()));
        assert_eq!(int_div(7, 2), Ok(3));
        assert_eq!(int_div(1, 0), Err(FAULT_DIV_ZERO.to_string()));
        assert_eq!(int_div(i64::MIN, -1), Err(FAULT_INT_OVERFLOW.to_string()));
        assert_eq!(int_rem(7, 3), Ok(1));
        assert_eq!(int_rem(1, 0), Err(FAULT_MOD_ZERO.to_string()));
        assert_eq!(int_neg(5), Ok(-5));
        assert_eq!(int_neg(i64::MIN), Err(FAULT_INT_OVERFLOW.to_string()));
    }

    #[test]
    fn compare_ord_matches_both_backends() {
        assert_eq!(
            compare_ord(&Value::Int(1), &Value::Int(2)),
            Ok(Some(Ordering::Less))
        );
        assert_eq!(
            compare_ord(&Value::Float(2.0), &Value::Float(2.0)),
            Ok(Some(Ordering::Equal))
        );
        // NaN: comparable type, but no ordering -> Ok(None) (callers project to `false`).
        assert_eq!(
            compare_ord(&Value::Float(f64::NAN), &Value::Float(1.0)),
            Ok(None)
        );
        // Mixed/non-numeric operands are a comparability fault.
        assert!(compare_ord(&Value::Int(1), &Value::Float(1.0)).is_err());
        assert!(compare_ord(&Value::Bool(true), &Value::Bool(false)).is_err());
    }

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
