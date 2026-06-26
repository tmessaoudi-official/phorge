use super::*;
use crate::types::Ty;
use crate::value::Value;

fn math_sqrt(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.sqrt())),
        _ => Err("Math.sqrt expects (float)".into()),
    }
}
fn math_pow(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(b), Value::Float(e)] => Ok(Value::Float(crate::value::float_pow(*b, *e))),
        _ => Err("Math.pow expects (float, float)".into()),
    }
}
fn math_ipow(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Single-sourced with the interpreter's `int ** int` arm via `value::int_pow`: a negative
        // exponent or overflow is a clean fault (EV-7), never a panic.
        [Value::Int(b), Value::Int(e)] => crate::value::int_pow(*b, *e).map(Value::Int),
        _ => Err("Math.ipow expects (int, int)".into()),
    }
}
fn math_floor(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.floor())),
        _ => Err("Math.floor expects (float)".into()),
    }
}
fn math_ceil(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.ceil())),
        _ => Err("Math.ceil expects (float)".into()),
    }
}
fn math_abs(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // `i64::MIN.abs()` overflows; a clean fault keeps EV-7 (never panic on input).
        [Value::Int(n)] => n
            .checked_abs()
            .map(Value::Int)
            .ok_or_else(|| "integer overflow in Math.abs".to_string()),
        _ => Err("Math.abs expects (int)".into()),
    }
}
fn math_min(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok(Value::Int((*a).min(*b))),
        _ => Err("Math.min expects (int, int)".into()),
    }
}
fn math_max(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok(Value::Int((*a).max(*b))),
        _ => Err("Math.max expects (int, int)".into()),
    }
}
fn math_round(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // PHP `round()` defaults to round-half-away-from-zero, matching Rust `f64::round`; the `(int)`
        // cast then truncates the already-integral result. Saturating `as i64` keeps EV-7 (no panic on
        // a huge magnitude); examples use small exact-representable values to stay byte-identical.
        [Value::Float(x)] => Ok(Value::Int(x.round() as i64)),
        _ => Err("Math.round expects (float)".into()),
    }
}

// --- Float predicates + special values (M-NUM S3). All PHP-core (`php -n`): `is_nan`/`is_finite`/
// `is_infinite`, `NAN`/`INF`. The predicates return `bool`, so they are byte-identical even for a
// non-representable float operand (the divergence is in float *display*, not in a `bool` result).
fn math_is_nan(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Bool(x.is_nan())),
        _ => Err("Math.isNan expects (float)".into()),
    }
}
fn math_is_finite(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Bool(x.is_finite())),
        _ => Err("Math.isFinite expects (float)".into()),
    }
}
fn math_is_infinite(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Bool(x.is_infinite())),
        _ => Err("Math.isInfinite expects (float)".into()),
    }
}
fn math_nan(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Float(f64::NAN)),
        _ => Err("Math.nan expects ()".into()),
    }
}
fn math_infinity(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Float(f64::INFINITY)),
        _ => Err("Math.infinity expects ()".into()),
    }
}
fn math_neg_infinity(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Float(f64::NEG_INFINITY)),
        _ => Err("Math.negInfinity expects ()".into()),
    }
}
/// `Math.intdiv(int, int) -> int` (M-NUM S3) — integer division truncating toward zero. Single-sourced
/// with `value::int_intdiv`: `b == 0` faults `"division by zero"`, `intdiv(i64::MIN, -1)` faults
/// `"integer overflow"` (both run≡runvm via FaultKind; PHP `intdiv` throws the matching class).
fn math_intdiv(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(a), Value::Int(b)] => crate::value::int_intdiv(*a, *b).map(Value::Int),
        _ => Err("Math.intdiv expects (int, int)".into()),
    }
}

