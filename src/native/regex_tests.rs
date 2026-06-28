use super::*;

/// Build a compiled `Regex` value for a pattern (the helper the natives consume).
fn re(pattern: &str) -> Value {
    let mut o = String::new();
    regex_compile(&[Value::Str(pattern.to_string())], &mut o).expect("valid pattern")
}

/// `Value::Str` → its text (`Value` has no `PartialEq` — it holds `f64`).
fn text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        other => panic!("expected a string value, got {other:?}"),
    }
}

/// A `Value::List` of strings → `Vec<String>`.
fn texts(v: &Value) -> Vec<String> {
    match v {
        Value::List(xs) => xs.iter().map(text).collect(),
        other => panic!("expected a list value, got {other:?}"),
    }
}

#[test]
fn compile_validates_and_faults_on_unsupported() {
    let mut o = String::new();
    // A valid regular pattern compiles to a `Regex` instance carrying the bare pattern.
    match re(r"\d{4}") {
        Value::Instance(inst) => {
            assert_eq!(inst.class, "Regex");
            assert_eq!(text(inst.fields.borrow().get("pattern").unwrap()), r"\d{4}");
        }
        other => panic!("compile returned {other:?}"),
    }
    // Unbalanced — a clean fault, never a panic.
    assert!(regex_compile(&[Value::Str("(".to_string())], &mut o).is_err());
    // Backreferences are unsupported by the linear-time engine → rejected at compile (ReDoS-safe).
    assert!(regex_compile(&[Value::Str(r"(\w)\1".to_string())], &mut o).is_err());
}

#[test]
fn matches_find_find_all_split() {
    let mut o = String::new();
    let word = re(r"\w+");
    assert!(matches!(
        regex_matches(&[word.clone(), Value::Str("ab cd".into())], &mut o),
        Ok(Value::Bool(true))
    ));
    assert!(matches!(
        regex_matches(&[re(r"\d+"), Value::Str("no digits".into())], &mut o),
        Ok(Value::Bool(false))
    ));
    // find: first whole match, else null.
    assert!(matches!(
        regex_find(&[re(r"\d+"), Value::Str("x 42 y".into())], &mut o),
        Ok(Value::Str(s)) if s == "42"
    ));
    assert!(matches!(
        regex_find(&[re(r"\d+"), Value::Str("none".into())], &mut o),
        Ok(Value::Null)
    ));
    // findAll: every whole match.
    let all = regex_find_all(&[word.clone(), Value::Str("a bb ccc".into())], &mut o).unwrap();
    assert_eq!(texts(&all), vec!["a", "bb", "ccc"]);
    // split.
    let parts = regex_split(&[re(r",\s*"), Value::Str("p, q,r".into())], &mut o).unwrap();
    assert_eq!(texts(&parts), vec!["p", "q", "r"]);
}

#[test]
fn find_groups_named_only_and_null() {
    let mut o = String::new();
    let date = re(r"(?<y>\d{4})-(?<m>\d{2})");
    match regex_find_groups(&[date.clone(), Value::Str("2026-06".into())], &mut o).unwrap() {
        Value::Map(pairs) => {
            // Named captures in group-index order; numbered captures omitted.
            assert_eq!(pairs.len(), 2);
            assert_eq!(pairs[0].0, crate::value::HKey::Str("y".into()));
            assert_eq!(text(&pairs[0].1), "2026");
            assert_eq!(pairs[1].0, crate::value::HKey::Str("m".into()));
            assert_eq!(text(&pairs[1].1), "06");
        }
        other => panic!("findGroups returned {other:?}"),
    }
    // No match → null.
    assert!(matches!(
        regex_find_groups(&[date, Value::Str("nope".into())], &mut o),
        Ok(Value::Null)
    ));
}

#[test]
fn replace_all() {
    let mut o = String::new();
    assert!(matches!(
        regex_replace(
            &[re(r"\d+"), Value::Str("a1b22c".into()), Value::Str("#".into())],
            &mut o
        ),
        Ok(Value::Str(s)) if s == "a#b#c"
    ));
}

#[test]
fn non_regex_first_arg_is_a_clean_fault() {
    let mut o = String::new();
    // A value that isn't a Regex instance → fault, never a panic (EV-7).
    assert!(regex_matches(&[Value::Int(1), Value::Str("x".into())], &mut o).is_err());
}

#[test]
fn php_emission_shapes() {
    let nats = regex_natives();
    let emit = |name: &str, a: &[&str]| {
        let args: Vec<String> = a.iter().map(|s| (*s).to_string()).collect();
        (nats.iter().find(|n| n.name == name).unwrap().php)(&args)
    };
    assert_eq!(emit("compile", &["$p"]), "new Regex($p)");
    assert_eq!(
        emit("matches", &["$re", "$s"]),
        "__phorj_regex_matches($re, $s)"
    );
    assert_eq!(emit("find", &["$re", "$s"]), "__phorj_regex_find($re, $s)");
    assert_eq!(
        emit("findGroups", &["$re", "$s"]),
        "__phorj_regex_find_groups($re, $s)"
    );
}
