//! 表达式里的**名字解析**（条件断点 `cond` 与观察表达式 `watch_expr` 共用）。
//!
//! ## 为什么单独一个模块
//!
//! 两套文法（条件 / 观察）都要回答同一个问题：「这个标识符指向什么」。
//! 答案依赖 `.symtab` + `.debug_info`（M-11）⇒ ⇒ 而 M-08 **不该**直接依赖
//! M-11（断点表是仿真核心，符号表是调试信息）⇒ ⇒ 这里只留两个**窄接口**：
//! [`NameResolver`]（解析）与 [`NameRef`]（答案的形状）。谁来喂它由调用方
//! 决定（宿主用 M-11 的符号表 + 局部变量表实现）。
//!
//! ## 解析顺序（冻结 · STAGE-55 表 9）
//!
//! 1. **寄存器 / ABI 别名** —— 文法层就解决了，走不到这里；
//! 2. **当前帧局部变量**（批 D 起）；
//! 3. **全局符号** —— 按 [`NameClass`] 决定「有没有值 / 在哪」；
//! 4. 都没有 ⇒ **未定义符号**（`E_DBG-02`）。
//!
//! ## 「有没有值」是分类的全部意义
//!
//! * **函数**没有值 ⇒ 名字在表达式里就是它的**地址常量**（档 2）；
//! * **全局变量**有值 ⇒ 名字是**变量的值**，要读内存（档 3）；
//! * **局部变量**在寄存器里 ⇒ 名字就是**寄存器值**（不用读内存，取指前就能求值）；
//!   在栈上 ⇒ **帧基址 + 偏移**处的字（store 边界求值）；
//! * **其它**（`STT_NOTYPE` 等）不保证 ⇒ 只许 `&名字` 取地址。
//!
//! ⚠️ 分类与「在哪」**成对**返回（[`NameRef`]）⇒ ⇒ 调用方拿不到「有分类、
//! 没有位置」的半截答案。

/// 一个名字指向的**东西是哪一类**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameClass {
    /// 函数：**没有值**，名字即地址。
    Function,
    /// 全局 / 静态变量：有值（读内存）。
    Object,
    /// 其它（`STT_NOTYPE` 等）：只保证有地址。
    Other,
    /// 当前帧局部变量（批 D 起）。
    Local,
}

/// **帧基址**去哪儿取（与 `m11_symbol::locals::FrameBase` 一一对应，
/// 但不直接引 M-11 的类型 ⇒ ⇒ M-08 不长出对 M-11 的依赖）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameBaseSlot {
    /// 某个通用寄存器里放着帧基址。
    Reg(u8),
    /// 「call frame address」⇒ ⇒ 批 D 按 `CFA == s0` 的口径取
    /// [`FRAME_POINTER`]。
    Cfa,
}

/// RISC-V psABI 的帧指针（`s0` / `x8`）。
///
/// ⚠️ 只在 [`FrameBaseSlot::Cfa`] 上用到，是一条**显式假设**（见
/// `m11_symbol::locals` 的模块头）。有帧指针的 `-O0` 产物上成立。
pub const FRAME_POINTER: u8 = 8;

impl FrameBaseSlot {
    /// 这个帧基址最终从**哪个寄存器**读（`Cfa` ⇒ 帧指针）。
    pub fn register(self) -> u8 {
        match self {
            Self::Reg(index) => index,
            Self::Cfa => FRAME_POINTER,
        }
    }
}

/// 一个**局部变量**放在哪儿。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSlot {
    /// 值就在这个寄存器里（不需要读内存）。
    Reg(u8),
    /// 地址 = `寄存器[base] + offset`（栈上）。
    Frame { base: FrameBaseSlot, offset: i32 },
}

/// 名字指向的东西**在哪儿**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameTarget {
    /// 地址已知（函数 / 全局 / 其它符号）。
    Addr(u32),
    /// 当前帧里的局部变量。
    Local(LocalSlot),
}