// --- Math breadth (M-NUM S4) -----------------------------------------------------------------
// Integer helpers (`sign`/`clamp`/`gcd`) return `int` → byte-identical regardless of float display.
// `gcd` has no PHP-core builtin (gmp is absent under `php -n`), so it erases to a `__phorge_gcd`
// helper. Transcendentals (`log`/`log10`/`exp`/`sin`/`cos`/`tan`/`pi`/`e`) erase to the libm builtins;
// a non-representable result diverges between Rust's shortest-round-trip and PHP, so examples exercise
// them at exact values or via `numberFormat`. `numberFormat` erases to a `__phorge_number_format`
// helper (identical string assembly both legs — see `value::number_format`).

fn math_sign(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // -1 / 0 / 1 — the sign of an int. Erases to PHP's `<=>` (spaceship), single-evaluating.
        [Value::Int(n)] => Ok(Value::Int(i64::from(*n > 0) - i64::from(*n < 0))),
        _ => Err("Math.sign expects (int)".into()),
    }
}
fn math_clamp(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // `max(lo, min(v, hi))` — mirrors the PHP emission and never panics when lo > hi (Rust's
        // `Ord::clamp` would). Identical to `(*v).min(*hi).max(*lo)`.
        [Value::Int(v), Value::Int(lo), Value::Int(hi)] => Ok(Value::Int((*v).min(*hi).max(*lo))),
        _ => Err("Math.clamp expects (int, int, int)".into()),
    }
}
fn math_gcd(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Euclid over the magnitudes (`unsigned_abs` so `i64::MIN` doesn't overflow `abs`). The
        // result overflows `i64` only for `gcd(i64::MIN, i64::MIN)`/`gcd(i64::MIN, 0)` (= 2^63) →
        // a clean fault (EV-7), never a panic.
        [Value::Int(a), Value::Int(b)] => {
            let (mut a, mut b) = (a.unsigned_abs(), b.unsigned_abs());
            while b != 0 {
                let t = b;
                b = a % b;
                a = t;
            }
            i64::try_from(a)
                .map(Value::Int)
                .map_err(|_| "integer overflow in Math.gcd".to_string())
        }
        _ => Err("Math.gcd expects (int, int)".into()),
    }
}
fn math_log(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.ln())),
        _ => Err("Math.log expects (float)".into()),
    }
}
fn math_log10(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.log10())),
        _ => Err("Math.log10 expects (float)".into()),
    }
}
fn math_exp(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.exp())),
        _ => Err("Math.exp expects (float)".into()),
    }
}
fn math_sin(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.sin())),
        _ => Err("Math.sin expects (float)".into()),
    }
}
fn math_cos(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.cos())),
        _ => Err("Math.cos expects (float)".into()),
    }
}
fn math_tan(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.tan())),
        _ => Err("Math.tan expects (float)".into()),
    }
}
fn math_pi(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Float(std::f64::consts::PI)),
        _ => Err("Math.pi expects ()".into()),
    }
}
fn math_e(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Float(std::f64::consts::E)),
        _ => Err("Math.e expects ()".into()),
    }
}
fn math_number_format(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // A negative `decimals` is clamped to 0 (matching the PHP helper), so this never faults.
        [Value::Float(v), Value::Int(d)] => Ok(Value::Str(crate::value::number_format(
            *v,
            (*d).max(0) as usize,
        ))),
        _ => Err("Math.numberFormat expects (float, int)".into()),
    }
}

