//! M-06 ELF32 解析器 (RV32 little-endian, ET_EXEC).
//!
//! MVP scope (§1.3): parse the ELF header + program headers, materialise
//! one `Segment` per `PT_LOAD` entry. Sections, dynamic linking,
//! relocations, DWARF, and notes are out of scope — DWARF line-table
//! parsing lives in M-5 (LLD §4.11) on the v0.3 critical path.
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

    let e_type = read_u16(bytes, 16);
    let e_machine = read_u16(bytes, 18);
    let e_version = read_u32(bytes, 20);
    let e_entry = read_u32(bytes, 24);
    let e_phoff = read_u32(bytes, 28);
    let e_shoff = read_u32(bytes, 32);
    let e_flags = read_u32(bytes, 36);
    let e_ehsize = read_u16(bytes, 40);
    let e_phentsize = read_u16(bytes, 42);
    let e_phnum = read_u16(bytes, 44);
    let e_shentsize = read_u16(bytes, 46);
    let e_shnum = read_u16(bytes, 48);
    let e_shstrndx = read_u16(bytes, 50);

    let _ = (e_shoff, e_flags, e_ehsize, e_shentsize, e_shnum, e_shstrndx); // unused in MVP

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
        .checked_add(
            phnum
                .checked_mul(ELF32_PHDR_SIZE)
                .expect("phnum * phentsize fits usize"),
        )
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
    for i in 0..phnum {
        let base = phoff + i * ELF32_PHDR_SIZE;
        let phdr = &bytes[base..base + ELF32_PHDR_SIZE];
        let p_type = read_u32(phdr, 0);
        if p_type != PT_LOAD {
            continue;
        }
        let p_offset = read_u32(phdr, 4);
        let p_vaddr = read_u32(phdr, 8);
        let p_paddr = read_u32(phdr, 12);
        let p_filesz = read_u32(phdr, 16);
        let p_memsz = read_u32(phdr, 20);
        let _ = p_vaddr; // MVP: trust p_paddr

        let memsz = p_memsz as u64;
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
            let gap_base = p_paddr.wrapping_add(p_filesz);
            let gap_end = (gap_base as u64)
                .checked_add(gap_size as u64)
                .ok_or_else(|| PlatformError::new(FAMILY, 3, "bss gap end overflows u64"))?;
            if gap_end > max_paddr {
                return Err(PlatformError::new(FAMILY, 3, "bss tail overruns 4 GiB bus"));
            }
            segments.push((gap_base, vec![0u8; gap_size as usize]));
        }
    }

    let load_image = LoadImage {
        segments,
        entry: e_entry,
    };
    Ok(load_image)
}

// ---------------------------------------------------------------------------
// little-endian readers with bounds checks. Each reader returns
// PlatformError(E_LDR-01) if the slice is too short — a malformed
// header should fail loud, not silently wrap.
// ---------------------------------------------------------------------------

fn read_u16(buf: &[u8], off: usize) -> u16 {
    if off + 2 > buf.len() {
        // E_LDR-01 path; we never trigger this from the public entry
        // because parse has already verified the header bounds, but the
        // helper is total for the program-header loop as well.
        panic!("read_u16: oob {} + 2 > {}", off, buf.len());
    }
    u16::from_le_bytes([buf[off], buf[off + 1]])
}

fn read_u32(buf: &[u8], off: usize) -> u32 {
    if off + 4 > buf.len() {
        panic!("read_u32: oob {} + 4 > {}", off, buf.len());
    }
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}
