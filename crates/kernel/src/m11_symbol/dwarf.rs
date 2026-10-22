//! M-11 DWARF 行表子组件 —— **Wave 1 的降级态**（不解析行表）。
//!
//! 冻结规格：`docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`
//! （§1 Wave 1 / Wave 2 的切分 · §2 裁决 F「DWARF 只支持 v2/v3/v4，
//! v5 明确降级」· §5.2 验收 #3「无 DWARF 独立可用」）。
//! 模块设计：`docs/detailed-design/M-11_SymbolDwarfTrace.html`
//! （§3.2 `resolveLine` 的两态语义 · 11-4）。
//!
//! # ⚠️ 本模块**现在**做什么（Wave 1）
//!
//! **只识别 `.debug_line` 节区在不在、版本是几** —— **不解析行表**。
//!
//! 理由：DWARF 行表解析是本模块 **10 人日的最大风险项**（评审 P-07），
//! 而 Wave 1 的目标是「反汇编面板 + 导出」能用 ⇒ 行表**降级**即可。
//!
//! # ⚠️ 本模块**将来**做什么（Wave 2）
//!
//! 解析 `.debug_line` 的行表程序（v2 / v3 / v4），把
//! [`DwarfLines::lookup_line`] 填上 ⇒ [`DwarfLines::resolve`] 从
//! 「恒降级」变成「能给出源码行」。
//!
//! # 三态 + 一个畸形态
//!
//! [`DwarfStatus`] 是**如实记录**的「这个 ELF 的 DWARF 到底怎么样」：
//!
//! - [`DwarfStatus::Absent`] —— 根本没有 `.debug_line`；
//! - [`DwarfStatus::Supported(v)`] —— 识别出 `.debug_line`，版本 v2/v3/v4
//!   （**本批只识别，不解析**）；
//! - [`DwarfStatus::UnsupportedVersion(v)`] —— 识别出 `.debug_line`，但
//!   版本不在支持集（**v5 走这里**，裁决 F）；
//! - [`DwarfStatus::Malformed`] —— 节区在，但头部**读不出来**（截断 /
//!   `unit_length` 越界）⇒ **降级**，**不是错误**。
//!
//! # 「降级」的含义（重要）
//!
//! [`DwarfLines::resolve`] 在 Wave 1 **恒**返回 [`LineLookup::NoDwarf`]：
//!
//! - 「这个 ELF 的 DWARF 我不支持」（v5）与「这个 ELF 没有 DWARF」
//!   对调用方是**同一件事**（都拿不到源码行）⇒ 同一态；
//! - 「有 DWARF（v2/v3/v4）但本批不解析」**也是** `NoDwarf` ——
//!   ⚠️ **绝不能**报 [`LineLookup::Missed`]（那等于宣称「我查过行表，
//!   这个地址确实不在里面」—— 我们**没查**）。
//!
//! ⛔ **绝不允许**「部分解析 v5 然后给出错误的行号」—— 那比没有行号更糟。
//!
//! # 畸形输入 ⇒ 降级，不是错误
//!
//! `.debug_line` 被截断、版本读不出来 ⇒ [`DwarfStatus::Malformed`]，
//! [`DwarfLines::from_sections`] **仍然返回 `Ok`**。理由：「解析不出来」
//! ≠「加载失败」—— 一个没有行表的 ELF 依然能反汇编、能导出（验收 #3）。

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::LineEntry;

use super::LineLookup;

/// M-06 带出的 DWARF 行表节区名（白名单，裁决 B）。
pub const DEBUG_LINE_SECTION: &str = ".debug_line";

/// DWARF 版本支持集（裁决 F：只支持 v2 / v3 / v4）。
///
/// ⚠️ **v5 不在支持集里** —— 本机工具链默认产出 DWARF 5（产物含
/// `.debug_line_str`），v5 的行表头（目录 / 文件表用 `DW_FORM_line_strp`）
/// 与 v2~v4 的行表程序**结构不同** ⇒ v5 **明确降级**，不猜。
pub const SUPPORTED_VERSIONS: &[u16] = &[2, 3, 4];

