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

/// The `Core.Math` registry entries (M3 Track B Wave 2).
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
