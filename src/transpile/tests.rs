use super::emit;
use crate::lexer::lex;
use crate::parser::Parser;

fn php(src: &str) -> String {
    let tokens = lex(src).expect("lex");
    let prog = Parser::new(tokens).parse_program().expect("parse");
    emit(&prog).expect("emit")
}

fn parse_only(src: &str) -> crate::ast::Program {
    // Auto-prepend the reserved `package Main;` (M5 S1, line-preserving) unless declared, so
    // transpiler tests need no per-case edit. The transpiler ignores the package in S1 (flat
    // emission); brace-namespaces for non-`main` packages land in S2c.
    let src = if src.trim_start().starts_with("package ") {
        src.to_string()
    } else {
        format!("package Main; {src}")
    };
    let tokens = lex(&src).expect("lex");
    Parser::new(tokens).parse_program().expect("parse")
}

#[test]
fn empty_program_emits_php_open_tag() {
    assert_eq!(php(""), "<?php\n");
}

#[test]
fn free_function_with_params_and_arithmetic() {
    let out = php("function add(int a, int b) -> int { int c = a + b; return c; }");
    assert!(out.contains("function add(int $a, int $b): int {"), "{out}");
    // `+` is string-concat-overloaded, so it routes through the `__phorge_add` runtime helper
    // (`is_string ? . : +`) — the transpiler has no static operand types (Phase 1 string slice).
    assert!(out.contains("$c = __phorge_add($a, $b);"), "{out}");
    assert!(out.contains("return $c;"), "{out}");
}

#[test]
fn force_unwrap_uses_native_throw_expression_not_helper() {
    // `opt!` lowers to PHP 8.0's null-coalescing throw expression `($v ?? throw new …)` — `??`
    // throws iff the value is null and evaluates the receiver once, exactly the old `__phorge_unwrap`
    // helper. No runtime helper function is emitted.
    let out =
        php("function f(int? o) -> int { return o!; } function main() -> void { int x = f(5); }");
    assert!(
        out.contains("($o ?? throw new \\RuntimeException(\"force-unwrap of null\"))"),
        "{out}"
    );
    assert!(!out.contains("__phorge_unwrap"), "{out}");
}

#[test]
fn clone_with_lowers_to_native_php85_two_arg_clone() {
    // T4: the transpile floor is PHP 8.5, where `clone($o, [...])` is native (clone + property
    // overrides, constructor bypassed, `__clone` honored) — exactly what `obj with { f = e }` means.
    // It replaces the old `__phorge_clone_with` runtime helper (which existed only for the prior 8.4
    // floor). An empty override list is still a one-arg `clone($o)`.
    let out = php("class P { constructor(public int x, public int y) {} } \
             function main() -> void { P a = P(1, 2); P b = a with { x = 9 }; }");
    assert!(
        out.contains("clone($a, ['x' => 9])"),
        "clone-with uses native two-arg clone:\n{out}"
    );
    assert!(
        !out.contains("__phorge_clone_with"),
        "the 8.4 helper is gone (call site and definition):\n{out}"
    );
}

#[test]
fn error_cause_routed_to_php_previous_chain() {
    // M-faults 2c: a conventional `cause` field of optional-`Error` type on an `Error` subtype is
    // routed into PHP's native exception chain via `parent::__construct($message, 0, $cause)`, so
    // the transpiled PHP reports an idiomatic "caused by" through `getPrevious()` too. The cause
    // property is typed `?\Throwable` (PHP's `$previous` type) — NOT the unrelated engine `Error`
    // class (which `Error` would otherwise resolve to) nor a lossy `mixed`.
    let out = php(
        "class IoError implements Error { constructor(public string message) {} } \
             class ConfigError implements Error { \
               constructor(public string message, public Error? cause) {} }",
    );
    assert!(
        out.contains("parent::__construct($message, 0, $cause);"),
        "cause routed to native previous chain:\n{out}"
    );
    assert!(
        out.contains("?\\Throwable $cause"),
        "cause typed as ?\\Throwable (not engine Error / mixed):\n{out}"
    );
}

#[test]
fn no_return_type_is_void() {
    let out = php("function f() -> void { return; }");
    assert!(out.contains("function f(): void {"), "{out}");
}

#[test]
fn explicit_void_return_emits_php_void() {
    let out = php("function f() -> void { return; }");
    assert!(out.contains("function f(): void {"), "{out}");
}

