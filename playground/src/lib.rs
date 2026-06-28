//! WASM bindings exposing the Phorj pipeline to a browser playground.
//!
//! The browser cannot use the CLI runners (`cli::cmd_run` etc.): they wrap `on_deep_stack`, which
//! spawns a 256 MB `std::thread` worker — unavailable on `wasm32-unknown-unknown`. So the wrapper
//! functions here call the **public inner pipeline directly** on the calling stack:
//!
//!   parse → `cli::check_and_expand` (via `cli::parse_checked_program`) → `interpreter::interpret`
//!         / `compiler::compile` + `vm::Vm::run` / `transpile::emit`.
//!
//! The wrapper *logic* lives in plain `*_json(&str) -> String` functions (no wasm dependency) so it is
//! unit-tested on the native target by `cargo test`. Only the thin `#[wasm_bindgen]` exports at the
//! bottom are `wasm32`-gated. The browser bumps the wasm stack size at build time; the interpreter /
//! checker / compiler depth guards (`phorj::limits`) keep recursion within it.

use phorj::cli;
use serde_json::{json, Value};

/// Parse the diagnostics-array JSON string produced by [`cli::check_json_program`] back into a value
/// so it can be embedded (not re-stringified) in the wrapper's own JSON object.
fn diag_array(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| json!([]))
}

/// `check`: surface checker diagnostics (errors **and** warnings) without aborting on the first error.
/// On a syntax error the parse fails before the checker runs — reported in `parseError`.
pub fn check_json(src: &str) -> String {
    match cli::parse_program(src) {
        Ok(prog) => {
            let (diags, had_errors) = cli::check_json_program(&prog);
            json!({
                "ok": !had_errors,
                "diagnostics": diag_array(&diags),
                "parseError": Value::Null,
            })
            .to_string()
        }
        Err(e) => json!({
            "ok": false,
            "diagnostics": json!([]),
            "parseError": e,
        })
        .to_string(),
    }
}

/// Shared shape for the two execution backends: `error` = a front-end rejection (type error or a
/// backend that can't lower the program); `fault` = a runtime fault (index OOB, force-unwrap, …).
fn exec_json(stdout: Result<String, ExecErr>) -> String {
    match stdout {
        Ok(out) => json!({ "ok": true, "stdout": out, "fault": Value::Null, "error": Value::Null }),
        Err(ExecErr::Front(e)) => {
            json!({ "ok": false, "stdout": "", "fault": Value::Null, "error": e })
        }
        Err(ExecErr::Fault(f)) => {
            json!({ "ok": false, "stdout": "", "fault": f, "error": Value::Null })
        }
    }
    .to_string()
}

enum ExecErr {
    /// A type-check / parse error or a backend lowering rejection (shown as `error`).
    Front(String),
    /// A runtime fault raised while executing (shown as `fault`).
    Fault(String),
}

/// `run`: the tree-walking interpreter backend.
pub fn run_json(src: &str) -> String {
    let result = (|| {
        let prog = cli::parse_checked_program(src).map_err(ExecErr::Front)?;
        phorj::interpreter::interpret(&prog).map_err(|d| ExecErr::Fault(d.render(src)))
    })();
    exec_json(result)
}

/// `runvm`: the bytecode compiler + stack VM backend. Must be byte-identical to [`run_json`].
pub fn runvm_json(src: &str) -> String {
    let result = (|| {
        let prog = cli::parse_checked_program(src).map_err(ExecErr::Front)?;
        let program = phorj::compiler::compile(&prog).map_err(|d| ExecErr::Front(d.render(src)))?;
        phorj::vm::Vm::new(&program)
            .run()
            .map_err(|d| ExecErr::Fault(d.render(src)))
    })();
    exec_json(result)
}

/// `transpile`: emit the PHP source. The program returned by [`cli::parse_checked_program`] is already
/// type-checked + alias/generic-expanded, exactly what [`phorj::transpile::emit`] consumes.
pub fn transpile_json(src: &str) -> String {
    match cli::parse_checked_program(src).and_then(|prog| phorj::transpile::emit(&prog)) {
        Ok(php) => json!({ "ok": true, "php": php, "error": Value::Null }),
        Err(e) => json!({ "ok": false, "php": Value::Null, "error": e }),
    }
    .to_string()
}

/// `lift`: read PHP source, emit a Phorj **draft** (the inverse of `transpile`). Best-effort and
/// review-required; anything outside the Tier-1 lift subset is a clear `error` rather than a guess.
/// `lift_source` runs the L1→L2→L4→L3 pipeline on the calling stack (its own depth guard, no
/// `on_deep_stack` worker), so it is browser-safe like the other wrappers.
pub fn lift_json(php_src: &str) -> String {
    match phorj::lift::lifter::lift_source(php_src) {
        Ok(phorj) => json!({ "ok": true, "phorj": phorj, "error": Value::Null }),
        Err(e) => json!({ "ok": false, "phorj": Value::Null, "error": e }),
    }
    .to_string()
}

