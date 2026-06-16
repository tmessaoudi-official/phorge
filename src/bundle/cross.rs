//! Cross-build orchestration + stub cache. Wired in Wave C.

use std::path::PathBuf;

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