/// DWARF 行表子组件的状态（**如实记录**「这个 ELF 的 DWARF 怎么样」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DwarfStatus {
    /// 没有 `.debug_line` 节区 ⇒ 压根没有 DWARF。
    Absent,
    /// 识别出 `.debug_line`，版本在支持集内（v2 / v3 / v4）。
    ///
    /// ⚠️ **Wave 1 只识别，不解析** —— [`DwarfLines::resolve`] 仍然
    /// 降级为 [`LineLookup::NoDwarf`]。
    Supported(u16),
    /// 识别出 `.debug_line`，但版本**不在**支持集（v5 及以上 ⇒ 裁决 F
    /// 的明确降级）。
    UnsupportedVersion(u16),
    /// `.debug_line` 在，但头部**读不出来**（截断 / `unit_length` 越界）
    /// ⇒ **降级**，不是错误。
    Malformed,
}

impl DwarfStatus {
    /// 是否**识别出**了 `.debug_line` 节区（不论能不能用）。
    pub fn is_recognised(self) -> bool {
        !matches!(self, DwarfStatus::Absent)
    }

    /// 读到的 DWARF 版本（读不出来 ⇒ `None`）。
    pub fn version(self) -> Option<u16> {
        match self {
            DwarfStatus::Supported(v) | DwarfStatus::UnsupportedVersion(v) => Some(v),
            DwarfStatus::Absent | DwarfStatus::Malformed => None,
        }
    }
}

/// DWARF 行表子组件（**Wave 1 = 降级态**）。
///
/// 依赖方向：M-06 带出**调试节区原始字节**（裁决 B）⇒ M-11 解析。
/// 本模块**不依赖** M-13 / M-19 / 契约层以外的东西。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DwarfLines {
    /// 如实记录的状态。
    status: DwarfStatus,
}

impl DwarfLines {
    /// 从 M-06 带出的调试节区构建。
    ///
    /// ⚠️ **Wave 1 恒返回 `Ok`** —— 畸形输入一律**降级**
    /// （[`DwarfStatus::Malformed`]），**不是错误**（「解析不出来」≠
    /// 「加载失败」）。`Result` 是为了 Wave 2 解析失败时留位置。
    ///
    /// ⚠️ 多个 `.debug_line` ⇒ **第一个**说了算（白名单提取时本就只应
    /// 有一个）。
    pub fn from_sections(sections: &[(String, Vec<u8>)]) -> Result<Self, PlatformError> {
        let mut status = DwarfStatus::Absent;
        for (name, bytes) in sections {
            if name == DEBUG_LINE_SECTION {
                status = parse_debug_line_header(bytes);
                break;
            }
        }
        Ok(Self { status })
    }

    /// 当前状态（宿主 / 测试据此区分「没有 DWARF」与「有 DWARF 但本批
    /// 不解析」）。
    pub fn status(&self) -> DwarfStatus {
        self.status
    }

    /// 读到的 DWARF 版本（读不出来 ⇒ `None`）。
    ///
    /// ⚠️ v5 走这里返回 `Some(5)` —— **明确降级，但版本如实报出**，
    /// 免得宿主以为「这个 ELF 根本没 DWARF」。
    pub fn version(&self) -> Option<u16> {
        self.status.version()
    }

    /// 是否识别出了 `.debug_line` 节区。
    pub fn is_recognised(&self) -> bool {
        self.status.is_recognised()
    }

