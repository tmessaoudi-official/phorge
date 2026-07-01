use super::*;
use crate::value::Value;
use std::sync::Mutex;

// `RANDOM_STATE` is a process global, so these tests must not interleave their seed/advance calls.
static RNG_LOCK: Mutex<()> = Mutex::new(());

fn seed(n: i64) {
    random_seed(&[Value::Int(n)], &mut String::new()).unwrap();
}
fn next() -> i64 {
    match random_next(&[], &mut String::new()).unwrap() {
        Value::Int(n) => n,
        other => panic!("expected int, got {other:?}"),
    }
}
fn between(lo: i64, hi: i64) -> i64 {
    match random_int_between(&[Value::Int(lo), Value::Int(hi)], &mut String::new()).unwrap() {
        Value::Int(n) => n,
        other => panic!("expected int, got {other:?}"),
    }
}
fn next_float() -> f64 {
    match random_next_float(&[], &mut String::new()).unwrap() {
        Value::Float(f) => f,
        other => panic!("expected float, got {other:?}"),
    }
}

#[test]
fn next_float_in_unit_interval_and_reproducible() {
    let _g = RNG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    seed(42);
    let first: Vec<f64> = (0..8).map(|_| next_float()).collect();
    // Every draw is in [0.0, 1.0).
    for &f in &first {
        assert!((0.0..1.0).contains(&f), "nextFloat {f} out of [0,1)");
    }
    // Same seed replays the same float stream.
    seed(42);
    let second: Vec<f64> = (0..8).map(|_| next_float()).collect();
    assert_eq!(
        first, second,
        "a fixed seed must replay the same float stream"
    );
    // Each value is a dyadic k/2^53, so `f * 2^53` is an exact non-negative integer (byte-identity
    // guarantee: exactly representable, matches PHP's identical division).
    for &f in &first {
        let scaled = f * 9_007_199_254_740_992.0;
        assert_eq!(
            scaled.fract(),
            0.0,
            "nextFloat {f} is not a clean dyadic k/2^53"
        );
    }
}

#[test]
fn same_seed_replays_the_same_stream() {
    let _g = RNG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    seed(42);
    let first: Vec<i64> = (0..8).map(|_| next()).collect();
    seed(42);
    let second: Vec<i64> = (0..8).map(|_| next()).collect();
    assert_eq!(first, second, "a fixed seed must be reproducible");
    // Every value is a non-negative i64 (masked to < 2^63).
    assert!(first.iter().all(|&v| v >= 0), "values must be non-negative");
}

#[test]
fn different_seeds_diverge() {
    let _g = RNG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    seed(1);
    let a: Vec<i64> = (0..8).map(|_| next()).collect();
    seed(2);
    let b: Vec<i64> = (0..8).map(|_| next()).collect();
    assert_ne!(a, b, "distinct seeds should produce distinct streams");
}

#[test]
fn int_between_stays_in_bounds() {
    let _g = RNG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    seed(7);
    for _ in 0..1000 {
        let v = between(1, 6);
        assert!((1..=6).contains(&v), "d6 roll {v} out of range");
    }
    // A degenerate single-value span always yields that value.
    assert_eq!(between(5, 5), 5);
}

#[test]
fn int_between_rejects_inverted_range() {
    let _g = RNG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let err = random_int_between(&[Value::Int(10), Value::Int(1)], &mut String::new());
    assert!(err.is_err(), "hi < lo must fault");
}
