use super::*;
use crate::value::Value;

fn int_of(v: Value) -> i64 {
    match v {
        Value::Int(n) => n,
        other => panic!("expected int, got {other:?}"),
    }
}

#[test]
fn monotonic_nanos_is_nondecreasing() {
    let a = int_of(runtime_monotonic_nanos(&[], &mut String::new()).unwrap());
    let b = int_of(runtime_monotonic_nanos(&[], &mut String::new()).unwrap());
    assert!(b >= a, "monotonic clock went backwards: {a} then {b}");
    assert!(a >= 0, "elapsed nanos must be non-negative");
}

#[test]
fn memory_counters_are_non_negative() {
    // On Linux these read /proc; elsewhere they return 0. Either way, never negative, never a panic.
    let cur = int_of(runtime_memory_bytes(&[], &mut String::new()).unwrap());
    let peak = int_of(runtime_peak_memory_bytes(&[], &mut String::new()).unwrap());
    assert!(cur >= 0);
    assert!(peak >= 0);
}

#[test]
fn reset_peak_never_fails() {
    assert!(matches!(
        runtime_reset_peak_memory(&[], &mut String::new()).unwrap(),
        Value::Unit
    ));
}

#[test]
fn arity_errors() {
    assert!(runtime_monotonic_nanos(&[Value::Int(1)], &mut String::new()).is_err());
    assert!(runtime_memory_bytes(&[Value::Int(1)], &mut String::new()).is_err());
    assert!(runtime_peak_memory_bytes(&[Value::Int(1)], &mut String::new()).is_err());
    assert!(runtime_reset_peak_memory(&[Value::Int(1)], &mut String::new()).is_err());
}

#[test]
fn all_runtime_natives_are_impure() {
    // The quarantine contract: every Core.Runtime native must be pure:false so an importing program
    // is auto-skipped from the byte-identity differential.
    assert!(
        runtime_natives().iter().all(|n| !n.pure),
        "Core.Runtime natives must all be pure:false (quarantine seam)"
    );
}
