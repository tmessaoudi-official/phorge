//! Magic-sniffing dispatcher: locate the phorge payload section in an executable image of any
//! supported object format. Unknown/malformed magic → None (caller falls through to the CLI).
use crate::bundle::elf;

/// Canonical phorge payload identifiers. ELF + PE use a named section; Mach-O uses segment+section.
pub const ELF_PE_SECTION: &str = ".phorge";
pub const MACHO_SEG: &str = "__PHORGE";
pub const MACHO_SECT: &str = "__source";

/// Locate the phorge payload section by sniffing the leading magic. Never panics.
pub fn find_section(bytes: &[u8]) -> Option<&[u8]> {
    match bytes.get(0..4)? {
        [0x7F, b'E', b'L', b'F'] => elf::elf_find_section(bytes, ELF_PE_SECTION),
        // PE/COFF (Windows), Mach-O, and fat arms are wired in Wave B.
        _ => None,
    }
}
