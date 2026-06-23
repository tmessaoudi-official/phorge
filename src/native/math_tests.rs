use super::*;

#[test]
fn math_natives_eval_and_emit() {
    let mut out = String::new();
    // float ops
    assert!(matches!(math_sqrt(&[Value::Float(16.0)], &mut out), Ok(Value::Float(x)) if x == 4.0));
    assert!(
        matches!(math_pow(&[Value::Float(2.0), Value::Float(10.0)], &mut out), Ok(Value::Float(x)) if x == 1024.0)
    );
    assert!(matches!(math_floor(&[Value::Float(3.7)], &mut out), Ok(Value::Float(x)) if x == 3.0));
    assert!(matches!(math_ceil(&[Value::Float(3.2)], &mut out), Ok(Value::Float(x)) if x == 4.0));
    // int ops
    assert!(matches!(
        math_abs(&[Value::Int(-5)], &mut out),
        Ok(Value::Int(5))
    ));
    assert!(matches!(
        math_min(&[Value::Int(3), Value::Int(8)], &mut out),
        Ok(Value::Int(3))
    ));
    assert!(matches!(
        math_max(&[Value::Int(3), Value::Int(8)], &mut out),
        Ok(Value::Int(8))
    ));
    // EV-7: abs of i64::MIN faults, never panics
    assert!(math_abs(&[Value::Int(i64::MIN)], &mut out).is_err());
    // resolvable by both index forms + PHP erasure to the same-named builtin
    let i = index_of("Core.Math", "pow").expect("pow registered");
    assert_eq!(index_of_by_leaf("Math", "pow"), Some(i));
    assert_eq!(
        (registry()[i].php)(&["2.0".into(), "10.0".into()]),
        "pow(2.0, 10.0)"
    );
    assert_eq!(
        (registry()[index_of("Core.Math", "min").unwrap()].php)(&["$a".into(), "$b".into()]),
        "min($a, $b)"
    );
    // round → int, half-away-from-zero (matches PHP's default mode), then truncating cast.
    assert!(matches!(
        math_round(&[Value::Float(2.5)], &mut out),
        Ok(Value::Int(3))
    ));
    assert!(matches!(
        math_round(&[Value::Float(2.4)], &mut out),
        Ok(Value::Int(2))
    ));
    assert!(matches!(
        math_round(&[Value::Float(-2.5)], &mut out),
        Ok(Value::Int(-3))
    ));
    assert_eq!(
        (registry()[index_of("Core.Math", "round").unwrap()].php)(&["$x".into()]),
        "(int)round($x)"
    );
}
