//! Cross-build orchestration + stub cache. Wired in Wave C.

use crate::bundle::{encode_container, section::ELF_PE_SECTION};
use std::path::PathBuf;

/// Copy `stub` to `out` with the phorge payload added as the ELF/PE `.phorge` section, then mark it
/// executable on unix. Format-aware (F5): `--set-section-flags noload,readonly` is proven on ELF but
/// unreliable on PE/COFF, and is cosmetic for our reader (find_section reads raw file bytes via the
/// section's file offset, never the loaded image), so the flags are applied only to ELF; PE embeds
/// with `--add-section` alone. (Mach-O embedding — `__PHORGE,__source` — lands with macOS support.)
pub(crate) fn embed_section(
    stub: &std::path::Path,
    out: &std::path::Path,
    src: &str,
) -> Result<(), String> {
    let payload = std::env::temp_dir().join(format!("phorge-build-{}.bin", std::process::id()));
    std::fs::write(&payload, encode_container(src.as_bytes()))
        .map_err(|e| format!("cannot write payload: {e}"))?;
    let objcopy = std::env::var("PHORGE_OBJCOPY").unwrap_or_else(|_| "llvm-objcopy".into());
    let is_elf = std::fs::read(stub)
        .ok()
        .map(|b| b.starts_with(&[0x7F, b'E', b'L', b'F']))
        .unwrap_or(false);
    let mut cmd = std::process::Command::new(&objcopy);
    cmd.args([
        "--add-section",
        &format!("{ELF_PE_SECTION}={}", payload.display()),
    ]);
    if is_elf {
        cmd.args([
            "--set-section-flags",
            &format!("{ELF_PE_SECTION}=noload,readonly"),
        ]);
    }
    let status = cmd.arg(stub).arg(out).status();
    let _ = std::fs::remove_file(&payload);
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => return Err(format!("{objcopy} failed with status {s}")),
        Err(e) => return Err(format!("cannot run {objcopy}: {e}")),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(out) {
            let mut perm = meta.permissions();
            perm.set_mode(perm.mode() | 0o111);
            let _ = std::fs::set_permissions(out, perm);
        }
    }
    Ok(())
}

/// Build for the host target: the stub is this running phorge binary. Returns the human report line.
pub fn build_host(src: &str, out: &std::path::Path) -> Result<String, String> {
    let stub = std::env::current_exe().map_err(|e| format!("cannot locate phorge binary: {e}"))?;
    embed_section(&stub, out, src)?;
    Ok(format!("built {}\n", out.display()))
}

/// The Phase-2 cross targets (macOS deferred — reader ships, stub does not).
pub const PHASE2_TARGETS: &[&str] = &[
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-gnu",
    "aarch64-unknown-linux-musl",
    "x86_64-pc-windows-gnu",
];

