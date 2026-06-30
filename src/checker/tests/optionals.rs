//! Checker tests — optionals (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn optional_binding_and_null_discipline() {
    // an optional binding accepts `null` and a widened non-null `T`
    assert!(errors_of("function main() -> void { int? x = null; }").is_empty());
    assert!(errors_of("function main() -> void { int? y = 5; }").is_empty());
    // `null` / `T?` cannot flow into a non-optional `T`
    let e1 = errors_of("function main() -> void { int x = null; }");
    assert!(
        e1.iter().any(|d| d.code == Some("E-OPT-ASSIGN")),
        "got {e1:?}"
    );
    let e2 = errors_of("function main() -> void { int? x = null; int y = x; }");
    assert!(
        e2.iter().any(|d| d.code == Some("E-OPT-ASSIGN")),
        "got {e2:?}"
    );
}

#[test]
fn if_let_binding_and_smart_cast() {
    // smart-cast: inside the then-block, the bound name is the non-optional inner `T`
    assert!(
        errors_of("function main() -> void { int? o = 5; if (var x = o) { int y = x; } }")
            .is_empty()
    );
    // the binding is NOT in scope in the else block
    let e1 =
        errors_of("function main() -> void { int? o = 5; if (var x = o) {} else { int y = x; } }");
    assert!(
        e1.iter().any(|d| d.code == Some("E-UNKNOWN-IDENT")),
        "got {e1:?}"
    );
    // the binding is NOT in scope after the if
    let e2 = errors_of("function main() -> void { int? o = 5; if (var x = o) {} int y = x; }");
    assert!(
        e2.iter().any(|d| d.code == Some("E-UNKNOWN-IDENT")),
        "got {e2:?}"
    );
    // the scrutinee must be optional — binding a non-optional is `E-IF-LET-TYPE`
    let e3 = errors_of("function main() -> void { int n = 5; if (var x = n) {} }");
    assert!(
        e3.iter().any(|d| d.code == Some("E-IF-LET-TYPE")),
        "got {e3:?}"
    );
}

#[test]
fn force_unwrap_typing_and_lint() {
    // `opt!` unwraps `T?` to `T`; the program type-checks and emits the W-FORCE-UNWRAP lint
    let src = "function main() -> void { int? o = 5; int x = o!; }";
    assert!(errors_of(src).is_empty(), "got {:?}", errors_of(src));
    let w = warnings_of(src);
    assert!(
        w.iter().any(|d| d.code == Some("W-FORCE-UNWRAP")),
        "expected W-FORCE-UNWRAP, got {w:?}"
    );
    // force-unwrapping a non-optional is an error (nothing to unwrap)
    let e = errors_of("function main() -> void { int n = 3; int x = n!; }");
    assert!(
        e.iter().any(|d| d.code == Some("E-OPT-UNWRAP")),
        "got {e:?}"
    );
}

#[test]
fn coalesce_typing() {
    // `T? ?? T` and `null ?? T` both yield the non-optional `T`.
    assert!(errors_of("function main() -> void { int? x = null; int y = x ?? 3; }").is_empty());
    assert!(errors_of("function main() -> void { int y = null ?? 3; }").is_empty());
    // `??` on a non-optional left operand is a misuse.
    assert!(!errors_of("function main() -> void { int a = 1; int y = a ?? 3; }").is_empty());
}

#[test]
fn safe_member_access_typing() {
    let cls =
        "class Box { constructor(public int v) {} function vOf() -> int { return this.v; } } ";
    // `?.` on an optional yields an optional member, usable via `??`.
    let ok_field =
        cls.to_string() + "function main() -> void { Box? b = null; int y = (b?.v) ?? -1; }";
    assert!(
        errors_of(&ok_field).is_empty(),
        "{:?}",
        errors_of(&ok_field)
    );
    let ok_method =
        cls.to_string() + "function main() -> void { Box? b = null; int y = (b?.vOf()) ?? -1; }";
    assert!(
        errors_of(&ok_method).is_empty(),
        "{:?}",
        errors_of(&ok_method)
    );
    // plain `.` on an optional is the non-null-discipline violation → E-OPT-USE.
    let bad_field = cls.to_string() + "function main() -> void { Box? b = null; int y = b.v; }";
    let e = errors_of(&bad_field);
    assert!(e.iter().any(|d| d.code == Some("E-OPT-USE")), "got {e:?}");
    let bad_method =
        cls.to_string() + "function main() -> void { Box? b = null; int y = b.vOf(); }";
    let em = errors_of(&bad_method);
    assert!(em.iter().any(|d| d.code == Some("E-OPT-USE")), "got {em:?}");
}

#[test]
fn var_from_null_is_rejected() {
    // A bare `null` has no inferable element type — `var x = null` needs `T? x = null;`.
    let errs = errors_of("function main() -> void { var x = null; }");
    assert!(
        errs.iter().any(|d| d.code == Some("E-INFER-NULL")),
        "got {errs:?}"
    );
}

#[test]
fn optional_type_is_now_supported() {
    // `T?` was deferred in M1; M3 S2 makes it a real type (here a widened `0 : int?`).
    assert!(errors_of("function main() -> void { int? n = 0; }").is_empty());
}

#[test]
fn while_let_binds_inner_in_body() {
    // while-let narrows the optional to its non-null inner inside the body (desugars to if-let).
    assert!(errors_of(
            "import Core.Output; function main() -> void { mutable int? o = 5; while (var v = o) { Output.printLine(\"{v}\"); o = null; } }"
        )
        .is_empty());
}
