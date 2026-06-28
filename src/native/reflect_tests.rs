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

#[test]
fn reflect_class_name_returns_runtime_class_for_objects_null_otherwise() {
    use crate::value::{EnumVal, FieldMap, Instance};
    use std::cell::RefCell;
    let mut out = String::new();
    // An instance reports its class name (≡ PHP get_class for a package-Main class).
    let inst = Value::Instance(Rc::new(Instance {
        class: "Point".into(),
        fields: RefCell::new(FieldMap::default()),
    }));
    assert!(matches!(reflect_class_name(&[inst], &mut out), Ok(Value::Str(s)) if s == "Point"));
    // An enum variant reports the VARIANT name — PHP get_class returns the variant subclass (Q3).
    let ev = Value::Enum(Rc::new(EnumVal {
        ty: "Color".into(),
        variant: "Red".into(),
        payload: vec![],
    }));
    assert!(matches!(reflect_class_name(&[ev], &mut out), Ok(Value::Str(s)) if s == "Red"));
    // A non-object (scalar / collection / closure) is not a class → null.
    assert!(matches!(
        reflect_class_name(&[Value::Int(1)], &mut out),
        Ok(Value::Null)
    ));
    assert!(matches!(
        reflect_class_name(&[Value::List(Rc::new(vec![]))], &mut out),
        Ok(Value::Null)
    ));
    assert!(matches!(
        reflect_class_name(
            &[Value::Closure(Rc::new(crate::value::ClosureData::Named(
                "f".into()
            )))],
            &mut out
        ),
        Ok(Value::Null)
    ));
}

#[test]
fn reflect_enumeration_natives_read_class_tables() {
    use crate::value::{FieldMap, Instance};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    let mut interfaces = BTreeMap::new();
    interfaces.insert(
        "Widget".to_string(),
        vec!["Drawable".into(), "Shape".into()],
    );
    let mut parents = BTreeMap::new();
    parents.insert("Widget".to_string(), vec!["Base".into()]);
    let tables = ClassTables {
        interfaces,
        parents,
        methods: BTreeMap::new(),
        fields: BTreeMap::new(),
    };
    let widget = Value::Instance(Rc::new(Instance {
        class: "Widget".into(),
        fields: RefCell::new(FieldMap::default()),
    }));
    let strs = |v: Value| match v {
        Value::List(items) => items
            .iter()
            .map(|e| match e {
                Value::Str(s) => s.clone(),
                other => panic!("non-string {other:?}"),
            })
            .collect::<Vec<_>>(),
        other => panic!("not a list: {other:?}"),
    };
    let i = index_of("Core.Reflect", "interfaces").expect("interfaces registered");
    let p = index_of("Core.Reflect", "parents").expect("parents registered");
    let call = |idx: usize, v: Value| match registry()[idx].eval {
        NativeEval::Reflective(f) => f(&[v], &tables).unwrap(),
        _ => panic!("expected a Reflective native"),
    };
    assert_eq!(strs(call(i, widget.clone())), vec!["Drawable", "Shape"]);
    assert_eq!(strs(call(p, widget)), vec!["Base"]);
    // A non-class value → empty list (matches the PHP `__phorge_reflect_of` non-object branch).
    assert!(strs(call(i, Value::Int(1))).is_empty());
}

#[test]
fn reflect_class_name_is_registered_and_php_emits_the_gated_helper() {
    let i = index_of("Core.Reflect", "className").expect("Reflect.className registered");
    assert_eq!(index_of_by_leaf("Reflect", "className"), Some(i));
    assert_eq!(
        (registry()[i].php)(&["$x".into()]),
        "__phorge_class_name($x)"
    );
}
