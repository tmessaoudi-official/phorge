//! `Core.Process` / `Core.Environment` — the ambient-environment natives (M-Batteries kickoff,
//! `docs/specs/2026-06-25-process-io-quarantine-seam-design.md`).
//!
//! These are the first `pure: false` natives: their results depend on the *process* (its argv / env
//! vars), not the program text. So a program that calls one is **quarantined** from the byte-identity
//! differential — the PHP leg runs in a separate process whose argv/env need not match the Rust
//! process, and the output isn't a fixed golden (it depends on the machine). They are tested
//! separately under a controlled environment in `tests/process.rs`, with a walkthrough (not a gated
//! example) under `examples/process/`.
//!
//! - `Core.Process.args() -> List<string>` — program arguments (everything after `phg run f.phg --`).
//! - `Core.Environment.get(name) -> string?` — one environment variable, or `null` if unset.
//! - `Core.Environment.all() -> Map<string, string>` — every environment variable, **sorted by key** (Q4) so
//!   the result is stable (OS iteration order is not).

use super::*;
use crate::types::Ty;
use crate::value::{HKey, Value};
use std::rc::Rc;
use std::sync::RwLock;

/// Program arguments visible to `Core.Process.args()`. A process global because a `phg run` is one
/// program in one process (Q3-b): the CLI populates it from the `--`-terminated tail before running,
/// a standalone built binary from the real `argv`, and `tests/process.rs` directly. `RwLock` (not
/// `OnceLock`) so tests can set it per-case.
static PROCESS_ARGS: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Set the arguments returned by `Core.Process.args()`. Called before running a program.
pub fn set_process_args(args: Vec<String>) {
    if let Ok(mut g) = PROCESS_ARGS.write() {
        *g = args;
    }
}

fn process_args(_args: &[Value], _: &mut String) -> Result<Value, String> {
    let items = PROCESS_ARGS
        .read()
        .map(|g| g.iter().map(|s| Value::Str(s.clone())).collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(Value::List(Rc::new(items)))
}

/// The current process args as a `Value::List<string>` — the value `Core.Process.args()` returns and
/// the value bound to `main`'s optional `List<string>` parameter (Batch-1 B). Single-sourced through
/// [`process_args`] so both surfaces stay identical.
pub fn process_args_value() -> Value {
    process_args(&[], &mut String::new()).unwrap_or_else(|_| Value::List(Rc::new(Vec::new())))
}

fn env_get(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // An unset variable is `null` (string?); composes with `??` / if-let like any optional.
        [Value::Str(name)] => Ok(std::env::var(name).map_or(Value::Null, Value::Str)),
        _ => Err("Environment.get expects (string)".into()),
    }
}

fn env_all(_args: &[Value], _: &mut String) -> Result<Value, String> {
    // Sorted by key (Q4): OS env iteration order is unspecified, so a stable order makes the result
    // deterministic within the Rust backends (these are quarantined from the PHP oracle anyway).
    let mut pairs: Vec<(String, String)> = std::env::vars().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let map: Vec<(HKey, Value)> = pairs
        .into_iter()
        .map(|(k, v)| (HKey::Str(k), Value::Str(v)))
        .collect();
    Ok(Value::Map(Rc::new(map)))
}

pub(crate) fn process_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Process",
            name: "arguments",
            params: vec![],
            ret: Ty::List(Box::new(Ty::String)),
            pure: false,
            eval: NativeEval::Pure(process_args),
            // PHP: the args after the script name. `$argv` exists under the CLI SAPI (register_argc_argv).
            php: |_| "array_slice($argv ?? [], 1)".to_string(),
        },
        NativeFn {
            module: "Core.Environment",
            name: "get",
            params: vec![Ty::String],
            ret: Ty::Optional(Box::new(Ty::String)),
            pure: false,
            // `getenv` returns `false` when unset → coerce to `null`. The arg is single-evaluated via
            // an assignment-expression temp (`$__phorj_env`), which Phorj variables never collide with.
            eval: NativeEval::Pure(env_get),
            php: |a| {
                format!(
                    "(($__phorj_env = getenv({})) === false ? null : $__phorj_env)",
                    parg(a, 0)
                )
            },
        },
        NativeFn {
            module: "Core.Environment",
            name: "all",
            params: vec![],
            ret: Ty::Map(Box::new(Ty::String), Box::new(Ty::String)),
            pure: false,
            eval: NativeEval::Pure(env_all),
            // `getenv()` (no arg) returns all vars as an assoc array (PHP 7.1+); `ksort` matches the
            // sorted-by-key Rust result.
            php: |_| "(function(){$e=getenv();ksort($e);return $e;})()".to_string(),
        },
    ]
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
