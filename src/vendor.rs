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

use crate::bundle::cross::fnv1a_64;
use crate::lock::{Lock, LockEntry};
use crate::manifest::{Manifest, Pin, Project};

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
        // Clone into a unique temp dir, check out the pin, resolve it to a full SHA.
        let clone_dir = unique_temp_dir(&dep.name);
        let _guard = TempDirGuard(clone_dir.clone());
        git(&["clone", "--quiet", &dep.git, path_str(&clone_dir)?], None)?;
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
fn git(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new("git");
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

/// A process-unique temp directory name for a clone (the dep name slashes flattened). Not created
/// here — `git clone` creates it.
fn unique_temp_dir(dep_name: &str) -> PathBuf {
    let safe: String = dep_name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    std::env::temp_dir().join(format!("phorge_vendor_{}_{safe}", std::process::id()))
}

/// Removes a temp clone directory on drop (best-effort), so a clone is cleaned up even on an early
/// `?` return.
struct TempDirGuard(PathBuf);
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
