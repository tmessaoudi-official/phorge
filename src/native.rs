//! Namespaced native (built-in) function registry — the stdlib's runtime + type + transpile
//! surface, addressed by `(module, name)` (e.g. module `core.console`, name `println`). One entry
//! single-sources all four facets of a native, so the four backends cannot drift:
//!   * `params` / `ret` — the checker's signature for a call to this native;
//!   * `eval` — the runtime behavior, shared by the tree-walking interpreter *and* the VM (the
//!     structural parity guarantee, exactly like the value kernels: one impl, two callers);
//!   * `php` — the transpile-time PHP emission (a `core.*` native erases to PHP's flat builtins;
//!     the namespace is a compile-time organizing layer, decisions N-2/D-L9).
//!
//! The registry is the load-bearing target of `import core.console;` (M3 namespace reshape, Wave 1,
//! `docs/specs/2026-06-18-m3-namespace-system-design.md`). The former free global `println` is
//! retired in favor of `core.console.println`, and `Op::Print` in favor of
//! `Op::CallNative(index, argc)` indexing this table.

use crate::ast::Item;
use crate::types::Ty;
use crate::value::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One built-in function, addressed by `(module, name)`. See the module docs for the four facets.
pub struct NativeFn {
    /// Dotted module path the native lives under — e.g. `"core.console"`.
    pub module: &'static str,
    /// Bare function name — e.g. `"println"`.
    pub name: &'static str,
    /// Parameter types — the checker validates call arguments against these.
    pub params: Vec<Ty>,
    /// Return type.
    pub ret: Ty,
    /// Runtime behavior, shared by the interpreter and the VM. Threads the program's output buffer
    /// so a side-effecting native (`console.println`) can append to it; pure natives ignore it. The
    /// arguments arrive in source order.
    pub eval: fn(&[Value], &mut String) -> Result<Value, String>,
    /// PHP emission: given the already-emitted PHP for each argument, return the PHP snippet this
    /// native erases to (decision N-2). For `console.println`: `echo {a} . "\n"`.
    pub php: fn(&[String]) -> String,
}

/// Pinned registry slot for `core.console.println` — the migrated former `Op::Print`. The compiler
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

// ---- core.math ----------------------------------------------------------------------------------
// Concrete-typed numeric natives (`Ty` has no type variable, so no overloading): the float ops
// `sqrt`/`pow`/`floor`/`ceil` are `float -> float`; `abs`/`min`/`max` are `int`. Each erases to the
// PHP builtin of the same name (D-L9). NOTE (KNOWN_ISSUES, float precision): an *irrational* result
// (`sqrt(2.0)`) renders with more digits on the Rust backends than PHP's default 14-sig-digit `echo`,
// so examples stay on exactly-representable values; the run↔runvm spine is unaffected (both Rust).

fn math_sqrt(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.sqrt())),
        _ => Err("math.sqrt expects (float)".into()),
    }
}
fn math_pow(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(b), Value::Float(e)] => Ok(Value::Float(b.powf(*e))),
        _ => Err("math.pow expects (float, float)".into()),
    }
}
fn math_floor(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.floor())),
        _ => Err("math.floor expects (float)".into()),
    }
}
fn math_ceil(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.ceil())),
        _ => Err("math.ceil expects (float)".into()),
    }
}
fn math_abs(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // `i64::MIN.abs()` overflows; a clean fault keeps EV-7 (never panic on input).
        [Value::Int(n)] => n
            .checked_abs()
            .map(Value::Int)
            .ok_or_else(|| "integer overflow in math.abs".to_string()),
        _ => Err("math.abs expects (int)".into()),
    }
}
fn math_min(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok(Value::Int((*a).min(*b))),
        _ => Err("math.min expects (int, int)".into()),
    }
}
fn math_max(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok(Value::Int((*a).max(*b))),
        _ => Err("math.max expects (int, int)".into()),
    }
}

