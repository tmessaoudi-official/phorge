//! Differential harness (M2 P3): the bytecode VM (`cmd_runvm`) must produce byte-identical
//! stdout to the tree-walking interpreter (`cmd_run`) for every P1–P3-surface program. This is
//! the M2 correctness spine (mirrors the transpiler round-trip-against-real-PHP technique).
//!
//! Parity covers *both* success and failure (M2 P3.5 Wave 0): `agree` checks the `Ok` output,
//! `agree_err` checks that a failing program faults the *same way* on both backends. Faults are
//! compared by semantic [`FaultKind`] rather than raw error text — the two backends share fault
//! bodies (e.g. `"division by zero"`) but the CLI wraps them with stage-specific prefixes
//! (`"runtime error:"` vs `"compile error:"`), so a raw `assert_eq!` would spuriously fail.

use phorge::cli::{cmd_run, cmd_runvm};

/// Assert the two backends agree on success output. Compares `Result` values structurally
/// (never `.expect()`): in release builds an unchecked-arithmetic divergence surfaces as an
/// `Err` rather than a panic, and a structural compare reports it as a clean mismatch.
fn agree(src: &str) {
    let tree = cmd_run(src);
    let vm = cmd_runvm(src);
    assert_eq!(
        tree, vm,
        "backend mismatch for:\n{src}\n  run={tree:?}\n  runvm={vm:?}"
    );
}

/// Semantic classification of a runtime fault, independent of the CLI's stage-specific prefix.
/// Compared instead of raw error strings so backends that fault for the *same reason* at
/// *different pipeline stages* still register as agreeing.
#[derive(Debug, PartialEq, Eq)]
enum FaultKind {
    IntOverflow,
    DivZero,
    ModZero,
    StackOverflow,
    Unsupported,
    /// Anything the corpus doesn't yet classify — carried verbatim so a mismatch stays legible.
    Other(String),
}

/// Map a rendered error message to its [`FaultKind`] by matching on the fault *body*
/// (substring), which ignores the `"runtime error:"` / `"compile error:"` / `"… at L:C:"`
/// prefix the CLI prepends per pipeline stage.
fn classify(err: &str) -> FaultKind {
    if err.contains("integer overflow") {
        FaultKind::IntOverflow
    } else if err.contains("division by zero") {
        FaultKind::DivZero
    } else if err.contains("modulo by zero") {
        FaultKind::ModZero
    } else if err.contains("stack overflow") {
        FaultKind::StackOverflow
    } else if err.contains("unsupported") || err.contains("compile error") {
        FaultKind::Unsupported
    } else {
        FaultKind::Other(err.to_string())
    }
}

/// Assert both backends *fail*, and fail with the same [`FaultKind`]. A backend that returns
/// `Ok` classifies to `None`, so an `Ok`-vs-`Err` divergence is flagged too.
fn agree_err(src: &str) {
    let tree = cmd_run(src);
    let vm = cmd_runvm(src);
    let tree_kind = tree.as_ref().err().map(|e| classify(e));
    let vm_kind = vm.as_ref().err().map(|e| classify(e));
    assert_eq!(
        tree_kind, vm_kind,
        "fault-kind mismatch for:\n{src}\n  run={tree:?}\n  runvm={vm:?}"
    );
    assert!(
        tree_kind.is_some(),
        "expected a fault but both backends succeeded for:\n{src}\n  run={tree:?}"
    );
}

/// Programs spanning the whole P2 surface. Each must run identically on both backends.
const P2_PROGRAMS: &[&str] = &[
    // literals + interpolation
    r#"function main() { println("hello"); }"#,
    r#"function main() { println("{42}"); println("{3.14}"); println("{true}"); }"#,
    // int + float arithmetic (formatting parity: 12.0 -> "12")
    r#"function main() { println("{1 + 2 * 3 - 4}"); }"#,
    r#"function main() { println("{2.0 * 3.0}"); println("{7.5 / 2.5}"); }"#,
    r#"function main() { println("{7 % 3}"); println("{7.5 % 2.0}"); }"#,
    // comparison + equality + logical short-circuit
    r#"function main() { println("{1 < 2}"); println("{2 <= 2}"); println("{3 > 4}"); }"#,
    r#"function main() { println("{1 == 1}"); println("{1 != 2}"); }"#,
    r#"function main() { println("{1 < 2 && 2 < 3}"); println("{1 > 2 || 3 > 2}"); }"#,
    // unary
    r#"function main() { println("{-5}"); println("{!false}"); }"#,
    // locals (int + float + string + bool)
    r#"function main() { int x = 10; float y = 2.5; println("{x}"); println("{y}"); }"#,
    r#"function main() { string s = "hi"; bool b = true; println("{s}"); println("{b}"); }"#,
    r#"function main() { int a = 3; int b = 4; println("{a * a + b * b}"); }"#,
    // if / else
    r#"function main() { if (1 < 2) { println("a"); } else { println("b"); } }"#,
    r#"function main() { int n = 5; if (n > 3) { println("big"); } println("end"); }"#,
    // for-in over list literals
    r#"function main() { List<int> xs = [1, 2, 3]; for (int x in xs) { println("{x}"); } }"#,
    r#"function main() { for (float f in [1.5, 2.5]) { println("{f * 2.0}"); } }"#,
    // nested blocks + for body locals
    r#"function main() { for (int x in [10, 20]) { int y = x + 1; println("{y}"); } }"#,
    // NB: `println` is single-arg only (the checker enforces it) — no multi-arg case here.
];

