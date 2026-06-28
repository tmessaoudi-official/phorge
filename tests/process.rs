//! Process/Env quarantine-seam tests under a CONTROLLED environment.
//!
//! `Core.Process`/`Core.Env` are `pure: false`: their results depend on the process, not the program
//! text, so the byte-identity differential SKIPS any program importing them (see
//! `uses_impure_native` in `tests/differential.rs`). They are instead exercised here, where the test
//! sets the args/env it expects. This crate is separate from the `#![forbid(unsafe_code)]` library,
//! so it may call the (edition-2024-`unsafe`) `std::env::set_var`.

use phorj::cli::cmd_run;
use phorj::cli::{cmd_run_exit, cmd_runvm_exit};
use phorj::native::set_process_args;
use std::sync::Mutex;

/// `PROCESS_ARGS` is a process global, so the argv-setting tests must not run concurrently (each sets
/// then reads it). Serialize them through this lock; poison-tolerant so one failing test doesn't cas-
/// cade. (The env test consolidates into a single fn for the same reason.)
static ARGS_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn process_args_are_visible_to_the_program() {
    let _g = ARGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_process_args(vec!["hello".into(), "world".into()]);
    let src = r#"package Main;
import Core.Console;
import Core.Process;
import Core.List;
function main() -> void {
    var a = Process.args();
    Console.println("n={List.length(a)}");
    for (string s in a) { Console.println(s); }
}"#;
    assert_eq!(cmd_run(src).unwrap(), "n=2\nhello\nworld\n");
    // `runvm` shares the same process global, so it agrees (the Rust backends always do — only the
    // PHP leg is unreliable, which is why these are quarantined from the oracle, not from run≡runvm).
    assert_eq!(phorj::cli::cmd_runvm(src).unwrap(), cmd_run(src).unwrap());
    set_process_args(Vec::new());
}

#[test]
fn env_natives_under_controlled_environment() {
    // Env-mutation lives in ONE test fn so the (process-global, edition-2024-`unsafe`) `set_var`
    // calls aren't racing parallel test threads. Unique var names avoid cross-suite interference.
    // SAFETY: this is the only place these vars are touched, set+read+removed within this fn.
    unsafe { std::env::set_var("PHORJ_IT_PRESENT", "yes") };

    // get → value | null (composes with `??`).
    let get_src = r#"package Main;
import Core.Console;
import Core.Env;
function main() -> void {
    Console.println(Env.get("PHORJ_IT_PRESENT") ?? "<unset>");
    Console.println(Env.get("PHORJ_IT_DEFINITELY_UNSET_XYZ") ?? "<unset>");
}"#;
    assert_eq!(cmd_run(get_src).unwrap(), "yes\n<unset>\n");

    // all → a Map keyed by every env var; the set var is present, and keys come back sorted.
    let all_src = r#"package Main;
import Core.Console;
import Core.Env;
import Core.Map;
function main() -> void {
    var e = Env.all();
    Console.println("has={Map.has(e, \"PHORJ_IT_PRESENT\")}");
}"#;
    assert_eq!(cmd_run(all_src).unwrap(), "has=true\n");

    unsafe { std::env::remove_var("PHORJ_IT_PRESENT") };
}

// --- Batch-1 B: entry-point exit codes + argv-to-main ------------------------------------------

#[test]
fn main_int_return_is_the_exit_code() {
    // `main(): int` — the returned int is the process exit code; stdout is unaffected, and both
    // backends agree on (stdout, exit).
    let src = r#"package Main;
import Core.Console;
function main(): int {
    Console.println("done");
    return 7;
}"#;
    assert_eq!(cmd_run_exit(src).unwrap(), ("done\n".to_string(), 7));
    assert_eq!(cmd_runvm_exit(src).unwrap(), ("done\n".to_string(), 7));
}

#[test]
fn main_void_exits_zero() {
    let src = r#"package Main;
import Core.Console;
function main(): void { Console.println("hi"); }"#;
    assert_eq!(cmd_run_exit(src).unwrap(), ("hi\n".to_string(), 0));
    assert_eq!(cmd_runvm_exit(src).unwrap(), ("hi\n".to_string(), 0));
}

#[test]
fn main_receives_argv_as_a_parameter() {
    // `main(List<string> args)` — the param is bound to the same argv `Core.Process.args()` exposes.
    let _g = ARGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_process_args(vec!["a".into(), "bb".into(), "ccc".into()]);
    let src = r#"package Main;
import Core.Console;
import Core.List;
function main(List<string> args): int {
    for (string s in args) { Console.println(s); }
    return List.length(args);
}"#;
    assert_eq!(cmd_run_exit(src).unwrap(), ("a\nbb\nccc\n".to_string(), 3));
    assert_eq!(
        cmd_runvm_exit(src).unwrap(),
        ("a\nbb\nccc\n".to_string(), 3)
    );
    set_process_args(Vec::new());
}
