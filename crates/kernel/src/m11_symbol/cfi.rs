//! **`.debug_frame`（CFI）的最小读法**（STAGE-55 · CFI 批）。
//!
//! ## 它回答什么问题
//!
//! GCC 在 RISC-V 上给函数的 `DW_AT_frame_base` 写的是
//! `DW_OP_call_frame_cfa`（**不是**某个寄存器）⇒ ⇒ 局部变量的地址
//! `fbreg(off)` = `CFA + off`，而 CFA 是多少**只有 CFI 知道**：
//! 它在 `.debug_frame` 里按 pc 分段写（序言、函数体、尾声各不相同）。
//!
//! 批 D 曾经把这条信息换成一条显式假设「`CFA == s0`」——那只在
//! 「`-O0` 有帧指针」的产物上成立（`s0` 恰好就是 CFA），一旦产物
//! `-O2` / `-fomit-frame-pointer` 就**静默读错**。本模块把那句假设换成
//! 真的读 `.debug_frame`。
//!
//! ## 只取 CFA 规则，只取一行
//!
//! 本模块**不**建完整的 unwind 表（不关心 RA / 各寄存器的保存位置）：
//! 只跑 CFA 相关的几条指令（`def_cfa` / `def_cfa_register` /
//! `def_cfa_offset` / `def_cfa_(offset_)sf` / `def_cfa_expression`），
//! 其余指令**照长度跳过**（只为了让游标走对）。
//!
//! 一个函数里 CFA 会随 pc 变，而「整帧可见」的口径（用户 2026-11-22
//! 拍板）要求**同一个变量在任何 pc 上都算出同一个位置** ⇒ ⇒ 取
//! **覆盖地址最多的那一行**（函数体主行）：
//!
//! * 序言 / 尾声的行只覆盖几条指令，按行精确反而会让「函数入口的断点」
//!   读到另一套偏移；
//! * 主行是用户真正会下断点、真正会看变量的地方，也是「整帧」这个词
//!   指向的那一段。
//!
//! ## 读不懂就**不猜**
//!
//! 任何畸形输入、`z` 之外的增补串、`DW_CFA_def_cfa_expression`（CFA 由
//! 表达式算，本模块不认）⇒ ⇒ 这一条 FDE / 这个函数的 CFA **没有答案**
//! （`None`）。上层（`m11_symbol::locals`）据此**丢掉该帧的栈上变量**，
//! 而不是退回旧假设。
//!
//! ⚠️ 地址宽度按 **4**（ELF32 / RV32）。M-06 只收 ELF32，这里不再分叉。

const CIE_ID: u32 = 0xffff_ffff;

/// 一条 CFA 规则：`CFA = 寄存器[reg] + offset`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CfaRule {
    /// 基址寄存器（RISC-V 的 `x` 编号：`2` = `sp`、`8` = `s0`）。
    pub reg: u8,
    /// 相对基址寄存器的偏移（字节）。
    pub offset: i32,
}

/// CFI 里的一行（从 `start` 起生效）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Row {
    start: u32,
    /// `None` = 这一段的 CFA 还没被任何指令定义（或由不认的表达式算）。
    cfa: Option<CfaRule>,
    /// 返回地址**存在哪儿**：`[CFA + ra_offset]`。
    ///
    /// `None` = 这一行没给 RA 规则（栈回溯到这一帧就断了，**不猜**）。
    ra_offset: Option<i32>,
}

/// 一个 FDE：它覆盖的地址区间 + 分段的行。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fde {
    low: u32,
    high: u32,
    rows: Vec<Row>,
}

/// 一个 CIE：初值规则 + 对齐因子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cie {
    code_align: u32,
    data_align: i32,
    initial: Option<CfaRule>,
    /// **返回地址列号**（RISC-V 上是 1 = `ra`）。
    ra_register: u8,
    /// CIE 自己给的 RA 初值规则（`DW_CFA_offset ra`）。
    initial_ra: Option<i32>,
    /// 增补串以 `z` 开头 ⇒ 每条记录前多一个「增补数据长度」。
    has_z_augmentation: bool,
}

