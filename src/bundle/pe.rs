//! PE/COFF section reader (Windows). Minimal lookup, checked arithmetic, None on malformed input.
//! Our payload section name (".phorge", 7 bytes) is <= 8 bytes, so no COFF string-table indirection.

/// Find a named section's bytes in a PE/COFF image. None on any malformed/oversized input.
pub(crate) fn pe_find_section<'a>(bytes: &'a [u8], name: &str) -> Option<&'a [u8]> {
    if bytes.len() < 0x40 || bytes[0] != b'M' || bytes[1] != b'Z' {
        return None;
    }
    let u16at = |o: usize| -> Option<u16> {
        Some(u16::from_le_bytes(
            bytes.get(o..o.checked_add(2)?)?.try_into().ok()?,
        ))
    };
    let u32at = |o: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(o..o.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    let pe_off = u32at(0x3C)? as usize; // e_lfanew
    if bytes.get(pe_off..pe_off.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    let coff = pe_off.checked_add(4)?; // COFF File Header start
    let num_sections = u16at(coff.checked_add(2)?)? as usize;
    let opt_hdr_size = u16at(coff.checked_add(16)?)? as usize;
    let sect_table = coff.checked_add(20)?.checked_add(opt_hdr_size)?;
    let want = name.as_bytes();
    if want.len() > 8 {
        return None; // long names need the COFF string table; the phorge payload name is short
    }
    for i in 0..num_sections {
        let sh = sect_table.checked_add(i.checked_mul(40)?)?;
        let raw = bytes.get(sh..sh.checked_add(8)?)?;
        let n = raw.iter().position(|&b| b == 0).unwrap_or(8);
        if &raw[..n] == want {
            let size = u32at(sh.checked_add(16)?)? as usize; // SizeOfRawData
            let ptr = u32at(sh.checked_add(20)?)? as usize; // PointerToRawData
            return bytes.get(ptr..ptr.checked_add(size)?);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal PE image: DOS stub (MZ + e_lfanew@0x3C) → "PE\0\0" → COFF header (1 section,
    /// 0-byte optional header) → one 40-byte section header named ".phorge" pointing at `payload`.
    fn pe_with_section(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0u8; 0x40];
        v[0] = b'M';
        v[1] = b'Z';
        let pe_off: u32 = 0x40;
        v[0x3C..0x40].copy_from_slice(&pe_off.to_le_bytes());
        v.extend_from_slice(b"PE\0\0"); // 0x40
                                        // COFF File Header (20 bytes): Machine@0, NumberOfSections@2=1, ..., SizeOfOptionalHeader@16=0.
        let mut coff = [0u8; 20];
        coff[2..4].copy_from_slice(&1u16.to_le_bytes()); // NumberOfSections
        v.extend_from_slice(&coff);
        // Section header (40 bytes) at 0x40 + 4 + 20 = 0x58. Name[8], SizeOfRawData@16, PointerToRawData@20.
        let sect_hdr_off = v.len();
        let mut sh = [0u8; 40];
        let nb = name.as_bytes();
        sh[..nb.len()].copy_from_slice(nb);
        v.extend_from_slice(&sh);
        let ptr = v.len() as u32; // raw data goes right after the section header
        v[sect_hdr_off + 16..sect_hdr_off + 20]
            .copy_from_slice(&(payload.len() as u32).to_le_bytes());
        v[sect_hdr_off + 20..sect_hdr_off + 24].copy_from_slice(&ptr.to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn pe_reader_finds_section() {
        let img = pe_with_section(".phorge", b"hello-pe");
        assert_eq!(pe_find_section(&img, ".phorge"), Some(&b"hello-pe"[..]));
        assert_eq!(pe_find_section(&img, ".other"), None);
    }

    #[test]
    fn pe_reader_rejects_malformed_without_panic() {
        assert_eq!(pe_find_section(b"", ".phorge"), None);
        assert_eq!(pe_find_section(b"MZ", ".phorge"), None); // too short for e_lfanew
                                                             // e_lfanew = u32::MAX would overflow a plain `+`; checked arithmetic must return None.
        let mut img = pe_with_section(".phorge", b"x");
        img[0x3C..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(pe_find_section(&img, ".phorge"), None);
        // NumberOfSections huge + an absent name -> the loop walks past EOF; the checked slice
        // `.get()` returns None (no overflow-panic), so the lookup is None. (Searching for the present
        // ".phorge" would correctly match at index 0 before the huge count is ever reached.)
        let mut img2 = pe_with_section(".phorge", b"x");
        img2[0x44..0x46].copy_from_slice(&u16::MAX.to_le_bytes()); // COFF NumberOfSections@(0x40+4)+2
        assert_eq!(pe_find_section(&img2, ".absent"), None);
    }
}
