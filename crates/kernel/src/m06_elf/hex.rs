//! M-06 Intel HEX 解析器 (IHEX).
//!
//! Spec: <https://en.wikipedia.org/wiki/Intel_HEX> §Record format.
//!
//! Each ASCII line: `:LLAAAATT[DD…]CC`
//!   LL    byte count   (1 byte, 0x00..=0xFF)
//!   AAAA  address      (2 bytes, big-endian)
//!   TT    record type  (1 byte)
//!   DD    payload bytes (LL of them)
//!   CC    checksum      (1 byte); two's complement of (sum of all bytes
//!         + CC) must be 0 mod 256.
//!
//! Record types we handle (M-06 §3.3):
//!   0x00 — data, contributes to the segment at `extHigh + AAAA`
//!   0x01 — EOF, ends parsing
//!   0x02 — Extended Segment Address (ESA); `extHigh = upper16 << 4`
//!   0x04 — Extended Linear Address (ELA); `extHigh = upper16 << 16`
//!   0x03 / 0x05 — Start Address records (ignored, M-06 §3.3)
//!   anything else — E_LDR-01
//!
//! Limits from [`super::ParseLimits`]:
//!   - line data count ≤ MAX_HEX_RECORD_BYTES (FF)
//!   - total merged segment count ≤ limits.segment_count
//!   - file size ≤ limits.elf_size
//!   - paddr end ≤ 4 GiB (else E_LDR-03)

use riscv_sim_contract::error::{ErrorFamily, PlatformError};

use super::{LoadImage, ParseLimits, BUS_SIZE};

const FAMILY: ErrorFamily = ErrorFamily::Loader;

/// One ASCII record byte is 2 hex chars. So the on-wire byte count is
/// `(LL * 2)` for the data section plus 8 (the leading `LLAAAATTCC`
/// header + tail).
const HEX_RECORD_HEADER_HEX: usize = 8;
const HEX_RECORD_MAX_DATA: usize = 0xFF;

/// Parsed Intel HEX payload.
pub fn parse(text: &str, limits: &ParseLimits) -> Result<LoadImage, PlatformError> {
    if text.len() > limits.elf_size {
        return Err(PlatformError::new(
            FAMILY,
            2,
            format!(
                "HEX payload {} exceeds parseLimit.elfSize {}",
                text.len(),
                limits.elf_size
            ),
        ));
    }
    // We accumulate every loadable byte into `extHigh + AAAA` ranges,
    // sorted by base. Adjacent runs merge into a single segment
    // (PROGRESS.md M-06 06-4).
    let mut ext_high: u32 = 0;
    let mut buf: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut line_no: usize = 0;
    for line in text.lines() {
        line_no += 1;
        let trimmed = line.trim_end_matches('\r').trim();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.starts_with(':') {
            return Err(PlatformError::new(
                FAMILY,
                1,
                format!("line {}: missing leading ':' ({:?})", line_no, trimmed),
            ));
        }
        let body = &trimmed[1..];
        // Length: LL (1 byte = 2 hex) + AAAA (2 bytes = 4 hex) + TT (1 byte = 2 hex)
        //   + DD × LL (LL × 2 hex) + CC (1 byte = 2 hex) = (1+2+1+LL+1) bytes = LL + 5 bytes
        // ASCII length = (LL + 5) * 2.
        if body.len() % 2 != 0 {
            return Err(PlatformError::new(
                FAMILY,
                1,
                format!("line {}: odd hex length {}", line_no, body.len()),
            ));
        }
        let total_bytes = body.len() / 2;
        if total_bytes < HEX_RECORD_HEADER_HEX / 2 {
            return Err(PlatformError::new(
                FAMILY,
                1,
                format!("line {}: header shorter than minimum", line_no),
            ));
        }
        let bytes = decode_hex(body).ok_or_else(|| {
            PlatformError::new(FAMILY, 1, format!("line {}: non-hex character", line_no))
        })?;
        // Equivalent to body.len() but pulled from already-parsed bytes.
        let count = bytes[0] as usize;
        if count > HEX_RECORD_MAX_DATA {
            return Err(PlatformError::new(
                FAMILY,
                2,
                format!("line {}: record byte count {} exceeds FF", line_no, count),
            ));
        }
        // 5 = LL + AAAA + TT + CC, so body = LL AAAA TT DD CC means `bytes`
        // has length 1+2+1+count+1 = count+5.
        if bytes.len() != count + 5 {
            return Err(PlatformError::new(
                FAMILY,
                1,
                format!(
                    "line {}: declared count {} != actual payload {}",
                    line_no,
                    count,
                    // `bytes` holds LL+AAAA+TT+DD*+CC; a short record can
                    // be shorter than that 5-byte header+tail, so the
                    // payload length must be computed with a saturating
                    // subtraction. A plain `bytes.len() - 5` underflows
                    // (and panics in debug builds) for a 4-byte record —
                    // the D-03 blocking defect.
                    bytes.len().saturating_sub(5)
                ),
            ));
        }
        // Checksum: lower 8 bits of (sum(bytes) + 0) must equal 0.
        let mut sum: u8 = 0;
        for b in &bytes {
            sum = sum.wrapping_add(*b);
        }
        if sum != 0 {
            return Err(PlatformError::new(
                FAMILY,
                1,
                format!("line {}: checksum mismatch (sum = 0x{:02x})", line_no, sum),
            ));
        }
        let addr_field = u16::from_be_bytes([bytes[1], bytes[2]]) as u32;
        let rec_type = bytes[3];
        let payload = &bytes[4..4 + count];

        match rec_type {
            0x00 => {
                let base = ext_high.checked_add(addr_field).ok_or_else(|| {
                    PlatformError::new(FAMILY, 3, "extended+local address overflow")
                })?;
                let bytes_len = count as u32;
                let end = base
                    .checked_add(bytes_len)
                    .ok_or_else(|| PlatformError::new(FAMILY, 3, "data range end overflows u32"))?;
                if (end as u64) > BUS_SIZE {
                    return Err(PlatformError::new(
                        FAMILY,
                        3,
                        format!("data at 0x{:08x}..0x{:08x} overruns 4 GiB bus", base, end),
                    ));
                }
                append_or_merge(&mut buf, base, payload);
            }
            0x01 => {
                // EOF — return early with the segments we have.
                return Ok(finalise(buf, limits));
            }
            0x02 => {
                // Extended Segment Address: 2 bytes, applied as `<< 4`.
                if count != 2 {
                    return Err(PlatformError::new(
                        FAMILY,
                        1,
                        "0x02 record must carry 2 bytes",
                    ));
                }
                let upper = u16::from_be_bytes([payload[0], payload[1]]);
                ext_high = (upper as u32) << 4;
            }
            0x04 => {
                // Extended Linear Address: 2 bytes, applied as `<< 16`.
                if count != 2 {
                    return Err(PlatformError::new(
                        FAMILY,
                        1,
                        "0x04 record must carry 2 bytes",
                    ));
                }
                let upper = u16::from_be_bytes([payload[0], payload[1]]);
                ext_high = (upper as u32) << 16;
            }
            0x03 | 0x05 => {
                // Start address records — informational only.
            }
            other => {
                return Err(PlatformError::new(
                    FAMILY,
                    1,
                    format!("line {}: unknown record type 0x{:02x}", line_no, other),
                ));
            }
        }
        if buf.len() > limits.segment_count as usize {
            return Err(PlatformError::new(
                FAMILY,
                2,
                format!(
                    "segment count {} exceeds parseLimit.segmentCount",
                    buf.len()
                ),
            ));
        }
    }
    // No EOF record → malformed.
    Err(PlatformError::new(
        FAMILY,
        1,
        "missing EOF (:00000001FF) record",
    ))
}

