//! 可观测只读面：M-22 健康 / 降级计数与动作、M-21 日志环快照。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。

use super::image::ParsedImage;
use super::{
    KernelHost, ADMIN_CHANNEL_DISABLED_MESSAGE, ADMIN_DIAG_OP, ADMIN_DRAIN_LOG_CTX,
    ADMIN_DRAIN_MAX_ENTRIES,
};
use crate::m21_logging::{LogEntry, LogLevel, LogQuery};
use crate::m22_hms::{DegradeAction, Health, Role};
use riscv_sim_contract::admin::LogEntryDto;
use riscv_sim_contract::error::{PlatformError, E_PLT_20_FW_ISA_MISMATCH, PLATFORM_FAMILY};
use riscv_sim_contract::platform::{mcu_isa_info, PlatformDescription};
use riscv_sim_contract::protocol::{AdminOp, EventPayload};

impl KernelHost {
    /// 整体健康状态（M-22 派生 · v0.28 · D-115）。
    ///
    /// ⚠️ **派生**语义：有 FATAL 信号 ⇒ `Critical`；只有 CRITICAL / WARNING
    /// ⇒ `Degraded`；否则 `Ok` ⇒ **只降不升**。
    pub fn health(&self) -> Health {
        self.hms.state()
    }

    /// 看门狗是否已经触发过（幂等标志 · M-22）。
    pub fn watchdog_fired(&self) -> bool {
        self.hms.watchdog_fired()
    }

    /// 降级执行器被问过的**总次数**（v0.46 · STAGE-31 · 验收 **C-1 / C-3**）。
    ///
    /// ⚠️ **零** = 降级**没有**走派发路径（M-22 走了自己的内置实现，或者宿主
    /// **没装**执行器）⇒ ⇒ 这个数是「降级真的问过执行器」的**唯一**证据。
    ///
    /// ⚠️ **它数的是「被问了几次」** ⇒ ⇒ 失败会**重试** ⇒ ⇒ 一次失败的降级
    /// 会让这个数**大于 1** ⇒ ⇒ 「不重试」那条判据就是断言它**恰好 1**。
    pub fn degrade_dispatch_total(&self) -> u32 {
        self.degrade_dispatch.borrow().total
    }

    /// 最后一次被问到的**降级动作**（v0.46 · STAGE-31 · 验收 **C-1 / C-3**）。
    ///
    /// ⚠️ `None` = 执行器**一次都没被问过** ⇒ ⇒ 降级**没有**走派发路径。
    pub fn degrade_dispatch_last(&self) -> Option<DegradeAction> {
        self.degrade_dispatch.borrow().last
    }

    /// M-22 的**降级失败**记录（`E_HMS-01` 那一族 · v0.46 · STAGE-31 · 验收 **C-3**）。
    ///
    /// ⚠️ **一次运行只报一次** ⇒ ⇒ 空 ⇒ ⇒ 「降级**没有**失败」⇒ ⇒
    /// `Unsupported`（做不了）**不是**失败 ⇒ ⇒ 它**不会**出现在这里。
    ///
    /// ⚠️ 这条让「降级失败」这件事**在宿主层看得见**（不是只活在 M-22 内部）。
    pub fn degrade_failures(&self) -> &[PlatformError] {
        self.hms.degrade_failures()
    }

    /// M-21 日志环的快照（管理员通道的读出口 · v0.28 · D-115）。
    ///
    /// ⚠️ **纯读**：不改缓冲、不动计数。
    /// ⚠️ 借不到缓冲（别处正持有着可变借用）⇒ **退化成空列表**而不是
    /// panic —— 诊断路径不该把内核打挂。
    pub fn log_entries(&self) -> Vec<LogEntry> {
        self.log_buffer
            .try_borrow()
            .map(|buffer| buffer.query(&LogQuery::default()))
            .unwrap_or_default()
    }

    /// 管理通道未启用的那个错误（**`E_CTX-05` 的唯一构造落点** · §3.3）。
    ///
    /// ⚠️ **码字符串从契约层常量推导**（**一个约束一个落点**）：族名来自
    /// [`riscv_sim_contract::error::CONTEXT_FAMILY`] · 场景号来自
    /// [`riscv_sim_contract::error::E_CTX_05_ADMIN_CHANNEL_DISABLED`]
    /// ⇒ ⇒ 契约层**只**给 `u16`（**不**加第二份字符串常量）⇒ ⇒ 宿主**不许**
    /// 再写裸的 `"E_CTX-05"` 字面量 ⇒ ⇒ **族名与场景号各只有一个落点**。
    ///
    /// ⚠️ **渲染复用宿主既有的那一个渲染点**（[`KernelHost::error_from`] —— 它
    /// 是宿主里**所有**错误码的**唯一**渲染处）⇒ ⇒ 「`E_<族>-<两位号>」这个
    /// 写法在宿主里**只有一处** ⇒ ⇒ 宿主**不**另写第二份 `format!`。
    ///
    /// ⚠️ **渲染结果与旧字面量逐字相同** ⇒ ⇒ **默认行为一字不变**（X-3）。
    pub(crate) fn admin_channel_disabled_error(&self) -> EventPayload {
        self.error_from(&PlatformError::new(
            riscv_sim_contract::error::CONTEXT_FAMILY,
            riscv_sim_contract::error::E_CTX_05_ADMIN_CHANNEL_DISABLED,
            ADMIN_CHANNEL_DISABLED_MESSAGE,
        ))
    }

