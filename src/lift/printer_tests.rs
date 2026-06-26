//! M-Lift L3 — Phorge pretty-printer tests. Two kinds: exact-output checks for representative
//! constructs, and a **round-trip** check (parse `.phg` → print → re-parse must succeed and printing
//! must be idempotent), which proves the output is valid Phorge that re-parses to a stable tree.

use super::printer::print_program;
use crate::parser::Parser;

/// Parse Phorge source into a program (panics on lex/parse error — tests use valid input).
fn phorge(src: &str) -> crate::ast::Program {
    let toks = crate::lexer::lex(src).expect("lex");
    Parser::new(toks).parse_program().expect("parse")
}

/// Parse → print.
fn pp(src: &str) -> String {
    print_program(&phorge(src)).expect("print")
}

#[test]
fn prints_package_and_function() {
    let out = pp("package Main;\nfunction add(int a, int b) -> int { return a + b; }\n");
    assert_eq!(
        out,
        "package Main;\n\nfunction add(int a, int b) -> int {\n    return a + b;\n}\n"
    );
}

#[test]
fn prints_import() {
    let out = pp("package Main;\nimport Core.Console;\nfunction main() -> void {}\n");
    assert!(out.contains("import Core.Console;"), "{out}");
}

#[test]
fn prints_class_with_constructor_and_method() {
    let out = pp(
        "package Main;\nclass Engine {\n  constructor(private int power) {}\n  function powerOf() -> int { return power; }\n}\n",
    );
    assert!(out.contains("class Engine {"), "{out}");
    assert!(out.contains("constructor(private int power) {}"), "{out}");
    assert!(out.contains("function powerOf() -> int {"), "{out}");
}

#[test]
fn prints_mutable_field_and_open_abstract() {
    let out = pp(
        "package Main;\nabstract class Shape {\n  mutable int n;\n  abstract function area() -> int;\n}\n",
    );
    assert!(out.contains("abstract class Shape {"), "{out}");
    assert!(out.contains("mutable int n;"), "{out}");
    assert!(out.contains("abstract function area() -> int;"), "{out}");
}

#[test]
fn prints_enum() {
    let out = pp("package Main;\nenum Dir { Up, Down, Left, Right }\n");
    assert!(out.contains("enum Dir { Up, Down, Left, Right }"), "{out}");
}

#[test]
fn prints_control_flow_and_var() {
    let out = pp(
        "package Main;\nfunction f() -> void {\n  mutable var i = 0;\n  while (i < 10) { i = i + 1; }\n  for (int x in 0..5) { Console.println(\"{x}\"); }\n}\n",
    );
    assert!(out.contains("mutable var i = 0;"), "{out}");
    assert!(out.contains("while (i < 10) {"), "{out}");
    assert!(out.contains("for (int x in 0..5) {"), "{out}");
}

#[test]
fn prints_if_elseif_else_chain() {
    let out = pp(
        "package Main;\nfunction f(int n) -> int {\n  if (n < 0) { return 0; } else if (n < 10) { return 1; } else { return 2; }\n}\n",
    );
    assert!(out.contains("if (n < 0) {"), "{out}");
    assert!(out.contains("} else if (n < 10) {"), "{out}");
    assert!(out.contains("} else {"), "{out}");
}

#[test]
fn prints_match_and_new() {
    let out = pp(
        "package Main;\nenum Color { Red, Green }\nfunction f(int n) -> int {\n  return match (n) { 0 => 1, _ => 2 };\n}\n",
    );
    assert!(out.contains("match (n) { 0 => 1, _ => 2 }"), "{out}");
}

#[test]
fn escapes_strings_including_braces() {
    // A literal `{`/`}`/quote/newline must be escaped so it re-parses as a literal, not interpolation.
    let out = pp("package Main;\nfunction f() -> void { Console.println(\"a\\{b\\}c\"); }\n");
    assert!(out.contains("\\{b\\}"), "braces must be escaped: {out}");
}

// ── C-5/6: minimal parentheses (precedence-aware) + prefix unary without parens ──

#[test]
fn minimal_parens_respect_precedence() {
    // `+`/`-` (11) inside `*` (12): the `*` operands keep parens, the outer expression does not.
    let out = pp("package Main;\nfunction f(int a, int b) -> int { return (a + b) * (a - b); }\n");
    assert!(out.contains("return (a + b) * (a - b);"), "{out}");
    // Higher-precedence child needs no parens: `a + b * c` ≡ `a + (b * c)`.
    let o2 = pp("package Main;\nfunction f(int a, int b, int c) -> int { return a + b * c; }\n");
    assert!(o2.contains("return a + b * c;"), "{o2}");
}

#[test]
fn minimal_parens_respect_left_associativity() {
    // Left-assoc: same-precedence left child needs no parens, a right-nested one does.
    let l = pp("package Main;\nfunction f(int a, int b, int c) -> int { return a - b + c; }\n");
    assert!(l.contains("return a - b + c;"), "{l}");
    let r = pp("package Main;\nfunction f(int a, int b, int c) -> int { return a - (b + c); }\n");
    assert!(r.contains("return a - (b + c);"), "{r}");
}

#[test]
fn prefix_unary_without_parens() {
    let n = pp("package Main;\nfunction f(int a) -> int { return ~a; }\n");
    assert!(n.contains("return ~a;"), "{n}");
    let g = pp("package Main;\nfunction f(bool a) -> bool { return !a; }\n");
    assert!(g.contains("return !a;"), "{g}");
    // A binary operand still needs parens (unary binds tighter than `+`).
    let b = pp("package Main;\nfunction f(int a, int b) -> int { return ~(a + b); }\n");
    assert!(b.contains("return ~(a + b);"), "{b}");
}

// ── round-trip: print output must be valid Phorge and printing must be idempotent ──

fn assert_roundtrip(src: &str) {
    let once = pp(src);
    // The printed output must itself parse (valid Phorge) and print identically (a fixed point).
    let twice = print_program(&phorge(&once)).expect("re-print");
    assert_eq!(
        once, twice,
        "printer not idempotent\n--- once ---\n{once}\n--- twice ---\n{twice}"
    );
}

#[test]
fn roundtrip_representative_program() {
    assert_roundtrip(
        "package Main;\n\
         import Core.Console;\n\
         enum Dir { Up, Down }\n\
         class Engine {\n\
           constructor(private mutable int power) {}\n\
           function bump() -> int { return power + 1; }\n\
         }\n\
         function classify(int n) -> int {\n\
           if (n < 0) { return 0; } else if (n == 0) { return 1; } else { return 2; }\n\
         }\n\
         function main() -> void {\n\
           mutable var total = 0;\n\
           for (int i in 0..10) { total = total + i; }\n\
           Console.println(\"{total}\");\n\
         }\n",
    );
}

#[test]
fn roundtrip_expressions() {
    assert_roundtrip(
        "package Main;\nfunction f(int a, int b) -> int { return ((a + b) * (a - b)); }\n",
    );
}
