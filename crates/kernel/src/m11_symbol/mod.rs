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
pub use dwarf::{DwarfLines, DwarfStatus, UnitHeaderInfo};
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
    /// Look up symbols by **name prefix** (v0.57 批 · STAGE-42 §6.2).
    ///
    /// * **前缀**匹配（**不是**子串匹配）；
    /// * 结果按**名字升序**；
    /// * 条数 **≤ `limit`**。
    ///
    /// ⚠️ **精确命中也会在结果里** ⇒ ⇒ 「候选里**不含**精确命中那条」是
    /// **调用方**（宿主）的事。
    fn lookup_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<Symbol>, PlatformError>;
}

/// Family used by errors raised from this module.
pub const SYMBOL_FAMILY: ErrorFamily = ErrorFamily::Loader;

/// M-11 的**索引**：符号表 + DWARF 行表 —— [`SymbolService`] 端口的
/// **生产实现**。
///
/// ⚠️ **为什么要有这一层**（v0.20 检视 B · D-4）：`SymbolService` 是 M-11
/// 对外的**端口**（LLD §9.1），此前**全仓 0 处实现** ⇒ 端口是**死代码**
/// ⇒ 「按地址查符号」这条能力**没有生产路径**（验收 #6 只在 M-11 单元层
/// 被覆盖）。
///
/// ⚠️ **Wave 2 的诚实性**（STAGE-7 裁决 J）：端口上的 `resolve_line`
/// **必须**给出**真的**判定 —— 「查过行表、没命中」是 `Missed`，「没查」
/// 是 `NoDwarf`，两态**不得混**。
///
/// ⇒ 之前这个端口是**挂在 [`SymbolTable`] 上**的，而 `SymbolTable` **不含**
/// 行表 ⇒ 端口的 `resolve_line` **只能**恒返回 `NoDwarf`（那是在**撒谎**：
/// 这个镜像明明有 DWARF）。⇒ 现在端口挂在**同时持有两者**的
/// [`SymbolIndex`] 上 ⇒ 端口有实现者、也有调用者，而且**说的是真话**。
// ⚠️ **不** derive `PartialEq` / `Eq`：`SymbolTable`（Lane C 拥有）没有
// 实现它们 ⇒ 这里不能凭空要求。
#[derive(Debug, Clone, Default)]
pub struct SymbolIndex {
    /// `.symtab` 的解析结果。
    symbols: SymbolTable,
    /// `.debug_line` 的解析结果（Wave 2 起**真的**解析行表）。
    dwarf: DwarfLines,
}

impl SymbolIndex {
    /// 从 M-06 带出的调试节区构建（符号表 + 行表）。
    ///
    /// ⚠️ 两者**一起**算出来再落位 ⇒ 不会留下「符号是新的、行表是旧的」
    /// 这种拼接体（宿主装载新工程时走这条路）。
    pub fn from_sections(sections: &[(String, Vec<u8>)]) -> Result<Self, PlatformError> {
        Ok(Self {
            symbols: SymbolTable::from_sections(sections)?,
            dwarf: DwarfLines::from_sections(sections)?,
        })
    }

    /// 符号表（只读）。
    pub fn symbols(&self) -> &SymbolTable {
        &self.symbols
    }

    /// DWARF 行表（只读）。
    pub fn dwarf(&self) -> &DwarfLines {
        &self.dwarf
    }

    /// 符号表里有多少条符号。
    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    /// DWARF 的状态（`Absent` = 没有 `.debug_line`）。
    pub fn dwarf_status(&self) -> DwarfStatus {
        self.dwarf.status()
    }

    /// 地址 → **源码行条目**（三态 ⇒ 两态的**唯一**映射点）。
    ///
    /// * [`LineLookup::Found`] ⇒ `Some(entry)`（**真的** `file` + `line`）；
    /// * [`LineLookup::Missed`] ⇒ `None`（查过行表、没命中）；
    /// * [`LineLookup::NoDwarf`] ⇒ `None`（压根没有 DWARF）；
    /// * 出错 ⇒ `None`。
    ///
    /// ⚠️ **两态不得混**（STAGE-7 裁决 J · STAGE-9 裁决 C）：上面两个
    /// `None` 在**这个返回值上确实分不出来** ⇒ **要**分请用
    /// [`SymbolService::resolve_line`]（它给回三态）。
    ///
    /// ⚠️ **绝不编造**：查不到就是 `None`——**不**返回 `Some("")`、
    /// **不**返回 `Some(0)`、**不**拿别的地址的行号顶替。
    ///
    /// ⚠️ 这个方法是**给反汇编器用的**（契约 v4 · D-102 / D-121）：
    /// [`crate::m11_symbol::Disassembler::disasm_with_lines`] 的解析器
    /// 直接接它 ⇒ 「`file` / `line` 由 M-11 填、宿主不猜」。
    pub fn resolve_line_entry(&self, addr: u32) -> Option<LineEntry> {
        match self.resolve_line(addr) {
            // ✅ 命中 ⇒ 给**真的**行。
            Ok(LineLookup::Found(entry)) => Some(entry),
            // ⚠️ 查过、没命中 ⇒ `None`（**不**拿上一行的行号顶替）。
            Ok(LineLookup::Missed) => None,
            // ⚠️ 压根没有 DWARF ⇒ `None`。
            Ok(LineLookup::NoDwarf) => None,
            // ⚠️ 出错 ⇒ `None`（**不猜**）。
            Err(_) => None,
        }
    }
}

