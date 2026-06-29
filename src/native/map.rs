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
/// `get(Map<K, V>, K) -> V?` — a *safe* lookup: the value when present, else `null`. Unlike `m[k]`
/// (which faults on a missing key), `get` surfaces absence as an optional, composing with `??`/if-let.
/// `V` is non-optional so a present value is never `null` — `null` unambiguously means "absent".
fn map_get(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Map(m), key] => {
            let hk = crate::value::HKey::from_value(key)
                .ok_or_else(|| format!("invalid map key: {}", key.type_name()))?;
            Ok(m.iter()
                .find(|(k, _)| *k == hk)
                .map_or(Value::Null, |(_, v)| v.clone()))
        }
        _ => Err("Map.get expects (Map<K, V>, K)".into()),
    }
}
/// `set(Map<K, V>, K, V) -> Map<K, V>` — a NEW map with `key` mapped to `v` (Phorj maps are
/// immutable; this is a functional update, COW). Insertion-ordered like PHP `$m[$k] = $v`: an existing
/// key keeps its position and takes the new value, a fresh key appends. Reuses the `value::map_set`
/// kernel on a clone, so it matches the M-mut element-set semantics.
fn map_set_native(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Map(m), key, v] => {
            let mut out = (**m).clone();
            crate::value::map_set(&mut out, key, v.clone())?;
            Ok(Value::Map(std::rc::Rc::new(out)))
        }
        _ => Err("Map.set expects (Map<K, V>, K, V)".into()),
    }
}
/// `remove(Map<K, V>, K) -> Map<K, V>` — a NEW map without `key` (functional, COW). Removing an absent
/// key is a no-op (returns an equal map), matching PHP `unset($m[$k])`. Surviving keys keep their order.
fn map_remove(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Map(m), key] => {
            let hk = crate::value::HKey::from_value(key)
                .ok_or_else(|| format!("invalid map key: {}", key.type_name()))?;
            let out: Vec<_> = m.iter().filter(|(k, _)| *k != hk).cloned().collect();
            Ok(Value::Map(std::rc::Rc::new(out)))
        }
        _ => Err("Map.remove expects (Map<K, V>, K)".into()),
    }
}

/// `getOr(Map<K,V>, K, V) -> V` — the value at `key`, or `default` if the key is absent. Unlike
/// `get` (`-> V?`), this never returns null for a *present* `key` whose value is null.
fn map_get_or(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Map(m), key, default] => {
            let hk = crate::value::HKey::from_value(key)
                .ok_or_else(|| format!("invalid map key: {}", key.type_name()))?;
            Ok(m.iter()
                .find(|(k, _)| *k == hk)
                .map_or_else(|| default.clone(), |(_, v)| v.clone()))
        }
        _ => Err("Map.getOr expects (Map<K, V>, K, V)".into()),
    }
}
/// `merge(a, b) -> Map<K,V>` — `a`'s entries with `b`'s merged in: a shared key keeps `a`'s position
/// but takes `b`'s value, `b`'s new keys append (≡ PHP `array_merge`, ≡ `build_map` over `a ++ b`).
fn map_merge(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Map(a), Value::Map(b)] => {
            let mut out = (**a).clone();
            for (bk, bv) in b.iter() {
                if let Some(slot) = out.iter_mut().find(|(k, _)| k == bk) {
                    slot.1 = bv.clone();
                } else {
                    out.push((bk.clone(), bv.clone()));
                }
            }
            Ok(Value::Map(std::rc::Rc::new(out)))
        }
        _ => Err("Map.merge expects (Map<K, V>, Map<K, V>)".into()),
    }
}
/// `map(Map<K,V>, (V) -> W) -> Map<K,W>` — transform each VALUE, keys preserved (≡ PHP `array_map`
/// over a single assoc array). Higher-order.
fn map_map(args: &[Value], call: &mut ClosureInvoker) -> Result<Value, String> {
    match args {
        [Value::Map(m), f] => {
            let mut out = Vec::with_capacity(m.len());
            for (k, v) in m.iter() {
                out.push((k.clone(), call(f, vec![v.clone()])?));
            }
            Ok(Value::Map(std::rc::Rc::new(out)))
        }
        _ => Err("Map.map expects (Map<K, V>, (V) -> W)".into()),
    }
}
/// `filter(Map<K,V>, (V) -> bool) -> Map<K,V>` — keep entries whose VALUE passes (≡ PHP `array_filter`,
/// default mode, keys preserved). Higher-order.
fn map_filter(args: &[Value], call: &mut ClosureInvoker) -> Result<Value, String> {
    match args {
        [Value::Map(m), f] => {
            let mut out = Vec::new();
            for (k, v) in m.iter() {
                match call(f, vec![v.clone()])? {
                    Value::Bool(true) => out.push((k.clone(), v.clone())),
                    Value::Bool(false) => {}
                    other => {
                        return Err(format!(
                            "Map.filter predicate must return bool, got {}",
                            other.type_name()
                        ))
                    }
                }
            }
            Ok(Value::Map(std::rc::Rc::new(out)))
        }
        _ => Err("Map.filter expects (Map<K, V>, (V) -> bool)".into()),
    }
}

