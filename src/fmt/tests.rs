//! `phg fmt` tests. Two invariants gate the formatter:
//!   * **meaning preservation** — for runnable programs, `cmd_run(src) == cmd_run(fmt(src))`;
//!   * **idempotence** — `fmt(fmt(src)) == fmt(src)` (across the full language surface).
//!
//! Plus: comments survive, and an unparseable file is refused (never reformatted).

use super::format;
use crate::cli::cmd_run;

fn fmt(src: &str) -> String {
    format(src).unwrap_or_else(|e| panic!("fmt failed: {e:?}\n--- src ---\n{src}"))
}

/// `fmt` is idempotent: a second pass changes nothing.
fn assert_idempotent(src: &str) {
    let once = fmt(src);
    let twice = fmt(&once);
    assert_eq!(
        once, twice,
        "not idempotent.\n--- once ---\n{once}\n--- twice ---\n{twice}"
    );
}

/// Formatting preserves runtime behavior: the program runs identically before and after.
fn assert_meaning_preserved(src: &str) {
    let before = cmd_run(src);
    let after = cmd_run(&fmt(src));
    assert_eq!(
        before, after,
        "behavior changed by fmt.\n--- before ---\n{before:?}\n--- after ---\n{after:?}\n--- fmt ---\n{}",
        fmt(src)
    );
    assert!(before.is_ok(), "sample must run: {before:?}");
    assert_idempotent(src);
}

#[test]
fn runnable_programs_keep_their_behavior() {
    // A spread of real surface: classes+ctor promotion, enums+match+guards, generics, lambdas+pipe,
    // optionals, ranges, string interpolation.
    let samples = [
        "package Main; import Core.Output;\nfunction main(): void { Output.printLine(\"hi\"); }",
        "package Main; import Core.Output;\n\
         function add<T>(T a, T b): T { return a; }\n\
         function main(): void { Output.printLine(\"{add(2, 3)}\"); }",
        "package Main; import Core.Output;\n\
         enum Shape { Circle(int r), Square(int s) }\n\
         function area(Shape s): int { return match (s) { Circle(r) => r * r, Square(x) => x * x }; }\n\
         function main(): void { Output.printLine(\"{area(new Circle(3))}\"); }",
        "package Main; import Core.Output;\n\
         class Point { constructor(public int x, public int y) {} function sum(): int { return this.x + this.y; } }\n\
         function main(): void { Point p = new Point(2, 5); Output.printLine(\"{p.sum()}\"); }",
        "package Main; import Core.Output;\n\
         function main(): void { var dbl = function(int x): int => x * 2; Output.printLine(\"{3 |> dbl}\"); }",
        "package Main; import Core.Output;\n\
         function main(): void { int? m = null; Output.printLine(\"{m ?? -1}\"); for (int i in 0..3) { Output.printLine(\"{i}\"); } }",
    ];
    for s in samples {
        assert_meaning_preserved(s);
    }
}

#[test]
fn full_surface_is_idempotent() {
    // Non-runnable fragments (library shapes): proves the printer handles every construct and is
    // stable. Idempotence + clean parse is the gate where we can't run the program.
    let samples = [
        "package Main;\ninterface Speaker { function speak(): string; }\n\
         class Dog implements Speaker { function speak(): string { return \"woof\"; } }",
        "package Main;\ntrait Greet { function hi(): string { return \"hi\"; } }\n\
         class P { use Greet; }",
        "package Main;\nclass Box<T> { constructor(public T value) {} }\n\
         function pick(int | string x): int { return 0; }",
        "package Main;\ntype UserId = int;",
        "package Main;\nclass C { int n { get => 1; } }",
        "package Main;\n\
         function f(): void { try { throw 1; } catch (Error e) { return; } finally { return; } }",
        "package Main;\nclass Pt { constructor(public int x, public int y) {} }\n\
         function f(Pt p): void { var Pt { x, y } = p; }",
        "package Main;\nfunction f(): void { var b = b\"\\x00ab\"; }",
        "package Main; import Core.Test;\ntest \"x\" { Test.assertTrue(true); }",
        "package Main;\nenum Opt<T> { Some(T v), None }\n\
         function f(Opt<int> o): int { return match (o) { Some(n) when n > 0 => n, Some(n) => 0, None => -1 }; }",
    ];
    for s in samples {
        assert_idempotent(s);
    }
}

#[test]
fn the_arrow_return_syntax_normalizes_to_colon() {
    // `-> T` is a transition alias for `: T`; fmt canonicalizes to `:` (both parse the same).
    let out = fmt("package Main;\nfunction f() -> int { return 1; }");
    assert!(out.contains("function f(): int"), "{out}");
    assert!(!out.contains("->"), "{out}");
}

#[test]
fn comments_are_preserved() {
    let src = "package Main;\n// a header comment\nfunction main(): void { /* body */ return; }\n";
    let out = fmt(src);
    assert!(
        out.contains("// a header comment"),
        "own-line comment lost:\n{out}"
    );
    assert!(out.contains("/* body */"), "block comment lost:\n{out}");
    assert_eq!(out, fmt(&out), "comment-bearing output must be idempotent");
}

#[test]
fn unparseable_source_is_refused_not_reformatted() {
    assert!(format("package Main;\nfunction (").is_err());
    assert!(format("@@@ not phorj").is_err());
}

#[test]
fn declaration_visibility_survives_formatting() {
    // Regression: the printer used to drop top-level `internal`/`private` visibility on free functions
    // and types (only `Public` is the default and elided). Splitting public types across files relies
    // on these surviving, so assert they round-trip and the form is idempotent.
    let src = "package Main;\n\
        internal function scale(int n): int { return n; }\n\
        private function clamp(int n): int { return n; }\n\
        internal class Helper { constructor() {} }\n\
        private enum Mode { On(), Off() }\n\
        function main(): void {}";
    let out = fmt(src);
    assert!(
        out.contains("internal function scale"),
        "lost internal fn:\n{out}"
    );
    assert!(
        out.contains("private function clamp"),
        "lost private fn:\n{out}"
    );
    assert!(
        out.contains("internal class Helper"),
        "lost internal class:\n{out}"
    );
    assert!(
        out.contains("private enum Mode"),
        "lost private enum:\n{out}"
    );
    assert_idempotent(src);
}
