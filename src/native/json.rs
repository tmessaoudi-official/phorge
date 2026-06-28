//! `Core.Json` — JSON parse / stringify over a compiler-injected `Json` enum value model
//! (`docs/specs/2026-06-26-core-json-design.md`). The `Json` enum is injected by
//! `cli::inject_json_prelude` when a program imports `Core.Json`, so these natives can construct +
//! receive ordinary `Value::Enum { ty: "Json", … }` values.
//!
//! The one `eval` body per native is shared by both Rust backends (the parity guarantee). The PHP
//! transpile of each native delegates to a `__phorj_json_*` helper (`transpile/program.rs`) that
//! walks the same enum hierarchy — kept byte-identical with the kernels here: floats render via the
//! shortest-round-trip positional formatter (`format!("{}")` / `__phorj_float`, NOT json's
//! scientific notation), strings escape to match PHP `json_encode`'s default, objects keep Map
//! insertion order, and number decoding distinguishes `Int` from `Float` exactly as `json_decode`.

use super::*;
use crate::types::Ty;
use crate::value::{build_map, EnumVal, HKey, Value};
use std::rc::Rc;

/// Build a `Json` enum node. `variant` is the Phorj variant name (`Null`/`Bool`/`Int`/`Float`/
/// `Str`/`Arr`/`Obj`); the transpiler mangles reserved ones to PHP class names, the backends use this
/// string directly.
fn jnode(variant: &str, payload: Vec<Value>) -> Value {
    Value::Enum(Rc::new(EnumVal {
        ty: "Json".to_string(),
        variant: variant.to_string(),
        payload,
    }))
}

// ---- encode (stringify) -------------------------------------------------------------------------

fn json_stringify(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [j] => {
            let mut s = String::new();
            encode(j, &mut s)?;
            Ok(Value::Str(s))
        }
        _ => Err("Json.stringify expects (Json)".into()),
    }
}

fn json_stringify_pretty(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [j] => {
            let mut s = String::new();
            encode_pretty(j, 0, &mut s)?;
            Ok(Value::Str(s))
        }
        _ => Err("Json.stringifyPretty expects (Json)".into()),
    }
}

fn as_json(v: &Value) -> Result<&EnumVal, String> {
    match v {
        Value::Enum(e) if e.ty == "Json" => Ok(e),
        _ => Err(format!("Json value expected, got {}", v.type_name())),
    }
}

/// An object key (typed `Map<string, Json>`, so always a string `HKey`).
fn key_str(k: &HKey) -> Result<&str, String> {
    match k {
        HKey::Str(s) => Ok(s),
        _ => Err("Json object key must be a string".into()),
    }
}

/// Compact encoding — matches `__phorj_json_encode` byte-for-byte.
fn encode(v: &Value, out: &mut String) -> Result<(), String> {
    let e = as_json(v)?;
    match (e.variant.as_str(), &e.payload[..]) {
        ("Null", []) => out.push_str("null"),
        ("Bool", [Value::Bool(b)]) => out.push_str(if *b { "true" } else { "false" }),
        ("Int", [Value::Int(n)]) => out.push_str(&n.to_string()),
        ("Float", [Value::Float(f)]) => out.push_str(&format!("{f}")),
        ("Str", [Value::Str(s)]) => encode_str(s, out),
        ("Arr", [Value::List(xs)]) => {
            out.push('[');
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode(x, out)?;
            }
            out.push(']');
        }
        ("Obj", [Value::Map(m)]) => {
            out.push('{');
            for (i, (k, val)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode_str(key_str(k)?, out);
                out.push(':');
                encode(val, out)?;
            }
            out.push('}');
        }
        _ => return Err(format!("malformed Json node `{}`", e.variant)),
    }
    Ok(())
}

