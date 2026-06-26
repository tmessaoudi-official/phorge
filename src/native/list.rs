use super::*;
use crate::types::Ty;
use crate::value::Value;
use std::cmp::Ordering;

/// Natural total order over the scalar element types, matching the PHP `__phorge_sort` comparator
/// byte-for-byte: ints/floats/bools numerically (Rust `cmp`/`total_cmp` ≡ PHP `<=>`), strings
/// lexicographically by byte (Rust `String` Ord ≡ PHP `strcmp` — NOT PHP's numeric-string-juggling
/// `<=>`). A homogeneous typed list never mixes arms; a stray mix is treated as equal (total, no panic).
fn natural_cmp(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x.total_cmp(y),
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => Ordering::Equal,
    }
}

/// `List.sort(List<T>) -> List<T>` — a new list in natural ascending order. Rust `sort_by` is stable
/// (≡ PHP 8.0+ `usort`); returns a fresh list (Phorge lists are immutable).
fn list_sort(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(xs)] => {
            let mut ys = (**xs).clone();
            ys.sort_by(natural_cmp);
            Ok(Value::List(std::rc::Rc::new(ys)))
        }
        _ => Err("List.sort expects (List<T>)".into()),
    }
}

/// `List.sortWith(List<T>, (T, T) -> int) -> List<T>` — a new list ordered by the comparator (negative
/// ⇒ a before b, like PHP `usort`). The comparator runs on the calling backend via the re-entrant
/// invoker; a fault (or a non-int result) is captured and propagated rather than panicking the sort.
fn list_sort_with(args: &[Value], call: &mut ClosureInvoker) -> Result<Value, String> {
    match args {
        [Value::List(xs), f] => {
            let mut ys = (**xs).clone();
            let mut err: Option<String> = None;
            ys.sort_by(|a, b| {
                if err.is_some() {
                    return Ordering::Equal;
                }
                match call(f, vec![a.clone(), b.clone()]) {
                    Ok(Value::Int(n)) => n.cmp(&0),
                    Ok(_) => {
                        err = Some("List.sortWith comparator must return int".into());
                        Ordering::Equal
                    }
                    Err(e) => {
                        err = Some(e);
                        Ordering::Equal
                    }
                }
            });
            match err {
                Some(e) => Err(e),
                None => Ok(Value::List(std::rc::Rc::new(ys))),
            }
        }
        _ => Err("List.sortWith expects (List<T>, (T, T) -> int)".into()),
    }
}

