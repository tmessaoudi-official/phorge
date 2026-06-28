use super::*;

#[test]
fn file_natives_eval_and_emit() {
    let mut o = String::new();
    // A missing path reads as `null` (the `string?` absent case), never a fault.
    let missing = "/nonexistent/phorj/definitely/not/here.txt";
    assert!(matches!(
        file_read(&[Value::Str(missing.into())], &mut o),
        Ok(Value::Null)
    ));
    assert!(matches!(
        file_exists(&[Value::Str(missing.into())], &mut o),
        Ok(Value::Bool(false))
    ));
    // write → read round-trip through a temp file (write is unit-tested, not exampled).
    let tmp = std::env::temp_dir().join("phorj_native_file_test.txt");
    let p = tmp.to_string_lossy().to_string();
    let _ = std::fs::remove_file(&tmp);
    assert!(matches!(
        file_write(&[Value::Str(p.clone()), Value::Str("hi\n".into())], &mut o),
        Ok(Value::Unit)
    ));
    assert!(matches!(
        file_exists(&[Value::Str(p.clone())], &mut o),
        Ok(Value::Bool(true))
    ));
    assert!(
        matches!(file_read(&[Value::Str(p.clone())], &mut o), Ok(Value::Str(s)) if s == "hi\n")
    );
    let _ = std::fs::remove_file(&tmp);
    // `read` returns `string?`; PHP erasure distinguishes empty file from missing.
    assert_eq!(
        crate::native::registry()[index_of("Core.File", "read").unwrap()].ret,
        Ty::Optional(Box::new(Ty::String))
    );
    assert_eq!(
        (registry()[index_of("Core.File", "read").unwrap()].php)(&["$p".into()]),
        "(($__c = @file_get_contents($p)) === false ? null : $__c)"
    );
    assert_eq!(
        index_of_by_leaf("File", "exists"),
        index_of("Core.File", "exists")
    );
}
