//! M-06 unit tests (LLD §7 + M-06 §7).
//!
//! Coverage:
//!   - ELF32 LE RV32 happy path (1 PT_LOAD, .text with a single RV32 ADDI)
//!   - ELF bss gap (`p_memsz > p_filesz`) produces a zero-filled
//!     secondary segment
//!   - ELF rejection paths: bad magic, bad class, oversize file,
//!     segment-count overflow, address overflow
//!   - Raw parser happy + out-of-bus
//!   - HEX parser: simple, extended linear address, EOF, last-wins
//!     overwrite, checksum mismatch, malformed lines
//!   - Loader trait dispatch + Detect_kind helper

use super::*;

// ---------------------------------------------------------------------------
// Helpers: build a minimal ELF32 LE RV32 ET_EXEC in memory.
// ---------------------------------------------------------------------------

/// Build a 1-instruction RV32 ELF (`addi x1, x0, 42`) whose
/// `.text` lives at paddr = 0, optionally extended with a `.bss`
/// gap (`p_memsz > p_filesz`).
fn make_minimal_elf(p_filesz: u32, p_memsz: u32) -> Vec<u8> {
    assert!(p_filesz <= p_memsz);
    let mut bytes = Vec::new();
    // ----- ELF header (52 bytes) -----
    bytes.extend_from_slice(&[
        0x7F, b'E', b'L', b'F', // magic (4 bytes)
        1,    // EI_CLASS = ELFCLASS32 (1 byte)
        1,    // EI_DATA  = ELFDATA2LSB (1 byte)
        1,    // EI_VERSION = EV_CURRENT (1 byte)
        0,    // EI_OSABI (1 byte)
        0, 0, 0, 0, 0, 0, 0, 0, // EI_ABIVERSION + padding (8 bytes)
        2, 0, // e_type = ET_EXEC
        0xF3, 0x00, // e_machine = EM_RISCV
        1, 0, 0, 0, // e_version
        0, 0, 0, 0, // e_entry = 0
        52, 0, 0, 0, // e_phoff = 52
        0, 0, 0, 0, // e_shoff
        0, 0, 0, 0, // e_flags
        52, 0, // e_ehsize
        32, 0, // e_phentsize
        1, 0, // e_phnum
        0, 0, // e_shentsize
        0, 0, // e_shnum
        0, 0, // e_shstrndx
    ]);
    assert_eq!(
        bytes.len(),
        52,
        "ELF32 header must be exactly 52 bytes, got {}",
        bytes.len()
    );

    // ----- Program header (32 bytes) -----
    let p_offset = (52 + 32) as u32; // text starts right after phdr
    bytes.extend_from_slice(&[
        1, 0, 0, 0, // p_type = PT_LOAD
    ]);
    bytes.extend_from_slice(&p_offset.to_le_bytes()); // p_offset
    bytes.extend_from_slice(&0u32.to_le_bytes()); // p_vaddr
    bytes.extend_from_slice(&0u32.to_le_bytes()); // p_paddr
    bytes.extend_from_slice(&p_filesz.to_le_bytes()); // p_filesz
    bytes.extend_from_slice(&p_memsz.to_le_bytes()); // p_memsz
    bytes.extend_from_slice(&5u32.to_le_bytes()); // p_flags = PF_R|PF_X
    bytes.extend_from_slice(&4u32.to_le_bytes()); // p_align
    assert_eq!(bytes.len(), 52 + 32);

    // ----- .text data (one RV32 ADDI x1, x0, 42) -----
    let instr = 0x02A0_0093u32;
    bytes.extend_from_slice(&instr.to_le_bytes());
    // Pad to p_filesz if larger.
    while (bytes.len() - p_offset as usize) < p_filesz as usize {
        bytes.push(0);
    }
    bytes
}

// ---------------------------------------------------------------------------
// ELF parser tests
// ---------------------------------------------------------------------------

#[test]
fn elf_parses_minimal_elf32_le_rv32() {
    let elf = make_minimal_elf(4, 4);
    let loader = DefaultLoader;
    let li = loader.load(&elf, Kind::Elf).expect("elf parse");
    assert_eq!(li.entry, 0);
    assert_eq!(li.segments.len(), 1);
    assert_eq!(li.segments[0].0, 0);
    assert_eq!(li.segments[0].1.len(), 4);
    // First 4 bytes are the ADDI encoding we planted (little-endian 0x02A0_0093).
    assert_eq!(li.segments[0].1, vec![0x93, 0x00, 0xA0, 0x02]);
}

