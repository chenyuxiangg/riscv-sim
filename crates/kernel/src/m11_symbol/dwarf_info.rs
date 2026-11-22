//! Minimal `.debug_info` / `.debug_abbrev` reader.
//!
//! 两个用途：
//!
//! 1. **`DW_AT_comp_dir`**（STAGE-7 起）：行表解析**不物化 DIE 树**（裁决 B），
//!    只从 CU 里取 `DW_AT_stmt_list` + `DW_AT_comp_dir`，行头里写的是相对路径
//!    时用它拼成**绝对路径**。
//! 2. **DIE 走查**（STAGE-55 批 D 起）：当前帧局部变量要 `DW_TAG_subprogram`
//!    的 `low_pc` / `high_pc` / `frame_base`，以及 `DW_TAG_variable` /
//!    `DW_TAG_formal_parameter` 的 `name` / `location`（`DW_OP_fbreg` /
//!    `DW_OP_regN` 的**字节块**）。
//!
//! 两次用途共用同一套 abbrev / form 解析 ⇒ ⇒ 只留一个**通用的 DIE 走查**
//! （[`walk_dies`]），上面再挂两个薄适配器。结构上的任何意外都不当错误：
//! 调用方拿到的只会是「少了一些 DIE」，不会让行表 / 符号表一起挂掉。

use std::collections::HashMap;

const DW_TAG_COMPILE_UNIT: u64 = 0x11;
const DW_AT_STMT_LIST: u64 = 0x10;
const DW_AT_COMP_DIR: u64 = 0x1b;

const DW_FORM_ADDR: u64 = 0x01;
const DW_FORM_BLOCK2: u64 = 0x03;
const DW_FORM_BLOCK4: u64 = 0x04;
const DW_FORM_DATA2: u64 = 0x05;
const DW_FORM_DATA4: u64 = 0x06;
const DW_FORM_DATA8: u64 = 0x07;
const DW_FORM_STRING: u64 = 0x08;
const DW_FORM_BLOCK: u64 = 0x09;
const DW_FORM_BLOCK1: u64 = 0x0a;
const DW_FORM_DATA1: u64 = 0x0b;
const DW_FORM_FLAG: u64 = 0x0c;
const DW_FORM_SDATA: u64 = 0x0d;
const DW_FORM_STRP: u64 = 0x0e;
const DW_FORM_UDATA: u64 = 0x0f;
const DW_FORM_REF_ADDR: u64 = 0x10;
const DW_FORM_REF1: u64 = 0x11;
const DW_FORM_REF2: u64 = 0x12;
const DW_FORM_REF4: u64 = 0x13;
const DW_FORM_REF8: u64 = 0x14;
const DW_FORM_REF_UDATA: u64 = 0x15;
const DW_FORM_INDIRECT: u64 = 0x16;
const DW_FORM_SEC_OFFSET: u64 = 0x17;
const DW_FORM_EXPRLOC: u64 = 0x18;
const DW_FORM_FLAG_PRESENT: u64 = 0x19;
const DW_FORM_STRX: u64 = 0x1a;
const DW_FORM_ADDRX: u64 = 0x1b;
const DW_FORM_REF_SUP4: u64 = 0x1c;
const DW_FORM_STRP_SUP: u64 = 0x1d;
const DW_FORM_DATA16: u64 = 0x1e;
const DW_FORM_LINE_STRP: u64 = 0x1f;
const DW_FORM_REF_SIG8: u64 = 0x20;
const DW_FORM_IMPLICIT_CONST: u64 = 0x21;
const DW_FORM_LOCLISTX: u64 = 0x22;
const DW_FORM_RNGLISTX: u64 = 0x23;
const DW_FORM_REF_SUP8: u64 = 0x24;
const DW_FORM_STRX1: u64 = 0x25;
const DW_FORM_STRX2: u64 = 0x26;
const DW_FORM_STRX3: u64 = 0x27;
const DW_FORM_STRX4: u64 = 0x28;
const DW_FORM_ADDRX1: u64 = 0x29;
const DW_FORM_ADDRX2: u64 = 0x2a;
const DW_FORM_ADDRX3: u64 = 0x2b;
const DW_FORM_ADDRX4: u64 = 0x2c;
const DW_FORM_REF_SUP1: u64 = 0x2d;