/// Pretty encoding (`JSON_PRETTY_PRINT` layout: 4-space indent, `": "` after a key, empty `[]`/`{}`
/// inline). `indent` is the current leading-space count. Matches `__phorj_json_pretty`.
fn encode_pretty(v: &Value, indent: usize, out: &mut String) -> Result<(), String> {
    let e = as_json(v)?;
    match (e.variant.as_str(), &e.payload[..]) {
        ("Arr", [Value::List(xs)]) if !xs.is_empty() => {
            let inner = indent + 4;
            out.push_str("[\n");
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                out.push_str(&" ".repeat(inner));
                encode_pretty(x, inner, out)?;
            }
            out.push('\n');
            out.push_str(&" ".repeat(indent));
            out.push(']');
        }
        ("Obj", [Value::Map(m)]) if !m.is_empty() => {
            let inner = indent + 4;
            out.push_str("{\n");
            for (i, (k, val)) in m.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                out.push_str(&" ".repeat(inner));
                encode_str(key_str(k)?, out);
                out.push_str(": ");
                encode_pretty(val, inner, out)?;
            }
            out.push('\n');
            out.push_str(&" ".repeat(indent));
            out.push('}');
        }
        // Scalars and empty containers render compactly (one line) — matches PHP.
        _ => encode(v, out)?,
    }
    Ok(())
}

