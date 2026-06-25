use super::*;
use std::rc::Rc;

#[test]
fn reflect_kind_maps_values_to_coarse_php_reproducible_kinds() {
    let mut out = String::new();
    let mut kind = |v: Value| match reflect_kind(&[v], &mut out) {
        Ok(Value::Str(s)) => s,
        other => panic!("reflect_kind returned {other:?}"),
    };
    // Scalars report their PHP-visible kind.
    assert_eq!(kind(Value::Int(1)), "int");
    assert_eq!(kind(Value::Float(1.0)), "float");
    assert_eq!(kind(Value::Bool(true)), "bool");
    assert_eq!(kind(Value::Str("x".into())), "string");
    // bytes erases to a PHP string, so its coarse kind is "string" (byte-identical with PHP).
    assert_eq!(kind(Value::Bytes(Rc::new(vec![1, 2]))), "string");
    assert_eq!(kind(Value::Null), "null");
    // List/Map/Set all erase to PHP `array`.
    assert_eq!(kind(Value::List(Rc::new(vec![]))), "array");
    assert_eq!(kind(Value::Map(Rc::new(vec![]))), "array");
    assert_eq!(kind(Value::Set(Rc::new(vec![]))), "array");
    // A closure is `is_callable` in PHP (checked before is_object).
    assert_eq!(
        kind(Value::Closure(Rc::new(crate::value::ClosureData::Named(
            "f".into()
        )))),
        "callable"
    );
}

#[test]
fn reflect_kind_is_registered_and_resolvable_by_leaf() {
    let i = index_of("Core.Reflect", "kind").expect("Reflect.kind registered");
    assert_eq!(index_of_by_leaf("Reflect", "kind"), Some(i));
}

#[test]
fn reflect_kind_php_emits_the_gated_helper() {
    let i = index_of("Core.Reflect", "kind").unwrap();
    assert_eq!((registry()[i].php)(&["$x".into()]), "__phorge_kind($x)");
}