/// 一个属性的**原始值**。
///
/// ⚠️ 块 / `exprloc` **保留字节**（批 D 起要读 `DW_OP_fbreg` 的偏移）——
/// 以前这里直接丢掉只返回 `Num(0)`，局部变量就没法定位了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    /// 数字（地址 / 长度 / 索引 / 标志）。
    Num(u64),
    /// 字符串（`.debug_str` 或 `.debug_line_str` 里取的）。
    Str(String),
    /// 块 / `exprloc` 的原始字节（DWARF 表达式）。
    Bytes(Vec<u8>),
}

/// 一个 DIE 的属性集合（按 `DW_AT` 编号查）。
#[derive(Debug, Default, Clone)]
pub(crate) struct DieAttrs {
    entries: Vec<(u64, u64, Value)>,
}

impl DieAttrs {
    /// 数字属性（`Num` 以外的形态一律当「没有」）。
    pub(crate) fn num(&self, attr: u64) -> Option<u64> {
        match self.get(attr)? {
            Value::Num(v) => Some(*v),
            _ => None,
        }
    }

    /// 字符串属性。
    pub(crate) fn text(&self, attr: u64) -> Option<&str> {
        match self.get(attr)? {
            Value::Str(v) => Some(v.as_str()),
            _ => None,
        }
    }

