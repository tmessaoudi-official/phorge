//! M-Lift L4 — lifter tests. Asserts the PHP→Phorge mapping (idiomatic, never mirroring warts), the
//! loud Tier-2 lift errors, and end-to-end that the lifted `.phg` is *valid Phorge* (it re-parses).

use super::lifter::lift_source;
use crate::parser::Parser;

/// Lift PHP → Phorge source (panics on lift error — for the happy-path tests).
fn lift(php: &str) -> String {
    lift_source(php).expect("lift")
}

/// Lift error message for PHP that should be refused.
fn lift_err(php: &str) -> String {
    lift_source(php).expect_err("expected a lift error")
}

/// The lifted output must be valid Phorge (re-lexes + re-parses without error).
fn assert_reparses(phg: &str) {
    let toks = crate::lexer::lex(phg)
        .unwrap_or_else(|e| panic!("lifted output failed to lex: {e:?}\n{phg}"));
    Parser::new(toks)
        .parse_program()
        .unwrap_or_else(|e| panic!("lifted output failed to parse: {e:?}\n{phg}"));
}

#[test]
fn lifts_typed_function() {
    let out = lift("<?php function add(int $a, int $b): int { return $a + $b; }");
    assert!(out.starts_with("package Main;"), "{out}");
    assert!(out.contains("function add(int a, int b): int {"), "{out}");
    assert!(out.contains("return a + b;"), "{out}");
    assert_reparses(&out);
}

