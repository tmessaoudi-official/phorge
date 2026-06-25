//! `Core.Reflect` — read-only, name-level runtime reflection
//! (`docs/specs/2026-06-25-core-reflect-design.md`).
//!
//! This module hosts the natives whose result a value can compute on its own. The hard part of the
//! design — `typeName`/`className` (resolved by *static* type in a checker pass) and the class-table
//! enumeration natives (`interfaces`/`parents`/… via `NativeEval::Reflective`) — lives elsewhere;
//! this is the foundation slice: `Reflect.kind`.
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

pub(crate) fn reflect_natives() -> Vec<NativeFn> {
    vec![NativeFn {
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
    }]
}

#[cfg(test)]
#[path = "reflect_tests.rs"]
mod tests;
