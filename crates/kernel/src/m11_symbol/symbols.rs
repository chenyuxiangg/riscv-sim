//! 符号表 —— 解析 M-06 带出的 `.symtab` + `.strtab`，提供名称 / 地址
//! 双向查询。
//!
//! 落地规格：`docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`（冻结）
//! §2 裁决 B · §5.2 验收 #6 / #7 / #8。
//!
//! ## 数据从哪来
//!
//! M-06 是**唯一的 ELF 容器解析者**（裁决 B）：它在解析节区表时按白名单
//! 抽出 `.symtab` / `.strtab` 等调试节区的**原始字节**，放进
//! [`crate::m06_elf::LoadImage::debug_sections`]。
//!
//! ⇒ 本模块**不再解析 ELF 容器**（那会再造一个「第二套 ELF 解析器」），
//! 只解析**符号表语义**：`.symtab` 的条目结构 + `.strtab` 的名字串。
//!
//! ## 符号条目的形状（ELF32）
//!
//! `Elf32_Sym` 固定 **16 字节**：
//!
//! | 字段 | 偏移 | 宽度 | 含义 |
//! | --- | --- | --- | --- |
//! | `st_name` | 0 | u32 | `.strtab` 里的名字偏移 |
//! | `st_value` | 4 | u32 | 符号地址（或 `SHN_ABS` 的绝对值） |
//! | `st_size` | 8 | u32 | 符号大小（字节） |
//! | `st_info` | 12 | u8 | 高 4 位 = 类型，低 4 位 = 绑定 |
//! | `st_other` | 13 | u8 | 可见性等 |
//! | `st_shndx` | 14 | u16 | 所属节区下标 |
//!
//! 名字从 `.strtab` 的 `st_name` 偏移处读，**以 NUL 结束**。
//!
//! ## 查询语义
//!
//! * [`SymbolTable::lookup`] —— 按**名字**查（未知名 ⇒ `None`，**不是**
//!   错误）。
//! * [`SymbolTable::at`] —— 按**地址**查：命中「**起始地址 ≤ addr 且是最近
//!   的那个**」符号（符号表按地址排序 ⇒ 二分查找）。
//!
//!   ⚠️ 这里刻意**不**按 `size` 判范围：契约层的 `Symbol::size` 是
//!   「大小信息」（很多符号是 0 / 未知），拿它做区间判断会让「落在两个
//!   符号之间的地址」变成查不到——而「最近的、起始地址 ≤ addr」这条规则
//!   总能给出一个**确定**的答案。
//!
//! ## 畸形输入的处置：**整体 `Err`**
//!
//! 节区字节被截断 / 偏移越界 / 名字串没有 NUL 终止 ⇒ **整体报错**，
//! **不**「跳过这一条继续」。理由：跳过会让调用方拿到一个**看起来能用
//! 其实少了一截**的符号表，而「符号表不完整」在调试器里比「符号表读不
//! 出来」更容易误导人。

use std::collections::HashMap;

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::Symbol;

use crate::m06_elf::ParseLimits;

/// 本模块的错误族：**Loader**（与 M-06 同族——「ELF / DWARF / 对象解析」
/// 在契约层的族表里就是 `Loader` 覆盖 M-06 与 M-11）。
pub const SYMBOL_FAMILY: ErrorFamily = ErrorFamily::Loader;

/// 场景码：**畸形输入**（截断 / 偏移越界 / 名字串无 NUL 终止）。
pub const E_LDR_01_MALFORMED: u16 = 1;

/// 场景码：**超过解析上限**（条目数 > `ParseLimits::symbol_count`）。
pub const E_LDR_02_PARSE_LIMIT: u16 = 2;

/// `.symtab` 里一个条目的大小（ELF32 `Elf32_Sym` = 16 字节）。
const SYM_ENTRY_SIZE: usize = 16;

/// 符号类型（`st_info` 的高 4 位）。
const STT_NOTYPE: u8 = 0;
const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;

/// `SHN_UNDEF` —— 符号**没有地址**（未定义 / 外部符号）。
const SHN_UNDEF: u16 = 0;

