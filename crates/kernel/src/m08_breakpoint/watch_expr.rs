//! 观察表达式：**解析 + 寻址**（契约 v14）。
//!
//! 只负责「这段文本指向寄存器还是内存地址」；真正的内存读要走 M-17/M-14
//! 的总线（未映射地址在**求值期**报错，不在**设置期**报错）。

use std::fmt;

use riscv_sim_contract::model::SimState;

use super::names::{NameResolver, NoNames};

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
///
/// ⚠️ **不再是 `Copy`** —— 为了 [`AddressBase::Symbol`] 那个名字（`String`）。
/// 这是有意的：符号基址**只在解析期**存在一瞬（见 [`parse_address_with`]
/// 会把它换成 [`AddressBase::Number`]），但文法层要先表达得出来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressBase {
    /// 立即数地址。
    Number(u32),
    /// 通用寄存器。
    Register(u8),
    /// 当前 PC。
    Pc,
    /// **符号名**（`@main+4` / `[g_counter+2]`）：解析期换成
    /// [`AddressBase::Number`]。
    Symbol(String),
}

/// 内存地址 = 基址 + 有符号偏移。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressSpec {
    /// 基址。
    pub base: AddressBase,
    /// 偏移（可正可负）。
    pub offset: i32,
}

/// 表达式指向的东西。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchTarget {
    /// 通用寄存器。
    Register(u8),
    /// 当前 PC。
    Pc,
    /// 内存地址（读一个 word）。
    Memory(AddressSpec),
}

impl WatchTarget {
    /// 解析表达式（**没有符号上下文**）。
    ///
    /// ⚠️ 这条路径上出现符号名 ⇒ ⇒ 「查无此名」⇒ ⇒ 与宿主那条
    /// [`WatchTarget::parse_with`] 的区别**只在这里**。
    pub fn parse(expr: &str) -> Result<Self, WatchExprError> {
        Self::parse_with(expr, &NoNames)
    }

    /// 解析表达式，**带名字解析**（`@main+4` / `[g_counter+2]`）。
    pub fn parse_with(expr: &str, names: &dyn NameResolver) -> Result<Self, WatchExprError> {
        let text = expr.trim();
        if text.is_empty() {
            return Err(WatchExprError::new("空表达式"));
        }
        let memory = if let Some(inner) = text.strip_prefix('[') {
            let inner = inner
                .strip_suffix(']')
                .ok_or_else(|| WatchExprError::new(format!("缺少右括号：{text}")))?;
            Some(inner.trim())
        } else {
            text.strip_prefix('@').map(|inner| inner.trim())
        };
        if let Some(inner) = memory {
            return Ok(WatchTarget::Memory(parse_address_with(inner, names)?));
        }
        match parse_base_with(text, names) {
            Ok(AddressBase::Register(r)) => Ok(WatchTarget::Register(r)),
            Ok(AddressBase::Pc) => Ok(WatchTarget::Pc),
            Ok(AddressBase::Number(n)) => Ok(WatchTarget::Memory(AddressSpec {
                base: AddressBase::Number(n),
                offset: 0,
            })),
            // ⚠️ 裸符号名（`@` 省略的写法里**不会**出现——那是 `parse_base`
            //   只在 `[...]` / `@...` 的内层见到的东西）⇒ ⇒ 这里兜住
            //   「解析期的符号基址」这一格。
            Ok(AddressBase::Symbol(name)) => Ok(WatchTarget::Memory(AddressSpec {
                base: AddressBase::Symbol(name),
                offset: 0,
            })),
            Err(err) => Err(err),
        }
    }

