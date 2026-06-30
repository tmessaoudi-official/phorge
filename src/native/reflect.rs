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
//! enum variants to `"object"`, a closure to `"callable"`. The finer Phorj distinctions
//! (Map-vs-Set, the enum/class name) are the job of `typeName`/`className`, which are resolved from
//! the static type at compile time and never consult PHP's erased runtime (see the spec).
//!
//! Erasure: `kind` emits the gated `__phorj_kind($x)` helper (defined once in
//! `transpile::program::emit_runtime_helpers`). A native's `php` closure can't set the transpiler's
//! `uses_*` flag, so `emit_member_call` special-cases `Core.Reflect.kind` to set `uses_reflect_kind`
//! before emitting — the established gated-helper pattern (`__phorj_str`/`__phorj_div`/…).

use super::*;
use crate::types::Ty;
use crate::value::Value;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Look up the sorted name list for `args[0]`'s class in a [`ClassTables`] map, as a `List<string>`.
/// A non-class value (scalar, collection, enum variant, closure) has no entry → the empty list (PHP
/// agrees: `__phorj_reflect_of` returns `[]` for a non-object / unknown class).
fn reflect_class_list(args: &[Value], table: &BTreeMap<String, Vec<String>>) -> Value {
    let names = match args {
        [Value::Instance(i)] => table.get(&i.class).cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    Value::List(Rc::new(names.into_iter().map(Value::Str).collect()))
}

/// `Reflect.interfaces(x) -> List<string>` — the (sorted, transitive) interfaces `x`'s class
/// implements, or `[]` for a non-class value.
fn reflect_interfaces(args: &[Value], t: &ClassTables) -> Result<Value, String> {
    Ok(reflect_class_list(args, &t.interfaces))
}

/// `Reflect.parents(x) -> List<string>` — the (sorted, transitive) ancestor class names of `x`'s
/// class, or `[]` for a non-class value.
fn reflect_parents(args: &[Value], t: &ClassTables) -> Result<Value, String> {
    Ok(reflect_class_list(args, &t.parents))
}

/// `Reflect.methods(x) -> List<string>` — the (sorted) method names of `x`'s class, including
/// inherited ones (the constructor and property hooks are excluded), or `[]` for a non-class value.
fn reflect_methods(args: &[Value], t: &ClassTables) -> Result<Value, String> {
    Ok(reflect_class_list(args, &t.methods))
}

/// `Reflect.fields(x) -> List<string>` — the (sorted) declared instance-field names of `x`'s class,
/// including inherited and constructor-promoted ones (static/const members and virtual hooks
/// excluded), or `[]` for a non-class value.
fn reflect_fields(args: &[Value], t: &ClassTables) -> Result<Value, String> {
    Ok(reflect_class_list(args, &t.fields))
}

/// `Reflect.kind(x) -> string` — the coarse, erasure-stable type tag. Mirrors the `__phorj_kind`
/// PHP helper exactly (which checks `is_callable` before `is_object`, since a PHP closure is both).
fn reflect_kind(args: &[Value], _: &mut String) -> Result<Value, String> {
    let kind = match args {
        [v] => match v {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            // A `decimal` erases to a PHP `string` (BCMath carrier), so the coarse PHP `kind` is
            // "string" — the Rust arm agrees for byte-identity (M-NUM S1).
            Value::Decimal { .. } => "string",
            Value::Bool(_) => "bool",
            // A real UTF-8 string and erased `bytes` are both a PHP `string` — coarse kind agrees.
            Value::Str(_) | Value::Bytes(_) => "string",
            // List/Map/Set all erase to a PHP `array`.
            Value::List(_) | Value::Map(_) | Value::Set(_) => "array",
            // A closure is `is_callable` in PHP; instances and enum variants are plain objects.
            Value::Closure(_) => "callable",
            Value::Instance(_) | Value::Enum(_) => "object",
            // Green-thread handles (M6 W4) are not transpilable — they never reach the PHP `kind`
            // helper. Mapped to "object" defensively so this arm stays total (the checker excludes
            // a channel/task from arithmetic/display; reflection over one is an edge, not a goal).
            Value::Channel(..) | Value::Task(_) => "object",
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

/// `Reflect.typeName(x) -> string` — the PRECISE Phorj type name. **Resolved at compile time by
/// `x`'s static type** (a checker pass, `checker::reflect`), so all three backends emit the *same*
/// answer and PHP's erasure is never consulted: a value type → a baked string literal
/// (`"int"`/`"List"`/`"Map"`/`"bytes"`/enum name/…), an object → the runtime `className`, an optional
/// → a null-branch, an erased generic → the coarse `kind`. By the time any backend runs, the call has
/// been rewritten away — so this `eval`/`php` is **dead** (defensive only). `eval` mirrors the static
/// rule from the runtime `Value` (keeps run/runvm correct under a hypothetical un-erased leak); `php`
/// can only be coarse (it falls back to `kind`), so a leak would diverge and the differential gate
/// would catch it. `typeName` is also excluded from UFCS (`checker::calls`) for the same reason.
fn reflect_type_name(args: &[Value], _: &mut String) -> Result<Value, String> {
    let name = match args {
        [v] => match v {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Decimal { .. } => "decimal",
            Value::Bool(_) => "bool",
            Value::Str(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::List(_) => "List",
            Value::Map(_) => "Map",
            Value::Set(_) => "Set",
            Value::Instance(i) => &i.class,
            // The static type is the *enum*, so the precise name is the enum's name (not the variant).
            Value::Enum(e) => &e.ty,
            Value::Closure(_) => "function",
            // Green-thread handles (M6 W4) — precise names; defensive (a channel/task is excluded
            // from `typeName`'s UFCS and never transpiles, so this arm is effectively dead).
            Value::Channel(..) => "Channel",
            Value::Task(_) => "Task",
            Value::Null | Value::Unit => "null",
        },
        _ => return Err("Reflect.typeName expects (T)".into()),
    };
    Ok(Value::Str(name.to_string()))
}

pub(crate) fn reflect_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Reflection",
            name: "kind",
            // Generic over any single argument (S7b registry-`Ty::Param` discipline — never erased to a
            // backend; the compiler types the call by expression shape, the transpiler via `php`).
            params: vec![Ty::Param("T".into())],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(reflect_kind),
            // `emit_member_call` sets `uses_reflect_kind` before calling this (the gated-helper pattern);
            // the helper is defined once in `emit_runtime_helpers`. `looks_like_global_call` adds the
            // leading `\` in namespaced mode.
            php: |a| format!("__phorj_kind({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Reflection",
            name: "className",
            params: vec![Ty::Param("T".into())],
            ret: Ty::Optional(Box::new(Ty::String)),
            pure: true,
            eval: NativeEval::Pure(reflect_class_name),
            // Gated `__phorj_class_name` helper (set in `emit_member_call`): single-evaluates its
            // argument (an inline `is_object($x) ? get_class($x) : null` would double-evaluate a
            // side-effecting argument) and excludes closures, matching the Rust arm.
            php: |a| format!("__phorj_class_name({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Reflection",
            name: "typeName",
            params: vec![Ty::Param("T".into())],
            ret: Ty::String,
            // Always erased before any backend by the `checker::reflect` static-type pass; this
            // eval/php is dead/defensive (see `reflect_type_name`). `php` can only be coarse.
            pure: true,
            eval: NativeEval::Pure(reflect_type_name),
            php: |a| format!("__phorj_kind({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Reflection",
            name: "interfaces",
            params: vec![Ty::Param("T".into())],
            ret: Ty::List(Box::new(Ty::String)),
            pure: true,
            // Needs the static class hierarchy (Reflective). `emit_member_call` sets
            // `uses_reflect_tables`; the `__phorj_reflect_of` helper + table are emitted once.
            eval: NativeEval::Reflective(reflect_interfaces),
            php: |a| format!("__phorj_reflect_of({}, \"interfaces\")", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Reflection",
            name: "parents",
            params: vec![Ty::Param("T".into())],
            ret: Ty::List(Box::new(Ty::String)),
            pure: true,
            eval: NativeEval::Reflective(reflect_parents),
            php: |a| format!("__phorj_reflect_of({}, \"parents\")", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Reflection",
            name: "methods",
            params: vec![Ty::Param("T".into())],
            ret: Ty::List(Box::new(Ty::String)),
            pure: true,
            eval: NativeEval::Reflective(reflect_methods),
            php: |a| format!("__phorj_reflect_of({}, \"methods\")", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Reflection",
            name: "fields",
            params: vec![Ty::Param("T".into())],
            ret: Ty::List(Box::new(Ty::String)),
            pure: true,
            eval: NativeEval::Reflective(reflect_fields),
            php: |a| format!("__phorj_reflect_of({}, \"fields\")", parg(a, 0)),
        },
    ]
}

#[cfg(test)]
#[path = "reflect_tests.rs"]
mod tests;
