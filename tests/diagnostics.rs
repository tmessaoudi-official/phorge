//! Golden-diagnostic corpus (M-DX S1) — a regression net for *rendered diagnostic quality*.
//!
//! Each `conformance/diagnostics/<name>.phg` is a program that must FAIL `check`, paired with a
//! sibling `<name>.expected` holding the exact rendered diagnostic (stage/line/col header, source
//! line, caret, `[CODE]`, and `hint:`). This is the "exact info + one exact fix" bar the milestone
//! sets: it pins not just *that* a program is rejected but *how the error reads*, so a regression in
//! a message, code, caret column, or hint is a loud CI failure.
//!
//! Complements the `every_emitted_diagnostic_code_has_an_explanation` coverage ratchet (which proves
//! every code is *explainable*) — here we prove a representative set of codes *render well*.
//!
//! Regenerate the `.expected` files after an intentional diagnostic change:
//!     PHORJ_BLESS=1 cargo test --test diagnostics
//! Review the diff before committing — a blessed change to a diagnostic is a deliberate act.

use std::fs;
use std::path::{Path, PathBuf};

use phorj::cli::cmd_check;

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("conformance/diagnostics")
}

/// All `<name>.phg` cases in the corpus, sorted for deterministic ordering.
fn cases() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(corpus_dir())
        .expect("read diagnostics corpus")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("phg"))
        .collect();
    v.sort();
    v
}

#[test]
fn corpus_is_nonempty() {
    assert!(
        !cases().is_empty(),
        "no diagnostic corpus cases found in {}",
        corpus_dir().display()
    );
}

#[test]
fn every_case_fails_check_with_its_golden_diagnostic() {
    let bless = std::env::var("PHORJ_BLESS").as_deref() == Ok("1");
    let mut failures = Vec::new();

    for phg in cases() {
        let src = fs::read_to_string(&phg).expect("read .phg");
        // A corpus case must be *rejected* — its whole point is to exercise a diagnostic.
        let rendered = match cmd_check(&src) {
            Ok(ok) => {
                failures.push(format!(
                    "{}: expected a diagnostic but `check` succeeded:\n{ok}",
                    phg.display()
                ));
                continue;
            }
            Err(rendered) => rendered,
        };
        let rendered = rendered.trim_end().to_string();

        let expected_path = phg.with_extension("expected");
        if bless {
            fs::write(&expected_path, format!("{rendered}\n")).expect("write .expected");
            continue;
        }

        let expected = match fs::read_to_string(&expected_path) {
            Ok(s) => s.trim_end().to_string(),
            Err(_) => {
                failures.push(format!(
                    "{}: missing `.expected` — run `PHORJ_BLESS=1 cargo test --test diagnostics`",
                    phg.display()
                ));
                continue;
            }
        };
        if rendered != expected {
            failures.push(format!(
                "{}: rendered diagnostic does not match golden\n--- expected ---\n{expected}\n--- actual ---\n{rendered}",
                phg.display()
            ));
        }
    }

    assert!(failures.is_empty(), "\n\n{}\n", failures.join("\n\n"));
}
