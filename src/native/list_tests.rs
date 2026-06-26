use super::*;
use std::rc::Rc;

#[test]
fn list_sort_natural_ascending() {
    let mut o = String::new();
    let ints = Value::List(Rc::new(vec![
        Value::Int(3),
        Value::Int(1),
        Value::Int(2),
        Value::Int(1),
    ]));
    match list_sort(&[ints], &mut o).unwrap() {
        Value::List(xs) => assert_eq!(
            xs.iter()
                .map(|v| match v {
                    Value::Int(n) => *n,
                    _ => -99,
                })
                .collect::<Vec<_>>(),
            vec![1, 1, 2, 3]
        ),
        other => panic!("sort returned {other:?}"),
    }
    // Strings sort lexicographically (byte order) — "10" before "9" (NOT PHP's numeric-string <=>).
    let strs = Value::List(Rc::new(vec![
        Value::Str("9".into()),
        Value::Str("10".into()),
        Value::Str("apple".into()),
    ]));
    match list_sort(&[strs], &mut o).unwrap() {
        Value::List(xs) => assert_eq!(
            xs.iter()
                .map(|v| match v {
                    Value::Str(s) => s.clone(),
                    _ => "?".into(),
                })
                .collect::<Vec<_>>(),
            vec!["10".to_string(), "9".into(), "apple".into()]
        ),
        other => panic!("sort returned {other:?}"),
    }
    // Empty list sorts to empty.
    assert!(
        matches!(list_sort(&[Value::List(Rc::new(vec![]))], &mut o), Ok(Value::List(xs)) if xs.is_empty())
    );
}

#[test]
fn list_sort_with_comparator_and_fault_parity() {
    let nums = Value::List(Rc::new(vec![Value::Int(3), Value::Int(1), Value::Int(2)]));
    let placeholder = Value::Int(0); // stands in for the closure value (eval passes it to `call`)
                                     // Descending comparator: cmp(a, b) = b - a.
    let mut desc = |_f: &Value, a: Vec<Value>| match a.as_slice() {
        [Value::Int(x), Value::Int(y)] => Ok(Value::Int(y - x)),
        _ => Err("bad arity".to_string()),
    };
    match list_sort_with(&[nums.clone(), placeholder.clone()], &mut desc).unwrap() {
        Value::List(xs) => assert_eq!(
            xs.iter()
                .map(|v| match v {
                    Value::Int(n) => *n,
                    _ => -99,
                })
                .collect::<Vec<_>>(),
            vec![3, 2, 1]
        ),
        other => panic!("sortWith returned {other:?}"),
    }
    // A comparator fault propagates cleanly (never a panic).
    let mut boom = |_f: &Value, _a: Vec<Value>| Err("kaboom".to_string());
    assert!(list_sort_with(&[nums.clone(), placeholder.clone()], &mut boom).is_err());
    // A non-int comparator result is a clean fault.
    let mut bad = |_f: &Value, _a: Vec<Value>| Ok(Value::Bool(true));
    assert!(list_sort_with(&[nums, placeholder], &mut bad).is_err());
}

#[test]
fn list_natives_eval_and_emit() {
    let mut o = String::new();
    // reverse: generic over the element type — works on any List, byte-identical to array_reverse.
    let nums = Value::List(std::rc::Rc::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ]));
    match list_reverse(std::slice::from_ref(&nums), &mut o).unwrap() {
        Value::List(xs) => {
            assert_eq!(xs.len(), 3);
            assert!(matches!(xs[0], Value::Int(3)));
            assert!(matches!(xs[2], Value::Int(1)));
        }
        other => panic!("reverse returned {other:?}"),
    }
    // length: generic over the element type — the count of any list (byte-identical to PHP count).
    assert!(matches!(
        list_length(std::slice::from_ref(&nums), &mut o),
        Ok(Value::Int(3))
    ));
    assert!(matches!(
        list_length(&[Value::List(std::rc::Rc::new(vec![]))], &mut o),
        Ok(Value::Int(0))
    ));
    // sum: concrete List<int> -> int.
    assert!(matches!(
        list_sum(std::slice::from_ref(&nums), &mut o),
        Ok(Value::Int(6))
    ));
    // sum over the empty list is 0.
    assert!(matches!(
        list_sum(&[Value::List(std::rc::Rc::new(vec![]))], &mut o),
        Ok(Value::Int(0))
    ));
    // EV-7: an overflowing sum faults cleanly, never panics.
    let huge = Value::List(std::rc::Rc::new(vec![Value::Int(i64::MAX), Value::Int(1)]));
    assert!(list_sum(&[huge], &mut o).is_err());
    // a non-int element is a clean fault.
    assert!(list_sum(
        &[Value::List(std::rc::Rc::new(vec![Value::Str("x".into())]))],
        &mut o
    )
    .is_err());
    // PHP erasure + both index forms + the generic return type is carried in the registry.
    assert_eq!(
        (registry()[index_of("Core.List", "reverse").unwrap()].php)(&["$xs".into()]),
        "array_reverse($xs)"
    );
    assert_eq!(
        (registry()[index_of("Core.List", "length").unwrap()].php)(&["$xs".into()]),
        "count($xs)"
    );
    assert_eq!(
        (registry()[index_of("Core.List", "sum").unwrap()].php)(&["$xs".into()]),
        "array_sum($xs)"
    );
    assert_eq!(
        index_of_by_leaf("List", "reverse"),
        index_of("Core.List", "reverse")
    );
    assert_eq!(
        registry()[index_of("Core.List", "reverse").unwrap()].ret,
        Ty::List(Box::new(Ty::Param("T".into())))
    );
}

