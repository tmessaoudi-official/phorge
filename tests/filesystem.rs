//! `Core.File` filesystem-mutation tests under a CONTROLLED temp directory.
//!
//! The mutation ops (`append`/`delete`/`rename`/`copy`) are `pure: false`, so the byte-identity
//! differential SKIPS any program importing `Core.File` (see `uses_impure_native` in
//! `tests/differential.rs`) — a filesystem side effect is state outside the program text. They are
//! exercised here instead, each in its own unique temp dir so the tests are order-independent and can
//! run concurrently. Every case also asserts `run ≡ runvm` (the Rust backends always agree — only the
//! PHP leg is unreliable across a separate process, which is why these are quarantined from the oracle,
//! not from run≡runvm).

use phorj::cli::{cmd_run, cmd_runvm};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh, unique, empty temp directory for one test (created here, removed on drop).
struct TmpDir(PathBuf);
impl TmpDir {
    fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("phorj_fs_{}_{}_{}", std::process::id(), tag, n));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TmpDir(dir)
    }
    /// A path inside the temp dir, as a forward-slash string for embedding in a program.
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().replace('\\', "/")
    }
}
impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run `src` on both backends, assert they agree, and return the shared stdout.
fn both(src: &str) -> String {
    let r = cmd_run(src).expect("run ok");
    assert_eq!(cmd_runvm(src).expect("runvm ok"), r, "run ≡ runvm");
    r
}

#[test]
fn write_append_read_round_trip() {
    let d = TmpDir::new("wart");
    let p = d.path("a.txt");
    let src = format!(
        r#"package Main;
import Core.Output;
import Core.File;
function main(): void {{
    File.write("{p}", "hello");
    File.append("{p}", " world");
    Output.printLine(File.read("{p}") ?? "<none>");
}}"#
    );
    assert_eq!(both(&src), "hello world\n");
}

#[test]
fn size_reflects_content_and_is_null_when_missing() {
    let d = TmpDir::new("size");
    let p = d.path("s.txt");
    let missing = d.path("nope.txt");
    let src = format!(
        r#"package Main;
import Core.Output;
import Core.File;
function main(): void {{
    File.write("{p}", "12345");
    Output.printLine("size={{File.size(\"{p}\") ?? -1}}");
    Output.printLine("missing={{File.size(\"{missing}\") ?? -1}}");
}}"#
    );
    assert_eq!(both(&src), "size=5\nmissing=-1\n");
}

#[test]
fn copy_returns_byte_count_and_duplicates() {
    let d = TmpDir::new("copy");
    let from = d.path("from.txt");
    let to = d.path("to.txt");
    let src = format!(
        r#"package Main;
import Core.Output;
import Core.File;
function main(): void {{
    File.write("{from}", "abcd");
    int n = File.copy("{from}", "{to}");
    Output.printLine("copied={{n}} both={{File.exists(\"{from}\")}}/{{File.exists(\"{to}\")}}");
    Output.printLine(File.read("{to}") ?? "<none>");
}}"#
    );
    assert_eq!(both(&src), "copied=4 both=true/true\nabcd\n");
}

#[test]
fn rename_moves_the_file() {
    let d = TmpDir::new("rename");
    let from = d.path("old.txt");
    let to = d.path("new.txt");
    let src = format!(
        r#"package Main;
import Core.Output;
import Core.File;
function main(): void {{
    File.write("{from}", "x");
    File.rename("{from}", "{to}");
    Output.printLine("old={{File.exists(\"{from}\")}} new={{File.exists(\"{to}\")}}");
}}"#
    );
    assert_eq!(both(&src), "old=false new=true\n");
}

#[test]
fn delete_removes_the_file() {
    let d = TmpDir::new("delete");
    let p = d.path("gone.txt");
    let src = format!(
        r#"package Main;
import Core.Output;
import Core.File;
function main(): void {{
    File.write("{p}", "x");
    Output.printLine("before={{File.exists(\"{p}\")}}");
    File.delete("{p}");
    Output.printLine("after={{File.exists(\"{p}\")}}");
}}"#
    );
    assert_eq!(both(&src), "before=true\nafter=false\n");
}
