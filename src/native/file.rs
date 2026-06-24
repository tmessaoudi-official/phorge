use super::*;
use crate::types::Ty;
use crate::value::Value;

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

/// The `Core.File` registry entries (M3 Track B Wave 2).
pub(crate) fn file_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.File",
            name: "read",
            params: vec![Ty::String],
            ret: Ty::Optional(Box::new(Ty::String)),
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
            eval: NativeEval::Pure(file_exists),
            php: |a| format!("file_exists({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.File",
            name: "write",
            params: vec![Ty::String, Ty::String],
            ret: Ty::Void,
            eval: NativeEval::Pure(file_write),
            php: |a| format!("file_put_contents({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

// ---- Core.Bytes ---------------------------------------------------------------------------------
// Octet-sequence natives bridging `bytes` ↔ `string` (M6 W0). `to_string` returns `string?` — `null`
// on invalid UTF-8 (composes with S2 `??` / if-let), never a fault. `len` is the BYTE count
// (`strlen`), as is `Core.Text.len` — the std stays extension-free (no mbstring). `slice` is a total,
// bounds-clamped half-open `[start, end)` (no fault, unlike list `xs[i]`). PHP strings are byte
// arrays, so the erasures are exact.

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
