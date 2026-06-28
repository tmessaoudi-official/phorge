//! `Core.Decimal` — runtime construction of the `decimal` primitive from a string (M-NUM S1). The
//! literal `19.99d` covers source constants; `Decimal.of(s)` covers dynamic/string input (parsed
//! input, config values), returning `decimal?` so a malformed or out-of-range string is a clean
//! `null` (composes with S2 `??`). Parsing is single-sourced in `value::decimal_of`, mirrored by the
//! PHP `__phorj_dec_of` PCRE helper (set in `transpile::emit_member_call`, the gated-helper pattern).

use super::*;
use crate::types::Ty;
use crate::value::{decimal_div, decimal_round, RoundMode, Value};

/// `Decimal.of(string) -> decimal?` — parse the same grammar as a `…d` literal (optional sign, digits
/// with an optional single fractional part; NO exponent/underscore/whitespace), returning the decimal
/// or `null` on malformed input / i128 overflow. Shared by the interpreter and the VM.
fn decimal_of(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(match crate::value::decimal_of(s) {
            Some((unscaled, scale)) => Value::Decimal { unscaled, scale },
            None => Value::Null,
        }),
        _ => Err("Decimal.of expects (string)".into()),
    }
}

/// Project a `RoundingMode` enum value onto a [`RoundMode`]. The checker types the argument as
/// `RoundingMode` (the injected enum), so at runtime it is always a `Value::Enum { ty: "RoundingMode",
/// … }`; an unknown variant or wrong value is checker-unreachable, handled defensively (EV-7).
fn round_mode(v: &Value) -> Result<RoundMode, String> {
    match v {
        Value::Enum(e) if e.ty == "RoundingMode" => RoundMode::from_variant(&e.variant)
            .ok_or_else(|| format!("unknown RoundingMode variant `{}`", e.variant)),
        _ => Err(format!("RoundingMode expected, got {}", v.type_name())),
    }
}

/// `Decimal.div(decimal a, decimal b, int scale, RoundingMode mode) -> decimal` (M-NUM S2) — the exact
/// rational `a / b`, rounded to `scale` fractional digits under `mode`. `b == 0` / `scale < 0` /
/// overflow each fault cleanly (see `value::decimal_div`). Shared by both backends.
fn decimal_div_native(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [a, b, Value::Int(scale), mode] => decimal_div(a, b, *scale, round_mode(mode)?),
        _ => Err("Decimal.div expects (decimal, decimal, int, RoundingMode)".into()),
    }
}

/// `Decimal.round(decimal d, int scale, RoundingMode mode) -> decimal` (M-NUM S2) — re-scale `d` to
/// exactly `scale` fractional digits (rounding down-scale, exact up-scale). `scale < 0` / overflow
/// fault cleanly. Shared by both backends.
fn decimal_round_native(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [d, Value::Int(scale), mode] => decimal_round(d, *scale, round_mode(mode)?),
        _ => Err("Decimal.round expects (decimal, int, RoundingMode)".into()),
    }
}

pub(crate) fn decimal_natives() -> Vec<NativeFn> {
    // The injected `RoundingMode` enum (cli::inject_rounding_mode_prelude) — referenced as a bare
    // `Ty::Named`; the type resolves because a call to `div`/`round` requires `import Core.Decimal;`,
    // which triggers the injection before the checker runs (mirrors `Json` in `json_natives`).
    let rmode = || Ty::Named("RoundingMode".to_string(), vec![]);
    vec![
        NativeFn {
            module: "Core.Decimal",
            name: "of",
            params: vec![Ty::String],
            ret: Ty::Optional(Box::new(Ty::Decimal)),
            pure: true,
            eval: NativeEval::Pure(decimal_of),
            // The `__phorj_dec_of` helper is gated on by `transpile::emit_member_call` (a native's
            // `php` closure has no `&mut self` to set `uses_dec_of`). Mirrors `value::decimal_of`.
            php: |a| format!("__phorj_dec_of({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Decimal",
            name: "div",
            params: vec![Ty::Decimal, Ty::Decimal, Ty::Int, rmode()],
            ret: Ty::Decimal,
            pure: true,
            eval: NativeEval::Pure(decimal_div_native),
            // `__phorj_dec_div($a, $b, $scale, $mode)` — gated in `transpile::call`. The mode enum
            // arrives in its PHP form; the helper switches on it. Mirrors `value::decimal_div`.
            php: |a| {
                format!(
                    "__phorj_dec_div({}, {}, {}, {})",
                    parg(a, 0),
                    parg(a, 1),
                    parg(a, 2),
                    parg(a, 3)
                )
            },
        },
        NativeFn {
            module: "Core.Decimal",
            name: "round",
            params: vec![Ty::Decimal, Ty::Int, rmode()],
            ret: Ty::Decimal,
            pure: true,
            eval: NativeEval::Pure(decimal_round_native),
            // `__phorj_dec_round($d, $scale, $mode)` — gated in `transpile::call`. Mirrors
            // `value::decimal_round`.
            php: |a| {
                format!(
                    "__phorj_dec_round({}, {}, {})",
                    parg(a, 0),
                    parg(a, 1),
                    parg(a, 2)
                )
            },
        },
    ]
}

#[cfg(test)]
#[path = "decimal_tests.rs"]
mod tests;
