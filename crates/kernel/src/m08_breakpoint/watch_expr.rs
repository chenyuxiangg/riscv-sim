//! 观察表达式：**解析 + 寻址**（契约 v14）。
//!
//! 只负责「这段文本指向寄存器还是内存地址」；真正的内存读要走 M-17/M-14
//! 的总线（未映射地址在**求值期**报错，不在**设置期**报错）。

use std::fmt;

use riscv_sim_contract::model::SimState;

/// 观察表达式的解析失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchExprError {
    message: String,
}

impl WatchExprError {
    fn new(detail: impl fmt::Display) -> Self {
        Self {
            message: format!("观察表达式无效：{detail}"),
        }
    }

    /// 完整错误消息。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for WatchExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WatchExprError {}

/// 地址基址。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressBase {
    /// 立即数地址。
    Number(u32),
    /// 通用寄存器。
    Register(u8),
    /// 当前 PC。
    Pc,
}

/// 内存地址 = 基址 + 有符号偏移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressSpec {
    /// 基址。
    pub base: AddressBase,
    /// 偏移（可正可负）。
    pub offset: i32,
}

/// 表达式指向的东西。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchTarget {
    /// 通用寄存器。
    Register(u8),
    /// 当前 PC。
    Pc,
    /// 内存地址（读一个 word）。
    Memory(AddressSpec),
}

impl WatchTarget {
    /// 解析表达式。
    pub fn parse(expr: &str) -> Result<Self, WatchExprError> {
        let text = expr.trim();
        if text.is_empty() {
            return Err(WatchExprError::new("空表达式"));
        }
        let memory = if let Some(inner) = text.strip_prefix('[') {
            let inner = inner
                .strip_suffix(']')
                .ok_or_else(|| WatchExprError::new(format!("缺少右括号：{text}")))?;
            Some(inner.trim())
        } else if let Some(inner) = text.strip_prefix('@') {
            Some(inner.trim())
        } else {
            None
        };
        if let Some(inner) = memory {
            return Ok(WatchTarget::Memory(parse_address(inner)?));
        }
        match parse_base(text) {
            Ok(AddressBase::Register(r)) => Ok(WatchTarget::Register(r)),
            Ok(AddressBase::Pc) => Ok(WatchTarget::Pc),
            Ok(AddressBase::Number(n)) => Ok(WatchTarget::Memory(AddressSpec {
                base: AddressBase::Number(n),
                offset: 0,
            })),
            Err(err) => Err(err),
        }
    }

    /// 解析出的内存地址（非内存表达式返回 `None`）。
    pub fn address(&self, state: &SimState) -> Option<u32> {
        let WatchTarget::Memory(spec) = self else {
            return None;
        };
        let base = match spec.base {
            AddressBase::Number(n) => n,
            AddressBase::Register(r) => *state.gpr.get(r as usize)?,
            AddressBase::Pc => state.pc,
        };
        Some(base.wrapping_add(spec.offset as u32))
    }

    /// 寄存器 / PC 的值（内存表达式返回 `None`）。
    pub fn value(&self, state: &SimState) -> Option<u32> {
        match self {
            WatchTarget::Register(r) => state.gpr.get(*r as usize).copied(),
            WatchTarget::Pc => Some(state.pc),
            WatchTarget::Memory(_) => None,
        }
    }
}

/// 解析「基址 ± 偏移」。
fn parse_address(text: &str) -> Result<AddressSpec, WatchExprError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(WatchExprError::new("内存表达式缺少地址"));
    }
    let split = text
        .char_indices()
        .skip(1)
        .find(|(_, c)| *c == '+' || *c == '-');
    let (base_text, offset) = match split {
        Some((i, c)) => {
            let base = &text[..i];
            let off = &text[i..];
            let offset = off
                .parse::<i32>()
                .map_err(|_| WatchExprError::new(format!("偏移不是整数：{off}")))?;
            (base, offset)
        }
        None => (text, 0),
    };
    let base = match parse_base(base_text)? {
        AddressBase::Number(n) => AddressBase::Number(n),
        AddressBase::Register(r) => AddressBase::Register(r),
        AddressBase::Pc => AddressBase::Pc,
    };
    Ok(AddressSpec { base, offset })
}

