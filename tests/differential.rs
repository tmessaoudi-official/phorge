//! Differential harness (M2 P3): the bytecode VM (`cmd_runvm`) must produce byte-identical
//! stdout to the tree-walking interpreter (`cmd_run`) for every P1–P3-surface program. This is
//! the M2 correctness spine (mirrors the transpiler round-trip-against-real-PHP technique).
//!
//! Parity covers *both* success and failure (M2 P3.5 Wave 0): `agree` checks the `Ok` output,
//! `agree_err` checks that a failing program faults the *same way* on both backends. Faults are
//! compared by semantic [`FaultKind`] rather than raw error text — the two backends share fault
//! bodies (e.g. `"division by zero"`) but the CLI wraps them with stage-specific prefixes
//! (`"runtime error:"` vs `"compile error:"`), so a raw `assert_eq!` would spuriously fail.

use phorj::cli::{cmd_run, cmd_run_exit, cmd_runvm, cmd_runvm_exit};
use phorj::{cli, loader};
use std::process::Command;

/// Type-check `src`; return the error diagnostics (empty = well-typed). Auto-prepends
/// `package Main;` if absent. Used to test checker rejections without running a backend.
fn check_errs(src: &str) -> Vec<phorj::diagnostic::Diagnostic> {
    let src = with_pkg(src);
    let tokens = phorj::lexer::lex(&src).expect("lex ok");
    let prog = phorj::parser::Parser::new(tokens)
        .parse_program()
        .expect("parse ok");
    match phorj::checker::check(&prog) {
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
    /// Bare `decimal /` with a non-terminating quotient (`1d/3d`, 2026-06-27 exact-or-fault). Both
    /// backends fault `"decimal division is not exact"`; classified by body substring so the VM's
    /// line prefix doesn't split it from the interpreter's prefix-less render.
    DecimalInexact,
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
    } else if err.contains("decimal division is not exact") {
        FaultKind::DecimalInexact
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
function main() -> void { Console.println("hello"); }"#,
    r#"import Core.Console;
function main() -> void { Console.println("{42}"); Console.println("{3.14}"); Console.println("{true}"); }"#,
    // int + float arithmetic (formatting parity: 12.0 -> "12")
    r#"import Core.Console;
function main() -> void { Console.println("{1 + 2 * 3 - 4}"); }"#,
    r#"import Core.Console;
function main() -> void { Console.println("{2.0 * 3.0}"); Console.println("{7.5 / 2.5}"); }"#,
    r#"import Core.Console;
function main() -> void { Console.println("{7 % 3}"); Console.println("{7.5 % 2.0}"); }"#,
    // comparison + equality + logical short-circuit
    r#"import Core.Console;
function main() -> void { Console.println("{1 < 2}"); Console.println("{2 <= 2}"); Console.println("{3 > 4}"); }"#,
    r#"import Core.Console;
function main() -> void { Console.println("{1 == 1}"); Console.println("{1 != 2}"); }"#,
    r#"import Core.Console;
function main() -> void { Console.println("{1 < 2 && 2 < 3}"); Console.println("{1 > 2 || 3 > 2}"); }"#,
    // unary
    r#"import Core.Console;
function main() -> void { Console.println("{-5}"); Console.println("{!false}"); }"#,
    // locals (int + float + string + bool)
    r#"import Core.Console;
function main() -> void { int x = 10; float y = 2.5; Console.println("{x}"); Console.println("{y}"); }"#,
    r#"import Core.Console;
function main() -> void { string s = "hi"; bool b = true; Console.println("{s}"); Console.println("{b}"); }"#,
    r#"import Core.Console;
function main() -> void { int a = 3; int b = 4; Console.println("{a * a + b * b}"); }"#,
    // if / else
    r#"import Core.Console;
function main() -> void { if (1 < 2) { Console.println("a"); } else { Console.println("b"); } }"#,
    r#"import Core.Console;
function main() -> void { int n = 5; if (n > 3) { Console.println("big"); } Console.println("end"); }"#,
    // for-in over list literals
    r#"import Core.Console;
function main() -> void { List<int> xs = [1, 2, 3]; for (int x in xs) { Console.println("{x}"); } }"#,
    r#"import Core.Console;
function main() -> void { for (float f in [1.5, 2.5]) { Console.println("{f * 2.0}"); } }"#,
    // nested blocks + for body locals
    r#"import Core.Console;
function main() -> void { for (int x in [10, 20]) { int y = x + 1; Console.println("{y}"); } }"#,
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
function main() -> void {
    Dog d = new Dog();
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
function main() -> void {
    Duck d = new Duck();
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
function main() -> void {
    Mid m = new Mid();
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
function main() -> void {
    Duck d = new Duck();
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
function main() -> void {
    Duck d = new Duck();
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
function main() -> void {
    Square s = new Square(3);
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
function main() -> void {
    Greeter g = new Greeter("Ada");
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
function main() -> void {
    Tally t = new Tally(7);
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
function main() -> void {
    Leaf l = new Leaf(42);
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
function main() -> void {
    Person p = new Person("Ada", 36);
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
function main() -> void {
    Player p = new Player("Bo", 21);
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
function main() -> void {
    Dog d = new Dog();
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
function main() -> void {
    Duck d = new Duck();
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
function main() -> void {
    C c = new C();
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
function main() -> void {
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
        function main() -> void {
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
        function main() -> void { Console.println("{tally(41)}"); }"#,
    );
}

/// M3 S1.1 — list indexing `xs[i]`. The checker already typed it; the backends were un-rejected
/// this slice. Reads must be byte-identical, and an out-of-range read must *fault* identically
/// (the VM's bounds check + the interpreter's must agree — `FaultKind::IndexOob`).
#[test]
fn s1_indexing_is_byte_identical() {
    agree(
        r#"import Core.Console;
function main() -> void { List<int> xs = [10, 20, 30]; Console.println("{xs[0]} {xs[2]}"); }"#,
    );
    // an index expression on a list literal, with the index coming from a loop variable
    agree(
        r#"import Core.Console;
function main() -> void { for (int i in [0, 1, 2]) { Console.println("{[5, 6, 7][i]}"); } }"#,
    );
}

#[test]
fn s1_index_oob_faults_identically() {
    agree_err(
        r#"import Core.Console;
function main() -> void { List<int> xs = [1, 2]; Console.println("{xs[5]}"); }"#,
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
function main() -> void { List<int> xs = [10, 20]; Console.println("{xs[0] + 1}"); }"#,
    );
    agree(
        r#"import Core.Console;
function main() -> void { List<float> fs = [1.5, 2.5]; Console.println("{fs[0] + fs[1]}"); }"#,
    );
    // index result of a range-materialized list, used arithmetically
    agree(
        r#"import Core.Console;
function main() -> void { var xs = 0..5; Console.println("{xs[2] * 10}"); }"#,
    );
}

/// M3 S1.2 — integer ranges `a..b` (exclusive) / `a..=b` (inclusive), materialized to `List<int>`
/// via the one new `Op::MakeRange`. The compiler/interpreter must build the *same* list (same order,
/// same emptiness rule) so `for…in` over a range is byte-identical on both backends.
#[test]
fn s1_ranges_are_byte_identical() {
    agree(
        r#"import Core.Console;
function main() -> void { for (int i in 0..3) { Console.println("{i}"); } }"#,
    ); // 0,1,2
    agree(
        r#"import Core.Console;
function main() -> void { for (int i in 1..=3) { Console.println("{i}"); } }"#,
    ); // 1,2,3
       // empty range (start >= end): the body never runs on either backend
    agree(
        r#"import Core.Console;
function main() -> void { for (int i in 5..5) { Console.println("{i}"); } Console.println("done"); }"#,
    );
    agree(
        r#"import Core.Console;
function main() -> void { for (int i in 5..2) { Console.println("{i}"); } Console.println("empty"); }"#,
    );
    // a range bound to a `var` (typed `List<int>`), then iterated
    agree(
        r#"import Core.Console;
function main() -> void { var xs = 0..3; for (int i in xs) { Console.println("{i + 1}"); } }"#,
    );
    // range bounds from expressions
    agree(
        r#"import Core.Console;
function lo() -> int { return 2; } function main() -> void { for (int i in lo()..lo() + 3) { Console.println("{i}"); } }"#,
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
function main() -> void { var x = if (1 < 2) { 10 } else { 20 }; Console.println("{x + x}"); }"#,
    );
    // in return position, both branches taken across two calls
    agree(
        r#"import Core.Console;
function pick(bool b) -> int { return if (b) { 1 } else { 2 }; }
           function main() -> void { Console.println("{pick(true)} {pick(false)}"); }"#,
    );
    // as a call argument (string-typed branches), inside a range loop
    agree(
        r#"import Core.Console;
function main() -> void { for (int i in 0..3) { Console.println(if (i == 1) { "one" } else { "x" }); } }"#,
    );
    // nested / float branches
    agree(
        r#"import Core.Console;
function main() -> void { float r = if (true) { 1.5 } else { 2.5 }; Console.println("{r * 2.0}"); }"#,
    );
}

/// P3 surface: user function calls, recursion, mutual recursion, void functions, returns in
/// branches, nested calls, float-returning functions, and calls as statements. Each must run
/// identically on both backends.
const P3_PROGRAMS: &[&str] = &[
    // single call used in interpolation
    r#"import Core.Console;
function inc(int n) -> int { return n + 1; } function main() -> void { Console.println("{inc(41)}"); }"#,
    // multiple params + call inside arithmetic
    r#"import Core.Console;
function add(int a, int b) -> int { return a + b; }
       function main() -> void { Console.println("{add(2, 3) * 10}"); }"#,
    // recursion (classic fib)
    r#"import Core.Console;
function fib(int n) -> int {
           if (n < 2) { return n; }
           return fib(n - 1) + fib(n - 2);
       }
       function main() -> void { Console.println("{fib(12)}"); }"#,
    // return in a branch vs fall-through
    r#"import Core.Console;
function sign(int n) -> int { if (n < 0) { return -1; } return 1; }
       function main() -> void { Console.println("{sign(-9)}"); Console.println("{sign(4)}"); }"#,
    // mutual recursion (forward reference: isEven calls isOdd declared later)
    r#"import Core.Console;
function isEven(int n) -> bool { if (n == 0) { return true; } return isOdd(n - 1); }
       function isOdd(int n) -> bool { if (n == 0) { return false; } return isEven(n - 1); }
       function main() -> void { Console.println("{isEven(10)}"); Console.println("{isOdd(7)}"); }"#,
    // nested calls
    r#"import Core.Console;
function sq(int n) -> int { return n * n; }
       function main() -> void { Console.println("{sq(sq(2))}"); }"#,
    // float-returning function in float arithmetic
    r#"import Core.Console;
function half(float x) -> float { return x / 2.0; }
       function main() -> void { Console.println("{half(5.0) + 1.0}"); }"#,
    // void function (no return type) called for its side effect
    r#"import Core.Console;
function greet(string who) -> void { Console.println("hi, {who}"); }
       function main() -> void { greet("Phorj"); greet("world"); }"#,
    // call used as a statement (return value discarded)
    r#"import Core.Console;
function noisy(int n) -> int { Console.println("got {n}"); return n; }
       function main() -> void { noisy(42); Console.println("done"); }"#,
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
       function main() -> void { Console.println(describe(new Pass(90))); Console.println(describe(new Fail(40))); }"#,
    // bare (no-payload) variants, wildcard arm, `match` in var-decl-init position
    r#"import Core.Console;
enum Color { Red, Green, Blue, }
       function main() -> void {
           Color c = Green;
           string name = match c { Red => "red", Green => "green", _ => "other", };
           Console.println(name);
       }"#,
    // literal int patterns + catch-all binding used in interpolation
    r#"import Core.Console;
function label(int n) -> string {
           return match n { 0 => "zero", 1 => "one", x => "many ({x})", };
       }
       function main() -> void { Console.println(label(0)); Console.println(label(1)); Console.println(label(7)); }"#,
    // bool literal patterns
    r#"import Core.Console;
function yn(bool b) -> string { return match b { true => "Y", false => "N", }; }
       function main() -> void { Console.println(yn(true)); Console.println(yn(false)); }"#,
    // string literal patterns + wildcard
    r#"import Core.Console;
function kind(string s) -> string {
           return match s { "a" => "first", "b" => "second", _ => "rest", };
       }
       function main() -> void { Console.println(kind("a")); Console.println(kind("b")); Console.println(kind("z")); }"#,
    // enum value flows through a local and equality (`==` on enums) before matching
    r#"import Core.Console;
enum Dir { N, S, }
       function main() -> void {
           Dir d = N;
           Console.println("{d == N}");
           string t = match d { N => "north", S => "south", };
           Console.println(t);
       }"#,
    // `match` in a *transient* position: as the rhs of `+`, with the lhs already on the operand
    // stack (exercises the compiler's operand-height tracking for the scrutinee slot).
    r#"import Core.Console;
function g(int n) -> int { return 1 + match n { 0 => 10, _ => 20 }; }
       function main() -> void { Console.println("{g(0)}"); Console.println("{g(5)}"); }"#,
    // nested `match` whose inner arm references the *outer* arm's binding (re-extraction across
    // two live scrutinees — the hardest binding/height case in P4a).
    r#"import Core.Console;
enum Pair { P(int a, int b), }
       function f(Pair p) -> string {
           return match p {
               P(a, b) => match a { 0 => "first=zero b={b}", _ => "a={a} b={b}", },
           };
       }
       function main() -> void { Console.println(f(new P(0, 9))); Console.println(f(new P(5, 2))); }"#,
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
       function main() -> void { Point p = new Point(3, 4); Console.println("{p.x},{p.y}"); }"#,
    // field read flowing through a typed local, then used as an arithmetic operand
    r#"import Core.Console;
class Point { constructor(public int x, public int y) {} }
       function main() -> void { Point p = new Point(3, 4); int s = p.x; Console.println("{s + p.y}"); }"#,
    // constructor *body* runs for side effects (a `println` in the ctor), using a promoted param
    r#"import Core.Console;
class Greeter { constructor(public string name) { Console.println("made {name}"); } }
       function main() -> void { Greeter g = new Greeter("Ada"); Console.println("hello {g.name}"); }"#,
    // a no-constructor class builds an empty instance; structural instance equality
    r#"import Core.Console;
class Empty {}
       function main() -> void { Empty a = new Empty(); Empty b = new Empty(); Console.println("{a == b}"); }"#,
    // instance equality is structural over fields (same class + equal fields)
    r#"import Core.Console;
class P { constructor(public int x) {} }
       function main() -> void { P a = new P(1); P b = new P(1); P c = new P(2); Console.println("{a == b} {a == c}"); }"#,
    // only *promoted* params become fields (the bare `seed` param is not a field)
    r#"import Core.Console;
class Acc { constructor(public int total, int seed) {} }
       function main() -> void { Acc a = new Acc(10, 99); Console.println("{a.total}"); }"#,
    // a field read as a call argument
    r#"import Core.Console;
class Box { constructor(public int v) {} }
       function dbl(int n) -> int { return n * 2; }
       function main() -> void { Box b = new Box(21); Console.println("{dbl(b.v)}"); }"#,
    // a bare `return;` in the ctor body is an early exit, but the promoted instance is *still*
    // returned (interpreter parity) — exercises the synthetic ctor's epilogue redirect.
    r#"import Core.Console;
class C { constructor(public int x) { if (x > 0) { return; } Console.println("nonpos"); } }
       function main() -> void { C a = new C(5); Console.println("{a.x}"); C b = new C(0); Console.println("{b.x}"); }"#,
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
           function main() -> void { Box b = new Box(5); Console.println("{b.tag}"); }"#,
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
       function main() -> void { Counter c = new Counter(100); Console.println("{c.add(23)}"); }"#,
    // a method calls another method via `this`, and reads a field via `this.`
    r#"import Core.Console;
class C { constructor(public int x) {}
           function dbl() -> int { return this.x + this.x; }
           function quad() -> int { int d = this.dbl(); return d + d; } }
       function main() -> void { C c = new C(5); Console.println("{c.quad()}"); }"#,
    // mixed bare-field + explicit-`this` field reads in one expression
    r#"import Core.Console;
class P { constructor(public int x, public int y) {} function sum() -> int { return x + this.y; } }
       function main() -> void { P p = new P(3, 4); Console.println("{p.sum()}"); }"#,
    // recursion *through* a method (`this.fact(n - 1)`)
    r#"import Core.Console;
class F { constructor(public int base) {}
           function fact(int n) -> int { if (n <= 1) { return 1; } return n * this.fact(n - 1); } }
       function main() -> void { F f = new F(0); Console.println("{f.fact(5)}"); }"#,
    // a void (no-return) method invoked as a statement, twice (side effects + Unit result)
    r#"import Core.Console;
class Logger { constructor(public string tag) {} function log() -> void { Console.println("log {tag}"); } }
       function main() -> void { Logger l = new Logger("X"); l.log(); l.log(); }"#,
];

#[test]
fn p4c_programs_match_between_backends() {
    for src in P4C_PROGRAMS {
        agree(src);
    }
}

/// True if `src` imports an **impure** stdlib module — one whose natives read the ambient environment
/// (`Core.Process` / `Core.Env`). Such a program is QUARANTINED from the byte-identity differential:
/// the PHP leg runs in a separate process whose argv/env need not match the Rust process, so the
/// output is not a fixed golden. These are tested separately under a controlled environment in
/// `tests/process.rs` (their `examples/process/` files are walkthroughs, not gated examples — Q2-A of
/// `docs/specs/2026-06-25-process-io-quarantine-seam-design.md`). The impure-module set is **derived
/// from the `NativeFn::pure` flag**, not hardcoded here, so a future impure module is covered with no
/// harness edit (the seam the `pure` marker exists for).
fn uses_impure_native(src: &str) -> bool {
    use std::collections::HashSet;
    let impure: HashSet<&str> = phorj::native::registry()
        .iter()
        .filter(|n| !n.pure)
        .map(|n| n.module)
        .collect();
    impure.iter().any(|m| src.contains(&format!("import {m}")))
}

/// Recursively collect every single-file `*.phg` under `dir`, **skipping project roots**. A
/// directory containing a `phorj.toml` is a multi-file project (M5): its files import each other
/// and only run when assembled through `loader::load`, so running them standalone here would fail.
/// `all_example_projects_match_between_backends` gates those instead. The exclusion is structural
/// (keyed on the manifest's presence), not name-based, so any project added under `examples/` later
/// is auto-excluded with no test edit.
fn collect_phg(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if dir.join("phorj.toml").is_file() {
        return; // a project root — handled by the project-aware harness below
    }
    // M8.5: `examples/interop/` holds foreign-PHP (`declare`) walkthroughs that are PHP-target-only —
    // they cannot run on the Rust backends (`E-FOREIGN-RUNTIME`), so they are not byte-identity-gated
    // here. `tests/interop.rs` validates them via transpile → real PHP golden output instead.
    if dir.file_name().and_then(|n| n.to_str()) == Some("interop") {
        return;
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

/// Recursively collect every project root (a directory holding a `phorj.toml`) under `dir`.
fn collect_projects(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    // M8.5: `examples/interop/` holds foreign-PHP (`declare`) walkthroughs — including `.d.phg`
    // declaration-file projects — that are PHP-target-only (`E-FOREIGN-RUNTIME`), so they cannot be
    // byte-identity-gated here. `tests/interop.rs` validates them via transpile → real PHP golden.
    if dir.file_name().and_then(|n| n.to_str()) == Some("interop") {
        return;
    }
    if dir.join("phorj.toml").is_file() {
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
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        // Quarantined (ambient-environment) examples are tested in tests/process.rs, not here.
        if uses_impure_native(&src) {
            eprintln!("differential: SKIP (impure/quarantined) {}", path.display());
            continue;
        }
        eprintln!("differential: {}", path.display()); // names the file if agree() panics
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
/// walks up to its `phorj.toml`, parses every file under the source root, validates folder=path, and
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
             function main() -> void { Console.println("hello"); Console.println("{2 + 2}"); }"#,
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
       function main() -> void { Point p = new Point(7, 4); Console.println("{p.x + 1}"); }"#,
    // (B) method-call result used arithmetically
    r#"import Core.Console;
class C { constructor(public int x) {} function get() -> int { return this.x; } }
       function main() -> void { C c = new C(5); Console.println("{c.get() + 1}"); }"#,
    // (C) nested field read `a.inner.x` — a class-typed field's field
    r#"import Core.Console;
class Inner { constructor(public int x) {} }
       class Outer { constructor(public Inner inner) {} }
       function main() -> void { Outer a = new Outer(new Inner(10)); Console.println("{a.inner.x + 1}"); }"#,
    // (D) a class-typed enum payload, bound in `match` and read arithmetically
    r#"import Core.Console;
class Point { constructor(public int x) {} }
       enum Opt { Some(Point p), Zero(int z), }
       function f(Opt o) -> int { return match o { Some(p) => p.x + 1, Zero(z) => z, }; }
       function main() -> void { Console.println("{f(new Some(new Point(41)))}"); Console.println("{f(new Zero(0))}"); }"#,
    // (E) a free function returning an instance, then a field of the result, used arithmetically
    r#"import Core.Console;
class Point { constructor(public int x) {} }
       function mk() -> Point { return new Point(3); }
       function main() -> void { Console.println("{mk().x + 1}"); }"#,
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
function main() -> void { int x = -9223372036854775807 - 1; Console.println("{-x}"); }"#,
    // integer overflow: i64::MAX + 1
    r#"import Core.Console;
function main() -> void { Console.println("{9223372036854775807 + 1}"); }"#,
    // division by zero
    r#"import Core.Console;
function main() -> void { int z = 0; Console.println("{1 / z}"); }"#,
    // modulo by zero
    r#"import Core.Console;
function main() -> void { int z = 0; Console.println("{1 % z}"); }"#,
    // float division by zero — faults like int /0 (no IEEE inf), byte-identical run/runvm
    r#"import Core.Console;
function main() -> void { float z = 0.0; Console.println("{1.0 / z}"); }"#,
    // float modulo by zero — faults like int %0 (PHP fmod would give NAN; we throw)
    r#"import Core.Console;
function main() -> void { float z = 0.0; Console.println("{1.0 % z}"); }"#,
    // decimal bare `/` non-terminating quotient — exact-or-fault faults on both backends
    r#"import Core.Console;
function main() -> void { decimal z = 3d; Console.println("{1d / z}"); }"#,
    // decimal bare `/` by zero — faults on both backends
    r#"import Core.Console;
function main() -> void { decimal z = 0d; Console.println("{1d / z}"); }"#,
    // decimal `%` by zero — faults on both backends
    r#"import Core.Console;
function main() -> void { decimal z = 0d; Console.println("{1d % z}"); }"#,
    // unbounded recursion: trips the shared `MAX_CALL_DEPTH` guard on both backends.
    // Before Task 0.3 the interpreter recursed on the native stack and SIGABRTed (exit 134)
    // while the VM cleanly reported "stack overflow" — a parity divergence in the fault path.
    r#"import Core.Console;
function rec(int n) -> int { return rec(n) + 1; } function main() -> void { Console.println("{rec(0)}"); }"#,
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
        "import Core.Console; function main() -> void {{ int x = {}1{}; Console.println(\"{{x}}\"); }}",
        "(".repeat(5000),
        ")".repeat(5000),
    );
    agree_err(&parens);
    let unary = format!(
        "import Core.Console; function main() -> void {{ bool b = {}true; Console.println(\"{{b}}\"); }}",
        "!".repeat(5000),
    );
    agree_err(&unary);
    // A long left-associative chain is built *iteratively*, so it escapes the parser's nesting
    // limit but produces a deeply left-leaning AST. The checker's depth guard (the gate both
    // backends share) must fault it identically rather than letting a walker overflow its stack.
    let chain = format!(
        "import Core.Console; function main() -> void {{ int x = 1{}; Console.println(\"{{x}}\"); }}",
        "+1".repeat(20_000),
    );
    agree_err(&chain);
}

#[test]
fn s2_null_and_optional_bind_and_run_on_both_backends() {
    // Task 1 foundation: `null` is a real runtime value and a non-null `T` widens to `T?`.
    // (Observing the null *value* needs the unwrap operators from later S2 tasks.) The exact-output
    // assertion is deliberate: `agree` alone passes vacuously if both backends share a rejection.
    let src = "import Core.Console; function main() -> void { int? x = null; int? y = 5; Console.println(\"optionals ok\"); }";
    assert_eq!(cmd_run(&with_pkg(src)).as_deref(), Ok("optionals ok\n"));
    agree(src); // run ≡ runvm
}

#[test]
fn s2_coalesce_is_byte_identical() {
    // `??`: a null lhs falls through to the default; a present value is kept.
    let src = "import Core.Console; function main() -> void { int? x = null; Console.println(\"{x ?? 7}\"); int? y = 9; Console.println(\"{y ?? 0}\"); }";
    assert_eq!(cmd_run(&with_pkg(src)).as_deref(), Ok("7\n9\n"));
    agree(src);
    // Short-circuit: the default (a printing call) must not run when the lhs is non-null.
    let sc = "import Core.Console; function side() -> int { Console.println(\"SIDE\"); return 0; } function main() -> void { int? y = 9; Console.println(\"{y ?? side()}\"); }";
    assert_eq!(cmd_run(&with_pkg(sc)).as_deref(), Ok("9\n"));
    agree(sc);
}

#[test]
fn s2_safe_access_is_byte_identical() {
    // `?.` short-circuits to null on a null receiver (→ the `?? -1` default) and reads through when
    // the receiver is present. Field read and method call both go through `?.`.
    // `v` is `public` so the `?.v` field-read case below is a legal external access (Wave 1.1
    // visibility enforcement); the method cases read `v` internally regardless.
    let cls = "class Box { constructor(public int v) {} function vOf() -> int { return this.v; } function plus(int n) -> int { return this.v + n; } }";
    let field = cls.to_string()
        + "import Core.Console;  function main() -> void { Box? a = null; Console.println(\"{(a?.v) ?? -1}\"); Box? b = new Box(7); Console.println(\"{(b?.v) ?? -1}\"); }";
    assert_eq!(cmd_run(&with_pkg(&field)).as_deref(), Ok("-1\n7\n"));
    agree(&field);
    let method = cls.to_string()
        + "import Core.Console;  function main() -> void { Box? a = null; Console.println(\"{(a?.vOf()) ?? -1}\"); Box? b = new Box(9); Console.println(\"{(b?.vOf()) ?? -1}\"); }";
    assert_eq!(cmd_run(&with_pkg(&method)).as_deref(), Ok("-1\n9\n"));
    agree(&method);
    // short-circuit: a safe call on a null receiver must NOT evaluate its arguments (no "SIDE").
    let sc = cls.to_string()
        + "import Core.Console;  function side() -> int { Console.println(\"SIDE\"); return 0; } function main() -> void { Box? a = null; Console.println(\"{(a?.plus(side())) ?? -1}\"); }";
    assert_eq!(cmd_run(&with_pkg(&sc)).as_deref(), Ok("-1\n"));
    agree(&sc);
}

#[test]
fn s2_if_let_is_byte_identical() {
    // `if (var x = opt)`: the then-branch runs (with `x` bound to the non-null inner) only when the
    // optional is present; otherwise the else-branch runs.
    let present =
        "import Core.Console; function main() -> void { int? o = 5; if (var x = o) { Console.println(\"got {x}\"); } else { Console.println(\"none\"); } }";
    assert_eq!(cmd_run(&with_pkg(present)).as_deref(), Ok("got 5\n"));
    agree(present);
    let absent =
        "import Core.Console; function main() -> void { int? o = null; if (var x = o) { Console.println(\"got {x}\"); } else { Console.println(\"none\"); } }";
    assert_eq!(cmd_run(&with_pkg(absent)).as_deref(), Ok("none\n"));
    agree(absent);
    // The smart-cast inner is a real arithmetic operand: `x + 1` must specialize identically on both
    // backends (guards the run↔runvm operand-type gap — see the cty-tracks-operand-types invariant).
    let arith =
        "import Core.Console; function main() -> void { int? o = 41; if (var x = o) { Console.println(\"{x + 1}\"); } else { Console.println(\"none\"); } }";
    assert_eq!(cmd_run(&with_pkg(arith)).as_deref(), Ok("42\n"));
    agree(arith);
}

#[test]
fn s2_force_unwrap_is_byte_identical() {
    // `opt!` on a present optional yields the inner value, identically on both backends.
    let present =
        "import Core.Console; function main() -> void { int? o = 5; Console.println(\"{o!}\"); }";
    assert_eq!(cmd_run(&with_pkg(present)).as_deref(), Ok("5\n"));
    agree(present);
    // The unwrapped value is a real arithmetic operand: `o! + 1` must specialize identically
    // (guards the run↔runvm operand-type gap — see the cty-tracks-operand-types invariant).
    let arith =
        "import Core.Console; function main() -> void { int? o = 41; Console.println(\"{o! + 1}\"); }";
    assert_eq!(cmd_run(&with_pkg(arith)).as_deref(), Ok("42\n"));
    agree(arith);
}

#[test]
fn s2_force_unwrap_null_faults_identically() {
    // `opt!` on null is a clean fault with the SAME FaultKind on both backends (no crash, no UB).
    let src = "function main() -> void { int? o = null; int x = o!; }";
    agree_err(src); // FaultKind::ForceUnwrap on both
}

#[test]
fn s2_multiple_null_ops_in_one_expr_are_byte_identical() {
    // Regression: two `??`/`?.`/`!` in one expression. Each stashes its receiver in a scratch slot;
    // that slot is the receiver's frame position (`height-1`), so live transients from an earlier
    // segment must not shift it. The interpreter is the oracle; the VM must match (not fault).
    let two_coalesce =
        "import Core.Console; function main() -> void { int? a = 5; int? b = null; Console.println(\"{a ?? -1} {b ?? -1}\"); }";
    assert_eq!(cmd_run(&with_pkg(two_coalesce)).as_deref(), Ok("5 -1\n"));
    agree(two_coalesce);

    let two_force = "import Core.Console; function main() -> void { int? a = 1; int? b = 2; Console.println(\"{a!} {b!}\"); }";
    assert_eq!(cmd_run(&with_pkg(two_force)).as_deref(), Ok("1 2\n"));
    agree(two_force);

    let cls =
        "class Box { constructor(private int v) {} function get() -> int { return this.v; } }";
    let two_safe = cls.to_string()
        + "import Core.Console;  function main() -> void { Box? a = new Box(7); Box? b = null; Console.println(\"{(a?.get()) ?? -1} {(b?.get()) ?? -1}\"); }";
    assert_eq!(cmd_run(&with_pkg(&two_safe)).as_deref(), Ok("7 -1\n"));
    agree(&two_safe);

    // Mixed + nested: a coalesce whose default is itself a safe-access-coalesce, beside a force.
    let mixed = cls.to_string()
        + "import Core.Console;  function main() -> void { Box? a = null; int? b = 9; Console.println(\"{(a?.get()) ?? (b ?? 0)} {b!}\"); }";
    assert_eq!(cmd_run(&with_pkg(&mixed)).as_deref(), Ok("9 9\n"));
    agree(&mixed);
}

#[test]
fn s2_match_over_optional_is_byte_identical() {
    // `match opt { null => …, v => … }`: the null arm fires on null, the binding arm narrows `v` to
    // the non-null inner `int` (used here as an arithmetic operand — guards the operand-type gap).
    let src = "import Core.Console; function f(int? o) -> int { return match o { null => -1, v => v + 1 }; } \
               function main() -> void { int? a = null; int? b = 7; Console.println(\"{f(a)}\"); Console.println(\"{f(b)}\"); }";
    assert_eq!(cmd_run(&with_pkg(src)).as_deref(), Ok("-1\n8\n"));
    agree(src);
}

// ── M3 S3: lambdas ─────────────────────────────────────────────────────────────────────────────

#[test]
fn lambdas_agree() {
    // Basic lambda var call
    agree("import Core.Console; function main() -> void { var d = fn(int x) => x*2; Console.println(\"{d(5)}\"); }");
    // Lambda capturing TWO enclosing vars (slot-ordering trigger — invariant #8)
    agree("import Core.Console; function main() -> void { var a=10; var b=100; var f=fn(int x)=>x+a+b; Console.println(\"{f(1)}\"); }");
    // Higher-order user function (lambda passed as argument)
    agree("import Core.Console; function twice(int x,(int)->int f)->int{return f(f(x));} function main()-> void { Console.println(\"{twice(3, fn(int n)=>n+1)}\"); }");
    // Lambda call inside string interpolation (height-sensitive — F13)
    agree("import Core.Console; function main()-> void { var inc=fn(int x)=>x+1; Console.println(\"{inc(1)} {inc(2)}\"); }");
    // Lambda call inside a match arm (height-sensitive — F13)
    agree("import Core.Console; enum E{A(),B()} function pick(E e,(int)->int f)->int{ return match e { A()=>f(1), B()=>f(2) }; } function main()-> void { Console.println(\"{pick(new A(), fn(int x)=>x*10)}\"); }");
    // Zero-param lambda
    agree("import Core.Console; function main()-> void { var greet=fn()=>42; Console.println(\"{greet()}\"); }");
}

#[test]
fn lambda_call_errors_agree() {
    // Arity mismatch: lambda expects 1 arg, called with 2
    agree_err("import Core.Console; function main()-> void { var f=fn(int x)=>x; Console.println(\"{f(1,2)}\"); }");
}

#[test]
fn statement_body_lambda_agrees() {
    agree("import Core.Console; function main()-> void { var base=100; var f = fn(int x) -> int { var y = x*2; return y + base; }; Console.println(\"{f(3)}\"); }");
    // 106
}

#[test]
fn statement_body_lambda_needs_return_type() {
    let errs =
        check_errs("package Main; function main()-> void { var f = fn(int x) { return x; }; }");
    assert!(
        errs.iter().any(|e| e.message.contains("explicit `-> T`")),
        "{errs:?}"
    );
}

#[test]
fn transpiles_statement_lambda_with_use_clause() {
    let php = transpile_ok("package Main; import Core.Console; function main()-> void { var base=100; var f = fn(int x) -> int { return x + base; }; Console.println(\"{f(3)}\"); }");
    // T6: `x` (int param) + `base` (int local) → native `+`, no `__phorj_add` helper.
    assert!(
        php.contains("function($x) use ($base)") && php.contains("return $x + $base"),
        "{php}"
    );
}

#[test]
fn pipe_agrees() {
    // `5 |> dbl |> inc` == inc(dbl(5)) == 11 (left-associative)
    agree("import Core.Console; function dbl(int x)->int{return x*2;} function inc(int x)->int{return x+1;} function main()-> void { Console.println(\"{5 |> dbl |> inc}\"); }");
    // inline lambda on the right: `3 |> fn(int v) => v + 10` == 13
    agree("import Core.Console; function main()-> void { var add=fn(int a,int b)->int{return a+b;}; Console.println(\"{3 |> fn(int v) => v + 10}\"); }");
    // precedence: `1 + 2 |> dbl` == dbl(1+2) == 6
    agree("import Core.Console; function dbl(int x)->int{return x*2;} function main()-> void { Console.println(\"{1 + 2 |> dbl}\"); }");
}

#[test]
fn mutation_reassign_agrees() {
    // M-mut.1: mutable locals + reassignment, byte-identical on both backends.
    // Plain reassignment.
    agree("import Core.Console; function main()-> void { mutable int x = 1; x = 2; Console.println(\"{x}\"); }");
    // Reassign from the variable's own value.
    agree("import Core.Console; function main()-> void { mutable int x = 1; x = x + 5; Console.println(\"{x}\"); }");
    // `mutable var` (inferred type) reassignment.
    agree("import Core.Console; function main()-> void { mutable var x = 10; x = x * 3; Console.println(\"{x}\"); }");
    // Two-binding SCALAR case (F13): a scalar copies, so reassigning `b` must not change `a`.
    agree("import Core.Console; function main()-> void { int a = 10; mutable int b = a; b = 99; Console.println(\"{a} {b}\"); }");
    // Reassignment inside a loop body (accumulator).
    agree("import Core.Console; function main()-> void { mutable int sum = 0; for (int n in 1..=3) { sum = sum + n; } Console.println(\"{sum}\"); }");
}

#[test]
fn mutation_compound_assign_agrees() {
    // M-mut.2: compound-assign + ++/-- + ??= desugar to `Stmt::Assign`, byte-identical on both.
    // The five op= forms as accumulators.
    agree("import Core.Console; function main()-> void { mutable int x = 10; x += 5; x -= 3; x *= 2; Console.println(\"{x}\"); }"); // 24
                                                                                                                                    // Integer `/=` routes through the intdiv kernel (F7): 24 / 5 = 4 (truncating), NOT float 4.8.
    agree("import Core.Console; function main()-> void { mutable int x = 24; x /= 5; Console.println(\"{x}\"); }"); // 4
                                                                                                                    // `%=` with a NEGATIVE dividend — PHP's sign-follows-dividend (spec §8 #3): -7 % 3 = -1.
    agree("import Core.Console; function main()-> void { mutable int x = 0 - 7; x %= 3; Console.println(\"{x}\"); }"); // -1
                                                                                                                       // `%=` positive dividend, negative divisor: 7 % -3 = 1 (sign follows dividend).
    agree("import Core.Console; function main()-> void { mutable int x = 7; x %= 0 - 3; Console.println(\"{x}\"); }"); // 1
                                                                                                                       // `??=` on an optional: assigns only when null.
    agree("import Core.Console; function main()-> void { mutable int? a = null; a ??= 7; mutable int? b = 3; b ??= 9; Console.println(\"{a ?? -1} {b ?? -1}\"); }"); // 7 3
                                                                                                                                                                     // Statement `++`/`--` counter.
    agree("import Core.Console; function main()-> void { mutable int n = 0; n++; n++; n++; n--; Console.println(\"{n}\"); }"); // 2
                                                                                                                               // Two-binding SCALAR observe (F13): a compound op on `b` must not touch `a` (value-copy).
    agree("import Core.Console; function main()-> void { int a = 5; mutable int b = a; b += 100; Console.println(\"{a} {b}\"); }"); // 5 105
                                                                                                                                    // Compound-assign inside a loop accumulator.
    agree("import Core.Console; function main()-> void { mutable int sum = 0; for (int i in 1..=5) { sum += i; } Console.println(\"{sum}\"); }");
    // 15
}

#[test]
fn mutation_element_set_agrees() {
    // M-mut.5: value-type element set xs[i]=e / m[k]=e — byte-identical on both backends.
    // List element set.
    agree("import Core.Console; function main()-> void { mutable List<int> xs = [1, 2, 3]; xs[1] = 20; Console.println(\"{xs[0]} {xs[1]} {xs[2]}\"); }"); // 1 20 3
                                                                                                                                                          // Compound element set rides the M-mut.2 desugar.
    agree("import Core.Console; function main()-> void { mutable List<int> xs = [1, 2, 3]; xs[0] += 100; xs[2] *= 5; Console.println(\"{xs[0]} {xs[2]}\"); }"); // 101 15
                                                                                                                                                                // COPY-ON-WRITE value semantics (the P0 catcher, F13): mutating `ys` must not touch `xs`.
    agree("import Core.Console; function main()-> void { mutable List<int> xs = [1, 2]; mutable List<int> ys = xs; ys[0] = 999; Console.println(\"{xs[0]} {ys[0]}\"); }"); // 1 999
                                                                                                                                                                           // Map update (existing key) + insert (new key), insertion-ordered.
    agree("import Core.Console; function main()-> void { mutable Map<string, int> m = [\"a\" => 1]; m[\"a\"] = 10; m[\"b\"] = 20; Console.println(\"{m[\"a\"]} {m[\"b\"]}\"); }"); // 10 20
                                                                                                                                                                                   // Map COW: a copy is independent.
    agree("import Core.Console; function main()-> void { mutable Map<string, int> m = [\"a\" => 1]; mutable Map<string, int> n = m; n[\"a\"] = 99; Console.println(\"{m[\"a\"]} {n[\"a\"]}\"); }"); // 1 99
                                                                                                                                                                                                    // Set element in a loop (accumulate into a list).
    agree("import Core.Console; function main()-> void { mutable List<int> xs = [0, 0, 0]; for (mutable int i = 0; i < 3; i++) { xs[i] = i * i; } Console.println(\"{xs[0]} {xs[1]} {xs[2]}\"); }");
    // 0 1 4
}

#[test]
fn mutation_element_set_oob_faults_agree() {
    // M-mut.5: an out-of-range list element SET faults identically on both Rust backends
    // (FaultKind::IndexOob). NOT PHP-gated — PHP would *extend* the array instead (KNOWN_ISSUES).
    agree_err("import Core.Console; function main()-> void { mutable List<int> xs = [1, 2]; xs[5] = 9; Console.println(\"unreached\"); }");
}

#[test]
fn mutation_instance_field_set_agrees() {
    // M-mut.6: shared-mutable instance field set `o.f = e` — handle semantics, byte-identical on
    // run/runvm + real PHP (`agree` is the 3-way oracle).
    // Basic field set + read-back.
    agree("import Core.Console; class P { constructor(public mutable int x) {} } function main()-> void { P p = new P(1); p.x = 42; Console.println(\"{p.x}\"); }"); // 42
                                                                                                                                                                     // HANDLE semantics (the P0 catcher, F13): mutate via one binding, observe via the alias — BOTH
                                                                                                                                                                     // see it (the opposite of value-type COW). This is the value/handle slip a 2-binding test catches.
    agree("import Core.Console; class P { constructor(public mutable int x) {} } function main()-> void { P p = new P(1); P q = p; p.x = 99; Console.println(\"{p.x} {q.x}\"); }"); // 99 99
                                                                                                                                                                                    // `this.f = e` inside a method, visible through the original binding across calls.
    agree("import Core.Console; class C { constructor(public mutable int n) {} function bump() -> int { this.n = this.n + 1; return this.n; } } function main()-> void { C c = new C(10); c.bump(); c.bump(); Console.println(\"{c.n}\"); }"); // 12
                                                                                                                                                                                                                                               // A declared (non-promoted) `mutable` field initialized in the ctor body via `this.f = e`.
    agree("import Core.Console; class B { mutable int v; constructor(int seed) { this.v = seed * 2; } function get() -> int { return this.v; } } function main()-> void { B b = new B(5); b.v = b.v + 1; Console.println(\"{b.get()}\"); }"); // 11
                                                                                                                                                                                                                                              // Field set on an instance reached through another field (`a.b.c = e`) — handle semantics all the way.
    agree("import Core.Console; class Inner { constructor(public mutable int v) {} } class Outer { constructor(public Inner inner) {} } function main()-> void { Outer o = new Outer(new Inner(1)); o.inner.v = 7; Console.println(\"{o.inner.v}\"); }");
    // 7
}

#[test]
fn mutation_static_field_agrees() {
    // M-mut.7: program-lifetime `static mutable` class fields, read/written as `ClassName.field` —
    // byte-identical run/runvm + real PHP. A static is shared across all instances (one program-level
    // slot), so a counter incremented in the constructor accumulates across constructions.
    agree("import Core.Console; class Counter { static mutable int total = 0; constructor() { Counter.total = Counter.total + 1; } } function main()-> void { new Counter(); new Counter(); new Counter(); Console.println(\"{Counter.total}\"); }"); // 3
                                                                                                                                                                                                                                                      // Direct read/write from a free function; an immutable static string too.
    agree("import Core.Console; class Cfg { static mutable int n = 10; static string name = \"cfg\"; } function main()-> void { Cfg.n = Cfg.n + 5; Console.println(\"{Cfg.name}={Cfg.n}\"); }"); // cfg=15
                                                                                                                                                                                                 // A static read used as an arithmetic operand inside a method (the CTy-operand path).
    agree("import Core.Console; class C { static mutable int k = 1; function step() -> int { C.k = C.k * 2; return C.k + 1; } } function main()-> void { C c = new C(); Console.println(\"{c.step()} {c.step()}\"); }");
    // 3 5
}

#[test]
fn mutation_property_hooks_agrees() {
    // M-mut.7b: property hooks `T name { get => …; set(T v) { … } }` — a get computes on read, a
    // set intercepts a write (typically mutating a backing `mutable` field). Byte-identical on
    // run/runvm + real PHP (the synthetic-method VM lowering ≡ the PHP 8.4 property hook).
    // A read-only computed hook reads a backing field.
    agree("import Core.Console; class C { constructor(public mutable int raw) {} int doubled { get => this.raw * 2; } } function main()-> void { C c = new C(21); Console.println(\"{c.doubled}\"); }"); // 42
                                                                                                                                                                                                         // A get used as an arithmetic operand — the CTy-operand path (`o.hook + 1` must specialize on the VM).
    agree("import Core.Console; class C { constructor(public mutable int raw) {} int doubled { get => this.raw * 2; } } function main()-> void { C c = new C(21); Console.println(\"{c.doubled + 1}\"); }"); // 43
                                                                                                                                                                                                             // A set writes a backing field; observe through both the hook (get) and the raw field.
    agree("import Core.Console; class C { constructor(public mutable int raw) {} int half { get => this.raw; set(int v) { this.raw = v / 2; } } } function main()-> void { C c = new C(0); c.half = 10; Console.println(\"{c.raw} {c.half}\"); }"); // 5 5
                                                                                                                                                                                                                                                    // HANDLE semantics through a hook: set via one binding, observe via the alias.
    agree("import Core.Console; class C { constructor(public mutable int raw) {} int v { get => this.raw; set(int n) { this.raw = n; } } } function main()-> void { C c = new C(1); C d = c; c.v = 99; Console.println(\"{d.v}\"); }"); // 99
                                                                                                                                                                                                                                        // A float computed property with exactly-representable values (Celsius↔Fahrenheit round-trip).
    agree("import Core.Console; class Temp { constructor(public mutable float celsius) {} float fahrenheit { get => this.celsius * 9.0 / 5.0 + 32.0; set(float f) { this.celsius = (f - 32.0) * 5.0 / 9.0; } } } function main()-> void { Temp t = new Temp(100.0); Console.println(\"{t.fahrenheit}\"); t.fahrenheit = 32.0; Console.println(\"{t.celsius}\"); }");
    // 212 then 0
}

#[test]
fn mutation_clone_with_agrees() {
    // M-mut.4a: `obj with { f = e }` — fresh instance, source unchanged, byte-identical on both.
    agree("import Core.Console; class P { constructor(public int x, public int y) {} } function main()-> void { P p = new P(1, 2); P q = p with { x = 9 }; Console.println(\"{p.x} {p.y} {q.x} {q.y}\"); }"); // 1 2 9 2
    agree("import Core.Console; class P { constructor(public int x, public int y) {} } function main()-> void { P p = new P(1, 2); P q = p with { x = 7, y = 8 }; Console.println(\"{q.x} {q.y}\"); }"); // 7 8
                                                                                                                                                                                                         // A method works on the cloned instance (the clone is a real instance; the ctor was not re-run).
    agree("import Core.Console; class P { constructor(public int x, public int y) {} function sum() -> int { return this.x + this.y; } } function main()-> void { P p = new P(1, 2); P q = p with { x = 10 }; Console.println(\"{q.sum()}\"); }"); // 12
                                                                                                                                                                                                                                                   // The override value may reference the source's own fields.
    agree("import Core.Console; class P { constructor(public int x, public int y) {} } function main()-> void { P p = new P(3, 4); P q = p with { x = p.x + p.y }; Console.println(\"{q.x} {q.y}\"); }");
    // 7 4
}

#[test]
fn mutation_condition_loops_agree() {
    // M-mut.3: while / do-while / C-for / while-let / break / continue, byte-identical on both.
    // Plain while accumulator.
    agree("import Core.Console; function main()-> void { mutable int i = 0; mutable int s = 0; while (i < 4) { s += i; i += 1; } Console.println(\"{s}\"); }"); // 6
                                                                                                                                                                // do-while runs the body once even when the condition is false up front.
    agree("import Core.Console; function main()-> void { mutable int n = 10; do { Console.println(\"once\"); n += 1; } while (n < 5); }");
    // continue skips, break stops.
    agree("import Core.Console; function main()-> void { mutable int i = 0; mutable int hit = 0; while (true) { i += 1; if (i == 2) { continue; } if (i >= 5) { break; } hit += 1; } Console.println(\"{hit}\"); }"); // i=1,3,4 → 3
                                                                                                                                                                                                                      // C-style for with continue + break.
    agree("import Core.Console; function main()-> void { mutable int sum = 0; for (mutable int k = 0; k < 6; k++) { if (k == 1) { continue; } if (k == 5) { break; } sum += k; } Console.println(\"{sum}\"); }"); // 0+2+3+4=9
                                                                                                                                                                                                                  // Nested C-for: an inner break exits only the inner loop.
    agree("import Core.Console; function main()-> void { mutable int t = 0; for (mutable int a = 0; a < 3; a += 1) { for (mutable int b = 0; b < 9; b += 1) { if (b == 2) { break; } t += 1; } } Console.println(\"{t}\"); }"); // 3*2=6
                                                                                                                                                                                                                                // while-let drains an optional.
    agree("import Core.Console; function main()-> void { mutable int? o = 7; while (var v = o) { Console.println(\"{v}\"); o = null; } Console.println(\"done\"); }");
    // break inside a for-in (the existing range loop) exits it.
    agree("import Core.Console; function main()-> void { mutable int last = 0; for (int x in 1..=10) { if (x == 4) { break; } last = x; } Console.println(\"{last}\"); }"); // 3
                                                                                                                                                                            // continue inside a for-in skips one iteration.
    agree("import Core.Console; function main()-> void { mutable int s = 0; for (int x in 1..=5) { if (x == 3) { continue; } s += x; } Console.println(\"{s}\"); }"); // 1+2+4+5=12
                                                                                                                                                                      // for(;;) terminated by break.
    agree("import Core.Console; function main()-> void { mutable int c = 0; for (;;) { c += 1; if (c == 3) { break; } } Console.println(\"{c}\"); }");
    // 3
}

#[test]
fn named_fn_ref_as_value_agrees() {
    // named fn defined BEFORE use
    agree("import Core.Console; function dbl(int x)->int{return x*2;} function twice(int x,(int)->int f)->int{return f(f(x));} function main()-> void { Console.println(\"{twice(2, dbl)}\"); }"); // 8
                                                                                                                                                                                                   // named fn defined AFTER use (forward reference)
    agree("import Core.Console; function apply(int x,(int)->int f)->int{return f(x);} function callsLater(int n)->int{ return apply(n, bump); } function bump(int x)->int{return x+5;} function main()-> void { Console.println(\"{callsLater(10)}\"); }");
    // 15
    // A bare named function bound to a `var`, then called THROUGH the local. The compiler infers
    // the local's `CTy::Fn` from the named-fn reference so the call dispatches via `CallValue`
    // (without the inference the VM rejected `f(5)` as "not a function").
    agree("import Core.Console; function dbl(int x)->int{return x*2;} function main()-> void { var f=dbl; Console.println(\"{f(5)}\"); }");
    // 10
}

#[test]
fn transpiles_lambda_literal_call_target() {
    let php = transpile_ok("package Main; import Core.Console; function main()-> void { Console.println(\"{3 |> fn(int v) => v + 100}\"); }");
    // T6: `v` (int param) + `100` (int literal) → native `+`.
    assert!(php.contains("(fn($v) => $v + 100)(3)"), "{php}");
}

#[test]
fn call_of_general_expression_callee_agrees_and_transpiles() {
    // Calling the result of a call — `adder()(41)` — a function-valued callee that is neither an
    // identifier, member, nor lambda literal. The checker accepts it and the interpreter ran it;
    // this guards the VM compiler + transpiler, which previously rejected it ("unsupported call
    // target") — a three-backend inconsistency on the byte-identity spine (Wave 1.4 audit).
    let src =
        "import Core.Console; function adder() -> (int) -> int { return fn(int x) => x + 1; } \
               function main()-> void { Console.println(\"{adder()(41)}\"); }";
    agree(src); // run ≡ runvm  → 42
    let php = transpile_ok(&with_pkg(src));
    assert!(php.contains("(adder())(41)"), "{php}");
}

#[test]
fn higher_order_natives_agree() {
    // map / filter / reduce with inline lambdas (results shown via List.sum — PHP can't echo arrays).
    agree("import Core.Console; import Core.List; function main()-> void { var d=List.map([1,2,3], fn(int x)=>x*2); Console.println(\"{List.sum(d)}\"); }"); // 12
    agree("import Core.Console; import Core.List; function main()-> void { var e=List.filter([1,2,3,4], fn(int x)=>x%2==0); Console.println(\"{List.sum(e)}\"); }"); // 6
    agree("import Core.Console; import Core.List; function main()-> void { Console.println(\"{List.reduce([1,2,3,4], 1, fn(int a,int x)=>a*x)}\"); }"); // 24
                                                                                                                                                        // A lambda capturing an enclosing local, passed to a native (capture window parity, invariant #8).
    agree("import Core.Console; import Core.List; function main()-> void { var k=10; var s=List.map([1,2,3], fn(int x)=>x*k); Console.println(\"{List.sum(s)}\"); }"); // 60
                                                                                                                                                                       // A bare NAMED function reference (zero-capture closure) passed straight to a native.
    agree("import Core.Console; import Core.List; function dbl(int x)->int{return x*2;} function main()-> void { var d=List.map([1,2,3], dbl); Console.println(\"{List.sum(d)}\"); }"); // 12
                                                                                                                                                                                        // RE-ENTRANCY: a native called from inside another native's closure (map nested in reduce's fn).
    agree("import Core.Console; import Core.List; function main()-> void { Console.println(\"{List.reduce([1,2,3], 0, fn(int a,int x)=>a + List.sum(List.map([x], fn(int y)=>y*y)))}\"); }");
    // 14
}

#[test]
fn higher_order_native_closure_fault_agrees() {
    // A fault raised *inside* a closure run by a native must propagate byte-identically on both
    // backends (interpreter `call_closure` ⇄ VM re-entrant `call_closure_value`). Can't be a runnable
    // example (every example must produce identical Ok output) — lives here as a fault-parity case.
    agree_err("import Core.Console; import Core.List; function main()-> void { var d=List.map([1,2,3], fn(int x)=>x/0); Console.println(\"{List.sum(d)}\"); }");
    // DivZero on both
}

#[test]
fn transpiles_higher_order_natives() {
    let php = transpile_ok("package Main; import Core.Console; import Core.List; function main()-> void { var d=List.map([1,2,3], fn(int x)=>x*2); var e=List.filter(d, fn(int x)=>x>2); Console.println(\"{List.reduce(e, 0, fn(int a,int x)=>a+x)}\"); }");
    assert!(php.contains("array_map(fn($x) => $x * 2,"), "{php}");
    assert!(php.contains("array_values(array_filter("), "{php}");
    assert!(php.contains("array_reduce("), "{php}");
}

#[test]
fn generic_methods_agree() {
    // A generic method (`<T>` on a method of a non-generic class) inferred from arguments must run
    // byte-identically on both backends — the type variable is erased before either backend, like a
    // generic free function (M-RT generics-all). `identity` reused at three concrete types.
    agree("import Core.Console; class U { function id<T>(T x)->T { return x; } } function main()-> void { var u=new U(); Console.println(\"{u.id(7)} {u.id(\\\"hi\\\")} {u.id(true)}\"); }"); // 7 hi true
                                                                                                                                                                                              // `T` inferred from a `List<T>` argument; the fallback shares it.
    agree("import Core.Console; class U { function firstOr<T>(List<T> xs, T d)->T { for (T x in xs) { return x; } return d; } } function main()-> void { var u=new U(); Console.println(\"{u.firstOr([10,20], -1)} {u.firstOr([], 99)}\"); }"); // 10 99
                                                                                                                                                                                                                                                // A type parameter inside a function-typed parameter, and the closure invoked in the method body
                                                                                                                                                                                                                                                // (exercises the VM's re-entrant closure path from inside a generic method).
    agree("import Core.Console; class U { function applyTwice<T>(T x, (T)->T f)->T { return f(f(x)); } } function main()-> void { var u=new U(); Console.println(\"{u.applyTwice(5, fn(int v)=>v+1)}\"); }");
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
           function main()-> void { Console.println(d(42)); Console.println(d(\"hi\")); Console.println(d(true)); }");
    // Arity overloads.
    agree(
        "import Core.Console; \
           function add(int a)->int { return a; } \
           function add(int a, int b)->int { return a+b; } \
           function main()-> void { Console.println(\"{add(5)} {add(5,6)}\"); }",
    );
    // Class + interface overloads with most-specific dispatch: a `Circle` value picks `area(Circle)`,
    // a `Square` (only a `Shape`) picks the `area(Shape)` fallback — same choice on both backends.
    agree("import Core.Console; \
           interface Shape {} \
           class Circle implements Shape { constructor(public int r) {} } \
           class Square implements Shape { constructor(public int s) {} } \
           function area(Circle c)->int { return c.r*c.r*3; } \
           function area(Shape s)->int { return 0; } \
           function main()-> void { Circle c=new Circle(2); Square q=new Square(4); Console.println(\"{area(c)} {area(q)}\"); }");
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
           function main()-> void { Printer p=new Printer(\"P\"); \
             Console.println(p.show(7)); Console.println(p.show(\"hi\")); Console.println(p.show(new Circle(3))); }");
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
               function main()-> void { Both b=new Both(0); Console.println(\"{pick(b, b)}\"); }",
    );
}

#[test]
fn transpiles_generic_method_to_mixed() {
    // A generic method erases to `mixed`-typed PHP (params and return), exactly as a generic free
    // function does; `List<T>` → `array`, `(T)->T` → `\Closure`. No type variable reaches the output.
    let php = transpile_ok("package Main; class U { function id<T>(T x)->T { return x; } function applyTwice<T>(T x, (T)->T f)->T { return f(f(x)); } } function main()-> void { var u=new U(); var n = u.id(1); var m = u.applyTwice(2, fn(int v)=>v+1); }");
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
    agree("import Core.Console; function mk(int a)->(int)->int{ return fn(int b)=>a+b; } function main()-> void { var f=mk(10); Console.println(\"{f(5)}\"); }"); // 15
                                                                                                                                                                  // Escaping closure capturing a `var` local of the enclosing function (not a param).
    agree("import Core.Console; function mk(int z)->(int)->int{ var a=z*2; return fn(int b)=>a+b; } function main()-> void { var f=mk(10); Console.println(\"{f(5)}\"); }"); // 25
                                                                                                                                                                             // Lexically NESTED lambda: a lambda whose body defines and returns another capturing lambda.
    agree("import Core.Console; function mk(int a)->(int)->int{ var outer=fn(int b)->(int)->int{ return fn(int c)=>a+b+c; }; return outer(a); } function main()-> void { var f=mk(100); Console.println(\"{f(11)}\"); }"); // 100+100+11 = 211
                                                                                                                                                                                                                           // Two functions defined before `main`, the first bearing a lambda — exercises the entry-index
                                                                                                                                                                                                                           // and Op::Call stability under the trailing-lambda block (a regression would call the wrong fn).
    agree("import Core.Console; function a(int x)->int{ var inc=fn(int n)=>n+1; return inc(x); } function b(int x)->int{ return x*10; } function main()-> void { Console.println(\"{a(4)} {b(4)}\"); }");
    // 5 40
    // A lambda inside a METHOD body (capturing a method param) — the constructor/method compile
    // loops number their lambdas from the same trailing block, so this guards that path too.
    agree("import Core.Console; class Box { constructor(public int v) {} function scaledBy(int k)->int{ var f=fn(int x)->int{ return x*k; }; return f(this.v); } } function main()-> void { var b=new Box(7); Console.println(\"{b.scaledBy(3)}\"); }");
    // 21
}

#[test]
fn html_literal_sugar_agrees() {
    // Core.Html Wave 3 — `html"…"` desugars to html.raw/html.text/html.concat, all of which are
    // already byte-identical across backends, so the sugar inherits parity. (run ≡ runvm here; the
    // glob test below adds run ≡ php on examples/guide/html.phg.)
    // A string hole auto-escapes; literal chunks pass through.
    agree(
        r#"import Core.Console; import Core.Html; function main()-> void { var n="a&<b>"; Console.println(Html.render(html"<h1>{n}</h1>")); }"#,
    ); // <h1>a&amp;&lt;b&gt;</h1>
       // A primitive hole stringifies then escapes.
    agree(
        r#"import Core.Console; import Core.Html; function main()-> void { var n=42; Console.println(Html.render(html"<p>{n}</p>")); }"#,
    ); // <p>42</p>
       // An Html hole embeds verbatim (no double-escape).
    agree(
        r#"import Core.Console; import Core.Html; function main()-> void { var inner=Html.text("a&b"); Console.println(Html.render(html"<div>{inner}</div>")); }"#,
    ); // <div>a&amp;b</div>
       // A nested html"…" as an Html hole — recursion through resolve_html.
    agree(
        r#"import Core.Console; import Core.Html; function main()-> void { var n="x"; var inner=html"<b>{n}</b>"; Console.println(Html.render(html"<p>{inner}</p>")); }"#,
    ); // <p><b>x</b></p>
       // Multi-line literal (spans lines for free, like a plain string).
    agree("import Core.Console; import Core.Html; function main()-> void { var n=\"z\"; Console.println(Html.render(html\"<ul>\n  <li>{n}</li>\n</ul>\")); }");
    // A literal with no holes is still Html.
    agree(
        r#"import Core.Console; import Core.Html; function main()-> void { Console.println(Html.render(html"<hr/>")); }"#,
    ); // <hr/>
}

#[test]
fn html_literal_bad_hole_rejected_by_both() {
    // A non-renderable hole type (an enum value) is `E-HTML-HOLE` — rejected on both backends.
    agree_err(
        r#"import Core.Html; enum E { A() } function main()-> void { var p = html"<h1>{new A()}</h1>"; }"#,
    );
    // `html"…"` without `import Core.Html;` is `E-HTML-IMPORT` — rejected on both backends.
    agree_err(r#"function main()-> void { var p = html"<h1>x</h1>"; }"#);
}

#[test]
fn transpiles_html_literal_to_kernel_calls() {
    // The desugaring targets only Wave-1/2 natives, so the PHP is the kernel emission: literal
    // chunks as strings, a string hole through htmlspecialchars(ENT_QUOTES), all joined by implode.
    let php = transpile_ok(
        r#"package Main; import Core.Console; import Core.Html; function main()-> void { var n="x"; Console.println(Html.render(html"<h1>{n}</h1>")); }"#,
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
        r#"import Core.Console; import Core.Html; function main()-> void { Console.println(Html.render(Html.a([Html.attr("href","/?x=1&y=2")],[Html.text("A & B")]))); }"#,
    ); // <a href="/?x=1&amp;y=2">A &amp; B</a>
       // Empty attr list accepted in call-arg position; tags nest.
    agree(
        r#"import Core.Console; import Core.Html; function main()-> void { Console.println(Html.render(Html.ul([],[Html.li([],[Html.text("x")])]))); }"#,
    ); // <ul><li>x</li></ul>
       // A void (self-closing) element.
    agree(
        r#"import Core.Console; import Core.Html; function main()-> void { Console.println(Html.render(Html.hr([]))); }"#,
    ); // <hr/>
       // A tag helper and the equivalent el() call produce identical bytes.
    agree(
        r#"import Core.Console; import Core.Html; function main()-> void { Console.println(Html.render(Html.p([],[Html.text("hi")]))); Console.println(Html.render(Html.el("p",[],[Html.text("hi")]))); }"#,
    ); // <p>hi</p>\n<p>hi</p>
}

#[test]
fn transpiles_named_tag_to_baked_php() {
    // A named tag erases to the same baked closure the kernel uses, with the tag compiled in (no $t).
    let php = transpile_ok(
        r#"package Main; import Core.Console; import Core.Html; function main()-> void { Console.println(Html.render(Html.div([],[Html.text("x")]))); }"#,
    );
    assert!(php.contains("'<div'"), "{php}");
    assert!(php.contains("'</div>'"), "{php}");
}

// ── M7: the PHP oracle — the third correctness leg ───────────────────────────────────────────────
// `run ≡ runvm` is gated by every test above. This gates `run ≡ php` (⇒ all three byte-identical):
// the transpiled PHP, executed by a real `php`, must print exactly what the interpreter prints.
// Gating contract (closes P0-ROOT — no more self-skip-to-PASS):
//   PHORJ_REQUIRE_PHP=1 → a missing php FAILS the test (CI / enforced mode).
//   unset/empty          → a missing php skips LOUDLY (dev convenience), never a silent green.
// Optional PHORJ_PHP=<path> overrides the php binary (non-PATH installs).
// Scope: stdout-parity over runnable (`Ok`) examples + projects. Fault classes (overflow, OOB,
// range-too-large) stay `run ≡ runvm` `agree_err` above — they are not runnable examples.

/// Resolve the php binary: `PHORJ_PHP` override, else `php` on PATH if `--version` succeeds.
fn php_bin() -> Option<String> {
    let cand = std::env::var("PHORJ_PHP").unwrap_or_else(|_| "php".to_string());
    let ok = Command::new(&cand)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    ok.then_some(cand)
}

/// The fails-not-skips gate. `Some(php)` ⇒ run; `None` ⇒ caller returns (loud skip). Under
/// `PHORJ_REQUIRE_PHP=1` a missing php panics instead of skipping.
fn php_or_gate(test: &str) -> Option<String> {
    if let Some(p) = php_bin() {
        return Some(p);
    }
    assert!(
        std::env::var("PHORJ_REQUIRE_PHP").as_deref() != Ok("1"),
        "{test}: php required (PHORJ_REQUIRE_PHP=1) but not found on PATH or $PHORJ_PHP"
    );
    eprintln!("SKIP {test}: php not found — set PHORJ_REQUIRE_PHP=1 to make this a failure");
    None
}

/// The `php` flags for a hermetic oracle run. `-n` ignores php.ini (so a machine's ini can't perturb
/// output), but `-n` *also* disables ini-loaded **shared** extensions — and decimal transpile emits
/// BCMath (`bcadd`/`bcmul`/…), which is a shared extension on most builds (notably CI's `setup-php`,
/// where it lives in `conf.d/` that `-n` skips). `-n` still honors `-d` directives, so we load bcmath
/// explicitly. Probe once: if bcmath is already present under bare `-n` (compiled-in, as on phpbrew
/// builds) no `-d` is needed; otherwise add `-d extension=bcmath`, which loads the shared `.so` from
/// the compiled-default extension_dir. This is the one deliberate exception to the "`-n` =
/// extension-free" contract — decimal money math fundamentally needs arbitrary-precision integers, and
/// BCMath is PHP's standard provider. (Fixes the CI oracle failure on `php -n` without bcmath.)
fn php_n_args(php: &str) -> &'static [&'static str] {
    static BUILTIN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let has_builtin = *BUILTIN.get_or_init(|| {
        Command::new(php)
            .args(["-n", "-r", "exit(extension_loaded('bcmath') ? 0 : 1);"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    });
    if has_builtin {
        &["-n"]
    } else {
        // `display_errors=stderr` keeps stdout clean even if the `.so` can't be found on some build —
        // a startup warning then goes to stderr (and an actually-missing bcmath still fatals loudly),
        // never polluting the stdout the oracle compares.
        &[
            "-n",
            "-d",
            "display_errors=stderr",
            "-d",
            "extension=bcmath",
        ]
    }
}

/// Write `php_src` to a per-label temp file (no collision under parallel `cargo test`), run it with
/// `php -n` (ignore php.ini → hermetic; notices go to stderr, we read stdout), return its stdout.
fn run_php(php: &str, php_src: &str, label: &str) -> String {
    let safe: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = std::env::temp_dir().join(format!("phorj_oracle_{safe}.php"));
    std::fs::write(&path, php_src).expect("write temp php");
    let out = Command::new(php)
        .args(php_n_args(php))
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

/// Batch-1 B: `main`'s `int` return is the process exit code on ALL three legs — `run`, `runvm`, and
/// the transpiled PHP — with byte-identical stdout. `run_php` asserts a zero exit, so this drives php
/// directly to read the non-zero status.
#[test]
fn main_exit_code_is_byte_identical_across_backends() {
    let src = "package Main;\nimport Core.Console;\n\
               function main(): int {\n    Console.println(\"x\");\n    return 7;\n}";
    let run = cmd_run_exit(src).expect("run ok");
    let runvm = cmd_runvm_exit(src).expect("runvm ok");
    assert_eq!(run, runvm, "run vs runvm (stdout, exit)");
    assert_eq!(run, ("x\n".to_string(), 7));
    if let Some(php) = php_or_gate("main_exit_code") {
        let php_src = cli::cmd_transpile(src).expect("transpile ok");
        let path = std::env::temp_dir().join("phorj_exitcode_oracle.php");
        std::fs::write(&path, &php_src).expect("write php");
        let out = Command::new(&php)
            .args(php_n_args(&php))
            .arg(&path)
            .output()
            .expect("spawn php");
        let _ = std::fs::remove_file(&path);
        assert_eq!(out.status.code(), Some(7), "php exit code\n{php_src}");
        assert_eq!(
            String::from_utf8(out.stdout).expect("utf-8"),
            "x\n",
            "php stdout\n{php_src}"
        );
    }
}

/// Batch-1 D: a class-`static` `main(): int` entry is byte-identical across all three legs, and its
/// `int` return is the process exit code. The transpiled PHP bootstraps `App::main()` (not
/// `\Main\main()`); `run_php` asserts exit-0, so php is driven directly to read the non-zero status.
#[test]
fn class_static_main_exit_code_is_byte_identical_across_backends() {
    let src = "package Main;\nimport Core.Console;\n\
               class App {\n  static function main(): int {\n    Console.println(\"x\");\n    return 7;\n  }\n}";
    let run = cmd_run_exit(src).expect("run ok");
    let runvm = cmd_runvm_exit(src).expect("runvm ok");
    assert_eq!(run, runvm, "run vs runvm (stdout, exit)");
    assert_eq!(run, ("x\n".to_string(), 7));
    if let Some(php) = php_or_gate("class_static_main_exit_code") {
        let php_src = cli::cmd_transpile(src).expect("transpile ok");
        let path = std::env::temp_dir().join("phorj_classmain_oracle.php");
        std::fs::write(&path, &php_src).expect("write php");
        let out = Command::new(&php)
            .args(php_n_args(&php))
            .arg(&path)
            .output()
            .expect("spawn php");
        let _ = std::fs::remove_file(&path);
        assert_eq!(out.status.code(), Some(7), "php exit code\n{php_src}");
        assert_eq!(
            String::from_utf8(out.stdout).expect("utf-8"),
            "x\n",
            "php stdout\n{php_src}"
        );
    }
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
function main() -> void {
    Duck d = new Duck();
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
        // Quarantined (ambient-environment) example — not byte-identity-gated against PHP (see
        // `uses_impure_native`); covered by tests/process.rs under a controlled environment.
        if uses_impure_native(&src) {
            continue;
        }
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

/// P0-1/P0-2/P0-4/QW-13: the emitter handles `/`, `%`, interpolation, compound operands, and ranges
/// without diverging from the Rust backends. Since T6, statically-typed operands emit the *native*
/// PHP construct (the divergence-safe choice — `intdiv`/`fmod`/inline-ternary), with the runtime
/// helper kept only as the fallback for operands of unknown kind. This pins the *emitted PHP shape*
/// (the oracle above pins the *runtime behavior* over the examples); together they make a P0
/// regression impossible to ship silently. `run ≡ runvm` for these was always correct — php-leg-only.
#[test]
fn m7_emitter_uses_correctness_helpers() {
    // P0-1 (int `/` ⇒ `intdiv`, never bare `/`) + P0-4 (int `%` ⇒ native `%`) — both literal-int.
    let div = transpile_ok(
        "package Main; import Core.Console; function main()-> void { Console.println(\"{7 / 2}\"); Console.println(\"{5 % 2}\"); }",
    );
    assert!(div.contains("intdiv(7, 2)"), "{div}");
    assert!(div.contains("5 % 2"), "{div}");
    assert!(
        !div.contains("__phorj_div") && !div.contains("__phorj_rem"),
        "{div}"
    );
    // P0-4 float path: float `%` ⇒ `fmod` (PHP's `%` int-casts — the divergence); float `/` ⇒ native.
    let fl = transpile_ok(
        "package Main; import Core.Console; function main()-> void { float a=5.5; float b=2.0; Console.println(\"{a % b}\"); Console.println(\"{a / b}\"); }",
    );
    assert!(fl.contains("fmod($a, $b)"), "{fl}");
    assert!(fl.contains("$a / $b"), "{fl}");
    // Helper fallback: when an operand's kind genuinely can't be resolved — here an *erased generic*
    // result, which is permanently `mixed` by design — the div helper is emitted and used, never a
    // bare `/`. The helper branches on operand types at PHP-runtime, so it stays correct (intdiv for
    // ints). This guards that the safe fallback survives all the T6 specialization layers.
    let fb = transpile_ok(
        "package Main; import Core.Console; function id<T>(T x) -> T { return x; } function main()-> void { Console.println(\"{id(7) / id(2)}\"); }",
    );
    assert!(
        fb.contains("__phorj_div(id(7), id(2))")
            && fb.contains("function __phorj_div")
            && fb.contains("intdiv"),
        "{fb}"
    );
    // P0-3: a bool interpolation hole renders `"true"/"false"` inline (PHP's `(string)bool` ⇒ `1`/``).
    let b = transpile_ok(
        "package Main; import Core.Console; function main()-> void { Console.println(\"{1 < 2}\"); }",
    );
    assert!(b.contains("\"true\" : \"false\""), "{b}");
    // P0-2: a compound operand keeps its grouping parens (no PHP re-association).
    let p = transpile_ok(
        "package Main; import Core.Console; function main()-> void { int a=1; int b=2; int c=3; Console.println(\"{a - (b - c)}\"); Console.println(\"{!(a < b)}\"); }",
    );
    assert!(p.contains("$a - ($b - $c)"), "{p}");
    assert!(p.contains("!($a < $b)"), "{p}");
    // QW-13: ranges route through the empty/reversed-safe helper (PHP range() descends; Phorj ⇒ []).
    let r = transpile_ok(
        "package Main; import Core.Console; function main()-> void { for (int i in 5..2) { Console.println(\"{i}\"); } }",
    );
    assert!(r.contains("__phorj_range(5, 2, false)"), "{r}");
}

/// P0-1: integer division truncates toward zero on both backends, with negative operands. (The php
/// leg is gated by the oracle over the division-bearing examples.)
#[test]
fn m7_int_division_truncates_toward_zero() {
    let src = "import Core.Console; function main()-> void { Console.println(\"{7 / 2} {-7 / 2} {7 / -2} {-7 / -2}\"); }";
    assert_eq!(cmd_run(&with_pkg(src)).as_deref(), Ok("3 -3 -3 3\n"));
    agree(src);
}

/// P1-#9: a range too wide to materialize faults cleanly on BOTH backends (`RangeTooLarge`) instead
/// of OOM-aborting (exit 101). Exclusive and inclusive forms both guard; the cap check precedes any
/// allocation, so the test is fast.
#[test]
fn m7_large_range_faults_identically() {
    agree_err(
        "import Core.Console; function main()-> void { for (int i in 0..2000000000) { Console.println(\"{i}\"); } }",
    );
    agree_err("import Core.Console; function main()-> void { var xs = 0..=2000000000; Console.println(\"{xs[0]}\"); }");
    // The exactly-at-cap boundary is also a fault (span >= MAX_RANGE_LEN), while a small range is fine.
    agree(
        "import Core.Console; function main()-> void { var xs = 0..1000; Console.println(\"{xs[999]}\"); }",
    );
}

/// Divergence-class edge: `i64::MIN / -1` overflows i64 — both backends fault (via the checked
/// `int_div` kernel) rather than panicking (EV-7). PHP's `intdiv(PHP_INT_MIN, -1)` likewise throws,
/// so the helper matches; it's a fault case, not a runnable example, so it lives here, not the oracle.
#[test]
fn m7_int_min_div_neg_one_faults_identically() {
    agree_err(
        "import Core.Console; function main()-> void { int x = -9223372036854775807 - 1; Console.println(\"{x / -1}\"); }",
    );
}

/// M-faults 2a: the fault intrinsics crash byte-identically on both backends (single-sourced
/// `FaultMsg` body → same `FaultKind::Panic`). `assert(true)` is a no-op, so the program completes.
#[test]
fn faults_panic_intrinsics_agree() {
    agree_err(r#"function main()-> void { panic("boom"); }"#);
    agree_err("function main()-> void { todo(); }");
    agree_err("function main()-> void { unreachable(); }");
    agree_err(r#"function main()-> void { assert(2 < 1, "nope"); }"#);
    agree_err("function main()-> void { assert(false); }");
    agree(
        r#"import Core.Console; function main()-> void { assert(1 < 2, "ok"); Console.println("done"); }"#,
    );
}

/// M-faults 2a: a `never`-typed `panic` at the tail of a value-returning function satisfies
/// return-on-all-paths (the totality engine treats it as diverging), and faults identically.
#[test]
fn never_intrinsic_satisfies_return_totality() {
    agree_err(
        r#"function bad() -> int { panic("never returns"); } function main()-> void { var x = bad(); }"#,
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
function main() -> void { P p = new P("x"); if (p instanceof Error) { } }"#
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
function main() -> void { BadInput e = new BadInput("bad input"); Console.println(e.message); }"#,
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
         function parse(int n) -> int throws E1 {{ if (n < 0) {{ throw new E1(\"neg\"); }} return n + 1; }} \
         function main() -> void {{ \
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
           try {{ if (n < 0) {{ throw new E1(\"inner\"); }} return n; }} \
           finally {{ Console.println(\"fin {{n}}\"); }} \
         }} \
         function main() -> void {{ \
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
           if (n == 1) {{ throw new E1(\"one\"); }} if (n == 2) {{ throw new E2(\"two\"); }} return n; \
         }} \
         function main() -> void {{ for (int i in [1, 2, 3]) {{ \
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
         function main() -> void {{ for (int i in [1, 2, 3, 4]) {{ \
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
         function f() -> int throws E1 {{ throw new E1(\"x\"); }} \
         function g() -> int throws E1 {{ return f()?; }} \
         function main() -> void {{ try {{ var n = g(); }} catch (E1 e) {{ Console.println(\"g threw {{e.message}}\"); }} }}"
    ));
}

#[test]
fn panic_bypasses_catch_on_both_backends() {
    // A `Runtime` fault (division by zero) is NOT a catchable `throw`: it passes straight through an
    // enclosing `catch` and aborts identically on both backends (panics are uncatchable by design).
    agree_err(&format!(
        "{ERR_HDR} \
         function main() -> void {{ var xs = [1, 0, 2]; \
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
function main() -> void { Console.println(new Crier().greet()); }",
        "hi\n",
        "s8_trait_method_reuse",
    );
}

#[test]
fn s8_trait_mutable_field_is_byte_identical() {
    // M-RT S8 T2: a trait carries `mutable` instance state; the using class sets it in its ctor and a
    // trait method mutates it. Field access is by name, so the flattened field works on both backends.
    agree_out_php(
        "import Core.Console;
trait Counter { mutable int n; function bump() -> void { this.n = this.n + 1; } function read() -> int { return this.n; } }
class C { use Counter; constructor() { this.n = 0; } }
function main() -> void { C c = new C(); c.bump(); c.bump(); c.bump(); Console.println(\"{c.read()}\"); }",
        "3\n",
        "s8_trait_mutable_field",
    );
}

#[test]
fn s8_trait_static_is_per_using_class_copy() {
    // M-RT S8 T2: a trait `static` field is a PER-USING-CLASS copy (PHP `use` semantics) — each class
    // gets its own `Class.field`. Byte-identical across backends and real PHP.
    agree_out_php(
        "import Core.Console;
trait Counted { static mutable int total = 0; }
class E { use Counted; }
class F { use Counted; }
function main() -> void { E.total = 5; F.total = 9; Console.println(\"{E.total} {F.total}\"); }",
        "5 9\n",
        "s8_trait_static_per_class",
    );
}

#[test]
fn s8_trait_private_method_is_byte_identical() {
    // M-RT S8 T2: a `private` trait method is flattened with its visibility and callable by a sibling
    // trait method; the transpiler emits it `private` inside the native trait.
    agree_out_php(
        "import Core.Console;
trait Loud { private function amp(string s) -> string { return \"{s}!\"; } function shout(string s) -> string { return this.amp(s); } }
class C { use Loud; }
function main() -> void { Console.println(new C().shout(\"hi\")); }",
        "hi!\n",
        "s8_trait_private_method",
    );
}

#[test]
fn s8_trait_constructor_promotion_is_byte_identical() {
    // M-RT S8 T3: a `use`d trait's constructor (pure promotion) becomes the using class's ctor; PHP
    // auto-inherits the trait's __construct. Byte-identical across backends and real PHP.
    agree_out_php(
        "import Core.Console;
trait Stamped { constructor(public int id) {} }
class Doc { use Stamped; }
function main() -> void { Doc d = new Doc(7); Console.println(\"{d.id}\"); }",
        "7\n",
        "s8_trait_ctor_promotion",
    );
}

#[test]
fn s8_trait_constructor_body_is_byte_identical() {
    // M-RT S8 T3: a trait ctor with a BODY (deriving a stored field) runs identically; folded into
    // ctor_plan on both backends, emitted as the trait's __construct in PHP.
    agree_out_php(
        "import Core.Console;
trait Paid { mutable int annual; constructor(int monthly) { this.annual = monthly * 12; } }
class Emp { use Paid; }
function main() -> void { Emp e = new Emp(1000); Console.println(\"{e.annual}\"); }",
        "12000\n",
        "s8_trait_ctor_body",
    );
}

#[test]
fn s8_trait_get_hook_is_byte_identical() {
    // M-RT S8 T4: a `use`d trait's property get-hook flattens into the using class; the synthetic
    // `$get` method dispatches on both backends and transpiles to a native PHP 8.4 trait hook.
    agree_out_php(
        "import Core.Console;
trait Labeled { mutable string raw; string display { get => \"<{this.raw}>\"; } }
class Tag { use Labeled; constructor() { this.raw = \"x\"; } }
function main() -> void { Tag t = new Tag(); Console.println(t.display); }",
        "<x>\n",
        "s8_trait_get_hook",
    );
}

#[test]
fn s8_trait_get_set_hook_is_byte_identical() {
    // M-RT S8 T4: a trait get+set hook — the set intercepts the write (doubles it), the get reads back.
    agree_out_php(
        "import Core.Console;
trait Clamped { mutable int raw; int value { get => this.raw; set(int v) { this.raw = v * 2; } } }
class Box { use Clamped; constructor() { this.raw = 0; } }
function main() -> void { Box b = new Box(); b.value = 5; Console.println(\"{b.value}\"); }",
        "10\n",
        "s8_trait_get_set_hook",
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
function main() -> void { Console.println(new Person().hello()); }",
        "Ada\n",
        "s8_trait_abstract_requirement",
    );
}

/// Pattern cluster S5.1 — match-arm guards over an enum: multiple arms share a shape with different
/// `when` conditions (first-match-wins, fall-through on a false guard), and a guard does arithmetic
/// on the bound payload (`n + 1` — the CTy-operand path must specialize identically on the VM).
#[test]
fn match_arm_guards_enum_byte_identical() {
    agree_out_php(
        "import Core.Console;
enum Code { Num(int n) }
function classify(Code c) -> string {
    return match c {
        Num(n) when n + 1 > 500 => \"server\",
        Num(n) when n >= 400 => \"client\",
        Num(n) => \"other ({n})\",
    };
}
function main() -> void {
    Console.println(classify(new Num(503)));
    Console.println(classify(new Num(404)));
    Console.println(classify(new Num(200)));
}",
        "server\nclient\nother (200)\n",
        "match_arm_guards_enum",
    );
}

/// Pattern cluster S5.1 — guards on type-patterns over a union, with a field access in the guard
/// (`c.r > 1.0`). A guarded `Circle` arm and an unguarded `Circle` fallback make the match exhaustive.
#[test]
fn match_arm_guards_union_type_pattern_byte_identical() {
    agree_out_php(
        "import Core.Console;
class Circle { constructor(public float r) {} }
class Square { constructor(public float side) {} }
function describe(Circle | Square sh) -> string {
    return match sh {
        Circle c when c.r > 1.0 => \"big circle\",
        Circle c => \"small circle\",
        Square s => \"square\",
    };
}
function main() -> void {
    Console.println(describe(new Circle(2.0)));
    Console.println(describe(new Circle(0.5)));
    Console.println(describe(new Square(3.0)));
}",
        "big circle\nsmall circle\nsquare\n",
        "match_arm_guards_union",
    );
}

/// Primitives sweep P1 — number-literal formats (hex / binary / octal / underscore separators). The
/// literal's *value* (not its surface form) reaches the AST, so every base collapses to the same
/// integer and stays byte-identical across backends + PHP; the result is also a real arithmetic operand.
#[test]
fn number_literal_formats_byte_identical() {
    agree_out_php(
        "import Core.Console;
function main() -> void {
    int mask = 0xFF;
    int flags = 0b1010;
    int perms = 0o17;
    int big = 1_000_000;
    Console.println(\"{mask} {flags} {perms} {big}\");
    Console.println(\"{mask + flags}\");
}",
        "255 10 15 1000000\n265\n",
        "number_literal_formats",
    );
}

/// Primitives sweep P2 — bitwise operators `& | ^ ~ << >>` (int-only, PHP-identical). Includes the
/// CTy-operand case `(a & b) + 1` (a bitwise result must specialize as an arithmetic operand on the
/// VM) and shift-right `>>` (two adjacent `Gt` in the parser, so nested generics are unaffected).
#[test]
fn bitwise_operators_byte_identical() {
    agree_out_php(
        "import Core.Console;
function main() -> void {
    int a = 0b1100;
    int b = 0b1010;
    Console.println(\"{a & b} {a | b} {a ^ b} {a << 2} {a >> 1} {~a} {(a & b) + 1}\");
}",
        "8 14 6 48 6 -13 9\n",
        "bitwise_operators",
    );
}

/// Primitives sweep P3 — `Console.print` (no trailing newline; space-joins like `println`). Composes
/// with `println` and string interpolation; transpiles to a bare PHP `echo`.
#[test]
fn console_print_byte_identical() {
    agree_out_php(
        "import Core.Console;
function main() -> void {
    Console.print(\"a\");
    Console.print(\"b\");
    Console.println(\"c\");
    Console.print(\"x {1 + 2} \");
    Console.println(\"y\");
}",
        "abc\nx 3 y\n",
        "console_print",
    );
}

/// Pattern cluster S5.2 — struct (named-field) destructuring: shorthand `Circle { r }`, rename
/// `Point { x: px }`, and nesting `Line { from: Point { x, y }, to }`. The instance test reuses
/// `Op::IsInstance` (no new op); each field is read by name. The nested `fx + fy` exercises the CTy
/// operand path (a struct-bound int must be an arithmetic operand on the VM). run ≡ runvm ≡ real PHP.
#[test]
fn struct_destructuring_byte_identical() {
    agree_out_php(
        "import Core.Console;
class Circle { constructor(public float r) {} }
class Square { constructor(public float side) {} }
class Point { constructor(public int x, public int y) {} }
class Line { constructor(public Point from, public Point to) {} }
function areaOf(Circle | Square sh) -> float {
    return match sh { Circle { r } => r, Square { side } => side, };
}
function originSum(Line l) -> int {
    return match l { Line { from: Point { x: fx, y: fy }, to } => fx + fy + to.x, _ => 0, };
}
function main() -> void {
    float a = areaOf(new Circle(2.5));
    float b = areaOf(new Square(4.0));
    int d = originSum(new Line(new Point(1, 2), new Point(10, 20)));
    Console.println(\"a={a} b={b} d={d}\");
}",
        "a=2.5 b=4 d=13\n",
        "struct_destructuring",
    );
}

/// Pattern cluster S5.3 — if-let `when` guard. `if (var u = e when g)` binds an optional and tests a
/// condition on the binding in one header; the then-branch runs only when the bind succeeds AND the
/// guard holds. Parser-desugared to a nested `if` (no new `Op`). run ≡ runvm ≡ real PHP.
#[test]
fn if_let_when_guard_byte_identical() {
    agree_out_php(
        "import Core.Console;
class User { constructor(public string name, public int age) {} }
function lookup(int id) -> User? {
    if (id == 1) { return new User(\"Ada\", 36); }
    if (id == 2) { return new User(\"Bob\", 15); }
    return null;
}
function greet(int id) -> string {
    if (var u = lookup(id) when u.age >= 18 && u.name != \"\") {
        return \"welcome {u.name}\";
    } else {
        return \"denied\";
    }
}
function main() -> void {
    Console.println(greet(1));
    Console.println(greet(2));
    Console.println(greet(3));
}",
        "welcome Ada\ndenied\ndenied\n",
        "if_let_when_guard",
    );
}

/// Pattern cluster S5.3-T3 — early-return flow-narrowing. After a diverging guard
/// `if (!(s instanceof Circle)) { return … }`, the rest of the function sees `s : Circle`. Narrowing
/// is checker-only — this confirms a program relying on it runs byte-identically.
#[test]
fn flow_narrowing_early_return_byte_identical() {
    agree_out_php(
        "import Core.Console;
class Circle { constructor(public float r) {} }
class Square { constructor(public float side) {} }
function dim(Circle | Square s) -> float {
    if (!(s instanceof Circle)) { return s.side; }
    return s.r;
}
function main() -> void {
    float a = dim(new Circle(2.5));
    float b = dim(new Square(4.0));
    Console.println(\"a={a} b={b}\");
}",
        "a=2.5 b=4\n",
        "flow_narrowing_early_return",
    );
}

/// Pattern cluster S5.3 — flow-narrowing (else / negative). After `if (s instanceof Circle)` the
/// else-branch narrows `s` to the remaining union member (`Square`), so the Square-only field reads
/// there. Narrowing is checker-only — this confirms a program relying on it runs byte-identically.
#[test]
fn flow_narrowing_else_byte_identical() {
    agree_out_php(
        "import Core.Console;
class Circle { constructor(public float r) {} }
class Square { constructor(public float side) {} }
function dim(Circle | Square s) -> float {
    if (s instanceof Circle) { return s.r; } else { return s.side; }
}
function main() -> void {
    float a = dim(new Circle(2.5));
    float b = dim(new Square(4.0));
    Console.println(\"a={a} b={b}\");
}",
        "a=2.5 b=4\n",
        "flow_narrowing_else",
    );
}

/// Pattern cluster S5.2-T2 — a nested type pattern inside a variant payload (`W(Circle c)`). The
/// payload (an interface) is matched to a concrete class via the same `Op::IsInstance` test, then the
/// binding's field is read (`c.r + 1.0` exercises the CTy operand path on a nested-pattern binding).
/// A refutable payload doesn't discharge the variant's coverage, so a `_` fallback is required.
/// run ≡ runvm ≡ real PHP.
#[test]
fn nested_type_pattern_in_variant_payload_byte_identical() {
    agree_out_php(
        "import Core.Console;
interface Shape {}
class Circle implements Shape { constructor(public float r) {} }
class Square implements Shape { constructor(public float side) {} }
enum Boxed { W(Shape inner) }
function f(Boxed b) -> float {
    return match b { W(Circle c) => c.r + 1.0, W(Square s) => s.side, _ => 0.0, };
}
function main() -> void {
    float a = f(new W(new Circle(2.5)));
    float b = f(new W(new Square(4.0)));
    Console.println(\"a={a} b={b}\");
}",
        "a=3.5 b=4\n",
        "nested_type_pattern_in_variant_payload",
    );
}

/// Primitives sweep P3.2 — the byte-safe stdlib subset: `Text.startsWith`/`endsWith`/`repeat`,
/// `Math.round` (→ int, half-away-from-zero like PHP's default), and `List.length`. Each erases 1:1
/// to a PHP builtin (`str_starts_with`/`str_ends_with`/`str_repeat`/`(int)round`/`count`). Bools are
/// rendered through an expression-`if` (PHP echoes a bool as `1`/`""`, not `true`/`false`).
#[test]
fn p3_byte_safe_stdlib_byte_identical() {
    agree_out_php(
        "import Core.Console;
import Core.Text;
import Core.Math;
import Core.List;
function main() -> void {
    string sw = if (Text.startsWith(\"hello\", \"he\")) { \"yes\" } else { \"no\" };
    string ew = if (Text.endsWith(\"hello\", \"lo\")) { \"yes\" } else { \"no\" };
    string rep = Text.repeat(\"ab\", 3);
    Console.println(\"sw={sw} ew={ew} rep={rep}\");
    int r1 = Math.round(2.5);
    int r2 = Math.round(2.4);
    int r3 = Math.round(-2.5);
    Console.println(\"round: {r1} {r2} {r3}\");
    List<int> xs = [10, 20, 30];
    int len = List.length(xs);
    Console.println(\"len={len}\");
}",
        "sw=yes ew=yes rep=ababab\nround: 3 2 -3\nlen=3\n",
        "p3_byte_safe_stdlib",
    );
}

#[test]
fn m_num_s2_decimal_div_by_zero_faults_identically() {
    // `Decimal.div` with a zero divisor faults the same way on both backends (the `decimal division
    // by zero` body contains `division by zero`, so it classifies as FaultKind::DivZero). The PHP
    // helper throws the same body — but a fault is not a runnable example (Ok-only rule), so this is a
    // run≡runvm parity check, not a 3-way one.
    agree_err(
        "import Core.Decimal; function main() -> void { decimal r = Decimal.div(10.00d, 0d, 2, new HalfUp()); }",
    );
}

#[test]
fn m_num_s2_decimal_scale_out_of_range_faults_identically() {
    // A negative `scale` faults `decimal scale out of range` on both backends (FaultKind::Other, but
    // the body is byte-identical so `agree_err` is satisfied).
    agree_err(
        "import Core.Decimal; function main() -> void { decimal r = Decimal.div(10.00d, 3d, -1, new HalfUp()); }",
    );
    agree_err(
        "import Core.Decimal; function main() -> void { decimal r = Decimal.round(2.345d, -1, new HalfUp()); }",
    );
}

#[test]
fn m_num_s2_decimal_div_overflow_faults_identically() {
    // A target scale that overflows 10^k before the division faults `decimal overflow` on both.
    agree_err(
        "import Core.Decimal; function main() -> void { decimal r = Decimal.div(1d, 3d, 200, new HalfUp()); }",
    );
}
