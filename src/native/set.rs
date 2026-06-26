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

/// The `Core.Set` registry entries (M-RT S7b). All generic over the element type `T`.
pub(crate) fn set_natives() -> Vec<NativeFn> {
    let t = || Ty::Param("T".into());
    vec![
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
    ]
}

#[cfg(test)]
#[path = "set_tests.rs"]
mod tests;
