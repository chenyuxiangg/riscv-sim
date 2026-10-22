//! M-06 ELF32 解析器 (RV32 little-endian, ET_EXEC).
//!
//! MVP scope (§1.3): parse the ELF header + program headers, materialise
//! one `Segment` per `PT_LOAD` entry. Dynamic linking, relocations and
//! notes are out of scope.
//!
//! The **section table** is parsed too, but only to hand the *whitelisted*
//! debug sections out as raw bytes (STAGE-5 裁决 B): M-06 is the only
//! ELF container parser, M-11 interprets the bytes (symbol table
//! semantics, DWARF line-table programs). The whole file is never kept —
//! only the whitelisted sections are (bounded by
//! [`ParseLimits::debug_section_bytes`]).
//!
//! The `object` crate is intentionally NOT pulled in for the MVP — a
//! hand-rolled 32-bit LE parser keeps both the dependency surface and
//! the WASM binary size manageable for the FR-05 deliverable.
//! (PROGRESS.md v0.5 决策: object crate 留 P1 兼容层.)

use riscv_sim_contract::error::{ErrorFamily, PlatformError};

use super::{LoadImage, ParseLimits};

/// Family used by all M-06 errors. Matches `ErrorFamily::Loader` from
/// the contract (`E_LDR-NN`).
const FAMILY: ErrorFamily = ErrorFamily::Loader;

/// EI_NIDENT — length of the e_ident array.
const ELF_EI_NIDENT: usize = 16;

/// ELF32 header size in bytes.
const ELF32_EHDR_SIZE: usize = 52;
/// ELF32 program-header entry size.
const ELF32_PHDR_SIZE: usize = 32;
/// ELF32 section-header entry size.
const ELF32_SHDR_SIZE: usize = 40;

/// ELF magic: `\x7FELF`.
const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// `e_ident[EI_CLASS]` — 32-bit objects.
const ELFCLASS32: u8 = 1;
/// `e_ident[EI_DATA]` — little-endian.
const ELFDATA2LSB: u8 = 1;
/// `e_ident[EI_VERSION]` — current version.
const EV_CURRENT: u8 = 1;

/// `e_type` — executable.
const ET_EXEC: u16 = 2;
/// `e_machine` — RISC-V (per the RISC-V psABI; ELF spec reserves 0xF3).
const EM_RISCV: u16 = 0xF3;

/// `p_type` — loadable segment.
const PT_LOAD: u32 = 1;

/// `sh_type` — the section occupies **no** space in the file.
const SHT_NOBITS: u32 = 8;

/// The debug / symbol sections M-06 extracts from the section table
/// (STAGE-5 裁决 B). Everything else — including `.shstrtab`, which is
/// only used internally to read the section names — is ignored.
const DEBUG_SECTION_WHITELIST: [&str; 7] = [
    ".symtab",
    ".strtab",
    ".debug_line",
    ".debug_line_str",
    ".debug_str",
    ".debug_abbrev",
    ".debug_info",
];

/// One PT_LOAD segment, materialised. `bss` (`data.is_empty()` while
/// `size > 0`) is mapped to RAM with zero bytes at M-14 integration
/// time (PROGRESS.md §1.2 + M-06 §6.2 (5)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Physical base address (`p_paddr` from the program header;
    /// RV32 linker scripts typically set `p_paddr == p_vaddr`).
    pub paddr: u32,
    /// Memory size (`p_memsz`). For pure `.bss` this is larger than
    /// `data.len()`; the kernel zero-pads the gap.
    pub memsz: u32,
    /// File bytes (`p_filesz` bytes copied from `p_offset`). Empty
    /// when this is a `.bss`-only segment.
    pub data: Vec<u8>,
}

