use super::*;
use crate::types::Ty;
use crate::value::Value;

fn set_of(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(xs)] => {
            let s = crate::value::build_set((**xs).clone())?;
            Ok(Value::Set(std::rc::Rc::new(s)))
        }
        _ => Err("Set.of expects (List<T>)".into()),
    }
}
fn set_contains(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Set(s), elem] => {
            let hk = crate::value::HKey::from_value(elem)
                .ok_or_else(|| format!("invalid set element: {}", elem.type_name()))?;
            Ok(Value::Bool(s.contains(&hk)))
        }
        _ => Err("Set.contains expects (Set<T>, T)".into()),
    }
}
fn set_size(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Set(s)] => Ok(Value::Int(s.len() as i64)),
        _ => Err("Set.size expects (Set<T>)".into()),
    }
}
/// `union(a, b) -> Set<T>` — every element of `a` then every element of `b` not already present
/// (first-occurrence order, like `build_set`). Already deduped (both inputs are sets); no rebuild
/// needed. PHP `array_unique(array_merge(...))` keeps the same first-seen order.
fn set_union(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Set(a), Value::Set(b)] => {
            let mut out = (**a).clone();
            for k in b.iter() {
                if !out.contains(k) {
                    out.push(k.clone());
                }
            }
            Ok(Value::Set(std::rc::Rc::new(out)))
        }
        _ => Err("Set.union expects (Set<T>, Set<T>)".into()),
    }
}
/// `intersection(a, b) -> Set<T>` — elements of `a` that are also in `b`, in `a`'s order (PHP
/// `array_intersect`, which preserves the first array's order).
fn set_intersection(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Set(a), Value::Set(b)] => {
            let out: Vec<_> = a.iter().filter(|k| b.contains(k)).cloned().collect();
            Ok(Value::Set(std::rc::Rc::new(out)))
        }
        _ => Err("Set.intersection expects (Set<T>, Set<T>)".into()),
    }
}
/// `difference(a, b) -> Set<T>` — elements of `a` not in `b`, in `a`'s order (PHP `array_diff`).
fn set_difference(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Set(a), Value::Set(b)] => {
            let out: Vec<_> = a.iter().filter(|k| !b.contains(k)).cloned().collect();
            Ok(Value::Set(std::rc::Rc::new(out)))
        }
        _ => Err("Set.difference expects (Set<T>, Set<T>)".into()),
    }
}

/// `add(s, x) -> Set<T>` — a new set with `x` added (no-op if already present), first-occurrence
/// order preserved (≡ `union` with a singleton).
fn set_add(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Set(s), elem] => {
            let hk = crate::value::HKey::from_value(elem)
                .ok_or_else(|| format!("invalid set element: {}", elem.type_name()))?;
            let mut out = (**s).clone();
            if !out.contains(&hk) {
                out.push(hk);
            }
            Ok(Value::Set(std::rc::Rc::new(out)))
        }
        _ => Err("Set.add expects (Set<T>, T)".into()),
    }
}
/// `remove(s, x) -> Set<T>` — a new set without `x` (no-op if absent), order preserved.
fn set_remove(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Set(s), elem] => {
            let hk = crate::value::HKey::from_value(elem)
                .ok_or_else(|| format!("invalid set element: {}", elem.type_name()))?;
            let out: Vec<_> = s.iter().filter(|k| **k != hk).cloned().collect();
            Ok(Value::Set(std::rc::Rc::new(out)))
        }
        _ => Err("Set.remove expects (Set<T>, T)".into()),
    }
}
/// `isSubset(a, b) -> bool` — every element of `a` is in `b`.
fn set_is_subset(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Set(a), Value::Set(b)] => Ok(Value::Bool(a.iter().all(|k| b.contains(k)))),
        _ => Err("Set.isSubset expects (Set<T>, Set<T>)".into()),
    }
}

fn set_is_empty(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Set(s)] => Ok(Value::Bool(s.is_empty())),
        _ => Err("Set.isEmpty expects (Set<T>)".into()),
    }
}

/// `Set.toList(Set<T>) -> List<T>` — the elements as a list in insertion order (the set's stored order).
fn set_to_list(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Set(s)] => Ok(Value::List(std::rc::Rc::new(
            s.iter().map(crate::value::HKey::to_value).collect(),
        ))),
        _ => Err("Set.toList expects (Set<T>)".into()),
    }
}