impl SymbolService for SymbolIndex {
    fn resolve_line(&self, addr: u32) -> Result<LineLookup, PlatformError> {
        // ⚠️ **三态如实区分**（STAGE-7 裁决 J）：`DwarfLines::resolve`
        // 自己分得清「查过没命中」（`Missed`）与「没查」（`NoDwarf`）⇒
        // 端口**原样透传**，**不自己编造**。
        Ok(self.dwarf.resolve(addr))
    }

    fn lookup_symbol(&self, name: &str) -> Result<Option<Symbol>, PlatformError> {
        // ⚠️ 未知名 ⇒ `None`（**不是错误**，验收 #7）。
        Ok(self.symbols.lookup(name).cloned())
    }

    fn symbol_at(&self, addr: u32) -> Result<Option<Symbol>, PlatformError> {
        // ⚠️ 命中「起始地址 ≤ `addr` 且最近的那个」；比所有符号都小 ⇒ `None`。
        Ok(self.symbols.at(addr).cloned())
    }

    fn lookup_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<Symbol>, PlatformError> {
        // ⚠️ 符号表解析失败**不在这里**发生（表已经建好了）⇒ ⇒ 这一条
        //   **不会**出错 ⇒ ⇒ 端口仍然返回 `Result`（与另外两条同形）。
        Ok(self.symbols.lookup_prefix(prefix, limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m11_symbol::dwarf::fixtures::V2_DL;

    /// v2 夹具的节区集合（只有 `.debug_line`）。
    fn v2_sections() -> Vec<(String, Vec<u8>)> {
        vec![(".debug_line".to_string(), V2_DL.to_vec())]
    }

    #[test]
    fn resolve_line_entry_gives_the_true_entry_when_the_line_table_hits() {
        // ⚠️ 断言值是**写死的字面量**（不从被测实现反推）。
        let index = SymbolIndex::from_sections(&v2_sections()).expect("索引");
        let entry = index
            .resolve_line_entry(0x0001_0094)
            .expect("命中 ⇒ 必须给出真行");
        assert_eq!(entry.addr, 0x0001_0094);
        assert_eq!(entry.file, "src/main.c");
        assert_eq!(entry.line, 1);
    }

    #[test]
    fn resolve_line_entry_is_none_when_the_line_table_misses() {
        // 「查过行表、没命中」⇒ `None`（**不**拿别的地址的行号顶替）。
        let index = SymbolIndex::from_sections(&v2_sections()).expect("索引");
        assert_eq!(index.dwarf_status(), DwarfStatus::Supported(2));
        // ⚠️ 0x0000_0000 **不在**任何一段行号序列里 ⇒ 真「没命中」。
        assert_eq!(
            index.resolve_line(0x0000_0000).expect("端口"),
            LineLookup::Missed,
            "前提：这一地址是 `Missed`（不是 `NoDwarf`）"
        );
        assert_eq!(
            index.resolve_line_entry(0x0000_0000),
            None,
            "查过、没命中 ⇒ `None`"
        );
    }

    #[test]
    fn resolve_line_entry_is_none_when_there_is_no_dwarf_at_all() {
        // 「压根没有 DWARF」⇒ `None`（**不查**）。
        let index = SymbolIndex::from_sections(&[]).expect("索引");
        assert_eq!(index.dwarf_status(), DwarfStatus::Absent);
        assert_eq!(
            index.resolve_line(0x0001_0094).expect("端口"),
            LineLookup::NoDwarf,
            "前提：这一状态是 `NoDwarf`（不是 `Missed`）"
        );
        assert_eq!(index.resolve_line_entry(0x0001_0094), None);
    }

    #[test]
    fn the_two_no_answers_stay_distinguishable_on_the_port() {
        // ⚠️ **两态不得混**（STAGE-7 裁决 J · STAGE-9 裁决 C）：DTO 上两态
        // 都是 `None`（契约形状决定）⇒ **端口**上必须还能分出来。
        let with_dwarf = SymbolIndex::from_sections(&v2_sections()).expect("索引");
        let without_dwarf = SymbolIndex::from_sections(&[]).expect("索引");

        assert_eq!(
            with_dwarf.resolve_line(0x0000_0000).expect("端口"),
            LineLookup::Missed
        );
        assert_eq!(
            without_dwarf.resolve_line(0x0000_0000).expect("端口"),
            LineLookup::NoDwarf
        );
        // 两者在 `resolve_line_entry` 上**都**是 `None` ⇒ 那一层**不**能
        // 分（这是契约形状的必然，不是实现偷懒）。
        assert_eq!(with_dwarf.resolve_line_entry(0x0000_0000), None);
        assert_eq!(without_dwarf.resolve_line_entry(0x0000_0000), None);
    }
}
