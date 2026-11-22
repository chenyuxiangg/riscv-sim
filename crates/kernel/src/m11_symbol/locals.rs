//! **当前帧局部变量**（STAGE-55 · 批 D · 档 4）。
//!
//! ## 它回答什么问题
//!
//! 条件表达式里写一个**局部变量名**（`x == 42`）⇒ ⇒ 要变成「读
//! `<帧基址 + 偏移>` 这个字」。名字 → 位置这件事**只有 DWARF 知道**：
//!
//! * `DW_TAG_subprogram` 给出函数的地址区间（`DW_AT_low_pc` /
//!   `DW_AT_high_pc`）与**帧基址**（`DW_AT_frame_base`）；
//! * `DW_TAG_variable` / `DW_TAG_formal_parameter` 给出变量的**位置**
//!   （`DW_AT_location`）：
//!   * `DW_OP_fbreg(off)` ⇒ 在栈上：地址 = 帧基址 + off；
//!   * `DW_OP_regN` ⇒ 就在寄存器 `N` 里（**没有**地址，取指前就能求值）。
//!
//! ## 「当前帧」的口径（用户拍板 · 2026-11-22）
//!
//! **一律当整帧可见**：只要 `pc` 落在某个函数的 `[low_pc, high_pc)` 里，
//! 这个函数的局部变量就**全部**可见——不管它写在函数的序言、尾声还是
//! 中间。⇒ ⇒ 本模块**不看** `DW_AT_location` 的区间列表（`loclist`），
//! 只看「单点位置」（`exprloc`）。
//!
//! ⚠️ 这条口径是**刻意的简化**：编译器给 `-O0` 之外的产物写区间列表时，
//! 本模块会**跳过一个变量**（而不是编一个位置出来）⇒ ⇒ 那个名字在表达式里
//! 就报「未定义」。**不猜**。
//!
//! ## 帧基址：`DW_OP_call_frame_cfa` 与 `.debug_frame`
//!
//! GCC 在 RISC-V 上默认给 `DW_AT_frame_base = DW_OP_call_frame_cfa`（不是
//! 某个寄存器）⇒ ⇒ 局部变量的地址 = `CFA + fbreg 的偏移`，而 CFA 是多少
//! 由 `.debug_frame` 的 CFI **按 pc 分段**给出（见 [`super::cfi`]）。
//!
//! 批 D 曾把这条信息换成显式假设「`CFA == s0`」；CFI 批把它换成**真的
//! 读表**：构建期对每个函数取**主行**（覆盖地址最多的那一行）的 CFA
//! 规则 ⇒ ⇒ `CFA = 寄存器[r] + cfa_offset` ⇒ ⇒ 变量的栈位置直接落成
//! 「`寄存器[r] + (cfa_offset + fbreg 偏移)`」。⇒ ⇒ 产物是 `sp` 基址
//! （`-O2` / 无帧指针）时也**算得对**，不再是运气。
//!
//! ⚠️ **读不到 CFI 就不猜**（用户 2026-11-22 拍板）：函数区间照收
//! （「所在函数」照样能显示），但该帧的**栈上变量丢掉** ⇒ ⇒ 表达式里
//! 写那个名字会报「未定义」，而不是拿一条没根据的规则静默读错。

use std::collections::BTreeMap;

use super::cfi::CfiTable;
use super::dwarf_info::{walk_dies, DieAttrs};

const DW_TAG_SUBPROGRAM: u64 = 0x2e;
const DW_TAG_VARIABLE: u64 = 0x34;
const DW_TAG_FORMAL_PARAMETER: u64 = 0x05;
const DW_TAG_LEXICAL_BLOCK: u64 = 0x0b;

const DW_AT_NAME: u64 = 0x03;
const DW_AT_LINKAGE_NAME: u64 = 0x6e;
const DW_AT_LOCATION: u64 = 0x02;
const DW_AT_LOW_PC: u64 = 0x11;
const DW_AT_HIGH_PC: u64 = 0x12;
const DW_AT_FRAME_BASE: u64 = 0x40;
const DW_AT_DECLARATION: u64 = 0x3c;

/// `DW_AT_high_pc` 用这个 form ⇒ 它是**绝对地址**；其余 form 都是长度。
const DW_FORM_ADDR: u64 = 0x01;

const DW_OP_REG0: u8 = 0x50;
const DW_OP_REG31: u8 = 0x6f;
const DW_OP_FBREG: u8 = 0x91;
const DW_OP_CALL_FRAME_CFA: u8 = 0x9c;

