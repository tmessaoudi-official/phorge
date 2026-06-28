//! Checker tests — mutation (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn reassign_immutable_is_error() {
    let bad = errors_of("function main() -> void { int x = 1; x = 2; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn reassign_mutable_is_ok() {
    assert!(errors_of("function main() -> void { mutable int x = 1; x = 2; }").is_empty());
}

#[test]
fn reassign_mutable_var_inferred_is_ok() {
    assert!(errors_of("function main() -> void { mutable var x = 1; x = 2; }").is_empty());
}

#[test]
fn reassign_type_mismatch_is_error() {
    let bad = errors_of("function main() -> void { mutable int x = 1; x = \"s\"; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-TYPE")),
        "{bad:?}"
    );
}

#[test]
fn reassign_unknown_is_error() {
    let bad = errors_of("function main() -> void { y = 2; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-UNKNOWN")),
        "{bad:?}"
    );
}

#[test]
fn field_assign_on_non_class_is_error() {
    // A field-set target whose object is not a class instance is `E-ASSIGN-TARGET` (M-mut.6).
    let bad = errors_of("function main() -> void { mutable int x = 1; x.f = 2; }");
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
                   function main() -> void { mutable Dog d = new Dog(); \
                     if (d instanceof Dog) { d = new Dog(); } }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn compound_assign_on_mutable_is_ok() {
    for op in ["+=", "-=", "*=", "/=", "%="] {
        let src = format!("function main() -> void {{ mutable int x = 6; x {op} 2; }}");
        assert!(errors_of(&src).is_empty(), "{op}: {:?}", errors_of(&src));
    }
}

#[test]
fn compound_assign_on_immutable_is_error() {
    // The desugar `x += 1` ⟶ `x = x + 1` inherits the immutability check.
    let bad = errors_of("function main() -> void { int x = 1; x += 1; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn increment_on_immutable_is_error() {
    let bad = errors_of("function main() -> void { int x = 1; x++; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn increment_on_unknown_is_error() {
    let bad = errors_of("function main() -> void { y++; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-UNKNOWN")),
        "{bad:?}"
    );
}

#[test]
fn coalesce_assign_on_optional_is_ok() {
    // `x ??= 0` ⟶ `x = x ?? 0`: assigning the non-null `int` back into the `int?` slot is fine.
    assert!(
        errors_of("function main() -> void { mutable int? x = null; x ??= 0; }").is_empty(),
        "{:?}",
        errors_of("function main() -> void { mutable int? x = null; x ??= 0; }")
    );
}

#[test]
fn increment_on_mutable_is_ok() {
    assert!(errors_of("function main() -> void { mutable int x = 0; x++; x--; }").is_empty());
}

#[test]
fn clone_with_valid_is_ok() {
    let src = "class P { constructor(public int x, public int y) {} } \
                   function main() -> void { P p = new P(1, 2); P q = p with { x = 9 }; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn clone_with_unknown_field_is_error() {
    let src = "class P { constructor(public int x) {} } \
                   function main() -> void { P p = new P(1); P q = p with { z = 9 }; }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-WITH-FIELD")),
        "{bad:?}"
    );
}

#[test]
fn clone_with_type_mismatch_is_error() {
    let src = "class P { constructor(public int x) {} } \
                   function main() -> void { P p = new P(1); P q = p with { x = \"s\" }; }";
    let bad = errors_of(src);
    assert!(bad.iter().any(|e| e.code == Some("E-WITH-TYPE")), "{bad:?}");
}

#[test]
fn clone_with_on_non_class_is_error() {
    let bad = errors_of("function main() -> void { int n = 5; int m = n with { x = 1 }; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-WITH-NONCLASS")),
        "{bad:?}"
    );
}

#[test]
fn list_element_set_is_ok() {
    assert!(
        errors_of("function main() -> void { mutable List<int> xs = [1, 2]; xs[0] = 9; }")
            .is_empty()
    );
}

#[test]
fn map_element_set_is_ok() {
    let src =
        "function main() -> void { mutable Map<string, int> m = [\"a\" => 1]; m[\"b\"] = 2; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn element_set_on_immutable_is_error() {
    let bad = errors_of("function main() -> void { List<int> xs = [1, 2]; xs[0] = 9; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn element_set_wrong_value_type_is_error() {
    let bad = errors_of("function main() -> void { mutable List<int> xs = [1]; xs[0] = \"s\"; }");
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-TYPE")),
        "{bad:?}"
    );
}

#[test]
fn element_compound_set_is_ok() {
    // `xs[0] += 5` rides the M-mut.2 desugar (`xs[0] = xs[0] + 5`) on an index target.
    assert!(
        errors_of("function main() -> void { mutable List<int> xs = [1, 2]; xs[0] += 5; }")
            .is_empty()
    );
}

#[test]
fn field_set_on_mutable_field_is_ok() {
    let src = "class P { constructor(public mutable int x) {} } \
                   function main() -> void { P p = new P(1); p.x = 2; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn field_set_on_immutable_field_is_error() {
    let src = "class P { constructor(public int x) {} } \
                   function main() -> void { P p = new P(1); p.x = 2; }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
        "{bad:?}"
    );
}

#[test]
fn field_set_unknown_field_is_error() {
    let src = "class P { constructor(public mutable int x) {} } \
                   function main() -> void { P p = new P(1); p.y = 2; }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-UNKNOWN")),
        "{bad:?}"
    );
}

#[test]
fn field_set_wrong_value_type_is_error() {
    let src = "class P { constructor(public mutable int x) {} } \
                   function main() -> void { P p = new P(1); p.x = \"s\"; }";
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
                   function main() -> void { C c = new C(10); discard c.bump(); }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn field_set_through_safe_access_is_error() {
    // `o?.f = e` is a meaningless assignment target → `E-ASSIGN-TARGET`.
    let src = "class P { constructor(public mutable int x) {} } \
                   function main() -> void { P? p = new P(1); p?.x = 2; }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-ASSIGN-TARGET")),
        "{bad:?}"
    );
}

#[test]
fn static_mutable_field_read_and_write_is_ok() {
    let src = "class C { static mutable int total = 0; } \
                   function main() -> void { C.total = C.total + 1; }";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn static_write_to_immutable_is_error() {
    let src = "class C { static int x = 0; } function main() -> void { C.x = 5; }";
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
fn static_field_expression_initializer_is_ok() {
    // Feature B-static lifted the literal-only restriction: a static field may now carry an arbitrary
    // expression (evaluated once at program start). See `checker::tests::field_init` / `static_init`.
    let src = "class C { static mutable int x = 1 + 1; } function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
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
fn instance_field_with_initializer_is_ok() {
    // Feature B lifted the old E-FIELD-INIT rejection: an instance field may now carry an expression
    // initializer (evaluated per-instance at construction). See `checker::tests::field_init`.
    let src = "class C { int x = 5; constructor() {} } function main() -> void {}";
    assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
}

#[test]
fn unknown_static_field_read_is_error() {
    let src = "import Core.Console; class C { static int x = 0; } \
                   function main() -> void { Console.println(\"{C.y}\"); }";
    let bad = errors_of(src);
    assert!(
        bad.iter().any(|e| e.code == Some("E-STATIC-UNKNOWN")),
        "{bad:?}"
    );
}