fn list_reverse(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(xs)] => {
            let mut v = (**xs).clone();
            v.reverse();
            Ok(Value::List(std::rc::Rc::new(v)))
        }
        _ => Err("List.reverse expects (List<T>)".into()),
    }
}
fn list_length(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Generic over the element type — the count of any list, byte-identical to PHP `count`.
        [Value::List(xs)] => Ok(Value::Int(xs.len() as i64)),
        _ => Err("List.length expects (List<T>)".into()),
    }
}
fn list_contains(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(xs), needle] => Ok(Value::Bool(xs.iter().any(|x| x.eq_val(needle)))),
        _ => Err("List.contains expects (List<T>, T)".into()),
    }
}
/// `slice(List<T>, int, int) -> List<T>` — a sub-list, mirroring PHP `array_slice($xs, offset, len)`
/// EXACTLY (so the erasure is the bare builtin): a negative `offset`/`len` counts from the end, an
/// out-of-range slice clamps to empty. Returns a fresh (re-indexed) list.
fn list_slice(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(xs), Value::Int(offset), Value::Int(length)] => {
            let n = xs.len() as i64;
            // PHP `array_slice` offset/length normalization, replicated for byte-identity.
            let start = if *offset < 0 {
                (n + *offset).max(0)
            } else {
                (*offset).min(n)
            };
            let end = if *length < 0 {
                (n + *length).max(start)
            } else {
                (start + *length).min(n)
            };
            let out: Vec<Value> = xs[start as usize..end as usize].to_vec();
            Ok(Value::List(std::rc::Rc::new(out)))
        }
        _ => Err("List.slice expects (List<T>, int, int)".into()),
    }
}
/// `indexOf(List<T>, T) -> int?` — the index of the first element equal to the needle (structural
/// `eq_val`, like `contains`), else `null`. Erases to a gated `__phorge_index_of` (PHP `array_search`
/// returns `false` on miss, mapped to `null`).
fn list_index_of(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(xs), needle] => Ok(xs
            .iter()
            .position(|x| x.eq_val(needle))
            .map_or(Value::Null, |i| Value::Int(i as i64))),
        _ => Err("List.indexOf expects (List<T>, T)".into()),
    }
}
/// `concat(List<T>, List<T>) -> List<T>` — the two lists joined (PHP `array_merge`, which re-indexes
/// sequential lists). A fresh list; both inputs are untouched (immutability).
fn list_concat(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(a), Value::List(b)] => {
            let mut out = (**a).clone();
            out.extend(b.iter().cloned());
            Ok(Value::List(std::rc::Rc::new(out)))
        }
        _ => Err("List.concat expects (List<T>, List<T>)".into()),
    }
}
/// `first(List<T>) -> T?` / `last(List<T>) -> T?` — the first/last element, or `null` for an empty
/// list. Erase inline to `($xs[0] ?? null)` / `($xs[count($xs) - 1] ?? null)`.
fn list_first(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(xs)] => Ok(xs.first().cloned().unwrap_or(Value::Null)),
        _ => Err("List.first expects (List<T>)".into()),
    }
}
fn list_last(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(xs)] => Ok(xs.last().cloned().unwrap_or(Value::Null)),
        _ => Err("List.last expects (List<T>)".into()),
    }
}

fn list_sum(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(xs)] => {
            let mut acc: i64 = 0;
            for x in xs.iter() {
                match x {
                    // Checked: an overflowing sum faults cleanly (EV-7), like the int arithmetic
                    // kernels. PHP `array_sum` would instead promote to float on overflow — examples
                    // stay well within i64 range (caveat in KNOWN_ISSUES).
                    Value::Int(n) => {
                        acc = acc
                            .checked_add(*n)
                            .ok_or_else(|| "integer overflow in List.sum".to_string())?;
                    }
                    other => {
                        return Err(format!(
                            "List.sum expects List<int>, found element of type {}",
                            other.type_name()
                        ))
                    }
                }
            }
            Ok(Value::Int(acc))
        }
        _ => Err("List.sum expects (List<int>)".into()),
    }
}

// The higher-order `Core.List` ops (M-RT S7b-3). Each takes a `Value::Closure` argument and calls it
// once per element via the backend-supplied `call` invoker ([`ClosureInvoker`]) — so the one body
// runs on the interpreter *and* the VM (parity), and any fault the closure raises propagates as a
// plain `String` that both backends classify identically. The element type `T` (and `map`/`reduce`'s
// result type `U`) are inferred at the call site by the generic-native path; the registry's
// `Ty::Param` never reaches a backend (M-RT S7b). They erase to PHP's `array_map`/`array_filter`/
// `array_reduce` (D-L9). `filter` wraps `array_filter` in `array_values` to re-index the result to a
// sequential list (PHP's `array_filter` preserves the original keys), matching the Rust `Vec`.

