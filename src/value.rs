//! Runtime values for both backends. The M1 heap is **immutable + acyclic**: no reassignment, no
//! post-construction field mutation, and a constructor's args are fully evaluated before the
//! instance exists (EV-1). So compound objects are *shared* via `Rc`, not deep-cloned (M2 P5a):
//! cloning a `Value` (the `Op::GetLocal` hot path + every interpreter var-read) is a refcount bump,
//! and `Drop` reclaims correctly — no cycle can leak, so no tracing collector is needed (that is
//! deferred to M3, when mutation could create cycles). See `docs/specs/2026-06-16-m2-p5-object-model-design.md`.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    /// Raw octet sequence (`bytes`). Shared (like `List`) — cloning is a refcount bump. Distinct from
    /// `Str` (which is UTF-8); converts only via the `core.bytes` natives (M6 W0).
    Bytes(Rc<Vec<u8>>),
    Unit,
    /// `null` — the sole inhabitant of an absent optional (`T?`). A non-optional `T` never holds it
    /// (the checker's non-null discipline); PHP-native, erases to PHP `null` (M3 S2).
    Null,
    /// Shared (M2 P5a): cloning a list value is a refcount bump, not a deep element copy.
    List(Rc<Vec<Value>>),
    /// An **insertion-ordered** key→value map (M-RT S3). The order is part of the value: PHP arrays
    /// preserve insertion order, so a `Vec` of pairs (not a `HashMap`) is what keeps a future
    /// `keys()`/iteration byte-identical with the PHP target (risk R1). Shared via `Rc` like `List`
    /// (cloning is a refcount bump). Built and indexed only through the `build_map`/`map_index`
    /// kernels below, so both backends agree on dedup and lookup semantics.
    Map(Rc<Vec<(HKey, Value)>>),
    /// An **insertion-ordered** set of hashable keys (M-RT S7b). Like `Map`, the order is part of the
    /// value (not a `HashSet`): PHP arrays preserve insertion order, so a `Vec` of keys keeps a future
    /// `Set` iteration / `array_values` byte-identical with the PHP target (risk R1). Shared via `Rc`
    /// like `List`/`Map` (cloning is a refcount bump). Built only through the `build_set` kernel below,
    /// so both backends dedup identically.
    Set(Rc<Vec<HKey>>),
    Instance(Rc<Instance>),
    Enum(Rc<EnumVal>),
    /// A first-class function value: either a tree-walking closure (interpreter),
    /// a bare named-function reference, or a VM bytecode closure (Task 4).
    Closure(Rc<ClosureData>),
}

/// The data of a first-class function value (M3 S3, Task 3).
///
/// - `Tree`: an expression-body lambda captured from the tree-walking interpreter.
/// - `Named`: a bare named-function reference (the name is resolved at call time).
/// - `Byte`: a bytecode closure constructed by the VM in Task 4; constructing it in the
///   interpreter is a bug — any such path panics with `unreachable!`.
#[derive(Debug, Clone)]
pub enum ClosureData {
    Tree {
        params: Vec<crate::ast::Param>,
        ret: Option<crate::ast::Type>,
        body: crate::ast::LambdaBody,
        env: Vec<(String, Value)>,
    },
    Named(String),
    /// Bytecode closure — constructed by the VM (Task 4). The interpreter never constructs
    /// this variant; encountering it at runtime is a bug (`unreachable!`).
    Byte {
        func: usize,
        captures: Vec<Value>,
    },
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

impl HKey {
    /// Project a runtime `Value` onto the hashable key subset, or `None` if it isn't a valid map key
    /// (`float`, list, instance, …). The checker forbids non-`{int,bool,string}` key *types*
    /// (`E-MAP-KEY`) and types the index of `m[k]` against the map's key type, so a `None` here is
    /// checker-unreachable — the callers turn it into a clean fault rather than a panic (EV-7).
    pub fn from_value(v: &Value) -> Option<HKey> {
        match v {
            Value::Int(n) => Some(HKey::Int(*n)),
            Value::Bool(b) => Some(HKey::Bool(*b)),
            Value::Str(s) => Some(HKey::Str(s.clone())),
            _ => None,
        }
    }

