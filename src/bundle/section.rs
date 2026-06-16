//! Magic-sniffing dispatcher: locate the phorge payload section in an executable image of any
//! supported object format. Unknown/malformed magic → None (caller falls through to the CLI).
use crate::bundle::{elf, macho, pe};

/// Canonical phorge payload identifiers. ELF + PE use a named section; Mach-O uses segment+section.
pub const ELF_PE_SECTION: &str = ".phorge";
pub const MACHO_SEG: &str = "__PHORGE";
pub const MACHO_SECT: &str = "__source";

/// Locate the phorge payload section by sniffing the leading magic. Never panics.
pub fn find_section(bytes: &[u8]) -> Option<&[u8]> {
    match bytes.get(0..4)? {
        [0x7F, b'E', b'L', b'F'] => elf::elf_find_section(bytes, ELF_PE_SECTION),
        [b'M', b'Z', _, _] => pe::pe_find_section(bytes, ELF_PE_SECTION),
        // MH_MAGIC_64 = 0xFEEDFACF, little-endian on disk → CF FA ED FE.
        [0xCF, 0xFA, 0xED, 0xFE] => macho::macho_find_section(bytes, MACHO_SEG, MACHO_SECT),
        // FAT_MAGIC = 0xCAFEBABE, big-endian on disk → CA FE BA BE.
        [0xCA, 0xFE, 0xBA, 0xBE] => macho::fat_find_section(bytes, MACHO_SEG, MACHO_SECT),
        _ => None,
    }
}
