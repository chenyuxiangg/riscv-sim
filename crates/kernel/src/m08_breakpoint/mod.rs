//! M-08 断点管理器 (Breakpoint Manager).
//!
//! 管理 **PC 断点**（无条件 / **带条件**）与 **观察点**（地址 + 访问类型）。
//! 命中检测 `hit_check` 是**唯一**的**地址匹配**入口（M-08 LLD §3.2「评审
//! 08-1」），挂在 **取指之前**——断点命中时该条指令**还没有执行**，寄存器 /
//! 内存状态与命中前完全一致（M-08 LLD §7.1「命中无副作用」用例）。
//!
//! ## 表结构（STAGE-55 批重构：**id 权威 + 两个派生索引**）
//!
//! - `by_id` —— **唯一业务状态**（id → 条目）。
//! - `addr_index` —— **派生索引**（地址 → id，只收 `Address` / `Symbol`），
//!   命中检测走这里；地址重复校验也走这里。
//! - `pred_ids` —— **派生索引**（`Condition` 条目的 id 集合），供每指令遍历。
//!
//! ⚠️ 换权威的原因：**观察式条件断点没有地址** ⇒ ⇒ 若权威仍是地址映射，
//! 这类条目**无处安放**（详见 [`BreakpointTable`] 的文档）。
//!
//! ## 观察式条件断点（STAGE-55 批）
//!
//! `kind == Condition` 的条目**没有地址**（`addr` 恒为 `0` 占位）⇒ ⇒
//! **不进** `addr_index`、**不参与**地址匹配 ⇒ ⇒ 命中检测走
//! [`BreakpointTable::hit_check_pred`]，按指令步求值谓词，为真即停。
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
use riscv_sim_contract::model::{
    Breakpoint, BreakpointInfo, BreakpointKind, WatchAccess, Watchpoint, WriteSize,
};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use riscv_sim_contract::error::E_DBG_02_CONDITION_INVALID;

pub mod watch_expr;

pub mod cond;

pub mod names;

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
    cond: Option<cond::BoundCondition>,
    /// **命中计数**（契约 v13）。只在内核判定「真的命中」时 +1。
    hits: Cell<u64>,
    /// **降级原因**（`None` ⇒ 没有降级）。见 [`BreakpointTable::mark_degraded`]。
    degraded: Option<String>,
}

/// 断点表 —— **id 权威 + 两个派生索引**（STAGE-55 批重构）。
///
/// ## 权威为什么从「地址」换成「id」
///
/// v0.124 之前 `by_addr` 是权威，因为每条断点**都必须有地址**。
/// STAGE-55 引入**观察式条件断点**（`kind == Condition`）⇒ ⇒ 它**没有
/// 地址**（`addr` 只是 `0` 占位）⇒ ⇒ 若权威仍是地址映射，这类条目
/// **无处安放**。所以权威换成 `by_id`，地址映射降级为**索引**。
///
/// ## 单一数据源（M-08 LLD §7.2(1)「无隐式状态」）
///
/// * `by_id` —— **唯一业务状态**：条目、`enabled`、命中计数、已解析 AST
///   全在这里；
/// * `addr_index` —— **派生索引**：`addr → id`，**只收** `Address` /
///   `Symbol` 条目；地址重复校验走这里；
/// * `pred_ids` —— **派生索引**：`Condition` 条目的 id 集合，供每指令
///   遍历用。
///
/// ⚠️ 两个索引都**能由 `by_id` 完整重算** ⇒ ⇒ 不是第二份业务状态 ⇒ ⇒
/// 「无隐式状态」这条纪律**仍然成立**。写路径必须同步三处；读取路径只用
/// 其中一处、**不交叉推断**。
#[derive(Debug, Default, Clone)]
pub struct BreakpointTable {
    /// **权威**：id → 条目。
    by_id: BTreeMap<u32, Entry>,
    /// **派生索引**：地址 → id（只收 `Address` / `Symbol`）。
    addr_index: BTreeMap<u32, u32>,
    /// **派生索引**：`Condition` 条目的 id。
    pred_ids: BTreeSet<u32>,
    /// **回执槽**：最后一次 store 边界命中的**触发指令地址**（`exec_pc`）。
    ///
    /// ⚠️ store 边界命中时 PC 已经推进到下一条 ⇒ ⇒ 宿主没法从 `pc` 反推
    /// 「是哪条 store 触发的」⇒ ⇒ 这里记一份。每步开始时由调用方清空。
    store_trigger: Cell<Option<u32>>,
}

/// 两段字节区间**有没有相交**（`(起始地址, 字节数)`）。
///
/// ⚠️ 用 `saturating_add`：地址逼近 `u32::MAX` 时区间末端不 panic（宁可
/// 判成「不相交」也不让一次比较把内核弄崩）。
fn overlaps(load: (u32, u32), writes: &[(u32, u32)]) -> bool {
    let (load_addr, load_len) = load;
    let load_end = load_addr.saturating_add(load_len);
    writes.iter().any(|&(write_addr, write_len)| {
        let write_end = write_addr.saturating_add(write_len);
        load_addr < write_end && write_addr < load_end
    })
}

