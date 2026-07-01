//! Secure value renderer (M-DX S2) — the single `Value → String` substrate shared by the
//! value-dump-on-fault (S3), assertion failure detail (S4), and the interactive debugger (S5).
//!
//! It is safe **by construction**, three ways:
//!
//! 1. **Secret redaction.** An instance of the injected `Secret<T>` wrapper class renders as
//!    `Secret(<redacted>)` — the renderer never descends into its `value` field. This mirrors the
//!    transpiler's `#[\SensitiveParameter]` and the type system's non-printability guarantee: a
//!    secret's plaintext cannot leak through a dump/debugger the way it cannot leak through `print`.
//! 2. **Bounded.** Depth, per-collection element count, and total byte length are all capped
//!    ([`RenderCaps`]); anything beyond is truncated with `…`. A hostile or merely huge value can
//!    never produce an unbounded dump.
//! 3. **Deterministic.** Insertion-ordered `Map`/`Set` (already `Rc<Vec<…>>`) and slot-ordered
//!    instance fields render in a stable order; no addresses, `Rc` counts, or hash iteration order
//!    ever appear. The output is reproducible, so it can be golden-tested.
//!
//! It lives **outside** the correctness spine: nothing here is transpiled, and its output goes to a
//! side-channel (stderr), never a program's stdout — so it can never change `run ≡ runvm ≡ PHP`.

use crate::value::{fmt_decimal, HKey, Value};

/// The injected opaque-wrapper class whose contents are redacted (see `cli::SECRET_PRELUDE`).
const SECRET_CLASS: &str = "Secret";

/// Truncation bounds for [`render`]. Defaults are generous enough for a readable post-mortem yet
/// bounded so no single value floods the terminal.
#[derive(Debug, Clone, Copy)]
pub struct RenderCaps {
    /// Maximum nesting depth (a list-in-map-in-instance…). Beyond this, a composite renders as `…`.
    pub max_depth: usize,
    /// Maximum elements/fields rendered per collection; the rest collapse to `… (+N more)`.
    pub max_elements: usize,
    /// Maximum bytes of a rendered string/bytes payload before it is truncated with `…`.
    pub max_scalar_bytes: usize,
}

impl Default for RenderCaps {
    fn default() -> Self {
        RenderCaps {
            max_depth: 6,
            max_elements: 32,
            max_scalar_bytes: 256,
        }
    }
}

/// Render `value` to a secure, bounded, deterministic string using the default [`RenderCaps`].
#[must_use]
pub fn render(value: &Value) -> String {
    render_with(value, &RenderCaps::default())
}

/// Render `value` with explicit caps.
#[must_use]
pub fn render_with(value: &Value, caps: &RenderCaps) -> String {
    let mut out = String::new();
    render_into(value, caps, 0, &mut out);
    out
}

