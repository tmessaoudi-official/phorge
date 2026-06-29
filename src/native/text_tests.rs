use super::*;

#[test]
fn text_capitalize_ascii_ucfirst() {
    let mut o = String::new();
    let mut cap = |s: &str| match text_capitalize(&[Value::Str(s.into())], &mut o).unwrap() {
        Value::Str(r) => r.to_string(),
        other => panic!("capitalize returned {other:?}"),
    };
    assert_eq!(cap("hello"), "Hello");
    assert_eq!(cap("Hello"), "Hello"); // already uppercase → unchanged
    assert_eq!(cap(""), ""); // empty → empty
    assert_eq!(cap("123"), "123"); // non-letter first byte → unchanged
                                   // PHP mapping is ucfirst.
    assert_eq!(
        (registry()[index_of("Core.Text", "capitalize").unwrap()].php)(&["$s".into()]),
        "ucfirst($s)"
    );
}

#[test]
fn text_lines_splits_on_newline() {
    let mut o = String::new();
    let collect = |v: Value| match v {
        Value::List(xs) => xs
            .iter()
            .map(|e| match e {
                Value::Str(s) => s.to_string(),
                _ => "?".into(),
            })
            .collect::<Vec<_>>(),
        other => panic!("lines returned {other:?}"),
    };
    // Three lines, no trailing newline.
    assert_eq!(
        collect(text_lines(&[Value::Str("a\nbb\nccc".into())], &mut o).unwrap()),
        vec!["a", "bb", "ccc"]
    );
    // Trailing newline yields a trailing empty string (explode semantics).
    assert_eq!(
        collect(text_lines(&[Value::Str("x\n".into())], &mut o).unwrap()),
        vec!["x", ""]
    );
    // Empty string → one empty line.
    assert_eq!(
        collect(text_lines(&[Value::Str("".into())], &mut o).unwrap()),
        vec![""]
    );
    // PHP mapping is explode on a literal "\n".
    assert_eq!(
        (registry()[index_of("Core.Text", "lines").unwrap()].php)(&["$s".into()]),
        "explode(\"\\n\", $s)"
    );
}

#[test]
fn text_parse_int_matches_rust_i64_fromstr() {
    let p = |s: &str| {
        let mut o = String::new();
        text_parse_int(&[Value::Str(s.into())], &mut o).unwrap()
    };
    // Valid integers → Some (the value itself; an optional's present case is the bare value).
    assert!(matches!(p("123"), Value::Int(123)));
    assert!(matches!(p("-7"), Value::Int(-7)));
    assert!(matches!(p("+5"), Value::Int(5))); // leading + accepted (Rust i64 FromStr)
    assert!(matches!(p("007"), Value::Int(7))); // leading zeros accepted
    assert!(matches!(p("0"), Value::Int(0)));
    // Invalid → None (Value::Null).
    assert!(matches!(p(""), Value::Null));
    assert!(matches!(p("abc"), Value::Null));
    assert!(matches!(p("12.5"), Value::Null));
    assert!(matches!(p("12abc"), Value::Null));
    assert!(matches!(p(" 5"), Value::Null)); // surrounding whitespace rejected
    assert!(matches!(p("0x10"), Value::Null));
    assert!(matches!(p("99999999999999999999"), Value::Null)); // i64 overflow → None
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
        index_of_by_leaf("Text", "length"),
        index_of("Core.Text", "length")
    );
}

#[test]
fn text_p3_byte_safe_natives() {
    let mut o = String::new();
    // startsWith / endsWith — byte-level prefix/suffix tests (PHP str_starts_with/str_ends_with).
    assert!(matches!(
        text_starts_with(
            &[Value::Str("hello".into()), Value::Str("he".into())],
            &mut o
        ),
        Ok(Value::Bool(true))
    ));
    assert!(matches!(
        text_starts_with(
            &[Value::Str("hello".into()), Value::Str("lo".into())],
            &mut o
        ),
        Ok(Value::Bool(false))
    ));
    assert!(matches!(
        text_ends_with(
            &[Value::Str("hello".into()), Value::Str("lo".into())],
            &mut o
        ),
        Ok(Value::Bool(true))
    ));
    // repeat — n copies; n == 0 is the empty string.
    assert!(
        matches!(text_repeat(&[Value::Str("ab".into()), Value::Int(3)], &mut o), Ok(Value::Str(s)) if s == "ababab")
    );
    assert!(
        matches!(text_repeat(&[Value::Str("ab".into()), Value::Int(0)], &mut o), Ok(Value::Str(s)) if s.is_empty())
    );
    // EV-7: a negative count faults cleanly (never panics / over-allocates).
    assert!(text_repeat(&[Value::Str("ab".into()), Value::Int(-1)], &mut o).is_err());
    // PHP erasure to the same-named builtins.
    assert_eq!(
        (registry()[index_of("Core.Text", "startsWith").unwrap()].php)(&["$s".into(), "$p".into()]),
        "str_starts_with($s, $p)"
    );
    assert_eq!(
        (registry()[index_of("Core.Text", "endsWith").unwrap()].php)(&["$s".into(), "$p".into()]),
        "str_ends_with($s, $p)"
    );
    assert_eq!(
        (registry()[index_of("Core.Text", "repeat").unwrap()].php)(&["$s".into(), "$n".into()]),
        "str_repeat($s, $n)"
    );
}