fn list_map(args: &[Value], call: &mut ClosureInvoker) -> Result<Value, String> {
    match args {
        [Value::List(xs), f] => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs.iter() {
                out.push(call(f, vec![x.clone()])?);
            }
            Ok(Value::List(std::rc::Rc::new(out)))
        }
        _ => Err("List.map expects (List<T>, (T) -> U)".into()),
    }
}
fn list_filter(args: &[Value], call: &mut ClosureInvoker) -> Result<Value, String> {
    match args {
        [Value::List(xs), f] => {
            let mut out = Vec::new();
            for x in xs.iter() {
                match call(f, vec![x.clone()])? {
                    Value::Bool(true) => out.push(x.clone()),
                    Value::Bool(false) => {}
                    other => {
                        return Err(format!(
                            "List.filter predicate must return bool, got {}",
                            other.type_name()
                        ))
                    }
                }
            }
            Ok(Value::List(std::rc::Rc::new(out)))
        }
        _ => Err("List.filter expects (List<T>, (T) -> bool)".into()),
    }
}
fn list_reduce(args: &[Value], call: &mut ClosureInvoker) -> Result<Value, String> {
    match args {
        [Value::List(xs), init, f] => {
            let mut acc = init.clone();
            for x in xs.iter() {
                acc = call(f, vec![acc, x.clone()])?;
            }
            Ok(acc)
        }
        _ => Err("List.reduce expects (List<T>, U, (U, T) -> U)".into()),
    }
}

