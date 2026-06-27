//! Checker tests — `Secret<T>` (Fork B). The guarantee is by construction: a `Secret` is opaque
//! (private field, no display), so printing/interpolating it is a type error and `.expose()` is the
//! only read path. `W-SECRET` is a non-fatal lint nudging when an exposed plaintext flows *directly*
//! into a sink. These tests declare the `Secret` class inline (the lint keys on the type name, so the
//! `cli::inject_secret_prelude` injection is not needed here).

use super::support::*;

/// The injected `Secret<T>` shape, declared inline so `check()` (which does not inject preludes) sees it.
const SECRET: &str =
    "class Secret<T> { constructor(private T value) {} function expose(): T { return this.value; } }";

fn src_of(body: &str) -> String {
    format!("import Core.Console; import Core.Text; import Core.File; {SECRET} {body}")
}

fn warns(body: &str, code: &str) -> bool {
    warnings_of(&src_of(body))
        .iter()
        .any(|w| w.code == Some(code))
}

fn errs(body: &str) -> Vec<Diagnostic> {
    errors_of(&src_of(body))
}

#[test]
fn secret_field_is_private() {
    let body = "function main() -> void { var s = new Secret(\"k\"); var v = s.value; }";
    let es = errs(body);
    assert!(
        es.iter().any(|e| e.code == Some("E-FIELD-VISIBILITY")),
        "{es:?}"
    );
}

#[test]
fn printing_a_secret_is_a_type_error() {
    let body = "function main() -> void { var s = new Secret(\"k\"); Console.println(s); }";
    let es = errs(body);
    assert!(
        !es.is_empty() && es.iter().any(|e| e.message.contains("string")),
        "{es:?}"
    );
}

#[test]
fn expose_is_the_read_path_and_checks_clean() {
    // A legitimate expose away from a sink: no error, no W-SECRET.
    let body =
        "function main() -> void { var s = new Secret(\"k\"); int n = Text.length(s.expose()); }";
    assert!(errs(body).is_empty(), "{:?}", errs(body));
    assert!(!warns(body, "W-SECRET"));
}

#[test]
fn expose_directly_into_println_warns() {
    let body =
        "function main() -> void { var s = new Secret(\"k\"); Console.println(s.expose()); }";
    assert!(warns(body, "W-SECRET"));
}

#[test]
fn expose_directly_into_file_write_warns() {
    let body =
        "function main() -> void { var s = new Secret(\"k\"); File.write(\"/tmp/x\", s.expose()); }";
    assert!(warns(body, "W-SECRET"));
}

#[test]
fn expose_laundered_through_a_local_is_not_flagged() {
    // Documented scope: the lint is syntactic on the direct sink argument; a value bound to a local
    // first is not flagged (the type-system non-printability is the real guarantee).
    let body = "function main() -> void { var s = new Secret(\"k\"); string p = s.expose(); Console.println(p); }";
    assert!(!warns(body, "W-SECRET"));
}
