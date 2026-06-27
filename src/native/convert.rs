//! `Core.Convert` — explicit value conversion (`docs/specs/2026-06-26-m4-casting-conversion-design.md`,
//! axis 1). The *cast* (type assertion / reinterpret) is the `as` operator; this module produces a
//! **new value** of another type, always explicitly (Phorge has no implicit coercion). Lossy
//! conversions are *named* (`truncate`/`round`), never a silent `(int)`. Because UFCS ships,
//! `Convert.toFloat(n)` and `n.toFloat()` are the same call — module + method API in one.

use super::*;
use crate::types::Ty;
use crate::value::Value;

/// `Convert.toString(T) -> string` — generic, runtime-dispatched, reusing `Value::as_display` (the
/// same rendering as string interpolation / the PHP `__phorge_str` helper): bool → `true`/`false`,
/// float → shortest-round-trip, int/string verbatim. Byte-identity contract is the scalar types; a
/// composite value (list/map/instance) is not displayable → a clean fault (documented edge).
fn convert_to_string(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [v] => v
            .as_display()
            .map(Value::Str)
            .ok_or_else(|| format!("Convert.toString cannot convert {}", v.type_name())),
        _ => Err("Convert.toString expects (T)".into()),
    }
}

/// `Convert.toFloat(int) -> float` — total widening (Rust `as f64` ≡ PHP `(float)`).
fn convert_to_float(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(n)] => Ok(Value::Float(*n as f64)),
        _ => Err("Convert.toFloat expects (int)".into()),
    }
}

/// `Convert.truncate(float) -> int` — toward zero (Rust `as i64` ≡ PHP `(int)`). Lossy, named.
fn convert_truncate(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(f)] => Ok(Value::Int(*f as i64)),
        _ => Err("Convert.truncate expects (float)".into()),
    }
}

/// `Convert.round(float) -> int` — half away from zero (Rust `f.round()` ≡ PHP `round()` default).
fn convert_round(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(f)] => Ok(Value::Int(f.round() as i64)),
        _ => Err("Convert.round expects (float)".into()),
    }
}

/// `Convert.toInt(float) -> int?` (M-NUM S3) — truncate toward zero, or `null` on NaN / ±∞ /
/// out-of-i64-range. Single-sourced with `value::float_to_int` (the edge-safe guards), so `run`/`runvm`
/// agree; mirrored by the PHP `__phorge_float_to_int` helper. Avoids PHP's `(int)NAN == 0`.
fn convert_to_int(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(f)] => Ok(crate::value::float_to_int(*f).map_or(Value::Null, Value::Int)),
        _ => Err("Convert.toInt expects (float)".into()),
    }
}

/// `Convert.intToDecimal(int) -> decimal` (M-NUM S3) — total widening to a scale-0 decimal. PHP carrier
/// is the integer's string form (`(string)$i`).
fn convert_int_to_decimal(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(n)] => Ok(Value::Decimal {
            unscaled: i128::from(*n),
            scale: 0,
        }),
        _ => Err("Convert.intToDecimal expects (int)".into()),
    }
}

/// `Convert.decimalToFloat(decimal) -> float` (M-NUM S3) — parse the decimal's rendered string to f64
/// (lossy by nature). The PHP carrier is already that string, so PHP `(float)$s` matches. A value other
/// than a decimal is checker-unreachable (handled defensively as a fault).
fn convert_decimal_to_float(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [v @ Value::Decimal { .. }] => {
            let s = v
                .as_display()
                .ok_or_else(|| "Convert.decimalToFloat: unrenderable decimal".to_string())?;
            let f: f64 = s
                .parse()
                .map_err(|_| "Convert.decimalToFloat: bad decimal string".to_string())?;
            Ok(Value::Float(f))
        }
        _ => Err("Convert.decimalToFloat expects (decimal)".into()),
    }
}