    /// 管理员通道（v0.44 · STAGE-29 · 裁决 G / H / I；v0.45 · STAGE-30 · 裁决 C / K）。
    ///
    /// ⚠️ **准入判据 = 角色**（裁决 C）：`Role::Admin` ⇒ 进来；`Role::Product`
    /// ⇒ ⇒ 回 `E_CTX-05` ⇒ ⇒ 默认角色是 `Product` ⇒ ⇒ 默认回应**一字不变**
    /// （`E_CTX-05` · B-1 / X-3）。
    ///
    /// ⚠️ **角色来源 = `Hms.role`**（裁决 K）⇒ ⇒ 准入判据与诊断分派**同一个
    /// 来源** ⇒ ⇒ 不是两个真相。
    ///
    /// ⚠️ **本批只兑现两个 op**（裁决 H）：
    ///
    /// | op | 回应 |
    /// | --- | --- |
    /// | `DrainLogs` | **恰好一个** [`EventPayload::AdminLogs`]（B-2 / B-3） |
    /// | `Custom("diag")` | **恰好一个** [`EventPayload::AdminDiag`]（B-4） |
    /// | `KillGuest` / `PersistProject` / 其它 `Custom` | **仍** `E_CTX-05`（B-5 / X-4） |
    ///
    /// ⚠️ **「不假装实现了」**（裁决 H）：`KillGuest` / `PersistProject` **本批
    /// 没有实现** ⇒ ⇒ 回**同一个** `E_CTX-05` ⇒ ⇒ 不给这个码**换第二个含义**
    /// （**一个码一个含义**）。
    pub(super) fn on_admin(&mut self, op: AdminOp) -> Vec<EventPayload> {
        // ⚠️ **准入判据 = 角色**（裁决 C / K）⇒ ⇒ 角色来源 = `Hms.role`
        // （**同一个来源** ⇒ ⇒ 不是第二个真相）⇒ ⇒ 「`role=Product` +
        // 通道已开」那种组合**再也表达不出来**。
        //
        // ⚠️ **默认关**（默认角色 `Product`）⇒ ⇒ 默认回应**一字不变**（X-3）
        // ⇒ ⇒ 这一句在**最前面**（默认路径**不碰**任何其它逻辑 ⇒ ⇒ 默认
        // 行为**零副作用**）。
        if self.hms.role() != Role::Admin {
            return vec![self.admin_channel_disabled_error()];
        }
        match op {
            AdminOp::DrainLogs => {
                vec![EventPayload::AdminLogs {
                    entries: self.drain_log_entries(),
                }]
            }
            // ⚠️ `Custom` 的**字符串**是**约定**不是类型 ⇒ ⇒ 只有**逐字等于**
            // `ADMIN_DIAG_OP` 的才走诊断包 ⇒ ⇒ **别的** `Custom` 一律 `E_CTX-05`
            // （**不猜**别的 op 名）。
            AdminOp::Custom(ref name) if name == ADMIN_DIAG_OP => {
                // ⚠️ `host_clock_available` 是**宿主注入**的事实（v0.43 · D-135
                // · 裁决 C：谁持有时钟谁回答）⇒ ⇒ 这里复用宿主已有的那个出口
                // ⇒ ⇒ **M-22 不主动去问时钟**。
                let kit = self.export_diagnostic_kit(None);
                vec![EventPayload::AdminDiag { diag: kit.to_dto() }]
            }
            _ => vec![self.admin_channel_disabled_error()],
        }
    }

