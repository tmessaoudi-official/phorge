//! Namespaced native (built-in) function registry — the stdlib's runtime + type + transpile
//! surface, addressed by `(module, name)` (e.g. module `Core.Console`, name `println`). One entry
//! single-sources all four facets of a native, so the four backends cannot drift:
//!   * `params` / `ret` — the checker's signature for a call to this native;
//!   * `eval` — the runtime behavior, shared by the tree-walking interpreter *and* the VM (the
//!     structural parity guarantee, exactly like the value kernels: one impl, two callers);
//!   * `php` — the transpile-time PHP emission (a `core.*` native erases to PHP's flat builtins;
//!     the namespace is a compile-time organizing layer, decisions N-2/D-L9).
//!
//! The registry is the load-bearing target of `import Core.Console;` (M3 namespace reshape, Wave 1,
//! `docs/specs/2026-06-18-m3-namespace-system-design.md`). The former free global `println` is
//! retired in favor of `Core.Console.println`, and `Op::Print` in favor of
//! `Op::CallNative(index, argc)` indexing this table.

use crate::ast::Item;
use crate::types::Ty;
use crate::value::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One built-in function, addressed by `(module, name)`. See the module docs for the four facets.
pub struct NativeFn {
    /// Dotted module path the native lives under — e.g. `"Core.Console"`.
    pub module: &'static str,
    /// Bare function name — e.g. `"println"`.
    pub name: &'static str,
    /// Parameter types — the checker validates call arguments against these.
    pub params: Vec<Ty>,
    /// Return type.
    pub ret: Ty,
    /// Runtime behavior, shared by the interpreter and the VM (the structural parity guarantee —
    /// one body, two callers). See [`NativeEval`].
    pub eval: NativeEval,
    /// PHP emission: given the already-emitted PHP for each argument, return the PHP snippet this
    /// native erases to (decision N-2). For `Console.println`: `echo {a} . "\n"`.
    pub php: fn(&[String]) -> String,
}

/// A backend's re-entrant closure invoker, handed to a [`NativeEval::HigherOrder`] body: given a
/// `Value::Closure` and its call arguments, run it on the calling backend and return its result (or
/// a fault as a plain `String`, the backend-shared contract). The interpreter wraps `call_closure`;
/// the VM wraps `call_closure_value` (a nested `run_until` over the shared `exec_op`).
pub type ClosureInvoker<'a> = dyn FnMut(&Value, Vec<Value>) -> Result<Value, String> + 'a;

/// How a native computes its result (M-RT S7b-3). Most natives are [`Pure`](NativeEval::Pure): a
/// function of their argument values, threading the program output buffer so a side-effecting native
/// (`Console.println`) can append to it. A [`HigherOrder`](NativeEval::HigherOrder) native instead
/// needs to *call back* into the calling backend to invoke a `Value::Closure` argument
/// (`Core.List.map`/`filter`/`reduce`); the backend supplies the invoker so the one `eval` body
/// drives both the interpreter and the VM — exactly the parity discipline of the pure path. Both
/// variants are `fn` pointers, so the enum stays `Copy` (a `CallNative` dispatch reads it by value,
/// ending the registry borrow before the invoker captures the backend).
#[derive(Clone, Copy)]
pub enum NativeEval {
    /// `(args, out) -> result`. Arguments arrive in source order; `out` is the program's output
    /// buffer (ignored by pure natives, appended to by side-effecting ones).
    Pure(fn(&[Value], &mut String) -> Result<Value, String>),
    /// `(args, invoke) -> result`, where `invoke(closure, call_args)` executes a `Value::Closure`
    /// on the calling backend and returns its value. The native never touches the output buffer
    /// directly — any side effect happens inside the invoked closure.
    HigherOrder(fn(&[Value], &mut ClosureInvoker) -> Result<Value, String>),
}

/// Pinned registry slot for `Core.Console.println` — the migrated former `Op::Print`. The compiler
/// bakes `Op::CallNative(CONSOLE_PRINTLN, 1)`; [`build`] self-checks this slot so the constant can
/// never silently drift from the table.
pub const CONSOLE_PRINTLN: usize = 0;

/// `console.println(string)` — append the argument's display rendering plus a newline to the
/// program's output buffer. Shared verbatim by both backends (the former `interpreter::
/// builtin_println` / VM `Op::Print` body); the space-join over multiple args is dead generality
/// (the checker fixes the arity at one `string`) kept for a future variadic.
fn console_println(args: &[Value], out: &mut String) -> Result<Value, String> {
    let mut line = String::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            line.push(' ');
        }
        match a.as_display() {
            Some(t) => line.push_str(&t),
            None => return Err(format!("println cannot print {}", a.type_name())),
        }
    }
    out.push_str(&line);
    out.push('\n');
    Ok(Value::Unit)
}

/// Index helper for a native's PHP emission: the already-emitted PHP for argument `i`, or `""` if
/// absent (the checker guarantees arity before `php` is ever called). Keeps the `php` closures terse.
fn parg(args: &[String], i: usize) -> &str {
    args.get(i).map_or("", String::as_str)
}

// ---- Core.Math ----------------------------------------------------------------------------------
// Concrete-typed numeric natives (`Ty` has no type variable, so no overloading): the float ops
// `sqrt`/`pow`/`floor`/`ceil` are `float -> float`; `abs`/`min`/`max` are `int`. Each erases to the
// PHP builtin of the same name (D-L9). NOTE (KNOWN_ISSUES, float precision): an *irrational* result
// (`sqrt(2.0)`) renders with more digits on the Rust backends than PHP's default 14-sig-digit `echo`,
// so examples stay on exactly-representable values; the run↔runvm spine is unaffected (both Rust).

fn math_sqrt(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.sqrt())),
        _ => Err("Math.sqrt expects (float)".into()),
    }
}
fn math_pow(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(b), Value::Float(e)] => Ok(Value::Float(b.powf(*e))),
        _ => Err("Math.pow expects (float, float)".into()),
    }
}
fn math_floor(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.floor())),
        _ => Err("Math.floor expects (float)".into()),
    }
}
fn math_ceil(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.ceil())),
        _ => Err("Math.ceil expects (float)".into()),
    }
}
fn math_abs(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // `i64::MIN.abs()` overflows; a clean fault keeps EV-7 (never panic on input).
        [Value::Int(n)] => n
            .checked_abs()
            .map(Value::Int)
            .ok_or_else(|| "integer overflow in Math.abs".to_string()),
        _ => Err("Math.abs expects (int)".into()),
    }
}
fn math_min(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok(Value::Int((*a).min(*b))),
        _ => Err("Math.min expects (int, int)".into()),
    }
}
fn math_max(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok(Value::Int((*a).max(*b))),
        _ => Err("Math.max expects (int, int)".into()),
    }
}

