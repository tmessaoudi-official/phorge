//! `Core.Random` — a seeded pseudo-random generator (native-stdlib wave).
//!
//! `pure: true` (2026-06-27): the result is deterministic w.r.t. the program text — a process-global
//! xorshift64 state advanced by each call, seeded deterministically, replaying the same stream on
//! every backend. The transpiler hand-rolls the **same** xorshift64 in PHP (`__phorj_rng_*`), so a
//! seeded sequence is **byte-identical** on `run`/`runvm`/transpiled PHP and Random is gated by the
//! oracle like any other native (no longer quarantined; the prior `mt_srand`/`mt_rand` divergence is
//! gone). The PHP `>>` is arithmetic where Rust's `u64 >>` is logical, so the emitted step masks the
//! sign-extended bits (`& 0x01FFFFFFFFFFFFFF` for the `>> 7`); `GOLDEN` is emitted as its signed-i64
//! reinterpretation (the unsigned literal exceeds `PHP_INT_MAX`). Correctness is exercised in
//! `tests/random.rs` (seed determinism + `run ≡ runvm` + bounds) and `examples/random/`.
//!
//! The Rust kernel is a `xorshift64` generator: only XOR and shifts in `1..=63` (no multiply that
//! could overflow-panic in debug, no float division), and every emitted value is masked to a
//! non-negative `i64` (`< 2^63`). Seeding is deterministic and bijective (XOR with the golden-ratio
//! constant), so the same seed always replays the same stream.
//!
//! - `Random.seed(int) -> void` — reset the generator to a deterministic state for that seed.
//! - `Random.nextInt() -> int` — advance and return the raw non-negative value.
//! - `Random.nextFloat() -> float` — advance and return a value in `[0.0, 1.0)` (dyadic, exact).
//! - `Random.intBetween(int lo, int hi) -> int` — advance and return a value in `[lo, hi]` (inclusive).

use super::*;
use crate::types::Ty;
use crate::value::Value;
use std::sync::RwLock;

/// The golden-ratio odd constant (`2^64 / φ`), used to mix the seed and as the non-zero fallback (a
/// `xorshift` state must never be zero, or it sticks at zero forever).
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// The process-wide generator state. A global because a `phg run` is one program in one process; the
/// Rust backends share it so `run ≡ runvm`. `RwLock` so a program can re-seed mid-run. Initialized to
/// a fixed non-zero constant, so an unseeded program is still deterministic.
static RANDOM_STATE: RwLock<u64> = RwLock::new(GOLDEN);

/// One `xorshift64` step: mutate `state` in place and return the new state. Pure shifts/XOR — no
/// overflow, no float (the plan's arithmetic constraints), so it is panic-safe in a debug build.
fn step(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// Advance the global generator and return the raw value masked to a non-negative `i64` (`< 2^63`).
fn advance() -> i64 {
    let mut g = RANDOM_STATE.write().unwrap_or_else(|e| e.into_inner());
    let raw = step(&mut g);
    (raw & (i64::MAX as u64)) as i64
}

fn random_seed(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(seed)] => {
            // Bijective mix; a zero result (only when `seed == GOLDEN`) falls back to GOLDEN so the
            // state is never zero.
            let mut s = (*seed as u64) ^ GOLDEN;
            if s == 0 {
                s = GOLDEN;
            }
            *RANDOM_STATE.write().unwrap_or_else(|e| e.into_inner()) = s;
            Ok(Value::Unit)
        }
        _ => Err("Random.seed expects (int)".into()),
    }
}

fn random_next(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Int(advance())),
        _ => Err("Random.nextInt expects ()".into()),
    }
}

/// Advance and return a float in `[0.0, 1.0)`. Uses the top 53 bits of the (non-negative, 63-bit)
/// step output as the mantissa, divided by `2^53` — a dyadic rational `k / 2^53` with `k < 2^53`, so
/// it is *exactly* representable in `f64` and the identical division is exact in PHP (both operands
/// exactly representable). This keeps `nextFloat` byte-identical on `run`/`runvm`/transpiled PHP,
/// unlike an irrational float (see KNOWN_ISSUES) — the value is a clean fraction, not a transcendental.
fn advance_float() -> f64 {
    // `advance()` is a non-negative i64 (< 2^63, 63 significant bits); `>> 10` keeps the top 53.
    let mantissa = (advance() as u64) >> 10;
    // 2^53 = 9007199254740992. Division of an integer < 2^53 by 2^53 is exact in IEEE-754.
    (mantissa as f64) / 9_007_199_254_740_992.0
}

fn random_next_float(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Float(advance_float())),
        _ => Err("Random.nextFloat expects ()".into()),
    }
}

fn random_int_between(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(lo), Value::Int(hi)] => {
            if hi < lo {
                return Err("Random.intBetween: hi must be >= lo".into());
            }
            // `hi - lo + 1` fits in i128 to avoid overflow on an extreme span, then back to i64.
            let span = (i128::from(*hi) - i128::from(*lo) + 1) as i64;
            let r = advance() % span;
            Ok(Value::Int(lo + r))
        }
        _ => Err("Random.intBetween expects (int, int)".into()),
    }
}

/// The `Core.Random` registry entries. `pure: true` (2026-06-27): the PHP emission **hand-rolls the
/// same xorshift64** (`__phorj_rng_*` helpers) rather than PHP's Mersenne-Twister, so a seeded
/// sequence is **byte-identical** on `run`/`runvm`/transpiled PHP — Random rejoins the oracle and
/// reproducibility survives transpile. The kernel itself is still process-global state (the result of
/// a call depends on prior calls), but it is *deterministic w.r.t. the program text*, which is what
/// `pure` means here; an unseeded program replays the same default-`GOLDEN` stream on every backend.
pub(crate) fn random_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Random",
            name: "seed",
            params: vec![Ty::Int],
            ret: Ty::Void,
            pure: true,
            eval: NativeEval::Pure(random_seed),
            php: |a| format!("__phorj_rng_seed({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Random",
            name: "nextInt",
            params: vec![],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(random_next),
            php: |_| "__phorj_rng_next()".to_string(),
        },
        NativeFn {
            module: "Core.Random",
            name: "nextFloat",
            params: vec![],
            ret: Ty::Float,
            pure: true,
            eval: NativeEval::Pure(random_next_float),
            php: |_| "__phorj_rng_next_float()".to_string(),
        },
        NativeFn {
            module: "Core.Random",
            name: "intBetween",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            pure: true,
            eval: NativeEval::Pure(random_int_between),
            php: |a| format!("__phorj_rng_int_between({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

#[cfg(test)]
#[path = "random_tests.rs"]
mod tests;