/// 一个镜像里**全部** CFI（按 `.debug_frame` 的 FDE 顺序）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CfiTable {
    fdes: Vec<Fde>,
}

impl CfiTable {
    /// 从 M-06 带出的调试节区构建。
    ///
    /// ⚠️ **没有 `.debug_frame`** ⇒ 空表（不是错误）：上层据此丢变量。
    pub fn from_sections(sections: &[(String, Vec<u8>)]) -> Self {
        let Some(bytes) = sections
            .iter()
            .find(|(name, _)| name == ".debug_frame")
            .map(|(_, bytes)| bytes.as_slice())
        else {
            return Self::default();
        };
        Self::parse(bytes)
    }

    /// 有没有解析到任何 FDE。
    pub fn is_empty(&self) -> bool {
        self.fdes.is_empty()
    }

    /// 解析到几条 FDE（诊断 / 测试用）。
    pub fn fde_count(&self) -> usize {
        self.fdes.len()
    }

    /// 覆盖 `[low, high)` 这个函数区间的**主行** CFA 规则。
    ///
    /// 取「覆盖地址最多」的那一行（见模块头）；没有 FDE 覆盖整个区间、
    /// 或主行没有可用规则 ⇒ `None`（**不猜**）。
    pub fn body_cfa(&self, low: u32, high: u32) -> Option<CfaRule> {
        let fde = self
            .fdes
            .iter()
            .filter(|fde| fde.low <= low && fde.high >= high)
            .min_by_key(|fde| fde.high.saturating_sub(fde.low))?;
        let mut best: Option<(u32, CfaRule)> = None;
        for (index, row) in fde.rows.iter().enumerate() {
            let end = fde
                .rows
                .get(index + 1)
                .map(|next| next.start)
                .unwrap_or(fde.high);
            let Some(rule) = row.cfa else {
                continue;
            };
            if end <= row.start {
                continue;
            }
            let span = end.saturating_sub(row.start);
            // ⚠️ 同宽取**后者**：函数体行通常排在序言之后。
            match best {
                Some((best_span, _)) if span < best_span => {}
                _ => best = Some((span, rule)),
            }
        }
        best.map(|(_, rule)| rule)
    }

    /// **`pc` 落在哪一行**（逐行，**不**取主行）。
    ///
    /// 栈回溯要的是「此刻这一条指令」的 CFA / RA 规则 ⇒ ⇒ 按 pc 精确取行；
    /// 局部变量那条路才要主行（见 [`Self::body_cfa`]）。
    pub fn row_at(&self, pc: u32) -> Option<UnwindRow> {
        let fde = self
            .fdes
            .iter()
            .filter(|fde| pc >= fde.low && pc < fde.high)
            .min_by_key(|fde| fde.high.saturating_sub(fde.low))?;
        let row = fde
            .rows
            .iter()
            .take_while(|row| row.start <= pc)
            .last()
            .or_else(|| fde.rows.first())?;
        Some(UnwindRow {
            cfa: row.cfa,
            ra_offset: row.ra_offset,
        })
    }

    fn parse(bytes: &[u8]) -> Self {
        let mut fdes = Vec::new();
        let mut cies: Vec<(usize, Cie)> = Vec::new();
        let mut offset = 0usize;
        while let Some(length) = read_u32(bytes, offset) {
            if length == 0 {
                break; // 终止符（`.debug_frame` 允许，GCC 不写）
            }
            let body = offset + 4;
            let end = body.saturating_add(length as usize);
            if length < 4 || end > bytes.len() {
                break; // 截断 / 畸形 ⇒ 已解析到的保留，后面的放弃
            }
            match read_u32(bytes, body) {
                Some(CIE_ID) => {
                    if let Some(cie) = Cie::parse(&bytes[body + 4..end]) {
                        // ⚠️ 键是**记录起点**（含 4 字节长度）：FDE 里的
                        //    `cie_pointer` 就是从节区开头数过来的这个偏移。
                        cies.push((offset, cie));
                    }
                }
                Some(cie_pointer) => {
                    let Some((_, cie)) = cies.iter().find(|(at, _)| *at == cie_pointer as usize)
                    else {
                        offset = end;
                        continue;
                    };
                    if let Some(fde) = Fde::parse(&bytes[body + 4..end], *cie) {
                        fdes.push(fde);
                    }
                }
                None => break,
            }
            offset = end;
        }
        Self { fdes }
    }
}