    /// 解析出的内存地址（非内存表达式返回 `None`）。
    ///
    /// ⚠️ [`AddressBase::Symbol`] 在**解析期**就该被换成数字（见
    /// [`WatchTarget::parse_with`]）⇒ ⇒ 走到这里还剩符号基址就是内部
    /// 没绑定 ⇒ ⇒ 返回 `None`（界面显示「无法解析出观察地址」，不猜）。
    pub fn address(&self, state: &SimState) -> Option<u32> {
        let WatchTarget::Memory(spec) = self else {
            return None;
        };
        let base = match &spec.base {
            AddressBase::Number(n) => *n,
            AddressBase::Register(r) => *state.gpr.get(*r as usize)?,
            AddressBase::Pc => state.pc,
            AddressBase::Symbol(_) => return None,
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

/// 解析「基址 ± 偏移」的**语法**：符号名**保留**成
/// [`AddressBase::Symbol`]，不查任何表。
///
/// ⚠️ 条件文法用它（`[main+4]` 的方括号内层）—— 名字要等到**绑定**期才
/// 定得下来；观察表达式用它来「先认得形状、再定地址」。
pub fn parse_address_syntax(text: &str) -> Result<AddressSpec, WatchExprError> {
    let (base_text, offset) = split_address(text)?;
    let base = parse_base_syntax(base_text)?;
    Ok(AddressSpec { base, offset })
}

/// 解析「基址 ± 偏移」（**带名字解析**）。
pub fn parse_address_with(
    text: &str,
    names: &dyn NameResolver,
) -> Result<AddressSpec, WatchExprError> {
    let (base_text, offset) = split_address(text)?;
    let base = parse_base_with(base_text, names)?;
    Ok(AddressSpec { base, offset })
}

/// 把「基址 ± 偏移」拆成两段。
fn split_address(text: &str) -> Result<(&str, i32), WatchExprError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(WatchExprError::new("内存表达式缺少地址"));
    }
    let split = text
        .char_indices()
        .skip(1)
        .find(|(_, c)| *c == '+' || *c == '-');
    match split {
        Some((i, _)) => {
            let base = &text[..i];
            let off = &text[i..];
            let offset = off
                .parse::<i32>()
                .map_err(|_| WatchExprError::new(format!("偏移不是整数：{off}")))?;
            Ok((base, offset))
        }
        None => Ok((text, 0)),
    }
}

/// 解析一个基址记号，**符号名原样留住**。
fn parse_base_syntax(text: &str) -> Result<AddressBase, WatchExprError> {
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
    // ⚠️ 剩下的当**名字**（它到底是不是符号、是函数还是变量，等绑定/解析
    //   那一层定）⇒ ⇒ 这里只做形状判断：标识符形状才收。
    let shape_ok = t
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic() || c == '_')
        .unwrap_or(false)
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if shape_ok {
        return Ok(AddressBase::Symbol(t.to_string()));
    }
    Err(WatchExprError::new(format!("未知寄存器名或地址 \"{t}\"")))
}

