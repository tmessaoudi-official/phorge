//! `Core.Csv` — single-row CSV parse/format (native-stdlib wave, Tier A).
//!
//! Pure, deterministic, std-only. Two natives over one logical CSV *record* (row):
//!   * `parse(string) -> List<string>` — split a row into fields, mirroring PHP
//!     `str_getcsv($s, ",", "\"", "")` (escape **disabled** — the forward-compatible RFC-4180 mode,
//!     no PHP proprietary backslash-escape, no 8.4+ deprecation noise);
//!   * `format(List<string>) -> string` — join fields into a row, quoting a field iff it contains
//!     the separator, the enclosure, or a line break, doubling any internal quote.
//!
//! The parser replicates `str_getcsv`'s exact quirks (verified against `php -n` 8.5), with **one**
//! documented deviation: an empty input yields `[""]` (one empty field) rather than PHP's `[null]`,
//! so the return type stays a clean `List<string>` — the PHP emission special-cases `""` to match.
//! Multi-row parsing (a `List<List<string>>`) is deferred (embedded newlines make line-splitting its
//! own problem). Every quoting edge is pinned to real `php -n` output in the unit tests.

use super::*;
use crate::types::Ty;
use crate::value::Value;

const SEP: u8 = b',';
const QUOTE: u8 = b'"';

/// Parse a single CSV row, mirroring PHP `str_getcsv($s, ",", "\"", "")`. Only ASCII control bytes
/// (`,` and `"`) are ever inspected or cut on, so every field slice falls on a UTF-8 boundary.
fn parse_row(s: &str) -> Vec<String> {
    // Deviation from str_getcsv (which returns `[null]`): an empty row is one empty field.
    if s.is_empty() {
        return vec![String::new()];
    }
    let b = s.as_bytes();
    let n = b.len();
    let mut fields: Vec<String> = Vec::new();
    let mut i = 0usize;
    loop {
        let mut field: Vec<u8> = Vec::new();
        // Peek past leading spaces/tabs: an enclosure there opens a *quoted* field (the whitespace is
        // discarded); anything else means the field is unquoted (and that whitespace is part of it).
        let mut j = i;
        while j < n && (b[j] == b' ' || b[j] == b'\t') {
            j += 1;
        }
        let next_i = if j < n && b[j] == QUOTE {
            let mut k = j + 1;
            loop {
                if k >= n {
                    break; // unterminated quote — content runs to end of input
                }
                if b[k] == QUOTE {
                    if k + 1 < n && b[k + 1] == QUOTE {
                        field.push(QUOTE); // doubled quote → one literal quote
                        k += 2;
                    } else {
                        k += 1; // closing quote
                        break;
                    }
                } else {
                    field.push(b[k]);
                    k += 1;
                }
            }
            // After the closing quote, any trailing bytes up to the separator are appended literally
            // (str_getcsv quirk: `"a" b` → `a b`).
            while k < n && b[k] != SEP {
                field.push(b[k]);
                k += 1;
            }
            k
        } else {
            // Unquoted field: read from `i` (leading whitespace kept) to the separator or end.
            let mut k = i;
            while k < n && b[k] != SEP {
                field.push(b[k]);
                k += 1;
            }
            k
        };
        // The control bytes guarantee a valid-UTF-8 field; build losslessly.
        fields.push(String::from_utf8(field).unwrap_or_default());
        if next_i < n {
            i = next_i + 1; // consume the separator and parse one more field
        } else {
            break;
        }
    }
    fields
}

/// Format one field for RFC-4180 output: quote (and double internal quotes) iff it contains the
/// separator, the enclosure, or a line break; otherwise emit it verbatim.
fn format_field(out: &mut String, f: &str) {
    let needs_quote = f
        .bytes()
        .any(|c| c == SEP || c == QUOTE || c == b'\n' || c == b'\r');
    if needs_quote {
        out.push('"');
        out.push_str(&f.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(f);
    }
}

fn csv_parse(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => {
            let fields = parse_row(s).into_iter().map(Value::Str).collect();
            Ok(Value::List(std::rc::Rc::new(fields)))
        }
        _ => Err("Csv.parse expects (string)".into()),
    }
}

fn csv_format(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::List(items)] => {
            let mut out = String::new();
            for (idx, it) in items.iter().enumerate() {
                let Value::Str(f) = it else {
                    return Err(format!(
                        "Csv.format expects List<string>, found element of type {}",
                        it.type_name()
                    ));
                };
                if idx > 0 {
                    out.push(',');
                }
                format_field(&mut out, f);
            }
            Ok(Value::Str(out))
        }
        _ => Err("Csv.format expects (List<string>)".into()),
    }
}

/// The `Core.Csv` registry entries. The PHP emission leans on `str_getcsv` (escape disabled) for
/// `parse` and a hand-written `array_map`/`implode` for `format` — both pinned byte-identical to the
/// Rust kernels in the unit tests.
pub(crate) fn csv_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Csv",
            name: "parse",
            params: vec![Ty::String],
            ret: Ty::List(Box::new(Ty::String)),
            pure: true,
            eval: NativeEval::Pure(csv_parse),
            // Empty input → `[""]` to match the Rust kernel (str_getcsv would give `[null]`); the
            // scratch `$__csv` avoids double-evaluating the argument expression.
            php: |a| {
                format!(
                    r#"(($__csv = {}) === "" ? [""] : str_getcsv($__csv, ",", "\"", ""))"#,
                    parg(a, 0)
                )
            },
        },
        NativeFn {
            module: "Core.Csv",
            name: "format",
            params: vec![Ty::List(Box::new(Ty::String))],
            ret: Ty::String,
            pure: true,
            eval: NativeEval::Pure(csv_format),
            php: |a| {
                format!(
                    r#"implode(",", array_map(fn($f) => (strpbrk($f, ",\"\n\r") === false) ? $f : '"' . str_replace('"', '""', $f) . '"', {}))"#,
                    parg(a, 0)
                )
            },
        },
    ]
}

#[cfg(test)]
#[path = "csv_tests.rs"]
mod tests;