impl Cie {
    fn parse(body: &[u8]) -> Option<Self> {
        let version = *body.first()?;
        // `.debug_frame` 的版本：1 / 3 / 4 的字段顺序一样（v2 / v5 不认）。
        if !matches!(version, 1 | 3 | 4) {
            return None;
        }
        let mut rest = &body[1..];
        let augmentation = read_cstr(&mut rest)?;
        // ⚠️ 增补串只认**空**或 `z` 打头：`eh` / `zR` / `zPLR` 之类要么是
        //    `.eh_frame` 的东西，要么要额外的指针解码 ⇒ ⇒ 整条 CIE 不认。
        let has_z_augmentation = augmentation.starts_with('z');
        if !augmentation.is_empty() && !has_z_augmentation {
            return None;
        }
        let code_align = uleb(&mut rest)? as u32;
        let data_align = sleb(&mut rest)? as i32;
        let ra_register = u8::try_from(uleb(&mut rest)?).ok()?;
        if has_z_augmentation {
            let aug_len = uleb(&mut rest)? as usize;
            rest = rest.get(aug_len..)?;
        }
        let (_, initial, initial_ra) = run_cfi(
            rest,
            0,
            None,
            None,
            None,
            ra_register,
            code_align,
            data_align,
            &mut Vec::new(),
        );
        Some(Self {
            code_align,
            data_align,
            initial,
            ra_register,
            initial_ra,
            has_z_augmentation,
        })
    }
}

impl Fde {
    fn parse(body: &[u8], cie: Cie) -> Option<Self> {
        let low = read_u32(body, 0)?;
        let range = read_u32(body, 4)?;
        let high = low.saturating_add(range);
        if high <= low {
            return None;
        }
        let mut rest = &body[8..];
        if cie.has_z_augmentation {
            let aug_len = uleb(&mut rest)? as usize;
            rest = rest.get(aug_len..)?;
        }
        let mut rows = Vec::new();
        let (_, _, _) = run_cfi(
            rest,
            low,
            cie.initial,
            cie.initial_ra,
            cie.initial_ra,
            cie.ra_register,
            cie.code_align,
            cie.data_align,
            &mut rows,
        );
        if rows.is_empty() {
            return None;
        }
        Some(Self { low, high, rows })
    }
}

/// 一行 CFI 给栈回溯的两条信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnwindRow {
    /// 这一段的 CFA 规则（`None` = 算不出来）。
    pub cfa: Option<CfaRule>,
    /// 返回地址存在 `[CFA + ra_offset]`（`None` = 这一行没给 RA 规则）。
    pub ra_offset: Option<i32>,
}

