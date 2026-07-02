use super::*;
use crate::value::{HKey, Value};

/// Parse INI text and return the ordered (key, value) string pairs, or panic if the shape is wrong.
fn parse(src: &str) -> Vec<(String, String)> {
    let mut out = String::new();
    match ini_parse(&[Value::Str(src.into())], &mut out).unwrap() {
        Value::Map(m) => m
            .iter()
            .map(|(k, v)| {
                let ks = match k {
                    HKey::Str(s) => s.clone(),
                    other => panic!("non-string ini key: {other:?}"),
                };
                let vs = match v {
                    Value::Str(s) => s.clone(),
                    other => panic!("non-string ini value: {other:?}"),
                };
                (ks, vs)
            })
            .collect(),
        other => panic!("expected a Map, got {other:?}"),
    }
}

#[test]
fn ini_registered() {
    assert!(crate::native::index_of("Core.Ini", "parse").is_some());
}

#[test]
fn basic_key_value_trimmed() {
    assert_eq!(
        parse("host = localhost\n  port=8080  "),
        vec![
            ("host".into(), "localhost".into()),
            ("port".into(), "8080".into())
        ]
    );
}

#[test]
fn sections_dot_the_keys() {
    let src = "top = 1\n[db]\nhost = pg\nport = 5432\n[cache]\nttl = 60";
    assert_eq!(
        parse(src),
        vec![
            ("top".into(), "1".into()),
            ("db.host".into(), "pg".into()),
            ("db.port".into(), "5432".into()),
            ("cache.ttl".into(), "60".into()),
        ]
    );
}

#[test]
fn comments_and_blanks_skipped() {
    let src = "; a comment\n# another\n\nkey = val\n   ; indented comment";
    assert_eq!(parse(src), vec![("key".into(), "val".into())]);
}

#[test]
fn value_may_contain_equals() {
    assert_eq!(parse("expr = a=b=c"), vec![("expr".into(), "a=b=c".into())]);
}

#[test]
fn no_type_coercion_values_stay_strings() {
    // PHP's parse_ini_string would coerce `on`/`true`/`1` to bool/int — Phorj does NOT.
    assert_eq!(
        parse("flag = on\nyes = true\nn = 1"),
        vec![
            ("flag".into(), "on".into()),
            ("yes".into(), "true".into()),
            ("n".into(), "1".into())
        ]
    );
}

#[test]
fn duplicate_key_last_wins_first_position() {
    assert_eq!(
        parse("a = 1\nb = 2\na = 3"),
        vec![("a".into(), "3".into()), ("b".into(), "2".into())]
    );
}

#[test]
fn malformed_line_skipped() {
    assert_eq!(
        parse("good = 1\nthis has no equals\nalso_good = 2"),
        vec![
            ("good".into(), "1".into()),
            ("also_good".into(), "2".into())
        ]
    );
}
