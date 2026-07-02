//! `Core.Ini` — a small, legible INI config parser: `key = value` lines, `[section]` headers, and
//! `;` / `#` comment lines, into an ordered `Map<string, string>`.
//!
//! Deliberately **not** PHP's `parse_ini_string`: no type coercion (`on`/`yes`/`true`/`1` all stay
//! strings), no quoted-value or backtick magic, no reserved words — Phorj removes PHP's surprises
//! rather than inheriting them. A `[section]` header dots the keys under it (`[db]` + `host = x` →
//! `db.host`). Blank lines and comment lines are skipped; keys and values are trimmed; a value may
//! contain `=` (only the first `=` splits); a later duplicate key overwrites (first position kept,
//! PHP-map semantics via `build_map`); a line that is neither a comment, a `[section]`, nor a
//! `key=value` is skipped.
//!
//! Byte-identical run/runvm/transpiled-PHP: the transpiler emits a matching hand-rolled
//! `__phorj_ini_parse` (never `parse_ini_string`); per-line trim uses PHP `trim()`'s exact default
//! set on both legs.

use super::*;
use crate::types::Ty;
use crate::value::{build_map, Value};
use std::rc::Rc;

fn ini_parse(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => {
            let mut pairs: Vec<(Value, Value)> = Vec::new();
            let mut section = String::new();
            for line in s.split('\n') {
                // Match PHP `trim()`'s default set exactly (space, \t, \r, \v, \0 — \n already split).
                let t = line.trim_matches([' ', '\t', '\r', '\u{0b}', '\0']);
                if t.is_empty() || t.starts_with(';') || t.starts_with('#') {
                    continue;
                }
                if t.starts_with('[') && t.ends_with(']') {
                    section = t[1..t.len() - 1]
                        .trim_matches([' ', '\t', '\r', '\u{0b}', '\0'])
                        .to_string();
                    continue;
                }
                if let Some(eq) = t.find('=') {
                    let key = t[..eq].trim_matches([' ', '\t', '\r', '\u{0b}', '\0']);
                    let val = t[eq + 1..].trim_matches([' ', '\t', '\r', '\u{0b}', '\0']);
                    let full = if section.is_empty() {
                        key.to_string()
                    } else {
                        format!("{section}.{key}")
                    };
                    pairs.push((Value::Str(full), Value::Str(val.to_string())));
                }
                // else: not a comment / section / key=value → skipped
            }
            Ok(Value::Map(Rc::new(build_map(pairs)?)))
        }
        _ => Err("Ini.parse expects (string)".into()),
    }
}

pub(crate) fn ini_natives() -> Vec<NativeFn> {
    vec![NativeFn {
        module: "Core.Ini",
        name: "parse",
        params: vec![Ty::String],
        ret: Ty::Map(Box::new(Ty::String), Box::new(Ty::String)),
        pure: true,
        eval: NativeEval::Pure(ini_parse),
        php: |a| format!("__phorj_ini_parse({})", parg(a, 0)),
    }]
}

#[cfg(test)]
#[path = "ini_tests.rs"]
mod tests;