/// 跑一遍 CFA / RA 指令；返回**结束时**的 `(pc, CFA 规则, RA 偏移)`。
///
/// `rows` 只在 pc 变化时追加（变更规则不影响已经开始的这一段）。
///
/// ⚠️ 只追两条规则：**CFA** 与**返回地址的保存位置**。其余寄存器规则
/// （callee-saved 存哪儿）照长度跳过 —— 那要完整 unwind 表，本批不做。
#[allow(clippy::too_many_arguments)]
fn run_cfi(
    mut code: &[u8],
    mut pc: u32,
    mut cfa: Option<CfaRule>,
    mut ra: Option<i32>,
    initial_ra: Option<i32>,
    ra_register: u8,
    code_align: u32,
    data_align: i32,
    rows: &mut Vec<Row>,
) -> (u32, Option<CfaRule>, Option<i32>) {
    let mut stack: Vec<(u32, Option<CfaRule>, Option<i32>)> = Vec::new();
    while let Some(&op) = code.first() {
        code = &code[1..];
        match op {
            0x00 => {} // nop
            0x01 => {
                // set_loc：绝对地址跳转（地址宽度 4）
                let Some(loc) = read_u32(code, 0) else {
                    break;
                };
                code = &code[4..];
                rows.push(Row {
                    start: pc,
                    cfa,
                    ra_offset: ra,
                });
                pc = loc;
            }
            0x02..=0x04 => {
                let width = 1usize << (op - 0x02);
                let Some(delta) = read_varint(code, width) else {
                    break;
                };
                code = &code[width..];
                rows.push(Row {
                    start: pc,
                    cfa,
                    ra_offset: ra,
                });
                pc = pc.saturating_add(delta.saturating_mul(code_align));
            }
            0x05 => {
                // restore_extended：把某个寄存器恢复到 CIE 初值
                let Some(reg) = uleb(&mut code) else {
                    break;
                };
                if u8::try_from(reg).ok() == Some(ra_register) {
                    ra = initial_ra;
                }
            }
            0x06 => {
                // undefined：该寄存器的值不再可知
                let Some(reg) = uleb(&mut code) else {
                    break;
                };
                if u8::try_from(reg).ok() == Some(ra_register) {
                    ra = None;
                }
            }
            0x07 => {
                // same_value：值没变（可能不在栈上）⇒ 本模块不追
                let Some(reg) = uleb(&mut code) else {
                    break;
                };
                if u8::try_from(reg).ok() == Some(ra_register) {
                    ra = None;
                }
            }
            0x08 => {
                // offset_extended：reg + ULEB（按 data_align 缩放）
                let (Some(reg), Some(offset)) = (uleb(&mut code), uleb(&mut code)) else {
                    break;
                };
                if u8::try_from(reg).ok() == Some(ra_register) {
                    ra = Some(scale(offset as i64, data_align));
                }
            }
            0x09 => {
                // register_extended：reg + reg（值在寄存器里）⇒ RA 不追
                if uleb(&mut code).is_none() || uleb(&mut code).is_none() {
                    break;
                }
            }
            0x0a => stack.push((pc, cfa, ra)),
            0x0b => {
                let Some((saved_pc, saved_cfa, saved_ra)) = stack.pop() else {
                    break;
                };
                pc = saved_pc;
                cfa = saved_cfa;
                ra = saved_ra;
            }
            0x0c => {
                // def_cfa：CFA 规则在这里第一次被定义
                let (Some(reg), Some(offset)) = (uleb(&mut code), uleb(&mut code)) else {
                    break;
                };
                match u8::try_from(reg) {
                    Ok(reg) => {
                        cfa = Some(CfaRule {
                            reg,
                            offset: offset as i32,
                        })
                    }
                    Err(_) => cfa = None,
                }
            }
            0x0d => {
                // def_cfa_register：只换寄存器，偏移不动
                let Some(reg) = uleb(&mut code) else {
                    break;
                };
                cfa = u8::try_from(reg).ok().and_then(|reg| {
                    cfa.map(|current| CfaRule {
                        reg,
                        offset: current.offset,
                    })
                });
            }
            0x0e => {
                // def_cfa_offset：只换偏移，寄存器不动
                let Some(offset) = uleb(&mut code) else {
                    break;
                };
                cfa = cfa.map(|current| CfaRule {
                    offset: offset as i32,
                    ..current
                });
            }
            0x0f => {
                // def_cfa_expression：CFA 由表达式算 ⇒ ⇒ 本模块不认
                let Some(length) = uleb(&mut code) else {
                    break;
                };
                let Some(rest) = code.get(length as usize..) else {
                    break;
                };
                code = rest;
                cfa = None;
            }
            0x10 | 0x16 => {
                // expression / val_expression：reg(仅 0x10) + 长度 + 字节
                if op == 0x10 && uleb(&mut code).is_none() {
                    break;
                }
                let Some(length) = uleb(&mut code) else {
                    break;
                };
                let Some(rest) = code.get(length as usize..) else {
                    break;
                };
                code = rest;
            }
            0x11 => {
                // offset_extended_sf：reg + 有符号偏移（按 data_align 缩放）
                let (Some(reg), Some(scaled)) = (uleb(&mut code), sleb(&mut code)) else {
                    break;
                };
                if u8::try_from(reg).ok() == Some(ra_register) {
                    ra = Some(scale(scaled, data_align));
                }
            }
            0x12 => {
                // def_cfa_sf：reg + 有符号偏移（按 data_align 缩放）
                let (Some(reg), Some(scaled)) = (uleb(&mut code), sleb(&mut code)) else {
                    break;
                };
                match u8::try_from(reg) {
                    Ok(reg) => {
                        cfa = Some(CfaRule {
                            reg,
                            offset: scale(scaled, data_align),
                        })
                    }
                    Err(_) => cfa = None,
                }
            }
            0x13 => {
                // def_cfa_offset_sf：有符号偏移（按 data_align 缩放）
                let Some(scaled) = sleb(&mut code) else {
                    break;
                };
                cfa = cfa.map(|current| CfaRule {
                    offset: scale(scaled, data_align),
                    ..current
                });
            }
            0x14 | 0x15 => {
                // val_offset / val_offset_sf：reg + 偏移
                if uleb(&mut code).is_none() {
                    break;
                }
                let next = if op == 0x14 {
                    uleb(&mut code).is_some()
                } else {
                    sleb(&mut code).is_some()
                };
                if !next {
                    break;
                }
            }
            0x2e => {
                // GNU_args_size（`.eh_frame` 的东西，照长度跳过）
                if uleb(&mut code).is_none() {
                    break;
                }
            }
            0x40..=0x7f => {
                // advance_loc：低 6 位 × code_align
                let delta = u32::from(op & 0x3f).saturating_mul(code_align);
                rows.push(Row {
                    start: pc,
                    cfa,
                    ra_offset: ra,
                });
                pc = pc.saturating_add(delta);
            }
            0x80..=0xbf => {
                // offset：reg（低 6 位）+ ULEB（按 data_align 缩放）
                let Some(offset) = uleb(&mut code) else {
                    break;
                };
                if op & 0x3f == ra_register {
                    ra = Some(scale(offset as i64, data_align));
                }
            }
            0xc0..=0xff => {
                // restore：把该寄存器恢复成 CIE 初值
                if op & 0x3f == ra_register {
                    ra = initial_ra;
                }
            }
            _ => break, // 未知主操作码 ⇒ 后面的看不懂了
        }
    }
    rows.push(Row {
        start: pc,
        cfa,
        ra_offset: ra,
    });
    (pc, cfa, ra)
}

