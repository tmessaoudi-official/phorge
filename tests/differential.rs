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
    agree(r#"import std.io;

function main() {
    println("Hello, Phorge!");
}"#);
    let fib = std::fs::read_to_string("examples/fib.phg").expect("read examples/fib.phg");
    agree(&fib);
}
