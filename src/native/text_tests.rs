use super::*;

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
