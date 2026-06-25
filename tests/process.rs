//! Process/Env quarantine-seam tests under a CONTROLLED environment.
//!
//! `Core.Process`/`Core.Env` are `pure: false`: their results depend on the process, not the program
//! text, so the byte-identity differential SKIPS any program importing them (see
//! `uses_impure_native` in `tests/differential.rs`). They are instead exercised here, where the test
//! sets the args/env it expects. This crate is separate from the `#![forbid(unsafe_code)]` library,
//! so it may call the (edition-2024-`unsafe`) `std::env::set_var`.

use phorge::cli::cmd_run;
use phorge::native::set_process_args;

#[test]
fn process_args_are_visible_to_the_program() {
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
    assert_eq!(phorge::cli::cmd_runvm(src).unwrap(), cmd_run(src).unwrap());
    set_process_args(Vec::new());
}

#[test]
fn env_natives_under_controlled_environment() {
    // Env-mutation lives in ONE test fn so the (process-global, edition-2024-`unsafe`) `set_var`
    // calls aren't racing parallel test threads. Unique var names avoid cross-suite interference.
    // SAFETY: this is the only place these vars are touched, set+read+removed within this fn.
    unsafe { std::env::set_var("PHORGE_IT_PRESENT", "yes") };

    // get → value | null (composes with `??`).
    let get_src = r#"package Main;
import Core.Console;
import Core.Env;
function main() -> void {
    Console.println(Env.get("PHORGE_IT_PRESENT") ?? "<unset>");
    Console.println(Env.get("PHORGE_IT_DEFINITELY_UNSET_XYZ") ?? "<unset>");
}"#;
    assert_eq!(cmd_run(get_src).unwrap(), "yes\n<unset>\n");

    // all → a Map keyed by every env var; the set var is present, and keys come back sorted.
    let all_src = r#"package Main;
import Core.Console;
import Core.Env;
import Core.Map;
function main() -> void {
    var e = Env.all();
    Console.println("has={Map.has(e, \"PHORGE_IT_PRESENT\")}");
}"#;
    assert_eq!(cmd_run(all_src).unwrap(), "has=true\n");

    unsafe { std::env::remove_var("PHORGE_IT_PRESENT") };
}
