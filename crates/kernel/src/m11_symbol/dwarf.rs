//! M-11 DWARF 行表子组件 —— **Wave 2：真解析**（STAGE-7 裁决 A/B/C/J）。
//!
//! 冻结规格：`docs/detailed-design/STAGE-7_Wave2_DWARF_Lines.md`
//! （§1 裁决 A「版本支持集扩到 v5」· 裁决 B「不解析 `.debug_info`」·
//! 裁决 C「v5 的 form 边界」· 裁决 J「诚实性红线」· §2.1 验收 A-1..A-13）。
//! 前批规格：`docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`
//! （§1 Wave 1 / Wave 2 的切分 —— Wave 1 的「只识别不解析」被本文件**取代**）。
//! 模块设计：`docs/detailed-design/M-11_SymbolDwarfTrace.html`
//! （§3.2 `resolveLine` 的两态语义 · 11-4）。
//!
//! # 本模块**现在**做什么（Wave 2）
//!
//! **解析 `.debug_line` 里全部的行号程序单元**，把产生的行记录按地址归并
//! 成一张表 ⇒ [`DwarfLines::resolve`] 能给出**真的**源码行号。
//!
//! 裁决 B 说清了**为什么只解析行表**：解析 `.debug_info` + `.debug_abbrev`
//! 拿到 `DW_AT_stmt_list` 是「DIE / 属性 / 缩写表」三套格式的全解析 ——
//! 评审 P-07 判定的「M-11 最大的风险项」。而行号程序**自带**地址与文件
//! 索引，足以回答「地址 → 行」。
//!
//! # ⚠️ 裁决 B 的**已知代价**（必须如实体现，不得假装）
//!
//! v2~v4 的**目录索引 0** 指的是编译单元的 `DW_AT_comp_dir`，它在
//! `.debug_info` 里 ⇒ 我们拿不到 ⇒ **目录索引 0 的文件只有文件名、没有
//! 目录前缀**。
//!
//! - v2~v4：`dirs[1] = "src"` + `file[1] = "main.c"` ⇒ `"src/main.c"`；
//! - v5：`dirs[0]` **就是**编译目录（DWARF 5 的目录表首项）⇒ 能拼出
//!   **完整路径**（`"/tmp/…/hook.c"`）。
//!
//! # 降级三态（裁决 J）
//!
//! | 状态 | `resolve` 的结果 | 含义 |
//! | --- | --- | --- |
//! | [`DwarfStatus::Absent`] | `NoDwarf` | 没有 `.debug_line` |
//! | [`DwarfStatus::UnsupportedVersion`] | `NoDwarf` | 版本不在支持集（v6+） |
//! | [`DwarfStatus::Malformed`] | `NoDwarf` | 截断 / 越界 / 未知 form / 未知操作码 |
//! | [`DwarfStatus::Supported`] | `Found` / `Missed` | **查过**行表了 |
//!
//! ⛔ **「查过行表、没命中」是 `Missed`，「没查」是 `NoDwarf`** —— 两态
//! **不得混**（Wave 1 的最大诚实性资产，Wave 2 **不得**把它弄丢）。
//!
//! ⛔ **绝不**产出「猜出来的行号」：遇到不支持的 form（裁决 C）⇒ **整个
//! 行号程序单元降级**，该单元**不贡献任何行记录**（不部分解析）。
//!
//! ⛔ **两条边界**（v0.22 检视轮 V-A 的 VA-01 / VA-02）：
//!
//! 1. **没有 `end_sequence` 收尾的段** ⇒ 它的**结束地址是未知的** ⇒
//!    这一段只覆盖「最后一条行记录的地址 + 1」⇒ 更远的地址是 `Missed`
//!    （⛔ **不是**「覆盖到地址空间的末尾」—— 那会把**任意**远处的地址
//!    报成一条**自信但错误**的源码行）；
//! 2. **取不到文件名的行**（文件索引越界）⇒ 跳过 ⇒ `Missed`
//!    （⛔ 契约层**只有**两种「没有答案」：`Missed`（`line = u32::MAX`）
//!    与 `NoDwarf`（`line = 0`）⇒ **绝不**产出 `file == ""` 但 `line`
//!    是**真行号**的**第三态**）。
//!
//! # 畸形输入 ⇒ 降级，不是错误
//!
//! `.debug_line` 被截断、版本读不出来 ⇒ [`DwarfStatus::Malformed`]，
//! [`DwarfLines::from_sections`] **仍然返回 `Ok`**。理由：「解析不出来」
//! ≠「加载失败」—— 一个没有行表的 ELF 依然能反汇编、能导出。

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::LineEntry;

use super::LineLookup;

/// M-06 带出的 DWARF 行表节区名（白名单，裁决 B）。
pub const DEBUG_LINE_SECTION: &str = ".debug_line";

/// v5 的路径字符串节区（`DW_FORM_line_strp` 指向它，裁决 C）。
pub const DEBUG_LINE_STR_SECTION: &str = ".debug_line_str";

/// 通用字符串节区（`DW_FORM_strp` 指向它，裁决 C）。
pub const DEBUG_STR_SECTION: &str = ".debug_str";

/// DWARF 版本支持集（裁决 A：v2 / v3 / v4 / **v5**）。
///
/// ⚠️ **v5 在支持集里** —— STAGE-5 裁决 F 的「只支持 v2/v3/v4，v5 明确降级」
/// 被 STAGE-7 裁决 A **显式修订**：本机主力工具链（`riscv-none-elf-gcc`
/// 15.2.0）默认产出 DWARF 5，坚持降级就会交付一个「在主力工具链上永远
/// 答不出行号」的半成品。
pub const SUPPORTED_VERSIONS: &[u16] = &[2, 3, 4, 5];

/// DWARF 行表子组件的状态（**如实记录**「这个 ELF 的 DWARF 怎么样」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DwarfStatus {
    /// 没有 `.debug_line` 节区 ⇒ 压根没有 DWARF。
    Absent,
    /// 识别出 `.debug_line`，版本在支持集内（v2 / v3 / v4 / v5），
    /// **且行表已解析成功** ⇒ [`DwarfLines::resolve`] 能给出真行号。
    Supported(u16),
    /// 识别出 `.debug_line`，但版本**不在**支持集（v6 及以上）。
    UnsupportedVersion(u16),
    /// `.debug_line` 在，但**解析不出来**（截断 / `unit_length` 越界 /
    /// 未知 form / 未知操作码）⇒ **降级**，不是错误。
    Malformed,
}

impl Default for DwarfStatus {
    /// 默认 = **没有 DWARF**（空索引的诚实默认值）。
    fn default() -> Self {
        DwarfStatus::Absent
    }
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

/// 一个行号程序单元的**头部自检**信息（诊断用）。
///
/// ⚠️ `program_start == 12 + header_length`（DWARF32 的 v5）是一道**很便宜
/// 的自校验**：它一次性抓住 v5 头部的所有错位（`line_base` 读成 SLEB128
/// 之类的经典错误会让它对不上）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitHeaderInfo {
    /// 这个单元的 DWARF 版本。
    pub version: u16,
    /// 头部里声明的 `header_length`。
    pub header_length: u32,
    /// 行号程序的起始偏移（相对 `.debug_line` 节区开头）。
    pub program_start: usize,
    /// 行号程序的结束偏移（= 单元末尾）。
    pub program_end: usize,
}

/// 一段**行号序列**（两次 `end_sequence` 之间）。
///
/// ⚠️ **序列边界不可跨越**（验收 A-7）：一段的行记录**只**覆盖
/// `[首行地址, end_addr)` —— `end_sequence` 的那个地址**不属于**这一段
/// （它是下一段的起点，或者根本没人管）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sequence {
    /// 这一段的行记录（按地址升序；同地址时**后出现的**在后 ⇒ 它生效）。
    rows: Vec<Row>,
    /// 这一段的**结束地址**（这一段**不含**它）。
    ///
    /// - 正常收尾（`DW_LNE_end_sequence`）⇒ = `end_sequence` 当时的地址；
    /// - **没收尾** ⇒ = **最后一条行记录的地址 + 1**（见 [`run_program`]）。
    ///
    /// ⚠️ 用 `u64` 而不是 `u32`：没收尾的那一段要表达
    /// 「最后一行地址 + 1」⇒ `u32` 在 `0xffff_ffff` 处会**溢出**
    /// （`saturating_add` 会把那一行变成永远查不到的死行）。
    end_addr: u64,
}

/// 行号程序发出的一行。
///
/// ⚠️ 契约的 [`LineEntry`] 只有 `{ addr, file, line }` —— 但状态机还有
/// `is_stmt` / `column` 两个寄存器。**它们必须被真的算出来**（验收 A-5
/// 明确点了 `negate_stmt` / `set_column`），否则「状态机处理了它们」只是
/// 一句没人能验证的话 ⇒ 内部多留两个字段，测试才**有判别力**。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    addr: u32,
    file: String,
    line: u32,
    /// `is_stmt` 寄存器（`negate_stmt` 会翻它）。
    is_stmt: bool,
    /// `column` 寄存器。
    column: u32,
}

impl Row {
    /// 这一行 → 契约的 [`LineEntry`]（**只**带契约有的三个字段）。
    fn to_entry(&self) -> LineEntry {
        LineEntry {
            addr: self.addr,
            file: self.file.clone(),
            line: self.line,
        }
    }
}

/// DWARF 行表子组件（**Wave 2 = 真解析**）。
///
/// 依赖方向：M-06 带出**调试节区原始字节**（裁决 B）⇒ M-11 解析。
/// 本模块**不依赖** M-13 / M-19 / 契约层以外的东西。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DwarfLines {
    /// 如实记录的状态。
    status: DwarfStatus,
    /// 全部行号程序单元的行记录（按序列归并）。
    sequences: Vec<Sequence>,
    /// 每个**成功解析**的单元的头部自检信息。
    unit_headers: Vec<UnitHeaderInfo>,
    /// 成功解析的单元数。
    units_parsed: usize,
    /// 降级（不贡献行记录）的单元数。
    units_degraded: usize,
}

impl DwarfLines {
    /// 从 M-06 带出的调试节区构建。
    ///
    /// ⚠️ **恒返回 `Ok`** —— 畸形输入一律**降级**
    /// （[`DwarfStatus::Malformed`]），**不是错误**（「解析不出来」≠
    /// 「加载失败」）。
    ///
    /// ⚠️ 多个 `.debug_line` ⇒ **最后一个**说了算（白名单提取时本就只应
    /// 有一个）。
    pub fn from_sections(sections: &[(String, Vec<u8>)]) -> Result<Self, PlatformError> {
        let mut line: Option<&[u8]> = None;
        let mut line_str: &[u8] = &[];
        let mut debug_str: &[u8] = &[];
        for (name, bytes) in sections {
            match name.as_str() {
                DEBUG_LINE_SECTION => line = Some(bytes.as_slice()),
                DEBUG_LINE_STR_SECTION => line_str = bytes.as_slice(),
                DEBUG_STR_SECTION => debug_str = bytes.as_slice(),
                _ => {}
            }
        }
        let Some(bytes) = line else {
            return Ok(Self {
                status: DwarfStatus::Absent,
                sequences: Vec::new(),
                unit_headers: Vec::new(),
                units_parsed: 0,
                units_degraded: 0,
            });
        };
        Ok(Self::parse(bytes, line_str, debug_str))
    }

    /// 解析 `.debug_line` 节区（**全部**行号程序单元）。
    fn parse(bytes: &[u8], line_str: &[u8], debug_str: &[u8]) -> Self {
        let mut sequences: Vec<Sequence> = Vec::new();
        let mut unit_headers: Vec<UnitHeaderInfo> = Vec::new();
        let mut units_parsed = 0usize;
        let mut units_degraded = 0usize;
        let mut first_version: Option<u16> = None;
        let mut unsupported_version: Option<u16> = None;

        let mut off = 0usize;
        while off < bytes.len() {
            match parse_unit(bytes, off, line_str, debug_str) {
                Ok(unit) => {
                    if unit.supported && !unit.degraded {
                        units_parsed += 1;
                        if first_version.is_none() {
                            first_version = Some(unit.version);
                        }
                        unit_headers.push(unit.header);
                        sequences.extend(unit.sequences);
                    } else {
                        // 降级（未知 form / 未知扩展操作码 / 版本不支持）⇒
                        // **只降级这个单元**，其余单元照常解析（裁决 B：
                        // 行号表单元之间互不依赖；裁决 C：降级是**单元级**的）。
                        units_degraded += 1;
                        if !unit.supported && unsupported_version.is_none() {
                            unsupported_version = Some(unit.version);
                        }
                    }
                    if unit.next_off <= off {
                        // 防死循环（`unit_length` 为 0 的畸形输入）。
                        break;
                    }
                    off = unit.next_off;
                }
                Err(_) => {
                    // 畸形 ⇒ **停在这里**：后面的单元边界已经不可信了。
                    units_degraded += 1;
                    break;
                }
            }
        }

        let status = match (units_parsed, first_version) {
            (0, _) => match unsupported_version {
                Some(v) => DwarfStatus::UnsupportedVersion(v),
                None => DwarfStatus::Malformed,
            },
            (_, Some(v)) => DwarfStatus::Supported(v),
            (_, None) => DwarfStatus::Malformed,
        };

        // 归并：按地址排序（**稳定**排序 ⇒ 同地址时后出现的行记录在后 ⇒
        // 它生效 —— DWARF 的语义是「后一行覆盖前一行」）。
        for seq in &mut sequences {
            seq.rows.sort_by_key(|row| row.addr);
        }

        Self {
            status,
            sequences,
            unit_headers,
            units_parsed,
            units_degraded,
        }
    }

    /// 当前状态（宿主 / 测试据此区分「没有 DWARF」「有 DWARF 但解析不出来」
    /// 与「有 DWARF 且**真的**查过」）。
    pub fn status(&self) -> DwarfStatus {
        self.status
    }

    /// 读到的 DWARF 版本（读不出来 ⇒ `None`）。
    pub fn version(&self) -> Option<u16> {
        self.status.version()
    }

    /// 是否识别出了 `.debug_line` 节区。
    pub fn is_recognised(&self) -> bool {
        self.status.is_recognised()
    }

    /// 成功解析的行号程序单元数。
    pub fn units_parsed(&self) -> usize {
        self.units_parsed
    }

    /// 降级（不贡献行记录）的行号程序单元数。
    ///
    /// ⚠️ **可见的降级**：一个单元降级了（未知 form / 畸形）⇒ 它的行记录
    /// **一条都没有** —— 没人知道就等于「行号表少了一截」。
    pub fn units_degraded(&self) -> usize {
        self.units_degraded
    }

    /// 每个成功解析的单元的头部自检信息。
    pub fn unit_headers(&self) -> &[UnitHeaderInfo] {
        &self.unit_headers
    }

    /// 行表里一共有多少条行记录。
    pub fn line_count(&self) -> usize {
        self.sequences.iter().map(|s| s.rows.len()).sum()
    }

    /// **测试专用**：全部行记录（含 `is_stmt` / `column` 两个状态机寄存器）。
    #[cfg(test)]
    fn raw_rows(&self) -> Vec<Row> {
        self.sequences
            .iter()
            .flat_map(|s| s.rows.iter().cloned())
            .collect()
    }

