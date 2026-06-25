use super::*;
use crate::types::Ty;
use crate::value::Value;

fn text_len(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Int(s.len() as i64)),
        _ => Err("Text.len expects (string)".into()),
    }
}
fn text_upper(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Str(s.to_ascii_uppercase())),
        _ => Err("Text.upper expects (string)".into()),
    }
}
fn text_lower(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Str(s.to_ascii_lowercase())),
        _ => Err("Text.lower expects (string)".into()),
    }
}
fn text_trim(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Str(s.trim().to_string())),
        _ => Err("Text.trim expects (string)".into()),
    }
}
fn text_contains(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s), Value::Str(sub)] => Ok(Value::Bool(s.contains(sub.as_str()))),
        _ => Err("Text.contains expects (string, string)".into()),
    }
}
fn text_split(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s), Value::Str(sep)] => {
            let parts: Vec<Value> = s
                .split(sep.as_str())
                .map(|p| Value::Str(p.into()))
                .collect();
            Ok(Value::List(std::rc::Rc::new(parts)))
        }
        _ => Err("Text.split expects (string, string)".into()),
    }
}
fn text_split_once(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Split on the FIRST occurrence → `[head, tail]`; `[whole]` (1 elem) if `sep` is absent.
        // Matches PHP `explode($sep, $s, 2)` exactly for a non-empty separator (the only use).
        [Value::Str(s), Value::Str(sep)] => {
            let parts: Vec<Value> = match s.split_once(sep.as_str()) {
                Some((head, tail)) => vec![Value::Str(head.into()), Value::Str(tail.into())],
                None => vec![Value::Str(s.clone())],
            };
            Ok(Value::List(std::rc::Rc::new(parts)))
        }
        _ => Err("Text.split_once expects (string, string)".into()),
    }
}
fn text_join(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(items), Value::Str(sep)] => {
            let mut parts: Vec<String> = Vec::with_capacity(items.len());
            for it in items.iter() {
                match it {
                    Value::Str(s) => parts.push(s.clone()),
                    other => {
                        return Err(format!(
                            "Text.join expects List<string>, found element of type {}",
                            other.type_name()
                        ))
                    }
                }
            }
            Ok(Value::Str(parts.join(sep)))
        }
        _ => Err("Text.join expects (List<string>, string)".into()),
    }
}
fn text_replace(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s), Value::Str(from), Value::Str(to)] => {
            Ok(Value::Str(s.replace(from.as_str(), to.as_str())))
        }
        _ => Err("Text.replace expects (string, string, string)".into()),
    }
}
fn text_starts_with(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s), Value::Str(pre)] => Ok(Value::Bool(s.starts_with(pre.as_str()))),
        _ => Err("Text.startsWith expects (string, string)".into()),
    }
}
fn text_ends_with(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s), Value::Str(suf)] => Ok(Value::Bool(s.ends_with(suf.as_str()))),
        _ => Err("Text.endsWith expects (string, string)".into()),
    }
}
fn text_repeat(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // PHP `str_repeat` requires count >= 0 (a `ValueError` otherwise); a negative count faults
        // cleanly here (EV-7 — never panic; `n as usize` on a negative i64 would be a huge alloc).
        [Value::Str(s), Value::Int(n)] => {
            if *n < 0 {
                return Err("Text.repeat count must be >= 0".into());
            }
            Ok(Value::Str(s.repeat(*n as usize)))
        }
        _ => Err("Text.repeat expects (string, int)".into()),
    }
}

/// The `Core.Text` registry entries (M3 Track B Wave 2). NOTE the PHP arg order: `explode`/`implode`
/// take the separator first, and `str_replace` is `(search, replace, subject)` — the `php` closures
/// reorder accordingly so the erasure matches Phorge's `(subject, …)` argument order.
pub(crate) fn text_natives() -> Vec<NativeFn> {
    let s = || Ty::String;
    vec![
        NativeFn {
            module: "Core.Text",
            name: "len",
            params: vec![s()],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(text_len),
            php: |a| format!("strlen({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "upper",
            params: vec![s()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(text_upper),
            php: |a| format!("strtoupper({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "lower",
            params: vec![s()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(text_lower),
            php: |a| format!("strtolower({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "trim",
            params: vec![s()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(text_trim),
            php: |a| format!("trim({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "contains",
            params: vec![s(), s()],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(text_contains),
            php: |a| format!("str_contains({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Text",
            name: "split",
            params: vec![s(), s()],
            ret: Ty::List(Box::new(Ty::String)),
            pure: true,
            eval: NativeEval::Pure(text_split),
            // PHP `explode(separator, string)` — separator first.
            php: |a| format!("explode({}, {})", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "splitOnce",
            params: vec![s(), s()],
            ret: Ty::List(Box::new(Ty::String)),
            pure: true,
            eval: NativeEval::Pure(text_split_once),
            // PHP `explode(separator, string, 2)` — separator first; the limit-2 yields [head, tail].
            php: |a| format!("explode({}, {}, 2)", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "join",
            params: vec![Ty::List(Box::new(Ty::String)), s()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(text_join),
            // PHP `implode(glue, array)` — glue first.
            php: |a| format!("implode({}, {})", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "replace",
            params: vec![s(), s(), s()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(text_replace),
            // PHP `str_replace(search, replace, subject)`.
            php: |a| {
                format!(
                    "str_replace({}, {}, {})",
                    parg(a, 1),
                    parg(a, 2),
                    parg(a, 0)
                )
            },
        },
        NativeFn {
            module: "Core.Text",
            name: "startsWith",
            params: vec![s(), s()],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(text_starts_with),
            php: |a| format!("str_starts_with({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Text",
            name: "endsWith",
            params: vec![s(), s()],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(text_ends_with),
            php: |a| format!("str_ends_with({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Text",
            name: "repeat",
            params: vec![s(), Ty::Int],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(text_repeat),
            php: |a| format!("str_repeat({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

// ---- Core.File ----------------------------------------------------------------------------------
// Filesystem natives (std::fs ↔ PHP file builtins, D-L9). `read` returns `string?` — `null` on any
// failure (missing file, permission, non-UTF-8) — exercising S2 null-safety (`??` / `if (var x =
// read(p))`). DETERMINISM: a file *read* is byte-identical across backends iff every backend reads
// the same bytes, so file examples read a **committed fixture**; `write` is a non-deterministic side
// effect and is excluded from the byte-identity-gated example set (it is unit-tested with a temp
// file). The run↔runvm spine shares the same `eval`, so it is always identical regardless.

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
