use super::*;
use crate::lexer::lex;
use crate::parser::Parser;
use crate::vm::Vm;

/// Compile + run a program on the VM, returning captured output. Auto-prepends the reserved
/// `package Main;` (M5 S1, line-preserving) so existing test programs need no per-case edit.
fn run(src: &str) -> Result<String, String> {
    let src = with_pkg(src);
    let tokens = lex(&src).expect("lex ok");
    let prog = Parser::new(tokens).parse_program().expect("parse ok");
    let program = compile(&prog).map_err(|d| d.to_string())?;
    Vm::new(&program).run().map_err(|d| d.to_string())
}

fn with_pkg(src: &str) -> String {
    if src.trim_start().starts_with("package ") {
        src.to_string()
    } else {
        format!("package Main; {src}")
    }
}

fn out(src: &str) -> String {
    run(src).expect("run ok")
}

#[test]
fn prints_a_literal_string() {
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { Console.println("hi"); }"#),
        "hi\n"
    );
}

#[test]
fn integer_arithmetic_in_interpolation() {
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { Console.println("{1 + 2 * 3}"); }"#),
        "7\n"
    );
}

#[test]
fn float_arithmetic_formats_like_interpreter() {
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { Console.println("{3.0 * 4.0}"); }"#),
        "12\n"
    );
}

#[test]
fn comparison_and_short_circuit() {
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { Console.println("{1 < 2 && 3 >= 3}"); }"#),
        "true\n"
    );
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { Console.println("{1 > 2 || false}"); }"#),
        "false\n"
    );
}

#[test]
fn unary_negation_and_not() {
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { Console.println("{-5}"); Console.println("{!true}"); }"#),
        "-5\nfalse\n"
    );
}

#[test]
fn division_by_zero_is_runtime_error() {
    let e = run(r#"import Core.Console;
function main() -> void { Console.println("{1 / 0}"); }"#)
    .unwrap_err();
    assert!(e.contains("division by zero"), "{e}");
}

#[test]
fn missing_main_is_compile_error() {
    let e = run(r#"function other() -> void {}"#).unwrap_err();
    assert!(e.contains("main"), "{e}");
}

#[test]
fn user_function_call_runs() {
    let src = r#"import Core.Console;
function inc(int n) -> int { return n + 1; } function main() -> void { Console.println("{inc(4)}"); }"#;
    assert_eq!(out(src), "5\n");
}

#[test]
fn recursion_runs() {
    let src = r#"import Core.Console;
function fib(int n) -> int {
            if (n < 2) { return n; }
            return fib(n - 1) + fib(n - 2);
        } function main() -> void { Console.println("{fib(10)}"); }"#;
    assert_eq!(out(src), "55\n");
}

#[test]
fn undefined_call_target_rejected() {
    // A name that is neither a function, `println`, a variant, nor a declared class is rejected
    // with the interpreter's wording (checker-unreachable; defensive compiler path).
    let src = r#"import Core.Console;
function main() -> void { Console.println("{Circle(2.0)}"); }"#;
    let e = run(src).unwrap_err();
    assert!(e.contains("not a function, variant, or class"), "{e}");
}

#[test]
fn class_construction_and_field_read() {
    let src = r#"import Core.Console;
class Point { constructor(public int x, public int y) {} }
            function main() -> void { Point p = Point(3, 4); Console.println("{p.x},{p.y}"); }"#;
    assert_eq!(out(src), "3,4\n");
}

#[test]
fn constructor_body_runs_for_side_effects() {
    // The promoted instance is the result; the body's `println` is a side effect.
    let src = r#"import Core.Console;
class Greeter { constructor(public string name) { Console.println("made {name}"); } }
            function main() -> void { Greeter g = Greeter("Ada"); Console.println("hi {g.name}"); }"#;
    assert_eq!(out(src), "made Ada\nhi Ada\n");
}

#[test]
fn constructor_early_return_still_yields_instance() {
    // A bare `return;` exits the body early but the promoted instance is still returned.
    let src = r#"import Core.Console;
class C { constructor(public int x) { if (x > 0) { return; } Console.println("np"); } }
            function main() -> void { C a = C(5); Console.println("{a.x}"); C b = C(0); Console.println("{b.x}"); }"#;
    assert_eq!(out(src), "5\nnp\n0\n");
}

#[test]
fn method_reads_bare_field_and_dispatches() {
    // `total` in the method body resolves to `this.total`; `c.add(23)` dispatches on the class.
    let src = r#"import Core.Console;
class Counter { constructor(private int total) {} function add(int n) -> int { return total + n; } }
            function main() -> void { Counter c = Counter(100); Console.println("{c.add(23)}"); }"#;
    assert_eq!(out(src), "123\n");
}

#[test]
fn method_calls_method_via_this() {
    let src = r#"import Core.Console;
class C { constructor(public int x) {}
                function dbl() -> int { return this.x + this.x; }
                function quad() -> int { int d = this.dbl(); return d + d; } }
            function main() -> void { C c = C(5); Console.println("{c.quad()}"); }"#;
    assert_eq!(out(src), "20\n");
}

#[test]
fn method_recursion_through_this() {
    let src = r#"import Core.Console;
class F { constructor(public int base) {}
                function fact(int n) -> int { if (n <= 1) { return 1; } return n * this.fact(n - 1); } }
            function main() -> void { F f = F(0); Console.println("{f.fact(5)}"); }"#;
    assert_eq!(out(src), "120\n");
}

#[test]
fn var_decl_and_use() {
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { int x = 10; Console.println("{x + 5}"); }"#),
        "15\n"
    );
}

#[test]
fn multiple_locals_resolve_to_distinct_slots() {
    let src = r#"import Core.Console;
function main() -> void { int a = 1; int b = 2; Console.println("{a + b}"); }"#;
    assert_eq!(out(src), "3\n");
}

#[test]
fn float_local_uses_float_arithmetic() {
    let src = r#"import Core.Console;
function main() -> void { float r = 2.0; Console.println("{r * r}"); }"#;
    assert_eq!(out(src), "4\n");
}

#[test]
fn if_else_picks_branch() {
    let src = r#"import Core.Console;
function main() -> void { if (1 < 2) { Console.println("yes"); } else { Console.println("no"); } }"#;
    assert_eq!(out(src), "yes\n");
}

#[test]
fn if_without_else() {
    let src = r#"import Core.Console;
function main() -> void { if (1 > 2) { Console.println("never"); } Console.println("after"); }"#;
    assert_eq!(out(src), "after\n");
}

#[test]
fn for_loop_over_list() {
    let src = r#"import Core.Console;
function main() -> void { List<int> xs = [1, 2, 3]; for (int x in xs) { Console.println("{x}"); } }"#;
    assert_eq!(out(src), "1\n2\n3\n");
}

#[test]
fn indexing_reads_element() {
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { List<int> xs = [7, 8, 9]; Console.println("{xs[1]}"); }"#),
        "8\n"
    );
}