/// `DW_CFA_*_sf` 的缩放：偏移 × `data_align`（GCC 在 RV32 上是 `-4`）。
fn scale(value: i64, data_align: i32) -> i32 {
    value.saturating_mul(i64::from(data_align)) as i32
}

/// 读一个小端 `u32`（越界 ⇒ `None`）。
fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// 读 `width ∈ {1,2,4}` 字节的小端无符号数。
fn read_varint(bytes: &[u8], width: usize) -> Option<u32> {
    match width {
        1 => bytes.first().map(|byte| u32::from(*byte)),
        2 => {
            let slice = bytes.get(..2)?;
            Some(u32::from(u16::from_le_bytes([slice[0], slice[1]])))
        }
        4 => read_u32(bytes, 0),
        _ => None,
    }
}

/// 读到 NUL 为止的一串字节（增补串）。
fn read_cstr<'a>(bytes: &mut &'a [u8]) -> Option<&'a str> {
    let end = bytes.iter().position(|byte| *byte == 0)?;
    let (head, tail) = bytes.split_at(end);
    *bytes = &tail[1..];
    std::str::from_utf8(head).ok()
}

fn uleb(bytes: &mut &[u8]) -> Option<u64> {
    let mut result = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *bytes.first()?;
        *bytes = &bytes[1..];
        if shift >= 64 {
            return None;
        }
        result |= u64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            return Some(result);
        }
    }
}

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