/// The `Core.Math` registry entries (M3 Track B Wave 2).
fn math_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Math",
            name: "sqrt",
            params: vec![Ty::Float],
            ret: Ty::Float,
            eval: NativeEval::Pure(math_sqrt),
            php: |a| format!("sqrt({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "pow",
            params: vec![Ty::Float, Ty::Float],
            ret: Ty::Float,
            eval: NativeEval::Pure(math_pow),
            php: |a| format!("pow({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Math",
            name: "floor",
            params: vec![Ty::Float],
            ret: Ty::Float,
            eval: NativeEval::Pure(math_floor),
            php: |a| format!("floor({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "ceil",
            params: vec![Ty::Float],
            ret: Ty::Float,
            eval: NativeEval::Pure(math_ceil),
            php: |a| format!("ceil({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "abs",
            params: vec![Ty::Int],
            ret: Ty::Int,
            eval: NativeEval::Pure(math_abs),
            php: |a| format!("abs({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "min",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            eval: NativeEval::Pure(math_min),
            php: |a| format!("min({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Math",
            name: "max",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            eval: NativeEval::Pure(math_max),
            php: |a| format!("max({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

// ---- Core.Text ----------------------------------------------------------------------------------
// String natives, all concrete-typed. Each erases to a PHP string builtin (D-L9). ASCII-oriented to
// stay byte-identical with PHP: `len` is the *byte* length (PHP `strlen`), and `upper`/`lower` are
// ASCII-case (PHP `strtoupper`/`strtolower`), so multi-byte text could differ between the Rust
// backends and PHP — examples use ASCII. The run↔runvm spine is always byte-identical (both Rust).

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

/// The `Core.Text` registry entries (M3 Track B Wave 2). NOTE the PHP arg order: `explode`/`implode`
/// take the separator first, and `str_replace` is `(search, replace, subject)` — the `php` closures
/// reorder accordingly so the erasure matches Phorge's `(subject, …)` argument order.
fn text_natives() -> Vec<NativeFn> {
    let s = || Ty::String;
    vec![
        NativeFn {
            module: "Core.Text",
            name: "len",
            params: vec![s()],
            ret: Ty::Int,
            eval: NativeEval::Pure(text_len),
            php: |a| format!("strlen({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "upper",
            params: vec![s()],
            ret: Ty::String,
            eval: NativeEval::Pure(text_upper),
            php: |a| format!("strtoupper({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "lower",
            params: vec![s()],
            ret: Ty::String,
            eval: NativeEval::Pure(text_lower),
            php: |a| format!("strtolower({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "trim",
            params: vec![s()],
            ret: Ty::String,
            eval: NativeEval::Pure(text_trim),
            php: |a| format!("trim({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "contains",
            params: vec![s(), s()],
            ret: Ty::Bool,
            eval: NativeEval::Pure(text_contains),
            php: |a| format!("str_contains({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Text",
            name: "split",
            params: vec![s(), s()],
            ret: Ty::List(Box::new(Ty::String)),
            eval: NativeEval::Pure(text_split),
            // PHP `explode(separator, string)` — separator first.
            php: |a| format!("explode({}, {})", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "splitOnce",
            params: vec![s(), s()],
            ret: Ty::List(Box::new(Ty::String)),
            eval: NativeEval::Pure(text_split_once),
            // PHP `explode(separator, string, 2)` — separator first; the limit-2 yields [head, tail].
            php: |a| format!("explode({}, {}, 2)", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "join",
            params: vec![Ty::List(Box::new(Ty::String)), s()],
            ret: Ty::String,
            eval: NativeEval::Pure(text_join),
            // PHP `implode(glue, array)` — glue first.
            php: |a| format!("implode({}, {})", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.Text",
            name: "replace",
            params: vec![s(), s(), s()],
            ret: Ty::String,
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
    ]
}

// ---- Core.File ----------------------------------------------------------------------------------
// Filesystem natives (std::fs ↔ PHP file builtins, D-L9). `read` returns `string?` — `null` on any
// failure (missing file, permission, non-UTF-8) — exercising S2 null-safety (`??` / `if (var x =
// read(p))`). DETERMINISM: a file *read* is byte-identical across backends iff every backend reads
// the same bytes, so file examples read a **committed fixture**; `write` is a non-deterministic side
// effect and is excluded from the byte-identity-gated example set (it is unit-tested with a temp
// file). The run↔runvm spine shares the same `eval`, so it is always identical regardless.

fn file_read(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Any read failure maps to `null` (the `string?` absent case), never a fault.
        [Value::Str(path)] => Ok(match std::fs::read_to_string(path) {
            Ok(s) => Value::Str(s),
            Err(_) => Value::Null,
        }),
        _ => Err("File.read expects (string)".into()),
    }
}
fn file_exists(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(path)] => Ok(Value::Bool(std::path::Path::new(path).exists())),
        _ => Err("File.exists expects (string)".into()),
    }
}
fn file_write(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(path), Value::Str(contents)] => match std::fs::write(path, contents) {
            Ok(()) => Ok(Value::Unit),
            Err(e) => Err(format!("File.write failed: {e}")),
        },
        _ => Err("File.write expects (string, string)".into()),
    }
}

/// The `Core.File` registry entries (M3 Track B Wave 2).
fn file_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.File",
            name: "read",
            params: vec![Ty::String],
            ret: Ty::Optional(Box::new(Ty::String)),
            eval: NativeEval::Pure(file_read),
            // `@` suppresses the missing-file warning; the assign-and-compare distinguishes a missing
            // file (`false` → null) from a legitimately empty one (`""`), which a bare `?:` would not.
            php: |a| {
                format!(
                    "(($__c = @file_get_contents({})) === false ? null : $__c)",
                    parg(a, 0)
                )
            },
        },
        NativeFn {
            module: "Core.File",
            name: "exists",
            params: vec![Ty::String],
            ret: Ty::Bool,
            eval: NativeEval::Pure(file_exists),
            php: |a| format!("file_exists({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.File",
            name: "write",
            params: vec![Ty::String, Ty::String],
            ret: Ty::Unit,
            eval: NativeEval::Pure(file_write),
            php: |a| format!("file_put_contents({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

// ---- Core.Bytes ---------------------------------------------------------------------------------
// Octet-sequence natives bridging `bytes` ↔ `string` (M6 W0). `to_string` returns `string?` — `null`
// on invalid UTF-8 (composes with S2 `??` / if-let), never a fault. `len` is the BYTE count
// (`strlen`), as is `Core.Text.len` — the std stays extension-free (no mbstring). `slice` is a total,
// bounds-clamped half-open `[start, end)` (no fault, unlike list `xs[i]`). PHP strings are byte
// arrays, so the erasures are exact.

fn bytes_from_string(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Bytes(std::rc::Rc::new(s.clone().into_bytes()))),
        _ => Err("Bytes.from_string expects (string)".into()),
    }
}
fn bytes_to_string(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Invalid UTF-8 → `null` (the `string?` absent case), never a fault.
        [Value::Bytes(b)] => Ok(match std::str::from_utf8(b) {
            Ok(s) => Value::Str(s.to_string()),
            Err(_) => Value::Null,
        }),
        _ => Err("Bytes.to_string expects (bytes)".into()),
    }
}
fn bytes_len(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Bytes(b)] => Ok(Value::Int(b.len() as i64)),
        _ => Err("Bytes.len expects (bytes)".into()),
    }
}
fn bytes_concat(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Bytes(a), Value::Bytes(b)] => {
            let mut out = Vec::with_capacity(a.len() + b.len());
            out.extend_from_slice(a);
            out.extend_from_slice(b);
            Ok(Value::Bytes(std::rc::Rc::new(out)))
        }
        _ => Err("Bytes.concat expects (bytes, bytes)".into()),
    }
}
fn bytes_find(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Index of the first occurrence of `needle` in `haystack`, or `null` (the `int?` absent case).
        // Empty needle → `0` (matches PHP 8 `strpos($h, "")`). Used to locate the HTTP head/body split.
        [Value::Bytes(haystack), Value::Bytes(needle)] => {
            let idx = if needle.is_empty() {
                Some(0)
            } else {
                haystack
                    .windows(needle.len())
                    .position(|w| w == needle.as_slice())
            };
            Ok(match idx {
                Some(i) => Value::Int(i as i64),
                None => Value::Null,
            })
        }
        _ => Err("Bytes.find expects (bytes, bytes)".into()),
    }
}
fn bytes_slice(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Half-open [start, end), bounds clamped to [0, len] — total, no fault.
        [Value::Bytes(b), Value::Int(start), Value::Int(end)] => {
            let len = b.len() as i64;
            let s = (*start).clamp(0, len) as usize;
            let e = (*end).clamp(0, len) as usize;
            let out = if s >= e { Vec::new() } else { b[s..e].to_vec() };
            Ok(Value::Bytes(std::rc::Rc::new(out)))
        }
        _ => Err("Bytes.slice expects (bytes, int, int)".into()),
    }
}

/// The `Core.Bytes` registry entries (M6 W0).
fn bytes_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Bytes",
            name: "fromString",
            params: vec![Ty::String],
            ret: Ty::Bytes,
            eval: NativeEval::Pure(bytes_from_string),
            // PHP strings are byte arrays → identity.
            php: |a| parg(a, 0).to_string(),
        },
        NativeFn {
            module: "Core.Bytes",
            name: "toString",
            params: vec![Ty::Bytes],
            ret: Ty::Optional(Box::new(Ty::String)),
            eval: NativeEval::Pure(bytes_to_string),
            // UTF-8 validity via PCRE (always compiled in), NOT mbstring's mb_check_encoding:
            // the oracle runs `php -n` and minimal/Alpine PHP drop ini-loaded mbstring, so a core
            // primitive must stay extension-free. preg_match returns 1 (valid) / 0 / false → keep
            // the string only on an exact `=== 1`, else null (the `string?` absent case).
            php: |a| format!("(preg_match('//u', {0}) === 1 ? {0} : null)", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Bytes",
            name: "len",
            params: vec![Ty::Bytes],
            ret: Ty::Int,
            eval: NativeEval::Pure(bytes_len),
            // BYTE count (strlen), not character count (mb_strlen).
            php: |a| format!("strlen({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Bytes",
            name: "find",
            params: vec![Ty::Bytes, Ty::Bytes],
            ret: Ty::Optional(Box::new(Ty::Int)),
            eval: NativeEval::Pure(bytes_find),
            // strpos returns int|false; map false → null (the `int?` absent case). Empty needle → 0.
            php: |a| {
                format!(
                    "(($__bp = strpos({0}, {1})) === false ? null : $__bp)",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.Bytes",
            name: "concat",
            params: vec![Ty::Bytes, Ty::Bytes],
            ret: Ty::Bytes,
            eval: NativeEval::Pure(bytes_concat),
            php: |a| format!("({} . {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Bytes",
            name: "slice",
            params: vec![Ty::Bytes, Ty::Int, Ty::Int],
            ret: Ty::Bytes,
            eval: NativeEval::Pure(bytes_slice),
            // Total, bounds-clamped half-open slice via an IIFE — matches the Rust clamp exactly.
            php: |a| {
                format!(
                    "(function($b,$s,$e){{$n=strlen($b);$s=max(0,min($s,$n));$e=max(0,min($e,$n));return $s<$e?substr($b,$s,$e-$s):\"\";}})({}, {}, {})",
                    parg(a, 0),
                    parg(a, 1),
                    parg(a, 2)
                )
            },
        },
    ]
}

// ---- Core.Html ----------------------------------------------------------------------------------
// Typed, auto-escaping HTML. `Html` (and Wave 2's `Attr`) is a distinct `Ty` (types.rs) that erases
// to PHP `string` and rides `Value::Str` at runtime; the safety is entirely in the checker's
// non-interchangeability of `Html`/`Attr` and `string`.
//
// Wave 1 — the escape kernel (the trust boundary):
//   * text(string)  -> Html    escape untrusted text IN  (the only safe lift)
//   * raw(string)   -> Html    audited trust opt-out (greppable: `grep html.raw`)
//   * render(Html)  -> string  finished HTML OUT, ready to print
//
// Wave 2 — the element builders (compose typed fragments; tag/attribute NAMES are author literals,
// so they are not escaped — only attribute *values* and text are, exactly as Wave 1):
//   * attr(string, string) -> Attr        ` name="ESC(value)"`   (leading space; value escaped)
//   * bool_attr(string)    -> Attr        ` name`                 (valueless: disabled/checked)
//   * el(string, List<Attr>, List<Html>) -> Html   `<tag ATTRS>CHILDREN</tag>`
//   * void_el(string, List<Attr>)        -> Html   `<tag ATTRS/>`   (self-closing: br/hr/img)
//   * concat(List<Html>)   -> Html        join Html fragments (no separator)
// Empty `[]` for the attr/child lists is accepted (checker call-arg expected-type rule), so
// `el("p", [], [text(x)])` reads naturally. The `html"…"` literal sugar is Wave 3.
//
// BYTE-IDENTITY: every builder's `eval` (Rust) and `php` emission must produce the same bytes; the
// `php` for `el`/`void_el` uses an IIFE so the tag expression is evaluated exactly once (no
// double-eval), matching the single Rust evaluation. The unit test pins each pair.

/// HTML-escape `s` exactly as PHP's `htmlspecialchars($s, ENT_QUOTES, 'UTF-8')` does for valid UTF-8
/// (Phorge strings are always valid UTF-8, so the invalid-byte/ENT_SUBSTITUTE path is unreachable).
/// `&` MUST be replaced first — otherwise the `&` this function inserts gets double-escaped. This
/// five-char table is THE byte-identity contract with the `php` emission below; the unit test pins it.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            _ => out.push(c),
        }
    }
    out
}

fn html_text(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Str(html_escape(s))),
        _ => Err("Html.text expects (string)".into()),
    }
}

/// `raw`/`render` are runtime identities on the underlying `Value::Str` — `raw` lifts a trusted
/// string to `Html`, `render` lowers finished `Html` back to a `string`; both are pure relabelings,
/// the type checker is what makes them meaningful.
fn html_identity(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Str(s.clone())),
        _ => Err("expected (string)".into()),
    }
}

