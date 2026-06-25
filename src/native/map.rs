use super::*;
use crate::types::Ty;
use crate::value::Value;

fn map_keys(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Map(m)] => Ok(Value::List(std::rc::Rc::new(
            m.iter().map(|(k, _)| k.to_value()).collect(),
        ))),
        _ => Err("Map.keys expects (Map<K, V>)".into()),
    }
}
fn map_values(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Map(m)] => Ok(Value::List(std::rc::Rc::new(
            m.iter().map(|(_, v)| v.clone()).collect(),
        ))),
        _ => Err("Map.values expects (Map<K, V>)".into()),
    }
}
fn map_has(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Map(m), key] => {
            let hk = crate::value::HKey::from_value(key)
                .ok_or_else(|| format!("invalid map key: {}", key.type_name()))?;
            Ok(Value::Bool(m.iter().any(|(k, _)| *k == hk)))
        }
        _ => Err("Map.has expects (Map<K, V>, K)".into()),
    }
}
fn map_size(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Map(m)] => Ok(Value::Int(m.len() as i64)),
        _ => Err("Map.size expects (Map<K, V>)".into()),
    }
}

/// The `Core.Map` registry entries (M-RT S7b). All generic over `K`/`V`; each erases to a PHP array
/// builtin (D-L9). NOTE the PHP arg order for `has`: `array_key_exists(key, array)` — key first.
pub(crate) fn map_natives() -> Vec<NativeFn> {
    let k = || Ty::Param("K".into());
    let v = || Ty::Param("V".into());
    let map = || Ty::Map(Box::new(k()), Box::new(v()));
    vec![
        NativeFn {
            module: "Core.Map",
            name: "keys",
            params: vec![map()],
            ret: Ty::List(Box::new(k())),
            pure: true,
            eval: NativeEval::Pure(map_keys),
            php: |a| format!("array_keys({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Map",
            name: "values",
            params: vec![map()],
            ret: Ty::List(Box::new(v())),
            pure: true,
            eval: NativeEval::Pure(map_values),
            php: |a| format!("array_values({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Map",
            name: "has",
            params: vec![map(), k()],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(map_has),
            // PHP `array_key_exists(key, array)` — key first.
            php: |a| format!("array_key_exists({}, {})", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.Map",
            name: "size",
            params: vec![map()],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(map_size),
            php: |a| format!("count({})", parg(a, 0)),
        },
    ]
}

// ---- Core.Set -----------------------------------------------------------------------------------
// Set natives, all generic over the element type. A `Value::Set` is an insertion-ordered, deduped
// `Rc<Vec<HKey>>` (the Map discipline — risk R1), built only via `value::build_set`. PHP represents a
// set as a plain deduped list, so `of` erases to `array_values(array_unique($xs, SORT_STRING))`
// (SORT_STRING matches `HKey` string-distinctness for a homogeneous `Set<T>` — SORT_REGULAR would
// loosely collapse e.g. "1"/"01"), `contains` to a strict `in_array`, `size` to `count`. Element type
// is the hashable subset (`int`/`bool`/`string`); a `float`/composite element is `E-MAP-KEY` at the
// type level, and a stray one faults cleanly at runtime (EV-7).

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