impl BreakpointTable {
    /// 创建一个空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个断点 —— **按 `kind` 分派校验**。
    ///
    /// **`Address` / `Symbol`**（M-08 LLD §3.1）：
    /// - 地址必须 2 字节对齐 → 否则 `E_CTX-07`；
    /// - 同一地址**重复注册** → `E_CTX-07`；
    ///
    /// **`Condition`（观察式）**：**不**做地址校验（它没有真实地址）；
    /// 但**必须**带表达式原文 ⇒ ⇒ 空表达式按 `cond::parse("")` 的原样拒。
    ///
    /// **共同**：
    /// - 同一 **id** 重复注册 → `E_CTX-07`（id 会话内唯一）；
    /// - `condition` 非法（语法错 / 未知寄存器名 / 空表达式）→ `E_DBG-02`
    ///   （裁决 I）。
    ///
    /// 校验失败时**表不变**（M-08 LLD §7.1「断点表不变」用例）。
    ///
    /// ⚠️ **既有三道地址校验的语义一字未改** ⇒ ⇒ 条件校验**排在它们之后**
    /// ⇒ ⇒ 「地址非法 + 条件非法」时报的仍是**地址**的错（`E_CTX-07`）。
    pub fn set(&mut self, bp: Breakpoint) -> Result<(), PlatformError> {
        self.set_with(bp, &names::NoNames)
    }

    /// 注册一个断点，**带名字解析**（条件里的 `main` / `&g_counter`）。
    ///
    /// ⚠️ 与 [`BreakpointTable::set`] 的差别**只有**「名字能不能解析」：
    /// 不带符号上下文的路径（表级测试 / 端口层的 `set_breakpoint(addr)`）
    /// 上出现名字 ⇒ ⇒ 如实报 `E_DBG-02` 的「未定义符号」。
    pub fn set_with(
        &mut self,
        bp: Breakpoint,
        resolver: &dyn names::NameResolver,
    ) -> Result<(), PlatformError> {
        let is_pred = bp.kind == BreakpointKind::Condition;
        if !is_pred {
            if bp.addr % BP_ADDR_ALIGN != 0 {
                return Err(self.error(format!(
                    "断点地址未按 {BP_ADDR_ALIGN} 字节对齐：0x{:08x}",
                    bp.addr
                )));
            }
            if self.addr_index.contains_key(&bp.addr) {
                return Err(self.error(format!("断点地址已存在：0x{:08x}", bp.addr)));
            }
        }
        if let Some(existing) = self.by_id.get(&bp.id) {
            return Err(self.error(format!(
                "断点 id {} 已绑定到 0x{:08x}",
                bp.id, existing.bp.addr
            )));
        }
        // ⚠️ 条件**在这里**解析（注册时一次）⇒ ⇒ 命中时**不再**解析 ⇒ ⇒
        //    命中路径上**没有**字符串处理。
        let cond = match bp.condition.as_deref() {
            None => None,
            // ⚠️ 解析 + **绑定**都在这里（注册时一次）⇒ ⇒ 表里存的是
            //    **绑定后**的 AST ⇒ ⇒ 命中路径上既没有字符串处理，也没有
            //    符号表查询。
            //
            // ⚠️ **帧上下文**（批 D）：地址类断点**知道自己站在哪个函数里**
            //    （`bp.addr`）⇒ ⇒ 局部变量按那个函数解析，同名不歧义；无地址
            //    的观察式条目没有这个上下文（`None`），局部变量只凭名字。
            Some(text) => {
                let scope = if is_pred { None } else { Some(bp.addr) };
                match cond::parse(text).and_then(|c| c.bind_at(resolver, scope)) {
                    Ok(expr) => Some(expr),
                    Err(err) => return Err(self.condition_error(err)),
                }
            }
        };
        // ⚠️ 观察式条目**必须**有表达式 ⇒ ⇒ 缺了就按「空表达式」原样拒
        //    （错误消息由 `cond` 给，前端不吞）。
        if is_pred && cond.is_none() {
            let err = cond::parse("").expect_err("空表达式必须被拒");
            return Err(self.condition_error(err));
        }
        let id = bp.id;
        if !is_pred {
            self.addr_index.insert(bp.addr, id);
        }
        self.by_id.insert(
            id,
            Entry {
                bp,
                cond,
                hits: Cell::new(0),
                degraded: None,
            },
        );
        if is_pred {
            self.pred_ids.insert(id);
        }
        debug_assert!(self.indexes_consistent(), "派生索引必须与 by_id 一致");
        Ok(())
    }

    /// 按 **id** 删除断点。返回它原先绑定的地址。
    ///
    /// **幂等**：id 不存在时返回 `Ok(None)`，不报错、不产生任何副作用
    /// （M-08 LLD §7.1「无效 id 幂等（评审 09-6 / N8）」）。
    ///
    /// ⚠️ `Condition` 条目**没有真实地址** ⇒ ⇒ 返回的是它的 `addr` 占位
    /// （恒为 `0`）⇒ ⇒ 调用方**不得**把这个 `0` 当地址用。
    pub fn clear(&mut self, id: u32) -> Result<Option<u32>, PlatformError> {
        let Some(entry) = self.by_id.remove(&id) else {
            return Ok(None);
        };
        let addr = entry.bp.addr;
        if entry.bp.kind == BreakpointKind::Condition {
            self.pred_ids.remove(&id);
        } else {
            self.addr_index.remove(&addr);
        }
        Ok(Some(addr))
    }

    /// 启用 / 禁用断点。返回 `true` 表示状态真的变了。
    ///
    /// **幂等**：id 不存在时返回 `Ok(false)`（同上）。
    pub fn set_enabled(&mut self, id: u32, enabled: bool) -> Result<bool, PlatformError> {
        let Some(entry) = self.by_id.get_mut(&id) else {
            return Ok(false);
        };
        if entry.bp.enabled == enabled {
            return Ok(false);
        }
        entry.bp.enabled = enabled;
        Ok(true)
    }