/// Concatenate a list of `Html`/`Attr` fragments (each erased to `Value::Str`) with no separator —
/// the runtime half of `el`/`void_el`/`concat`. PHP-side this is `implode('', $list)`.
fn html_join_fragments(items: &[Value]) -> Result<String, String> {
    let mut out = String::new();
    for it in items {
        match it {
            Value::Str(s) => out.push_str(s),
            other => {
                return Err(format!(
                    "html builder expects rendered string fragments, found {}",
                    other.type_name()
                ))
            }
        }
    }
    Ok(out)
}

/// `attr(name, value)` -> ` name="ESC(value)"` (leading space, so attrs concatenate directly between
/// the tag and `>`). The NAME is an author literal (trusted, not escaped, like the tag); only the
/// VALUE is escaped — the same `htmlspecialchars(_, ENT_QUOTES)` boundary as `text`.
fn html_attr(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(name), Value::Str(value)] => {
            Ok(Value::Str(format!(" {name}=\"{}\"", html_escape(value))))
        }
        _ => Err("Html.attr expects (string, string)".into()),
    }
}

/// `bool_attr(name)` -> ` name` — a valueless boolean attribute (`disabled`, `checked`, `required`).
fn html_bool_attr(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(name)] => Ok(Value::Str(format!(" {name}"))),
        _ => Err("Html.bool_attr expects (string)".into()),
    }
}

/// `el(tag, attrs, children)` -> `<tag ATTRS>CHILDREN</tag>`. Attrs already carry their leading
/// space; children are pre-rendered `Html` joined with no separator.
fn html_el(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(tag), Value::List(attrs), Value::List(children)] => {
            let a = html_join_fragments(attrs)?;
            let c = html_join_fragments(children)?;
            Ok(Value::Str(format!("<{tag}{a}>{c}</{tag}>")))
        }
        _ => Err("Html.el expects (string, List<Attr>, List<Html>)".into()),
    }
}

/// `void_el(tag, attrs)` -> `<tag ATTRS/>` — a self-closing void element (`br`, `hr`, `img`, …).
fn html_void_el(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(tag), Value::List(attrs)] => {
            let a = html_join_fragments(attrs)?;
            Ok(Value::Str(format!("<{tag}{a}/>")))
        }
        _ => Err("Html.void_el expects (string, List<Attr>)".into()),
    }
}

/// `concat(parts)` -> the `Html` parts joined with no separator (combine sibling fragments).
fn html_concat(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(parts)] => Ok(Value::Str(html_join_fragments(parts)?)),
        _ => Err("Html.concat expects (List<Html>)".into()),
    }
}

// Named per-tag helpers (`div`/`p`/`br`/…) — sugar over `el`/`void_el` with the tag baked in, so
// `Html.div([], [text(x)])` reads like `<div>…</div>` without repeating the tag string. The blocker
// that deferred these (the `eval`/`php` are bare `fn` pointers and cannot close over a runtime tag)
// is dissolved by MONOMORPHIZING: each macro invocation emits its own `ev`/`php` pair with the tag
// literal compiled in via `concat!`, so every tag is a uniform registry entry with a real, byte-
// identity-testable eval+php — no new runtime surface, no checker/parser/backend change. Tag names
// are single lowercase words, so they need no casing migration in the namespace reshape.

