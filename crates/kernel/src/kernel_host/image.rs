//! 镜像解析产物与调试面的纯函数映射（不持有、不访问宿主私有字段）。
//!
//! ⚠️ 这里的类型 / 函数都是内核内部装配用，不是公开 API ⇒ ⇒ 用
//! `pub(crate)` 对父模块 [`super::KernelHost`] 可见。

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{TraceFormat, TraceSource};
use riscv_sim_contract::protocol::EventPayload;

use crate::m06_elf::ElfIsaFacts;
use crate::m11_symbol::trace::{E_CTX_03_ILLEGAL_EXPORT_FORMAT, TRACE_FAMILY};
use crate::m11_symbol::LineLookup;
use crate::m14_bus::{Bus, BusWidth};
use crate::m19_waveform::ExportHandle;

/// M-06 解析出的**调试节区**（名字 + 原始字节）。
///
/// ⚠️ 它**不过契约层**（契约只传查询结果，不传节区字节）⇒ 在**内核
/// 内部**从 M-06 交给 M-11（STAGE-5 §4 的数据流）。
pub(crate) type DebugSections = Vec<(String, Vec<u8>)>;

/// **解析后**的镜像（**还没写进总线**）。
///
/// ⚠️ 「解析」与「写入」分成两步，是因为内存图档位要按镜像用到的
/// **地址**挑 —— 见 [`super::KernelHost::parse_image`]。
pub(crate) struct ParsedImage {
    /// 入口地址（raw blob 是 0）。
    pub(crate) entry: u32,
    /// `(物理地址, 段内容)`。
    pub(crate) segments: Vec<(u32, Vec<u8>)>,
    /// 调试节区的原始字节（`(节名, 内容)`）。
    pub(crate) debug_sections: DebugSections,
    /// ELF 头的 ISA 事实（v0.75 · STAGE-51 · D-175 的 F）；非 ELF
    /// （raw/hex）为缺省（`bits = 0` ⇒ ISA 门禁跳过它）。
    pub(crate) elf_isa: ElfIsaFacts,
}

/// 同一工程**多个镜像之间**的装载段重叠检查。
///
/// M-06 已经在单个镜像内拒绝重叠；这里补上跨镜像的检查，因为宿主
/// 是逐镜像解析、最后统一写总线的。重叠一律 `E_LDR-01`，且在复位 /
/// 建图之前返回，机器状态不动。
pub(crate) fn load_segment_overlap_error(parsed: &[ParsedImage]) -> Option<PlatformError> {
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    for p in parsed {
        for (addr, bytes) in &p.segments {
            if bytes.is_empty() {
                continue;
            }
            let start = *addr as u64;
            let end = start.saturating_add(bytes.len() as u64);
            ranges.push((start, end));
        }
    }
    ranges.sort_by_key(|(start, _)| *start);
    for window in ranges.windows(2) {
        let (prev_start, prev_end) = window[0];
        let (start, end) = window[1];
        if start < prev_end {
            return Some(PlatformError::new(
                ErrorFamily::Loader,
                1,
                format!(
                    "overlapping load segments across images: \
                     [0x{prev_start:08x}..0x{prev_end:08x}) and [0x{start:08x}..0x{end:08x})"
                ),
            ));
        }
    }
    None
}

/// 取指 / 读内存的**宽度映射**。
///
/// ⚠️ `BusWidth` **没有** `From<u32>` ⇒ 宽度要自己映射。
/// ⚠️ **必须真的只读 `size` 字节**（「读一个字取低半」会让映射区**末端**
/// 的压缩指令越界 ⇒ 被误报成「未映射」，LLD 11-1）。
pub(crate) fn fetch_bytes(bus: &Bus, addr: u32, size: u32) -> Result<u32, PlatformError> {
    let width = match size {
        1 => BusWidth::Byte,
        2 => BusWidth::Half,
        4 => BusWidth::Word,
        other => {
            return Err(PlatformError::new(
                ErrorFamily::Bus,
                1,
                format!("取指宽度 {other} 字节不受支持（只支持 1 / 2 / 4）"),
            ))
        }
    };
    // ⚠️ 用 `read_only`（不计入统计 / 不触发总线 hook）：调试器的读**不是**
    // 访客的访存 ⇒ 它不能进 M-18 的访存 trace 环，也不能进观察点匹配。
    bus.read_only(addr, width)
}

