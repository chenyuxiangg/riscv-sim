//! v0.26 批（Lane B · 契约 + 宿主 + 外设）的**内存图**用例。
//!
//! 冻结规格：`docs/detailed-design/STAGE-11_V026_Memory_Map_A.md`
//! §2.2 的 **B-2**（快照往返）与 **B-4**（QemuVirt 档区域数 13 → 6）。
//!
//! ## 为什么 B-2 **手工构造**载荷
//!
//! ⚠️ **B-2 必须手工构造 `MemPageDto` 列表**，不许用
//! `Bus::export_pages()` 生成 ⇒ 这样它在「旧 M-14」（`export_pages` 按
//! 64 KiB 整块导出）与「新 M-14」（`export_pages` 只导出**写过的** 4 KiB
//! 页）两棵树里**都成立** ⇒ 它测的是「**还原**这条路对不对」，而不是
//! 「**导出**这条路长什么样」。
//!
//! ⚠️ **B-4 不许只数区域条数**：换档失败时
//! [`riscv_sim_kernel::kernel_host::KernelHost::new_with_profile`] 会
//! **静默退回默认档**（只留一个 `probe_wiring_failures` 计数）⇒ 断言必须
//! 同时落在「**真的是 QemuVirt 布局**」这件事上：一个 `base == 0x8000_0000`
//! 且 `size == 128 MiB` 的 RAM 区域。

//! ## ⚠️ 期望值的来源：**描述** + **独立字面量交叉核对**（v0.27 · D-123-B）
//!
//! v0.27 之后内存图是**数据**（一份平台描述）⇒ 断言的期望值应当
//! **从描述查**（规格 §6.3 的第 1 ~ 8 / 12 / 15 条）⇒ 不再抄一份宿主
//! 常量。
//!
//! ⚠️ 但**「从描述查」不等于「从被测实现反推」** ⇒ 本文件的写法是
//! **两条断言并列**：
//!
//! 1. **主断言**：期望值**从描述查**（`described_*` 助手）；
//! 2. **交叉核对**：描述的值必须与**独立字面量**一致（规格 §6.3 第 16
//!    条的纪律：那些字面量**故意不从描述取** ⇒ 它们的作用是「**不与
//!    实现同源**」⇒ 描述漂了它们会红）。
//!
//! ⇒ 描述本身的内容由 `kernel_host/tests_platform.rs` 的 B-1 / B-2 用例
//! （**字面量**）钉住 ⇒ 「字面量 → 描述 → 内存图」这条链是**闭合**的。

use riscv_sim_contract::platform::{PlatformDescription, RegionSpec};
use riscv_sim_contract::session::{CheckpointMetaDto, MemPageDto, REGION_KIND_RAM};
use riscv_sim_kernel::kernel_host::platform;
use riscv_sim_kernel::kernel_host::{KernelHost, MemoryProfile};
use riscv_sim_kernel::m14_bus::{Bus, BusWidth};

// ---------------------------------------------------------------------------
// 平台描述（**取值来源**：从描述查，不抄宿主常量）
// ---------------------------------------------------------------------------

/// `qemu-virt` 描述。
fn qemu_virt_description() -> PlatformDescription {
    platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置描述 qemu-virt")
}

/// `default` 描述。
fn default_description() -> PlatformDescription {
    platform::builtin_description(platform::PLATFORM_ID_DEFAULT).expect("内置描述 default")
}

/// 描述里的**全部区域**（按 `base` 升序 —— 与 `export_map()` 的顺序一致）。
fn described_regions(description: &PlatformDescription) -> Vec<RegionSpec> {
    platform::regions_by_base(description)
        .into_iter()
        .cloned()
        .collect()
}

/// 描述里的 **RAM 区域**（按 `base` 升序）。
fn described_ram_regions(description: &PlatformDescription) -> Vec<RegionSpec> {
    described_regions(description)
        .into_iter()
        .filter(|region| region.kind() == REGION_KIND_RAM)
        .collect()
}

/// 描述里的**高段 RAM**（`(base, size)`）—— 取**最大**的那段（无歧义，
/// 不靠下标）。
fn described_high_ram(description: &PlatformDescription) -> (u32, u32) {
    let mut rams = described_ram_regions(description);
    rams.sort_by_key(|region| region.declared_size().unwrap_or(0));
    let high = rams.last().expect("描述里有 RAM 区域");
    (
        high.base(),
        high.declared_size().expect("RAM 区域有声明大小"),
    )
}