    /// **命中检测** —— 唯一的**地址匹配**入口。
    ///
    /// 挂在取指之前：命中时该条指令尚未执行，状态无副作用。
    /// 禁用的断点**不命中**（M-08 LLD §3.2「禁用断点不命中」）。
    ///
    /// ⚠️ **只看地址与 `enabled`** ⇒ ⇒ **无条件断点**的语义与 v0.56 批
    /// **逐字相同**（K-14）。观察式条目**没有地址** ⇒ ⇒ **不进**这条路径。
    ///
    /// ⚠️ **带条件**的那一条走 [`Self::hit_check_cond`]（在**本方法**的
    /// 命中**之上**再判条件）⇒ ⇒ 「地址匹配」仍然只有**这一个**入口。
    pub fn hit_check(&self, pc: u32) -> Option<Breakpoint> {
        Some(self.hit_entry(pc)?.bp.clone())
    }

    /// **带条件**的命中检测 —— 在「地址匹配」之上再判条件。
    ///
    /// * **无条件断点**（`condition == None`）⇒ ⇒ 与 [`Self::hit_check`]
    ///   **逐字相同**；
    /// * **条件为真** ⇒ ⇒ 命中；**条件为假 / 求值失败** ⇒ ⇒ 不命中。
    ///
    /// ⚠️ **求值时刻 = 取指之前** ⇒ ⇒ `gpr` 里的值是**断点地址那条指令
    /// 执行前**的值（裁决 H）。
    ///
    /// ⚠️ **这一条里的内存读当场读总线**（STAGE-55 表 6 第 2 行）：地址命中
    /// 的频率极低（一个地址一条指令最多一次）⇒ ⇒ 成本可以忽略 ⇒ ⇒ 不必
    /// 为它把求值挪到 store 边界（挪了反而会让「地址 + 条件」的语义变成
    /// 「停在写之后」，与地址断点的心智模型冲突）。
    pub fn hit_check_cond(
        &self,
        pc: u32,
        gpr: &[u32; 32],
        mem: &dyn cond::MemReader,
    ) -> Option<Breakpoint> {
        let entry = self.hit_entry(pc)?;
        let hit = match &entry.cond {
            None => true,
            // ⚠️ **求值失败（`None`）= 未命中**（不把「读不到」当成结论）。
            Some(expr) => expr.eval(gpr, pc, mem).is_some_and(|value| value != 0),
        };
        if !hit {
            return None;
        }
        entry.hits.set(entry.hits.get() + 1);
        Some(entry.bp.clone())
    }

    /// **观察式条目**的命中检测（取指之前那一支）。
    ///
    /// 只遍历 `Condition` 条目（`pred_ids` 派生索引）⇒ ⇒ 地址断点**不**
    /// 走这条路径 ⇒ ⇒ 两条路径**互不干扰**。
    ///
    /// ⚠️ **只收「不读内存」的条目**：含内存读的条目走
    /// [`Self::hit_check_pred_at_store`]（STAGE-55 表 6）⇒ ⇒ 同一时刻不会被
    /// 两条路径各求值一次（命中计数不会双加）。
    ///
    /// ⚠️ **按 id 升序**遍历，**首个**为真的条目即返回 ⇒ ⇒ 多条同时为真
    /// 时只停一条（与观察点 `match_access` 取首条**同口径**）。
    /// ⚠️ **禁用的条目不命中、不计数**。
    pub fn hit_check_pred(
        &self,
        pc: u32,
        gpr: &[u32; 32],
        mem: &dyn cond::MemReader,
    ) -> Option<Breakpoint> {
        for id in &self.pred_ids {
            let Some(entry) = self.by_id.get(id) else {
                continue;
            };
            if !entry.bp.enabled {
                continue;
            }
            let Some(expr) = entry.cond.as_ref() else {
                continue;
            };
            if expr.reads_memory() {
                continue;
            }
            if expr.eval(gpr, pc, mem).is_some_and(|value| value != 0) {
                entry.hits.set(entry.hits.get() + 1);
                return Some(entry.bp.clone());
            }
        }
        None
    }

    /// **store 边界**的观察式命中检测（含内存读的条目）。
    ///
    /// `writes` 是**这条指令刚刚成功提交的写区间**（`(地址, 宽度)`，宽度以
    /// **字节**计）。判定分两层：
    ///
    /// 1. **区间过滤**（等价优化，不是近似）：条目引用的每个地址区间都与
    ///    本次写区间**不相交** ⇒ ⇒ 它读到的每个值都不可能变 ⇒ ⇒ 跳过。
    ///    ⚠️ 前提是「store 不改 GPR」（RV32 的 S 型没有 `rd`）。
    /// 2. **引用 `pc` 的条目不做过滤**：提交后 `pc` 已推进 ⇒ ⇒ 过滤不再
    ///    等价 ⇒ ⇒ 每条 store 都求值（保守）。
    ///
    /// ⚠️ 求值失败（`None`）= **本条未命中**，不是「为假」。
    pub fn hit_check_pred_at_store(
        &self,
        pc: u32,
        gpr: &[u32; 32],
        mem: &dyn cond::MemReader,
        writes: &[(u32, u32)],
    ) -> Option<Breakpoint> {
        for id in &self.pred_ids {
            let Some(entry) = self.by_id.get(id) else {
                continue;
            };
            if !entry.bp.enabled {
                continue;
            }
            let Some(expr) = entry.cond.as_ref() else {
                continue;
            };
            if !expr.reads_memory() {
                continue;
            }
            // 区间过滤：不相交且不引用 pc ⇒ 值不可能变 ⇒ 跳过。
            if !expr.reads_pc() {
                let loads = expr.load_intervals(gpr, pc);
                if !loads.iter().any(|load| overlaps(*load, writes)) {
                    continue;
                }
            }
            if expr.eval(gpr, pc, mem).is_some_and(|value| value != 0) {
                entry.hits.set(entry.hits.get() + 1);
                return Some(entry.bp.clone());
            }
        }
        None
    }

