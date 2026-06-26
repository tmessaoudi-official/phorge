use super::*;

#[test]
fn map_natives_eval_and_emit() {
    use crate::value::HKey;
    let mut o = String::new();
    // insertion-ordered map ["a"=>1, "b"=>2]; keys/values preserve that order.
    let m = Value::Map(std::rc::Rc::new(vec![
        (HKey::Str("a".into()), Value::Int(1)),
        (HKey::Str("b".into()), Value::Int(2)),
    ]));
    match map_keys(std::slice::from_ref(&m), &mut o).unwrap() {
        Value::List(ks) => {
            assert_eq!(ks.len(), 2);
            assert!(matches!(&ks[0], Value::Str(s) if s == "a"));
            assert!(matches!(&ks[1], Value::Str(s) if s == "b"));
        }
        other => panic!("keys returned {other:?}"),
    }
    match map_values(std::slice::from_ref(&m), &mut o).unwrap() {
        Value::List(vs) => {
            assert!(matches!(vs[0], Value::Int(1)));
            assert!(matches!(vs[1], Value::Int(2)));
        }
        other => panic!("values returned {other:?}"),
    }
    assert!(matches!(
        map_has(&[m.clone(), Value::Str("a".into())], &mut o),
        Ok(Value::Bool(true))
    ));
    assert!(matches!(
        map_has(&[m.clone(), Value::Str("z".into())], &mut o),
        Ok(Value::Bool(false))
    ));
    // a non-hashable key (float) is a clean fault, never a panic (EV-7).
    assert!(map_has(&[m.clone(), Value::Float(1.0)], &mut o).is_err());
    assert!(matches!(
        map_size(std::slice::from_ref(&m), &mut o),
        Ok(Value::Int(2))
    ));
    // PHP erasures (note has: array_key_exists(key, array) — key first) + generic return types.
    let php = |n: &str, a: &[&str]| {
        let args: Vec<String> = a.iter().map(|s| (*s).to_string()).collect();
        (registry()[index_of("Core.Map", n).unwrap()].php)(&args)
    };
    assert_eq!(php("keys", &["$m"]), "array_keys($m)");
    assert_eq!(php("values", &["$m"]), "array_values($m)");
    assert_eq!(php("has", &["$m", "$k"]), "array_key_exists($k, $m)");
    assert_eq!(php("size", &["$m"]), "count($m)");
    assert_eq!(
        index_of_by_leaf("Map", "keys"),
        index_of("Core.Map", "keys")
    );
    assert_eq!(
        registry()[index_of("Core.Map", "keys").unwrap()].ret,
        Ty::List(Box::new(Ty::Param("K".into())))
    );
    assert_eq!(
        registry()[index_of("Core.Map", "values").unwrap()].ret,
        Ty::List(Box::new(Ty::Param("V".into())))
    );
}

#[test]
fn map_get_set_remove_eval_and_emit() {
    use crate::value::HKey;
    let mut o = String::new();
    let m = Value::Map(std::rc::Rc::new(vec![
        (HKey::Str("a".into()), Value::Int(1)),
        (HKey::Str("b".into()), Value::Int(2)),
    ]));
    // get: present → value, absent → null (V is non-optional, so null = absent).
    assert!(matches!(
        map_get(&[m.clone(), Value::Str("a".into())], &mut o),
        Ok(Value::Int(1))
    ));
    assert!(matches!(
        map_get(&[m.clone(), Value::Str("z".into())], &mut o),
        Ok(Value::Null)
    ));
    // set: a NEW map; existing key keeps position + takes new value, fresh key appends. Original
    // map is untouched (immutability).
    match map_set_native(&[m.clone(), Value::Str("a".into()), Value::Int(9)], &mut o).unwrap() {
        Value::Map(out) => {
            assert_eq!(out.len(), 2);
            assert!(matches!(&out[0], (HKey::Str(s), Value::Int(9)) if s == "a")); // updated in place
            assert!(matches!(&out[1], (HKey::Str(s), Value::Int(2)) if s == "b"));
        }
        other => panic!("set returned {other:?}"),
    }
    match map_set_native(&[m.clone(), Value::Str("c".into()), Value::Int(3)], &mut o).unwrap() {
        Value::Map(out) => {
            assert_eq!(out.len(), 3);
            assert!(matches!(&out[2], (HKey::Str(s), Value::Int(3)) if s == "c"));
            // appended
        }
        other => panic!("set(append) returned {other:?}"),
    }
    // the source map is unchanged after both sets.
    assert!(matches!(&m, Value::Map(src) if src.len() == 2 && matches!(src[0].1, Value::Int(1))));
    // remove: a NEW map without the key; removing an absent key is a no-op.
    match map_remove(&[m.clone(), Value::Str("a".into())], &mut o).unwrap() {
        Value::Map(out) => {
            assert_eq!(out.len(), 1);
            assert!(matches!(&out[0], (HKey::Str(s), Value::Int(2)) if s == "b"));
        }
        other => panic!("remove returned {other:?}"),
    }
    match map_remove(&[m.clone(), Value::Str("z".into())], &mut o).unwrap() {
        Value::Map(out) => assert_eq!(out.len(), 2), // no-op
        other => panic!("remove(absent) returned {other:?}"),
    }
    // non-hashable key faults cleanly (EV-7), never a panic.
    assert!(map_get(&[m.clone(), Value::Float(1.0)], &mut o).is_err());
    assert!(map_remove(&[m.clone(), Value::Float(1.0)], &mut o).is_err());
    // PHP erasures + generic types.
    let php = |n: &str, a: &[&str]| {
        let args: Vec<String> = a.iter().map(|s| (*s).to_string()).collect();
        (registry()[index_of("Core.Map", n).unwrap()].php)(&args)
    };
    assert_eq!(php("get", &["$m", "$k"]), "($m[$k] ?? null)");
    assert_eq!(
        php("set", &["$m", "$k", "$v"]),
        "__phorge_map_set($m, $k, $v)"
    );
    assert_eq!(php("remove", &["$m", "$k"]), "__phorge_map_remove($m, $k)");
    assert_eq!(
        registry()[index_of("Core.Map", "get").unwrap()].ret,
        Ty::Optional(Box::new(Ty::Param("V".into())))
    );
}
