//! Runtime overload dispatch (M-RT method/function overloading).
//!
//! Phorj overloading is **dynamic multiple dispatch**: the runtime types of the arguments select
//! the most-specific matching overload. The selection runs identically in the tree-walking
//! interpreter and the stack VM — both feed the *same* [`ParamKind`]s (derived from the static
//! parameter types via [`param_kind`]) to the *same* [`select_overload`], so a call resolves to the
//! same body on both backends: byte-identical by construction. The PHP transpiler emits the
//! equivalent `is_*`/`instanceof` dispatcher, so real PHP agrees too.
//!
//! A parameter type that cannot be told apart from another at runtime (optional, union,
//! intersection, erased generic) collapses to [`ParamKind::Any`] — a deliberate MVP limitation
//! (overloads are expected to differ in concrete, runtime-distinguishable parameter types).

use crate::ast::Type;
use crate::value::Value;
use std::collections::BTreeMap;

/// A runtime-checkable summary of a parameter's static type — enough to test an argument *value* at
/// a call site without any static-type information at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamKind {
    Int,
    Float,
    Bool,
    Str,
    Bytes,
    List,
    Map,
    Set,
    Fn,
    /// A class, interface, or enum type by name. Matches an instance of that class (or a subtype via
    /// the `class_implements` oracle), or an enum value of that exact type.
    Named(String),
    /// Any value — the fallback for types not distinguished at runtime (optional/union/intersection/
    /// erased). Matches everything; least specific.
    Any,
}

/// One overload set as the VM stores it: each candidate's parameter kinds paired with the compiled
/// function index to call when that candidate is selected. [`select_overload`] returns the position
/// in this list; the caller calls `set[pos].1`.
pub type OverloadSet = Vec<(Vec<ParamKind>, usize)>;

/// Derive the runtime [`ParamKind`] of a parameter from its static type.
pub fn param_kind(ty: &Type) -> ParamKind {
    match ty {
        Type::Named { name, .. } => match name.as_str() {
            "int" => ParamKind::Int,
            "float" => ParamKind::Float,
            "bool" => ParamKind::Bool,
            "string" => ParamKind::Str,
            "bytes" => ParamKind::Bytes,
            "List" => ParamKind::List,
            "Map" => ParamKind::Map,
            "Set" => ParamKind::Set,
            other => ParamKind::Named(other.to_string()),
        },
        Type::Function { .. } => ParamKind::Fn,
        _ => ParamKind::Any,
    }
}

/// Concrete class `a` is a subtype of `b`: equal, or `a` implements/extends interface `b`
/// (transitively) — read from the flattened `class_implements` map both backends already hold.
fn is_subtype(a: &str, b: &str, oracle: &BTreeMap<String, Vec<String>>) -> bool {
    a == b || oracle.get(a).is_some_and(|v| v.iter().any(|i| i == b))
}

/// Whether argument value `v` matches parameter kind `k`.
fn kind_matches(k: &ParamKind, v: &Value, oracle: &BTreeMap<String, Vec<String>>) -> bool {
    match (k, v) {
        (ParamKind::Any, _) => true,
        (ParamKind::Int, Value::Int(_)) => true,
        (ParamKind::Float, Value::Float(_)) => true,
        (ParamKind::Bool, Value::Bool(_)) => true,
        (ParamKind::Str, Value::Str(_)) => true,
        (ParamKind::Bytes, Value::Bytes(_)) => true,
        (ParamKind::List, Value::List(_)) => true,
        (ParamKind::Map, Value::Map(_)) => true,
        (ParamKind::Set, Value::Set(_)) => true,
        (ParamKind::Fn, Value::Closure(_)) => true,
        (ParamKind::Named(n), Value::Instance(inst)) => is_subtype(&inst.class, n, oracle),
        (ParamKind::Named(n), Value::Enum(e)) => &e.ty == n,
        _ => false,
    }
}

/// Whether `a` is at least as specific as `b` at one parameter position: equal, a class subtype (for
/// `Named`), or `b` is the catch-all `Any`.
fn at_least_as_specific(
    a: &ParamKind,
    b: &ParamKind,
    oracle: &BTreeMap<String, Vec<String>>,
) -> bool {
    match (a, b) {
        _ if a == b => true,
        (_, ParamKind::Any) => true,
        (ParamKind::Named(x), ParamKind::Named(y)) => is_subtype(x, y, oracle),
        _ => false,
    }
}

/// Whether overload `a` is strictly more specific than `b` (at least as specific in every position,
/// and strictly more in at least one). Used by the transpiler to order an overload set's dispatch
/// branches most-specific-first, so the emitted PHP `if`-chain picks the same body the backends'
/// `select_overload` does for a resolvable call.
pub fn dominates(a: &[ParamKind], b: &[ParamKind], oracle: &BTreeMap<String, Vec<String>>) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| at_least_as_specific(x, y, oracle))
        && a.iter()
            .zip(b)
            .any(|(x, y)| x != y && at_least_as_specific(x, y, oracle))
}

/// Why an overloaded call could not resolve to a single body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectErr {
    /// No overload's parameter kinds match the arguments (the checker rejects this for a well-typed
    /// call; the backends fault defensively + identically).
    NoMatch,
    /// Two or more overloads match and none is strictly most-specific (cross-cutting multi-argument
    /// ambiguity). A clean, byte-identical runtime fault.
    Ambiguous,
}

/// Select the unique most-specific overload whose parameter kinds all match `args`, returning its
/// index into `candidates`. Most-specific = a matching candidate at least as specific as every other
/// matching candidate in every position; if exactly one such dominator exists it wins, else the call
/// is [`SelectErr::Ambiguous`]. Identical-signature candidates cannot occur (`E-OVERLOAD-DUPLICATE`).
pub fn select_overload(
    candidates: &[Vec<ParamKind>],
    args: &[Value],
    oracle: &BTreeMap<String, Vec<String>>,
) -> Result<usize, SelectErr> {
    let matching: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, ks)| {
            ks.len() == args.len() && ks.iter().zip(args).all(|(k, v)| kind_matches(k, v, oracle))
        })
        .map(|(i, _)| i)
        .collect();
    match matching.len() {
        0 => return Err(SelectErr::NoMatch),
        1 => return Ok(matching[0]),
        _ => {}
    }
    let dominators: Vec<usize> = matching
        .iter()
        .copied()
        .filter(|&i| {
            matching.iter().all(|&j| {
                candidates[i]
                    .iter()
                    .zip(&candidates[j])
                    .all(|(a, b)| at_least_as_specific(a, b, oracle))
            })
        })
        .collect();
    if dominators.len() == 1 {
        Ok(dominators[0])
    } else {
        Err(SelectErr::Ambiguous)
    }
}
