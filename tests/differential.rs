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
    /// Reading a field absent from an instance — a checker-valid, runtime-reachable fault when an
    /// explicit (uninitialized) `Field` member is read (construction only populates promoted ctor
    /// params). Classified by body substring so the VM's line prefix doesn't split it from the
    /// interpreter's prefix-less rendering (M2 P4b).
    NoField,
    /// A list index outside `0..len` — a checker-valid, runtime-reachable fault (the checker proves
    /// the index is an `int`, never that it is in range). Classified by body substring so the VM's
    /// `runtime error at N:` line prefix doesn't split it from the interpreter's prefix-less render
    /// (M3 S1.1). Without this arm an OOB program would fall to `Other(full_string)` and the line
    /// prefix would spuriously fail `agree_err`.
    IndexOob,
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
    } else if err.contains("list index out of range") {
        FaultKind::IndexOob
    } else if err.contains("no field") {
        FaultKind::NoField
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

/// M3 S0.2 — `var` local type inference is a front-end-only feature (type erased after checking),
/// so both backends must run a `var` program byte-identically.
#[test]
fn s0_var_inference_is_byte_identical() {
    agree(
        r#"function main() {
            var x = 21;
            var s = "n=";
            println("{s}{x + x}");
        }"#,
    );
}

/// `var` whose initializer is a call result and a `match` value — both must specialize arithmetic
/// identically (the compiler infers the local's `CTy` from the initializer, not an annotation).
#[test]
fn s0_var_inference_from_call_and_match_inits() {
    agree(
        r#"function dbl(int n) -> int { return n * 2; }
        function main() {
            var a = dbl(10);
            var b = match a { 20 => 100, n => n };
            println("{a + b}");
        }"#,
    );
}

/// M3 S0.3 — a `type` alias is compile-time-only (erased); resolving params/returns through it
/// must not change runtime behavior on either backend.
#[test]
fn s0_type_alias_is_byte_identical() {
    agree(
        r#"type Count = int;
        function tally(Count n) -> Count { return n + 1; }
        function main() { println("{tally(41)}"); }"#,
    );
}