    /// 管理员通道：**导出日志条目**（一次导出**有上界** · B-6）。
    ///
    /// ⚠️ **条数与顺序取自 M-21 的环**（B-3）：用
    /// [`LogBuffer::query_recent`]（**按虚拟时间**的全局最近）⇒ ⇒ 顺序
    /// **天然**是虚拟时间序 ⇒ ⇒ **不重排**、**不造条目**、**不去重**。
    ///
    /// ⚠️ **截断必须明确告知（不是静默）**，而 `AdminLogs` 的**线上形状是
    /// 冻结的**（只有 `entries` 一个字段 ⇒ 跨层 fixture 已钉住）⇒ ⇒ 三条
    /// 约束同时成立的做法是：**在 M-21 自己的环里写一条说明截断的日志**
    /// ⇒ ⇒ ① 用户**看得见**（下一次 `DrainLogs` 与宿主日志里都有）②
    /// `AdminLogs` 事件**恰好一个** ③ `entries` 里**没有**合成条目。
    ///
    /// ⚠️ **为什么不往 `entries` 里塞「已截断」的合成条目**：那会破 B-3 的
    /// 「条数与 M-21 环里一致」⇒ ⇒ **不许**。
    ///
    /// ⚠️ **为什么不加第二个事件**：那会破 B-2 的「**恰好一个**」⇒ ⇒
    /// **不许**。
    fn drain_log_entries(&mut self) -> Vec<LogEntryDto> {
        // ① 取**环里**的最近 N 条（`N` = 上界）+ 环里**现驻留**的总条数。
        //
        // ⚠️ `len()` 是**驻留条数**（已被环自己挤掉的不算 —— 那是环的语义，
        // 不是本次导出的语义）⇒ ⇒ 「被本次导出截掉」= 驻留条数 - 导出条数。
        let (entries, resident) = {
            let buffer = self.log_buffer.borrow();
            let entries = buffer.query_recent(&LogQuery::default(), ADMIN_DRAIN_MAX_ENTRIES);
            (entries, buffer.len())
        };
        let dropped = resident.saturating_sub(entries.len());
        // ② 截断 ⇒ **明确告知**（写进 M-21 自己的环 ⇒ 下次导出与宿主日志
        //    都看得见 ⇒ ⇒ **不是静默**）。
        if dropped > 0 {
            self.admin_drain_truncated = self.admin_drain_truncated.wrapping_add(dropped as u64);
            let now_ps = self.backend.engine().now_ps();
            let t_host_ms = self.host_now_ms();
            let msg = format!(
                "日志导出被截断：环内 {resident} 条，本次只导出最近 {ADMIN_DRAIN_MAX_ENTRIES} 条，另有 {dropped} 条未导出"
            );
            // ⚠️ `err_code` 传**空串** = 没有码 ⇒ ⇒ 线上是 `None`
            // ⇒ ⇒ **不发明**一个新码（那会「一个码两个含义」）。
            self.log_buffer.borrow_mut().log(
                LogLevel::Warn,
                ADMIN_DRAIN_LOG_CTX,
                &msg,
                "",
                now_ps,
                t_host_ms,
            );
        }
        // ③ 转换住在 **M-21**（日志条目是它拥有的形状 · 裁决 I）。
        entries.iter().map(LogEntry::to_dto).collect()
    }

    /// **ISA 门禁的单一落点**（v0.75 · STAGE-51 · D-175 的 F）。
    ///
    /// 判据 = 板声明的 `mcu.isa` ⟷ 镜像的 ELF 头 ⇒ ⇒ **一个约束一个落点**：
    /// 用户板与自动选出的内置板**都**从这一处拿判据（词法解析走契约层
    /// [`mcu_isa_info`]，**不**在这里重抄词法）。
    ///
    /// ⚠️ 只对 **ELF** 镜像判（`elf_isa.bits != 0`）⇒ ⇒ raw/hex 没有
    /// ELF 头 ⇒ 无 ISA 可判 ⇒ ⇒ 跳过。
    ///
    /// ⚠️ 判据只有两条（ELF 头给得出来的）：
    /// ① **class 双向严格**：板声明 RV64 + ELF32 固件 ⇒ 拒；
    /// ② **c 位单方向严格**：板声明**不含** `c` + 固件 `e_flags` 带
    ///    `EF_RISCV_RVC` ⇒ 拒；板含 `c` ⇒ **不要求**（`c` 是能力不是
    ///    义务，双向会把存量非压缩固件全灭）。
    /// m/a/f/d 在 ELF 头查不到 ⇒ ⇒ **不校验**（规格 §2.4 ③，如实登记）。
    ///
    /// ⚠️ 判不过返回 `E_PLT-20` ⇒ ⇒ 调用方在**应用板之前**返回 ⇒ ⇒
    /// 机器/生效板/修订号**全都不动**（与「未知系统」门禁同款次序纪律）。
    pub(crate) fn isa_gate_error(
        board: &PlatformDescription,
        parsed: &[ParsedImage],
    ) -> Option<PlatformError> {
        let mcu = board.mcu.as_ref()?;
        // ⚠️ V-23（契约层）已保证词法合法 ⇒ 这里 `expect` 不触发。
        let info = mcu_isa_info(&mcu.isa).expect("V-23 已保证 mcu.isa 词法合法");
        for p in parsed {
            if p.elf_isa.bits == 0 {
                continue; // 非 ELF（raw/hex）⇒ 无 ISA 可判
            }
            if p.elf_isa.bits != info.bits {
                return Some(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_20_FW_ISA_MISMATCH,
                    format!(
                        "固件是 RV{}（ELF class），这块板声明的 MCU {} 是 RV{} ⇒ ISA 位宽不匹配",
                        p.elf_isa.bits, mcu.isa, info.bits
                    ),
                ));
            }
            if !info.has_c && p.elf_isa.has_rvc {
                return Some(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_20_FW_ISA_MISMATCH,
                    format!(
                        "固件带压缩指令扩展（EF_RISCV_RVC），这块板声明的 MCU {} 不含 c ⇒ ISA 不匹配",
                        mcu.isa
                    ),
                ));
            }
        }
        None
    }
}
