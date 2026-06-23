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
use phorge::{cli, loader};
use std::process::Command;

/// Type-check `src`; return the error diagnostics (empty = well-typed). Auto-prepends
/// `package Main;` if absent. Used to test checker rejections without running a backend.
fn check_errs(src: &str) -> Vec<phorge::diagnostic::Diagnostic> {
    let src = with_pkg(src);
    let tokens = phorge::lexer::lex(&src).expect("lex ok");
    let prog = phorge::parser::Parser::new(tokens)
        .parse_program()
        .expect("parse ok");
    match phorge::checker::check(&prog) {
        Ok(_warnings) => Vec::new(),
        Err(e) => e,
    }
}

/// Transpile `src` to PHP; panics if the program fails to type-check or transpile.
/// Auto-prepends `package Main;` if absent.
fn transpile_ok(src: &str) -> String {
    let src = with_pkg(src);
    cli::cmd_transpile(&src).expect("transpile ok")
}

/// Assert the two backends agree on success output. Compares `Result` values structurally
/// (never `.expect()`): in release builds an unchecked-arithmetic divergence surfaces as an
/// `Err` rather than a panic, and a structural compare reports it as a clean mismatch.
/// Prepend the reserved `package Main;` (M5 S1: every file is packaged, never inferred) to a test
/// program that doesn't already declare one. Done on a single leading segment with no newline so
/// line numbers are preserved — fault diagnostics that assert a line stay valid.
fn with_pkg(src: &str) -> String {
    if src.trim_start().starts_with("package ") {
        src.to_string()
    } else {
        format!("package Main; {src}")
    }
}

fn agree(src: &str) {
    let src = with_pkg(src);
    let tree = cmd_run(&src);
    let vm = cmd_runvm(&src);
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
    /// `opt!` force-unwrap of a `null` value — a checker-allowed, runtime-reachable fault (the
    /// checker permits `!` but warns; absence is only known at runtime). Classified by body
    /// substring so the VM's `Op::Fault(ForceUnwrapNull)` and the interpreter's `rt(..)` agree (M3
    /// S2.5).
    ForceUnwrap,
    /// A range literal wider than `value::MAX_RANGE_LEN` — a checker-valid, runtime-reachable fault
    /// (the checker proves the bounds are ints, never that the span fits in memory). Both backends
    /// fault `"range too large"` (P1-#9) instead of OOM-aborting (exit 101); classified by body
    /// substring so the VM's line prefix doesn't split it from the interpreter's prefix-less render.
    RangeTooLarge,
    /// An explicit programmer abort — `panic`/`todo`/`unreachable`/failed `assert` (M-faults 2a). All
    /// four share one kind: their bodies are single-sourced on `FaultMsg`, so both backends render
    /// byte-identically; classifying by body substring keeps the VM's line prefix from splitting them.
    Panic,
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
    } else if err.contains("force-unwrap of null") {
        FaultKind::ForceUnwrap
    } else if err.contains("range too large") {
        FaultKind::RangeTooLarge
    } else if err.contains("panic:")
        || err.contains("not yet implemented")
        || err.contains("unreachable code")
        || err.contains("assertion failed")
    {
        FaultKind::Panic
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
    let src = with_pkg(src);
    let tree = cmd_run(&src);
    let vm = cmd_runvm(&src);
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
    r#"import Core.Console;
function main() { Console.println("hello"); }"#,
    r#"import Core.Console;
function main() { Console.println("{42}"); Console.println("{3.14}"); Console.println("{true}"); }"#,
    // int + float arithmetic (formatting parity: 12.0 -> "12")
    r#"import Core.Console;
function main() { Console.println("{1 + 2 * 3 - 4}"); }"#,
    r#"import Core.Console;
function main() { Console.println("{2.0 * 3.0}"); Console.println("{7.5 / 2.5}"); }"#,
    r#"import Core.Console;
function main() { Console.println("{7 % 3}"); Console.println("{7.5 % 2.0}"); }"#,
    // comparison + equality + logical short-circuit
    r#"import Core.Console;
function main() { Console.println("{1 < 2}"); Console.println("{2 <= 2}"); Console.println("{3 > 4}"); }"#,
    r#"import Core.Console;
function main() { Console.println("{1 == 1}"); Console.println("{1 != 2}"); }"#,
    r#"import Core.Console;
function main() { Console.println("{1 < 2 && 2 < 3}"); Console.println("{1 > 2 || 3 > 2}"); }"#,
    // unary
    r#"import Core.Console;
function main() { Console.println("{-5}"); Console.println("{!false}"); }"#,
    // locals (int + float + string + bool)
    r#"import Core.Console;
function main() { int x = 10; float y = 2.5; Console.println("{x}"); Console.println("{y}"); }"#,
    r#"import Core.Console;
function main() { string s = "hi"; bool b = true; Console.println("{s}"); Console.println("{b}"); }"#,
    r#"import Core.Console;
function main() { int a = 3; int b = 4; Console.println("{a * a + b * b}"); }"#,
    // if / else
    r#"import Core.Console;
function main() { if (1 < 2) { Console.println("a"); } else { Console.println("b"); } }"#,
    r#"import Core.Console;
function main() { int n = 5; if (n > 3) { Console.println("big"); } Console.println("end"); }"#,
    // for-in over list literals
    r#"import Core.Console;
function main() { List<int> xs = [1, 2, 3]; for (int x in xs) { Console.println("{x}"); } }"#,
    r#"import Core.Console;
function main() { for (float f in [1.5, 2.5]) { Console.println("{f * 2.0}"); } }"#,
    // nested blocks + for body locals
    r#"import Core.Console;
function main() { for (int x in [10, 20]) { int y = x + 1; Console.println("{y}"); } }"#,
    // NB: `println` is single-arg only (the checker enforces it) — no multi-arg case here.
];

#[test]
fn p2_programs_match_between_backends() {
    for src in P2_PROGRAMS {
        agree(src);
    }
}

/// M-RT S6a — single inheritance: an inherited method, an overridden method (via a subclass ref),
/// and dynamic dispatch (via a superclass-typed ref holding the subclass) all resolve identically on
/// `run` and `runvm`. The interpreter walks the parent chain; the compiler pre-flattens the same
/// lookup into the VM's method table.
#[test]
fn s6_inheritance_dispatch_is_byte_identical() {
    agree(
        r#"import Core.Console;
open class Animal {
    function speak() -> string { return "..."; }
    open function kind() -> string { return "animal"; }
}
class Dog extends Animal {
    function kind() -> string { return "dog"; }
}
function main() {
    Dog d = Dog();
    Console.println(d.speak());
    Console.println(d.kind());
    Animal a = d;
    Console.println(a.kind());
}"#,
    );
}

/// M-RT S6b.1 — multi-parent composition. A method declared on the *second* parent must dispatch on
/// both backends (the latent trap: the interpreter walked only the first-parent chain while the
/// compiler BFS-flattened every parent — so `d.soar()` faulted on `run` but resolved on `runvm`). A
/// non-overridden method from the first parent and a diamond shared-base method (auto-merged because
/// both arms reach the *same* declaring method) must also resolve identically.
#[test]
fn s6b_multi_parent_dispatch_is_byte_identical() {
    agree(
        r#"import Core.Console;
open class Swimmer {
    open function move() -> string { return "swims"; }
    function wet() -> string { return "wet"; }
}
open class Flyer {
    open function soar() -> string { return "soars"; }
}
class Duck extends Swimmer, Flyer {}
function main() {
    Duck d = Duck();
    Console.println(d.move()); // first parent
    Console.println(d.soar()); // SECOND parent — the latent divergence
    Console.println(d.wet());  // inherited, non-overridden
}"#,
    );
}

/// M-RT S6b.1 — diamond shared base. `Mid` reaches `Base.tag()` through both `Left` and `Right`;
/// because both arms resolve to the *same* declaring method, it auto-merges (no conflict) and
/// dispatches identically on both backends. A subtype flows into any ancestor-typed binding.
#[test]
fn s6b_diamond_shared_base_is_byte_identical() {
    agree(
        r#"import Core.Console;
open class Base { open function tag() -> string { return "base"; } }
open class Left extends Base {}
open class Right extends Base {}
class Mid extends Left, Right {}
function main() {
    Mid m = Mid();
    Console.println(m.tag());
    Base b = m;
    Console.println(b.tag());
}"#,
    );
}

/// M-RT S6b.2 — a cross-parent method collision resolved with `use P.m` dispatches identically on
/// both backends. `Swimmer.move` and `Flyer.move` collide; `use Flyer.move` (the *second* parent —
/// the case BFS-first-wins would get wrong) picks Flyer's, so `d.move()` must run Flyer's body on
/// `run` and `runvm` alike.
#[test]
fn s6b_resolution_use_picks_the_named_parent() {
    agree(
        r#"import Core.Console;
open class Swimmer { open function move() -> string { return "swims"; } }
open class Flyer { open function move() -> string { return "flies"; } }
class Duck extends Swimmer, Flyer {
    use Flyer.move
}
function main() {
    Duck d = Duck();
    Console.println(d.move()); // Flyer's, per the resolution clause
}"#,
    );
}

/// M-RT S6b.2 — `rename P.m as n` keeps both colliding methods: the renamed one under the new name,
/// the other under the original. `rename Flyer.move as glide` leaves `move` resolved to Swimmer (the
/// only remaining source) and binds `glide` to Flyer's `move`. Both calls dispatch identically.
#[test]
fn s6b_resolution_rename_keeps_both() {
    agree(
        r#"import Core.Console;
open class Swimmer { open function move() -> string { return "swims"; } }
open class Flyer { open function move() -> string { return "flies"; } }
class Duck extends Swimmer, Flyer {
    rename Flyer.move as glide
}
function main() {
    Duck d = Duck();
    Console.println(d.move());  // Swimmer's (the remaining source)
    Console.println(d.glide()); // Flyer's, under the new name
}"#,
    );
}

/// M-RT S6b.3 — abstract classes. A concrete method on an `abstract class` (`describe`) calls an
/// `abstract` method (`area`); on a concrete subclass instance, dispatch resolves `area` to the
/// subclass's implementation (the template-method pattern). Both backends must agree, including
/// through a base-typed binding.
#[test]
fn s6b_abstract_template_method_is_byte_identical() {
    agree(
        r#"import Core.Console;
abstract class Shape {
    abstract function area() -> int;
    function describe() -> string { return "area={this.area()}"; }
}
class Square extends Shape {
    constructor(public int side) {}
    function area() -> int { return this.side * this.side; }
}
function main() {
    Square s = Square(3);
    Console.println("{s.area()}");
    Console.println(s.describe()); // describe() dispatches to Square.area()
    Shape sh = s;
    Console.println(sh.describe());
}"#,
    );
}

/// M-RT S6c.1 — field collision. A same-named instance field declared independently on two parents
/// (promoted ctor params here) has no PHP `insteadof` for properties, so it is `E-MI-FIELD-CONFLICT`
/// unless the child redeclares it. The checker must flag it (the previous silent first-parent-wins
/// merge masked the clash).
#[test]
fn s6c_field_conflict_rejected() {
    let errs = check_errs(
        r#"open class Swimmer { constructor(public int depth) {} }
open class Flyer { constructor(public int depth) {} }
class Duck extends Swimmer, Flyer {}"#,
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-MI-FIELD-CONFLICT")),
        "two parents declaring `depth` must be E-MI-FIELD-CONFLICT, got: {errs:?}"
    );
}

/// M-RT S6c.1 — a diamond-shared field auto-merges (no conflict): `id` reaches `Mid` through both
/// `Left` and `Right`, but both arms resolve to the *same* declaring origin (`Base`), so it dedups
/// exactly like a diamond-shared method. No `E-MI-FIELD-CONFLICT`.
#[test]
fn s6c_diamond_shared_field_is_not_a_conflict() {
    let errs = check_errs(
        r#"open class Base { constructor(public int id) {} }
open class Left extends Base {}
open class Right extends Base {}
class Mid extends Left, Right {}"#,
    );
    assert!(
        !errs.iter().any(|e| e.code == Some("E-MI-FIELD-CONFLICT")),
        "a diamond-shared field must auto-merge (no conflict), got: {errs:?}"
    );
}

/// Assert both backends AND real PHP all produce exactly `expected` for `src`. A construction test
/// needs this stronger check than `agree` — a shared *failure* (e.g. a checker rejection) makes a bare
/// `agree` pass vacuously (both backends "agree" on the error). Auto-prepends `package Main;`.
fn agree_out_php(src: &str, expected: &str, label: &str) {
    let src = with_pkg(src);
    let tree = cmd_run(&src);
    let vm = cmd_runvm(&src);
    assert_eq!(
        tree, vm,
        "run vs runvm for {label}:\n  run={tree:?}\n  runvm={vm:?}"
    );
    let out = tree.unwrap_or_else(|e| panic!("{label}: program errored on `run`: {e}"));
    assert_eq!(out, expected, "interpreter output for {label}");
    if let Some(php) = php_or_gate(label) {
        let php_src = cli::cmd_transpile(&src).expect("transpile ok");
        let got = run_php(&php, &php_src, label);
        assert_eq!(
            got, expected,
            "PHP ≠ expected for {label}\n--- php ---\n{php_src}"
        );
    }
}

