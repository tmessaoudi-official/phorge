use super::*;
use crate::value::Value;

#[test]
fn convert_to_string_dispatches_by_type() {
    let s = |v: Value| {
        let mut o = String::new();
        convert_to_string(&[v], &mut o).unwrap()
    };
    assert!(matches!(s(Value::Int(5)), Value::Str(t) if t == "5"));
    assert!(matches!(s(Value::Int(-7)), Value::Str(t) if t == "-7"));
    assert!(matches!(s(Value::Bool(true)), Value::Str(t) if t == "true"));
    assert!(matches!(s(Value::Bool(false)), Value::Str(t) if t == "false"));
    assert!(matches!(s(Value::Float(2.5)), Value::Str(t) if t == "2.5"));
    assert!(matches!(s(Value::Float(5.0)), Value::Str(t) if t == "5")); // shortest-round-trip
    assert!(matches!(s(Value::Str("hi".into())), Value::Str(t) if t == "hi"));
}

#[test]
fn convert_numeric() {
    let mut o = String::new();
    assert!(matches!(convert_to_float(&[Value::Int(5)], &mut o), Ok(Value::Float(f)) if f == 5.0));
    assert!(
        matches!(convert_to_float(&[Value::Int(-3)], &mut o), Ok(Value::Float(f)) if f == -3.0)
    );
    // truncate toward zero.
    assert!(matches!(
        convert_truncate(&[Value::Float(3.9)], &mut o),
        Ok(Value::Int(3))
    ));
    assert!(matches!(
        convert_truncate(&[Value::Float(-3.9)], &mut o),
        Ok(Value::Int(-3))
    ));
    // round half away from zero.
    assert!(matches!(
        convert_round(&[Value::Float(3.5)], &mut o),
        Ok(Value::Int(4))
    ));
    assert!(matches!(
        convert_round(&[Value::Float(2.5)], &mut o),
        Ok(Value::Int(3))
    ));
    assert!(matches!(
        convert_round(&[Value::Float(-2.5)], &mut o),
        Ok(Value::Int(-3))
    ));
    assert!(matches!(
        convert_round(&[Value::Float(2.4)], &mut o),
        Ok(Value::Int(2))
    ));
}

#[test]
fn convert_to_int_guards_special_and_range() {
    let mut o = String::new();
    // normal + fractional truncate toward zero
    assert!(matches!(
        convert_to_int(&[Value::Float(42.0)], &mut o),
        Ok(Value::Int(42))
    ));
    assert!(matches!(
        convert_to_int(&[Value::Float(3.9)], &mut o),
        Ok(Value::Int(3))
    ));
    assert!(matches!(
        convert_to_int(&[Value::Float(-3.9)], &mut o),
        Ok(Value::Int(-3))
    ));
    // NaN / +Inf / out-of-range → null (avoids PHP (int)NAN==0)
    assert!(matches!(
        convert_to_int(&[Value::Float(f64::NAN)], &mut o),
        Ok(Value::Null)
    ));
    assert!(matches!(
        convert_to_int(&[Value::Float(f64::INFINITY)], &mut o),
        Ok(Value::Null)
    ));
    assert!(matches!(
        convert_to_int(&[Value::Float(1e30)], &mut o),
        Ok(Value::Null)
    ));
}

#[test]
fn convert_int_to_decimal_is_scale_zero() {
    let mut o = String::new();
    assert!(matches!(
        convert_int_to_decimal(&[Value::Int(42)], &mut o),
        Ok(Value::Decimal {
            unscaled: 42,
            scale: 0
        })
    ));
    assert!(matches!(
        convert_int_to_decimal(&[Value::Int(-7)], &mut o),
        Ok(Value::Decimal {
            unscaled: -7,
            scale: 0
        })
    ));
}

#[test]
fn convert_decimal_to_float_parses_carrier() {
    let mut o = String::new();
    // 12.5 is exactly representable → parses back losslessly.
    assert!(matches!(
        convert_decimal_to_float(&[Value::Decimal { unscaled: 125, scale: 1 }], &mut o),
        Ok(Value::Float(f)) if f == 12.5
    ));
    assert!(matches!(
        convert_decimal_to_float(&[Value::Decimal { unscaled: -250, scale: 2 }], &mut o),
        Ok(Value::Float(f)) if f == -2.5
    ));
}

