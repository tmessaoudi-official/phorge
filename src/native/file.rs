use super::*;
use crate::types::Ty;
use crate::value::Value;
use std::io::Write as _;

fn file_read(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Any read failure maps to `null` (the `string?` absent case), never a fault.
        [Value::Str(path)] => Ok(match std::fs::read_to_string(path) {
            Ok(s) => Value::Str(s),
            Err(_) => Value::Null,
        }),
        _ => Err("File.read expects (string)".into()),
    }
}
fn file_exists(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(path)] => Ok(Value::Bool(std::path::Path::new(path).exists())),
        _ => Err("File.exists expects (string)".into()),
    }
}
fn file_write(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(path), Value::Str(contents)] => match std::fs::write(path, contents) {
            Ok(()) => Ok(Value::Unit),
            Err(e) => Err(format!("File.write failed: {e}")),
        },
        _ => Err("File.write expects (string, string)".into()),
    }
}
fn file_append(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(path), Value::Str(contents)] => {
            let r = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| f.write_all(contents.as_bytes()));
            match r {
                Ok(()) => Ok(Value::Unit),
                Err(e) => Err(format!("File.append failed: {e}")),
            }
        }
        _ => Err("File.append expects (string, string)".into()),
    }
}
fn file_delete(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(path)] => match std::fs::remove_file(path) {
            Ok(()) => Ok(Value::Unit),
            Err(e) => Err(format!("File.delete failed: {e}")),
        },
        _ => Err("File.delete expects (string)".into()),
    }
}
fn file_rename(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(from), Value::Str(to)] => match std::fs::rename(from, to) {
            Ok(()) => Ok(Value::Unit),
            Err(e) => Err(format!("File.rename failed: {e}")),
        },
        _ => Err("File.rename expects (string, string)".into()),
    }
}
fn file_copy(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // Returns the number of bytes copied (PHP `copy` returns a bool; the byte count is the more
        // useful, still-deterministic-for-a-fixed-source value).
        [Value::Str(from), Value::Str(to)] => match std::fs::copy(from, to) {
            Ok(n) => Ok(Value::Int(i64::try_from(n).unwrap_or(i64::MAX))),
            Err(e) => Err(format!("File.copy failed: {e}")),
        },
        _ => Err("File.copy expects (string, string)".into()),
    }
}
fn file_size(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        // `int?` — `null` when the path is missing/unreadable (composes with `??` / if-let), never a
        // fault. A file larger than i64::MAX bytes clamps (a 9-exabyte file is not a realistic input).
        [Value::Str(path)] => Ok(match std::fs::metadata(path) {
            Ok(m) => Value::Int(i64::try_from(m.len()).unwrap_or(i64::MAX)),
            Err(_) => Value::Null,
        }),
        _ => Err("File.size expects (string)".into()),
    }
}

/// The `Core.File` registry entries (M3 Track B Wave 2; filesystem-mutation ops added 2026-07-01).
///
/// The mutation ops (`append`/`delete`/`rename`/`copy`) are `pure: false`: they mutate the filesystem
/// non-idempotently (append accumulates; delete/rename are stateful; copy creates), so a program
/// importing `Core.File` is now QUARANTINED from the byte-identity differential (the `Core.Process`
/// recipe — `uses_impure_native` derives the impure-module set from the `pure` flag). `read`/`exists`/
/// `write`/`size` stay `pure: true` (read-only or idempotent-overwrite), but they share the now-impure
/// module, so the whole surface is tested in `tests/filesystem.rs` under a controlled temp dir rather
/// than the auto-glob oracle.
pub(crate) fn file_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.File",
            name: "read",
            params: vec![Ty::String],
            ret: Ty::Optional(Box::new(Ty::String)),
            pure: true,
            eval: NativeEval::Pure(file_read),
            // `@` suppresses the missing-file warning; the assign-and-compare distinguishes a missing
            // file (`false` → null) from a legitimately empty one (`""`), which a bare `?:` would not.
            php: |a| {
                format!(
                    "(($__c = @file_get_contents({})) === false ? null : $__c)",
                    parg(a, 0)
                )
            },
        },
        NativeFn {
            module: "Core.File",
            name: "exists",
            params: vec![Ty::String],
            ret: Ty::Bool,
            pure: true,
            eval: NativeEval::Pure(file_exists),
            php: |a| format!("file_exists({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.File",
            name: "write",
            params: vec![Ty::String, Ty::String],
            ret: Ty::Void,
            pure: true,
            eval: NativeEval::Pure(file_write),
            php: |a| format!("file_put_contents({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.File",
            name: "append",
            params: vec![Ty::String, Ty::String],
            ret: Ty::Void,
            pure: false,
            eval: NativeEval::Pure(file_append),
            php: |a| {
                format!(
                    "file_put_contents({}, {}, FILE_APPEND)",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.File",
            name: "delete",
            params: vec![Ty::String],
            ret: Ty::Void,
            pure: false,
            eval: NativeEval::Pure(file_delete),
            php: |a| format!("@unlink({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.File",
            name: "rename",
            params: vec![Ty::String, Ty::String],
            ret: Ty::Void,
            pure: false,
            eval: NativeEval::Pure(file_rename),
            php: |a| format!("rename({}, {})", parg(a, 0), parg(a, 1)),
        },
        NativeFn {
            module: "Core.File",
            name: "copy",
            params: vec![Ty::String, Ty::String],
            ret: Ty::Int,
            pure: false,
            eval: NativeEval::Pure(file_copy),
            // PHP `copy` returns a bool; emit the byte count to match the Phorj `int` return.
            php: |a| {
                format!(
                    "(copy({from}, {to}) ? filesize({to}) : 0)",
                    from = parg(a, 0),
                    to = parg(a, 1)
                )
            },
        },
        NativeFn {
            module: "Core.File",
            name: "size",
            params: vec![Ty::String],
            ret: Ty::Optional(Box::new(Ty::Int)),
            // Read-only (like `read`/`exists`) → pure; the module is already impure via the mutation ops
            // below, so an example using `size` is quarantined regardless, but the flag stays honest.
            pure: true,
            eval: NativeEval::Pure(file_size),
            php: |a| {
                format!(
                    "(($__sz = @filesize({})) === false ? null : $__sz)",
                    parg(a, 0)
                )
            },
        },
    ]
}

// ---- Core.Bytes ---------------------------------------------------------------------------------
// Octet-sequence natives bridging `bytes` ↔ `string` (M6 W0). `to_string` returns `string?` — `null`
// on invalid UTF-8 (composes with S2 `??` / if-let), never a fault. `len` is the BYTE count
// (`strlen`), as is `Core.Text.length` — the std stays extension-free (no mbstring). `slice` is a total,
// bounds-clamped half-open `[start, end)` (no fault, unlike list `xs[i]`). PHP strings are byte
// arrays, so the erasures are exact.

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
