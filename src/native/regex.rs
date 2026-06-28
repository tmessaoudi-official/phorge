//! `Core.Regex` — a ReDoS-safe regular-expression engine over a compiler-injected `Regex` class
//! value (`docs/specs/2026-06-28-core-regex-design.md`). Backed by the `regex` crate (the project's
//! 2nd vetted dependency, `docs/specs/2026-06-27-dependency-policy.md`): a RE2-style finite automaton
//! with **guaranteed linear-time matching** (ReDoS-immune by construction). Its restricted feature
//! set (no backreferences / lookaround) is exactly the *regular* subset PHP `preg_*` matches
//! identically, so the byte-identity spine holds; an unsupported pattern is rejected at
//! [`Regex.compile`] (a clean fault), never reaching a backend.
//!
//! A compiled `Regex` value is a `Value::Instance { class: "Regex", fields: { pattern } }` carrying
//! the **bare** pattern (no delimiters), built directly by `compile` (the hand-built-value technique,
//! exactly like `Core.Json`'s `jnode`). The user constructs one only via `Regex.compile`. The
//! engines are memoized in a thread-local cache keyed by the bare pattern, recovering "compile once,
//! reuse" with no new `Value` variant. The PHP transpile is a peer emission target only — the
//! engine runs natively on both Rust backends (the dependency-policy native-runtime rule).

use super::*;
use crate::types::Ty;
use crate::value::{build_map, Instance, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    /// Memoized compiled engines, keyed by the **bare** pattern. `compile` populates it (and is the
    /// only place a compile error can surface); the query natives look up the cached engine, with a
    /// recompile fallback that cannot fault (the pattern was already validated by `compile`). Pure
    /// optimization — semantics are identical without the cache.
    static CACHE: RefCell<HashMap<String, Rc<::regex::Regex>>> = RefCell::new(HashMap::new());
}

/// Compile `pattern` (bare, Unicode), memoizing the engine. Returns a clean fault on an invalid or
/// unsupported (backref/lookaround) pattern — the `regex` crate's own compile error, surfaced
/// uniformly so the failure is identical on both backends.
fn compiled(pattern: &str) -> Result<Rc<::regex::Regex>, String> {
    if let Some(re) = CACHE.with(|c| c.borrow().get(pattern).cloned()) {
        return Ok(re);
    }
    match ::regex::Regex::new(pattern) {
        Ok(re) => {
            let re = Rc::new(re);
            CACHE.with(|c| c.borrow_mut().insert(pattern.to_string(), re.clone()));
            Ok(re)
        }
        Err(e) => Err(format!("invalid or unsupported regex: {e}")),
    }
}

/// Build the opaque `Regex` value holding the bare pattern.
fn regex_value(pattern: &str) -> Value {
    let mut fields = crate::value::FieldMap::default();
    fields.insert("pattern".to_string(), Value::Str(pattern.to_string()));
    Value::Instance(Rc::new(Instance {
        class: "Regex".to_string(),
        fields: RefCell::new(fields),
    }))
}

/// Extract the bare pattern from a `Regex` instance argument.
fn as_pattern(v: &Value) -> Result<String, String> {
    match v {
        Value::Instance(inst) if inst.class == "Regex" => inst
            .fields
            .borrow()
            .get("pattern")
            .and_then(|p| match p {
                Value::Str(s) => Some(s.clone()),
                _ => None,
            })
            .ok_or_else(|| "Regex value is missing its pattern".to_string()),
        _ => Err(format!("Regex value expected, got {}", v.type_name())),
    }
}

// ---- natives ------------------------------------------------------------------------------------

/// `Regex.compile(string) -> Regex` — validate + memoize; faults on an invalid/unsupported pattern.
fn regex_compile(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(p)] => {
            compiled(p)?; // validate now (and cache); the value carries only the bare pattern.
            Ok(regex_value(p))
        }
        _ => Err("Regex.compile expects (string)".into()),
    }
}

/// `Regex.matches(Regex, string) -> bool` — is there a match anywhere in the subject?
fn regex_matches(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [re, Value::Str(s)] => {
            let pat = as_pattern(re)?;
            Ok(Value::Bool(compiled(&pat)?.is_match(s)))
        }
        _ => Err("Regex.matches expects (Regex, string)".into()),
    }
}

/// `Regex.find(Regex, string) -> string?` — the first whole match, else `null`.
fn regex_find(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [re, Value::Str(s)] => {
            let pat = as_pattern(re)?;
            Ok(compiled(&pat)?
                .find(s)
                .map_or(Value::Null, |m| Value::Str(m.as_str().to_string())))
        }
        _ => Err("Regex.find expects (Regex, string)".into()),
    }
}

/// `Regex.findAll(Regex, string) -> List<string>` — every whole match (empty list if none).
fn regex_find_all(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [re, Value::Str(s)] => {
            let pat = as_pattern(re)?;
            let out: Vec<Value> = compiled(&pat)?
                .find_iter(s)
                .map(|m| Value::Str(m.as_str().to_string()))
                .collect();
            Ok(Value::List(Rc::new(out)))
        }
        _ => Err("Regex.findAll expects (Regex, string)".into()),
    }
}