/// Output filename for a target: `<stem>` (or `<stem>.exe` for windows).
pub(crate) fn output_name(stem: &str, target: &str) -> String {
    if target.contains("windows") {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

/// Error if the rustup std for `target` is not installed (precise, actionable message).
pub(crate) fn ensure_target_installed(target: &str) -> Result<(), String> {
    let out = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map_err(|e| format!("cannot run rustup: {e}"))?;
    let installed = String::from_utf8_lossy(&out.stdout);
    if installed.lines().any(|l| l.trim() == target) {
        Ok(())
    } else {
        Err(format!(
            "target '{target}' not installed — run: rustup target add {target}"
        ))
    }
}

/// The host target triple, parsed from `rustc -vV`'s `host:` line. `None` if rustc is unavailable or
/// the line is missing — callers fall back to a literal label so `--all` still names the artifact.
pub(crate) fn host_triple() -> Option<String> {
    let out = std::process::Command::new("rustc")
        .arg("-vV")
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(|t| t.trim().to_string()))
}

/// Reject apple/darwin targets in Phase 2: `embed_section` writes only the ELF/PE `.phorge` section,
/// but a Mac binary self-reads via `__PHORGE,__source` — embedding into a Mac stub would silently
/// yield a binary that can't find its source (INVARIANTS #1). Reject rather than emit a broken
/// artifact (F7 / design §6, §8).
fn reject_if_macos(target: &str) -> Result<(), String> {
    if target.contains("apple") || target.contains("darwin") {
        return Err(format!(
            "target '{target}': macOS stub production is deferred — Phase 2 builds Linux + Windows \
             only (the Mach-O reader ships, but the Mac stub + `__PHORGE,__source` embed do not). \
             See design §8."
        ));
    }
    Ok(())
}

/// Build for a single explicit target (cross-compile + embed).
pub fn build_target(
    input_path: &str,
    src: &str,
    target: &str,
    out_path: Option<&str>,
) -> Result<String, String> {
    crate::cli::cmd_check(src)?;
    reject_if_macos(target)?;
    ensure_target_installed(target)?;
    let stem = std::path::Path::new(input_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("cannot derive output name from {input_path}"))?;
    let out = match out_path {
        Some(p) => std::path::PathBuf::from(p),
        None => std::path::PathBuf::from(output_name(stem, target)),
    };
    let stub = build_stub(target)?;
    embed_section(&stub, &out, src)?;
    Ok(format!("built {} ({target})\n", out.display()))
}

/// Build for host + all Phase-2 targets into `dist/`. `out_path` is ignored (per-target names).
pub fn build_all(input_path: &str, src: &str, _out_path: Option<&str>) -> Result<String, String> {
    crate::cli::cmd_check(src)?;
    let stem = std::path::Path::new(input_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("cannot derive output name from {input_path}"))?;
    std::fs::create_dir_all("dist").map_err(|e| format!("cannot create dist/: {e}"))?;
    let mut report = String::new();
    // host first — name it with the real host triple for a consistent <stem>-<triple> scheme (P2-10).
    let host_label = host_triple().unwrap_or_else(|| "host".to_string());
    let host_out = std::path::PathBuf::from(format!(
        "dist/{}",
        output_name(&format!("{stem}-{host_label}"), &host_label)
    ));
    build_host(src, &host_out)?;
    report.push_str(&format!("built {} ({host_label})\n", host_out.display()));
    for t in PHASE2_TARGETS {
        ensure_target_installed(t)?;
        let out =
            std::path::PathBuf::from(format!("dist/{}", output_name(&format!("{stem}-{t}"), t)));
        let stub = build_stub(t)?;
        embed_section(&stub, &out, src)?;
        report.push_str(&format!("built {} ({t})\n", out.display()));
    }
    Ok(report)
}

/// Cross-compile a phorge stub for `target` via cargo-zigbuild; cached. Wired in C4.
pub(crate) fn build_stub(target: &str) -> Result<std::path::PathBuf, String> {
    Err(format!("cross-build for {target} not yet implemented (C4)"))
}

/// FNV-1a-64 of a byte slice — a cache-key identity hash (NOT a security hash). std-only, ~10 lines.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325; // offset basis
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3); // FNV prime
    }
    hash
}

/// `${XDG_CACHE_HOME:-$HOME/.cache}/phorge/stubs/<fnv-of-phorge>` — keyed on the host phorge bytes so
/// a rebuilt phorge invalidates stale cross-stubs (design B-6/P2-3: the parity-spine guard).
pub fn cache_dir(phorge_bytes: &[u8]) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(
        base.join("phorge")
            .join("stubs")
            .join(format!("{:016x}", fnv1a_64(phorge_bytes))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_64_known_vectors() {
        // FNV-1a-64: empty -> offset basis; "a" -> 0xaf63dc4c8601ec8c (canonical reference vectors).
        assert_eq!(fnv1a_64(b""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xAF63_DC4C_8601_EC8C);
    }

    #[test]
    fn cache_dir_layout_includes_phorge_hash() {
        std::env::set_var("XDG_CACHE_HOME", "/tmp/phorge-cache-test");
        let dir = cache_dir(b"phorge-bytes").expect("cache dir");
        let s = dir.to_string_lossy();
        assert!(
            s.starts_with("/tmp/phorge-cache-test/phorge/stubs/"),
            "got {s}"
        );
        assert!(
            s.ends_with(&format!("{:016x}", fnv1a_64(b"phorge-bytes"))),
            "got {s}"
        );
        std::env::remove_var("XDG_CACHE_HOME");
    }
}
