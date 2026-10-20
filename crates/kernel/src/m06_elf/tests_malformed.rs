//! M-06 畸形输入表驱动用例 (D-23 门禁缺口第 2 项).
//!
//! 台账 D-23 的结论：现有三条门禁对「畸形输入」这一整类缺陷**完全无
//! 感知**——`cargo test` 只覆盖 Rust，而 D-03 是一个 `panic!` 而不是
//! `Err`，测试从未喂过那个输入。本文件补上这一项门禁。
//!
//! 契约（对本模块的全部输入成立）：
//!
//! > 任何畸形输入都必须返回 `Err(PlatformError)`，**不得 panic**。
//!
//! 在 WASM 里 panic 等于 worker 崩溃，所以这条契约比「解析失败」严重得多。
//!
//! 覆盖矩阵（表驱动，见 [`MalformedCase`]）：
//!
//! | 类别 | 用例 |
//! | --- | --- |
//! | 空文件 | ELF / HEX / raw 三条路径 |
//! | 4 字节短记录 | IHEX `:01234567`（D-03 复现输入） |
//! | 坏 magic | ELF magic 被改写 |
//! | filesz > memsz | 畸形 PT_LOAD（D-15） |
//! | 超长段 | 段大小超过 `max_region_bytes`（D-10） |
//! | count 与实际长度不符 | IHEX 声明 count ≠ 实际 payload |
//! | 截断的 ELF header | 头长于 52 字节但被截到一半 |
//! | 非 UTF-8 字节 | HEX 路径收到非法 UTF-8 |
//! | 整数溢出边界 | HEX 扩展地址 + 局部地址溢出；ELF `p_offset + p_filesz` 溢出 |
//!
//! 另有一条 fuzz 式回归用例（固定种子，保证确定性）喂随机字节与变异后的
//! 合法镜像，要求同样「不得 panic」。

use super::*;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Build one IHEX record line (with the trailing newline) from raw
/// bytes, computing the two's-complement checksum.
fn ihex_line(body: &[u8]) -> String {
    let mut bytes = body.to_vec();
    let sum: u8 = bytes.iter().fold(0u8, |acc, b| acc.wrapping_add(*b));
    bytes.push(0u8.wrapping_sub(sum));
    let ascii: String = bytes.iter().map(|b| format!("{:02X}", b)).collect();
    format!(":{}\n", ascii)
}

/// A well-formed IHEX data record: `count` bytes at `addr`.
fn ihex_data(addr: u16, data: &[u8]) -> String {
    let mut body = vec![data.len() as u8];
    body.extend_from_slice(&addr.to_be_bytes());
    body.push(0x00);
    body.extend_from_slice(data);
    ihex_line(&body)
}

/// The well-formed IHEX EOF record.
fn ihex_eof() -> String {
    ihex_line(&[0x00, 0x00, 0x00, 0x01])
}

/// A well-formed ELF32 LE RV32 ET_EXEC with one PT_LOAD of `memsz`
/// bytes at paddr 0.
fn minimal_elf(memsz: u32) -> Vec<u8> {
    let text_offset = 52u32 + 32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[
        0x7F, b'E', b'L', b'F', 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0xF3, 0x00, 1, 0, 0, 0,
        0, 0, 0, 0, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 52, 0, 32, 0, 1, 0, 0, 0, 0, 0, 0, 0,
    ]);
    bytes.extend_from_slice(&1u32.to_le_bytes()); // p_type = PT_LOAD
    bytes.extend_from_slice(&text_offset.to_le_bytes()); // p_offset
    bytes.extend_from_slice(&0u32.to_le_bytes()); // p_vaddr
    bytes.extend_from_slice(&0u32.to_le_bytes()); // p_paddr
    bytes.extend_from_slice(&4u32.to_le_bytes()); // p_filesz
    bytes.extend_from_slice(&memsz.to_le_bytes()); // p_memsz
    bytes.extend_from_slice(&5u32.to_le_bytes()); // p_flags
    bytes.extend_from_slice(&4u32.to_le_bytes()); // p_align
    bytes.extend_from_slice(&0x02A0_0093u32.to_le_bytes()); // ADDI x1, x0, 42
    bytes
}

// ---------------------------------------------------------------------------
// Table-driven malformed inputs
// ---------------------------------------------------------------------------

/// One malformed input: how it is fed to the loader, and what the
/// loader must answer.
struct MalformedCase {
    /// Human-readable label (used in the assertion message).
    name: &'static str,
    /// Which parser receives the bytes.
    kind: Kind,
    /// The malformed payload.
    bytes: Vec<u8>,
    /// The limits the case is parsed under.
    limits: ParseLimits,
    /// The error scenario the loader must report.
    expect_scenario: u16,
}

impl MalformedCase {
    fn new(name: &'static str, kind: Kind, bytes: Vec<u8>, expect_scenario: u16) -> Self {
        Self {
            name,
            kind,
            bytes,
            limits: ParseLimits::default(),
            expect_scenario,
        }
    }