/// The `core.math` registry entries (M3 Track B Wave 2).
fn math_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "core.math",
            name: "sqrt",
            params: vec![Ty::Float],
            ret: Ty::Float,
            eval: math_sqrt,
            php: |a| format!("sqrt({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.math",
            name: "pow",
            params: vec![Ty::Float, Ty::Float],
            ret: Ty::Float,
            eval: math_pow,
            php: |a| format!("pow({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "core.math",
            name: "floor",
            params: vec![Ty::Float],
            ret: Ty::Float,
            eval: math_floor,
            php: |a| format!("floor({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.math",
            name: "ceil",
            params: vec![Ty::Float],
            ret: Ty::Float,
            eval: math_ceil,
            php: |a| format!("ceil({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.math",
            name: "abs",
            params: vec![Ty::Int],
            ret: Ty::Int,
            eval: math_abs,
            php: |a| format!("abs({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.math",
            name: "min",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            eval: math_min,
            php: |a| format!("min({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "core.math",
            name: "max",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            eval: math_max,
            php: |a| format!("max({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

// ---- core.text ----------------------------------------------------------------------------------
// String natives, all concrete-typed. Each erases to a PHP string builtin (D-L9). ASCII-oriented to
// stay byte-identical with PHP: `len` is the *byte* length (PHP `strlen`), and `upper`/`lower` are
// ASCII-case (PHP `strtoupper`/`strtolower`), so multi-byte text could differ between the Rust
// backends and PHP — examples use ASCII. The run↔runvm spine is always byte-identical (both Rust).

fn text_len(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Int(s.len() as i64)),
        _ => Err("text.len expects (string)".into()),
    }
}
fn text_upper(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Str(s.to_ascii_uppercase())),
        _ => Err("text.upper expects (string)".into()),
    }
}
fn text_lower(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Str(s.to_ascii_lowercase())),
        _ => Err("text.lower expects (string)".into()),
    }
}
fn text_trim(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Str(s.trim().to_string())),
        _ => Err("text.trim expects (string)".into()),
    }
}
fn text_contains(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s), Value::Str(sub)] => Ok(Value::Bool(s.contains(sub.as_str()))),
        _ => Err("text.contains expects (string, string)".into()),
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
        _ => Err("text.split expects (string, string)".into()),
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
        _ => Err("text.split_once expects (string, string)".into()),
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
                            "text.join expects List<string>, found element of type {}",
                            other.type_name()
                        ))
                    }
                }
            }
            Ok(Value::Str(parts.join(sep)))
        }
        _ => Err("text.join expects (List<string>, string)".into()),
    }
}
fn text_replace(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s), Value::Str(from), Value::Str(to)] => {
            Ok(Value::Str(s.replace(from.as_str(), to.as_str())))
        }
        _ => Err("text.replace expects (string, string, string)".into()),
    }
}