/// **帧基址**是怎么算出来的（DWARF 里的原始说法）。
///
/// ⚠️ 只有 [`FrameBase::Cfa`] 需要 CFI 才能落地 ⇒ ⇒ 构建期就把它解析成
/// 具体寄存器 + 偏移（见 [`LocalTable::from_sections`]）；出来的
/// [`LocalLocation`] 里**不再有** `Cfa`，M-08 也就没有「帧指针」这个假设。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameBase {
    /// 某个通用寄存器里放着帧基址（`DW_OP_regN`）。
    Reg(u8),
    /// `DW_OP_call_frame_cfa` ⇒ ⇒ 构建期按 `.debug_frame` 的主行解析。
    Cfa,
}

/// 一个局部变量的**位置**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalLocation {
    /// 值就在这个寄存器里（`DW_OP_regN`）⇒ ⇒ **不需要**读内存。
    Reg(u8),
    /// 在栈上：地址 = `寄存器[base] + offset`（`DW_OP_fbreg`，其中
    /// `base` / `offset` 已经吸收了 CFA 规则）。
    Frame { base: u8, offset: i32 },
}

/// 一个局部变量（或形参）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVar {
    /// 源程序里的名字（`DW_AT_name`；没有则用 `DW_AT_linkage_name`）。
    pub name: String,
    /// 它放在哪儿。
    pub location: LocalLocation,
}

/// 一个函数（`DW_TAG_subprogram`）的局部变量范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalScope {
    /// `DW_AT_low_pc`。
    pub low_pc: u32,
    /// `DW_AT_high_pc`（**已经**换算成「结束地址」，不是长度）。
    pub high_pc: u32,
    /// 这个函数的帧基址。
    pub frame_base: FrameBase,
    /// 函数名（只用于报错时点名，比如「`x` 在 `main` / `helper` 里都有」）。
    pub name: String,
    /// 它的局部变量 / 形参（含词法块里的，**不**含其它函数里的）。
    pub vars: Vec<LocalVar>,
}

/// **按名字**在一次解析里的答案（三态，缺一不可）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalLookup {
    /// 只有一个函数有这个变量 ⇒ ⇒ 位置定了。
    Found(LocalLocation),
    /// 没有任何函数有这个变量。
    Missing,
    /// **多个函数都有同名变量** ⇒ ⇒ 光凭名字分不出来，必须在报错里点名。
    Ambiguous(Vec<String>),
}

/// 一个镜像里**所有**函数的局部变量表。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalTable {
    scopes: Vec<LocalScope>,
}

impl LocalTable {
    /// 从 M-06 带出的调试节区构建。
    ///
    /// ⚠️ **没有 `.debug_info` / `.debug_abbrev`** ⇒ 空表（不是错误）：
    /// 裸机镜像照样能下地址断点，只是局部变量名解不出来。
    pub fn from_sections(sections: &[(String, Vec<u8>)]) -> Self {
        let Some(info) = section(sections, ".debug_info") else {
            return Self::default();
        };
        let Some(abbrev) = section(sections, ".debug_abbrev") else {
            return Self::default();
        };
        let strings = section(sections, ".debug_str").unwrap_or(&[]);
        let line_strings = section(sections, ".debug_line_str").unwrap_or(&[]);
        // ⚠️ CFI 是可选的：没有它的时候**函数区间照收、变量丢掉**
        //    （用户拍板「保留区间、丢变量」）。
        let cfi = CfiTable::from_sections(sections);

        let mut builder = Builder::new(&cfi);
        walk_dies(
            info,
            abbrev,
            strings,
            line_strings,
            &mut |depth, tag, attrs| {
                builder.on_die(depth, tag, attrs);
            },
        );
        builder.finish()
    }