    /// 块 / `exprloc` 属性的字节。
    pub(crate) fn bytes(&self, attr: u64) -> Option<&[u8]> {
        match self.get(attr)? {
            Value::Bytes(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    /// 这个属性用的是**哪个 `DW_FORM`**（`DW_AT_high_pc` 要靠它区分
    /// 「绝对结束地址」和「长度」）。
    pub(crate) fn form(&self, attr: u64) -> Option<u64> {
        self.entries
            .iter()
            .find(|(key, _, _)| *key == attr)
            .map(|(_, form, _)| *form)
    }

    fn get(&self, attr: u64) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(key, _, _)| *key == attr)
            .map(|(_, _, value)| value)
    }
}

/// **走一遍所有单元的 DIE**，每个 DIE 调一次 `sink(depth, tag, attrs)`。
///
/// * `depth` —— 这个 DIE 在树里的深度（CU 是 `0`）；
/// * 畸形单元 / 畸形 abbrev 表 ⇒ **跳过整个单元**（走查继续往下），
///   不返回错误：调用方要的是「能解析多少算多少」。
pub(crate) fn walk_dies(
    debug_info: &[u8],
    debug_abbrev: &[u8],
    debug_str: &[u8],
    line_str: &[u8],
    sink: &mut dyn FnMut(usize, u64, &DieAttrs),
) {
    let mut abbrev_cache: HashMap<usize, Option<HashMap<u64, Abbrev>>> = HashMap::new();
    let mut off = 0usize;
    while off < debug_info.len() {
        let next = match walk_unit(
            debug_info,
            off,
            debug_abbrev,
            debug_str,
            line_str,
            &mut abbrev_cache,
            sink,
        ) {
            Ok(next) => next,
            Err(()) => break,
        };
        if next <= off {
            break;
        }
        off = next;
    }
}

/// Build `DW_AT_stmt_list` → `DW_AT_comp_dir` from the raw debug sections.
///
/// Errors are intentionally **not** exposed: an unreadable `.debug_info`
/// means "no compile directory", never "the line table is malformed".
pub(crate) fn comp_dirs(
    debug_info: &[u8],
    debug_abbrev: &[u8],
    debug_str: &[u8],
    line_str: &[u8],
) -> HashMap<u64, String> {
    // 一个单元一个单元地收：`stmt_list` / `comp_dir` 是 CU 自己的属性。
    let mut out = HashMap::new();
    let mut stmt_list: Option<u64> = None;
    let mut comp_dir: Option<String> = None;
    walk_dies(
        debug_info,
        debug_abbrev,
        debug_str,
        line_str,
        &mut |depth, tag, attrs| {
            if depth != 0 || tag != DW_TAG_COMPILE_UNIT {
                return;
            }
            // 新单元开始 ⇒ 上一个单元的配对先结算。
            if let (Some(stmt), Some(dir)) = (stmt_list.take(), comp_dir.take()) {
                if !dir.is_empty() {
                    out.insert(stmt, dir);
                }
            }
            stmt_list = attrs.num(DW_AT_STMT_LIST);
            comp_dir = attrs.text(DW_AT_COMP_DIR).map(str::to_string);
        },
    );
    if let (Some(stmt), Some(dir)) = (stmt_list, comp_dir) {
        if !dir.is_empty() {
            out.insert(stmt, dir);
        }
    }
    out
}

struct Abbrev {
    tag: u64,
    children: bool,
    attrs: Vec<AttrSpec>,
}

struct AttrSpec {
    attr: u64,
    form: u64,
    implicit_const: i64,
}

fn walk_unit(
    bytes: &[u8],
    start: usize,
    abbrev_bytes: &[u8],
    debug_str: &[u8],
    line_str: &[u8],
    abbrev_cache: &mut HashMap<usize, Option<HashMap<u64, Abbrev>>>,
    sink: &mut dyn FnMut(usize, u64, &DieAttrs),
) -> Result<usize, ()> {
    let (offset_size, unit_len) = initial_length(bytes, start)?;
    let unit_end = (start as u64)
        .checked_add(offset_size as u64)
        .and_then(|v| v.checked_add(unit_len))
        .filter(|v| *v <= bytes.len() as u64)
        .ok_or(())? as usize;
    let mut cur = Cursor::new(bytes, start + offset_size, unit_end);
    let version = cur.u16()?;
    let abbrev_offset = if version >= 5 {
        let _unit_type = cur.u8()?;
        let _address_size = cur.u8()?;
        cur.offset(offset_size)?
    } else {
        let abbrev_offset = cur.offset(offset_size)?;
        let _address_size = cur.u8()?;
        abbrev_offset
    } as usize;
    let table = abbrev_cache
        .entry(abbrev_offset)
        .or_insert_with(|| parse_abbrev_table(abbrev_bytes, abbrev_offset).ok())
        .as_ref()
        .ok_or(())?;

    let mut depth = 0usize;
    loop {
        if cur.pos >= unit_end {
            break;
        }
        let code = cur.uleb()?;
        if code == 0 {
            if depth == 0 {
                break;
            }
            depth -= 1;
            continue;
        }
        let abbrev = table.get(&code).ok_or(())?;
        let mut attrs = DieAttrs::default();
        for spec in &abbrev.attrs {
            let value = read_value(&mut cur, spec, offset_size, debug_str, line_str)?;
            attrs.entries.push((spec.attr, spec.form, value));
        }
        sink(depth, abbrev.tag, &attrs);
        if abbrev.children {
            depth += 1;
        }
    }

    Ok(unit_end)
}

fn initial_length(bytes: &[u8], start: usize) -> Result<(usize, u64), ()> {
    let prefix = read_u32(bytes, start)?;
    if prefix == 0xffff_ffff {
        let len = read_u64(bytes, start + 4)?;
        Ok((12, len))
    } else {
        Ok((4, u64::from(prefix)))
    }
}

fn parse_abbrev_table(bytes: &[u8], offset: usize) -> Result<HashMap<u64, Abbrev>, ()> {
    let mut cur = Cursor::new(bytes, offset, bytes.len());
    let mut table = HashMap::new();
    loop {
        let code = cur.uleb()?;
        if code == 0 {
            break;
        }
        let tag = cur.uleb()?;
        let children = cur.u8()? != 0;
        let mut attrs = Vec::new();
        loop {
            let attr = cur.uleb()?;
            let form = cur.uleb()?;
            if attr == 0 && form == 0 {
                break;
            }
            let implicit_const = if form == DW_FORM_IMPLICIT_CONST {
                cur.sleb()?
            } else {
                0
            };
            attrs.push(AttrSpec {
                attr,
                form,
                implicit_const,
            });
        }
        table.insert(
            code,
            Abbrev {
                tag,
                children,
                attrs,
            },
        );
    }
    Ok(table)
}

fn read_value(
    cur: &mut Cursor<'_>,
    spec: &AttrSpec,
    offset_size: usize,
    debug_str: &[u8],
    line_str: &[u8],
) -> Result<Value, ()> {
    read_form(
        cur,
        spec.form,
        offset_size,
        debug_str,
        line_str,
        spec.implicit_const,
    )
}

fn read_form(
    cur: &mut Cursor<'_>,
    form: u64,
    offset_size: usize,
    debug_str: &[u8],
    line_str: &[u8],
    implicit_const: i64,
) -> Result<Value, ()> {
    match form {
        DW_FORM_ADDR => Ok(Value::Num(cur.offset(offset_size)?)),
        DW_FORM_DATA1 | DW_FORM_FLAG | DW_FORM_REF1 | DW_FORM_REF_SUP1 => {
            Ok(Value::Num(u64::from(cur.u8()?)))
        }
        DW_FORM_DATA2 | DW_FORM_REF2 => Ok(Value::Num(u64::from(cur.u16()?))),
        DW_FORM_DATA4 | DW_FORM_REF4 | DW_FORM_REF_SUP4 => Ok(Value::Num(u64::from(cur.u32()?))),
        DW_FORM_DATA8 | DW_FORM_REF8 | DW_FORM_REF_SIG8 | DW_FORM_REF_SUP8 => {
            Ok(Value::Num(cur.u64()?))
        }
        DW_FORM_STRING => Ok(Value::Str(cur.cstring()?.to_string())),
        DW_FORM_STRP | DW_FORM_STRP_SUP => {
            let off = cur.offset(offset_size)? as usize;
            Ok(Value::Str(cstring_at(debug_str, off)?.to_string()))
        }
        DW_FORM_LINE_STRP => {
            let off = cur.offset(offset_size)? as usize;
            Ok(Value::Str(cstring_at(line_str, off)?.to_string()))
        }
        DW_FORM_UDATA | DW_FORM_REF_UDATA | DW_FORM_LOCLISTX | DW_FORM_RNGLISTX => {
            Ok(Value::Num(cur.uleb()?))
        }
        DW_FORM_SDATA => Ok(Value::Num(cur.sleb()? as u64)),
        DW_FORM_SEC_OFFSET | DW_FORM_REF_ADDR => Ok(Value::Num(cur.offset(offset_size)?)),
        DW_FORM_FLAG_PRESENT => Ok(Value::Num(1)),
        DW_FORM_IMPLICIT_CONST => Ok(Value::Num(implicit_const as u64)),
        // ⚠️ 块 / `exprloc`：**留字节**（批 D 的 `DW_OP_fbreg` / `DW_OP_regN`
        //    就住在里面）。
        DW_FORM_BLOCK2 => {
            let len = cur.u16()? as usize;
            Ok(Value::Bytes(cur.take(len)?.to_vec()))
        }
        DW_FORM_BLOCK4 => {
            let len = cur.u32()? as usize;
            Ok(Value::Bytes(cur.take(len)?.to_vec()))
        }
        DW_FORM_BLOCK1 => {
            let len = cur.u8()? as usize;
            Ok(Value::Bytes(cur.take(len)?.to_vec()))
        }
        DW_FORM_BLOCK | DW_FORM_EXPRLOC => {
            let len = cur.uleb()? as usize;
            Ok(Value::Bytes(cur.take(len)?.to_vec()))
        }
        DW_FORM_DATA16 => {
            cur.skip(16)?;
            Ok(Value::Num(0))
        }
        DW_FORM_STRX1 | DW_FORM_ADDRX1 => {
            cur.skip(1)?;
            Ok(Value::Num(0))
        }
        DW_FORM_STRX2 | DW_FORM_ADDRX2 => {
            cur.skip(2)?;
            Ok(Value::Num(0))
        }
        DW_FORM_STRX3 | DW_FORM_ADDRX3 => {
            cur.skip(3)?;
            Ok(Value::Num(0))
        }
        DW_FORM_STRX4 | DW_FORM_ADDRX4 => {
            cur.skip(4)?;
            Ok(Value::Num(0))
        }
        DW_FORM_STRX | DW_FORM_ADDRX => {
            cur.uleb()?;
            Ok(Value::Num(0))
        }
        DW_FORM_INDIRECT => {
            let inner = cur.uleb()?;
            read_form(cur, inner, offset_size, debug_str, line_str, implicit_const)
        }
        _ => Err(()),
    }
}

fn cstring_at(bytes: &[u8], offset: usize) -> Result<&str, ()> {
    let rest = bytes.get(offset..).ok_or(())?;
    let end = rest.iter().position(|b| *b == 0).ok_or(())?;
    std::str::from_utf8(&rest[..end]).map_err(|_| ())
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, ()> {
    let chunk = bytes.get(at..at + 4).ok_or(())?;
    Ok(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

fn read_u64(bytes: &[u8], at: usize) -> Result<u64, ()> {
    let chunk = bytes.get(at..at + 8).ok_or(())?;
    let mut b = [0u8; 8];
    b.copy_from_slice(chunk);
    Ok(u64::from_le_bytes(b))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
    end: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], pos: usize, end: usize) -> Self {
        Self { bytes, pos, end }
    }

    /// 取 `len` 字节，同时检查**单元边界**（不是只检查节区边界）⇒
    /// 畸形 `.debug_info` 不会读到下一个单元里去。
    fn take(&mut self, len: usize) -> Result<&'a [u8], ()> {
        let start = self.pos;
        let next = start.checked_add(len).ok_or(())?;
        if next > self.end || next > self.bytes.len() {
            return Err(());
        }
        let chunk = self.bytes.get(start..next).ok_or(())?;
        self.pos = next;
        Ok(chunk)
    }

    fn u8(&mut self) -> Result<u8, ()> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ()> {
        let chunk = self.take(2)?;
        Ok(u16::from_le_bytes([chunk[0], chunk[1]]))
    }

    fn u32(&mut self) -> Result<u32, ()> {
        let chunk = self.take(4)?;
        Ok(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
    }

    fn u64(&mut self) -> Result<u64, ()> {
        let chunk = self.take(8)?;
        let mut b = [0u8; 8];
        b.copy_from_slice(chunk);
        Ok(u64::from_le_bytes(b))
    }

    fn offset(&mut self, size: usize) -> Result<u64, ()> {
        match size {
            4 => Ok(u64::from(self.u32()?)),
            8 => self.u64(),
            _ => Err(()),
        }
    }

    fn skip(&mut self, len: usize) -> Result<(), ()> {
        let next = self.pos.checked_add(len).ok_or(())?;
        if next > self.end || next > self.bytes.len() {
            return Err(());
        }
        self.pos = next;
        Ok(())
    }

    fn uleb(&mut self) -> Result<u64, ()> {
        let mut result = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = self.u8()?;
            if shift >= 64 {
                return Err(());
            }
            result |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
        }
    }

    fn sleb(&mut self) -> Result<i64, ()> {
        let mut result = 0i64;
        let mut shift = 0u32;
        loop {
            let byte = self.u8()?;
            if shift >= 64 {
                return Err(());
            }
            result |= i64::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                if shift < 64 && (byte & 0x40) != 0 {
                    result |= !0i64 << shift;
                }
                return Ok(result);
            }
        }
    }