/// The `Core.Set` registry entries (M-RT S7b). All generic over the element type `T`.
pub(crate) fn set_natives() -> Vec<NativeFn> {
    let t = || Ty::Param("T".into());
    vec![
        NativeFn {
            module: "Core.Set",
            name: "isEmpty",
            params: vec![Ty::Set(Box::new(t()))],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(set_is_empty),
            php: |a| format!("count({}) === 0", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Set",
            name: "toList",
            params: vec![Ty::Set(Box::new(t()))],
            ret: Ty::List(Box::new(t())),
            pure: true,
            eval: NativeEval::Pure(set_to_list),
            // The set is already a sequential array; array_values is a defensive re-index.
            php: |a| format!("array_values({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Set",
            name: "of",
            params: vec![Ty::List(Box::new(t()))],
            ret: Ty::Set(Box::new(t())),
            pure: true,
            eval: NativeEval::Pure(set_of),
            // Dedup preserving first-occurrence order; SORT_STRING matches HKey string-distinctness.
            php: |a| format!("array_values(array_unique({}, SORT_STRING))", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Set",
            name: "contains",
            params: vec![Ty::Set(Box::new(t())), t()],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(set_contains),
            // Strict in_array(needle, haystack) — needle first.
            php: |a| format!("in_array({}, {}, true)", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.Set",
            name: "size",
            params: vec![Ty::Set(Box::new(t()))],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(set_size),
            php: |a| format!("count({})", parg(a, 0)),
        },
        // Set algebra — each returns a new set; the result order follows the FIRST set (and, for
        // union, the second set's new elements after it). SORT_STRING in the union's `array_unique`
        // matches `HKey` string-distinctness (as `Set.of` does); `array_intersect`/`array_diff`
        // compare by string too, agreeing for a homogeneous `Set<T>`.
        NativeFn {
            module: "Core.Set",
            name: "union",
            params: vec![Ty::Set(Box::new(t())), Ty::Set(Box::new(t()))],
            ret: Ty::Set(Box::new(t())),
            pure: true,
            eval: NativeEval::Pure(set_union),
            php: |a| {
                format!(
                    "array_values(array_unique(array_merge({}, {}), SORT_STRING))",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.Set",
            name: "intersection",
            params: vec![Ty::Set(Box::new(t())), Ty::Set(Box::new(t()))],
            ret: Ty::Set(Box::new(t())),
            pure: true,
            eval: NativeEval::Pure(set_intersection),
            php: |a| {
                format!(
                    "array_values(array_intersect({}, {}))",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.Set",
            name: "difference",
            params: vec![Ty::Set(Box::new(t())), Ty::Set(Box::new(t()))],
            ret: Ty::Set(Box::new(t())),
            pure: true,
            eval: NativeEval::Pure(set_difference),
            php: |a| format!("array_values(array_diff({}, {}))", parg(a, 0), parg(a, 1)),
        },
        // `add` / `remove` return a new set (sets are immutable); `isSubset` is a containment test.
        NativeFn {
            module: "Core.Set",
            name: "add",
            params: vec![Ty::Set(Box::new(t())), t()],
            ret: Ty::Set(Box::new(t())),
            pure: true,
            eval: NativeEval::Pure(set_add),
            // ≡ union with a singleton: dedup-merge keeping first-occurrence order (SORT_STRING ≡ HKey).
            php: |a| {
                format!(
                    "array_values(array_unique(array_merge({}, [{}]), SORT_STRING))",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.Set",
            name: "remove",
            params: vec![Ty::Set(Box::new(t())), t()],
            ret: Ty::Set(Box::new(t())),
            pure: true,
            eval: NativeEval::Pure(set_remove),
            php: |a| {
                format!(
                    "array_values(array_filter({}, fn($e) => $e !== {}))",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.Set",
            name: "isSubset",
            params: vec![Ty::Set(Box::new(t())), Ty::Set(Box::new(t()))],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(set_is_subset),
            // a ⊆ b ⇔ a has no element absent from b.
            php: |a| format!("count(array_diff({}, {})) === 0", parg(a, 0), parg(a, 1)),
        },
    ]
}

#[cfg(test)]
#[path = "set_tests.rs"]
mod tests;