/// 解析一个基址记号（**带名字解析**）。
///
/// 顺序：`pc` → 数字 → 寄存器 → 符号名 → 报错 ⇒ ⇒ 与条件文法同一条口径
/// （STAGE-55 表 9）。符号名**当场换成数字基址**，不把名字留到运行期。
fn parse_base_with(text: &str, names: &dyn NameResolver) -> Result<AddressBase, WatchExprError> {
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
    // ⚠️ **符号名**：解析期就把地址定下来（`@main+4` = `main` 的地址 + 4）。
    //
    // ⚠️ **局部变量不进地址文法**（批 D 的边界）：它的「地址」随帧变化，
    //    而地址文法的符号基址在**解析期**就落定（批 B 的口径）⇒ ⇒ 这里只收
    //    「有地址常量」的符号；局部变量在条件文法里直接用名字。
    if let Some(address) = names.resolve(t).and_then(|name| name.addr()) {
        return Ok(AddressBase::Number(address));
    }
    if names.resolve(t).is_some() {
        return Err(WatchExprError::new(format!(
            "\"{t}\" 是当前帧的局部变量（没有固定地址）⇒ ⇒ 地址表达式里请用寄存器或数字"
        )));
    }
    Err(WatchExprError::new(format!(
        "未知寄存器名或地址 \"{t}\"（也不是已加载的符号）"
    )))
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
        let mut state = SimState {
            pc: 0x1000,
            ..Default::default()
        };
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

    /// 一张只认 `main` 的假符号表。
    struct FakeNames;

    impl NameResolver for FakeNames {
        fn resolve(&self, name: &str) -> Option<super::super::names::NameRef> {
            (name == "main").then_some(super::super::names::NameRef {
                class: super::super::names::NameClass::Function,
                target: super::super::names::NameTarget::Addr(0x1000),
            })
        }
    }

    /// **符号基址**（`@main+4` / `[main-4]`）：解析期就换成数字。
    #[test]
    fn a_symbol_base_resolves_at_parse_time() {
        // ⚠️ **基址换成符号的地址、偏移原样留着** ⇒ ⇒ 求值期只剩下数字 /
        //    寄存器 / pc 三种基址 ⇒ ⇒ 不需要符号表。
        assert_eq!(
            WatchTarget::parse_with("@main+4", &FakeNames).unwrap(),
            WatchTarget::Memory(AddressSpec {
                base: AddressBase::Number(0x1000),
                offset: 4,
            })
        );
        let target = WatchTarget::parse_with("@main+4", &FakeNames).unwrap();
        assert_eq!(target.address(&state()), Some(0x1004));
        let target = WatchTarget::parse_with("[main-4]", &FakeNames).unwrap();
        assert_eq!(target.address(&state()), Some(0x0ffc));
    }

    /// 一张只认**局部变量** `x` 的假符号表（栈位置，没有固定地址）。
    struct LocalNames;

    impl NameResolver for LocalNames {
        fn resolve(&self, name: &str) -> Option<super::super::names::NameRef> {
            (name == "x").then_some(super::super::names::NameRef {
                class: super::super::names::NameClass::Local,
                target: super::super::names::NameTarget::Local(
                    super::super::names::LocalSlot::Frame {
                        base: 8,
                        offset: -24,
                    },
                ),
            })
        }
    }

    /// **地址文法不接局部变量**（批 D 的边界）：它的地址随帧变化，而符号
    /// 基址在解析期就落定 ⇒ ⇒ 拒，并指出该用寄存器 / 数字。
    #[test]
    fn a_local_variable_is_rejected_as_an_address_base() {
        for form in ["@x+4", "[x-4]"] {
            let err =
                WatchTarget::parse_with(form, &LocalNames).expect_err("局部变量不该进地址文法");
            assert!(
                err.message.contains("局部变量") && err.message.contains("寄存器或数字"),
                "{form} 的错误消息要说明原因与替代写法：{}",
                err.message
            );
        }
        // ⚠️ 但**寄存器**写法照旧可用（`x` 与 `x8` 撞名时按寄存器优先）。
        assert_eq!(
            WatchTarget::parse_with("@x8+4", &LocalNames).unwrap(),
            WatchTarget::Memory(AddressSpec {
                base: AddressBase::Register(8),
                offset: 4,
            }),
            "寄存器写法不受局部变量影响"
        );
    }

    /// **没有符号上下文**时符号名仍是错（消息里说清「也不是已加载的符号」）。
    #[test]
    fn a_symbol_base_without_names_is_rejected() {
        let err = WatchTarget::parse("@main+4").unwrap_err();
        assert!(err.message().contains("main"), "错误消息要指出哪个记号");
        assert!(
            err.message().contains("符号"),
            "错误消息要说明「也不是已加载的符号」：{}",
            err.message()
        );
    }

    #[test]
    fn rejects_unknown_tokens() {
        let err = WatchTarget::parse("foo").unwrap_err();
        assert!(err.message().contains("foo"), "错误消息要指出哪个记号");
    }
}
