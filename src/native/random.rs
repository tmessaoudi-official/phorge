//! `Core.Random` — a seeded pseudo-random generator (native-stdlib wave, QUARANTINED).
//!
//! These are `pure: false` natives, like `Core.Process`/`Core.Env`: they read+advance a process
//! **global** RNG state, so a call's result depends on prior calls, not on the program text alone.
//! A program that imports `Core.Random` is therefore **quarantined** from the byte-identity
//! differential (`uses_impure_native` in `tests/differential.rs`) — the Rust backends share the one
//! global generator (so `run ≡ runvm` always), but the transpiled PHP uses PHP's own Mersenne-Twister
//! (`mt_srand`/`mt_rand`), whose sequence need not match. Correctness is exercised in `tests/random.rs`
//! (seed determinism + `run ≡ runvm` + bounds) and walked through in `examples/random/` (not gated).
//!
//! The Rust kernel is a `xorshift64` generator: only XOR and shifts in `1..=63` (no multiply that
//! could overflow-panic in debug, no float division), and every emitted value is masked to a
//! non-negative `i64` (`< 2^63`). Seeding is deterministic and bijective (XOR with the golden-ratio
//! constant), so the same seed always replays the same stream.
//!
//! - `Random.seed(int) -> void` — reset the generator to a deterministic state for that seed.
//! - `Random.next() -> int` — advance and return the raw non-negative value.
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
        _ => Err("Random.next expects ()".into()),
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

/// The `Core.Random` registry entries. All `pure: false` (quarantined). The PHP emission uses PHP's
/// native Mersenne-Twister (`mt_srand`/`mt_rand`); the sequence need not match the Rust kernel because
/// importing this module excludes the program from the oracle.
pub(crate) fn random_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Random",
            name: "seed",
            params: vec![Ty::Int],
            ret: Ty::Void,
            pure: false,
            eval: NativeEval::Pure(random_seed),
            php: |a| format!("mt_srand({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Random",
            name: "next",
            params: vec![],
            ret: Ty::Int,
            pure: false,
            eval: NativeEval::Pure(random_next),
            php: |_| "mt_rand()".to_string(),
        },
        NativeFn {
            module: "Core.Random",
            name: "intBetween",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            pure: false,
            eval: NativeEval::Pure(random_int_between),
            php: |a| format!("mt_rand({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

#[cfg(test)]
#[path = "random_tests.rs"]
mod tests;
