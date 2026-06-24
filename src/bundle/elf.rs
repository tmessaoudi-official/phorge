//! ELF64 little-endian section reader. Hand-rolled, std-only, checked arithmetic (EV-7). Moved
//! verbatim from the Phase-1 `bundle.rs`; the only change is `pub(crate)` visibility.

/// Find a named section's bytes in an ELF64 little-endian image. Returns `None` on any
/// malformed/unsupported input (too short, not ELF, 32-bit, big-endian, OOB offset).
/// Hand-rolled — no object-parsing crate may link into the produced binary.
pub(crate) fn elf_find_section<'a>(bytes: &'a [u8], name: &str) -> Option<&'a [u8]> {
    // e_ident: 0x7f 'E' 'L' 'F', EI_CLASS=2 (ELF64), EI_DATA=1 (little-endian).
    if bytes.len() < 64 || bytes[0..4] != *b"\x7fELF" || bytes[4] != 2 || bytes[5] != 1 {
        return None;
    }
    // ALL offset arithmetic is checked. `e_shoff` is read as a full u64 cast to usize, so a crafted
    // header can drive a derived offset to usize::MAX; a plain `+` would overflow-panic under the
    // debug/test profile (overflow-checks on). Adversarial input must return None, never panic (EV-7).
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
    let u64at = |o: usize| -> Option<u64> {
        Some(u64::from_le_bytes(
            bytes.get(o..o.checked_add(8)?)?.try_into().ok()?,
        ))
    };

    let e_shoff = u64at(0x28)? as usize; // section header table file offset
    let e_shentsize = u16at(0x3A)? as usize; // per-entry size (64 for ELF64)
    let e_shnum = u16at(0x3C)? as usize;
    let e_shstrndx = u16at(0x3E)? as usize;
    if e_shentsize < 64 {
        return None;
    }

    // Section-name string table (the section header at index e_shstrndx).
    let strtab_hdr = e_shoff.checked_add(e_shstrndx.checked_mul(e_shentsize)?)?;
    let strtab_off = u64at(strtab_hdr.checked_add(24)?)? as usize; // sh_offset
    let strtab_size = u64at(strtab_hdr.checked_add(32)?)? as usize; // sh_size
    let strtab = bytes.get(strtab_off..strtab_off.checked_add(strtab_size)?)?;

    for i in 0..e_shnum {
        let sh = e_shoff.checked_add(i.checked_mul(e_shentsize)?)?;
        let sh_name = u32at(sh)? as usize; // offset into strtab
        let rest = strtab.get(sh_name..)?;
        let nul = rest.iter().position(|&b| b == 0)?;
        if std::str::from_utf8(&rest[..nul]).ok()? == name {
            let off = u64at(sh.checked_add(24)?)? as usize; // sh_offset
            let sz = u64at(sh.checked_add(32)?)? as usize; // sh_size
            return bytes.get(off..off.checked_add(sz)?);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::container::{decode_container, encode_container};
    use crate::bundle::section::ELF_PE_SECTION;

    #[test]
    fn elf_reader_finds_added_section() {
        // Use the compiled test binary itself as a real ELF64 to objcopy into.
        let exe = std::env::current_exe().expect("current_exe");
        let tmp = std::env::temp_dir().join("phorge_bundle_reader_test");
        let payload = std::env::temp_dir().join("phorge_bundle_reader_payload");
        let src = b"import Core.Console; function main() -> void { Console.println(\"x\"); }";
        std::fs::write(&payload, encode_container(src)).unwrap();
        let objcopy = std::env::var("PHORGE_OBJCOPY").unwrap_or_else(|_| "llvm-objcopy".into());
        let status = std::process::Command::new(&objcopy)
            .args([
                "--add-section",
                &format!(".phorge={}", payload.display()),
                "--set-section-flags",
                ".phorge=noload,readonly",
            ])
            .arg(&exe)
            .arg(&tmp)
            .status();
        let _ = std::fs::remove_file(&payload);
        match status {
            Ok(s) if s.success() => {}
            _ => {
                eprintln!("skipping: {objcopy} unavailable");
                let _ = std::fs::remove_file(&tmp);
                return;
            }
        }
        let bytes = std::fs::read(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        let section = elf_find_section(&bytes, ELF_PE_SECTION).expect("section found");
        assert_eq!(decode_container(section).as_deref(), Some(&src[..]));
    }

    #[test]
    fn elf_reader_rejects_adversarial_offsets_without_panic() {
        // A 64-byte ELF64-LE header whose e_shoff drives derived section-header offsets to/near
        // usize::MAX. A plain `+` would overflow-panic under the debug profile (overflow-checks on,
        // the profile the quality gate runs); checked arithmetic must return None instead — the
        // function's "Never panics" contract and INVARIANTS EV-7 ("never SIGABRT/panic on input").
        fn header(e_shoff: u64, e_shentsize: u16, e_shnum: u16, e_shstrndx: u16) -> Vec<u8> {
            let mut h = vec![0u8; 64];
            h[0..4].copy_from_slice(b"\x7fELF");
            h[4] = 2; // EI_CLASS = ELF64
            h[5] = 1; // EI_DATA = little-endian
            h[0x28..0x30].copy_from_slice(&e_shoff.to_le_bytes());
            h[0x3A..0x3C].copy_from_slice(&e_shentsize.to_le_bytes());
            h[0x3C..0x3E].copy_from_slice(&e_shnum.to_le_bytes());
            h[0x3E..0x40].copy_from_slice(&e_shstrndx.to_le_bytes());
            h
        }
        // e_shoff = u64::MAX -> strtab_hdr = usize::MAX -> `strtab_hdr + 24` would overflow.
        assert_eq!(
            elf_find_section(&header(u64::MAX, 64, 0, 0), ELF_PE_SECTION),
            None
        );
        // e_shoff = u64::MAX - 28 -> overflow surfaces inside the u64at closure (`o + 8`).
        assert_eq!(
            elf_find_section(&header(u64::MAX - 28, 64, 0, 0), ELF_PE_SECTION),
            None
        );
        // e_shstrndx = 0xFFFF with a small base -> large in-range offset, OOB `.get()` -> None.
        assert_eq!(
            elf_find_section(&header(64, 64, 0, 0xFFFF), ELF_PE_SECTION),
            None
        );
        // Sanity: a well-formed-looking but section-less header returns None, no panic.
        assert_eq!(
            elf_find_section(&header(64, 64, 0, 0), ELF_PE_SECTION),
            None
        );
    }
}
