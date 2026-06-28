//! `Core.Validate` — syntactic string predicates (native-stdlib wave, Tier A).
//!
//! Pure, deterministic, std-only. Each predicate is `string -> bool`. Phorj has no regex crate (the
//! library is std-only), so the checks are hand-rolled in Rust and the PHP side emits a `preg_match`
//! with the *same* anchored, explicit-char-class pattern — so the two cannot disagree (no
//! `filter_var`, whose validation semantics we'd have to chase). These are *format* checks, not
//! semantic validators (e.g. `isInt` is `^[+-]?[0-9]+$`, not "fits in an i64").

use super::*;
use crate::types::Ty;
use crate::value::Value;

/// `^[+-]?[0-9]+$`
fn is_int(s: &str) -> bool {
    let b = s.as_bytes();
    let start = usize::from(b.first().is_some_and(|&c| c == b'+' || c == b'-'));
    b.len() > start && b[start..].iter().all(u8::is_ascii_digit)
}

/// `^[+-]?[0-9]+(\.[0-9]+)?$`
fn is_number(s: &str) -> bool {
    let b = s.as_bytes();
    let start = usize::from(b.first().is_some_and(|&c| c == b'+' || c == b'-'));
    let digits = &b[start..];
    match digits.iter().position(|&c| c == b'.') {
        None => !digits.is_empty() && digits.iter().all(u8::is_ascii_digit),
        Some(dot) => {
            let (int_part, frac_part) = (&digits[..dot], &digits[dot + 1..]);
            !int_part.is_empty()
                && int_part.iter().all(u8::is_ascii_digit)
                && !frac_part.is_empty()
                && frac_part.iter().all(u8::is_ascii_digit)
        }
    }
}

/// `^[A-Za-z]+$`
fn is_alpha(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic())
}

/// `^[A-Za-z0-9]+$`
fn is_alnum(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// `^[0-9A-Fa-f]+$`
fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn pred(args: &[Value], f: fn(&str) -> bool, who: &str) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Bool(f(s))),
        _ => Err(format!("Validate.{who} expects (string)")),
    }
}
fn is_int_native(a: &[Value], _: &mut String) -> Result<Value, String> {
    pred(a, is_int, "isInt")
}
fn is_number_native(a: &[Value], _: &mut String) -> Result<Value, String> {
    pred(a, is_number, "isNumber")
}
fn is_alpha_native(a: &[Value], _: &mut String) -> Result<Value, String> {
    pred(a, is_alpha, "isAlpha")
}
fn is_alnum_native(a: &[Value], _: &mut String) -> Result<Value, String> {
    pred(a, is_alnum, "isAlnum")
}
fn is_hex_native(a: &[Value], _: &mut String) -> Result<Value, String> {
    pred(a, is_hex, "isHex")
}

/// The `Core.Validate` registry entries. Each `string -> bool`, the Rust hand-roll mirrored by a PHP
/// `preg_match(pattern) === 1` over the identical anchored pattern.
pub(crate) fn validate_natives() -> Vec<NativeFn> {
    fn entry(
        name: &'static str,
        eval: fn(&[Value], &mut String) -> Result<Value, String>,
        php: fn(&[String]) -> String,
    ) -> NativeFn {
        NativeFn {
            module: "Core.Validate",
            name,
            params: vec![Ty::String],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(eval),
            php,
        }
    }
    vec![
        entry("isInt", is_int_native, |a| {
            format!("(preg_match('/^[+-]?[0-9]+$/', {}) === 1)", parg(a, 0))
        }),
        entry("isNumber", is_number_native, |a| {
            format!(
                "(preg_match('/^[+-]?[0-9]+(\\.[0-9]+)?$/', {}) === 1)",
                parg(a, 0)
            )
        }),
        entry("isAlpha", is_alpha_native, |a| {
            format!("(preg_match('/^[A-Za-z]+$/', {}) === 1)", parg(a, 0))
        }),
        entry("isAlnum", is_alnum_native, |a| {
            format!("(preg_match('/^[A-Za-z0-9]+$/', {}) === 1)", parg(a, 0))
        }),
        entry("isHex", is_hex_native, |a| {
            format!("(preg_match('/^[0-9A-Fa-f]+$/', {}) === 1)", parg(a, 0))
        }),
    ]
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;
