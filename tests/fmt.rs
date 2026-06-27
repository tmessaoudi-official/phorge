//! `phg fmt` CLI integration tests (M-fmt F3) — in-place write, `--check` exit codes, parse-error
//! safety (file untouched, exit 2), and idempotence at the CLI layer.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use phorge::cli;

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let unique = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "phorge_fmt_it_{tag}_{}_{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::write(&p, contents).unwrap();
        p
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const MESSY: &str = "package Main;import Core.Console;\nfunction  main()->void{int  x=1+2 ;Console.println(\"{x}\");}\n";

#[test]
fn check_reports_unformatted_then_clean_after_write() {
    let d = TempDir::new("check");
    let f = d.write("a.phg", MESSY);

    // --check on a messy file: exit 1, no write.
    let (report, code) = cli::cmd_fmt(&[f.display().to_string()], true);
    assert_eq!(code, 1, "{report}");
    assert!(report.contains("would reformat"), "{report}");
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        MESSY,
        "check must not write"
    );

    // format in place: exit 0, file rewritten.
    let (report, code) = cli::cmd_fmt(&[f.display().to_string()], false);
    assert_eq!(code, 0, "{report}");
    let formatted = std::fs::read_to_string(&f).unwrap();
    assert_ne!(formatted, MESSY);
    assert!(formatted.contains("int x = 1 + 2;"), "{formatted}");

    // now --check is clean: exit 0.
    let (_r, code) = cli::cmd_fmt(&[f.display().to_string()], true);
    assert_eq!(code, 0);
}

#[test]
fn unparseable_file_is_left_untouched_exit_2() {
    let d = TempDir::new("broken");
    let broken = "package Main;\nfunction (\n";
    let f = d.write("bad.phg", broken);
    let (report, code) = cli::cmd_fmt(&[f.display().to_string()], false);
    assert_eq!(code, 2, "{report}");
    assert!(report.contains("did not parse"), "{report}");
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        broken,
        "a broken file must never be rewritten"
    );
}

#[test]
fn directory_formats_all_phg_recursively() {
    let d = TempDir::new("dir");
    d.write("a.phg", MESSY);
    d.write("b.phg", MESSY);
    let (report, code) = cli::cmd_fmt(&[d.path().display().to_string()], false);
    assert_eq!(code, 0, "{report}");
    assert!(report.contains("2 file(s) formatted"), "{report}");
}

#[test]
fn stdin_path_formats_a_source_string() {
    let out = cli::fmt_source(MESSY).expect("formats");
    assert!(out.contains("function main(): void {"), "{out}");
    // idempotent at the source level.
    assert_eq!(out, cli::fmt_source(&out).unwrap());
}