/// The `Core.List` registry entries (M-RT S7b). `reverse` is generic over the element type; `sum` is
/// concrete `List<int> -> int`; `map`/`filter`/`reduce` are the higher-order ops (S7b-3). All erase
/// to the PHP array builtin of the same shape (D-L9).
pub(crate) fn list_natives() -> Vec<NativeFn> {
    let t = || Ty::Param("T".into());
    let u = || Ty::Param("U".into());
    let list = |e: Ty| Ty::List(Box::new(e));
    vec![
        NativeFn {
            module: "Core.List",
            name: "reverse",
            params: vec![Ty::List(Box::new(t()))],
            ret: Ty::List(Box::new(t())),
            pure: true,
            eval: NativeEval::Pure(list_reverse),
            // array_reverse re-indexes a list (sequential keys) — byte-identical to the Rust Vec.
            php: |a| format!("array_reverse({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.List",
            name: "length",
            params: vec![Ty::List(Box::new(t()))],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(list_length),
            php: |a| format!("count({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.List",
            name: "sum",
            params: vec![Ty::List(Box::new(Ty::Int))],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(list_sum),
            php: |a| format!("array_sum({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.List",
            name: "contains",
            params: vec![list(t()), t()],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(list_contains),
            // strict `in_array` (=== ) matches Phorge's value equality for scalars + nested
            // lists/maps; arg order is (needle, haystack) — the reverse of `contains(list, value)`.
            // (A list of class instances would differ: PHP `===` is identity, Phorge is structural —
            // KNOWN_ISSUES; scalar/collection element lists are byte-identical.)
            php: |a| format!("in_array({}, {}, true)", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.List",
            name: "map",
            params: vec![list(t()), Ty::Function(vec![t()], Box::new(u()))],
            ret: list(u()),
            pure: true,
            eval: NativeEval::HigherOrder(list_map),
            // array_map(callable, array) — note the order is swapped vs Phorge's map(list, f).
            php: |a| format!("array_map({}, {})", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.List",
            name: "filter",
            params: vec![list(t()), Ty::Function(vec![t()], Box::new(Ty::Bool))],
            ret: list(t()),
            pure: true,
            eval: NativeEval::HigherOrder(list_filter),
            // array_filter preserves original keys; array_values re-indexes to a sequential list.
            php: |a| format!("array_values(array_filter({}, {}))", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.List",
            name: "reduce",
            params: vec![list(t()), u(), Ty::Function(vec![u(), t()], Box::new(u()))],
            ret: u(),
            pure: true,
            eval: NativeEval::HigherOrder(list_reduce),
            // array_reduce(array, callback, initial) — initial is Phorge's 2nd arg, fn its 3rd.
            php: |a| {
                format!(
                    "array_reduce({}, {}, {})",
                    parg(a, 0),
                    parg(a, 2),
                    parg(a, 1)
                )
            },
        },
        // `sort(List<T>) -> List<T>` — natural ascending (PHP `sort`, but byte-stable + string-byte
        // order). Gated `__phorge_sort` helper (a `<=>`/`strcmp` type-dispatched `usort` over a copy).
        NativeFn {
            module: "Core.List",
            name: "sort",
            params: vec![list(t())],
            ret: list(t()),
            pure: true,
            eval: NativeEval::Pure(list_sort),
            php: |a| format!("__phorge_sort({})", parg(a, 0)),
        },
        // `sortWith(List<T>, (T, T) -> int) -> List<T>` — comparator (PHP `usort`), higher-order.
        NativeFn {
            module: "Core.List",
            name: "sortWith",
            params: vec![list(t()), Ty::Function(vec![t(), t()], Box::new(Ty::Int))],
            ret: list(t()),
            pure: true,
            eval: NativeEval::HigherOrder(list_sort_with),
            php: |a| format!("__phorge_sort_with({}, {})", parg(a, 0), parg(a, 1)),
        },
        // `slice(List<T>, int, int) -> List<T>` — PHP `array_slice` (offset, length; negatives count
        // from the end; out-of-range clamps to empty).
        NativeFn {
            module: "Core.List",
            name: "slice",
            params: vec![list(t()), Ty::Int, Ty::Int],
            ret: list(t()),
            pure: true,
            eval: NativeEval::Pure(list_slice),
            php: |a| {
                format!(
                    "array_slice({}, {}, {})",
                    parg(a, 0),
                    parg(a, 1),
                    parg(a, 2)
                )
            },
        },
        // `indexOf(List<T>, T) -> int?` — gated `__phorge_index_of` (PHP `array_search` strict → null).
        NativeFn {
            module: "Core.List",
            name: "indexOf",
            params: vec![list(t()), t()],
            ret: Ty::Optional(Box::new(Ty::Int)),
            pure: true,
            eval: NativeEval::Pure(list_index_of),
            php: |a| format!("__phorge_index_of({}, {})", parg(a, 0), parg(a, 1)),
        },
        // `concat(List<T>, List<T>) -> List<T>` — PHP `array_merge` (re-indexes sequential lists).
        NativeFn {
            module: "Core.List",
            name: "concat",
            params: vec![list(t()), list(t())],
            ret: list(t()),
            pure: true,
            eval: NativeEval::Pure(list_concat),
            php: |a| format!("array_merge({}, {})", parg(a, 0), parg(a, 1)),
        },
        // `first(List<T>) -> T?` / `last(List<T>) -> T?` — head/tail or null for an empty list.
        NativeFn {
            module: "Core.List",
            name: "first",
            params: vec![list(t())],
            ret: Ty::Optional(Box::new(t())),
            pure: true,
            eval: NativeEval::Pure(list_first),
            php: |a| format!("({}[0] ?? null)", parg(a, 0)),
        },
        NativeFn {
            module: "Core.List",
            name: "last",
            params: vec![list(t())],
            ret: Ty::Optional(Box::new(t())),
            pure: true,
            eval: NativeEval::Pure(list_last),
            php: |a| format!("({0}[count({0}) - 1] ?? null)", parg(a, 0)),
        },
    ]
}

// ---- Core.Map -----------------------------------------------------------------------------------
// Map query natives, all generic over the key/value types (`keys(Map<K,V>) -> List<K>`). They read
// the insertion-ordered `Value::Map` rep (a `Vec<(HKey, Value)>`, not a `HashMap` — risk R1), so
// `keys`/`values` are byte-identical with PHP's order-preserving `array_keys`/`array_values`. KEY
// COERCION CAVEAT (KNOWN_ISSUES): PHP arrays coerce integer-like string keys and bools to int keys,
// so a `keys()` over such a map renders differently under PHP than on the Rust backends; examples use
// plain (non-numeric) string keys, which PHP keeps verbatim. The run↔runvm spine is always identical.

#[cfg(test)]
#[path = "list_tests.rs"]
mod tests;
