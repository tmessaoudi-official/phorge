//! `Core.Path` — pure filesystem-*path* string manipulation (M4 stdlib breadth, Tier 1).
//!
//! These natives operate on a path **as a string** — they never touch the filesystem, so they are
//! pure, deterministic, and byte-identical across `run`/`runvm`/real PHP. Every function maps to a
//! PHP core builtin (`basename`/`dirname`/`pathinfo`) available under `php -n`, so the PHP oracle
//! verifies the Rust kernels directly. Separator is `/` only (the Linux oracle's
//! `DIRECTORY_SEPARATOR`); the algorithms below were derived from PHP 8.5 ground truth.
//!
//! Filesystem *access* (read/write/exists) lives in `Core.File`; this module is the path-arithmetic
//! companion (the `basename`/`dirname`/`pathinfo` family), kept separate so it stays Tier 1.

use super::*;
use crate::types::Ty;
use crate::value::Value;

/// PHP `basename($p)`: strip trailing `/`, then the component after the last `/`. All-slash (or
/// empty) input → `""`. Examples: `/a/b.txt`→`b.txt`, `/a/b/`→`b`, `/a//b//`→`b`, `/`→`""`.
fn php_basename(p: &str) -> String {
    let t = p.trim_end_matches('/');
    if t.is_empty() {
        return String::new();
    }
    match t.rfind('/') {
        Some(i) => t[i + 1..].to_string(),
        None => t.to_string(),
    }
}

/// PHP `dirname($p)`: the directory part. Empty → `""`; all-slash → `/`; no slash → `.`; otherwise
/// the prefix before the last slash with its own trailing slashes stripped (empty prefix → `/`).
/// Examples: `/a/b.txt`→`/a`, `/a`→`/`, `a/b`→`a`, `a`→`.`, `./a`→`.`, `/a//b//`→`/a`.
fn php_dirname(p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    let t = p.trim_end_matches('/');
    if t.is_empty() {
        return "/".to_string(); // input was all slashes
    }
    match t.rfind('/') {
        None => ".".to_string(),
        Some(i) => {
            let pre = t[..i].trim_end_matches('/');
            if pre.is_empty() {
                "/".to_string() // last slash was the leading root
            } else {
                pre.to_string()
            }
        }
    }
}

/// PHP `pathinfo($p, PATHINFO_EXTENSION)`: the chars after the last `.` **in the basename**, or `""`
/// when the basename has no dot. A leading-dot basename (`.hidden`) is all-extension (`hidden`); a
/// trailing dot (`a.`) yields `""`. Examples: `a.b.c`→`c`, `/a/b.txt`→`txt`, `a`→`""`.
fn php_extension(p: &str) -> String {
    let b = php_basename(p);
    match b.rfind('.') {
        Some(j) => b[j + 1..].to_string(),
        None => String::new(),
    }
}

/// PHP `pathinfo($p, PATHINFO_FILENAME)`: the basename with its extension removed (everything before
/// the last `.`). No dot → the whole basename; leading-dot basename (`.hidden`) → `""`. Examples:
/// `/a/b.txt`→`b`, `a.b.c`→`a.b`, `noext`→`noext`.
fn php_stem(p: &str) -> String {
    let b = php_basename(p);
    match b.rfind('.') {
        Some(j) => b[..j].to_string(),
        None => b,
    }
}

/// Join two path segments with a single `/`: the left's trailing slashes and the right's leading
/// slashes are collapsed to one separator (PHP has no builtin; emitted as `rtrim/ltrim` so it is
/// single-eval and byte-identical). Examples: `(a, b)`→`a/b`, `(a/, /b)`→`a/b`, `("", b)`→`/b`.
fn path_join_str(a: &str, b: &str) -> String {
    format!("{}/{}", a.trim_end_matches('/'), b.trim_start_matches('/'))
}

fn path_basename(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(p)] => Ok(Value::Str(php_basename(p))),
        _ => Err("Path.basename expects (string)".into()),
    }
}
fn path_dirname(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(p)] => Ok(Value::Str(php_dirname(p))),
        _ => Err("Path.dirname expects (string)".into()),
    }
}
fn path_extension(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(p)] => Ok(Value::Str(php_extension(p))),
        _ => Err("Path.extension expects (string)".into()),
    }
}
fn path_stem(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(p)] => Ok(Value::Str(php_stem(p))),
        _ => Err("Path.stem expects (string)".into()),
    }
}
fn path_join(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(a), Value::Str(b)] => Ok(Value::Str(path_join_str(a, b))),
        _ => Err("Path.join expects (string, string)".into()),
    }
}

/// The `Core.Path` registry entries (subject-first; all Tier 1, pure).
pub(crate) fn path_natives() -> Vec<NativeFn> {
    let s = || Ty::String;
    vec![
        NativeFn {
            module: "Core.Path",
            name: "basename",
            params: vec![s()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(path_basename),
            php: |a| format!("basename({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Path",
            name: "dirname",
            params: vec![s()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(path_dirname),
            php: |a| format!("dirname({})", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Path",
            name: "extension",
            params: vec![s()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(path_extension),
            php: |a| format!("pathinfo({}, PATHINFO_EXTENSION)", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Path",
            name: "stem",
            params: vec![s()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(path_stem),
            php: |a| format!("pathinfo({}, PATHINFO_FILENAME)", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Path",
            name: "join",
            params: vec![s(), s()],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(path_join),
            php: |a| {
                format!(
                    "rtrim({}, '/') . '/' . ltrim({}, '/')",
                    parg(a, 0),
                    parg(a, 1)
                )
            },
        },
    ]
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;