/// Finalise the merged buffer. Two rules apply (06-4):
/// 1. Same-base rows → last byte wins (later record overwrites).
/// 2. Adjacent rows where `prev.end == next.base` → fold into one row
///    (saves M-14 segment slots).
///
/// Overlapping but distinct rows are NOT merged — M-14 catches them
/// as E_BUS-03 collisions and the host surfaces a clear error.
fn finalise(mut buf: Vec<(u32, Vec<u8>)>, limits: &ParseLimits) -> LoadImage {
    buf.sort_by_key(|(b, _)| *b);

    // Step 1: collapse same-base duplicates (last wins).
    let mut collapsed: Vec<(u32, Vec<u8>)> = Vec::new();
    for (base, data) in buf {
        if let Some((last_base, last_data)) = collapsed.last_mut() {
            if *last_base == base {
                if last_data.len() < data.len() {
                    last_data.resize(data.len(), 0);
                }
                let n = data.len().min(last_data.len());
                last_data[..n].copy_from_slice(&data[..n]);
                continue;
            }
        }
        collapsed.push((base, data));
    }

    // Step 2: fold adjacent runs (prev.end == curr.base).
    let mut final_segments: Vec<(u32, Vec<u8>)> = Vec::new();
    for (base, mut data) in collapsed {
        if let Some((prev_base, prev_data)) = final_segments.last_mut() {
            if prev_base.wrapping_add(prev_data.len() as u32) == base {
                prev_data.append(&mut data);
                continue;
            }
        }
        final_segments.push((base, data));
    }

    if final_segments.len() > limits.segment_count as usize {
        final_segments.truncate(limits.segment_count as usize);
    }
    LoadImage {
        segments: final_segments,
        entry: 0,
    }
}

/// Append `bytes` at `base` into `buf`, merging with the previous
/// `(base, data)` row when the previous run ends exactly at `base`
/// (06-4).
fn append_or_merge(buf: &mut Vec<(u32, Vec<u8>)>, base: u32, bytes: &[u8]) {
    if let Some((last_base, last_data)) = buf.last_mut() {
        if *last_base == base {
            // Same exact base — last-wins overwrite (data len capped).
            if last_data.len() < bytes.len() {
                last_data.resize(bytes.len(), 0);
            }
            let n = bytes.len().min(last_data.len());
            last_data[..n].copy_from_slice(&bytes[..n]);
            return;
        }
        if last_base.wrapping_add(last_data.len() as u32) == base {
            last_data.extend_from_slice(bytes);
            return;
        }
    }
    buf.push((base, bytes.to_vec()));
}

/// Decode an even-length ASCII hex string into raw bytes.
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_nibble(bytes[i])?;
        let lo = hex_nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
