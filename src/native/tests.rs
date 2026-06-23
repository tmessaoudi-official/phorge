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
    assert!(matches!(math_sqrt(&[Value::Float(16.0)], &mut out), Ok(Value::Float(x)) if x == 4.0));
    assert!(
        matches!(math_pow(&[Value::Float(2.0), Value::Float(10.0)], &mut out), Ok(Value::Float(x)) if x == 1024.0)
    );
    assert!(matches!(math_floor(&[Value::Float(3.7)], &mut out), Ok(Value::Float(x)) if x == 3.0));
    assert!(matches!(math_ceil(&[Value::Float(3.2)], &mut out), Ok(Value::Float(x)) if x == 4.0));
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
        (registry()[index_of("Core.Text", "split").unwrap()].php)(&["$s".into(), "\",\"".into()]),
        "explode(\",\", $s)"
    );
    assert_eq!(
        (registry()[index_of("Core.Text", "join").unwrap()].php)(&["$xs".into(), "\"-\"".into()]),
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
        (registry()[index_of("Core.List", "filter").unwrap()].php)(&["$xs".into(), "$f".into()]),
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
        type_only: false,
        span: sp,
    }];
    let m = import_map(&items);
    assert_eq!(m.get("Console").map(String::as_str), Some("Core.Console"));

    // An alias overrides the bound qualifier (M5 S2c).
    let aliased = vec![Item::Import {
        path: vec!["acme".into(), "util".into()],
        alias: Some("u".into()),
        type_only: false,
        span: sp,
    }];
    let m = import_map(&aliased);
    assert_eq!(m.get("u").map(String::as_str), Some("acme.util"));
    assert!(!m.contains_key("util"), "alias replaces the leaf qualifier");
}
