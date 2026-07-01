//! `Core.Runtime` quarantine-seam tests (M-DOGFOOD W1).
//!
//! `Core.Runtime` natives (monotonic clock + resident-memory counters) are `pure: false`: their
//! results depend on the running process, so the byte-identity differential SKIPS any program that
//! imports them (see `uses_impure_native` in `tests/differential.rs`). They are exercised here
//! instead, asserting: (1) both Rust backends agree (they share the process, so run ≡ runvm always
//! holds — only the PHP leg is unreliable, which is why the oracle is skipped, not run≡runvm), and
//! (2) the manual-benchmark shape works end to end. This also guards the shipped walkthrough example
//! (`examples/bench/manual/`) against rot, since the differential harness never runs it.

use phorj::cli::{cmd_run, cmd_runvm};

/// A monotonic clock and memory counters used together — the manual-benchmark shape. Sanity booleans
/// (not raw numbers) keep the assertion deterministic while proving the API is wired on both backends.
const BENCH_SHAPE: &str = r#"package Main;
import Core.Output;
import Core.Runtime;
function fib(int n) -> int { if (n < 2) { return n; } return fib(n - 1) + fib(n - 2); }
function main() -> void {
    Runtime.resetPeakMemory();
    int t0 = Runtime.monotonicNanos();
    int r = fib(25);
    int elapsed = Runtime.monotonicNanos() - t0;
    Output.printLine("fib={r}");
    Output.printLine("elapsed_ok={elapsed >= 0}");
    Output.printLine("mem_ok={Runtime.memoryBytes() >= 0}");
    Output.printLine("peak_ok={Runtime.peakMemoryBytes() >= 0}");
}"#;

#[test]
fn runtime_bench_shape_runs_and_backends_agree() {
    let run = cmd_run(BENCH_SHAPE).expect("run ok");
    assert_eq!(
        run, "fib=75025\nelapsed_ok=true\nmem_ok=true\npeak_ok=true\n",
        "unexpected output: {run}"
    );
    // The Rust backends share the process, so they always agree even for impure natives.
    assert_eq!(cmd_runvm(BENCH_SHAPE).expect("runvm ok"), run);
}

#[test]
fn shipped_manual_example_runs_on_both_backends() {
    let src = std::fs::read_to_string("examples/bench/manual/stopwatch-and-memory.phg")
        .expect("read shipped manual-bench example");
    let run = cmd_run(&src).expect("shipped example must run");
    assert!(run.contains("fib(30)           = 832040"), "{run}");
    assert_eq!(
        cmd_runvm(&src).expect("shipped example must run on the VM"),
        run
    );
}