#[test]
fn convert_decimal_to_int_truncates_or_nulls() {
    let mut o = String::new();
    assert!(matches!(
        convert_decimal_to_int(
            &[Value::Decimal {
                unscaled: 1999,
                scale: 2
            }],
            &mut o
        ),
        Ok(Value::Int(19))
    ));
    assert!(matches!(
        convert_decimal_to_int(
            &[Value::Decimal {
                unscaled: -1999,
                scale: 2
            }],
            &mut o
        ),
        Ok(Value::Int(-19))
    ));
    // out-of-i64-range integer part → null
    assert!(matches!(
        convert_decimal_to_int(
            &[Value::Decimal {
                unscaled: i128::from(i64::MAX) + 1,
                scale: 0
            }],
            &mut o
        ),
        Ok(Value::Null)
    ));
}

#[test]
fn convert_natives_registered_and_emit() {
    for name in [
        "toString",
        "toFloat",
        "truncate",
        "round",
        "toInt",
        "intToDecimal",
        "decimalToFloat",
        "decimalToInt",
    ] {
        assert!(
            crate::native::index_of("Core.Convert", name).is_some(),
            "Core.Convert.{name} not registered"
        );
    }
    let php = |name: &str, args: &[&str]| {
        let i = crate::native::index_of("Core.Convert", name).unwrap();
        let a: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        (crate::native::registry()[i].php)(&a)
    };
    assert_eq!(php("toInt", &["$f"]), "__phorge_float_to_int($f)");
    assert_eq!(php("intToDecimal", &["$i"]), "(string)($i)");
    assert_eq!(php("decimalToFloat", &["$d"]), "(float)($d)");
    assert_eq!(php("decimalToInt", &["$d"]), "__phorge_dec_to_int($d)");
}

#[test]
fn convert_exact_int_is_integral_or_null() {
    // M4 `float as int` — exact-or-null: only an integral, in-range float converts.
    let f = |x: f64| {
        let mut o = String::new();
        convert_float_to_int_exact(&[Value::Float(x)], &mut o).unwrap()
    };
    assert!(matches!(f(3.0), Value::Int(3)));
    assert!(matches!(f(-3.0), Value::Int(-3)));
    assert!(matches!(f(3.9), Value::Null)); // non-integral → null (never a silent truncate)
    assert!(matches!(f(f64::NAN), Value::Null));
    assert!(matches!(f(f64::INFINITY), Value::Null));

    // M4 `decimal as int` — exact-or-null over the i128 carrier.
    let d = |unscaled: i128, scale: u8| {
        let mut o = String::new();
        convert_decimal_to_int_exact(&[Value::Decimal { unscaled, scale }], &mut o).unwrap()
    };
    assert!(matches!(d(300, 2), Value::Int(3))); // 3.00 → 3
    assert!(matches!(d(-300, 2), Value::Int(-3)));
    assert!(matches!(d(350, 2), Value::Null)); // 3.50 → null
    assert!(matches!(d(7, 0), Value::Int(7)));

    // PHP emission of the two exact converters.
    let php = |name: &str, args: &[&str]| {
        let i = crate::native::index_of("Core.Convert", name).unwrap();
        let a: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        (crate::native::registry()[i].php)(&a)
    };
    assert_eq!(
        php("floatToIntExact", &["$f"]),
        "__phorge_float_to_int_exact($f)"
    );
    assert_eq!(
        php("decimalToIntExact", &["$d"]),
        "__phorge_dec_to_int_exact($d)"
    );
}

#[test]
fn convert_runtime_assertions_keep_or_null() {
    // M4 S2 `as int/float/bool` on a union — return the value when its variant matches, else null.
    let call = |f: fn(&[Value], &mut String) -> Result<Value, String>, v: Value| {
        let mut o = String::new();
        f(&[v], &mut o).unwrap()
    };
    assert!(matches!(call(convert_as_int, Value::Int(7)), Value::Int(7)));
    assert!(matches!(
        call(convert_as_int, Value::Str("x".into())),
        Value::Null
    ));
    assert!(matches!(
        call(convert_as_float, Value::Float(1.5)),
        Value::Float(_)
    ));
    assert!(matches!(call(convert_as_float, Value::Int(1)), Value::Null));
    assert!(matches!(
        call(convert_as_bool, Value::Bool(true)),
        Value::Bool(true)
    ));
    assert!(matches!(call(convert_as_bool, Value::Int(1)), Value::Null));

    // PHP emission is an arrow-IIFE (single-eval of the operand).
    let php = |name: &str| {
        let i = crate::native::index_of("Core.Convert", name).unwrap();
        (crate::native::registry()[i].php)(&["$x".to_string()])
    };
    assert_eq!(php("asInt"), "(fn($__a) => is_int($__a) ? $__a : null)($x)");
    assert_eq!(
        php("asBool"),
        "(fn($__a) => is_bool($__a) ? $__a : null)($x)"
    );
}