/// M-RT S6c.2a — single-parent constructor inheritance. A subclass with **no own constructor**
/// inherits its parent's: `Greeter("Ada")` runs the inherited ctor (promoting `name`) on a `Greeter`
/// instance. PHP inherits the ctor natively; the interpreter walks the parent chain and the compiler
/// uses the inherited ctor for the instance descriptor — all three must agree on the *output*.
#[test]
fn s6c_single_parent_ctor_inheritance_is_byte_identical() {
    agree_out_php(
        r#"import Core.Console;
open class Named { constructor(public string name) {} }
class Greeter extends Named {}
function main() {
    Greeter g = Greeter("Ada");
    Console.println(g.name);
}"#,
        "Ada\n",
        "s6c_single_parent_ctor_inheritance",
    );
}

/// M-RT S6c.2a — a parent constructor with a *body* (not just promotion) runs identically through the
/// child, and the inheritance chains through multiple no-own-ctor levels.
#[test]
fn s6c_inherited_ctor_body_and_chain_are_byte_identical() {
    // parent ctor body sets a non-promoted field; child inherits it
    agree_out_php(
        r#"import Core.Console;
open class Counter {
    mutable int n;
    constructor(int start) { this.n = start; }
    function value() -> int { return this.n; }
}
class Tally extends Counter {}
function main() {
    Tally t = Tally(7);
    Console.println("{t.value()}");
}"#,
        "7\n",
        "s6c_inherited_ctor_body",
    );
    // a two-level chain: Mid and Leaf both have no own ctor, inherit Root's
    agree_out_php(
        r#"import Core.Console;
open class Root { constructor(public int id) {} }
open class Mid extends Root {}
class Leaf extends Mid {}
function main() {
    Leaf l = Leaf(42);
    Console.println("{l.id}");
}"#,
        "42\n",
        "s6c_inherited_ctor_chain",
    );
}

/// M-RT S6c.2b — multi-parent orchestrating constructor. A class with ≥2 parents and **no own
/// constructor** inherits a synthesized constructor whose parameters are the parents' ctor params
/// concatenated in `extends` order; constructing it runs each parent's constructor (with its arg
/// slice) on the one instance, initializing every inherited field. Byte-identical run≡runvm≡real PHP.
#[test]
fn s6c_multi_parent_ctor_is_byte_identical() {
    // promotion-only parents: all inherited fields populated from the concatenated args
    agree_out_php(
        r#"import Core.Console;
open class Named { constructor(public string name) {} }
open class Aged { constructor(public int age) {} }
class Person extends Named, Aged {}
function main() {
    Person p = Person("Ada", 36);
    Console.println("{p.name} is {p.age}");
}"#,
        "Ada is 36\n",
        "s6c_multi_parent_ctor_promotion",
    );
    // a parent constructor with a *body* (derives a field) runs through the orchestration
    agree_out_php(
        r#"import Core.Console;
open class Named { constructor(public string name) {} }
open class Scored {
    mutable int doubled;
    constructor(int score) { this.doubled = score * 2; }
}
class Player extends Named, Scored {}
function main() {
    Player p = Player("Bo", 21);
    Console.println("{p.name} {p.doubled}");
}"#,
        "Bo 42\n",
        "s6c_multi_parent_ctor_body",
    );
}

/// M-RT S6c.3 — `instanceof`/subtyping across the full class lattice. The runtime `instanceof` oracle
/// previously consulted only interfaces (`class_implements`), so `d instanceof Animal` against a
/// *parent class* was wrongly `false` on both Rust backends — and a multi-parent class lowers to PHP
/// `implements I…`, so `instanceof Swimmer` (the concrete class) was wrong there too. The fix threads
/// the transitive parent-class set into the oracle (`ast::instanceof_table`) and emits the interface
/// form for a decomposed ancestor in PHP. Single-parent, multi-parent, and a parent-typed param must
/// all be byte-identical run≡runvm≡real PHP.
#[test]
fn s6c_instanceof_across_lattice_is_byte_identical() {
    // single-parent: `instanceof` against the parent class is true
    agree_out_php(
        r#"import Core.Console;
open class Animal {}
class Dog extends Animal {}
function main() {
    Dog d = Dog();
    Console.println("{d instanceof Animal} {d instanceof Dog}");
}"#,
        "true true\n",
        "s6c_instanceof_single_parent",
    );
    // multi-parent: `instanceof` against each parent + a parent-typed param accepts the subtype
    agree_out_php(
        r#"import Core.Console;
open class Swimmer { open function move() -> string { return "swims"; } }
open class Flyer { open function soar() -> string { return "soars"; } }
class Duck extends Swimmer, Flyer {}
function describe(Swimmer s) -> string { return s.move(); }
function main() {
    Duck d = Duck();
    Console.println(describe(d));
    Console.println("{d instanceof Swimmer} {d instanceof Flyer}");
}"#,
        "swims\ntrue true\n",
        "s6c_instanceof_multi_parent",
    );
    // a non-subtype `instanceof` stays false across the lattice
    agree_out_php(
        r#"import Core.Console;
open class A {}
open class B {}
class C extends A {}
function main() {
    C c = C();
    Console.println("{c instanceof A} {c instanceof B}");
}"#,
        "true false\n",
        "s6c_instanceof_non_subtype",
    );
}

/// M3 S0.2 — `var` local type inference is a front-end-only feature (type erased after checking),
/// so both backends must run a `var` program byte-identically.
#[test]
fn s0_var_inference_is_byte_identical() {
    agree(
        r#"import Core.Console;
function main() {
            var x = 21;
            var s = "n=";
            Console.println("{s}{x + x}");
        }"#,
    );
}

/// `var` whose initializer is a call result and a `match` value — both must specialize arithmetic
/// identically (the compiler infers the local's `CTy` from the initializer, not an annotation).
#[test]
fn s0_var_inference_from_call_and_match_inits() {
    agree(
        r#"import Core.Console;
function dbl(int n) -> int { return n * 2; }
        function main() {
            var a = dbl(10);
            var b = match a { 20 => 100, n => n };
            Console.println("{a + b}");
        }"#,
    );
}

/// M3 S0.3 — a `type` alias is compile-time-only (erased); resolving params/returns through it
/// must not change runtime behavior on either backend.
#[test]
fn s0_type_alias_is_byte_identical() {
    agree(
        r#"import Core.Console;
type Count = int;
        function tally(Count n) -> Count { return n + 1; }
        function main() { Console.println("{tally(41)}"); }"#,
    );
}

/// M3 S1.1 — list indexing `xs[i]`. The checker already typed it; the backends were un-rejected
/// this slice. Reads must be byte-identical, and an out-of-range read must *fault* identically
/// (the VM's bounds check + the interpreter's must agree — `FaultKind::IndexOob`).
#[test]
fn s1_indexing_is_byte_identical() {
    agree(
        r#"import Core.Console;
function main() { List<int> xs = [10, 20, 30]; Console.println("{xs[0]} {xs[2]}"); }"#,
    );
    // an index expression on a list literal, with the index coming from a loop variable
    agree(
        r#"import Core.Console;
function main() { for (int i in [0, 1, 2]) { Console.println("{[5, 6, 7][i]}"); } }"#,
    );
}

#[test]
fn s1_index_oob_faults_identically() {
    agree_err(
        r#"import Core.Console;
function main() { List<int> xs = [1, 2]; Console.println("{xs[5]}"); }"#,
    );
}

/// An index *result* used as an arithmetic operand (`xs[0] + 1`). The compiler must know the list's
/// element type to pick `AddI`/`AddF` — so `CTy` tracks `List<elem>` and `ctype(Index)` unwraps it.
/// (Regression guard: un-rejecting indexing without this made the VM compile-reject `xs[0] + 1`
/// while the interpreter accepted it.)
#[test]
fn s1_index_result_in_arithmetic_is_byte_identical() {
    agree(
        r#"import Core.Console;
function main() { List<int> xs = [10, 20]; Console.println("{xs[0] + 1}"); }"#,
    );
    agree(
        r#"import Core.Console;
function main() { List<float> fs = [1.5, 2.5]; Console.println("{fs[0] + fs[1]}"); }"#,
    );
    // index result of a range-materialized list, used arithmetically
    agree(
        r#"import Core.Console;
function main() { var xs = 0..5; Console.println("{xs[2] * 10}"); }"#,
    );
}

/// M3 S1.2 — integer ranges `a..b` (exclusive) / `a..=b` (inclusive), materialized to `List<int>`
/// via the one new `Op::MakeRange`. The compiler/interpreter must build the *same* list (same order,
/// same emptiness rule) so `for…in` over a range is byte-identical on both backends.
#[test]
fn s1_ranges_are_byte_identical() {
    agree(
        r#"import Core.Console;
function main() { for (int i in 0..3) { Console.println("{i}"); } }"#,
    ); // 0,1,2
    agree(
        r#"import Core.Console;
function main() { for (int i in 1..=3) { Console.println("{i}"); } }"#,
    ); // 1,2,3
       // empty range (start >= end): the body never runs on either backend
    agree(
        r#"import Core.Console;
function main() { for (int i in 5..5) { Console.println("{i}"); } Console.println("done"); }"#,
    );
    agree(
        r#"import Core.Console;
function main() { for (int i in 5..2) { Console.println("{i}"); } Console.println("empty"); }"#,
    );
    // a range bound to a `var` (typed `List<int>`), then iterated
    agree(
        r#"import Core.Console;
function main() { var xs = 0..3; for (int i in xs) { Console.println("{i + 1}"); } }"#,
    );
    // range bounds from expressions
    agree(
        r#"import Core.Console;
function lo() -> int { return 2; } function main() { for (int i in lo()..lo() + 3) { Console.println("{i}"); } }"#,
    );
}

/// M3 S1.3 — expression `if` (`if (c) { e } else { e }`) in value position. No new `Op` — it lowers
/// to the existing branch ops (like `&&`/`||`/`match`), so both backends leave the same single value
/// on the stack and must agree.
#[test]
fn s1_expression_if_is_byte_identical() {
    // value-position in a `var` initializer, then used arithmetically (specialization parity)
    agree(
        r#"import Core.Console;
function main() { var x = if (1 < 2) { 10 } else { 20 }; Console.println("{x + x}"); }"#,
    );
    // in return position, both branches taken across two calls
    agree(
        r#"import Core.Console;
function pick(bool b) -> int { return if (b) { 1 } else { 2 }; }
           function main() { Console.println("{pick(true)} {pick(false)}"); }"#,
    );
    // as a call argument (string-typed branches), inside a range loop
    agree(
        r#"import Core.Console;
function main() { for (int i in 0..3) { Console.println(if (i == 1) { "one" } else { "x" }); } }"#,
    );
    // nested / float branches
    agree(
        r#"import Core.Console;
function main() { float r = if (true) { 1.5 } else { 2.5 }; Console.println("{r * 2.0}"); }"#,
    );
}