#[test]
fn lifts_untyped_function_with_no_value_return_as_void() {
    // C-45: a PHP function with no return hint and no value-returning `return` is provably `void`.
    let out = lift(r#"<?php function greet(string $n) { echo "hi"; }"#);
    assert!(out.contains("function greet(string n): void {"), "{out}");
    assert_reparses(&out);
}

#[test]
fn refuses_untyped_function_that_returns_a_value() {
    // C-45: no return hint but a value `return` — the type can't be inferred → loud reject (never
    // emit a Phorge function with no return type, which would fail the checker silently).
    let err = lift_err(r#"<?php function f() { return 5; }"#);
    assert!(err.contains("no return type but returns a value"), "{err}");
}

#[test]
fn lifts_instanceof() {
    // C-46: PHP `instanceof` → Phorge's existing `instanceof` (static class name).
    let out = lift(r#"<?php function f(Animal $x): bool { return $x instanceof Dog; }"#);
    assert!(out.contains("x instanceof Dog"), "{out}");
    assert_reparses(&out);
}

#[test]
fn refuses_dynamic_instanceof() {
    // C-46: a dynamic RHS has no Phorge equivalent — loud reject.
    let err =
        lift_err(r#"<?php function f(Animal $x, Animal $y): bool { return $x instanceof $y; }"#);
    assert!(err.contains("dynamic `instanceof"), "{err}");
}

#[test]
fn lifts_bitwise_operators() {
    // C-47: bitwise `& | ^ ~` and shifts `<< >>` map 1:1, at PHP precedence.
    let out = lift(
        r#"<?php function f(int $a, int $b): int { return ($a & $b) | ($a ^ $b) << 1 >> 1; }"#,
    );
    for op in ["&", "|", "^", "<<", ">>"] {
        assert!(out.contains(op), "missing `{op}` in {out}");
    }
    assert_reparses(&out);
    let not = lift(r#"<?php function g(int $a): int { return ~$a; }"#);
    assert!(not.contains('~'), "{not}"); // printer parenthesizes: `~(a)` until C-5/6 lands
    assert_reparses(&not);
}

// ── C-1: PHP string interpolation → Phorge `"{…}"` (faithful access-chain subset) ──

#[test]
fn lifts_simple_variable_interpolation() {
    // C-1: `"$name"` simple syntax → Phorge `"{name}"`.
    let out = lift(r#"<?php function f(string $name): string { return "hi $name!"; }"#);
    assert!(out.contains(r#""hi {name}!""#), "{out}");
    assert_reparses(&out);
}

#[test]
fn lifts_simple_property_interpolation() {
    // C-1: simple syntax allows ONE `->prop`.
    let out = lift(
        r#"<?php class P { public function __construct(public string $n) {} public function g(): string { return "v=$this->n"; } }"#,
    );
    assert!(out.contains(r#""v={this.n}""#), "{out}");
    assert_reparses(&out);
}

#[test]
fn lifts_complex_member_and_method_interpolation() {
    // C-1: `{$o->prop}` and `{$o->m()}` complex syntax → `{o.prop}` / `{o.m()}`.
    let out =
        lift(r#"<?php function f(Box $o): string { return "p={$o->total} m={$o->label()}"; }"#);
    assert!(out.contains("{o.total}"), "{out}");
    assert!(out.contains("{o.label()}"), "{out}");
    assert_reparses(&out);
}

#[test]
fn lifts_index_interpolation() {
    // C-1: `{$a[$k]}` and simple `$a[0]` → `{a[k]}` / `{a[0]}`. Top-level so `$xs`/`$k` are locals
    // (a PHP `array` param type is itself Tier-2).
    let out = lift(r#"<?php $xs = [10, 20]; $k = 0; echo "a={$xs[$k]} b=$xs[0]";"#);
    assert!(out.contains("{xs[k]}"), "{out}");
    assert!(out.contains("{xs[0]}"), "{out}");
    assert_reparses(&out);
}

#[test]
fn refuses_operator_in_interpolation() {
    // C-1: a top-level operator inside `{$…}` is a PHP parse error AND outside Phorge's `{…}`
    // grammar — loud reject (never lift to something that won't parse).
    let e = lift_err(r#"<?php function f(int $a, int $b): int { return "{$a + $b}"; }"#);
    assert!(e.contains("access chain"), "{e}");
}

#[test]
fn refuses_bareword_simple_subscript() {
    // C-1: `"$a[key]"` coerces `key` to the string `'key'` — surprising. Reject; nudge to `{$a['key']}`.
    let e = lift_err(r#"<?php function f(Map $a): string { return "$a[key]"; }"#);
    assert!(e.contains("bareword subscript"), "{e}");
}

#[test]
fn top_level_code_becomes_main_with_console_import() {
    let out = lift(r#"<?php $x = 1; echo $x;"#);
    assert!(out.contains("import Core.Console;"), "{out}");
    assert!(out.contains("function main(): void {"), "{out}");
    assert!(out.contains("mutable var x = 1;"), "{out}");
    assert!(out.contains("Console.print(x);"), "{out}");
    assert_reparses(&out);
}

#[test]
fn php_concat_becomes_plus_and_strict_eq_becomes_eq() {
    let out = lift(r#"<?php function g(string $a): string { return $a . "!"; }"#);
    assert!(out.contains(r#"return a + "!";"#), "concat → +: {out}");
    let out2 = lift("<?php function h(int $a, int $b): bool { return $a === $b; }");
    assert!(out2.contains("return a == b;"), "=== → ==: {out2}");
    assert_reparses(&out);
    assert_reparses(&out2);
}

#[test]
fn lifts_class_with_promotion_and_method() {
    let out = lift(
        "<?php class Engine {\n\
           public function __construct(private int $power) {}\n\
           public function powerOf(): int { return $this->power; }\n\
         }",
    );
    assert!(out.contains("class Engine {"), "{out}");
    // Promoted prop is private + mutable (PHP fields are mutable); `constructor` keyword.
    assert!(
        out.contains("constructor(private mutable int power) {}"),
        "{out}"
    );
    // A public, non-final PHP method lifts to an `open` Phorge method (override-by-default).
    assert!(out.contains("function powerOf(): int {"), "{out}");
    assert!(out.contains("return this.power;"), "{out}");
    assert_reparses(&out);
}

#[test]
fn lifts_pure_enum() {
    let out = lift("<?php enum Dir { case Up; case Down; }");
    assert!(out.contains("enum Dir { Up, Down }"), "{out}");
    assert_reparses(&out);
}

#[test]
fn lifts_c_style_for_loop() {
    let out = lift("<?php function f(): void { for ($i = 0; $i < 3; $i++) { echo $i; } }");
    assert!(
        out.contains("for (mutable var i = 0; i < 3; i = i + 1) {"),
        "{out}"
    );
    assert_reparses(&out);
}

#[test]
fn lifts_keyless_foreach() {
    // A-6 gave for-in element-type inference, so a keyless PHP foreach now lifts to the idiomatic
    // Phorge `foreach (xs as x)` (printed from a Type::Infer for-in).
    let out = lift("<?php $xs = [1, 2, 3]; foreach ($xs as $x) { echo $x; }");
    assert!(out.contains("foreach (xs as x) {"), "{out}");
    assert_reparses(&out);
}

#[test]
fn lifts_match_with_default() {
    let out = lift(
        r#"<?php function f(int $n): string { return match ($n) { 0, 1 => "low", default => "hi" }; }"#,
    );
    // PHP multi-cond arm `0, 1 => …` duplicates per literal; default → `_`.
    assert!(
        out.contains(r#"0 => "low""#) && out.contains(r#"1 => "low""#),
        "{out}"
    );
    assert!(out.contains(r#"_ => "hi""#), "{out}");
    assert_reparses(&out);
}

#[test]
fn lifts_array_literals_to_list_and_map() {
    let out = lift(r#"<?php function f(): void { $a = [1, 2]; $m = ["k" => 3]; }"#);
    assert!(out.contains("mutable var a = [1, 2];"), "{out}");
    assert!(out.contains(r#"mutable var m = ["k" => 3];"#), "{out}");
    assert_reparses(&out);
}

// ── loud Tier-2 / no-equivalent lift errors (never a silent wrong lift) ──

#[test]
fn refuses_tier2_constructs() {
    for (php, frag) in [
        ("<?php enum Suit: string { case H = 'h'; }", "backed enum"),
        (
            "<?php enum E { case A; public function f(): int { return 1; } }",
            "has methods",
        ),
        ("<?php function f(array $xs): void {}", "`array` type"),
        (
            "<?php function f(int $n = 7): void {}",
            "default value on parameter",
        ),
        ("<?php class C { private int $x = 0; }", "has a default"),
        (
            "<?php function f(array $m): void { foreach ($m as $k => $v) {} }",
            "`array` type", // array param trips first; key-foreach covered below
        ),
        ("<?php function f($x): void {}", "has no type"),
        (
            r#"<?php function f(int $a): int { return $a ?: 1; }"#,
            "elvis",
        ),
        (
            r#"<?php function f(int $n): int { return match ($n) { foo() => 1, default => 0 }; }"#,
            "non-literal condition",
        ),
    ] {
        let e = lift_err(php);
        assert!(
            e.contains(frag),
            "for {php:?}\n  expected substring {frag:?}\n  got {e}"
        );
    }
}

#[test]
fn refuses_key_foreach() {
    let e = lift_err("<?php function f(): void { foreach ([1] as $k => $v) {} }");
    assert!(e.contains("foreach with a key"), "{e}");
}

#[test]
fn refuses_main_collision() {
    let e = lift_err("<?php function main(): void {} echo 1;");
    assert!(e.contains("ambiguous entry"), "{e}");
}

#[test]
fn end_to_end_representative_program_reparses() {
    let out = lift(
        "<?php\n\
         class Counter {\n\
           public function __construct(private int $n) {}\n\
           public function value(): int { return $this->n; }\n\
         }\n\
         function classify(int $x): string {\n\
           if ($x < 0) { return \"neg\"; } elseif ($x === 0) { return \"zero\"; } else { return \"pos\"; }\n\
         }\n\
         $total = 0;\n\
         for ($i = 0; $i < 5; $i++) { $total = $total + $i; }\n\
         echo classify($total);\n",
    );
    assert!(out.contains("class Counter {"), "{out}");
    assert!(
        out.contains("constructor(private mutable int n) {}"),
        "{out}"
    );
    assert!(out.contains("function classify(int x): string {"), "{out}");
    assert!(out.contains("function main(): void {"), "{out}");
    assert_reparses(&out);
}
