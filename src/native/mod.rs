//! Namespaced native (built-in) function registry — the stdlib's runtime + type + transpile
//! surface, addressed by `(module, name)` (e.g. module `Core.Console`, name `println`). One entry
//! single-sources all four facets of a native, so the four backends cannot drift:
//!   * `params` / `ret` — the checker's signature for a call to this native;
//!   * `eval` — the runtime behavior, shared by the tree-walking interpreter *and* the VM (the
//!     structural parity guarantee, exactly like the value kernels: one impl, two callers);
//!   * `php` — the transpile-time PHP emission (a `core.*` native erases to PHP's flat builtins;
//!     the namespace is a compile-time organizing layer, decisions N-2/D-L9).
//!
//! The registry is the load-bearing target of `import Core.Console;` (M3 namespace reshape, Wave 1,
//! `docs/specs/2026-06-18-m3-namespace-system-design.md`). The former free global `println` is
//! retired in favor of `Core.Console.println`, and `Op::Print` in favor of
//! `Op::CallNative(index, argc)` indexing this table.

use crate::ast::Item;
use crate::types::Ty;
use crate::value::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

// Per-leaf stdlib modules: each owns its `*_natives()` builder + bodies; `build()` below is the sole
// ordering coordinator (the pinned-slot invariant). `Core.Console` stays here (slot 0, inlined).
mod bytes;
mod file;
mod html;
mod list;
mod map;
mod math;
mod process;
mod reflect;
mod set;
mod text;

pub use process::set_process_args;

/// One built-in function, addressed by `(module, name)`. See the module docs for the four facets.
pub struct NativeFn {
    /// Dotted module path the native lives under — e.g. `"Core.Console"`.
    pub module: &'static str,
    /// Bare function name — e.g. `"println"`.
    pub name: &'static str,
    /// Parameter types — the checker validates call arguments against these.
    pub params: Vec<Ty>,
    /// Return type.
    pub ret: Ty,
    /// Runtime behavior, shared by the interpreter and the VM (the structural parity guarantee —
    /// one body, two callers). See [`NativeEval`].
    pub eval: NativeEval,
    /// PHP emission: given the already-emitted PHP for each argument, return the PHP snippet this
    /// native erases to (decision N-2). For `Console.println`: `echo {a} . "\n"`.
    pub php: fn(&[String]) -> String,
    /// Whether this native is **deterministic** w.r.t. the program text (`true` for all but the
    /// ambient-environment natives — `Core.Process`/`Core.Env`, whose result depends on the process,
    /// not the source). A program that calls an impure native is *quarantined* from the byte-identity
    /// differential (the PHP leg runs in a separate process whose argv/env need not match) and tested
    /// separately under a controlled environment — see `tests/process.rs`. Declared per-native here
    /// (not hardcoded in the harness) so the differential stays generic: it reads this flag via
    /// `program_uses_impure_native` (`docs/specs/2026-06-25-process-io-quarantine-seam-design.md`, Q1).
    pub pure: bool,
}

/// A backend's re-entrant closure invoker, handed to a [`NativeEval::HigherOrder`] body: given a
/// `Value::Closure` and its call arguments, run it on the calling backend and return its result (or
/// a fault as a plain `String`, the backend-shared contract). The interpreter wraps `call_closure`;
/// the VM wraps `call_closure_value` (a nested `run_until` over the shared `exec_op`).
pub type ClosureInvoker<'a> = dyn FnMut(&Value, Vec<Value>) -> Result<Value, String> + 'a;

/// How a native computes its result (M-RT S7b-3). Most natives are [`Pure`](NativeEval::Pure): a
/// function of their argument values, threading the program output buffer so a side-effecting native
/// (`Console.println`) can append to it. A [`HigherOrder`](NativeEval::HigherOrder) native instead
/// needs to *call back* into the calling backend to invoke a `Value::Closure` argument
/// (`Core.List.map`/`filter`/`reduce`); the backend supplies the invoker so the one `eval` body
/// drives both the interpreter and the VM — exactly the parity discipline of the pure path. Both
/// variants are `fn` pointers, so the enum stays `Copy` (a `CallNative` dispatch reads it by value,
/// ending the registry borrow before the invoker captures the backend).
#[derive(Clone, Copy)]
pub enum NativeEval {
    /// `(args, out) -> result`. Arguments arrive in source order; `out` is the program's output
    /// buffer (ignored by pure natives, appended to by side-effecting ones).
    Pure(fn(&[Value], &mut String) -> Result<Value, String>),
    /// `(args, invoke) -> result`, where `invoke(closure, call_args)` executes a `Value::Closure`
    /// on the calling backend and returns its value. The native never touches the output buffer
    /// directly — any side effect happens inside the invoked closure.
    HigherOrder(fn(&[Value], &mut ClosureInvoker) -> Result<Value, String>),
}

