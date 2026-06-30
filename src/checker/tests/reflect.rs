//! Checker tests — Core.Reflection (`kind` / `className` / the precise `typeName` static-type pass).

use super::support::*;

#[test]
fn type_name_resolves_to_string() {
    // `Reflection.typeName(x)` resolves to `string` (usable wherever a string is expected).
    assert!(errors_of(
        r#"import Core.Reflection;
function main() -> void { string s = Reflection.typeName(42); }"#
    )
    .is_empty());
}

#[test]
fn type_name_arity_is_checked() {
    let errs = errors_of(
        r#"import Core.Reflection;
function main() -> void { string s = Reflection.typeName(1, 2); }"#,
    );
    assert!(
        errs.iter()
            .any(|e| e.message.contains("exactly one argument")),
        "{errs:?}"
    );
}

#[test]
fn type_name_is_excluded_from_ufcs() {
    // `typeName` is erased from the static type; a UFCS-produced raw call would reach the backend
    // un-erased and diverge, so `.typeName()` is not offered as UFCS — it must be called qualified.
    let errs = errors_of(
        r#"import Core.Reflection;
function main() -> void { int x = 5; string s = x.typeName(); }"#,
    );
    assert!(
        !errs.is_empty(),
        "x.typeName() must not resolve via UFCS (it is erased + qualified-only)"
    );
}

#[test]
fn class_name_and_kind_resolve() {
    // `className` is `string?`; `kind` is `string`. Both are plain natives (no static pass).
    assert!(errors_of(
        r#"import Core.Reflection;
class P { constructor(public int x) {} }
function main() -> void {
  var p = new P(1);
  string? c = Reflection.className(p);
  string k = Reflection.kind(p);
}"#
    )
    .is_empty());
}

#[test]
fn kind_and_class_name_stay_ufcs_eligible() {
    // Unlike `typeName`, `kind`/`className` are byte-identical plain natives, so UFCS still works:
    // `p.kind()` ≡ `Reflection.kind(p)`.
    assert!(errors_of(
        r#"import Core.Reflection;
class P { constructor(public int x) {} }
function main() -> void { var p = new P(1); string k = p.kind(); }"#
    )
    .is_empty());
}