#[test]
fn list_contains_eval_and_emit() {
    let mut o = String::new();
    let nums = Value::List(std::rc::Rc::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ]));
    assert!(matches!(
        list_contains(&[nums.clone(), Value::Int(2)], &mut o).unwrap(),
        Value::Bool(true)
    ));
    assert!(matches!(
        list_contains(&[nums, Value::Int(9)], &mut o).unwrap(),
        Value::Bool(false)
    ));
    // strict in_array, with (needle, haystack) — the reverse of contains(list, value).
    assert_eq!(
        (registry()[index_of("Core.List", "contains").unwrap()].php)(&["$xs".into(), "$v".into()]),
        "in_array($v, $xs, true)"
    );
}

#[test]
fn list_breadth_slice_indexof_concat_first_last() {
    let mut o = String::new();
    let nums = || {
        Value::List(std::rc::Rc::new(vec![
            Value::Int(10),
            Value::Int(20),
            Value::Int(30),
            Value::Int(40),
            Value::Int(50),
        ]))
    };
    let ints = |xs: &Value| match xs {
        Value::List(v) => v
            .iter()
            .map(|e| match e {
                Value::Int(n) => *n,
                other => panic!("non-int {other:?}"),
            })
            .collect::<Vec<_>>(),
        other => panic!("non-list {other:?}"),
    };
    // slice mirrors PHP array_slice(offset, length): positive, negative, out-of-range → clamp/empty.
    assert_eq!(
        ints(&list_slice(&[nums(), Value::Int(1), Value::Int(2)], &mut o).unwrap()),
        vec![20, 30]
    );
    assert_eq!(
        ints(&list_slice(&[nums(), Value::Int(-2), Value::Int(1)], &mut o).unwrap()),
        vec![40]
    );
    assert_eq!(
        ints(&list_slice(&[nums(), Value::Int(1), Value::Int(-1)], &mut o).unwrap()),
        vec![20, 30, 40]
    );
    assert_eq!(
        ints(&list_slice(&[nums(), Value::Int(9), Value::Int(2)], &mut o).unwrap()),
        Vec::<i64>::new()
    );
    // indexOf: first match → int, miss → null.
    assert!(matches!(
        list_index_of(&[nums(), Value::Int(30)], &mut o).unwrap(),
        Value::Int(2)
    ));
    assert!(matches!(
        list_index_of(&[nums(), Value::Int(99)], &mut o).unwrap(),
        Value::Null
    ));
    // concat: joins; both inputs unchanged.
    let a = Value::List(std::rc::Rc::new(vec![Value::Int(1), Value::Int(2)]));
    let b = Value::List(std::rc::Rc::new(vec![Value::Int(3)]));
    assert_eq!(ints(&list_concat(&[a, b], &mut o).unwrap()), vec![1, 2, 3]);
    // first / last: head/tail, or null for an empty list.
    assert!(matches!(
        list_first(&[nums()], &mut o).unwrap(),
        Value::Int(10)
    ));
    assert!(matches!(
        list_last(&[nums()], &mut o).unwrap(),
        Value::Int(50)
    ));
    let empty = Value::List(std::rc::Rc::new(vec![]));
    assert!(matches!(
        list_first(std::slice::from_ref(&empty), &mut o).unwrap(),
        Value::Null
    ));
    assert!(matches!(
        list_last(std::slice::from_ref(&empty), &mut o).unwrap(),
        Value::Null
    ));
    // PHP erasures + optional return types.
    let php = |n: &str, a: &[&str]| {
        let args: Vec<String> = a.iter().map(|s| (*s).to_string()).collect();
        (registry()[index_of("Core.List", n).unwrap()].php)(&args)
    };
    assert_eq!(
        php("slice", &["$xs", "$o", "$l"]),
        "array_slice($xs, $o, $l)"
    );
    assert_eq!(php("indexOf", &["$xs", "$n"]), "__phorge_index_of($xs, $n)");
    assert_eq!(php("concat", &["$a", "$b"]), "array_merge($a, $b)");
    assert_eq!(php("first", &["$xs"]), "($xs[0] ?? null)");
    assert_eq!(php("last", &["$xs"]), "($xs[count($xs) - 1] ?? null)");
    assert_eq!(
        registry()[index_of("Core.List", "indexOf").unwrap()].ret,
        Ty::Optional(Box::new(Ty::Int))
    );
    assert_eq!(
        registry()[index_of("Core.List", "first").unwrap()].ret,
        Ty::Optional(Box::new(Ty::Param("T".into())))
    );
}

