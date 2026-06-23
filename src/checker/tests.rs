use super::*;
use crate::lexer::lex;
use crate::parser::Parser;

/// Lex + parse `src` into a Program, panicking on lex/parse failure (tests here only care
/// about type-checking). Auto-prepends the reserved `package Main;` (M5 S1, line-preserving)
/// unless the source already declares a package, so existing checker tests need no per-case
/// edit. Use [`prog_raw`] when a test must exercise the package rules themselves.
fn prog(src: &str) -> Program {
    let src = if src.trim_start().starts_with("package ") {
        src.to_string()
    } else {
        format!("package Main; {src}")
    };
    prog_raw(&src)
}

/// Lex + parse without injecting a package — for tests of the package rules themselves.
fn prog_raw(src: &str) -> Program {
    let tokens = lex(src).expect("lex ok");
    Parser::new(tokens).parse_program().expect("parse ok")
}

/// Type-check `src` and return the errors (empty == well-typed).
fn errors_of(src: &str) -> Vec<Diagnostic> {
    match check(&prog(src)) {
        Ok(_warnings) => Vec::new(),
        Err(e) => e,
    }
}

/// Type-check `src` and return the non-fatal warnings (empty unless a lint fired).
fn warnings_of(src: &str) -> Vec<Diagnostic> {
    check(&prog(src)).unwrap_or_default()
}

/// Type-check a *raw* source (no injected package) and return the errors.
fn errors_of_raw(src: &str) -> Vec<Diagnostic> {
    match check(&prog_raw(src)) {
        Ok(_) => Vec::new(),
        Err(e) => e,
    }
}

// --- M-RT S6: single inheritance ---

#[test]
fn subclass_is_assignable_and_inherits_methods() {
    // S6a.3: Dog <: Animal (assignability) + Dog inherits Animal's method.
    let errs = errors_of(
        "open class Animal { function name() -> string { return \"a\"; } } \
             class Dog extends Animal {} \
             function f() -> string { Animal a = Dog(); return a.name(); } \
             function g() -> string { Dog d = Dog(); return d.name(); }",
    );
    assert!(errs.is_empty(), "expected clean, got {errs:?}");
}

#[test]
fn extending_a_non_open_class_errors() {
    let errs = errors_of("class Animal {} class Dog extends Animal {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-EXTEND-FINAL")),
        "got {errs:?}"
    );
}

#[test]
fn extending_an_unknown_name_errors() {
    let errs = errors_of("class Dog extends Bogus {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-EXTEND-UNKNOWN")),
        "got {errs:?}"
    );
}

#[test]
fn class_extends_cycle_errors() {
    let errs = errors_of("open class A extends B {} open class B extends A {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-MI-CYCLE")),
        "got {errs:?}"
    );
}

#[test]
fn overriding_a_final_method_errors() {
    // S6a.4: Animal.kind is final-by-default; Dog redefining it is E-OVERRIDE-FINAL.
    let errs = errors_of(
        "open class Animal { function kind() -> string { return \"a\"; } } \
             class Dog extends Animal { function kind() -> string { return \"d\"; } }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERRIDE-FINAL")),
        "got {errs:?}"
    );
}

#[test]
fn overriding_an_open_method_is_allowed() {
    // S6a.4: marking the parent method `open` permits the override.
    let errs = errors_of(
        "open class Animal { open function kind() -> string { return \"a\"; } } \
             class Dog extends Animal { function kind() -> string { return \"d\"; } }",
    );
    assert!(errs.is_empty(), "got {errs:?}");
}

#[test]
fn instantiating_an_abstract_class_errors() {
    // S6b.3: an abstract class cannot be constructed.
    let errs = errors_of(
        "abstract class Shape { abstract function area() -> int; } \
             function main() { Shape s = Shape(); }",
    );
    assert!(
        errs.iter()
            .any(|e| e.code == Some("E-ABSTRACT-INSTANTIATE")),
        "got {errs:?}"
    );
}

#[test]
fn concrete_subclass_missing_abstract_impl_errors() {
    // S6b.3: a non-abstract subclass must implement every inherited abstract method.
    let errs = errors_of(
        "abstract class Shape { abstract function area() -> int; } \
             class Blob extends Shape {}",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-ABSTRACT-UNIMPL")),
        "got {errs:?}"
    );
}

#[test]
fn abstract_method_in_concrete_class_errors() {
    // S6b.3: a non-abstract class may not itself declare an abstract method (same check, origin is
    // the class itself).
    let errs = errors_of("class Shape { abstract function area() -> int; }");
    assert!(
        errs.iter().any(|e| e.code == Some("E-ABSTRACT-UNIMPL")),
        "got {errs:?}"
    );
}

#[test]
fn s8_use_unknown_trait_errors() {
    // M-RT S8: `use` must name a declared trait.
    let errs = errors_of("class C { use Nope; } function main() {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-USE-UNKNOWN")),
        "got {errs:?}"
    );
}

#[test]
fn s8_use_a_class_as_trait_errors() {
    // M-RT S8: `use` composes a trait, not a class — naming a class is E-USE-UNKNOWN.
    let errs = errors_of("class Base {} class C { use Base; } function main() {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-USE-UNKNOWN")),
        "got {errs:?}"
    );
}

#[test]
fn s8_instanceof_trait_errors() {
    // M-RT S8: a trait is not a type — it cannot be an `instanceof` target.
    let errs = errors_of(
        "trait T { function f() -> int { return 1; } } \
             class C { use T; } \
             function main() { C c = C(); var b = c instanceof T; }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-INSTANCEOF-TYPE")),
        "got {errs:?}"
    );
}

#[test]
fn s8_trait_as_type_annotation_errors() {
    // M-RT S8: a value cannot be typed as a trait.
    let errs = errors_of(
        "trait T { function f() -> int { return 1; } } \
             function main() { T x = x; }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-USE-AS-TYPE")),
        "got {errs:?}"
    );
}

#[test]
fn s8_unmet_trait_abstract_requirement_errors() {
    // M-RT S8: a using class must satisfy a trait's abstract requirement (reuses E-ABSTRACT-UNIMPL).
    let errs = errors_of(
        "trait Greeter { abstract function name() -> string; } \
             class P { use Greeter; } function main() {}",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-ABSTRACT-UNIMPL")),
        "got {errs:?}"
    );
}

#[test]
fn s8_two_trait_constructors_collide() {
    // M-RT S8 T3: a class composing two ctor-bearing traits (no own ctor) is E-TRAIT-CTOR-COLLISION.
    let errs = errors_of(
        "trait A { constructor(public int a) {} } \
             trait B { constructor(public int b) {} } \
             class C { use A; use B; } function main() {}",
    );
    assert!(
        errs.iter()
            .any(|e| e.code == Some("E-TRAIT-CTOR-COLLISION")),
        "got {errs:?}"
    );
}

#[test]
fn s8_class_ctor_shadows_trait_ctor_warns() {
    // M-RT S8 T3 (D8): a class's own ctor shadows a `use`d trait's ctor — warning, not error.
    let warns = warnings_of(
        "trait T { constructor(public int id) {} } \
             class C { use T; constructor() {} } function main() {}",
    );
    assert!(
        warns
            .iter()
            .any(|w| w.code == Some("W-TRAIT-CTOR-SHADOWED")),
        "got {warns:?}"
    );
}

#[test]
fn s8_trait_ctor_skips_parent_warns() {
    // M-RT S8 T3 (D6): trait ctor wins over an inherited parent ctor — warn the silent skip.
    let warns = warnings_of(
        "open class Base { constructor(public int b) {} } \
             trait T { constructor(public int id) {} } \
             class C extends Base { use T; } function main() {}",
    );
    assert!(
        warns
            .iter()
            .any(|w| w.code == Some("W-TRAIT-CTOR-PARENT-SKIPPED")),
        "got {warns:?}"
    );
}

#[test]
fn concrete_subclass_implementing_abstract_is_ok() {
    // S6b.3: providing the body satisfies the abstract contract — no error.
    let errs = errors_of(
        "abstract class Shape { abstract function area() -> int; } \
             class Square extends Shape { constructor(public int side) {} \
                 function area() -> int { return this.side * this.side; } }",
    );
    assert!(
        !errs
            .iter()
            .any(|e| matches!(e.code, Some("E-ABSTRACT-UNIMPL") | Some("E-OVERRIDE-FINAL"))),
        "got {errs:?}"
    );
}

#[test]
fn open_static_method_errors() {
    // S6b.3: a method cannot be both `open` and `static` (statics are not virtual).
    let errs = errors_of("class C { open static function f() -> int { return 1; } }");
    assert!(
        errs.iter().any(|e| e.code == Some("E-OPEN-STATIC")),
        "got {errs:?}"
    );
}

#[test]
fn unresolved_cross_parent_collision_errors() {
    // S6b.2: two parents each declare `move`; `Duck` neither resolves nor overrides it.
    let errs = errors_of(
        "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer {}",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
        "got {errs:?}"
    );
}

#[test]
fn use_clause_resolves_the_collision() {
    // S6b.2: `use Swimmer.move` picks a winner — no conflict.
    let errs = errors_of(
        "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { use Swimmer.move }",
    );
    assert!(
        !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
        "got {errs:?}"
    );
}

#[test]
fn exclude_clause_resolves_the_collision() {
    // S6b.2: `exclude Flyer.move` drops one source, leaving `move` unambiguous.
    let errs = errors_of(
        "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { exclude Flyer.move }",
    );
    assert!(
        !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
        "got {errs:?}"
    );
}

#[test]
fn child_override_resolves_the_collision() {
    // S6b.2: declaring `move` in the child overrides both parents — no conflict (and the parent
    // methods are `open`, so the override itself is legal).
    let errs = errors_of(
        "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { function move() -> string { return \"d\"; } }",
    );
    assert!(
        !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
        "got {errs:?}"
    );
}

#[test]
fn diamond_shared_base_is_not_a_conflict() {
    // S6b.2: `Mid` reaches `Base.tag` through both arms, but both resolve to the same declaring
    // method — auto-merge, never E-MI-CONFLICT.
    let errs = errors_of(
        "open class Base { open function tag() -> string { return \"b\"; } } \
             open class Left extends Base {} open class Right extends Base {} \
             class Mid extends Left, Right {}",
    );
    assert!(
        !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
        "got {errs:?}"
    );
}

#[test]
fn overriding_a_final_method_of_the_second_parent_errors() {
    // S6b.1: override-finality is checked against *every* parent, not just the first. `Flyer.move`
    // (the second parent) is final-by-default; `Duck` redefining it is E-OVERRIDE-FINAL even
    // though the first parent has no such method.
    let errs = errors_of(
        "open class Swimmer { open function dive() -> string { return \"d\"; } } \
             open class Flyer { function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { function move() -> string { return \"m\"; } }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERRIDE-FINAL")),
        "got {errs:?}"
    );
}

// --- M-RT S7: erased generics ---

