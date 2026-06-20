//! `phg vendor` — fetch `[require]` git dependencies into an offline `vendor/` tree.
//!
//! This is the **only** part of Phorge that touches the network, and it runs **only** on an explicit
//! `phg vendor` (never on `run`/`check`/`transpile` — those resolve offline from the committed
//! `vendor/`). For each dependency it: `git clone`s the repo, checks out the pinned `tag`/`rev`,
//! resolves that to a full commit SHA, reads the dependency's own manifest to find its source root,
//! and copies that source subtree into `vendor/<vendor>/<package>/` (the dep's coordinate — so each
//! dependency owns one subtree, making re-vendoring idempotent without a blanket wipe). It then
//! writes `phorge.lock` pinning every dependency to its resolved SHA + an FNV-1a-64 content hash.
//!
//! **Layout (M5-10 / O-7):** `vendor/<vendor>/<package>/` is each dependency's own mini source root;
//! files inside keep their internal package directory structure (`package acme.strutil` ⇒
//! `…/acme/strutil/x.phg`). folder=path is validated against the per-dependency root at load time
//! ([`crate::loader`]). There is deliberately **no nested `phorge.toml`** under `vendor/` — a vendored
//! tree is a flat package forest, so the project-aware test harness never mistakes a dependency for a
//! standalone project.
//!
//! **Determinism:** a tag/rev pins to a full SHA in the lockfile; the committed `vendor/` is the
//! source of truth at run time. Tests exercise this against a `file://` local-git fixture — never a
//! live remote (the same determinism rule that defers URL/network features to M6).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::bundle::cross::fnv1a_64;
use crate::lock::{Lock, LockEntry};
use crate::manifest::{validate_path_component, Manifest, Pin, Project};

/// Vendor every `[require]` dependency of `project` and (re)write `phorge.lock`. Returns a short
/// human-readable summary. Network access happens here and nowhere else.
///
/// `[require-dev]` is parsed but not vendored in this slice (dev-only dependency vendoring is an M5
/// follow-up); the run path consumes `[require]` only.
pub fn vendor(project: &Project) -> Result<String, String> {
    let deps = &project.manifest.require;
    if deps.is_empty() {
        return Ok("phg vendor: no [require] dependencies to vendor\n".to_string());
    }
    let vendor_root = project.root.join("vendor");
    let mut entries: Vec<LockEntry> = Vec::with_capacity(deps.len());
    let mut summary = String::new();

    for dep in deps {
        let pin = match &dep.pin {
            Pin::Tag(t) => t,
            Pin::Rev(r) => r,
        };
        // Security boundary (GA blocker B1): a `git`/`pin` value from an attacker-authored
        // `phorge.toml` reaches the `git` CLI. Reject anything that would be read as a git option
        // (leading `-`) or a command-executing remote helper (`ext::`/`file::`) before it is passed.
        validate_git_arg("dependency git URL", &dep.git)?;
        validate_git_arg("dependency pin", pin)?;
        // Defensive re-check of the name at the join boundary (already validated at parse time, but a
        // `Manifest` constructed by other means must not be trusted) — GA blocker B2.
        validate_path_component("dependency name", &dep.name)?;
        // Clone into a unique temp dir, check out the pin, resolve it to a full SHA.
        let clone_dir = unique_temp_dir(&dep.name);
        let _guard = TempDirGuard(clone_dir.clone());
        // `--` ends option parsing so a hostile URL can never be an option; `protocol.ext.allow=never`
        // disables the command-executing `ext::` remote helper at the git level (the standard
        // `file://` scheme, used by the offline test fixtures, is a different protocol and stays
        // enabled). `checkout` takes no `--` (that would force the ref to be read as a pathspec); the
        // leading-`-` rejection above is its guard.
        git(
            &[
                "-c",
                "protocol.ext.allow=never",
                "clone",
                "--quiet",
                "--",
                &dep.git,
                path_str(&clone_dir)?,
            ],
            None,
        )?;
        git(&["checkout", "--quiet", pin], Some(&clone_dir))?;
        let rev = git(&["rev-parse", "HEAD"], Some(&clone_dir))?
            .trim()
            .to_string();

        // Read the dependency's manifest to locate its source root.
        let dep_toml = clone_dir.join(Manifest::MANIFEST_FILE);
        let dep_manifest = Manifest::parse(&read(&dep_toml)?)
            .map_err(|e| format!("dependency `{}`: {}: {e}", dep.name, dep_toml.display()))?;
        let dep_src = clone_dir.join(&dep_manifest.source);

        // Copy the dependency's `.phg` source tree into `vendor/<name>/` via a staging dir, then
        // swap atomically — idempotent and crash-safe, touching only this dependency's own subtree.
        let dest = vendor_root.join(&dep.name);
        let staging = dest.with_extension("phorge-staging");
        remove_tree(&staging)?;
        let copied = copy_phg_tree(&dep_src, &staging)?;
        if copied == 0 {
            return Err(format!(
                "dependency `{}`: no `.phg` files found under its source root `{}`",
                dep.name, dep_manifest.source
            ));
        }
        remove_tree(&dest)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::rename(&staging, &dest)
            .map_err(|e| format!("cannot place vendored `{}`: {e}", dep.name))?;

        let hash = hash_tree(&dest)?;
        summary.push_str(&format!(
            "  vendored {} @ {} ({} file{}, {:016x})\n",
            dep.name,
            &rev[..rev.len().min(12)],
            copied,
            if copied == 1 { "" } else { "s" },
            hash
        ));
        entries.push(LockEntry {
            name: dep.name.clone(),
            git: dep.git.clone(),
            rev,
            hash: format!("{hash:016x}"),
        });
    }

    let lock = Lock { packages: entries };
    let lock_path = project.root.join(Lock::LOCK_FILE);
    std::fs::write(&lock_path, lock.render())
        .map_err(|e| format!("cannot write {}: {e}", lock_path.display()))?;

    Ok(format!(
        "phg vendor: {} dependenc{} vendored → {} + {}\n{summary}",
        deps.len(),
        if deps.len() == 1 { "y" } else { "ies" },
        vendor_root.display(),
        lock_path.display(),
    ))
}

