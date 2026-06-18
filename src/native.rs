//! Namespaced native (built-in) function registry — the stdlib's runtime + type + transpile
//! surface, addressed by `(module, name)` (e.g. module `core.console`, name `println`). One entry
//! single-sources all four facets of a native, so the four backends cannot drift:
//!   * `params` / `ret` — the checker's signature for a call to this native;
//!   * `eval` — the runtime behavior, shared by the tree-walking interpreter *and* the VM (the
//!     structural parity guarantee, exactly like the value kernels: one impl, two callers);
//!   * `php` — the transpile-time PHP emission (a `core.*` native erases to PHP's flat builtins;
//!     the namespace is a compile-time organizing layer, decisions N-2/D-L9).
//!
//! The registry is the load-bearing target of `import core.console;` (M3 namespace reshape, Wave 1,
//! `docs/specs/2026-06-18-m3-namespace-system-design.md`). The former free global `println` is
//! retired in favor of `core.console.println`, and `Op::Print` in favor of
//! `Op::CallNative(index, argc)` indexing this table.

use crate::ast::Item;
use crate::types::Ty;
use crate::value::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One built-in function, addressed by `(module, name)`. See the module docs for the four facets.
pub struct NativeFn {
    /// Dotted module path the native lives under — e.g. `"core.console"`.
    pub module: &'static str,
    /// Bare function name — e.g. `"println"`.
    pub name: &'static str,
    /// Parameter types — the checker validates call arguments against these.
    pub params: Vec<Ty>,
    /// Return type.
    pub ret: Ty,
    /// Runtime behavior, shared by the interpreter and the VM. Threads the program's output buffer
    /// so a side-effecting native (`console.println`) can append to it; pure natives ignore it. The
    /// arguments arrive in source order.
    pub eval: fn(&[Value], &mut String) -> Result<Value, String>,
    /// PHP emission: given the already-emitted PHP for each argument, return the PHP snippet this
    /// native erases to (decision N-2). For `console.println`: `echo {a} . "\n"`.
    pub php: fn(&[String]) -> String,
}

/// Pinned registry slot for `core.console.println` — the migrated former `Op::Print`. The compiler
/// bakes `Op::CallNative(CONSOLE_PRINTLN, 1)`; [`build`] self-checks this slot so the constant can
/// never silently drift from the table.
pub const CONSOLE_PRINTLN: usize = 0;

/// `console.println(string)` — append the argument's display rendering plus a newline to the
/// program's output buffer. Shared verbatim by both backends (the former `interpreter::
/// builtin_println` / VM `Op::Print` body); the space-join over multiple args is dead generality
/// (the checker fixes the arity at one `string`) kept for a future variadic.
fn console_println(args: &[Value], out: &mut String) -> Result<Value, String> {
    let mut line = String::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            line.push(' ');
        }
        match a.as_display() {
            Some(t) => line.push_str(&t),
            None => return Err(format!("println cannot print {}", a.type_name())),
        }
    }
    out.push_str(&line);
    out.push('\n');
    Ok(Value::Unit)
}

/// Index helper for a native's PHP emission: the already-emitted PHP for argument `i`, or `""` if
/// absent (the checker guarantees arity before `php` is ever called). Keeps the `php` closures terse.
fn parg(args: &[String], i: usize) -> &str {
    args.get(i).map_or("", String::as_str)
}

// ---- core.math ----------------------------------------------------------------------------------
// Concrete-typed numeric natives (`Ty` has no type variable, so no overloading): the float ops
// `sqrt`/`pow`/`floor`/`ceil` are `float -> float`; `abs`/`min`/`max` are `int`. Each erases to the
// PHP builtin of the same name (D-L9). NOTE (KNOWN_ISSUES, float precision): an *irrational* result
// (`sqrt(2.0)`) renders with more digits on the Rust backends than PHP's default 14-sig-digit `echo`,
// so examples stay on exactly-representable values; the run↔runvm spine is unaffected (both Rust).