/// 符号表：名称 → 符号、地址 → 符号。
///
/// 构造： [`SymbolTable::from_sections`]（用默认上限）或
/// [`SymbolTable::from_sections_with_limits`]（显式上限）。
#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    /// 名字 → 符号。**同名取先出现的那条**（`.symtab` 的原始顺序：
    /// 局部符号在前、全局符号在后）。
    by_name: HashMap<String, Symbol>,
    /// 按地址排序的 `(地址, 符号)`（**只有有地址的符号**）。
    by_addr: Vec<(u32, Symbol)>,
}

impl SymbolTable {
    /// 从调试节区里解析符号表（用 `ParseLimits::default().symbol_count`
    /// 作上限）。
    ///
    /// * 没有 `.symtab` ⇒ **空表**（不是错误）：验收 #3「无调试信息也能
    ///   独立使用」。
    /// * 有 `.symtab` 但没有 `.strtab` ⇒ **畸形**（`E_LDR-01`）。
    /// * 条目数超上限 ⇒ **拒绝**（`E_LDR-02`），**不**截断后静默少给
    ///   （验收 #8）。
    pub fn from_sections(sections: &[(String, Vec<u8>)]) -> Result<Self, PlatformError> {
        Self::from_sections_with_limits(sections, ParseLimits::default().symbol_count)
    }

    /// 从调试节区里解析符号表，显式给出**符号条数上限**。
    ///
    /// 上限是**条目数**（`.symtab` 里的 `Elf32_Sym` 个数，含被过滤掉的
    /// 那些）——它是「解析工作量」的界，不是「给出去多少条」的界。
    pub fn from_sections_with_limits(
        sections: &[(String, Vec<u8>)],
        max_symbols: u32,
    ) -> Result<Self, PlatformError> {
        let symtab = find_section(sections, ".symtab");
        let strtab = find_section(sections, ".strtab");

        let Some(symtab) = symtab else {
            // 没有符号表 ⇒ 空表。**不是错误**：一个没有调试信息的 ELF
            // 依然要能反汇编（验收 #3）。
            return Ok(SymbolTable::default());
        };
        let Some(strtab) = strtab else {
            return Err(malformed(".symtab present but .strtab is missing"));
        };
        if symtab.len() % SYM_ENTRY_SIZE != 0 {
            return Err(malformed(format!(
                ".symtab length {} is not a multiple of {SYM_ENTRY_SIZE}",
                symtab.len()
            )));
        }
        let count = symtab.len() / SYM_ENTRY_SIZE;
        if count > max_symbols as usize {
            return Err(PlatformError::new(
                SYMBOL_FAMILY,
                E_LDR_02_PARSE_LIMIT,
                format!("symbol table has {count} entries, limit is {max_symbols}"),
            ));
        }

        let mut by_name: HashMap<String, Symbol> = HashMap::new();
        let mut by_addr: Vec<(u32, Symbol)> = Vec::new();

        for index in 0..count {
            let entry = &symtab[index * SYM_ENTRY_SIZE..(index + 1) * SYM_ENTRY_SIZE];
            let st_name = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
            let st_value = u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]);
            let st_size = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]);
            let st_info = entry[12];
            let st_shndx = u16::from_le_bytes([entry[14], entry[15]]);

            // 只保留「函数 / 对象 / 无类型」三类。文件符号（STT_FILE=4）
            // 与节区符号（STT_SECTION=3）不是**可定位的**符号：把它们
            // 放进地址索引，会让「节区起点」 masquerade 成一个函数。
            let sym_type = st_info >> 4;
            if sym_type != STT_NOTYPE && sym_type != STT_OBJECT && sym_type != STT_FUNC {
                continue;
            }

            let name = read_cstr(strtab, st_name)?;
            if name.is_empty() {
                // 空名字 = `.symtab` 的第 0 号条目（保留的空符号）。
                continue;
            }

            let symbol = Symbol {
                name: name.clone(),
                // `SHN_UNDEF` ⇒ 未定义符号 ⇒ **没有地址**。
                addr: if st_shndx == SHN_UNDEF {
                    None
                } else {
                    Some(st_value)
                },
                // 大小 0 ⇒ 「没有大小信息」（不是「大小为零」）。
                size: if st_size == 0 { None } else { Some(st_size) },
            };

            // **只有有地址的符号**进地址索引：未定义符号的 `st_value`
            // 是 0（不是地址）⇒ 放进去会让 `at(0)` 命中一个外部符号。
            if symbol.addr.is_some() {
                by_addr.push((st_value, symbol.clone()));
            }
            // 同名取先出现的那条。
            by_name.entry(name).or_insert(symbol);
        }

        by_addr.sort_by_key(|(addr, _)| *addr);
        Ok(SymbolTable { by_name, by_addr })
    }

    /// 按**名字**查符号。
    ///
    /// 未知名 ⇒ `None`（**不是错误**，验收 #7）。
    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        self.by_name.get(name)
    }

    /// 按**地址**查符号：命中「起始地址 ≤ `addr` 且**最近**的那个」。
    ///
    /// 比所有符号的起始地址都小 ⇒ `None`。
    ///
    /// ⚠️ **同地址的并列**：`nm -n` 会把同地址的符号都列出来，而这里只
    /// 给一个 ⇒ 取「`.symtab` 里**先出现**的那条」（排序是稳定的 ⇒ 并列
    /// 组里的第一条就是先出现的那条）。
    pub fn at(&self, addr: u32) -> Option<&Symbol> {
        // 「起始地址 < addr」的条目数 ⇒ 候选在它后面（正好等于 addr）或
        // 它前面（起始地址更小的最近一个）。
        let index = self.by_addr.partition_point(|(start, _)| *start < addr);
        if index < self.by_addr.len() && self.by_addr[index].0 == addr {
            // 正好落在某个符号的起点上 ⇒ `index` 就是该地址组的**第一条**
            // （`partition_point` 停在第一个 ≥ addr 的条目上）。
            return Some(&self.by_addr[index].1);
        }
        if index == 0 {
            // 所有符号的起始地址都 ≥ addr ⇒ 没有「≤ addr 且最近」的。
            return None;
        }
        // 落在两个符号之间 ⇒ 取起始地址最大的那个；**同地址并列时**取该
        // 组里的**第一条**（与上面「正好命中」那一条保持同一条规则）。
        let group_addr = self.by_addr[index - 1].0;
        let group_start = self
            .by_addr
            .partition_point(|(start, _)| *start < group_addr);
        Some(&self.by_addr[group_start].1)
    }

    /// 符号条数（**有名字的**那些）。
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// 符号表为空？
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

