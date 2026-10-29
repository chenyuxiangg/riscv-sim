//! M-08 断点管理器 (Breakpoint Manager).
//!
//! 管理 **PC 断点**（无条件 / **带条件**）与 **观察点**（地址 + 访问类型）。
//! 命中检测 `hit_check` 是**唯一**的**地址匹配**入口（M-08 LLD §3.2「评审
//! 08-1」），挂在 **取指之前**——断点命中时该条指令**还没有执行**，寄存器 /
//! 内存状态与命中前完全一致（M-08 LLD §7.1「命中无副作用」用例）。
//!
//! ## 表结构
//!
//! - `by_addr` —— 按地址索引，命中检测 **O(1)**（M-08 LLD 表38）。
//! - `by_id` —— id → 地址反查索引，用于 `clear` / `enable` / `disable`。
//!
//! ## 条件断点（v0.57 批 · STAGE-42 §4）
//!
//! 契约的 `Breakpoint` 现在有一个**可选**字段 `condition`（表达式）⇒ ⇒
//! `None` ⇒ **无条件断点** ⇒ ⇒ **既有行为一字不变**；`Some(expr)` ⇒ 命中
//! 该地址时**还要**满足 `expr` 才停。
//!
//! ⚠️ **表达式的解析与求值住在 [`cond`]**（本模块的子模块）⇒ ⇒ 语法
//! **逐字**照 STAGE-42 §4 的 EBNF ⇒ ⇒ **一条约束一个落点**。
//!
//! ⚠️ **条件在「取指之前」求值**（裁决 H）⇒ ⇒ 条件里看到的寄存器值 =
//! **断点地址那条指令执行前**的值 ⇒ ⇒ 与既有断点语义**完全一致**。
//!
//! ⚠️ **表达式非法 ⇒ `SetBreakpoint` 被拒**（`E_DBG_02`）⇒ ⇒ 断点表
//! **不变**（裁决 I）—— 与「地址非法 ⇒ 表不变」**同一条纪律**。
//!
//! ## 错误码
//!
//! M-08 LLD §6.1 明确「本模块错误码归属 **E_CTX** 族」，
//! `E_CTX-07` = 无效地址 / 重复注册。**那一族一字不动**。
//!
//! ⚠️ **条件表达式的错误走 `E_DBG` 族**（契约层 `E_DBG-02` · STAGE-42
//! §2.6）⇒ ⇒ 这是与 M-08 LLD §6.1 的**有意偏差**（契约层对 `Debug` 族的
//! 定义逐字含「breakpoint / register-memory」）⇒ ⇒ **如实登记，不藏**。
//! ⚠️ 「一场景一码」⇒ ⇒ 两个码**语义不同**（一个是地址，一个是表达式）
//! ⇒ ⇒ **不**复用 `E_CTX-07`。
//!
//! ## 状态
//!
//! v0.13：真实实现（v0.11 之前是 53 行的空壳，只有一个 `BTreeMap`）。
//! v0.57：**条件断点**落地（STAGE-42 §4）—— ⚠️ 此前本模块的文档写着
//! 「条件断点因此**未实现**（记为台账 **D-32** 的缺口）」⇒ ⇒ 那是**过期
//! 引用**（D-32 早已在 v0.21 闭环，7 项缺口里**不含**条件断点）⇒ ⇒ 本批
//! **顺手订正**成指向本规格（STAGE-42 · F-15）。

use riscv_sim_contract::error::{ErrorFamily, PlatformError, DEBUG_FAMILY};
use riscv_sim_contract::model::{Breakpoint, WatchAccess, Watchpoint, WriteSize};
use std::collections::BTreeMap;

use riscv_sim_contract::error::E_DBG_02_CONDITION_INVALID;

pub mod cond;

/// 断点地址的对齐粒度（字节）。
///
/// RV32IMC 的 C 扩展允许 **半字（2 字节）** 对齐的压缩指令，所以断点
/// 地址只要求 2 字节对齐——比「必须 4 字节」更贴合本平台的 ISA。
/// 未对齐的地址按 `E_CTX-07` 拒绝（M-08 LLD §3.1「地址校验」）。
pub const BP_ADDR_ALIGN: u32 = 2;