/// Parse an ELF32 LE executable into a [`LoadImage`].
///
/// Returns one of:
/// - `E_LDR-01` malformed ELF (magic / class / version / machine /
///   header bounds out of range),
/// - `E_LDR-02` parse limits exceeded (file size, program-header count,
///   segment count),
/// - `E_LDR-03` segment overflows the 32-bit bus (paddr + memsz
///   exceeds `BUS_SIZE`).
pub fn parse(bytes: &[u8], limits: &ParseLimits) -> Result<LoadImage, PlatformError> {
    let max = limits.elf_size;
    if bytes.len() > max {
        return Err(PlatformError::new(
            FAMILY,
            2,
            format!(
                "ELF size {} exceeds parseLimit.elfSize {}",
                bytes.len(),
                max
            ),
        ));
    }
    if bytes.len() < ELF32_EHDR_SIZE {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!(
                "truncated ELF header: {} < {}",
                bytes.len(),
                ELF32_EHDR_SIZE
            ),
        ));
    }

    let ident = &bytes[..ELF_EI_NIDENT];
    if ident[..4] != ELF_MAGIC {
        return Err(PlatformError::new(
            FAMILY,
            1,
            "bad ELF magic (want \\x7FELF)",
        ));
    }
    if ident[4] != ELFCLASS32 {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!("unsupported ELF class {}", ident[4]),
        ));
    }
    if ident[5] != ELFDATA2LSB {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!("unsupported ELF data encoding {} (want LE=1)", ident[5]),
        ));
    }
    if ident[6] != EV_CURRENT {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!("unsupported ELF version {}", ident[6]),
        ));
    }

    let e_type = read_u16(bytes, 16)?;
    let e_machine = read_u16(bytes, 18)?;
    let e_version = read_u32(bytes, 20)?;
    let e_entry = read_u32(bytes, 24)?;
    let e_phoff = read_u32(bytes, 28)?;
    let e_shoff = read_u32(bytes, 32)?;
    let e_flags = read_u32(bytes, 36)?;
    let e_ehsize = read_u16(bytes, 40)?;
    let e_phentsize = read_u16(bytes, 42)?;
    let e_phnum = read_u16(bytes, 44)?;
    let e_shentsize = read_u16(bytes, 46)?;
    let e_shnum = read_u16(bytes, 48)?;
    let e_shstrndx = read_u16(bytes, 50)?;

    let _ = (e_flags, e_ehsize); // unused in MVP

    if e_type != ET_EXEC {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!("unsupported ELF e_type={} (want ET_EXEC=2)", e_type),
        ));
    }
    if e_machine != EM_RISCV {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!(
                "unsupported e_machine=0x{:x} (want EM_RISCV=0xF3)",
                e_machine
            ),
        ));
    }
    if e_version != 1 {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!("unsupported ELF e_version={}", e_version),
        ));
    }
    if e_phentsize as usize != ELF32_PHDR_SIZE {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!(
                "unexpected e_phentsize={} (ELF32 should be {})",
                e_phentsize, ELF32_PHDR_SIZE
            ),
        ));
    }

    let phnum = e_phnum as usize;
    let max_ph = limits.segment_count as usize;
    if phnum > max_ph {
        return Err(PlatformError::new(
            FAMILY,
            2,
            format!(
                "e_phnum {} exceeds parseLimit.segmentCount {}",
                phnum, max_ph
            ),
        ));
    }

    let phoff = e_phoff as usize;
    let ph_end = phoff
        .checked_add(phnum.checked_mul(ELF32_PHDR_SIZE).ok_or_else(|| {
            PlatformError::new(FAMILY, 2, "e_phnum * e_phentsize overflows usize")
        })?)
        .ok_or_else(|| PlatformError::new(FAMILY, 1, "phoff + phnum*phentsize overflows usize"))?;
    if ph_end > bytes.len() {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!(
                "program header table [0x{:x}, 0x{:x}) outside file ({})",
                phoff,
                ph_end,
                bytes.len()
            ),
        ));
    }

    let max_paddr: u64 = crate::m14_bus::BUS_SIZE; // 4 GiB.
    let mut segments: Vec<(u32, Vec<u8>)> = Vec::new();
    // Bytes the loader would materialise for this image (file parts +
    // zero-filled `.bss` gaps). Bounded by `total_image_bytes` (D-10).
    let mut total_bytes: u64 = 0;
    for i in 0..phnum {
        let base = phoff + i * ELF32_PHDR_SIZE;
        let phdr = &bytes[base..base + ELF32_PHDR_SIZE];
        let p_type = read_u32(phdr, 0)?;
        if p_type != PT_LOAD {
            continue;
        }
        let p_offset = read_u32(phdr, 4)?;
        let p_vaddr = read_u32(phdr, 8)?;
        let p_paddr = read_u32(phdr, 12)?;
        let p_filesz = read_u32(phdr, 16)?;
        let p_memsz = read_u32(phdr, 20)?;
        let _ = p_vaddr; // MVP: trust p_paddr

        // The in-memory footprint of a PT_LOAD is `p_memsz`; the file
        // contributes `p_filesz` of it. A segment whose file part is
        // larger than its memory part is malformed — accepting it would
        // build a segment that claims more memory than it declares.
        if p_filesz > p_memsz {
            return Err(PlatformError::new(
                FAMILY,
                1,
                format!(
                    "PT_LOAD p_filesz {} exceeds p_memsz {} (paddr=0x{:08x})",
                    p_filesz, p_memsz, p_paddr
                ),
            ));
        }

        let memsz = p_memsz as u64;

        // D-10: bound the memory this segment makes the loader
        // materialise. `p_memsz` is the whole footprint (file bytes plus
        // the zero-filled `.bss` tail), and the tail is *not* stored in
        // the file — so without this check an 84-byte ELF can declare a
        // gigabyte of RAM and have the loader allocate it.
        if memsz > limits.max_region_bytes {
            return Err(PlatformError::new(
                FAMILY,
                2,
                format!(
                    "PT_LOAD memsz {} exceeds parseLimit.maxRegionBytes {} (paddr=0x{:08x})",
                    p_memsz, limits.max_region_bytes, p_paddr
                ),
            ));
        }
        total_bytes = total_bytes.saturating_add(memsz);
        if total_bytes > limits.total_image_bytes {
            return Err(PlatformError::new(
                FAMILY,
                2,
                format!(
                    "image size {} exceeds parseLimit.totalImageBytes {}",
                    total_bytes, limits.total_image_bytes
                ),
            ));
        }

        let paddr_end = (p_paddr as u64).checked_add(memsz).ok_or_else(|| {
            PlatformError::new(
                FAMILY,
                3,
                format!("segment end overflows u64 at paddr=0x{:08x}", p_paddr),
            )
        })?;
        if paddr_end > max_paddr {
            return Err(PlatformError::new(
                FAMILY,
                3,
                format!(
                    "segment at paddr=0x{:08x} memsz=0x{:x} overruns 4 GiB bus",
                    p_paddr, p_memsz
                ),
            ));
        }

        let filesz = p_filesz as usize;
        let offset = p_offset as usize;
        if filesz > 0 {
            let file_end = offset.checked_add(filesz).ok_or_else(|| {
                PlatformError::new(FAMILY, 1, "p_offset + p_filesz overflows usize")
            })?;
            if file_end > bytes.len() {
                return Err(PlatformError::new(
                    FAMILY,
                    1,
                    format!(
                        "PT_LOAD bytes [0x{:x}, 0x{:x}) outside file ({})",
                        offset,
                        file_end,
                        bytes.len()
                    ),
                ));
            }
            segments.push((p_paddr, bytes[offset..file_end].to_vec()));
        }
        // For PT_LOADs whose p_memsz > p_filesz we emit a second,
        // zero-filled segment covering the tail so M-14 zero-initialises
        // a bss-like gap without the ELF PT_LOAD doing the work itself.
        if p_memsz > p_filesz {
            let gap_size = p_memsz - p_filesz;
            // `checked_add` rather than `wrapping_add`. The overflow
            // itself is already unreachable — the `paddr_end` check
            // above rejects any segment that would cross the 4 GiB
            // bus, and `p_filesz <= p_memsz` — but a silent wrap
            // would materialise the zero-filled gap at address 0
            // instead of failing, so the explicit error is cheaper
            // than trusting that ordering to hold (review V3 finding
            // L-6).
            let gap_base = p_paddr.checked_add(p_filesz).ok_or_else(|| {
                PlatformError::new(
                    FAMILY,
                    3,
                    format!(
                        "bss gap base overflows u32 (paddr=0x{p_paddr:08x} filesz=0x{p_filesz:x})"
                    ),
                )
            })?;
            let gap_end = (gap_base as u64)
                .checked_add(gap_size as u64)
                .ok_or_else(|| PlatformError::new(FAMILY, 3, "bss gap end overflows u64"))?;
            if gap_end > max_paddr {
                return Err(PlatformError::new(FAMILY, 3, "bss tail overruns 4 GiB bus"));
            }
            segments.push((gap_base, vec![0u8; gap_size as usize]));
        }
    }

    // The budget must hold for the segments we actually hand to M-14,
    // not just for the program-header count: every PT_LOAD with a
    // `.bss` tail produces two output segments, so a file with
    // `segment_count` program headers can yield up to twice as many
    // load segments (D-22 — the limit has to bind the real output).
    if segments.len() > limits.segment_count as usize {
        return Err(PlatformError::new(
            FAMILY,
            2,
            format!(
                "segment count {} exceeds parseLimit.segmentCount {}",
                segments.len(),
                limits.segment_count
            ),
        ));
    }

    // The whitelisted debug sections (STAGE-5 裁决 B). M-06 hands the
    // raw bytes over; M-11 interprets them.
    let debug_sections =
        extract_debug_sections(bytes, e_shoff, e_shentsize, e_shnum, e_shstrndx, limits)?;

    let load_image = LoadImage {
        segments,
        entry: e_entry,
        debug_sections,
    };
    Ok(load_image)
}

