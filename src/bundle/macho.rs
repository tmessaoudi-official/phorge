//! Mach-O 64 (thin) + fat/universal section readers. macOS stub production is deferred: the reader
//! ships and is fixture-tested now; the Mac *stub* and the `__PHORJ,__source` embed land with the
//! macOS leg. std-only, checked arithmetic (EV-7). Endianness trap: thin Mach-O bodies are
//! little-endian; fat headers are big-endian.

const MH_MAGIC_64: u32 = 0xFEED_FACF; // little-endian on disk
const LC_SEGMENT_64: u32 = 0x19;

/// Compare a fixed-width null-padded Mach-O name field to a str.
fn name16_eq(field: &[u8], want: &str) -> bool {
    let n = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    field.get(..n) == Some(want.as_bytes())
}

/// Find a (segment, section) in a thin Mach-O 64 LE image. None on any malformed input.
pub(crate) fn macho_find_section<'a>(bytes: &'a [u8], seg: &str, sect: &str) -> Option<&'a [u8]> {
    let u32at = |o: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(o..o.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    let u64at = |o: usize| -> Option<u64> {
        Some(u64::from_le_bytes(
            bytes.get(o..o.checked_add(8)?)?.try_into().ok()?,
        ))
    };
    if bytes.len() < 32 || u32at(0)? != MH_MAGIC_64 {
        return None;
    }
    let ncmds = u32at(16)? as usize;
    let mut off = 32usize; // load commands start after mach_header_64
    for _ in 0..ncmds {
        let cmd = u32at(off)?;
        let cmdsize = u32at(off.checked_add(4)?)? as usize;
        if cmdsize == 0 {
            return None; // malformed: would not advance
        }
        if cmd == LC_SEGMENT_64 {
            let nsects = u32at(off.checked_add(64)?)? as usize;
            let mut s = off.checked_add(72)?;
            for _ in 0..nsects {
                let sectname = bytes.get(s..s.checked_add(16)?)?;
                let segname = bytes.get(s.checked_add(16)?..s.checked_add(32)?)?;
                if name16_eq(sectname, sect) && name16_eq(segname, seg) {
                    let size = u64at(s.checked_add(40)?)? as usize;
                    let foff = u32at(s.checked_add(48)?)? as usize;
                    return bytes.get(foff..foff.checked_add(size)?);
                }
                s = s.checked_add(80)?;
            }
        }
        off = off.checked_add(cmdsize)?;
    }
    None
}

const FAT_MAGIC: u32 = 0xCAFE_BABE; // big-endian on disk