#[test]
fn empty_return_emits_no_php_hint() {
    // `Empty` must NOT emit `: void`/`: mixed`/`: null` — PHP would reject a fall-off or a bare
    // `return;`. No hint → PHP infers a capturable `null`.
    let out = php("function f() -> Empty { } function main() -> void { Empty x = f(); }");
    assert!(
        out.contains("function f() {"),
        "expected no return hint:\n{out}"
    );
    assert!(
        !out.contains("function f():"),
        "must not have a colon hint:\n{out}"
    );
}

#[test]
fn if_and_for_and_unary() {
    // Phorge is immutable (no reassignment) — use fresh var decls inside branches.
    let out = php("function f(int n) -> int { \
               List<int> xs = [1, 2]; \
               for (int x in xs) { if (x > 0) { int a = -x; } else { bool b = !true; } } \
               return n; }");
    assert!(out.contains("foreach ($xs as $x) {"), "{out}");
    assert!(out.contains("if ($x > 0) {"), "{out}");
    assert!(out.contains("} else {"), "{out}");
    assert!(
        out.contains("$a = -$x;") && out.contains("$b = !true;"),
        "{out}"
    );
    assert!(out.contains("[1, 2]"), "{out}");
}

#[test]
fn indexing_emits_php_subscript() {
    let out = php("function at(List<int> xs, int i) -> int { return xs[i]; }");
    assert!(out.contains("$xs[$i]"), "{out}");
}