/// The `Core.Math` registry entries (M3 Track B Wave 2; breadth added in M-NUM S4).
pub(crate) fn math_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Math",
            name: "sqrt",
            params: vec![Ty::Float],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_sqrt),
            php: |a| format!("sqrt({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "pow",
            params: vec![Ty::Float, Ty::Float],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_pow),
            php: |a| format!("pow({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            // `Math.ipow(int, int) -> int` — integer power as a value (the `**` operator's named twin,
            // Phase 1 operators slice). PHP's `pow` returns an `int` for non-negative int args whose
            // result fits, matching the kernel's safe domain; the negative/overflow cases fault in
            // Phorge (never reached by a byte-identity example).
            module: "Core.Math",
            name: "ipow",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(math_ipow),
            php: |a| format!("pow({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Math",
            name: "floor",
            params: vec![Ty::Float],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_floor),
            php: |a| format!("floor({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "ceil",
            params: vec![Ty::Float],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_ceil),
            php: |a| format!("ceil({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "abs",
            params: vec![Ty::Int],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(math_abs),
            php: |a| format!("abs({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "min",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(math_min),
            php: |a| format!("min({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Math",
            name: "max",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(math_max),
            php: |a| format!("max({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Math",
            name: "round",
            params: vec![Ty::Float],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(math_round),
            php: |a| format!("(int)round({})", parg(a, 0)),
        },
        // --- Float predicates + special values + intdiv (M-NUM S3) ---
        NativeFn {
            module: "Core.Math",
            name: "isNan",
            params: vec![Ty::Float],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(math_is_nan),
            php: |a| format!("is_nan({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "isFinite",
            params: vec![Ty::Float],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(math_is_finite),
            php: |a| format!("is_finite({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "isInfinite",
            params: vec![Ty::Float],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(math_is_infinite),
            php: |a| format!("is_infinite({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "nan",
            params: vec![],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_nan),
            php: |_| "NAN".to_string(),
        },
        NativeFn {
            module: "Core.Math",
            name: "infinity",
            params: vec![],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_infinity),
            php: |_| "INF".to_string(),
        },
        NativeFn {
            module: "Core.Math",
            name: "negInfinity",
            params: vec![],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_neg_infinity),
            php: |_| "-INF".to_string(),
        },
        NativeFn {
            module: "Core.Math",
            name: "intdiv",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(math_intdiv),
            php: |a| format!("intdiv({}, {})", parg(a, 0), parg(a, 1)),
        },
        // --- Math breadth (M-NUM S4) ---
        NativeFn {
            module: "Core.Math",
            name: "sign",
            params: vec![Ty::Int],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(math_sign),
            // PHP `<=>` yields -1/0/1 and evaluates its operand once (no double-emission).
            php: |a| format!("({} <=> 0)", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "clamp",
            params: vec![Ty::Int, Ty::Int, Ty::Int],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(math_clamp),
            php: |a| format!("max({}, min({}, {}))", parg(a, 1), parg(a, 0), parg(a, 2)),
        },
        NativeFn {
            module: "Core.Math",
            name: "gcd",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(math_gcd),
            php: |a| format!("__phorge_gcd({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Math",
            name: "log",
            params: vec![Ty::Float],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_log),
            php: |a| format!("log({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "log10",
            params: vec![Ty::Float],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_log10),
            php: |a| format!("log10({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "exp",
            params: vec![Ty::Float],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_exp),
            php: |a| format!("exp({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "sin",
            params: vec![Ty::Float],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_sin),
            php: |a| format!("sin({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "cos",
            params: vec![Ty::Float],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_cos),
            php: |a| format!("cos({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "tan",
            params: vec![Ty::Float],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_tan),
            php: |a| format!("tan({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Math",
            name: "pi",
            params: vec![],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_pi),
            php: |_| "M_PI".to_string(),
        },
        NativeFn {
            module: "Core.Math",
            name: "e",
            params: vec![],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(math_e),
            php: |_| "M_E".to_string(),
        },
        NativeFn {
            module: "Core.Math",
            name: "numberFormat",
            params: vec![Ty::Float, Ty::Int],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(math_number_format),
            php: |a| format!("__phorge_number_format({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

// ---- Core.Text ----------------------------------------------------------------------------------
// String natives, all concrete-typed. Each erases to a PHP string builtin (D-L9). ASCII-oriented to
// stay byte-identical with PHP: `len` is the *byte* length (PHP `strlen`), and `upper`/`lower` are
// ASCII-case (PHP `strtoupper`/`strtolower`), so multi-byte text could differ between the Rust
// backends and PHP — examples use ASCII. The run↔runvm spine is always byte-identical (both Rust).

#[cfg(test)]
#[path = "math_tests.rs"]
mod tests;