#[test]
fn p2_programs_match_between_backends() {
    for src in P2_PROGRAMS {
        agree(src);
    }
}

/// P3 surface: user function calls, recursion, mutual recursion, void functions, returns in
/// branches, nested calls, float-returning functions, and calls as statements. Each must run
/// identically on both backends.
const P3_PROGRAMS: &[&str] = &[
    // single call used in interpolation
    r#"function inc(int n) -> int { return n + 1; } function main() { println("{inc(41)}"); }"#,
    // multiple params + call inside arithmetic
    r#"function add(int a, int b) -> int { return a + b; }
       function main() { println("{add(2, 3) * 10}"); }"#,
    // recursion (classic fib)
    r#"function fib(int n) -> int {
           if (n < 2) { return n; }
           return fib(n - 1) + fib(n - 2);
       }
       function main() { println("{fib(12)}"); }"#,
    // return in a branch vs fall-through
    r#"function sign(int n) -> int { if (n < 0) { return -1; } return 1; }
       function main() { println("{sign(-9)}"); println("{sign(4)}"); }"#,
    // mutual recursion (forward reference: isEven calls isOdd declared later)
    r#"function isEven(int n) -> bool { if (n == 0) { return true; } return isOdd(n - 1); }
       function isOdd(int n) -> bool { if (n == 0) { return false; } return isEven(n - 1); }
       function main() { println("{isEven(10)}"); println("{isOdd(7)}"); }"#,
    // nested calls
    r#"function sq(int n) -> int { return n * n; }
       function main() { println("{sq(sq(2))}"); }"#,
    // float-returning function in float arithmetic
    r#"function half(float x) -> float { return x / 2.0; }
       function main() { println("{half(5.0) + 1.0}"); }"#,
    // void function (no return type) called for its side effect
    r#"function greet(string who) { println("hi, {who}"); }
       function main() { greet("Phorge"); greet("world"); }"#,
    // call used as a statement (return value discarded)
    r#"function noisy(int n) -> int { println("got {n}"); return n; }
       function main() { noisy(42); println("done"); }"#,
];

#[test]
fn p3_programs_match_between_backends() {
    for src in P3_PROGRAMS {
        agree(src);
    }
}

#[test]
fn examples_match_between_backends() {
    // `examples/hello.phg` (P2) and `examples/fib.phg` (P3 recursion) both run on the VM.
    // `examples/grades.phg` and the Shape/area sample use enums/classes/`match` (P4), so the
    // full examples sweep arrives in P6. This test documents the boundary explicitly.
    agree(
        r#"import std.io;

function main() {
    println("Hello, Phorge!");
}"#,
    );
    let fib = std::fs::read_to_string("examples/fib.phg").expect("read examples/fib.phg");
    agree(&fib);
}

/// Error-parity corpus (M2 P3.5 Wave 0): programs that must *fail identically* on both backends.
/// `i64::MIN` is reached via `-9223372036854775807 - 1` because the bare literal `9223372036854775808`
/// overflows `i64` at lex time. Negating it (`-x`) is the `Op::Neg` overflow that previously panicked
/// the VM while the interpreter reported a clean error. Deep-recursion (`StackOverflow`) and
/// unsupported-construct cases join this corpus alongside their guards in Wave 0 Task 0.3.
const ERR_PROGRAMS: &[&str] = &[
    // integer overflow: negating i64::MIN
    r#"function main() { int x = -9223372036854775807 - 1; println("{-x}"); }"#,
    // integer overflow: i64::MAX + 1
    r#"function main() { println("{9223372036854775807 + 1}"); }"#,
    // division by zero
    r#"function main() { int z = 0; println("{1 / z}"); }"#,
    // modulo by zero
    r#"function main() { int z = 0; println("{1 % z}"); }"#,
    // unbounded recursion: trips the shared `MAX_CALL_DEPTH` guard on both backends.
    // Before Task 0.3 the interpreter recursed on the native stack and SIGABRTed (exit 134)
    // while the VM cleanly reported "stack overflow" — a parity divergence in the fault path.
    r#"function rec(int n) -> int { return rec(n) + 1; } function main() { println("{rec(0)}"); }"#,
];

#[test]
fn error_parity_between_backends() {
    for src in ERR_PROGRAMS {
        agree_err(src);
    }
}

/// Pathological nesting must fault *identically* on both backends (M2 P3.5 Wave 0, Task 0.4).
/// The recursive-descent parser caps nesting depth, so deeply-nested parens / unary chains return
/// a clean parse `Diagnostic` instead of a native stack overflow (SIGABRT). Both backends share the same
/// parser, so the rendered fault is byte-identical. 5000 levels is well past the 512 limit. Built
/// programmatically rather than as a string literal to keep the corpus readable.
#[test]
fn deep_nesting_faults_identically() {
    let parens = format!(
        "function main() {{ int x = {}1{}; println(\"{{x}}\"); }}",
        "(".repeat(5000),
        ")".repeat(5000),
    );
    agree_err(&parens);
    let unary = format!(
        "function main() {{ bool b = {}true; println(\"{{b}}\"); }}",
        "!".repeat(5000),
    );
    agree_err(&unary);
    // A long left-associative chain is built *iteratively*, so it escapes the parser's nesting
    // limit but produces a deeply left-leaning AST. The checker's depth guard (the gate both
    // backends share) must fault it identically rather than letting a walker overflow its stack.
    let chain = format!(
        "function main() {{ int x = 1{}; println(\"{{x}}\"); }}",
        "+1".repeat(20_000),
    );
    agree_err(&chain);
}