/// 描述里的**低段 RAM**（`(base, size)`）—— 取 `base == 0` 的那段。
fn described_low_ram(description: &PlatformDescription) -> (u32, u32) {
    described_ram_regions(description)
        .into_iter()
        .find(|region| region.base() == 0)
        .map(|region| (region.base(), region.declared_size().unwrap_or(0)))
        .expect("描述里有 base 为 0 的低段 RAM")
}

// ---------------------------------------------------------------------------
// 架构字面量（**不从被测实现反推**）
// ---------------------------------------------------------------------------

/// 物理页大小（4 KiB）—— 快照页的**对齐**粒度。
const PAGE_BYTES: u32 = 4096;

/// 128 MiB —— QEMU virt 档那一整段 RAM 的大小。
///
/// ⚠️ **自抄副本**（规格 §6.3 第 16 条）：它**故意不从描述取** ⇒ 它的
/// 作用是「**不与实现同源**」⇒ 描述漂了它会红。
///
/// ⚠️ **它与 `qemu-virt` 描述的对应关系**：描述里那段高段 RAM 的
/// `size`（`base = 0x8000_0000` / `size = 134217728`）⇒ 本常量就是那个
/// `size` 的**独立副本**。
const QEMU_VIRT_RAM_BYTES: u32 = 128 * 1024 * 1024;

/// QEMU virt 档的 RAM 起点（ITCM）。
///
/// ⚠️ **自抄副本**（规格 §6.3 第 16 条）⇒ **不从描述取**。
/// ⚠️ **对应关系**：描述里那段高段 RAM 的 `base`。
const QEMU_VIRT_RAM_BASE: u32 = 0x8000_0000;

/// QEMU virt 档 RAM 段的**末尾 + 1**（osz 的堆尾落在 `0x8800_0000` 附近）。
///
/// ⚠️ **自抄副本**（规格 §6.3 第 16 条）⇒ **不从描述取**。
/// ⚠️ **对应关系**：描述里那段高段 RAM 的 `base + size`。
const QEMU_VIRT_RAM_END: u32 = 0x8800_0000;

/// 旧实现里那 8 段 16 MiB 的**碎片**大小 —— v0.26 之后**不许**再出现。
const LEGACY_RAM_CHUNK_BYTES: u32 = 16 * 1024 * 1024;

/// QEMU virt 档的区域总数（1 低段 RAM + 1 高段 RAM + GPIO + UART + CLINT
/// + PLIC）。
///
/// ⚠️ **自抄副本**（规格 §6.3 第 16 条）⇒ **不从描述取**。
/// ⚠️ **对应关系**：`qemu-virt` 描述里 `regions` 的条数。
const QEMU_VIRT_REGION_COUNT: usize = 6;

/// `default` 档的区域总数（低段 RAM + 高段 RAM + GPIO）。
///
/// ⚠️ **自抄副本**（同上）⇒ **不从描述取**。
/// ⚠️ **对应关系**：`default` 描述里 `regions` 的条数。
const DEFAULT_REGION_COUNT: usize = 3;

/// `default` 档高段 RAM 的大小（4 MiB）。
///
/// ⚠️ **自抄副本**（同上）⇒ **不从描述取**。
/// ⚠️ **对应关系**：`default` 描述里那段高段 RAM 的 `size`。
const DEFAULT_HIGH_RAM_BYTES: u32 = 4 * 1024 * 1024;

/// `default` 档的每区域护栏预算（16 MiB）。
///
/// ⚠️ **自抄副本**（同上）⇒ **不从描述取**。
/// ⚠️ **对应关系**：`default` 描述的 `region_budget_bytes`。
const DEFAULT_REGION_BUDGET_BYTES: u32 = 16 * 1024 * 1024;

/// 手工构造的那一页的**基址**（4096 对齐）。
const PAGE_BASE: u32 = 0x1000;

/// 一页里**没写过**的另一个页（用来证明「没写过的页仍读回 0」）。
const UNTOUCHED_PAGE_BASE: u32 = 0x3000;

// ---------------------------------------------------------------------------
// B-2 · 快照往返
// ---------------------------------------------------------------------------

/// 一页里的**逐字节可区分**的内容 —— 不能全 0（那与「没写过」不可区分）。
fn page_pattern() -> Vec<u8> {
    (0..PAGE_BYTES as usize)
        .map(|i| ((i * 7 + 13) % 251) as u8)
        .collect()
}

