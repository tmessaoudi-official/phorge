//! `Core.Runtime` — process introspection for MANUAL benchmarking (M-DOGFOOD W1).
//!
//! These read the live process — a monotonic high-resolution clock and resident memory — so they
//! are inherently NON-DETERMINISTIC and marked `pure: false`. A program importing `Core.Runtime` is
//! therefore QUARANTINED from the byte-identity differential (exactly like `Core.Time` /
//! `Core.Process`): its output varies per run, so it cannot be a gated example. This is the
//! sanctioned "roll your own benchmark harness" seam that complements the external
//! `phg bench --vs-php` — the user asked to time and measure memory from inside a program, and this
//! is how without breaking the `run ≡ runvm ≡ PHP` spine (quarantine, not identity).
//!
//! PHP emission is best-effort-EQUIVALENT, not byte-identical (the quarantine makes identity moot):
//! `monotonicNanos → hrtime(true)`, `memoryBytes → memory_get_usage(true)`, `peakMemoryBytes →
//! memory_get_peak_usage(true)`, `resetPeakMemory → memory_reset_peak_usage()` (PHP 8.2+).
//!
//! Memory sampling is Linux-only (`/proc/self/status`, via `crate::mem`); on a platform where it is
//! unavailable the byte counters return `0` (documented — never a panic).

use super::*;
use crate::types::Ty;
use crate::value::Value;
use std::sync::OnceLock;
use std::time::Instant;

/// Process-start reference for the monotonic clock. `monotonicNanos()` returns nanoseconds elapsed
/// since this point — monotonic and immune to wall-clock adjustments (unlike `Core.Time`). Lazily
/// initialized on first read (`Instant::now()` cannot run at `const` init).
fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn runtime_monotonic_nanos(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => {
            let ns = start().elapsed().as_nanos();
            Ok(Value::Int(i64::try_from(ns).unwrap_or(i64::MAX)))
        }
        _ => Err("Runtime.monotonicNanos expects ()".into()),
    }
}

/// Current resident set size in bytes (`0` if the platform can't sample it).
fn runtime_memory_bytes(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Int(kb_to_bytes(crate::mem::current_rss_kb()))),
        _ => Err("Runtime.memoryBytes expects ()".into()),
    }
}

/// Peak resident set size in bytes since process start (or the last `resetPeakMemory`).
fn runtime_peak_memory_bytes(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Int(kb_to_bytes(crate::mem::peak_rss_kb()))),
        _ => Err("Runtime.peakMemoryBytes expects ()".into()),
    }
}

fn runtime_reset_peak_memory(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [] => {
            crate::mem::reset_peak_rss();
            Ok(Value::Unit)
        }
        _ => Err("Runtime.resetPeakMemory expects ()".into()),
    }
}

/// KiB → bytes, saturating; `None` (unsampleable platform) → `0`. Never panics (EV-7).
fn kb_to_bytes(kb: Option<u64>) -> i64 {
    kb.map(|kb| i64::try_from(kb.saturating_mul(1024)).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// The `Core.Runtime` registry entries. All `pure: false` — they read the live process, so a program
/// that imports this module is quarantined from the byte-identity differential (see the module docs).
pub(crate) fn runtime_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Runtime",
            name: "monotonicNanos",
            params: vec![],
            ret: Ty::Int,
            pure: false,
            eval: NativeEval::Pure(runtime_monotonic_nanos),
            php: |_| "hrtime(true)".to_string(),
        },
        NativeFn {
            module: "Core.Runtime",
            name: "memoryBytes",
            params: vec![],
            ret: Ty::Int,
            pure: false,
            eval: NativeEval::Pure(runtime_memory_bytes),
            php: |_| "memory_get_usage(true)".to_string(),
        },
        NativeFn {
            module: "Core.Runtime",
            name: "peakMemoryBytes",
            params: vec![],
            ret: Ty::Int,
            pure: false,
            eval: NativeEval::Pure(runtime_peak_memory_bytes),
            php: |_| "memory_get_peak_usage(true)".to_string(),
        },
        NativeFn {
            module: "Core.Runtime",
            name: "resetPeakMemory",
            params: vec![],
            ret: Ty::Void,
            pure: false,
            eval: NativeEval::Pure(runtime_reset_peak_memory),
            php: |_| "memory_reset_peak_usage()".to_string(),
        },
    ]
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
