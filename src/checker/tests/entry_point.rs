//! Checker tests — `main` entry-point signature (Batch-1 B).
//!
//! `main` is the program entry point: it accepts **zero or one** parameters (the one allowed param is
//! `List<string>`, the program argv), and returns `void` or `int` (the process exit code). Any other
//! shape is `E-MAIN-SIGNATURE`. Only the entry `main` is constrained — a library/user function named
//! `main` is mangled away by the loader, so this never bites ordinary code.

use super::support::*;

fn has(src: &str, code: &str) -> bool {
    errors_of(src).iter().any(|e| e.code == Some(code))
}

#[test]
fn main_void_no_args_ok() {
    assert!(!has("function main(): void { }", "E-MAIN-SIGNATURE"));
}

#[test]
fn main_int_no_args_ok() {
    assert!(!has(
        "function main(): int { return 0; }",
        "E-MAIN-SIGNATURE"
    ));
}

#[test]
fn main_argv_void_ok() {
    assert!(!has(
        "function main(List<string> args): void { }",
        "E-MAIN-SIGNATURE"
    ));
}

#[test]
fn main_argv_int_ok() {
    assert!(!has(
        "function main(List<string> args): int { return 0; }",
        "E-MAIN-SIGNATURE"
    ));
}

#[test]
fn main_non_list_param_rejected() {
    let src = "function main(int x): void { }";
    assert!(has(src, "E-MAIN-SIGNATURE"), "{:?}", errors_of(src));
}

#[test]
fn main_wrong_list_elem_rejected() {
    let src = "function main(List<int> a): void { }";
    assert!(has(src, "E-MAIN-SIGNATURE"), "{:?}", errors_of(src));
}

#[test]
fn main_extra_param_rejected() {
    let src = "function main(List<string> a, int b): int { return 0; }";
    assert!(has(src, "E-MAIN-SIGNATURE"), "{:?}", errors_of(src));
}

#[test]
fn main_string_return_rejected() {
    let src = "function main(): string { return \"\"; }";
    assert!(has(src, "E-MAIN-SIGNATURE"), "{:?}", errors_of(src));
}

#[test]
fn non_main_function_is_unconstrained() {
    // An ordinary function may take any params / return any type — only `main` is gated.
    let src = "function helper(int x): string { return \"\"; } function main(): void { }";
    assert!(!has(src, "E-MAIN-SIGNATURE"), "{:?}", errors_of(src));
}
