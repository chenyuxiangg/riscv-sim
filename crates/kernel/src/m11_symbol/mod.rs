//! M-11 符号表 / DWARF / 反汇编 / trace 导出 (Symbol / DWARF / Disasm / Trace).
//!
//! 落地规格：`docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`（冻结）。
//!
//! ## 四个子组件（LLD 评审 P-07 的双子组件拆分 + 本批的进一步细分）
//!
//! - [`symbols`] —— `.symtab` 解析与名称/地址双向查询
//! - [`render`] —— 助记符渲染器（`Decoded` → 文本）
//! - [`disasm`] —— 反汇编序列、按地址缓存、自修改代码失效
//! - [`dwarf`] —— DWARF 行表（v2/v3/v4）与地址→行查询
//! - [`trace`] —— 执行轨迹环形缓冲与导出
//!
//! ## 两条不变量
//!
//! 1. **解码的唯一真相是 M-13**（STAGE-5 裁决 A）：本模块**不维护第二张
//!    解码表**，只把 M-13 的 [`riscv_sim_kernel::m13_iss::Decoded`] 渲染成
//!    文本。⇒ 两套解码语义漂移的风险**结构上不存在**。
//! 2. **本模块不依赖 M-19**（STAGE-5 裁决 D）：波形导出由宿主直接调 M-19，
//!    执行轨迹导出由本模块承担 ⇒ 依赖方向与 LLD §9.1 一致。

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{LineEntry, Symbol};

pub mod disasm;
pub mod dwarf;
pub mod render;
pub mod symbols;
pub mod trace;

// 本 lane（C1）拥有的三个子组件的公开出口。
//
// ⚠️ `dwarf` / `trace` 的 `pub use` 由统筹方统一补（C2 拥有那两个文件），
// 这里**不**加，免得两边同时改同一段。
pub use disasm::{Disassembler, MAX_DISASM_COUNT};
pub use render::{render_entry, render_mnemonic};
pub use symbols::SymbolTable;

// C2 的两个子组件的公开出口（统筹方统一补——C2 拥有那两个文件）。
pub use dwarf::{DwarfLines, DwarfStatus};
pub use trace::{ExportChunk, TraceBuffer, TraceEntry, TraceKind, TRACE_CAP};

/// Result of an address → source-line lookup.
///
/// The **two "no answer" states are distinct on purpose** (LLD 11-4):
/// - [`LineLookup::NoDwarf`] — there is no DWARF at all → the contract
///   reports `file = ""`, `line = 0` (degraded: disassembly still works).
/// - [`LineLookup::Missed`] — DWARF *is* present but the address missed
///   the line table → the contract reports `file = ""`, `line = u32::MAX`.
///
/// Collapsing them would lose the difference between "this build has no
/// source info" and "this address is not on a source line".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineLookup {
    /// No DWARF in the loaded image (degraded, not an error).
    NoDwarf,
    /// The address resolved to a source location.
    Found(LineEntry),
    /// DWARF is present but the address is not covered by the line table.
    Missed,
}

/// Public port of M-11's symbol / source-line services.
///
/// ⚠️ The **trace export** is deliberately *not* on this trait: the
/// waveform export reads M-19's ring and is routed by the host
/// assembly layer (STAGE-5 裁决 D), so putting it here would give M-11
/// a dependency on M-19.
pub trait SymbolService {
    /// Resolve an address to a source location (DWARF line table).
    fn resolve_line(&self, addr: u32) -> Result<LineLookup, PlatformError>;
    /// Look up a symbol by name.
    fn lookup_symbol(&self, name: &str) -> Result<Option<Symbol>, PlatformError>;
    /// Look up a symbol by address.
    fn symbol_at(&self, addr: u32) -> Result<Option<Symbol>, PlatformError>;
}

/// Family used by errors raised from this module.
pub const SYMBOL_FAMILY: ErrorFamily = ErrorFamily::Loader;

/// [`SymbolService`] 的**生产实现**——`SymbolTable` 自己的适配器。
///
/// ⚠️ **为什么要有这一层**（v0.20 检视 B · D-4）：`SymbolService` 是 M-11
/// 对外的**端口**（LLD §9.1），但此前**全仓 0 处实现** ⇒ 端口是**死代码**
/// ⇒ 「按地址查符号」这条能力**没有生产路径**（验收 #6 只在 M-11 单元层
/// 被覆盖）。
///
/// ⇒ 宿主走**这个端口**取符号 ⇒ 端口有实现者、也有调用者。
impl SymbolService for SymbolTable {
    fn resolve_line(&self, _addr: u32) -> Result<LineLookup, PlatformError> {
        // ⚠️ **Wave 1 的诚实口径**：M-11 的 `DwarfLines::resolve` 恒返回
        // 降级态（真正的行表解析是 Wave 2）⇒ 端口这一侧**恒**是
        // `NoDwarf` ⇒ **绝不**返回 `Missed`（那等于宣称「查过行表、没命中」）。
        Ok(LineLookup::NoDwarf)
    }

    fn lookup_symbol(&self, name: &str) -> Result<Option<Symbol>, PlatformError> {
        // ⚠️ 未知名 ⇒ `None`（**不是错误**，验收 #7）。
        Ok(self.lookup(name).cloned())
    }

    fn symbol_at(&self, addr: u32) -> Result<Option<Symbol>, PlatformError> {
        // ⚠️ 命中「起始地址 ≤ `addr` 且最近的那个」；比所有符号都小 ⇒ `None`。
        Ok(self.at(addr).cloned())
    }
}