/// Extract the whitelisted debug sections from the ELF section table.
///
/// Returns `(section name, raw bytes)` pairs in **section-header order**
/// (not sorted, not de-duplicated).
///
/// The section table is optional: a bare-metal image or a stripped ELF
/// legitimately has none, and a missing section-name table must not
/// fail the whole load. Those cases yield an **empty** vector:
///
/// * `e_shoff == 0` — no section table at all;
/// * `e_shnum == 0` — the extended form (the real count lives in
///   section 0's `sh_size`), which we do not implement;
/// * `e_shstrndx` out of range — the section names cannot be read, so
///   the name whitelist cannot be applied.
///
/// Everything else that is *malformed* (a section-header table outside
/// the file, `e_shentsize != 40`, a section whose content lies outside
/// the file, an out-of-range or unterminated section name) is an
/// `E_LDR-01` error — a malformed file must not be half-parsed.
fn extract_debug_sections(
    bytes: &[u8],
    e_shoff: u32,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
    limits: &ParseLimits,
) -> Result<Vec<(String, Vec<u8>)>, PlatformError> {
    // No section table (bare-metal image / stripped ELF).
    if e_shoff == 0 {
        return Ok(Vec::new());
    }
    // Extended section count (`e_shnum == 0` ⇒ the count lives in
    // section 0's `sh_size`). Not implemented: report "no debug
    // sections" instead of guessing a count.
    if e_shnum == 0 {
        return Ok(Vec::new());
    }
    if e_shentsize as usize != ELF32_SHDR_SIZE {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!(
                "unexpected e_shentsize={} (ELF32 should be {})",
                e_shentsize, ELF32_SHDR_SIZE
            ),
        ));
    }

    let shnum = e_shnum as usize;
    let shoff = e_shoff as usize;
    let table_bytes = shnum
        .checked_mul(ELF32_SHDR_SIZE)
        .ok_or_else(|| PlatformError::new(FAMILY, 1, "e_shnum * e_shentsize overflows usize"))?;
    let sh_end = shoff.checked_add(table_bytes).ok_or_else(|| {
        PlatformError::new(FAMILY, 1, "e_shoff + e_shnum*e_shentsize overflows usize")
    })?;
    if sh_end > bytes.len() {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!(
                "section header table [0x{:x}, 0x{:x}) outside file ({})",
                shoff,
                sh_end,
                bytes.len()
            ),
        ));
    }

    // The section-name table (`.shstrtab`) is internal: without it the
    // name whitelist cannot be applied, so we report "no debug
    // sections" rather than failing the load.
    if e_shstrndx as usize >= shnum {
        return Ok(Vec::new());
    }

    let shstr_base = shoff + e_shstrndx as usize * ELF32_SHDR_SIZE;
    let shstr_hdr = &bytes[shstr_base..shstr_base + ELF32_SHDR_SIZE];
    let shstr_type = read_u32(shstr_hdr, 4)?;
    let shstr_offset = read_u32(shstr_hdr, 16)?;
    let shstr_size = read_u32(shstr_hdr, 20)?;
    if shstr_type == SHT_NOBITS {
        // A NOBITS string table occupies no file space ⇒ no names.
        return Ok(Vec::new());
    }
    let shstr_end = (shstr_offset as u64)
        .checked_add(shstr_size as u64)
        .ok_or_else(|| {
            PlatformError::new(FAMILY, 1, "shstrtab sh_offset + sh_size overflows u64")
        })?;
    if shstr_end > bytes.len() as u64 {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!(
                "section name table [0x{:x}, 0x{:x}) outside file ({})",
                shstr_offset,
                shstr_end,
                bytes.len()
            ),
        ));
    }
    let shstrtab = &bytes[shstr_offset as usize..shstr_end as usize];

    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    // Total bytes of the extracted sections — bounded by
    // `ParseLimits::debug_section_bytes`.
    let mut total: u64 = 0;
    for i in 0..shnum {
        let base = shoff + i * ELF32_SHDR_SIZE;
        let hdr = &bytes[base..base + ELF32_SHDR_SIZE];
        let sh_name = read_u32(hdr, 0)?;
        let sh_type = read_u32(hdr, 4)?;
        let sh_offset = read_u32(hdr, 16)?;
        let sh_size = read_u32(hdr, 20)?;

        // `SHT_NOBITS` occupies no space in the file: it is not
        // extracted and it does not count against the byte budget.
        if sh_type == SHT_NOBITS {
            continue;
        }

        let name = read_section_name(shstrtab, sh_name)?;
        if !DEBUG_SECTION_WHITELIST.contains(&name.as_str()) {
            continue;
        }

        let end = (sh_offset as u64)
            .checked_add(sh_size as u64)
            .ok_or_else(|| {
                PlatformError::new(FAMILY, 1, "section sh_offset + sh_size overflows u64")
            })?;
        if end > bytes.len() as u64 {
            return Err(PlatformError::new(
                FAMILY,
                1,
                format!(
                    "section {:?} bytes [0x{:x}, 0x{:x}) outside file ({})",
                    name,
                    sh_offset,
                    end,
                    bytes.len()
                ),
            ));
        }

        total += sh_size as u64;
        if total > limits.debug_section_bytes as u64 {
            return Err(PlatformError::new(
                FAMILY,
                2,
                format!(
                    "debug section bytes {} exceeds parseLimit.debugSectionBytes {}",
                    total, limits.debug_section_bytes
                ),
            ));
        }

        out.push((name, bytes[sh_offset as usize..end as usize].to_vec()));
    }
    Ok(out)
}