/// `Regex.findGroups(Regex, string) -> Map<string, string>?` — the **named** captures of the first
/// match, keyed by group name, else `null`. Numbered-only captures are omitted (named is the API).
fn regex_find_groups(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [re, Value::Str(s)] => {
            let pat = as_pattern(re)?;
            let engine = compiled(&pat)?;
            match engine.captures(s) {
                None => Ok(Value::Null),
                Some(caps) => {
                    let mut pairs: Vec<(Value, Value)> = Vec::new();
                    for name in engine.capture_names().flatten() {
                        if let Some(m) = caps.name(name) {
                            pairs.push((
                                Value::Str(name.to_string()),
                                Value::Str(m.as_str().to_string()),
                            ));
                        }
                    }
                    Ok(Value::Map(Rc::new(build_map(pairs)?)))
                }
            }
        }
        _ => Err("Regex.findGroups expects (Regex, string)".into()),
    }
}

/// `Regex.replace(Regex, string, string) -> string` — replace every match. The replacement uses the
/// `$1` / `${name}` capture syntax shared by the `regex` crate and PHP `preg_replace`.
fn regex_replace(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [re, Value::Str(s), Value::Str(repl)] => {
            let pat = as_pattern(re)?;
            Ok(Value::Str(
                compiled(&pat)?.replace_all(s, repl.as_str()).into_owned(),
            ))
        }
        _ => Err("Regex.replace expects (Regex, string, string)".into()),
    }
}

/// `Regex.split(Regex, string) -> List<string>` — split the subject on matches.
fn regex_split(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [re, Value::Str(s)] => {
            let pat = as_pattern(re)?;
            let out: Vec<Value> = compiled(&pat)?
                .split(s)
                .map(|p| Value::Str(p.to_string()))
                .collect();
            Ok(Value::List(Rc::new(out)))
        }
        _ => Err("Regex.split expects (Regex, string)".into()),
    }
}

// ---- registry -----------------------------------------------------------------------------------

/// The `Core.Regex` registry entries. `Regex` is the compiler-injected class
/// (`cli::inject_regex_prelude`) — referenced as a bare `Ty::Named`; the type resolves because a call
/// to any of these natives requires `import Core.Regex;`, which triggers the injection before the
/// checker runs. The `php` emitters reference the `__phorge_regex_*` runtime helpers
/// (`transpile/program.rs`); the injected `Regex` class transpiles to a PHP class with a public
/// `$pattern` (the bare pattern), so a global helper can build the `/u`-delimited form.
pub(crate) fn regex_natives() -> Vec<NativeFn> {
    let regex_ty = || Ty::Named("Regex".to_string(), vec![]);
    let list_str = || Ty::List(Box::new(Ty::String));
    let opt_str = || Ty::Optional(Box::new(Ty::String));
    let opt_map = || {
        Ty::Optional(Box::new(Ty::Map(
            Box::new(Ty::String),
            Box::new(Ty::String),
        )))
    };
    vec![
        NativeFn {
            module: "Core.Regex",
            name: "compile",
            params: vec![Ty::String],
            ret: regex_ty(),
            pure: true,
            eval: NativeEval::Pure(regex_compile),
            php: |a| format!("new Regex({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Regex",
            name: "matches",
            params: vec![regex_ty(), Ty::String],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(regex_matches),
            php: |a| format!("__phorge_regex_matches({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Regex",
            name: "find",
            params: vec![regex_ty(), Ty::String],
            ret: opt_str(),
            pure: true,
            eval: NativeEval::Pure(regex_find),
            php: |a| format!("__phorge_regex_find({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Regex",
            name: "findAll",
            params: vec![regex_ty(), Ty::String],
            ret: list_str(),
            pure: true,
            eval: NativeEval::Pure(regex_find_all),
            php: |a| format!("__phorge_regex_find_all({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Regex",
            name: "findGroups",
            params: vec![regex_ty(), Ty::String],
            ret: opt_map(),
            pure: true,
            eval: NativeEval::Pure(regex_find_groups),
            php: |a| format!("__phorge_regex_find_groups({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.Regex",
            name: "replace",
            params: vec![regex_ty(), Ty::String, Ty::String],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(regex_replace),
            php: |a| {
                format!(
                    "__phorge_regex_replace({}, {}, {})",
                    parg(a, 0),
                    parg(a, 1),
                    parg(a, 2)
                )
            },
        },
        NativeFn {
            module: "Core.Regex",
            name: "split",
            params: vec![regex_ty(), Ty::String],
            ret: list_str(),
            pure: true,
            eval: NativeEval::Pure(regex_split),
            php: |a| format!("__phorge_regex_split({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

#[cfg(test)]
#[path = "regex_tests.rs"]
mod tests;