/// `Convert.decimalToInt(decimal) -> int?` (M-NUM S3) — truncate toward zero (drop the fraction), or
/// `null` if the integer part is out of i64 range. Single-sourced with `value::decimal_to_int` (exact
/// i128 carrier math, no BCMath); mirrored by the PHP `__phorge_dec_to_int` helper (string split before
/// the dot). For *rounded* decimal→int, compose `Decimal.round(d, 0, mode)` then `decimalToInt`.
fn convert_decimal_to_int(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [v @ Value::Decimal { .. }] => {
            Ok(crate::value::decimal_to_int(v).map_or(Value::Null, Value::Int))
        }
        _ => Err("Convert.decimalToInt expects (decimal)".into()),
    }
}

/// `Convert.floatToIntExact(float) -> int?` (M4 as-matrix) — the `float as int` kernel: `Some` only
/// when the float is integral & in range (`3.0 → 3`, `3.9 → null`), never a silent truncate.
/// Single-sourced with `value::float_to_int_exact`; PHP `__phorge_float_to_int_exact`.
fn convert_float_to_int_exact(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(f)] => {
            Ok(crate::value::float_to_int_exact(*f).map_or(Value::Null, Value::Int))
        }
        _ => Err("Convert.floatToIntExact expects (float)".into()),
    }
}

/// `Convert.decimalToIntExact(decimal) -> int?` (M4 as-matrix) — the `decimal as int` kernel: `Some`
/// only when the decimal is integral & in range (`3.00d → 3`, `3.50d → null`), never a silent
/// truncate. Single-sourced with `value::decimal_to_int_exact`; PHP `__phorge_dec_to_int_exact`.
fn convert_decimal_to_int_exact(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [v @ Value::Decimal { .. }] => {
            Ok(crate::value::decimal_to_int_exact(v).map_or(Value::Null, Value::Int))
        }
        _ => Err("Convert.decimalToIntExact expects (decimal)".into()),
    }
}

/// `value as <int|float|bool>` on a **union** source (M4 as-matrix S2) — runtime type ASSERTION,
/// not conversion: return the value when its runtime variant is the target primitive, else `null`
/// (`(int|string) as int` ⇒ the int, or `null` for the string arm). The PHP carrier is a real
/// `int`/`float`/`bool`, so `is_int`/`is_float`/`is_bool` distinguish them; `decimal` is deferred
/// (its carrier is a string, indistinguishable from a `string` union member in PHP).
fn convert_as_int(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [v @ Value::Int(_)] => Ok(v.clone()),
        [_] => Ok(Value::Null),
        _ => Err("Convert.asInt expects (T)".into()),
    }
}

fn convert_as_float(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [v @ Value::Float(_)] => Ok(v.clone()),
        [_] => Ok(Value::Null),
        _ => Err("Convert.asFloat expects (T)".into()),
    }
}

fn convert_as_bool(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [v @ Value::Bool(_)] => Ok(v.clone()),
        [_] => Ok(Value::Null),
        _ => Err("Convert.asBool expects (T)".into()),
    }
}

