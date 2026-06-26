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

#[test]
fn set_algebra_union_intersection_difference() {
    use crate::value::HKey;
    let mut o = String::new();
    let set = |ns: &[i64]| {
        Value::Set(std::rc::Rc::new(
            ns.iter().map(|n| HKey::Int(*n)).collect::<Vec<_>>(),
        ))
    };
    let ints = |v: &Value| match v {
        Value::Set(s) => s
            .iter()
            .map(|k| match k {
                HKey::Int(n) => *n,
                other => panic!("non-int {other:?}"),
            })
            .collect::<Vec<_>>(),
        other => panic!("non-set {other:?}"),
    };
    let a = set(&[1, 2, 3]);
    let b = set(&[2, 3, 4]);
    // union: a's order, then b's new elements.
    assert_eq!(
        ints(&set_union(&[a.clone(), b.clone()], &mut o).unwrap()),
        vec![1, 2, 3, 4]
    );
    // intersection: a's order, members also in b.
    assert_eq!(
        ints(&set_intersection(&[a.clone(), b.clone()], &mut o).unwrap()),
        vec![2, 3]
    );
    // difference: a's elements not in b.
    assert_eq!(ints(&set_difference(&[a, b], &mut o).unwrap()), vec![1]);
    // PHP erasures.
    let php = |n: &str, args: &[&str]| {
        let a: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        (registry()[index_of("Core.Set", n).unwrap()].php)(&a)
    };
    assert_eq!(
        php("union", &["$a", "$b"]),
        "array_values(array_unique(array_merge($a, $b), SORT_STRING))"
    );
    assert_eq!(
        php("intersection", &["$a", "$b"]),
        "array_values(array_intersect($a, $b))"
    );
    assert_eq!(
        php("difference", &["$a", "$b"]),
        "array_values(array_diff($a, $b))"
    );
    assert_eq!(
        registry()[index_of("Core.Set", "union").unwrap()].ret,
        Ty::Set(Box::new(Ty::Param("T".into())))
    );
}