/// 错误码场景号：无效地址 / 重复注册（M-08 LLD §6.1 `E_CTX-07`）。
pub const E_CTX_07_INVALID_OR_DUPLICATE: u16 = 7;

/// 断点表里的一条 —— **契约 DTO** + **已解析的条件**。
///
/// ⚠️ `cond` 是**推导量**：它由 `bp.condition`（**文本**）解析而来 ⇒ ⇒
/// 条件的**文本**只有**一份**（在 DTO 里）⇒ ⇒ 不会出现「文本与 AST
/// 漂移」。
#[derive(Debug, Clone, Default)]
struct Entry {
    /// 契约的 `Breakpoint` DTO（**真相**）。
    bp: Breakpoint,
    /// **已解析**的条件（`None` ⇒ 无条件断点）。
    cond: Option<cond::Condition>,
}

/// 断点表 —— 按地址索引 + id 反查索引。
///
/// 单一数据源：注册 / 删除 / 使能只改这两张表，没有第二份状态
/// （M-08 LLD §7.2(1)「无隐式状态」）。
#[derive(Debug, Default, Clone)]
pub struct BreakpointTable {
    /// 地址 → 断点条目。命中检测走这里（O(log n) 树查找，键即地址）。
    by_addr: BTreeMap<u32, Entry>,
    /// 断点 id → 地址。`clear` / `enable` / `disable` 走这里。
    by_id: BTreeMap<u32, u32>,
}

impl BreakpointTable {
    /// 创建一个空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册或更新一个断点。
    ///
    /// **校验**（M-08 LLD §3.1）：
    /// - 地址必须 2 字节对齐 → 否则 `E_CTX-07`；
    /// - 同一地址**重复注册** → `E_CTX-07`；
    /// - 同一 **id** 复用到另一个地址 → `E_CTX-07`（id 会话内唯一）。
    ///
    /// **条件**（v0.57 批 · 裁决 I）：
    /// - `condition` 非法（语法错 / 未知寄存器名 / 空表达式）→ `E_DBG-02`。
    ///
    /// 校验失败时**表不变**（M-08 LLD §7.1「断点表不变」用例）。
    ///
    /// ⚠️ **既有三道校验的语义一字未改** ⇒ ⇒ 条件校验**排在它们之后**
    /// ⇒ ⇒ 「地址非法 + 条件非法」时报的仍是**地址**的错（`E_CTX-07`）。
    pub fn set(&mut self, bp: Breakpoint) -> Result<(), PlatformError> {
        if bp.addr % BP_ADDR_ALIGN != 0 {
            return Err(self.error(format!(
                "断点地址未按 {BP_ADDR_ALIGN} 字节对齐：0x{:08x}",
                bp.addr
            )));
        }
        if self.by_addr.contains_key(&bp.addr) {
            return Err(self.error(format!("断点地址已存在：0x{:08x}", bp.addr)));
        }
        if let Some(&existing_addr) = self.by_id.get(&bp.id) {
            if existing_addr != bp.addr {
                return Err(self.error(format!(
                    "断点 id {} 已绑定到 0x{:08x}",
                    bp.id, existing_addr
                )));
            }
        }
        // ⚠️ 条件**在这里**解析（注册时一次）⇒ ⇒ 命中时**不再**解析 ⇒ ⇒
        //    命中路径上**没有**字符串处理。
        let cond = match bp.condition.as_deref() {
            None => None,
            Some(text) => match cond::parse(text) {
                Ok(expr) => Some(expr),
                Err(err) => return Err(self.condition_error(err)),
            },
        };
        let id = bp.id;
        let addr = bp.addr;
        self.by_addr.insert(addr, Entry { bp, cond });
        self.by_id.insert(id, addr);
        Ok(())
    }

