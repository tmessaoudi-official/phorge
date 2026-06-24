//! Checker tests — intersections (M-Decomp W2b, by language feature).

use super::support::*;

#[test]
fn intersection_param_accepts_a_class_implementing_both() {
    // all-members-required-in: a Badge (implements Drawable AND Named) flows into the intersection.
    let ok = errors_of(&format!(
        "{IFACES} function describe(Drawable & Named x) -> string {{ return x.draw(); }} \
             function main() -> void {{ string s = describe(new Badge(\"b\")); }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn intersection_member_access_reaches_each_member() {
    // A method from *each* member interface is in scope on the intersection value.
    let ok = errors_of(&format!(
            "{IFACES} function f(Drawable & Named x) -> string {{ return \"{{x.draw()}} {{x.name()}}\"; }} \
             function main() -> void {{ string s = f(new Badge(\"b\")); }}"
        ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn intersection_flows_out_to_a_single_member() {
    // some-member-out: A & B is assignable to a slot typed as just one member.
    let ok = errors_of(&format!(
        "{IFACES} function onlyDraw(Drawable d) -> string {{ return d.draw(); }} \
             function f(Drawable & Named x) -> string {{ return onlyDraw(x); }} \
             function main() -> void {{}}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn intersection_one_class_plus_interface_is_allowed() {
    // D1: at most one concrete class plus interfaces is a well-formed intersection.
    let ok = errors_of(&format!(
        "{IFACES} function f(Badge & Drawable x) -> string {{ return x.draw(); }} \
             function main() -> void {{ string s = f(new Badge(\"b\")); }}"
    ));
    assert!(ok.is_empty(), "expected clean, got {ok:?}");
}

#[test]
fn intersection_rejects_two_classes() {
    let bad = errors_of(&format!(
        "{SHAPES} function f(Circle & Square x) -> void {{}} function main() -> void {{}}"
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
        "{IFACES} function f(int & Drawable x) -> void {{}} function main() -> void {{}}"
    ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-INTERSECT-MEMBER")),
        "{bad:?}"
    );
}

#[test]
fn intersection_arity_collapse_is_error() {
    let bad = errors_of(&format!(
        "{IFACES} function f(Drawable & Drawable x) -> void {{}} function main() -> void {{}}"
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
             function f(A & B x) -> void {} function main() -> void {}",
    );
    assert!(
        bad.iter().any(|e| e.code == Some("E-INTERSECT-SIG")),
        "{bad:?}"
    );
}

#[test]
fn intersection_member_access_unknown_is_error() {
    let bad = errors_of(&format!(
            "{IFACES} function f(Drawable & Named x) -> int {{ return x.nope(); }} function main() -> void {{}}"
        ));
    assert!(
        bad.iter().any(|e| e.code == Some("E-INTERSECT-NO-MEMBER")),
        "{bad:?}"
    );
}