    /// 命中检测的**内部**实现：按地址取条目，并剔除**禁用**的断点。
    ///
    /// ⚠️ 「地址匹配 + 启用」这条判定**只有这一个落点** ⇒ ⇒
    /// [`Self::hit_check`] 与 [`Self::hit_check_cond`] 不会漂移。
    fn hit_entry(&self, pc: u32) -> Option<&Entry> {
        let id = *self.addr_index.get(&pc)?;
        let entry = self.by_id.get(&id)?;
        if !entry.bp.enabled {
            return None;
        }
        Some(entry)
    }

    /// 按 id 取断点。
    pub fn get(&self, id: u32) -> Option<&Breakpoint> {
        self.by_id.get(&id).map(|entry| &entry.bp)
    }

    /// 按地址取断点。
    ///
    /// ⚠️ 观察式条目**不在**地址索引里 ⇒ ⇒ 这里**永远**取不到它们。
    pub fn get_by_addr(&self, addr: u32) -> Option<&Breakpoint> {
        let id = *self.addr_index.get(&addr)?;
        self.by_id.get(&id).map(|entry| &entry.bp)
    }

    /// 断点数量（含观察式条目）。
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// 表是否为空。
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// 按地址升序遍历**地址类**断点，随后按 id 升序遍历**观察式**条目。
    ///
    /// ⚠️ 顺序与 v0.124 之前**逐字相同**（地址升序）；观察式条目没有地址
    /// ⇒ ⇒ 排在最后（v0.124 之前根本没有这类条目 ⇒ ⇒ 旧行为不变）。
    pub fn iter(&self) -> impl Iterator<Item = &Breakpoint> {
        self.addr_index
            .values()
            .filter_map(|id| self.by_id.get(id))
            .chain(self.pred_ids.iter().filter_map(|id| self.by_id.get(id)))
            .map(|entry| &entry.bp)
    }

    /// **只读回执**：返回「断点 + 命中次数」（契约 v13）。
    ///
    /// ⚠️ 顺序同 [`Self::iter`]（地址升序，观察式条目排最后）。
    pub fn infos(&self) -> Vec<BreakpointInfo> {
        self.addr_index
            .values()
            .filter_map(|id| self.by_id.get(id))
            .chain(self.pred_ids.iter().filter_map(|id| self.by_id.get(id)))
            .map(|entry| BreakpointInfo {
                breakpoint: entry.bp.clone(),
                hit_count: entry.hits.get(),
                degraded: entry.degraded.is_some(),
            })
            .collect()
    }

    /// 某个 id 的命中次数（未知 id ⇒ `None`）。
    pub fn hit_count(&self, id: u32) -> Option<u64> {
        self.by_id.get(&id).map(|entry| entry.hits.get())
    }

    /// **标降级**：注册期动态求值失败时由**调用方**（后端）打标。
    ///
    /// ⚠️ 表自己**不知道**内存长什么样（它没有总线）⇒ ⇒ 「这条表达式现在
    /// 求不出来」这件事只能由拿得到内存的人告诉我们 ⇒ ⇒ 这里**只存**原因。
    ///
    /// ⚠️ **降级 ≠ 失效**：条目照常参与判定，只是**求值期失败按未命中**
    /// （STAGE-55 表 7）。
    ///
    /// 返回 `false` 表示 id 不存在（不报错）。
    pub fn mark_degraded(&mut self, id: u32, reason: impl Into<String>) -> bool {
        match self.by_id.get_mut(&id) {
            Some(entry) => {
                entry.degraded = Some(reason.into());
                true
            }
            None => false,
        }
    }

    /// 记一次 **store 边界命中**的触发指令地址（`exec_pc`）。
    pub fn note_store_trigger(&self, exec_pc: u32) {
        self.store_trigger.set(Some(exec_pc));
    }

    /// 取走「最后一次 store 边界命中」的触发地址（没有 ⇒ `None`）。
    pub fn last_store_trigger(&self) -> Option<u32> {
        self.store_trigger.get()
    }

    /// 清空触发地址槽（每步开始 / 会话重置时调）。
    pub fn clear_store_trigger(&self) {
        self.store_trigger.set(None);
    }

    /// 某个 id 的**已绑定条件**（无条件的条目 / 未知 id ⇒ `None`）。
    ///
    /// ⚠️ 给**注册期试求值**用（STAGE-55 表 7）：调用方拿得到内存 ⇒ ⇒ 由它
    /// 判断「这条表达式当下求不求得出来」⇒ ⇒ 求不出来就
    /// [`Self::mark_degraded`]。
    pub fn bound_condition(&self, id: u32) -> Option<&cond::BoundCondition> {
        self.by_id.get(&id)?.cond.as_ref()
    }

    /// 某个 id 的**降级原因**（没降级 / 未知 id ⇒ `None`）。
    pub fn degraded_reason(&self, id: u32) -> Option<&str> {
        self.by_id.get(&id)?.degraded.as_deref()
    }

    /// 清空整张表（会话重置用）。
    pub fn clear_all(&mut self) {
        self.by_id.clear();
        self.addr_index.clear();
        self.pred_ids.clear();
        self.store_trigger.set(None);
    }

