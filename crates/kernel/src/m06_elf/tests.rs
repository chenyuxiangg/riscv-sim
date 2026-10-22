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
    make_minimal_elf_at(0, p_filesz, p_memsz)
}

/// The same, but with the segment placed at an explicit physical
/// address — used to exercise the address-space boundaries.
fn make_minimal_elf_at(p_paddr: u32, p_filesz: u32, p_memsz: u32) -> Vec<u8> {
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
    bytes.extend_from_slice(&p_paddr.to_le_bytes()); // p_vaddr
    bytes.extend_from_slice(&p_paddr.to_le_bytes()); // p_paddr
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
// D-15 regression: p_filesz must not exceed p_memsz.
// ---------------------------------------------------------------------------

#[test]
fn elf_rejects_filesz_greater_than_memsz() {
    let mut elf = make_minimal_elf(4, 4);
    // p_memsz lives at program-header offset 20; the program header
    // starts at file offset 52 → 52 + 20 = 72.
    elf[72..76].copy_from_slice(&0u32.to_le_bytes());
    let err = DefaultLoader.load(&elf, Kind::Elf).unwrap_err();
    assert_eq!(err.family, ErrorFamily::Loader);
    assert_eq!(err.scenario, 1, "E_LDR-01 malformed");
    assert!(
        err.message.contains("p_filesz"),
        "unexpected message: {}",
        err.message
    );
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
// D-08 regression: a HEX image whose segment count exceeds the budget must
// be rejected, never silently truncated. A truncated image looks valid to
// the host and would run with firmware bytes missing.
// ---------------------------------------------------------------------------

#[test]
fn hex_rejects_image_exceeding_segment_budget() {
    // Three disjoint data records + EOF. With a budget of 2 segments the
    // image must be rejected instead of being cut down to 2 segments.
    let text = format!(
        "{}{}{}{}",
        hex_line(0x0000, 0x00, &[0xAA]),
        hex_line(0x1000, 0x00, &[0xBB]),
        hex_line(0x2000, 0x00, &[0xCC]),
        hex_line(0x0000, 0x01, &[]),
    );
    let limits = ParseLimits {
        segment_count: 2,
        ..ParseLimits::default()
    };
    let err = DefaultLoader
        .load_with_limits(text.as_bytes(), Kind::Hex, &limits)
        .unwrap_err();
    assert_eq!(err.family, ErrorFamily::Loader);
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
    assert!(
        err.message.contains("segmentCount"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn hex_ignores_zero_length_data_records() {
    // A data record with a zero payload contributes no bytes. It must
    // not become a zero-length segment: M-14 would map a phantom RAM
    // region the firmware never contained.
    let text = format!(
        "{}{}{}",
        hex_line(0x0000, 0x00, &[]),
        hex_line(0x0000, 0x00, &[0xAA, 0xBB]),
        hex_line(0x0000, 0x01, &[]),
    );
    let li = DefaultLoader.load(text.as_bytes(), Kind::Hex).expect("hex");
    assert_eq!(li.segments.len(), 1);
    assert_eq!(li.segments[0].0, 0x0000);
    assert_eq!(li.segments[0].1, vec![0xAA, 0xBB]);
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
    // D-21: an unknown extension must NOT be assumed to be ELF. Guessing
    // ELF made every extension-less raw file fail with "bad ELF magic".
    assert_eq!(detect_kind("garbage"), Kind::RawHex);
    assert_eq!(detect_kind(""), Kind::RawHex);
}

// ---------------------------------------------------------------------------
// D-21: magic-byte sniffing.
// ---------------------------------------------------------------------------

#[test]
fn detect_kind_from_bytes_sniffs_elf_magic() {
    let elf = make_minimal_elf(4, 4);
    assert_eq!(detect_kind_from_bytes(&elf), Some(Kind::Elf));
}

#[test]
fn detect_kind_from_bytes_sniffs_ihex_record_marker() {
    let text = hex_line(0x0000, 0x00, &[0xAA]) + &hex_line(0x0000, 0x01, &[]);
    assert_eq!(detect_kind_from_bytes(text.as_bytes()), Some(Kind::Hex));
}

#[test]
fn detect_kind_from_bytes_is_inconclusive_for_raw_blobs() {
    assert_eq!(detect_kind_from_bytes(&[0x00, 0x01, 0x02, 0x03]), None);
    assert_eq!(detect_kind_from_bytes(&[]), None);
}

/// A raw binary image may legitimately start with `0x3A` (`:`) — the
/// record marker on its own must not turn a raw image into HEX
/// (review V3 finding P1).
#[test]
fn a_raw_blob_starting_with_the_record_marker_is_not_sniffed_as_ihex() {
    // A blob that starts with ':' but is plainly binary.
    let blob = [
        0x3Au8, 0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA, 0xF9, 0xF8, 0xF7, 0xF6, 0xF5,
    ];
    assert_eq!(
        detect_kind_from_bytes(&blob),
        None,
        "a raw blob that merely starts with ':' must not be sniffed as HEX"
    );
    assert_eq!(
        detect_kind_with_hint("bin", &blob),
        Kind::RawHex,
        "and it must still load as a raw image"
    );
}

/// A line that is not a well-formed IHEX record must not be sniffed
/// as HEX — the sniff has to be structural, not just "starts with
/// `:`".
#[test]
fn a_malformed_ihex_record_is_not_sniffed_as_ihex() {
    let cases: &[&[u8]] = &[
        // Too short for the fixed header (marker + count + address +
        // type + checksum is 11 characters).
        b":0000001\n",
        // Even length — a record is always `11 + 2 * count` characters.
        b":00000001FF0\n",
        // The declared byte count (0x02) does not match the one data
        // byte the record actually carries.
        b":0200000000FF\n",
        // A non-hex character after the marker.
        b":00000001GG\n",
    ];
    for case in cases {
        assert_eq!(
            detect_kind_from_bytes(case),
            None,
            "{:?} must not be sniffed as HEX",
            String::from_utf8_lossy(case)
        );
    }
}

/// A well-formed record is still recognised — including the
/// 11-character EOF record, the shortest one there is.
#[test]
fn a_well_formed_ihex_record_is_still_sniffed_as_ihex() {
    // The EOF record: marker, count 00, address 0000, type 01, checksum FF.
    assert_eq!(detect_kind_from_bytes(b":00000001FF\n"), Some(Kind::Hex));
    // A data record with one payload byte: 13 characters.
    let text = hex_line(0x0000, 0x00, &[0xAA]);
    assert_eq!(detect_kind_from_bytes(text.as_bytes()), Some(Kind::Hex));
}

// ---------------------------------------------------------------------------
// The 4 GiB bus boundary (review V3 finding L-6).
// ---------------------------------------------------------------------------

/// A segment whose in-memory footprint crosses the 4 GiB bus
/// boundary must be rejected.
///
/// This is the check that stands in front of the `.bss` gap base
/// arithmetic: a segment that ends past the bus would otherwise have
/// its gap base wrapped around a `u32` and materialise a zero-filled
/// segment at the wrong address.
#[test]
fn a_segment_whose_footprint_crosses_the_bus_boundary_is_rejected() {
    // p_paddr + p_memsz = 0xFFFF_FFF0 + 0x20 = 0x1_0000_0010 → past the
    // 4 GiB bus.
    let elf = make_minimal_elf_at(0xFFFF_FFF0, 4, 0x20);
    let err = DefaultLoader
        .load(&elf, Kind::Elf)
        .expect_err("a segment crossing the 4 GiB bus must be rejected");
    assert_eq!(err.family, ErrorFamily::Loader);
    assert_eq!(err.scenario, 3, "out-of-bus -> LDR-03");
}

/// The last segment that still fits must load — the boundary is
/// inclusive, not off by one.
#[test]
fn a_segment_ending_exactly_at_the_bus_boundary_is_accepted() {
    // p_paddr + p_memsz = 0xFFFF_FFE0 + 0x20 = 0x1_0000_0000 → exactly
    // the end of the 4 GiB bus.
    let elf = make_minimal_elf_at(0xFFFF_FFE0, 4, 0x20);
    let li = DefaultLoader
        .load(&elf, Kind::Elf)
        .expect("a segment ending exactly at the bus boundary is legal");
    assert_eq!(li.segments.len(), 2, "the file part plus the .bss gap");
    assert_eq!(li.segments[0].0, 0xFFFF_FFE0);
}

#[test]
fn detect_kind_with_hint_prefers_magic_bytes_over_extension() {
    // A valid ELF named ".hex" is still an ELF.
    let elf = make_minimal_elf(4, 4);
    assert_eq!(detect_kind_with_hint("hex", &elf), Kind::Elf);
    // An extension-less raw blob is raw, not a broken ELF (D-21).
    let raw = vec![0x00u8, 0x01, 0x02, 0x03];
    assert_eq!(detect_kind_with_hint("", &raw), Kind::RawHex);
    assert_eq!(detect_kind_with_hint("garbage", &raw), Kind::RawHex);
}

#[test]
fn detect_kind_with_hint_honours_a_specific_extension_when_magic_is_inconclusive() {
    // The magic does not check out, but the extension names a format:
    // the parser must get the chance to report a precise error rather
    // than the file being silently loaded as a raw blob.
    let raw = vec![0x00u8, 0x01, 0x02, 0x03];
    assert_eq!(detect_kind_with_hint("elf", &raw), Kind::Elf);
    assert_eq!(detect_kind_with_hint("hex", &raw), Kind::Hex);
    // ... and a raw blob named ".elf" really does fail with a clear
    // "bad ELF magic" instead of being loaded as raw. The blob is long
    // enough to pass the header-length check so that the magic check is
    // the one that fires.
    let raw = vec![0x00u8; 64];
    let err = DefaultLoader
        .load(&raw, detect_kind_with_hint("elf", &raw))
        .unwrap_err();
    assert_eq!(err.scenario, 1);
    assert!(err.message.contains("magic"), "unexpected: {}", err.message);
}

// ---------------------------------------------------------------------------
// D-22: the caller-supplied limits must actually be enforced.
// ---------------------------------------------------------------------------

#[test]
fn load_with_limits_enforces_the_elf_size_limit() {
    let elf = make_minimal_elf(4, 4);
    let limits = ParseLimits {
        elf_size: 16,
        ..ParseLimits::default()
    };
    let err = DefaultLoader
        .load_with_limits(&elf, Kind::Elf, &limits)
        .unwrap_err();
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
    assert!(
        err.message.contains("elfSize"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn load_with_limits_enforces_the_raw_size_limit() {
    let raw = vec![0u8; 32];
    let limits = ParseLimits {
        raw_size: 16,
        ..ParseLimits::default()
    };
    let err = DefaultLoader
        .load_with_limits(&raw, Kind::RawHex, &limits)
        .unwrap_err();
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
    assert!(
        err.message.contains("rawSize"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn load_with_limits_enforces_the_hex_size_limit() {
    let text = hex_line(0x0000, 0x00, &[0xAA]) + &hex_line(0x0000, 0x01, &[]);
    let limits = ParseLimits {
        hex_size: 4,
        ..ParseLimits::default()
    };
    let err = DefaultLoader
        .load_with_limits(text.as_bytes(), Kind::Hex, &limits)
        .unwrap_err();
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
    assert!(
        err.message.contains("hexSize"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn load_with_limits_enforces_the_segment_count_limit() {
    let elf = make_minimal_elf(4, 4);
    let limits = ParseLimits {
        segment_count: 0,
        ..ParseLimits::default()
    };
    let err = DefaultLoader
        .load_with_limits(&elf, Kind::Elf, &limits)
        .unwrap_err();
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
}

#[test]
fn empty_raw_image_is_rejected() {
    let err = DefaultLoader.load(&[], Kind::RawHex).unwrap_err();
    assert_eq!(err.scenario, 1, "E_LDR-01 malformed");
    assert!(err.message.contains("empty"), "unexpected: {}", err.message);
}

// ---------------------------------------------------------------------------
// D-10 (M-06 side): the image budget must bound what the loader
// materialises. A tiny file can declare a huge image — the `.bss` tail
// of a PT_LOAD is not stored in the file at all.
// ---------------------------------------------------------------------------

#[test]
fn elf_rejects_segment_larger_than_the_region_budget() {
    // An 88-byte ELF whose single PT_LOAD declares a 1 MiB `.bss` tail.
    let elf = make_minimal_elf(4, 1024 * 1024);
    let limits = ParseLimits {
        max_region_bytes: 4096,
        ..ParseLimits::default()
    };
    let err = DefaultLoader
        .load_with_limits(&elf, Kind::Elf, &limits)
        .unwrap_err();
    assert_eq!(err.family, ErrorFamily::Loader);
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
    assert!(
        err.message.contains("maxRegionBytes"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn elf_rejects_image_larger_than_the_total_budget() {
    // Two PT_LOADs of 4 KiB each — each is within the per-region
    // budget, but together they exceed the total image budget.
    let elf = make_two_segment_elf(4096, 4096);
    let limits = ParseLimits {
        max_region_bytes: 8192,
        total_image_bytes: 4096,
        ..ParseLimits::default()
    };
    let err = DefaultLoader
        .load_with_limits(&elf, Kind::Elf, &limits)
        .unwrap_err();
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
    assert!(
        err.message.contains("totalImageBytes"),
        "unexpected message: {}",
        err.message
    );
}

/// Build an ELF32 LE RV32 ET_EXEC with exactly two PT_LOAD program
/// headers, each declaring `memsz` bytes of memory at a distinct
/// paddr. The program-header table sits right after the ELF header and
/// the shared `.text` follows it.
fn make_two_segment_elf(memsz1: u32, memsz2: u32) -> Vec<u8> {
    let phnum = 2u16;
    let text_offset = 52 + phnum as usize * 32;
    let mut bytes = Vec::new();
    // ----- ELF header (52 bytes) -----
    bytes.extend_from_slice(&[
        0x7F, b'E', b'L', b'F', // magic
        1,    // EI_CLASS = ELFCLASS32
        1,    // EI_DATA  = ELFDATA2LSB
        1,    // EI_VERSION = EV_CURRENT
        0,    // EI_OSABI
        0, 0, 0, 0, 0, 0, 0, 0, // EI_ABIVERSION + padding
        2, 0, // e_type = ET_EXEC
        0xF3, 0x00, // e_machine = EM_RISCV
        1, 0, 0, 0, // e_version
        0, 0, 0, 0, // e_entry
        52, 0, 0, 0, // e_phoff
        0, 0, 0, 0, // e_shoff
        0, 0, 0, 0, // e_flags
        52, 0, // e_ehsize
        32, 0, // e_phentsize
        0, 0, // e_phnum (patched below)
        0, 0, // e_shentsize
        0, 0, // e_shnum
        0, 0, // e_shstrndx
    ]);
    bytes[44..46].copy_from_slice(&phnum.to_le_bytes());

    // ----- Two program headers -----
    for (paddr, memsz) in [(0u32, memsz1), (0x1000u32, memsz2)] {
        bytes.extend_from_slice(&1u32.to_le_bytes()); // p_type = PT_LOAD
        bytes.extend_from_slice(&(text_offset as u32).to_le_bytes()); // p_offset
        bytes.extend_from_slice(&paddr.to_le_bytes()); // p_vaddr
        bytes.extend_from_slice(&paddr.to_le_bytes()); // p_paddr
        bytes.extend_from_slice(&4u32.to_le_bytes()); // p_filesz
        bytes.extend_from_slice(&memsz.to_le_bytes()); // p_memsz
        bytes.extend_from_slice(&5u32.to_le_bytes()); // p_flags
        bytes.extend_from_slice(&4u32.to_le_bytes()); // p_align
    }
    assert_eq!(bytes.len(), text_offset);

    // ----- .text (one RV32 ADDI x1, x0, 42) -----
    bytes.extend_from_slice(&0x02A0_0093u32.to_le_bytes());
    bytes
}

#[test]
fn hex_rejects_image_larger_than_the_total_budget() {
    let text = format!(
        "{}{}",
        hex_line(0x0000, 0x00, &[0xAA; 16]),
        hex_line(0x0000, 0x01, &[]),
    );
    let limits = ParseLimits {
        total_image_bytes: 8,
        ..ParseLimits::default()
    };
    let err = DefaultLoader
        .load_with_limits(text.as_bytes(), Kind::Hex, &limits)
        .unwrap_err();
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
    assert!(
        err.message.contains("totalImageBytes"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn hex_rejects_merged_segment_larger_than_the_region_budget() {
    // Two adjacent 8-byte records merge into one 16-byte segment. The
    // per-record check cannot see that, the merged-segment check can.
    let text = format!(
        "{}{}{}",
        hex_line(0x0000, 0x00, &[0xAA; 8]),
        hex_line(0x0008, 0x00, &[0xBB; 8]),
        hex_line(0x0000, 0x01, &[]),
    );
    let limits = ParseLimits {
        max_region_bytes: 12,
        ..ParseLimits::default()
    };
    let err = DefaultLoader
        .load_with_limits(text.as_bytes(), Kind::Hex, &limits)
        .unwrap_err();
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
    assert!(
        err.message.contains("maxRegionBytes"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn raw_rejects_blob_larger_than_the_region_budget() {
    let raw = vec![0u8; 64];
    let limits = ParseLimits {
        max_region_bytes: 32,
        ..ParseLimits::default()
    };
    let err = DefaultLoader
        .load_with_limits(&raw, Kind::RawHex, &limits)
        .unwrap_err();
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
    assert!(
        err.message.contains("maxRegionBytes"),
        "unexpected message: {}",
        err.message
    );
}

// ---------------------------------------------------------------------------
// STAGE-5 裁决 B · W1.1 — the section table and the debug-section
// whitelist.
//
// The fixtures below build a **complete ELF32 image in memory** (byte
// arrays, no external toolchain, no external file), so the section-table
// parser is exercised against a hand-written container rather than a
// toolchain artifact.
// ---------------------------------------------------------------------------

/// `sh_type` — ordinary file-backed section.
pub(crate) const SHT_PROGBITS: u32 = 1;
/// `sh_type` — string table.
pub(crate) const SHT_STRTAB: u32 = 3;
/// `sh_type` — the section occupies no space in the file.
pub(crate) const SHT_NOBITS: u32 = 8;

/// One section of the synthetic section table.
pub(crate) struct TestSection {
    /// Section name, as it appears in `.shstrtab`.
    pub(crate) name: &'static str,
    /// `sh_type`.
    pub(crate) sh_type: u32,
    /// File content. Ignored for `SHT_NOBITS`.
    pub(crate) data: Vec<u8>,
    /// `sh_size` override — defaults to `data.len()`.
    pub(crate) size_override: Option<u32>,
    /// `sh_entsize` — defaults to `0`（「未声明」）。
    pub(crate) entsize: u32,
}

impl TestSection {
    /// A file-backed section with the given name and content.
    pub(crate) fn progbits(name: &'static str, data: &[u8]) -> Self {
        Self {
            name,
            sh_type: SHT_PROGBITS,
            data: data.to_vec(),
            size_override: None,
            entsize: 0,
        }
    }

    /// A `SHT_NOBITS` section: it declares `declared_size` bytes but
    /// occupies no space in the file.
    pub(crate) fn nobits(name: &'static str, declared_size: u32) -> Self {
        Self {
            name,
            sh_type: SHT_NOBITS,
            data: Vec::new(),
            size_override: Some(declared_size),
            entsize: 0,
        }
    }

    /// A section that **declares** its entry size (`sh_entsize`).
    ///
    /// `0` means「未声明」in the ELF spec — some old toolchains write it
    /// that way — and is accepted.
    pub(crate) fn with_entsize(mut self, entsize: u32) -> Self {
        self.entsize = entsize;
        self
    }
}

/// The synthetic ELF plus the offsets a test needs in order to mutate
/// it (the malformed-table cases live in `tests_malformed.rs`).
pub(crate) struct TestElf {
    /// The complete file.
    pub(crate) bytes: Vec<u8>,
    /// File offset of the section-header table.
    pub(crate) shoff: u32,
    /// `(file offset, length)` of the `.shstrtab` content.
    pub(crate) shstrtab: (u32, u32),
}

impl TestElf {
    /// File offset of section-header entry `index`.
    pub(crate) fn shdr_offset(&self, index: usize) -> usize {
        self.shoff as usize + index * 40
    }
}

/// Build an ELF32 LE RV32 ET_EXEC with a section table.
///
/// Layout:
///
/// ```text
/// [0, 52)          ELF header
/// [52, 84)         program header (one PT_LOAD covering `.text`)
/// [84, 88)         `.text` payload
/// ...              section payloads, in the order given
/// ...              `.shstrtab` payload
/// [shoff, ...)     section header table:
///                   [0] = SHT_NULL, [1] = `.shstrtab`, [2..] = the
///                   user sections
/// ```
pub(crate) fn build_elf_with_sections(sections: &[TestSection]) -> TestElf {
    const EHDR_SIZE: usize = 52;
    const PHDR_SIZE: usize = 32;
    const SHDR_SIZE: usize = 40;
    const TEXT_OFF: u32 = (EHDR_SIZE + PHDR_SIZE) as u32;
    const TEXT_LEN: u32 = 4;

    // ----- `.shstrtab` payload -----
    // Offset 0 is the empty name (the null section's `sh_name`).
    let mut shstr: Vec<u8> = vec![0u8];
    let mut name_offsets: Vec<u32> = Vec::with_capacity(sections.len());
    for s in sections {
        name_offsets.push(shstr.len() as u32);
        shstr.extend_from_slice(s.name.as_bytes());
        shstr.push(0);
    }
    let shstrtab_name_offset = shstr.len() as u32;
    shstr.extend_from_slice(b".shstrtab");
    shstr.push(0);

    // ----- file body -----
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(&[0u8; EHDR_SIZE]); // ELF header (filled in below)
    bytes.extend_from_slice(&[0u8; PHDR_SIZE]); // program header (filled in below)
    bytes.extend_from_slice(&[0x93, 0x00, 0xA0, 0x02]); // `.text`: ADDI x1, x0, 42

    // Section payloads.
    let mut section_offsets: Vec<u32> = Vec::with_capacity(sections.len());
    for s in sections {
        section_offsets.push(bytes.len() as u32);
        if s.sh_type != SHT_NOBITS {
            bytes.extend_from_slice(&s.data);
        }
    }

    // `.shstrtab` payload.
    let shstrtab_offset = bytes.len() as u32;
    let shstrtab_len = shstr.len() as u32;
    bytes.extend_from_slice(&shstr);

    // ----- section header table -----
    let shnum = 2 + sections.len();
    let shoff = bytes.len() as u32;
    let mut shdrs: Vec<u8> = Vec::with_capacity(shnum * SHDR_SIZE);
    shdrs.extend_from_slice(&[0u8; SHDR_SIZE]); // [0] SHT_NULL
    push_shdr(
        &mut shdrs,
        shstrtab_name_offset,
        SHT_STRTAB,
        shstrtab_offset,
        shstrtab_len,
        0,
    );
    for (i, s) in sections.iter().enumerate() {
        let size = s.size_override.unwrap_or(s.data.len() as u32);
        push_shdr(
            &mut shdrs,
            name_offsets[i],
            s.sh_type,
            section_offsets[i],
            size,
            s.entsize,
        );
    }
    bytes.extend_from_slice(&shdrs);

    // ----- ELF header -----
    bytes[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    bytes[4] = 1; // EI_CLASS = ELFCLASS32
    bytes[5] = 1; // EI_DATA  = ELFDATA2LSB
    bytes[6] = 1; // EI_VERSION = EV_CURRENT
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes()); // e_type = ET_EXEC
    bytes[18..20].copy_from_slice(&0xF3u16.to_le_bytes()); // e_machine = EM_RISCV
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
    bytes[24..28].copy_from_slice(&0u32.to_le_bytes()); // e_entry
    bytes[28..32].copy_from_slice(&(EHDR_SIZE as u32).to_le_bytes()); // e_phoff
    bytes[32..36].copy_from_slice(&shoff.to_le_bytes()); // e_shoff
    bytes[36..40].copy_from_slice(&0u32.to_le_bytes()); // e_flags
    bytes[40..42].copy_from_slice(&(EHDR_SIZE as u16).to_le_bytes()); // e_ehsize
    bytes[42..44].copy_from_slice(&(PHDR_SIZE as u16).to_le_bytes()); // e_phentsize
    bytes[44..46].copy_from_slice(&1u16.to_le_bytes()); // e_phnum
    bytes[46..48].copy_from_slice(&(SHDR_SIZE as u16).to_le_bytes()); // e_shentsize
    bytes[48..50].copy_from_slice(&(shnum as u16).to_le_bytes()); // e_shnum
    bytes[50..52].copy_from_slice(&1u16.to_le_bytes()); // e_shstrndx

    // ----- program header (one PT_LOAD over `.text`) -----
    bytes[52..56].copy_from_slice(&1u32.to_le_bytes()); // p_type = PT_LOAD
    bytes[56..60].copy_from_slice(&TEXT_OFF.to_le_bytes()); // p_offset
    bytes[60..64].copy_from_slice(&0u32.to_le_bytes()); // p_vaddr
    bytes[64..68].copy_from_slice(&0u32.to_le_bytes()); // p_paddr
    bytes[68..72].copy_from_slice(&TEXT_LEN.to_le_bytes()); // p_filesz
    bytes[72..76].copy_from_slice(&TEXT_LEN.to_le_bytes()); // p_memsz
    bytes[76..80].copy_from_slice(&5u32.to_le_bytes()); // p_flags = PF_R|PF_X
    bytes[80..84].copy_from_slice(&4u32.to_le_bytes()); // p_align

    TestElf {
        bytes,
        shoff,
        shstrtab: (shstrtab_offset, shstrtab_len),
    }
}

/// Append one ELF32 section header (40 bytes) to `out`.
fn push_shdr(
    out: &mut Vec<u8>,
    sh_name: u32,
    sh_type: u32,
    sh_offset: u32,
    sh_size: u32,
    sh_entsize: u32,
) {
    out.extend_from_slice(&sh_name.to_le_bytes()); // sh_name
    out.extend_from_slice(&sh_type.to_le_bytes()); // sh_type
    out.extend_from_slice(&0u32.to_le_bytes()); // sh_flags
    out.extend_from_slice(&0u32.to_le_bytes()); // sh_addr
    out.extend_from_slice(&sh_offset.to_le_bytes()); // sh_offset
    out.extend_from_slice(&sh_size.to_le_bytes()); // sh_size
    out.extend_from_slice(&0u32.to_le_bytes()); // sh_link
    out.extend_from_slice(&0u32.to_le_bytes()); // sh_info
    out.extend_from_slice(&1u32.to_le_bytes()); // sh_addralign
    out.extend_from_slice(&sh_entsize.to_le_bytes()); // sh_entsize
}

/// The section name of every extracted debug section, in order.
fn debug_section_names(image: &LoadImage) -> Vec<&str> {
    image
        .debug_sections
        .iter()
        .map(|(name, _)| name.as_str())
        .collect()
}

/// The whitelist M-06 extracts (STAGE-5 裁决 B).
const EXPECTED_WHITELIST: [&str; 7] = [
    ".symtab",
    ".strtab",
    ".debug_line",
    ".debug_line_str",
    ".debug_str",
    ".debug_abbrev",
    ".debug_info",
];

/// The happy path: a minimal ELF with `.text` + `.symtab` + `.strtab`
/// yields exactly the two whitelisted sections, with the bytes the
/// fixture planted.
#[test]
fn elf_extracts_the_whitelisted_debug_sections_with_their_bytes() {
    let symtab = vec![0x11u8; 24];
    let strtab = b"\0sym_a\0".to_vec();
    let elf = build_elf_with_sections(&[
        TestSection::progbits(".symtab", &symtab),
        TestSection::progbits(".strtab", &strtab),
    ]);

    let li = DefaultLoader
        .load(&elf.bytes, Kind::Elf)
        .expect("elf parse");

    // The loadable image is unaffected by the section table.
    assert_eq!(li.segments.len(), 1);
    assert_eq!(li.segments[0].1, vec![0x93, 0x00, 0xA0, 0x02]);

    assert_eq!(
        debug_section_names(&li),
        vec![".symtab", ".strtab"],
        "exactly the two whitelisted sections, in section-header order"
    );
    assert_eq!(li.debug_sections[0].1, symtab, "`.symtab` bytes");
    assert_eq!(li.debug_sections[1].1, strtab, "`.strtab` bytes");
}

/// The whitelist is exact: every whitelisted name is extracted, and
/// nothing else is — `.shstrtab` (internal only), `.text`, `.comment`
/// and arbitrary custom sections stay out.
#[test]
fn elf_debug_section_whitelist_is_exact() {
    let mut sections: Vec<TestSection> = EXPECTED_WHITELIST
        .iter()
        .map(|name| TestSection::progbits(name, name.as_bytes()))
        .collect();
    sections.push(TestSection::progbits(".text", b"\x93\x00"));
    sections.push(TestSection::progbits(".comment", b"a comment"));
    sections.push(TestSection::progbits(".my_custom_section", b"custom"));

    let elf = build_elf_with_sections(&sections);
    let li = DefaultLoader
        .load(&elf.bytes, Kind::Elf)
        .expect("elf parse");

    assert_eq!(
        debug_section_names(&li),
        EXPECTED_WHITELIST.to_vec(),
        "the whitelist is exact, in section-header order"
    );
    for (name, bytes) in &li.debug_sections {
        assert_eq!(
            bytes,
            name.as_bytes(),
            "section {:?} must carry its own bytes",
            name
        );
    }
    // `.shstrtab` is used internally only — it must never be handed out.
    assert!(
        !debug_section_names(&li).contains(&".shstrtab"),
        "`.shstrtab` is internal, not a debug section"
    );
}

/// `SHT_NOBITS` sections occupy no file space: they are neither
/// extracted nor counted against the debug-section byte budget.
#[test]
fn elf_skips_nobits_sections() {
    let symtab = vec![0xAAu8; 8];
    let elf = build_elf_with_sections(&[
        TestSection::progbits(".symtab", &symtab),
        // A 4 MiB `.debug_info` that lives in no file space at all.
        TestSection::nobits(".debug_info", 4 * 1024 * 1024),
    ]);

    // A budget of 16 bytes is enough for `.symtab` — but only if the
    // NOBITS section is *not* counted against it.
    let limits = ParseLimits {
        debug_section_bytes: 16,
        ..ParseLimits::default()
    };
    let li = DefaultLoader
        .load_with_limits(&elf.bytes, Kind::Elf, &limits)
        .expect("a NOBITS section must not count against the budget");

    assert_eq!(debug_section_names(&li), vec![".symtab"]);
    assert_eq!(li.debug_sections[0].1, symtab);
}

/// An ELF without a section table (`e_shoff == 0`) loads fine and
/// yields no debug sections — that is the normal bare-metal image.
#[test]
fn elf_without_a_section_table_yields_no_debug_sections() {
    let elf = make_minimal_elf(4, 4); // e_shoff = 0
    let li = DefaultLoader.load(&elf, Kind::Elf).expect("elf parse");
    assert!(
        li.debug_sections.is_empty(),
        "an ELF without a section table has no debug sections"
    );
    assert_eq!(li.segments.len(), 1, "the loadable image is unaffected");
}

/// `e_shnum == 0` (the extended section count) is not an error either —
/// M-06 does not implement that form and simply reports no debug
/// sections.
#[test]
fn elf_with_the_extended_section_count_yields_no_debug_sections() {
    let mut elf = build_elf_with_sections(&[TestSection::progbits(".symtab", &[0xAA; 4])]);
    elf.bytes[48..50].copy_from_slice(&0u16.to_le_bytes()); // e_shnum = 0
    let li = DefaultLoader
        .load(&elf.bytes, Kind::Elf)
        .expect("elf parse");
    assert!(li.debug_sections.is_empty());
    assert_eq!(li.segments.len(), 1, "the loadable image is unaffected");
}

/// A missing / out-of-range section-name table must not fail the load:
/// without names the whitelist cannot be applied, so there are no debug
/// sections.
#[test]
fn elf_with_an_unusable_section_name_table_yields_no_debug_sections() {
    let mut elf = build_elf_with_sections(&[TestSection::progbits(".symtab", &[0xAA; 4])]);
    // e_shstrndx (50..52) points past the end of the section table.
    elf.bytes[50..52].copy_from_slice(&999u16.to_le_bytes());
    let li = DefaultLoader
        .load(&elf.bytes, Kind::Elf)
        .expect("elf parse");
    assert!(li.debug_sections.is_empty());
    assert_eq!(li.segments.len(), 1, "the loadable image is unaffected");
}

/// The debug-section byte budget is a **limit, not a truncation**: an
/// image whose whitelisted sections exceed it is rejected.
#[test]
fn elf_rejects_debug_sections_over_the_byte_budget() {
    let elf = build_elf_with_sections(&[
        TestSection::progbits(".symtab", &[0xAA; 32]),
        TestSection::progbits(".strtab", &[0xBB; 32]),
    ]);
    let limits = ParseLimits {
        debug_section_bytes: 48,
        ..ParseLimits::default()
    };
    let err = DefaultLoader
        .load_with_limits(&elf.bytes, Kind::Elf, &limits)
        .unwrap_err();
    assert_eq!(err.family, ErrorFamily::Loader);
    assert_eq!(err.scenario, 2, "E_LDR-02 parse-limit");
    assert!(
        err.message.contains("debugSectionBytes"),
        "unexpected message: {}",
        err.message
    );
}

/// The budget is exactly the limit: sections that fit exactly are
/// accepted (the boundary is inclusive, not off by one).
#[test]
fn elf_accepts_debug_sections_exactly_at_the_byte_budget() {
    let elf = build_elf_with_sections(&[
        TestSection::progbits(".symtab", &[0xAA; 32]),
        TestSection::progbits(".strtab", &[0xBB; 16]),
    ]);
    let limits = ParseLimits {
        debug_section_bytes: 48,
        ..ParseLimits::default()
    };
    let li = DefaultLoader
        .load_with_limits(&elf.bytes, Kind::Elf, &limits)
        .expect("48 bytes of debug sections fit a 48-byte budget");
    assert_eq!(debug_section_names(&li), vec![".symtab", ".strtab"]);
}

/// The order of `debug_sections` is the **section-header order** — not
/// sorted, not de-duplicated. M-11 relies on it to find `.symtab` and
/// its `.strtab`.
#[test]
fn elf_preserves_the_section_header_order() {
    let elf = build_elf_with_sections(&[
        TestSection::progbits(".debug_info", &[0x01]),
        TestSection::progbits(".symtab", &[0x02]),
        TestSection::progbits(".debug_line", &[0x03]),
    ]);
    let li = DefaultLoader
        .load(&elf.bytes, Kind::Elf)
        .expect("elf parse");
    assert_eq!(
        debug_section_names(&li),
        vec![".debug_info", ".symtab", ".debug_line"],
        "the order is the section-header order, not the whitelist order"
    );
}

/// Two sections with the same name are both handed out (no
/// de-duplication).
#[test]
fn elf_keeps_duplicate_section_names() {
    let elf = build_elf_with_sections(&[
        TestSection::progbits(".symtab", &[0x01]),
        TestSection::progbits(".symtab", &[0x02]),
    ]);
    let li = DefaultLoader
        .load(&elf.bytes, Kind::Elf)
        .expect("elf parse");
    assert_eq!(debug_section_names(&li), vec![".symtab", ".symtab"]);
    assert_eq!(li.debug_sections[0].1, vec![0x01]);
    assert_eq!(li.debug_sections[1].1, vec![0x02]);
}

/// A section whose content lies outside the file is malformed — even
/// when the section is *not* whitelisted? No: only whitelisted sections
/// are read, so a bogus offset on an ignored section must not fail the
/// load. This pins that boundary.
#[test]
fn elf_ignores_a_bogus_offset_on_a_non_whitelisted_section() {
    let mut elf = build_elf_with_sections(&[
        TestSection::progbits(".symtab", &[0xAA; 4]),
        TestSection::progbits(".comment", &[0xBB; 4]),
    ]);
    // The `.comment` header is entry #3 (null, .shstrtab, .symtab,
    // .comment): point its content far outside the file.
    let hdr = elf.shdr_offset(3);
    elf.bytes[hdr + 16..hdr + 20].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    let li = DefaultLoader
        .load(&elf.bytes, Kind::Elf)
        .expect("an ignored section's offset must not fail the load");
    assert_eq!(debug_section_names(&li), vec![".symtab"]);
}

/// The default budget is 8 MiB and `LoadImage::default()` carries no
/// debug sections.
#[test]
fn parse_limits_default_debug_section_budget_is_8_mib() {
    assert_eq!(ParseLimits::default().debug_section_bytes, 8 * 1024 * 1024);
    assert!(LoadImage::default().debug_sections.is_empty());
}

// ---------------------------------------------------------------------------
// D-101 ② — the symbol table's declared entry size must be self-consistent
// ---------------------------------------------------------------------------

/// **谎报 `sh_entsize` 的符号表必须被拒收**（台账 **D-101 ②** · 验收
/// **C-5**）。
///
/// M-11 reads the symbol table with a **fixed 16-byte** stride and cannot
/// see `sh_entsize` (it lives in the section header; M-11 only gets the
/// raw `.symtab` bytes) ⇒ a file that claims `sh_entsize = 32` while its
/// entries are still 16 bytes is **indistinguishable** from a correct
/// file as far as M-11 is concerned.
///
/// ⇒ **the container check belongs to M-06** (the only ELF container
/// parser in the tree) ⇒ the **whole image is rejected** ⇒ M-11 never
/// sees the lying file.
#[test]
fn elf_rejects_a_symtab_that_lies_about_its_entry_size() {
    let symtab = vec![0x11u8; 32]; // two 16-byte entries
    let strtab = b"\0sym_a\0".to_vec();
    let elf = build_elf_with_sections(&[
        TestSection::progbits(".symtab", &symtab).with_entsize(32),
        TestSection::progbits(".strtab", &strtab),
    ]);

    let err = DefaultLoader.load(&elf.bytes, Kind::Elf).unwrap_err();
    assert_eq!(err.family, ErrorFamily::Loader);
    assert_eq!(err.scenario, 1, "E_LDR-01 malformed");
    assert!(
        err.message.contains("sh_entsize"),
        "unexpected message: {}",
        err.message
    );
}

/// **显式声明 16 必须放行**（判据的另一侧）。
#[test]
fn elf_accepts_a_symtab_that_declares_the_right_entry_size() {
    let symtab = vec![0x11u8; 32];
    let elf =
        build_elf_with_sections(&[TestSection::progbits(".symtab", &symtab).with_entsize(16)]);
    let li = DefaultLoader
        .load(&elf.bytes, Kind::Elf)
        .expect("a symtab that declares 16 bytes per entry must be accepted");
    assert_eq!(debug_section_names(&li), vec![".symtab"]);
}

/// **`sh_entsize == 0`（未声明）必须放行** —— ELF 里 0 表示「这一节没有
/// 固定大小的条目」（一些老工具链这么写）。
///
/// ⚠️ 把 `0` 也拒掉会**误杀合法镜像** ⇒ 判据必须是「**显式声明了
/// （非 0）且不等于 16** ⇒ 拒收」。
#[test]
fn elf_accepts_a_symtab_that_does_not_declare_its_entry_size() {
    let symtab = vec![0x11u8; 32];
    let elf = build_elf_with_sections(&[TestSection::progbits(".symtab", &symtab).with_entsize(0)]);
    let li = DefaultLoader
        .load(&elf.bytes, Kind::Elf)
        .expect("sh_entsize = 0 means「未声明」and must be accepted");
    assert_eq!(debug_section_names(&li), vec![".symtab"]);
}

/// **非符号表的节区不受这条校验约束** —— `.debug_info` 之类没有固定大小
/// 的条目（`sh_entsize = 0`），而 `.debug_line` 之类自带条目步长的节区
/// 也不该被符号表的规则误伤。
#[test]
fn elf_does_not_apply_the_symbol_entry_rule_to_other_sections() {
    let elf = build_elf_with_sections(&[
        TestSection::progbits(".symtab", &[0xAA; 16]),
        TestSection::progbits(".debug_info", &[0xBB; 16]).with_entsize(64),
    ]);
    let li = DefaultLoader
        .load(&elf.bytes, Kind::Elf)
        .expect("the symbol-entry rule must not touch other sections");
    assert_eq!(debug_section_names(&li), vec![".symtab", ".debug_info"]);
}