#[test]
fn elf_splits_bss_gap_into_zero_filled_segment() {
    // 4-byte file, 12-byte memsz → gap from 0x0004..0x000C = 8 bytes zero.
    let elf = make_minimal_elf(4, 12);
    let li = DefaultLoader.load(&elf, Kind::Elf).expect("elf parse");
    assert_eq!(li.segments.len(), 2);
    assert_eq!(li.segments[0].0, 0);
    assert_eq!(li.segments[0].1, vec![0x93, 0x00, 0xA0, 0x02]);
    assert_eq!(li.segments[1].0, 0x4);
    assert_eq!(li.segments[1].1.len(), 8);
    assert!(li.segments[1].1.iter().all(|b| *b == 0));
}

#[test]
fn elf_rejects_bad_magic() {
    let mut elf = make_minimal_elf(4, 4);
    elf[1] = b'X'; // poison magic
    let err = DefaultLoader.load(&elf, Kind::Elf).unwrap_err();
    assert_eq!(err.family, ErrorFamily::Loader);
    assert_eq!(err.scenario, 1, "E_LDR-01 malformed");
    assert!(err.message.contains("magic"));
}

#[test]
fn elf_rejects_non_rv32_machine() {
    let mut elf = make_minimal_elf(4, 4);
    // e_machine lives at offset 18..20.
    elf[18] = 0x03; // EM_386
    elf[19] = 0x00;
    let err = DefaultLoader.load(&elf, Kind::Elf).unwrap_err();
    assert_eq!(err.scenario, 1);
    assert!(err.message.contains("machine"));
}

#[test]
fn elf_rejects_e_phnum_overflow() {
    let mut elf = make_minimal_elf(4, 4);
    // e_phnum at offset 44..46.
    elf[44] = 0xFF;
    elf[45] = 0x00; // 255
    let limits = ParseLimits {
        segment_count: 64,
        ..Default::default()
    };
    let err = DefaultLoader
        .load_with_limits(&elf, Kind::Elf, &limits)
        .unwrap_err();
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
}

#[test]
fn elf_rejects_size_over_parse_limit() {
    let limits = ParseLimits {
        elf_size: 16,
        ..ParseLimits::default()
    };
    // We built an 88-byte ELF, so the size check trips first.
    let elf = make_minimal_elf(4, 4);
    let err = DefaultLoader
        .load_with_limits(&elf, Kind::Elf, &limits)
        .unwrap_err();
    assert_eq!(err.scenario, 2, "E_LDR-02 size > limit");
}

#[test]
fn elf_rejects_truncated_header() {
    let err = DefaultLoader.load(&[0u8; 10], Kind::Elf).unwrap_err();
    assert_eq!(err.scenario, 1, "truncated header -> LDR-01");
}

// ---------------------------------------------------------------------------
// Raw loader tests
// ---------------------------------------------------------------------------

#[test]
fn raw_parser_round_trip_with_base() {
    let payload = vec![1u8, 2, 3, 4, 5];
    let li = parse_raw(&payload, 0x4000_0000, &ParseLimits::default()).unwrap();
    assert_eq!(li.entry, 0);
    assert_eq!(li.segments, vec![(0x4000_0000, payload.clone())]);
}

#[test]
fn raw_parser_rejects_paddr_out_of_bus() {
    // base = 0xFFFF_FFF0 + length 32 → end crosses 4 GiB.
    let payload = vec![0u8; 32];
    let err = parse_raw(&payload, 0xFFFF_FFF0, &ParseLimits::default()).unwrap_err();
    assert_eq!(err.scenario, 3, "out-of-bus -> LDR-03");
}

// ---------------------------------------------------------------------------
// HEX parser tests
// ---------------------------------------------------------------------------

