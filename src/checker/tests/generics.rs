//! Checker tests — generics (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn generic_identity_typechecks_and_infers() {
    // A generic function used at two distinct concrete types — both inferred clean.
    let ok = errors_of(
        "function id<T>(T x) -> T { return x; } \
             function main() -> void { int n = id(42); string s = id(\"hi\"); }",
    );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_call_result_is_substituted() {
    // `id(42)` returns `int`, so binding it to a `string` is a type error (the return type was
    // unified to the concrete argument type, not left abstract).
    let bad = errors_of(
        "function id<T>(T x) -> T { return x; } function main() -> void { string s = id(42); }",
    );
    assert!(!bad.is_empty(), "expected a type error, got none");
}

#[test]
fn generic_unifies_through_list_and_function() {
    // `firstOr<T>(List<T>, T) -> T` binds T from the list element; `applyTwice<T>(T, (T)->T) -> T`
    // unifies a function-typed parameter. Both infer clean against concrete arguments.
    let ok = errors_of(
            "function firstOr<T>(List<T> xs, T fallback) -> T { for (T x in xs) { return x; } return fallback; } \
             function applyTwice<T>(T x, (T) -> T f) -> T { return f(f(x)); } \
             function main() -> void { List<int> xs = [1, 2]; int a = firstOr(xs, 0); int b = applyTwice(5, function(int v) => v + 1); }",
        );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_argument_must_unify_consistently() {
    // Two `T` parameters bound to incompatible concrete types — the second arg cannot match the
    // `int` bound from the first.
    let bad = errors_of(
        "function pairEq<T>(T a, T b) -> bool { return true; } \
             function main() -> void { bool r = pairEq(1, \"x\"); }",
    );
    assert!(!bad.is_empty(), "expected a unification error, got none");
}

#[test]
fn type_param_shadowing_builtin_is_rejected() {
    let e = errors_of("function f<int>(int x) -> int { return x; } function main() -> void {}");
    assert!(
        e.iter().any(|d| d.code == Some("E-GENERIC-PARAM")),
        "got {e:?}"
    );
}

#[test]
fn duplicate_type_param_is_rejected() {
    let e = errors_of("function f<T, T>(T x) -> T { return x; } function main() -> void {}");
    assert!(
        e.iter().any(|d| d.code == Some("E-GENERIC-PARAM")),
        "got {e:?}"
    );
}

#[test]
fn type_param_must_be_pascalcase() {
    let e = errors_of("function f<t>(t x) -> t { return x; } function main() -> void {}");
    assert!(e.iter().any(|d| d.code == Some("E-TYPE-CASE")), "got {e:?}");
}

#[test]
fn generic_method_typechecks_and_infers() {
    // A generic method on a non-generic class, inferred from arguments at two distinct types.
    let ok = errors_of(
        "class U { function id<T>(T x) -> T { return x; } } \
             function main() -> void { var u = new U(); int n = u.id(42); string s = u.id(\"hi\"); }",
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
             function main() -> void { var u = U(); string s = u.id(42); }",
    );
    assert!(!bad.is_empty(), "expected a type error, got none");
}

#[test]
fn generic_method_argument_must_unify_consistently() {
    // Two `T` parameters of a method bound to incompatible concrete types.
    let bad = errors_of(
        "class U { function pairEq<T>(T a, T b) -> bool { return true; } } \
             function main() -> void { var u = U(); bool r = u.pairEq(1, \"x\"); }",
    );
    assert!(!bad.is_empty(), "expected a unification error, got none");
}

#[test]
fn generic_method_param_must_be_pascalcase() {
    let e =
        errors_of("class U { function f<t>(t x) -> t { return x; } } function main() -> void {}");
    assert!(e.iter().any(|d| d.code == Some("E-TYPE-CASE")), "got {e:?}");
}

#[test]
fn generic_class_construction_infers_and_substitutes() {
    // `Box(7)` infers T=int; `get()` returns int; a two-parameter `Pair<A, B>` binds each
    // parameter independently from its constructor argument.
    let ok = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             class Pair<A, B> { constructor(private A first, private B second) {} \
                function left() -> A { return this.first; } function right() -> B { return this.second; } } \
             function main() -> void { var b = new Box(7); int x = b.get(); \
                var p = new Pair(1, \"s\"); int l = p.left(); string r = p.right(); }",
        );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_class_result_is_substituted() {
    // `Box(7).get()` is int; binding it to a string is an error — proving use-site reification
    // (the instance carries `T=int`, recovered at the member access), not an abstract/mixed result.
    let bad = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             function main() -> void { var b = Box(7); string s = b.get(); }",
        );
    assert!(!bad.is_empty(), "expected a type error, got none");
}

#[test]
fn generic_class_method_param_substituted() {
    // A method *taking* a `T` rejects a wrong-typed argument at the instance's concrete type.
    let bad = errors_of(
            "class Box<T> { constructor(private T value) {} function orElse(T f) -> T { return this.value; } } \
             function main() -> void { var b = Box(7); int y = b.orElse(\"x\"); }",
        );
    assert!(!bad.is_empty(), "expected an argument type error, got none");
}

#[test]
fn generic_class_annotation_arity_checked() {
    // A bare `Box` annotation (no type argument) on a generic class is an arity error.
    let bad = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             function main() -> void { Box b = Box(7); }",
        );
    assert!(!bad.is_empty(), "expected an arity error, got none");
}

#[test]
fn generic_class_explicit_type_argument_ok() {
    let ok = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             function main() -> void { Box<int> b = new Box(7); int x = b.get(); }",
        );
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_enum_construction_infers_and_binds() {
    // `Some(7)` infers `Option<int>`; matching it binds the payload at the concrete int, so using
    // the binding where an int is expected is clean.
    let ok = errors_of(&format!(
        "{OPTION} function main() -> void {{ var o = new Some(7); \
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
        "{OPTION} function main() -> void {{ var o = Some(7); \
             string s = match o {{ Some(n) => n, None() => \"x\" }}; }}"
    ));
    assert!(!bad.is_empty(), "expected a type error, got none");
}

#[test]
fn generic_enum_annotation_arity_checked() {
    // A bare `Option` annotation (no type argument) on a generic enum is an arity error.
    let bad = errors_of(&format!(
        "{OPTION} function main() -> void {{ Option o = Some(7); }}"
    ));
    assert!(!bad.is_empty(), "expected an arity error, got none");
}

#[test]
fn generic_enum_annotated_non_inferring_variant_ok() {
    // `None` mentions no `T`, so it cannot infer the argument — annotating the binding fixes it.
    let ok = errors_of(&format!(
        "{OPTION} function main() -> void {{ Option<int> n = new None(); \
             int x = match n {{ Some(v) => v, None() => 0 }}; }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_enum_two_params_independent() {
    // `Result<T, E>` binds `T` from `Ok`'s argument and `E` from `Err`'s, independently.
    let ok = errors_of(&format!(
        "{RESULT} function ok() -> Result<int, string> {{ return new Ok(1); }} \
             function bad() -> Result<int, string> {{ return new Err(\"no\"); }} \
             function main() -> void {{ string r = match ok() {{ Ok(v) => \"v\", Err(e) => e }}; }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_enum_variant_arity_checked() {
    // A generic variant constructor still checks its own arity: `Some` takes exactly one field.
    let bad = errors_of(&format!(
        "{OPTION} function main() -> void {{ var o = Some(1, 2); }}"
    ));
    assert!(!bad.is_empty(), "expected an arity error, got none");
}

#[test]
fn generic_enum_param_must_be_pascalcase() {
    // A type parameter shadowing a built-in type name is `E-GENERIC-PARAM`.
    let bad = errors_of("enum Box<int> { Wrap(int x) } function main() -> void {}");
    assert!(
        bad.iter().any(|d| d.code == Some("E-GENERIC-PARAM")),
        "expected E-GENERIC-PARAM, got {bad:?}"
    );
}

#[test]
fn erase_generics_strips_enum_type_params() {
    use crate::ast::{Item, Type};
    let e = erase_generics(prog(&format!("{OPTION} function main() -> void {{}}")));
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
    let bad =
        errors_of("enum Color { Red, Green } function main() -> void { Color<int> c = Red(); }");
    assert!(!bad.is_empty(), "expected an arity error, got none");
}

#[test]
fn generic_class_param_must_be_pascalcase() {
    let e = errors_of(
        "class Box<t> { constructor(private t value) {} } function main() -> void { var b = Box(7); }",
    );
    assert!(e.iter().any(|d| d.code == Some("E-TYPE-CASE")), "got {e:?}");
}

#[test]
fn method_type_param_shadowing_class_param_rejected() {
    let e = errors_of(
        "class Box<T> { constructor(private T value) {} function id<T>(T x) -> T { return x; } } \
             function main() -> void { var b = Box(7); }",
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
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } function main() -> void {}",
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
    let p = prog("class U { function id<T>(T x) -> T { return x; } } function main() -> void {}");
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
    let p = prog("function id<T>(T x) -> T { return x; } function main() -> void {}");
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
fn generic_native_call_infers_and_substitutes() {
    // A generic native (`Map.keys(Map<K,V>) -> List<K>`, `List.reverse(List<T>) -> List<T>`) is
    // unified at the call site exactly like a generic free function — its `Ty::Param` resolves to
    // the concrete argument types, so a well-typed program type-checks clean (M-RT S7b).
    assert!(errors_of(
        r#"package Main;
import Core.Output;
import Core.List;
import Core.Map;
function main() -> void {
    var nums = [1, 2, 3];
    var rev = List.reverse(nums);
    var total = List.sum(rev);
    var ages = ["a" => 10, "b" => 20];
    var ks = Map.keys(ages);
    var n = Map.size(ages);
    Output.printLine("{total} {n}");
    for (string k in ks) { Output.printLine(k); }
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
function main() -> void {
    var ages = ["a" => 10];
    var bad = Map.has(ages, 7);
}"#,
    );
    assert!(
        errs.iter().any(|e| e.message.contains("Map.has")),
        "{errs:?}"
    );
}

// ── generic type-argument invariance (Soundness Batch B, finding #2) ─────────────────────────────

const BOX: &str =
    "class Box<T> { constructor(public T value) {} function get() -> T { return this.value; } }";

#[test]
fn generic_class_different_args_is_rejected() {
    // `Box<string>` must NOT flow into a `Box<int>` slot — the reflexive same-head short-circuit used
    // to accept it, smuggling a string into a statically-`int` slot (finding #2).
    let bad = errors_of(&format!(
        "{BOX} function main() -> void {{ Box<string> bs = new Box(\"hi\"); Box<int> bi = bs; }}"
    ));
    assert!(
        !bad.is_empty(),
        "expected rejection of Box<string> -> Box<int>"
    );
}

#[test]
fn generic_class_same_args_is_ok() {
    // Regression guard: identical type arguments still assign cleanly.
    let ok = errors_of(&format!(
        "{BOX} function main() -> void {{ Box<int> a = new Box(7); Box<int> b = a; }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn generic_enum_different_args_is_rejected() {
    // Same hole closes for generic enums (`Option<string>` !<: `Option<int>`).
    let bad = errors_of(
        "enum Option<T> { Some(T value), None() } \
         function main() -> void { Option<string> os = new Some(\"hi\"); Option<int> oi = os; }",
    );
    assert!(
        !bad.is_empty(),
        "expected rejection of Option<string> -> Option<int>"
    );
}

#[test]
fn nominal_subtype_still_assignable() {
    // The fix must not break real subtyping: a subclass still flows into a superclass slot.
    let ok = errors_of(
        "open class Animal { constructor() {} } \
         class Dog extends Animal {} \
         function main() -> void { Animal a = new Dog(); }",
    );
    assert!(
        ok.is_empty(),
        "expected clean subtype assignment, got {ok:?}"
    );
}
