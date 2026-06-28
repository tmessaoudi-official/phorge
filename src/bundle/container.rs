//! The versioned, CRC-guarded payload container (design §3). Format-agnostic — shared by every
//! object-format reader. Moved verbatim from the Phase-1 `bundle.rs`.

const MAGIC: [u8; 8] = *b"PHORJ\0\0\0";
const CONTAINER_VERSION: u16 = 1;
const HEADER_LEN: u16 = 32;

/// CRC-32 (IEEE 802.3, reflected, poly 0xEDB88320), bitwise — std-only, no static table.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Build the payload container for `source` (Phase 1: `payload_kind = 0`, source UTF-8).
/// Layout per design §3: magic | version | header_len | kind | comp | enc | flags | len |
/// payload_crc32 | header_crc32(over [0..28)) | payload.
pub fn encode_container(source: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN as usize + source.len());
    out.extend_from_slice(&MAGIC); // 0..8
    out.extend_from_slice(&CONTAINER_VERSION.to_le_bytes()); // 8..10
    out.extend_from_slice(&HEADER_LEN.to_le_bytes()); // 10..12
    out.push(0); // 12 payload_kind = source_utf8
    out.push(0); // 13 compression = none
    out.push(0); // 14 encryption = none
    out.push(0); // 15 flags
    out.extend_from_slice(&(source.len() as u64).to_le_bytes()); // 16..24
    out.extend_from_slice(&crc32(source).to_le_bytes()); // 24..28 payload_crc32
    let header_crc = crc32(&out[0..28]); // 28..32 header_crc32
    out.extend_from_slice(&header_crc.to_le_bytes());
    out.extend_from_slice(source); // 32..
    out
}

/// Validate + extract the source bytes from a container blob. Returns `None` on any malformed,
/// tampered, truncated, or unsupported-version/kind input — callers fall through to the CLI.
pub fn decode_container(blob: &[u8]) -> Option<Vec<u8>> {
    if blob.len() < HEADER_LEN as usize || blob[0..8] != MAGIC {
        return None;
    }
    if u16::from_le_bytes([blob[8], blob[9]]) > CONTAINER_VERSION {
        return None; // artifact built for a newer phorj
    }
    let header_len = u16::from_le_bytes([blob[10], blob[11]]) as usize;
    if header_len < HEADER_LEN as usize || header_len > blob.len() {
        return None;
    }
    let header_crc = u32::from_le_bytes([blob[28], blob[29], blob[30], blob[31]]);
    if crc32(&blob[0..28]) != header_crc {
        return None; // can't trust payload_len from a corrupt header
    }
    if blob[12] != 0 {
        return None; // only source_utf8 in Phase 1
    }
    let payload_len = u64::from_le_bytes(blob[16..24].try_into().ok()?) as usize;
    let payload_crc = u32::from_le_bytes([blob[24], blob[25], blob[26], blob[27]]);
    let end = header_len.checked_add(payload_len)?;
    if end > blob.len() {
        return None;
    }
    let payload = &blob[header_len..end];
    if crc32(payload) != payload_crc {
        return None;
    }
    Some(payload.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        // Canonical CRC-32 of "123456789" is 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn container_round_trips() {
        let src = b"import Core.Console; function main() -> void { Console.println(\"hi\"); }";
        let blob = encode_container(src);
        assert_eq!(decode_container(&blob).as_deref(), Some(&src[..]));
    }

    #[test]
    fn rejects_bad_magic() {
        let mut blob = encode_container(b"x");
        blob[0] = b'Q';
        assert_eq!(decode_container(&blob), None);
    }

    #[test]
    fn rejects_tampered_payload() {
        let mut blob = encode_container(b"abcd");
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        assert_eq!(decode_container(&blob), None);
    }

    #[test]
    fn rejects_tampered_header() {
        let mut blob = encode_container(b"abcd");
        blob[16] ^= 0xFF; // corrupt payload_len -> header_crc mismatch
        assert_eq!(decode_container(&blob), None);
    }

    #[test]
    fn rejects_truncated() {
        let blob = encode_container(b"abcd");
        assert_eq!(decode_container(&blob[..20]), None);
        assert_eq!(decode_container(&[]), None);
    }

    #[test]
    fn rejects_future_version() {
        let mut blob = encode_container(b"abcd");
        blob[8] = 2; // container_version = 2
        blob[9] = 0;
        // header_crc now stale -> rejected (also future-version guard would catch it)
        assert_eq!(decode_container(&blob), None);
    }
}
