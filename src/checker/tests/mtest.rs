//! M-Test checker tests — `test "name" { … }` items, test-mode gating, and body checking.

use super::support::*;

#[test]
fn test_item_outside_test_mode_is_rejected() {
    // A `test` block in a normal build (run/runvm/transpile) must not compile — production code
    // cannot smuggle test blocks. Only `phg test` (test mode) accepts them.
    let errs = errors_of("test \"x\" { var y = 1; }");
    assert!(
        errs.iter().any(|d| d.code == Some("E-TEST-OUTSIDE-TESTS")),
        "expected E-TEST-OUTSIDE-TESTS, got {errs:?}"
    );
}

#[test]
fn test_item_in_test_mode_is_accepted() {
    let errs = test_errors_of("test \"x\" { var y = 1 + 2; }");
    assert!(
        errs.is_empty(),
        "expected no errors in test mode, got {errs:?}"
    );
}

#[test]
fn test_body_is_type_checked() {
    // The body is checked like a `-> void` function body: a real type error inside still fires.
    let errs = test_errors_of("test \"bad\" { var y = 1 + true; }");
    assert!(!errs.is_empty(), "expected a body type error in test mode");
    assert!(
        errs.iter().all(|d| d.code != Some("E-TEST-OUTSIDE-TESTS")),
        "the outside-tests gate should not fire in test mode: {errs:?}"
    );
}

#[test]
fn test_body_has_no_this() {
    // A test block captures no `this` (it is not a method).
    let errs = test_errors_of("test \"no this\" { var z = this; }");
    assert!(
        !errs.is_empty(),
        "expected an error referencing `this` in a test body"
    );
}