fn math_sqrt(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.sqrt())),
        _ => Err("math.sqrt expects (float)".into()),
    }
}
fn math_pow(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(b), Value::Float(e)] => Ok(Value::Float(b.powf(*e))),
        _ => Err("math.pow expects (float, float)".into()),
    }
}
fn math_floor(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.floor())),
        _ => Err("math.floor expects (float)".into()),
    }
}
fn math_ceil(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Float(x)] => Ok(Value::Float(x.ceil())),
        _ => Err("math.ceil expects (float)".into()),
    }
}
fn math_abs(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // `i64::MIN.abs()` overflows; a clean fault keeps EV-7 (never panic on input).
        [Value::Int(n)] => n
            .checked_abs()
            .map(Value::Int)
            .ok_or_else(|| "integer overflow in math.abs".to_string()),
        _ => Err("math.abs expects (int)".into()),
    }
}
fn math_min(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok(Value::Int((*a).min(*b))),
        _ => Err("math.min expects (int, int)".into()),
    }
}
fn math_max(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok(Value::Int((*a).max(*b))),
        _ => Err("math.max expects (int, int)".into()),
    }
}

/// The `core.math` registry entries (M3 Track B Wave 2).
fn math_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "core.math",
            name: "sqrt",
            params: vec![Ty::Float],
            ret: Ty::Float,
            eval: math_sqrt,
            php: |a| format!("sqrt({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.math",
            name: "pow",
            params: vec![Ty::Float, Ty::Float],
            ret: Ty::Float,
            eval: math_pow,
            php: |a| format!("pow({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "core.math",
            name: "floor",
            params: vec![Ty::Float],
            ret: Ty::Float,
            eval: math_floor,
            php: |a| format!("floor({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.math",
            name: "ceil",
            params: vec![Ty::Float],
            ret: Ty::Float,
            eval: math_ceil,
            php: |a| format!("ceil({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.math",
            name: "abs",
            params: vec![Ty::Int],
            ret: Ty::Int,
            eval: math_abs,
            php: |a| format!("abs({})", parg(a, 0)),
        },
        NativeFn {
            module: "core.math",
            name: "min",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            eval: math_min,
            php: |a| format!("min({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "core.math",
            name: "max",
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            eval: math_max,
            php: |a| format!("max({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

/// Construct the native table once. Order is load-bearing: [`CONSOLE_PRINTLN`] pins slot 0; every
/// other native is resolved by `(module, name)` (or leaf+name) at compile time, so appended order is
/// free. Modules are grouped by `*_natives()` builders (one per `core.*` leaf).
fn build() -> Vec<NativeFn> {
    let mut registry = vec![NativeFn {
        module: "core.console",
        name: "println",
        params: vec![Ty::String],
        ret: Ty::Unit,
        eval: console_println,
        php: |args| {
            let a = args
                .first()
                .map_or_else(|| "\"\"".to_string(), String::clone);
            format!(r#"echo {a} . "\n""#)
        },
    }];
    registry.extend(math_natives());
    // Pinned-slot invariant: the constant the compiler bakes into `Op::CallNative` must address the
    // entry it names. Cheap one-time check at first `registry()` access.
    assert_eq!(
        registry[CONSOLE_PRINTLN].module, "core.console",
        "CONSOLE_PRINTLN slot drifted"
    );
    assert_eq!(registry[CONSOLE_PRINTLN].name, "println");
    registry
}

/// The process-wide native table, built once. A `Vec<Ty>` isn't const-constructible, so this can't
/// be a plain `static` — `OnceLock` defers the allocation to first use (design §5).
pub fn registry() -> &'static [NativeFn] {
    static REG: OnceLock<Vec<NativeFn>> = OnceLock::new();
    REG.get_or_init(build)
}

/// Index of the native `(module, name)`, or `None`. Used by the checker and the transpiler, which
/// carry the import map and resolve the *exact* module a leaf qualifier was imported as.
pub fn index_of(module: &str, name: &str) -> Option<usize> {
    registry()
        .iter()
        .position(|n| n.module == module && n.name == name)
}

/// Index of a native by its module's *leaf* segment + name — e.g. leaf `"console"`, name
/// `"println"`. Used by the interpreter and compiler, which (unlike the transpiler) track variable
/// scope and resolve a member call `q.m(..)` locals-first: a qualifier `q` is only leaf-looked-up
/// once it is known *not* to be a bound variable, and the checker has already enforced that `q` was
/// imported and the native exists. Unambiguous while every stdlib leaf is distinct (Waves 1–2);
/// leaf collisions with user packages are resolved by import aliasing (design O-D, deferred).
pub fn index_of_by_leaf(leaf: &str, name: &str) -> Option<usize> {
    registry()
        .iter()
        .position(|n| n.name == name && n.module.rsplit('.').next() == Some(leaf))
}

/// Build the active import map (leaf qualifier → full dotted module path) from a program's items:
/// `import core.console;` binds the call-site qualifier `console` to module `core.console`. Carried
/// by the checker (import-required + shadowing enforcement) and the transpiler (which has no
/// variable-scope tracking to tell a qualifier from a value).
pub fn import_map(items: &[Item]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for item in items {
        if let Item::Import { path, .. } = item {
            if let Some(leaf) = path.last() {
                map.insert(leaf.clone(), path.join("."));
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_console_println_slot() {
        let r = registry();
        assert_eq!(r[CONSOLE_PRINTLN].module, "core.console");
        assert_eq!(r[CONSOLE_PRINTLN].name, "println");
    }

    #[test]
    fn index_lookups_resolve_console_println() {
        assert_eq!(index_of("core.console", "println"), Some(CONSOLE_PRINTLN));
        assert_eq!(
            index_of_by_leaf("console", "println"),
            Some(CONSOLE_PRINTLN)
        );
        assert_eq!(index_of("core.console", "nope"), None);
        assert_eq!(index_of_by_leaf("nope", "println"), None);
    }

    #[test]
    fn console_println_appends_line() {
        let mut out = String::new();
        let r = console_println(&[Value::Str("hi".into())], &mut out).unwrap();
        assert_eq!(out, "hi\n");
        assert!(matches!(r, Value::Unit));
    }

    #[test]
    fn console_println_rejects_composite() {
        let mut out = String::new();
        let err = console_println(&[Value::List(vec![].into())], &mut out).unwrap_err();
        assert!(err.contains("cannot print"), "{err}");
    }

    #[test]
    fn php_emission_is_echo_with_newline() {
        let php = (registry()[CONSOLE_PRINTLN].php)(&["$x".to_string()]);
        assert_eq!(php, r#"echo $x . "\n""#);
    }

    #[test]
    fn math_natives_eval_and_emit() {
        let mut out = String::new();
        // float ops
        assert!(
            matches!(math_sqrt(&[Value::Float(16.0)], &mut out), Ok(Value::Float(x)) if x == 4.0)
        );
        assert!(
            matches!(math_pow(&[Value::Float(2.0), Value::Float(10.0)], &mut out), Ok(Value::Float(x)) if x == 1024.0)
        );
        assert!(
            matches!(math_floor(&[Value::Float(3.7)], &mut out), Ok(Value::Float(x)) if x == 3.0)
        );
        assert!(
            matches!(math_ceil(&[Value::Float(3.2)], &mut out), Ok(Value::Float(x)) if x == 4.0)
        );
        // int ops
        assert!(matches!(
            math_abs(&[Value::Int(-5)], &mut out),
            Ok(Value::Int(5))
        ));
        assert!(matches!(
            math_min(&[Value::Int(3), Value::Int(8)], &mut out),
            Ok(Value::Int(3))
        ));
        assert!(matches!(
            math_max(&[Value::Int(3), Value::Int(8)], &mut out),
            Ok(Value::Int(8))
        ));
        // EV-7: abs of i64::MIN faults, never panics
        assert!(math_abs(&[Value::Int(i64::MIN)], &mut out).is_err());
        // resolvable by both index forms + PHP erasure to the same-named builtin
        let i = index_of("core.math", "pow").expect("pow registered");
        assert_eq!(index_of_by_leaf("math", "pow"), Some(i));
        assert_eq!(
            (registry()[i].php)(&["2.0".into(), "10.0".into()]),
            "pow(2.0, 10.0)"
        );
        assert_eq!(
            (registry()[index_of("core.math", "min").unwrap()].php)(&["$a".into(), "$b".into()]),
            "min($a, $b)"
        );
    }

    #[test]
    fn import_map_binds_leaf_to_full_path() {
        use crate::token::Span;
        let sp = Span {
            start: 0,
            len: 0,
            line: 1,
            col: 1,
        };
        let items = vec![Item::Import {
            path: vec!["core".into(), "console".into()],
            span: sp,
        }];
        let m = import_map(&items);
        assert_eq!(m.get("console").map(String::as_str), Some("core.console"));
    }
}
