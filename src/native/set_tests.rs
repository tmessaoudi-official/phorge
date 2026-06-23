use super::*;

#[test]
fn set_natives_eval_and_emit() {
    let mut o = String::new();
    // of: dedup preserving first-occurrence order.
    let xs = Value::List(std::rc::Rc::new(vec![
        Value::Int(3),
        Value::Int(1),
        Value::Int(3),
        Value::Int(2),
        Value::Int(1),
    ]));
    let s = set_of(std::slice::from_ref(&xs), &mut o).unwrap();
    match &s {
        Value::Set(elems) => {
            assert_eq!(elems.len(), 3); // {3, 1, 2}
            assert_eq!(elems[0], crate::value::HKey::Int(3)); // first-seen order
            assert_eq!(elems[1], crate::value::HKey::Int(1));
            assert_eq!(elems[2], crate::value::HKey::Int(2));
        }
        other => panic!("of returned {other:?}"),
    }
    assert!(matches!(
        set_contains(&[s.clone(), Value::Int(2)], &mut o),
        Ok(Value::Bool(true))
    ));
    assert!(matches!(
        set_contains(&[s.clone(), Value::Int(9)], &mut o),
        Ok(Value::Bool(false))
    ));
    assert!(matches!(
        set_size(std::slice::from_ref(&s), &mut o),
        Ok(Value::Int(3))
    ));
    // a non-hashable element (float) is a clean fault, never a panic (EV-7).
    assert!(set_contains(&[s, Value::Float(2.0)], &mut o).is_err());
    assert!(set_of(
        &[Value::List(std::rc::Rc::new(vec![Value::Float(1.0)]))],
        &mut o
    )
    .is_err());
    // PHP erasures + generic return type.
    let php = |n: &str, a: &[&str]| {
        let args: Vec<String> = a.iter().map(|s| (*s).to_string()).collect();
        (registry()[index_of("Core.Set", n).unwrap()].php)(&args)
    };
    assert_eq!(
        php("of", &["$xs"]),
        "array_values(array_unique($xs, SORT_STRING))"
    );
    assert_eq!(php("contains", &["$s", "$x"]), "in_array($x, $s, true)");
    assert_eq!(php("size", &["$s"]), "count($s)");
    assert_eq!(index_of_by_leaf("Set", "of"), index_of("Core.Set", "of"));
    assert_eq!(
        registry()[index_of("Core.Set", "of").unwrap()].ret,
        Ty::Set(Box::new(Ty::Param("T".into())))
    );
}