    /// 有没有解析到任何函数。
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }

    /// 解析到几个函数。
    pub fn scope_count(&self) -> usize {
        self.scopes.len()
    }

    /// `pc` 落在哪个函数里（**最内层**的赢 ⇒ 区间小的优先）。
    pub fn scope_at(&self, pc: u32) -> Option<&LocalScope> {
        self.scopes
            .iter()
            .filter(|scope| pc >= scope.low_pc && pc < scope.high_pc)
            .min_by_key(|scope| scope.high_pc.saturating_sub(scope.low_pc))
    }

    /// 在 **`pc` 所在的那个函数**里找 `name`。
    pub fn lookup_at(&self, name: &str, pc: u32) -> Option<LocalLocation> {
        let scope = self.scope_at(pc)?;
        scope
            .vars
            .iter()
            .find(|var| var.name == name)
            .map(|var| var.location)
    }

    /// **只凭名字**找 `name`（不知道 `pc` 时用，比如注册一条无地址的条件
    /// 断点）。
    ///
    /// ⚠️ 同名变量出现在两个函数里 ⇒ [`LocalLookup::Ambiguous`]（**不**
    /// 随手挑一个：挑错了就是「看着停在那儿、其实读的是别人的变量」）。
    pub fn lookup_any(&self, name: &str) -> LocalLookup {
        let mut hits: BTreeMap<(u8, i32), Vec<String>> = BTreeMap::new();
        let mut reg_hits: Vec<(u8, Vec<String>)> = Vec::new();
        let mut functions = Vec::new();
        for scope in &self.scopes {
            let Some(var) = scope.vars.iter().find(|var| var.name == name) else {
                continue;
            };
            functions.push(scope.name.clone());
            match var.location {
                LocalLocation::Reg(index) => match reg_hits.iter_mut().find(|(i, _)| *i == index) {
                    Some((_, names)) => names.push(scope.name.clone()),
                    None => reg_hits.push((index, vec![scope.name.clone()])),
                },
                LocalLocation::Frame { base, offset } => {
                    hits.entry((base, offset))
                        .or_default()
                        .push(scope.name.clone());
                }
            }
        }
        if functions.is_empty() {
            return LocalLookup::Missing;
        }
        // ⚠️ **不同函数、同一个位置** = 可以合并（`static` / 内联共享槽）；
        //    位置不同才是**真歧义**。
        let distinct = hits.len() + reg_hits.len();
        if distinct > 1 {
            return LocalLookup::Ambiguous(functions);
        }
        if let Some(((base, offset), _)) = hits.into_iter().next() {
            return LocalLookup::Found(LocalLocation::Frame { base, offset });
        }
        if let Some((index, _)) = reg_hits.into_iter().next() {
            return LocalLookup::Found(LocalLocation::Reg(index));
        }
        LocalLookup::Missing
    }

    /// 解析出来的全部函数（只读，诊断 / 测试用）。
    pub fn scopes(&self) -> &[LocalScope] {
        &self.scopes
    }
}

fn section<'a>(sections: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
    sections
        .iter()
        .find(|(section_name, _)| section_name == name)
        .map(|(_, bytes)| bytes.as_slice())
}

/// DIE 走查里**正在攒**的那个函数。
struct OpenScope {
    /// 这个 `DW_TAG_subprogram` 在树里的深度。
    depth: usize,
    low_pc: u32,
    high_pc: u32,
    frame_base: FrameBase,
    name: String,
    /// ⚠️ 变量的位置**先攒着**：栈上变量的最终地址要等这个函数收尾时
    /// 拿 `.debug_frame` 的主行 CFA 规则才能落定（CFA 在函数级才查得到）。
    pending: Vec<(String, ParsedLocation)>,
}

struct Builder<'a> {
    cfi: &'a CfiTable,
    open: Vec<OpenScope>,
    done: Vec<LocalScope>,
}

impl<'a> Builder<'a> {
    fn new(cfi: &'a CfiTable) -> Self {
        Self {
            cfi,
            open: Vec::new(),
            done: Vec::new(),
        }
    }