/// M3 S1.1 — list indexing `xs[i]`. The checker already typed it; the backends were un-rejected
/// this slice. Reads must be byte-identical, and an out-of-range read must *fault* identically
/// (the VM's bounds check + the interpreter's must agree — `FaultKind::IndexOob`).
#[test]
fn s1_indexing_is_byte_identical() {
    agree(r#"function main() { List<int> xs = [10, 20, 30]; println("{xs[0]} {xs[2]}"); }"#);
    // an index expression on a list literal, with the index coming from a loop variable
    agree(r#"function main() { for (int i in [0, 1, 2]) { println("{[5, 6, 7][i]}"); } }"#);
}

#[test]
fn s1_index_oob_faults_identically() {
    agree_err(r#"function main() { List<int> xs = [1, 2]; println("{xs[5]}"); }"#);
}

/// An index *result* used as an arithmetic operand (`xs[0] + 1`). The compiler must know the list's
/// element type to pick `AddI`/`AddF` — so `CTy` tracks `List<elem>` and `ctype(Index)` unwraps it.
/// (Regression guard: un-rejecting indexing without this made the VM compile-reject `xs[0] + 1`
/// while the interpreter accepted it.)
#[test]
fn s1_index_result_in_arithmetic_is_byte_identical() {
    agree(r#"function main() { List<int> xs = [10, 20]; println("{xs[0] + 1}"); }"#);
    agree(r#"function main() { List<float> fs = [1.5, 2.5]; println("{fs[0] + fs[1]}"); }"#);
    // index result of a range-materialized list, used arithmetically
    agree(r#"function main() { var xs = 0..5; println("{xs[2] * 10}"); }"#);
}

/// M3 S1.2 — integer ranges `a..b` (exclusive) / `a..=b` (inclusive), materialized to `List<int>`
/// via the one new `Op::MakeRange`. The compiler/interpreter must build the *same* list (same order,
/// same emptiness rule) so `for…in` over a range is byte-identical on both backends.
#[test]
fn s1_ranges_are_byte_identical() {
    agree(r#"function main() { for (int i in 0..3) { println("{i}"); } }"#); // 0,1,2
    agree(r#"function main() { for (int i in 1..=3) { println("{i}"); } }"#); // 1,2,3
                                                                              // empty range (start >= end): the body never runs on either backend
    agree(r#"function main() { for (int i in 5..5) { println("{i}"); } println("done"); }"#);
    agree(r#"function main() { for (int i in 5..2) { println("{i}"); } println("empty"); }"#);
    // a range bound to a `var` (typed `List<int>`), then iterated
    agree(r#"function main() { var xs = 0..3; for (int i in xs) { println("{i + 1}"); } }"#);
    // range bounds from expressions
    agree(
        r#"function lo() -> int { return 2; } function main() { for (int i in lo()..lo() + 3) { println("{i}"); } }"#,
    );
}

/// M3 S1.3 — expression `if` (`if (c) { e } else { e }`) in value position. No new `Op` — it lowers
/// to the existing branch ops (like `&&`/`||`/`match`), so both backends leave the same single value
/// on the stack and must agree.
#[test]
fn s1_expression_if_is_byte_identical() {
    // value-position in a `var` initializer, then used arithmetically (specialization parity)
    agree(r#"function main() { var x = if (1 < 2) { 10 } else { 20 }; println("{x + x}"); }"#);
    // in return position, both branches taken across two calls
    agree(
        r#"function pick(bool b) -> int { return if (b) { 1 } else { 2 }; }
           function main() { println("{pick(true)} {pick(false)}"); }"#,
    );
    // as a call argument (string-typed branches), inside a range loop
    agree(
        r#"function main() { for (int i in 0..3) { println(if (i == 1) { "one" } else { "x" }); } }"#,
    );
    // nested / float branches
    agree(r#"function main() { float r = if (true) { 1.5 } else { 2.5 }; println("{r * 2.0}"); }"#);
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

/// P4a surface: single-payload enums + exhaustive `match`. Construction (`Variant(args)` and
/// bare `Variant`), `match` in both return and var-decl-init position, variant/literal/wildcard/
/// binding patterns, and payload destructuring. Each must run identically on both backends.
const P4A_PROGRAMS: &[&str] = &[
    // payload enum, variant patterns binding the payload, `match` in return position
    r#"enum Grade { Pass(int score), Fail(int score), }
       function describe(Grade g) -> string {
           return match g {
               Pass(s) => "PASS ({s})",
               Fail(s) => "FAIL ({s})",
           };
       }
       function main() { println(describe(Pass(90))); println(describe(Fail(40))); }"#,
    // bare (no-payload) variants, wildcard arm, `match` in var-decl-init position
    r#"enum Color { Red, Green, Blue, }
       function main() {
           Color c = Green;
           string name = match c { Red => "red", Green => "green", _ => "other", };
           println(name);
       }"#,
    // literal int patterns + catch-all binding used in interpolation
    r#"function label(int n) -> string {
           return match n { 0 => "zero", 1 => "one", x => "many ({x})", };
       }
       function main() { println(label(0)); println(label(1)); println(label(7)); }"#,
    // bool literal patterns
    r#"function yn(bool b) -> string { return match b { true => "Y", false => "N", }; }
       function main() { println(yn(true)); println(yn(false)); }"#,
    // string literal patterns + wildcard
    r#"function kind(string s) -> string {
           return match s { "a" => "first", "b" => "second", _ => "rest", };
       }
       function main() { println(kind("a")); println(kind("b")); println(kind("z")); }"#,
    // enum value flows through a local and equality (`==` on enums) before matching
    r#"enum Dir { N, S, }
       function main() {
           Dir d = N;
           println("{d == N}");
           string t = match d { N => "north", S => "south", };
           println(t);
       }"#,
    // `match` in a *transient* position: as the rhs of `+`, with the lhs already on the operand
    // stack (exercises the compiler's operand-height tracking for the scrutinee slot).
    r#"function g(int n) -> int { return 1 + match n { 0 => 10, _ => 20 }; }
       function main() { println("{g(0)}"); println("{g(5)}"); }"#,
    // nested `match` whose inner arm references the *outer* arm's binding (re-extraction across
    // two live scrutinees — the hardest binding/height case in P4a).
    r#"enum Pair { P(int a, int b), }
       function f(Pair p) -> string {
           return match p {
               P(a, b) => match a { 0 => "first=zero b={b}", _ => "a={a} b={b}", },
           };
       }
       function main() { println(f(P(0, 9))); println(f(P(5, 2))); }"#,
];

#[test]
fn p4a_programs_match_between_backends() {
    for src in P4A_PROGRAMS {
        agree(src);
    }
}

/// P4b: classes — construction (incl. constructor promotion + body side effects) and field reads.
/// Each must run identically on both backends.
const P4B_PROGRAMS: &[&str] = &[
    // promoted fields; field reads in interpolation
    r#"class Point { constructor(public int x, public int y) {} }
       function main() { Point p = Point(3, 4); println("{p.x},{p.y}"); }"#,
    // field read flowing through a typed local, then used as an arithmetic operand
    r#"class Point { constructor(public int x, public int y) {} }
       function main() { Point p = Point(3, 4); int s = p.x; println("{s + p.y}"); }"#,
    // constructor *body* runs for side effects (a `println` in the ctor), using a promoted param
    r#"class Greeter { constructor(public string name) { println("made {name}"); } }
       function main() { Greeter g = Greeter("Ada"); println("hello {g.name}"); }"#,
    // a no-constructor class builds an empty instance; structural instance equality
    r#"class Empty {}
       function main() { Empty a = Empty(); Empty b = Empty(); println("{a == b}"); }"#,
    // instance equality is structural over fields (same class + equal fields)
    r#"class P { constructor(public int x) {} }
       function main() { P a = P(1); P b = P(1); P c = P(2); println("{a == b} {a == c}"); }"#,
    // only *promoted* params become fields (the bare `seed` param is not a field)
    r#"class Acc { constructor(public int total, int seed) {} }
       function main() { Acc a = Acc(10, 99); println("{a.total}"); }"#,
    // a field read as a call argument
    r#"class Box { constructor(public int v) {} }
       function dbl(int n) -> int { return n * 2; }
       function main() { Box b = Box(21); println("{dbl(b.v)}"); }"#,
    // a bare `return;` in the ctor body is an early exit, but the promoted instance is *still*
    // returned (interpreter parity) — exercises the synthetic ctor's epilogue redirect.
    r#"class C { constructor(public int x) { if (x > 0) { return; } println("nonpos"); } }
       function main() { C a = C(5); println("{a.x}"); C b = C(0); println("{b.x}"); }"#,
];

#[test]
fn p4b_programs_match_between_backends() {
    for src in P4B_PROGRAMS {
        agree(src);
    }
}

/// P4b error parity: reading an explicit (uninitialized) `Field` member type-checks — the checker
/// registers it as a field — but construction only populates *promoted* ctor params, so the read
/// faults `no field` identically on both backends at *runtime* (not at the check stage). This is
/// the field-read analogue of the runtime backstop; it is genuinely reachable (unlike P4a's
/// checker-enforced exhaustiveness), so it gets a real `agree_err` case.
#[test]
fn p4b_field_miss_faults_identically() {
    agree_err(
        r#"class Box { public int tag; constructor(public int x) {} }
           function main() { Box b = Box(5); println("{b.tag}"); }"#,
    );
}

/// P4c: instance methods + `this`. Method dispatch is on the receiver's runtime class; a method
/// body reads fields by bare name (resolved against the current class) or via `this`. Each must run
/// identically on both backends. (No `agree_err` case: like P4a's exhaustiveness, method existence
/// is checker-enforced, so the VM's method-not-found fault is a checker-unreachable backstop.)
const P4C_PROGRAMS: &[&str] = &[
    // a method reads a *bare* field (`total` resolves to `this.total`) + a param
    r#"class Counter { constructor(private int total) {} function add(int n) -> int { return total + n; } }
       function main() { Counter c = Counter(100); println("{c.add(23)}"); }"#,
    // a method calls another method via `this`, and reads a field via `this.`
    r#"class C { constructor(public int x) {}
           function dbl() -> int { return this.x + this.x; }
           function quad() -> int { int d = this.dbl(); return d + d; } }
       function main() { C c = C(5); println("{c.quad()}"); }"#,
    // mixed bare-field + explicit-`this` field reads in one expression
    r#"class P { constructor(public int x, public int y) {} function sum() -> int { return x + this.y; } }
       function main() { P p = P(3, 4); println("{p.sum()}"); }"#,
    // recursion *through* a method (`this.fact(n - 1)`)
    r#"class F { constructor(public int base) {}
           function fact(int n) -> int { if (n <= 1) { return 1; } return n * this.fact(n - 1); } }
       function main() { F f = F(0); println("{f.fact(5)}"); }"#,
    // a void (no-return) method invoked as a statement, twice (side effects + Unit result)
    r#"class Logger { constructor(public string tag) {} function log() { println("log {tag}"); } }
       function main() { Logger l = Logger("X"); l.log(); l.log(); }"#,
];

#[test]
fn p4c_programs_match_between_backends() {
    for src in P4C_PROGRAMS {
        agree(src);
    }
}

/// Recursively collect every `*.phg` under `dir`.
fn collect_phg(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read_dir examples/") {
        let path = entry.expect("examples dir entry").path();
        if path.is_dir() {
            collect_phg(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("phg") {
            out.push(path);
        }
    }
}

/// Every runnable example under `examples/` must produce byte-identical stdout on both backends.
/// Globbing (not an explicit list) means a newly-added example is gated with no test edit — the
/// "add examples as we go" contract (`docs/specs/2026-06-16-examples-coverage-design.md`).
#[test]
fn all_examples_match_between_backends() {
    let mut files = Vec::new();
    collect_phg(std::path::Path::new("examples"), &mut files);
    files.sort();
    assert!(
        files.len() >= 3,
        "expected at least the seed examples, found {}",
        files.len()
    );
    for path in &files {
        eprintln!("differential: {}", path.display()); // names the file if agree() panics
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        agree(&src);
    }
}

/// M2 Wave 4: class-aware operand types. Each program type-checks and runs on the interpreter, but
/// the *coarse* pre-Wave-4 compiler rejected it at compile time — its `num_ty` could not see
/// through a field read on an arbitrary instance, a method-call result, a nested `a.b.c`, a
/// class-typed enum payload, or a free function returning an instance. The class-aware `ctype`
/// resolver closes all five. Verified red (interpreter `Ok`, VM `compile error: cannot infer
/// numeric type`) before the fix; both backends agree after it (measured 2026-06-16).
const WAVE4_PROGRAMS: &[&str] = &[
    // (A) field of an arbitrary instance local, used as an arithmetic operand
    r#"class Point { constructor(public int x, public int y) {} }
       function main() { Point p = Point(7, 4); println("{p.x + 1}"); }"#,
    // (B) method-call result used arithmetically
    r#"class C { constructor(public int x) {} function get() -> int { return this.x; } }
       function main() { C c = C(5); println("{c.get() + 1}"); }"#,
    // (C) nested field read `a.inner.x` — a class-typed field's field
    r#"class Inner { constructor(public int x) {} }
       class Outer { constructor(public Inner inner) {} }
       function main() { Outer a = Outer(Inner(10)); println("{a.inner.x + 1}"); }"#,
    // (D) a class-typed enum payload, bound in `match` and read arithmetically
    r#"class Point { constructor(public int x) {} }
       enum Opt { Some(Point p), Zero(int z), }
       function f(Opt o) -> int { return match o { Some(p) => p.x + 1, Zero(z) => z, }; }
       function main() { println("{f(Some(Point(41)))}"); println("{f(Zero(0))}"); }"#,
    // (E) a free function returning an instance, then a field of the result, used arithmetically
    r#"class Point { constructor(public int x) {} }
       function mk() -> Point { return Point(3); }
       function main() { println("{mk().x + 1}"); }"#,
];

#[test]
fn wave4_programs_match_between_backends() {
    for src in WAVE4_PROGRAMS {
        agree(src);
    }
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

#[test]
fn s2_null_and_optional_bind_and_run_on_both_backends() {
    // Task 1 foundation: `null` is a real runtime value and a non-null `T` widens to `T?`.
    // (Observing the null *value* needs the unwrap operators from later S2 tasks.) The exact-output
    // assertion is deliberate: `agree` alone passes vacuously if both backends share a rejection.
    let src = "function main() { int? x = null; int? y = 5; println(\"optionals ok\"); }";
    assert_eq!(cmd_run(src).as_deref(), Ok("optionals ok\n"));
    agree(src); // run ≡ runvm
}

#[test]
fn s2_coalesce_is_byte_identical() {
    // `??`: a null lhs falls through to the default; a present value is kept.
    let src = "function main() { int? x = null; println(\"{x ?? 7}\"); int? y = 9; println(\"{y ?? 0}\"); }";
    assert_eq!(cmd_run(src).as_deref(), Ok("7\n9\n"));
    agree(src);
    // Short-circuit: the default (a printing call) must not run when the lhs is non-null.
    let sc = "function side() -> int { println(\"SIDE\"); return 0; } function main() { int? y = 9; println(\"{y ?? side()}\"); }";
    assert_eq!(cmd_run(sc).as_deref(), Ok("9\n"));
    agree(sc);
}

#[test]
fn s2_safe_access_is_byte_identical() {
    // `?.` short-circuits to null on a null receiver (→ the `?? -1` default) and reads through when
    // the receiver is present. Field read and method call both go through `?.`.
    let cls = "class Box { constructor(private int v) {} function v_of() -> int { return v; } function plus(int n) -> int { return v + n; } }";
    let field = cls.to_string()
        + " function main() { Box? a = null; println(\"{(a?.v) ?? -1}\"); Box? b = Box(7); println(\"{(b?.v) ?? -1}\"); }";
    assert_eq!(cmd_run(&field).as_deref(), Ok("-1\n7\n"));
    agree(&field);
    let method = cls.to_string()
        + " function main() { Box? a = null; println(\"{(a?.v_of()) ?? -1}\"); Box? b = Box(9); println(\"{(b?.v_of()) ?? -1}\"); }";
    assert_eq!(cmd_run(&method).as_deref(), Ok("-1\n9\n"));
    agree(&method);
    // short-circuit: a safe call on a null receiver must NOT evaluate its arguments (no "SIDE").
    let sc = cls.to_string()
        + " function side() -> int { println(\"SIDE\"); return 0; } function main() { Box? a = null; println(\"{(a?.plus(side())) ?? -1}\"); }";
    assert_eq!(cmd_run(&sc).as_deref(), Ok("-1\n"));
    agree(&sc);
}
