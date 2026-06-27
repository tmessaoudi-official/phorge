//! `Core.Encoding` — base64 and hex codecs (native-stdlib wave, Tier A).
//!
//! Pure, deterministic, std-only. Each encoder/decoder is byte-identical to a PHP **core** function
//! (available under `php -n` — no ini extension needed): `base64_encode` / `base64_decode($s, true)`
//! (strict) / `bin2hex` / `hex2bin`. Encoders take `bytes` → `string`; decoders take `string` →
//! `bytes?` (`null` on malformed input — the optional absent case, never a fault). Single-sourced
//! kernels here are mirrored by the `php` emission; the differential's PHP oracle pins the parity.

use super::*;
use crate::types::Ty;
use crate::value::Value;
use std::rc::Rc;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with `=` padding — identical to PHP `base64_encode`.
fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(B64[((n >> 18) & 63) as usize] as char);
        out.push(B64[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Strict base64 decode — `None` on any character outside the alphabet, misplaced padding, or a bad
/// length. Mirrors PHP `base64_decode($s, true)` for well-formed/clearly-malformed input.
fn b64_decode_strict(s: &str) -> Option<Vec<u8>> {
    let mut rev = [255u8; 256];
    let mut i = 0;
    while i < 64 {
        rev[B64[i] as usize] = i as u8;
        i += 1;
    }
    let raw = s.as_bytes();
    let mut sextets: Vec<u8> = Vec::with_capacity(raw.len());
    let mut pad = 0usize;
    for &c in raw {
        if c == b'=' {
            pad += 1;
            continue;
        }
        if pad > 0 {
            return None; // data after padding
        }
        let v = rev[c as usize];
        if v == 255 {
            return None; // outside the alphabet (incl. whitespace — strict)
        }
        sextets.push(v);
    }
    // Padding makes the total a multiple of 4; a lone leftover sextet is invalid.
    if pad > 2 || (sextets.len() + pad) % 4 != 0 || sextets.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(sextets.len() / 4 * 3);
    for group in sextets.chunks(4) {
        let n = group
            .iter()
            .enumerate()
            .fold(0u32, |acc, (j, &v)| acc | (u32::from(v) << (18 - 6 * j)));
        out.push((n >> 16) as u8);
        if group.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if group.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// Lowercase hex — identical to PHP `bin2hex`.
fn hex_encode(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for &b in data {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

/// Hex decode — `None` on an odd length or a non-hex digit. Mirrors PHP `hex2bin` (which accepts both
/// cases and returns false otherwise).
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let raw = s.as_bytes();
    if raw.len() % 2 != 0 {
        return None;
    }
    let nib = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(raw.len() / 2);
    for pair in raw.chunks(2) {
        out.push((nib(pair[0])? << 4) | nib(pair[1])?);
    }
    Some(out)
}

fn base64_encode_native(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Bytes(b)] => Ok(Value::Str(b64_encode(b))),
        _ => Err("Encoding.base64Encode expects (bytes)".into()),
    }
}
fn base64_decode_native(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(match b64_decode_strict(s) {
            Some(bytes) => Value::Bytes(Rc::new(bytes)),
            None => Value::Null,
        }),
        _ => Err("Encoding.base64Decode expects (string)".into()),
    }
}
fn hex_encode_native(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Bytes(b)] => Ok(Value::Str(hex_encode(b))),
        _ => Err("Encoding.hexEncode expects (bytes)".into()),
    }
}
fn hex_decode_native(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(match hex_decode(s) {
            Some(bytes) => Value::Bytes(Rc::new(bytes)),
            None => Value::Null,
        }),
        _ => Err("Encoding.hexDecode expects (string)".into()),
    }
}

/// The `Core.Encoding` registry entries.
pub(crate) fn encoding_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Encoding",
            name: "base64Encode",
            params: vec![Ty::Bytes],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(base64_encode_native),
            php: |a| format!("base64_encode({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Encoding",
            name: "base64Decode",
            params: vec![Ty::String],
            ret: Ty::Optional(Box::new(Ty::Bytes)),
            pure: true,
            eval: NativeEval::Pure(base64_decode_native),
            // strict mode (2nd arg true) → false on malformed; map false → null (the `bytes?` absent).
            php: |a| {
                format!(
                    "(($__b64 = base64_decode({}, true)) === false ? null : $__b64)",
                    parg(a, 0)
                )
            },
        },
        NativeFn {
            module: "Core.Encoding",
            name: "hexEncode",
            params: vec![Ty::Bytes],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(hex_encode_native),
            php: |a| format!("bin2hex({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Encoding",
            name: "hexDecode",
            params: vec![Ty::String],
            ret: Ty::Optional(Box::new(Ty::Bytes)),
            pure: true,
            eval: NativeEval::Pure(hex_decode_native),
            // hex2bin returns false (+ warning to stderr) on odd length / non-hex; map false → null.
            // `@` suppresses the warning so stdout stays clean (the oracle compares stdout).
            php: |a| {
                format!(
                    "(($__hx = @hex2bin({})) === false ? null : $__hx)",
                    parg(a, 0)
                )
            },
        },
    ]
}

#[cfg(test)]
#[path = "encoding_tests.rs"]
mod tests;
