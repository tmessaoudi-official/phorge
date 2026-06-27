//! `Core.Test` — unit-test assertions (M-Test T2). Each assertion is an ordinary native: it returns
//! `unit` on success and **faults** on failure (a plain `Err(String)`, which the backends turn into a
//! runtime fault). The `phg test` runner (M-Test T3) catches that fault per-`test` block, records a
//! failure, and continues — so a failed assertion reads exactly like any other fault, with the
//! Slice-1 stack trace, and needs no new control-flow concept.
//!
//! These natives are `pure` (deterministic) but only meaningful inside a `test` block run by
//! `phg test`; they never appear in a byte-identity `examples/` program, so the PHP oracle never
//! exercises them. The `php` emission exists only for a future `--emit-phpunit` bridge (D4) and uses
//! PHP 8's `throw` expression; it is **not** byte-identity-gated.

use super::*;
use crate::types::Ty;
use crate::value::Value;

/// Render a value for an assertion message — its display form, or its type name when it has none
/// (e.g. a class instance), so the message is always informative and never fails.
fn shown(v: &Value) -> String {
    v.as_display()
        .unwrap_or_else(|| format!("<{}>", v.type_name()))
}

fn test_assert(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Bool(c), Value::Str(msg)] => {
            if *c {
                Ok(Value::Unit)
            } else {
                Err(format!("assertion failed: {msg}"))
            }
        }
        _ => Err("Test.assert expects (bool, string)".into()),
    }
}

fn test_assert_true(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Bool(true)] => Ok(Value::Unit),
        [Value::Bool(false)] => Err("assertion failed: expected true, got false".into()),
        _ => Err("Test.assertTrue expects (bool)".into()),
    }
}

fn test_assert_false(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Bool(false)] => Ok(Value::Unit),
        [Value::Bool(true)] => Err("assertion failed: expected false, got true".into()),
        _ => Err("Test.assertFalse expects (bool)".into()),
    }
}

fn test_assert_equals(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Value equality via the shared `eq_val` kernel (the same equality `==` uses), so the two
        // backends agree. The message names both operands without claiming which is "expected".
        [a, b] => {
            if a.eq_val(b) {
                Ok(Value::Unit)
            } else {
                Err(format!(
                    "assertion failed: {} is not equal to {}",
                    shown(a),
                    shown(b)
                ))
            }
        }
        _ => Err("Test.assertEquals expects (T, T)".into()),
    }
}

fn test_assert_not_equals(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [a, b] => {
            if a.eq_val(b) {
                Err(format!(
                    "assertion failed: {} is unexpectedly equal to {}",
                    shown(a),
                    shown(b)
                ))
            } else {
                Ok(Value::Unit)
            }
        }
        _ => Err("Test.assertNotEquals expects (T, T)".into()),
    }
}

fn test_assert_null(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Null] => Ok(Value::Unit),
        [v] => Err(format!("assertion failed: expected null, got {}", shown(v))),
        _ => Err("Test.assertNull expects (T?)".into()),
    }
}

fn test_assert_not_null(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Null] => Err("assertion failed: expected non-null, got null".into()),
        [_] => Ok(Value::Unit),
        _ => Err("Test.assertNotNull expects (T?)".into()),
    }
}

/// The `Core.Test` registry entries (M-Test T2). All `pure` (deterministic) but only used by the
/// `phg test` runner — never in a byte-identity example, so never seen by the PHP oracle.
pub(crate) fn test_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Test",
            name: "assert",
            params: vec![Ty::Bool, Ty::String],
            ret: Ty::Void,
            pure: true,
            eval: NativeEval::Pure(test_assert),
            php: |a| {
                format!(
                    "({} ? null : throw new \\Exception('assertion failed: ' . {}))",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.Test",
            name: "assertTrue",
            params: vec![Ty::Bool],
            ret: Ty::Void,
            pure: true,
            eval: NativeEval::Pure(test_assert_true),
            php: |a| {
                format!(
                    "({} ? null : throw new \\Exception('assertion failed: expected true, got false'))",
                    parg(a, 0)
                )
            },
        },
        NativeFn {
            module: "Core.Test",
            name: "assertFalse",
            params: vec![Ty::Bool],
            ret: Ty::Void,
            pure: true,
            eval: NativeEval::Pure(test_assert_false),
            php: |a| {
                format!(
                    "(!{} ? null : throw new \\Exception('assertion failed: expected false, got true'))",
                    parg(a, 0)
                )
            },
        },
        NativeFn {
            module: "Core.Test",
            name: "assertEquals",
            params: vec![Ty::Param("T".into()), Ty::Param("T".into())],
            ret: Ty::Void,
            pure: true,
            eval: NativeEval::Pure(test_assert_equals),
            php: |a| {
                format!(
                    "(({}) == ({}) ? null : throw new \\Exception('assertion failed: values not equal'))",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.Test",
            name: "assertNotEquals",
            params: vec![Ty::Param("T".into()), Ty::Param("T".into())],
            ret: Ty::Void,
            pure: true,
            eval: NativeEval::Pure(test_assert_not_equals),
            php: |a| {
                format!(
                    "(({}) != ({}) ? null : throw new \\Exception('assertion failed: values unexpectedly equal'))",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.Test",
            name: "assertNull",
            // A plain generic `T` (not `T?`): like PHPUnit, `assertNull` accepts any value — the eval
            // checks for `null` at runtime. (The native-generic unifier binds `T` from the argument;
            // a `T?` param would reject a non-optional argument since it does not widen on binding.)
            params: vec![Ty::Param("T".into())],
            ret: Ty::Void,
            pure: true,
            eval: NativeEval::Pure(test_assert_null),
            php: |a| {
                format!(
                    "(({}) === null ? null : throw new \\Exception('assertion failed: expected null'))",
                    parg(a, 0)
                )
            },
        },
        NativeFn {
            module: "Core.Test",
            name: "assertNotNull",
            params: vec![Ty::Param("T".into())],
            ret: Ty::Void,
            pure: true,
            eval: NativeEval::Pure(test_assert_not_null),
            php: |a| {
                format!(
                    "(({}) !== null ? null : throw new \\Exception('assertion failed: expected non-null'))",
                    parg(a, 0)
                )
            },
        },
    ]
}

#[cfg(test)]
#[path = "test_tests.rs"]
mod tests;