    fn with_limits(mut self, limits: ParseLimits) -> Self {
        self.limits = limits;
        self
    }
}

/// Every malformed input must be an `Err`, never a panic.
#[test]
fn malformed_inputs_are_rejected_without_panicking() {
    let tight = ParseLimits {
        max_region_bytes: 4096,
        ..ParseLimits::default()
    };

    let cases: Vec<MalformedCase> = vec![
        // ----- 空文件 -----
        MalformedCase::new("empty file / ELF", Kind::Elf, Vec::new(), 1),
        MalformedCase::new("empty file / HEX", Kind::Hex, Vec::new(), 1),
        MalformedCase::new("empty file / raw", Kind::RawHex, Vec::new(), 1),
        // ----- 4 字节短记录 (D-03 复现输入) -----
        MalformedCase::new(
            "4-byte short IHEX record",
            Kind::Hex,
            b":01234567\n:00000001FF\n".to_vec(),
            1,
        ),
        // ----- 声明 count 与实际长度不符 -----
        MalformedCase::new(
            "IHEX declared count != actual length",
            Kind::Hex,
            // count says 4, but only 2 data bytes follow.
            format!(
                "{}{}",
                ihex_line(&[0x04, 0x00, 0x00, 0x00, 0xAA, 0xBB]),
                ihex_eof()
            )
            .into_bytes(),
            1,
        ),
        // ----- 其他 IHEX 畸形 -----
        MalformedCase::new(
            "IHEX odd hex length",
            Kind::Hex,
            b":0123456\n:00000001FF\n".to_vec(),
            1,
        ),
        MalformedCase::new(
            "IHEX non-hex character",
            Kind::Hex,
            b":01ZZ4567\n:00000001FF\n".to_vec(),
            1,
        ),
        MalformedCase::new(
            "IHEX missing leading colon",
            Kind::Hex,
            b"0100000000FF\n".to_vec(),
            1,
        ),
        MalformedCase::new(
            "IHEX checksum mismatch",
            Kind::Hex,
            b":04000000AABBCCDD00\n:00000001FF\n".to_vec(),
            1,
        ),
        MalformedCase::new(
            "IHEX unknown record type",
            Kind::Hex,
            format!("{}{}", ihex_line(&[0x00, 0x00, 0x00, 0x07]), ihex_eof()).into_bytes(),
            1,
        ),
        MalformedCase::new(
            "IHEX missing EOF record",
            Kind::Hex,
            ihex_data(0x0000, &[0xAA, 0xBB]).into_bytes(),
            1,
        ),
        // ----- 坏 magic -----
        MalformedCase::new(
            "bad ELF magic",
            Kind::Elf,
            {
                let mut elf = minimal_elf(4);
                elf[1] = b'X';
                elf
            },
            1,
        ),
        // ----- 截断的 ELF header -----
        MalformedCase::new("truncated ELF header", Kind::Elf, vec![0u8; 30], 1),
        // ----- filesz > memsz (D-15) -----
        MalformedCase::new(
            "p_filesz > p_memsz",
            Kind::Elf,
            {
                let mut elf = minimal_elf(4);
                // p_memsz at program-header offset 20 → file offset 52 + 20.
                elf[72..76].copy_from_slice(&0u32.to_le_bytes());
                elf
            },
            1,
        ),
        // ----- 超长段 (D-10) -----
        MalformedCase::new(
            "segment larger than max_region_bytes",
            Kind::Elf,
            minimal_elf(1024 * 1024),
            2,
        )
        .with_limits(tight),
        // ----- 整数溢出边界 -----
        MalformedCase::new(
            "ELF p_offset + p_filesz overflows the file",
            Kind::Elf,
            {
                let mut elf = minimal_elf(4);
                // p_offset at program-header offset 4 → file offset 52 + 4.
                elf[56..60].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
                elf
            },
            1,
        ),
        MalformedCase::new(
            "HEX extended address + local address overflows u32",
            Kind::Hex,
            format!(
                "{}{}{}",
                // ELA 0xFFFF → ext_high = 0xFFFF_0000
                ihex_line(&[0x02, 0x00, 0x00, 0x04, 0xFF, 0xFF]),
                ihex_data(0xFFFF, &[0xAA]),
                ihex_eof()
            )
            .into_bytes(),
            3,
        ),
        MalformedCase::new(
            "HEX data range end overflows u32",
            Kind::Hex,
            format!(
                "{}{}{}",
                ihex_line(&[0x02, 0x00, 0x00, 0x04, 0xFF, 0xFF]),
                ihex_data(0xFFF0, &[0xAA; 16]),
                ihex_eof()
            )
            .into_bytes(),
            3,
        ),
        // ----- 地址越界 -----
        MalformedCase::new(
            "ELF segment overruns the 4 GiB bus",
            Kind::Elf,
            {
                let mut elf = minimal_elf(4);
                // p_paddr at program-header offset 12 → file offset 52 + 12.
                elf[64..68].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
                // p_memsz at program-header offset 20 → file offset 52 + 20.
                elf[72..76].copy_from_slice(&0x100u32.to_le_bytes());
                elf
            },
            3,
        ),
        // ----- 非 UTF-8 字节 -----
        MalformedCase::new(
            "HEX payload is not valid UTF-8",
            Kind::Hex,
            vec![b':', b'0', b'1', 0xFF, 0xFE, b'6', b'7', b'\n'],
            1,
        ),
        // ----- 段数预算 -----
        MalformedCase::new(
            "HEX image exceeds the segment budget",
            Kind::Hex,
            format!(
                "{}{}{}{}",
                ihex_data(0x0000, &[0xAA]),
                ihex_data(0x1000, &[0xBB]),
                ihex_data(0x2000, &[0xCC]),
                ihex_eof()
            )
            .into_bytes(),
            2,
        )
        .with_limits(ParseLimits {
            segment_count: 2,
            ..ParseLimits::default()
        }),
        MalformedCase::new(
            "ELF e_phnum exceeds the segment budget",
            Kind::Elf,
            {
                let mut elf = minimal_elf(4);
                elf[44..46].copy_from_slice(&0x00FFu16.to_le_bytes());
                elf
            },
            2,
        ),
    ];

    assert!(
        cases.len() >= 9,
        "the malformed table must cover at least the 9 mandated categories"
    );

    for case in &cases {
        let result = DefaultLoader.load_with_limits(&case.bytes, case.kind, &case.limits);
        let err = match result {
            Ok(image) => panic!(
                "case {:?} must be rejected, but it parsed into {} segment(s)",
                case.name,
                image.segments.len()
            ),
            Err(e) => e,
        };
        assert_eq!(
            err.family,
            ErrorFamily::Loader,
            "case {:?}: wrong error family",
            case.name
        );
        assert_eq!(
            err.scenario, case.expect_scenario,
            "case {:?}: wrong error scenario (message: {})",
            case.name, err.message
        );
    }
}