/// Pinned registry slot for `Core.Console.println` — the migrated former `Op::Print`. The compiler
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

/// `Console.print` — like `println` but with no trailing newline (primitives P3). Space-joins multiple
/// args, same as `println`; transpiles to a bare PHP `echo`.
fn console_print(args: &[Value], out: &mut String) -> Result<Value, String> {
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        match a.as_display() {
            Some(t) => out.push_str(&t),
            None => return Err(format!("print cannot print {}", a.type_name())),
        }
    }
    Ok(Value::Unit)
}

/// Index helper for a native's PHP emission: the already-emitted PHP for argument `i`, or `""` if
/// absent (the checker guarantees arity before `php` is ever called). Keeps the `php` closures terse.
fn parg(args: &[String], i: usize) -> &str {
    args.get(i).map_or("", String::as_str)
}

/// Construct the native table once. Order is load-bearing: [`CONSOLE_PRINTLN`] pins slot 0; every
/// other native is resolved by `(module, name)` (or leaf+name) at compile time, so appended order is
/// free. Modules are grouped by `*_natives()` builders (one per `core.*` leaf).
fn build() -> Vec<NativeFn> {
    let mut registry = vec![NativeFn {
        module: "Core.Console",
        name: "println",
        params: vec![Ty::String],
        ret: Ty::Void,
        pure: true,
        eval: NativeEval::Pure(console_println),
        php: |args| {
            let a = args
                .first()
                .map_or_else(|| "\"\"".to_string(), String::clone);
            format!(r#"echo {a} . "\n""#)
        },
    }];
    // `Console.print` — no trailing newline (primitives P3). Not slot-pinned; resolved by (module,name).
    registry.push(NativeFn {
        module: "Core.Console",
        name: "print",
        params: vec![Ty::String],
        ret: Ty::Void,
        pure: true,
        eval: NativeEval::Pure(console_print),
        php: |args| {
            let a = args
                .first()
                .map_or_else(|| "\"\"".to_string(), String::clone);
            format!("echo {a}")
        },
    });
    registry.extend(math::math_natives());
    registry.extend(text::text_natives());
    registry.extend(file::file_natives());
    registry.extend(bytes::bytes_natives());
    registry.extend(html::html_natives());
    registry.extend(list::list_natives());
    registry.extend(map::map_natives());
    registry.extend(set::set_natives());
    registry.extend(reflect::reflect_natives());
    registry.extend(process::process_natives());
    // Pinned-slot invariant: the constant the compiler bakes into `Op::CallNative` must address the
    // entry it names. Cheap one-time check at first `registry()` access.
    assert_eq!(
        registry[CONSOLE_PRINTLN].module, "Core.Console",
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
/// `import Core.Console;` binds the call-site qualifier `console` to module `Core.Console`. Carried
/// by the checker (import-required + shadowing enforcement) and the transpiler (which has no
/// variable-scope tracking to tell a qualifier from a value).
pub fn import_map(items: &[Item]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for item in items {
        if let Item::Import {
            path,
            alias,
            type_only: false,
            ..
        } = item
        {
            // The bound qualifier is the alias when present (`import a.b as c;` ⇒ `c`), else the
            // path's last segment (M5 S2c). A terminal `import type …;` binds a *type* name, not a
            // call qualifier, so it is excluded from this (call-site) map.
            let qualifier = alias.clone().or_else(|| path.last().cloned());
            if let Some(q) = qualifier {
                map.insert(q, path.join("."));
            }
        }
    }
    map
}

#[cfg(test)]
mod tests;