#[test]
fn a_hand_built_page_snapshot_restores_byte_for_byte() {
    // ---- 源会话：写一页，然后**手工**构造那份页列表 ----
    let mut source = Bus::new();
    source
        .map_ram(0, 1024 * 1024, &[])
        .expect("低段 RAM 必须能映射");
    let pattern = page_pattern();
    for (i, byte) in pattern.iter().enumerate() {
        source
            .write(PAGE_BASE + i as u32, BusWidth::Byte, u32::from(*byte))
            .expect("写那一页");
    }

    // ⚠️ **手工构造**（不是 `export_pages()`）：`base` 4096 对齐、`bytes`
    // 正好一页。
    let hand_built = vec![MemPageDto {
        base: PAGE_BASE,
        bytes: pattern.clone(),
    }];

    // ---- 目标会话：一张空的总线，只还原这一页 ----
    let mut target = Bus::new();
    target
        .map_ram(0, 1024 * 1024, &[])
        .expect("低段 RAM 必须能映射");
    target
        .load_pages(&hand_built)
        .expect("手工构造的那一页必须能还原");

    // ---- 写过的页**逐字节**相同 ----
    for (i, byte) in pattern.iter().enumerate() {
        let addr = PAGE_BASE + i as u32;
        let read_back = target.read(addr, BusWidth::Byte).expect("读那一页");
        assert_eq!(
            read_back & 0xFF,
            u32::from(*byte),
            "第 {i} 个字节（0x{addr:08x}）必须与载荷里**逐字节**相同"
        );
    }

    // ---- 没写过的页**仍读回 0**（不是残留、不是未映射陷阱） ----
    for addr in [UNTOUCHED_PAGE_BASE, PAGE_BASE + PAGE_BYTES] {
        let word = target.read(addr, BusWidth::Word).expect("读没写过的那一页");
        assert_eq!(word, 0, "0x{addr:08x} 那一页**没被载荷写过** ⇒ 必须读回 0");
    }
}

#[test]
fn a_hand_built_page_snapshot_survives_the_host_round_trip() {
    // ---- 源宿主：默认档（低段 1 MiB RAM @ 0x0）----
    let mut source = KernelHost::new();
    let pattern = page_pattern();
    for (i, byte) in pattern.iter().enumerate() {
        source
            .bus_mut()
            .write(PAGE_BASE + i as u32, BusWidth::Byte, u32::from(*byte))
            .expect("写那一页");
    }

    // ---- 采一份**合法**载荷（其余五段用它当模板）----
    let mut payload = source.capture_session(CheckpointMetaDto {
        id: 1,
        t_virt: 0,
        t_host: 0,
        base_seq: 0,
        version: riscv_sim_contract::session::SESSION_SNAPSHOT_SCHEMA_VERSION,
    });

    // ⚠️ **手工构造**页列表：只含**写过的那一页**，`base` 4096 对齐。
    payload.m13_state.mem_pages = vec![MemPageDto {
        base: PAGE_BASE,
        bytes: pattern.clone(),
    }];
    payload
        .validate()
        .expect("手工构造的载荷必须自洽（页落在 RAM 区内、升序、不重叠）");

    // ---- 还原进一个**全新的**宿主 ----
    let mut target = KernelHost::new();
    target.restore_session(&payload).expect("还原必须成功");

    // ---- 写过的页**逐字节**相同 ----
    for (i, byte) in pattern.iter().enumerate() {
        let addr = PAGE_BASE + i as u32;
        let read_back = target
            .bus_mut()
            .read(addr, BusWidth::Byte)
            .expect("读那一页");
        assert_eq!(
            read_back & 0xFF,
            u32::from(*byte),
            "第 {i} 个字节（0x{addr:08x}）必须与载荷里**逐字节**相同"
        );
    }

    // ---- 没写过的页**仍读回 0** ----
    for addr in [UNTOUCHED_PAGE_BASE, PAGE_BASE + PAGE_BYTES] {
        let word = target
            .bus_mut()
            .read(addr, BusWidth::Word)
            .expect("读没写过的那一页");
        assert_eq!(word, 0, "0x{addr:08x} 那一页**没被载荷写过** ⇒ 必须读回 0");
    }
}

// ---------------------------------------------------------------------------
// B-4 · QemuVirt 档：8 个区域 → 1 个 128 MiB 区域
// ---------------------------------------------------------------------------