/// Run `git` with `args` (optionally in `cwd`), returning stdout on success. Any non-zero exit or a
/// missing `git` binary is a clean error — never a panic.
///
/// Git env vars (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_COMMON_DIR`) are cleared so
/// that `phg vendor` works correctly when invoked from inside a git worktree (e.g. from a
/// pre-commit hook where git sets these vars to point at the parent repo's worktree).  Without
/// this, `git clone` would try to reuse the caller's working tree instead of the destination.
fn git(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd.output().map_err(|e| {
        format!(
            "failed to run `git {}`: {e} (is git installed?)",
            args.join(" ")
        )
    })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "`git {}` failed: {}",
            args.join(" "),
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Reject a `git` URL or a `pin` (tag/rev) value that could subvert the `git` invocation. Combined
/// with the `--` separator and `-c protocol.ext.allow=never` at the call site, this closes the
/// `phg vendor` argument-injection / arbitrary-command-execution vector (GA blocker B1):
/// - a leading `-` would be parsed as a git option (e.g. `--upload-pack=…`);
/// - `ext::` / `file::` are git *remote-helper transports* — `ext::sh -c '…'` runs an arbitrary
///   command on clone. (The standard `file://` URL scheme is NOT one of these and stays allowed:
///   `"file://…".starts_with("file::")` is false, so the offline `file://` test fixtures pass.)
fn validate_git_arg(kind: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{kind} must not be empty"));
    }
    if value.starts_with('-') {
        return Err(format!(
            "{kind} `{value}` must not start with `-` (it would be parsed as a git option)"
        ));
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("ext::") || lower.starts_with("file::") {
        return Err(format!(
            "{kind} `{value}` uses a disallowed git transport \
             (`ext::`/`file::` can execute arbitrary commands)"
        ));
    }
    Ok(())
}

/// Recursively copy every `.phg` file under `src` into `dest`, preserving relative paths. Returns
/// the number of files copied. A non-`.phg` file (or any non-source artifact) is skipped — only the
/// language sources are vendored.
fn copy_phg_tree(src: &Path, dest: &Path) -> Result<usize, String> {
    let mut count = 0;
    copy_phg_rec(src, src, dest, &mut count)?;
    Ok(count)
}

fn copy_phg_rec(root: &Path, dir: &Path, dest: &Path, count: &mut usize) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .map(|e| e.map(|e| e.path()).map_err(|e| format!("read entry: {e}")))
        .collect::<Result<_, _>>()?;
    entries.sort();
    for p in entries {
        if p.is_dir() {
            copy_phg_rec(root, &p, dest, count)?;
        } else if p.extension().and_then(|s| s.to_str()) == Some("phg") {
            let rel = p
                .strip_prefix(root)
                .map_err(|_| format!("{} escaped its source root", p.display()))?;
            let target = dest.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
            std::fs::copy(&p, &target)
                .map_err(|e| format!("cannot copy {} → {}: {e}", p.display(), target.display()))?;
            *count += 1;
        }
    }
    Ok(())
}

