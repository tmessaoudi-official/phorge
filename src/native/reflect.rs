//! `Core.Reflect` — read-only, name-level runtime reflection
//! (`docs/specs/2026-06-25-core-reflect-design.md`).
//!
//! This module hosts the natives whose result a value can compute on its own: `Reflect.kind` (the
//! coarse erasure-stable tag) and `Reflect.className` (the runtime `get_class` name, or null). The
//! genuinely static piece — `typeName` (resolved by *static* type in a checker pass) and the
//! class-table enumeration natives (`interfaces`/`parents`/… via `NativeEval::Reflective`) — lands
//! in later slices.
//!
//! **`kind` is the coarse, PHP-reproducible type tag** (the developer's "parent type" idea). It
//! returns exactly what the PHP backend can still see *after erasure*, so it is byte-identical for
//! every input: `List`/`Map`/`Set` all collapse to `"array"`, `bytes` to `"string"`, instances and
//! enum variants to `"object"`, a closure to `"callable"`. The finer Phorge distinctions
//! (Map-vs-Set, the enum/class name) are the job of `typeName`/`className`, which are resolved from
//! the static type at compile time and never consult PHP's erased runtime (see the spec).
//!
//! Erasure: `kind` emits the gated `__phorge_kind($x)` helper (defined once in
//! `transpile::program::emit_runtime_helpers`). A native's `php` closure can't set the transpiler's
//! `uses_*` flag, so `emit_member_call` special-cases `Core.Reflect.kind` to set `uses_reflect_kind`
//! before emitting — the established gated-helper pattern (`__phorge_str`/`__phorge_div`/…).

use super::*;
use crate::types::Ty;
use crate::value::Value;

/// `Reflect.kind(x) -> string` — the coarse, erasure-stable type tag. Mirrors the `__phorge_kind`
/// PHP helper exactly (which checks `is_callable` before `is_object`, since a PHP closure is both).
fn reflect_kind(args: &[Value], _: &mut String) -> Result<Value, String> {
    let kind = match args {
        [v] => match v {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            // A real UTF-8 string and erased `bytes` are both a PHP `string` — coarse kind agrees.
            Value::Str(_) | Value::Bytes(_) => "string",
            // List/Map/Set all erase to a PHP `array`.
            Value::List(_) | Value::Map(_) | Value::Set(_) => "array",
            // A closure is `is_callable` in PHP; instances and enum variants are plain objects.
            Value::Closure(_) => "callable",
            Value::Instance(_) | Value::Enum(_) => "object",
            // `null` is its own kind; `unit` (void) never reaches here (uncapturable), but map it
            // to PHP's `null` defensively so the arm is total.
            Value::Null | Value::Unit => "null",
        },
        _ => return Err("Reflect.kind expects (T)".into()),
    };
    Ok(Value::Str(kind.to_string()))
}

/// `Reflect.className(x) -> string?` — the runtime class name for an object (`get_class`), or `null`
/// for a non-object. Byte-identical with PHP `get_class` for a `package Main` class. An enum variant
/// reports the **variant** name (`"Red"`) — PHP transpiles a variant to a `final class <Variant>
/// extends <Enum>`, so `get_class` returns the variant subclass (Q3). A closure is excluded (PHP's
/// `get_class` would report `"Closure"`; both sides agree on `null` instead — the helper guards it).
fn reflect_class_name(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Instance(i)] => Ok(Value::Str(i.class.clone())),
        [Value::Enum(e)] => Ok(Value::Str(e.variant.clone())),
        // A scalar / collection / closure is not a class instance → `null` (string?).
        [_] => Ok(Value::Null),
        _ => Err("Reflect.className expects (T)".into()),
    }
}

pub(crate) fn reflect_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Reflect",
            name: "kind",
            // Generic over any single argument (S7b registry-`Ty::Param` discipline — never erased to a
            // backend; the compiler types the call by expression shape, the transpiler via `php`).
            params: vec![Ty::Param("T".into())],
            ret: Ty::String,
            eval: NativeEval::Pure(reflect_kind),
            // `emit_member_call` sets `uses_reflect_kind` before calling this (the gated-helper pattern);
            // the helper is defined once in `emit_runtime_helpers`. `looks_like_global_call` adds the
            // leading `\` in namespaced mode.
            php: |a| format!("__phorge_kind({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Reflect",
            name: "className",
            params: vec![Ty::Param("T".into())],
            ret: Ty::Optional(Box::new(Ty::String)),
            eval: NativeEval::Pure(reflect_class_name),
            // Gated `__phorge_class_name` helper (set in `emit_member_call`): single-evaluates its
            // argument (an inline `is_object($x) ? get_class($x) : null` would double-evaluate a
            // side-effecting argument) and excludes closures, matching the Rust arm.
            php: |a| format!("__phorge_class_name({})", parg(a, 0)),
        },
    ]
}

#[cfg(test)]
#[path = "reflect_tests.rs"]
mod tests;