#[test]
fn the_qemuvirt_profile_maps_one_128_mib_ram_region() {
    let host = KernelHost::new_with_profile(MemoryProfile::QemuVirt);

    // ⚠️ **换档失败会静默退回默认档**（`new_with_profile` 的失败路径只留
    // 一个计数）⇒ 这条断言是「**没有**发生静默降级」的直接证据。
    assert_eq!(
        host.probe_wiring_failures(),
        0,
        "换档失败会**静默退回默认档** ⇒ 失败次数必须是 0"
    );

    let map = host.bus().export_map();

    // ---- 区域总数 13 → 6 ----
    //
    // ⚠️ **期望值从 `qemu-virt` 描述查**（规格 §6.3 第 1 条 · v0.27 ·
    // D-123-B）：内存图是**数据** ⇒ 断言的来源也必须是那份数据。
    let described_count = described_regions(&qemu_virt_description()).len();
    assert_eq!(
        map.regions.len(),
        described_count,
        "QemuVirt 档的区域数必须与**描述**一致（1 低段 RAM + 1 高段 RAM + GPIO + UART + CLINT + PLIC），\
         实际是 {}：{:#?}",
        map.regions.len(),
        map.regions
    );
    // ⚠️ **交叉核对**（规格 §6.3 第 16 条）：描述的条数必须与**独立
    // 字面量**一致 ⇒ 描述漂了这条会红。
    assert_eq!(
        described_count, QEMU_VIRT_REGION_COUNT,
        "qemu-virt 描述里的区域数必须还是 {QEMU_VIRT_REGION_COUNT}（独立字面量交叉核对）"
    );

    // ---- RAM 区域恰好两个（低段 + 高段）----
    //
    // ⚠️ **期望值从描述查**（规格 §6.3 第 2 条）。
    let described_ram_count = described_ram_regions(&qemu_virt_description()).len();
    let ram: Vec<_> = map.regions.iter().filter(|r| r.kind == "ram").collect();
    assert_eq!(
        ram.len(),
        described_ram_count,
        "QemuVirt 档的 RAM 区域数必须与**描述**一致（低段 1 MiB + 高段 128 MiB），实际是 {}：{:#?}",
        ram.len(),
        ram
    );

    // ---- 高段 RAM 是**一个** 128 MiB 的区域（不是 8 个 16 MiB 的）----
    //
    // ⚠️ **期望值从描述查**（规格 §6.3 第 3 条：`base` / `size` /
    // `base + size` 三样都从描述查）。
    let (described_base, described_size) = described_high_ram(&qemu_virt_description());
    let high = ram
        .iter()
        .find(|r| r.base == described_base)
        .expect("必须有描述里那段高段 RAM 区域");
    assert_eq!(
        high.size, described_size,
        "高段 RAM 必须是**一个** 128 MiB 的区域，实际是 {} 字节",
        high.size
    );
    assert_eq!(
        high.base + high.size,
        described_base + described_size,
        "高段 RAM 必须**连续**（osz 的堆尾落在 0x8800_0000 附近）"
    );
    // ⚠️ **独立字面量交叉核对**（规格 §6.3 第 16 条）：那三份**自抄副本**
    // **故意不从描述取** ⇒ 描述漂了它们会红 ⇒ 它们才是「不与实现同源」
    // 的那道防线。
    assert_eq!(
        described_base, QEMU_VIRT_RAM_BASE,
        "qemu-virt 描述里高段 RAM 的 base 必须还是 0x8000_0000（独立字面量交叉核对）"
    );
    assert_eq!(
        described_size, QEMU_VIRT_RAM_BYTES,
        "qemu-virt 描述里高段 RAM 的 size 必须还是 128 MiB（独立字面量交叉核对）"
    );
    assert_eq!(
        described_base + described_size,
        QEMU_VIRT_RAM_END,
        "qemu-virt 描述里高段 RAM 的末尾必须还是 0x8800_0000（独立字面量交叉核对）"
    );

    // ---- 低段 RAM（复位向量）仍是 1 MiB @ 0x0 ----
    //
    // ⚠️ **期望值从描述查**（规格 §6.3 第 4 条）。
    let (low_base, low_size) = described_low_ram(&qemu_virt_description());
    let low = ram
        .iter()
        .find(|r| r.base == low_base)
        .expect("必须有 0x0 起的低段 RAM 区域");
    assert_eq!(
        low.size, low_size,
        "低段 RAM 必须是 1 MiB，实际是 {} 字节",
        low.size
    );

    // ---- **不许**再有 16 MiB 的碎片区域（旧实现的 8 段）----
    assert!(
        !map.regions.iter().any(|r| r.size == LEGACY_RAM_CHUNK_BYTES),
        "QemuVirt 档不许再有 16 MiB 的碎片区域：{:#?}",
        map.regions
    );
}

