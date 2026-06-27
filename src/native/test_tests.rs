use super::*;

#[test]
fn assert_natives_pass_and_fail() {
    let mut out = String::new();
    // assert(bool, message)
    assert!(matches!(
        test_assert(&[Value::Bool(true), Value::Str("ok".into())], &mut out),
        Ok(Value::Unit)
    ));
    assert_eq!(
        test_assert(&[Value::Bool(false), Value::Str("boom".into())], &mut out).unwrap_err(),
        "assertion failed: boom"
    );
    // assertTrue / assertFalse
    assert!(matches!(
        test_assert_true(&[Value::Bool(true)], &mut out),
        Ok(Value::Unit)
    ));
    assert!(test_assert_true(&[Value::Bool(false)], &mut out).is_err());
    assert!(matches!(
        test_assert_false(&[Value::Bool(false)], &mut out),
        Ok(Value::Unit)
    ));
    assert!(test_assert_false(&[Value::Bool(true)], &mut out).is_err());
}

#[test]
fn assert_equals_uses_eq_kernel() {
    let mut out = String::new();
    assert!(matches!(
        test_assert_equals(&[Value::Int(4), Value::Int(4)], &mut out),
        Ok(Value::Unit)
    ));
    let err = test_assert_equals(&[Value::Int(4), Value::Int(5)], &mut out).unwrap_err();
    assert!(err.contains("not equal"), "{err}");
    assert!(err.contains('4') && err.contains('5'), "{err}");
    // assertNotEquals is the dual
    assert!(matches!(
        test_assert_not_equals(&[Value::Int(4), Value::Int(5)], &mut out),
        Ok(Value::Unit)
    ));
    assert!(test_assert_not_equals(&[Value::Int(4), Value::Int(4)], &mut out).is_err());
}

#[test]
fn assert_null_natives() {
    let mut out = String::new();
    assert!(matches!(
        test_assert_null(&[Value::Null], &mut out),
        Ok(Value::Unit)
    ));
    assert!(test_assert_null(&[Value::Int(1)], &mut out).is_err());
    assert!(matches!(
        test_assert_not_null(&[Value::Int(1)], &mut out),
        Ok(Value::Unit)
    ));
    assert!(test_assert_not_null(&[Value::Null], &mut out).is_err());
}

#[test]
fn test_natives_registered_and_typed() {
    // All seven asserts are addressable by (module, name) and by leaf, and are `pure`.
    for name in [
        "assert",
        "assertTrue",
        "assertFalse",
        "assertEquals",
        "assertNotEquals",
        "assertNull",
        "assertNotNull",
    ] {
        let i = index_of("Core.Test", name).unwrap_or_else(|| panic!("{name} registered"));
        assert_eq!(index_of_by_leaf("Test", name), Some(i), "{name} leaf");
        assert!(registry()[i].pure, "{name} should be pure");
        assert_eq!(registry()[i].ret, Ty::Void, "{name} returns void");
    }
}
