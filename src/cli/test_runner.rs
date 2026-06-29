//! `phg test` — discover and run `test "name" { … }` blocks (M-Test T3).
//!
//! Discovery: with no path, every `*.phg` under the project's `tests/` directory (the project root
//! is the nearest ancestor holding a `phorj.toml`, else the current directory); with a path, that
//! file, or every `*.phg` under that directory.
//!
//! Each file is loaded through the normal loader (so a test gets packages, imports, cross-package
//! types — a real program) and validated once in **test mode** (`check_tests`, which allows `test`
//! items). Then every `test` block is run **independently on the interpreter**: the block's body is
//! lowered into a synthetic `main` and routed through the ordinary `check_and_expand` → `interpret`
//! pipeline, so every front-end pass (alias/generic/`html`/UFCS/`new` rewrites) processes the body
//! exactly as it would a normal function — no test-specific backend path. A failing assertion (or
//! any other fault) surfaces as a runtime error with its stack trace; the runner records it and
//! moves on. Exit code is `0` iff every test passed, else `1`.

use std::path::{Path, PathBuf};

use crate::ast::{FunctionDecl, Item, Program, Stmt, Type, Visibility};
use crate::interpreter::interpret;
use crate::loader::{self, Unit};
use crate::token::Span;

use super::check_and_expand;

/// One test's result: its file, its name, and `None` on pass or the rendered error on failure. A
/// file that fails to load/check contributes a single outcome with a sentinel name.
struct Outcome {
    file: String,
    name: String,
    error: Option<String>,
}

/// Run `phg test` over `paths` (empty ⇒ the project's `tests/` directory). Returns the report text
/// and the process exit code (`0` all-pass, `1` any failure or a discovery error).
pub fn cmd_test(paths: &[String]) -> (String, i64) {
    let files = match discover(paths) {
        Ok(f) => f,
        Err(e) => return (format!("test discovery failed: {e}\n"), 1),
    };
    if files.is_empty() {
        return (
            "no test files found (looked for `*.phg` under `tests/`)\n".to_string(),
            0,
        );
    }

    let mut out = String::new();
    let mut outcomes: Vec<Outcome> = Vec::new();
    for file in &files {
        run_file(file, &mut out, &mut outcomes);
    }

    let failed = outcomes.iter().filter(|o| o.error.is_some()).count();
    let passed = outcomes.len() - failed;
    if failed > 0 {
        out.push_str("\nfailures:\n");
        for o in outcomes.iter().filter(|o| o.error.is_some()) {
            let msg = o.error.as_deref().unwrap_or("");
            out.push_str(&format!("\n  {} :: {}\n", o.file, o.name));
            for line in msg.lines() {
                out.push_str(&format!("    {line}\n"));
            }
        }
    }
    out.push_str(&format!(
        "\n{passed} passed, {failed} failed, {} tests in {} files\n",
        outcomes.len(),
        files.len()
    ));
    (out, i64::from(failed > 0))
}

/// Load, validate (test mode), and run every `test` block in one file, appending a per-test result
/// line to `out` and an [`Outcome`] per test (or one sentinel outcome if the file itself is bad).
fn run_file(file: &Path, out: &mut String, outcomes: &mut Vec<Outcome>) {
    let fname = file.display().to_string();
    let unit = match loader::load(file) {
        Ok(u) => u,
        Err(e) => {
            out.push_str(&format!("{fname} :: <load> ... FAILED\n"));
            outcomes.push(Outcome {
                file: fname,
                name: "<load>".into(),
                error: Some(e),
            });
            return;
        }
    };
    // Validate the whole file once, in test mode (so `test` items are allowed).
    if let Err(errs) = crate::checker::check_tests(&unit.program) {
        let rendered = errs
            .iter()
            .map(|e| e.render(&unit.diag_src))
            .collect::<Vec<_>>()
            .join("\n");
        out.push_str(&format!("{fname} :: <check> ... FAILED\n"));
        outcomes.push(Outcome {
            file: fname,
            name: "<check>".into(),
            error: Some(rendered),
        });
        return;
    }

    let tests: Vec<(String, Vec<Stmt>, Span)> = unit
        .program
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Test { name, body, span } => Some((name.clone(), body.clone(), *span)),
            _ => None,
        })
        .collect();

    for (name, body, span) in tests {
        let error = run_one(&unit, &body, span);
        let status = if error.is_none() { "ok" } else { "FAILED" };
        out.push_str(&format!("{fname} :: {name} ... {status}\n"));
        outcomes.push(Outcome {
            file: fname.clone(),
            name,
            error,
        });
    }
}

/// Run a single `test` body: lower it into a synthetic `main`, route it through the ordinary
/// check/expand/interpret pipeline, and return `None` on success or the rendered fault on failure.
fn run_one(unit: &Unit, body: &[Stmt], span: Span) -> Option<String> {
    let synth = synthesize_main(&unit.program, body, span);
    match check_and_expand(&synth, &unit.diag_src) {
        Ok(expanded) => match interpret(&expanded) {
            Ok(_stdout) => None,
            Err(mut e) => {
                let src = unit.attribute_frames(&mut e);
                Some(e.render(&src))
            }
        },
        // The file already type-checked in test mode; a failure here is unexpected, but surface it.
        Err(err) => Some(err),
    }
}

/// Build a runnable program from `program` with the test body as `main`: keep every item except the
/// `test` blocks and any existing top-level `main` (the synthetic one is the entry), then append a
/// `function main(): void { body }`. The result has no `test` items, so the normal (non-test-mode)
/// pipeline processes it like any program.
fn synthesize_main(program: &Program, body: &[Stmt], span: Span) -> Program {
    let mut items: Vec<Item> = program
        .items
        .iter()
        .filter(|i| {
            !matches!(i, Item::Test { .. }) && !matches!(i, Item::Function(f) if f.name == "main")
        })
        .cloned()
        .collect();
    items.push(Item::Function(FunctionDecl {
        modifiers: Vec::new(),
        attrs: Vec::new(),
        vis: Visibility::Public,
        name: "main".into(),
        type_params: Vec::new(),
        params: Vec::new(),
        ret: Some(Type::Named {
            name: "void".into(),
            args: Vec::new(),
            span,
        }),
        throws: Vec::new(),
        body: body.to_vec(),
        foreign: false,
        generic_ret_from_param: None,
        span,
    }));
    Program {
        package: program.package.clone(),
        items,
        span: program.span,
    }
}

/// Resolve the test files to run. Empty `paths` ⇒ `<project-root>/tests/` (project root = nearest
/// ancestor with a `phorj.toml`, else the current directory). A path argument is taken literally:
/// a file runs as-is, a directory is searched recursively for `*.phg`.
fn discover(paths: &[String]) -> Result<Vec<PathBuf>, String> {
    if paths.is_empty() {
        let root = project_root();
        let tests = root.join("tests");
        return loader::discover_phg(&tests);
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

/// The nearest ancestor of the current directory that contains a `phorj.toml`, else the current
/// directory itself.
fn project_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = cwd.as_path();
    loop {
        if dir.join("phorj.toml").is_file() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => return cwd,
        }
    }
}