// ---------------------------------------------------------------------------
// Fuzz-style regression: random bytes must never panic.
// ---------------------------------------------------------------------------

/// Deterministic xorshift64* PRNG. The seed is fixed so the corpus is
/// reproducible: the point is not randomness but a *stable* corpus that
/// fails the build if a panic ever appears.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn next_u8(&mut self) -> u8 {
        (self.next_u64() >> 24) as u8
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng::new(0x5EED_1234_ABCD_0001);
    for _ in 0..2_000 {
        let len = 1 + rng.below(256);
        let mut bytes = vec![0u8; len];
        for b in bytes.iter_mut() {
            *b = rng.next_u8();
        }

        // Random bytes are never a valid RV32 ELF and never a valid
        // IHEX file, so both parsers must reject them. A raw blob is
        // accepted by design — that is the whole point of the raw
        // fallback (D-21).
        let elf = DefaultLoader.load(&bytes, Kind::Elf);
        assert!(
            elf.is_err(),
            "random bytes must not parse as ELF (len {})",
            len
        );
        let hex = DefaultLoader.load(&bytes, Kind::Hex);
        assert!(
            hex.is_err(),
            "random bytes must not parse as HEX (len {})",
            len
        );
        let raw = DefaultLoader.load(&bytes, Kind::RawHex);
        assert!(raw.is_ok(), "raw bytes must load (len {})", len);
    }
}

#[test]
fn mutated_valid_images_never_panic() {
    // Tight limits keep the allocations of a mutated (huge) declared
    // segment small while the mutation is exercised.
    let limits = ParseLimits {
        elf_size: 4096,
        hex_size: 4096,
        raw_size: 4096,
        segment_count: 8,
        total_image_bytes: 64 * 1024,
        max_region_bytes: 64 * 1024,
        ..ParseLimits::default()
    };

    let mut rng = Rng::new(0x5EED_1234_ABCD_0002);
    let elf_seed = minimal_elf(4096);
    let hex_seed = format!(
        "{}{}",
        ihex_data(0x0000, &[0x93, 0x00, 0xA0, 0x02]),
        ihex_eof()
    );

    for _ in 0..2_000 {
        // ----- mutate a valid ELF -----
        let mut elf = elf_seed.clone();
        let flips = 1 + rng.below(4);
        for _ in 0..flips {
            let idx = rng.below(elf.len());
            elf[idx] = rng.next_u8();
        }
        // The result may be Ok or Err — a single byte flip can still
        // leave a valid ELF. What matters is that it never panics.
        let _ = DefaultLoader.load_with_limits(&elf, Kind::Elf, &limits);

        // ----- mutate a valid IHEX file -----
        let mut hex_bytes = hex_seed.as_bytes().to_vec();
        let flips = 1 + rng.below(4);
        for _ in 0..flips {
            let idx = rng.below(hex_bytes.len());
            hex_bytes[idx] = rng.next_u8();
        }
        let _ = DefaultLoader.load_with_limits(&hex_bytes, Kind::Hex, &limits);
    }
}