/// JSON string escaping matching PHP `json_encode`'s default: escapes `"` `\` `/`, the named control
/// escapes, other control chars (`<0x20`) as `\u00xx`, and every non-ASCII (`>0x7f`) code point as
/// `\uxxxx` (a surrogate pair for `>0xFFFF`). Lowercase hex (PHP's convention).
fn encode_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) > 0x7f => {
                let cp = c as u32;
                if cp > 0xFFFF {
                    let v = cp - 0x10000;
                    let hi = 0xD800 + (v >> 10);
                    let lo = 0xDC00 + (v & 0x3FF);
                    out.push_str(&format!("\\u{hi:04x}\\u{lo:04x}"));
                } else {
                    out.push_str(&format!("\\u{cp:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// ---- decode (parse) -----------------------------------------------------------------------------

fn json_parse(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // `None` (malformed) is `Value::Null`; a present value is the `Json` enum directly.
        [Value::Str(s)] => Ok(parse_json(s).unwrap_or(Value::Null)),
        _ => Err("Json.parse expects (string)".into()),
    }
}

/// Std-only recursive-descent JSON parser → a `Json` enum value, or `None` on any syntax error
/// (including trailing non-whitespace). Mirrors `json_decode`: `{}`≠`[]`, integers without a
/// `.`/`e` are `Int` (overflow falls back to `Float`), duplicate object keys keep first position /
/// last value (via `build_map`).
fn parse_json(s: &str) -> Option<Value> {
    let chars: Vec<char> = s.chars().collect();
    let mut p = JParser { c: &chars, i: 0 };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.c.len() {
        return None; // trailing junk
    }
    Some(v)
}

struct JParser<'a> {
    c: &'a [char],
    i: usize,
}

impl JParser<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.i += 1;
        }
        c
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.i += 1;
        }
    }

    fn value(&mut self) -> Option<Value> {
        self.ws();
        match self.peek()? {
            'n' => self.lit("null", jnode("Null", vec![])),
            't' => self.lit("true", jnode("Bool", vec![Value::Bool(true)])),
            'f' => self.lit("false", jnode("Bool", vec![Value::Bool(false)])),
            '"' => {
                let s = self.string()?;
                Some(jnode("Str", vec![Value::Str(s)]))
            }
            '[' => self.array(),
            '{' => self.object(),
            '-' | '0'..='9' => self.number(),
            _ => None,
        }
    }

    fn lit(&mut self, kw: &str, v: Value) -> Option<Value> {
        for ch in kw.chars() {
            if self.bump()? != ch {
                return None;
            }
        }
        Some(v)
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.i;
        if self.peek() == Some('-') {
            self.i += 1;
        }
        match self.peek()? {
            '0' => self.i += 1, // a leading 0 must stand alone (no `01`)
            '1'..='9' => {
                while matches!(self.peek(), Some('0'..='9')) {
                    self.i += 1;
                }
            }
            _ => return None,
        }
        let mut is_float = false;
        if self.peek() == Some('.') {
            is_float = true;
            self.i += 1;
            if !matches!(self.peek(), Some('0'..='9')) {
                return None;
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            self.i += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.i += 1;
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                return None;
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.i += 1;
            }
        }
        let tok: String = self.c[start..self.i].iter().collect();
        if is_float {
            Some(jnode("Float", vec![Value::Float(tok.parse::<f64>().ok()?)]))
        } else {
            // Integer; an i64 overflow falls back to Float, matching `json_decode`.
            match tok.parse::<i64>() {
                Ok(n) => Some(jnode("Int", vec![Value::Int(n)])),
                Err(_) => Some(jnode("Float", vec![Value::Float(tok.parse::<f64>().ok()?)])),
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        if self.bump() != Some('"') {
            return None;
        }
        let mut s = String::new();
        loop {
            match self.bump()? {
                '"' => return Some(s),
                '\\' => match self.bump()? {
                    '"' => s.push('"'),
                    '\\' => s.push('\\'),
                    '/' => s.push('/'),
                    'b' => s.push('\u{08}'),
                    'f' => s.push('\u{0c}'),
                    'n' => s.push('\n'),
                    'r' => s.push('\r'),
                    't' => s.push('\t'),
                    'u' => s.push(self.unicode_escape()?),
                    _ => return None,
                },
                c if (c as u32) < 0x20 => return None, // a raw control char is invalid in a JSON string
                c => s.push(c),
            }
        }
    }

    /// Read 4 hex digits.
    fn hex4(&mut self) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..4 {
            v = v * 16 + self.bump()?.to_digit(16)?;
        }
        Some(v)
    }

    /// A `\uXXXX` escape (the `\u` already consumed), combining a surrogate pair when present. A lone
    /// surrogate is invalid (`None`), matching `json_decode`'s strict default.
    fn unicode_escape(&mut self) -> Option<char> {
        let u = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&u) {
            if self.bump()? != '\\' || self.bump()? != 'u' {
                return None;
            }
            let lo = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&lo) {
                return None;
            }
            let cp = 0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00);
            char::from_u32(cp)
        } else if (0xDC00..=0xDFFF).contains(&u) {
            None
        } else {
            char::from_u32(u)
        }
    }

    fn array(&mut self) -> Option<Value> {
        self.bump(); // '['
        self.ws();
        let mut xs = Vec::new();
        if self.peek() == Some(']') {
            self.bump();
            return Some(jnode("Arr", vec![Value::List(Rc::new(xs))]));
        }
        loop {
            xs.push(self.value()?);
            self.ws();
            match self.bump()? {
                ',' => self.ws(),
                ']' => return Some(jnode("Arr", vec![Value::List(Rc::new(xs))])),
                _ => return None,
            }
        }
    }

    fn object(&mut self) -> Option<Value> {
        self.bump(); // '{'
        self.ws();
        let mut pairs: Vec<(Value, Value)> = Vec::new();
        if self.peek() == Some('}') {
            self.bump();
            return self.make_obj(pairs);
        }
        loop {
            self.ws();
            if self.peek() != Some('"') {
                return None;
            }
            let key = self.string()?;
            self.ws();
            if self.bump()? != ':' {
                return None;
            }
            let val = self.value()?;
            pairs.push((Value::Str(key), val));
            self.ws();
            match self.bump()? {
                ',' => {}
                '}' => return self.make_obj(pairs),
                _ => return None,
            }
        }
    }

    fn make_obj(&self, pairs: Vec<(Value, Value)>) -> Option<Value> {
        // String keys ⇒ `build_map` never rejects; it dedups first-position/last-value (PHP assoc).
        let entries = build_map(pairs).ok()?;
        Some(jnode("Obj", vec![Value::Map(Rc::new(entries))]))
    }
}

// ---- registry -----------------------------------------------------------------------------------

/// The `Core.Json` registry entries. `Json` is the compiler-injected enum (`cli::inject_json_prelude`)
/// — referenced here as a bare `Ty::Named`; the type resolves because a *call* to one of these natives
/// requires `import Core.Json;`, which triggers the injection before the checker runs.
pub(crate) fn json_natives() -> Vec<NativeFn> {
    let json = || Ty::Named("Json".to_string(), vec![]);
    vec![
        NativeFn {
            module: "Core.Json",
            name: "parse",
            params: vec![Ty::String],
            ret: Ty::Optional(Box::new(json())),
            pure: true,
            eval: NativeEval::Pure(json_parse),
            php: |a| format!("__phorj_json_decode({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Json",
            name: "stringify",
            params: vec![json()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(json_stringify),
            php: |a| format!("__phorj_json_encode({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Json",
            name: "stringifyPretty",
            params: vec![json()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(json_stringify_pretty),
            php: |a| format!("__phorj_json_encode_pretty({})", parg(a, 0)),
        },
    ]
}

#[cfg(test)]
#[path = "json_tests.rs"]
mod tests;
