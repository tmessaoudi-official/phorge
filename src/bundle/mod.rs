//! Self-contained-executable support (M2.5). Embed a `.phg` program as a named section in a phorj
//! binary and detect+extract it at startup. std-only: the readers are hand-rolled (no `object`/
//! `goblin`) because they link into the produced binary. `unsafe` is forbidden crate-wide (lib.rs).
pub mod container;
pub mod cross;
mod elf;
mod macho;
pub mod manifest;
mod pe;
pub mod section;
pub mod sha256;

pub use container::encode_container;
pub use section::{find_section, ELF_PE_SECTION as SECTION_NAME};

/// If this executable carries an embedded phorj payload, return its source **and the build profile**
/// baked into its container (M-DX S0). Any failure — no payload, unreadable `current_exe`, malformed
/// image, bad CRC — returns `None`, so the caller falls through to normal CLI dispatch. Never panics.
pub fn embedded_program() -> Option<(String, crate::profile::Profile)> {
    let exe = std::env::current_exe().ok()?;
    let bytes = std::fs::read(exe).ok()?;
    let section = section::find_section(&bytes)?;
    let (payload, profile) = container::decode_container_full(section)?;
    Some((String::from_utf8(payload).ok()?, profile))
}