    /// 地址 → 源码行。
    ///
    /// ⚠️ **Wave 1 恒返回 [`LineLookup::NoDwarf`]** —— 行表尚未解析，
    /// 任何地址都拿不到行号。
    ///
    /// ⚠️ 为什么**不是** [`LineLookup::Missed`]：那等于宣称「查过行表、
    /// 确实没命中」。我们**没查**（行表根本没解析）⇒ 诚实地说
    /// 「没有 DWARF 意义上的行号」。
    pub fn resolve(&self, addr: u32) -> LineLookup {
        match self.lookup_line(addr) {
            Some(entry) => LineLookup::Found(entry),
            // ⚠️ Wave 1：`lookup_line` 恒为 `None`（行表未解析）⇒
            // 一律降级为 `NoDwarf`，**不**降级为 `Missed`。
            None => LineLookup::NoDwarf,
        }
    }

    /// 行表查询（**Wave 2 的落点**）。
    ///
    /// ⚠️ Wave 1 **恒返回 `None`** —— 行表程序尚未解析。Wave 2 在这里
    /// 填上「按地址查行表」的逻辑 ⇒ [`DwarfLines::resolve`] 自动
    /// 从「恒降级」变成「能给行号」。
    fn lookup_line(&self, _addr: u32) -> Option<LineEntry> {
        None
    }
}