/// The `core.text` registry entries (M3 Track B Wave 2). NOTE the PHP arg order: `explode`/`implode`
/// take the separator first, and `str_replace` is `(search, replace, subject)` — the `php` closures
/// reorder accordingly so the erasure matches Phorge's `(subject, …)` argument order.
fn text_natives() -> Vec<NativeFn> {
    let s = || Ty::String;
    vec![
        NativeFn {
            module: "core.text",
            name: "len",
            params: vec![s()],
            ret: Ty::Int,
            eval: text_len,
            php: |a| format!("strlen({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.text",
            name: "upper",
            params: vec![s()],
            ret: Ty::String,
            eval: text_upper,
            php: |a| format!("strtoupper({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.text",
            name: "lower",
            params: vec![s()],
            ret: Ty::String,
            eval: text_lower,
            php: |a| format!("strtolower({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.text",
            name: "trim",
            params: vec![s()],
            ret: Ty::String,
            eval: text_trim,
            php: |a| format!("trim({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.text",
            name: "contains",
            params: vec![s(), s()],
            ret: Ty::Bool,
            eval: text_contains,
            php: |a| format!("str_contains({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "core.text",
            name: "split",
            params: vec![s(), s()],
            ret: Ty::List(Box::new(Ty::String)),
            eval: text_split,
            // PHP `explode(separator, string)` — separator first.
            php: |a| format!("explode({}, {})", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "core.text",
            name: "split_once",
            params: vec![s(), s()],
            ret: Ty::List(Box::new(Ty::String)),
            eval: text_split_once,
            // PHP `explode(separator, string, 2)` — separator first; the limit-2 yields [head, tail].
            php: |a| format!("explode({}, {}, 2)", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "core.text",
            name: "join",
            params: vec![Ty::List(Box::new(Ty::String)), s()],
            ret: Ty::String,
            eval: text_join,
            // PHP `implode(glue, array)` — glue first.
            php: |a| format!("implode({}, {})", parg(a, 1), parg(a, 0)),
        },
        NativeFn {
            module: "core.text",
            name: "replace",
            params: vec![s(), s(), s()],
            ret: Ty::String,
            eval: text_replace,
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

// ---- core.file ----------------------------------------------------------------------------------
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
        _ => Err("file.read expects (string)".into()),
    }
}
fn file_exists(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(path)] => Ok(Value::Bool(std::path::Path::new(path).exists())),
        _ => Err("file.exists expects (string)".into()),
    }
}
fn file_write(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(path), Value::Str(contents)] => match std::fs::write(path, contents) {
            Ok(()) => Ok(Value::Unit),
            Err(e) => Err(format!("file.write failed: {e}")),
        },
        _ => Err("file.write expects (string, string)".into()),
    }
}

/// The `core.file` registry entries (M3 Track B Wave 2).
fn file_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "core.file",
            name: "read",
            params: vec![Ty::String],
            ret: Ty::Optional(Box::new(Ty::String)),
            eval: file_read,
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
            module: "core.file",
            name: "exists",
            params: vec![Ty::String],
            ret: Ty::Bool,
            eval: file_exists,
            php: |a| format!("file_exists({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.file",
            name: "write",
            params: vec![Ty::String, Ty::String],
            ret: Ty::Unit,
            eval: file_write,
            php: |a| format!("file_put_contents({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

// ---- core.bytes ---------------------------------------------------------------------------------
// Octet-sequence natives bridging `bytes` ↔ `string` (M6 W0). `to_string` returns `string?` — `null`
// on invalid UTF-8 (composes with S2 `??` / if-let), never a fault. `len` is the BYTE count
// (`strlen`), as is `core.text.len` — the std stays extension-free (no mbstring). `slice` is a total,
// bounds-clamped half-open `[start, end)` (no fault, unlike list `xs[i]`). PHP strings are byte
// arrays, so the erasures are exact.

fn bytes_from_string(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Bytes(std::rc::Rc::new(s.clone().into_bytes()))),
        _ => Err("bytes.from_string expects (string)".into()),
    }
}
fn bytes_to_string(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Invalid UTF-8 → `null` (the `string?` absent case), never a fault.
        [Value::Bytes(b)] => Ok(match std::str::from_utf8(b) {
            Ok(s) => Value::Str(s.to_string()),
            Err(_) => Value::Null,
        }),
        _ => Err("bytes.to_string expects (bytes)".into()),
    }
}
fn bytes_len(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Bytes(b)] => Ok(Value::Int(b.len() as i64)),
        _ => Err("bytes.len expects (bytes)".into()),
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
        _ => Err("bytes.concat expects (bytes, bytes)".into()),
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
        _ => Err("bytes.find expects (bytes, bytes)".into()),
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
        _ => Err("bytes.slice expects (bytes, int, int)".into()),
    }
}

/// The `core.bytes` registry entries (M6 W0).
fn bytes_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "core.bytes",
            name: "from_string",
            params: vec![Ty::String],
            ret: Ty::Bytes,
            eval: bytes_from_string,
            // PHP strings are byte arrays → identity.
            php: |a| parg(a, 0).to_string(),
        },
        NativeFn {
            module: "core.bytes",
            name: "to_string",
            params: vec![Ty::Bytes],
            ret: Ty::Optional(Box::new(Ty::String)),
            eval: bytes_to_string,
            // UTF-8 validity via PCRE (always compiled in), NOT mbstring's mb_check_encoding:
            // the oracle runs `php -n` and minimal/Alpine PHP drop ini-loaded mbstring, so a core
            // primitive must stay extension-free. preg_match returns 1 (valid) / 0 / false → keep
            // the string only on an exact `=== 1`, else null (the `string?` absent case).
            php: |a| format!("(preg_match('//u', {0}) === 1 ? {0} : null)", parg(a, 0)),
        },
        NativeFn {
            module: "core.bytes",
            name: "len",
            params: vec![Ty::Bytes],
            ret: Ty::Int,
            eval: bytes_len,
            // BYTE count (strlen), not character count (mb_strlen).
            php: |a| format!("strlen({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.bytes",
            name: "find",
            params: vec![Ty::Bytes, Ty::Bytes],
            ret: Ty::Optional(Box::new(Ty::Int)),
            eval: bytes_find,
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
            module: "core.bytes",
            name: "concat",
            params: vec![Ty::Bytes, Ty::Bytes],
            ret: Ty::Bytes,
            eval: bytes_concat,
            php: |a| format!("({} . {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "core.bytes",
            name: "slice",
            params: vec![Ty::Bytes, Ty::Int, Ty::Int],
            ret: Ty::Bytes,
            eval: bytes_slice,
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

// ---- core.html ----------------------------------------------------------------------------------
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
        _ => Err("html.text expects (string)".into()),
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
        _ => Err("html.attr expects (string, string)".into()),
    }
}

/// `bool_attr(name)` -> ` name` — a valueless boolean attribute (`disabled`, `checked`, `required`).
fn html_bool_attr(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(name)] => Ok(Value::Str(format!(" {name}"))),
        _ => Err("html.bool_attr expects (string)".into()),
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
        _ => Err("html.el expects (string, List<Attr>, List<Html>)".into()),
    }
}

