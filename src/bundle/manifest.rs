//! The cross-stub integrity manifest (M2.5 Phase 3a): a line-based text mapping each cross target
//! triple to the SHA-256 of its prebuilt runtime stub. The released `x86_64-linux-gnu` primary bakes
//! this manifest in (via `build.rs` + `PHORGE_BAKE_STUB_MANIFEST`); a distributed phg verifies a
//! downloaded stub against it before embedding (`cross::download_stub`). Format:
//!
//! ```text
//! # comment / blank lines skipped
//! version 0.5.0-alpha.1                 # optional, sanity-checked against CARGO_PKG_VERSION
//! x86_64-unknown-linux-musl  <64-hex>
//! aarch64-unknown-linux-gnu  <64-hex>
//! ```
//!
//! Parsing is tolerant: a malformed entry is skipped (never a panic). Lookups must match exactly.

/// The baked manifest text. `build.rs` writes `$OUT_DIR/stub_manifest.txt` — the CI primary build
/// copies the real manifest in; every other build writes an empty file (so a cross stub's bytes are
/// manifest-independent, breaking the stub↔manifest circularity — design §6/P3-3).
const BAKED: &str = include_str!(concat!(env!("OUT_DIR"), "/stub_manifest.txt"));

/// The release-asset name for a target's stub — shared by the download client and the CI workflow.
/// `phg-stub-<triple>` (`.exe` for a Windows target).
#[must_use]
pub fn asset_name(target: &str) -> String {
    if target.contains("windows") {
        format!("phg-stub-{target}.exe")
    } else {
        format!("phg-stub-{target}")
    }
}

/// A parsed manifest: `(triple, sha256-hex)` entries in file order.
#[derive(Debug, Default, Clone)]
pub struct Manifest {
    entries: Vec<(String, String)>,
}

impl Manifest {
    /// The expected SHA-256 (lowercase hex) for `target`, or `None` if absent.
    #[must_use]
    pub fn lookup(&self, target: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(t, _)| t == target)
            .map(|(_, h)| h.as_str())
    }

    /// Number of entries (test/diagnostic use).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the manifest carries no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A 64-char lowercase hex string?
fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Parse a manifest. Comments (`#`) and blank lines are skipped; a `version <v>` line is recognized
/// (consumed, not stored — the caller may sanity-check it); every other line must be
/// `<triple> <64-hex>` or it is skipped. Tolerant by design — never panics on bad input.
#[must_use]
pub fn parse(text: &str) -> Manifest {
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(a), Some(b)) = (it.next(), it.next()) else {
            continue; // a single token (or none) — not an entry
        };
        if it.next().is_some() {
            continue; // trailing garbage → skip the whole line
        }
        if a == "version" {
            continue; // the optional version line — recognized, not an entry
        }
        let hash = b.to_ascii_lowercase();
        if is_sha256_hex(&hash) {
            entries.push((a.to_string(), hash));
        }
        // else: malformed hash → skip silently (tolerant parse)
    }
    Manifest { entries }
}

/// The active manifest: the runtime `PHORGE_STUB_MANIFEST=<path>` override (a test/mirror seam) if set
/// and readable, else the baked-in manifest. An unreadable override falls back to the baked manifest.
#[must_use]
pub fn active() -> Manifest {
    if let Some(path) = std::env::var_os("PHORGE_STUB_MANIFEST") {
        if let Ok(text) = std::fs::read_to_string(&path) {
            return parse(&text);
        }
    }
    parse(BAKED)
}

/// The registry base URL the download client fetches stubs from. `PHORGE_STUB_REGISTRY` overrides it
/// (trailing `/` normalised); otherwise the default is the crate's repository releases for the running
/// version: `{CARGO_PKG_REPOSITORY}/releases/download/v{CARGO_PKG_VERSION}/`. `None` when no override
/// is set and the crate has no `repository` (so the caller emits the "needs a source checkout" error).
#[must_use]
pub fn registry_base() -> Option<String> {
    if let Some(v) = std::env::var_os("PHORGE_STUB_REGISTRY") {
        let mut s = v.to_string_lossy().into_owned();
        if !s.ends_with('/') {
            s.push('/');
        }
        return Some(s);
    }
    let repo = option_env!("CARGO_PKG_REPOSITORY").unwrap_or("");
    if repo.is_empty() {
        return None;
    }
    let repo = repo.trim_end_matches('/');
    let ver = env!("CARGO_PKG_VERSION");
    Some(format!("{repo}/releases/download/v{ver}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_with_comments_and_version() {
        let m = parse(
            "# header comment\n\
             version 0.5.0-alpha.1\n\
             \n\
             x86_64-unknown-linux-musl ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
             aarch64-unknown-linux-gnu e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n",
        );
        assert_eq!(m.len(), 2);
        assert_eq!(
            m.lookup("x86_64-unknown-linux-musl"),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert!(m.lookup("nope-triple").is_none());
    }

    #[test]
    fn uppercase_hash_is_normalised() {
        let m = parse("x86_64-pc-windows-gnu BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD");
        assert_eq!(
            m.lookup("x86_64-pc-windows-gnu"),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
    }

    #[test]
    fn malformed_entries_are_skipped() {
        let m = parse(
            "good-triple ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
             short-hash deadbeef\n\
             not-hex zzzz816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
             lonely-token\n\
             too many tokens here\n",
        );
        assert_eq!(m.len(), 1);
        assert!(m.lookup("good-triple").is_some());
        assert!(m.lookup("short-hash").is_none());
    }

    #[test]
    fn empty_manifest_has_no_entries() {
        assert!(parse("").is_empty());
        assert!(parse("# only a comment\n\nversion 1.2.3\n").is_empty());
    }

    #[test]
    fn asset_name_windows_gets_exe() {
        assert_eq!(
            asset_name("x86_64-unknown-linux-musl"),
            "phg-stub-x86_64-unknown-linux-musl"
        );
        assert_eq!(
            asset_name("x86_64-pc-windows-gnu"),
            "phg-stub-x86_64-pc-windows-gnu.exe"
        );
    }

    #[test]
    fn registry_base_override_normalises_trailing_slash() {
        // SAFETY of test isolation: these env vars are unique to this crate's stub registry.
        std::env::set_var("PHORGE_STUB_REGISTRY", "file:///tmp/reg");
        assert_eq!(registry_base().as_deref(), Some("file:///tmp/reg/"));
        std::env::set_var("PHORGE_STUB_REGISTRY", "file:///tmp/reg/");
        assert_eq!(registry_base().as_deref(), Some("file:///tmp/reg/"));
        std::env::remove_var("PHORGE_STUB_REGISTRY");
    }
}
