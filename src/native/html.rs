use super::*;
use crate::types::Ty;
use crate::value::Value;

/// HTML-escape `s` exactly as PHP's `htmlspecialchars($s, ENT_QUOTES, 'UTF-8')` does for valid UTF-8
/// (Phorj strings are always valid UTF-8, so the invalid-byte/ENT_SUBSTITUTE path is unreachable).
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
            pure: true,
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
            pure: true,
            eval: NativeEval::Pure(ev),
            php,
        }
    }};
}

pub(crate) fn html_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Html",
            name: "text",
            params: vec![Ty::String],
            ret: Ty::Html,
            pure: true,
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
            pure: true,
            eval: NativeEval::Pure(html_identity),
            php: |a| format!("({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Html",
            name: "render",
            params: vec![Ty::Html],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(html_identity),
            php: |a| format!("({})", parg(a, 0)),
        },
        // ---- Wave 2 builders ----
        NativeFn {
            module: "Core.Html",
            name: "attr",
            params: vec![Ty::String, Ty::String],
            ret: Ty::Attr,
            pure: true,
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
            pure: true,
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
            pure: true,
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
            pure: true,
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
            pure: true,
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

#[cfg(test)]
#[path = "html_tests.rs"]
mod tests;
