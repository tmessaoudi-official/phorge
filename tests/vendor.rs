//! `phorge vendor` integration test — the one network-touching code path, exercised **offline**.
//!
//! A live remote would make the test non-deterministic (the same reason URL/network features are
//! deferred to M6), so we build a throwaway local git repository in a temp dir and fetch it via a
//! `file://` URL. That drives the real `git clone`/`checkout`/`rev-parse` path while staying fully
//! offline and reproducible. The test then proves the end-to-end contract: vendored sources load
//! and run **byte-identically on both backends** through the offline project loader.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use phorge::cli;
use phorge::loader;
use phorge::lock::Lock;
use phorge::manifest::Project;
use phorge::vendor::vendor;

/// A unique temp dir, removed on drop.
struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let unique = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "phorge_vendor_it_{tag}_{}_{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, contents).unwrap();
        p
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(args: &[&str], cwd: &Path) -> String {
    let out = Command::new("git")
        // Config flags must precede the subcommand; keep commits deterministic and independent of
        // the host's git config.
        .args(["-c", "user.email=test@phorge", "-c", "user.name=test"])
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Build a local "upstream" git repo for a one-package library dependency, tagged `v1.0.0`.
/// Returns its path; the `file://` URL of it is the dependency's `git` coordinate.
fn build_upstream(dir: &TempDir) {
    dir.write(
        "phorge.toml",
        "name = \"acme/greet\"\nversion = \"1.0.0\"\nsource = \"src\"\n",
    );
    dir.write(
        "src/acme/greet/hello.phg",
        "package acme.greet;\nfunction hi(string who) -> string { return \"hi {who}\"; }\n",
    );
    let p = dir.path();
    git(&["init", "-q"], p);
    git(&["add", "-A"], p);
    git(&["commit", "-q", "-m", "greet v1.0.0"], p);
    git(&["tag", "v1.0.0"], p);
}

/// Vendoring a `file://` dependency populates `vendor/` + a correct `phorge.lock`, and the vendored
/// package then loads + runs byte-identically on both backends.
#[test]
fn vendor_fetches_and_loads_offline() {
    let upstream = TempDir::new("upstream");
    build_upstream(&upstream);
    let url = format!("file://{}", upstream.path().display());

    // A consumer project that requires the upstream library at its tag.
    let consumer = TempDir::new("consumer");
    consumer.write(
        "phorge.toml",
        &format!("name = \"acme/app\"\nsource = \"src\"\n\n[require]\n\"acme/greet\" = {{ git = \"{url}\", tag = \"v1.0.0\" }}\n"),
    );
    let entry = consumer.write(
        "src/main.phg",
        "package main;\nimport core.console;\nimport acme.greet;\n\
         function main() { console.println(greet.hi(\"phorge\")); }\n",
    );

    // --- vendor (the network/git path, here over file://) ------------------
    let project = Project::detect(consumer.path()).unwrap().expect("project");
    let summary = vendor(&project).expect("vendor succeeds");
    assert!(summary.contains("acme/greet"), "summary: {summary}");

    // The lockfile pins the resolved SHA + a content hash.
    let lock_path = consumer.path().join(Lock::LOCK_FILE);
    assert!(lock_path.is_file(), "phorge.lock written");
    let lock = Lock::parse(&std::fs::read_to_string(&lock_path).unwrap()).unwrap();
    assert_eq!(lock.packages.len(), 1);
    assert_eq!(lock.packages[0].name, "acme/greet");
    assert_eq!(lock.packages[0].rev.len(), 40, "a full commit SHA");
    assert_eq!(lock.packages[0].hash.len(), 16, "fnv-1a-64 hex");

    // The vendored source landed under vendor/<name>/<package-path>/.
    let vendored = consumer
        .path()
        .join("vendor/acme/greet/acme/greet/hello.phg");
    assert!(
        vendored.is_file(),
        "vendored file at {}",
        vendored.display()
    );

    // --- load + run offline (no network) -----------------------------------
    let unit = loader::load(&entry).expect("offline load");
    let run = cli::run_program(&unit.program, &unit.diag_src);
    let runvm = cli::runvm_program(&unit.program, &unit.diag_src);
    assert_eq!(run.as_deref(), Ok("hi phorge\n"), "interpreter output");
    assert_eq!(run, runvm, "backends must be byte-identical");
}

/// Re-running `vendor` is idempotent: the second run reproduces the same lock (same SHA + hash).
#[test]
fn vendor_is_idempotent() {
    let upstream = TempDir::new("upstream2");
    build_upstream(&upstream);
    let url = format!("file://{}", upstream.path().display());

    let consumer = TempDir::new("consumer2");
    consumer.write(
        "phorge.toml",
        &format!("name = \"acme/app\"\nsource = \"src\"\n\n[require]\n\"acme/greet\" = {{ git = \"{url}\", tag = \"v1.0.0\" }}\n"),
    );
    let project = Project::detect(consumer.path()).unwrap().expect("project");

    vendor(&project).expect("first vendor");
    let first = std::fs::read_to_string(consumer.path().join(Lock::LOCK_FILE)).unwrap();
    vendor(&project).expect("second vendor");
    let second = std::fs::read_to_string(consumer.path().join(Lock::LOCK_FILE)).unwrap();
    assert_eq!(first, second, "re-vendoring is deterministic");
}

/// A `[require]` dependency that was never vendored is a clean `E-VENDOR-MISSING` at load time —
/// the run path never falls back to the network.
#[test]
fn missing_vendor_is_rejected() {
    let consumer = TempDir::new("consumer3");
    consumer.write(
        "phorge.toml",
        "name = \"acme/app\"\nsource = \"src\"\n\n[require]\n\"acme/greet\" = { git = \"file:///nonexistent\", tag = \"v1.0.0\" }\n",
    );
    let entry = consumer.write("src/main.phg", "package main;\nfunction main() {}\n");
    let err = loader::load(&entry).unwrap_err();
    assert!(err.contains("E-VENDOR-MISSING"), "got: {err}");
}
