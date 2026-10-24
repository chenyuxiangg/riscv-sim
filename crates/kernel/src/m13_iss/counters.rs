//! Zicntr 硬件计数器 —— `mcycle` / `minstret` 与它们的影子。
//!
//! 冻结规格：`docs/detailed-design/STAGE-20_V036_Zicntr_Counters.md`
//! §3.1~§3.6（内核设计）与 §1.2（参考侧十四条**实测**事实 R1~R14）。
//!
//! # 三个「一个约束一个落点」
//!
//! | 问题 | 答案住在哪 | 本模块怎么用 |
//! | --- | --- | --- |
//! | 一个计数器是**哪两个 CSR**？ | 契约层 [`counter_pair`] | **只引用**，自己不写 `match` |
//! | 一个影子 CSR **跟谁**？ | 契约层 [`counter_shadow`] | 只在**读**路径用（[`super::csr::csr_read`]） |
//! | 自增**在哪**发生？ | [`super::executor::execute_one`] 的**正常完成出口** | 本模块提供 [`retire`]，由那**一处**调用 |
//!
//! ⚠️ **半部配对只有契约层那一份**（裁决 C）—— 本模块**不许**自己写
//! 「`0xB00` 与 `0xB80` 是一对」这种 `match` ⇒ ⇒ 「一个计数器是几半」
//! **只有一处答案** ⇒ ⇒ 不会与影子表漂移。
//!
//! # 计数器的**物理落点**
//!
//! 一个 64 位计数器 = **CSR 文件里的两个 `u32` 条目**（低半部 + 高半部），
//! 加进位 ⇒ ⇒ 与 `u64` **逐位等价**（规格 §3.1 / 裁决 I）。
//!
//! | 计数器 | 低半部 | 高半部 |
//! | --- | --- | --- |
//! | `mcycle` | `0xB00`（[`CSR_MCYCLE`]） | `0xB80` |
//! | `minstret` | `0xB02`（[`CSR_MINSTRET`]） | `0xB82` |
//!
//! ⚠️ **为什么不加 `SimState` 字段**：那会触发 `SIM_STATE_SCHEMA_VERSION`
//! **2 → 3** ⇒ ⇒ 契约层 + 前端镜像 + 快照 DTO 三处都要动 ⇒ ⇒ 本批明确
//! **不升版**（裁决 B / X-3 / X-5）⇒ ⇒ CSR 文件本来就是快照的一部分 ⇒ ⇒
//! 会话往返**免费**。
//!
//! # 抑制：**写计数器的那条指令自己**不** +1
//!
//! 规格 §3.3 / 裁决 H：抑制**按计数器**分 ——
//!
//! | 本指令写的 CSR | 抑制谁本指令的 +1 |
//! | --- | --- |
//! | `mcycle` / `mcycleh` | **只**抑制 `mcycle` |
//! | `minstret` / `minstreth` | **只**抑制 `minstret` |
//! | 其它（含全部影子） | **不抑制** |
//!
//! ⚠️ 判定看的是**低半部是谁** ⇒ ⇒ 写**高**半部**也**抑制（R13）。
//!
//! ⚠️ 影子 CSR 的写被**只读规则**拦住（契约层 `is_csr_read_only`）⇒ ⇒
//! 根本进不了 CSR 文件 ⇒ ⇒ `wrote_csr` **不会**因影子写而置位 ⇒ ⇒
//! 影子写**天然不抑制**（R7）—— 这里**不需要**任何特判。
//!
//! ⚠️ 抑制的粒度是**本指令** ⇒ ⇒ 之后照常 +1（R13 的实测：
//! `Q_W_MINSTRETH_ONLY_NOP2` → **12** 比 `Q_W_MINSTRETH_ONLY` → **11** 多 1）。

use riscv_sim_contract::csr::{counter_pair, CSR_MCYCLE, CSR_MINSTRET};
use riscv_sim_contract::model::SimState;

use super::csr;

