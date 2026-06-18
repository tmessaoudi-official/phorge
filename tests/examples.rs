//! Guard rail: every program in `examples/` must be a *runnable* Phorge program.
//! Running `phg run <file>` on each must exit 0. This turns "someone committed a
//! broken/fragment example" into a caught regression rather than a silent rot.

use std::path::Path;
use std::process::Command;

/// Path to the compiled `phorge` binary (Cargo sets this for integration tests).
const BIN: &str = env!("CARGO_BIN_EXE_phg");

#[test]
fn every_example_runs_clean() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut phg: Vec<_> = std::fs::read_dir(&dir)
        .expect("examples/ dir must exist")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "phg"))
        .collect();
    phg.sort();

    assert!(
        !phg.is_empty(),
        "no .phg files found under {}",
        dir.display()
    );

    for file in &phg {
        let out = Command::new(BIN)
            .args(["run", file.to_str().unwrap()])
            .output()
            .expect("spawn phorge");
        assert!(
            out.status.success(),
            "example {} did not run clean (exit {:?}):\n{}",
            file.display(),
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
