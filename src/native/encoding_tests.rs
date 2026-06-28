use super::*;
use crate::value::Value;
use std::rc::Rc;

fn bytes(s: &str) -> Value {
    Value::Bytes(Rc::new(s.as_bytes().to_vec()))
}
fn enc(f: fn(&[Value], &mut String) -> Result<Value, String>, v: Value) -> Value {
    f(&[v], &mut String::new()).unwrap()
}

#[test]
fn base64_encode_matches_php() {
    // Pinned to real `php -n` output (base64_encode).
    assert!(matches!(enc(base64_encode_native, bytes("hi")), Value::Str(s) if s == "aGk="));
    assert!(
        matches!(enc(base64_encode_native, bytes("Hello, Phorj!")), Value::Str(s) if s == "SGVsbG8sIFBob3JqIQ==")
    );
    assert!(matches!(enc(base64_encode_native, bytes("")), Value::Str(s) if s.is_empty()));
    // padding variants: 1 and 2 leftover bytes.
    assert!(matches!(enc(base64_encode_native, bytes("a")), Value::Str(s) if s == "YQ=="));
    assert!(matches!(enc(base64_encode_native, bytes("ab")), Value::Str(s) if s == "YWI="));
}

#[test]
fn hex_encode_matches_php() {
    // Pinned to real `php -n` output (bin2hex) — lowercase.
    assert!(matches!(enc(hex_encode_native, bytes("hi")), Value::Str(s) if s == "6869"));
    assert!(matches!(enc(hex_encode_native, bytes("Phorj")), Value::Str(s) if s == "50686f726a"));
    assert!(matches!(enc(hex_encode_native, bytes("")), Value::Str(s) if s.is_empty()));
}

fn decoded_bytes(v: Value) -> Vec<u8> {
    match v {
        Value::Bytes(b) => (*b).clone(),
        other => panic!("expected bytes, got {other:?}"),
    }
}

#[test]
fn base64_roundtrip() {
    let raw = "The quick brown fox \u{1f98a}".as_bytes().to_vec();
    let Value::Str(b64) = enc(base64_encode_native, Value::Bytes(Rc::new(raw.clone()))) else {
        panic!("encode");
    };
    assert_eq!(
        decoded_bytes(enc(base64_decode_native, Value::Str(b64))),
        raw
    );
}

#[test]
fn hex_roundtrip() {
    let raw = b"\x00\x01\xfePhorj".to_vec();
    let Value::Str(hex) = enc(hex_encode_native, Value::Bytes(Rc::new(raw.clone()))) else {
        panic!("encode");
    };
    assert_eq!(decoded_bytes(enc(hex_decode_native, Value::Str(hex))), raw);
}

#[test]
fn decode_invalid_is_null() {
    // base64: a character outside the alphabet (strict) → null.
    assert!(matches!(
        enc(base64_decode_native, Value::Str("not base64!".into())),
        Value::Null
    ));
    // hex: odd length / non-hex digit → null.
    assert!(matches!(
        enc(hex_decode_native, Value::Str("abc".into())),
        Value::Null
    ));
    assert!(matches!(
        enc(hex_decode_native, Value::Str("zz".into())),
        Value::Null
    ));
}