    /// **派生索引一致性**（调试 / 测试用）：两个索引都必须能由 `by_id`
    /// 完整重算。
    fn indexes_consistent(&self) -> bool {
        let addr_index: BTreeMap<u32, u32> = self
            .by_id
            .iter()
            .filter(|(_, entry)| entry.bp.kind != BreakpointKind::Condition)
            .map(|(id, entry)| (entry.bp.addr, *id))
            .collect();
        let pred_ids: BTreeSet<u32> = self
            .by_id
            .iter()
            .filter(|(_, entry)| entry.bp.kind == BreakpointKind::Condition)
            .map(|(id, _)| *id)
            .collect();
        addr_index == self.addr_index && pred_ids == self.pred_ids
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
            kind: BreakpointKind::Address,
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
            kind: BreakpointKind::Address,
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
            t.hit_check_cond(0x8000_0000, &gpr, &cond::NoMem).is_some(),
            "条件为真 ⇒ 命中"
        );
        gpr[1] = 6;
        assert!(
            t.hit_check_cond(0x8000_0000, &gpr, &cond::NoMem).is_none(),
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
            t.hit_check_cond(0x8000_0000, &gpr, &cond::NoMem)
                .map(|b| b.id),
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
        assert!(t.hit_check_cond(0x8000_0000, &gpr, &cond::NoMem).is_none());
    }