    /// 按 **id** 删除断点。返回它原先绑定的地址。
    ///
    /// **幂等**：id 不存在时返回 `Ok(None)`，不报错、不产生任何副作用
    /// （M-08 LLD §7.1「无效 id 幂等（评审 09-6 / N8）」）。
    pub fn clear(&mut self, id: u32) -> Result<Option<u32>, PlatformError> {
        let Some(addr) = self.by_id.remove(&id) else {
            return Ok(None);
        };
        self.by_addr.remove(&addr);
        Ok(Some(addr))
    }

    /// 启用 / 禁用断点。返回 `true` 表示状态真的变了。
    ///
    /// **幂等**：id 不存在时返回 `Ok(false)`（同上）。
    pub fn set_enabled(&mut self, id: u32, enabled: bool) -> Result<bool, PlatformError> {
        let Some(&addr) = self.by_id.get(&id) else {
            return Ok(false);
        };
        let Some(bp) = self.by_addr.get_mut(&addr) else {
            // by_id 与 by_addr 不一致属于内部状态损坏；当作「不存在」处理，
            // 不 panic（WASM 里 panic = Worker 崩）。
            return Ok(false);
        };
        if bp.bp.enabled == enabled {
            return Ok(false);
        }
        bp.bp.enabled = enabled;
        Ok(true)
    }

    /// **命中检测** —— 唯一的**地址匹配**入口。
    ///
    /// 挂在取指之前：命中时该条指令尚未执行，状态无副作用。
    /// 禁用的断点**不命中**（M-08 LLD §3.2「禁用断点不命中」）。
    ///
    /// ⚠️ **只看地址与 `enabled`** ⇒ ⇒ **无条件断点**的语义与 v0.56 批
    /// **逐字相同**（K-14）。
    ///
    /// ⚠️ **带条件**的那一条走 [`Self::hit_check_cond`]（在**本方法**的
    /// 命中**之上**再判条件）⇒ ⇒ 「地址匹配」仍然只有**这一个**入口。
    pub fn hit_check(&self, pc: u32) -> Option<Breakpoint> {
        Some(self.hit_entry(pc)?.bp.clone())
    }

    /// **带条件**的命中检测 —— 在 [`Self::hit_check`] 的命中**之上**再判
    /// 条件。
    ///
    /// * **无条件断点**（`condition == None`）⇒ ⇒ 与 [`Self::hit_check`]
    ///   **逐字相同**；
    /// * **条件为真** ⇒ ⇒ 命中；
    /// * **条件为假** ⇒ ⇒ **不命中**（继续执行）。
    ///
    /// ⚠️ **求值时刻 = 取指之前** ⇒ ⇒ `gpr` 里的值是**断点地址那条指令
    /// 执行前**的值（裁决 H）。
    ///
    /// ⚠️ **求值失败在 v1 结构上不可能**（只有寄存器读 ⇒ 永远读得到）
    /// ⇒ ⇒ **不**设计「求值出错」的语义（裁决 J）。
    pub fn hit_check_cond(&self, pc: u32, gpr: &[u32; 32]) -> Option<Breakpoint> {
        let entry = self.hit_entry(pc)?;
        match &entry.cond {
            None => Some(entry.bp.clone()),
            Some(expr) => {
                if expr.eval(gpr, pc) != 0 {
                    Some(entry.bp.clone())
                } else {
                    None
                }
            }
        }
    }

    /// 命中检测的**内部**实现：按地址取条目，并剔除**禁用**的断点。
    ///
    /// ⚠️ 「地址匹配 + 启用」这条判定**只有这一个落点** ⇒ ⇒
    /// [`Self::hit_check`] 与 [`Self::hit_check_cond`] 不会漂移。
    fn hit_entry(&self, pc: u32) -> Option<&Entry> {
        let entry = self.by_addr.get(&pc)?;
        if !entry.bp.enabled {
            return None;
        }
        Some(entry)
    }

    /// 按 id 取断点。
    pub fn get(&self, id: u32) -> Option<&Breakpoint> {
        let addr = *self.by_id.get(&id)?;
        self.by_addr.get(&addr).map(|entry| &entry.bp)
    }

