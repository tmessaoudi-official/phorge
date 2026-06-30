//! Checker tests — the `decimal` primitive (M-NUM S1).

use super::support::*;

#[test]
fn decimal_literal_and_arithmetic_type_as_decimal() {
    // A `decimal` literal types as `decimal`; `decimal + decimal` and `decimal * int` stay decimal.
    let src = "function main() -> void { \
               decimal price = 19.99d; \
               decimal sum = price + 1.00d; \
               decimal scaled = price * 3; \
               decimal scaled2 = 3 * price; \
               decimal neg = -price; \
               }";
    assert!(errors_of(src).is_empty(), "got {:?}", errors_of(src));
}

#[test]
fn decimal_plus_float_is_a_clean_error() {
    let e = errors_of(
        "function main() -> void { decimal d = 1.00d; float f = 1.5; decimal r = d + f; }",
    );
    assert!(
        e.iter().any(|d| d.code == Some("E-DECIMAL-FLOAT-MIX")),
        "got {e:?}"
    );
}

#[test]
fn float_is_not_assignable_to_a_decimal_slot() {
    // No implicit `float -> decimal` coercion (the whole point of the primitive).
    let e = errors_of("function main() -> void { decimal d = 1.5; }");
    assert!(
        !e.is_empty(),
        "float literal into a decimal slot must error"
    );
    // And the reverse: a decimal literal does not fit a float slot.
    let e2 = errors_of("function main() -> void { float f = 1.50d; }");
    assert!(
        !e2.is_empty(),
        "decimal literal into a float slot must error"
    );
}

#[test]
fn int_is_not_assignable_to_a_decimal_slot() {
    // The int-widening is operator-level only — a bare `int` does not fit a `decimal` slot.
    let e = errors_of("function main() -> void { decimal d = 3; }");
    assert!(!e.is_empty(), "int literal into a decimal slot must error");
}

#[test]
fn decimal_comparison_and_equality() {
    // `decimal <,>,==` with decimal or int operands type-check; a float mix is an error.
    let ok = "function main() -> void { \
              decimal a = 1.50d; decimal b = 1.5d; \
              bool eq = a == b; \
              bool gt = a > 1; \
              bool eqi = a == 2; \
              }";
    assert!(errors_of(ok).is_empty(), "got {:?}", errors_of(ok));
    let bad =
        errors_of("function main() -> void { decimal a = 1.50d; float f = 1.5; bool x = a < f; }");
    assert!(
        bad.iter().any(|d| d.code == Some("E-DECIMAL-FLOAT-MIX")),
        "got {bad:?}"
    );
}

#[test]
fn decimal_division_typechecks_exact_or_fault() {
    // Bare `decimal / decimal` is the exact-or-fault operator (2026-06-27) — it type-checks and
    // yields `decimal` (a non-terminating quotient faults at runtime, not at compile time).
    let e = errors_of("function main() -> void { decimal a = 1.00d; decimal r = a / 2; }");
    assert!(
        !e.iter().any(|d| d.code == Some("E-DECIMAL-DIV")),
        "decimal `/` must now type-check (exact-or-fault), got {e:?}"
    );
    let e2 = errors_of(
        "function main() -> void { decimal a = 10.00d; decimal b = 4d; decimal r = a / b; }",
    );
    assert!(
        e2.is_empty(),
        "decimal / decimal should be clean, got {e2:?}"
    );
}

#[test]
fn decimal_modulo_typechecks_and_is_decimal() {
    // `decimal % …` is the exact-remainder operator (2026-06-27) — it type-checks and yields decimal.
    let e = errors_of("function main() -> void { decimal a = 10.00d; decimal r = a % 3; }");
    assert!(
        !e.iter().any(|d| d.code == Some("E-DECIMAL-DIV")),
        "decimal `%` must now type-check (exact remainder), got {e:?}"
    );
    // A `decimal % decimal` result flowing into a `decimal` binding must also be accepted.
    let e2 = errors_of(
        "function main() -> void { decimal a = 10.50d; decimal b = 3.00d; decimal r = a % b; }",
    );
    assert!(
        e2.is_empty(),
        "decimal % decimal should be clean, got {e2:?}"
    );
}

#[test]
fn decimal_div_and_round_natives_typecheck() {
    // `Decimal.divide`/`Decimal.round` accept (decimal, …, scale, RoundingMode) and yield decimal. The
    // `RoundingMode` enum is injected by the CLI's `check_and_expand` chokepoint (gated on
    // `import Core.Decimal;`), so this goes through that path rather than the raw checker.
    let src = "package Main; import Core.Decimal; \
               function main() -> void { \
               decimal u = Decimal.divide(10.00d, 3d, 2, new HalfEven()); \
               decimal c = Decimal.round(2.345d, 2, new HalfUp()); \
               }";
    let prog = prog_raw(src);
    assert!(
        crate::cli::check_and_expand(&prog, src).is_ok(),
        "div/round natives must typecheck via the injected RoundingMode enum"
    );
}

#[test]
fn decimal_of_returns_optional_decimal() {
    let src = "package Main; import Core.Decimal; \
               function main() -> void { decimal d = Decimal.of(\"12.34\") ?? 0d; }";
    assert!(errors_of(src).is_empty(), "got {:?}", errors_of(src));
}

#[test]
fn decimal_interpolates_into_a_string() {
    let src = "function main() -> void { decimal d = 1.50d; }";
    assert!(errors_of(src).is_empty());
    let src2 =
        "import Core.Output; function main() -> void { decimal d = 1.50d; Output.printLine(\"d = {d}\"); }";
    // Console import lives at the package level — prepend it raw.
    let full = format!("package Main; {src2}");
    assert!(
        errors_of_raw(&full).is_empty(),
        "got {:?}",
        errors_of_raw(&full)
    );
}