    /// 地址 → 源码行。
    ///
    /// * **命中** ⇒ [`LineLookup::Found`]（带真的 `file` + `line`）；
    /// * **查过行表、没命中** ⇒ [`LineLookup::Missed`]；
    /// * **压根没查**（无 `.debug_line` / 版本不支持 / 解析失败）⇒
    ///   [`LineLookup::NoDwarf`]。
    ///
    /// ⚠️ **两态不得混**（裁决 J）：「查过没命中」与「没查」对调用方是
    /// **两件事**（契约把前者映射成 `line = u32::MAX`，后者映射成
    /// `line = 0`）。
    pub fn resolve(&self, addr: u32) -> LineLookup {
        if !matches!(self.status, DwarfStatus::Supported(_)) {
            return LineLookup::NoDwarf;
        }
        match self.lookup_line(addr) {
            Some(entry) => LineLookup::Found(entry),
            None => LineLookup::Missed,
        }
    }

    /// 行表查询：取「**最后一条 address ≤ 目标地址**」的行记录，
    /// 且**不跨过 `end_sequence` 的段边界**（验收 A-7）。
    fn lookup_line(&self, addr: u32) -> Option<LineEntry> {
        let mut best: Option<&Row> = None;
        for seq in &self.sequences {
            // 这一段**不覆盖**这个地址（`end_sequence` 的地址属于下一段）。
            if u64::from(addr) >= seq.end_addr {
                continue;
            }
            // 二分：最后一条 `addr <= 目标` 的行记录。
            let idx = seq.rows.partition_point(|row| row.addr <= addr);
            if idx == 0 {
                continue;
            }
            let row = &seq.rows[idx - 1];
            // ⚠️ `line == 0` 的行记录表示「这一行**没有**源码行号」
            // （DWARF 的 `line = 0` 语义）⇒ 不报行号（报 0 会与契约的
            // 「没有 DWARF」态撞车）。
            if row.line == 0 {
                continue;
            }
            // ⚠️ **文件名为空**的行记录（VA-02）：文件索引越界 / 名字取不到
            // ⇒ 这一行**不知道自己属于哪个文件** ⇒ 报出去就是契约层的
            // **第三种「没有答案」**（`file == ""` 但 `line` 是**真行号**）
            // ⇒ 与 `Missed`（`line = u32::MAX`）、`NoDwarf`（`line = 0`）
            // **都不同** ⇒ 前端 `location?.file` 判空会把它**吞掉**
            // （用户看到「没变化」，分不清是没点生效还是真的没有）。
            //
            // 处理方式与 `line == 0` **完全一致**：这一段**答不了**这个
            // 地址 ⇒ 跳过这一段（`continue`），**不**退回到上一行 ——
            // 上一行的行号是**上一行**的（DWARF 里行号是寄存器，逐行可变）
            // ⇒ 拿它顶替就是「猜出来的行号」（裁决 J 第 1 条）。
            if row.file.is_empty() {
                continue;
            }
            let better = match best {
                None => true,
                Some(current) => row.addr >= current.addr,
            };
            if better {
                best = Some(row);
            }
        }
        best.map(Row::to_entry)
    }
}

// ---------------------------------------------------------------------------
// 解析内部
// ---------------------------------------------------------------------------

/// 解析失败的三种原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseError {
    /// 节区被截断 / 越界 / 结构不自洽 ⇒ 降级。
    Malformed,
    /// 遇到**不支持的 form**（裁决 C）⇒ **整个单元**降级。
    UnsupportedForm,
    /// 遇到**未知的扩展操作码** ⇒ 降级（不猜它的语义）。
    UnknownExtendedOpcode,
}

/// 一个行号程序单元的解析结果。
struct UnitResult {
    /// 这个单元的 DWARF 版本。
    version: u16,
    /// 版本在支持集里吗？
    supported: bool,
    /// 这个单元**降级**了吗（未知 form / 未知扩展操作码）？
    ///
    /// ⚠️ 降级的单元**不贡献任何行记录**，但**不牵连**别的单元
    /// （裁决 C：「整个行号程序单元降级」—— 单元级，不是节区级）。
    degraded: bool,
    /// 这个单元贡献的行记录（按序列）。
    sequences: Vec<Sequence>,
    /// 头部自检信息。
    header: UnitHeaderInfo,
    /// 下一个单元的起始偏移。
    next_off: usize,
}

/// 解析一个行号程序单元（`start` = `unit_length` 字段的偏移）。
fn parse_unit(
    bytes: &[u8],
    start: usize,
    line_str: &[u8],
    debug_str: &[u8],
) -> Result<UnitResult, ParseError> {
    if start + 4 > bytes.len() {
        return Err(ParseError::Malformed);
    }
    let prefix = u32::from_le_bytes([
        bytes[start],
        bytes[start + 1],
        bytes[start + 2],
        bytes[start + 3],
    ]);
    // DWARF64：`initial_length == 0xffff_ffff` ⇒ 长度字段占 12 字节
    // （标记 4 + 长度 8），`version` 在偏移 12。
    let (length_field, unit_len) = if prefix == 0xffff_ffff {
        if start + 12 > bytes.len() {
            return Err(ParseError::Malformed);
        }
        let mut b = [0u8; 8];
        b.copy_from_slice(&bytes[start + 4..start + 12]);
        (12usize, u64::from_le_bytes(b))
    } else {
        (4usize, u64::from(prefix))
    };
    let unit_end = (start as u64)
        .checked_add(length_field as u64)
        .and_then(|v| v.checked_add(unit_len))
        .ok_or(ParseError::Malformed)? as usize;
    // ⚠️ `unit_length` 越界 ⇒ 节区被截断 ⇒ 畸形（验收 A-10）。
    if unit_end > bytes.len() {
        return Err(ParseError::Malformed);
    }

    // 单元内部的相对偏移（0 = `unit_length` 字段）。
    let unit = &bytes[start..unit_end];
    let mut cur = Cursor::new(unit, length_field);
    let version = cur.u16()?;

    if !SUPPORTED_VERSIONS.contains(&version) {
        return Ok(UnitResult {
            version,
            supported: false,
            degraded: true,
            sequences: Vec::new(),
            header: UnitHeaderInfo {
                version,
                header_length: 0,
                program_start: unit_end - start,
                program_end: unit_end - start,
            },
            next_off: unit_end,
        });
    }

    let dwarf64 = length_field == 12;
    // ⚠️ 「可降级」的错误（未知 form / 未知扩展操作码）⇒ **只降级这个
    // 单元**：它的边界是**知道的**（`unit_length` 已读到）⇒ 后面的单元
    // 照常解析。真正的「畸形」（截断 / 越界）⇒ 停。
    let parsed = (|| -> Result<(Header, Vec<Sequence>), ParseError> {
        let mut header = parse_header(&mut cur, version, dwarf64, line_str, debug_str)?;
        let mut prog = Cursor::new(unit, header.program_start);
        let sequences = run_program(&mut prog, &mut header, unit_end - start)?;
        Ok((header, sequences))
    })();
    let (header, sequences) = match parsed {
        Ok(v) => v,
        Err(ParseError::UnsupportedForm) | Err(ParseError::UnknownExtendedOpcode) => {
            return Ok(UnitResult {
                version,
                supported: true,
                degraded: true,
                sequences: Vec::new(),
                header: UnitHeaderInfo {
                    version,
                    header_length: 0,
                    program_start: unit_end - start,
                    program_end: unit_end - start,
                },
                next_off: unit_end,
            });
        }
        Err(e) => return Err(e),
    };

    Ok(UnitResult {
        version,
        supported: true,
        degraded: false,
        sequences,
        header: UnitHeaderInfo {
            version,
            header_length: header.header_length,
            program_start: header.program_start,
            program_end: unit_end - start,
        },
        next_off: unit_end,
    })
}

/// 行号程序单元的头部。
struct Header {
    version: u16,
    /// `minimum_instruction_length`。
    min_inst_len: u64,
    /// `maximum_operations_per_instruction`（v4+）。
    max_ops: u64,
    /// `default_is_stmt`。
    default_is_stmt: bool,
    /// `line_base`（**有符号单字节** —— 不是 SLEB128！）。
    line_base: i64,
    /// `line_range`。
    line_range: u64,
    /// `opcode_base`。
    opcode_base: u8,
    /// `standard_opcode_lengths`（长度 = `opcode_base - 1`）。
    std_opcode_lengths: Vec<u8>,
    /// 目录表（v2~v4 的索引 0 = 编译目录 ⇒ **拿不到** ⇒ `None`）。
    dirs: Vec<Option<String>>,
    /// 文件表（v2~v4 的索引 0 未用 ⇒ `None`；v5 从 0 开始）。
    files: Vec<Option<FileEntry>>,
    /// v5 的文件表项格式（`(content_type, form)` 序列）。
    file_format: Vec<(u16, u16)>,
    /// 头部长度（`header_length` 字段的值）。
    header_length: u32,
    /// 行号程序的起始偏移（相对单元开头）。
    program_start: usize,
    /// DWARF64？（影响 form 的偏移宽度）。
    dwarf64: bool,
    /// `file` 寄存器的**初值**（v2~v4 = 1，v5 = 0）。
    initial_file: u64,
}

/// 文件表的一项。
#[derive(Debug, Clone)]
struct FileEntry {
    /// 文件名（DWARF 里记的是**编译期路径**）。
    name: String,
    /// 目录索引。
    dir_index: u64,
}

impl Header {
    /// 文件索引 → 路径。
    ///
    /// ⚠️ **裁决 B 的代价就在这里**：v2~v4 的 `dir_index == 0` 指向
    /// `.debug_info` 里的 `DW_AT_comp_dir` ⇒ 拿不到 ⇒ **只有文件名**。
    fn file_path(&self, index: u64) -> String {
        let Some(entry) = self.files.get(index as usize).and_then(|f| f.as_ref()) else {
            // 越界的文件索引 ⇒ 没有文件名（**不猜**）。
            return String::new();
        };
        if entry.name.is_empty() {
            return String::new();
        }
        if entry.name.starts_with('/') {
            // 本身就是绝对路径 ⇒ 不用拼目录。
            return entry.name.clone();
        }
        match self
            .dirs
            .get(entry.dir_index as usize)
            .and_then(|d| d.as_ref())
        {
            Some(dir) if !dir.is_empty() => format!("{dir}/{}", entry.name),
            // 目录索引 0（v2~v4）⇒ 编译目录拿不到 ⇒ **只有文件名**。
            _ => entry.name.clone(),
        }
    }
}

/// 解析单元头部（v2~v4 与 v5 的布局**不同**）。
fn parse_header(
    cur: &mut Cursor,
    version: u16,
    dwarf64: bool,
    line_str: &[u8],
    debug_str: &[u8],
) -> Result<Header, ParseError> {
    if version >= 5 {
        parse_header_v5(cur, dwarf64, line_str, debug_str)
    } else {
        parse_header_v2_v4(cur, version, dwarf64)
    }
}

/// v2 / v3 / v4 的头部布局。
fn parse_header_v2_v4(cur: &mut Cursor, version: u16, dwarf64: bool) -> Result<Header, ParseError> {
    let header_length = cur.u32()?;
    // `version`(2) + `header_length`(4) ⇒ 程序起点 = 长度字段 + 6 + hl。
    let program_start = cur.pos + header_length as usize;

    let min_inst_len = u64::from(cur.u8()?);
    let max_ops = if version >= 4 {
        u64::from(cur.u8()?)
    } else {
        1
    };
    let default_is_stmt = cur.u8()? != 0;
    // ⚠️ `line_base` 是**有符号单字节**，**不是** SLEB128。
    let line_base = i64::from(cur.u8()? as i8);
    let line_range = u64::from(cur.u8()?);
    if line_range == 0 {
        // 除零 ⇒ 畸形。
        return Err(ParseError::Malformed);
    }
    let opcode_base = cur.u8()?;
    if opcode_base == 0 {
        return Err(ParseError::Malformed);
    }
    let std_opcode_lengths = read_standard_opcode_lengths(cur, opcode_base)?;

    // 目录表：`NUL` 结尾的字符串序列，以**空串**结束。
    // ⚠️ 索引 0 = 编译单元的编译目录（`.debug_info` 里的 `DW_AT_comp_dir`）
    // ⇒ 裁决 B：**拿不到** ⇒ 记成 `None`。
    let mut dirs: Vec<Option<String>> = vec![None];
    loop {
        let name = cur.cstring()?;
        if name.is_empty() {
            break;
        }
        dirs.push(Some(name));
    }

    // 文件表：`{ name, dir_index, mtime, length }` 序列，以**空 name**结束。
    // ⚠️ v2~v4 的文件索引从 **1** 开始 ⇒ 索引 0 未用。
    let mut files: Vec<Option<FileEntry>> = vec![None];
    loop {
        let name = cur.cstring()?;
        if name.is_empty() {
            break;
        }
        let dir_index = cur.uleb()?;
        let _mtime = cur.uleb()?;
        let _length = cur.uleb()?;
        files.push(Some(FileEntry { name, dir_index }));
    }

    finish_header(
        cur,
        Header {
            version,
            min_inst_len,
            max_ops,
            default_is_stmt,
            line_base,
            line_range,
            opcode_base,
            std_opcode_lengths,
            dirs,
            files,
            file_format: Vec::new(),
            header_length,
            program_start,
            dwarf64,
            initial_file: 1,
        },
    )
}

/// v5 的头部布局（目录 / 文件表用 `DW_FORM_*` 描述）。
fn parse_header_v5(
    cur: &mut Cursor,
    dwarf64: bool,
    line_str: &[u8],
    debug_str: &[u8],
) -> Result<Header, ParseError> {
    let address_size = cur.u8()?;
    if address_size == 0 || address_size > 8 {
        // 0 或超过 8 字节的地址宽度 ⇒ 结构不自洽 ⇒ 畸形。
        return Err(ParseError::Malformed);
    }
    let _segment_selector_size = cur.u8()?;
    let header_length = cur.u32()?;
    // `version`(2) + `address_size`(1) + `segment_selector_size`(1) +
    // `header_length`(4) ⇒ 程序起点 = 长度字段 + 8 + hl（DWARF32 ⇒ 12 + hl）。
    let program_start = cur.pos + header_length as usize;

    let min_inst_len = u64::from(cur.u8()?);
    let max_ops = u64::from(cur.u8()?);
    let default_is_stmt = cur.u8()? != 0;
    // ⚠️ `line_base` 是**有符号单字节**，**不是** SLEB128。
    let line_base = i64::from(cur.u8()? as i8);
    let line_range = u64::from(cur.u8()?);
    if line_range == 0 {
        return Err(ParseError::Malformed);
    }
    let opcode_base = cur.u8()?;
    if opcode_base == 0 {
        return Err(ParseError::Malformed);
    }
    let std_opcode_lengths = read_standard_opcode_lengths(cur, opcode_base)?;

    // 目录表。
    let dir_format = read_entry_format(cur)?;
    let dir_count = cur.uleb()?;
    let mut dirs: Vec<Option<String>> = Vec::with_capacity(dir_count as usize);
    for _ in 0..dir_count {
        dirs.push(read_v5_entry(cur, &dir_format, dwarf64, line_str, debug_str)?.path);
    }

    // 文件表（v5 的文件索引从 **0** 开始）。
    let file_format = read_entry_format(cur)?;
    let file_count = cur.uleb()?;
    let mut files: Vec<Option<FileEntry>> = Vec::with_capacity(file_count as usize);
    for _ in 0..file_count {
        let entry = read_v5_entry(cur, &file_format, dwarf64, line_str, debug_str)?;
        let name = entry.path.ok_or(ParseError::Malformed)?;
        files.push(Some(FileEntry {
            name,
            dir_index: entry.dir_index,
        }));
    }

    finish_header(
        cur,
        Header {
            version: 5,
            min_inst_len,
            max_ops,
            default_is_stmt,
            line_base,
            line_range,
            opcode_base,
            std_opcode_lengths,
            dirs,
            files,
            file_format,
            header_length,
            program_start,
            dwarf64,
            initial_file: 0,
        },
    )
}

