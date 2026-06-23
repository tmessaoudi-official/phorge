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