    /// 按地址取断点。
    pub fn get_by_addr(&self, addr: u32) -> Option<&Breakpoint> {
        self.by_addr.get(&addr).map(|entry| &entry.bp)
    }

    /// 断点数量。
    pub fn len(&self) -> usize {
        self.by_addr.len()
    }

    /// 表是否为空。
    pub fn is_empty(&self) -> bool {
        self.by_addr.is_empty()
    }

    /// 按地址升序遍历断点。
    pub fn iter(&self) -> impl Iterator<Item = &Breakpoint> {
        self.by_addr.values().map(|entry| &entry.bp)
    }

    /// 清空整张表（会话重置用）。
    pub fn clear_all(&mut self) {
        self.by_addr.clear();
        self.by_id.clear();
    }

    /// 构造本模块的错误（`E_CTX-07`）。
    fn error(&self, message: String) -> PlatformError {
        PlatformError::new(Self::FAMILY, E_CTX_07_INVALID_OR_DUPLICATE, message)
    }

    /// 构造**条件表达式**的错误（`E_DBG-02`）。
    ///
    /// ⚠️ **族是 `Debug`**（契约层 `E_DBG_02_CONDITION_INVALID` · STAGE-42
    /// §2.6）⇒ ⇒ **不**复用 `E_CTX-07`（那是「地址非法 / 重复注册」）⇒ ⇒
    /// 「一场景一码」。
    ///
    /// ⚠️ 消息**原样**带上 `cond` 模块给的那句（含「哪一段不对」的线索）
    /// ⇒ ⇒ 前端**不吞**。
    fn condition_error(&self, err: cond::CondError) -> PlatformError {
        PlatformError::new(DEBUG_FAMILY, E_DBG_02_CONDITION_INVALID, err.message())
    }

    /// Family used by errors raised from this module.
    ///
    /// **`Context`（`E_CTX`）而不是 `Debug`（`E_DBG`）**——M-08 LLD §6.1
    /// 逐字规定「本模块错误码归属 E_CTX 族」，且错误码是「一场景一码」。
    ///
    /// ⚠️ **v0.57 批（STAGE-42 §2.6）**：**条件表达式**的错误走 `E_DBG-02`
    /// ⇒ ⇒ 那一条**不走**本常量 ⇒ ⇒ 本常量仍然只管「地址 / 重复注册」。
    pub const FAMILY: ErrorFamily = ErrorFamily::Context;
}

/// 观察点表 —— 按 id 索引。
///
/// 观察点是**地址 + 访问类型**的组合（M-08 LLD §2.2「watch 地址表达式」
/// 与契约 `model::Watchpoint`）。契约的 `Watchpoint` 字段齐备
/// （`id` / `addr` / `size` / `access`），所以观察点在本阶段**可以**做到
/// 精确匹配，不像条件断点那样被契约缺口卡住。
#[derive(Debug, Default, Clone)]
pub struct WatchpointTable {
    inner: BTreeMap<u32, Watchpoint>,
}

impl WatchpointTable {
    /// 创建一个空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个观察点。同一 id 重复注册 → 覆盖（观察点没有「地址唯一」
    /// 的语义，它的身份就是 id）。
    pub fn set(&mut self, wp: Watchpoint) -> Result<(), PlatformError> {
        self.inner.insert(wp.id, wp);
        Ok(())
    }

    /// 按 id 删除观察点。返回被删除的观察点；id 不存在返回 `None`（幂等）。
    pub fn clear(&mut self, id: u32) -> Option<Watchpoint> {
        self.inner.remove(&id)
    }

