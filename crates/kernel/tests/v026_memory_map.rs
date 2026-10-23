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

use riscv_sim_contract::session::{CheckpointMetaDto, MemPageDto};
use riscv_sim_kernel::kernel_host::{KernelHost, MemoryProfile};
use riscv_sim_kernel::m14_bus::{Bus, BusWidth};

// ---------------------------------------------------------------------------
// 架构字面量（**不从被测实现反推**）
// ---------------------------------------------------------------------------

/// 物理页大小（4 KiB）—— 快照页的**对齐**粒度。
const PAGE_BYTES: u32 = 4096;

/// 128 MiB —— QEMU virt 档那一整段 RAM 的大小。
const QEMU_VIRT_RAM_BYTES: u32 = 128 * 1024 * 1024;

/// QEMU virt 档的 RAM 起点（ITCM）。
const QEMU_VIRT_RAM_BASE: u32 = 0x8000_0000;

/// QEMU virt 档 RAM 段的**末尾 + 1**（osz 的堆尾落在 `0x8800_0000` 附近）。
const QEMU_VIRT_RAM_END: u32 = 0x8800_0000;

/// 旧实现里那 8 段 16 MiB 的**碎片**大小 —— v0.26 之后**不许**再出现。
const LEGACY_RAM_CHUNK_BYTES: u32 = 16 * 1024 * 1024;

/// QEMU virt 档的区域总数（1 低段 RAM + 1 高段 RAM + GPIO + UART + CLINT
/// + PLIC）。
const QEMU_VIRT_REGION_COUNT: usize = 6;

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
    assert_eq!(
        map.regions.len(),
        QEMU_VIRT_REGION_COUNT,
        "QemuVirt 档的区域数必须是 6（1 低段 RAM + 1 高段 RAM + GPIO + UART + CLINT + PLIC），\
         实际是 {}：{:#?}",
        map.regions.len(),
        map.regions
    );

    // ---- RAM 区域恰好两个（低段 + 高段）----
    let ram: Vec<_> = map.regions.iter().filter(|r| r.kind == "ram").collect();
    assert_eq!(
        ram.len(),
        2,
        "QemuVirt 档的 RAM 区域必须是 2 个（低段 1 MiB + 高段 128 MiB），实际是 {}：{:#?}",
        ram.len(),
        ram
    );

    // ---- 高段 RAM 是**一个** 128 MiB 的区域（不是 8 个 16 MiB 的）----
    let high = ram
        .iter()
        .find(|r| r.base == QEMU_VIRT_RAM_BASE)
        .expect("必须有 0x8000_0000 起的 RAM 区域");
    assert_eq!(
        high.size, QEMU_VIRT_RAM_BYTES,
        "高段 RAM 必须是**一个** 128 MiB 的区域，实际是 {} 字节",
        high.size
    );
    assert_eq!(
        high.base + high.size,
        QEMU_VIRT_RAM_END,
        "0x8000_0000 起必须**连续** 128 MiB（osz 的堆尾落在 0x8800_0000 附近）"
    );

    // ---- 低段 RAM（复位向量）仍是 1 MiB @ 0x0 ----
    let low = ram
        .iter()
        .find(|r| r.base == 0)
        .expect("必须有 0x0 起的低段 RAM 区域");
    assert_eq!(
        low.size,
        1024 * 1024,
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
    assert_eq!(
        map.regions.len(),
        3,
        "Default 档仍是 3 个区域（低段 RAM + 高段 RAM + GPIO），实际是 {}：{:#?}",
        map.regions.len(),
        map.regions
    );
    let high = map
        .regions
        .iter()
        .find(|r| r.base == QEMU_VIRT_RAM_BASE)
        .expect("Default 档也有 0x8000_0000 起的 RAM 区域");
    assert_eq!(
        high.size,
        4 * 1024 * 1024,
        "Default 档的高段 RAM 仍是 4 MiB，实际是 {} 字节",
        high.size
    );
}

#[test]
fn the_qemuvirt_profile_raises_the_per_region_guardrail_budget() {
    // ⚠️ **护栏预算是宿主策略**（裁决 B / G）：QemuVirt 档必须把每区域的
    // 上限抬到 128 MiB，否则「声明一段 128 MiB」会被 16 MiB 的默认值挡下
    // （`E_BUS-09`）⇒ 换档会**失败** ⇒ 静默退回默认档。
    let host = KernelHost::new_with_profile(MemoryProfile::QemuVirt);
    assert_eq!(
        host.bus().max_region_bytes(),
        u64::from(QEMU_VIRT_RAM_BYTES),
        "QemuVirt 档的每区域护栏预算必须抬到 128 MiB"
    );

    // ⚠️ **Default 档的护栏没被削弱**（仍是默认的 16 MiB）。
    let default_host = KernelHost::new();
    assert_eq!(
        default_host.bus().max_region_bytes(),
        16 * 1024 * 1024,
        "Default 档的每区域护栏必须保持默认的 16 MiB"
    );
}
