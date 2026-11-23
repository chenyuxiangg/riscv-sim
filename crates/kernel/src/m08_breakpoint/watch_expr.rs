//! 观察表达式：**目标 + 取值**（契约 v14）。
//!
//! ⚠️ **地址文法不在本模块**：`@x2+4` / `[main-4]` 的解析原先在这里，但
//! 「文本 → 地址」这一半只有条件文法在用 ⇒ ⇒ v0.137 起并回
//! [`super::cond::addr`]，地址解析只剩一个落点。本模块只留：
//!
//! 1. **这条表达式指向什么**（寄存器 / pc / 内存）；
//! 2. **从 `SimState` 里把值取出来**（内存值仍走 M-17 / M-14 的总线；
//!    未映射地址在**求值期**报错，不在**设置期**报错）。
//!
//! ⚠️ 契约 v14 的 `SetWatchExpr` / `ClearWatchExpr` / `ListWatchExprs` 与
//!    `WatchValues` 仍在，但界面已于 v0.136 下线 ⇒ ⇒ 本模块目前是**薄壳**
//!    （没有界面在用）；彻底删契约变体那一批会把它一起删掉。

use riscv_sim_contract::model::SimState;

use super::cond::addr::{
    parse_address_with, parse_base_with, AddrParseError, AddressBase, AddressSpec,
};
use super::names::{NameResolver, NoNames};

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
    pub fn parse(expr: &str) -> Result<Self, AddrParseError> {
        Self::parse_with(expr, &NoNames)
    }

    /// 解析表达式，**带名字解析**（`@main+4` / `[g_counter+2]`）。
    pub fn parse_with(expr: &str, names: &dyn NameResolver) -> Result<Self, AddrParseError> {
        let text = expr.trim();
        if text.is_empty() {
            return Err(AddrParseError::new("空表达式"));
        }
        let memory = if let Some(inner) = text.strip_prefix('[') {
            let inner = inner
                .strip_suffix(']')
                .ok_or_else(|| AddrParseError::new(format!("缺少右括号：{text}")))?;
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
                err.message().contains("局部变量") && err.message().contains("寄存器或数字"),
                "{form} 的错误消息要说明原因与替代写法：{}",
                err.message()
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