/// 解析 `.debug_line` 节区头 ⇒ 状态。
///
/// ⚠️ **只读版本号**（v2~v4 与 v5 的头部前 6 字节布局相同：
/// `unit_length`(u32) + `version`(u16)）⇒ **不碰**行表程序。
///
/// 畸形输入（截断 / `unit_length` 越界）⇒ [`DwarfStatus::Malformed`]，
/// **不 panic**。
fn parse_debug_line_header(bytes: &[u8]) -> DwarfStatus {
    // ---- `unit_length` --------------------------------------------------
    if bytes.len() < 4 {
        // 连长度都读不出来 ⇒ 头部不可解析。
        return DwarfStatus::Malformed;
    }
    let prefix = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);

    // DWARF64：`initial_length == 0xffff_ffff` ⇒ 长度字段占 8 字节，
    // `version` 在偏移 12；DWARF32：`version` 在偏移 4。
    let (version_off, body_off, unit_len) = if prefix == 0xffff_ffff {
        if bytes.len() < 12 {
            return DwarfStatus::Malformed;
        }
        let mut b = [0u8; 8];
        b.copy_from_slice(&bytes[4..12]);
        (12usize, 12usize, u64::from_le_bytes(b))
    } else {
        (4usize, 4usize, u64::from(prefix))
    };

    // ---- 长度自检 ------------------------------------------------------
    // 单元至少要装下 `version`(2 字节)；且**不许**越过节区末尾
    // （越过 ⇒ 节区被截断 ⇒ 畸形）。
    if unit_len < 2 {
        return DwarfStatus::Malformed;
    }
    let remaining = bytes.len() - body_off; // `body_off <= bytes.len()` 已保证
    if unit_len > remaining as u64 {
        return DwarfStatus::Malformed;
    }

    // ---- `version` ------------------------------------------------------
    // 上面两条检查保证 `version_off + 2 <= bytes.len()`。
    let version = u16::from_le_bytes([bytes[version_off], bytes[version_off + 1]]);
    if SUPPORTED_VERSIONS.contains(&version) {
        DwarfStatus::Supported(version)
    } else {
        // v5（及更高）⇒ 明确降级（裁决 F）。
        DwarfStatus::UnsupportedVersion(version)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个 `.debug_line` 节区夹具（**自包含**：不依赖外部工具链 / 外部
    /// ELF 文件）。
    ///
    /// 结构：`unit_length`(u32) + `version`(u16) + `header_length`(u32) +
    /// 前导字节。`unit_length` 覆盖其后全部字节。
    fn debug_line_unit(version: u16, prologue: &[u8]) -> Vec<u8> {
        let body_len = 2 + 4 + prologue.len(); // version + header_length + 前导
        let mut out = Vec::new();
        out.extend_from_slice(&(body_len as u32).to_le_bytes());
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&(prologue.len() as u32).to_le_bytes());
        out.extend_from_slice(prologue);
        out
    }

    /// DWARF64 版的夹具（`initial_length == 0xffff_ffff`）。
    fn debug_line_unit64(version: u16, prologue: &[u8]) -> Vec<u8> {
        let body_len = 2 + 4 + prologue.len();
        let mut out = Vec::new();
        out.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
        out.extend_from_slice(&(body_len as u64).to_le_bytes());
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&(prologue.len() as u32).to_le_bytes());
        out.extend_from_slice(prologue);
        out
    }

    /// 一段像样的 v2~v4 行表前导（最小指令长度 / 缺省 is_stmt / 行基 / 行域 /
    /// opcode 基）。
    fn prologue_v4() -> Vec<u8> {
        vec![
            0x02, // minimum_instruction_length
            0x01, // maximum_operations_per_instruction
            0x01, // default_is_stmt
            0xfb, // line_base (-5)
            0x0e, // line_range (14)
            0x0d, // opcode_base (13)
        ]
    }

    /// 一段像样的 v5 行表前导（v5 多了 `address_size` /
    /// `segment_selector_size`）。
    fn prologue_v5() -> Vec<u8> {
        let mut p = vec![0x04, 0x00]; // address_size / segment_selector_size
        p.extend_from_slice(&prologue_v4());
        p
    }

    fn sections(name: &str, bytes: Vec<u8>) -> Vec<(String, Vec<u8>)> {
        vec![(name.to_string(), bytes)]
    }

    // -----------------------------------------------------------------
    // 1. 三态（任务要求 #9）
    // -----------------------------------------------------------------

    #[test]
    fn an_image_without_debug_line_reports_no_dwarf() {
        let lines = DwarfLines::from_sections(&[]).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Absent);
        assert_eq!(lines.version(), None);
        assert!(!lines.is_recognised());
        assert_eq!(lines.resolve(0x1000), LineLookup::NoDwarf);
    }

    #[test]
    fn other_debug_sections_do_not_count_as_a_line_table() {
        // 有 `.debug_info` / `.debug_abbrev`，但**没有** `.debug_line`
        // ⇒ 依然「无 DWARF 行表」。
        let secs = vec![
            (".debug_info".to_string(), vec![0x01, 0x02, 0x03]),
            (".debug_abbrev".to_string(), vec![0x04, 0x05]),
        ];
        let lines = DwarfLines::from_sections(&secs).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Absent);
        assert_eq!(lines.resolve(0x1000), LineLookup::NoDwarf);
    }

    #[test]
    fn dwarf_five_is_degraded_but_its_version_is_reported() {
        let bytes = debug_line_unit(5, &prologue_v5());
        let lines =
            DwarfLines::from_sections(&sections(DEBUG_LINE_SECTION, bytes)).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::UnsupportedVersion(5));
        assert_eq!(lines.version(), Some(5), "版本必须**如实报出**是 5");
        assert!(lines.is_recognised());
        assert_eq!(
            lines.resolve(0x1000),
            LineLookup::NoDwarf,
            "v5 ⇒ 降级为「无 DWARF」，**绝不**给一个猜出来的行号"
        );
    }

    #[test]
    fn dwarf_four_is_recognised_but_not_parsed_in_wave_one() {
        let bytes = debug_line_unit(4, &prologue_v4());
        let lines =
            DwarfLines::from_sections(&sections(DEBUG_LINE_SECTION, bytes)).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Supported(4));
        assert_eq!(lines.version(), Some(4));
        assert!(lines.is_recognised());
        assert_eq!(
            lines.resolve(0x1000),
            LineLookup::NoDwarf,
            "Wave 1 不解析行表 ⇒ 仍然降级（**不是** `Missed`）"
        );
    }

    #[test]
    fn versions_two_and_three_are_supported() {
        for v in [2u16, 3u16] {
            let bytes = debug_line_unit(v, &prologue_v4());
            let lines =
                DwarfLines::from_sections(&sections(DEBUG_LINE_SECTION, bytes)).expect("不得报错");
            assert_eq!(lines.status(), DwarfStatus::Supported(v));
            assert_eq!(lines.version(), Some(v));
            assert_eq!(lines.resolve(0x2000), LineLookup::NoDwarf);
        }
    }

    // -----------------------------------------------------------------
    // 2. 畸形输入：不 panic、降级而非报错（任务要求 #10）
    // -----------------------------------------------------------------

    #[test]
    fn a_truncated_debug_line_degrades_without_panicking() {
        let full = debug_line_unit(4, &prologue_v4());
        let truncated = &full[..full.len() / 2]; // 头部还在，单元被截断
        let lines = DwarfLines::from_sections(&sections(DEBUG_LINE_SECTION, truncated.to_vec()))
            .expect("畸形输入**不是**加载失败");
        assert_eq!(lines.status(), DwarfStatus::Malformed);
        assert_eq!(lines.resolve(0x1000), LineLookup::NoDwarf);
    }

    #[test]
    fn a_too_short_debug_line_degrades_without_panicking() {
        for len in 0..4usize {
            let bytes = vec![0xabu8; len];
            let lines = DwarfLines::from_sections(&sections(DEBUG_LINE_SECTION, bytes))
                .expect("畸形输入**不是**加载失败");
            assert_eq!(lines.status(), DwarfStatus::Malformed, "长度 {len}");
            assert_eq!(lines.resolve(0x1000), LineLookup::NoDwarf);
        }
    }

    #[test]
    fn a_zero_unit_length_is_malformed() {
        let bytes = vec![0x00, 0x00, 0x00, 0x00, 0x04, 0x00];
        let lines = DwarfLines::from_sections(&sections(DEBUG_LINE_SECTION, bytes)).expect("降级");
        assert_eq!(lines.status(), DwarfStatus::Malformed);
    }

    #[test]
    fn a_dwarf64_header_is_understood() {
        let bytes = debug_line_unit64(4, &prologue_v4());
        let lines =
            DwarfLines::from_sections(&sections(DEBUG_LINE_SECTION, bytes)).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Supported(4));
        assert_eq!(lines.version(), Some(4));
    }

    #[test]
    fn a_truncated_dwarf64_header_is_malformed() {
        let full = debug_line_unit64(4, &prologue_v4());
        let truncated = &full[..8]; // 连 version 都够不着
        let lines = DwarfLines::from_sections(&sections(DEBUG_LINE_SECTION, truncated.to_vec()))
            .expect("降级");
        assert_eq!(lines.status(), DwarfStatus::Malformed);
    }

    // -----------------------------------------------------------------
    // 3. Wave 1 的诚实性
    // -----------------------------------------------------------------

    #[test]
    fn wave_one_never_claims_a_missed_line() {
        // 逐个状态走一遍：resolve 只能是 `NoDwarf` ——
        // **绝不** `Missed`（那等于宣称「查过、没命中」）。
        let cases = vec![
            DwarfStatus::Absent,
            DwarfStatus::Supported(4),
            DwarfStatus::UnsupportedVersion(5),
            DwarfStatus::Malformed,
        ];
        for status in cases {
            let lines = DwarfLines { status };
            for addr in [0u32, 0x1000, 0xffff_ffff] {
                assert_eq!(
                    lines.resolve(addr),
                    LineLookup::NoDwarf,
                    "状态 {status:?} 不得报出 `Missed`"
                );
            }
        }
    }

    #[test]
    fn the_supported_version_set_is_exactly_v2_v3_v4() {
        assert_eq!(SUPPORTED_VERSIONS, &[2, 3, 4]);
        for v in [2u16, 3, 4] {
            assert!(SUPPORTED_VERSIONS.contains(&v));
        }
        for v in [0u16, 1, 5, 6] {
            assert!(!SUPPORTED_VERSIONS.contains(&v), "v{v} 不该在支持集里");
        }
    }
}