#[test]
fn text_parse_float_strict_and_permissive() {
    let p = |s: &str, permissive: bool| {
        let mut o = String::new();
        text_parse_float(&[Value::Str(s.into()), Value::Bool(permissive)], &mut o).unwrap()
    };
    // strict: accepts integer/fraction/exponent forms; rejects leading/trailing dot, inf/nan, ws.
    assert!(matches!(p("1.5", false), Value::Float(f) if (f - 1.5).abs() < 1e-12));
    assert!(matches!(p("-2.5e3", false), Value::Float(f) if (f + 2500.0).abs() < 1e-9));
    assert!(matches!(p("42", false), Value::Float(f) if (f - 42.0).abs() < 1e-12));
    assert!(matches!(p("+5", false), Value::Float(f) if (f - 5.0).abs() < 1e-12));
    assert!(matches!(p(".5", false), Value::Null)); // strict rejects leading dot
    assert!(matches!(p("5.", false), Value::Null)); // strict rejects trailing dot
    assert!(matches!(p("inf", false), Value::Null)); // byte-identity: never inf
    assert!(matches!(p("nan", false), Value::Null)); // byte-identity: never nan
    assert!(matches!(p(" 1.5", false), Value::Null)); // surrounding whitespace
    assert!(matches!(p("1.2.3", false), Value::Null));
    assert!(matches!(p("", false), Value::Null));
    // permissive: additionally accepts a lone leading/trailing dot; still rejects inf/nan.
    assert!(matches!(p(".5", true), Value::Float(f) if (f - 0.5).abs() < 1e-12));
    assert!(matches!(p("5.", true), Value::Float(f) if (f - 5.0).abs() < 1e-12));
    assert!(matches!(p("inf", true), Value::Null));
    assert!(matches!(p(".", true), Value::Null));
    // PHP erasure + signature.
    let reg = &registry()[index_of("Core.Text", "parseFloat").unwrap()];
    assert_eq!(
        (reg.php)(&["$s".into(), "$p".into()]),
        "__phorj_parse_float($s, $p)"
    );
    assert_eq!(reg.ret, Ty::Optional(Box::new(Ty::Float)));
    assert_eq!(reg.params, vec![Ty::String, Ty::Bool]);
    // the permissive flag defaults to strict (M4 default parameters).
    assert!(matches!(
        crate::native::native_defaults("Core.Text", "parseFloat"),
        [crate::native::NativeDefault::Bool(false)]
    ));
}

#[test]
fn text_parse_bool_is_strict_no_truthiness() {
    // M4 `string as bool` — ONLY "true"/"false" parse; never PHP truthiness ("0"/""/"false"-as-true).
    let p = |s: &str| {
        let mut o = String::new();
        text_parse_bool(&[Value::Str(s.into())], &mut o).unwrap()
    };
    assert!(matches!(p("true"), Value::Bool(true)));
    assert!(matches!(p("false"), Value::Bool(false)));
    assert!(matches!(p("1"), Value::Null)); // NOT PHP-truthy
    assert!(matches!(p("0"), Value::Null));
    assert!(matches!(p(""), Value::Null));
    assert!(matches!(p("True"), Value::Null)); // case-sensitive
    assert!(matches!(p("yes"), Value::Null));
}