/// 在节区列表里找**第一个**同名节区。
fn find_section<'a>(sections: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
    sections
        .iter()
        .find(|(section_name, _)| section_name == name)
        .map(|(_, bytes)| bytes.as_slice())
}

/// 读 `.strtab` 里 `offset` 处的 **NUL 结束**字符串。
///
/// * 偏移越界 ⇒ 畸形（`E_LDR-01`）。
/// * 到表尾都没有 NUL ⇒ 畸形（`E_LDR-01`）。
/// * 名字不是合法 UTF-8 ⇒ **按有损转换**处理（符号名在实践中是 ASCII；
///   为一个坏字节拒绝整张符号表不划算）。
fn read_cstr(buf: &[u8], offset: u32) -> Result<String, PlatformError> {
    let start = offset as usize;
    if start >= buf.len() {
        return Err(malformed(format!(
            "symbol name offset {offset} is out of range (strtab is {} bytes)",
            buf.len()
        )));
    }
    let rest = &buf[start..];
    let end = rest.iter().position(|byte| *byte == 0).ok_or_else(|| {
        malformed(format!(
            "symbol name at offset {offset} is not NUL-terminated"
        ))
    })?;
    Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
}

/// 造一个「畸形输入」错误。
fn malformed(message: impl Into<String>) -> PlatformError {
    PlatformError::new(SYMBOL_FAMILY, E_LDR_01_MALFORMED, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个 `.strtab`（名字之间用 NUL 隔开，末尾也补 NUL）。
    ///
    /// ⚠️ 真实的 `.strtab` 的**第 0 个字节就是 NUL** ⇒ 偏移 0 = 空串
    /// （`.symtab` 第 0 号保留条目的 `st_name` 就是 0）。这里照抄这个
    /// 约定，否则「保留的空符号」会拿到第一个真名字。
    fn strtab(names: &[&str]) -> Vec<u8> {
        let mut out = vec![0u8];
        for name in names {
            out.extend_from_slice(name.as_bytes());
            out.push(0);
        }
        out
    }

    /// 造一个 `.symtab` 条目。
    fn sym_entry(
        st_name: u32,
        st_value: u32,
        st_size: u32,
        sym_type: u8,
        st_shndx: u16,
    ) -> [u8; 16] {
        let mut entry = [0u8; 16];
        entry[0..4].copy_from_slice(&st_name.to_le_bytes());
        entry[4..8].copy_from_slice(&st_value.to_le_bytes());
        entry[8..12].copy_from_slice(&st_size.to_le_bytes());
        entry[12] = sym_type << 4; // st_info：高 4 位 = 类型
        entry[13] = 0; // st_other
        entry[14..16].copy_from_slice(&st_shndx.to_le_bytes());
        entry
    }

    /// 一张「像样」的符号表：`main` @0x1000、`helper` @0x1010、`data`
    /// @0x2000、`ext`（未定义 ⇒ 无地址）、`alias` @0x1010（与 `helper`
    /// 同地址，用来钉「同地址时取哪一条」）。
    fn sample_sections() -> Vec<(String, Vec<u8>)> {
        let strings = strtab(&["main", "helper", "data", "ext", "alias"]);
        // 名字偏移（`.strtab` 的 0 号字节是 NUL ⇒ 第一个名字在 1）：
        // main=1, helper=6, data=13, ext=18, alias=22
        let mut symtab = Vec::new();
        symtab.extend_from_slice(&sym_entry(0, 0, 0, STT_NOTYPE, 0)); // 保留的空符号
        symtab.extend_from_slice(&sym_entry(1, 0x1000, 16, STT_FUNC, 1)); // main
        symtab.extend_from_slice(&sym_entry(6, 0x1010, 32, STT_FUNC, 1)); // helper
        symtab.extend_from_slice(&sym_entry(13, 0x2000, 4, STT_OBJECT, 2)); // data
        symtab.extend_from_slice(&sym_entry(18, 0, 0, STT_NOTYPE, SHN_UNDEF)); // ext
        symtab.extend_from_slice(&sym_entry(22, 0x1010, 8, STT_FUNC, 1)); // alias
        vec![
            (".symtab".to_string(), symtab),
            (".strtab".to_string(), strings),
        ]
    }

    #[test]
    fn lookup_finds_symbols_by_name() {
        let table = SymbolTable::from_sections(&sample_sections()).expect("parse");
        let main = table.lookup("main").expect("main must be found");
        assert_eq!(main.addr, Some(0x1000));
        assert_eq!(main.size, Some(16));
        let helper = table.lookup("helper").expect("helper must be found");
        assert_eq!(helper.addr, Some(0x1010));
        assert_eq!(helper.size, Some(32));
        let data = table.lookup("data").expect("data must be found");
        assert_eq!(data.addr, Some(0x2000));
        // 大小为 0 ⇒ 「没有大小信息」。
        let ext = table.lookup("ext").expect("ext must be found");
        assert_eq!(ext.addr, None, "SHN_UNDEF symbols have no address");
        assert_eq!(ext.size, None);
    }

    #[test]
    fn lookup_of_unknown_name_is_none_not_an_error() {
        let table = SymbolTable::from_sections(&sample_sections()).expect("parse");
        assert!(table.lookup("nope").is_none());
    }

    #[test]
    fn at_finds_the_nearest_symbol_at_or_below_the_address() {
        let table = SymbolTable::from_sections(&sample_sections()).expect("parse");
        // 正好落在符号起点上。
        assert_eq!(table.at(0x1000).map(|s| s.name.as_str()), Some("main"));
        // 落在 main 的范围里（0x1000..0x1010）⇒ 仍是 main。
        assert_eq!(table.at(0x1008).map(|s| s.name.as_str()), Some("main"));
        // 正好落在 helper 的起点上。
        assert_eq!(table.at(0x1010).map(|s| s.name.as_str()), Some("helper"));
        // 落在 helper 与 data 之间 ⇒ **最近的、起始地址 ≤ addr** 那个。
        assert_eq!(table.at(0x1FFF).map(|s| s.name.as_str()), Some("helper"));
        // 比所有符号都靠前 ⇒ 查不到。
        assert!(table.at(0x0FFF).is_none());
        // 未定义符号（无地址）不参与地址索引。
        assert!(table.at(0x0).is_none());
    }

    #[test]
    fn a_table_without_symtab_is_empty_not_an_error() {
        let sections = vec![(".strtab".to_string(), strtab(&["x"]))];
        let table = SymbolTable::from_sections(&sections).expect("parse");
        assert!(table.is_empty());
        assert!(table.lookup("x").is_none());
        assert!(table.at(0x1000).is_none());
    }

    #[test]
    fn symtab_without_strtab_is_malformed() {
        let sections = vec![(".symtab".to_string(), vec![0u8; 16])];
        let err = SymbolTable::from_sections(&sections).expect_err("must be rejected");
        assert_eq!(err.family, SYMBOL_FAMILY);
        assert_eq!(err.scenario, E_LDR_01_MALFORMED);
    }

    #[test]
    fn a_truncated_symtab_is_malformed() {
        let mut sections = sample_sections();
        sections[0].1.pop(); // 少一个字节 ⇒ 不再是 16 的整数倍
        let err = SymbolTable::from_sections(&sections).expect_err("must be rejected");
        assert_eq!(err.family, SYMBOL_FAMILY);
        assert_eq!(err.scenario, E_LDR_01_MALFORMED);
    }

    #[test]
    fn a_name_offset_out_of_range_is_malformed() {
        let mut sections = sample_sections();
        // 把 `main` 的名字偏移改成越界值。
        sections[0].1[0..4].copy_from_slice(&9999u32.to_le_bytes());
        let err = SymbolTable::from_sections(&sections).expect_err("must be rejected");
        assert_eq!(err.family, SYMBOL_FAMILY);
        assert_eq!(err.scenario, E_LDR_01_MALFORMED);
    }

    #[test]
    fn a_name_without_nul_terminator_is_malformed() {
        let mut sections = sample_sections();
        // `.strtab` 的最后一个字节的 NUL 去掉 ⇒ 最后一个名字没有终止符。
        let last = sections[1].1.len() - 1;
        sections[1].1[last] = b'X';
        let err = SymbolTable::from_sections(&sections).expect_err("must be rejected");
        assert_eq!(err.family, SYMBOL_FAMILY);
        assert_eq!(err.scenario, E_LDR_01_MALFORMED);
    }

    #[test]
    fn too_many_symbols_are_rejected_not_truncated() {
        let sections = sample_sections();
        // 上限 2 ⇒ 表里有 5 条 ⇒ **拒绝**（不是截断后静默少给）。
        let err =
            SymbolTable::from_sections_with_limits(&sections, 2).expect_err("must be rejected");
        assert_eq!(err.family, SYMBOL_FAMILY);
        assert_eq!(err.scenario, E_LDR_02_PARSE_LIMIT);
        // 上限刚好够 ⇒ 通过，且**一条不少**。
        let table = SymbolTable::from_sections_with_limits(&sections, 6).expect("parse");
        assert_eq!(table.len(), 5);
        assert!(table.lookup("alias").is_some());
    }

    #[test]
    fn section_and_file_symbols_are_not_indexed_by_address() {
        let strings = strtab(&["main", ".text"]);
        let mut symtab = Vec::new();
        symtab.extend_from_slice(&sym_entry(0, 0, 0, STT_NOTYPE, 0));
        symtab.extend_from_slice(&sym_entry(1, 0x1000, 16, STT_FUNC, 1)); // "main"
                                                                          // 节区符号（STT_SECTION = 3）⇒ 不进表。
        symtab.extend_from_slice(&sym_entry(6, 0x1000, 0, 3, 1));
        let sections = vec![
            (".symtab".to_string(), symtab),
            (".strtab".to_string(), strings),
        ];
        let table = SymbolTable::from_sections(&sections).expect("parse");
        assert_eq!(table.len(), 1);
        assert!(table.lookup(".text").is_none());
        assert_eq!(table.at(0x1000).map(|s| s.name.as_str()), Some("main"));
    }
}
