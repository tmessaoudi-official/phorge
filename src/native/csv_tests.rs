use super::*;
use crate::value::Value;

/// Parse a row and extract `Vec<String>` (panics on a non-string element — the kernel never makes one).
fn p(input: &str) -> Vec<String> {
    match csv_parse(&[Value::Str(input.into())], &mut String::new()).unwrap() {
        Value::List(items) => items
            .iter()
            .map(|v| match v {
                Value::Str(s) => s.clone(),
                other => panic!("non-string field: {other:?}"),
            })
            .collect(),
        other => panic!("expected list, got {other:?}"),
    }
}

fn f(fields: &[&str]) -> String {
    let list = Value::List(std::rc::Rc::new(
        fields.iter().map(|s| Value::Str((*s).into())).collect(),
    ));
    match csv_format(&[list], &mut String::new()).unwrap() {
        Value::Str(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

// Every expected value is pinned to `php -n` 8.5 `str_getcsv($s, ",", "\"", "")` output, except the
// empty-input row (documented `[""]` deviation from PHP's `[null]`).
#[test]
fn parse_matches_php_str_getcsv() {
    assert_eq!(p("a,b,c"), ["a", "b", "c"]);
    assert_eq!(p("a,\"b,c\",d"), ["a", "b,c", "d"]);
    assert_eq!(
        p("a,\"he said \"\"hi\"\"\",d"),
        ["a", "he said \"hi\"", "d"]
    );
    assert_eq!(p(""), [""]); // deviation: PHP returns [null]
    assert_eq!(p("a,,c"), ["a", "", "c"]);
    assert_eq!(p("\"\""), [""]);
    assert_eq!(p(" \"a\""), ["a"]); // leading space before quote is discarded
    assert_eq!(p("\"a\"b"), ["ab"]); // junk after closing quote is appended
    assert_eq!(p("\"a\" b"), ["a b"]);
    assert_eq!(p("\"a\" "), ["a "]);
    assert_eq!(p("a\"b"), ["a\"b"]); // quote mid unquoted field is literal
    assert_eq!(p(" a"), [" a"]); // leading space on unquoted field is kept
    assert_eq!(p("\"a\nb\""), ["a\nb"]); // embedded newline inside quotes
    assert_eq!(p("a,"), ["a", ""]);
    assert_eq!(p(","), ["", ""]);
    assert_eq!(p("\"unterminated"), ["unterminated"]);
    assert_eq!(p("x,\"un,term"), ["x", "un,term"]);
    assert_eq!(p("\"\"\"\""), ["\""]); // four quotes → one literal quote
}

#[test]
fn format_matches_php() {
    assert_eq!(f(&["a", "b", "c"]), "a,b,c");
    assert_eq!(f(&["a,b", "c"]), "\"a,b\",c");
    assert_eq!(f(&["he \"q\"", "x"]), "\"he \"\"q\"\"\",x");
    assert_eq!(f(&[""]), "");
    assert_eq!(f(&["a", ""]), "a,");
    assert_eq!(f(&["line1\nline2"]), "\"line1\nline2\"");
}

#[test]
fn parse_then_format_round_trips() {
    for row in [
        vec!["a", "b,c", "d"],
        vec!["x\"y", "z"],
        vec!["", ""],
        vec!["a"],
        vec!["a\nb", "c"],
    ] {
        let formatted = f(&row);
        assert_eq!(p(&formatted), row, "round-trip of {row:?}");
    }
}
