use super::*;

#[test]
fn process_env_natives_are_impure_and_registered() {
    for (module, name) in [
        ("Core.Process", "args"),
        ("Core.Env", "get"),
        ("Core.Env", "all"),
    ] {
        let i = index_of(module, name).unwrap_or_else(|| panic!("{module}.{name} registered"));
        assert!(
            !registry()[i].pure,
            "{module}.{name} must be marked impure (pure == false)"
        );
        assert_eq!(
            index_of_by_leaf(module.rsplit('.').next().unwrap(), name),
            Some(i)
        );
    }
}

#[test]
fn every_other_native_is_pure() {
    // The quarantine seam relies on exactly the quarantined modules being impure: the
    // ambient-environment natives (`Core.Process`/`Core.Env`) plus the global-state generator
    // (`Core.Random`, whose PHP `mt_rand` sequence can't match the Rust kernel).
    let impure_modules = ["Core.Process", "Core.Env", "Core.Random"];
    for n in registry() {
        let impure = impure_modules.contains(&n.module);
        assert_eq!(
            n.pure, !impure,
            "{}.{} purity flag disagrees with its module",
            n.module, n.name
        );
    }
}

#[test]
fn process_args_reads_the_process_global() {
    set_process_args(vec!["alpha".into(), "beta".into()]);
    let mut out = String::new();
    match process_args(&[], &mut out) {
        Ok(Value::List(items)) => {
            let got: Vec<String> = items
                .iter()
                .map(|v| match v {
                    Value::Str(s) => s.clone(),
                    other => panic!("non-string arg {other:?}"),
                })
                .collect();
            assert_eq!(got, vec!["alpha".to_string(), "beta".to_string()]);
        }
        other => panic!("process_args returned {other:?}"),
    }
    set_process_args(Vec::new()); // reset so other tests aren't affected
}

#[test]
fn env_get_unset_is_null() {
    // The crate forbids `unsafe`, so setting an env var (now `unsafe` in edition 2024) lives in the
    // integration test `tests/process.rs`. Here we only assert the deterministic unset → null case.
    let mut out = String::new();
    assert!(matches!(
        env_get(
            &[Value::Str("PHORGE_TEST_ENV_DEFINITELY_UNSET_XYZ".into())],
            &mut out
        ),
        Ok(Value::Null)
    ));
}

#[test]
fn env_all_is_sorted_by_key() {
    let mut out = String::new();
    match env_all(&[], &mut out) {
        Ok(Value::Map(pairs)) => {
            let keys: Vec<&HKey> = pairs.iter().map(|(k, _)| k).collect();
            let mut sorted = keys.clone();
            sorted.sort_by(|a, b| match (a, b) {
                (HKey::Str(x), HKey::Str(y)) => x.cmp(y),
                _ => std::cmp::Ordering::Equal,
            });
            assert_eq!(keys, sorted, "Env.all() keys must be sorted");
        }
        other => panic!("env_all returned {other:?}"),
    }
}