pub(crate) fn convert_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Convert",
            name: "toString",
            params: vec![Ty::Param("T".into())],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(convert_to_string),
            // Reuses the existing `__phorge_str` helper (gated via `uses_str`, set in transpile/call.rs).
            php: |a| format!("__phorge_str({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Convert",
            name: "toFloat",
            params: vec![Ty::Int],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(convert_to_float),
            php: |a| format!("(float)({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Convert",
            name: "truncate",
            params: vec![Ty::Float],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(convert_truncate),
            php: |a| format!("(int)({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Convert",
            name: "round",
            params: vec![Ty::Float],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(convert_round),
            php: |a| format!("(int)round({})", parg(a, 0)),
        },
        // --- Numeric conversions (M-NUM S3) ---
        NativeFn {
            module: "Core.Convert",
            name: "toInt",
            params: vec![Ty::Float],
            ret: Ty::Optional(Box::new(Ty::Int)),
            pure: true,
            // `__phorge_float_to_int` is gated in `transpile::emit_member_call` (a native's `php`
            // closure has no `&mut self`). Mirrors `value::float_to_int`.
            php: |a| format!("__phorge_float_to_int({})", parg(a, 0)),
            eval: NativeEval::Pure(convert_to_int),
        },
        NativeFn {
            module: "Core.Convert",
            name: "intToDecimal",
            params: vec![Ty::Int],
            ret: Ty::Decimal,
            pure: true,
            // The decimal carrier is the integer's string form (M-NUM S1 carrier convention).
            php: |a| format!("(string)({})", parg(a, 0)),
            eval: NativeEval::Pure(convert_int_to_decimal),
        },
        NativeFn {
            module: "Core.Convert",
            name: "decimalToFloat",
            params: vec![Ty::Decimal],
            ret: Ty::Float,
            pure: true,
            // The carrier is already the decimal's string form; `(float)$s` parses it (lossy).
            php: |a| format!("(float)({})", parg(a, 0)),
            eval: NativeEval::Pure(convert_decimal_to_float),
        },
        NativeFn {
            module: "Core.Convert",
            name: "decimalToInt",
            params: vec![Ty::Decimal],
            ret: Ty::Optional(Box::new(Ty::Int)),
            pure: true,
            // `__phorge_dec_to_int` is gated in `transpile::emit_member_call`. Mirrors
            // `value::decimal_to_int` (split the carrier string before the dot, range-check).
            php: |a| format!("__phorge_dec_to_int({})", parg(a, 0)),
            eval: NativeEval::Pure(convert_decimal_to_int),
        },
        // --- exact int conversions (M4 `as`-matrix `float/decimal as int`) ---
        NativeFn {
            module: "Core.Convert",
            name: "floatToIntExact",
            params: vec![Ty::Float],
            ret: Ty::Optional(Box::new(Ty::Int)),
            pure: true,
            php: |a| format!("__phorge_float_to_int_exact({})", parg(a, 0)),
            eval: NativeEval::Pure(convert_float_to_int_exact),
        },
        NativeFn {
            module: "Core.Convert",
            name: "decimalToIntExact",
            params: vec![Ty::Decimal],
            ret: Ty::Optional(Box::new(Ty::Int)),
            pure: true,
            php: |a| format!("__phorge_dec_to_int_exact({})", parg(a, 0)),
            eval: NativeEval::Pure(convert_decimal_to_int_exact),
        },
        // --- runtime type assertions (M4 as-matrix S2: union source `as int/float/bool`) ---
        NativeFn {
            module: "Core.Convert",
            name: "asInt",
            params: vec![Ty::Param("T".into())],
            ret: Ty::Optional(Box::new(Ty::Int)),
            pure: true,
            // Arrow-IIFE so the operand is evaluated exactly once (the `as` single-eval contract).
            php: |a| format!("(fn($__a) => is_int($__a) ? $__a : null)({})", parg(a, 0)),
            eval: NativeEval::Pure(convert_as_int),
        },
        NativeFn {
            module: "Core.Convert",
            name: "asFloat",
            params: vec![Ty::Param("T".into())],
            ret: Ty::Optional(Box::new(Ty::Float)),
            pure: true,
            php: |a| format!("(fn($__a) => is_float($__a) ? $__a : null)({})", parg(a, 0)),
            eval: NativeEval::Pure(convert_as_float),
        },
        NativeFn {
            module: "Core.Convert",
            name: "asBool",
            params: vec![Ty::Param("T".into())],
            ret: Ty::Optional(Box::new(Ty::Bool)),
            pure: true,
            php: |a| format!("(fn($__a) => is_bool($__a) ? $__a : null)({})", parg(a, 0)),
            eval: NativeEval::Pure(convert_as_bool),
        },
    ]
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