/// `explain CODE`: the diagnostic-code help text, or a fallback for an unknown code.
pub fn explain(code: &str) -> String {
    cli::explain_text(code).unwrap_or_else(|| format!("No explanation available for `{code}`."))
}

// --- WASM exports (browser only) ---------------------------------------------------------------
// Thin, `wasm32`-gated wrappers. Native builds/tests never touch wasm-bindgen.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn pg_check(src: &str) -> String {
    check_json(src)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn pg_run(src: &str) -> String {
    run_json(src)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn pg_runvm(src: &str) -> String {
    runvm_json(src)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn pg_transpile(src: &str) -> String {
    transpile_json(src)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn pg_lift(php_src: &str) -> String {
    lift_json(php_src)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn pg_explain(code: &str) -> String {
    explain(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const HELLO: &str =
        "package Main;\nimport Core.Console;\nfunction main() -> void {\n    Console.println(\"hi\");\n}\n";

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).expect("wrapper must emit valid JSON")
    }

    #[test]
    fn check_clean_program_is_ok_with_no_error_diagnostics() {
        let v = parse(&check_json(HELLO));
        assert_eq!(v["ok"], json!(true));
        assert!(v["parseError"].is_null());
        // Clean program: no *error* diagnostics (warnings, if any, do not gate `ok`).
        assert!(v["diagnostics"].is_array());
    }

    #[test]
    fn check_type_error_is_not_ok_and_lists_a_diagnostic() {
        let bad = "package Main;\nfunction main() -> void {\n    int x = \"not an int\";\n}\n";
        let v = parse(&check_json(bad));
        assert_eq!(v["ok"], json!(false));
        assert!(v["parseError"].is_null(), "type error is not a parse error");
        assert!(
            !v["diagnostics"].as_array().unwrap().is_empty(),
            "a type error must surface at least one diagnostic"
        );
    }

    #[test]
    fn check_syntax_error_populates_parse_error() {
        let v = parse(&check_json("package Main;\nfunction main( {\n}\n"));
        assert_eq!(v["ok"], json!(false));
        assert!(
            v["parseError"].is_string(),
            "a syntax error must populate parseError"
        );
    }

    #[test]
    fn run_hello_prints_hi() {
        let v = parse(&run_json(HELLO));
        assert_eq!(v["ok"], json!(true));
        assert_eq!(v["stdout"], json!("hi\n"));
        assert!(v["fault"].is_null());
        assert!(v["error"].is_null());
    }

    #[test]
    fn runvm_hello_matches_run() {
        let r = parse(&run_json(HELLO));
        let vm = parse(&runvm_json(HELLO));
        assert_eq!(r["stdout"], vm["stdout"], "run and runvm must agree");
        assert_eq!(vm["ok"], json!(true));
    }

    #[test]
    fn run_index_out_of_range_is_a_fault_not_a_panic() {
        let oob = "package Main;\nimport Core.Console;\nfunction main() -> void {\n    List<int> xs = [1];\n    Console.println(\"{xs[5]}\");\n}\n";
        let v = parse(&run_json(oob));
        assert_eq!(v["ok"], json!(false));
        assert!(
            v["fault"].is_string(),
            "an out-of-range index is a runtime fault"
        );
        assert!(v["error"].is_null());
    }

    #[test]
    fn transpile_hello_emits_php() {
        let v = parse(&transpile_json(HELLO));
        assert_eq!(v["ok"], json!(true));
        let php = v["php"].as_str().unwrap();
        assert!(php.contains("<?php"), "transpiled output must be PHP");
    }

    #[test]
    fn transpile_type_error_reports_error_not_php() {
        let bad = "package Main;\nfunction main() -> void {\n    int x = \"nope\";\n}\n";
        let v = parse(&transpile_json(bad));
        assert_eq!(v["ok"], json!(false));
        assert!(v["php"].is_null());
        assert!(v["error"].is_string());
    }

    #[test]
    fn explain_known_and_unknown_codes() {
        // A known code returns non-empty help; an unknown code returns the fallback.
        assert!(!explain("E-FORCE-UNWRAP").is_empty());
        assert!(explain("E-NOPE-NOT-A-CODE").contains("No explanation"));
    }

    #[test]
    fn lift_php_emits_phorj_draft() {
        let v = parse(&lift_json(
            "<?php function add(int $a, int $b): int { return $a + $b; }",
        ));
        assert_eq!(v["ok"], json!(true));
        let phg = v["phorj"].as_str().unwrap();
        assert!(phg.contains("function add(int a, int b): int {"), "{phg}");
        assert!(v["error"].is_null());
    }

    #[test]
    fn lift_outside_tier1_reports_error_not_phorj() {
        let v = parse(&lift_json("<?php function f(array $xs): void {}"));
        assert_eq!(v["ok"], json!(false));
        assert!(v["phorj"].is_null());
        assert!(v["error"].as_str().unwrap().contains("`array` type"));
    }
}