/// Account for **one retired instruction**.
///
/// 这是**自增的唯一落点**的**实现**（规格 §3.2 / 裁决 F）—— 调用它的地方
/// **只有一处**：[`super::executor::execute_one`] 的**正常完成出口**。
///
/// ⚠️ **为什么落在那里**（规格 §3.2 的三条理由，逐条可验）：
///
/// 1. **两条 dispatch 路径都经 `execute_one`** ⇒ ⇒ 闭包口
///    ([`super::Iss::step_one`]) 与总线口 ([`super::Iss::step_one_via_bus`])
///    **共用**这个函数 ⇒ ⇒ 不可能两条路径行为不一致。
/// 2. **陷阱指令天然被排除**（裁决 G）—— `execute_one` 的陷阱分支是**提前
///    `return`**（`Ecall` / `Ebreak` / 非法指令 / 非对齐访问 / 访问故障）
///    ⇒ ⇒ **到不了**调用点 ⇒ ⇒ R11 自动成立，不需要额外判「这是不是陷阱」。
/// 3. **读到的值 = 退休前的值**（R5）—— CSR 指令在 `execute_one` **内部**
///    读文件 ⇒ ⇒ 读到的值是**自增前**的。
///
/// # 参数
///
/// * `wrote_csr` —— 本指令写入的 CSR 号（`None` = 没写任何 CSR）。
///   它是 [`super::executor::execute_one`] **已有的形参** ⇒ ⇒ 本模块
///   **零新增状态、零新增参数、零 DTO**（裁决 H）。
///
/// ⚠️ 只有**真的写进去了**才会置位它 ⇒ ⇒ 被忽略的写（只读 / 未实现）
/// **不抑制** —— 与「没写」同义。
pub(super) fn retire(state: &mut SimState, wrote_csr: Option<u16>) {
    // ⚠️ **抑制按计数器分**（R13 / R14）⇒ ⇒ 判定看的是**低半部是谁** ⇒ ⇒
    // 写**高**半部（`mcycleh` / `minstreth`）**也**抑制。
    let suppressed = wrote_csr.and_then(counter_pair).map(|(low, _)| low);

    for low_csr in [CSR_MCYCLE, CSR_MINSTRET] {
        if suppressed == Some(low_csr) {
            // ⚠️ **本指令写了它** ⇒ ⇒ 它本指令**不** +1（R3 / R13）。
            // 抑制的粒度是**本指令** ⇒ ⇒ 下一条指令照常 +1。
            continue;
        }
        // ⚠️ **高半部从契约层拿**（[`counter_pair`]）—— 这里**不**自己写
        // 「`0xB00` 配 `0xB80`」这种字面量（裁决 C）。
        if let Some((_, high_csr)) = counter_pair(low_csr) {
            bump(state, low_csr, high_csr);
        }
    }
}

/// Add **one** to a 64-bit counter stored as **two `u32` halves**.
///
/// ⚠️ **进位**（规格 §3.2 / 裁决 I / R12）：低半部回绕时**高半部跟着 +1**
/// ⇒ ⇒ 与 `u64` 的 `wrapping_add(1)` **逐位等价**。
///
/// ⚠️ **高半部自己也 `wrapping_add`** ⇒ ⇒ 64 位**整体**回绕（高半部从
/// `0xFFFF_FFFF` 回到 `0`）—— 上游 `rv32mi/instret_overflow` 的
/// `TEST_CASE(4)` 正是这条（写 `minstret` = `0xFFFFFFFF`、写 `minstreth`
/// = `0xFFFFFFFF`、一条 `nop` ⇒ 低半部回绕到 `0`、高半部**也**回绕到 `0`）。
fn bump(state: &mut SimState, low_csr: u16, high_csr: u16) {
    let low = csr::csr_read(state, low_csr);
    let new_low = low.wrapping_add(1);
    if low == u32::MAX {
        // ⚠️ **进位**：低半部从 `0xFFFF_FFFF` 回绕 ⇒ ⇒ 高半部 +1。
        let high = csr::csr_read(state, high_csr);
        csr::csr_write_raw(state, high_csr, high.wrapping_add(1));
    }
    csr::csr_write_raw(state, low_csr, new_low);
}