#[test]
fn text_breadth_pad_indexof_substring() {
    let mut o = String::new();
    let s = |v: &str| Value::Str(v.into());
    let str_of = |r: Result<Value, String>| match r.unwrap() {
        Value::Str(t) => t,
        other => panic!("non-string {other:?}"),
    };
    // padLeft / padRight (PHP str_pad): pad to width, repeating+truncating the pad; already-wide → no-op.
    assert_eq!(
        str_of(text_pad_left(&[s("7"), Value::Int(3), s("0")], &mut o)),
        "007"
    );
    assert_eq!(
        str_of(text_pad_right(&[s("7"), Value::Int(3), s("0")], &mut o)),
        "700"
    );
    assert_eq!(
        str_of(text_pad_left(&[s("ab"), Value::Int(5), s("xy")], &mut o)),
        "xyxab"
    );
    assert_eq!(
        str_of(text_pad_left(
            &[s("toolong"), Value::Int(3), s(" ")],
            &mut o
        )),
        "toolong"
    );
    // empty pad faults cleanly (PHP ValueError), never panics.
    assert!(text_pad_left(&[s("x"), Value::Int(3), s("")], &mut o).is_err());
    // indexOf (PHP strpos): first byte offset, else null; empty needle → 0.
    assert!(matches!(
        text_index_of(&[s("hello"), s("ll")], &mut o).unwrap(),
        Value::Int(2)
    ));
    assert!(matches!(
        text_index_of(&[s("hello"), s("z")], &mut o).unwrap(),
        Value::Null
    ));
    assert!(matches!(
        text_index_of(&[s("hello"), s("")], &mut o).unwrap(),
        Value::Int(0)
    ));
    // substring (PHP substr): positive, negative, out-of-range.
    assert_eq!(
        str_of(text_substring(
            &[s("hello"), Value::Int(1), Value::Int(3)],
            &mut o
        )),
        "ell"
    );
    assert_eq!(
        str_of(text_substring(
            &[s("hello"), Value::Int(-2), Value::Int(1)],
            &mut o
        )),
        "l"
    );
    assert_eq!(
        str_of(text_substring(
            &[s("hello"), Value::Int(1), Value::Int(-1)],
            &mut o
        )),
        "ell"
    );
    assert_eq!(
        str_of(text_substring(
            &[s("hi"), Value::Int(9), Value::Int(2)],
            &mut o
        )),
        ""
    );
    // PHP erasures.
    let php = |n: &str, a: &[&str]| {
        let args: Vec<String> = a.iter().map(|x| (*x).to_string()).collect();
        (registry()[index_of("Core.Text", n).unwrap()].php)(&args)
    };
    assert_eq!(
        php("padLeft", &["$s", "$w", "$p"]),
        "str_pad($s, $w, $p, STR_PAD_LEFT)"
    );
    assert_eq!(
        php("padRight", &["$s", "$w", "$p"]),
        "str_pad($s, $w, $p, STR_PAD_RIGHT)"
    );
    assert_eq!(
        php("indexOf", &["$s", "$n"]),
        "__phorj_text_index_of($s, $n)"
    );
    assert_eq!(php("substring", &["$s", "$a", "$b"]), "substr($s, $a, $b)");
    assert_eq!(
        registry()[index_of("Core.Text", "indexOf").unwrap()].ret,
        Ty::Optional(Box::new(Ty::Int))
    );
}

#[test]
fn text_is_empty_trims_and_count() {
    let mut o = String::new();
    assert!(matches!(
        text_is_empty(&[Value::Str("".into())], &mut o).unwrap(),
        Value::Bool(true)
    ));
    assert!(matches!(
        text_is_empty(&[Value::Str("x".into())], &mut o).unwrap(),
        Value::Bool(false)
    ));
    assert!(matches!(
        text_trim_start(&[Value::Str("  hi  ".into())], &mut o).unwrap(),
        Value::Str(s) if s == "hi  "
    ));
    assert!(matches!(
        text_trim_end(&[Value::Str("  hi  ".into())], &mut o).unwrap(),
        Value::Str(s) if s == "  hi"
    ));
    // non-overlapping count
    assert!(matches!(
        text_count(
            &[Value::Str("banana".into()), Value::Str("a".into())],
            &mut o
        )
        .unwrap(),
        Value::Int(3)
    ));
    assert!(matches!(
        text_count(&[Value::Str("aaa".into()), Value::Str("aa".into())], &mut o).unwrap(),
        Value::Int(1)
    ));
    // empty needle is a clean fault (matches PHP substr_count rejecting it)
    assert!(text_count(&[Value::Str("x".into()), Value::Str("".into())], &mut o).is_err());
}
