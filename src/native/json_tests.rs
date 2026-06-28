use super::*;
use crate::value::Value;

/// Encode a `Json` value to its compact string (the `stringify` kernel).
fn enc(v: &Value) -> String {
    let mut s = String::new();
    encode(v, &mut s).expect("encode");
    s
}
/// Pretty-encode (the `stringifyPretty` kernel).
fn pretty(v: &Value) -> String {
    let mut s = String::new();
    encode_pretty(v, 0, &mut s).expect("encode_pretty");
    s
}
/// Parse a JSON string, then re-encode it compactly (round-trip through both kernels).
fn roundtrip(src: &str) -> Option<String> {
    parse_json(src).map(|v| enc(&v))
}

#[test]
fn encode_scalars() {
    assert_eq!(enc(&jnode("Null", vec![])), "null");
    assert_eq!(enc(&jnode("Bool", vec![Value::Bool(true)])), "true");
    assert_eq!(enc(&jnode("Bool", vec![Value::Bool(false)])), "false");
    assert_eq!(enc(&jnode("Int", vec![Value::Int(42)])), "42");
    assert_eq!(enc(&jnode("Int", vec![Value::Int(-7)])), "-7");
    // Integral float renders without a trailing `.0` (Rust `{}` / __phorj_float).
    assert_eq!(enc(&jnode("Float", vec![Value::Float(42.0)])), "42");
    assert_eq!(enc(&jnode("Float", vec![Value::Float(3.5)])), "3.5");
}

#[test]
fn encode_strings_match_php_json_encode_default() {
    let s = |t: &str| enc(&jnode("Str", vec![Value::Str(t.into())]));
    assert_eq!(s("hi"), "\"hi\"");
    assert_eq!(s("a/b"), "\"a\\/b\""); // forward slash escaped (PHP default)
    assert_eq!(s("café"), "\"caf\\u00e9\""); // non-ASCII → lowercase \u
    assert_eq!(s("😀"), "\"\\ud83d\\ude00\""); // surrogate pair for >0xFFFF
    assert_eq!(s("\u{01}\u{08}\u{0c}\n\r\t"), "\"\\u0001\\b\\f\\n\\r\\t\"");
    assert_eq!(s("quote\"back\\"), "\"quote\\\"back\\\\\"");
}

#[test]
fn parse_distinguishes_int_and_float_like_json_decode() {
    assert_eq!(roundtrip("42").unwrap(), "42"); // int
    assert_eq!(roundtrip("42.0").unwrap(), "42"); // float, integral → "42"
    assert_eq!(roundtrip("1e3").unwrap(), "1000"); // float
    assert_eq!(roundtrip("3.14").unwrap(), "3.14");
    assert_eq!(roundtrip("-0.5").unwrap(), "-0.5");
    // i64 overflow falls back to float (matches json_decode).
    assert_eq!(
        roundtrip("9999999999999999999").unwrap(),
        "10000000000000000000"
    );
}

#[test]
fn parse_structures_and_roundtrip() {
    assert_eq!(roundtrip("[]").unwrap(), "[]");
    assert_eq!(roundtrip("{}").unwrap(), "{}");
    assert_eq!(roundtrip("[1,2,3]").unwrap(), "[1,2,3]");
    assert_eq!(roundtrip(" [ 1 , 2 ] ").unwrap(), "[1,2]"); // whitespace skipped
    assert_eq!(
        roundtrip("{\"name\":\"phorj\",\"n\":2}").unwrap(),
        "{\"name\":\"phorj\",\"n\":2}"
    );
    assert_eq!(
        roundtrip("{\"a\":[true,null],\"b\":{\"c\":1}}").unwrap(),
        "{\"a\":[true,null],\"b\":{\"c\":1}}"
    );
}

#[test]
fn parse_object_key_order_and_dup_keys() {
    // Insertion order is preserved; a duplicate key keeps its first position, last value (PHP assoc).
    assert_eq!(roundtrip("{\"b\":1,\"a\":2}").unwrap(), "{\"b\":1,\"a\":2}");
    assert_eq!(
        roundtrip("{\"a\":1,\"b\":2,\"a\":3}").unwrap(),
        "{\"a\":3,\"b\":2}"
    );
}

#[test]
fn parse_string_escapes() {
    assert_eq!(roundtrip("\"a\\/b\"").unwrap(), "\"a\\/b\"");
    assert_eq!(roundtrip("\"\\u00e9\"").unwrap(), "\"\\u00e9\""); // é round-trips
    assert_eq!(
        roundtrip("\"\\ud83d\\ude00\"").unwrap(),
        "\"\\ud83d\\ude00\""
    ); // emoji surrogate
    assert_eq!(roundtrip("\"tab\\there\"").unwrap(), "\"tab\\there\"");
}

#[test]
fn parse_malformed_is_none() {
    assert!(parse_json("").is_none());
    assert!(parse_json("nul").is_none());
    assert!(parse_json("[1,2").is_none());
    assert!(parse_json("{\"a\":}").is_none());
    assert!(parse_json("01").is_none()); // leading zero
    assert!(parse_json("1.").is_none()); // bare decimal point
    assert!(parse_json("42 junk").is_none()); // trailing junk
    assert!(parse_json("\"\\ud83d\"").is_none()); // lone surrogate
}

#[test]
fn pretty_matches_json_pretty_print_layout() {
    // {"name":"phorj","nums":[1,2],"meta":{"ok":true,"empty":[]}} pretty-printed (verified vs PHP).
    let v = parse_json("{\"name\":\"phorj\",\"nums\":[1,2],\"meta\":{\"ok\":true,\"empty\":[]}}")
        .unwrap();
    let expected = "{\n    \"name\": \"phorj\",\n    \"nums\": [\n        1,\n        2\n    ],\n    \"meta\": {\n        \"ok\": true,\n        \"empty\": []\n    }\n}";
    assert_eq!(pretty(&v), expected);
}

#[test]
fn pretty_empty_and_scalar() {
    assert_eq!(
        pretty(&jnode("Arr", vec![Value::List(std::rc::Rc::new(vec![]))])),
        "[]"
    );
    assert_eq!(pretty(&jnode("Int", vec![Value::Int(7)])), "7");
}

#[test]
fn natives_registered_with_expected_signatures() {
    assert!(crate::native::index_of("Core.Json", "parse").is_some());
    assert!(crate::native::index_of("Core.Json", "stringify").is_some());
    assert!(crate::native::index_of("Core.Json", "stringifyPretty").is_some());
}