    fn cstring(&mut self) -> Result<&'a str, ()> {
        let rest = self.bytes.get(self.pos..self.end).ok_or(())?;
        let end = rest.iter().position(|b| *b == 0).ok_or(())?;
        let value = std::str::from_utf8(&rest[..end]).map_err(|_| ())?;
        self.pos += end + 1;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个最小 v4 单元：CU（带 `stmt_list` + `comp_dir`）+ 子 DIE（带一块
    /// `exprloc`）。字节是**手搓**的，只为钉住「块字节不再被丢掉」。
    fn tiny_unit() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        // .debug_abbrev：1 = CU（children, stmt_list DATA4, comp_dir STRP），
        //                2 = 子 DIE（children=false, location EXPRLOC）
        let mut abbrev = Vec::new();
        for (code, tag, children, attrs) in [
            (1u64, 0x11u64, true, vec![(0x10u64, 0x06u64), (0x1b, 0x0e)]),
            (2, 0x34, false, vec![(0x02, 0x18)]),
        ] {
            abbrev.extend(uleb(code));
            abbrev.extend(uleb(tag));
            abbrev.push(if children { 1 } else { 0 });
            for (attr, form) in attrs {
                abbrev.extend(uleb(attr));
                abbrev.extend(uleb(form));
            }
            abbrev.extend([0, 0]);
        }
        abbrev.push(0);

