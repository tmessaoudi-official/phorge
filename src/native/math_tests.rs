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
    // `ipow` is the integer-power native (the `**` twin); single-sourced with `value::int_pow`, so
    // a negative exponent faults rather than widening to a float.
    assert!(matches!(
        math_ipow(&[Value::Int(2), Value::Int(10)], &mut out),
        Ok(Value::Int(1024))
    ));
    assert!(math_ipow(&[Value::Int(2), Value::Int(-1)], &mut out).is_err());
    assert_eq!(
        (registry()[index_of("Core.Math", "ipow").unwrap()].php)(&["5".into(), "2".into()]),
        "pow(5, 2)"
    );
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

#[test]
fn math_s3_predicates_special_and_intdiv() {
    let mut out = String::new();
    // predicates → bool (byte-identical even for non-representable floats)
    assert!(matches!(
        math_is_nan(&[Value::Float(f64::NAN)], &mut out),
        Ok(Value::Bool(true))
    ));
    assert!(matches!(
        math_is_nan(&[Value::Float(1.0)], &mut out),
        Ok(Value::Bool(false))
    ));
    assert!(matches!(
        math_is_finite(&[Value::Float(1.0)], &mut out),
        Ok(Value::Bool(true))
    ));
    assert!(matches!(
        math_is_finite(&[Value::Float(f64::INFINITY)], &mut out),
        Ok(Value::Bool(false))
    ));
    assert!(matches!(
        math_is_infinite(&[Value::Float(f64::INFINITY)], &mut out),
        Ok(Value::Bool(true))
    ));
    assert!(matches!(
        math_is_infinite(&[Value::Float(2.0)], &mut out),
        Ok(Value::Bool(false))
    ));
    // special-value constructors
    assert!(matches!(math_nan(&[], &mut out), Ok(Value::Float(x)) if x.is_nan()));
    assert!(
        matches!(math_infinity(&[], &mut out), Ok(Value::Float(x)) if x.is_infinite() && x > 0.0)
    );
    assert!(
        matches!(math_neg_infinity(&[], &mut out), Ok(Value::Float(x)) if x.is_infinite() && x < 0.0)
    );
    // round-trip: nan() through isNan, infinity() through isInfinite (the byte-identity-safe path)
    let nan = math_nan(&[], &mut out).unwrap();
    assert!(matches!(
        math_is_nan(&[nan], &mut out),
        Ok(Value::Bool(true))
    ));
    // intdiv: truncate toward zero + faults
    assert!(matches!(
        math_intdiv(&[Value::Int(7), Value::Int(2)], &mut out),
        Ok(Value::Int(3))
    ));
    assert!(matches!(
        math_intdiv(&[Value::Int(-7), Value::Int(2)], &mut out),
        Ok(Value::Int(-3))
    ));
    assert_eq!(
        math_intdiv(&[Value::Int(5), Value::Int(0)], &mut out).unwrap_err(),
        "division by zero"
    );
    assert_eq!(
        math_intdiv(&[Value::Int(i64::MIN), Value::Int(-1)], &mut out).unwrap_err(),
        "integer overflow"
    );
    // PHP erasure
    let php = |name: &str, args: &[&str]| {
        let i = index_of("Core.Math", name).unwrap();
        let a: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        (registry()[i].php)(&a)
    };
    assert_eq!(php("isNan", &["$f"]), "is_nan($f)");
    assert_eq!(php("isFinite", &["$f"]), "is_finite($f)");
    assert_eq!(php("isInfinite", &["$f"]), "is_infinite($f)");
    assert_eq!(php("nan", &[]), "NAN");
    assert_eq!(php("infinity", &[]), "INF");
    assert_eq!(php("negInfinity", &[]), "-INF");
    assert_eq!(php("intdiv", &["$a", "$b"]), "intdiv($a, $b)");
}
