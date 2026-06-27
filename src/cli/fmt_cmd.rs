//! `phg fmt` — the formatter CLI (M-fmt F3). Mirrors `gofmt`/`cargo fmt`:
//!   * `phg fmt <file|dir>…` — format in place (write only when the content actually changes).
//!   * `phg fmt --check [paths…]` — report files that are not already formatted; exit 1 if any. No
//!     writes (the CI gate).
//!   * `phg fmt` (no path) — format every `*.phg` under the current directory, recursively.
//!   * `phg fmt -` (stdin) — handled by `main.rs` via [`fmt_source`].
//!
//! An unparseable file is **never** rewritten — its diagnostic is reported and the file left intact
//! (exit 2), so the formatter can never corrupt broken source.

use std::path::{Path, PathBuf};

use crate::fmt;
use crate::loader;

/// Format one source string (the stdin path). Returns the formatted text or a rendered diagnostic.
pub fn fmt_source(src: &str) -> Result<String, String> {
    fmt::format(src).map_err(|d| d.render(src))
}

/// Run `phg fmt` over `paths` (empty ⇒ recursively under the current directory). `check` is the
/// no-write CI mode. Returns the report and the exit code: `2` if any file failed to parse, else `1`
/// in `--check` mode when a file would change, else `0`.
pub fn cmd_fmt(paths: &[String], check: bool) -> (String, i64) {
    let files = match discover(paths) {
        Ok(f) => f,
        Err(e) => return (format!("fmt: {e}\n"), 2),
    };
    if files.is_empty() {
        return ("fmt: no `*.phg` files found\n".to_string(), 0);
    }

    let mut out = String::new();
    let mut changed = 0usize;
    let mut errors = 0usize;
    for file in &files {
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                out.push_str(&format!("fmt: cannot read {}: {e}\n", file.display()));
                errors += 1;
                continue;
            }
        };
        match fmt::format(&src) {
            Ok(formatted) => {
                if formatted == src {
                    continue; // already canonical
                }
                changed += 1;
                if check {
                    out.push_str(&format!("would reformat {}\n", file.display()));
                } else {
                    match std::fs::write(file, &formatted) {
                        Ok(()) => out.push_str(&format!("formatted {}\n", file.display())),
                        Err(e) => {
                            out.push_str(&format!("fmt: cannot write {}: {e}\n", file.display()));
                            errors += 1;
                        }
                    }
                }
            }
            Err(diag) => {
                // Parse/lex failure — leave the file untouched, report the diagnostic.
                errors += 1;
                out.push_str(&format!(
                    "fmt: {} did not parse (left unchanged):\n{}\n",
                    file.display(),
                    diag.render(&src)
                ));
            }
        }
    }

    let code = if errors > 0 {
        2
    } else if check && changed > 0 {
        1
    } else {
        0
    };
    if check {
        out.push_str(&format!(
            "\n{changed} file(s) would be reformatted, {errors} error(s)\n"
        ));
    } else {
        out.push_str(&format!(
            "\n{changed} file(s) formatted, {errors} error(s)\n"
        ));
    }
    (out, code)
}

/// Resolve the files to format. Empty ⇒ `*.phg` under the current directory, recursively. A path
/// argument is a file (taken as-is) or a directory (searched recursively).
fn discover(paths: &[String]) -> Result<Vec<PathBuf>, String> {
    if paths.is_empty() {
        return loader::discover_phg(Path::new("."));
    }
    let mut out = Vec::new();
    for p in paths {
        let path = Path::new(p);
        if path.is_dir() {
            out.extend(loader::discover_phg(path)?);
        } else if path.is_file() {
            out.push(path.to_path_buf());
        } else {
            return Err(format!("no such file or directory: {p}"));
        }
    }
    Ok(out)
}
