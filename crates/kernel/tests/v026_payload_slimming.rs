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

use riscv_sim_contract::session::{CheckpointMetaDto, SESSION_SNAPSHOT_SCHEMA_VERSION};

use riscv_sim_kernel::kernel_host::{KernelHost, MemoryProfile};
use riscv_sim_kernel::m14_bus::BusWidth;

// ---------------------------------------------------------------------------
// 架构字面量（**不从被测实现反推**）
// ---------------------------------------------------------------------------

/// QemuVirt 的 RAM 起点。
const QEMU_VIRT_RAM_BASE: u32 = 0x8000_0000;

/// QemuVirt 的 RAM 长度（128 MiB）。
const QEMU_VIRT_RAM_SIZE: u32 = 128 * 1024 * 1024;

/// 物化页的大小（4 KiB）。
const PAGE_BYTES: usize = 4096;

/// 整份 JSON 的上限（1 MiB）。
const JSON_LIMIT: usize = 1024 * 1024;

/// QemuVirt 档应有的区域总数（低段 RAM + 高段 RAM + GPIO + UART + CLINT + PLIC）。
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
    assert_eq!(
        map.regions.len(),
        QEMU_VIRT_REGION_COUNT,
        "QemuVirt 档应有 {QEMU_VIRT_REGION_COUNT} 个区域（低段 RAM + 高段 RAM + GPIO + UART + CLINT + PLIC）\
         —— 若是 3，那是换档失败后**静默退回默认档**了（`new_with_profile` 不 panic）"
    );
    let big_ram = map
        .regions
        .iter()
        .filter(|r| r.kind == "ram" && r.base == QEMU_VIRT_RAM_BASE)
        .collect::<Vec<_>>();
    assert_eq!(
        big_ram.len(),
        1,
        "QemuVirt 的 RAM 必须是**一个** 128 MiB 区域（不是 8 个 16 MiB 的）"
    );
    assert_eq!(
        big_ram[0].size, QEMU_VIRT_RAM_SIZE,
        "QemuVirt 的 RAM 区域大小必须是 128 MiB"
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
