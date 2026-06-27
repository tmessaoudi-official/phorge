use super::*;
use crate::value::Value;

fn s(f: fn(&[Value], &mut String) -> Result<Value, String>, input: &str) -> Value {
    f(&[Value::Str(input.into())], &mut String::new()).unwrap()
}
fn str_of(v: Value) -> String {
    match v {
        Value::Str(t) => t,
        other => panic!("expected string, got {other:?}"),
    }
}

// All reference values captured from real `php -n` (urlencode/rawurlencode/urldecode/rawurldecode).

#[test]
fn url_encode_matches_php() {
    assert_eq!(str_of(s(url_encode_native, "hello world")), "hello+world");
    assert_eq!(
        str_of(s(url_encode_native, "a+b/c?d=e&f")),
        "a%2Bb%2Fc%3Fd%3De%26f"
    );
    assert_eq!(
        str_of(s(url_encode_native, "Café/Restaurant")),
        "Caf%C3%A9%2FRestaurant"
    );
    // urlencode encodes `~` (%7E) and turns space into `+`.
    assert_eq!(str_of(s(url_encode_native, "a-_.~b")), "a-_.%7Eb");
    assert_eq!(str_of(s(url_encode_native, "100%")), "100%25");
    assert_eq!(str_of(s(url_encode_native, "")), "");
}

#[test]
fn raw_url_encode_matches_php() {
    // rawurlencode: space → %20, `~` left as-is (RFC 3986).
    assert_eq!(
        str_of(s(raw_url_encode_native, "hello world")),
        "hello%20world"
    );
    assert_eq!(str_of(s(raw_url_encode_native, "a-_.~b")), "a-_.~b");
    assert_eq!(
        str_of(s(raw_url_encode_native, "a+b/c?d=e&f")),
        "a%2Bb%2Fc%3Fd%3De%26f"
    );
}

#[test]
fn url_decode_matches_php() {
    assert_eq!(str_of(s(url_decode_native, "hello+world")), "hello world");
    assert_eq!(str_of(s(url_decode_native, "a%20b")), "a b");
    assert_eq!(str_of(s(url_decode_native, "100%25")), "100%");
    assert_eq!(str_of(s(url_decode_native, "%2B")), "+");
    assert_eq!(str_of(s(url_decode_native, "caf%C3%A9")), "café");
    // lenient: an invalid escape is left literal (never a fault).
    assert_eq!(str_of(s(url_decode_native, "bad%ZZ")), "bad%ZZ");
    assert_eq!(str_of(s(url_decode_native, "trail%")), "trail%");
}

#[test]
fn raw_url_decode_matches_php() {
    // rawurldecode does NOT turn `+` into a space.
    assert_eq!(str_of(s(raw_url_decode_native, "a+b")), "a+b");
    assert_eq!(str_of(s(raw_url_decode_native, "a%20b")), "a b");
    assert_eq!(str_of(s(raw_url_decode_native, "%2B")), "+");
    assert_eq!(str_of(s(raw_url_decode_native, "caf%C3%A9")), "café");
}

#[test]
fn decode_to_invalid_utf8_is_null() {
    // %FF alone decodes to byte 0xFF, which is not valid UTF-8 → null (the string? absent case).
    assert!(matches!(s(url_decode_native, "%FF"), Value::Null));
    assert!(matches!(s(raw_url_decode_native, "%FF"), Value::Null));
}

#[test]
fn encode_decode_roundtrip() {
    for original in ["a b&c=d", "Ünïcödé/path", "100% sure?"] {
        let enc = str_of(s(url_encode_native, original));
        assert_eq!(str_of(s(url_decode_native, &enc)), original);
        let renc = str_of(s(raw_url_encode_native, original));
        assert_eq!(str_of(s(raw_url_decode_native, &renc)), original);
    }
}