/// M-11 的行表判定 → 契约事件的**唯一**映射点（LLD 11-4 的两态）。
///
/// ⚠️ 抽成自由函数是为了让它**可测**：Wave 1 的 `DwarfLines::resolve` 恒
/// 返回降级态 ⇒ 「有 DWARF 但未命中」这一态在生产路径上**走不到**，
/// 只有直接喂三种 `LineLookup` 才能钉住这个映射。
pub(crate) fn source_location_event(addr: u32, lookup: LineLookup) -> EventPayload {
    match lookup {
        LineLookup::Found(entry) => EventPayload::SourceLocation {
            addr,
            file: entry.file,
            line: entry.line,
        },
        // 有 DWARF 但地址没命中 ⇒ `line = u32::MAX`（**不是** 0）。
        LineLookup::Missed => EventPayload::SourceLocation {
            addr,
            file: String::new(),
            line: u32::MAX,
        },
        // 压根没有 DWARF ⇒ `line = 0`。
        LineLookup::NoDwarf => EventPayload::SourceLocation {
            addr,
            file: String::new(),
            line: 0,
        },
    }
}

/// 「这个 `source` / `format` 组合不合法」的错误（`E_CTX-03`）。
///
/// ⚠️ **复用 M-11 已定义的常量**（一条错误码一个真相）：宿主**不**再
/// 定义一份 `E_CTX-03`——M-11 的 `trace` 模块里那份才是权威。
pub(crate) fn illegal_export(message: &str) -> PlatformError {
    PlatformError::new(
        TRACE_FAMILY,
        E_CTX_03_ILLEGAL_EXPORT_FORMAT,
        message.to_string(),
    )
}

/// M-19 的 `ExportHandle` → 契约的 `TraceExport` 事件（**唯一的映射点**）。
///
/// ⚠️ **为什么抽成自由函数**：这条映射里有**两处**「形状对不上」需要人工
/// 翻译 —— M-19 的 `ExportHandle` **没有** `offset` 字段（用请求里的回显）
/// **也没有** `bytes` 字段（用块的长度）。把它们藏在一个方法里 ⇒ 没人能
/// 单独测这条映射 ⇒ **v0.20 检视 B 实测**：把 `overflow_marked` 改成
/// `false`，**795 + 5 条用例全绿** ⇒ 「导出窗口不完整却没人知道」正是
/// 这个字段要防的事，而它在 `Waveform` 这一侧**没有判别力**。
///
/// ⚠️ 抽出来之后，那条映射**可以被单独钉住**（见 `waveform_export_event`
/// 的用例）。
pub(crate) fn waveform_export_event(
    handle: ExportHandle,
    format: TraceFormat,
    offset: u64,
) -> EventPayload {
    EventPayload::TraceExport {
        source: TraceSource::Waveform,
        format,
        // ⚠️ M-19 的 `ExportHandle` **没有** `offset` 字段 ⇒ 用**请求
        // 里的** offset 回显（它就是本块的起点）。
        offset,
        next_offset: handle.next_offset as u64,
        total: handle.total as u64,
        // ⚠️ M-19 的 `ExportHandle` **没有** `bytes` 字段 ⇒ 用块的长度。
        bytes: handle.chunk.len() as u64,
        chunk: handle.chunk,
        // ⚠️ **窗口不完整的标记必须原样传出去**（D-50 那一侧的「不完整」
        // 也得看得见）——这一行**曾经**没有任何判别力。
        overflow_marked: handle.overflow_marked,
    }
}