    /// Inverse of [`HKey::from_value`] — used when a key flows back out as a `Value` (a future
    /// `keys()` native). Total: every `HKey` variant maps to exactly one `Value`.
    pub fn to_value(&self) -> Value {
        match self {
            HKey::Int(n) => Value::Int(*n),
            HKey::Bool(b) => Value::Bool(*b),
            HKey::Str(s) => Value::Str(s.clone()),
        }
    }
}

/// Build an **insertion-ordered** map from evaluated `(key, value)` pairs, matching PHP literal
/// semantics: a duplicate key keeps its **first position** but takes the **last value**
/// (`["a" => 1, "a" => 2]` ⇒ `["a" => 2]`, position of the first `"a"`). Single-sourced so the
/// interpreter (`Expr::Map`) and the VM (`Op::MakeMap`) dedup identically — `run ≡ runvm` (and a
/// non-`HKey` key, checker-unreachable, faults cleanly rather than panicking, EV-7).
pub fn build_map(pairs: Vec<(Value, Value)>) -> Result<Vec<(HKey, Value)>, String> {
    let mut out: Vec<(HKey, Value)> = Vec::with_capacity(pairs.len());
    for (k, v) in pairs {
        let key =
            HKey::from_value(&k).ok_or_else(|| format!("invalid map key: {}", k.type_name()))?;
        if let Some(slot) = out.iter_mut().find(|(ek, _)| *ek == key) {
            slot.1 = v; // existing key: keep first position, take last value (PHP semantics)
        } else {
            out.push((key, v));
        }
    }
    Ok(out)
}

/// Build an **insertion-ordered, deduplicated** set from evaluated element values, keeping each
/// element's **first occurrence** and discarding later duplicates (`Set.of([1, 2, 1]) ⇒ {1, 2}`,
/// in that order) — the same first-seen-order discipline as [`build_map`]'s keys. Single-sourced so
/// the interpreter and the VM dedup identically (`run ≡ runvm`); a non-`HKey` element
/// (checker-unreachable, the checker constrains a `Set<T>` element to the hashable subset) faults
/// cleanly rather than panicking (EV-7).
pub fn build_set(elems: Vec<Value>) -> Result<Vec<HKey>, String> {
    let mut out: Vec<HKey> = Vec::with_capacity(elems.len());
    for e in elems {
        let key = HKey::from_value(&e)
            .ok_or_else(|| format!("invalid set element: {}", e.type_name()))?;
        if !out.contains(&key) {
            out.push(key);
        }
    }
    Ok(out)
}

/// Look a key up in an insertion-ordered map. A missing key is a clean fault (`"map key not found"`),
/// byte-identical across both backends — the differential harness excludes fault cases, and the
/// present-key path is byte-identical to PHP `$m[$k]`. A non-`HKey` index is checker-unreachable
/// (`m[k]` types `k` against the map's key type) but handled defensively (EV-7).
pub fn map_index(map: &[(HKey, Value)], index: &Value) -> Result<Value, String> {
    let key =
        HKey::from_value(index).ok_or_else(|| format!("invalid map key: {}", index.type_name()))?;
    map.iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.clone())
        .ok_or_else(|| "map key not found".to_string())
}

/// Set `list[idx] = v` in place with bounds-checking (M-mut.5). The caller owns the copy-on-write
/// (`Rc::make_mut` before calling), so this mutates a uniquely-owned `Vec`. An out-of-range index
/// faults identically to a read (`"list index out of range"`, `FaultKind::IndexOob`) — note this
/// diverges from PHP, which would *extend* the array; examples only set in-bounds (KNOWN_ISSUES).
pub fn list_set(list: &mut [Value], idx: i64, v: Value) -> Result<(), String> {
    let i = usize::try_from(idx)
        .ok()
        .filter(|i| *i < list.len())
        .ok_or_else(|| "list index out of range".to_string())?;
    list[i] = v;
    Ok(())
}