    fn on_die(&mut self, depth: usize, tag: u64, attrs: &DieAttrs) {
        // 先结算已经离开的函数（当前 DIE 的深度 ≤ 那个函数的深度）。
        while let Some(top) = self.open.last() {
            if top.depth < depth {
                break;
            }
            let top = self.open.pop().expect("刚看过非空");
            let scope = self.close(top);
            self.done.push(scope);
        }

        if tag == DW_TAG_SUBPROGRAM {
            // ⚠️ 只有**声明**没有定义（`DW_AT_declaration`）⇒ ⇒ 没有代码、
            //    也就没有帧 ⇒ ⇒ 不收。
            if attrs.num(DW_AT_DECLARATION).unwrap_or(0) != 0 {
                return;
            }
            let (Some(low), Some(high_raw)) = (attrs.num(DW_AT_LOW_PC), attrs.num(DW_AT_HIGH_PC))
            else {
                return;
            };
            let low = low as u32;
            let raw = high_raw as u32;
            // `DW_AT_high_pc` 既可能是**绝对结束地址**（`DW_FORM_addr`），
            // 也可能是**长度**（`data*`）⇒ ⇒ 判据是**属性的 form**，不是值
            // 的大小（`0x40` 当结束地址就比 `low` 大，拿大小判会当场判错）。
            let high = if attrs.form(DW_AT_HIGH_PC) == Some(DW_FORM_ADDR) {
                raw
            } else {
                low.saturating_add(raw)
            };
            if high <= low {
                return;
            }
            let Some(frame_base) = attrs.bytes(DW_AT_FRAME_BASE).and_then(parse_frame_base) else {
                // ⚠️ 没有帧基址 ⇒ 栈上的变量**没法定位** ⇒ 整个函数不收
                //    （寄存器位置的变量理论上还能用，但本批不为它开特例：
                //    收一半的函数会让「当前帧有什么」变得不可预期）。
                return;
            };
            let name = attrs
                .text(DW_AT_NAME)
                .or_else(|| attrs.text(DW_AT_LINKAGE_NAME))
                .unwrap_or("")
                .to_string();
            self.open.push(OpenScope {
                depth,
                low_pc: low,
                high_pc: high,
                frame_base,
                name,
                pending: Vec::new(),
            });
            return;
        }

        // ⚠️ 局部变量 / 形参：只收**函数里**的（CU 级别的变量是全局符号，
        //    归 `.symtab`，这里重复收会变成第二个真相）。
        let is_var = tag == DW_TAG_VARIABLE || tag == DW_TAG_FORMAL_PARAMETER;
        if !is_var {
            return;
        }
        let Some(top) = self.open.last_mut() else {
            return;
        };
        if depth <= top.depth {
            return;
        }
        let Some(name) = attrs
            .text(DW_AT_NAME)
            .or_else(|| attrs.text(DW_AT_LINKAGE_NAME))
        else {
            return;
        };
        let Some(location) = attrs.bytes(DW_AT_LOCATION).and_then(parse_location) else {
            // ⚠️ 位置是**区间列表**（或本模块不认的表达式）⇒ ⇒ **跳过这个
            //    变量**：「整帧可见」的口径也要有个位置才谈得上可见。
            return;
        };
        top.pending.push((name.to_string(), location));
    }

    /// 结算一个函数：**在这里**把 `Cfa` 换成 `.debug_frame` 的主行规则。
    fn close(&self, top: OpenScope) -> LocalScope {
        let (base, bias) = match top.frame_base {
            FrameBase::Reg(index) => (Some(index), 0),
            FrameBase::Cfa => match self.cfi.body_cfa(top.low_pc, top.high_pc) {
                Some(rule) => (Some(rule.reg), rule.offset),
                // ⚠️ 读不到 CFI ⇒ ⇒ **丢变量**（保留区间，见模块头）。
                None => (None, 0),
            },
        };
        let vars = match base {
            Some(base) => top
                .pending
                .into_iter()
                .map(|(name, location)| LocalVar {
                    name,
                    location: match location {
                        ParsedLocation::Reg(index) => LocalLocation::Reg(index),
                        // 栈上：`fbreg` 的偏移与 CFA 的偏移**相加** ⇒ ⇒ 最终
                        // 只剩「寄存器 + 一个偏移」。
                        ParsedLocation::FrameOffset(offset) => LocalLocation::Frame {
                            base,
                            offset: bias.wrapping_add(offset),
                        },
                    },
                })
                .collect(),
            None => Vec::new(),
        };
        LocalScope {
            low_pc: top.low_pc,
            high_pc: top.high_pc,
            frame_base: base.map_or(top.frame_base, FrameBase::Reg),
            name: top.name,
            vars,
        }
    }

    fn finish(mut self) -> LocalTable {
        while let Some(top) = self.open.pop() {
            let scope = self.close(top);
            self.done.push(scope);
        }
        // ⚠️ 词法块（`DW_TAG_LEXICAL_BLOCK`）里的变量已经在同一次走查里按
        //    深度收进来了（它们是 `top` 的后代，`depth > top.depth`）
        //    ⇒ ⇒ 这里不用再遍历一遍。
        let _ = DW_TAG_LEXICAL_BLOCK;
        LocalTable { scopes: self.done }
    }
}

enum ParsedLocation {
    Reg(u8),
    FrameOffset(i32),
}

fn parse_frame_base(bytes: &[u8]) -> Option<FrameBase> {
    match *bytes.first()? {
        DW_OP_CALL_FRAME_CFA => Some(FrameBase::Cfa),
        op if (DW_OP_REG0..=DW_OP_REG31).contains(&op) => Some(FrameBase::Reg(op - DW_OP_REG0)),
        _ => None,
    }
}

fn parse_location(bytes: &[u8]) -> Option<ParsedLocation> {
    match *bytes.first()? {
        DW_OP_FBREG => {
            let mut rest = &bytes[1..];
            Some(ParsedLocation::FrameOffset(sleb(&mut rest)? as i32))
        }
        op if (DW_OP_REG0..=DW_OP_REG31).contains(&op) => {
            Some(ParsedLocation::Reg(op - DW_OP_REG0))
        }
        _ => None,
    }
}

