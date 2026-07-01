//! Build profiles (M-DX S0) — the `Dev` / `Release` gate that every environment-sensitive,
//! value-exposing, or diagnostic-verbosity feature keys off.
//!
//! # The keystone principle
//!
//! **A build profile may change diagnostics, observability, and side-channels ONLY — never observable
//! program behavior or output.** `run ≡ runvm ≡ real PHP` must hold *identically* under both `Dev`
//! and `Release`. Consequences:
//! - Assertions are always-checked; they are NOT stripped in `Release` (unlike C `NDEBUG`). A profile
//!   may only make a *failure diagnostic* terser, never remove a check (that would change control flow).
//! - No profile-conditional semantics (no "checked overflow in Dev, wrapping in Release").
//! - All value-exposing / observability output goes to **stderr**, outside the correctness spine.
//!
//! # How the profile is chosen (compile-time / entry-time, never a runtime env var)
//!
//! - `phg run` / `runvm` / `test` — the interactive developer tool — are **Dev** (the default).
//! - `phg serve` is **Release** unless `--dev` is passed (rich HTML fault pages leak traces/source,
//!   so they are Dev-only).
//! - `phg build` bakes the profile **into the artifact's container** (secure-by-construction):
//!   **Release by default**, `--dev` opt-in. A shipped binary therefore carries its own profile —
//!   no environment variable can flip a Release artifact into Dev at runtime.
//!
//! S0 establishes the type, threads it through `serve`, and bakes it into `phg build` artifacts.
//! The value-exposing consumers (value-dump S3, assertion richness S4, the debugger S5) read
//! [`active`] and are gated to `Dev`.

use std::sync::OnceLock;

/// The build profile in force for a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// The interactive developer experience: rich diagnostics, value inspection, the debugger.
    #[default]
    Dev,
    /// A shipped artifact: value-exposing machinery is gated off (and, as it lands, compiled out).
    Release,
}

impl Profile {
    #[must_use]
    pub fn is_dev(self) -> bool {
        matches!(self, Profile::Dev)
    }

    #[must_use]
    pub fn is_release(self) -> bool {
        matches!(self, Profile::Release)
    }

    /// The lowercase name, for CLI banners and `--profile` echoing.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Profile::Dev => "dev",
            Profile::Release => "release",
        }
    }

    /// Encode into the container `flags` byte's low bit (bit 0): `Dev = 1`, `Release = 0`. `Release`
    /// is `0` so a pre-profile artifact (flags byte `0`) decodes as `Release` — the secure default.
    #[must_use]
    pub fn to_flag_bit(self) -> u8 {
        match self {
            Profile::Dev => 1,
            Profile::Release => 0,
        }
    }

    /// Decode from a container `flags` byte (reads bit 0 only; other bits reserved).
    #[must_use]
    pub fn from_flags(flags: u8) -> Self {
        if flags & 1 == 1 {
            Profile::Dev
        } else {
            Profile::Release
        }
    }
}

/// The process-wide active profile, set **once** at the program entry (the CLI verb, a `serve` flag,
/// or a built artifact's embedded container) and read by every profile-gated consumer. It is
/// deliberately *not* settable from an environment variable — `Release` is secure by construction.
static ACTIVE: OnceLock<Profile> = OnceLock::new();

/// Set the active profile for this process. The first call wins; later calls are ignored (the entry
/// point sets it exactly once). Returns the effective profile.
pub fn set_active(profile: Profile) -> Profile {
    *ACTIVE.get_or_init(|| profile)
}

/// The active profile — [`Profile::Dev`] (the default) until an entry point [`set_active`]s otherwise.
/// A built artifact always sets it from its embedded container before running, so a shipped binary is
/// never accidentally `Dev`.
#[must_use]
pub fn active() -> Profile {
    ACTIVE.get().copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_release_predicates() {
        assert!(Profile::Dev.is_dev() && !Profile::Dev.is_release());
        assert!(Profile::Release.is_release() && !Profile::Release.is_dev());
        assert_eq!(Profile::default(), Profile::Dev);
    }

    #[test]
    fn flag_bit_round_trips_and_release_is_zero() {
        // Release must be 0 so a pre-profile container (flags byte 0) decodes as the secure default.
        assert_eq!(Profile::Release.to_flag_bit(), 0);
        assert_eq!(Profile::Dev.to_flag_bit(), 1);
        assert_eq!(Profile::from_flags(0), Profile::Release);
        assert_eq!(Profile::from_flags(1), Profile::Dev);
        // Only bit 0 is significant; reserved high bits are ignored.
        assert_eq!(Profile::from_flags(0b1111_1110), Profile::Release);
        assert_eq!(Profile::from_flags(0b0000_0001), Profile::Dev);
    }
}