/// 读 `standard_opcode_lengths`（长度 = `opcode_base - 1`）。
fn read_standard_opcode_lengths(cur: &mut Cursor, opcode_base: u8) -> Result<Vec<u8>, ParseError> {
    let mut lengths = Vec::with_capacity(opcode_base.saturating_sub(1) as usize);
    for _ in 0..opcode_base.saturating_sub(1) {
        lengths.push(cur.u8()?);
    }
    Ok(lengths)
}

/// 读 v5 的 `(content_type, form)` 格式描述。
fn read_entry_format(cur: &mut Cursor) -> Result<Vec<(u16, u16)>, ParseError> {
    let count = cur.u8()?;
    let mut format = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let content_type = cur.uleb()? as u16;
        let form = cur.uleb()? as u16;
        format.push((content_type, form));
    }
    Ok(format)
}

/// v5 表项里我们关心的两个字段。
struct V5Entry {
    /// `DW_LNCT_path`。
    path: Option<String>,
    /// `DW_LNCT_directory_index`。
    dir_index: u64,
}

/// 按格式读一个 v5 表项。
fn read_v5_entry(
    cur: &mut Cursor,
    format: &[(u16, u16)],
    dwarf64: bool,
    line_str: &[u8],
    debug_str: &[u8],
) -> Result<V5Entry, ParseError> {
    let mut path = None;
    let mut dir_index = 0u64;
    for (content_type, form) in format {
        let value = read_form(cur, *form, dwarf64, line_str, debug_str)?;
        match *content_type {
            // `DW_LNCT_path`
            1 => match value {
                FormValue::Str(s) => path = Some(s),
                _ => return Err(ParseError::Malformed),
            },
            // `DW_LNCT_directory_index`
            2 => match value {
                FormValue::Num(n) => dir_index = n,
                _ => return Err(ParseError::Malformed),
            },
            // 其余内容类型（时间戳 / 大小 / MD5）⇒ 读掉即可，**不猜**。
            _ => {}
        }
    }
    Ok(V5Entry { path, dir_index })
}

/// 一个 form 读出来的值。
enum FormValue {
    Str(String),
    Num(u64),
}

/// 读一个 form 的值（裁决 C 的 form 边界）。
fn read_form(
    cur: &mut Cursor,
    form: u16,
    dwarf64: bool,
    line_str: &[u8],
    debug_str: &[u8],
) -> Result<FormValue, ParseError> {
    match form {
        // `DW_FORM_string`（内联 NUL 结尾的字符串）
        0x08 => Ok(FormValue::Str(cur.cstring()?)),
        // `DW_FORM_strp`（偏移 → `.debug_str`）
        0x0e => {
            let off = read_offset(cur, dwarf64)?;
            Ok(FormValue::Str(str_at(debug_str, off)?))
        }
        // `DW_FORM_line_strp`（偏移 → `.debug_line_str`）
        0x1f => {
            let off = read_offset(cur, dwarf64)?;
            Ok(FormValue::Str(str_at(line_str, off)?))
        }
        // `DW_FORM_udata`
        0x0f => Ok(FormValue::Num(cur.uleb()?)),
        // `DW_FORM_data1` / `data2` / `data4` / `data8`
        0x0b => Ok(FormValue::Num(u64::from(cur.u8()?))),
        0x05 => Ok(FormValue::Num(u64::from(cur.u16()?))),
        0x06 => Ok(FormValue::Num(u64::from(cur.u32()?))),
        0x07 => Ok(FormValue::Num(cur.u64()?)),
        // `DW_FORM_block`（只取长度，内容跳过）
        0x09 => {
            let len = cur.uleb()? as usize;
            cur.take(len)?;
            Ok(FormValue::Num(len as u64))
        }
        // ⛔ `DW_FORM_strx*`（0x1a / 0x25 / 0x26 / 0x27 / 0x28）—— 裁决 C：
        // 需要 `.debug_str_offsets` + 编译单元基址 ⇒ **整个单元降级**，
        // **绝不**部分解析。
        0x1a | 0x25 | 0x26 | 0x27 | 0x28 => Err(ParseError::UnsupportedForm),
        _ => Err(ParseError::UnsupportedForm),
    }
}

/// 读一个字符串偏移（DWARF32 = 4 字节，DWARF64 = 8 字节）。
fn read_offset(cur: &mut Cursor, dwarf64: bool) -> Result<usize, ParseError> {
    let raw = if dwarf64 {
        cur.u64()?
    } else {
        u64::from(cur.u32()?)
    };
    usize::try_from(raw).map_err(|_| ParseError::Malformed)
}

/// 取节区里偏移处的 NUL 结尾字符串。
fn str_at(section: &[u8], offset: usize) -> Result<String, ParseError> {
    if offset >= section.len() {
        // 偏移越界 ⇒ 畸形（**不猜**）。
        return Err(ParseError::Malformed);
    }
    let rest = &section[offset..];
    let end = rest
        .iter()
        .position(|b| *b == 0)
        .ok_or(ParseError::Malformed)?;
    Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
}

/// 头部的收尾：校验「表区没有越过 `header_length`」，并把游标挪到程序起点。
fn finish_header(cur: &mut Cursor, mut header: Header) -> Result<Header, ParseError> {
    if cur.pos > header.program_start {
        // 表区比 `header_length` 还长 ⇒ 头部自相矛盾 ⇒ 畸形。
        return Err(ParseError::Malformed);
    }
    // `header_length` 允许有**填充**（游标可能停在程序起点之前）⇒ 跳过。
    cur.pos = header.program_start;
    if cur.pos > cur.bytes.len() {
        return Err(ParseError::Malformed);
    }
    header.max_ops = header.max_ops.max(1);
    Ok(header)
}

/// 行号程序的状态机寄存器。
struct LineState {
    address: u64,
    file: u64,
    line: i64,
    column: u64,
    is_stmt: bool,
    basic_block: bool,
    prologue_end: bool,
    epilogue_begin: bool,
}

impl LineState {
    fn new(header: &Header) -> Self {
        Self {
            address: 0,
            file: header.initial_file,
            line: 1,
            column: 0,
            is_stmt: header.default_is_stmt,
            basic_block: false,
            prologue_end: false,
            epilogue_begin: false,
        }
    }
}

/// 跑一遍行号程序，返回它产生的序列。
fn run_program(
    cur: &mut Cursor,
    header: &mut Header,
    unit_end: usize,
) -> Result<Vec<Sequence>, ParseError> {
    let mut state = LineState::new(header);
    let mut sequences: Vec<Sequence> = Vec::new();
    let mut rows: Vec<Row> = Vec::new();

    while cur.pos < unit_end {
        let opcode = cur.u8()?;
        if opcode == 0 {
            // -- 扩展操作码：ULEB 长度 + ULEB 子操作码 --------------
            let len = cur.uleb()? as usize;
            let end = cur.pos.checked_add(len).ok_or(ParseError::Malformed)?;
            if end > unit_end {
                return Err(ParseError::Malformed);
            }
            let sub = cur.uleb()?;
            match sub {
                // `DW_LNE_end_sequence`
                1 => {
                    sequences.push(Sequence {
                        rows: std::mem::take(&mut rows),
                        end_addr: u64::from(clamp_u32(state.address)),
                    });
                    // 序列结束 ⇒ 状态机复位（地址 / 文件 / 行都回到初值）。
                    state = LineState::new(header);
                }
                // `DW_LNE_set_address`（操作数宽度 = 扩展操作码的长度字段）
                2 => {
                    let width = end - cur.pos;
                    let value = read_uint(cur, width)?;
                    state.address = value;
                }
                // `DW_LNE_define_file`
                3 => define_file(cur, header)?,
                // `DW_LNE_set_discriminator`（DWARF 4/5 新增）——
                // 操作数是 ULEB，**不影响**地址 / 行号 ⇒ 读掉即可。
                4 => {
                    let _discriminator = cur.uleb()?;
                }
                // 未知扩展操作码 ⇒ **降级**（不猜它的语义）。
                _ => return Err(ParseError::UnknownExtendedOpcode),
            }
            // 跳过这个扩展操作码的剩余字节。
            cur.pos = end;
        } else if opcode >= header.opcode_base {
            // -- 特殊操作码（adjusted opcode）----------------------
            let adjusted = u64::from(opcode - header.opcode_base);
            state.address = state
                .address
                .saturating_add((adjusted / header.line_range) * header.min_inst_len);
            state.line = state
                .line
                .saturating_add(header.line_base + (adjusted % header.line_range) as i64);
            emit_row(&mut rows, &state, header);
        } else {
            match opcode {
                // `DW_LNS_copy`
                1 => emit_row(&mut rows, &state, header),
                // `DW_LNS_advance_pc`
                2 => {
                    let delta = cur.uleb()?;
                    state.address = state
                        .address
                        .saturating_add(delta.saturating_mul(header.min_inst_len));
                }
                // `DW_LNS_advance_line`（**SLEB128**）
                3 => {
                    let delta = cur.sleb()?;
                    state.line = state.line.saturating_add(delta);
                }
                // `DW_LNS_set_file`
                4 => {
                    state.file = cur.uleb()?;
                }
                // `DW_LNS_set_column`
                5 => {
                    state.column = cur.uleb()?;
                }
                // `DW_LNS_negate_stmt`
                6 => {
                    state.is_stmt = !state.is_stmt;
                }
                // `DW_LNS_set_basic_block`
                7 => {
                    state.basic_block = true;
                }
                // `DW_LNS_const_add_pc`
                8 => {
                    let adjusted = 255u64 - u64::from(header.opcode_base);
                    state.address = state
                        .address
                        .saturating_add((adjusted / header.line_range) * header.min_inst_len);
                }
                // `DW_LNS_fixed_advance_pc`（**不乘** `minimum_instruction_length`）
                9 => {
                    let delta = u64::from(cur.u16()?);
                    state.address = state.address.saturating_add(delta);
                }
                // `DW_LNS_set_prologue_end`
                10 => {
                    state.prologue_end = true;
                }
                // `DW_LNS_set_epilogue_begin`
                11 => {
                    state.epilogue_begin = true;
                }
                // `DW_LNS_set_isa`
                12 => {
                    let _isa = cur.uleb()?;
                }
                // 用户定义的标准操作码 ⇒ 按 `standard_opcode_lengths`
                // 跳过它的参数（**不猜**它的语义，但它不影响行号状态）。
                _ => {
                    let count = header
                        .std_opcode_lengths
                        .get(opcode as usize - 1)
                        .copied()
                        .unwrap_or(0);
                    for _ in 0..count {
                        let _operand = cur.uleb()?;
                    }
                }
            }
        }
    }

    if !rows.is_empty() {
        // ⚠️ **没有 `end_sequence` 收尾的段**（VA-01）：DWARF 规定每段行号
        // 序列**必须**以 `DW_LNE_end_sequence` 收尾 ⇒ 没收尾 ⇒ 这一段的
        // **结束地址是未知的**。
        //
        // ⛔ 旧实现把它当成「覆盖到地址空间的末尾」（`end_addr = u32::MAX`）
        // ⇒ **任意远处的地址**都会被报成 `Found`（带一个**自信但错误**的
        // 源码行）⇒ 违反裁决 J 第 1 条「绝不产出猜出来的行号」。
        //
        // 修法：把 `end_addr` 收到**本段最后一条行记录的地址 + 1** ——
        // 「我们真的看到的行」照常可用，**不再声称**覆盖到看不见的地方
        // （越界 ⇒ `Missed`）。
        //
        // ⚠️ 为什么**不**把这一段整个丢掉（另一种修法）：那些行记录是
        // **真的**（地址、文件、行号都是解析出来的真值）⇒ 丢掉它们是把
        // 「真值」降级成「没有」；收窄区间只影响「我们没看见的那部分」
        // ⇒ 诚实性的损失更小、保留的信息更多。
        //
        // ⚠️ 取**所有行里的最大地址**而不是「最后一条」：这一段的行记录
        // 在 `parse` 里才排序 ⇒ 这里按**程序顺序**追加（DWARF 规定地址
        // 单调递增，但取 max 不依赖这个前提）。
        let last_addr = rows
            .iter()
            .map(|row| u64::from(row.addr))
            .max()
            .unwrap_or(0);
        sequences.push(Sequence {
            rows,
            end_addr: last_addr + 1,
        });
    }
    Ok(sequences)
}

/// `DW_LNE_define_file` —— 往文件表里追加一项。
fn define_file(cur: &mut Cursor, header: &mut Header) -> Result<(), ParseError> {
    if header.version >= 5 {
        // v5：按文件表项格式读**一项**。
        let entry = read_v5_entry(cur, &header.file_format, header.dwarf64, &[], &[])?;
        let name = entry.path.ok_or(ParseError::Malformed)?;
        header.files.push(Some(FileEntry {
            name,
            dir_index: entry.dir_index,
        }));
    } else {
        // v2~v4：`{ name, dir_index, mtime, length }`。
        let name = cur.cstring()?;
        let dir_index = cur.uleb()?;
        let _mtime = cur.uleb()?;
        let _length = cur.uleb()?;
        header.files.push(Some(FileEntry { name, dir_index }));
    }
    Ok(())
}

/// 发一行行记录。
fn emit_row(rows: &mut Vec<Row>, state: &LineState, header: &Header) {
    let line = if state.line <= 0 {
        0
    } else {
        u32::try_from(state.line).unwrap_or(u32::MAX)
    };
    rows.push(Row {
        addr: clamp_u32(state.address),
        file: header.file_path(state.file),
        line,
        is_stmt: state.is_stmt,
        column: u32::try_from(state.column).unwrap_or(u32::MAX),
    });
}