/// `void_el(tag, attrs)` -> `<tag ATTRS/>` — a self-closing void element (`br`, `hr`, `img`, …).
fn html_void_el(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(tag), Value::List(attrs)] => {
            let a = html_join_fragments(attrs)?;
            Ok(Value::Str(format!("<{tag}{a}/>")))
        }
        _ => Err("html.void_el expects (string, List<Attr>)".into()),
    }
}

/// `concat(parts)` -> the `Html` parts joined with no separator (combine sibling fragments).
fn html_concat(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(parts)] => Ok(Value::Str(html_join_fragments(parts)?)),
        _ => Err("html.concat expects (List<Html>)".into()),
    }
}

fn html_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "core.html",
            name: "text",
            params: vec![Ty::String],
            ret: Ty::Html,
            eval: html_text,
            // Flags PINNED (not PHP's version-varying default) so the output is stable and `php -n`
            // safe; htmlspecialchars is tier-1 (ext/standard, always compiled).
            php: |a| format!("htmlspecialchars({}, ENT_QUOTES, 'UTF-8')", parg(a, 0)),
        },
        NativeFn {
            module: "core.html",
            name: "raw",
            params: vec![Ty::String],
            ret: Ty::Html,
            eval: html_identity,
            php: |a| format!("({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.html",
            name: "render",
            params: vec![Ty::Html],
            ret: Ty::String,
            eval: html_identity,
            php: |a| format!("({})", parg(a, 0)),
        },
        // ---- Wave 2 builders ----
        NativeFn {
            module: "core.html",
            name: "attr",
            params: vec![Ty::String, Ty::String],
            ret: Ty::Attr,
            eval: html_attr,
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
            module: "core.html",
            name: "bool_attr",
            params: vec![Ty::String],
            ret: Ty::Attr,
            eval: html_bool_attr,
            php: |a| format!("' ' . {}", parg(a, 0)),
        },
        NativeFn {
            module: "core.html",
            name: "el",
            params: vec![
                Ty::String,
                Ty::List(Box::new(Ty::Attr)),
                Ty::List(Box::new(Ty::Html)),
            ],
            ret: Ty::Html,
            eval: html_el,
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
            module: "core.html",
            name: "void_el",
            params: vec![Ty::String, Ty::List(Box::new(Ty::Attr))],
            ret: Ty::Html,
            eval: html_void_el,
            php: |a| {
                format!(
                    "(function($t,$a){{return '<' . $t . implode('', $a) . '/>';}})({}, {})",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "core.html",
            name: "concat",
            params: vec![Ty::List(Box::new(Ty::Html))],
            ret: Ty::Html,
            eval: html_concat,
            php: |a| format!("implode('', {})", parg(a, 0)),
        },
    ]
}

/// Construct the native table once. Order is load-bearing: [`CONSOLE_PRINTLN`] pins slot 0; every
/// other native is resolved by `(module, name)` (or leaf+name) at compile time, so appended order is
/// free. Modules are grouped by `*_natives()` builders (one per `core.*` leaf).
fn build() -> Vec<NativeFn> {
    let mut registry = vec![NativeFn {
        module: "core.console",
        name: "println",
        params: vec![Ty::String],
        ret: Ty::Unit,
        eval: console_println,
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
    // Pinned-slot invariant: the constant the compiler bakes into `Op::CallNative` must address the
    // entry it names. Cheap one-time check at first `registry()` access.
    assert_eq!(
        registry[CONSOLE_PRINTLN].module, "core.console",
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
/// `import core.console;` binds the call-site qualifier `console` to module `core.console`. Carried
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
        assert_eq!(r[CONSOLE_PRINTLN].module, "core.console");
        assert_eq!(r[CONSOLE_PRINTLN].name, "println");
    }

    #[test]
    fn index_lookups_resolve_console_println() {
        assert_eq!(index_of("core.console", "println"), Some(CONSOLE_PRINTLN));
        assert_eq!(
            index_of_by_leaf("console", "println"),
            Some(CONSOLE_PRINTLN)
        );
        assert_eq!(index_of("core.console", "nope"), None);
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
        let i = index_of("core.math", "pow").expect("pow registered");
        assert_eq!(index_of_by_leaf("math", "pow"), Some(i));
        assert_eq!(
            (registry()[i].php)(&["2.0".into(), "10.0".into()]),
            "pow(2.0, 10.0)"
        );
        assert_eq!(
            (registry()[index_of("core.math", "min").unwrap()].php)(&["$a".into(), "$b".into()]),
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
            (registry()[index_of("core.text", "split").unwrap()].php)(&[
                "$s".into(),
                "\",\"".into()
            ]),
            "explode(\",\", $s)"
        );
        assert_eq!(
            (registry()[index_of("core.text", "join").unwrap()].php)(&[
                "$xs".into(),
                "\"-\"".into()
            ]),
            "implode(\"-\", $xs)"
        );
        assert_eq!(
            (registry()[index_of("core.text", "replace").unwrap()].php)(&[
                "$s".into(),
                "$a".into(),
                "$b".into()
            ]),
            "str_replace($a, $b, $s)"
        );
        assert_eq!(
            index_of_by_leaf("text", "len"),
            index_of("core.text", "len")
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
            (registry()[index_of("core.html", "text").unwrap()].php)(&["$s".into()]),
            "htmlspecialchars($s, ENT_QUOTES, 'UTF-8')"
        );
        assert_eq!(
            (registry()[index_of("core.html", "raw").unwrap()].php)(&["$s".into()]),
            "($s)"
        );
        assert_eq!(
            index_of_by_leaf("html", "render"),
            index_of("core.html", "render")
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
            (registry()[index_of("core.html", n).unwrap()].php)(&args)
        };
        assert_eq!(
            php("attr", &["$n", "$v"]),
            "' ' . $n . '=\"' . htmlspecialchars($v, ENT_QUOTES, 'UTF-8') . '\"'"
        );
        assert_eq!(php("bool_attr", &["$n"]), "' ' . $n");
        assert_eq!(
            php("el", &["$t", "$a", "$c"]),
            "(function($t,$a,$c){return '<' . $t . implode('', $a) . '>' . implode('', $c) . '</' . $t . '>';})($t, $a, $c)"
        );
        assert_eq!(
            php("void_el", &["$t", "$a"]),
            "(function($t,$a){return '<' . $t . implode('', $a) . '/>';})($t, $a)"
        );
        assert_eq!(php("concat", &["$xs"]), "implode('', $xs)");
        // All builders resolve by both index forms + carry the Attr/Html return types.
        assert_eq!(index_of_by_leaf("html", "el"), index_of("core.html", "el"));
        assert_eq!(
            registry()[index_of("core.html", "attr").unwrap()].ret,
            Ty::Attr
        );
        assert_eq!(
            registry()[index_of("core.html", "el").unwrap()].ret,
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
            crate::native::registry()[index_of("core.file", "read").unwrap()].ret,
            Ty::Optional(Box::new(Ty::String))
        );
        assert_eq!(
            (registry()[index_of("core.file", "read").unwrap()].php)(&["$p".into()]),
            "(($__c = @file_get_contents($p)) === false ? null : $__c)"
        );
        assert_eq!(
            index_of_by_leaf("file", "exists"),
            index_of("core.file", "exists")
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
            path: vec!["core".into(), "console".into()],
            alias: None,
            span: sp,
        }];
        let m = import_map(&items);
        assert_eq!(m.get("console").map(String::as_str), Some("core.console"));

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