#[test]
fn generic_identity_typechecks_and_infers() {
    // A generic function used at two distinct concrete types — both inferred clean.
    let ok = errors_of(
        "function id<T>(T x) -> T { return x; } \
             function main() { int n = id(42); string s = id(\"hi\"); }",
    );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_call_result_is_substituted() {
    // `id(42)` returns `int`, so binding it to a `string` is a type error (the return type was
    // unified to the concrete argument type, not left abstract).
    let bad =
        errors_of("function id<T>(T x) -> T { return x; } function main() { string s = id(42); }");
    assert!(!bad.is_empty(), "expected a type error, got none");
}

#[test]
fn generic_unifies_through_list_and_function() {
    // `firstOr<T>(List<T>, T) -> T` binds T from the list element; `applyTwice<T>(T, (T)->T) -> T`
    // unifies a function-typed parameter. Both infer clean against concrete arguments.
    let ok = errors_of(
            "function firstOr<T>(List<T> xs, T fallback) -> T { for (T x in xs) { return x; } return fallback; } \
             function applyTwice<T>(T x, (T) -> T f) -> T { return f(f(x)); } \
             function main() { List<int> xs = [1, 2]; int a = firstOr(xs, 0); int b = applyTwice(5, fn(int v) => v + 1); }",
        );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_argument_must_unify_consistently() {
    // Two `T` parameters bound to incompatible concrete types — the second arg cannot match the
    // `int` bound from the first.
    let bad = errors_of(
        "function pairEq<T>(T a, T b) -> bool { return true; } \
             function main() { bool r = pairEq(1, \"x\"); }",
    );
    assert!(!bad.is_empty(), "expected a unification error, got none");
}

#[test]
fn type_param_shadowing_builtin_is_rejected() {
    let e = errors_of("function f<int>(int x) -> int { return x; } function main() {}");
    assert!(
        e.iter().any(|d| d.code == Some("E-GENERIC-PARAM")),
        "got {e:?}"
    );
}

#[test]
fn duplicate_type_param_is_rejected() {
    let e = errors_of("function f<T, T>(T x) -> T { return x; } function main() {}");
    assert!(
        e.iter().any(|d| d.code == Some("E-GENERIC-PARAM")),
        "got {e:?}"
    );
}

#[test]
fn type_param_must_be_pascalcase() {
    let e = errors_of("function f<t>(t x) -> t { return x; } function main() {}");
    assert!(e.iter().any(|d| d.code == Some("E-TYPE-CASE")), "got {e:?}");
}

// --- M-RT generics-all: generic *methods* ---

#[test]
fn generic_method_typechecks_and_infers() {
    // A generic method on a non-generic class, inferred from arguments at two distinct types.
    let ok = errors_of(
        "class U { function id<T>(T x) -> T { return x; } } \
             function main() { var u = U(); int n = u.id(42); string s = u.id(\"hi\"); }",
    );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_method_result_is_substituted() {
    // `u.id(42)` returns `int`; binding it to a `string` is a type error — proving the method
    // sig was treated as generic (return unified to the concrete arg), not left abstract or
    // checked by the plain non-generic path.
    let bad = errors_of(
        "class U { function id<T>(T x) -> T { return x; } } \
             function main() { var u = U(); string s = u.id(42); }",
    );
    assert!(!bad.is_empty(), "expected a type error, got none");
}

#[test]
fn generic_method_argument_must_unify_consistently() {
    // Two `T` parameters of a method bound to incompatible concrete types.
    let bad = errors_of(
        "class U { function pairEq<T>(T a, T b) -> bool { return true; } } \
             function main() { var u = U(); bool r = u.pairEq(1, \"x\"); }",
    );
    assert!(!bad.is_empty(), "expected a unification error, got none");
}

#[test]
fn generic_method_param_must_be_pascalcase() {
    let e = errors_of("class U { function f<t>(t x) -> t { return x; } } function main() {}");
    assert!(e.iter().any(|d| d.code == Some("E-TYPE-CASE")), "got {e:?}");
}

// --- M-RT generics-all: generic *types* / classes ---

#[test]
fn generic_class_construction_infers_and_substitutes() {
    // `Box(7)` infers T=int; `get()` returns int; a two-parameter `Pair<A, B>` binds each
    // parameter independently from its constructor argument.
    let ok = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             class Pair<A, B> { constructor(private A first, private B second) {} \
                function left() -> A { return this.first; } function right() -> B { return this.second; } } \
             function main() { var b = Box(7); int x = b.get(); \
                var p = Pair(1, \"s\"); int l = p.left(); string r = p.right(); }",
        );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_class_result_is_substituted() {
    // `Box(7).get()` is int; binding it to a string is an error — proving use-site reification
    // (the instance carries `T=int`, recovered at the member access), not an abstract/mixed result.
    let bad = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             function main() { var b = Box(7); string s = b.get(); }",
        );
    assert!(!bad.is_empty(), "expected a type error, got none");
}

#[test]
fn generic_class_method_param_substituted() {
    // A method *taking* a `T` rejects a wrong-typed argument at the instance's concrete type.
    let bad = errors_of(
            "class Box<T> { constructor(private T value) {} function orElse(T f) -> T { return this.value; } } \
             function main() { var b = Box(7); int y = b.orElse(\"x\"); }",
        );
    assert!(!bad.is_empty(), "expected an argument type error, got none");
}

#[test]
fn generic_class_annotation_arity_checked() {
    // A bare `Box` annotation (no type argument) on a generic class is an arity error.
    let bad = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             function main() { Box b = Box(7); }",
        );
    assert!(!bad.is_empty(), "expected an arity error, got none");
}

#[test]
fn generic_class_explicit_type_argument_ok() {
    let ok = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             function main() { Box<int> b = Box(7); int x = b.get(); }",
        );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

// --- M-RT: generic enums (`Option<T>` / `Result<T, E>`) ---

const OPTION: &str = "enum Option<T> { None, Some(T value) }";
const RESULT: &str = "enum Result<T, E> { Ok(T value), Err(E error) }";

#[test]
fn generic_enum_construction_infers_and_binds() {
    // `Some(7)` infers `Option<int>`; matching it binds the payload at the concrete int, so using
    // the binding where an int is expected is clean.
    let ok = errors_of(&format!(
        "{OPTION} function main() {{ var o = Some(7); \
             int x = match o {{ Some(n) => n, None() => 0 }}; }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_enum_match_payload_is_concrete() {
    // Matching an `Option<int>` and binding the `Some` payload to a string is a type error —
    // proving the payload is reified to int at the match (via the scrutinee's type argument), not
    // left abstract/mixed.
    let bad = errors_of(&format!(
        "{OPTION} function main() {{ var o = Some(7); \
             string s = match o {{ Some(n) => n, None() => \"x\" }}; }}"
    ));
    assert!(!bad.is_empty(), "expected a type error, got none");
}

#[test]
fn generic_enum_annotation_arity_checked() {
    // A bare `Option` annotation (no type argument) on a generic enum is an arity error.
    let bad = errors_of(&format!(
        "{OPTION} function main() {{ Option o = Some(7); }}"
    ));
    assert!(!bad.is_empty(), "expected an arity error, got none");
}

#[test]
fn generic_enum_annotated_non_inferring_variant_ok() {
    // `None` mentions no `T`, so it cannot infer the argument — annotating the binding fixes it.
    let ok = errors_of(&format!(
        "{OPTION} function main() {{ Option<int> n = None(); \
             int x = match n {{ Some(v) => v, None() => 0 }}; }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_enum_two_params_independent() {
    // `Result<T, E>` binds `T` from `Ok`'s argument and `E` from `Err`'s, independently.
    let ok = errors_of(&format!(
        "{RESULT} function ok() -> Result<int, string> {{ return Ok(1); }} \
             function bad() -> Result<int, string> {{ return Err(\"no\"); }} \
             function main() {{ string r = match ok() {{ Ok(v) => \"v\", Err(e) => e }}; }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_enum_variant_arity_checked() {
    // A generic variant constructor still checks its own arity: `Some` takes exactly one field.
    let bad = errors_of(&format!(
        "{OPTION} function main() {{ var o = Some(1, 2); }}"
    ));
    assert!(!bad.is_empty(), "expected an arity error, got none");
}

#[test]
fn generic_enum_param_must_be_pascalcase() {
    // A type parameter shadowing a built-in type name is `E-GENERIC-PARAM`.
    let bad = errors_of("enum Box<int> { Wrap(int x) } function main() {}");
    assert!(
        bad.iter().any(|d| d.code == Some("E-GENERIC-PARAM")),
        "expected E-GENERIC-PARAM, got {bad:?}"
    );
}

#[test]
fn erase_generics_strips_enum_type_params() {
    use crate::ast::{Item, Type};
    let e = erase_generics(prog(&format!("{OPTION} function main() {{}}")));
    let en = e
        .items
        .iter()
        .find_map(|it| match it {
            Item::Enum(en) if en.name == "Option" => Some(en),
            _ => None,
        })
        .expect("enum Option present");
    assert!(en.type_params.is_empty(), "enum type params not erased");
    let some = en
        .variants
        .iter()
        .find(|v| v.name == "Some")
        .expect("Some variant present");
    assert!(
        matches!(some.fields[0].ty, Type::Erased(_)),
        "Some payload not erased: {:?}",
        some.fields[0].ty
    );
}

#[test]
fn non_generic_enum_rejects_type_argument() {
    // A plain (non-generic) enum still takes no type arguments.
    let bad = errors_of("enum Color { Red, Green } function main() { Color<int> c = Red(); }");
    assert!(!bad.is_empty(), "expected an arity error, got none");
}

// --- M-faults Slice 2a: `?` error propagation on Result ---

const RESULT_DEF: &str = "enum Result<T, E> { Ok(T value), Err(E error) }";

#[test]
fn propagate_in_result_fn_is_clean() {
    // `?` in a let-initializer inside a `Result`-returning fn unwraps the `Ok` payload (an `int`).
    let ok = errors_of(&format!(
        "{RESULT_DEF} \
             function f() -> Result<int, string> {{ return Ok(1); }} \
             function g() -> Result<int, string> {{ int x = f()?; return Ok(x + 1); }} \
             function main() {{}}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn propagate_outside_let_initializer_is_position_error() {
    // `?` nested in a larger expression is `E-PROPAGATE-POSITION` (not a whole let-initializer).
    let bad = errors_of(&format!(
        "{RESULT_DEF} \
             function f() -> Result<int, string> {{ return Ok(1); }} \
             function g() -> Result<int, string> {{ int x = f()? + 1; return Ok(x); }} \
             function main() {{}}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-PROPAGATE-POSITION")),
        "expected E-PROPAGATE-POSITION, got {bad:?}"
    );
}

#[test]
fn intrinsic_panic_requires_string_literal() {
    // A non-literal panic message (interpolation) is `E-INTRINSIC-LITERAL`.
    let bad = errors_of(r#"function main() { var n = 1; panic("bad {n}"); }"#);
    assert!(
        bad.iter().any(|d| d.code == Some("E-INTRINSIC-LITERAL")),
        "expected E-INTRINSIC-LITERAL, got {bad:?}"
    );
}

#[test]
fn intrinsic_assert_condition_must_be_bool() {
    let bad = errors_of(r#"function main() { assert(1, "x"); }"#);
    assert!(
        !bad.is_empty(),
        "expected a type error for a non-bool assert condition"
    );
}

#[test]
fn intrinsic_name_is_reserved() {
    let bad = errors_of("function unreachable() { return; } function main() {}");
    assert!(
        bad.iter().any(|d| d.code == Some("E-RESERVED-INTRINSIC")),
        "expected E-RESERVED-INTRINSIC, got {bad:?}"
    );
}

#[test]
fn panic_tail_satisfies_return_totality() {
    // `panic` is `never`-typed, so a value-returning fn ending in it needs no further `return`.
    let ok = errors_of(r#"function f() -> int { panic("x"); } function main() {}"#);
    assert!(
        ok.is_empty(),
        "expected clean (never satisfies totality), got {ok:?}"
    );
}

#[test]
fn propagate_in_non_result_fn_is_context_error() {
    // `?` requires the enclosing fn to return the same `Result` — otherwise `E-PROPAGATE-CONTEXT`.
    let bad = errors_of(&format!(
        "{RESULT_DEF} \
             function f() -> Result<int, string> {{ return Ok(1); }} \
             function g() -> int {{ int x = f()?; return x; }} \
             function main() {{}}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-PROPAGATE-CONTEXT")),
        "expected E-PROPAGATE-CONTEXT, got {bad:?}"
    );
}

// --- M-faults Slice 2b: checked exceptions (throws / throw / try-catch enforcement) ---

const ERRDEF: &str = "class BadInput implements Error { constructor(public string message) {} } \
         class NotFound implements Error { constructor(public string message) {} }";

#[test]
fn throw_undeclared_and_uncaught_is_error() {
    // A helper that throws but neither declares `throws` nor wraps it in a `try`.
    let bad = errors_of(&format!(
        "{ERRDEF} function f() {{ throw BadInput(\"x\"); }} function main() {{}}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-THROW-UNDECLARED")),
        "expected E-THROW-UNDECLARED, got {bad:?}"
    );
}

#[test]
fn throw_declared_then_caught_at_call_is_clean() {
    // `f` declares `throws BadInput` (discharges its own throw); `main` calls it inside a `try`
    // catching `BadInput` (discharges the call). Both sides handled — clean.
    let ok = errors_of(&format!(
        "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function main() {{ try {{ f(); }} catch (BadInput e) {{}} }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn throw_in_main_is_uncaught() {
    let bad = errors_of(&format!(
        "{ERRDEF} function main() {{ throw BadInput(\"x\"); }}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-UNCAUGHT-THROW")),
        "expected E-UNCAUGHT-THROW, got {bad:?}"
    );
}

#[test]
fn main_may_not_declare_throws() {
    let bad = errors_of(&format!(
        "{ERRDEF} function main() throws BadInput {{ throw BadInput(\"x\"); }}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-UNCAUGHT-THROW")),
        "expected E-UNCAUGHT-THROW, got {bad:?}"
    );
}

#[test]
fn throws_error_root_is_too_broad() {
    let bad = errors_of(&format!(
        "{ERRDEF} function f() throws Error {{ throw BadInput(\"x\"); }} \
             function main() {{ try {{ f(); }} catch (Error e) {{}} }}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-THROWS-TOO-BROAD")),
        "expected E-THROWS-TOO-BROAD, got {bad:?}"
    );
}

#[test]
fn throw_non_error_value_is_type_error() {
    let bad = errors_of("function main() { throw 42; }");
    assert!(
        bad.iter().any(|d| d.code == Some("E-THROW-TYPE")),
        "expected E-THROW-TYPE, got {bad:?}"
    );
}

#[test]
fn bare_call_to_throwing_fn_is_unhandled() {
    let bad = errors_of(&format!(
        "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function main() {{ f(); }}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-CALL-UNHANDLED")),
        "expected E-CALL-UNHANDLED, got {bad:?}"
    );
}

#[test]
fn propagate_throws_to_declared_is_clean() {
    // `g` propagates `f`'s `BadInput` with `?` and declares it — clean; `main` catches the call.
    let ok = errors_of(&format!(
        "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function g() throws BadInput {{ f()?; }} \
             function main() {{ try {{ g(); }} catch (BadInput e) {{}} }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn propagate_throws_without_declaration_is_unhandled() {
    // `g` uses `?` but does not declare `throws BadInput` — the propagation is unhandled.
    let bad = errors_of(&format!(
        "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function g() {{ f()?; }} function main() {{}}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-CALL-UNHANDLED")),
        "expected E-CALL-UNHANDLED, got {bad:?}"
    );
}

#[test]
fn catch_non_error_type_is_error() {
    let bad = errors_of(&format!(
        "{ERRDEF} function main() {{ try {{}} catch (int e) {{}} }}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-CATCH-TYPE")),
        "expected E-CATCH-TYPE, got {bad:?}"
    );
}

#[test]
fn shadowed_catch_clause_warns() {
    // A second `catch (BadInput …)` after the first can never run — a non-fatal lint.
    let warns = warnings_of(&format!(
        "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function main() {{ try {{ f(); }} catch (BadInput e) {{}} catch (BadInput e2) {{}} }}"
    ));
    assert!(
        warns.iter().any(|d| d.code == Some("W-CATCH-UNREACHABLE")),
        "expected W-CATCH-UNREACHABLE, got {warns:?}"
    );
}

#[test]
fn union_catch_covers_each_member() {
    // `catch (BadInput | NotFound e)` discharges a call that throws `BadInput` (a member).
    let ok = errors_of(&format!(
        "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function main() {{ try {{ f(); }} catch (BadInput | NotFound e) {{}} }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn try_with_returning_arms_satisfies_totality() {
    // A `-> int` fn whose `try` body and `catch` both return diverges on every path — total.
    let ok = errors_of(&format!(
            "{ERRDEF} function g() -> int {{ try {{ return 1; }} catch (BadInput e) {{ return 0; }} }} \
             function main() {{}}"
        ));
    assert!(ok.is_empty(), "expected clean (try totality), got {ok:?}");
}

#[test]
fn try_falling_through_misses_return() {
    // Both arms fall through, so the `-> int` fn does not return on all paths.
    let bad = errors_of(&format!(
        "{ERRDEF} function g() -> int {{ try {{}} catch (BadInput e) {{}} }} function main() {{}}"
    ));
    assert!(
        bad.iter().any(|d| d.code == Some("E-MISSING-RETURN")),
        "expected E-MISSING-RETURN, got {bad:?}"
    );
}

#[test]
fn throw_tail_satisfies_totality() {
    // A `throw` diverges, so a `-> int` fn whose only statement is a `throw` is total.
    let ok = errors_of(&format!(
        "{ERRDEF} function g() -> int throws BadInput {{ throw BadInput(\"x\"); }} \
             function main() {{ try {{ var n = g(); }} catch (BadInput e) {{}} }}"
    ));
    assert!(ok.is_empty(), "expected clean (throw diverges), got {ok:?}");
}

#[test]
fn throws_mode_propagate_is_recorded_for_erasure() {
    // A throws-mode `?` is a checker-only marker: it must be recorded (mapped to the bare call)
    // so `resolve_html` erases the `Propagate` node before any backend sees it.
    let p = prog(&format!(
        "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function g() throws BadInput {{ f()?; }} function main() {{}}"
    ));
    let (_warns, subst) = check_resolutions(&p).expect("checks clean");
    assert_eq!(
        subst.len(),
        1,
        "exactly one throws-? recorded, got {subst:?}"
    );
    assert!(
        matches!(subst.values().next(), Some(crate::ast::Expr::Call { .. })),
        "the erased `?` must map to the bare call, got {subst:?}"
    );
    // And the substitution actually removes the `Propagate` node.
    let expanded = resolve_html(p, &subst);
    assert!(
        !program_has_propagate(&expanded),
        "throws-mode `?` Propagate node was not erased"
    );
}

/// Recursively scan a checked program for any surviving `Expr::Propagate` node (test helper for
/// the throws-`?` erasure invariant).
fn program_has_propagate(p: &Program) -> bool {
    use crate::ast::{ClassMember, Expr, Item, LambdaBody, Stmt};
    fn in_expr(e: &Expr) -> bool {
        match e {
            Expr::Propagate { .. } => true,
            Expr::Unary { expr, .. } | Expr::Force { inner: expr, .. } => in_expr(expr),
            Expr::Binary { lhs, rhs, .. } => in_expr(lhs) || in_expr(rhs),
            Expr::Call { callee, args, .. } => in_expr(callee) || args.iter().any(in_expr),
            Expr::Member { object, .. } => in_expr(object),
            Expr::Index { object, index, .. } => in_expr(object) || in_expr(index),
            Expr::List(items, _) => items.iter().any(in_expr),
            Expr::Match {
                scrutinee, arms, ..
            } => in_expr(scrutinee) || arms.iter().any(|a| in_expr(&a.body)),
            Expr::Lambda { body, .. } => match body {
                LambdaBody::Expr(e) => in_expr(e),
                LambdaBody::Block(b) => b.iter().any(in_stmt),
            },
            _ => false,
        }
    }
    fn in_stmt(s: &Stmt) -> bool {
        match s {
            Stmt::Expr(e, _) | Stmt::Throw { value: e, .. } => in_expr(e),
            Stmt::VarDecl { init, .. } => in_expr(init),
            Stmt::Return { value: Some(e), .. } => in_expr(e),
            Stmt::Block(b, _) => b.iter().any(in_stmt),
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                body.iter().any(in_stmt)
                    || catches.iter().any(|c| c.body.iter().any(in_stmt))
                    || finally_block
                        .as_ref()
                        .is_some_and(|fb| fb.iter().any(in_stmt))
            }
            _ => false,
        }
    }
    fn in_fn(body: &[Stmt]) -> bool {
        body.iter().any(in_stmt)
    }
    p.items.iter().any(|it| match it {
        Item::Function(f) => in_fn(&f.body),
        Item::Class(c) => c.members.iter().any(|m| match m {
            ClassMember::Method(f) => in_fn(&f.body),
            ClassMember::Constructor { body, .. } => in_fn(body),
            _ => false,
        }),
        _ => false,
    })
}

// --- M-RT S4: union types + match-over-union ---

const SHAPES: &str = "class Circle { constructor(public int radius) {} } \
        class Square { constructor(public int side) {} } \
        class Triangle { constructor(public int base) {} }";

#[test]
fn union_param_accepts_each_member() {
    let ok = errors_of(&format!(
        "{SHAPES} function f(Circle | Square s) {{}} \
             function main() {{ f(Circle(1)); f(Square(2)); }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn union_param_rejects_non_member() {
    let bad = errors_of(&format!(
        "{SHAPES} function f(Circle | Square s) {{}} \
             function main() {{ f(Triangle(3)); }}"
    ));
    assert!(
        !bad.is_empty(),
        "expected a type error passing a non-member"
    );
}

#[test]
fn match_over_union_exhaustive_ok() {
    let ok = errors_of(&format!(
        "{SHAPES} function area(Circle | Square s) -> int {{ \
               return match s {{ Circle c => c.radius, Square sq => sq.side }}; }} \
             function main() {{ int a = area(Circle(2)); }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn match_over_union_non_exhaustive_lists_missing() {
    let bad = errors_of(&format!(
        "{SHAPES} function area(Circle | Square s) -> int {{ \
               return match s {{ Circle c => c.radius }}; }} \
             function main() {{}}"
    ));
    assert!(
        bad.iter()
            .any(|e| e.message.contains("non-exhaustive") && e.message.contains("Square")),
        "{bad:?}"
    );
}

#[test]
fn union_rejects_enum_member() {
    let bad = errors_of(&format!(
            "{SHAPES} enum Color {{ Red, Green }} function f(Circle | Color x) {{}} function main() {{}}"
        ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-UNION-MEMBER")),
        "{bad:?}"
    );
}

#[test]
fn union_arity_collapse_is_error() {
    let bad = errors_of(&format!(
        "{SHAPES} function f(Circle | Circle x) {{}} function main() {{}}"
    ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-UNION-ARITY")),
        "{bad:?}"
    );
}

#[test]
fn type_pattern_must_name_a_class_or_interface() {
    let bad = errors_of(&format!(
        "{SHAPES} function f(Circle | Square s) -> int {{ \
               return match s {{ Circle c => c.radius, Nope n => 0 }}; }} function main() {{}}"
    ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-MATCH-TYPE")),
        "{bad:?}"
    );
}

#[test]
fn instanceof_narrows_a_union_operand() {
    let ok = errors_of(&format!(
        "{SHAPES} function f(Circle | Square s) -> int {{ \
               if (s instanceof Circle) {{ return s.radius; }} return 0; }} function main() {{}}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn primitive_union_literal_match_ok() {
    let ok = errors_of(
        "function classify(int | string code) -> string { \
               return match code { 0 => \"zero\", \"ok\" => \"okay\", _ => \"other\" }; } \
             function main() { string s = classify(0); }",
    );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn primitive_union_accepts_int_and_string() {
    let ok = errors_of("function f(int | string x) {} function main() { f(1); f(\"a\"); }");
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn type_pattern_nested_in_variant_is_rejected() {
    // A type pattern is top-level-only; nesting it in a variant payload would diverge from the
    // transpiler (which emits only simple payload bindings), so the checker rejects it.
    let bad = errors_of(&format!(
        "{SHAPES} enum Wrap {{ One(Circle inner) }} \
             function f(Wrap w) -> int {{ return match w {{ One(Circle c) => c.radius }}; }} \
             function main() {{}}"
    ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-MATCH-TYPE")),
        "{bad:?}"
    );
}

// M-RT S5 — intersection types. Two interfaces and a class implementing both.
const IFACES: &str = "interface Drawable { function draw() -> string; } \
        interface Named { function name() -> string; } \
        class Badge implements Drawable, Named { \
            constructor(public string label) {} \
            function draw() -> string { return \"[]\"; } \
            function name() -> string { return this.label; } }";

#[test]
fn intersection_param_accepts_a_class_implementing_both() {
    // all-members-required-in: a Badge (implements Drawable AND Named) flows into the intersection.
    let ok = errors_of(&format!(
        "{IFACES} function describe(Drawable & Named x) -> string {{ return x.draw(); }} \
             function main() {{ string s = describe(Badge(\"b\")); }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn intersection_member_access_reaches_each_member() {
    // A method from *each* member interface is in scope on the intersection value.
    let ok = errors_of(&format!(
            "{IFACES} function f(Drawable & Named x) -> string {{ return \"{{x.draw()}} {{x.name()}}\"; }} \
             function main() {{ string s = f(Badge(\"b\")); }}"
        ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn intersection_flows_out_to_a_single_member() {
    // some-member-out: A & B is assignable to a slot typed as just one member.
    let ok = errors_of(&format!(
        "{IFACES} function onlyDraw(Drawable d) -> string {{ return d.draw(); }} \
             function f(Drawable & Named x) -> string {{ return onlyDraw(x); }} \
             function main() {{}}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn intersection_one_class_plus_interface_is_allowed() {
    // D1: at most one concrete class plus interfaces is a well-formed intersection.
    let ok = errors_of(&format!(
        "{IFACES} function f(Badge & Drawable x) -> string {{ return x.draw(); }} \
             function main() {{ string s = f(Badge(\"b\")); }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn intersection_rejects_two_classes() {
    let bad = errors_of(&format!(
        "{SHAPES} function f(Circle & Square x) {{}} function main() {{}}"
    ));
    assert!(
        bad.iter()
            .any(|e| e.code == Some("E-INTERSECT-MULTI-CLASS")),
        "{bad:?}"
    );
}

#[test]
fn intersection_rejects_primitive_member() {
    let bad = errors_of(&format!(
        "{IFACES} function f(int & Drawable x) {{}} function main() {{}}"
    ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-INTERSECT-MEMBER")),
        "{bad:?}"
    );
}

#[test]
fn intersection_arity_collapse_is_error() {
    let bad = errors_of(&format!(
        "{IFACES} function f(Drawable & Drawable x) {{}} function main() {{}}"
    ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-INTERSECT-ARITY")),
        "{bad:?}"
    );
}

#[test]
fn intersection_rejects_conflicting_shared_method_signature() {
    // D2: two members declare `tag` with differing return types — no class can implement both.
    let bad = errors_of(
        "interface A { function tag() -> string; } \
             interface B { function tag() -> int; } \
             function f(A & B x) {} function main() {}",
    );
    assert!(
        bad.iter().any(|e| e.code == Some("E-INTERSECT-SIG")),
        "{bad:?}"
    );
}

#[test]
fn intersection_member_access_unknown_is_error() {
    let bad = errors_of(&format!(
            "{IFACES} function f(Drawable & Named x) -> int {{ return x.nope(); }} function main() {{}}"
        ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-INTERSECT-NO-MEMBER")),
        "{bad:?}"
    );
}

#[test]
fn generic_class_param_must_be_pascalcase() {
    let e = errors_of(
        "class Box<t> { constructor(private t value) {} } function main() { var b = Box(7); }",
    );
    assert!(e.iter().any(|d| d.code == Some("E-TYPE-CASE")), "got {e:?}");
}

#[test]
fn method_type_param_shadowing_class_param_rejected() {
    let e = errors_of(
        "class Box<T> { constructor(private T value) {} function id<T>(T x) -> T { return x; } } \
             function main() { var b = Box(7); }",
    );
    assert!(
        e.iter().any(|d| d.code == Some("E-GENERIC-PARAM")),
        "got {e:?}"
    );
}

#[test]
fn erase_generics_strips_class_type_params() {
    use crate::ast::{ClassMember, Item, Type};
    let p = prog(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } function main() {}",
        );
    let e = erase_generics(p);
    let c = e
        .items
        .iter()
        .find_map(|it| match it {
            Item::Class(c) if c.name == "Box" => Some(c),
            _ => None,
        })
        .expect("class Box present");
    assert!(c.type_params.is_empty(), "class type params not erased");
    for m in &c.members {
        match m {
            ClassMember::Constructor { params, .. } => assert!(
                matches!(params[0].ty, Type::Erased(_)),
                "ctor param not erased: {:?}",
                params[0].ty
            ),
            ClassMember::Method(f) if f.name == "get" => assert!(
                matches!(f.ret, Some(Type::Erased(_))),
                "method ret not erased: {:?}",
                f.ret
            ),
            _ => {}
        }
    }
}

#[test]
fn erase_generics_strips_method_type_params() {
    use crate::ast::{ClassMember, Item, Type};
    let p = prog("class U { function id<T>(T x) -> T { return x; } } function main() {}");
    let e = erase_generics(p);
    let m = e
        .items
        .iter()
        .find_map(|it| match it {
            Item::Class(c) => c.members.iter().find_map(|mem| match mem {
                ClassMember::Method(f) if f.name == "id" => Some(f),
                _ => None,
            }),
            _ => None,
        })
        .expect("method id present");
    assert!(m.type_params.is_empty(), "method type params not erased");
    assert!(
        matches!(m.params[0].ty, Type::Erased(_)),
        "param type not erased: {:?}",
        m.params[0].ty
    );
    assert!(
        matches!(m.ret, Some(Type::Erased(_))),
        "return type not erased: {:?}",
        m.ret
    );
}

#[test]
fn erase_generics_strips_type_params_and_rewrites_types() {
    use crate::ast::{Item, Type};
    let p = prog("function id<T>(T x) -> T { return x; } function main() {}");
    let e = erase_generics(p);
    let f = e
        .items
        .iter()
        .find_map(|it| match it {
            Item::Function(f) if f.name == "id" => Some(f),
            _ => None,
        })
        .expect("id present");
    assert!(f.type_params.is_empty(), "type params not erased");
    assert!(
        matches!(f.params[0].ty, Type::Erased(_)),
        "param type not erased: {:?}",
        f.params[0].ty
    );
    assert!(
        matches!(f.ret, Some(Type::Erased(_))),
        "return type not erased: {:?}",
        f.ret
    );
}

#[test]
fn map_literal_and_indexing_typecheck() {
    // A well-typed map literal + index of the right key type checks clean.
    let ok = errors_of("function main() { Map<string, int> m = [\"a\" => 1]; int x = m[\"a\"]; }");
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
    // Indexing with the wrong key type is an error.
    let bad = errors_of("function main() { Map<string, int> m = [\"a\" => 1]; int x = m[0]; }");
    assert!(
        bad.iter().any(|d| d.message.contains("map index must be")),
        "got {bad:?}"
    );
}

#[test]
fn map_key_must_be_hashable() {
    // A `float` key is not hashable → E-MAP-KEY.
    let e = errors_of("function main() { Map<float, int> m = [1.0 => 1]; }");
    assert!(e.iter().any(|d| d.code == Some("E-MAP-KEY")), "got {e:?}");
}

#[test]
fn package_is_mandatory_and_core_is_reserved() {
    // M5 S1: every file is packaged, never inferred. No declaration → E-NO-PACKAGE.
    let e = errors_of_raw("function main() {}");
    assert!(
        e.iter().any(|d| d.code == Some("E-NO-PACKAGE")),
        "got {e:?}"
    );
    // The `Core` root is reserved for the standard library → E-RESERVED-PACKAGE.
    let e2 = errors_of_raw("package Core; function main() {}");
    assert!(
        e2.iter().any(|d| d.code == Some("E-RESERVED-PACKAGE")),
        "got {e2:?}"
    );
    let e3 = errors_of_raw("package Core.Evil; function main() {}");
    assert!(
        e3.iter().any(|d| d.code == Some("E-RESERVED-PACKAGE")),
        "got {e3:?}"
    );
    // A well-formed user package (and the reserved `Main`) type-check cleanly.
    assert!(check(&prog_raw("package App.Util; function main() {}")).is_ok());
    assert!(check(&prog_raw("package Main; function main() {}")).is_ok());
}

#[test]
fn package_and_import_segments_must_be_pascalcase() {
    // Reshape slice 2b: a lowercase package segment is rejected (E-PKG-CASE).
    let e = errors_of_raw("package app.util; function main() {}");
    assert!(e.iter().any(|d| d.code == Some("E-PKG-CASE")), "got {e:?}");
    // Each non-PascalCase segment is flagged; a single-segment lowercase package too.
    let e2 = errors_of_raw("package acme; function main() {}");
    assert!(
        e2.iter().any(|d| d.code == Some("E-PKG-CASE")),
        "got {e2:?}"
    );
    // A lowercase import path segment is rejected.
    let e3 = errors_of_raw("package Main; import acme.util; function main() { int x = util.f(); }");
    assert!(
        e3.iter().any(|d| d.code == Some("E-PKG-CASE")),
        "got {e3:?}"
    );
    // A lowercase import alias is rejected (it occupies a leaf position).
    let e4 = errors_of_raw(
        "package Main; import Acme.Util as util; function main() { int x = util.f(); }",
    );
    assert!(
        e4.iter().any(|d| d.code == Some("E-PKG-CASE")),
        "got {e4:?}"
    );
    // PascalCase package + import + alias type-check cleanly (no E-PKG-CASE noise).
    let ok = errors_of_raw("package App.Util; function main() {}");
    assert!(
        !ok.iter().any(|d| d.code == Some("E-PKG-CASE")),
        "got {ok:?}"
    );
}

#[test]
fn optional_binding_and_null_discipline() {
    // an optional binding accepts `null` and a widened non-null `T`
    assert!(errors_of("function main() { int? x = null; }").is_empty());
    assert!(errors_of("function main() { int? y = 5; }").is_empty());
    // `null` / `T?` cannot flow into a non-optional `T`
    let e1 = errors_of("function main() { int x = null; }");
    assert!(
        e1.iter().any(|d| d.code == Some("E-OPT-ASSIGN")),
        "got {e1:?}"
    );
    let e2 = errors_of("function main() { int? x = null; int y = x; }");
    assert!(
        e2.iter().any(|d| d.code == Some("E-OPT-ASSIGN")),
        "got {e2:?}"
    );
}

#[test]
fn if_let_binding_and_smart_cast() {
    // smart-cast: inside the then-block, the bound name is the non-optional inner `T`
    assert!(errors_of("function main() { int? o = 5; if (var x = o) { int y = x; } }").is_empty());
    // the binding is NOT in scope in the else block
    let e1 = errors_of("function main() { int? o = 5; if (var x = o) {} else { int y = x; } }");
    assert!(
        e1.iter().any(|d| d.code == Some("E-UNKNOWN-IDENT")),
        "got {e1:?}"
    );
    // the binding is NOT in scope after the if
    let e2 = errors_of("function main() { int? o = 5; if (var x = o) {} int y = x; }");
    assert!(
        e2.iter().any(|d| d.code == Some("E-UNKNOWN-IDENT")),
        "got {e2:?}"
    );
    // the scrutinee must be optional — binding a non-optional is `E-IF-LET-TYPE`
    let e3 = errors_of("function main() { int n = 5; if (var x = n) {} }");
    assert!(
        e3.iter().any(|d| d.code == Some("E-IF-LET-TYPE")),
        "got {e3:?}"
    );
}

#[test]
fn match_over_optional() {
    // null arm + catch-all binding is exhaustive for `T?`, and the binding narrows to inner `T`
    // (so it can be used as a non-optional — here as an `int` arithmetic operand)
    assert!(
        errors_of("function f(int? o) -> int { return match o { null => -1, v => v + 1 }; }")
            .is_empty()
    );
    // a `null` pattern requires an optional scrutinee
    let e1 = errors_of("function main() { int n = 3; int x = match n { null => 0, v => v }; }");
    assert!(
        e1.iter().any(|d| d.message.contains("`null` pattern")),
        "got {e1:?}"
    );
    // a `null` arm alone (no catch-all for the non-null case) is non-exhaustive
    let e2 = errors_of("function f(int? o) -> int { return match o { null => -1 }; }");
    assert!(
        e2.iter().any(|d| d.message.contains("non-exhaustive")),
        "got {e2:?}"
    );
}

#[test]
fn force_unwrap_typing_and_lint() {
    // `opt!` unwraps `T?` to `T`; the program type-checks and emits the W-FORCE-UNWRAP lint
    let src = "function main() { int? o = 5; int x = o!; }";
    assert!(errors_of(src).is_empty(), "got {:?}", errors_of(src));
    let w = warnings_of(src);
    assert!(
        w.iter().any(|d| d.code == Some("W-FORCE-UNWRAP")),
        "expected W-FORCE-UNWRAP, got {w:?}"
    );
    // force-unwrapping a non-optional is an error (nothing to unwrap)
    let e = errors_of("function main() { int n = 3; int x = n!; }");
    assert!(
        e.iter().any(|d| d.code == Some("E-OPT-UNWRAP")),
        "got {e:?}"
    );
}

#[test]
fn coalesce_typing() {
    // `T? ?? T` and `null ?? T` both yield the non-optional `T`.
    assert!(errors_of("function main() { int? x = null; int y = x ?? 3; }").is_empty());
    assert!(errors_of("function main() { int y = null ?? 3; }").is_empty());
    // `??` on a non-optional left operand is a misuse.
    assert!(!errors_of("function main() { int a = 1; int y = a ?? 3; }").is_empty());
}

#[test]
fn safe_member_access_typing() {
    let cls = "class Box { constructor(private int v) {} function vOf() -> int { return v; } } ";
    // `?.` on an optional yields an optional member, usable via `??`.
    let ok_field = cls.to_string() + "function main() { Box? b = null; int y = (b?.v) ?? -1; }";
    assert!(
        errors_of(&ok_field).is_empty(),
        "{:?}",
        errors_of(&ok_field)
    );
    let ok_method =
        cls.to_string() + "function main() { Box? b = null; int y = (b?.vOf()) ?? -1; }";
    assert!(
        errors_of(&ok_method).is_empty(),
        "{:?}",
        errors_of(&ok_method)
    );
    // plain `.` on an optional is the non-null-discipline violation → E-OPT-USE.
    let bad_field = cls.to_string() + "function main() { Box? b = null; int y = b.v; }";
    let e = errors_of(&bad_field);
    assert!(e.iter().any(|d| d.code == Some("E-OPT-USE")), "got {e:?}");
    let bad_method = cls.to_string() + "function main() { Box? b = null; int y = b.vOf(); }";
    let em = errors_of(&bad_method);
    assert!(em.iter().any(|d| d.code == Some("E-OPT-USE")), "got {em:?}");
}

#[test]
fn empty_program_checks_ok() {
    assert!(errors_of("").is_empty());
}

#[test]
fn var_infers_init_type_and_catches_later_misuse() {
    // `var x = 5` infers int; using it where a string is required is then a type error.
    let errs = errors_of("function main() { var x = 5; string y = x; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("expected `string`, found `int`")),
        "{errs:?}"
    );
}

#[test]
fn var_infers_and_well_typed_use_is_clean() {
    assert!(errors_of("function main() { var x = 5; int y = x; }").is_empty());
}

#[test]
fn var_from_null_is_rejected() {
    // A bare `null` has no inferable element type — `var x = null` needs `T? x = null;`.
    let errs = errors_of("function main() { var x = null; }");
    assert!(
        errs.iter().any(|d| d.code == Some("E-INFER-NULL")),
        "got {errs:?}"
    );
}

#[test]
fn type_alias_resolves_and_alias_of_alias_works() {
    // `B` -> `A` -> `int`: a param/return typed `B` checks exactly like `int`.
    let errs = errors_of(
        "type A = int; type B = A; function f(B x) -> B { return x + 1; } function main() {}",
    );
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn type_alias_cycle_is_an_error() {
    let errs = errors_of("type A = B; type B = A; function f(A x) {} function main() {}");
    assert!(errs.iter().any(|e| e.message.contains("cycle")), "{errs:?}");
}

#[test]
fn duplicate_type_name_is_an_error() {
    let errs = errors_of("type A = int; type A = float; function main() {}");
    assert!(
        errs.iter().any(|e| e.message.contains("duplicate")),
        "{errs:?}"
    );
}

#[test]
fn unknown_identifier_suggests_the_nearest_in_scope_name() {
    // `cont` is one edit from the in-scope `count` → the diagnostic carries a code + hint.
    let errs = errors_of(
        "import Core.Console; function main() { int count = 0; Console.println(\"{cont}\"); }",
    );
    let d = errs
        .iter()
        .find(|e| e.message.contains("unknown identifier"))
        .expect("an unknown-identifier error");
    assert_eq!(d.code, Some("E-UNKNOWN-IDENT"));
    assert!(
        d.hint.as_deref().unwrap_or("").contains("count"),
        "hint: {:?}",
        d.hint
    );
}

#[test]
fn snake_case_function_is_rejected() {
    // A function name with `_` is not camelCase → E-NAME-CASE, with a converted-form hint.
    let errs = errors_of("function c_to_f(int c) -> int { return c; } function main() {}");
    let d = errs
        .iter()
        .find(|d| d.code == Some("E-NAME-CASE"))
        .unwrap_or_else(|| panic!("expected E-NAME-CASE, got {errs:?}"));
    assert!(
        d.hint.as_deref().unwrap_or("").contains("cToF"),
        "hint: {:?}",
        d.hint
    );
}

#[test]
fn snake_case_var_binding_is_rejected() {
    // A `var`/typed local binding with `_` is a value identifier → E-NAME-CASE.
    let errs = errors_of("function main() { int my_count = 0; }");
    assert!(
        errs.iter().any(|d| d.code == Some("E-NAME-CASE")),
        "got {errs:?}"
    );
}

#[test]
fn non_pascal_type_enum_variant_is_rejected() {
    // class name, enum name, and a variant name that are not PascalCase → E-TYPE-CASE.
    let cls = errors_of("class box {} function main() {}");
    assert!(
        cls.iter().any(|d| d.code == Some("E-TYPE-CASE")),
        "class: {cls:?}"
    );
    let en = errors_of("enum color { red() } function main() {}");
    // both the enum name `color` and the variant `red` violate PascalCase.
    assert!(
        en.iter().filter(|d| d.code == Some("E-TYPE-CASE")).count() >= 2,
        "enum: {en:?}"
    );
    let alias = errors_of("type myInt = int; function main() {}");
    assert!(
        alias.iter().any(|d| d.code == Some("E-TYPE-CASE")),
        "alias: {alias:?}"
    );
}

#[test]
fn conformant_casing_is_clean() {
    // camelCase fns/params/vars + PascalCase types/enums/variants type-check with no casing error.
    let src = "enum Shape { Circle(float r) } \
                   class Box { constructor(private int width) {} function widthOf() -> int { return width; } } \
                   function areaOf(Shape s) -> int { int localCount = 0; return localCount; } \
                   function main() {}";
    let errs = errors_of(src);
    assert!(
        !errs
            .iter()
            .any(|d| d.code == Some("E-NAME-CASE") || d.code == Some("E-TYPE-CASE")),
        "expected no casing errors, got {errs:?}"
    );
}

#[test]
fn case_converters() {
    assert!(is_camel("main") && is_camel("splitOnce") && !is_camel("split_once"));
    assert!(is_pascal("Shape") && !is_pascal("shape") && !is_pascal("Http_Request"));
    assert_eq!(to_camel("split_once"), "splitOnce");
    assert_eq!(to_camel("c_to_f"), "cToF");
    assert_eq!(to_pascal("shape"), "Shape");
    assert_eq!(to_pascal("http_request"), "HttpRequest");
}

#[test]
fn unknown_type_carries_a_code() {
    let errs = errors_of("function main() { Nope n = 0; }");
    let d = errs
        .iter()
        .find(|e| e.message.contains("unknown type"))
        .expect("an unknown-type error");
    assert_eq!(d.code, Some("E-UNKNOWN-TYPE"));
}

#[test]
fn expand_aliases_dealiases_the_program_for_backends() {
    // After expansion the backends must see no alias names: `B`/`A` collapse to `int`.
    let p = prog("type A = int; type B = A; function f(B x) -> B { return x; } function main() {}");
    let e = expand_aliases(&p);
    // no TypeAlias items survive
    assert!(
        !e.items
            .iter()
            .any(|it| matches!(it, crate::ast::Item::TypeAlias { .. })),
        "alias items leaked"
    );
    // f's param + return are now `int`
    if let crate::ast::Item::Function(f) = e
        .items
        .iter()
        .find(|it| matches!(it, crate::ast::Item::Function(_)))
        .unwrap()
    {
        assert!(
            matches!(&f.params[0].ty, crate::ast::Type::Named { name, .. } if name == "int"),
            "param not de-aliased: {:?}",
            f.params[0].ty
        );
        assert!(
            matches!(&f.ret, Some(crate::ast::Type::Named { name, .. }) if name == "int"),
            "return not de-aliased: {:?}",
            f.ret
        );
    } else {
        panic!("no function item");
    }
}

#[test]
fn resolve_maps_primitives_and_list() {
    use crate::ast::Type;
    use crate::token::Span;
    let sp = Span {
        start: 0,
        len: 1,
        line: 1,
        col: 1,
    };
    let mut c = Checker::new();
    assert_eq!(
        c.resolve_type(&Type::Named {
            name: "int".into(),
            args: vec![],
            span: sp
        }),
        Ty::Int
    );
    let list = Type::Named {
        name: "List".into(),
        args: vec![Type::Named {
            name: "int".into(),
            args: vec![],
            span: sp,
        }],
        span: sp,
    };
    assert_eq!(c.resolve_type(&list), Ty::List(Box::new(Ty::Int)));
    assert_eq!(c.errors.len(), 0);
}

#[test]
fn unknown_type_in_var_decl_errors() {
    let errs = errors_of("function main() { Nope n = 0; }");
    assert!(
        errs.iter().any(|e| e.message.contains("unknown type")),
        "{errs:?}"
    );
}

#[test]
fn optional_type_is_now_supported() {
    // `T?` was deferred in M1; M3 S2 makes it a real type (here a widened `0 : int?`).
    assert!(errors_of("function main() { int? n = 0; }").is_empty());
}

#[test]
fn decimal_type_is_deferred_corner() {
    let errs = errors_of("function main() { decimal d = 0; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("decimal") && e.message.contains("not yet supported")),
        "{errs:?}"
    );
}

#[test]
fn var_decl_type_mismatch_errors() {
    let errs = errors_of("function main() { int n = true; }");
    assert!(
        errs.iter().any(|e| e.message.contains("expected `int`")),
        "{errs:?}"
    );
}

#[test]
fn good_var_decl_and_arithmetic_ok() {
    assert!(errors_of("function main() { int a = 1; int b = a + 2; }").is_empty());
}

#[test]
fn arithmetic_mixing_int_float_errors() {
    let errs = errors_of("function main() { float x = 1 + 2.0; }");
    assert!(!errs.is_empty(), "mixing int and float must error");
}

#[test]
fn if_condition_must_be_bool() {
    let errs = errors_of("function main() { if (1) { } }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("condition must be `bool`")),
        "{errs:?}"
    );
}

#[test]
fn equality_requires_same_type() {
    let errs = errors_of("function main() { bool b = 1 == true; }");
    assert!(
        errs.iter().any(|e| e.message.contains("cross-type")),
        "{errs:?}"
    );
}

#[test]
fn unknown_identifier_errors() {
    let errs = errors_of("function main() { int n = missing; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("unknown identifier")),
        "{errs:?}"
    );
}

#[test]
fn block_scoping_pops_bindings() {
    let errs = errors_of("function main() { { int x = 1; } int y = x; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("unknown identifier")),
        "{errs:?}"
    );
}

#[test]
fn return_type_checked_against_signature() {
    let errs = errors_of("function f() -> int { return true; }");
    assert!(
        errs.iter().any(|e| e.message.contains("expected `int`")),
        "{errs:?}"
    );
}

#[test]
fn function_call_arity_and_type_checked() {
    assert!(errors_of(
        "function inc(int n) -> int { return n + 1; } function main() { int x = inc(1); }"
    )
    .is_empty());
    let bad_arity = errors_of(
        "function inc(int n) -> int { return n; } function main() { int x = inc(1, 2); }",
    );
    assert!(
        bad_arity
            .iter()
            .any(|e| e.message.contains("expects 1 argument")),
        "{bad_arity:?}"
    );
    let bad_type = errors_of(
        "function inc(int n) -> int { return n; } function main() { int x = inc(true); }",
    );
    assert!(
        bad_type.iter().any(|e| e.message.contains("argument 1")),
        "{bad_type:?}"
    );
}

#[test]
fn unknown_function_call_errors() {
    let errs = errors_of("function main() { nope(); }");
    assert!(
        errs.iter().any(|e| e.message.contains("unknown function")),
        "{errs:?}"
    );
}

#[test]
fn overloaded_functions_by_arity_are_legal() {
    // M-RT overloading: same name, distinct parameter signatures, same return type — a valid
    // overload set (was rejected pre-overloading).
    let errs = errors_of("function f() {} function f(int n) {}");
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn overloaded_functions_by_type_are_legal() {
    let errs = errors_of(
        "function show(int x) -> int { return x; } \
             function show(string s) -> int { return 1; }",
    );
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn overload_set_must_share_return_type() {
    let errs = errors_of(
        "function f(int x) -> int { return x; } \
             function f(string s) -> string { return s; }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERLOAD-RETURN")),
        "{errs:?}"
    );
}

#[test]
fn overload_set_rejects_identical_signatures() {
    let errs = errors_of("function f(int x) {} function f(int y) {}");
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERLOAD-DUPLICATE")),
        "{errs:?}"
    );
}

#[test]
fn generic_function_cannot_be_overloaded() {
    let errs = errors_of(
        "function id<T>(T x) -> T { return x; } \
             function id(int n) -> int { return n; }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERLOAD-GENERIC")),
        "{errs:?}"
    );
}

#[test]
fn overloaded_call_with_no_matching_argument_type_errors() {
    let errs = errors_of(
        "function show(int x) -> int { return x; } \
             function show(string s) -> int { return 1; } \
             function main() { var r = show(true); }",
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-OVERLOAD-NO-MATCH")),
        "{errs:?}"
    );
}

#[test]
fn println_accepts_string() {
    assert!(errors_of(
        r#"import Core.Console;
function main() { Console.println("hi"); }"#
    )
    .is_empty());
}

#[test]
fn console_println_rejects_non_string() {
    // The native's signature is `(string)`, so an `int` argument is a type error (M3 Wave 1).
    let errs = errors_of(
        r#"import Core.Console;
function main() { Console.println(42); }"#,
    );
    assert!(
        errs.iter().any(|e| e.message.contains("Console.println")),
        "{errs:?}"
    );
}

#[test]
fn bare_println_is_unknown_function() {
    // The global `println` is retired: a bare call now resolves as an unknown free function.
    let errs = errors_of(r#"function main() { println("hi"); }"#);
    assert!(
        errs.iter()
            .any(|e| e.message.contains("unknown function") && e.message.contains("println")),
        "{errs:?}"
    );
}

#[test]
fn console_println_without_import_errors() {
    // "nothing in the wind": without `import Core.Console;`, the qualifier is unbound, so the
    // member call cannot resolve to the native and is an error.
    let errs = errors_of(r#"function main() { Console.println("hi"); }"#);
    assert!(!errs.is_empty(), "expected an error without the import");
}

#[test]
fn generic_native_call_infers_and_substitutes() {
    // A generic native (`Map.keys(Map<K,V>) -> List<K>`, `List.reverse(List<T>) -> List<T>`) is
    // unified at the call site exactly like a generic free function — its `Ty::Param` resolves to
    // the concrete argument types, so a well-typed program type-checks clean (M-RT S7b).
    assert!(errors_of(
        r#"package Main;
import Core.Console;
import Core.List;
import Core.Map;
function main() {
    var nums = [1, 2, 3];
    var rev = List.reverse(nums);
    var total = List.sum(rev);
    var ages = ["a" => 10, "b" => 20];
    var ks = Map.keys(ages);
    var n = Map.size(ages);
    Console.println("{total} {n}");
    for (string k in ks) { Console.println(k); }
}"#
    )
    .is_empty());
}

#[test]
fn generic_native_key_type_mismatch_errors() {
    // `Map.has(Map<string,int>, K)` unifies `K = string` from the receiver, so an `int` key is a
    // type error — the unifier propagates the binding across arguments.
    let errs = errors_of(
        r#"package Main;
import Core.Map;
function main() {
    var ages = ["a" => 10];
    var bad = Map.has(ages, 7);
}"#,
    );
    assert!(
        errs.iter().any(|e| e.message.contains("Map.has")),
        "{errs:?}"
    );
}

#[test]
fn local_shadowing_imported_qualifier_errors() {
    // A value binding may not shadow an imported module qualifier (keeps all backends
    // consistent — see `declare`). Coded `E-SHADOW-IMPORT`. (Stdlib qualifiers are now
    // PascalCase, so a camelCase local can never collide with one — the guard still bites a
    // lowercase user-package leaf, which is what this exercises.)
    let errs = errors_of(
        r#"import acme.helper;
function main() { int helper = 0; int x = helper; }"#,
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-SHADOW-IMPORT")),
        "{errs:?}"
    );
}

#[test]
fn html_literal_bad_hole_is_coded() {
    // A hole whose type is neither Html, string, nor a primitive is `E-HTML-HOLE` (Core.Html
    // Wave 3): there is no safe HTML rendering for an enum value.
    let errs = errors_of(
        r#"import Core.Html;
enum E { A() }
function main() { var p = html"<h1>{A()}</h1>"; }"#,
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-HTML-HOLE")),
        "{errs:?}"
    );
}

#[test]
fn html_literal_without_import_is_coded() {
    // `html"…"` desugars to Core.Html kernel calls, so the module must be imported; otherwise
    // `E-HTML-IMPORT`.
    let errs = errors_of(r#"function main() { var p = html"<h1>x</h1>"; }"#);
    assert!(
        errs.iter().any(|e| e.code == Some("E-HTML-IMPORT")),
        "{errs:?}"
    );
}

#[test]
fn local_shadowing_function_name_errors() {
    // A value binding may not shadow a top-level function name: a bare `f(…)` call dispatches
    // functions-first in the run backends but locals-first in the transpiler, so an overlap is
    // a silent four-backend divergence (made reachable once functions became first-class values
    // in M3 S3). Coded `E-SHADOW-FN`. See `declare`.
    let errs = errors_of(
        r#"function dbl(int x) -> int { return x * 2; }
function main() { var dbl = fn(int x) => x + 1000; }"#,
    );
    assert!(
        errs.iter().any(|e| e.code == Some("E-SHADOW-FN")),
        "{errs:?}"
    );
}

const SHAPE: &str = "enum Shape { Circle(float radius), Rect(float w, float h), }";

#[test]
fn variant_constructor_returns_enum() {
    let src = format!("{SHAPE} function main() {{ Shape s = Circle(2.0); }}");
    assert!(errors_of(&src).is_empty());
}

#[test]
fn variant_constructor_arg_type_checked() {
    let src = format!("{SHAPE} function main() {{ Shape s = Circle(true); }}");
    let errs = errors_of(&src);
    assert!(
        errs.iter().any(|e| e.message.contains("argument 1")),
        "{errs:?}"
    );
}

#[test]
fn list_literal_unifies_elements() {
    let src =
        format!("{SHAPE} function main() {{ List<Shape> xs = [Circle(1.0), Rect(2.0, 3.0)]; }}");
    assert!(errors_of(&src).is_empty());
}

#[test]
fn list_literal_mixed_elements_error() {
    let errs = errors_of("function main() { List<int> xs = [1, true]; }");
    assert!(
        errs.iter().any(|e| e.message.contains("list elements")),
        "{errs:?}"
    );
}

#[test]
fn for_in_binds_element_type() {
    let src = format!(
            "{SHAPE} function area(Shape s) -> float {{ return 0.0; }} \
             function main() {{ List<Shape> xs = [Circle(1.0)]; for (Shape s in xs) {{ float a = area(s); }} }}"
        );
    assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
}

#[test]
fn for_in_requires_list() {
    let errs = errors_of("function main() { for (int i in 5) { } }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("`for`-`in` requires a List")),
        "{errs:?}"
    );
}

#[test]
fn range_in_for_checks_clean_and_binds_int() {
    assert!(errors_of("function main() { for (int i in 0..5) { int x = i + 1; } }").is_empty());
    assert!(errors_of("function main() { for (int i in 0..=5) { } }").is_empty());
    // a range bound to a local is `List<int>`
    assert!(errors_of("function main() { List<int> xs = 0..3; }").is_empty());
}

#[test]
fn range_non_int_bound_is_error() {
    let errs = errors_of("function main() { for (int i in 0..3.0) { } }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("range bounds must be `int`")
                && e.code == Some("E-RANGE-TYPE")),
        "{errs:?}"
    );
}

#[test]
fn expression_if_unifies_branch_types() {
    assert!(
        errors_of("function main() { var x = if (1 < 2) { 10 } else { 20 }; int y = x; }")
            .is_empty()
    );
}

#[test]
fn expression_if_branch_type_mismatch_errors() {
    let errs = errors_of("function main() { var x = if (true) { 1 } else { false }; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("branches must share one type")),
        "{errs:?}"
    );
}

#[test]
fn expression_if_condition_must_be_bool() {
    let errs = errors_of("function main() { var x = if (3) { 1 } else { 2 }; }");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("condition must be `bool`")),
        "{errs:?}"
    );
}

#[test]
fn list_indexing_yields_element() {
    assert!(errors_of("function main() { List<int> xs = [1, 2]; int y = xs[0]; }").is_empty());
}

const GREETER: &str = "class Greeter { private string name; constructor(string name) {} function greet() -> string { return \"Hi\"; } }";

#[test]
fn constructor_call_and_method_call_ok() {
    let src = format!(
        "{GREETER} function main() {{ Greeter g = Greeter(\"Tak\"); string s = g.greet(); }}"
    );
    assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
}

#[test]
fn constructor_arg_type_checked() {
    let src = format!("{GREETER} function main() {{ Greeter g = Greeter(123); }}");
    let errs = errors_of(&src);
    assert!(
        errs.iter().any(|e| e.message.contains("argument 1")),
        "{errs:?}"
    );
}

#[test]
fn unknown_method_errors() {
    let src = format!("{GREETER} function main() {{ Greeter g = Greeter(\"x\"); g.missing(); }}");
    let errs = errors_of(&src);
    assert!(
        errs.iter()
            .any(|e| e.message.contains("no method `missing`")),
        "{errs:?}"
    );
}

#[test]
fn field_access_typed() {
    let src = "class Box { public int n; constructor(int n) {} } function main() { Box b = Box(1); int x = b.n; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn bare_field_visible_in_method() {
    let src = "class C { private string name; constructor(string name) {} function who() -> string { return name; } }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn this_outside_method_errors() {
    let errs = errors_of("function main() { string s = this; }");
    assert!(
        errs.iter().any(|e| e.message.contains("`this`")),
        "{errs:?}"
    );
}

#[test]
fn interpolation_allows_primitives() {
    assert!(errors_of("function main() { float x = 1.5; string s = \"v = {x}\"; }").is_empty());
    assert!(errors_of("function main() { int n = 3; string s = \"n = {n}\"; }").is_empty());
}

#[test]
fn interpolation_rejects_objects() {
    let src = "class C { private int n; constructor(int n) {} } function main() { C c = C(1); string s = \"{c}\"; }";
    let errs = errors_of(src);
    assert!(
        errs.iter()
            .any(|e| e.message.contains("cannot be interpolated")),
        "{errs:?}"
    );
}

#[test]
fn match_over_enum_is_typed_and_exhaustive() {
    let src = format!(
        "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => 3.14 * r * r, Rect(w, h) => w * h, }}; }}"
    );
    assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
}

#[test]
fn non_exhaustive_match_errors() {
    let src = format!(
        "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => 3.14 * r * r, }}; }}"
    );
    let errs = errors_of(&src);
    assert!(
        errs.iter()
            .any(|e| e.message.contains("non-exhaustive") && e.message.contains("Rect")),
        "{errs:?}"
    );
}

#[test]
fn non_exhaustive_match_lists_missing_variants_sorted() {
    // Variants declared out of alphabetical order; covering the middle one leaves Gamma+Beta
    // missing. The list must render sorted ("Beta, Gamma") regardless of the HashMap key order,
    // so the error message is deterministic across runs (no intermittent test/diff hazard).
    let src = "enum E { Gamma(int x), Alpha(int x), Beta(int x) } \
                   function f(E e) -> int { return match e { Alpha(x) => x, }; } \
                   function main() {}";
    let errs = errors_of(src);
    assert!(
        errs.iter().any(|e| e
            .message
            .contains("non-exhaustive match: missing Beta, Gamma")),
        "{errs:?}"
    );
}

#[test]
fn wildcard_makes_match_exhaustive() {
    let src = format!(
        "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, _ => 0.0, }}; }}"
    );
    assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
}

#[test]
fn match_arm_type_mismatch_errors() {
    let src = format!(
        "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, Rect(w, h) => true, }}; }}"
    );
    let errs = errors_of(&src);
    assert!(
        errs.iter().any(|e| e.message.contains("match arms")),
        "{errs:?}"
    );
}

#[test]
fn variant_pattern_arity_checked() {
    let src = format!(
        "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r, x) => r, Rect(w, h) => w, }}; }}"
    );
    let errs = errors_of(&src);
    assert!(
        errs.iter().any(|e| e.message.contains("expects 1 field")),
        "{errs:?}"
    );
}

#[test]
fn unknown_variant_pattern_errors() {
    let src = format!(
        "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, Triangle(x) => x, Rect(w,h) => w, }}; }}"
    );
    let errs = errors_of(&src);
    assert!(
        errs.iter()
            .any(|e| e.message.contains("no variant `Triangle`")),
        "{errs:?}"
    );
}

#[test]
fn promoted_ctor_param_is_field() {
    // Constructor promotion alone (no explicit `private int total;`) must type-check:
    // the promoted param becomes an instance field, matching the evaluator (EV-4).
    let errs = errors_of(
        "class C { constructor(private int total) {} \
               function add(int n) -> int { return total + n; } }",
    );
    assert!(errs.is_empty(), "promoted field should resolve: {errs:?}");
}

#[test]
fn explicit_field_decl_wins_over_promotion_type() {
    // Explicit field decl is authoritative regardless of member order; a promoted
    // param of the same name does not override its declared type.
    let errs = errors_of(
        "class C { private int total; constructor(private int total) {} \
               function add(int n) -> int { return total + n; } }",
    );
    assert!(
        errs.is_empty(),
        "redundant explicit+promoted (matching type) is fine: {errs:?}"
    );
}

#[test]
fn unmodified_ctor_param_is_not_a_field() {
    // A plain ctor param (no visibility modifier) is NOT promoted, so referencing it
    // bare in a method is still an unknown identifier — matches the evaluator.
    let errs = errors_of(
        "class C { constructor(int total) {} \
               function add(int n) -> int { return total + n; } }",
    );
    assert!(
        errs.iter()
            .any(|e| e.message.contains("unknown identifier")),
        "{errs:?}"
    );
}

#[test]
fn function_typed_binding_rejects_non_function() {
    // (int) -> int f = 5;  -> int not assignable to a function type
    let errs = errors_of("function main() { (int) -> int f = 5; }");
    assert!(
        errs.iter().any(|e| e.message.contains("(int) -> int")),
        "{errs:?}"
    );
}

// ---- M-RT S2: interfaces + implements ----

#[test]
fn interface_conformance_and_subtyping_ok() {
    // A class providing every interface method type-checks; its instance flows into an
    // interface-typed parameter (nominal subtyping) and an interface-typed local.
    let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> string { return \"w\"; } } \
                   function announce(Speaker s) -> string { return s.speak(); } \
                   function main() { Speaker sp = Dog(); announce(sp); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn interface_missing_method_is_unimpl() {
    let src = "interface Speaker { function speak() -> string; } \
                   class Mute implements Speaker {} \
                   function main() {}";
    let e = errors_of(src);
    assert!(e.iter().any(|d| d.code == Some("E-IFACE-UNIMPL")), "{e:?}");
}

#[test]
fn interface_wrong_signature_is_sig() {
    // `speak` must return `string`; returning `int` is a signature mismatch.
    let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> int { return 1; } } \
                   function main() {}";
    let e = errors_of(src);
    assert!(e.iter().any(|d| d.code == Some("E-IFACE-SIG")), "{e:?}");
}

#[test]
fn implements_a_non_interface_is_impl_error() {
    // `implements` must name a declared interface, not a class.
    let src = "class A {} class B implements A {} function main() {}";
    let e = errors_of(src);
    assert!(e.iter().any(|d| d.code == Some("E-IFACE-IMPL")), "{e:?}");
}

#[test]
fn interface_extends_cycle_is_rejected() {
    let src = "interface A extends B { function a() -> int; } \
                   interface B extends A { function b() -> int; } \
                   function main() {}";
    let e = errors_of(src);
    assert!(e.iter().any(|d| d.code == Some("E-IFACE-CYCLE")), "{e:?}");
}

#[test]
fn interface_is_not_assignable_to_unrelated_class() {
    // A Speaker is not a Dog: interface → concrete class is not a subtype.
    let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> string { return \"w\"; } } \
                   function main() { Speaker s = Dog(); Dog d = s; }";
    let e = errors_of(src);
    assert!(!e.is_empty(), "expected an assignability error, got none");
}

#[test]
fn instanceof_against_interface_narrows() {
    // `instanceof` accepts an interface RHS, and inside the then-block the operand is
    // smart-cast to the interface so its methods resolve.
    let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> string { return \"w\"; } } \
                   function main() { Dog d = Dog(); \
                     if (d instanceof Speaker) { d.speak(); } }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

// ---- M-mut.1: mutable locals + reassignment ----

#[test]
fn reassign_immutable_is_error() {
    let bad = errors_of("function main() { int x = 1; x = 2; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn reassign_mutable_is_ok() {
    assert!(errors_of("function main() { mutable int x = 1; x = 2; }").is_empty());
}

#[test]
fn reassign_mutable_var_inferred_is_ok() {
    assert!(errors_of("function main() { mutable var x = 1; x = 2; }").is_empty());
}

#[test]
fn reassign_type_mismatch_is_error() {
    let bad = errors_of("function main() { mutable int x = 1; x = \"s\"; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-TYPE")),
        "{bad:?}"
    );
}

#[test]
fn reassign_unknown_is_error() {
    let bad = errors_of("function main() { y = 2; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-UNKNOWN")),
        "{bad:?}"
    );
}

#[test]
fn field_assign_on_non_class_is_error() {
    // A field-set target whose object is not a class instance is `E-ASSIGN-TARGET` (M-mut.6).
    let bad = errors_of("function main() { mutable int x = 1; x.f = 2; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-TARGET")),
        "{bad:?}"
    );
}

#[test]
fn mutable_var_stays_reassignable_in_narrowed_block() {
    // smart-cast interaction (M-mut.1): the narrowed `instanceof` shadow inherits the outer
    // binding's mutability, so a `mutable` var is still reassignable inside the narrowed block.
    let src = "class Dog { constructor() {} } \
                   function main() { mutable Dog d = Dog(); \
                     if (d instanceof Dog) { d = Dog(); } }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

// ---- M-mut.2: compound-assign + ++/-- + ??= (desugar reuses the M-mut.1 Assign arm) ----

#[test]
fn compound_assign_on_mutable_is_ok() {
    for op in ["+=", "-=", "*=", "/=", "%="] {
        let src = format!("function main() {{ mutable int x = 6; x {op} 2; }}");
        assert!(errors_of(&src).is_empty(), "{op}: {:?}", errors_of(&src));
    }
}

#[test]
fn compound_assign_on_immutable_is_error() {
    // The desugar `x += 1` ⟶ `x = x + 1` inherits the immutability check.
    let bad = errors_of("function main() { int x = 1; x += 1; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn increment_on_immutable_is_error() {
    let bad = errors_of("function main() { int x = 1; x++; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn increment_on_unknown_is_error() {
    let bad = errors_of("function main() { y++; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-UNKNOWN")),
        "{bad:?}"
    );
}

#[test]
fn coalesce_assign_on_optional_is_ok() {
    // `x ??= 0` ⟶ `x = x ?? 0`: assigning the non-null `int` back into the `int?` slot is fine.
    assert!(
        errors_of("function main() { mutable int? x = null; x ??= 0; }").is_empty(),
        "{:?}",
        errors_of("function main() { mutable int? x = null; x ??= 0; }")
    );
}

#[test]
fn increment_on_mutable_is_ok() {
    assert!(errors_of("function main() { mutable int x = 0; x++; x--; }").is_empty());
}

// ---- M-mut.3: condition loops + break/continue ----

#[test]
fn while_loop_is_ok() {
    assert!(
        errors_of("function main() { mutable int i = 0; while (i < 3) { i += 1; } }").is_empty()
    );
}

#[test]
fn while_condition_must_be_bool() {
    let bad = errors_of("function main() { while (1) { } }");
    assert!(!bad.is_empty(), "expected a non-bool-condition error");
}

#[test]
fn c_for_is_ok() {
    assert!(errors_of(
            "import Core.Console; function main() { for (mutable int i = 0; i < 3; i++) { Console.println(\"{i}\"); } }"
        )
        .is_empty());
}

#[test]
fn c_for_immutable_counter_step_is_error() {
    // The counter is reassigned by the step, so it must be `mutable` (immutable-by-default).
    let bad = errors_of("function main() { for (int i = 0; i < 3; i++) { } }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn break_outside_loop_is_error() {
    let bad = errors_of("function main() { break; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-BREAK-OUTSIDE-LOOP")),
        "{bad:?}"
    );
}

#[test]
fn continue_outside_loop_is_error() {
    let bad = errors_of("function main() { continue; }");
    assert!(
        bad.iter()
            .any(|e| e.code == Some("E-CONTINUE-OUTSIDE-LOOP")),
        "{bad:?}"
    );
}

#[test]
fn break_inside_loop_is_ok() {
    assert!(errors_of(
        "function main() { mutable int i = 0; while (i < 9) { i += 1; if (i == 3) { break; } } }"
    )
    .is_empty());
}

// ---- M-mut.4a: clone with ----

#[test]
fn clone_with_valid_is_ok() {
    let src = "class P { constructor(public int x, public int y) {} } \
                   function main() { P p = P(1, 2); P q = p with { x = 9 }; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn clone_with_unknown_field_is_error() {
    let src = "class P { constructor(public int x) {} } \
                   function main() { P p = P(1); P q = p with { z = 9 }; }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-WITH-FIELD")),
        "{bad:?}"
    );
}

#[test]
fn clone_with_type_mismatch_is_error() {
    let src = "class P { constructor(public int x) {} } \
                   function main() { P p = P(1); P q = p with { x = \"s\" }; }";
    let bad = errors_of(src);
    assert!(bad.iter().any(|e| e.code == Some("E-WITH-TYPE")), "{bad:?}");
}

#[test]
fn clone_with_on_non_class_is_error() {
    let bad = errors_of("function main() { int n = 5; int m = n with { x = 1 }; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-WITH-NONCLASS")),
        "{bad:?}"
    );
}

// ---- M-mut.5: value-type element set ----

#[test]
fn list_element_set_is_ok() {
    assert!(errors_of("function main() { mutable List<int> xs = [1, 2]; xs[0] = 9; }").is_empty());
}

#[test]
fn map_element_set_is_ok() {
    let src = "function main() { mutable Map<string, int> m = [\"a\" => 1]; m[\"b\"] = 2; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn element_set_on_immutable_is_error() {
    let bad = errors_of("function main() { List<int> xs = [1, 2]; xs[0] = 9; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn element_set_wrong_value_type_is_error() {
    let bad = errors_of("function main() { mutable List<int> xs = [1]; xs[0] = \"s\"; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-TYPE")),
        "{bad:?}"
    );
}

#[test]
fn element_compound_set_is_ok() {
    // `xs[0] += 5` rides the M-mut.2 desugar (`xs[0] = xs[0] + 5`) on an index target.
    assert!(errors_of("function main() { mutable List<int> xs = [1, 2]; xs[0] += 5; }").is_empty());
}

#[test]
fn while_let_binds_inner_in_body() {
    // while-let narrows the optional to its non-null inner inside the body (desugars to if-let).
    assert!(errors_of(
            "import Core.Console; function main() { mutable int? o = 5; while (var v = o) { Console.println(\"{v}\"); o = null; } }"
        )
        .is_empty());
}

// ---- M-mut.6: shared-mutable instance field set `o.f = e` ----

#[test]
fn field_set_on_mutable_field_is_ok() {
    let src = "class P { constructor(public mutable int x) {} } \
                   function main() { P p = P(1); p.x = 2; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn field_set_on_immutable_field_is_error() {
    let src = "class P { constructor(public int x) {} } \
                   function main() { P p = P(1); p.x = 2; }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn field_set_unknown_field_is_error() {
    let src = "class P { constructor(public mutable int x) {} } \
                   function main() { P p = P(1); p.y = 2; }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-UNKNOWN")),
        "{bad:?}"
    );
}

#[test]
fn field_set_wrong_value_type_is_error() {
    let src = "class P { constructor(public mutable int x) {} } \
                   function main() { P p = P(1); p.x = \"s\"; }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-TYPE")),
        "{bad:?}"
    );
}

#[test]
fn field_set_via_this_in_method_is_ok() {
    // `this.f = e` inside a method resolves the receiver via `cur_class`; the explicit declared
    // `mutable` field is writable.
    let src = "class C { mutable int n; \
                     constructor(public mutable int seed) { this.n = seed; } \
                     function bump() -> int { this.n = this.n + 1; return this.n; } } \
                   function main() { C c = C(10); c.bump(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn field_set_through_safe_access_is_error() {
    // `o?.f = e` is a meaningless assignment target → `E-ASSIGN-TARGET`.
    let src = "class P { constructor(public mutable int x) {} } \
                   function main() { P? p = P(1); p?.x = 2; }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-TARGET")),
        "{bad:?}"
    );
}

// ---- M-mut.7: static mutable fields `ClassName.field` ----

#[test]
fn static_mutable_field_read_and_write_is_ok() {
    let src = "class C { static mutable int total = 0; } \
                   function main() { C.total = C.total + 1; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn static_write_to_immutable_is_error() {
    let src = "class C { static int x = 0; } function main() { C.x = 5; }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn static_field_without_initializer_is_error() {
    let bad = errors_of("class C { static mutable int x; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-STATIC-NO-INIT")),
        "{bad:?}"
    );
}

#[test]
fn static_field_non_const_initializer_is_error() {
    let bad = errors_of("class C { static mutable int x = 1 + 1; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-STATIC-INIT-CONST")),
        "{bad:?}"
    );
}

#[test]
fn static_field_initializer_type_mismatch_is_error() {
    let bad = errors_of("class C { static mutable int x = \"s\"; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-STATIC-INIT-TYPE")),
        "{bad:?}"
    );
}

#[test]
fn instance_field_with_initializer_is_error() {
    let bad = errors_of("class C { int x = 5; constructor() {} }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-FIELD-INIT")),
        "{bad:?}"
    );
}

#[test]
fn unknown_static_field_read_is_error() {
    let src = "import Core.Console; class C { static int x = 0; } \
                   function main() { Console.println(\"{C.y}\"); }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-STATIC-UNKNOWN")),
        "{bad:?}"
    );
}

// --- M-RT totality cluster ---

#[test]
fn never_resolves_as_a_return_type() {
    // A `-> never` function that diverges (infinite loop) type-checks clean.
    let src = "function spin() -> never { while (true) {} } function main() {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn never_is_a_reserved_builtin_type_name() {
    // Aliasing `never` is rejected exactly like aliasing `int`.
    let bad = errors_of("type never = int; function main() {}");
    assert!(
        bad.iter()
            .any(|e| e.message.contains("built-in type `never`")),
        "{bad:?}"
    );
}

#[test]
fn typed_fn_falling_off_the_end_is_error() {
    let bad = errors_of("function f() -> int { } function main() {}");
    assert!(
        bad.iter().any(|e| e.code == Some("E-MISSING-RETURN")),
        "{bad:?}"
    );
}

#[test]
fn if_both_branches_return_is_total() {
    let src = "function f(int x) -> int { if (x > 0) { return 1; } else { return 2; } } \
                   function main() {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn if_without_else_falls_through() {
    let bad = errors_of("function f(int x) -> int { if (x > 0) { return 1; } } function main() {}");
    assert!(
        bad.iter().any(|e| e.code == Some("E-MISSING-RETURN")),
        "{bad:?}"
    );
}

#[test]
fn if_no_else_then_trailing_return_is_total() {
    let src = "function f(int x) -> int { if (x > 0) { return 1; } return 2; } \
                   function main() {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn infinite_loop_tail_is_total() {
    // No explicit return, but `while (true) {}` with no break never falls through.
    let src = "function f() -> int { while (true) {} } function main() {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn while_true_with_break_still_needs_return() {
    let bad = errors_of("function f() -> int { while (true) { break; } } function main() {}");
    assert!(
        bad.iter().any(|e| e.code == Some("E-MISSING-RETURN")),
        "{bad:?}"
    );
}

#[test]
fn never_fn_that_can_return_is_error() {
    // A `-> never` body that falls through (could return normally) is rejected.
    let bad = errors_of("function f() -> never { } function main() {}");
    assert!(
        bad.iter().any(|e| e.code == Some("E-NEVER-RETURN")),
        "{bad:?}"
    );
}

#[test]
fn calling_a_never_fn_diverges() {
    // An expression statement calling a `-> never` function terminates the block, so the
    // enclosing `-> int` function needs no further return.
    let src = "function spin() -> never { while (true) {} } \
                   function f() -> int { spin(); } function main() {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn unit_fn_needs_no_return() {
    let src = "import Core.Console; function f() { Console.println(\"hi\"); } function main() {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn return_match_is_total() {
    let src = "enum E { A(), B() } \
                   function f(E e) -> int { return match e { A() => 1, B() => 2 }; } \
                   function main() {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn code_after_return_warns_unreachable_once() {
    let src = "import Core.Console; \
                   function f() -> int { return 1; Console.println(\"x\"); Console.println(\"y\"); } \
                   function main() {}";
    let warns = warnings_of(src);
    let n = warns
        .iter()
        .filter(|w| w.code == Some("W-UNREACHABLE"))
        .count();
    assert_eq!(n, 1, "exactly one dead-region warning: {warns:?}");
}

#[test]
fn clean_function_has_no_unreachable_warning() {
    let src = "function f() -> int { return 1; } function main() {}";
    assert!(
        warnings_of(src)
            .iter()
            .all(|w| w.code != Some("W-UNREACHABLE")),
        "{:?}",
        warnings_of(src)
    );
}

#[test]
fn match_arm_after_catch_all_warns() {
    let src = "function f(int x) -> int { return match x { _ => 0, 1 => 9 }; } \
                   function main() {}";
    assert!(
        warnings_of(src)
            .iter()
            .any(|w| w.code == Some("W-MATCH-UNREACHABLE")),
        "{:?}",
        warnings_of(src)
    );
}

#[test]
fn duplicate_match_literal_arm_warns() {
    let src = "function f(int x) -> int { return match x { 1 => 1, 1 => 2, _ => 0 }; } \
                   function main() {}";
    assert!(
        warnings_of(src)
            .iter()
            .any(|w| w.code == Some("W-MATCH-UNREACHABLE")),
        "{:?}",
        warnings_of(src)
    );
}

#[test]
fn exhaustive_distinct_match_has_no_unreachable_warning() {
    let src = "function f(int x) -> int { return match x { 1 => 1, 2 => 2, _ => 0 }; } \
                   function main() {}";
    assert!(
        warnings_of(src)
            .iter()
            .all(|w| w.code != Some("W-MATCH-UNREACHABLE")),
        "{:?}",
        warnings_of(src)
    );
}