    /// 访问匹配：给定一次总线访问（地址 / 宽度 / 方向），返回命中的观察点。
    ///
    /// 命中条件（M-08 LLD §2.2）：
    /// - **区间相交**：访问区间 `[addr, addr+size)` 与观察区间
    ///   `[wp.addr, wp.addr+wp_size)` 有交集；
    /// - **方向匹配**：`WatchAccess::Either` 匹配读写，`Read` 只匹配读，
    ///   `Write` 只匹配写。
    ///
    /// 返回**第一个**命中的观察点（按 id 升序，保证确定性）。
    pub fn match_access(&self, addr: u32, size: WriteSize, is_write: bool) -> Option<Watchpoint> {
        let access_len = size_bytes(size);
        let access_end = addr.checked_add(access_len)?;
        for wp in self.inner.values() {
            if !access_matches(wp.access, is_write) {
                continue;
            }
            let wp_len = size_bytes(wp.size);
            let wp_end = wp.addr.checked_add(wp_len)?;
            if addr < wp_end && wp.addr < access_end {
                return Some(wp.clone());
            }
        }
        None
    }

    /// 按 id 取观察点。
    pub fn get(&self, id: u32) -> Option<&Watchpoint> {
        self.inner.get(&id)
    }

    /// 观察点数量。
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// 表是否为空。
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// 按 id 升序遍历观察点。
    pub fn iter(&self) -> impl Iterator<Item = &Watchpoint> {
        self.inner.values()
    }

    /// 清空整张表。
    pub fn clear_all(&mut self) {
        self.inner.clear();
    }
}

/// 访问宽度（字节）。
fn size_bytes(size: WriteSize) -> u32 {
    match size {
        WriteSize::Byte => 1,
        WriteSize::Half => 2,
        WriteSize::Word => 4,
    }
}