/// P3 surface: user function calls, recursion, mutual recursion, void functions, returns in
/// branches, nested calls, float-returning functions, and calls as statements. Each must run
/// identically on both backends.
const P3_PROGRAMS: &[&str] = &[
    // single call used in interpolation
    r#"import Core.Console;
function inc(int n) -> int { return n + 1; } function main() { Console.println("{inc(41)}"); }"#,
    // multiple params + call inside arithmetic
    r#"import Core.Console;
function add(int a, int b) -> int { return a + b; }
       function main() { Console.println("{add(2, 3) * 10}"); }"#,
    // recursion (classic fib)
    r#"import Core.Console;
function fib(int n) -> int {
           if (n < 2) { return n; }
           return fib(n - 1) + fib(n - 2);
       }
       function main() { Console.println("{fib(12)}"); }"#,
    // return in a branch vs fall-through
    r#"import Core.Console;
function sign(int n) -> int { if (n < 0) { return -1; } return 1; }
       function main() { Console.println("{sign(-9)}"); Console.println("{sign(4)}"); }"#,
    // mutual recursion (forward reference: isEven calls isOdd declared later)
    r#"import Core.Console;
function isEven(int n) -> bool { if (n == 0) { return true; } return isOdd(n - 1); }
       function isOdd(int n) -> bool { if (n == 0) { return false; } return isEven(n - 1); }
       function main() { Console.println("{isEven(10)}"); Console.println("{isOdd(7)}"); }"#,
    // nested calls
    r#"import Core.Console;
function sq(int n) -> int { return n * n; }
       function main() { Console.println("{sq(sq(2))}"); }"#,
    // float-returning function in float arithmetic
    r#"import Core.Console;
function half(float x) -> float { return x / 2.0; }
       function main() { Console.println("{half(5.0) + 1.0}"); }"#,
    // void function (no return type) called for its side effect
    r#"import Core.Console;
function greet(string who) { Console.println("hi, {who}"); }
       function main() { greet("Phorge"); greet("world"); }"#,
    // call used as a statement (return value discarded)
    r#"import Core.Console;
function noisy(int n) -> int { Console.println("got {n}"); return n; }
       function main() { noisy(42); Console.println("done"); }"#,
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
    r#"import Core.Console;
enum Grade { Pass(int score), Fail(int score), }
       function describe(Grade g) -> string {
           return match g {
               Pass(s) => "PASS ({s})",
               Fail(s) => "FAIL ({s})",
           };
       }
       function main() { Console.println(describe(Pass(90))); Console.println(describe(Fail(40))); }"#,
    // bare (no-payload) variants, wildcard arm, `match` in var-decl-init position
    r#"import Core.Console;
enum Color { Red, Green, Blue, }
       function main() {
           Color c = Green;
           string name = match c { Red => "red", Green => "green", _ => "other", };
           Console.println(name);
       }"#,
    // literal int patterns + catch-all binding used in interpolation
    r#"import Core.Console;
function label(int n) -> string {
           return match n { 0 => "zero", 1 => "one", x => "many ({x})", };
       }
       function main() { Console.println(label(0)); Console.println(label(1)); Console.println(label(7)); }"#,
    // bool literal patterns
    r#"import Core.Console;
function yn(bool b) -> string { return match b { true => "Y", false => "N", }; }
       function main() { Console.println(yn(true)); Console.println(yn(false)); }"#,
    // string literal patterns + wildcard
    r#"import Core.Console;
function kind(string s) -> string {
           return match s { "a" => "first", "b" => "second", _ => "rest", };
       }
       function main() { Console.println(kind("a")); Console.println(kind("b")); Console.println(kind("z")); }"#,
    // enum value flows through a local and equality (`==` on enums) before matching
    r#"import Core.Console;
enum Dir { N, S, }
       function main() {
           Dir d = N;
           Console.println("{d == N}");
           string t = match d { N => "north", S => "south", };
           Console.println(t);
       }"#,
    // `match` in a *transient* position: as the rhs of `+`, with the lhs already on the operand
    // stack (exercises the compiler's operand-height tracking for the scrutinee slot).
    r#"import Core.Console;
function g(int n) -> int { return 1 + match n { 0 => 10, _ => 20 }; }
       function main() { Console.println("{g(0)}"); Console.println("{g(5)}"); }"#,
    // nested `match` whose inner arm references the *outer* arm's binding (re-extraction across
    // two live scrutinees — the hardest binding/height case in P4a).
    r#"import Core.Console;
enum Pair { P(int a, int b), }
       function f(Pair p) -> string {
           return match p {
               P(a, b) => match a { 0 => "first=zero b={b}", _ => "a={a} b={b}", },
           };
       }
       function main() { Console.println(f(P(0, 9))); Console.println(f(P(5, 2))); }"#,
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
    r#"import Core.Console;
class Point { constructor(public int x, public int y) {} }
       function main() { Point p = Point(3, 4); Console.println("{p.x},{p.y}"); }"#,
    // field read flowing through a typed local, then used as an arithmetic operand
    r#"import Core.Console;
class Point { constructor(public int x, public int y) {} }
       function main() { Point p = Point(3, 4); int s = p.x; Console.println("{s + p.y}"); }"#,
    // constructor *body* runs for side effects (a `println` in the ctor), using a promoted param
    r#"import Core.Console;
class Greeter { constructor(public string name) { Console.println("made {name}"); } }
       function main() { Greeter g = Greeter("Ada"); Console.println("hello {g.name}"); }"#,
    // a no-constructor class builds an empty instance; structural instance equality
    r#"import Core.Console;
class Empty {}
       function main() { Empty a = Empty(); Empty b = Empty(); Console.println("{a == b}"); }"#,
    // instance equality is structural over fields (same class + equal fields)
    r#"import Core.Console;
class P { constructor(public int x) {} }
       function main() { P a = P(1); P b = P(1); P c = P(2); Console.println("{a == b} {a == c}"); }"#,
    // only *promoted* params become fields (the bare `seed` param is not a field)
    r#"import Core.Console;
class Acc { constructor(public int total, int seed) {} }
       function main() { Acc a = Acc(10, 99); Console.println("{a.total}"); }"#,
    // a field read as a call argument
    r#"import Core.Console;
class Box { constructor(public int v) {} }
       function dbl(int n) -> int { return n * 2; }
       function main() { Box b = Box(21); Console.println("{dbl(b.v)}"); }"#,
    // a bare `return;` in the ctor body is an early exit, but the promoted instance is *still*
    // returned (interpreter parity) — exercises the synthetic ctor's epilogue redirect.
    r#"import Core.Console;
class C { constructor(public int x) { if (x > 0) { return; } Console.println("nonpos"); } }
       function main() { C a = C(5); Console.println("{a.x}"); C b = C(0); Console.println("{b.x}"); }"#,
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
        r#"import Core.Console;
class Box { public int tag; constructor(public int x) {} }
           function main() { Box b = Box(5); Console.println("{b.tag}"); }"#,
    );
}

/// P4c: instance methods + `this`. Method dispatch is on the receiver's runtime class; a method
/// body reads fields by bare name (resolved against the current class) or via `this`. Each must run
/// identically on both backends. (No `agree_err` case: like P4a's exhaustiveness, method existence
/// is checker-enforced, so the VM's method-not-found fault is a checker-unreachable backstop.)
const P4C_PROGRAMS: &[&str] = &[
    // a method reads a *bare* field (`total` resolves to `this.total`) + a param
    r#"import Core.Console;
class Counter { constructor(private int total) {} function add(int n) -> int { return total + n; } }
       function main() { Counter c = Counter(100); Console.println("{c.add(23)}"); }"#,
    // a method calls another method via `this`, and reads a field via `this.`
    r#"import Core.Console;
class C { constructor(public int x) {}
           function dbl() -> int { return this.x + this.x; }
           function quad() -> int { int d = this.dbl(); return d + d; } }
       function main() { C c = C(5); Console.println("{c.quad()}"); }"#,
    // mixed bare-field + explicit-`this` field reads in one expression
    r#"import Core.Console;
class P { constructor(public int x, public int y) {} function sum() -> int { return x + this.y; } }
       function main() { P p = P(3, 4); Console.println("{p.sum()}"); }"#,
    // recursion *through* a method (`this.fact(n - 1)`)
    r#"import Core.Console;
class F { constructor(public int base) {}
           function fact(int n) -> int { if (n <= 1) { return 1; } return n * this.fact(n - 1); } }
       function main() { F f = F(0); Console.println("{f.fact(5)}"); }"#,
    // a void (no-return) method invoked as a statement, twice (side effects + Unit result)
    r#"import Core.Console;
class Logger { constructor(public string tag) {} function log() { Console.println("log {tag}"); } }
       function main() { Logger l = Logger("X"); l.log(); l.log(); }"#,
];

#[test]
fn p4c_programs_match_between_backends() {
    for src in P4C_PROGRAMS {
        agree(src);
    }
}