        let debug_str = b"/tmp/build\0".to_vec();
        // .debug_info（v4）：长度 / 版本 / abbrev 偏移 / 地址宽度 + DIE
        let mut body = Vec::new();
        body.extend([0x04, 0x00]); // version 4
        body.extend([0x00, 0x00, 0x00, 0x00]); // abbrev offset
        body.extend([0x04]); // address size
        body.extend(uleb(1)); // CU
        body.extend([0x34, 0x12, 0x00, 0x00]); // stmt_list = 0x1234
        body.extend([0x00, 0x00, 0x00, 0x00]); // comp_dir = 0
        body.extend(uleb(2)); // 子 DIE
        body.extend([0x02, 0x91, 0x68]); // exprloc: len=2, fbreg -24
        body.push(0); // 子 DIE 结束
        let mut info = Vec::new();
        info.extend((body.len() as u32).to_le_bytes());
        info.extend(body);
        (info, abbrev, debug_str)
    }

    fn uleb(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let low = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(low);
                return out;
            }
            out.push(low | 0x80);
        }
    }

    #[test]
    fn comp_dirs_still_works_through_the_generic_walk() {
        let (info, abbrev, strings) = tiny_unit();
        let map = comp_dirs(&info, &abbrev, &strings, &[]);
        assert_eq!(map.get(&0x1234).map(String::as_str), Some("/tmp/build"));
    }

    #[test]
    fn a_block_attribute_keeps_its_bytes() {
        let (info, abbrev, strings) = tiny_unit();
        let mut seen: Vec<(usize, u64, Vec<u8>)> = Vec::new();
        walk_dies(&info, &abbrev, &strings, &[], &mut |depth, tag, attrs| {
            if let Some(bytes) = attrs.bytes(0x02) {
                seen.push((depth, tag, bytes.to_vec()));
            }
        });
        assert_eq!(
            seen,
            vec![(1, 0x34, vec![0x91, 0x68])],
            "块的字节必须原样留下"
        );
    }

    #[test]
    fn a_malformed_unit_yields_no_second_unit() {
        // 长度字段说「有 9 个字节」，但里面只有 2 个 ⇒ 单元畸形 ⇒ 走查停，
        // 不 panic、不越界读。
        let info = vec![0x09, 0x00, 0x00, 0x00, 0x04, 0x00];
        let mut hits = 0;
        walk_dies(&info, &[], &[], &[], &mut |_, _, _| hits += 1);
        assert_eq!(hits, 0);
    }
}
