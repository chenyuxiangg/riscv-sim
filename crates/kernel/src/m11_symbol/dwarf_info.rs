//! Minimal `.debug_info` / `.debug_abbrev` reader for `DW_AT_comp_dir`.
//!
//! The line-table parser deliberately does **not** materialise DIEs (裁决 B):
//! the only thing it needs from `.debug_info` is the compile directory that
//! v2–v4 line headers refer to through directory index 0. This module reads
//! exactly that pair of attributes (`DW_AT_stmt_list` + `DW_AT_comp_dir`) and
//! returns a `stmt_list → comp_dir` map. Any structural surprise is treated as
//! "no comp_dir" (best effort) — the line table itself must still load.

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
    let mut out = HashMap::new();
    let mut abbrev_cache: HashMap<usize, Option<HashMap<u64, Abbrev>>> = HashMap::new();
    let mut off = 0usize;
    while off < debug_info.len() {
        let unit = match parse_unit(
            debug_info,
            off,
            debug_abbrev,
            debug_str,
            line_str,
            &mut abbrev_cache,
        ) {
            Ok(unit) => unit,
            Err(()) => break,
        };
        if let (Some(stmt), Some(dir)) = (unit.stmt_list, unit.comp_dir) {
            if !dir.is_empty() {
                out.insert(stmt, dir);
            }
        }
        if unit.next_off <= off {
            break;
        }
        off = unit.next_off;
    }
    out
}

struct ParsedUnit {
    next_off: usize,
    stmt_list: Option<u64>,
    comp_dir: Option<String>,
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

enum Value {
    Num(u64),
    Str(String),
}

fn parse_unit(
    bytes: &[u8],
    start: usize,
    abbrev_bytes: &[u8],
    debug_str: &[u8],
    line_str: &[u8],
    abbrev_cache: &mut HashMap<usize, Option<HashMap<u64, Abbrev>>>,
) -> Result<ParsedUnit, ()> {
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

    let mut stmt_list = None;
    let mut comp_dir = None;
    let mut depth = 0i32;
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
        let is_cu = depth == 0 && abbrev.tag == DW_TAG_COMPILE_UNIT;
        for spec in &abbrev.attrs {
            let value = read_value(&mut cur, spec, offset_size, debug_str, line_str)?;
            if is_cu {
                match spec.attr {
                    DW_AT_STMT_LIST => {
                        if let Value::Num(v) = value {
                            stmt_list = Some(v);
                        }
                    }
                    DW_AT_COMP_DIR => {
                        if let Value::Str(v) = value {
                            comp_dir = Some(v);
                        }
                    }
                    _ => {}
                }
            }
        }
        if abbrev.children {
            depth += 1;
        }
    }

    Ok(ParsedUnit {
        next_off: unit_end,
        stmt_list,
        comp_dir,
    })
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
        DW_FORM_BLOCK2 => {
            let len = cur.u16()? as usize;
            cur.skip(len)?;
            Ok(Value::Num(0))
        }
        DW_FORM_BLOCK4 => {
            let len = cur.u32()? as usize;
            cur.skip(len)?;
            Ok(Value::Num(0))
        }
        DW_FORM_BLOCK1 => {
            let len = cur.u8()? as usize;
            cur.skip(len)?;
            Ok(Value::Num(0))
        }
        DW_FORM_BLOCK | DW_FORM_EXPRLOC => {
            let len = cur.uleb()? as usize;
            cur.skip(len)?;
            Ok(Value::Num(0))
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
