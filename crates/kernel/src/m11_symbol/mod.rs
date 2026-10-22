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