/// 访问方向是否与观察点的 `access` 匹配。
fn access_matches(access: WatchAccess, is_write: bool) -> bool {
    match access {
        WatchAccess::Either => true,
        WatchAccess::Read => !is_write,
        WatchAccess::Write => is_write,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bp(id: u32, addr: u32) -> Breakpoint {
        Breakpoint {
            id,
            addr,
            source_hint: None,
            enabled: true,
            condition: None,
        }
    }

    /// 造一个**带条件**的断点。
    fn bp_cond(id: u32, addr: u32, condition: &str) -> Breakpoint {
        Breakpoint {
            id,
            addr,
            source_hint: None,
            enabled: true,
            condition: Some(condition.to_string()),
        }
    }

    fn wp(id: u32, addr: u32, size: WriteSize, access: WatchAccess) -> Watchpoint {
        Watchpoint {
            id,
            addr,
            size,
            access,
        }
    }

    // -- 注册与校验 ---------------------------------------------------------

    #[test]
    fn set_and_get_by_id_and_addr() {
        let mut t = BreakpointTable::new();
        t.set(bp(1, 0x8000_0000)).expect("set");
        assert_eq!(t.len(), 1);
        assert_eq!(t.get(1).map(|b| b.addr), Some(0x8000_0000));
        assert_eq!(t.get_by_addr(0x8000_0000).map(|b| b.id), Some(1));
    }

    #[test]
    fn misaligned_address_is_rejected_and_table_unchanged() {
        let mut t = BreakpointTable::new();
        let err = t.set(bp(1, 0x8000_0001)).unwrap_err();
        assert_eq!(err.family, ErrorFamily::Context);
        assert_eq!(err.scenario, E_CTX_07_INVALID_OR_DUPLICATE);
        assert!(err.to_string().starts_with("E_CTX-07:"));
        assert!(t.is_empty(), "被拒绝的注册不得留下痕迹");
    }

    #[test]
    fn halfword_aligned_address_is_accepted() {
        // C 扩展的压缩指令半字对齐 —— 2 字节对齐必须放行。
        let mut t = BreakpointTable::new();
        t.set(bp(1, 0x8000_0002)).expect("半字对齐应当放行");
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn duplicate_address_is_rejected() {
        let mut t = BreakpointTable::new();
        t.set(bp(1, 0x8000_0000)).expect("first");
        let err = t.set(bp(2, 0x8000_0000)).unwrap_err();
        assert_eq!(err.scenario, E_CTX_07_INVALID_OR_DUPLICATE);
        assert_eq!(t.len(), 1, "重复注册不得改变表");
    }

    #[test]
    fn duplicate_id_on_other_address_is_rejected() {
        let mut t = BreakpointTable::new();
        t.set(bp(1, 0x8000_0000)).expect("first");
        let err = t.set(bp(1, 0x8000_0004)).unwrap_err();
        assert_eq!(err.scenario, E_CTX_07_INVALID_OR_DUPLICATE);
        assert_eq!(t.len(), 1);
    }

    // -- 删除 / 使能 --------------------------------------------------------

    #[test]
    fn clear_removes_by_id_and_returns_addr() {
        let mut t = BreakpointTable::new();
        t.set(bp(7, 0x8000_0000)).expect("set");
        assert_eq!(t.clear(7).unwrap(), Some(0x8000_0000));
        assert!(t.is_empty());
        assert_eq!(t.get(7), None);
    }

    #[test]
    fn clear_unknown_id_is_idempotent() {
        let mut t = BreakpointTable::new();
        t.set(bp(7, 0x8000_0000)).expect("set");
        assert_eq!(t.clear(999).unwrap(), None, "未知 id 必须幂等");
        assert_eq!(t.len(), 1, "幂等清除不得误伤别的断点");
    }

    #[test]
    fn disable_then_enable_round_trip() {
        let mut t = BreakpointTable::new();
        t.set(bp(7, 0x8000_0000)).expect("set");
        assert!(t.set_enabled(7, false).unwrap(), "状态确实变了");
        assert!(!t.set_enabled(7, false).unwrap(), "重复禁用不改变状态");
        assert!(t.hit_check(0x8000_0000).is_none(), "禁用的断点不命中");
        assert!(t.set_enabled(7, true).unwrap());
        assert!(t.hit_check(0x8000_0000).is_some());
    }

    #[test]
    fn set_enabled_unknown_id_is_idempotent() {
        let mut t = BreakpointTable::new();
        assert!(!t.set_enabled(42, true).unwrap());
        assert!(t.is_empty());
    }

    // -- 命中检测 -----------------------------------------------------------

    #[test]
    fn hit_check_only_fires_on_exact_pc() {
        let mut t = BreakpointTable::new();
        t.set(bp(1, 0x8000_0000)).expect("set");
        assert!(t.hit_check(0x8000_0000).is_some());
        assert!(t.hit_check(0x8000_0004).is_none());
    }

    #[test]
    fn hit_check_reports_the_breakpoint_id() {
        let mut t = BreakpointTable::new();
        t.set(bp(42, 0x8000_0000)).expect("set");
        assert_eq!(t.hit_check(0x8000_0000).map(|b| b.id), Some(42));
    }

    #[test]
    fn hit_check_has_no_side_effects() {
        let mut t = BreakpointTable::new();
        t.set(bp(1, 0x8000_0000)).expect("set");
        let before = t.len();
        let _ = t.hit_check(0x8000_0000);
        assert_eq!(t.len(), before, "命中检测不得改变表");
    }

    // -- 条件断点（v0.57 批 · STAGE-42） --------------------------------------

    /// **K-11 的表侧那一半**：`SetBreakpoint` 带**非法条件** ⇒ `E_DBG-02`
    /// 且**断点表不变**（裁决 I）。
    #[test]
    fn an_illegal_condition_is_rejected_and_the_table_is_unchanged() {
        let mut t = BreakpointTable::new();
        t.set(bp(1, 0x8000_0000)).expect("set");
        let before = t.len();
        let err = t
            .set(bp_cond(2, 0x8000_0004, "foo"))
            .expect_err("非法条件必须被拒");
        assert_eq!(err.family, ErrorFamily::Debug);
        assert_eq!(err.scenario, E_DBG_02_CONDITION_INVALID);
        assert!(
            err.message.contains("foo"),
            "错误消息要指出「哪一段不对」：{}",
            err.message
        );
        assert_eq!(t.len(), before, "被拒的注册不得改变表");
        assert!(t.get_by_addr(0x8000_0004).is_none());
    }

    /// **条件为真 ⇒ 命中；为假 ⇒ 不命中**。
    #[test]
    fn the_condition_decides_whether_the_breakpoint_hits() {
        let mut t = BreakpointTable::new();
        t.set(bp_cond(1, 0x8000_0000, "x1 == 5")).expect("set");
        let mut gpr = [0u32; 32];
        gpr[1] = 5;
        assert!(
            t.hit_check_cond(0x8000_0000, &gpr).is_some(),
            "条件为真 ⇒ 命中"
        );
        gpr[1] = 6;
        assert!(
            t.hit_check_cond(0x8000_0000, &gpr).is_none(),
            "条件为假 ⇒ 不命中"
        );
    }

    /// **无条件断点 ⇒ `hit_check_cond` 与 `hit_check` 逐字相同**（K-14）。
    #[test]
    fn an_unconditional_breakpoint_behaves_exactly_as_before() {
        let mut t = BreakpointTable::new();
        t.set(bp(1, 0x8000_0000)).expect("set");
        let gpr = [7u32; 32];
        assert_eq!(
            t.hit_check_cond(0x8000_0000, &gpr).map(|b| b.id),
            t.hit_check(0x8000_0000).map(|b| b.id)
        );
    }

    /// **禁用的断点**在带条件的那一支上**也不命中**。
    #[test]
    fn a_disabled_breakpoint_never_hits_even_with_a_true_condition() {
        let mut t = BreakpointTable::new();
        t.set(bp_cond(1, 0x8000_0000, "x1 == 5")).expect("set");
        t.set_enabled(1, false).expect("disable");
        let mut gpr = [0u32; 32];
        gpr[1] = 5;
        assert!(t.hit_check_cond(0x8000_0000, &gpr).is_none());
    }

    /// **条件里看到的是「该指令执行前」的值**（裁决 H）—— 这里用「PC」把
    /// 求值时刻钉住：命中时 PC **就是**断点地址。
    #[test]
    fn the_condition_sees_the_pre_execution_pc() {
        let mut t = BreakpointTable::new();
        t.set(bp_cond(1, 0x8000_0000, "pc == 0x80000000"))
            .expect("set");
        let gpr = [0u32; 32];
        assert!(t.hit_check_cond(0x8000_0000, &gpr).is_some());
        assert!(t.hit_check_cond(0x8000_0004, &gpr).is_none());
    }

    // -- 观察点 -------------------------------------------------------------

    #[test]
    fn watchpoint_matches_write_in_range() {
        let mut t = WatchpointTable::new();
        t.set(wp(1, 0x8000_0010, WriteSize::Word, WatchAccess::Write))
            .expect("set");
        assert!(t.match_access(0x8000_0010, WriteSize::Word, true).is_some());
        assert!(
            t.match_access(0x8000_0010, WriteSize::Word, false)
                .is_none(),
            "只监视写，就不该被读命中"
        );
    }

    #[test]
    fn watchpoint_matches_overlapping_access() {
        let mut t = WatchpointTable::new();
        t.set(wp(1, 0x8000_0010, WriteSize::Word, WatchAccess::Either))
            .expect("set");
        // 访问区间 [0x8000_000e, 0x8000_0012) 与观察区间 [0x10, 0x14) 相交。
        assert!(t.match_access(0x8000_000e, WriteSize::Word, true).is_some());
        // 不相交的访问不命中。
        assert!(t.match_access(0x8000_0020, WriteSize::Word, true).is_none());
    }

    #[test]
    fn watchpoint_either_matches_both_directions() {
        let mut t = WatchpointTable::new();
        t.set(wp(1, 0x8000_0010, WriteSize::Byte, WatchAccess::Either))
            .expect("set");
        assert!(t.match_access(0x8000_0010, WriteSize::Byte, true).is_some());
        assert!(t
            .match_access(0x8000_0010, WriteSize::Byte, false)
            .is_some());
    }

    #[test]
    fn watchpoint_clear_is_idempotent() {
        let mut t = WatchpointTable::new();
        t.set(wp(1, 0x8000_0010, WriteSize::Word, WatchAccess::Write))
            .expect("set");
        assert!(t.clear(1).is_some());
        assert!(t.clear(1).is_none(), "重复清除必须幂等");
        assert!(t.is_empty());
    }
}
