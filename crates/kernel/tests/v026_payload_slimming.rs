//! v0.26 批 · **B-3 / S-2**：内存图通用化 A 批的**载荷瘦身**。
//!
//! 冻结规格：`docs/detailed-design/STAGE-11_V026_Memory_Map_A.md`
//! （裁决 C / G，验收 **B-3** 与本批专属 **S-2**）。
//!
//! ## 这条用例抓什么
//!
//! 台账 **D-123** 实测的代价：**QemuVirt 档一次会话快照 = 258.06 MiB 的
//! JSON**（RAM 段 129.00 MiB，**几乎全是零**）—— 因为旧 M-14 的
//! `export_pages` 把每段 RAM 按 64 KiB **整块导出、不跳过全零块** ⇒
//! guest 只用了几 KiB，快照却背着 258 MiB。
//!
//! **裁决 C** 改成 **4 KiB 页级懒分配** + **只导出写过的页**之后：
//!
//! - **一个字都没写过**的会话 ⇒ `mem_pages` **为空** ⇒ 整份 JSON **< 1 MiB**；
//! - 写过**一页** ⇒ `mem_pages` **恰好 1 条**、`bytes.len() == 4096`。
//!
//! ## ⚠️ 为什么必须**同时**断言「真的是 QemuVirt 布局」
//!
//! `KernelHost::new_with_profile` 的返回值**不是 `Result`**：换档失败时它
//! **静默退回默认档**（只留一个 `probe_wiring_failures` 计数，**不 panic**）
//! ⇒ 只断言「载荷小」的话，**降级成 Default 档也会绿** ⇒ 那就是**白测**。
//!
//! ⇒ 所以这条用例**先**断言 QemuVirt 的实际布局（`0x8000_0000` 起有
//! **一个** 128 MiB 的 RAM 区域、区域总数 6），**再**断言载荷。
//!
//! ## ⚠️ 断言不许靠「反推」
//!
//! 页大小、RAM 长度、JSON 上限全部写成**架构字面量**，不从被测实现反推。

use riscv_sim_contract::platform::{PlatformDescription, RegionSpec};
use riscv_sim_contract::session::{CheckpointMetaDto, SESSION_SNAPSHOT_SCHEMA_VERSION};

use riscv_sim_kernel::kernel_host::platform;
use riscv_sim_kernel::kernel_host::{KernelHost, MemoryProfile};
use riscv_sim_kernel::m14_bus::BusWidth;

// ---------------------------------------------------------------------------
// 平台描述（**取值来源**：从描述查，不抄宿主常量）
// ---------------------------------------------------------------------------

/// `qemu-virt` 描述。
fn qemu_virt_description() -> PlatformDescription {
    platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置描述 qemu-virt")
}

/// 描述里的**全部区域**（按 `base` 升序 —— 与 `export_map()` 的顺序一致）。
fn described_regions() -> Vec<RegionSpec> {
    platform::regions_by_base(&qemu_virt_description())
        .into_iter()
        .cloned()
        .collect()
}

/// 描述里的**高段 RAM**（`(base, size)`）—— 取**最大**的那段（无歧义，
/// 不靠下标）。
fn described_high_ram() -> (u32, u32) {
    let mut rams: Vec<(u32, u32)> = described_regions()
        .iter()
        .filter_map(|region| match region {
            RegionSpec::Ram { base, size } => Some((*base, *size)),
            _ => None,
        })
        .collect();
    rams.sort_by_key(|(_, size)| *size);
    *rams.last().expect("qemu-virt 描述里有 RAM")
}

// ---------------------------------------------------------------------------
// 架构字面量（**不从被测实现反推**）
// ---------------------------------------------------------------------------

/// QemuVirt 的 RAM 起点。
///
/// ⚠️ **自抄副本**（规格 §6.3 第 16 条）：它**故意不从描述取** ⇒ 它的
/// 作用是「**不与实现同源**」⇒ 描述漂了它会红。
/// ⚠️ **它与 `qemu-virt` 描述的对应关系**：描述里那段高段 RAM 的
/// `base`（`base = 0x8000_0000` / `size = 134217728`）。
const QEMU_VIRT_RAM_BASE: u32 = 0x8000_0000;

/// QemuVirt 的 RAM 长度（128 MiB）。
///
/// ⚠️ **自抄副本**（规格 §6.3 第 16 条）⇒ **不从描述取**。
/// ⚠️ **对应关系**：描述里那段高段 RAM 的 `size`。
const QEMU_VIRT_RAM_SIZE: u32 = 128 * 1024 * 1024;

/// 物化页的大小（4 KiB）。
const PAGE_BYTES: usize = 4096;

/// 整份 JSON 的上限（1 MiB）。
const JSON_LIMIT: usize = 1024 * 1024;

/// QemuVirt 档应有的区域总数（低段 RAM + 高段 RAM + GPIO + UART + CLINT + PLIC）。
///
/// ⚠️ **自抄副本**（规格 §6.3 第 16 条的口径）⇒ **不从描述取**。
/// ⚠️ **对应关系**：`qemu-virt` 描述里 `regions` 的条数。
const QEMU_VIRT_REGION_COUNT: usize = 6;