/// 解析一个基址记号。
fn parse_base(text: &str) -> Result<AddressBase, WatchExprError> {
    let t = text.trim().trim_start_matches('$');
    if t.is_empty() {
        return Err(WatchExprError::new("缺少基址"));
    }
    if t.eq_ignore_ascii_case("pc") {
        return Ok(AddressBase::Pc);
    }
    if let Some(n) = parse_number(t) {
        return Ok(AddressBase::Number(n));
    }
    if let Some(r) = parse_register(t) {
        return Ok(AddressBase::Register(r));
    }
    Err(WatchExprError::new(format!("未知寄存器名或地址 \"{t}\"")))
}

fn parse_number(text: &str) -> Option<u32> {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).ok();
    }
    t.parse::<u32>().ok()
}

fn parse_register(text: &str) -> Option<u8> {
    let t = text.trim().to_ascii_lowercase();
    if let Some(num) = t.strip_prefix('x') {
        if let Ok(n) = num.parse::<u8>() {
            return (n < 32).then_some(n);
        }
    }
    let idx = match t.as_str() {
        "zero" => 0,
        "ra" => 1,
        "sp" => 2,
        "gp" => 3,
        "tp" => 4,
        "t0" => 5,
        "t1" => 6,
        "t2" => 7,
        "s0" | "fp" => 8,
        "s1" => 9,
        "a0" => 10,
        "a1" => 11,
        "a2" => 12,
        "a3" => 13,
        "a4" => 14,
        "a5" => 15,
        "a6" => 16,
        "a7" => 17,
        "s2" => 18,
        "s3" => 19,
        "s4" => 20,
        "s5" => 21,
        "s6" => 22,
        "s7" => 23,
        "s8" => 24,
        "s9" => 25,
        "s10" => 26,
        "s11" => 27,
        "t3" => 28,
        "t4" => 29,
        "t5" => 30,
        "t6" => 31,
        _ => return None,
    };
    Some(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SimState {
        let mut state = SimState::default();
        state.pc = 0x1000;
        state.gpr[1] = 0x8000;
        state.gpr[2] = 0x9000;
        state
    }

    #[test]
    fn parses_pc_and_registers() {
        assert_eq!(WatchTarget::parse("pc").unwrap(), WatchTarget::Pc);
        assert_eq!(WatchTarget::parse("$pc").unwrap(), WatchTarget::Pc);
        assert_eq!(WatchTarget::parse("ra").unwrap(), WatchTarget::Register(1));
        assert_eq!(WatchTarget::parse("$x2").unwrap(), WatchTarget::Register(2));
        assert_eq!(
            WatchTarget::parse("x31").unwrap(),
            WatchTarget::Register(31)
        );
    }

    #[test]
    fn parses_bare_address_and_memory_forms() {
        assert_eq!(
            WatchTarget::parse("0x2000").unwrap(),
            WatchTarget::Memory(AddressSpec {
                base: AddressBase::Number(0x2000),
                offset: 0
            })
        );
        assert_eq!(
            WatchTarget::parse("@sp+4").unwrap(),
            WatchTarget::Memory(AddressSpec {
                base: AddressBase::Register(2),
                offset: 4
            })
        );
        assert_eq!(
            WatchTarget::parse("[pc-4]").unwrap(),
            WatchTarget::Memory(AddressSpec {
                base: AddressBase::Pc,
                offset: -4
            })
        );
    }

    #[test]
    fn resolves_addresses() {
        let s = state();
        let reg = WatchTarget::parse("ra").unwrap();
        assert_eq!(reg.address(&s), None, "寄存器不是地址");
        let mem = WatchTarget::parse("@sp+4").unwrap();
        assert_eq!(mem.address(&s), Some(0x9004));
        let mem = WatchTarget::parse("[pc-4]").unwrap();
        assert_eq!(mem.address(&s), Some(0x0ffc));
    }

    #[test]
    fn rejects_unknown_tokens() {
        let err = WatchTarget::parse("foo").unwrap_err();
        assert!(err.message().contains("foo"), "错误消息要指出哪个记号");
    }
}
