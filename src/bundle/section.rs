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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::container::encode_container;

    #[test]
    fn dispatch_unknown_magic_is_none() {
        assert_eq!(find_section(b"\0\0\0\0not an exe"), None);
        assert_eq!(find_section(b""), None);
    }

    #[test]
    fn dispatch_routes_pe_to_pe_reader() {
        // An MZ image carrying a `.phorge` section with a real container: find_section must sniff PE,
        // route to the PE reader, and return the raw container that decodes back to the source.
        let payload = encode_container(b"function main() { println(\"pe\"); }");
        let mut img = vec![0u8; 0x40];
        img[0] = b'M';
        img[1] = b'Z';
        img[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        img.extend_from_slice(b"PE\0\0");
        let mut coff = [0u8; 20];
        coff[2..4].copy_from_slice(&1u16.to_le_bytes());
        img.extend_from_slice(&coff);
        let sh_off = img.len();
        let mut sh = [0u8; 40];
        sh[..7].copy_from_slice(b".phorge");
        img.extend_from_slice(&sh);
        let ptr = img.len() as u32;
        img[sh_off + 16..sh_off + 20].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        img[sh_off + 20..sh_off + 24].copy_from_slice(&ptr.to_le_bytes());
        img.extend_from_slice(&payload);
        let got = find_section(&img).expect("section");
        assert_eq!(
            crate::bundle::container::decode_container(got).as_deref(),
            Some(&b"function main() { println(\"pe\"); }"[..])
        );
    }
}