#[test]
fn ranges_emit_php_range() {
    // Ranges route through `__phorge_range` (QW-13): the helper yields `[]` for an empty/reversed
    // range, where PHP's bare `range()` would descend. The `inclusive` flag is the third arg.
    let out = php(r#"import Core.Console;
function main() -> void { for (int i in 0..3) { Console.println("{i}"); } }"#);
    assert!(out.contains("__phorge_range(0, 3, false)"), "{out}");
    assert!(out.contains("function __phorge_range"), "{out}");
    let inc = php(r#"import Core.Console;
function main() -> void { for (int i in 1..=3) { Console.println("{i}"); } }"#);
    assert!(inc.contains("__phorge_range(1, 3, true)"), "{inc}");
}

#[test]
fn expression_if_emits_ternary() {
    let out = php("function pick(bool b) -> int { return if (b) { 1 } else { 2 }; }");
    assert!(out.contains("($b ? 1 : 2)"), "{out}");
}

#[test]
fn interpolation_emits_concatenation() {
    // Each interpolated value is coerced via `__phorge_str` (P0-3: bool ⇒ "true"/"false").
    let out = php("function greet(string name) -> string { return \"Hello {name}\"; }");
    assert!(
        out.contains(r#"return "Hello " . __phorge_str($name);"#),
        "{out}"
    );
}

#[test]
fn float_interpolation_emits_phorge_float_helper() {
    // A float reaches PHP only through interpolation (`Console.println` takes `string`), so the
    // `__phorge_str` chokepoint routes floats through `__phorge_float`, which reproduces Rust's
    // shortest-round-trip positional `f64` Display (no PHP precision-14 / scientific divergence).
    let out = php("function f(float x) -> string { return \"v={x}\"; }");
    assert!(
        out.contains(r#"return "v=" . __phorge_str($x);"#),
        "call site routes through __phorge_str: {out}"
    );
    assert!(
        out.contains("if (is_float($v)) { return __phorge_float($v); }"),
        "__phorge_str delegates floats to __phorge_float: {out}"
    );
    assert!(
        out.contains("function __phorge_float($v) {")
            && out.contains(r#"$cand = sprintf("%.{$p}e", $a);"#),
        "__phorge_float helper is defined with the shortest-round-trip loop: {out}"
    );
    // Only tier-1 PHP functions — must stay correct under `php -n` (extension policy).
    for forbidden in ["mb_", "ctype_", "iconv", "bcadd"] {
        assert!(
            !out.contains(forbidden),
            "__phorge_float must use tier-1 functions only, found `{forbidden}`: {out}"
        );
    }
}

#[test]
fn pure_string_literal_no_concat() {
    let out = php("function f() -> string { return \"hi\"; }");
    assert!(out.contains(r#"return "hi";"#), "{out}");
}

#[test]
fn literal_match_with_binding_emits_native_match() {
    // T1: a value `match` of literals + a bare-binding catch-all lowers to a native PHP `match`
    // expression (PHP `match` is strict `===`, agreeing with Phorge literal patterns). The binding
    // is assigned *inside* the subject (`match ($x = $n)`) so the `default` arm can reference it —
    // single evaluation, no `if/elseif` chain, no IIFE.
    let out = php(
            "function sign(int n) -> string { string s = match n { 0 => \"z\", 1 => \"one\", x => \"other\" }; return s; }",
        );
    assert!(out.contains("$s = match ($x = $n) {"), "{out}");
    assert!(out.contains("0 => \"z\","), "{out}");
    assert!(out.contains("1 => \"one\","), "{out}");
    assert!(out.contains("default => \"other\","), "{out}");
    // No legacy if-chain or stranded defensive throw.
    assert!(!out.contains("elseif ($n === 1)"), "{out}");
}

#[test]
fn literal_match_with_wildcard_emits_native_match() {
    // A wildcard `_` catch-all needs no binding, so the subject is the bare scrutinee.
    let out = php(
            "function classify(int code) -> string { return match code { 0 => \"zero\", 1 => \"one\", _ => \"other\" }; }",
        );
    assert!(out.contains("return match ($code) {"), "{out}");
    assert!(out.contains("0 => \"zero\","), "{out}");
    assert!(out.contains("default => \"other\","), "{out}");
    assert!(!out.contains("if ($code === 0)"), "{out}");
}

#[test]
fn expression_position_literal_match_emits_native_match() {
    // T1: a literal value `match` in expression position is a native PHP `match` expression
    // (parenthesized so it composes), NOT an IIFE. The binding catch-all still works in expression
    // position via the assignment-as-subject trick (`match ($x = $n)`).
    let out = php(
            "function f(int n) -> int { int base = 5; int r = (match n { 0 => 10, x => x }) + base; return r; }",
        );
    assert!(out.contains("(match ($x = $n) {"), "{out}");
    assert!(out.contains("0 => 10,"), "{out}");
    assert!(out.contains("default => $x,"), "{out}");
    // No IIFE wrapper for a pure literal match.
    assert!(!out.contains("function() use"), "{out}");
}

#[test]
fn println_becomes_echo() {
    let out = php("import Core.Console; function main() -> void { Console.println(\"hi\"); }");
    assert!(out.contains(r#"echo "hi" . "\n";"#), "{out}");
}

#[test]
fn main_is_invoked_when_present() {
    let out = php("import Core.Console; function main() -> void { Console.println(\"hi\"); }");
    assert!(out.trim_end().ends_with("main();"), "{out}");
    // no main -> no call
    let no_main = php("function helper() -> int { return 1; }");
    assert!(!no_main.contains("main();"), "{no_main}");
}

const SHAPE: &str = "enum Shape { Circle(float radius), Rect(float w, float h), }";

#[test]
fn enum_emits_base_and_subclasses() {
    let out = php(SHAPE);
    assert!(out.contains("abstract class Shape {}"), "{out}");
    assert!(out.contains("final class Circle extends Shape {"), "{out}");
    assert!(
        out.contains("public function __construct(public float $radius) {}"),
        "{out}"
    );
    assert!(out.contains("final class Rect extends Shape {"), "{out}");
    assert!(
        out.contains("public function __construct(public float $w, public float $h) {}"),
        "{out}"
    );
}

#[test]
fn variant_construction_uses_new() {
    let out = php(&format!(
        "{SHAPE} function f() -> Shape {{ return Circle(2.0); }}"
    ));
    assert!(out.contains("return new Circle(2.0);"), "{out}");
}

#[test]
fn free_function_call_no_new() {
    let out = php("function inc(int n) -> int { return n + 1; } \
             function f() -> int { return inc(1); }");
    assert!(out.contains("return inc(1);"), "{out}");
}

#[test]
fn class_with_promotion_and_method() {
    let out = php("class Greeter { constructor(private string name) {} \
               function greet() -> string { return \"Hello {name}\"; } }");
    assert!(out.contains("class Greeter {"), "{out}");
    assert!(
        out.contains("function __construct(private string $name) {}"),
        "{out}"
    );
    assert!(out.contains("function greet(): string {"), "{out}");
    // bare field ref inside a method resolves to $this->name (coerced via __phorge_str — P0-3)
    assert!(
        out.contains(r#"return "Hello " . __phorge_str($this->name);"#),
        "{out}"
    );
}

#[test]
fn explicit_non_promoted_field_emitted() {
    // A plain field (not a ctor param) is emitted as a standalone property.
    let out = php("class C { private int count; constructor() {} }");
    assert!(out.contains("private int $count;"), "{out}");
}

#[test]
fn promoted_field_not_redeclared() {
    // Declared both explicitly AND via promotion: emit only the promotion (PHP forbids
    // redeclaring a promoted property as a separate one — caught by the round-trip test).
    let out = php("class C { private int total; constructor(private int total) {} }");
    assert!(
        out.contains("function __construct(private int $total) {}"),
        "{out}"
    );
    assert!(
        !out.contains("private int $total;"),
        "standalone redeclaration must be gone: {out}"
    );
}

#[test]
fn member_access_and_method_call() {
    let out = php(
        "import core.console; class Greeter { constructor(private string name) {} \
               function greet() -> string { return name; } } \
             function main() -> void { Greeter g = Greeter(\"Tak\"); Console.println(g.greet()); }",
    );
    assert!(out.contains(r#"$g = new Greeter("Tak");"#), "{out}");
    assert!(out.contains("$g->greet()"), "{out}");
}

#[test]
fn match_in_return_emits_instanceof_chain() {
    let out = php(&format!(
        "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => 3.14159 * r * r, Rect(w, h) => w * h, }}; }}"
    ));
    assert!(out.contains("if ($s instanceof Circle) {"), "{out}");
    assert!(out.contains("$r = $s->radius;"), "{out}"); // positional: r <- field 0 (radius)
                                                        // P0-2: a compound operand keeps grouping parens (`3.14159 * r * r` is left-assoc Mul, so the
                                                        // left operand of the outer `*` is the inner product, conservatively parenthesized).
    assert!(out.contains("return (3.14159 * $r) * $r;"), "{out}");
    assert!(out.contains("if ($s instanceof Rect) {"), "{out}");
    assert!(
        out.contains("$w = $s->w;") && out.contains("$h = $s->h;"),
        "{out}"
    );
    assert!(out.contains("throw new \\UnhandledMatchError();"), "{out}");
}

#[test]
fn match_in_var_decl_assigns_in_each_arm() {
    let out = php(&format!(
        "{SHAPE} function f(Shape s) -> float {{ \
               float a = match s {{ Circle(r) => r, Rect(w, h) => w, }}; return a; }}"
    ));
    assert!(
        out.contains("if ($s instanceof Circle) { $r = $s->radius; $a = $r; }"),
        "{out}"
    );
    assert!(out.contains("if ($s instanceof Rect) {"), "{out}");
}

#[test]
fn wildcard_arm_has_no_trailing_throw() {
    let out = php(&format!(
        "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, _ => 0.0, }}; }}"
    ));
    assert!(!out.contains("UnhandledMatchError"), "{out}");
}

#[test]
fn match_as_call_argument_emits_match_true() {
    // T2: a variant `match` in expression position (here a call argument) lowers to a native PHP
    // `match (true) { <cond> => <body>, … }` expression, NOT an IIFE. PHP `if` is a statement and
    // `match` is an expression, so the if-chain stays for statement-position matches while
    // expression position uses `match` — mapping Phorge's match onto PHP's own statement/expression
    // duality. Payload bindings ride into the condition as `(($x = access) || true)` conjuncts (the
    // same proven technique the guarded if-chain uses), so no preceding statement is needed.
    let prog = parse_only(&format!(
        "{SHAPE} function f(Shape s) -> float {{ \
               float a = id(match s {{ Circle(r) => r, Rect(w, h) => w, }}); return a; }}"
    ));
    let out = emit(&prog).expect("expression-position match transpiles");
    assert!(out.contains("id((match (true) {"), "{out}");
    assert!(
        out.contains("$s instanceof Circle && (($r = $s->radius) || true) => $r,"),
        "{out}"
    );
    assert!(
        out.contains(
            "$s instanceof Rect && (($w = $s->w) || true) && (($h = $s->h) || true) => $w,"
        ),
        "{out}"
    );
    // No IIFE.
    assert!(!out.contains("function () use"), "{out}");
    assert!(!out.contains("function() use"), "{out}");
}

// ── M3 S3 Task 5: expression lambdas + named-fn references ──────────────

#[test]
fn transpiles_expression_lambda_to_arrow_fn() {
    let php_out = php("package Main; import Core.Console; function main()-> void { var d = fn(int x) => x*2; Console.println(\"{d(5)}\"); }");
    assert!(php_out.contains("fn($x) => $x * 2"), "{php_out}");
}

#[test]
fn transpiles_named_fn_reference() {
    let php_out = php("package Main; function inc(int x)->int{return x+1;} function apply(int x,(int)->int f)->int{return f(x);} function main()-> void { apply(1, inc); }");
    assert!(
        php_out.contains("inc(...)"),
        "first-class callable: {php_out}"
    );
}