#[test]
fn the_default_profile_keeps_its_own_region_layout() {
    // ⚠️ **Default 档不受本批影响**（裁决 G 只动 QemuVirt 档）⇒ 这条
    // 用例钉住「没动」。
    let host = KernelHost::new();
    assert_eq!(host.probe_wiring_failures(), 0);
    let map = host.bus().export_map();
    // ⚠️ **期望值从 `default` 描述查**（规格 §6.3 第 5 条）。
    let described_count = described_regions(&default_description()).len();
    assert_eq!(
        map.regions.len(),
        described_count,
        "Default 档的区域数必须与**描述**一致（低段 RAM + 高段 RAM + GPIO），实际是 {}：{:#?}",
        map.regions.len(),
        map.regions
    );
    // ⚠️ **独立字面量交叉核对**（规格 §6.3 第 16 条的口径）。
    assert_eq!(
        described_count, DEFAULT_REGION_COUNT,
        "default 描述里的区域数必须还是 {DEFAULT_REGION_COUNT}（独立字面量交叉核对）"
    );
    // ⚠️ **期望值从描述查**（规格 §6.3 第 6 条：Default 高段 `size`）。
    let (described_base, described_size) = described_high_ram(&default_description());
    let high = map
        .regions
        .iter()
        .find(|r| r.base == described_base)
        .expect("Default 档也有高段 RAM 区域");
    assert_eq!(
        high.size, described_size,
        "Default 档的高段 RAM 必须是 4 MiB，实际是 {} 字节",
        high.size
    );
    // ⚠️ **独立字面量交叉核对**（规格 §6.3 第 16 条的口径）。
    assert_eq!(
        described_size, DEFAULT_HIGH_RAM_BYTES,
        "default 描述里高段 RAM 的 size 必须还是 4 MiB（独立字面量交叉核对）"
    );
}

#[test]
fn the_qemuvirt_profile_raises_the_per_region_guardrail_budget() {
    // ⚠️ **护栏预算是宿主策略**（裁决 B / G）：QemuVirt 档必须把每区域的
    // 上限抬到 128 MiB，否则「声明一段 128 MiB」会被 16 MiB 的默认值挡下
    // （`E_BUS-09`）⇒ 换档会**失败** ⇒ 静默退回默认档。
    let host = KernelHost::new_with_profile(MemoryProfile::QemuVirt);
    // ⚠️ **期望值从 `qemu-virt` 描述查**（规格 §6.3 第 7 条）：护栏预算是
    // 描述里的 `region_budget_bytes` ⇒ 宿主**不按 id 硬编码**预算。
    let described_budget = qemu_virt_description().region_budget_bytes;
    assert_eq!(
        host.bus().max_region_bytes(),
        u64::from(described_budget),
        "QemuVirt 档的每区域护栏预算必须等于**描述**里的 region_budget_bytes"
    );
    // ⚠️ **独立字面量交叉核对**（规格 §6.3 第 16 条的口径）：预算必须是
    // 128 MiB（= 那段高段 RAM 的大小）。
    assert_eq!(
        described_budget, QEMU_VIRT_RAM_BYTES,
        "qemu-virt 描述的 region_budget_bytes 必须还是 128 MiB（独立字面量交叉核对）"
    );

    // ⚠️ **Default 档的护栏没被削弱**（仍是默认的 16 MiB）。
    let default_host = KernelHost::new();
    // ⚠️ **期望值从 `default` 描述查**（规格 §6.3 第 7 条）。
    let described_default_budget = default_description().region_budget_bytes;
    assert_eq!(
        default_host.bus().max_region_bytes(),
        u64::from(described_default_budget),
        "Default 档的每区域护栏必须等于**描述**里的 region_budget_bytes"
    );
    // ⚠️ **独立字面量交叉核对**（规格 §6.3 第 16 条的口径）。
    assert_eq!(
        described_default_budget, DEFAULT_REGION_BUDGET_BYTES,
        "default 描述的 region_budget_bytes 必须还是 16 MiB（独立字面量交叉核对）"
    );
}