/// Recursively collect every single-file `*.phg` under `dir`, **skipping project roots**. A
/// directory containing a `phorge.toml` is a multi-file project (M5): its files import each other
/// and only run when assembled through `loader::load`, so running them standalone here would fail.
/// `all_example_projects_match_between_backends` gates those instead. The exclusion is structural
/// (keyed on the manifest's presence), not name-based, so any project added under `examples/` later
/// is auto-excluded with no test edit.
fn collect_phg(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if dir.join("phorge.toml").is_file() {
        return; // a project root — handled by the project-aware harness below
    }
    for entry in std::fs::read_dir(dir).expect("read_dir examples/") {
        let path = entry.expect("examples dir entry").path();
        if path.is_dir() {
            collect_phg(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("phg") {
            out.push(path);
        }
    }
}

/// Recursively collect every project root (a directory holding a `phorge.toml`) under `dir`.
fn collect_projects(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if dir.join("phorge.toml").is_file() {
        out.push(dir.to_path_buf());
        return; // projects don't nest in the example set — don't descend further
    }
    for entry in std::fs::read_dir(dir).expect("read_dir examples/") {
        let path = entry.expect("examples dir entry").path();
        if path.is_dir() {
            collect_projects(&path, out);
        }
    }
}

/// The `package Main` entry of a project: the (single) file named `main.phg` under the project root.
/// Examples follow the convention `src/main.phg`, but this walks so a project may place it anywhere.
fn find_main_phg(project_dir: &std::path::Path) -> std::path::PathBuf {
    fn walk(dir: &std::path::Path) -> Option<std::path::PathBuf> {
        let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        entries.sort();
        for p in &entries {
            if p.is_file() && p.file_name().and_then(|n| n.to_str()) == Some("main.phg") {
                return Some(p.clone());
            }
        }
        for p in &entries {
            if p.is_dir() {
                if let Some(found) = walk(p) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(project_dir)
        .unwrap_or_else(|| panic!("project {} has no main.phg entry", project_dir.display()))
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
        // Every example must *run* (produce identical Ok output) — not merely agree. `agree` alone
        // is vacuously green when both backends fail identically (e.g. a broken import), which would
        // hide a malformed example; assert success explicitly so a regression surfaces loudly.
        assert!(
            cmd_run(&src).is_ok(),
            "example {} must run successfully, got {:?}",
            path.display(),
            cmd_run(&src)
        );
        agree(&src);
    }
}

/// M5 S2d — every multi-file **project** under `examples/` must also run byte-identically on both
/// backends. Unlike the single-file glob above, a project is assembled through `loader::load` (which
/// walks up to its `phorge.toml`, parses every file under the source root, validates folder=path, and
/// resolves cross-package qualified calls into one flat program). Because the loader produces concrete
/// bare names before any backend runs, run==runvm is structural — but a malformed example (an import
/// that resolves to nothing, a folder=path violation) would surface as a *shared* failure, which the
/// explicit `Ok` assertion catches. Discovery is glob-based, so a project added later is auto-gated.
#[test]
fn all_example_projects_match_between_backends() {
    let mut projects = Vec::new();
    collect_projects(std::path::Path::new("examples"), &mut projects);
    projects.sort();
    assert!(
        !projects.is_empty(),
        "expected at least one example project (examples/project/*), found none"
    );
    for project in &projects {
        let entry = find_main_phg(project);
        eprintln!("project: {} (entry {})", project.display(), entry.display());
        let unit = loader::load(&entry)
            .unwrap_or_else(|e| panic!("project {} must load: {e}", project.display()));
        let run = cli::run_program(&unit);
        let runvm = cli::runvm_program(&unit);
        assert!(
            run.is_ok(),
            "project {} must run on the interpreter, got {run:?}",
            project.display()
        );
        assert_eq!(
            run,
            runvm,
            "backend mismatch for project {}:\n  run={run:?}\n  runvm={runvm:?}",
            project.display()
        );
    }
}

/// The namespaced stdlib's first native: `Console.println` must lower + run byte-identically on both
/// backends after `import Core.Console;` (M3 Wave 1, the migrated former global `println`).
#[test]
fn namespaced_console_println_matches_between_backends() {
    agree(
        r#"import Core.Console;
             function main() { Console.println("hello"); Console.println("{2 + 2}"); }"#,
    );
}

/// M2 Wave 4: class-aware operand types. Each program type-checks and runs on the interpreter, but
/// the *coarse* pre-Wave-4 compiler rejected it at compile time — its `num_ty` could not see
/// through a field read on an arbitrary instance, a method-call result, a nested `a.b.c`, a
/// class-typed enum payload, or a free function returning an instance. The class-aware `ctype`
/// resolver closes all five. Verified red (interpreter `Ok`, VM `compile error: cannot infer
/// numeric type`) before the fix; both backends agree after it (measured 2026-06-16).
const WAVE4_PROGRAMS: &[&str] = &[
    // (A) field of an arbitrary instance local, used as an arithmetic operand
    r#"import Core.Console;
class Point { constructor(public int x, public int y) {} }
       function main() { Point p = Point(7, 4); Console.println("{p.x + 1}"); }"#,
    // (B) method-call result used arithmetically
    r#"import Core.Console;
class C { constructor(public int x) {} function get() -> int { return this.x; } }
       function main() { C c = C(5); Console.println("{c.get() + 1}"); }"#,
    // (C) nested field read `a.inner.x` — a class-typed field's field
    r#"import Core.Console;
class Inner { constructor(public int x) {} }
       class Outer { constructor(public Inner inner) {} }
       function main() { Outer a = Outer(Inner(10)); Console.println("{a.inner.x + 1}"); }"#,
    // (D) a class-typed enum payload, bound in `match` and read arithmetically
    r#"import Core.Console;
class Point { constructor(public int x) {} }
       enum Opt { Some(Point p), Zero(int z), }
       function f(Opt o) -> int { return match o { Some(p) => p.x + 1, Zero(z) => z, }; }
       function main() { Console.println("{f(Some(Point(41)))}"); Console.println("{f(Zero(0))}"); }"#,
    // (E) a free function returning an instance, then a field of the result, used arithmetically
    r#"import Core.Console;
class Point { constructor(public int x) {} }
       function mk() -> Point { return Point(3); }
       function main() { Console.println("{mk().x + 1}"); }"#,
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
    r#"import Core.Console;
function main() { int x = -9223372036854775807 - 1; Console.println("{-x}"); }"#,
    // integer overflow: i64::MAX + 1
    r#"import Core.Console;
function main() { Console.println("{9223372036854775807 + 1}"); }"#,
    // division by zero
    r#"import Core.Console;
function main() { int z = 0; Console.println("{1 / z}"); }"#,
    // modulo by zero
    r#"import Core.Console;
function main() { int z = 0; Console.println("{1 % z}"); }"#,
    // unbounded recursion: trips the shared `MAX_CALL_DEPTH` guard on both backends.
    // Before Task 0.3 the interpreter recursed on the native stack and SIGABRTed (exit 134)
    // while the VM cleanly reported "stack overflow" — a parity divergence in the fault path.
    r#"import Core.Console;
function rec(int n) -> int { return rec(n) + 1; } function main() { Console.println("{rec(0)}"); }"#,
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
        "import Core.Console; function main() {{ int x = {}1{}; Console.println(\"{{x}}\"); }}",
        "(".repeat(5000),
        ")".repeat(5000),
    );
    agree_err(&parens);
    let unary = format!(
        "import Core.Console; function main() {{ bool b = {}true; Console.println(\"{{b}}\"); }}",
        "!".repeat(5000),
    );
    agree_err(&unary);
    // A long left-associative chain is built *iteratively*, so it escapes the parser's nesting
    // limit but produces a deeply left-leaning AST. The checker's depth guard (the gate both
    // backends share) must fault it identically rather than letting a walker overflow its stack.
    let chain = format!(
        "import Core.Console; function main() {{ int x = 1{}; Console.println(\"{{x}}\"); }}",
        "+1".repeat(20_000),
    );
    agree_err(&chain);
}

#[test]
fn s2_null_and_optional_bind_and_run_on_both_backends() {
    // Task 1 foundation: `null` is a real runtime value and a non-null `T` widens to `T?`.
    // (Observing the null *value* needs the unwrap operators from later S2 tasks.) The exact-output
    // assertion is deliberate: `agree` alone passes vacuously if both backends share a rejection.
    let src = "import Core.Console; function main() { int? x = null; int? y = 5; Console.println(\"optionals ok\"); }";
    assert_eq!(cmd_run(&with_pkg(src)).as_deref(), Ok("optionals ok\n"));
    agree(src); // run ≡ runvm
}

#[test]
fn s2_coalesce_is_byte_identical() {
    // `??`: a null lhs falls through to the default; a present value is kept.
    let src = "import Core.Console; function main() { int? x = null; Console.println(\"{x ?? 7}\"); int? y = 9; Console.println(\"{y ?? 0}\"); }";
    assert_eq!(cmd_run(&with_pkg(src)).as_deref(), Ok("7\n9\n"));
    agree(src);
    // Short-circuit: the default (a printing call) must not run when the lhs is non-null.
    let sc = "import Core.Console; function side() -> int { Console.println(\"SIDE\"); return 0; } function main() { int? y = 9; Console.println(\"{y ?? side()}\"); }";
    assert_eq!(cmd_run(&with_pkg(sc)).as_deref(), Ok("9\n"));
    agree(sc);
}

#[test]
fn s2_safe_access_is_byte_identical() {
    // `?.` short-circuits to null on a null receiver (→ the `?? -1` default) and reads through when
    // the receiver is present. Field read and method call both go through `?.`.
    let cls = "class Box { constructor(private int v) {} function vOf() -> int { return v; } function plus(int n) -> int { return v + n; } }";
    let field = cls.to_string()
        + "import Core.Console;  function main() { Box? a = null; Console.println(\"{(a?.v) ?? -1}\"); Box? b = Box(7); Console.println(\"{(b?.v) ?? -1}\"); }";
    assert_eq!(cmd_run(&with_pkg(&field)).as_deref(), Ok("-1\n7\n"));
    agree(&field);
    let method = cls.to_string()
        + "import Core.Console;  function main() { Box? a = null; Console.println(\"{(a?.vOf()) ?? -1}\"); Box? b = Box(9); Console.println(\"{(b?.vOf()) ?? -1}\"); }";
    assert_eq!(cmd_run(&with_pkg(&method)).as_deref(), Ok("-1\n9\n"));
    agree(&method);
    // short-circuit: a safe call on a null receiver must NOT evaluate its arguments (no "SIDE").
    let sc = cls.to_string()
        + "import Core.Console;  function side() -> int { Console.println(\"SIDE\"); return 0; } function main() { Box? a = null; Console.println(\"{(a?.plus(side())) ?? -1}\"); }";
    assert_eq!(cmd_run(&with_pkg(&sc)).as_deref(), Ok("-1\n"));
    agree(&sc);
}

#[test]
fn s2_if_let_is_byte_identical() {
    // `if (var x = opt)`: the then-branch runs (with `x` bound to the non-null inner) only when the
    // optional is present; otherwise the else-branch runs.
    let present =
        "import Core.Console; function main() { int? o = 5; if (var x = o) { Console.println(\"got {x}\"); } else { Console.println(\"none\"); } }";
    assert_eq!(cmd_run(&with_pkg(present)).as_deref(), Ok("got 5\n"));
    agree(present);
    let absent =
        "import Core.Console; function main() { int? o = null; if (var x = o) { Console.println(\"got {x}\"); } else { Console.println(\"none\"); } }";
    assert_eq!(cmd_run(&with_pkg(absent)).as_deref(), Ok("none\n"));
    agree(absent);
    // The smart-cast inner is a real arithmetic operand: `x + 1` must specialize identically on both
    // backends (guards the run↔runvm operand-type gap — see the cty-tracks-operand-types invariant).
    let arith =
        "import Core.Console; function main() { int? o = 41; if (var x = o) { Console.println(\"{x + 1}\"); } else { Console.println(\"none\"); } }";
    assert_eq!(cmd_run(&with_pkg(arith)).as_deref(), Ok("42\n"));
    agree(arith);
}

#[test]
fn s2_force_unwrap_is_byte_identical() {
    // `opt!` on a present optional yields the inner value, identically on both backends.
    let present = "import Core.Console; function main() { int? o = 5; Console.println(\"{o!}\"); }";
    assert_eq!(cmd_run(&with_pkg(present)).as_deref(), Ok("5\n"));
    agree(present);
    // The unwrapped value is a real arithmetic operand: `o! + 1` must specialize identically
    // (guards the run↔runvm operand-type gap — see the cty-tracks-operand-types invariant).
    let arith =
        "import Core.Console; function main() { int? o = 41; Console.println(\"{o! + 1}\"); }";
    assert_eq!(cmd_run(&with_pkg(arith)).as_deref(), Ok("42\n"));
    agree(arith);
}

#[test]
fn s2_force_unwrap_null_faults_identically() {
    // `opt!` on null is a clean fault with the SAME FaultKind on both backends (no crash, no UB).
    let src = "function main() { int? o = null; int x = o!; }";
    agree_err(src); // FaultKind::ForceUnwrap on both
}

#[test]
fn s2_multiple_null_ops_in_one_expr_are_byte_identical() {
    // Regression: two `??`/`?.`/`!` in one expression. Each stashes its receiver in a scratch slot;
    // that slot is the receiver's frame position (`height-1`), so live transients from an earlier
    // segment must not shift it. The interpreter is the oracle; the VM must match (not fault).
    let two_coalesce =
        "import Core.Console; function main() { int? a = 5; int? b = null; Console.println(\"{a ?? -1} {b ?? -1}\"); }";
    assert_eq!(cmd_run(&with_pkg(two_coalesce)).as_deref(), Ok("5 -1\n"));
    agree(two_coalesce);

    let two_force = "import Core.Console; function main() { int? a = 1; int? b = 2; Console.println(\"{a!} {b!}\"); }";
    assert_eq!(cmd_run(&with_pkg(two_force)).as_deref(), Ok("1 2\n"));
    agree(two_force);

    let cls = "class Box { constructor(private int v) {} function get() -> int { return v; } }";
    let two_safe = cls.to_string()
        + "import Core.Console;  function main() { Box? a = Box(7); Box? b = null; Console.println(\"{(a?.get()) ?? -1} {(b?.get()) ?? -1}\"); }";
    assert_eq!(cmd_run(&with_pkg(&two_safe)).as_deref(), Ok("7 -1\n"));
    agree(&two_safe);

    // Mixed + nested: a coalesce whose default is itself a safe-access-coalesce, beside a force.
    let mixed = cls.to_string()
        + "import Core.Console;  function main() { Box? a = null; int? b = 9; Console.println(\"{(a?.get()) ?? (b ?? 0)} {b!}\"); }";
    assert_eq!(cmd_run(&with_pkg(&mixed)).as_deref(), Ok("9 9\n"));
    agree(&mixed);
}

#[test]
fn s2_match_over_optional_is_byte_identical() {
    // `match opt { null => …, v => … }`: the null arm fires on null, the binding arm narrows `v` to
    // the non-null inner `int` (used here as an arithmetic operand — guards the operand-type gap).
    let src = "import Core.Console; function f(int? o) -> int { return match o { null => -1, v => v + 1 }; } \
               function main() { int? a = null; int? b = 7; Console.println(\"{f(a)}\"); Console.println(\"{f(b)}\"); }";
    assert_eq!(cmd_run(&with_pkg(src)).as_deref(), Ok("-1\n8\n"));
    agree(src);
}

// ── M3 S3: lambdas ─────────────────────────────────────────────────────────────────────────────

#[test]
fn lambdas_agree() {
    // Basic lambda var call
    agree("import Core.Console; function main() { var d = fn(int x) => x*2; Console.println(\"{d(5)}\"); }");
    // Lambda capturing TWO enclosing vars (slot-ordering trigger — invariant #8)
    agree("import Core.Console; function main() { var a=10; var b=100; var f=fn(int x)=>x+a+b; Console.println(\"{f(1)}\"); }");
    // Higher-order user function (lambda passed as argument)
    agree("import Core.Console; function twice(int x,(int)->int f)->int{return f(f(x));} function main(){ Console.println(\"{twice(3, fn(int n)=>n+1)}\"); }");
    // Lambda call inside string interpolation (height-sensitive — F13)
    agree("import Core.Console; function main(){ var inc=fn(int x)=>x+1; Console.println(\"{inc(1)} {inc(2)}\"); }");
    // Lambda call inside a match arm (height-sensitive — F13)
    agree("import Core.Console; enum E{A(),B()} function pick(E e,(int)->int f)->int{ return match e { A()=>f(1), B()=>f(2) }; } function main(){ Console.println(\"{pick(A(), fn(int x)=>x*10)}\"); }");
    // Zero-param lambda
    agree("import Core.Console; function main(){ var greet=fn()=>42; Console.println(\"{greet()}\"); }");
}

#[test]
fn lambda_call_errors_agree() {
    // Arity mismatch: lambda expects 1 arg, called with 2
    agree_err("import Core.Console; function main(){ var f=fn(int x)=>x; Console.println(\"{f(1,2)}\"); }");
}

#[test]
fn statement_body_lambda_agrees() {
    agree("import Core.Console; function main(){ var base=100; var f = fn(int x) -> int { var y = x*2; return y + base; }; Console.println(\"{f(3)}\"); }");
    // 106
}

#[test]
fn statement_body_lambda_needs_return_type() {
    let errs = check_errs("package Main; function main(){ var f = fn(int x) { return x; }; }");
    assert!(
        errs.iter().any(|e| e.message.contains("explicit `-> T`")),
        "{errs:?}"
    );
}

#[test]
fn transpiles_statement_lambda_with_use_clause() {
    let php = transpile_ok("package Main; import Core.Console; function main(){ var base=100; var f = fn(int x) -> int { return x + base; }; Console.println(\"{f(3)}\"); }");
    assert!(
        php.contains("function($x) use ($base)") && php.contains("return $x + $base"),
        "{php}"
    );
}

#[test]
fn pipe_agrees() {
    // `5 |> dbl |> inc` == inc(dbl(5)) == 11 (left-associative)
    agree("import Core.Console; function dbl(int x)->int{return x*2;} function inc(int x)->int{return x+1;} function main(){ Console.println(\"{5 |> dbl |> inc}\"); }");
    // inline lambda on the right: `3 |> fn(int v) => v + 10` == 13
    agree("import Core.Console; function main(){ var add=fn(int a,int b)->int{return a+b;}; Console.println(\"{3 |> fn(int v) => v + 10}\"); }");
    // precedence: `1 + 2 |> dbl` == dbl(1+2) == 6
    agree("import Core.Console; function dbl(int x)->int{return x*2;} function main(){ Console.println(\"{1 + 2 |> dbl}\"); }");
}

#[test]
fn mutation_reassign_agrees() {
    // M-mut.1: mutable locals + reassignment, byte-identical on both backends.
    // Plain reassignment.
    agree("import Core.Console; function main(){ mutable int x = 1; x = 2; Console.println(\"{x}\"); }");
    // Reassign from the variable's own value.
    agree("import Core.Console; function main(){ mutable int x = 1; x = x + 5; Console.println(\"{x}\"); }");
    // `mutable var` (inferred type) reassignment.
    agree("import Core.Console; function main(){ mutable var x = 10; x = x * 3; Console.println(\"{x}\"); }");
    // Two-binding SCALAR case (F13): a scalar copies, so reassigning `b` must not change `a`.
    agree("import Core.Console; function main(){ int a = 10; mutable int b = a; b = 99; Console.println(\"{a} {b}\"); }");
    // Reassignment inside a loop body (accumulator).
    agree("import Core.Console; function main(){ mutable int sum = 0; for (int n in 1..=3) { sum = sum + n; } Console.println(\"{sum}\"); }");
}

#[test]
fn mutation_compound_assign_agrees() {
    // M-mut.2: compound-assign + ++/-- + ??= desugar to `Stmt::Assign`, byte-identical on both.
    // The five op= forms as accumulators.
    agree("import Core.Console; function main(){ mutable int x = 10; x += 5; x -= 3; x *= 2; Console.println(\"{x}\"); }"); // 24
                                                                                                                            // Integer `/=` routes through the intdiv kernel (F7): 24 / 5 = 4 (truncating), NOT float 4.8.
    agree("import Core.Console; function main(){ mutable int x = 24; x /= 5; Console.println(\"{x}\"); }"); // 4
                                                                                                            // `%=` with a NEGATIVE dividend — PHP's sign-follows-dividend (spec §8 #3): -7 % 3 = -1.
    agree("import Core.Console; function main(){ mutable int x = 0 - 7; x %= 3; Console.println(\"{x}\"); }"); // -1
                                                                                                               // `%=` positive dividend, negative divisor: 7 % -3 = 1 (sign follows dividend).
    agree("import Core.Console; function main(){ mutable int x = 7; x %= 0 - 3; Console.println(\"{x}\"); }"); // 1
                                                                                                               // `??=` on an optional: assigns only when null.
    agree("import Core.Console; function main(){ mutable int? a = null; a ??= 7; mutable int? b = 3; b ??= 9; Console.println(\"{a ?? -1} {b ?? -1}\"); }"); // 7 3
                                                                                                                                                             // Statement `++`/`--` counter.
    agree("import Core.Console; function main(){ mutable int n = 0; n++; n++; n++; n--; Console.println(\"{n}\"); }"); // 2
                                                                                                                       // Two-binding SCALAR observe (F13): a compound op on `b` must not touch `a` (value-copy).
    agree("import Core.Console; function main(){ int a = 5; mutable int b = a; b += 100; Console.println(\"{a} {b}\"); }"); // 5 105
                                                                                                                            // Compound-assign inside a loop accumulator.
    agree("import Core.Console; function main(){ mutable int sum = 0; for (int i in 1..=5) { sum += i; } Console.println(\"{sum}\"); }");
    // 15
}

#[test]
fn mutation_element_set_agrees() {
    // M-mut.5: value-type element set xs[i]=e / m[k]=e — byte-identical on both backends.
    // List element set.
    agree("import Core.Console; function main(){ mutable List<int> xs = [1, 2, 3]; xs[1] = 20; Console.println(\"{xs[0]} {xs[1]} {xs[2]}\"); }"); // 1 20 3
                                                                                                                                                  // Compound element set rides the M-mut.2 desugar.
    agree("import Core.Console; function main(){ mutable List<int> xs = [1, 2, 3]; xs[0] += 100; xs[2] *= 5; Console.println(\"{xs[0]} {xs[2]}\"); }"); // 101 15
                                                                                                                                                        // COPY-ON-WRITE value semantics (the P0 catcher, F13): mutating `ys` must not touch `xs`.
    agree("import Core.Console; function main(){ mutable List<int> xs = [1, 2]; mutable List<int> ys = xs; ys[0] = 999; Console.println(\"{xs[0]} {ys[0]}\"); }"); // 1 999
                                                                                                                                                                   // Map update (existing key) + insert (new key), insertion-ordered.
    agree("import Core.Console; function main(){ mutable Map<string, int> m = [\"a\" => 1]; m[\"a\"] = 10; m[\"b\"] = 20; Console.println(\"{m[\"a\"]} {m[\"b\"]}\"); }"); // 10 20
                                                                                                                                                                           // Map COW: a copy is independent.
    agree("import Core.Console; function main(){ mutable Map<string, int> m = [\"a\" => 1]; mutable Map<string, int> n = m; n[\"a\"] = 99; Console.println(\"{m[\"a\"]} {n[\"a\"]}\"); }"); // 1 99
                                                                                                                                                                                            // Set element in a loop (accumulate into a list).
    agree("import Core.Console; function main(){ mutable List<int> xs = [0, 0, 0]; for (mutable int i = 0; i < 3; i++) { xs[i] = i * i; } Console.println(\"{xs[0]} {xs[1]} {xs[2]}\"); }");
    // 0 1 4
}

#[test]
fn mutation_element_set_oob_faults_agree() {
    // M-mut.5: an out-of-range list element SET faults identically on both Rust backends
    // (FaultKind::IndexOob). NOT PHP-gated — PHP would *extend* the array instead (KNOWN_ISSUES).
    agree_err("import Core.Console; function main(){ mutable List<int> xs = [1, 2]; xs[5] = 9; Console.println(\"unreached\"); }");
}

#[test]
fn mutation_instance_field_set_agrees() {
    // M-mut.6: shared-mutable instance field set `o.f = e` — handle semantics, byte-identical on
    // run/runvm + real PHP (`agree` is the 3-way oracle).
    // Basic field set + read-back.
    agree("import Core.Console; class P { constructor(public mutable int x) {} } function main(){ P p = P(1); p.x = 42; Console.println(\"{p.x}\"); }"); // 42
                                                                                                                                                         // HANDLE semantics (the P0 catcher, F13): mutate via one binding, observe via the alias — BOTH
                                                                                                                                                         // see it (the opposite of value-type COW). This is the value/handle slip a 2-binding test catches.
    agree("import Core.Console; class P { constructor(public mutable int x) {} } function main(){ P p = P(1); P q = p; p.x = 99; Console.println(\"{p.x} {q.x}\"); }"); // 99 99
                                                                                                                                                                        // `this.f = e` inside a method, visible through the original binding across calls.
    agree("import Core.Console; class C { constructor(public mutable int n) {} function bump() -> int { this.n = this.n + 1; return this.n; } } function main(){ C c = C(10); c.bump(); c.bump(); Console.println(\"{c.n}\"); }"); // 12
                                                                                                                                                                                                                                   // A declared (non-promoted) `mutable` field initialized in the ctor body via `this.f = e`.
    agree("import Core.Console; class B { mutable int v; constructor(int seed) { this.v = seed * 2; } function get() -> int { return this.v; } } function main(){ B b = B(5); b.v = b.v + 1; Console.println(\"{b.get()}\"); }"); // 11
                                                                                                                                                                                                                                  // Field set on an instance reached through another field (`a.b.c = e`) — handle semantics all the way.
    agree("import Core.Console; class Inner { constructor(public mutable int v) {} } class Outer { constructor(public Inner inner) {} } function main(){ Outer o = Outer(Inner(1)); o.inner.v = 7; Console.println(\"{o.inner.v}\"); }");
    // 7
}

#[test]
fn mutation_static_field_agrees() {
    // M-mut.7: program-lifetime `static mutable` class fields, read/written as `ClassName.field` —
    // byte-identical run/runvm + real PHP. A static is shared across all instances (one program-level
    // slot), so a counter incremented in the constructor accumulates across constructions.
    agree("import Core.Console; class Counter { static mutable int total = 0; constructor() { Counter.total = Counter.total + 1; } } function main(){ Counter(); Counter(); Counter(); Console.println(\"{Counter.total}\"); }"); // 3
                                                                                                                                                                                                                                  // Direct read/write from a free function; an immutable static string too.
    agree("import Core.Console; class Cfg { static mutable int n = 10; static string name = \"cfg\"; } function main(){ Cfg.n = Cfg.n + 5; Console.println(\"{Cfg.name}={Cfg.n}\"); }"); // cfg=15
                                                                                                                                                                                         // A static read used as an arithmetic operand inside a method (the CTy-operand path).
    agree("import Core.Console; class C { static mutable int k = 1; function step() -> int { C.k = C.k * 2; return C.k + 1; } } function main(){ C c = C(); Console.println(\"{c.step()} {c.step()}\"); }");
    // 3 5
}

#[test]
fn mutation_property_hooks_agrees() {
    // M-mut.7b: property hooks `T name { get => …; set(T v) { … } }` — a get computes on read, a
    // set intercepts a write (typically mutating a backing `mutable` field). Byte-identical on
    // run/runvm + real PHP (the synthetic-method VM lowering ≡ the PHP 8.4 property hook).
    // A read-only computed hook reads a backing field.
    agree("import Core.Console; class C { constructor(public mutable int raw) {} int doubled { get => this.raw * 2; } } function main(){ C c = C(21); Console.println(\"{c.doubled}\"); }"); // 42
                                                                                                                                                                                             // A get used as an arithmetic operand — the CTy-operand path (`o.hook + 1` must specialize on the VM).
    agree("import Core.Console; class C { constructor(public mutable int raw) {} int doubled { get => this.raw * 2; } } function main(){ C c = C(21); Console.println(\"{c.doubled + 1}\"); }"); // 43
                                                                                                                                                                                                 // A set writes a backing field; observe through both the hook (get) and the raw field.
    agree("import Core.Console; class C { constructor(public mutable int raw) {} int half { get => this.raw; set(int v) { this.raw = v / 2; } } } function main(){ C c = C(0); c.half = 10; Console.println(\"{c.raw} {c.half}\"); }"); // 5 5
                                                                                                                                                                                                                                        // HANDLE semantics through a hook: set via one binding, observe via the alias.
    agree("import Core.Console; class C { constructor(public mutable int raw) {} int v { get => this.raw; set(int n) { this.raw = n; } } } function main(){ C c = C(1); C d = c; c.v = 99; Console.println(\"{d.v}\"); }"); // 99
                                                                                                                                                                                                                            // A float computed property with exactly-representable values (Celsius↔Fahrenheit round-trip).
    agree("import Core.Console; class Temp { constructor(public mutable float celsius) {} float fahrenheit { get => this.celsius * 9.0 / 5.0 + 32.0; set(float f) { this.celsius = (f - 32.0) * 5.0 / 9.0; } } } function main(){ Temp t = Temp(100.0); Console.println(\"{t.fahrenheit}\"); t.fahrenheit = 32.0; Console.println(\"{t.celsius}\"); }");
    // 212 then 0
}

#[test]
fn mutation_clone_with_agrees() {
    // M-mut.4a: `obj with { f = e }` — fresh instance, source unchanged, byte-identical on both.
    agree("import Core.Console; class P { constructor(public int x, public int y) {} } function main(){ P p = P(1, 2); P q = p with { x = 9 }; Console.println(\"{p.x} {p.y} {q.x} {q.y}\"); }"); // 1 2 9 2
    agree("import Core.Console; class P { constructor(public int x, public int y) {} } function main(){ P p = P(1, 2); P q = p with { x = 7, y = 8 }; Console.println(\"{q.x} {q.y}\"); }"); // 7 8
                                                                                                                                                                                             // A method works on the cloned instance (the clone is a real instance; the ctor was not re-run).
    agree("import Core.Console; class P { constructor(public int x, public int y) {} function sum() -> int { return this.x + this.y; } } function main(){ P p = P(1, 2); P q = p with { x = 10 }; Console.println(\"{q.sum()}\"); }"); // 12
                                                                                                                                                                                                                                       // The override value may reference the source's own fields.
    agree("import Core.Console; class P { constructor(public int x, public int y) {} } function main(){ P p = P(3, 4); P q = p with { x = p.x + p.y }; Console.println(\"{q.x} {q.y}\"); }");
    // 7 4
}

#[test]
fn mutation_condition_loops_agree() {
    // M-mut.3: while / do-while / C-for / while-let / break / continue, byte-identical on both.
    // Plain while accumulator.
    agree("import Core.Console; function main(){ mutable int i = 0; mutable int s = 0; while (i < 4) { s += i; i += 1; } Console.println(\"{s}\"); }"); // 6
                                                                                                                                                        // do-while runs the body once even when the condition is false up front.
    agree("import Core.Console; function main(){ mutable int n = 10; do { Console.println(\"once\"); n += 1; } while (n < 5); }");
    // continue skips, break stops.
    agree("import Core.Console; function main(){ mutable int i = 0; mutable int hit = 0; while (true) { i += 1; if (i == 2) { continue; } if (i >= 5) { break; } hit += 1; } Console.println(\"{hit}\"); }"); // i=1,3,4 → 3
                                                                                                                                                                                                              // C-style for with continue + break.
    agree("import Core.Console; function main(){ mutable int sum = 0; for (mutable int k = 0; k < 6; k++) { if (k == 1) { continue; } if (k == 5) { break; } sum += k; } Console.println(\"{sum}\"); }"); // 0+2+3+4=9
                                                                                                                                                                                                          // Nested C-for: an inner break exits only the inner loop.
    agree("import Core.Console; function main(){ mutable int t = 0; for (mutable int a = 0; a < 3; a += 1) { for (mutable int b = 0; b < 9; b += 1) { if (b == 2) { break; } t += 1; } } Console.println(\"{t}\"); }"); // 3*2=6
                                                                                                                                                                                                                        // while-let drains an optional.
    agree("import Core.Console; function main(){ mutable int? o = 7; while (var v = o) { Console.println(\"{v}\"); o = null; } Console.println(\"done\"); }");
    // break inside a for-in (the existing range loop) exits it.
    agree("import Core.Console; function main(){ mutable int last = 0; for (int x in 1..=10) { if (x == 4) { break; } last = x; } Console.println(\"{last}\"); }"); // 3
                                                                                                                                                                    // continue inside a for-in skips one iteration.
    agree("import Core.Console; function main(){ mutable int s = 0; for (int x in 1..=5) { if (x == 3) { continue; } s += x; } Console.println(\"{s}\"); }"); // 1+2+4+5=12
                                                                                                                                                              // for(;;) terminated by break.
    agree("import Core.Console; function main(){ mutable int c = 0; for (;;) { c += 1; if (c == 3) { break; } } Console.println(\"{c}\"); }");
    // 3
}

#[test]
fn named_fn_ref_as_value_agrees() {
    // named fn defined BEFORE use
    agree("import Core.Console; function dbl(int x)->int{return x*2;} function twice(int x,(int)->int f)->int{return f(f(x));} function main(){ Console.println(\"{twice(2, dbl)}\"); }"); // 8
                                                                                                                                                                                           // named fn defined AFTER use (forward reference)
    agree("import Core.Console; function apply(int x,(int)->int f)->int{return f(x);} function callsLater(int n)->int{ return apply(n, bump); } function bump(int x)->int{return x+5;} function main(){ Console.println(\"{callsLater(10)}\"); }");
    // 15
    // A bare named function bound to a `var`, then called THROUGH the local. The compiler infers
    // the local's `CTy::Fn` from the named-fn reference so the call dispatches via `CallValue`
    // (without the inference the VM rejected `f(5)` as "not a function").
    agree("import Core.Console; function dbl(int x)->int{return x*2;} function main(){ var f=dbl; Console.println(\"{f(5)}\"); }");
    // 10
}

#[test]
fn transpiles_lambda_literal_call_target() {
    let php = transpile_ok("package Main; import Core.Console; function main(){ Console.println(\"{3 |> fn(int v) => v + 100}\"); }");
    assert!(php.contains("(fn($v) => $v + 100)(3)"), "{php}");
}

#[test]
fn higher_order_natives_agree() {
    // map / filter / reduce with inline lambdas (results shown via List.sum — PHP can't echo arrays).
    agree("import Core.Console; import Core.List; function main(){ var d=List.map([1,2,3], fn(int x)=>x*2); Console.println(\"{List.sum(d)}\"); }"); // 12
    agree("import Core.Console; import Core.List; function main(){ var e=List.filter([1,2,3,4], fn(int x)=>x%2==0); Console.println(\"{List.sum(e)}\"); }"); // 6
    agree("import Core.Console; import Core.List; function main(){ Console.println(\"{List.reduce([1,2,3,4], 1, fn(int a,int x)=>a*x)}\"); }"); // 24
                                                                                                                                                // A lambda capturing an enclosing local, passed to a native (capture window parity, invariant #8).
    agree("import Core.Console; import Core.List; function main(){ var k=10; var s=List.map([1,2,3], fn(int x)=>x*k); Console.println(\"{List.sum(s)}\"); }"); // 60
                                                                                                                                                               // A bare NAMED function reference (zero-capture closure) passed straight to a native.
    agree("import Core.Console; import Core.List; function dbl(int x)->int{return x*2;} function main(){ var d=List.map([1,2,3], dbl); Console.println(\"{List.sum(d)}\"); }"); // 12
                                                                                                                                                                                // RE-ENTRANCY: a native called from inside another native's closure (map nested in reduce's fn).
    agree("import Core.Console; import Core.List; function main(){ Console.println(\"{List.reduce([1,2,3], 0, fn(int a,int x)=>a + List.sum(List.map([x], fn(int y)=>y*y)))}\"); }");
    // 14
}

#[test]
fn higher_order_native_closure_fault_agrees() {
    // A fault raised *inside* a closure run by a native must propagate byte-identically on both
    // backends (interpreter `call_closure` ⇄ VM re-entrant `call_closure_value`). Can't be a runnable
    // example (every example must produce identical Ok output) — lives here as a fault-parity case.
    agree_err("import Core.Console; import Core.List; function main(){ var d=List.map([1,2,3], fn(int x)=>x/0); Console.println(\"{List.sum(d)}\"); }");
    // DivZero on both
}

#[test]
fn transpiles_higher_order_natives() {
    let php = transpile_ok("package Main; import Core.Console; import Core.List; function main(){ var d=List.map([1,2,3], fn(int x)=>x*2); var e=List.filter(d, fn(int x)=>x>2); Console.println(\"{List.reduce(e, 0, fn(int a,int x)=>a+x)}\"); }");
    assert!(php.contains("array_map(fn($x) => $x * 2,"), "{php}");
    assert!(php.contains("array_values(array_filter("), "{php}");
    assert!(php.contains("array_reduce("), "{php}");
}

#[test]
fn generic_methods_agree() {
    // A generic method (`<T>` on a method of a non-generic class) inferred from arguments must run
    // byte-identically on both backends — the type variable is erased before either backend, like a
    // generic free function (M-RT generics-all). `identity` reused at three concrete types.
    agree("import Core.Console; class U { function id<T>(T x)->T { return x; } } function main(){ var u=U(); Console.println(\"{u.id(7)} {u.id(\\\"hi\\\")} {u.id(true)}\"); }"); // 7 hi true
                                                                                                                                                                                  // `T` inferred from a `List<T>` argument; the fallback shares it.
    agree("import Core.Console; class U { function firstOr<T>(List<T> xs, T d)->T { for (T x in xs) { return x; } return d; } } function main(){ var u=U(); Console.println(\"{u.firstOr([10,20], -1)} {u.firstOr([], 99)}\"); }"); // 10 99
                                                                                                                                                                                                                                    // A type parameter inside a function-typed parameter, and the closure invoked in the method body
                                                                                                                                                                                                                                    // (exercises the VM's re-entrant closure path from inside a generic method).
    agree("import Core.Console; class U { function applyTwice<T>(T x, (T)->T f)->T { return f(f(x)); } } function main(){ var u=U(); Console.println(\"{u.applyTwice(5, fn(int v)=>v+1)}\"); }");
    // 7
}

#[test]
fn overloaded_free_functions_agree() {
    // M-RT overloading (dynamic multiple dispatch): the runtime argument types select the overload,
    // identically on both backends. Primitive overloads (disjoint by construction).
    agree("import Core.Console; \
           function d(int n)->string { return \"int:{n}\"; } \
           function d(string s)->string { return \"str:{s}\"; } \
           function d(bool b)->string { return \"bool:{b}\"; } \
           function main(){ Console.println(d(42)); Console.println(d(\"hi\")); Console.println(d(true)); }");
    // Arity overloads.
    agree(
        "import Core.Console; \
           function add(int a)->int { return a; } \
           function add(int a, int b)->int { return a+b; } \
           function main(){ Console.println(\"{add(5)} {add(5,6)}\"); }",
    );
    // Class + interface overloads with most-specific dispatch: a `Circle` value picks `area(Circle)`,
    // a `Square` (only a `Shape`) picks the `area(Shape)` fallback — same choice on both backends.
    agree("import Core.Console; \
           interface Shape {} \
           class Circle implements Shape { constructor(public int r) {} } \
           class Square implements Shape { constructor(public int s) {} } \
           function area(Circle c)->int { return c.r*c.r*3; } \
           function area(Shape s)->int { return 0; } \
           function main(){ Circle c=Circle(2); Square q=Square(4); Console.println(\"{area(c)} {area(q)}\"); }");
}

#[test]
fn overloaded_methods_agree() {
    // M-RT overloading on class methods: the receiver's runtime argument types select the overload,
    // identically on both backends (the `this` receiver is excluded from the dispatch).
    agree("import Core.Console; \
           interface Shape {} \
           class Circle implements Shape { constructor(public int r) {} } \
           class Printer { \
             constructor(public string tag) {} \
             function show(int n)->string { return \"{this.tag}/int:{n}\"; } \
             function show(string s)->string { return \"{this.tag}/str:{s}\"; } \
             function show(Circle c)->string { return \"{this.tag}/circle:{c.r}\"; } \
           } \
           function main(){ Printer p=Printer(\"P\"); \
             Console.println(p.show(7)); Console.println(p.show(\"hi\")); Console.println(p.show(Circle(3))); }");
}

#[test]
fn ambiguous_overloaded_call_faults_on_both_backends() {
    // A multi-argument cross-cutting overload set with no unique most-specific match for the call is
    // a clean runtime fault — and the SAME fault on both backends (byte-identical message → same
    // classification). `Both` satisfies A and B, so `pick(Both, Both)` matches both overloads and
    // neither dominates.
    agree_err(
        "import Core.Console; \
               interface A {} interface B {} \
               class Both implements A, B { constructor(public int v) {} } \
               function pick(A x, B y)->int { return 1; } \
               function pick(B x, A y)->int { return 2; } \
               function main(){ Both b=Both(0); Console.println(\"{pick(b, b)}\"); }",
    );
}

#[test]
fn transpiles_generic_method_to_mixed() {
    // A generic method erases to `mixed`-typed PHP (params and return), exactly as a generic free
    // function does; `List<T>` → `array`, `(T)->T` → `\Closure`. No type variable reaches the output.
    let php = transpile_ok("package Main; class U { function id<T>(T x)->T { return x; } function applyTwice<T>(T x, (T)->T f)->T { return f(f(x)); } } function main(){ var u=U(); var n = u.id(1); var m = u.applyTwice(2, fn(int v)=>v+1); }");
    assert!(php.contains("function id(mixed $x): mixed"), "{php}");
    assert!(
        php.contains("function applyTwice(mixed $x, \\Closure $f): mixed"),
        "{php}"
    );
}

#[test]
fn escaping_and_nested_lambdas_agree() {
    // A closure that ESCAPES its defining frame: a named function returns a lambda capturing its
    // param, then it is called after the function has returned. Captures live in the closure's Rc,
    // so both backends must agree. (Guards the trailing-lambda-block layout: a lambda defined in a
    // function *before* `main` must not shift `main`'s entry index.)
    agree("import Core.Console; function mk(int a)->(int)->int{ return fn(int b)=>a+b; } function main(){ var f=mk(10); Console.println(\"{f(5)}\"); }"); // 15
                                                                                                                                                          // Escaping closure capturing a `var` local of the enclosing function (not a param).
    agree("import Core.Console; function mk(int z)->(int)->int{ var a=z*2; return fn(int b)=>a+b; } function main(){ var f=mk(10); Console.println(\"{f(5)}\"); }"); // 25
                                                                                                                                                                     // Lexically NESTED lambda: a lambda whose body defines and returns another capturing lambda.
    agree("import Core.Console; function mk(int a)->(int)->int{ var outer=fn(int b)->(int)->int{ return fn(int c)=>a+b+c; }; return outer(a); } function main(){ var f=mk(100); Console.println(\"{f(11)}\"); }"); // 100+100+11 = 211
                                                                                                                                                                                                                   // Two functions defined before `main`, the first bearing a lambda — exercises the entry-index
                                                                                                                                                                                                                   // and Op::Call stability under the trailing-lambda block (a regression would call the wrong fn).
    agree("import Core.Console; function a(int x)->int{ var inc=fn(int n)=>n+1; return inc(x); } function b(int x)->int{ return x*10; } function main(){ Console.println(\"{a(4)} {b(4)}\"); }");
    // 5 40
    // A lambda inside a METHOD body (capturing a method param) — the constructor/method compile
    // loops number their lambdas from the same trailing block, so this guards that path too.
    agree("import Core.Console; class Box { constructor(public int v) {} function scaledBy(int k)->int{ var f=fn(int x)->int{ return x*k; }; return f(this.v); } } function main(){ var b=Box(7); Console.println(\"{b.scaledBy(3)}\"); }");
    // 21
}

#[test]
fn html_literal_sugar_agrees() {
    // Core.Html Wave 3 — `html"…"` desugars to html.raw/html.text/html.concat, all of which are
    // already byte-identical across backends, so the sugar inherits parity. (run ≡ runvm here; the
    // glob test below adds run ≡ php on examples/guide/html.phg.)
    // A string hole auto-escapes; literal chunks pass through.
    agree(
        r#"import Core.Console; import Core.Html; function main(){ var n="a&<b>"; Console.println(Html.render(html"<h1>{n}</h1>")); }"#,
    ); // <h1>a&amp;&lt;b&gt;</h1>
       // A primitive hole stringifies then escapes.
    agree(
        r#"import Core.Console; import Core.Html; function main(){ var n=42; Console.println(Html.render(html"<p>{n}</p>")); }"#,
    ); // <p>42</p>
       // An Html hole embeds verbatim (no double-escape).
    agree(
        r#"import Core.Console; import Core.Html; function main(){ var inner=Html.text("a&b"); Console.println(Html.render(html"<div>{inner}</div>")); }"#,
    ); // <div>a&amp;b</div>
       // A nested html"…" as an Html hole — recursion through resolve_html.
    agree(
        r#"import Core.Console; import Core.Html; function main(){ var n="x"; var inner=html"<b>{n}</b>"; Console.println(Html.render(html"<p>{inner}</p>")); }"#,
    ); // <p><b>x</b></p>
       // Multi-line literal (spans lines for free, like a plain string).
    agree("import Core.Console; import Core.Html; function main(){ var n=\"z\"; Console.println(Html.render(html\"<ul>\n  <li>{n}</li>\n</ul>\")); }");
    // A literal with no holes is still Html.
    agree(
        r#"import Core.Console; import Core.Html; function main(){ Console.println(Html.render(html"<hr/>")); }"#,
    ); // <hr/>
}

#[test]
fn html_literal_bad_hole_rejected_by_both() {
    // A non-renderable hole type (an enum value) is `E-HTML-HOLE` — rejected on both backends.
    agree_err(
        r#"import Core.Html; enum E { A() } function main(){ var p = html"<h1>{A()}</h1>"; }"#,
    );
    // `html"…"` without `import Core.Html;` is `E-HTML-IMPORT` — rejected on both backends.
    agree_err(r#"function main(){ var p = html"<h1>x</h1>"; }"#);
}

#[test]
fn transpiles_html_literal_to_kernel_calls() {
    // The desugaring targets only Wave-1/2 natives, so the PHP is the kernel emission: literal
    // chunks as strings, a string hole through htmlspecialchars(ENT_QUOTES), all joined by implode.
    let php = transpile_ok(
        r#"package Main; import Core.Console; import Core.Html; function main(){ var n="x"; Console.println(Html.render(html"<h1>{n}</h1>")); }"#,
    );
    assert!(php.contains("implode('', ["), "{php}");
    assert!(
        php.contains("htmlspecialchars($n, ENT_QUOTES, 'UTF-8')"),
        "{php}"
    );
}

#[test]
fn named_tag_helpers_agree() {
    // Core.Html Option 1 — `html.<tag>(attrs, children)` bakes the tag, byte-identical to el/void_el,
    // so it inherits parity. (run ≡ runvm here; the glob test adds run ≡ php on the guide example.)
    // Content element: attribute value escaped, text child escaped.
    agree(
        r#"import Core.Console; import Core.Html; function main(){ Console.println(Html.render(Html.a([Html.attr("href","/?x=1&y=2")],[Html.text("A & B")]))); }"#,
    ); // <a href="/?x=1&amp;y=2">A &amp; B</a>
       // Empty attr list accepted in call-arg position; tags nest.
    agree(
        r#"import Core.Console; import Core.Html; function main(){ Console.println(Html.render(Html.ul([],[Html.li([],[Html.text("x")])]))); }"#,
    ); // <ul><li>x</li></ul>
       // A void (self-closing) element.
    agree(
        r#"import Core.Console; import Core.Html; function main(){ Console.println(Html.render(Html.hr([]))); }"#,
    ); // <hr/>
       // A tag helper and the equivalent el() call produce identical bytes.
    agree(
        r#"import Core.Console; import Core.Html; function main(){ Console.println(Html.render(Html.p([],[Html.text("hi")]))); Console.println(Html.render(Html.el("p",[],[Html.text("hi")]))); }"#,
    ); // <p>hi</p>\n<p>hi</p>
}

#[test]
fn transpiles_named_tag_to_baked_php() {
    // A named tag erases to the same baked closure the kernel uses, with the tag compiled in (no $t).
    let php = transpile_ok(
        r#"package Main; import Core.Console; import Core.Html; function main(){ Console.println(Html.render(Html.div([],[Html.text("x")]))); }"#,
    );
    assert!(php.contains("'<div'"), "{php}");
    assert!(php.contains("'</div>'"), "{php}");
}

// ── M7: the PHP oracle — the third correctness leg ───────────────────────────────────────────────
// `run ≡ runvm` is gated by every test above. This gates `run ≡ php` (⇒ all three byte-identical):
// the transpiled PHP, executed by a real `php`, must print exactly what the interpreter prints.
// Gating contract (closes P0-ROOT — no more self-skip-to-PASS):
//   PHORGE_REQUIRE_PHP=1 → a missing php FAILS the test (CI / enforced mode).
//   unset/empty          → a missing php skips LOUDLY (dev convenience), never a silent green.
// Optional PHORGE_PHP=<path> overrides the php binary (non-PATH installs).
// Scope: stdout-parity over runnable (`Ok`) examples + projects. Fault classes (overflow, OOB,
// range-too-large) stay `run ≡ runvm` `agree_err` above — they are not runnable examples.

/// Resolve the php binary: `PHORGE_PHP` override, else `php` on PATH if `--version` succeeds.
fn php_bin() -> Option<String> {
    let cand = std::env::var("PHORGE_PHP").unwrap_or_else(|_| "php".to_string());
    let ok = Command::new(&cand)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    ok.then_some(cand)
}

/// The fails-not-skips gate. `Some(php)` ⇒ run; `None` ⇒ caller returns (loud skip). Under
/// `PHORGE_REQUIRE_PHP=1` a missing php panics instead of skipping.
fn php_or_gate(test: &str) -> Option<String> {
    if let Some(p) = php_bin() {
        return Some(p);
    }
    assert!(
        std::env::var("PHORGE_REQUIRE_PHP").as_deref() != Ok("1"),
        "{test}: php required (PHORGE_REQUIRE_PHP=1) but not found on PATH or $PHORGE_PHP"
    );
    eprintln!("SKIP {test}: php not found — set PHORGE_REQUIRE_PHP=1 to make this a failure");
    None
}

/// Write `php_src` to a per-label temp file (no collision under parallel `cargo test`), run it with
/// `php -n` (ignore php.ini → hermetic; notices go to stderr, we read stdout), return its stdout.
fn run_php(php: &str, php_src: &str, label: &str) -> String {
    let safe: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = std::env::temp_dir().join(format!("phorge_oracle_{safe}.php"));
    std::fs::write(&path, php_src).expect("write temp php");
    let out = Command::new(php)
        .arg("-n")
        .arg(&path)
        .output()
        .expect("spawn php");
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "php exited non-zero for {label}:\n{}\n--- transpiled php ---\n{php_src}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 php stdout")
}

/// M-RT S6b.4 — the `rename` resolution clause lowers to PHP `T::m insteadof …; T::m as n;`. The
/// guide example exercises `use`; this gates the `rename` path (the trickier emission — `as` alone
/// does not remove the original method, so an `insteadof` for the remaining winner is also required)
/// end-to-end through real PHP, asserting the transpiled output equals the interpreter's.
#[test]
fn s6b_rename_decomposition_matches_php() {
    let Some(php) = php_or_gate("s6b_rename_decomposition_matches_php") else {
        return;
    };
    let src = with_pkg(
        r#"import Core.Console;
open class Swimmer { open function move() -> string { return "swims"; } }
open class Flyer { open function move() -> string { return "flies"; } }
class Duck extends Swimmer, Flyer {
    rename Flyer.move as glide
}
function main() {
    Duck d = Duck();
    Console.println(d.move());
    Console.println(d.glide());
}"#,
    );
    let expected = cmd_run(&src).expect("interpreter runs");
    let php_src = cli::cmd_transpile(&src).expect("transpiles");
    assert!(
        php_src.contains("insteadof") && php_src.contains("as glide"),
        "expected insteadof + as in:\n{php_src}"
    );
    assert_eq!(
        run_php(&php, &php_src, "s6b_rename"),
        expected,
        "PHP ≠ interpreter for the rename decomposition"
    );
}

/// Every runnable single-file example: transpiled PHP run by `php` prints exactly what `cmd_run`
/// (the interpreter) prints. Globbed like `all_examples_match_between_backends`, so a new example is
/// auto-gated. A non-`Ok` example is skipped here (it's gated by the run≡runvm glob); the oracle is
/// stdout-parity on success only.
#[test]
fn all_examples_transpile_and_match_php() {
    let Some(php) = php_or_gate("all_examples_transpile_and_match_php") else {
        return;
    };
    let mut files = Vec::new();
    collect_phg(std::path::Path::new("examples"), &mut files);
    files.sort();
    assert!(files.len() >= 3, "expected examples, found {}", files.len());
    let mut deferred = 0usize;
    for path in &files {
        let label = path.display().to_string();
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {label}: {e}"));
        let expected = match cmd_run(&src) {
            Ok(o) => o,
            Err(_) => continue, // non-runnable example — gated by the run≡runvm glob, not here
        };
        let php_src = match cli::cmd_transpile(&src) {
            Ok(php) => php,
            // A transpiler feature the backend explicitly defers (e.g. literal `match` patterns,
            // expr-position `match`, `is` — all scheduled for M11). This is NOT a silent skip: it's
            // logged + counted, and a genuine transpile regression (any other error) still panics.
            // As M11 implements each construct, the deferral error disappears and the example
            // auto-enrolls in the oracle with no test edit.
            Err(e) if e.contains("not yet supported") => {
                eprintln!("DEFER {label}: {e} (M11 transpiler gap — oracle-skipped)");
                deferred += 1;
                continue;
            }
            Err(e) => panic!("transpile {label}: {e}"),
        };
        let got = run_php(&php, &php_src, &label);
        assert_eq!(got, expected, "PHP ≠ interpreter for example {label}");
    }
    eprintln!(
        "php oracle: {} examples gated, {deferred} deferred to M11 (transpiler feature gaps)",
        files.len() - deferred
    );
}

/// Every multi-file example project: the namespaced transpile (`namespace …{}` + `\Main\main()`
/// bootstrap — a distinct emit path from the flat single-file one) run by `php` must match the
/// interpreter's output. Assembled through `loader::load`, mirroring `all_example_projects_match…`.
#[test]
fn all_example_projects_transpile_and_match_php() {
    let Some(php) = php_or_gate("all_example_projects_transpile_and_match_php") else {
        return;
    };
    let mut projects = Vec::new();
    collect_projects(std::path::Path::new("examples"), &mut projects);
    projects.sort();
    assert!(
        !projects.is_empty(),
        "expected an example project, found none"
    );
    for project in &projects {
        let entry = find_main_phg(project);
        let label = project.display().to_string();
        let unit = loader::load(&entry).unwrap_or_else(|e| panic!("load {label}: {e}"));
        let expected = cli::run_program(&unit).unwrap_or_else(|e| panic!("run {label}: {e}"));
        let php_src = cli::transpile_program(&unit.program, &unit.diag_src)
            .unwrap_or_else(|e| panic!("transpile {label}: {e}"));
        let got = run_php(&php, &php_src, &label);
        assert_eq!(got, expected, "PHP ≠ interpreter for project {label}");
    }
}

// ── M7: divergence-class regression guards ───────────────────────────────────────────────────────

/// P0-1/P0-2/P0-4/QW-13: the emitter routes `/`, `%`, interpolation, compound operands, and ranges
/// through the correctness machinery. This pins the *emitted PHP shape* directly (the oracle above
/// pins the *runtime behavior* over the examples); together they make a P0 regression impossible to
/// ship silently. `run ≡ runvm` for these was always correct — the bug class was php-leg-only.
#[test]
fn m7_emitter_uses_correctness_helpers() {
    // P0-1 (int `/` ⇒ intdiv) + P0-4 (`%` ⇒ type-driven) route through runtime helpers, not bare ops.
    let div = transpile_ok(
        "package Main; import Core.Console; function main(){ Console.println(\"{7 / 2}\"); Console.println(\"{5 % 2}\"); }",
    );
    assert!(div.contains("__phorge_div(7, 2)"), "{div}");
    assert!(div.contains("__phorge_rem(5, 2)"), "{div}");
    assert!(
        div.contains("function __phorge_div") && div.contains("intdiv"),
        "{div}"
    );
    assert!(
        div.contains("function __phorge_rem") && div.contains("fmod"),
        "{div}"
    );
    // P0-3: an interpolated value is coerced via __phorge_str (bool ⇒ "true"/"false").
    let b = transpile_ok(
        "package Main; import Core.Console; function main(){ Console.println(\"{1 < 2}\"); }",
    );
    assert!(
        b.contains("__phorge_str(") && b.contains("\"true\" : \"false\""),
        "{b}"
    );
    // P0-2: a compound operand keeps its grouping parens (no PHP re-association).
    let p = transpile_ok(
        "package Main; import Core.Console; function main(){ int a=1; int b=2; int c=3; Console.println(\"{a - (b - c)}\"); Console.println(\"{!(a < b)}\"); }",
    );
    assert!(p.contains("$a - ($b - $c)"), "{p}");
    assert!(p.contains("!($a < $b)"), "{p}");
    // QW-13: ranges route through the empty/reversed-safe helper (PHP range() descends; Phorge ⇒ []).
    let r = transpile_ok(
        "package Main; import Core.Console; function main(){ for (int i in 5..2) { Console.println(\"{i}\"); } }",
    );
    assert!(r.contains("__phorge_range(5, 2, false)"), "{r}");
}

/// P0-1: integer division truncates toward zero on both backends, with negative operands. (The php
/// leg is gated by the oracle over the division-bearing examples.)
#[test]
fn m7_int_division_truncates_toward_zero() {
    let src = "import Core.Console; function main(){ Console.println(\"{7 / 2} {-7 / 2} {7 / -2} {-7 / -2}\"); }";
    assert_eq!(cmd_run(&with_pkg(src)).as_deref(), Ok("3 -3 -3 3\n"));
    agree(src);
}

/// P1-#9: a range too wide to materialize faults cleanly on BOTH backends (`RangeTooLarge`) instead
/// of OOM-aborting (exit 101). Exclusive and inclusive forms both guard; the cap check precedes any
/// allocation, so the test is fast.
#[test]
fn m7_large_range_faults_identically() {
    agree_err(
        "import Core.Console; function main(){ for (int i in 0..2000000000) { Console.println(\"{i}\"); } }",
    );
    agree_err("import Core.Console; function main(){ var xs = 0..=2000000000; Console.println(\"{xs[0]}\"); }");
    // The exactly-at-cap boundary is also a fault (span >= MAX_RANGE_LEN), while a small range is fine.
    agree(
        "import Core.Console; function main(){ var xs = 0..1000; Console.println(\"{xs[999]}\"); }",
    );
}

/// Divergence-class edge: `i64::MIN / -1` overflows i64 — both backends fault (via the checked
/// `int_div` kernel) rather than panicking (EV-7). PHP's `intdiv(PHP_INT_MIN, -1)` likewise throws,
/// so the helper matches; it's a fault case, not a runnable example, so it lives here, not the oracle.
#[test]
fn m7_int_min_div_neg_one_faults_identically() {
    agree_err(
        "import Core.Console; function main(){ int x = -9223372036854775807 - 1; Console.println(\"{x / -1}\"); }",
    );
}

/// M-faults 2a: the fault intrinsics crash byte-identically on both backends (single-sourced
/// `FaultMsg` body → same `FaultKind::Panic`). `assert(true)` is a no-op, so the program completes.
#[test]
fn faults_panic_intrinsics_agree() {
    agree_err(r#"function main(){ panic("boom"); }"#);
    agree_err("function main(){ todo(); }");
    agree_err("function main(){ unreachable(); }");
    agree_err(r#"function main(){ assert(2 < 1, "nope"); }"#);
    agree_err("function main(){ assert(false); }");
    agree(
        r#"import Core.Console; function main(){ assert(1 < 2, "ok"); Console.println("done"); }"#,
    );
}

/// M-faults 2a: a `never`-typed `panic` at the tail of a value-returning function satisfies
/// return-on-all-paths (the totality engine treats it as diverging), and faults identically.
#[test]
fn never_intrinsic_satisfies_return_totality() {
    agree_err(
        r#"function bad() -> int { panic("never returns"); } function main(){ var x = bad(); }"#,
    );
}

/// M-faults 2b.2: the built-in `Error` marker interface is reserved (user code can't redefine it)
/// and implementable (a class may `implements Error`; `instanceof Error` works).
#[test]
fn error_base_type_reserved_and_implementable() {
    assert!(
        !check_errs("class Error {}").is_empty(),
        "`class Error` must be rejected (reserved)"
    );
    assert!(
        !check_errs("interface Error {}").is_empty(),
        "`interface Error` must be rejected (reserved)"
    );
    assert!(
        !check_errs("type Error = int;").is_empty(),
        "`type Error` must be rejected (reserved)"
    );
    assert!(
        check_errs("class P implements Error { constructor(public string message) {} }").is_empty(),
        "a class may implement Error"
    );
    assert!(
        check_errs(
            r#"class P implements Error { constructor(public string message) {} }
function main() { P p = P("x"); if (p instanceof Error) { } }"#
        )
        .is_empty(),
        "instanceof Error must type-check"
    );
}

/// M-faults 2b.2: a class `implements Error` is a usable value type — construct it and read its
/// `message` field — byte-identical on run/runvm AND real PHP. In PHP it transpiles to
/// `class ParseError extends \Exception` with the promoted `message` emitted UNTYPED (a typed
/// redeclaration of \Exception's inherited `$message` is a PHP fatal) + `parent::__construct`, so
/// `e.message` (a plain field read) returns the value on every backend.
#[test]
fn error_subtype_value_is_byte_identical() {
    // NB: avoid PHP built-in error names (`Error`/`TypeError`/`ParseError`/…) — they collide with
    // PHP's reserved classes on transpile. `BadInput` is safe.
    let src = with_pkg(
        r#"import Core.Console;
class BadInput implements Error { constructor(public string message) {} }
function main() { BadInput e = BadInput("bad input"); Console.println(e.message); }"#,
    );
    let tree = cmd_run(&src);
    let vm = cmd_runvm(&src);
    assert_eq!(tree, vm, "run vs runvm:\n  run={tree:?}\n  runvm={vm:?}");
    if let Some(php) = php_or_gate("error_subtype_value_is_byte_identical") {
        let php_src = cli::cmd_transpile(&src).expect("transpile ok");
        let got = run_php(&php, &php_src, "error_subtype_value");
        let expected = tree.expect("run ok");
        assert_eq!(got, expected, "PHP ≠ interpreter\n--- php ---\n{php_src}");
    }
}

/// M-faults 2b.5: native unwinding on the VM (`Op::Throw`/`PushHandler`/`PopHandler`) is
/// byte-identical to the interpreter. These are `run ≡ runvm` only — the PHP transpile of
/// `throw`/`try`/`catch`/`finally` lands in 2b.6, after which an `examples/guide/errors.phg` adds the
/// three-way (`run ≡ runvm ≡ php`) gate (2b.7). The shared header defines two `Error` subtypes.
#[cfg(test)]
const ERR_HDR: &str = "import Core.Console; \
    class E1 implements Error { constructor(public string message) {} } \
    class E2 implements Error { constructor(public string message) {} }";

#[test]
fn throw_caught_and_finally_runs_on_both_backends() {
    // Normal path runs `a = parse(5)`; the throw path is caught; `finally` runs on every exit edge.
    agree(&format!(
        "{ERR_HDR} \
         function parse(int n) -> int throws E1 {{ if (n < 0) {{ throw E1(\"neg\"); }} return n + 1; }} \
         function main() {{ \
           try {{ \
             var a = parse(5); Console.println(\"a={{a}}\"); \
             var b = parse(0 - 3); Console.println(\"unreached\"); \
           }} catch (E1 e) {{ Console.println(\"caught {{e.message}}\"); }} \
           finally {{ Console.println(\"cleanup\"); }} \
         }}"
    ));
}

#[test]
fn return_through_finally_and_nested_rethrow_agree() {
    // `pick` returns through its `finally` on the ok path and re-throws (finally still runs) on the
    // throw path; the outer `try` catches the re-thrown exception.
    agree(&format!(
        "{ERR_HDR} \
         function pick(int n) -> int throws E1 {{ \
           try {{ if (n < 0) {{ throw E1(\"inner\"); }} return n; }} \
           finally {{ Console.println(\"fin {{n}}\"); }} \
         }} \
         function main() {{ \
           try {{ var a = pick(2); Console.println(\"a={{a}}\"); var b = pick(0 - 1); }} \
           catch (E1 e) {{ Console.println(\"outer {{e.message}}\"); }} \
         }}"
    ));
}

#[test]
fn multiple_and_union_catch_dispatch_agree() {
    // Multiple sequential `catch` clauses dispatch by type; a union `throws E1 | E2` is the set
    // {E1, E2}, each discharged by its own clause.
    agree(&format!(
        "{ERR_HDR} \
         function risky(int n) -> int throws E1 | E2 {{ \
           if (n == 1) {{ throw E1(\"one\"); }} if (n == 2) {{ throw E2(\"two\"); }} return n; \
         }} \
         function main() {{ for (int i in [1, 2, 3]) {{ \
           try {{ var r = risky(i); Console.println(\"ok {{r}}\"); }} \
           catch (E1 e) {{ Console.println(\"E1 {{e.message}}\"); }} \
           catch (E2 e) {{ Console.println(\"E2 {{e.message}}\"); }} \
         }} }}"
    ));
}

#[test]
fn break_and_continue_through_finally_agree() {
    // A `break`/`continue` out of a `try` inside a loop still runs the `finally` (and drops the
    // handler) before transferring — byte-identical on both backends.
    agree(&format!(
        "{ERR_HDR} \
         function main() {{ for (int i in [1, 2, 3, 4]) {{ \
           try {{ \
             if (i == 3) {{ break; }} if (i == 2) {{ continue; }} Console.println(\"body {{i}}\"); \
           }} finally {{ Console.println(\"fin {{i}}\"); }} \
         }} Console.println(\"done\"); }}"
    ));
}

#[test]
fn propagate_throws_with_question_mark_agrees() {
    // `f()?` on a throwing call propagates to the enclosing `throws`; the outer `try` catches it.
    agree(&format!(
        "{ERR_HDR} \
         function f() -> int throws E1 {{ throw E1(\"x\"); }} \
         function g() -> int throws E1 {{ return f()?; }} \
         function main() {{ try {{ var n = g(); }} catch (E1 e) {{ Console.println(\"g threw {{e.message}}\"); }} }}"
    ));
}

#[test]
fn panic_bypasses_catch_on_both_backends() {
    // A `Runtime` fault (division by zero) is NOT a catchable `throw`: it passes straight through an
    // enclosing `catch` and aborts identically on both backends (panics are uncatchable by design).
    agree_err(&format!(
        "{ERR_HDR} \
         function main() {{ var xs = [1, 0, 2]; \
           try {{ for (int x in xs) {{ var q = 10 / x; Console.println(\"q {{q}}\"); }} }} \
           catch (E1 e) {{ Console.println(\"nope\"); }} }}"
    ));
}

#[test]
fn s8_trait_method_reuse_is_byte_identical() {
    // M-RT S8 T1: a class composes a trait via `use`; the trait's method is flattened in and dispatches
    // identically on both backends and through native PHP `trait`/`use`.
    agree_out_php(
        "import Core.Console;
trait Loud { function shout(string s) -> string { return s; } function greet() -> string { return this.shout(\"hi\"); } }
class Crier { use Loud; }
function main() { Console.println(Crier().greet()); }",
        "hi\n",
        "s8_trait_method_reuse",
    );
}

#[test]
fn s8_trait_abstract_requirement_satisfied_is_byte_identical() {
    // A trait may *require* a method (abstract); a using class that provides it composes cleanly, and a
    // trait method calling the requirement dispatches to the class's implementation on both backends.
    agree_out_php(
        "import Core.Console;
trait Greeter { abstract function name() -> string; function hello() -> string { return this.name(); } }
class Person { use Greeter; function name() -> string { return \"Ada\"; } }
function main() { Console.println(Person().hello()); }",
        "Ada\n",
        "s8_trait_abstract_requirement",
    );
}