/// Find a (segment, section) inside a fat/universal binary by scanning each slice's thin Mach-O.
pub(crate) fn fat_find_section<'a>(bytes: &'a [u8], seg: &str, sect: &str) -> Option<&'a [u8]> {
    let u32be = |o: usize| -> Option<u32> {
        Some(u32::from_be_bytes(
            bytes.get(o..o.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    if bytes.len() < 8 || u32be(0)? != FAT_MAGIC {
        return None;
    }
    let nfat = u32be(4)? as usize;
    for i in 0..nfat {
        let arch = 8usize.checked_add(i.checked_mul(20)?)?;
        let off = u32be(arch.checked_add(8)?)? as usize;
        let size = u32be(arch.checked_add(12)?)? as usize;
        let slice = bytes.get(off..off.checked_add(size)?)?;
        if let Some(found) = macho_find_section(slice, seg, sect) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal Mach-O 64 LE: mach_header_64 (ncmds=1) → one LC_SEGMENT_64 with one section_64
    /// named ("__PHORJ","__source") pointing at `payload` placed after all headers.
    fn macho_with_section(seg: &str, sect: &str, payload: &[u8]) -> Vec<u8> {
        fn name16(s: &str) -> [u8; 16] {
            let mut a = [0u8; 16];
            a[..s.len()].copy_from_slice(s.as_bytes());
            a
        }
        let seg_cmd_size: u32 = 72 + 80; // segment_command_64 header + one section_64
        let mut v = Vec::new();
        // mach_header_64 (32 bytes)
        v.extend_from_slice(&0xFEED_FACFu32.to_le_bytes()); // magic MH_MAGIC_64
        v.extend_from_slice(&0x0100_0007u32.to_le_bytes()); // cputype (arbitrary)
        v.extend_from_slice(&0u32.to_le_bytes()); // cpusubtype
        v.extend_from_slice(&2u32.to_le_bytes()); // filetype MH_EXECUTE
        v.extend_from_slice(&1u32.to_le_bytes()); // ncmds
        v.extend_from_slice(&seg_cmd_size.to_le_bytes()); // sizeofcmds
        v.extend_from_slice(&0u32.to_le_bytes()); // flags
        v.extend_from_slice(&0u32.to_le_bytes()); // reserved
                                                  // segment_command_64 header (72 bytes)
        v.extend_from_slice(&0x19u32.to_le_bytes()); // cmd LC_SEGMENT_64
        v.extend_from_slice(&seg_cmd_size.to_le_bytes()); // cmdsize
        v.extend_from_slice(&name16("__PHORJ")); // segname
        v.extend_from_slice(&[0u8; 8 * 4]); // vmaddr,vmsize,fileoff,filesize (4 x u64)
        v.extend_from_slice(&0u32.to_le_bytes()); // maxprot
        v.extend_from_slice(&0u32.to_le_bytes()); // initprot
        v.extend_from_slice(&1u32.to_le_bytes()); // nsects
        v.extend_from_slice(&0u32.to_le_bytes()); // flags
                                                  // section_64 (80 bytes): sectname[16], segname[16], addr,size(u64), offset(u32), ...
        let sect_hdr_at = v.len();
        v.extend_from_slice(&name16(sect)); // sectname
        v.extend_from_slice(&name16(seg)); // segname
        v.extend_from_slice(&0u64.to_le_bytes()); // addr
        v.extend_from_slice(&(payload.len() as u64).to_le_bytes()); // size @ +40
        let offset_field_at = v.len();
        v.extend_from_slice(&0u32.to_le_bytes()); // offset @ +48 (patched below)
        v.extend_from_slice(&[0u8; 28]); // align,reloff,nreloc,flags,reserved1/2/3
        let data_off = v.len() as u32;
        v[offset_field_at..offset_field_at + 4].copy_from_slice(&data_off.to_le_bytes());
        let _ = sect_hdr_at;
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn macho_reader_finds_section() {
        let img = macho_with_section("__PHORJ", "__source", b"hi-macho");
        assert_eq!(
            macho_find_section(&img, "__PHORJ", "__source"),
            Some(&b"hi-macho"[..])
        );
        assert_eq!(macho_find_section(&img, "__PHORJ", "__nope"), None);
    }

    #[test]
    fn macho_reader_rejects_malformed_without_panic() {
        assert_eq!(macho_find_section(b"", "__PHORJ", "__source"), None);
        // cmdsize = 0 must not infinite-loop.
        let mut img = macho_with_section("__PHORJ", "__source", b"x");
        img[36..40].copy_from_slice(&0u32.to_le_bytes()); // first LC cmdsize (at off 32+4)
        assert_eq!(macho_find_section(&img, "__PHORJ", "__source"), None);
        // Wrong magic (big-endian swapped) → None, not a misparse.
        let mut img2 = macho_with_section("__PHORJ", "__source", b"x");
        img2[0..4].copy_from_slice(&0xCFFA_EDFEu32.to_le_bytes());
        assert_eq!(macho_find_section(&img2, "__PHORJ", "__source"), None);
    }

    /// Minimal fat binary: big-endian fat_header (nfat_arch=1) + one fat_arch pointing at a Mach-O slice.
    fn fat_wrapping(macho: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&0xCAFE_BABEu32.to_be_bytes()); // FAT_MAGIC (big-endian)
        v.extend_from_slice(&1u32.to_be_bytes()); // nfat_arch
                                                  // fat_arch (20 bytes, BE): cputype, cpusubtype, offset, size, align
        v.extend_from_slice(&0x0100_0007u32.to_be_bytes()); // cputype
        v.extend_from_slice(&0u32.to_be_bytes()); // cpusubtype
        let offset_at = v.len();
        v.extend_from_slice(&0u32.to_be_bytes()); // offset (patched)
        v.extend_from_slice(&(macho.len() as u32).to_be_bytes()); // size
        v.extend_from_slice(&0u32.to_be_bytes()); // align
        let off = v.len() as u32;
        v[offset_at..offset_at + 4].copy_from_slice(&off.to_be_bytes());
        v.extend_from_slice(macho);
        v
    }

    #[test]
    fn fat_reader_finds_section_in_slice() {
        let thin = macho_with_section("__PHORJ", "__source", b"fat-payload");
        let fat = fat_wrapping(&thin);
        assert_eq!(
            fat_find_section(&fat, "__PHORJ", "__source"),
            Some(&b"fat-payload"[..])
        );
    }

    #[test]
    fn fat_reader_rejects_malformed_without_panic() {
        assert_eq!(fat_find_section(b"", "__PHORJ", "__source"), None);
        // offset beyond EOF -> slice .get() returns None.
        let thin = macho_with_section("__PHORJ", "__source", b"x");
        let mut fat = fat_wrapping(&thin);
        let offset_at = 8 + 8; // fat_header(8) + cputype(4)+cpusubtype(4)
        fat[offset_at..offset_at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(fat_find_section(&fat, "__PHORJ", "__source"), None);
    }
}