    /// **条件里看到的是「该指令执行前」的值**（裁决 H）—— 这里用「PC」把
    /// 求值时刻钉住：命中时 PC **就是**断点地址。
    #[test]
    fn the_condition_sees_the_pre_execution_pc() {
        let mut t = BreakpointTable::new();
        t.set(bp_cond(1, 0x8000_0000, "pc == 0x80000000"))
            .expect("set");
        let gpr = [0u32; 32];
        assert!(t.hit_check_cond(0x8000_0000, &gpr, &cond::NoMem).is_some());
        assert!(t.hit_check_cond(0x8000_0004, &gpr, &cond::NoMem).is_none());
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

    // -- 契约 v13：只读回执 / 命中计数 --------------------------------------

    #[test]
    fn infos_lists_by_addr_with_zero_hits() {
        let mut t = BreakpointTable::new();
        t.set(bp(2, 0x8000_0020)).expect("set");
        t.set(bp(1, 0x8000_0010)).expect("set");
        let infos = t.infos();
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].breakpoint.addr, 0x8000_0010, "回执按地址升序");
        assert_eq!(infos[1].breakpoint.addr, 0x8000_0020);
        assert_eq!(infos[0].hit_count, 0, "未命中过 ⇒ 0");
    }

    #[test]
    fn hit_check_cond_increments_only_on_true_hit() {
        let mut t = BreakpointTable::new();
        t.set(bp_cond(1, 0x8000_0010, "a0 != 0")).expect("set");
        let mut gpr = [0u32; 32];
        assert!(
            t.hit_check_cond(0x8000_0010, &gpr, &cond::NoMem).is_none(),
            "条件假不命中"
        );
        assert_eq!(t.hit_count(1), Some(0), "条件假不能计数");
        gpr[10] = 7;
        assert!(t.hit_check_cond(0x8000_0010, &gpr, &cond::NoMem).is_some());
        assert_eq!(t.hit_count(1), Some(1));
        assert!(t.hit_check_cond(0x8000_0010, &gpr, &cond::NoMem).is_some());
        assert_eq!(t.hit_count(1), Some(2));
    }

    #[test]
    fn disabled_breakpoint_does_not_count_hits() {
        let mut t = BreakpointTable::new();
        t.set(bp(1, 0x8000_0010)).expect("set");
        assert!(t.set_enabled(1, false).expect("enable"));
        let gpr = [0u32; 32];
        assert!(t.hit_check_cond(0x8000_0010, &gpr, &cond::NoMem).is_none());
        assert_eq!(t.hit_count(1), Some(0), "禁用断点不命中 ⇒ 不计数");
    }

    // -- STAGE-55：观察式条件断点（表侧） ------------------------------------

    /// 造一条**观察式**条目：没有真实地址，`addr` 只是 `0` 占位。
    fn bp_pred(id: u32, condition: &str) -> Breakpoint {
        Breakpoint {
            id,
            addr: 0,
            source_hint: None,
            enabled: true,
            kind: BreakpointKind::Condition,
            condition: Some(condition.to_string()),
        }
    }

    /// 一张**假的符号表**（批 B 的绑定测试用）。
    struct FakeNames;

    impl names::NameResolver for FakeNames {
        fn resolve(&self, name: &str) -> Option<names::NameRef> {
            match name {
                "main" => Some(names::NameRef {
                    class: names::NameClass::Function,
                    target: names::NameTarget::Addr(0x1000),
                }),
                "g_counter" => Some(names::NameRef {
                    class: names::NameClass::Object,
                    target: names::NameTarget::Addr(0x2000),
                }),
                _ => None,
            }
        }
    }

    // ================= 批 D · 局部变量的**帧上下文**接线 =================

    /// 一张**带帧**的假符号表：`counter` 在 `helper`(0x0..0x38) 是 x11、
    /// 在 `main`(0x38..0x78) 是 x12 ⇒ ⇒ 光凭名字分不出来。
    struct ScopedNames;

    impl names::NameResolver for ScopedNames {
        fn resolve(&self, name: &str) -> Option<names::NameRef> {
            match name {
                "main" => Some(names::NameRef {
                    class: names::NameClass::Function,
                    target: names::NameTarget::Addr(0x1000),
                }),
                _ => None,
            }
        }

        fn resolve_at(
            &self,
            name: &str,
            pc: Option<u32>,
        ) -> Result<names::NameRef, names::ResolveError> {
            if name != "counter" {
                return self.resolve(name).ok_or(names::ResolveError::Undefined);
            }
            let slot = match pc {
                Some(pc) if (0x00..0x38).contains(&pc) => names::LocalSlot::Reg(11),
                Some(pc) if (0x38..0x78).contains(&pc) => names::LocalSlot::Reg(12),
                _ => {
                    return Err(names::ResolveError::Ambiguous(vec![
                        "helper".to_string(),
                        "main".to_string(),
                    ]))
                }
            };
            Ok(names::NameRef {
                class: names::NameClass::Local,
                target: names::NameTarget::Local(slot),
            })
        }
    }

    /// **地址类断点按自己的地址当帧上下文**（批 D 的接线点）。
    ///
    /// ⚠️ 这条钉的是 `set_with` **把 `bp.addr` 传下去了**：不传的话，同名
    /// 局部变量会一路退化成「只凭名字」⇒ ⇒ 报歧义、注册被拒。
    #[test]
    fn an_address_breakpoint_binds_its_condition_against_its_own_frame() {
        let mut t = BreakpointTable::new();
        t.set_with(bp_cond(1, 0x10, "counter == 1"), &ScopedNames)
            .expect("helper 里的地址断点应当能解析自己的局部变量");
        let bound = t.bound_condition(1).expect("要绑上");
        // helper ⇒ x11。
        let mut gpr = [0u32; 32];
        gpr[11] = 1;
        gpr[12] = 0;
        assert_eq!(bound.eval(&gpr, 0x10, &cond::NoMem), Some(1));
        gpr[11] = 0;
        gpr[12] = 1;
        assert_eq!(
            bound.eval(&gpr, 0x10, &cond::NoMem),
            Some(0),
            "0x10 处必须读 x11（不能读到 main 的 x12）"
        );
    }

    /// **观察式条目没有帧上下文** ⇒ 同名局部变量报歧义（消息要能被用户读懂）。
    #[test]
    fn a_condition_entry_without_a_frame_reports_the_ambiguity() {
        let mut t = BreakpointTable::new();
        let err = t
            .set_with(bp_pred(1, "counter == 1"), &ScopedNames)
            .expect_err("无地址的观察式条目不该随手挑一个同名局部变量");
        assert!(
            err.message.contains("在多个函数里都有")
                && err.message.contains("helper")
                && err.message.contains("main"),
            "错误消息要点名两个函数：{}",
            err.message
        );
        assert_eq!(t.len(), 0, "失败不该留下半条表项");
    }

    /// **带符号上下文注册**：`main == 0x1000` 能落表，并按函数地址求值。
    #[test]
    fn a_named_condition_binds_against_the_resolver() {
        let mut t = BreakpointTable::new();
        t.set_with(bp_pred(3, "main == 0x1000"), &FakeNames)
            .expect("函数名能绑成地址常量");
        assert_eq!(t.get(3).map(|b| b.kind), Some(BreakpointKind::Condition));
        let gpr = [0u32; 32];
        let hit = t
            .hit_check_pred(0x2000, &gpr, &cond::NoMem)
            .expect("谓词为真 ⇒ 命中");
        assert_eq!(hit.id, 3);
    }

    /// **没有符号上下文**时同名表达式 ⇒ 如实报「未定义符号」，表不变。
    #[test]
    fn a_named_condition_without_symbols_is_rejected() {
        let mut t = BreakpointTable::new();
        let err = t
            .set(bp_pred(4, "main == 0x1000"))
            .expect_err("没有符号表就没法定下 main");
        assert_eq!(
            err.scenario, E_DBG_02_CONDITION_INVALID,
            "名字解析失败仍是 E_DBG-02：{err}"
        );
        assert!(
            err.message.contains("未定义符号") && err.message.contains("main"),
            "错误消息要指出哪个名字没定义：{}",
            err.message
        );
        assert_eq!(t.len(), 0, "校验失败时表不变");
    }

    /// **变量名（要读内存）现在能注册**（档 3：批 C）。
    ///
    /// ⚠️ 它**不会**在取指前求值（归 store 边界）⇒ ⇒ 这里只钉「注册成功 +
    ///    绑成了读 0x2000」这两件事，执行时刻由 `a_memory_predicate_...` 钉。
    #[test]
    fn a_variable_condition_now_registers_as_a_memory_read() {
        let mut t = BreakpointTable::new();
        t.set_with(bp_pred(5, "g_counter == 3"), &FakeNames)
            .expect("变量名是合法表达式（档 3）");
        assert_eq!(t.len(), 1);
        let cond = t.bound_condition(5).expect("绑好的条件必须在表里");
        assert!(cond.reads_memory(), "变量名 ⇒ 读内存");
        assert_eq!(cond.load_intervals(&[0u32; 32], 0), vec![(0x2000, 4)]);
    }

    /// **没有「值」语义的符号（`STT_NOTYPE`）仍拒**：它只许 `&名字`。
    #[test]
    fn a_symbol_without_a_value_is_still_rejected_as_a_value() {
        let mut t = BreakpointTable::new();
        let names = FakeNamesOnlyOther;
        let err = t
            .set_with(bp_pred(6, "_start == 3"), &names)
            .expect_err("未分类符号没有值");
        assert_eq!(err.scenario, E_DBG_02_CONDITION_INVALID);
        assert!(
            err.message.contains("不是已知的变量") && err.message.contains("&_start"),
            "错误消息要说清「没有值」并给 &名字 写法：{}",
            err.message
        );
        assert_eq!(t.len(), 0);
    }

    /// 只认识一个 `STT_NOTYPE` 符号（`Other` 分类）的假符号表。
    struct FakeNamesOnlyOther;

    impl names::NameResolver for FakeNamesOnlyOther {
        fn resolve(&self, name: &str) -> Option<names::NameRef> {
            (name == "_start").then_some(names::NameRef {
                class: names::NameClass::Other,
                target: names::NameTarget::Addr(0x0100),
            })
        }
    }

    /// **观察式条目不吃地址槽**：它没有地址，所以地址 `0` 的真实断点
    /// **仍然**能注册（也就是「单表 + 派生索引」必须成立的理由）。
    #[test]
    fn a_condition_entry_does_not_occupy_an_address_slot() {
        let mut t = BreakpointTable::new();
        t.set(bp_pred(1, "x1 == 5")).expect("观察式条目应当能注册");
        t.set(bp(2, 0))
            .expect("地址 0 的地址断点不应被观察式条目挡住");
        assert_eq!(t.len(), 2, "两条都在表里");
        assert_eq!(t.get_by_addr(0).map(|b| b.id), Some(2), "地址槽归地址断点");
        assert_eq!(t.get(1).map(|b| b.kind), Some(BreakpointKind::Condition));
        assert!(t.indexes_consistent(), "派生索引必须与 by_id 一致");
    }

    /// **谓词为真 ⇒ 命中；为假 ⇒ 不命中**（观察式路径）。
    #[test]
    fn the_condition_entry_hits_only_when_the_predicate_is_true() {
        let mut t = BreakpointTable::new();
        t.set(bp_pred(1, "x1 == 5")).expect("set");
        let mut gpr = [0u32; 32];
        gpr[1] = 5;
        assert_eq!(
            t.hit_check_pred(0x8000_0000, &gpr, &cond::NoMem)
                .map(|b| b.id),
            Some(1),
            "谓词真 ⇒ 命中"
        );
        assert_eq!(t.hit_count(1), Some(1));
        gpr[1] = 6;
        assert!(
            t.hit_check_pred(0x8000_0000, &gpr, &cond::NoMem).is_none(),
            "谓词假 ⇒ 不停"
        );
        assert_eq!(t.hit_count(1), Some(1), "谓词假不得计数");
    }

    /// **观察式条目与 PC 无关**：同一条谓词在**任何** `pc` 上都能命中。
    #[test]
    fn a_condition_entry_is_independent_of_pc() {
        let mut t = BreakpointTable::new();
        t.set(bp_pred(1, "x1 == 5")).expect("set");
        let mut gpr = [0u32; 32];
        gpr[1] = 5;
        for pc in [0x8000_0000u32, 0x8000_0004, 0x1000_0000] {
            assert!(
                t.hit_check_pred(pc, &gpr, &cond::NoMem).is_some(),
                "观察式条目不该受 pc 限制：0x{pc:08x}"
            );
        }
    }

    /// **禁用的观察式条目**不命中、不计数。
    #[test]
    fn a_disabled_condition_entry_never_hits() {
        let mut t = BreakpointTable::new();
        t.set(bp_pred(1, "x1 == 5")).expect("set");
        t.set_enabled(1, false).expect("disable");
        let mut gpr = [0u32; 32];
        gpr[1] = 5;
        assert!(t.hit_check_pred(0x8000_0000, &gpr, &cond::NoMem).is_none());
        assert_eq!(t.hit_count(1), Some(0), "禁用 ⇒ 不计数");
    }

    /// **观察式条目必须带表达式** ⇒ ⇒ 空表达式按 `E_DBG-02` 原样拒。
    #[test]
    fn a_condition_entry_without_an_expression_is_rejected() {
        let mut t = BreakpointTable::new();
        let err = t
            .set(Breakpoint {
                id: 1,
                kind: BreakpointKind::Condition,
                ..Breakpoint::default()
            })
            .expect_err("空表达式必须被拒");
        assert_eq!(err.family, ErrorFamily::Debug);
        assert_eq!(err.scenario, E_DBG_02_CONDITION_INVALID);
        assert!(t.is_empty(), "被拒的注册不得留下痕迹");
    }

    /// **地址校验仍然先于条件校验**（既有纪律一字未改）。
    #[test]
    fn the_address_check_still_wins_over_the_condition_check() {
        let mut t = BreakpointTable::new();
        let err = t
            .set(Breakpoint {
                id: 1,
                addr: 0x8000_0001,
                kind: BreakpointKind::Address,
                condition: Some("foo".to_string()),
                ..Breakpoint::default()
            })
            .unwrap_err();
        assert_eq!(
            err.scenario, E_CTX_07_INVALID_OR_DUPLICATE,
            "地址非法 + 条件非法 ⇒ 报的必须是地址的错"
        );
        assert!(t.is_empty());
    }

    /// **删除 / 启停之后派生索引仍然一致**（这是「单表 + 派生索引」的
    /// 唯一真正风险点）。
    #[test]
    fn clear_and_enable_keep_the_derived_indexes_consistent() {
        let mut t = BreakpointTable::new();
        t.set(bp(2, 0x8000_0000)).expect("地址条目");
        t.set(bp_pred(1, "x1 == 5")).expect("观察式条目");
        assert!(t.indexes_consistent());

        t.set_enabled(1, false).expect("disable");
        assert!(t.indexes_consistent());

        assert_eq!(t.clear(1).unwrap(), Some(0), "观察式条目没有真实地址");
        assert!(t.indexes_consistent(), "删掉观察式条目后索引必须一致");
        assert_eq!(t.len(), 1);

        assert_eq!(t.clear(2).unwrap(), Some(0x8000_0000));
        assert!(t.indexes_consistent());
        assert!(t.is_empty());
    }

    // -- 批 C（档 3）：内存读 / store 边界 / 降级 ---------------------------

    /// 一张**假内存**：`addr → word`（表外地址 ⇒ 读不到 ⇒ 求值失败）。
    struct FakeMem(Vec<(u32, u32)>);

    impl cond::MemReader for FakeMem {
        fn read_word(&self, addr: u32) -> Option<u32> {
            self.0
                .iter()
                .find(|(candidate, _)| *candidate == addr)
                .map(|(_, value)| *value)
        }
    }

    /// **含内存读的条目不在取指前求值**（STAGE-55 表 6）：它归 store 边界
    /// ⇒ ⇒ 否则同一时刻两条路径都会算一遍，命中计数会双加。
    #[test]
    fn a_memory_predicate_is_not_evaluated_before_the_fetch() {
        let mut t = BreakpointTable::new();
        t.set_with(bp_pred(5, "g_counter == 7"), &FakeNames)
            .expect("变量名能绑成「读 0x2000」");
        let gpr = [0u32; 32];
        let mem = FakeMem(vec![(0x2000, 7)]);
        // 取指前：**跳过**（哪怕内存里已经是 7）。
        assert!(t.hit_check_pred(0x2000, &gpr, &mem).is_none());
        // store 边界：写到了 0x2000 ⇒ 求值 ⇒ 命中。
        let hit = t
            .hit_check_pred_at_store(0x2004, &gpr, &mem, &[(0x2000, 4)])
            .expect("写后新值是 7 ⇒ 命中");
        assert_eq!(hit.id, 5);
        assert_eq!(t.hit_count(5), Some(1), "只算一次（没被两条路径各算一遍）");
    }

    /// **区间过滤是等价优化**：写一段不相交的地址 ⇒ 谓词读到的值不可能变
    /// ⇒ ⇒ 跳过（命中计数不动）。
    #[test]
    fn a_disjoint_write_is_filtered_out() {
        let mut t = BreakpointTable::new();
        t.set_with(bp_pred(6, "g_counter == 0"), &FakeNames)
            .expect("绑成「读 0x2000」");
        let gpr = [0u32; 32];
        // 内存里是 0 ⇒ 谓词为真 ⇒ 若真求值就会命中。
        let mem = FakeMem(vec![(0x2000, 0)]);
        assert!(
            t.hit_check_pred_at_store(0x1004, &gpr, &mem, &[(0x1000, 4)])
                .is_none(),
            "写到 0x1000 不影响 0x2000 的值"
        );
        assert_eq!(t.hit_count(6), Some(0));
        // 写到 0x2000 ⇒ 不再过滤 ⇒ 命中。
        assert!(t
            .hit_check_pred_at_store(0x1004, &gpr, &mem, &[(0x2000, 4)])
            .is_some());
        assert_eq!(t.hit_count(6), Some(1));
    }

    /// **引用 pc 的条目不做区间过滤**（提交后 pc 已变 ⇒ 过滤不再等价）。
    #[test]
    fn a_pc_referencing_predicate_ignores_the_filter() {
        let mut t = BreakpointTable::new();
        t.set_with(bp_pred(7, "pc == 0x1000 && g_counter == 0"), &FakeNames)
            .expect("pc 子式是合法语法");
        let gpr = [0u32; 32];
        let mem = FakeMem(vec![(0x2000, 0)]);
        // 写的是 0x1000（与「读 0x2000」不相交）——但条目引用了 pc ⇒ 必须
        // 照常求值（这里 pc 传 0x1000 ⇒ 前半为真、后半为真 ⇒ 命中）。
        assert!(
            t.hit_check_pred_at_store(0x1000, &gpr, &mem, &[(0x1000, 4)])
                .is_some(),
            "引用 pc 的条目不许被区间过滤跳过"
        );
    }

    /// **读不到内存 ⇒ 未命中**（不是「为假」也不是错误）。
    #[test]
    fn an_unreadable_memory_predicate_is_a_miss() {
        let mut t = BreakpointTable::new();
        t.set_with(bp_pred(8, "g_counter == 0"), &FakeNames)
            .expect("绑成「读 0x2000」");
        let gpr = [0u32; 32];
        // 内存里读不到 0x2000（空表）⇒ 求值失败 ⇒ 未命中。
        assert!(t
            .hit_check_pred_at_store(0x2004, &gpr, &FakeMem(vec![]), &[(0x2000, 4)])
            .is_none());
        assert_eq!(t.hit_count(8), Some(0));
    }

    /// **降级标记**：注册期求不出来 ⇒ 允许注册、回执里带 `degraded`。
    #[test]
    fn a_degraded_entry_still_registers_and_shows_in_the_receipt() {
        let mut t = BreakpointTable::new();
        t.set_with(bp_pred(9, "g_counter == 0"), &FakeNames)
            .expect("静态上没问题");
        assert!(t.mark_degraded(9, "注册时读不到 0x2000"));
        assert_eq!(t.degraded_reason(9), Some("注册时读不到 0x2000"));
        let info = t
            .infos()
            .into_iter()
            .find(|info| info.breakpoint.id == 9)
            .expect("回执里必须有它");
        assert!(info.degraded, "降级标记要跟着回执出去");
        // 未知 id ⇒ `false`（不报错）。
        assert!(!t.mark_degraded(404, "nope"));
    }

    /// **回执顺序与 v0.124 之前逐字相同**：地址升序在前，观察式条目收尾。
    #[test]
    fn infos_keep_the_address_order_and_put_condition_entries_last() {
        let mut t = BreakpointTable::new();
        t.set(bp(3, 0x8000_0008)).expect("set");
        t.set(bp(1, 0x8000_0000)).expect("set");
        t.set(bp_pred(9, "x1 == 5")).expect("set");
        let ids: Vec<u32> = t.infos().into_iter().map(|i| i.breakpoint.id).collect();
        assert_eq!(ids, vec![1, 3, 9], "地址升序在前，观察式排最后");
    }
}