/// 地址 → u32（超出范围就**夹住**，不猜）。
fn clamp_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// 读一个小端无符号整数（`width` 字节）。
fn read_uint(cur: &mut Cursor, width: usize) -> Result<u64, ParseError> {
    let bytes = cur.take(width)?;
    let mut value = 0u64;
    for (i, byte) in bytes.iter().enumerate() {
        value |= u64::from(*byte) << (8 * i);
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// 字节游标
// ---------------------------------------------------------------------------

/// 一个只读的字节游标（越界 ⇒ [`ParseError::Malformed`]，**不 panic**）。
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], pos: usize) -> Self {
        Self { bytes, pos }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let end = self.pos.checked_add(n).ok_or(ParseError::Malformed)?;
        if end > self.bytes.len() {
            return Err(ParseError::Malformed);
        }
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ParseError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, ParseError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, ParseError> {
        let b = self.take(8)?;
        let mut v = [0u8; 8];
        v.copy_from_slice(b);
        Ok(u64::from_le_bytes(v))
    }

    /// ULEB128。
    fn uleb(&mut self) -> Result<u64, ParseError> {
        let mut result = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = self.u8()?;
            if shift < 64 {
                result |= u64::from(byte & 0x7f) << shift;
            }
            if byte & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= 70 {
                return Err(ParseError::Malformed);
            }
        }
    }

    /// SLEB128。
    fn sleb(&mut self) -> Result<i64, ParseError> {
        let mut result = 0i64;
        let mut shift = 0u32;
        loop {
            let byte = self.u8()?;
            if shift < 64 {
                result |= i64::from(byte & 0x7f) << shift;
            }
            shift += 7;
            if byte & 0x80 == 0 {
                if shift < 64 && (byte & 0x40) != 0 {
                    result |= -1i64 << shift;
                }
                return Ok(result);
            }
            if shift >= 70 {
                return Err(ParseError::Malformed);
            }
        }
    }

    /// NUL 结尾的字符串。
    fn cstring(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != 0 {
            self.pos += 1;
        }
        if self.pos >= self.bytes.len() {
            // 没有 NUL 结尾 ⇒ 截断 ⇒ 畸形。
            return Err(ParseError::Malformed);
        }
        let out = String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned();
        self.pos += 1;
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 测试夹具（**真工具链产物**的字节常量）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod fixtures {
    //! **真工具链产物**的字节常量。
    //!
    //! ⚠️ 仓库**自包含**：这里存的是**字节**，不是 ELF 文件；测试**不依赖**
    //! 外部工具链存在。
    //!
    //! | 夹具 | 来源 | 用途 |
    //! | --- | --- | --- |
    //! | [`OSZ_DL`] / [`OSZ_DLS`] | `riscv-none-elf-gcc` 15.2.0 编的真实 RISC-V OS（osz）3 个 CU，**完全链接** | v5 + 多单元归并（A-4 / A-13） |
    //! | [`V2_DL`] / [`V3_DL`] / [`V4_DL`] | 同一工具链 `-gdwarf-2 / -3 / -4` 编的小程序 | v2 / v3 / v4（A-1 / A-2 / A-3） |

    /// `.debug_line`（osz · 3 个行号程序单元 · DWARF 5）（1596 字节）。
    pub const OSZ_DL: &[u8] = &[
        0xbe, 0x00, 0x00, 0x00, 0x05, 0x00, 0x04, 0x00, 0x40, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01,
        0xfb, 0x0e, 0x0d, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01,
        0x01, 0x01, 0x1f, 0x04, 0x00, 0x00, 0x00, 0x00, 0x39, 0x00, 0x00, 0x00, 0x5d, 0x00, 0x00,
        0x00, 0x80, 0x00, 0x00, 0x00, 0x02, 0x01, 0x1f, 0x02, 0x0f, 0x04, 0x32, 0x00, 0x00, 0x00,
        0x01, 0x32, 0x00, 0x00, 0x00, 0x01, 0xa8, 0x00, 0x00, 0x00, 0x02, 0xb0, 0x00, 0x00, 0x00,
        0x03, 0x05, 0x01, 0x00, 0x05, 0x02, 0x00, 0x00, 0x00, 0x80, 0x19, 0x05, 0x13, 0x03, 0x01,
        0x09, 0x08, 0x00, 0x01, 0x05, 0x05, 0x03, 0x00, 0x09, 0x04, 0x00, 0x01, 0x05, 0x24, 0x03,
        0x01, 0x09, 0x02, 0x00, 0x01, 0x05, 0x09, 0x03, 0x00, 0x09, 0x12, 0x00, 0x01, 0x05, 0x48,
        0x03, 0x02, 0x09, 0x02, 0x00, 0x01, 0x05, 0x23, 0x03, 0x00, 0x09, 0x12, 0x00, 0x01, 0x05,
        0x21, 0x03, 0x00, 0x09, 0x02, 0x00, 0x01, 0x05, 0x11, 0x03, 0x01, 0x09, 0x08, 0x00, 0x01,
        0x03, 0x02, 0x09, 0x02, 0x00, 0x01, 0x05, 0x55, 0x00, 0x02, 0x04, 0x02, 0x03, 0x7a, 0x09,
        0x02, 0x00, 0x01, 0x05, 0x24, 0x00, 0x02, 0x04, 0x01, 0x03, 0x00, 0x09, 0x0a, 0x00, 0x01,
        0x05, 0x01, 0x03, 0x09, 0x09, 0x06, 0x00, 0x01, 0x09, 0x0c, 0x00, 0x00, 0x01, 0x01, 0xb2,
        0x00, 0x00, 0x00, 0x05, 0x00, 0x04, 0x00, 0x40, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0xfb,
        0x0e, 0x0d, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01,
        0x01, 0x1f, 0x04, 0x00, 0x00, 0x00, 0x00, 0xe2, 0x00, 0x00, 0x00, 0x5d, 0x00, 0x00, 0x00,
        0x06, 0x01, 0x00, 0x00, 0x02, 0x01, 0x1f, 0x02, 0x0f, 0x04, 0xdb, 0x00, 0x00, 0x00, 0x01,
        0xdb, 0x00, 0x00, 0x00, 0x01, 0xa8, 0x00, 0x00, 0x00, 0x02, 0x2e, 0x01, 0x00, 0x00, 0x03,
        0x05, 0x01, 0x00, 0x05, 0x02, 0x5e, 0x00, 0x00, 0x80, 0x03, 0x0b, 0x01, 0x05, 0x11, 0x03,
        0x01, 0x09, 0x08, 0x00, 0x01, 0x05, 0x05, 0x03, 0x00, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x30,
        0x03, 0x01, 0x09, 0x02, 0x00, 0x01, 0x05, 0x18, 0x03, 0x00, 0x09, 0x06, 0x00, 0x01, 0x05,
        0x0c, 0x03, 0x01, 0x09, 0x04, 0x00, 0x01, 0x05, 0x0d, 0x03, 0x01, 0x09, 0x06, 0x00, 0x01,
        0x05, 0x3f, 0x00, 0x02, 0x04, 0x02, 0x03, 0x7d, 0x09, 0x06, 0x00, 0x01, 0x05, 0x33, 0x00,
        0x02, 0x04, 0x01, 0x03, 0x00, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x05, 0x03, 0x06, 0x09, 0x10,
        0x00, 0x01, 0x05, 0x01, 0x03, 0x01, 0x09, 0x0c, 0x00, 0x01, 0x09, 0x0a, 0x00, 0x00, 0x01,
        0x01, 0xc0, 0x04, 0x00, 0x00, 0x05, 0x00, 0x04, 0x00, 0x2e, 0x00, 0x00, 0x00, 0x01, 0x01,
        0x01, 0xfb, 0x0e, 0x0d, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x01, 0x01, 0x01, 0x1f, 0x02, 0x00, 0x00, 0x00, 0x00, 0x5e, 0x01, 0x00, 0x00, 0x02, 0x01,
        0x1f, 0x02, 0x0f, 0x02, 0x54, 0x01, 0x00, 0x00, 0x01, 0x54, 0x01, 0x00, 0x00, 0x01, 0x05,
        0x29, 0x00, 0x05, 0x02, 0xba, 0x00, 0x00, 0x80, 0x16, 0x05, 0x49, 0x03, 0x01, 0x09, 0x0e,
        0x00, 0x01, 0x05, 0x15, 0x00, 0x02, 0x04, 0x02, 0x03, 0x00, 0x09, 0x0c, 0x00, 0x01, 0x05,
        0x22, 0x00, 0x02, 0x04, 0x04, 0x03, 0x00, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x2f, 0x00, 0x02,
        0x04, 0x06, 0x03, 0x00, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x3c, 0x00, 0x02, 0x04, 0x08, 0x03,
        0x00, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x49, 0x00, 0x02, 0x04, 0x0a, 0x03, 0x00, 0x09, 0x0a,
        0x00, 0x01, 0x00, 0x02, 0x04, 0x0b, 0x06, 0x03, 0x00, 0x09, 0x0a, 0x00, 0x01, 0x03, 0x00,
        0x09, 0x02, 0x00, 0x01, 0x00, 0x02, 0x04, 0x0c, 0x03, 0x00, 0x09, 0x02, 0x00, 0x01, 0x05,
        0x01, 0x06, 0x03, 0x01, 0x09, 0x02, 0x00, 0x01, 0x05, 0x24, 0x03, 0x02, 0x09, 0x0a, 0x00,
        0x01, 0x05, 0x15, 0x03, 0x01, 0x09, 0x0e, 0x00, 0x01, 0x00, 0x02, 0x04, 0x01, 0x06, 0x03,
        0x00, 0x09, 0x0c, 0x00, 0x01, 0x00, 0x02, 0x04, 0x03, 0x03, 0x00, 0x09, 0x0c, 0x00, 0x01,
        0x03, 0x00, 0x09, 0x02, 0x00, 0x01, 0x00, 0x02, 0x04, 0x04, 0x03, 0x00, 0x09, 0x02, 0x00,
        0x01, 0x05, 0x01, 0x06, 0x03, 0x01, 0x09, 0x02, 0x00, 0x01, 0x05, 0x24, 0x03, 0x02, 0x09,
        0x0a, 0x00, 0x01, 0x05, 0x23, 0x03, 0x01, 0x09, 0x0e, 0x00, 0x01, 0x05, 0x16, 0x00, 0x02,
        0x04, 0x01, 0x03, 0x00, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x23, 0x00, 0x02, 0x04, 0x04, 0x03,
        0x00, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x30, 0x00, 0x02, 0x04, 0x05, 0x03, 0x00, 0x09, 0x0c,
        0x00, 0x01, 0x05, 0x23, 0x00, 0x02, 0x04, 0x07, 0x03, 0x00, 0x09, 0x0c, 0x00, 0x01, 0x06,
        0x03, 0x00, 0x09, 0x02, 0x00, 0x01, 0x00, 0x02, 0x04, 0x08, 0x03, 0x00, 0x09, 0x02, 0x00,
        0x01, 0x05, 0x01, 0x06, 0x03, 0x01, 0x09, 0x02, 0x00, 0x01, 0x05, 0x25, 0x03, 0x02, 0x09,
        0x0a, 0x00, 0x01, 0x05, 0x35, 0x03, 0x01, 0x09, 0x0e, 0x00, 0x01, 0x05, 0x16, 0x00, 0x02,
        0x04, 0x01, 0x03, 0x00, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x35, 0x00, 0x02, 0x04, 0x03, 0x03,
        0x00, 0x09, 0x0c, 0x00, 0x01, 0x06, 0x03, 0x00, 0x09, 0x0a, 0x00, 0x01, 0x00, 0x02, 0x04,
        0x04, 0x03, 0x00, 0x09, 0x02, 0x00, 0x01, 0x05, 0x01, 0x06, 0x03, 0x01, 0x09, 0x04, 0x00,
        0x01, 0x05, 0x33, 0x03, 0x02, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x09, 0x03, 0x03, 0x09, 0x12,
        0x00, 0x01, 0x05, 0x08, 0x00, 0x02, 0x04, 0x01, 0x03, 0x00, 0x09, 0x0a, 0x00, 0x01, 0x05,
        0x13, 0x03, 0x01, 0x09, 0x02, 0x00, 0x01, 0x05, 0x0f, 0x03, 0x00, 0x09, 0x04, 0x00, 0x01,
        0x05, 0x10, 0x03, 0x01, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x0f, 0x00, 0x02, 0x04, 0x01, 0x03,
        0x00, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x11, 0x03, 0x01, 0x09, 0x02, 0x00, 0x01, 0x05, 0x0f,
        0x00, 0x02, 0x04, 0x01, 0x03, 0x00, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x10, 0x03, 0x02, 0x09,
        0x0a, 0x00, 0x01, 0x05, 0x23, 0x03, 0x03, 0x09, 0x04, 0x00, 0x01, 0x00, 0x02, 0x04, 0x01,
        0x06, 0x03, 0x00, 0x09, 0x0c, 0x00, 0x01, 0x03, 0x00, 0x09, 0x04, 0x00, 0x01, 0x00, 0x02,
        0x04, 0x02, 0x03, 0x00, 0x09, 0x02, 0x00, 0x01, 0x05, 0x01, 0x06, 0x03, 0x01, 0x09, 0x02,
        0x00, 0x01, 0x05, 0x41, 0x03, 0x02, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x11, 0x03, 0x01, 0x09,
        0x14, 0x00, 0x01, 0x05, 0x13, 0x03, 0x01, 0x09, 0x08, 0x00, 0x01, 0x05, 0x09, 0x03, 0x01,
        0x09, 0x04, 0x00, 0x01, 0x03, 0x01, 0x09, 0x06, 0x00, 0x01, 0x03, 0x01, 0x09, 0x04, 0x00,
        0x01, 0x05, 0x0b, 0x03, 0x03, 0x09, 0x04, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x01, 0x09, 0x02,
        0x00, 0x01, 0x03, 0x7f, 0x09, 0x0a, 0x00, 0x01, 0x00, 0x02, 0x04, 0x01, 0x06, 0x03, 0x00,
        0x09, 0x0e, 0x00, 0x01, 0x05, 0x09, 0x06, 0x03, 0x05, 0x09, 0x02, 0x00, 0x01, 0x05, 0x08,
        0x03, 0x00, 0x09, 0x08, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x01, 0x09, 0x08, 0x00, 0x01, 0x05,
        0x10, 0x03, 0x01, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x0f, 0x03, 0x00, 0x09, 0x08, 0x00, 0x01,
        0x05, 0x0e, 0x03, 0x01, 0x09, 0x08, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x01, 0x09, 0x06, 0x00,
        0x01, 0x05, 0x08, 0x03, 0x04, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x0d, 0x03, 0x01, 0x09, 0x06,
        0x00, 0x01, 0x05, 0x0c, 0x03, 0x00, 0x09, 0x08, 0x00, 0x01, 0x05, 0x21, 0x03, 0x01, 0x09,
        0x08, 0x00, 0x01, 0x05, 0x12, 0x03, 0x00, 0x09, 0x06, 0x00, 0x01, 0x05, 0x10, 0x03, 0x01,
        0x09, 0x08, 0x00, 0x01, 0x05, 0x22, 0x00, 0x02, 0x04, 0x01, 0x03, 0x00, 0x09, 0x0c, 0x00,
        0x01, 0x05, 0x16, 0x03, 0x01, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x15, 0x03, 0x01, 0x09, 0x06,
        0x00, 0x01, 0x05, 0x16, 0x03, 0x02, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x14, 0x03, 0x01, 0x09,
        0x06, 0x00, 0x01, 0x05, 0x12, 0x03, 0x03, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x0f, 0x03, 0x02,
        0x09, 0x08, 0x00, 0x01, 0x05, 0x0d, 0x03, 0x02, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x0c, 0x03,
        0x00, 0x09, 0x08, 0x00, 0x01, 0x05, 0x20, 0x00, 0x02, 0x04, 0x01, 0x03, 0x00, 0x09, 0x08,
        0x00, 0x01, 0x05, 0x19, 0x00, 0x02, 0x04, 0x01, 0x03, 0x00, 0x09, 0x0a, 0x00, 0x01, 0x05,
        0x31, 0x00, 0x02, 0x04, 0x02, 0x03, 0x00, 0x09, 0x08, 0x00, 0x01, 0x05, 0x2b, 0x00, 0x02,
        0x04, 0x02, 0x03, 0x00, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x11, 0x03, 0x01, 0x09, 0x08, 0x00,
        0x01, 0x05, 0x08, 0x03, 0x05, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x12, 0x00, 0x02, 0x04, 0x01,
        0x03, 0x00, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x01, 0x09, 0x0c, 0x00, 0x01, 0x05,
        0x1d, 0x00, 0x02, 0x04, 0x01, 0x03, 0x00, 0x09, 0x06, 0x00, 0x01, 0x05, 0x10, 0x03, 0x01,
        0x09, 0x0a, 0x00, 0x01, 0x05, 0x26, 0x03, 0x04, 0x09, 0x04, 0x00, 0x01, 0x05, 0x13, 0x03,
        0x00, 0x09, 0x04, 0x00, 0x01, 0x05, 0x1c, 0x03, 0x01, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x09,
        0x03, 0x00, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x0b, 0x03, 0x02, 0x09, 0x04, 0x00, 0x01, 0x05,
        0x15, 0x03, 0x01, 0x09, 0x02, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x02, 0x09, 0x14, 0x00, 0x01,
        0x05, 0x16, 0x03, 0x04, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x03, 0x09, 0x06, 0x00,
        0x01, 0x05, 0x10, 0x03, 0x01, 0x09, 0x06, 0x00, 0x01, 0x05, 0x21, 0x00, 0x02, 0x04, 0x01,
        0x03, 0x00, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x39, 0x00, 0x02, 0x04, 0x02, 0x03, 0x00, 0x09,
        0x0c, 0x00, 0x01, 0x05, 0x50, 0x00, 0x02, 0x04, 0x02, 0x03, 0x00, 0x09, 0x04, 0x00, 0x01,
        0x05, 0x36, 0x00, 0x02, 0x04, 0x02, 0x03, 0x00, 0x09, 0x04, 0x00, 0x01, 0x05, 0x1a, 0x03,
        0x01, 0x09, 0x04, 0x00, 0x01, 0x05, 0x18, 0x03, 0x01, 0x09, 0x06, 0x00, 0x01, 0x05, 0x21,
        0x03, 0x02, 0x09, 0x08, 0x00, 0x01, 0x05, 0x2a, 0x03, 0x00, 0x09, 0x0c, 0x00, 0x01, 0x05,
        0x18, 0x03, 0x00, 0x09, 0x04, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x04, 0x09, 0x06, 0x00, 0x01,
        0x03, 0x6d, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x11, 0x03, 0x00, 0x09, 0x08, 0x00, 0x01, 0x05,
        0x0d, 0x03, 0x04, 0x09, 0x04, 0x00, 0x01, 0x05, 0x08, 0x03, 0x13, 0x09, 0x02, 0x00, 0x01,
        0x05, 0x11, 0x03, 0x01, 0x09, 0x06, 0x00, 0x01, 0x00, 0x02, 0x04, 0x01, 0x06, 0x03, 0x00,
        0x09, 0x06, 0x00, 0x01, 0x00, 0x02, 0x04, 0x02, 0x03, 0x00, 0x09, 0x06, 0x00, 0x01, 0x00,
        0x02, 0x04, 0x04, 0x03, 0x00, 0x09, 0x04, 0x00, 0x01, 0x05, 0x08, 0x06, 0x03, 0x04, 0x09,
        0x06, 0x00, 0x01, 0x05, 0x10, 0x03, 0x01, 0x09, 0x06, 0x00, 0x01, 0x05, 0x08, 0x03, 0x03,
        0x09, 0x04, 0x00, 0x01, 0x05, 0x10, 0x03, 0x01, 0x09, 0x06, 0x00, 0x01, 0x05, 0x08, 0x03,
        0x04, 0x09, 0x04, 0x00, 0x01, 0x05, 0x23, 0x03, 0x03, 0x09, 0x08, 0x00, 0x01, 0x05, 0x22,
        0x03, 0x00, 0x09, 0x04, 0x00, 0x01, 0x05, 0x10, 0x03, 0x00, 0x09, 0x04, 0x00, 0x01, 0x05,
        0x0c, 0x03, 0x03, 0x09, 0x04, 0x00, 0x01, 0x05, 0x01, 0x03, 0x01, 0x09, 0x04, 0x00, 0x01,
        0x09, 0x0a, 0x00, 0x00, 0x01, 0x01,
    ];

    /// `.debug_line_str`（v5 的路径字符串）（381 字节）。
    pub const OSZ_DLS: &[u8] = &[
        0x2f, 0x74, 0x6d, 0x70, 0x2f, 0x6f, 0x73, 0x7a, 0x2d, 0x66, 0x69, 0x78, 0x32, 0x00, 0x2f,
        0x74, 0x6d, 0x70, 0x2f, 0x6f, 0x73, 0x7a, 0x2d, 0x70, 0x72, 0x6f, 0x62, 0x65, 0x2f, 0x6b,
        0x65, 0x72, 0x6e, 0x65, 0x6c, 0x2f, 0x62, 0x61, 0x73, 0x65, 0x2f, 0x68, 0x6f, 0x6f, 0x6b,
        0x2f, 0x73, 0x72, 0x63, 0x2f, 0x68, 0x6f, 0x6f, 0x6b, 0x2e, 0x63, 0x00, 0x2f, 0x74, 0x6d,
        0x70, 0x2f, 0x6f, 0x73, 0x7a, 0x2d, 0x70, 0x72, 0x6f, 0x62, 0x65, 0x2f, 0x6b, 0x65, 0x72,
        0x6e, 0x65, 0x6c, 0x2f, 0x62, 0x61, 0x73, 0x65, 0x2f, 0x68, 0x6f, 0x6f, 0x6b, 0x2f, 0x73,
        0x72, 0x63, 0x00, 0x2f, 0x74, 0x6d, 0x70, 0x2f, 0x6f, 0x73, 0x7a, 0x2d, 0x70, 0x72, 0x6f,
        0x62, 0x65, 0x2f, 0x6b, 0x65, 0x72, 0x6e, 0x65, 0x6c, 0x2f, 0x63, 0x6f, 0x6d, 0x6d, 0x2f,
        0x69, 0x6e, 0x63, 0x6c, 0x75, 0x64, 0x65, 0x00, 0x2f, 0x74, 0x6d, 0x70, 0x2f, 0x6f, 0x73,
        0x7a, 0x2d, 0x70, 0x72, 0x6f, 0x62, 0x65, 0x2f, 0x6b, 0x65, 0x72, 0x6e, 0x65, 0x6c, 0x2f,
        0x62, 0x61, 0x73, 0x65, 0x2f, 0x68, 0x6f, 0x6f, 0x6b, 0x2f, 0x69, 0x6e, 0x63, 0x6c, 0x75,
        0x64, 0x65, 0x00, 0x7a, 0x74, 0x79, 0x70, 0x65, 0x2e, 0x68, 0x00, 0x68, 0x6f, 0x6f, 0x6b,
        0x2e, 0x68, 0x00, 0x2f, 0x74, 0x6d, 0x70, 0x2f, 0x6f, 0x73, 0x7a, 0x2d, 0x70, 0x72, 0x6f,
        0x62, 0x65, 0x2f, 0x6b, 0x65, 0x72, 0x6e, 0x65, 0x6c, 0x2f, 0x62, 0x61, 0x73, 0x65, 0x2f,
        0x69, 0x6e, 0x69, 0x74, 0x2f, 0x73, 0x72, 0x63, 0x2f, 0x69, 0x6e, 0x69, 0x74, 0x2e, 0x63,
        0x00, 0x2f, 0x74, 0x6d, 0x70, 0x2f, 0x6f, 0x73, 0x7a, 0x2d, 0x70, 0x72, 0x6f, 0x62, 0x65,
        0x2f, 0x6b, 0x65, 0x72, 0x6e, 0x65, 0x6c, 0x2f, 0x62, 0x61, 0x73, 0x65, 0x2f, 0x69, 0x6e,
        0x69, 0x74, 0x2f, 0x73, 0x72, 0x63, 0x00, 0x2f, 0x74, 0x6d, 0x70, 0x2f, 0x6f, 0x73, 0x7a,
        0x2d, 0x70, 0x72, 0x6f, 0x62, 0x65, 0x2f, 0x6b, 0x65, 0x72, 0x6e, 0x65, 0x6c, 0x2f, 0x62,
        0x61, 0x73, 0x65, 0x2f, 0x69, 0x6e, 0x69, 0x74, 0x2f, 0x69, 0x6e, 0x63, 0x6c, 0x75, 0x64,
        0x65, 0x00, 0x69, 0x6e, 0x69, 0x74, 0x2e, 0x68, 0x00, 0x2f, 0x74, 0x6d, 0x70, 0x2f, 0x6f,
        0x73, 0x7a, 0x2d, 0x70, 0x72, 0x6f, 0x62, 0x65, 0x2f, 0x6b, 0x65, 0x72, 0x6e, 0x65, 0x6c,
        0x2f, 0x63, 0x6f, 0x6d, 0x6d, 0x2f, 0x73, 0x72, 0x63, 0x2f, 0x73, 0x74, 0x72, 0x74, 0x6f,
        0x75, 0x6c, 0x2e, 0x63, 0x00, 0x2f, 0x74, 0x6d, 0x70, 0x2f, 0x6f, 0x73, 0x7a, 0x2d, 0x70,
        0x72, 0x6f, 0x62, 0x65, 0x2f, 0x6b, 0x65, 0x72, 0x6e, 0x65, 0x6c, 0x2f, 0x63, 0x6f, 0x6d,
        0x6d, 0x2f, 0x73, 0x72, 0x63, 0x00,
    ];

    /// `.debug_line`（DWARF 2 · 2 个单元）（230 字节）。
    pub const V2_DL: &[u8] = &[
        0x9a, 0x00, 0x00, 0x00, 0x02, 0x00, 0x1e, 0x00, 0x00, 0x00, 0x01, 0x01, 0xfb, 0x0e, 0x0a,
        0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01, 0x73, 0x72, 0x63, 0x00, 0x00, 0x6d,
        0x61, 0x69, 0x6e, 0x2e, 0x63, 0x00, 0x01, 0x00, 0x00, 0x00, 0x05, 0x12, 0x00, 0x05, 0x02,
        0x94, 0x00, 0x01, 0x00, 0x01, 0x05, 0x09, 0x03, 0x01, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x07,
        0x03, 0x01, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x01, 0x09, 0x0a, 0x00, 0x01, 0x05,
        0x01, 0x03, 0x01, 0x09, 0x04, 0x00, 0x01, 0x05, 0x13, 0x03, 0x02, 0x09, 0x0a, 0x00, 0x01,
        0x05, 0x09, 0x03, 0x01, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x01, 0x09, 0x0e, 0x00,
        0x01, 0x05, 0x01, 0x03, 0x01, 0x09, 0x04, 0x00, 0x01, 0x05, 0x10, 0x03, 0x02, 0x09, 0x0a,
        0x00, 0x01, 0x05, 0x0d, 0x03, 0x01, 0x09, 0x08, 0x00, 0x01, 0x03, 0x01, 0x09, 0x08, 0x00,
        0x01, 0x05, 0x0e, 0x03, 0x01, 0x09, 0x08, 0x00, 0x01, 0x05, 0x01, 0x03, 0x01, 0x09, 0x0a,
        0x00, 0x01, 0x09, 0x0a, 0x00, 0x00, 0x01, 0x01, 0x44, 0x00, 0x00, 0x00, 0x02, 0x00, 0x1e,
        0x00, 0x00, 0x00, 0x01, 0x01, 0xfb, 0x0e, 0x0a, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x01, 0x73, 0x72, 0x63, 0x00, 0x00, 0x75, 0x74, 0x69, 0x6c, 0x2e, 0x63, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x05, 0x13, 0x00, 0x05, 0x02, 0x16, 0x01, 0x01, 0x00, 0x01, 0x05, 0x0e,
        0x03, 0x01, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x01, 0x03, 0x01, 0x09, 0x06, 0x00, 0x01, 0x09,
        0x0a, 0x00, 0x00, 0x01, 0x01,
    ];

    /// `.debug_line`（DWARF 3 · 2 个单元）（236 字节）。
    pub const V3_DL: &[u8] = &[
        0x9d, 0x00, 0x00, 0x00, 0x03, 0x00, 0x21, 0x00, 0x00, 0x00, 0x01, 0x01, 0xfb, 0x0e, 0x0d,
        0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x73, 0x72, 0x63,
        0x00, 0x00, 0x6d, 0x61, 0x69, 0x6e, 0x2e, 0x63, 0x00, 0x01, 0x00, 0x00, 0x00, 0x05, 0x12,
        0x00, 0x05, 0x02, 0x94, 0x00, 0x01, 0x00, 0x01, 0x05, 0x09, 0x03, 0x01, 0x09, 0x0c, 0x00,
        0x01, 0x05, 0x07, 0x03, 0x01, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x01, 0x09, 0x0a,
        0x00, 0x01, 0x05, 0x01, 0x03, 0x01, 0x09, 0x04, 0x00, 0x01, 0x05, 0x13, 0x03, 0x02, 0x09,
        0x0a, 0x00, 0x01, 0x05, 0x09, 0x03, 0x01, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x01,
        0x09, 0x0e, 0x00, 0x01, 0x05, 0x01, 0x03, 0x01, 0x09, 0x04, 0x00, 0x01, 0x05, 0x10, 0x03,
        0x02, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x0d, 0x03, 0x01, 0x09, 0x08, 0x00, 0x01, 0x03, 0x01,
        0x09, 0x08, 0x00, 0x01, 0x05, 0x0e, 0x03, 0x01, 0x09, 0x08, 0x00, 0x01, 0x05, 0x01, 0x03,
        0x01, 0x09, 0x0a, 0x00, 0x01, 0x09, 0x0a, 0x00, 0x00, 0x01, 0x01, 0x47, 0x00, 0x00, 0x00,
        0x03, 0x00, 0x21, 0x00, 0x00, 0x00, 0x01, 0x01, 0xfb, 0x0e, 0x0d, 0x00, 0x01, 0x01, 0x01,
        0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x73, 0x72, 0x63, 0x00, 0x00, 0x75, 0x74,
        0x69, 0x6c, 0x2e, 0x63, 0x00, 0x01, 0x00, 0x00, 0x00, 0x05, 0x13, 0x00, 0x05, 0x02, 0x16,
        0x01, 0x01, 0x00, 0x01, 0x05, 0x0e, 0x03, 0x01, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x01, 0x03,
        0x01, 0x09, 0x06, 0x00, 0x01, 0x09, 0x0a, 0x00, 0x00, 0x01, 0x01,
    ];

    /// `.debug_line`（DWARF 4 · 2 个单元）（238 字节）。
    pub const V4_DL: &[u8] = &[
        0x9e, 0x00, 0x00, 0x00, 0x04, 0x00, 0x22, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0xfb, 0x0e,
        0x0d, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x73, 0x72,
        0x63, 0x00, 0x00, 0x6d, 0x61, 0x69, 0x6e, 0x2e, 0x63, 0x00, 0x01, 0x00, 0x00, 0x00, 0x05,
        0x12, 0x00, 0x05, 0x02, 0x94, 0x00, 0x01, 0x00, 0x01, 0x05, 0x09, 0x03, 0x01, 0x09, 0x0c,
        0x00, 0x01, 0x05, 0x07, 0x03, 0x01, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x0c, 0x03, 0x01, 0x09,
        0x0a, 0x00, 0x01, 0x05, 0x01, 0x03, 0x01, 0x09, 0x04, 0x00, 0x01, 0x05, 0x13, 0x03, 0x02,
        0x09, 0x0a, 0x00, 0x01, 0x05, 0x09, 0x03, 0x01, 0x09, 0x0c, 0x00, 0x01, 0x05, 0x0c, 0x03,
        0x01, 0x09, 0x0e, 0x00, 0x01, 0x05, 0x01, 0x03, 0x01, 0x09, 0x04, 0x00, 0x01, 0x05, 0x10,
        0x03, 0x02, 0x09, 0x0a, 0x00, 0x01, 0x05, 0x0d, 0x03, 0x01, 0x09, 0x08, 0x00, 0x01, 0x03,
        0x01, 0x09, 0x08, 0x00, 0x01, 0x05, 0x0e, 0x03, 0x01, 0x09, 0x08, 0x00, 0x01, 0x05, 0x01,
        0x03, 0x01, 0x09, 0x0a, 0x00, 0x01, 0x09, 0x0a, 0x00, 0x00, 0x01, 0x01, 0x48, 0x00, 0x00,
        0x00, 0x04, 0x00, 0x22, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0xfb, 0x0e, 0x0d, 0x00, 0x01,
        0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x73, 0x72, 0x63, 0x00, 0x00,
        0x75, 0x74, 0x69, 0x6c, 0x2e, 0x63, 0x00, 0x01, 0x00, 0x00, 0x00, 0x05, 0x13, 0x00, 0x05,
        0x02, 0x16, 0x01, 0x01, 0x00, 0x01, 0x05, 0x0e, 0x03, 0x01, 0x09, 0x0c, 0x00, 0x01, 0x05,
        0x01, 0x03, 0x01, 0x09, 0x06, 0x00, 0x01, 0x09, 0x0a, 0x00, 0x00, 0x01, 0x01,
    ];
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::fixtures::{OSZ_DL, OSZ_DLS, V2_DL, V3_DL, V4_DL};
    use super::*;

    /// osz 夹具里 `hook.c` 的**完整路径**（v5 的目录表首项 = 编译目录 ⇒
    /// 能拼出完整路径）。
    const OSZ_HOOK: &str = "/tmp/osz-probe/kernel/base/hook/src/hook.c";
    /// osz 夹具里 `init.c` 的完整路径。
    const OSZ_INIT: &str = "/tmp/osz-probe/kernel/base/init/src/init.c";
    /// osz 夹具里 `strtoul.c` 的完整路径。
    const OSZ_STRTOUL: &str = "/tmp/osz-probe/kernel/comm/src/strtoul.c";

    /// osz 的节区集合（`.debug_line` + `.debug_line_str`）。
    fn osz_sections() -> Vec<(String, Vec<u8>)> {
        vec![
            (DEBUG_LINE_SECTION.to_string(), OSZ_DL.to_vec()),
            (DEBUG_LINE_STR_SECTION.to_string(), OSZ_DLS.to_vec()),
        ]
    }

    /// v2~v4 夹具的节区集合（只有 `.debug_line`）。
    fn v_sections(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        vec![(DEBUG_LINE_SECTION.to_string(), bytes.to_vec())]
    }

    /// 断言某个地址命中，并返回 `(file, line)`。
    fn found(lines: &DwarfLines, addr: u32) -> (String, u32) {
        match lines.resolve(addr) {
            LineLookup::Found(entry) => (entry.file, entry.line),
            other => panic!("地址 {addr:#010x} 应当命中，实际是 {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // A-4：v5 真产物 ⇒ 真的 file + line（真值来自 `addr2line` 与
    // `readelf --debug-dump=decodedline` 两个**独立** oracle）。
    // -----------------------------------------------------------------

    #[test]
    fn a_v5_real_product_resolves_the_true_file_and_line() {
        let lines = DwarfLines::from_sections(&osz_sections()).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Supported(5));
        assert_eq!(lines.version(), Some(5));
        // 三个编译单元（hook.c / init.c / strtoul.c）。
        assert_eq!(lines.units_parsed(), 3);
        assert_eq!(lines.units_degraded(), 0);

        // hook.c（第一个 CU）
        assert_eq!(found(&lines, 0x8000_0000), (OSZ_HOOK.to_string(), 8));
        assert_eq!(found(&lines, 0x8000_0004), (OSZ_HOOK.to_string(), 8));
        assert_eq!(found(&lines, 0x8000_0008), (OSZ_HOOK.to_string(), 9));
        assert_eq!(found(&lines, 0x8000_000c), (OSZ_HOOK.to_string(), 9));
        assert_eq!(found(&lines, 0x8000_0010), (OSZ_HOOK.to_string(), 10));
        assert_eq!(found(&lines, 0x8000_0020), (OSZ_HOOK.to_string(), 10));
        assert_eq!(found(&lines, 0x8000_0040), (OSZ_HOOK.to_string(), 15));
        // init.c（第二个 CU）—— 地址落在**另一个**单元里。
        assert_eq!(found(&lines, 0x8000_0060), (OSZ_INIT.to_string(), 12));
        // strtoul.c（第三个 CU）
        assert_eq!(
            found(&lines, 0x8000_00ba),
            (OSZ_STRTOUL.to_string(), 5),
            "第三个 CU 的地址也要查得到（A-13）"
        );
    }

    #[test]
    fn the_v5_header_self_check_pins_the_program_start() {
        // ⚠️ `行号程序起始 == 12 + header_length`（DWARF32 的 v5）是一道
        // **很便宜的自校验**：它一次性抓住 v5 头部的所有错位（把
        // `line_base` 当 SLEB128 读这种经典错误会让它对不上）。
        let lines = DwarfLines::from_sections(&osz_sections()).expect("不得报错");
        let headers = lines.unit_headers();
        assert_eq!(headers.len(), 3, "三个行号程序单元");
        for h in headers {
            assert_eq!(h.version, 5);
            assert_eq!(
                h.program_start,
                12 + h.header_length as usize,
                "行号程序的起始必须落在 header_length 之后（header = {h:?}）"
            );
            assert!(h.program_end > h.program_start);
        }
    }

    // -----------------------------------------------------------------
    // A-13：多个行号程序单元 ⇒ 归并成一张表，**都能查到**。
    // -----------------------------------------------------------------

    #[test]
    fn several_line_program_units_are_merged_into_one_table() {
        let lines = DwarfLines::from_sections(&osz_sections()).expect("不得报错");
        // 三个单元的行记录**都在同一张表**里。
        let files: Vec<String> = lines.raw_rows().iter().map(|r| r.file.clone()).collect();
        assert!(
            files.iter().any(|f| f == OSZ_HOOK),
            "hook.c 的行记录必须在表里"
        );
        assert!(
            files.iter().any(|f| f == OSZ_INIT),
            "init.c 的行记录必须在表里"
        );
        assert!(
            files.iter().any(|f| f == OSZ_STRTOUL),
            "strtoul.c 的行记录必须在表里"
        );
        // 三个 CU 的地址分别落在不同的单元 ⇒ 都能查到。
        assert_eq!(found(&lines, 0x8000_0000).0, OSZ_HOOK);
        assert_eq!(found(&lines, 0x8000_0060).0, OSZ_INIT);
        assert_eq!(found(&lines, 0x8000_00ba).0, OSZ_STRTOUL);
    }

    // -----------------------------------------------------------------
    // A-1 / A-2 / A-3：v2 / v3 / v4 真产物 ⇒ 真的 file + line。
    // -----------------------------------------------------------------

    #[test]
    fn v2_v3_and_v4_resolve_the_true_file_and_line() {
        for (version, bytes) in [(2u16, V2_DL), (3, V3_DL), (4, V4_DL)] {
            let lines = DwarfLines::from_sections(&v_sections(bytes)).expect("不得报错");
            assert_eq!(lines.status(), DwarfStatus::Supported(version));
            assert_eq!(lines.units_parsed(), 2, "v{version}：两个单元");
            // 第一个单元（main.c）
            assert_eq!(
                found(&lines, 0x0001_0094),
                ("src/main.c".to_string(), 1),
                "v{version} 的首行"
            );
            assert_eq!(
                found(&lines, 0x0001_00a0),
                ("src/main.c".to_string(), 2),
                "v{version} 的第二行"
            );
            assert_eq!(
                found(&lines, 0x0001_00b8),
                ("src/main.c".to_string(), 5),
                "v{version} 的第五行"
            );
            // 第二个单元（util.c）—— 地址落在**另一个**单元里。
            assert_eq!(
                found(&lines, 0x0001_0116),
                ("src/util.c".to_string(), 1),
                "v{version}：第二个单元的第一行"
            );
            assert_eq!(
                found(&lines, 0x0001_0122),
                ("src/util.c".to_string(), 2),
                "v{version}：第二个单元的第二行"
            );
        }
    }

    #[test]
    fn the_v2_v4_path_does_not_invent_the_compilation_directory() {
        // ⚠️ 裁决 B 的**已知代价**：v2~v4 的目录索引 0 = 编译单元的
        // `DW_AT_comp_dir`（在 `.debug_info` 里）⇒ 拿不到 ⇒ **只有文件名**
        // （这里目录索引 1 = "src" 是**知道**的，所以拼出 "src/main.c"）。
        //
        // ⛔ **绝不**假装拿到了完整路径。
        let lines = DwarfLines::from_sections(&v_sections(V2_DL)).expect("不得报错");
        let (file, _) = found(&lines, 0x0001_0094);
        assert_eq!(file, "src/main.c");
        assert!(
            !file.starts_with('/'),
            "v2~v4 拿不到编译目录 ⇒ 路径**不该**是绝对路径：{file}"
        );
    }

    // -----------------------------------------------------------------
    // A-6：特殊操作码（adjusted opcode）。
    // -----------------------------------------------------------------

    #[test]
    fn the_special_opcode_advances_the_address_and_the_line() {
        // osz 的第一个单元的第一行就是「特殊操作码 12」产生的：
        // 行号从 1 跳到 8（`line_base = -5`，`line_range = 14`）。
        let lines = DwarfLines::from_sections(&osz_sections()).expect("不得报错");
        assert_eq!(found(&lines, 0x8000_0000).1, 8, "特殊操作码的行增量");
        assert_eq!(found(&lines, 0x8000_0000).0, OSZ_HOOK);
    }

    // -----------------------------------------------------------------
    // A-5：状态机（标准操作码 + 扩展操作码）。
    // -----------------------------------------------------------------

    /// 造一个 v4 单元：把**每个**标准操作码都用一遍。
    ///
    /// 行号程序（`min_inst_len = 1`，`line_base = -5`，`line_range = 14`，
    /// `opcode_base = 13`）：
    ///
    /// ```text
    /// set_address 0x1000
    /// copy                      ⇒ (0x1000, inc/a.c, 1)
    /// advance_pc 4              ⇒ 0x1004
    /// advance_line +2           ⇒ 行 3
    /// copy                      ⇒ (0x1004, inc/a.c, 3)
    /// set_file 2                ⇒ b.c
    /// negate_stmt               ⇒ is_stmt = false
    /// set_column 7
    /// const_add_pc              ⇒ +17 ⇒ 0x1015
    /// copy                      ⇒ (0x1015, b.c, 3)
    /// fixed_advance_pc 0x10     ⇒ +16（**不乘** min_inst_len）⇒ 0x1025
    /// advance_line -1           ⇒ 行 2
    /// copy                      ⇒ (0x1025, b.c, 2)
    /// advance_pc 2              ⇒ 0x1027
    /// define_file "c.c" dir 0   ⇒ 文件表追加第 3 项
    /// set_file 3
    /// copy                      ⇒ (0x1027, c.c, 2)
    /// advance_pc 4              ⇒ 0x102b
    /// end_sequence              ⇒ 本段到 0x102b 为止
    /// ```
    fn state_machine_unit() -> Vec<u8> {
        // v2~v4 头部（不含 `unit_length` / `version` / `header_length`）。
        let mut header: Vec<u8> = vec![
            1,    // minimum_instruction_length
            1,    // maximum_operations_per_instruction（v4+）
            1,    // default_is_stmt
            0xfb, // line_base = -5
            0x0e, // line_range = 14
            0x0d, // opcode_base = 13
        ];
        header.extend_from_slice(&[0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1]); // standard_opcode_lengths
        header.extend_from_slice(b"inc\0"); // 目录 1
        header.push(0); // 目录表结束（空串）
        header.extend_from_slice(b"a.c\0"); // 文件 1
        header.extend_from_slice(&[1, 0, 0]); // dir=1, mtime=0, len=0
        header.extend_from_slice(b"b.c\0"); // 文件 2
        header.extend_from_slice(&[0, 0, 0]); // dir=0（编译目录拿不到）
        header.push(0); // 文件表结束（空 name）

        let mut program: Vec<u8> = Vec::new();
        // DW_LNE_set_address（长度 = 1 + 4）
        program.push(0);
        program.push(5);
        program.push(2);
        program.extend_from_slice(&0x1000u32.to_le_bytes());
        program.push(1); // copy
        program.push(2); // advance_pc
        program.push(4); // 操作数 4
        program.push(3); // advance_line
        program.push(2); // 操作数 +2（SLEB）
        program.push(1); // copy
        program.push(4); // set_file
        program.push(2); // 文件 2
        program.push(6); // negate_stmt
        program.push(5); // set_column
        program.push(7); // 操作数 7
        program.push(8); // const_add_pc
        program.push(1); // copy
        program.push(9); // fixed_advance_pc
        program.extend_from_slice(&0x10u16.to_le_bytes());
        program.push(3); // advance_line
        program.push(0x7f); // 操作数 -1（SLEB）
        program.push(1); // copy
        program.push(2); // advance_pc
        program.push(2); // 操作数 2
                         // DW_LNE_define_file（长度 = 1 + name(4) + 3 个 ULEB）
        program.push(0);
        program.push(8);
        program.push(3);
        program.extend_from_slice(b"c.c\0");
        program.extend_from_slice(&[0, 0, 0]); // dir=0, mtime=0, len=0
        program.push(4); // set_file
        program.push(3); // 文件 3
        program.push(1); // copy
        program.push(2); // advance_pc
        program.push(4); // 操作数 4
                         // DW_LNE_end_sequence（长度 = 1）
        program.push(0);
        program.push(1);
        program.push(1);

        let mut unit: Vec<u8> = Vec::new();
        let body_len = 2 + 4 + header.len() + program.len();
        unit.extend_from_slice(&(body_len as u32).to_le_bytes());
        unit.extend_from_slice(&4u16.to_le_bytes());
        unit.extend_from_slice(&(header.len() as u32).to_le_bytes());
        unit.extend_from_slice(&header);
        unit.extend_from_slice(&program);
        unit
    }

    #[test]
    fn the_state_machine_handles_every_standard_opcode() {
        let lines =
            DwarfLines::from_sections(&v_sections(&state_machine_unit())).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Supported(4));
        let rows = lines.raw_rows();
        let shape: Vec<(u32, String, u32, bool, u32)> = rows
            .iter()
            .map(|r| (r.addr, r.file.clone(), r.line, r.is_stmt, r.column))
            .collect();
        assert_eq!(
            shape,
            vec![
                (0x1000, "inc/a.c".to_string(), 1, true, 0),
                (0x1004, "inc/a.c".to_string(), 3, true, 0),
                // `negate_stmt` 翻了 `is_stmt` ⇒ 后面的行都是 false。
                (0x1015, "b.c".to_string(), 3, false, 7),
                (0x1025, "b.c".to_string(), 2, false, 7),
                // `define_file` 追加的文件表项。
                (0x1027, "c.c".to_string(), 2, false, 7),
            ],
            "状态机的每一行都必须算对"
        );
    }

    #[test]
    fn the_state_machine_lookup_follows_the_rows() {
        let lines =
            DwarfLines::from_sections(&v_sections(&state_machine_unit())).expect("不得报错");
        assert_eq!(found(&lines, 0x1000), ("inc/a.c".to_string(), 1));
        assert_eq!(found(&lines, 0x1003), ("inc/a.c".to_string(), 1));
        assert_eq!(found(&lines, 0x1004), ("inc/a.c".to_string(), 3));
        assert_eq!(found(&lines, 0x1014), ("inc/a.c".to_string(), 3));
        assert_eq!(found(&lines, 0x1015), ("b.c".to_string(), 3));
        assert_eq!(found(&lines, 0x1024), ("b.c".to_string(), 3));
        assert_eq!(found(&lines, 0x1025), ("b.c".to_string(), 2));
        assert_eq!(found(&lines, 0x1026), ("b.c".to_string(), 2));
        assert_eq!(found(&lines, 0x1027), ("c.c".to_string(), 2));
        assert_eq!(found(&lines, 0x102a), ("c.c".to_string(), 2));
        // `end_sequence` 的地址**不属于**这一段 ⇒ 查不到。
        assert_eq!(lines.resolve(0x102b), LineLookup::Missed);
    }

    // -----------------------------------------------------------------
    // A-7：`end_sequence` 之后**不**把行记录延续到下一段。
    // -----------------------------------------------------------------

    #[test]
    fn the_end_of_a_sequence_does_not_bleed_into_the_next_one() {
        // v2 夹具：单元 1 在 0x10116 结束，单元 2 从 0x10116 开始。
        let lines = DwarfLines::from_sections(&v_sections(V2_DL)).expect("不得报错");
        let (file, line) = found(&lines, 0x0001_0116);
        assert_eq!(
            (file.as_str(), line),
            ("src/util.c", 1),
            "0x10116 是**第二个**单元的第一行，不是第一个单元的尾巴"
        );
        // 单元 1 的最后一行仍然归它自己。
        assert_eq!(found(&lines, 0x0001_010c), ("src/main.c".to_string(), 16));
        // 最后一个单元的 `end_sequence` 地址 ⇒ 查不到（不是「延续」）。
        assert_eq!(lines.resolve(0x0001_0132), LineLookup::Missed);
    }

    // -----------------------------------------------------------------
    // A-8：查过行表、没命中 ⇒ `Missed`（**不是** `NoDwarf`）。
    // -----------------------------------------------------------------

    #[test]
    fn an_address_outside_every_sequence_is_a_miss_not_a_no_dwarf() {
        let lines = DwarfLines::from_sections(&osz_sections()).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Supported(5));
        for addr in [0u32, 0x1000, 0x7fff_ffff, 0xffff_ffff] {
            assert_eq!(
                lines.resolve(addr),
                LineLookup::Missed,
                "地址 {addr:#010x} 没命中行表 ⇒ `Missed`（**不是** `NoDwarf`）"
            );
        }
    }

    // -----------------------------------------------------------------
    // A-9：没有 `.debug_line` ⇒ `NoDwarf`。
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
        let secs = vec![
            (".debug_info".to_string(), vec![0x01, 0x02, 0x03]),
            (".debug_abbrev".to_string(), vec![0x04, 0x05]),
        ];
        let lines = DwarfLines::from_sections(&secs).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Absent);
        assert_eq!(lines.resolve(0x1000), LineLookup::NoDwarf);
    }

    // -----------------------------------------------------------------
    // A-10：畸形输入 ⇒ 降级，**不 panic**，**不给**猜出来的行号。
    // -----------------------------------------------------------------

    #[test]
    fn a_truncated_debug_line_degrades_without_panicking() {
        let truncated = &OSZ_DL[..OSZ_DL.len() / 2];
        let lines = DwarfLines::from_sections(&[
            (DEBUG_LINE_SECTION.to_string(), truncated.to_vec()),
            (DEBUG_LINE_STR_SECTION.to_string(), OSZ_DLS.to_vec()),
        ])
        .expect("畸形输入**不是**加载失败");
        // ⚠️ 截断在**第三个**单元 ⇒ 前两个单元**完好** ⇒ 它们的行记录
        // **仍然可用**（那不是「猜出来的行号」，是真行号）。
        assert_eq!(lines.status(), DwarfStatus::Supported(5));
        assert_eq!(lines.units_parsed(), 2, "完好的两个单元照常解析");
        assert_eq!(lines.units_degraded(), 1, "被截断的单元降级");
        assert_eq!(found(&lines, 0x8000_0000), (OSZ_HOOK.to_string(), 8));
        assert_eq!(found(&lines, 0x8000_0060), (OSZ_INIT.to_string(), 12));
        // 被截断的单元（strtoul.c）的地址**查不到**（**没查** ⇒ `Missed`）。
        assert_eq!(lines.resolve(0x8000_00ba), LineLookup::Missed);
    }

    #[test]
    fn a_truncation_inside_the_first_unit_degrades_the_whole_section() {
        // 截断在**第一个**单元里 ⇒ 边界不可信 ⇒ 整个节区降级。
        let truncated = &OSZ_DL[..64];
        let lines = DwarfLines::from_sections(&[
            (DEBUG_LINE_SECTION.to_string(), truncated.to_vec()),
            (DEBUG_LINE_STR_SECTION.to_string(), OSZ_DLS.to_vec()),
        ])
        .expect("畸形输入**不是**加载失败");
        assert_eq!(lines.status(), DwarfStatus::Malformed);
        assert_eq!(lines.resolve(0x8000_0000), LineLookup::NoDwarf);
    }

    #[test]
    fn a_unit_length_out_of_bounds_degrades_without_panicking() {
        let mut bytes = OSZ_DL.to_vec();
        // 把第一个单元的 `unit_length` 改成天文数字 ⇒ 越界。
        bytes[0..4].copy_from_slice(&0xffff_0000u32.to_le_bytes());
        let lines = DwarfLines::from_sections(&[
            (DEBUG_LINE_SECTION.to_string(), bytes),
            (DEBUG_LINE_STR_SECTION.to_string(), OSZ_DLS.to_vec()),
        ])
        .expect("畸形输入**不是**加载失败");
        assert_eq!(lines.status(), DwarfStatus::Malformed);
        assert_eq!(lines.resolve(0x8000_0000), LineLookup::NoDwarf);
    }

    #[test]
    fn a_too_short_debug_line_degrades_without_panicking() {
        for len in 0..4usize {
            let bytes = vec![0xabu8; len];
            let lines = DwarfLines::from_sections(&v_sections(&bytes)).expect("降级");
            assert_eq!(lines.status(), DwarfStatus::Malformed, "长度 {len}");
            assert_eq!(lines.resolve(0x1000), LineLookup::NoDwarf);
        }
    }

    #[test]
    fn a_zero_unit_length_is_malformed() {
        let bytes = vec![0x00, 0x00, 0x00, 0x00, 0x04, 0x00];
        let lines = DwarfLines::from_sections(&v_sections(&bytes)).expect("降级");
        assert_eq!(lines.status(), DwarfStatus::Malformed);
    }

    /// 取 v2 夹具的**第一个**单元（只含它 ⇒ 单元级的断言干净）。
    fn first_v2_unit() -> Vec<u8> {
        let len = u32::from_le_bytes([V2_DL[0], V2_DL[1], V2_DL[2], V2_DL[3]]) as usize;
        V2_DL[..4 + len].to_vec()
    }

    #[test]
    fn an_unsupported_version_degrades_without_panicking() {
        // v6 = 支持集之外 ⇒ 明确降级，**版本如实报出**。
        let mut unit = first_v2_unit();
        unit[4..6].copy_from_slice(&6u16.to_le_bytes());
        let lines = DwarfLines::from_sections(&v_sections(&unit)).expect("降级");
        assert_eq!(lines.status(), DwarfStatus::UnsupportedVersion(6));
        assert_eq!(lines.version(), Some(6));
        assert_eq!(lines.resolve(0x1000), LineLookup::NoDwarf);
    }

    #[test]
    fn an_unknown_extended_opcode_degrades_the_unit() {
        // 造一个带**未知扩展操作码**（子操作码 9）的单元。
        let mut unit = state_machine_unit();
        // 最后一个字节是 `end_sequence` 的子操作码（1）⇒ 改成 9。
        let len = unit.len();
        unit[len - 1] = 9;
        let lines = DwarfLines::from_sections(&v_sections(&unit)).expect("降级");
        assert_eq!(lines.status(), DwarfStatus::Malformed);
        assert_eq!(lines.resolve(0x1000), LineLookup::NoDwarf);
    }

    // -----------------------------------------------------------------
    // A-12：`DW_FORM_strx*` ⇒ **整个单元**降级（不部分解析）。
    // -----------------------------------------------------------------

    /// 把每个单元**头部**里的 `DW_FORM_line_strp`（0x1f）换成
    /// `DW_FORM_strx1`（0x25）⇒ 不支持的 form。
    fn patch_forms_to_strx(bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        let mut off = 0usize;
        while off + 4 <= out.len() {
            let len =
                u32::from_le_bytes([out[off], out[off + 1], out[off + 2], out[off + 3]]) as usize;
            let unit_end = (off + 4 + len).min(out.len());
            // v5 的头部结束于 `12 + header_length`（DWARF32）。
            let header_len =
                u32::from_le_bytes([out[off + 8], out[off + 9], out[off + 10], out[off + 11]])
                    as usize;
            let header_end = (off + 12 + header_len).min(unit_end);
            for byte in out[off..header_end].iter_mut() {
                if *byte == 0x1f {
                    *byte = 0x25; // DW_FORM_strx1
                }
            }
            off = unit_end;
        }
        out
    }

    #[test]
    fn an_unsupported_form_degrades_every_unit() {
        let patched = patch_forms_to_strx(OSZ_DL);
        let lines = DwarfLines::from_sections(&[
            (DEBUG_LINE_SECTION.to_string(), patched),
            (DEBUG_LINE_STR_SECTION.to_string(), OSZ_DLS.to_vec()),
        ])
        .expect("降级");
        assert_eq!(
            lines.status(),
            DwarfStatus::Malformed,
            "不支持的 form ⇒ 整个节区降级（**绝不**部分解析）"
        );
        assert_eq!(lines.units_parsed(), 0);
        assert_eq!(lines.resolve(0x8000_0000), LineLookup::NoDwarf);
    }

    #[test]
    fn a_degraded_unit_contributes_no_rows_but_its_neighbours_still_work() {
        // 只降级**第一个**单元 ⇒ 后两个单元照常解析（行号表**不**是
        // 「一半对一半错」—— 但**没降级**的单元必须还是对的）。
        let mut patched = OSZ_DL.to_vec();
        let first_unit_len =
            u32::from_le_bytes([patched[0], patched[1], patched[2], patched[3]]) as usize;
        let header_len =
            u32::from_le_bytes([patched[8], patched[9], patched[10], patched[11]]) as usize;
        let header_end = 12 + header_len;
        for byte in patched[..header_end].iter_mut() {
            if *byte == 0x1f {
                *byte = 0x25;
            }
        }
        let lines = DwarfLines::from_sections(&[
            (DEBUG_LINE_SECTION.to_string(), patched),
            (DEBUG_LINE_STR_SECTION.to_string(), OSZ_DLS.to_vec()),
        ])
        .expect("降级");
        assert_eq!(lines.units_parsed(), 2, "后两个单元照常解析");
        assert_eq!(lines.units_degraded(), 1, "第一个单元降级");
        // 第一个单元的地址查不到（**没查** ⇒ `Missed`，不是「猜的行号」）。
        assert_eq!(lines.resolve(0x8000_0000), LineLookup::Missed);
        // 后面的单元还是对的。
        assert_eq!(found(&lines, 0x8000_0060).0, OSZ_INIT);
        let _ = first_unit_len;
    }

    // -----------------------------------------------------------------
    // A-11：v5 的 `DW_FORM_line_strp` **真的**从 `.debug_line_str` 取。
    // -----------------------------------------------------------------

    #[test]
    fn the_v5_paths_come_from_the_debug_line_str_section() {
        // 路径**只可能**来自 `.debug_line_str`（`.debug_line` 里存的是偏移）。
        let lines = DwarfLines::from_sections(&osz_sections()).expect("不得报错");
        let (file, _) = found(&lines, 0x8000_0000);
        assert_eq!(file, OSZ_HOOK);
        assert!(
            OSZ_DLS
                .windows(OSZ_HOOK.len())
                .any(|w| w == OSZ_HOOK.as_bytes()),
            "路径字符串必须在 `.debug_line_str` 节区里"
        );
    }

    #[test]
    fn a_missing_debug_line_str_degrades_instead_of_inventing_a_path() {
        // `.debug_line_str` 缺失 ⇒ `line_strp` 偏移越界 ⇒ **降级**，
        // ⛔ **绝不**给一个空路径或猜出来的路径。
        let lines = DwarfLines::from_sections(&[
            (DEBUG_LINE_SECTION.to_string(), OSZ_DL.to_vec()),
            (DEBUG_LINE_STR_SECTION.to_string(), Vec::new()),
        ])
        .expect("降级");
        assert_eq!(lines.status(), DwarfStatus::Malformed);
        assert_eq!(lines.resolve(0x8000_0000), LineLookup::NoDwarf);
    }

    // -----------------------------------------------------------------
    // 诚实性（裁决 J）：三态必须**如实区分**。
    // -----------------------------------------------------------------

    #[test]
    fn the_three_states_stay_distinguishable() {
        // 没有 `.debug_line` ⇒ `NoDwarf`
        let absent = DwarfLines::from_sections(&[]).expect("不得报错");
        assert_eq!(absent.status(), DwarfStatus::Absent);
        assert_eq!(absent.resolve(0x1000), LineLookup::NoDwarf);

        // 版本不支持 ⇒ `NoDwarf`（版本**如实报出**）
        let mut unit = first_v2_unit();
        unit[4..6].copy_from_slice(&9u16.to_le_bytes());
        let unsupported = DwarfLines::from_sections(&v_sections(&unit)).expect("降级");
        assert_eq!(unsupported.status(), DwarfStatus::UnsupportedVersion(9));
        assert_eq!(unsupported.resolve(0x1000), LineLookup::NoDwarf);

        // 畸形 ⇒ `NoDwarf`
        let mut bytes = OSZ_DL.to_vec();
        bytes[0..4].copy_from_slice(&0xffff_0000u32.to_le_bytes());
        let malformed = DwarfLines::from_sections(&[
            (DEBUG_LINE_SECTION.to_string(), bytes),
            (DEBUG_LINE_STR_SECTION.to_string(), OSZ_DLS.to_vec()),
        ])
        .expect("降级");
        assert_eq!(malformed.status(), DwarfStatus::Malformed);
        assert_eq!(malformed.resolve(0x1000), LineLookup::NoDwarf);

        // 解析成功 ⇒ `Found` / `Missed`（**不是** `NoDwarf`）
        let parsed = DwarfLines::from_sections(&osz_sections()).expect("不得报错");
        assert_eq!(parsed.status(), DwarfStatus::Supported(5));
        assert!(matches!(parsed.resolve(0x8000_0000), LineLookup::Found(_)));
        assert_eq!(parsed.resolve(0x0), LineLookup::Missed);
    }

    #[test]
    fn the_supported_version_set_is_v2_v3_v4_and_v5() {
        assert_eq!(SUPPORTED_VERSIONS, &[2, 3, 4, 5]);
        for v in [2u16, 3, 4, 5] {
            assert!(SUPPORTED_VERSIONS.contains(&v));
        }
        for v in [0u16, 1, 6, 7] {
            assert!(!SUPPORTED_VERSIONS.contains(&v), "v{v} 不该在支持集里");
        }
    }

    #[test]
    fn a_dwarf64_header_is_understood() {
        // DWARF64：`initial_length == 0xffff_ffff` ⇒ 长度字段占 12 字节
        // （标记 4 + 长度 8），`version` 在偏移 12。
        //
        // 做法：把一个**能解析**的 v4 单元的 32 位长度改成 64 位。
        let v4 = state_machine_unit();
        let body = &v4[4..]; // 长度字段之后的全部内容
        let mut unit: Vec<u8> = Vec::new();
        unit.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
        unit.extend_from_slice(&(body.len() as u64).to_le_bytes());
        unit.extend_from_slice(body);
        let lines = DwarfLines::from_sections(&v_sections(&unit)).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Supported(4));
        assert_eq!(lines.version(), Some(4));
        // 行记录**一样**（长度字段变了，别的没变）。
        assert_eq!(found(&lines, 0x1000), ("inc/a.c".to_string(), 1));
        assert_eq!(found(&lines, 0x1027), ("c.c".to_string(), 2));
    }

    #[test]
    fn a_truncated_dwarf64_header_is_malformed() {
        let mut unit: Vec<u8> = Vec::new();
        unit.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
        unit.extend_from_slice(&8u64.to_le_bytes());
        let lines = DwarfLines::from_sections(&v_sections(&unit)).expect("降级");
        assert_eq!(lines.status(), DwarfStatus::Malformed);
    }

    // -----------------------------------------------------------------
    // VA-01 / VA-02 / VA-03 的**合成夹具**。
    //
    // ⚠️ 为什么这三条用**合成**字节，而不用真工具链产物：它们要钉的是
    // 「行号程序**没有** `end_sequence` 收尾」「文件索引**越界**」「文件
    // 寄存器**初值**」这三件**语义** —— 真工具链**不会**产出前两种
    // （它总是 `end_sequence` 收尾，文件索引也不越界）⇒ 合成夹具是
    // **唯一**能覆盖它们的方式；真产物夹具（OSZ_DL / V2_DL …）负责
    // 覆盖「正常路径」，两者互补。
    // -----------------------------------------------------------------

    /// v2~v4 的行号程序头部：`min_inst_len = 1`、`line_base = -5`、
    /// `line_range = 14`、`opcode_base = 13`；目录表只有一项 `inc`；
    /// 文件表**索引 0 未用**、**索引 1** = `main.c`（目录索引 1 ⇒ 路径
    /// 拼成 `inc/main.c`）。
    fn v4_header_bytes() -> Vec<u8> {
        let mut header: Vec<u8> = vec![
            1,    // minimum_instruction_length
            1,    // maximum_operations_per_instruction（v4+）
            1,    // default_is_stmt
            0xfb, // line_base = -5
            0x0e, // line_range = 14
            0x0d, // opcode_base = 13
        ];
        header.extend_from_slice(&[0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1]); // standard_opcode_lengths
        header.extend_from_slice(b"inc\0"); // 目录 1
        header.push(0); // 目录表结束（空串）
        header.extend_from_slice(b"main.c\0"); // 文件 1
        header.extend_from_slice(&[1, 0, 0]); // dir=1, mtime=0, len=0
        header.push(0); // 文件表结束（空 name）
        header
    }

    /// 组装一个 **v4** 单元（`unit_length` + `version` + `header_length`
    /// + 头部 + 行号程序）。
    fn v4_unit(program: &[u8]) -> Vec<u8> {
        let header = v4_header_bytes();
        let mut unit: Vec<u8> = Vec::new();
        // `unit_length` 数的是**它自己之后**的字节数。
        let body_len = 2 + 4 + header.len() + program.len();
        unit.extend_from_slice(&(body_len as u32).to_le_bytes());
        unit.extend_from_slice(&4u16.to_le_bytes());
        unit.extend_from_slice(&(header.len() as u32).to_le_bytes());
        unit.extend_from_slice(&header);
        unit.extend_from_slice(program);
        unit
    }

    /// 行号程序：`set_address 0x1000` → `copy`（发一行）→ `advance_pc 4`
    /// → `end_sequence`。
    ///
    /// ⚠️ **为什么中间要 `advance_pc`**：`end_sequence` 用的就是**当前
    /// 地址**；若 `copy` 之后地址没动，`end_sequence` 的地址就等于那一行
    /// 的地址 ⇒ 那一行**落在本段之外**（`addr >= end_addr`）⇒ 查不到。
    fn copy_at_0x1000() -> Vec<u8> {
        let mut program: Vec<u8> = Vec::new();
        // DW_LNE_set_address（长度 = 1 + 4）
        program.push(0);
        program.push(5);
        program.push(2);
        program.extend_from_slice(&0x1000u32.to_le_bytes());
        program.push(1); // DW_LNS_copy
        program.push(2); // DW_LNS_advance_pc
        program.push(4); // 操作数 4

        // DW_LNE_end_sequence（长度 = 1）
        program.push(0);
        program.push(1);
        program.push(1);
        program
    }

    /// 组装一个 **v5** 单元：文件表**索引 0** = `v5-zero.c`、
    /// **索引 1** = `v5-one.c`（目录表为空 ⇒ 路径就是文件名）。
    ///
    /// ⚠️ v5 的头部布局与 v2~v4 **不同**：多出 `address_size` /
    /// `segment_selector_size`，而目录表 / 文件表改用
    /// `(content_type, form)` 格式描述。
    fn v5_default_file_unit() -> Vec<u8> {
        let mut header: Vec<u8> = vec![
            1,    // minimum_instruction_length
            1,    // maximum_operations_per_instruction
            1,    // default_is_stmt
            0xfb, // line_base = -5
            0x0e, // line_range = 14
            0x0d, // opcode_base = 13
        ];
        header.extend_from_slice(&[0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1]); // standard_opcode_lengths

        // 目录表：格式 0 项、数量 0 ⇒ 没有目录。
        header.push(0); // directory_entry_format_count
        header.push(0); // directories_count（ULEB）

        // 文件表：格式 2 项 —— (DW_LNCT_path, DW_FORM_string) 与
        // (DW_LNCT_directory_index, DW_FORM_udata)。
        header.push(2); // file_name_entry_format_count
        header.push(1); // DW_LNCT_path
        header.push(0x08); // DW_FORM_string
        header.push(2); // DW_LNCT_directory_index
        header.push(0x0f); // DW_FORM_udata
        header.push(2); // file_names_count（ULEB）
        header.extend_from_slice(b"v5-zero.c\0");
        header.push(0); // directory_index = 0
        header.extend_from_slice(b"v5-one.c\0");
        header.push(0); // directory_index = 0

        let program = copy_at_0x1000();

        let mut unit: Vec<u8> = Vec::new();
        // v5 的 `unit_length` 之后是 version(2) + address_size(1) +
        // segment_selector_size(1) + header_length(4) + 头部 + 程序。
        let body_len = 2 + 1 + 1 + 4 + header.len() + program.len();
        unit.extend_from_slice(&(body_len as u32).to_le_bytes());
        unit.extend_from_slice(&5u16.to_le_bytes());
        unit.push(4); // address_size
        unit.push(0); // segment_selector_size
        unit.extend_from_slice(&(header.len() as u32).to_le_bytes());
        unit.extend_from_slice(&header);
        unit.extend_from_slice(&program);
        unit
    }

    /// 行号程序：`set_address 0x1000` → `copy`（发一行）→ **没有**
    /// `end_sequence` 收尾。
    fn unterminated_program() -> Vec<u8> {
        let mut program: Vec<u8> = Vec::new();
        // DW_LNE_set_address（长度 = 1 + 4）
        program.push(0);
        program.push(5);
        program.push(2);
        program.extend_from_slice(&0x1000u32.to_le_bytes());
        program.push(1); // DW_LNS_copy
        program
    }

    /// 行号程序：`set_address 0x1000` → `set_file 9`（**越界**）→
    /// `copy` → `advance_pc 4` → `end_sequence`。
    fn out_of_range_file_program() -> Vec<u8> {
        let mut program: Vec<u8> = Vec::new();
        // DW_LNE_set_address（长度 = 1 + 4）
        program.push(0);
        program.push(5);
        program.push(2);
        program.extend_from_slice(&0x1000u32.to_le_bytes());
        program.push(4); // DW_LNS_set_file
        program.push(9); // 文件索引 9（文件表只有第 1 项 ⇒ **越界**）
        program.push(1); // DW_LNS_copy
        program.push(2); // DW_LNS_advance_pc
        program.push(4); // 操作数 4
                         // DW_LNE_end_sequence（长度 = 1）
        program.push(0);
        program.push(1);
        program.push(1);
        program
    }

    // -----------------------------------------------------------------
    // VA-01：没有 `end_sequence` 收尾的段**不得**覆盖到地址空间的末尾。
    // -----------------------------------------------------------------

    #[test]
    fn a_sequence_without_end_sequence_does_not_cover_the_whole_address_space() {
        let lines = DwarfLines::from_sections(&v_sections(&v4_unit(&unterminated_program())))
            .expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Supported(4));
        assert_eq!(lines.units_parsed(), 1);
        assert_eq!(lines.units_degraded(), 0);
        // 真的那一行**照常**查得到（不猜，也不把真值丢掉）。
        assert_eq!(found(&lines, 0x1000), ("inc/main.c".to_string(), 1));
        // ⛔ 但这一段**没有** `end_sequence` 收尾 ⇒ 它的**结束地址未知**
        // ⇒ 远处的地址**不得**被报成命中（那是「自信但错误」的源码行）。
        assert_eq!(
            lines.resolve(0x9999_9999),
            LineLookup::Missed,
            "没收尾的段**不得**覆盖到地址空间的末尾"
        );
        // 紧挨着最后一行之后的地址同样查不到（这一段只到「最后一行 + 1」）。
        assert_eq!(lines.resolve(0x1001), LineLookup::Missed);
    }

    // -----------------------------------------------------------------
    // VA-02：文件名为空 ⇒ **不是** `Found{file:""}`（契约层不许有第三态）。
    // -----------------------------------------------------------------

    #[test]
    fn a_row_without_a_file_name_is_a_miss_not_an_empty_file() {
        let lines = DwarfLines::from_sections(&v_sections(&v4_unit(&out_of_range_file_program())))
            .expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Supported(4));
        // 夹具前提：那一行**确实**没有文件名（文件索引越界）。
        let rows = lines.raw_rows();
        assert_eq!(rows.len(), 1, "这一段只发了一行");
        assert!(
            rows[0].file.is_empty(),
            "夹具前提：文件索引越界 ⇒ 这一行**没有**文件名"
        );
        assert_eq!(rows[0].line, 1, "行号**是真的**（问题只在文件名）");
        // ⛔ 契约层只许两种「没有答案」：`Missed`（line = u32::MAX）与
        // `NoDwarf`（line = 0）⇒ **不许**出现「空文件名 + 真行号」的
        // **第三态**。
        assert_eq!(
            lines.resolve(0x1000),
            LineLookup::Missed,
            "文件名取不到 ⇒ `Missed`（**不是**「空文件名 + 真行号」那种第三态）"
        );
    }

    // -----------------------------------------------------------------
    // VA-03：v5 与 v2~v4 的「文件索引基址」不同（0 vs 1）—— 零护栏。
    // -----------------------------------------------------------------

    #[test]
    fn the_v5_default_file_register_is_index_zero() {
        // DWARF 5：行号程序的 `file` 寄存器**初值是 0** ⇒ 首行（**不**
        // 显式 `set_file`）属于**文件表第 0 项**。
        //
        // ⚠️ 这一条以前**零护栏**：把 v5 的 `initial_file` 从 0 改成 1，
        // 全仓**一条用例都不红**（首行会变成文件表第 1 项 ⇒ 路径错了，
        // 但没人钉它）。
        let lines =
            DwarfLines::from_sections(&v_sections(&v5_default_file_unit())).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Supported(5));
        assert_eq!(lines.units_parsed(), 1);
        assert_eq!(
            found(&lines, 0x1000),
            ("v5-zero.c".to_string(), 1),
            "v5 的首行必须落在文件表第 **0** 项（不是第 1 项）"
        );
    }

    #[test]
    fn the_v2_v4_default_file_register_is_index_one() {
        // DWARF 2/3/4：行号程序的 `file` 寄存器**初值是 1**（索引 0 未用、
        // 是主源文件的占位）⇒ 首行（**不**显式 `set_file`）属于**文件表
        // 第 1 项**。
        //
        // ⚠️ 这一条以前**零护栏**：把 v2~v4 的 `initial_file` 从 1 改成 0，
        // 首行会指向**未用的索引 0** ⇒ 路径取不到，同样没人钉它。
        let lines =
            DwarfLines::from_sections(&v_sections(&v4_unit(&copy_at_0x1000()))).expect("不得报错");
        assert_eq!(lines.status(), DwarfStatus::Supported(4));
        assert_eq!(lines.units_parsed(), 1);
        assert_eq!(
            found(&lines, 0x1000),
            ("inc/main.c".to_string(), 1),
            "v2~v4 的首行必须落在文件表第 **1** 项（不是第 0 项）"
        );
    }
}