/// A normal (content) element helper: `tag_el!("div")` ⇒ a `NativeFn` for
/// `Html.div(List<Attr>, List<Html>) -> Html` emitting `<div ATTRS>CHILDREN</div>`. Byte-identical to
/// `el("div", attrs, children)` on both Rust backends and PHP (same IIFE-free baked form).
macro_rules! tag_el {
    ($tag:literal) => {{
        fn ev(args: &[Value], _: &mut String) -> Result<Value, String> {
            match args {
                [Value::List(attrs), Value::List(children)] => {
                    let a = html_join_fragments(attrs)?;
                    let c = html_join_fragments(children)?;
                    Ok(Value::Str(format!(
                        concat!("<", $tag, "{}>{}</", $tag, ">"),
                        a, c
                    )))
                }
                _ => Err(concat!("Html.", $tag, " expects (List<Attr>, List<Html>)").into()),
            }
        }
        fn php(a: &[String]) -> String {
            format!(
                concat!(
                    "(function($a,$c){{return '<",
                    $tag,
                    "' . implode('', $a) . '>' . implode('', $c) . '</",
                    $tag,
                    ">';}})({}, {})"
                ),
                parg(a, 0),
                parg(a, 1)
            )
        }
        NativeFn {
            module: "Core.Html",
            name: $tag,
            params: vec![Ty::List(Box::new(Ty::Attr)), Ty::List(Box::new(Ty::Html))],
            ret: Ty::Html,
            eval: NativeEval::Pure(ev),
            php,
        }
    }};
}

/// A void (self-closing) element helper: `tag_void!("br")` ⇒ `Html.br(List<Attr>) -> Html` emitting
/// `<br ATTRS/>`. Byte-identical to `void_el("br", attrs)`.
macro_rules! tag_void {
    ($tag:literal) => {{
        fn ev(args: &[Value], _: &mut String) -> Result<Value, String> {
            match args {
                [Value::List(attrs)] => {
                    let a = html_join_fragments(attrs)?;
                    Ok(Value::Str(format!(concat!("<", $tag, "{}/>"), a)))
                }
                _ => Err(concat!("Html.", $tag, " expects (List<Attr>)").into()),
            }
        }
        fn php(a: &[String]) -> String {
            format!(
                concat!(
                    "(function($a){{return '<",
                    $tag,
                    "' . implode('', $a) . '/>';}})({})"
                ),
                parg(a, 0)
            )
        }
        NativeFn {
            module: "Core.Html",
            name: $tag,
            params: vec![Ty::List(Box::new(Ty::Attr))],
            ret: Ty::Html,
            eval: NativeEval::Pure(ev),
            php,
        }
    }};
}