fn render_into(value: &Value, caps: &RenderCaps, depth: usize, out: &mut String) {
    match value {
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Float(f) => out.push_str(&format!("{f}")),
        Value::Decimal { unscaled, scale } => out.push_str(&fmt_decimal(*unscaled, *scale)),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Str(s) => push_quoted_str(s, caps, out),
        Value::Bytes(b) => push_bytes(b, caps, out),
        Value::Unit => out.push_str("unit"),
        Value::Null => out.push_str("null"),
        Value::List(xs) => push_seq(out, '[', ']', xs.len(), caps, depth, |i, out| {
            render_into(&xs[i], caps, depth + 1, out);
        }),
        Value::Map(pairs) => push_seq(out, '[', ']', pairs.len(), caps, depth, |i, out| {
            push_hkey(&pairs[i].0, caps, out);
            out.push_str(" => ");
            render_into(&pairs[i].1, caps, depth + 1, out);
        }),
        Value::Set(keys) => {
            out.push_str("Set");
            push_seq(out, '{', '}', keys.len(), caps, depth, |i, out| {
                push_hkey(&keys[i], caps, out);
            });
        }
        Value::Instance(inst) => {
            // Secret redaction — never descend into a secret's fields.
            if inst.class == SECRET_CLASS {
                out.push_str("Secret(<redacted>)");
                return;
            }
            out.push_str(&inst.class);
            out.push(' ');
            let names = inst.layout.names();
            let fields = inst.fields.borrow();
            push_seq(out, '{', '}', names.len(), caps, depth, |i, out| {
                out.push_str(&names[i]);
                out.push_str(": ");
                match &fields[i] {
                    Some(v) => render_into(v, caps, depth + 1, out),
                    None => out.push_str("<unset>"),
                }
            });
        }
        Value::Enum(e) => {
            out.push_str(&e.variant);
            if !e.payload.is_empty() {
                push_seq(out, '(', ')', e.payload.len(), caps, depth, |i, out| {
                    render_into(&e.payload[i], caps, depth + 1, out);
                });
            }
        }
        // Opaque handles — never carry inspectable structure, and rendering an address/id would
        // break determinism. A stable type tag is the whole safe surface.
        Value::Closure(_) => out.push_str("<function>"),
        Value::Channel(_, _) => out.push_str("<channel>"),
        Value::Task(_) => out.push_str("<task>"),
    }
}

/// Render a delimited sequence with element + depth caps. `render_elem(i, out)` renders element `i`.
fn push_seq(
    out: &mut String,
    open: char,
    close: char,
    len: usize,
    caps: &RenderCaps,
    depth: usize,
    mut render_elem: impl FnMut(usize, &mut String),
) {
    if depth >= caps.max_depth {
        out.push(open);
        out.push('…');
        out.push(close);
        return;
    }
    out.push(open);
    let shown = len.min(caps.max_elements);
    for i in 0..shown {
        if i > 0 {
            out.push_str(", ");
        }
        render_elem(i, out);
    }
    if len > shown {
        out.push_str(&format!(", … (+{} more)", len - shown));
    }
    out.push(close);
}

fn push_hkey(k: &HKey, caps: &RenderCaps, out: &mut String) {
    match k {
        HKey::Int(n) => out.push_str(&n.to_string()),
        HKey::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        HKey::Str(s) => push_quoted_str(s, caps, out),
    }
}

/// A double-quoted string with a byte cap. Control characters are escaped so the dump stays on one
/// line and can't inject terminal escapes.
fn push_quoted_str(s: &str, caps: &RenderCaps, out: &mut String) {
    out.push('"');
    let mut written = 0usize;
    for c in s.chars() {
        if written >= caps.max_scalar_bytes {
            out.push('…');
            break;
        }
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{{{:04x}}}", c as u32)),
            c => out.push(c),
        }
        written += c.len_utf8();
    }
    out.push('"');
}

