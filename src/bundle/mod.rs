//! Self-contained-executable support (M2.5). Embed a `.phg` program as a named section in a phorge
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

/// If this executable carries an embedded phorge payload, return its source. Any failure — no
/// payload, unreadable `current_exe`, malformed image, bad CRC — returns `None`, so the caller
/// falls through to normal CLI dispatch. Never panics.
pub fn embedded_source() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let bytes = std::fs::read(exe).ok()?;
    let section = section::find_section(&bytes)?;
    let payload = container::decode_container(section)?;
    String::from_utf8(payload).ok()
}