/// FNV-1a-64 over the sorted `(relative-path \0 bytes)` of every `.phg` file under `dir`. Including
/// the path keeps two different layouts with the same byte payloads from colliding; the sort makes
/// the hash independent of directory iteration order.
fn hash_tree(dir: &Path) -> Result<u64, String> {
    let mut files: Vec<PathBuf> = Vec::new();
    gather(dir, &mut files)?;
    files.sort();
    let mut buf: Vec<u8> = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(dir).unwrap_or(f);
        buf.extend_from_slice(rel.to_string_lossy().as_bytes());
        buf.push(0);
        buf.extend_from_slice(&read_bytes(f)?);
        buf.push(0);
    }
    Ok(fnv1a_64(&buf))
}

fn gather(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for e in std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))? {
        let p = e.map_err(|e| format!("read entry: {e}"))?.path();
        if p.is_dir() {
            gather(&p, out)?;
        } else if p.extension().and_then(|s| s.to_str()) == Some("phg") {
            out.push(p);
        }
    }
    Ok(())
}

/// Remove a directory tree if it exists (a no-op when absent). Used to make vendoring idempotent.
fn remove_tree(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)
            .map_err(|e| format!("cannot remove {}: {e}", dir.display()))?;
    }
    Ok(())
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn path_str(p: &Path) -> Result<&str, String> {
    p.to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {}", p.display()))
}

/// A call-unique temp directory name for a clone (the dep name slashes flattened). Not created here
/// — `git clone` creates it. The name carries BOTH the pid (unique across processes) AND a
/// process-local atomic counter (unique across calls within one process): vendoring the *same* dep
/// concurrently — e.g. two integration tests in one parallel `cargo test` process — must not target
/// the same path, or the second `git clone` fails on a non-empty destination.
fn unique_temp_dir(dep_name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let unique = N.fetch_add(1, Ordering::Relaxed);
    let safe: String = dep_name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    std::env::temp_dir().join(format!(
        "phorge_vendor_{}_{unique}_{safe}",
        std::process::id()
    ))
}

/// Removes a temp clone directory on drop (best-effort), so a clone is cleaned up even on an early
/// `?` return.
struct TempDirGuard(PathBuf);
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::{unique_temp_dir, validate_git_arg};

    /// Two clones of the SAME dependency within one process must target distinct paths — otherwise
    /// parallel `cargo test` (one process, shared pid) races on `git clone` into a non-empty dir.
    #[test]
    fn unique_temp_dir_is_call_unique() {
        let a = unique_temp_dir("acme/greet");
        let b = unique_temp_dir("acme/greet");
        assert_ne!(a, b, "same-dep clone dirs must differ per call");
    }

    /// B1: option-injection and command-executing remote helpers are rejected; legitimate URLs
    /// (including the `file://` scheme the offline fixtures use) and pins are accepted.
    #[test]
    fn validate_git_arg_blocks_injection_allows_real_urls() {
        // Rejected: leading-dash option injection, and the ext::/file:: command transports.
        assert!(validate_git_arg("git", "--upload-pack=/bin/sh").is_err());
        assert!(validate_git_arg("git", "ext::sh -c 'touch pwned'").is_err());
        assert!(validate_git_arg("git", "EXT::sh -c x").is_err()); // case-insensitive
        assert!(validate_git_arg("git", "file::/tmp/evil").is_err());
        assert!(validate_git_arg("pin", "-v9").is_err());
        assert!(validate_git_arg("git", "").is_err());
        // Accepted: the file:// scheme (NOT file::), https, ssh shorthand, ordinary tags/revs.
        assert!(validate_git_arg("git", "file:///tmp/upstream").is_ok());
        assert!(validate_git_arg("git", "https://github.com/acme/lib.phg").is_ok());
        assert!(validate_git_arg("git", "git@github.com:acme/lib.phg").is_ok());
        assert!(validate_git_arg("pin", "v1.2.0").is_ok());
        assert!(validate_git_arg("pin", "a1b2c3d4e5").is_ok());
    }
}
