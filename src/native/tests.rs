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
    assert_eq!(php, r#"echo $x, "\n""#);
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