/// Build a single IHEX `:LLAAAATTDD…CC` line followed by `\n`. The
/// caller supplies the raw bytes; we wrap them with length + address
/// + type + checksum.
fn hex_line(addr: u16, rec_type: u8, data: &[u8]) -> String {
    let mut body = Vec::new();
    body.push(data.len() as u8);
    body.extend_from_slice(&addr.to_be_bytes());
    body.push(rec_type);
    body.extend_from_slice(data);
    let sum: u8 = body.iter().fold(0u8, |acc, b| acc.wrapping_add(*b));
    let cc: u8 = (0u8).wrapping_sub(sum); // two's complement
    body.push(cc);
    let ascii: String = body.iter().map(|b| format!("{:02X}", b)).collect();
    format!(":{}\n", ascii)
}

#[test]
fn hex_parses_two_disjoint_data_records_and_eof() {
    // Two data records at non-adjacent addresses (0x0000 and 0x0010),
    // so the segment-adjacency merge keeps them separate.
    let text = format!(
        "{}{}{}",
        hex_line(0x0000, 0x00, &[0x93, 0x00, 0xA0, 0x02]),
        hex_line(0x0010, 0x00, &[0x13, 0x01, 0x01, 0x00]),
        hex_line(0x0000, 0x01, &[]),
    );
    let li = DefaultLoader.load(text.as_bytes(), Kind::Hex).expect("hex");
    assert_eq!(li.entry, 0);
    assert_eq!(li.segments.len(), 2);
    assert_eq!(li.segments[0].0, 0x0000);
    assert_eq!(li.segments[0].1, vec![0x93, 0x00, 0xA0, 0x02]);
    assert_eq!(li.segments[1].0, 0x0010);
    assert_eq!(li.segments[1].1, vec![0x13, 0x01, 0x01, 0x00]);
}

#[test]
fn hex_adjacent_records_merge_into_single_segment() {
    // First record spans 0..4, second spans 4..8 — they touch, so
    // the final merge folds them into a single 8-byte segment.
    let text = format!(
        "{}{}{}",
        hex_line(0x0000, 0x00, &[0x93, 0x00, 0xA0, 0x02]),
        hex_line(0x0004, 0x00, &[0x13, 0x01, 0x01, 0x00]),
        hex_line(0x0000, 0x01, &[]),
    );
    let li = DefaultLoader.load(text.as_bytes(), Kind::Hex).expect("hex");
    assert_eq!(li.segments.len(), 1);
    assert_eq!(li.segments[0].0, 0);
    assert_eq!(
        li.segments[0].1,
        vec![0x93, 0x00, 0xA0, 0x02, 0x13, 0x01, 0x01, 0x00]
    );
}

#[test]
fn hex_extended_linear_address_applied_to_records() {
    let text = format!(
        "{}{}{}",
        // ELA = 0x0001, so extHigh = 0x0001 << 16 = 0x0001_0000.
        // Payload is big-endian: [0x00, 0x01] → upper16 = 0x0001.
        hex_line(0x0000, 0x04, &[0x00, 0x01]),
        // Data record at local 0x0000 → real base = 0x0001_0000.
        hex_line(0x0000, 0x00, &[0xDE, 0xAD, 0xBE, 0xEF]),
        hex_line(0x0000, 0x01, &[]),
    );
    let li = DefaultLoader.load(text.as_bytes(), Kind::Hex).expect("hex");
    assert_eq!(
        li.segments,
        vec![(0x0001_0000, vec![0xDE, 0xAD, 0xBE, 0xEF])]
    );
}

#[test]
fn hex_last_wins_overwrites_same_base() {
    // Two data records both at local 0x0000 — later bytes win.
    let text = format!(
        "{}{}{}",
        hex_line(0x0000, 0x00, &[0x00, 0x00, 0x00, 0x00]),
        hex_line(0x0000, 0x00, &[0x11, 0x22, 0x33]),
        hex_line(0x0000, 0x01, &[]),
    );
    let li = DefaultLoader.load(text.as_bytes(), Kind::Hex).expect("hex");
    // Final row: [0x11, 0x22, 0x33, 0x00] — last-wins preserves the
    // earlier 4-byte row's last (zero) byte untouched by the 3-byte
    // overwrite.
    assert_eq!(li.segments, vec![(0x0000, vec![0x11, 0x22, 0x33, 0x00])]);
}