/// 检查点元数据（段 0）。
fn meta(host: &KernelHost) -> CheckpointMetaDto {
    CheckpointMetaDto {
        id: 1,
        t_virt: host.now_ps(),
        t_host: 0,
        base_seq: 0,
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
    }
}

/// 断言这个宿主**真的**铺的是 QemuVirt 布局（而不是静默降级成默认档）。
///
/// ⚠️ 这条断言是 B-3 / S-2 的**前提**：没有它，「载荷小」在降级时也成立。
fn assert_qemu_virt_layout(host: &KernelHost) {
    let map = host.bus().export_map();
    // ⚠️ **期望值从 `qemu-virt` 描述查**（规格 §6.3 第 8 条 · v0.27 ·
    // D-123-B）：内存图是**数据** ⇒ 断言的来源也必须是那份数据 ⇒ 描述
    // 的内容由 `tests_platform.rs` 的 B-2 用例（**字面量**）钉住。
    let described_count = described_regions().len();
    assert_eq!(
        map.regions.len(),
        described_count,
        "QemuVirt 档的区域数必须与**描述**一致 —— 若是 3，那是换档失败后\
         **静默退回默认档**了（`new_with_profile` 不 panic）"
    );
    // ⚠️ **独立字面量交叉核对**（规格 §6.3 第 16 条的口径）：描述的条数
    // 必须与**独立字面量**一致 ⇒ 描述漂了这条会红。
    assert_eq!(
        described_count, QEMU_VIRT_REGION_COUNT,
        "qemu-virt 描述里的区域数必须还是 {QEMU_VIRT_REGION_COUNT}（独立字面量交叉核对）"
    );

    // ⚠️ **期望值从描述查**（规格 §6.3 第 8 条）。
    let (described_base, described_size) = described_high_ram();
    let big_ram = map
        .regions
        .iter()
        .filter(|r| r.kind == "ram" && r.base == described_base)
        .collect::<Vec<_>>();
    assert_eq!(
        big_ram.len(),
        1,
        "QemuVirt 的 RAM 必须是**一个** 128 MiB 区域（不是 8 个 16 MiB 的）"
    );
    assert_eq!(
        big_ram[0].size, described_size,
        "QemuVirt 的 RAM 区域大小必须与**描述**一致"
    );
    // ⚠️ **独立字面量交叉核对**（规格 §6.3 第 16 条）：那两份**自抄副本**
    // **故意不从描述取** ⇒ 描述漂了它们会红。
    assert_eq!(
        described_base, QEMU_VIRT_RAM_BASE,
        "qemu-virt 描述里高段 RAM 的 base 必须还是 0x8000_0000（独立字面量交叉核对）"
    );
    assert_eq!(
        described_size, QEMU_VIRT_RAM_SIZE,
        "qemu-virt 描述里高段 RAM 的 size 必须还是 128 MiB（独立字面量交叉核对）"
    );
}

/// **B-3 / S-2**：一个字都没写过的 QemuVirt 会话 ⇒ 载荷里**没有**内存页。
#[test]
fn an_untouched_qemu_virt_session_carries_no_memory_pages() {
    let mut host = KernelHost::new_with_profile(MemoryProfile::QemuVirt);
    assert_qemu_virt_layout(&host);

    let payload = host.capture_session(meta(&host));

    assert!(
        payload.m13_state.mem_pages.is_empty(),
        "没写过任何字节的会话不应带内存页，实际带了 {} 页",
        payload.m13_state.mem_pages.len()
    );

    let json = serde_json::to_string(&payload).expect("会话载荷必须能序列化成 JSON");
    assert!(
        json.len() < JSON_LIMIT,
        "空会话的 JSON 应小于 {JSON_LIMIT} 字节，实际 {} 字节（基线是 258.06 MiB）",
        json.len()
    );
}

/// **B-3 的反面**：写过**一页** ⇒ 载荷里**恰好一页**，且是 4 KiB。
#[test]
fn one_written_page_shows_up_as_exactly_one_page() {
    let mut host = KernelHost::new_with_profile(MemoryProfile::QemuVirt);
    assert_qemu_virt_layout(&host);

    host.bus_mut()
        .write(QEMU_VIRT_RAM_BASE, BusWidth::Byte, 0xAA)
        .expect("往 QemuVirt 的 RAM 写一个字节必须成功");

    let payload = host.capture_session(meta(&host));

    assert_eq!(
        payload.m13_state.mem_pages.len(),
        1,
        "只写了一个字节 ⇒ 物化一页 ⇒ 载荷里恰好一页"
    );
    let page = &payload.m13_state.mem_pages[0];
    assert_eq!(
        page.base, QEMU_VIRT_RAM_BASE,
        "页的基址必须是它那一页的起点"
    );
    assert_eq!(
        page.bytes.len(),
        PAGE_BYTES,
        "一个物化页必须是 {PAGE_BYTES} 字节"
    );
    assert_eq!(page.bytes[0], 0xAA, "页里的第一个字节必须是写进去的那个值");
}
