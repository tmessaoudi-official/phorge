use super::*;

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
    assert!(matches!(eval("br", std::slice::from_ref(&empty)), Ok(Value::Str(s)) if s == "<br/>"));
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
