//! `Core.Time` — the clock seam (M-TIME, `docs/specs/2026-06-28-m-time-design.md`).
//!
//! This module holds the ONLY native part of the time library: reading the wall clock. Everything else
//! (`Instant`/`Duration`/`Date`/`DateTime`, all calendar + formatting math) is an **injected pure-Phorj
//! prelude** (`cli::inject_time_prelude`) that runs through the same backends + transpiler as user code,
//! so it is byte-identical by construction with zero hand-rolled-PHP divergence.
//!
//! Reading the wall clock is inherently non-deterministic, which would break the byte-identity spine. So
//! the clock is **freezable** (the `Core.Random` lesson): a process-global `RwLock<Option<i64>>` holds an
//! optional frozen epoch-millis value. `Time.freeze(ms)` pins it so every shipped example/conformance
//! program is deterministic; `Time.unfreeze()` restores real-clock behavior; `Time.nowMillis()` returns
//! the frozen value when set, else the real wall clock. The transpiler hand-rolls the SAME freezable
//! clock in PHP (`__phorj_now_*`), so a frozen program is byte-identical on `run`/`runvm`/transpiled PHP.
//!
//! These natives are `pure: false` — an unfrozen `nowMillis()` depends on the environment, so it must not
//! be folded or treated as deterministic. A program that wants reproducible output freezes first.

use super::*;
use crate::types::Ty;
use crate::value::Value;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// The process-wide frozen clock. `None` = read the real wall clock; `Some(ms)` = a pinned epoch-millis
/// value (set by `Time.freeze`). A `phg run` is one program in one process, and the Rust backends share
/// this so `run ≡ runvm`.
static FROZEN: RwLock<Option<i64>> = RwLock::new(None);

/// Current epoch-millis: the frozen value if pinned, else the real wall clock. A pre-1970 system clock
/// (`duration_since` errs) is clamped to 0 — never panics.
fn now_millis() -> i64 {
    if let Some(ms) = *FROZEN.read().unwrap_or_else(|e| e.into_inner()) {
        return ms;
    }
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

fn time_now_millis(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Int(now_millis())),
        _ => Err("Time.nowMillis expects ()".into()),
    }
}

fn time_freeze(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(ms)] => {
            *FROZEN.write().unwrap_or_else(|e| e.into_inner()) = Some(*ms);
            Ok(Value::Unit)
        }
        _ => Err("Time.freeze expects (int)".into()),
    }
}

fn time_unfreeze(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => {
            *FROZEN.write().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(Value::Unit)
        }
        _ => Err("Time.unfreeze expects ()".into()),
    }
}

/// The `Core.Time` registry entries. `pure: false`: an unfrozen `nowMillis()` reads the environment, so
/// it is never deterministic w.r.t. the program text (unlike `Core.Random`, whose state is seeded from a
/// constant). The PHP emission hand-rolls the same freezable clock (`__phorj_now_*`), so a *frozen*
/// program is byte-identical across all backends.
pub(crate) fn time_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Time",
            name: "nowMillis",
            params: vec![],
            ret: Ty::Int,
            pure: false,
            eval: NativeEval::Pure(time_now_millis),
            php: |_| "__phorj_now_millis()".to_string(),
        },
        NativeFn {
            module: "Core.Time",
            name: "freeze",
            params: vec![Ty::Int],
            ret: Ty::Void,
            pure: false,
            eval: NativeEval::Pure(time_freeze),
            php: |a| format!("__phorj_now_freeze({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Time",
            name: "unfreeze",
            params: vec![],
            ret: Ty::Void,
            pure: false,
            eval: NativeEval::Pure(time_unfreeze),
            php: |_| "__phorj_now_unfreeze()".to_string(),
        },
    ]
}

#[cfg(test)]
#[path = "time_tests.rs"]
mod tests;
