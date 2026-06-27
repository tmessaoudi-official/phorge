use super::*;
use crate::value::Value;

fn b(f: fn(&[Value], &mut String) -> Result<Value, String>, input: &str) -> bool {
    match f(&[Value::Str(input.into())], &mut String::new()).unwrap() {
        Value::Bool(t) => t,
        other => panic!("expected bool, got {other:?}"),
    }
}

// Expected sets pinned to real `php -n` preg_match over the identical patterns.

#[test]
fn is_int_matches_php() {
    for ok in ["42", "-7", "+9", "0", "007"] {
        assert!(b(is_int_native, ok), "{ok}");
    }
    for no in ["", "3.14", "abc", "1e3", "ab c", "+", "-"] {
        assert!(!b(is_int_native, no), "{no}");
    }
}

#[test]
fn is_number_matches_php() {
    for ok in ["42", "-7", "+9", "3.14", "-0.5"] {
        assert!(b(is_number_native, ok), "{ok}");
    }
    for no in ["", "12.", ".5", "1e3", "abc", "1.2.3"] {
        assert!(!b(is_number_native, no), "{no}");
    }
}

#[test]
fn is_alpha_matches_php() {
    for ok in ["abc", "DEADbeef", "Hello"] {
        assert!(b(is_alpha_native, ok), "{ok}");
    }
    for no in ["", "abc1", "ab c", "café"] {
        assert!(!b(is_alpha_native, no), "{no}");
    }
}

#[test]
fn is_alnum_matches_php() {
    for ok in ["42", "abc", "abc1", "DEADbeef", "1e3"] {
        assert!(b(is_alnum_native, ok), "{ok}");
    }
    for no in ["", "ab c", "a-b", "3.14"] {
        assert!(!b(is_alnum_native, no), "{no}");
    }
}

#[test]
fn is_hex_matches_php() {
    for ok in ["42", "abc", "abc1", "DEADbeef", "1e3", "FF00"] {
        assert!(b(is_hex_native, ok), "{ok}");
    }
    for no in ["", "xyz", "g1", "0x1f"] {
        assert!(!b(is_hex_native, no), "{no}");
    }
}
