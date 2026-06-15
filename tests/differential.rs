//! Differential harness (M2 P2): the bytecode VM (`cmd_runvm`) must produce byte-identical
//! stdout to the tree-walking interpreter (`cmd_run`) for every P2-surface program. This is
//! the M2 correctness spine (mirrors the transpiler round-trip-against-real-PHP technique).

use phorge::cli::{cmd_run, cmd_runvm};

/// Assert the two backends agree.
fn agree(src: &str) {
    let tree = cmd_run(src).expect("interpreter ok");
    let vm = cmd_runvm(src).expect("vm ok");
    assert_eq!(tree, vm, "backend mismatch for:\n{src}\n  run={tree:?}\n  runvm={vm:?}");
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

#[test]
fn examples_that_are_p2_compatible_match() {
    // `examples/hello.phg` is P2-compatible; `examples/fib.phg` and the Shape/area sample
    // use user function calls / enums (P3/P4), so the full examples sweep arrives in P6.
    // This test documents the boundary explicitly.
    agree(r#"import std.io;

function main() {
    println("Hello, Phorge!");
}"#);
}