/// Set `map[key] = v` (M-mut.5): update in place if `key` is present, else append — insertion-ordered
/// like PHP `$m[$k] = $v`, preserving the `Rc<Vec<(HKey, Value)>>` order invariant (R1). The caller
/// owns the COW. A non-`HKey` key is checker-unreachable (EV-7).
pub fn map_set(map: &mut Vec<(HKey, Value)>, key: &Value, v: Value) -> Result<(), String> {
    let k = HKey::from_value(key).ok_or_else(|| format!("invalid map key: {}", key.type_name()))?;
    if let Some(slot) = map.iter_mut().find(|(ek, _)| *ek == k) {
        slot.1 = v;
    } else {
        map.push((k, v));
    }
    Ok(())
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
            Value::Bytes(_) => "bytes",
            Value::Unit => "unit",
            Value::Null => "null",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Set(_) => "set",
            Value::Instance(_) => "instance",
            Value::Enum(_) => "enum",
            Value::Closure(_) => "function",
        }
    }

    /// Render a *primitive* value for interpolation / `println`. `None` for a
    /// composite value (the caller turns that into a runtime `Diagnostic`). Floats use
    /// Rust `{}` formatting (EV-6): `12.0` -> `"12"`.
    pub fn as_display(&self) -> Option<String> {
        match self {
            Value::Int(n) => Some(n.to_string()),
            Value::Float(x) => Some(format!("{x}")),
            Value::Bool(b) => Some(b.to_string()),
            Value::Str(s) => Some(s.clone()),
            Value::Unit => Some("unit".to_string()),
            // Functions cannot be displayed (the checker forbids interpolating a function
            // value; this arm is only reached through the fallback `_ => None` path — EV-7).
            Value::Closure(_) => None,
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
            (Bytes(a), Bytes(b)) => a == b,
            (Unit, Unit) => true,
            (List(a), List(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.eq_val(y))
            }
            // Maps compare **order-independently** (insertion order is part of iteration, not of
            // identity): same key set with `eq_val` values. This matches PHP associative `==`.
            (Map(a), Map(b)) => {
                a.len() == b.len()
                    && a.iter().all(|(k, v)| {
                        b.iter()
                            .find(|(bk, _)| bk == k)
                            .is_some_and(|(_, bv)| v.eq_val(bv))
                    })
            }
            // Sets compare **order-independently** (insertion order is iteration, not identity):
            // same cardinality and same membership. Both are deduped by `build_set`, so a one-way
            // containment check at equal length suffices.
            (Set(a), Set(b)) => a.len() == b.len() && a.iter().all(|k| b.contains(k)),
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
            (Null, Null) => true,
            // Functions are not comparable — the checker forbids `==`/`!=` on function
            // types; this arm is a defensive fallback (EV-7, well-typed programs never reach it).
            (Closure(_), _) | (_, Closure(_)) => false,
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

/// Maximum number of elements a range literal may materialize before faulting (P1-#9). An unbounded
/// `0..n` would otherwise allocate an arbitrarily large `Vec` and abort the process (OOM, exit 101)
/// instead of producing a clean, byte-identical fault on both backends (EV-7). ~10M × 16 B ≈ 160 MB
/// ceiling — generous for any realistic program, well below uncontrolled OOM. Tunable.
pub const MAX_RANGE_LEN: i64 = 10_000_000;

/// Materialize an integer range exactly as both backends do, with a shared size guard (P1-#9). `hi`
/// is the inclusive upper bound: `end` for `..=`, `end - 1` for `..`. An empty/reversed range
/// (`start > hi`) yields `[]`. A range wider than [`MAX_RANGE_LEN`] faults `"range too large"` rather
/// than OOM-aborting. All arithmetic is checked (EV-7): `end - 1` underflow (exclusive `..i64::MIN`)
/// and `hi - start` overflow both resolve without panicking. Single-sourced so `run`/`runvm` fault
/// identically (the differential harness classifies the body substring as `RangeTooLarge`).
pub fn build_range(start: i64, end: i64, inclusive: bool) -> Result<Vec<Value>, String> {
    let hi = if inclusive {
        end
    } else {
        match end.checked_sub(1) {
            Some(h) => h,
            None => return Ok(Vec::new()), // exclusive `start..i64::MIN` — always empty
        }
    };
    if start > hi {
        return Ok(Vec::new());
    }
    let span = hi.checked_sub(start).ok_or("range too large")?;
    if span >= MAX_RANGE_LEN {
        return Err("range too large".to_string());
    }
    Ok((start..=hi).map(Value::Int).collect())
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
    fn build_map_dedups_first_position_last_value() {
        // PHP semantics: a duplicate key keeps its first position but takes the last value.
        let m = build_map(vec![
            (Value::Str("a".into()), Value::Int(1)),
            (Value::Str("b".into()), Value::Int(2)),
            (Value::Str("a".into()), Value::Int(9)),
        ])
        .unwrap();
        // `Value` isn't `PartialEq` (holds `f64`), so compare keys directly + values via `eq_val`.
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].0, HKey::Str("a".into())); // first position kept
        assert!(m[0].1.eq_val(&Value::Int(9))); // last value taken
        assert_eq!(m[1].0, HKey::Str("b".into()));
        assert!(m[1].1.eq_val(&Value::Int(2)));
    }

    #[test]
    fn build_map_rejects_non_hashable_key() {
        let e = build_map(vec![(Value::Float(1.0), Value::Int(1))]).unwrap_err();
        assert!(e.contains("invalid map key"), "{e}");
    }

    #[test]
    fn map_index_found_and_missing() {
        let m = vec![
            (HKey::Str("x".into()), Value::Int(10)),
            (HKey::Int(2), Value::Str("two".into())),
        ];
        assert!(map_index(&m, &Value::Str("x".into()))
            .unwrap()
            .eq_val(&Value::Int(10)));
        assert!(map_index(&m, &Value::Int(2))
            .unwrap()
            .eq_val(&Value::Str("two".into())));
        match map_index(&m, &Value::Str("missing".into())) {
            Err(e) => assert_eq!(e, "map key not found"),
            Ok(_) => panic!("expected missing-key fault"),
        }
    }

    #[test]
    fn hkey_value_round_trip() {
        for v in [Value::Int(7), Value::Bool(true), Value::Str("k".into())] {
            assert!(HKey::from_value(&v).unwrap().to_value().eq_val(&v));
        }
        assert!(HKey::from_value(&Value::Float(1.0)).is_none());
    }

    #[test]
    fn map_eq_is_order_independent() {
        let a = Value::Map(Rc::new(vec![
            (HKey::Str("a".into()), Value::Int(1)),
            (HKey::Str("b".into()), Value::Int(2)),
        ]));
        let b = Value::Map(Rc::new(vec![
            (HKey::Str("b".into()), Value::Int(2)),
            (HKey::Str("a".into()), Value::Int(1)),
        ]));
        let c = Value::Map(Rc::new(vec![(HKey::Str("a".into()), Value::Int(1))]));
        assert!(a.eq_val(&b)); // same entries, different order → equal
        assert!(!a.eq_val(&c)); // different key set → not equal
    }

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
        let inst = Value::Instance(Rc::new(Instance {
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
        assert!(Value::Null.eq_val(&Value::Null)); // null == null
        assert!(!Value::Null.eq_val(&Value::Int(0))); // null != a non-null value
        let a = Value::Enum(Rc::new(EnumVal {
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
        assert_eq!(Value::List(Rc::new(vec![])).type_name(), "list");
        assert_eq!(Value::Set(Rc::new(vec![])).type_name(), "set");
    }

    #[test]
    fn build_set_dedups_first_seen() {
        // First occurrence kept, later duplicates dropped, order preserved (M-RT S7b).
        let s = build_set(vec![
            Value::Int(3),
            Value::Int(1),
            Value::Int(3),
            Value::Int(2),
            Value::Int(1),
        ])
        .unwrap();
        assert_eq!(s, vec![HKey::Int(3), HKey::Int(1), HKey::Int(2)]);
        // a non-hashable element faults cleanly, never panics (EV-7).
        assert!(build_set(vec![Value::Float(1.0)]).is_err());
    }

    #[test]
    fn eq_val_sets_are_order_independent() {
        let a = Value::Set(Rc::new(vec![HKey::Int(1), HKey::Int(2), HKey::Int(3)]));
        let b = Value::Set(Rc::new(vec![HKey::Int(3), HKey::Int(1), HKey::Int(2)]));
        let c = Value::Set(Rc::new(vec![HKey::Int(1), HKey::Int(2)]));
        assert!(a.eq_val(&b)); // same membership, different order
        assert!(!a.eq_val(&c)); // different cardinality
    }
}
