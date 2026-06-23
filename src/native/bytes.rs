use super::*;
use crate::types::Ty;
use crate::value::Value;

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
pub(crate) fn bytes_natives() -> Vec<NativeFn> {
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
