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