/// The `Core.Map` registry entries (M-RT S7b). All generic over `K`/`V`; each erases to a PHP array
/// builtin (D-L9). NOTE the PHP arg order for `has`: `array_key_exists(key, array)` — key first.
fn map_is_empty(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Map(m)] => Ok(Value::Bool(m.is_empty())),
        _ => Err("Map.isEmpty expects (Map<K, V>)".into()),
    }
}

pub(crate) fn map_natives() -> Vec<NativeFn> {
    let k = || Ty::Param("K".into());
    let v = || Ty::Param("V".into());
    let w = || Ty::Param("W".into());
    let map = || Ty::Map(Box::new(k()), Box::new(v()));
    vec![
        NativeFn {
            module: "Core.Map",
            name: "isEmpty",
            params: vec![map()],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(map_is_empty),
            php: |a| format!("count({}) === 0", parg(a, 0)),
        },
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
        NativeFn {
            module: "Core.Map",
            name: "get",
            params: vec![map(), k()],
            ret: Ty::Optional(Box::new(v())),
            pure: true,
            eval: NativeEval::Pure(map_get),
            // `V` is non-optional, so a present value is never null → `?? null` means "absent".
            php: |a| format!("({}[{}] ?? null)", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Map",
            name: "set",
            params: vec![map(), k(), v()],
            ret: map(),
            pure: true,
            eval: NativeEval::Pure(map_set_native),
            // Gated `__phorj_map_set($m, $k, $v)` — a copy-then-assign (PHP arrays are COW value
            // types, so `$m` inside the helper is already a copy → a new map, caller untouched).
            php: |a| {
                format!(
                    "__phorj_map_set({}, {}, {})",
                    parg(a, 0),
                    parg(a, 1),
                    parg(a, 2)
                )
            },
        },
        NativeFn {
            module: "Core.Map",
            name: "remove",
            params: vec![map(), k()],
            ret: map(),
            pure: true,
            eval: NativeEval::Pure(map_remove),
            php: |a| format!("__phorj_map_remove({}, {})", parg(a, 0), parg(a, 1)),
        },
        // `getOr` — safe access with a fallback (never faults / returns the default for an absent key).
        NativeFn {
            module: "Core.Map",
            name: "getOr",
            params: vec![map(), k(), v()],
            ret: v(),
            pure: true,
            eval: NativeEval::Pure(map_get_or),
            // array_key_exists (not `??`) so a present key with a null value returns that null.
            php: |a| {
                format!(
                    "(array_key_exists({1}, {0}) ? {0}[{1}] : {2})",
                    parg(a, 0),
                    parg(a, 1),
                    parg(a, 2)
                )
            },
        },
        // `merge` — a new map; a shared key takes the SECOND map's value at the first's position.
        NativeFn {
            module: "Core.Map",
            name: "merge",
            params: vec![map(), map()],
            ret: map(),
            pure: true,
            eval: NativeEval::Pure(map_merge),
            php: |a| format!("array_merge({}, {})", parg(a, 0), parg(a, 1)),
        },
        // `map` / `filter` over VALUES (keys preserved) — higher-order, like the `Core.List` versions.
        NativeFn {
            module: "Core.Map",
            name: "map",
            params: vec![map(), Ty::Function(vec![v()], Box::new(w()))],
            ret: Ty::Map(Box::new(k()), Box::new(w())),
            pure: true,
            eval: NativeEval::HigherOrder(map_map),
            // array_map(callable, array) over a single assoc array preserves keys.
            php: |a| format!("array_map({}, {})", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.Map",
            name: "filter",
            params: vec![map(), Ty::Function(vec![v()], Box::new(Ty::Bool))],
            ret: map(),
            pure: true,
            eval: NativeEval::HigherOrder(map_filter),
            // array_filter(array, callback) default mode: callback gets the value, keys preserved.
            php: |a| format!("array_filter({}, {})", parg(a, 0), parg(a, 1)),
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
