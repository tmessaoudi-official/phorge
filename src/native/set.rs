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
    ]
}

#[cfg(test)]
#[path = "set_tests.rs"]
mod tests;