fn html_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Html",
            name: "text",
            params: vec![Ty::String],
            ret: Ty::Html,
            eval: NativeEval::Pure(html_text),
            // Flags PINNED (not PHP's version-varying default) so the output is stable and `php -n`
            // safe; htmlspecialchars is tier-1 (ext/standard, always compiled).
            php: |a| format!("htmlspecialchars({}, ENT_QUOTES, 'UTF-8')", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Html",
            name: "raw",
            params: vec![Ty::String],
            ret: Ty::Html,
            eval: NativeEval::Pure(html_identity),
            php: |a| format!("({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Html",
            name: "render",
            params: vec![Ty::Html],
            ret: Ty::String,
            eval: NativeEval::Pure(html_identity),
            php: |a| format!("({})", parg(a, 0)),
        },
        // ---- Wave 2 builders ----
        NativeFn {
            module: "Core.Html",
            name: "attr",
            params: vec![Ty::String, Ty::String],
            ret: Ty::Attr,
            eval: NativeEval::Pure(html_attr),
            // ` name="ESC(value)"` — name trusted (author literal), value escaped (same boundary as
            // `text`). Single-quoted PHP literals carry the leading space + `="` + closing `"`.
            php: |a| {
                format!(
                    "' ' . {} . '=\"' . htmlspecialchars({}, ENT_QUOTES, 'UTF-8') . '\"'",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.Html",
            name: "boolAttr",
            params: vec![Ty::String],
            ret: Ty::Attr,
            eval: NativeEval::Pure(html_bool_attr),
            php: |a| format!("' ' . {}", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Html",
            name: "el",
            params: vec![
                Ty::String,
                Ty::List(Box::new(Ty::Attr)),
                Ty::List(Box::new(Ty::Html)),
            ],
            ret: Ty::Html,
            eval: NativeEval::Pure(html_el),
            // IIFE so the tag expr is evaluated once (no double-eval) — byte-identical to the single
            // Rust evaluation: `<` . tag . implode(attrs) . `>` . implode(children) . `</` . tag . `>`.
            php: |a| {
                format!(
                    "(function($t,$a,$c){{return '<' . $t . implode('', $a) . '>' . implode('', $c) . '</' . $t . '>';}})({}, {}, {})",
                    parg(a, 0),
                    parg(a, 1),
                    parg(a, 2)
                )
            },
        },
        NativeFn {
            module: "Core.Html",
            name: "voidEl",
            params: vec![Ty::String, Ty::List(Box::new(Ty::Attr))],
            ret: Ty::Html,
            eval: NativeEval::Pure(html_void_el),
            php: |a| {
                format!(
                    "(function($t,$a){{return '<' . $t . implode('', $a) . '/>';}})({}, {})",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.Html",
            name: "concat",
            params: vec![Ty::List(Box::new(Ty::Html))],
            ret: Ty::Html,
            eval: NativeEval::Pure(html_concat),
            php: |a| format!("implode('', {})", parg(a, 0)),
        },
        // ---- Option 1: named per-tag helpers (curated common HTML5 set) ----
        // Content elements `html.<tag>(attrs, children) -> Html`.
        tag_el!("div"),
        tag_el!("span"),
        tag_el!("p"),
        tag_el!("a"),
        tag_el!("ul"),
        tag_el!("ol"),
        tag_el!("li"),
        tag_el!("h1"),
        tag_el!("h2"),
        tag_el!("h3"),
        tag_el!("h4"),
        tag_el!("h5"),
        tag_el!("h6"),
        tag_el!("section"),
        tag_el!("article"),
        tag_el!("header"),
        tag_el!("footer"),
        tag_el!("nav"),
        tag_el!("main"),
        tag_el!("aside"),
        tag_el!("button"),
        tag_el!("label"),
        tag_el!("form"),
        tag_el!("table"),
        tag_el!("thead"),
        tag_el!("tbody"),
        tag_el!("tr"),
        tag_el!("td"),
        tag_el!("th"),
        tag_el!("em"),
        tag_el!("strong"),
        tag_el!("b"),
        tag_el!("i"),
        tag_el!("small"),
        tag_el!("code"),
        tag_el!("pre"),
        tag_el!("blockquote"),
        // Void (self-closing) elements `html.<tag>(attrs) -> Html`.
        tag_void!("br"),
        tag_void!("hr"),
        tag_void!("img"),
        tag_void!("input"),
        tag_void!("meta"),
        tag_void!("link"),
    ]
}

// ---- Core.List ----------------------------------------------------------------------------------
// List query natives. These are the first *generic* natives: their signatures carry `Ty::Param`
// (`reverse(List<T>) -> List<T>`), so the checker routes a call through the same call-site
// unification as a generic free function (`check_native_call` → `check_generic_call` when the sig
// has a type parameter). The registry's `Ty::Param` lives only in the stored signature (consumed by
// the checker's unifier); it never reaches a backend — the compiler types a native call by its
// *expression shape* (→ `CTy::Other`) and the transpiler emits via the `php` closure, so neither
// materializes the native's `ret` (M-RT S7b). `sum` is concrete `List<int> -> int` and routes through
// the ordinary non-generic path. The higher-order ops (`map`/`filter`/`reduce`) land in a later slice.

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
fn list_natives() -> Vec<NativeFn> {
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
fn map_natives() -> Vec<NativeFn> {
    let k = || Ty::Param("K".into());
    let v = || Ty::Param("V".into());
    let map = || Ty::Map(Box::new(k()), Box::new(v()));
    vec![
        NativeFn {
            module: "Core.Map",
            name: "keys",
            params: vec![map()],
            ret: Ty::List(Box::new(k())),
            eval: NativeEval::Pure(map_keys),
            php: |a| format!("array_keys({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Map",
            name: "values",
            params: vec![map()],
            ret: Ty::List(Box::new(v())),
            eval: NativeEval::Pure(map_values),
            php: |a| format!("array_values({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Map",
            name: "has",
            params: vec![map(), k()],
            ret: Ty::Bool,
            eval: NativeEval::Pure(map_has),
            // PHP `array_key_exists(key, array)` — key first.
            php: |a| format!("array_key_exists({}, {})", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.Map",
            name: "size",
            params: vec![map()],
            ret: Ty::Int,
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
fn set_natives() -> Vec<NativeFn> {
    let t = || Ty::Param("T".into());
    vec![
        NativeFn {
            module: "Core.Set",
            name: "of",
            params: vec![Ty::List(Box::new(t()))],
            ret: Ty::Set(Box::new(t())),
            eval: NativeEval::Pure(set_of),
            // Dedup preserving first-occurrence order; SORT_STRING matches HKey string-distinctness.
            php: |a| format!("array_values(array_unique({}, SORT_STRING))", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Set",
            name: "contains",
            params: vec![Ty::Set(Box::new(t())), t()],
            ret: Ty::Bool,
            eval: NativeEval::Pure(set_contains),
            // Strict in_array(needle, haystack) — needle first.
            php: |a| format!("in_array({}, {}, true)", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "Core.Set",
            name: "size",
            params: vec![Ty::Set(Box::new(t()))],
            ret: Ty::Int,
            eval: NativeEval::Pure(set_size),
            php: |a| format!("count({})", parg(a, 0)),
        },
    ]
}

/// Construct the native table once. Order is load-bearing: [`CONSOLE_PRINTLN`] pins slot 0; every
/// other native is resolved by `(module, name)` (or leaf+name) at compile time, so appended order is
/// free. Modules are grouped by `*_natives()` builders (one per `core.*` leaf).
fn build() -> Vec<NativeFn> {
    let mut registry = vec![NativeFn {
        module: "Core.Console",
        name: "println",
        params: vec![Ty::String],
        ret: Ty::Unit,
        eval: NativeEval::Pure(console_println),
        php: |args| {
            let a = args
                .first()
                .map_or_else(|| "\"\"".to_string(), String::clone);
            format!(r#"echo {a} . "\n""#)
        },
    }];
    registry.extend(math_natives());
    registry.extend(text_natives());
    registry.extend(file_natives());
    registry.extend(bytes_natives());
    registry.extend(html_natives());
    registry.extend(list_natives());
    registry.extend(map_natives());
    registry.extend(set_natives());
    // Pinned-slot invariant: the constant the compiler bakes into `Op::CallNative` must address the
    // entry it names. Cheap one-time check at first `registry()` access.
    assert_eq!(
        registry[CONSOLE_PRINTLN].module, "Core.Console",
        "CONSOLE_PRINTLN slot drifted"
    );
    assert_eq!(registry[CONSOLE_PRINTLN].name, "println");
    registry
}

/// The process-wide native table, built once. A `Vec<Ty>` isn't const-constructible, so this can't
/// be a plain `static` — `OnceLock` defers the allocation to first use (design §5).
pub fn registry() -> &'static [NativeFn] {
    static REG: OnceLock<Vec<NativeFn>> = OnceLock::new();
    REG.get_or_init(build)
}

/// Index of the native `(module, name)`, or `None`. Used by the checker and the transpiler, which
/// carry the import map and resolve the *exact* module a leaf qualifier was imported as.
pub fn index_of(module: &str, name: &str) -> Option<usize> {
    registry()
        .iter()
        .position(|n| n.module == module && n.name == name)
}

/// Index of a native by its module's *leaf* segment + name — e.g. leaf `"console"`, name
/// `"println"`. Used by the interpreter and compiler, which (unlike the transpiler) track variable
/// scope and resolve a member call `q.m(..)` locals-first: a qualifier `q` is only leaf-looked-up
/// once it is known *not* to be a bound variable, and the checker has already enforced that `q` was
/// imported and the native exists. Unambiguous while every stdlib leaf is distinct (Waves 1–2);
/// leaf collisions with user packages are resolved by import aliasing (design O-D, deferred).
pub fn index_of_by_leaf(leaf: &str, name: &str) -> Option<usize> {
    registry()
        .iter()
        .position(|n| n.name == name && n.module.rsplit('.').next() == Some(leaf))
}

/// Build the active import map (leaf qualifier → full dotted module path) from a program's items:
/// `import Core.Console;` binds the call-site qualifier `console` to module `Core.Console`. Carried
/// by the checker (import-required + shadowing enforcement) and the transpiler (which has no
/// variable-scope tracking to tell a qualifier from a value).
pub fn import_map(items: &[Item]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for item in items {
        if let Item::Import { path, alias, .. } = item {
            // The bound qualifier is the alias when present (`import a.b as c;` ⇒ `c`), else the
            // path's last segment (M5 S2c).
            let qualifier = alias.clone().or_else(|| path.last().cloned());
            if let Some(q) = qualifier {
                map.insert(q, path.join("."));
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_console_println_slot() {
        let r = registry();
        assert_eq!(r[CONSOLE_PRINTLN].module, "Core.Console");
        assert_eq!(r[CONSOLE_PRINTLN].name, "println");
    }

    #[test]
    fn index_lookups_resolve_console_println() {
        assert_eq!(index_of("Core.Console", "println"), Some(CONSOLE_PRINTLN));
        assert_eq!(
            index_of_by_leaf("Console", "println"),
            Some(CONSOLE_PRINTLN)
        );
        assert_eq!(index_of("Core.Console", "nope"), None);
        assert_eq!(index_of_by_leaf("nope", "println"), None);
    }

    #[test]
    fn console_println_appends_line() {
        let mut out = String::new();
        let r = console_println(&[Value::Str("hi".into())], &mut out).unwrap();
        assert_eq!(out, "hi\n");
        assert!(matches!(r, Value::Unit));
    }

    #[test]
    fn console_println_rejects_composite() {
        let mut out = String::new();
        let err = console_println(&[Value::List(vec![].into())], &mut out).unwrap_err();
        assert!(err.contains("cannot print"), "{err}");
    }

    #[test]
    fn php_emission_is_echo_with_newline() {
        let php = (registry()[CONSOLE_PRINTLN].php)(&["$x".to_string()]);
        assert_eq!(php, r#"echo $x . "\n""#);
    }

    #[test]
    fn math_natives_eval_and_emit() {
        let mut out = String::new();
        // float ops
        assert!(
            matches!(math_sqrt(&[Value::Float(16.0)], &mut out), Ok(Value::Float(x)) if x == 4.0)
        );
        assert!(
            matches!(math_pow(&[Value::Float(2.0), Value::Float(10.0)], &mut out), Ok(Value::Float(x)) if x == 1024.0)
        );
        assert!(
            matches!(math_floor(&[Value::Float(3.7)], &mut out), Ok(Value::Float(x)) if x == 3.0)
        );
        assert!(
            matches!(math_ceil(&[Value::Float(3.2)], &mut out), Ok(Value::Float(x)) if x == 4.0)
        );
        // int ops
        assert!(matches!(
            math_abs(&[Value::Int(-5)], &mut out),
            Ok(Value::Int(5))
        ));
        assert!(matches!(
            math_min(&[Value::Int(3), Value::Int(8)], &mut out),
            Ok(Value::Int(3))
        ));
        assert!(matches!(
            math_max(&[Value::Int(3), Value::Int(8)], &mut out),
            Ok(Value::Int(8))
        ));
        // EV-7: abs of i64::MIN faults, never panics
        assert!(math_abs(&[Value::Int(i64::MIN)], &mut out).is_err());
        // resolvable by both index forms + PHP erasure to the same-named builtin
        let i = index_of("Core.Math", "pow").expect("pow registered");
        assert_eq!(index_of_by_leaf("Math", "pow"), Some(i));
        assert_eq!(
            (registry()[i].php)(&["2.0".into(), "10.0".into()]),
            "pow(2.0, 10.0)"
        );
        assert_eq!(
            (registry()[index_of("Core.Math", "min").unwrap()].php)(&["$a".into(), "$b".into()]),
            "min($a, $b)"
        );
    }

    #[test]
    fn text_natives_eval_and_emit() {
        let mut o = String::new();
        assert!(matches!(
            text_len(&[Value::Str("hello".into())], &mut o),
            Ok(Value::Int(5))
        ));
        assert!(
            matches!(text_upper(&[Value::Str("aB".into())], &mut o), Ok(Value::Str(s)) if s == "AB")
        );
        assert!(
            matches!(text_lower(&[Value::Str("aB".into())], &mut o), Ok(Value::Str(s)) if s == "ab")
        );
        assert!(
            matches!(text_trim(&[Value::Str("  hi  ".into())], &mut o), Ok(Value::Str(s)) if s == "hi")
        );
        assert!(matches!(
            text_contains(
                &[Value::Str("hello".into()), Value::Str("ell".into())],
                &mut o
            ),
            Ok(Value::Bool(true))
        ));
        assert!(matches!(
            text_contains(
                &[Value::Str("hello".into()), Value::Str("z".into())],
                &mut o
            ),
            Ok(Value::Bool(false))
        ));
        assert!(
            matches!(text_replace(&[Value::Str("a-b-c".into()), Value::Str("-".into()), Value::Str("_".into())], &mut o), Ok(Value::Str(s)) if s == "a_b_c")
        );
        // split → List<string>, then join back is the inverse
        let parts = text_split(
            &[Value::Str("a,b,c".into()), Value::Str(",".into())],
            &mut o,
        )
        .unwrap();
        match &parts {
            Value::List(xs) => assert_eq!(xs.len(), 3),
            other => panic!("split returned {other:?}"),
        }
        let joined = text_join(&[parts, Value::Str("|".into())], &mut o).unwrap();
        assert!(matches!(joined, Value::Str(s) if s == "a|b|c"));
        // join rejects a non-string element cleanly
        assert!(text_join(
            &[
                Value::List(std::rc::Rc::new(vec![Value::Int(1)])),
                Value::Str(",".into())
            ],
            &mut o
        )
        .is_err());
        // PHP arg-order reordering (the sharp edge): explode/implode separator-first, str_replace search-first
        assert_eq!(
            (registry()[index_of("Core.Text", "split").unwrap()].php)(&[
                "$s".into(),
                "\",\"".into()
            ]),
            "explode(\",\", $s)"
        );
        assert_eq!(
            (registry()[index_of("Core.Text", "join").unwrap()].php)(&[
                "$xs".into(),
                "\"-\"".into()
            ]),
            "implode(\"-\", $xs)"
        );
        assert_eq!(
            (registry()[index_of("Core.Text", "replace").unwrap()].php)(&[
                "$s".into(),
                "$a".into(),
                "$b".into()
            ]),
            "str_replace($a, $b, $s)"
        );
        assert_eq!(
            index_of_by_leaf("Text", "len"),
            index_of("Core.Text", "len")
        );
    }

    #[test]
    fn html_natives_eval_and_emit() {
        let mut o = String::new();
        // THE byte-identity contract: the Rust escape table must match `htmlspecialchars(_, ENT_QUOTES,
        // 'UTF-8')` exactly. All five chars + a realistic XSS payload, with `&` first (no double-escape).
        assert_eq!(html_escape("&<>\"'"), "&amp;&lt;&gt;&quot;&#039;");
        assert_eq!(
            html_escape("<script>alert(\"x\")</script>"),
            "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;"
        );
        assert_eq!(html_escape("a & b"), "a &amp; b"); // inserted `&` is not re-escaped
        assert_eq!(html_escape("plain text"), "plain text"); // no-op on safe input
                                                             // text escapes; raw + render are identities on the underlying string.
        assert!(
            matches!(html_text(&[Value::Str("a<b".into())], &mut o), Ok(Value::Str(s)) if s == "a&lt;b")
        );
        assert!(
            matches!(html_identity(&[Value::Str("<hr/>".into())], &mut o), Ok(Value::Str(s)) if s == "<hr/>")
        );
        // PHP emission: pinned flags on text; identity wrap on raw/render.
        assert_eq!(
            (registry()[index_of("Core.Html", "text").unwrap()].php)(&["$s".into()]),
            "htmlspecialchars($s, ENT_QUOTES, 'UTF-8')"
        );
        assert_eq!(
            (registry()[index_of("Core.Html", "raw").unwrap()].php)(&["$s".into()]),
            "($s)"
        );
        assert_eq!(
            index_of_by_leaf("Html", "render"),
            index_of("Core.Html", "render")
        );

        // ---- Wave 2 builders: eval bytes + PHP emission ----
        // attr: name trusted, value escaped, leading space + quotes.
        assert!(
            matches!(html_attr(&[Value::Str("href".into()), Value::Str("a&b".into())], &mut o), Ok(Value::Str(s)) if s == " href=\"a&amp;b\"")
        );
        assert!(
            matches!(html_bool_attr(&[Value::Str("disabled".into())], &mut o), Ok(Value::Str(s)) if s == " disabled")
        );
        // el: tag + joined attrs + joined children. Attrs/children are Html/Attr erased to Value::Str.
        let attrs = Value::List(std::rc::Rc::new(vec![Value::Str(" class=\"box\"".into())]));
        let kids = Value::List(std::rc::Rc::new(vec![Value::Str("hi".into())]));
        assert!(
            matches!(html_el(&[Value::Str("p".into()), attrs.clone(), kids.clone()], &mut o), Ok(Value::Str(s)) if s == "<p class=\"box\">hi</p>")
        );
        // el with EMPTY attr list (the call-arg expected-type case) → no attributes.
        let empty = Value::List(std::rc::Rc::new(vec![]));
        assert!(
            matches!(html_el(&[Value::Str("p".into()), empty.clone(), kids.clone()], &mut o), Ok(Value::Str(s)) if s == "<p>hi</p>")
        );
        // void_el: self-closing.
        let src = Value::List(std::rc::Rc::new(vec![Value::Str(" src=\"x.png\"".into())]));
        assert!(
            matches!(html_void_el(&[Value::Str("img".into()), src], &mut o), Ok(Value::Str(s)) if s == "<img src=\"x.png\"/>")
        );
        assert!(
            matches!(html_void_el(&[Value::Str("br".into()), empty.clone()], &mut o), Ok(Value::Str(s)) if s == "<br/>")
        );
        // concat: join Html fragments; empty → "".
        let frags = Value::List(std::rc::Rc::new(vec![
            Value::Str("<i>".into()),
            Value::Str("x".into()),
            Value::Str("</i>".into()),
        ]));
        assert!(matches!(html_concat(&[frags], &mut o), Ok(Value::Str(s)) if s == "<i>x</i>"));
        assert!(matches!(html_concat(&[empty], &mut o), Ok(Value::Str(s)) if s.is_empty()));
        // A non-string fragment is rejected cleanly (never a panic).
        assert!(html_concat(
            &[Value::List(std::rc::Rc::new(vec![Value::Int(1)]))],
            &mut o
        )
        .is_err());
        // PHP emission — the byte-identity counterparts.
        let php = |n: &str, a: &[&str]| {
            let args: Vec<String> = a.iter().map(|s| (*s).to_string()).collect();
            (registry()[index_of("Core.Html", n).unwrap()].php)(&args)
        };
        assert_eq!(
            php("attr", &["$n", "$v"]),
            "' ' . $n . '=\"' . htmlspecialchars($v, ENT_QUOTES, 'UTF-8') . '\"'"
        );
        assert_eq!(php("boolAttr", &["$n"]), "' ' . $n");
        assert_eq!(
            php("el", &["$t", "$a", "$c"]),
            "(function($t,$a,$c){return '<' . $t . implode('', $a) . '>' . implode('', $c) . '</' . $t . '>';})($t, $a, $c)"
        );
        assert_eq!(
            php("voidEl", &["$t", "$a"]),
            "(function($t,$a){return '<' . $t . implode('', $a) . '/>';})($t, $a)"
        );
        assert_eq!(php("concat", &["$xs"]), "implode('', $xs)");
        // All builders resolve by both index forms + carry the Attr/Html return types.
        assert_eq!(index_of_by_leaf("Html", "el"), index_of("Core.Html", "el"));
        assert_eq!(
            registry()[index_of("Core.Html", "attr").unwrap()].ret,
            Ty::Attr
        );
        assert_eq!(
            registry()[index_of("Core.Html", "el").unwrap()].ret,
            Ty::Html
        );
    }

    #[test]
    fn tag_helpers_eval_and_emit() {
        // Option 1 named tags are macro-monomorphized registry entries — exercise them through the
        // registered `eval`/`php` (not the local macro fns) so the test pins what callers actually hit.
        let eval = |n: &str, args: &[Value]| -> Result<Value, String> {
            match registry()[index_of("Core.Html", n).unwrap()].eval {
                NativeEval::Pure(f) => f(args, &mut String::new()),
                NativeEval::HigherOrder(_) => panic!("{n} is not a pure native"),
            }
        };
        let php = |n: &str, a: &[&str]| {
            let args: Vec<String> = a.iter().map(|s| (*s).to_string()).collect();
            (registry()[index_of("Core.Html", n).unwrap()].php)(&args)
        };
        let attrs = Value::List(std::rc::Rc::new(vec![Value::Str(" class=\"box\"".into())]));
        let kids = Value::List(std::rc::Rc::new(vec![Value::Str("hi".into())]));
        let empty = Value::List(std::rc::Rc::new(vec![]));
        // Content element `div`: baked tag, byte-identical to el("div", attrs, children).
        assert!(
            matches!(eval("div", &[attrs.clone(), kids.clone()]), Ok(Value::Str(s)) if s == "<div class=\"box\">hi</div>")
        );
        assert!(matches!(eval("p", &[empty.clone(), kids]), Ok(Value::Str(s)) if s == "<p>hi</p>"));
        // Void elements `img`/`br`: self-closing, byte-identical to void_el(tag, attrs).
        let src = Value::List(std::rc::Rc::new(vec![Value::Str(" src=\"x.png\"".into())]));
        assert!(matches!(eval("img", &[src]), Ok(Value::Str(s)) if s == "<img src=\"x.png\"/>"));
        assert!(
            matches!(eval("br", std::slice::from_ref(&empty)), Ok(Value::Str(s)) if s == "<br/>")
        );
        // Wrong arity is a clean fault, never a panic.
        assert!(eval("div", &[empty]).is_err());
        // PHP emission — the byte-identity counterparts (baked tag, so no `$t` parameter).
        assert_eq!(
            php("div", &["$a", "$c"]),
            "(function($a,$c){return '<div' . implode('', $a) . '>' . implode('', $c) . '</div>';})($a, $c)"
        );
        assert_eq!(
            php("br", &["$a"]),
            "(function($a){return '<br' . implode('', $a) . '/>';})($a)"
        );
        // Resolve by both index forms + carry the Html return type.
        assert_eq!(
            index_of_by_leaf("Html", "div"),
            index_of("Core.Html", "div")
        );
        assert_eq!(
            registry()[index_of("Core.Html", "section").unwrap()].ret,
            Ty::Html
        );
        assert_eq!(
            registry()[index_of("Core.Html", "hr").unwrap()].ret,
            Ty::Html
        );
    }

    #[test]
    fn file_natives_eval_and_emit() {
        let mut o = String::new();
        // A missing path reads as `null` (the `string?` absent case), never a fault.
        let missing = "/nonexistent/phorge/definitely/not/here.txt";
        assert!(matches!(
            file_read(&[Value::Str(missing.into())], &mut o),
            Ok(Value::Null)
        ));
        assert!(matches!(
            file_exists(&[Value::Str(missing.into())], &mut o),
            Ok(Value::Bool(false))
        ));
        // write → read round-trip through a temp file (write is unit-tested, not exampled).
        let tmp = std::env::temp_dir().join("phorge_native_file_test.txt");
        let p = tmp.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&tmp);
        assert!(matches!(
            file_write(&[Value::Str(p.clone()), Value::Str("hi\n".into())], &mut o),
            Ok(Value::Unit)
        ));
        assert!(matches!(
            file_exists(&[Value::Str(p.clone())], &mut o),
            Ok(Value::Bool(true))
        ));
        assert!(
            matches!(file_read(&[Value::Str(p.clone())], &mut o), Ok(Value::Str(s)) if s == "hi\n")
        );
        let _ = std::fs::remove_file(&tmp);
        // `read` returns `string?`; PHP erasure distinguishes empty file from missing.
        assert_eq!(
            crate::native::registry()[index_of("Core.File", "read").unwrap()].ret,
            Ty::Optional(Box::new(Ty::String))
        );
        assert_eq!(
            (registry()[index_of("Core.File", "read").unwrap()].php)(&["$p".into()]),
            "(($__c = @file_get_contents($p)) === false ? null : $__c)"
        );
        assert_eq!(
            index_of_by_leaf("File", "exists"),
            index_of("Core.File", "exists")
        );
    }

    #[test]
    fn list_natives_eval_and_emit() {
        let mut o = String::new();
        // reverse: generic over the element type — works on any List, byte-identical to array_reverse.
        let nums = Value::List(std::rc::Rc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ]));
        match list_reverse(std::slice::from_ref(&nums), &mut o).unwrap() {
            Value::List(xs) => {
                assert_eq!(xs.len(), 3);
                assert!(matches!(xs[0], Value::Int(3)));
                assert!(matches!(xs[2], Value::Int(1)));
            }
            other => panic!("reverse returned {other:?}"),
        }
        // sum: concrete List<int> -> int.
        assert!(matches!(
            list_sum(std::slice::from_ref(&nums), &mut o),
            Ok(Value::Int(6))
        ));
        // sum over the empty list is 0.
        assert!(matches!(
            list_sum(&[Value::List(std::rc::Rc::new(vec![]))], &mut o),
            Ok(Value::Int(0))
        ));
        // EV-7: an overflowing sum faults cleanly, never panics.
        let huge = Value::List(std::rc::Rc::new(vec![Value::Int(i64::MAX), Value::Int(1)]));
        assert!(list_sum(&[huge], &mut o).is_err());
        // a non-int element is a clean fault.
        assert!(list_sum(
            &[Value::List(std::rc::Rc::new(vec![Value::Str("x".into())]))],
            &mut o
        )
        .is_err());
        // PHP erasure + both index forms + the generic return type is carried in the registry.
        assert_eq!(
            (registry()[index_of("Core.List", "reverse").unwrap()].php)(&["$xs".into()]),
            "array_reverse($xs)"
        );
        assert_eq!(
            (registry()[index_of("Core.List", "sum").unwrap()].php)(&["$xs".into()]),
            "array_sum($xs)"
        );
        assert_eq!(
            index_of_by_leaf("List", "reverse"),
            index_of("Core.List", "reverse")
        );
        assert_eq!(
            registry()[index_of("Core.List", "reverse").unwrap()].ret,
            Ty::List(Box::new(Ty::Param("T".into())))
        );
    }

    #[test]
    fn list_higher_order_eval_and_emit() {
        // The HOF natives drive the closure via the backend-supplied invoker; here a stub invoker
        // stands in for a backend (the `f` Value is a placeholder the stub ignores). The end-to-end
        // closure path is covered by the differential harness; this pins the iteration/collect logic.
        let nums = Value::List(std::rc::Rc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]));
        let placeholder = Value::Int(0);

        // map: double each element.
        let mut dbl = |_f: &Value, a: Vec<Value>| match a.as_slice() {
            [Value::Int(n)] => Ok(Value::Int(n * 2)),
            _ => Err("bad arity".to_string()),
        };
        match list_map(&[nums.clone(), placeholder.clone()], &mut dbl).unwrap() {
            Value::List(xs) => {
                assert_eq!(xs.len(), 4);
                assert!(matches!(xs[0], Value::Int(2)));
                assert!(matches!(xs[3], Value::Int(8)));
            }
            other => panic!("map returned {other:?}"),
        }

        // filter: keep the even elements (predicate returns bool).
        let mut even = |_f: &Value, a: Vec<Value>| match a.as_slice() {
            [Value::Int(n)] => Ok(Value::Bool(n % 2 == 0)),
            _ => Err("bad arity".to_string()),
        };
        match list_filter(&[nums.clone(), placeholder.clone()], &mut even).unwrap() {
            Value::List(xs) => {
                assert_eq!(xs.len(), 2);
                assert!(matches!(xs[0], Value::Int(2)));
                assert!(matches!(xs[1], Value::Int(4)));
            }
            other => panic!("filter returned {other:?}"),
        }

        // filter: a non-bool predicate result is a clean fault, never a panic.
        let mut bad = |_f: &Value, _a: Vec<Value>| Ok(Value::Int(7));
        assert!(list_filter(&[nums.clone(), placeholder.clone()], &mut bad).is_err());

        // reduce: sum, seeded with 100.
        let mut add = |_f: &Value, a: Vec<Value>| match a.as_slice() {
            [Value::Int(acc), Value::Int(x)] => Ok(Value::Int(acc + x)),
            _ => Err("bad arity".to_string()),
        };
        assert!(matches!(
            list_reduce(
                &[nums.clone(), Value::Int(100), placeholder.clone()],
                &mut add
            ),
            Ok(Value::Int(110))
        ));

        // reduce over the empty list returns the seed unchanged (the closure is never called).
        let empty = Value::List(std::rc::Rc::new(vec![]));
        let mut never = |_f: &Value, _a: Vec<Value>| Err("must not be called".to_string());
        assert!(matches!(
            list_reduce(&[empty, Value::Int(42), placeholder.clone()], &mut never),
            Ok(Value::Int(42))
        ));

        // A fault from the closure propagates as a plain `String` (the backend-shared contract).
        let mut boom = |_f: &Value, _a: Vec<Value>| Err("kaboom".to_string());
        assert_eq!(
            list_map(&[nums, placeholder], &mut boom).unwrap_err(),
            "kaboom"
        );

        // PHP erasure: array_map (arg order swapped), array_values(array_filter), array_reduce.
        assert_eq!(
            (registry()[index_of("Core.List", "map").unwrap()].php)(&["$xs".into(), "$f".into()]),
            "array_map($f, $xs)"
        );
        assert_eq!(
            (registry()[index_of("Core.List", "filter").unwrap()].php)(&[
                "$xs".into(),
                "$f".into()
            ]),
            "array_values(array_filter($xs, $f))"
        );
        assert_eq!(
            (registry()[index_of("Core.List", "reduce").unwrap()].php)(&[
                "$xs".into(),
                "$init".into(),
                "$f".into()
            ]),
            "array_reduce($xs, $f, $init)"
        );
        assert_eq!(
            index_of_by_leaf("List", "map"),
            index_of("Core.List", "map")
        );
    }

    #[test]
    fn map_natives_eval_and_emit() {
        use crate::value::HKey;
        let mut o = String::new();
        // insertion-ordered map ["a"=>1, "b"=>2]; keys/values preserve that order.
        let m = Value::Map(std::rc::Rc::new(vec![
            (HKey::Str("a".into()), Value::Int(1)),
            (HKey::Str("b".into()), Value::Int(2)),
        ]));
        match map_keys(std::slice::from_ref(&m), &mut o).unwrap() {
            Value::List(ks) => {
                assert_eq!(ks.len(), 2);
                assert!(matches!(&ks[0], Value::Str(s) if s == "a"));
                assert!(matches!(&ks[1], Value::Str(s) if s == "b"));
            }
            other => panic!("keys returned {other:?}"),
        }
        match map_values(std::slice::from_ref(&m), &mut o).unwrap() {
            Value::List(vs) => {
                assert!(matches!(vs[0], Value::Int(1)));
                assert!(matches!(vs[1], Value::Int(2)));
            }
            other => panic!("values returned {other:?}"),
        }
        assert!(matches!(
            map_has(&[m.clone(), Value::Str("a".into())], &mut o),
            Ok(Value::Bool(true))
        ));
        assert!(matches!(
            map_has(&[m.clone(), Value::Str("z".into())], &mut o),
            Ok(Value::Bool(false))
        ));
        // a non-hashable key (float) is a clean fault, never a panic (EV-7).
        assert!(map_has(&[m.clone(), Value::Float(1.0)], &mut o).is_err());
        assert!(matches!(
            map_size(std::slice::from_ref(&m), &mut o),
            Ok(Value::Int(2))
        ));
        // PHP erasures (note has: array_key_exists(key, array) — key first) + generic return types.
        let php = |n: &str, a: &[&str]| {
            let args: Vec<String> = a.iter().map(|s| (*s).to_string()).collect();
            (registry()[index_of("Core.Map", n).unwrap()].php)(&args)
        };
        assert_eq!(php("keys", &["$m"]), "array_keys($m)");
        assert_eq!(php("values", &["$m"]), "array_values($m)");
        assert_eq!(php("has", &["$m", "$k"]), "array_key_exists($k, $m)");
        assert_eq!(php("size", &["$m"]), "count($m)");
        assert_eq!(
            index_of_by_leaf("Map", "keys"),
            index_of("Core.Map", "keys")
        );
        assert_eq!(
            registry()[index_of("Core.Map", "keys").unwrap()].ret,
            Ty::List(Box::new(Ty::Param("K".into())))
        );
        assert_eq!(
            registry()[index_of("Core.Map", "values").unwrap()].ret,
            Ty::List(Box::new(Ty::Param("V".into())))
        );
    }

    #[test]
    fn set_natives_eval_and_emit() {
        let mut o = String::new();
        // of: dedup preserving first-occurrence order.
        let xs = Value::List(std::rc::Rc::new(vec![
            Value::Int(3),
            Value::Int(1),
            Value::Int(3),
            Value::Int(2),
            Value::Int(1),
        ]));
        let s = set_of(std::slice::from_ref(&xs), &mut o).unwrap();
        match &s {
            Value::Set(elems) => {
                assert_eq!(elems.len(), 3); // {3, 1, 2}
                assert_eq!(elems[0], crate::value::HKey::Int(3)); // first-seen order
                assert_eq!(elems[1], crate::value::HKey::Int(1));
                assert_eq!(elems[2], crate::value::HKey::Int(2));
            }
            other => panic!("of returned {other:?}"),
        }
        assert!(matches!(
            set_contains(&[s.clone(), Value::Int(2)], &mut o),
            Ok(Value::Bool(true))
        ));
        assert!(matches!(
            set_contains(&[s.clone(), Value::Int(9)], &mut o),
            Ok(Value::Bool(false))
        ));
        assert!(matches!(
            set_size(std::slice::from_ref(&s), &mut o),
            Ok(Value::Int(3))
        ));
        // a non-hashable element (float) is a clean fault, never a panic (EV-7).
        assert!(set_contains(&[s, Value::Float(2.0)], &mut o).is_err());
        assert!(set_of(
            &[Value::List(std::rc::Rc::new(vec![Value::Float(1.0)]))],
            &mut o
        )
        .is_err());
        // PHP erasures + generic return type.
        let php = |n: &str, a: &[&str]| {
            let args: Vec<String> = a.iter().map(|s| (*s).to_string()).collect();
            (registry()[index_of("Core.Set", n).unwrap()].php)(&args)
        };
        assert_eq!(
            php("of", &["$xs"]),
            "array_values(array_unique($xs, SORT_STRING))"
        );
        assert_eq!(php("contains", &["$s", "$x"]), "in_array($x, $s, true)");
        assert_eq!(php("size", &["$s"]), "count($s)");
        assert_eq!(index_of_by_leaf("Set", "of"), index_of("Core.Set", "of"));
        assert_eq!(
            registry()[index_of("Core.Set", "of").unwrap()].ret,
            Ty::Set(Box::new(Ty::Param("T".into())))
        );
    }

    #[test]
    fn import_map_binds_leaf_to_full_path() {
        use crate::token::Span;
        let sp = Span {
            start: 0,
            len: 0,
            line: 1,
            col: 1,
        };
        let items = vec![Item::Import {
            path: vec!["Core".into(), "Console".into()],
            alias: None,
            span: sp,
        }];
        let m = import_map(&items);
        assert_eq!(m.get("Console").map(String::as_str), Some("Core.Console"));

        // An alias overrides the bound qualifier (M5 S2c).
        let aliased = vec![Item::Import {
            path: vec!["acme".into(), "util".into()],
            alias: Some("u".into()),
            span: sp,
        }];
        let m = import_map(&aliased);
        assert_eq!(m.get("u").map(String::as_str), Some("acme.util"));
        assert!(!m.contains_key("util"), "alias replaces the leaf qualifier");
    }
}