#[test]
fn list_higher_order_eval_and_emit() {
    // The HOF natives drive the closure via the backend-supplied invoker; here a stub invoker
    // stands in for a backend (the `f` Value is a placeholder the stub ignores). The end-to-end
    // closure path is covered by the differential harness; this pins the iteration/collect logic.
    let nums = Value::List(std::rc::Rc::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
        Value::Int(4),
    ]));
    let placeholder = Value::Int(0);

    // map: double each element.
    let mut dbl = |_f: &Value, a: Vec<Value>| match a.as_slice() {
        [Value::Int(n)] => Ok(Value::Int(n * 2)),
        _ => Err("bad arity".to_string()),
    };
    match list_map(&[nums.clone(), placeholder.clone()], &mut dbl).unwrap() {
        Value::List(xs) => {
            assert_eq!(xs.len(), 4);
            assert!(matches!(xs[0], Value::Int(2)));
            assert!(matches!(xs[3], Value::Int(8)));
        }
        other => panic!("map returned {other:?}"),
    }

    // filter: keep the even elements (predicate returns bool).
    let mut even = |_f: &Value, a: Vec<Value>| match a.as_slice() {
        [Value::Int(n)] => Ok(Value::Bool(n % 2 == 0)),
        _ => Err("bad arity".to_string()),
    };
    match list_filter(&[nums.clone(), placeholder.clone()], &mut even).unwrap() {
        Value::List(xs) => {
            assert_eq!(xs.len(), 2);
            assert!(matches!(xs[0], Value::Int(2)));
            assert!(matches!(xs[1], Value::Int(4)));
        }
        other => panic!("filter returned {other:?}"),
    }

    // filter: a non-bool predicate result is a clean fault, never a panic.
    let mut bad = |_f: &Value, _a: Vec<Value>| Ok(Value::Int(7));
    assert!(list_filter(&[nums.clone(), placeholder.clone()], &mut bad).is_err());

    // reduce: sum, seeded with 100.
    let mut add = |_f: &Value, a: Vec<Value>| match a.as_slice() {
        [Value::Int(acc), Value::Int(x)] => Ok(Value::Int(acc + x)),
        _ => Err("bad arity".to_string()),
    };
    assert!(matches!(
        list_reduce(
            &[nums.clone(), Value::Int(100), placeholder.clone()],
            &mut add
        ),
        Ok(Value::Int(110))
    ));

    // reduce over the empty list returns the seed unchanged (the closure is never called).
    let empty = Value::List(std::rc::Rc::new(vec![]));
    let mut never = |_f: &Value, _a: Vec<Value>| Err("must not be called".to_string());
    assert!(matches!(
        list_reduce(&[empty, Value::Int(42), placeholder.clone()], &mut never),
        Ok(Value::Int(42))
    ));

    // A fault from the closure propagates as a plain `String` (the backend-shared contract).
    let mut boom = |_f: &Value, _a: Vec<Value>| Err("kaboom".to_string());
    assert_eq!(
        list_map(&[nums, placeholder], &mut boom).unwrap_err(),
        "kaboom"
    );

    // PHP erasure: array_map (arg order swapped), array_values(array_filter), array_reduce.
    assert_eq!(
        (registry()[index_of("Core.List", "map").unwrap()].php)(&["$xs".into(), "$f".into()]),
        "array_map($f, $xs)"
    );
    assert_eq!(
        (registry()[index_of("Core.List", "filter").unwrap()].php)(&["$xs".into(), "$f".into()]),
        "array_values(array_filter($xs, $f))"
    );
    assert_eq!(
        (registry()[index_of("Core.List", "reduce").unwrap()].php)(&[
            "$xs".into(),
            "$init".into(),
            "$f".into()
        ]),
        "array_reduce($xs, $f, $init)"
    );
    assert_eq!(
        index_of_by_leaf("List", "map"),
        index_of("Core.List", "map")
    );
}