/// Read a NUL-terminated section name out of the section-name table.
///
/// A name offset outside the table, or a name that is not terminated
/// inside the table, is malformed (`E_LDR-01`) — the name is what the
/// whitelist filter works on, so a bogus name cannot be ignored.
fn read_section_name(table: &[u8], off: u32) -> Result<String, PlatformError> {
    let start = off as usize;
    if start >= table.len() {
        return Err(PlatformError::new(
            FAMILY,
            1,
            format!(
                "section name offset {} outside the section name table ({})",
                off,
                table.len()
            ),
        ));
    }
    let rest = &table[start..];
    let end = rest.iter().position(|b| *b == 0).ok_or_else(|| {
        PlatformError::new(
            FAMILY,
            1,
            format!("section name at offset {} is not NUL-terminated", off),
        )
    })?;
    Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
}

// ---------------------------------------------------------------------------
// little-endian readers with bounds checks. Each reader returns
// PlatformError(E_LDR-01) if the slice is too short — a malformed
// header should fail loud, not silently wrap.
//
// These are total functions over untrusted input: they return `Err`
// instead of panicking, so a malformed file can never take the process
// (or the WASM worker) down. D-09.
// ---------------------------------------------------------------------------

fn read_u16(buf: &[u8], off: usize) -> Result<u16, PlatformError> {
    let end = off.checked_add(2).ok_or_else(|| {
        PlatformError::new(FAMILY, 1, format!("read_u16: offset {} overflows", off))
    })?;
    let raw = buf.get(off..end).ok_or_else(|| {
        PlatformError::new(
            FAMILY,
            1,
            format!("read_u16: oob {} + 2 > {}", off, buf.len()),
        )
    })?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(buf: &[u8], off: usize) -> Result<u32, PlatformError> {
    let end = off.checked_add(4).ok_or_else(|| {
        PlatformError::new(FAMILY, 1, format!("read_u32: offset {} overflows", off))
    })?;
    let raw = buf.get(off..end).ok_or_else(|| {
        PlatformError::new(
            FAMILY,
            1,
            format!("read_u32: oob {} + 4 > {}", off, buf.len()),
        )
    })?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

/// Tests for the private little-endian readers. They live here (rather
/// than in the module-level `tests.rs`) because the readers are private
/// to this module, and the D-09 contract — "out-of-bounds is an `Err`,
/// never a panic" — is about the readers themselves.
#[cfg(test)]
mod reader_tests {
    use super::*;

    /// D-09: an out-of-bounds read must be an `Err`, not a panic.
    #[test]
    fn read_u16_out_of_bounds_is_an_error_not_a_panic() {
        let buf = [0u8; 4];
        let err = read_u16(&buf, 3).unwrap_err();
        assert_eq!(err.family, ErrorFamily::Loader);
        assert_eq!(err.scenario, 1, "E_LDR-01 malformed");
        assert!(err.message.contains("oob"), "unexpected: {}", err.message);
    }

    #[test]
    fn read_u32_out_of_bounds_is_an_error_not_a_panic() {
        let buf = [0u8; 4];
        let err = read_u32(&buf, 1).unwrap_err();
        assert_eq!(err.scenario, 1, "E_LDR-01 malformed");
        assert!(err.message.contains("oob"), "unexpected: {}", err.message);
    }

    /// A wildly out-of-range offset must not overflow the offset
    /// arithmetic either.
    #[test]
    fn read_u32_at_absurd_offset_is_an_error_not_a_panic() {
        let buf = [0u8; 4];
        let err = read_u32(&buf, usize::MAX).unwrap_err();
        assert_eq!(err.scenario, 1, "E_LDR-01 malformed");
        assert!(
            err.message.contains("overflow"),
            "unexpected: {}",
            err.message
        );
    }

    /// The happy path still decodes little-endian correctly.
    #[test]
    fn readers_decode_little_endian() {
        let buf = [0x34u8, 0x12, 0x78, 0x56, 0xBC, 0x9A];
        assert_eq!(read_u16(&buf, 0).unwrap(), 0x1234);
        assert_eq!(read_u32(&buf, 0).unwrap(), 0x5678_1234);
        assert_eq!(read_u16(&buf, 4).unwrap(), 0x9ABC);
    }
}