/// A `bytes` literal `b"\xHH…"` with a byte cap (shows the length so a truncated dump is unambiguous).
fn push_bytes(b: &[u8], caps: &RenderCaps, out: &mut String) {
    out.push_str(&format!("b[{}]", b.len()));
    out.push('"');
    let shown = b.len().min(caps.max_scalar_bytes);
    for &byte in &b[..shown] {
        out.push_str(&format!("\\x{byte:02x}"));
    }
    if b.len() > shown {
        out.push('…');
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{ClassLayout, EnumVal, Instance};
    use std::rc::Rc;

    fn instance(class: &str, fields: &[(&str, Value)]) -> Value {
        let names: Vec<String> = fields.iter().map(|(n, _)| (*n).to_string()).collect();
        let layout = ClassLayout::new(names);
        let inst = Instance::new(class.to_string(), layout);
        for (n, v) in fields {
            inst.set_field(n, v.clone());
        }
        Value::Instance(Rc::new(inst))
    }

    #[test]
    fn scalars_render_readably() {
        assert_eq!(render(&Value::Int(42)), "42");
        assert_eq!(render(&Value::Bool(true)), "true");
        assert_eq!(render(&Value::Null), "null");
        assert_eq!(render(&Value::Unit), "unit");
        assert_eq!(render(&Value::Str("hi".into())), "\"hi\"");
        assert_eq!(
            render(&Value::Decimal {
                unscaled: 1999,
                scale: 2
            }),
            "19.99"
        );
    }

    #[test]
    fn strings_escape_control_chars_and_quotes() {
        assert_eq!(render(&Value::Str("a\"b\nc\t".into())), "\"a\\\"b\\nc\\t\"");
    }

    #[test]
    fn collections_are_insertion_ordered_and_deterministic() {
        let list = Value::List(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        assert_eq!(render(&list), "[1, 2, 3]");
        let map = Value::Map(Rc::new(vec![
            (HKey::Str("b".into()), Value::Int(2)),
            (HKey::Str("a".into()), Value::Int(1)),
        ]));
        // Insertion order preserved (b before a), not sorted — determinism is the value's own order.
        assert_eq!(render(&map), "[\"b\" => 2, \"a\" => 1]");
        let set = Value::Set(Rc::new(vec![HKey::Int(3), HKey::Int(1)]));
        assert_eq!(render(&set), "Set{3, 1}");
    }

    #[test]
    fn instance_and_enum_render_structurally() {
        let inst = instance("Point", &[("x", Value::Int(1)), ("y", Value::Int(2))]);
        assert_eq!(render(&inst), "Point {x: 1, y: 2}");
        let e = Value::Enum(Rc::new(EnumVal {
            ty: "Shape".into(),
            variant: "Circle".into(),
            payload: vec![Value::Float(2.0)],
        }));
        assert_eq!(render(&e), "Circle(2)");
    }

    #[test]
    fn secret_instance_is_redacted_not_descended() {
        // A Secret wrapper's private `value` field must never appear in the rendered output.
        let secret = instance("Secret", &[("value", Value::Str("hunter2".into()))]);
        let rendered = render(&secret);
        assert_eq!(rendered, "Secret(<redacted>)");
        assert!(
            !rendered.contains("hunter2"),
            "secret plaintext leaked: {rendered}"
        );
    }

    #[test]
    fn nested_secret_is_still_redacted() {
        // A secret nested inside a list/instance is redacted where it sits.
        let secret = instance("Secret", &[("value", Value::Str("pw".into()))]);
        let list = Value::List(Rc::new(vec![Value::Int(1), secret]));
        let rendered = render(&list);
        assert_eq!(rendered, "[1, Secret(<redacted>)]");
        assert!(!rendered.contains("pw"));
    }

    #[test]
    fn depth_cap_truncates_deep_nesting() {
        let caps = RenderCaps {
            max_depth: 2,
            max_elements: 32,
            max_scalar_bytes: 256,
        };
        // depth 0 [ depth 1 [ depth 2 -> truncated ] ]
        let deep = Value::List(Rc::new(vec![Value::List(Rc::new(vec![Value::List(
            Rc::new(vec![Value::Int(9)]),
        )]))]));
        assert_eq!(render_with(&deep, &caps), "[[[…]]]");
    }

    #[test]
    fn element_cap_collapses_the_tail() {
        let caps = RenderCaps {
            max_depth: 6,
            max_elements: 2,
            max_scalar_bytes: 256,
        };
        let list = Value::List(Rc::new((0..5).map(Value::Int).collect()));
        assert_eq!(render_with(&list, &caps), "[0, 1, … (+3 more)]");
    }

    #[test]
    fn scalar_byte_cap_truncates_long_strings() {
        let caps = RenderCaps {
            max_depth: 6,
            max_elements: 32,
            max_scalar_bytes: 4,
        };
        let s = Value::Str("abcdefgh".into());
        assert_eq!(render_with(&s, &caps), "\"abcd…\"");
    }

    #[test]
    fn bytes_show_length_and_hex() {
        let b = Value::Bytes(Rc::new(vec![0x00, 0xff, 0x41]));
        assert_eq!(render(&b), "b[3]\"\\x00\\xff\\x41\"");
    }
}
