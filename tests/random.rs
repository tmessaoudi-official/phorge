//! `Core.Random` quarantine-seam tests.
//!
//! `Core.Random` is `pure: false`: it advances a process-global generator, so a program importing it
//! is SKIPPED by the byte-identity differential (`uses_impure_native` in `tests/differential.rs`).
//! The Rust backends share the one generator, so `run ≡ runvm` still holds deterministically — that
//! (plus seed reproducibility and bounds) is what this dedicated suite checks. The PHP leg is *not*
//! checked here: the transpiled code uses PHP's `mt_rand`, whose sequence intentionally differs.

use phorj::cli::{cmd_run, cmd_runvm};
use std::sync::Mutex;

/// `RANDOM_STATE` is a process global, so these tests must not interleave their seed/advance calls
/// (a concurrent `seed` between this program's `seed` and `next` would corrupt the stream). Serialize
/// them; poison-tolerant so one failure doesn't cascade.
static RNG_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn seeded_random_is_deterministic_and_run_matches_runvm() {
    let _g = RNG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let src = r#"package Main;
import Core.Output;
import Core.Random;
function main() -> void {
    Random.seed(42);
    for (int i in 0..5) {
        Output.printLine("{Random.intBetween(1, 6)}");
    }
}"#;
    // A fixed seed replays the same stream on repeated runs (the global is reset by `seed`).
    let first = cmd_run(src).unwrap();
    let again = cmd_run(src).unwrap();
    assert_eq!(
        first, again,
        "a fixed seed must be reproducible across runs"
    );

    // The two Rust backends share the generator, so they agree (only the PHP leg is quarantined).
    let vm = cmd_runvm(src).unwrap();
    assert_eq!(first, vm, "run ≡ runvm under a shared generator");

    // Five rolls, each a valid d6.
    let lines: Vec<&str> = first.lines().collect();
    assert_eq!(lines.len(), 5, "five rolls");
    for l in &lines {
        let n: i64 = l.parse().expect("roll is an int");
        assert!((1..=6).contains(&n), "roll {n} out of d6 range");
    }
}

#[test]
fn distinct_seeds_diverge_across_backends_consistently() {
    let _g = RNG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prog = |seed: i64| {
        format!(
            r#"package Main;
import Core.Output;
import Core.Random;
function main() -> void {{
    Random.seed({seed});
    Output.printLine("{{Random.nextInt()}}");
}}"#
        )
    };
    let a = cmd_run(&prog(1)).unwrap();
    let b = cmd_run(&prog(2)).unwrap();
    assert_ne!(a, b, "distinct seeds should produce distinct output");
    // Each backend agrees with itself on the same seed.
    assert_eq!(cmd_runvm(&prog(1)).unwrap(), a);
}