#[test]
fn indexing_out_of_range_faults() {
    let e = run(r#"import Core.Console;
function main() -> void { List<int> xs = [1]; Console.println("{xs[3]}"); }"#)
    .unwrap_err();
    assert!(e.contains("list index out of range"), "{e}");
}

#[test]
fn for_loop_body_locals_do_not_leak() {
    // A body-local must be cleaned each iteration (stack stays balanced).
    let src = r#"import Core.Console;
function main() -> void {
            List<int> xs = [1, 2];
            for (int x in xs) { int y = x + 10; Console.println("{y}"); }
            Console.println("done");
        }"#;
    assert_eq!(out(src), "11\n12\ndone\n");
}

#[test]
fn ranges_iterate_on_vm() {
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { for (int i in 0..3) { Console.println("{i}"); } }"#),
        "0\n1\n2\n"
    );
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { for (int i in 2..=4) { Console.println("{i}"); } }"#),
        "2\n3\n4\n"
    );
}

#[test]
fn expression_if_on_vm() {
    // value-position if, then arithmetic on the result (height-merge + ctype specialization)
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { var x = if (true) { 10 } else { 20 }; Console.println("{x + x}"); }"#),
        "20\n"
    );
    assert_eq!(
        out(r#"import Core.Console;
function main() -> void { var x = if (false) { 10 } else { 20 }; Console.println("{x + 1}"); }"#),
        "21\n"
    );
}

#[test]
fn enum_construct_and_match_binds_payload() {
    let src = r#"import Core.Console;
enum Grade { Pass(int s), Fail(int s), }
            function d(Grade g) -> string { return match g { Pass(s) => "P{s}", Fail(s) => "F{s}", }; }
            function main() -> void { Console.println(d(Pass(9))); Console.println(d(Fail(3))); }"#;
    assert_eq!(out(src), "P9\nF3\n");
}

#[test]
fn match_literal_arms_and_catch_all_binding() {
    let src = r#"import Core.Console;
function f(int n) -> string { return match n { 0 => "z", 1 => "o", x => "m{x}", }; }
            function main() -> void { Console.println(f(0)); Console.println(f(1)); Console.println(f(9)); }"#;
    assert_eq!(out(src), "z\no\nm9\n");
}

#[test]
fn match_as_binary_operand_tracks_scrutinee_slot() {
    // The lhs `1` is live on the operand stack when the `match` rhs compiles, so the scrutinee
    // must spill to a transient-aware slot (not `locals.len()`).
    let src = r#"import Core.Console;
function g(int n) -> int { return 1 + match n { 0 => 10, _ => 20 }; }
            function main() -> void { Console.println("{g(0)}"); Console.println("{g(5)}"); }"#;
    assert_eq!(out(src), "11\n21\n");
}

#[test]
fn nested_match_reextracts_outer_binding() {
    // Inner `match` compiles while the outer scrutinee occupies slot `locals.len()`; its own
    // scrutinee must land one slot higher (height tracking), and the inner arm re-extracts the
    // outer binding `b` from the outer scrutinee.
    let src = r#"import Core.Console;
enum Pair { P(int a, int b), }
            function f(Pair p) -> string {
                return match p { P(a, b) => match a { 0 => "z b={b}", _ => "a={a} b={b}", }, };
            }
            function main() -> void { Console.println(f(P(0, 9))); Console.println(f(P(5, 2))); }"#;
    assert_eq!(out(src), "z b=9\na=5 b=2\n");
}
