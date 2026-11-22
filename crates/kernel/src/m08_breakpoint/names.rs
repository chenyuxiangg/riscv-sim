//! 表达式里的**名字解析**（条件断点 `cond` 与观察表达式 `watch_expr` 共用）。
//!
//! ## 为什么单独一个模块
//!
//! 两套文法（条件 / 观察）都要回答同一个问题：「这个标识符指向什么」。
//! 答案依赖 `.symtab`（M-11）⇒ ⇒ 而 M-08 **不该**直接依赖 M-11（断点表是
//! 仿真核心，符号表是调试信息）⇒ ⇒ 这里只留一个**窄接口**：
//! [`NameResolver`]。谁来喂它由调用方决定（宿主用 M-11 的符号表实现）。
//!
//! ## 解析顺序（冻结 · STAGE-55 表 9）
//!
//! 1. **寄存器 / ABI 别名** —— 文法层就解决了，走不到这里；
//! 2. **当前帧局部变量**（批 D 起）；
//! 3. **全局符号**（本模块）—— 按 [`SymbolClass`] 决定「有没有值」；
//! 4. 都没有 ⇒ **未定义符号**（`E_DBG-02`）。
//!
//! ## 「有没有值」是分类的全部意义
//!
//! * **函数**没有值 ⇒ 名字在表达式里就是它的**地址常量**（档 2）；
//! * **变量**有值 ⇒ 名字是**变量的值**，要读内存（档 3，批 C 起）；
//! * **其它**（`STT_NOTYPE` 等）不保证 ⇒ 只许 `&名字` 取地址。
//!
//! ⚠️ 分类与地址**成对**返回（[`NameRef`]）⇒ ⇒ 调用方拿不到「有分类没有
//! 地址」的半截答案。

/// 一个名字指向的**东西是哪一类**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameClass {
    /// 函数：**没有值**，名字即地址。
    Function,
    /// 全局 / 静态变量：有值（读内存）。
    Object,
    /// 其它（`STT_NOTYPE` 等）：只保证有地址。
    Other,
    /// 当前帧局部变量（批 D 起才会出现）。
    Local,
}

/// 一次解析的**结果**：分类 + 地址。
///
/// ⚠️ **地址是必填的** —— 没有地址的符号（`SHN_UNDEF`）在解析层就当
/// 「查无此名」处理（返回 `None`），不会走到这里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameRef {
    /// 这个名字是哪一类东西。
    pub class: NameClass,
    /// 它的地址。
    pub addr: u32,
}

/// 名字 → [`NameRef`] 的**窄接口**。
///
/// ⚠️ 只回答「有没有、是什么、在哪」⇒ ⇒ 不暴露符号表结构 ⇒ ⇒ M-08 不会
/// 长出对 M-11 的依赖。
pub trait NameResolver {
    /// 解析一个名字。未知名（或没有地址）⇒ `None`。
    fn resolve(&self, name: &str) -> Option<NameRef>;
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
    }
}