#[test]
fn hex_rejects_checksum_mismatch() {
    // Hand-craft a bad-checksum line: write the payload at 0x0000
    // but flip the checksum nibble.
    let mut body = vec![0x04u8, 0x00, 0x00, 0x00, 0xAA, 0xBB, 0xCC, 0xDD];
    let sum: u8 = body.iter().fold(0u8, |acc, b| acc.wrapping_add(*b));
    let cc: u8 = (0u8).wrapping_sub(sum).wrapping_add(1); // intentionally wrong
    body.push(cc);
    let ascii: String = body.iter().map(|b| format!("{:02X}", b)).collect();
    let text = format!(":{}\n:00000001FF", ascii);
    let err = DefaultLoader.load(text.as_bytes(), Kind::Hex).unwrap_err();
    assert_eq!(err.scenario, 1, "checksum mismatch -> LDR-01");
}

#[test]
fn hex_rejects_missing_eof() {
    let bad = hex_line(0x0000, 0x00, &[0xAA, 0xBB]);
    let err = DefaultLoader.load(bad.as_bytes(), Kind::Hex).unwrap_err();
    assert_eq!(err.scenario, 1);
    assert!(err.message.contains("EOF"));
}

// ---------------------------------------------------------------------------
// D-03 regression: a short IHEX record must be rejected, not panic.
//
// The declared byte count and the actual payload length disagree, and the
// record is shorter than the 5-byte header+tail (LL+AAAA+TT+CC). The
// diagnostic used to compute `bytes.len() - 5`, which underflowed for a
// 4-byte record → `panic!("attempt to subtract with overflow")`. In WASM
// that is a worker crash, so the whole "any malformed input must be an
// `Err`, never a panic" rule is now pinned by a test.
// ---------------------------------------------------------------------------

#[test]
fn hex_rejects_short_record_without_panicking() {
    // Body "01234567" → 4 decoded bytes: LL=0x01, AAAA=0x2345, TT=0x67.
    // Declared count (1) + 5 != 4 → rejected.
    let err = DefaultLoader
        .load(b":01234567\n:00000001FF\n", Kind::Hex)
        .unwrap_err();
    assert_eq!(err.family, ErrorFamily::Loader);
    assert_eq!(err.scenario, 1, "E_LDR-01 malformed");
    assert!(
        err.message.contains("declared count"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn hex_rejects_unknown_record_type() {
    // Build a line manually with rec_type = 0x07 (unknown).
    let body = vec![0x00u8, 0x00, 0x00, 0x07];
    let sum: u8 = body.iter().fold(0u8, |acc, b| acc.wrapping_add(*b));
    let cc: u8 = (0u8).wrapping_sub(sum);
    let mut body = body;
    body.push(cc);
    let ascii: String = body.iter().map(|b| format!("{:02X}", b)).collect();
    let bad_line = format!(":{}\n:00000001FF", ascii);
    let err = DefaultLoader
        .load(bad_line.as_bytes(), Kind::Hex)
        .unwrap_err();
    assert_eq!(err.scenario, 1);
    assert!(err.message.contains("unknown record type"));
}

// ---------------------------------------------------------------------------
// Loader trait dispatch + detect_kind helper
// ---------------------------------------------------------------------------

#[test]
fn load_with_kind_routes_correctly() {
    let elf = make_minimal_elf(4, 4);
    let text = hex_line(0x0000, 0x00, &[0xAA]) + &hex_line(0x0000, 0x01, &[]);
    let raw = vec![1u8, 2, 3];

    let l: &dyn Loader = &DefaultLoader;
    assert_eq!(l.load(&elf, Kind::Elf).unwrap().segments.len(), 1);
    assert!(l.load(text.as_bytes(), Kind::Hex).is_ok());
    assert_eq!(
        l.load(&raw, Kind::RawHex).unwrap().segments,
        vec![(0, raw.clone())]
    );
}

#[test]
fn detect_kind_classifies_extensions() {
    assert_eq!(detect_kind("elf"), Kind::Elf);
    assert_eq!(detect_kind("ELF"), Kind::Elf);
    assert_eq!(detect_kind("out"), Kind::Elf);
    assert_eq!(detect_kind("hex"), Kind::Hex);
    assert_eq!(detect_kind("iHex"), Kind::Hex);
    assert_eq!(detect_kind("bin"), Kind::RawHex);
    assert_eq!(detect_kind("garbage"), Kind::Elf); // default
}
