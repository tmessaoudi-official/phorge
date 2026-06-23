use super::*;
use crate::types::Ty;
use crate::value::Value;

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
            eval: NativeEval::Pure(list_reverse),
            // array_reverse re-indexes a list (sequential keys) — byte-identical to the Rust Vec.
            php: |a| format!("array_reverse({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.List",
            name: "length",
            params: vec![Ty::List(Box::new(t()))],
            ret: Ty::Int,
            eval: NativeEval::Pure(list_length),
            php: |a| format!("count({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.List",
            name: "sum",
            params: vec![Ty::List(Box::new(Ty::Int))],
            ret: Ty::Int,
            eval: NativeEval::Pure(list_sum),
            php: |a| format!("array_sum({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.List",
            name: "map",
            params: vec![list(t()), Ty::Function(vec![t()], Box::new(u()))],
            ret: list(u()),
            eval: NativeEval::HigherOrder(list_map),
            // array_map(callable, array) — note the order is swapped vs Phorge's map(list, f).
            php: |a| format!("array_map({}, {})", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.List",
            name: "filter",
            params: vec![list(t()), Ty::Function(vec![t()], Box::new(Ty::Bool))],
            ret: list(t()),
            eval: NativeEval::HigherOrder(list_filter),
            // array_filter preserves original keys; array_values re-indexes to a sequential list.
            php: |a| format!("array_values(array_filter({}, {}))", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.List",
            name: "reduce",
            params: vec![list(t()), u(), Ty::Function(vec![u(), t()], Box::new(u()))],
            ret: u(),
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