/// 解一个 SLEB128（只在这里用，够小就够用）。
fn sleb(bytes: &mut &[u8]) -> Option<i64> {
    let mut result = 0i64;
    let mut shift = 0u32;
    loop {
        let byte = *bytes.first()?;
        *bytes = &bytes[1..];
        if shift >= 64 {
            return None;
        }
        result |= i64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 64 && (byte & 0x40) != 0 {
                result |= !0i64 << shift;
            }
            return Some(result);
        }
    }
}

/// **测试专用**：真工具链夹具的四个调试节区（`(名字, 字节)`）。
///
/// ⚠️ 只给同一 crate 的用例用（`kernel_host::tests_m11` 拿它造 ELF ⇒ ⇒
/// 走**真实装载路径**验「DWARF ⇒ 局部变量 ⇒ 断点条件」这条链路）。
/// 夹具本体在 [`tests::fixtures`]（真 `riscv-none-elf-gcc` 15.2.0 `-O0 -g`
/// 产物按节头切出来的字节）。
#[cfg(test)]
pub(crate) fn debug_sections_fixture() -> Vec<(String, Vec<u8>)> {
    vec![
        (
            ".debug_info".to_string(),
            tests::fixtures::DEBUG_INFO.to_vec(),
        ),
        (
            ".debug_abbrev".to_string(),
            tests::fixtures::DEBUG_ABBREV.to_vec(),
        ),
        (
            ".debug_str".to_string(),
            tests::fixtures::DEBUG_STR.to_vec(),
        ),
        (
            ".debug_line_str".to_string(),
            tests::fixtures::DEBUG_LINE_STR.to_vec(),
        ),
        // CFI：局部变量的「真 CFA」从这里算（缺它 ⇒ 区间留着、变量丢掉）。
        (
            ".debug_frame".to_string(),
            crate::m11_symbol::cfi::DEBUG_FRAME.to_vec(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真工具链产物（GCC 15.2.0 · `-march=rv32im -mabi=ilp32 -O0 -g` 编的
    /// 一个小程序，链接到 `0x0`）。
    ///
    /// | 函数 | 区间 | 局部变量 |
    /// | --- | --- | --- |
    /// | `helper(int a)` | `0x00..0x38` | `a` = `fbreg(-36)`、`t` = `fbreg(-20)` |
    /// | `main` | `0x38..0x78` | `x` = `fbreg(-24)`、`y` = `fbreg(-20)` |
    ///
    /// ⚠️ 两个函数的 `DW_AT_frame_base` 都是 `DW_OP_call_frame_cfa` ⇒ ⇒ 本批
    /// 走「`CFA == s0`」的口径（与反汇编逐字对过：`x` 就存在 `-24(s0)`）。
    pub(super) mod fixtures {
        use super::super::LocalTable;

        pub const DEBUG_INFO: &[u8] = &[
            0xa1, 0x00, 0x00, 0x00, 0x05, 0x00, 0x01, 0x04, 0x00, 0x00, 0x00, 0x00, 0x02, 0x0a,
            0x00, 0x00, 0x00, 0x1d, 0x03, 0x47, 0x16, 0x03, 0x00, 0x0f, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x78, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x05, 0x3d, 0x00, 0x00, 0x00, 0x05,
            0x03, 0x78, 0x10, 0x00, 0x00, 0x04, 0x04, 0x05, 0x69, 0x6e, 0x74, 0x00, 0x05, 0x6a,
            0x00, 0x00, 0x00, 0x01, 0x08, 0x05, 0x3d, 0x00, 0x00, 0x00, 0x38, 0x00, 0x00, 0x00,
            0x40, 0x00, 0x00, 0x00, 0x01, 0x9c, 0x75, 0x00, 0x00, 0x00, 0x01, 0x78, 0x00, 0x09,
            0x3d, 0x00, 0x00, 0x00, 0x02, 0x91, 0x68, 0x01, 0x79, 0x00, 0x0a, 0x3d, 0x00, 0x00,
            0x00, 0x02, 0x91, 0x6c, 0x00, 0x06, 0x63, 0x00, 0x00, 0x00, 0x01, 0x03, 0x05, 0x3d,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x38, 0x00, 0x00, 0x00, 0x01, 0x9c, 0x07,
            0x61, 0x00, 0x01, 0x03, 0x10, 0x3d, 0x00, 0x00, 0x00, 0x02, 0x91, 0x5c, 0x01, 0x74,
            0x00, 0x04, 0x3d, 0x00, 0x00, 0x00, 0x02, 0x91, 0x6c, 0x00, 0x00,
        ];
        pub const DEBUG_ABBREV: &[u8] = &[
            0x01, 0x34, 0x00, 0x03, 0x08, 0x3a, 0x21, 0x01, 0x3b, 0x0b, 0x39, 0x21, 0x09, 0x49,
            0x13, 0x02, 0x18, 0x00, 0x00, 0x02, 0x11, 0x01, 0x25, 0x0e, 0x13, 0x0b, 0x90, 0x01,
            0x0b, 0x91, 0x01, 0x06, 0x03, 0x1f, 0x1b, 0x1f, 0x11, 0x01, 0x12, 0x06, 0x10, 0x17,
            0x00, 0x00, 0x03, 0x34, 0x00, 0x03, 0x0e, 0x3a, 0x0b, 0x3b, 0x0b, 0x39, 0x0b, 0x49,
            0x13, 0x3f, 0x19, 0x02, 0x18, 0x00, 0x00, 0x04, 0x24, 0x00, 0x0b, 0x0b, 0x3e, 0x0b,
            0x03, 0x08, 0x00, 0x00, 0x05, 0x2e, 0x01, 0x3f, 0x19, 0x03, 0x0e, 0x3a, 0x0b, 0x3b,
            0x0b, 0x39, 0x0b, 0x27, 0x19, 0x49, 0x13, 0x11, 0x01, 0x12, 0x06, 0x40, 0x18, 0x7c,
            0x19, 0x01, 0x13, 0x00, 0x00, 0x06, 0x2e, 0x01, 0x3f, 0x19, 0x03, 0x0e, 0x3a, 0x0b,
            0x3b, 0x0b, 0x39, 0x0b, 0x27, 0x19, 0x49, 0x13, 0x11, 0x01, 0x12, 0x06, 0x40, 0x18,
            0x7a, 0x19, 0x00, 0x00, 0x07, 0x05, 0x00, 0x03, 0x08, 0x3a, 0x0b, 0x3b, 0x0b, 0x39,
            0x0b, 0x49, 0x13, 0x02, 0x18, 0x00, 0x00, 0x00,
        ];
        pub const DEBUG_STR: &[u8] = &[
            0x67, 0x5f, 0x63, 0x6f, 0x75, 0x6e, 0x74, 0x65, 0x72, 0x00, 0x47, 0x4e, 0x55, 0x20,
            0x43, 0x32, 0x33, 0x20, 0x31, 0x35, 0x2e, 0x32, 0x2e, 0x30, 0x20, 0x2d, 0x6d, 0x61,
            0x62, 0x69, 0x3d, 0x69, 0x6c, 0x70, 0x33, 0x32, 0x20, 0x2d, 0x6d, 0x69, 0x73, 0x61,
            0x2d, 0x73, 0x70, 0x65, 0x63, 0x3d, 0x32, 0x30, 0x31, 0x39, 0x31, 0x32, 0x31, 0x33,
            0x20, 0x2d, 0x6d, 0x61, 0x72, 0x63, 0x68, 0x3d, 0x72, 0x76, 0x33, 0x32, 0x69, 0x6d,
            0x5f, 0x7a, 0x6d, 0x6d, 0x75, 0x6c, 0x20, 0x2d, 0x67, 0x20, 0x2d, 0x4f, 0x30, 0x20,
            0x2d, 0x66, 0x66, 0x72, 0x65, 0x65, 0x73, 0x74, 0x61, 0x6e, 0x64, 0x69, 0x6e, 0x67,
            0x00, 0x68, 0x65, 0x6c, 0x70, 0x65, 0x72, 0x00, 0x6d, 0x61, 0x69, 0x6e, 0x00,
        ];
        pub const DEBUG_LINE_STR: &[u8] = &[
            0x2f, 0x74, 0x6d, 0x70, 0x2f, 0x6d, 0x31, 0x31, 0x6c, 0x6f, 0x63, 0x61, 0x6c, 0x73,
            0x00, 0x74, 0x2e, 0x63, 0x00,
        ];

        pub fn table() -> LocalTable {
            LocalTable::from_sections(&[
                (".debug_info".to_string(), DEBUG_INFO.to_vec()),
                (".debug_abbrev".to_string(), DEBUG_ABBREV.to_vec()),
                (".debug_str".to_string(), DEBUG_STR.to_vec()),
                (".debug_line_str".to_string(), DEBUG_LINE_STR.to_vec()),
                (
                    ".debug_frame".to_string(),
                    crate::m11_symbol::cfi::DEBUG_FRAME.to_vec(),
                ),
            ])
        }

        /// 同一份 `<info>/<abbrev>`，但**没有** `.debug_frame`。
        pub fn table_without_frame() -> LocalTable {
            LocalTable::from_sections(&[
                (".debug_info".to_string(), DEBUG_INFO.to_vec()),
                (".debug_abbrev".to_string(), DEBUG_ABBREV.to_vec()),
                (".debug_str".to_string(), DEBUG_STR.to_vec()),
                (".debug_line_str".to_string(), DEBUG_LINE_STR.to_vec()),
            ])
        }
    }

    #[test]
    fn the_real_fixture_yields_both_functions_with_their_variables() {
        let table = fixtures::table();
        assert_eq!(table.scope_count(), 2, "两个函数都得收下");

        let main = table.scope_at(0x38).expect("0x38 是 main 的第一条");
        assert_eq!(main.name, "main");
        assert_eq!((main.low_pc, main.high_pc), (0x38, 0x78));
        assert_eq!(main.frame_base, FrameBase::Reg(8), "CFA 主行是 s0+0");
        let names: Vec<&str> = main.vars.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["x", "y"]);
        // ⚠️ 与反汇编逐字一致：`x` 存在 `-24(s0)`、`y` 存在 `-20(s0)`。
        //    主行 CFA 是 `s0+0` ⇒ ⇒ 偏移与 `fbreg` 写的一模一样。
        assert_eq!(
            table.lookup_at("x", 0x38),
            Some(LocalLocation::Frame {
                base: 8,
                offset: -24
            })
        );
        assert_eq!(
            table.lookup_at("y", 0x38),
            Some(LocalLocation::Frame {
                base: 8,
                offset: -20
            })
        );

        let helper = table.scope_at(0x10).expect("0x10 落在 helper 里");
        assert_eq!(helper.name, "helper");
        assert_eq!((helper.low_pc, helper.high_pc), (0x0, 0x38));
        assert_eq!(
            table.lookup_at("a", 0x10),
            Some(LocalLocation::Frame {
                base: 8,
                offset: -36
            })
        );
    }

    #[test]
    fn a_pc_outside_every_function_has_no_locals() {
        let table = fixtures::table();
        assert_eq!(table.scope_at(0x200), None);
        assert_eq!(table.lookup_at("x", 0x200), None);
        // ⚠️ 边界：`high_pc` 是**开区间**（`0x38` 属于 main、不属于 helper）。
        assert_eq!(
            table.scope_at(0x37).map(|s| s.name.as_str()),
            Some("helper")
        );
        assert_eq!(table.scope_at(0x38).map(|s| s.name.as_str()), Some("main"));
        assert_eq!(table.scope_at(0x78), None, "0x78 已经出 main 了");
    }

    #[test]
    fn a_global_is_not_a_local() {
        let table = fixtures::table();
        assert_eq!(table.lookup_at("g_counter", 0x38), None);
        assert_eq!(table.lookup_any("g_counter"), LocalLookup::Missing);
        assert_eq!(table.lookup_any("main"), LocalLookup::Missing);
    }

    #[test]
    fn one_name_in_one_function_resolves_without_a_pc() {
        let table = fixtures::table();
        // `x` / `y` / `a` / `t` 各只出现在一个函数里 ⇒ ⇒ 无 pc 也能定。
        assert_eq!(
            table.lookup_any("x"),
            LocalLookup::Found(LocalLocation::Frame {
                base: 8,
                offset: -24
            })
        );
        assert_eq!(
            table.lookup_any("a"),
            LocalLookup::Found(LocalLocation::Frame {
                base: 8,
                offset: -36
            })
        );
    }

    #[test]
    fn without_cfi_the_scopes_stay_but_their_variables_go() {
        // ⚠️ 用户拍板的口径：读不到 `.debug_frame` ⇒ ⇒ **保留区间、丢变量**。
        //    「所在函数」照样能显示，写局部变量名则当作不认识（不猜位置）。
        let table = fixtures::table_without_frame();
        assert_eq!(table.scope_count(), 2, "区间照收");
        assert_eq!(
            table.scope_at(0x38).map(|scope| scope.name.as_str()),
            Some("main"),
            "所在函数照旧可查"
        );
        assert!(
            table.scope_at(0x38).expect("main").vars.is_empty(),
            "没有 CFA ⇒ 栈上变量全丢"
        );
        assert_eq!(table.lookup_at("x", 0x38), None);
        assert_eq!(table.lookup_any("x"), LocalLookup::Missing);
    }

    #[test]
    fn an_sp_based_frame_moves_the_offsets_onto_sp() {
        // ⚠️ 这条钉「不再是 s0 假设」：把 `.debug_frame` 换成
        //    「main 的 CFA = sp+32」（`-O2` / 无帧指针产物的形状）⇒
        //    真夹具里 `x`（fbreg -24）必须落到 `[sp+8]`，而不是 `[s0-24]`。
        let mut sections = vec![
            (".debug_info".to_string(), fixtures::DEBUG_INFO.to_vec()),
            (".debug_abbrev".to_string(), fixtures::DEBUG_ABBREV.to_vec()),
            (".debug_str".to_string(), fixtures::DEBUG_STR.to_vec()),
            (
                ".debug_line_str".to_string(),
                fixtures::DEBUG_LINE_STR.to_vec(),
            ),
        ];
        sections.push((".debug_frame".to_string(), sp_based_frame()));
        let table = LocalTable::from_sections(&sections);
        assert_eq!(
            table.lookup_at("x", 0x38),
            Some(LocalLocation::Frame { base: 2, offset: 8 }),
            "CFA = sp+32、fbreg -24 ⇒ sp+8"
        );
        // 没有 FDE 的 `helper` ⇒ 变量照口径丢掉（区间还在）。
        assert_eq!(
            table.scope_at(0x10).map(|s| s.name.as_str()),
            Some("helper")
        );
        assert_eq!(table.lookup_at("a", 0x10), None);
    }

    /// 一条**只覆盖 `main`** 的 `.debug_frame`：CIE `sp+0`，FDE 在 `0x3c`
    /// 处切成 `sp+32`（覆盖 `0x3c..0x78` ⇒ 主行就是它）。
    fn sp_based_frame() -> Vec<u8> {
        let mut bytes = vec![
            0x0c, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x03, 0x00, 0x01, 0x7c, 0x01, 0x0c,
            0x02, 0x00,
        ];
        bytes.extend_from_slice(&[
            0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x38, 0x00, 0x00, 0x00, 0x40, 0x00,
            0x00, 0x00, 0x44, 0x0c, 0x02, 0x20,
        ]);
        bytes
    }

    #[test]
    fn the_same_name_in_two_functions_is_reported_as_ambiguous() {
        // 夹具里 `y`（main · -20）与 `t`（helper · -20）不同名，所以要单独
        // 造一张表：两个函数都有 `dup`，且**位置不同**。
        let table = LocalTable {
            scopes: vec![
                LocalScope {
                    low_pc: 0x0,
                    high_pc: 0x10,
                    frame_base: FrameBase::Reg(8),
                    name: "alpha".to_string(),
                    vars: vec![LocalVar {
                        name: "dup".to_string(),
                        location: LocalLocation::Frame {
                            base: 8,
                            offset: -8,
                        },
                    }],
                },
                LocalScope {
                    low_pc: 0x10,
                    high_pc: 0x20,
                    frame_base: FrameBase::Reg(8),
                    name: "beta".to_string(),
                    vars: vec![LocalVar {
                        name: "dup".to_string(),
                        location: LocalLocation::Frame {
                            base: 8,
                            offset: -12,
                        },
                    }],
                },
            ],
        };
        assert_eq!(
            table.lookup_any("dup"),
            LocalLookup::Ambiguous(vec!["alpha".to_string(), "beta".to_string()]),
            "两个函数里的同名变量位置不同 ⇒ 必须报歧义，不许随手挑一个"
        );
        // 有 pc 时就没有歧义了。
        assert_eq!(
            table.lookup_at("dup", 0x4),
            Some(LocalLocation::Frame {
                base: 8,
                offset: -8
            })
        );
        assert_eq!(
            table.lookup_at("dup", 0x14),
            Some(LocalLocation::Frame {
                base: 8,
                offset: -12
            })
        );
    }

    #[test]
    fn a_registered_variable_is_a_register_location() {
        // `DW_OP_reg10` ⇒ ⇒ 值在 `a0` 里（`DW_OP_regN` 的 N 是 **DWARF** 的
        // 编号，RISC-V 上就是同一个 x 编号）。
        let table = LocalTable {
            scopes: vec![LocalScope {
                low_pc: 0x0,
                high_pc: 0x10,
                frame_base: FrameBase::Reg(8),
                name: "f".to_string(),
                vars: vec![LocalVar {
                    name: "arg".to_string(),
                    location: LocalLocation::Reg(10),
                }],
            }],
        };
        assert_eq!(table.lookup_at("arg", 0x0), Some(LocalLocation::Reg(10)));
    }

    #[test]
    fn no_debug_sections_is_an_empty_table_not_an_error() {
        let table = LocalTable::from_sections(&[(".symtab".to_string(), vec![0u8; 4])]);
        assert!(table.is_empty());
        assert_eq!(table.lookup_any("x"), LocalLookup::Missing);
        assert_eq!(table.scope_at(0), None);
    }
}