/// 一次解析的**结果**：分类 + 位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameRef {
    /// 这个名字是哪一类东西。
    pub class: NameClass,
    /// 它在哪儿。
    pub target: NameTarget,
}

impl NameRef {
    /// 地址（只有 [`NameTarget::Addr`] 有）。
    pub fn addr(&self) -> Option<u32> {
        match self.target {
            NameTarget::Addr(addr) => Some(addr),
            NameTarget::Local(_) => None,
        }
    }
}

/// 解析失败的**原因**（缺一不可：两种情况给用户的下一步**完全不同**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// 谁都不认识这个名字。
    Undefined,
    /// **多个函数**里都有同名局部变量 ⇒ ⇒ 光凭名字分不出来。
    ///
    /// 带着函数名，只为了报错时能点名（「`x` 在 `main` / `helper` 里都有」）。
    Ambiguous(Vec<String>),
}

/// 名字 → [`NameRef`] 的**窄接口**。
///
/// ⚠️ 只回答「有没有、是什么、在哪」⇒ ⇒ 不暴露符号表 / DWARF 结构。
pub trait NameResolver {
    /// 解析一个名字（**全局视角**：只看符号表，不看当前帧）。
    ///
    /// 未知名（或没有地址）⇒ `None`。
    fn resolve(&self, name: &str) -> Option<NameRef>;

    /// 带**帧上下文**的解析（批 D）。
    ///
    /// * `pc = Some(x)` ⇒ 先看 `x` 所在函数里的局部变量，再看全局符号；
    /// * `pc = None` ⇒ 局部变量**只凭名字**（多个函数都有 ⇒ [`ResolveError::Ambiguous`]）。
    ///
    /// ⚠️ 默认实现退化成 [`Self::resolve`]（只认全局符号）⇒ ⇒ 没有局部变量
    /// 上下文的实现者（`NoNames`、表级单元测试）不用改。
    fn resolve_at(&self, name: &str, _pc: Option<u32>) -> Result<NameRef, ResolveError> {
        self.resolve(name).ok_or(ResolveError::Undefined)
    }
}

/// **一个名字都不认识**的解析器。
///
/// ⚠️ 它不是「空符号表」的替身，而是**没有符号上下文**时的唯一选择
/// （表级单元测试、端口层 `set_breakpoint(addr, enabled)` 这类老路径）⇒ ⇒
/// 这些路径上出现名字就是 `E_DBG-02` 的「未定义符号」（如实）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoNames;

impl NameResolver for NoNames {
    fn resolve(&self, _name: &str) -> Option<NameRef> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_names_resolves_nothing() {
        assert_eq!(NoNames.resolve("main"), None);
        assert_eq!(NoNames.resolve(""), None);
        assert_eq!(
            NoNames.resolve_at("main", Some(0)),
            Err(ResolveError::Undefined)
        );
    }

    #[test]
    fn an_address_name_reports_the_address_and_a_local_does_not() {
        let global = NameRef {
            class: NameClass::Object,
            target: NameTarget::Addr(0x1000),
        };
        assert_eq!(global.addr(), Some(0x1000));
        let local = NameRef {
            class: NameClass::Local,
            target: NameTarget::Local(LocalSlot::Frame {
                base: FrameBaseSlot::Cfa,
                offset: -24,
            }),
        };
        assert_eq!(local.addr(), None, "局部变量没有「地址常量」这回事");
    }

    #[test]
    fn a_cfa_frame_base_reads_the_frame_pointer() {
        assert_eq!(FrameBaseSlot::Cfa.register(), FRAME_POINTER);
        assert_eq!(FrameBaseSlot::Reg(8).register(), 8);
        assert_eq!(
            FrameBaseSlot::Reg(2).register(),
            2,
            "sp 就是寄存器 2，不许被 CFA 的口径顶掉"
        );
    }
}