/// **真夹具**：GCC 15.2.0 `-O0` 产物里的 `.debug_frame`（84 字节，
/// 两个 FDE：`0x00..0x38`、`0x38..0x78`）。
///
/// ⚠️ 只给同一 crate 的用例用：`locals` 的真夹具把它拼进调试节区 ⇒ ⇒
/// 「真 `.debug_frame` ⇒ CFA ⇒ 局部变量地址」这条链路才算真的走通。
#[cfg(test)]
pub(crate) const DEBUG_FRAME: &[u8] = &[
    0x0c, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x03, 0x00, 0x01, 0x7c, 0x01, 0x0c, 0x02, 0x00,
    0x24, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x38, 0x00, 0x00, 0x00,
    0x44, 0x0e, 0x30, 0x48, 0x81, 0x01, 0x88, 0x02, 0x44, 0x0c, 0x08, 0x00, 0x5c, 0xc1, 0x44, 0xc8,
    0x0c, 0x02, 0x30, 0x44, 0x0e, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x38, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x44, 0x0e, 0x20, 0x48, 0x81, 0x01, 0x88, 0x02,
    0x44, 0x0c, 0x08, 0x00,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn table(bytes: &[u8]) -> CfiTable {
        CfiTable::parse(bytes)
    }

    #[test]
    fn the_return_address_rule_lands_on_the_rows_that_have_it() {
        // 真夹具：`DW_CFA_offset ra at cfa-4`（字节 `0x81 0x01`）出现在
        // helper 的 0x0c 之后 ⇒ 函数体两行都带 `ra = Some(-4)`。
        let table = table(DEBUG_FRAME);
        assert_eq!(
            table.row_at(0x10),
            Some(UnwindRow {
                cfa: Some(CfaRule { reg: 8, offset: 0 }),
                ra_offset: Some(-4),
            }),
            "helper 函数体：CFA = s0+0、返回地址在 cfa-4"
        );
        assert_eq!(
            table.row_at(0x48),
            Some(UnwindRow {
                cfa: Some(CfaRule { reg: 8, offset: 0 }),
                ra_offset: Some(-4),
            })
        );
    }

    #[test]
    fn rows_before_the_ra_store_have_no_return_address() {
        let table = table(DEBUG_FRAME);
        // helper 序言 0x04..0x0c：CFA 已经是 sp+48，但 `ra` 还没存进栈。
        assert_eq!(
            table.row_at(0x04),
            Some(UnwindRow {
                cfa: Some(CfaRule { reg: 2, offset: 48 }),
                ra_offset: None,
            })
        );
        // 尾声把 `ra` 恢复回寄存器 ⇒ ⇒ 行里也没有「存哪儿」。
        assert_eq!(
            table.row_at(0x34),
            Some(UnwindRow {
                cfa: Some(CfaRule { reg: 2, offset: 0 }),
                ra_offset: None,
            })
        );
    }

    #[test]
    fn row_at_takes_the_row_for_the_pc_not_the_body_row() {
        let table = table(DEBUG_FRAME);
        // 主行是函数体（`s0+0`），但 0x04 那一条要看**序言**的 `sp+48`。
        assert_eq!(
            table.body_cfa(0x00, 0x38),
            Some(CfaRule { reg: 8, offset: 0 })
        );
        assert_eq!(
            table.row_at(0x04).map(|row| row.cfa),
            Some(Some(CfaRule { reg: 2, offset: 48 }))
        );
    }

    #[test]
    fn row_at_outside_every_fde_has_no_answer() {
        let table = table(DEBUG_FRAME);
        assert_eq!(table.row_at(0x38 + 0x78), None);
        assert_eq!(table.row_at(0x200), None);
    }

    #[test]
    fn the_real_fixture_yields_one_fde_per_function() {
        let table = table(DEBUG_FRAME);
        assert_eq!(table.fde_count(), 2, "两个函数各一条 FDE");
        assert!(!table.is_empty());
    }

    #[test]
    fn the_body_row_wins_over_the_prologue_and_the_epilogue() {
        // helper：序言 `sp+0`（0x00..0x04）、`sp+48`（0x04..0x10）、
        // 函数体 `s0+0`（0x10..0x2c）、尾声又回到 `sp+48` / `sp+0`。
        assert_eq!(
            table(DEBUG_FRAME).body_cfa(0x00, 0x38),
            Some(CfaRule { reg: 8, offset: 0 }),
            "主行是函数体那一段（s0+0），不是序言"
        );
        assert_eq!(
            table(DEBUG_FRAME).body_cfa(0x38, 0x78),
            Some(CfaRule { reg: 8, offset: 0 })
        );
    }

    #[test]
    fn a_function_without_an_fde_has_no_answer() {
        let table = table(DEBUG_FRAME);
        // 区间比 FDE 大 ⇒ ⇒ 本模块不拿半个 FDE 硬凑。
        assert_eq!(table.body_cfa(0x00, 0x80), None);
        assert_eq!(table.body_cfa(0x100, 0x140), None);
    }

    #[test]
    fn a_missing_section_is_an_empty_table_not_an_error() {
        let table = CfiTable::from_sections(&[(".debug_info".to_string(), vec![0u8; 4])]);
        assert!(table.is_empty());
        assert_eq!(table.body_cfa(0, 0x10), None);
    }

    #[test]
    fn a_truncated_section_yields_whatever_parsed_before_the_damage() {
        // 切掉最后一个 FDE 的一半 ⇒ ⇒ 后半条放弃，前半条仍然可用。
        // 第一个 FDE 到 0x38，第二个 FDE 从 0x38 起 ⇒ ⇒ 切在 0x3c 正好
        // 把第二条切掉、第一条留着。
        let cut = &DEBUG_FRAME[..0x3c];
        let parsed = table(cut);
        assert_eq!(parsed.fde_count(), 1, "第一条 FDE 完整 ⇒ 保留");
        assert_eq!(
            parsed.body_cfa(0x00, 0x38),
            Some(CfaRule { reg: 8, offset: 0 })
        );
        // 畸形到连 CIE 都没有 ⇒ 空表，且不 panic。
        assert!(table(&[0xff, 0xff]).is_empty());
        assert!(table(&[]).is_empty());
    }

    #[test]
    fn an_sp_based_body_is_reported_verbatim() {
        // 造一条「函数体 CFA = sp+16」的 CIE+FDE（`-O2` / 无帧指针的产物
        // 就是这个形状）⇒ ⇒ 规则必须**逐字**报出来，不能被当成 s0。
        let mut bytes = Vec::new();
        // CIE：长度 0x0c、id = 0xffffffff、v3、aug=""、code_align=1、
        // data_align=-4、ra=1、def_cfa r2+0
        bytes.extend_from_slice(&[0x0c, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff]);
        bytes.extend_from_slice(&[0x03, 0x00, 0x01, 0x7c, 0x01, 0x0c, 0x02, 0x00]);
        // FDE：长度 0x10、cie=0、pc=0x100、range=0x40、
        // advance_loc 4 ⇒ def_cfa r2+16，advance_loc 0x3c ⇒ 尾声回到 r2+0
        bytes.extend_from_slice(&[0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x44, 0x0e, 0x10, 0x7c, 0x0e, 0x00]);
        let table = table(&bytes);
        assert_eq!(table.fde_count(), 1);
        assert_eq!(
            table.body_cfa(0x100, 0x140),
            Some(CfaRule { reg: 2, offset: 16 }),
            "sp+16 就是 sp+16，不许改成 s0"
        );
    }
}
