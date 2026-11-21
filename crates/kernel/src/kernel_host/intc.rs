//! 中断控制器（M-16）通路：端口装配、外设源同步、IRQ 行 / 定时器比较值包装与状态只读。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。

use super::{platform, KernelHost, PLIC_CONTEXT_MACHINE};
use crate::m17_backend::SharedInterruptPort;
use riscv_sim_contract::csr::{IRQ_SOFTWARE, IRQ_UART_RX_READY};
use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::port::InterruptSink;
use riscv_sim_contract::session::M16IntcDto;
use std::rc::Rc;

impl KernelHost {
    /// 装上 M-16 的中断控制器**端口**（接缝 #2 / #3 / #4）。
    ///
    /// 宿主是**唯一**知道「引擎那条 `InterruptSink` 通路」与「M-17 的
    /// 采样 / 交付通路」必须看到**同一个**控制器的地方——它把同一个
    /// `Rc<RefCell<InterruptController>>` 的两份引用分别交给两边。
    ///
    /// ⚠️ 幂等：重复调用只是换掉后端里的端口盒子，控制器还是**同一个**。
    pub(super) fn install_interrupt_port(&mut self) {
        self.backend
            .install_interrupt_port(Box::new(SharedInterruptPort::new(Rc::clone(
                &self.intc.controller,
            ))));
    }
    /// ⚠️ **v0.76（STAGE-52 · B3 / 裁决 C）**：自动选档链路（`choose_platform`
    ///   方法 + [`platform::choose_description`]）**已删除** ⇒ ⇒ 无用户板时
    ///   装载**不再自动挑板**，改拒 `E_PLT-22`（见装载流程 None 分支）。
    ///
    /// 把外设的中断源**同步**到 M-16（v0.23）。
    ///
    /// 三条通路，逐条说清楚：
    ///
    /// 1. **CLINT 的 `mtimecmp`（单位：周期）→ M-16 的定时器比较值
    ///    （单位：ps）**。M-16 的 `poll()` 自己管「`mtime >= mtimecmp`
    ///    ⇒ 挂起定时器线」，它只缺一个比较值 ⇒ 宿主每轮把 CLINT 的
    ///    `mtimecmp` 换算成 ps 喂给它。
    /// 2. **UART 的 RX 就绪 → PLIC 的源（**按连线表**）**。真实 RTOS 的
    ///    UART 中断是**外部中断**（经 PLIC 到 `MEIP`）⇒ 中断号落在 PLIC
    ///    上，不在 M-16 上。
    ///
    ///    ⚠️ **v0.34（D-125）**：那个源号**不再是源码里的字面量** ⇒ 它来自
    ///    **中断源表**（描述里每个外设的 `irq_line`）⇒ ⇒ 换一块板改号
    ///    **不用改宿主**。
    /// 3. **PLIC 的待交付源 → M-16 的外部中断线**。⚠️ **claim 是 guest
    ///    的权利**：平台这一侧**只读** PLIC 的「当前该交付哪个源」
    ///    （`Plic::deliverable_source`），**不代 guest claim** ——
    ///    真正的 `claim` 会消费 pending，宿主每拍调它就会抢走 guest 的源。
    ///
    /// ⚠️ **这一段是「宿主唯一知道外设挂了哪些」的地方**（同
    /// [`SelfIssBackend::install_peripheral_ticker`] 的理由）—— 外设
    /// 本身不持有 M-16 的引用（M-15 LLD 的形状），不知道该往哪抬线。
    pub(crate) fn sync_interrupts(&mut self) {
        // ⚠️ **角色 = id，句柄 = 从实例表里查出来的**（裁决 B）⇒ ⇒ 「第一个
        // CLINT」= 时间源（裁决 D：逐字不变）。
        // ⚠️ **时间源角色**（STAGE-49 · D-177 的 C）—— 同上：**缺省逐字不变**。
        // ⚠️ **同一个落点**（`resolve_roles`）—— **不**在这里自己回落
        // （v0.73 施工期订正：那会**两处**保证同一条性质 ⇒ ⇒ ⇒ 不可证伪）。
        let Some(clint) = self
            .roles
            .time_source
            .as_deref()
            .and_then(|id| self.devices.clint(id))
        else {
            return;
        };
        // ⚠️ **没有 PLIC ⇒ 整个函数不做任何事**（**存量逐字不变**：
        // `let Some(plic) = ... else { return }` ⇒ ⇒ 定时器比较值也**不**喂
        // ⇒ ⇒ 「无 PLIC 的板不抬任何线」这条不变式保留）。
        //
        // ⚠️ **单板单 PLIC**（契约 V-27 / `E_PLT-24`）：角色缺省已经在
        // `resolve_roles` 里落到那块唯一 PLIC ⇒ 这里只取**一个**句柄，
        // 第 2 步抬源与第 3 步只读仲裁用**同一个** ⇒ 不再有「遍历所有
        // PLIC」这条半通路径。
        let Some(plic) = self
            .roles
            .main_plic
            .as_deref()
            .and_then(|id| self.devices.plic(id))
        else {
            return;
        };
        let now_ps = self.backend.engine().now_ps();

        // ---- 1) 定时器：CLINT 的 mtimecmp（周期）→ M-16（ps） ----
        //
        // ⚠️ `mtimecmp == 0` 在 CLINT 里表示「无期限」；M-16 里 `0` 也
        // 表示「定时器关闭」⇒ 两边**同义**，直接传过去即可。
        let cmp_ps = match clint.try_borrow() {
            Ok(c) => c.mtimecmp_ps(),
            Err(_) => return,
        };
        self.intc.controller.borrow_mut().set_timer_compare(cmp_ps);

        // ---- 1b) CLINT 的 msip（软件中断）→ M-16 的软件线（v0.77 补线）----
        //
        // ⚠️ **这一段以前不存在**：CLINT 的 `msip` 有寄存器模型、可写可读、
        // 进快照，但**没有任何人把它搬进控制器** ⇒ guest 写 `msip = 1` 什么
        // 都不会发生（比未实现更危险：寄存器读回来是 1）。
        //
        // ⚠️ **电平语义**（与 UART 那条线一致）：`msip` 的 bit0 本身就是
        // **挂起电平** —— `1` 抬线、`0` 撤线。guest 在 handler 里写回
        // `msip = 0` 之后，宿主下一拍就把线撤掉 ⇒ 下一次自陷才有机会进来。
        //
        // ⚠️ **hart 0** 是平台唯一被交付的 hart（与 `PLIC_CONTEXT_MACHINE`、
        // 定时器 `mtimecmp` 同一口径）。
        let msip = match clint.try_borrow() {
            Ok(c) => c.msip_pending(),
            Err(_) => return,
        };
        if msip {
            let _ = self.intc.controller.borrow_mut().request(
                IRQ_SOFTWARE,
                now_ps,
                "clint",
                crate::m16_interrupt::TRIGGER_LEVEL,
            );
        } else {
            // ---- 2) 每个中断外设按**自己的** ⚠️中断号抬 PLIC 源 ----
            //
            // ⚠️ UART 只有一个源：RX（IER bit0 ∧ LSR bit0）与 TX-empty
            //   （IER bit1 ∧ LSR.THRE）**共用**它（物理 16550 只有一根 IRQ 线）。
            // ⚠️ GPIO 整块只有一个源：任意引脚有待处理事件就抬线，直到
            //   guest 用 W1C 把 pending 清完才落线。
            //
            // ⚠️ 这里的 `plic` 与第 3 步只读仲裁用的是**同一个句柄**（单板单
            //   PLIC ⇒ 不再按 `first_plic()` 另查一遍）。
            for irq in self.peripheral_irqs.iter() {
                match irq.kind {
                    platform::PeripheralIrqKind::Uart => {
                        let Some(uart) = self.devices.uart(&irq.id) else {
                            continue;
                        };
                        // ⚠️ **RX 判据交给 UART 自己**（v0.82）：真 16550 的
                        //   RDI 条件随 `FCR.FE` 变 —— FIFO 关是「非空」，FIFO 开是
                        //   「深度 ≥ 触发水位 ∨ 字符超时」⇒ 原来在这里写死
                        //   `IER bit0 ∧ LSR bit0` 就等于**强行把 FIFO 关掉**，
                        //   读一个字节不会落线 ⇒ 中断风暴（实测症状：波形只有
                        //   上升沿、且定时器被压）。
                        let (rx, tx) = match uart.try_borrow() {
                            Ok(u) => (u.rx_interrupt_pending(), u.tx_interrupt_pending()),
                            Err(_) => continue,
                        };
                        if let Ok(mut p) = plic.try_borrow_mut() {
                            // Renode 口径：这里只同步**网关输入电平**；
                            // 源落下不 retract 已经送进 PLIC 的请求。
                            p.set_source_level(u32::from(irq.source), rx || tx);
                        }
                        // 设备级电平通道：这里正是「源抬手 / 落手」的时刻。
                        self.sample_irq_source(u32::from(irq.source), rx || tx);
                    }
                    platform::PeripheralIrqKind::Gpio => {
                        let Some(gpio) = self.devices.gpio(&irq.id) else {
                            continue;
                        };
                        // ⚠️ **pending 归外设、由 guest W1C 消费**（Renode
                        // 口径）：宿主只把位图映射成 PLIC 源的**网关电平**
                        // —— 非空抬线、为空落线；落线不 retract 已送进
                        // PLIC 的请求，**不**替 guest 清位。
                        let pending = match gpio.try_borrow() {
                            Ok(g) => g.pending_irqs() != 0,
                            Err(_) => continue,
                        };
                        if let Ok(mut p) = plic.try_borrow_mut() {
                            p.set_source_level(u32::from(irq.source), pending);
                        }
                        self.sample_irq_source(u32::from(irq.source), pending);
                    }
                }
            }

            self.intc.controller.borrow_mut().clear(IRQ_SOFTWARE);
        }

        // ---- 3) PLIC 的待交付源 → M-16 的外部中断线 ----
        //
        // ⚠️ 用**外部中断线**（`IRQ_UART_RX_READY` = 1）：平台的内部线号
        // 空间里 1 与 2 都映射到**架构 cause 码 11**（外部中断）⇒
        // guest 读到的 `mcause` 是 `0x8000_000B`，与 RISC-V 架构一致。
        //
        // ⚠️ **单板单 PLIC**：只问这一个 PLIC 的「当前该交付哪个源」⇒
        //   与第 2 步抬源用同一个句柄 ⇒ 「外设抬到 A、claim 从 B 取」这种
        //   半通缺口在结构上不存在；也不再有「遍历所有 PLIC」那条路径。
        //
        // ⚠️ **读不到 ⇒ 什么都不做**（不抬也不撤）：与原先「一个 PLIC 都
        //   读不到 ⇒ 什么都不做」逐字一致。
        // ⚠️ **宿主只读仲裁结果，不代 guest claim**：claim 是 guest
        // 读 Claim 寄存器的动作，会消费 pending。这里必须用只读的
        // `deliverable_source`。
        let deliverable = match plic.try_borrow() {
            Ok(p) => p.deliverable_source(PLIC_CONTEXT_MACHINE),
            Err(_) => return,
        };
        match deliverable {
            Some(_source) => {
                let _ = self.intc.controller.borrow_mut().request(
                    IRQ_UART_RX_READY,
                    now_ps,
                    "plic",
                    crate::m16_interrupt::TRIGGER_LEVEL,
                );
            }
            // ⚠️ **PLIC 侧没有可交付的源 ⇒ 撤掉外部线**（STAGE-40 · v0.55 ·
            // 裁决 B2）。
            //
            // ⚠️ **这一支原先是空的** —— ⇒ ⇒ 平台**只加不减** ⇒ ⇒ 外部线
            // 的挂起项**永远**留在队列里、`active_irq` **永远**不复位 ⇒ ⇒
            // 单级嵌套护栏把**之后所有中断**永久堵死（D-158 的另一半根因）。
            //
            // ⚠️ **判据就是 PLIC 自己的「当前应该交付哪个源」**
            // （`Plic::deliverable_source`：已挂起 ∧ 已使能 ∧ 优先级**严格大于**
            // 门限 ∧ **严格大于 active 栈顶**）⇒ ⇒ 「源被 guest 解除」/「源被门限挡住」/「源被禁用」
            // 三种情况都会让只读仲裁返回 `None` ⇒ ⇒ **撤线是对的** ⇒ ⇒
            // **一条判据管三件事**。
            //
            // ⚠️ **与上面的 `Some` 分支完全对称** ⇒ ⇒ 「有源就抬线、无源就
            // 撤线」⇒ ⇒ 外部线也是**电平**语义。
            //
            // ⚠️ **不替 guest 掩盖**（裁决 E）：guest **没有**解除源 ⇒ ⇒
            // PLIC 的挂起位仍在 ⇒ ⇒ 只读仲裁仍返回 `Some` ⇒ ⇒ 平台
            // **继续**请求 ⇒ ⇒ guest 立刻再次陷入（死循环）⇒ ⇒ 那是
            // **guest 的 bug**，平台**不**替它擦。
            //
            // ⚠️ **读不到 PLIC ⇒ 什么都不做**（不抬也不撤）⇒ ⇒ 「读不到就不改」
            // 这条口径保持不变。
            None => {
                self.intc.controller.borrow_mut().clear(IRQ_UART_RX_READY);
            }
        }
    }
    /// M-16 的**定时器比较值**（ps）—— guest 写下的 `mtimecmp` 换算后的值。
    ///
    /// ⚠️ `0` = 定时器关闭（M-16 的口径）。
    pub fn intc_timer_compare_ps(&self) -> u64 {
        match self.intc.controller.try_borrow() {
            Ok(c) => c.timer_compare(),
            Err(_) => 0,
        }
    }

    /// M-16 当前**挂起**的中断线（内部线号）。
    pub fn intc_pending_lines(&self) -> Vec<u16> {
        match self.intc.controller.try_borrow() {
            Ok(c) => c.pending_lines(),
            Err(_) => Vec::new(),
        }
    }

    /// 把外设的中断源**同步**到 M-16（宿主接线的公开入口）。
    ///
    /// ⚠️ 正常运行路径里它由 [`Self::on_run`] 自动调用；暴露出来是为了
    /// 让「宿主接线真的接上了」这件事**可以被单独观察**（台账教训：
    /// 「加了守卫」≠「守卫接上了」⇒ 要查它的**比较点**）。
    pub fn sync_interrupts_now(&mut self) {
        self.sync_interrupts();
    }
    /// 当前会话的**已拉起 IRQ 行**（段 5 的同一份导出）。
    pub fn intc_state(&self) -> M16IntcDto {
        self.intc_state_inner()
    }
    /// 拉起一条 IRQ 行（走的是**引擎那条路**——同一个委派）。
    ///
    /// ⚠️ 目前**没有生产路径**会调它：M-15 的 pending IRQ 采样点属于
    /// **M-16 落地**的范畴（台账 D-63 / D-64 / D-72 / D-74）。本方法
    /// 让「段 5 恒为空」的成因精确落在「M-15 没有生产路径」上，而不是
    /// 「intc 是个空壳」。
    pub fn raise_irq(&mut self, irq: u16) -> Result<(), PlatformError> {
        self.sinks.raise(irq)
    }

    /// 撤下一条 IRQ 行。
    pub fn lower_irq(&mut self, irq: u16) -> Result<(), PlatformError> {
        self.sinks.lower(irq)
    }

    /// **清除**一条 IRQ 行（接缝 #7：调试器 / 固件清中断）。
    ///
    /// 语义与 M-16 的 [`InterruptController::clear`] 逐字相同：移除挂起项 +
    /// 撤销线电平 + 复位 `active_irq`。
    ///
    /// ⚠️ 争用（`RefCell` 拿不到）时**记一次失败并返回 `Err`**——与
    /// [`HostSinks::delegate`] **同一种结果**：同一个失败条件必须是
    /// 同一个口径（`Err` + 计数 +1），否则「同一个量两个真相」。
    pub fn clear_irq(&mut self, line: u16) -> Result<(), PlatformError> {
        self.sinks.delegate(|intc| {
            intc.clear(line);
            Ok(())
        })
    }

    /// 设置**定时器比较值**（接缝 #6 / 裁决 E）。
    ///
    /// `mtime >= mtimecmp` 时按**电平**语义挂起 line 3（timer-compare），
    /// 于是「定时器 → 仲裁 → 交付 → 陷入」整条链可以端到端验证。
    ///
    /// ⚠️ **固件侧的 MMIO 通路推迟到 P1**（裁决 E / 台账 D-82）：今天
    /// guest 自己写不了 `mtimecmp`，**宿主 API** 是本批的入口。
    ///
    /// ⚠️ `0` 表示**定时器关闭**（否则 `mtime >= 0` 恒成立，那是一条
    /// 永不消失的中断）。
    pub fn set_timer_compare(&mut self, mtimecmp_ps: u64) -> Result<(), PlatformError> {
        self.sinks.delegate(|intc| {
            intc.set_timer_compare(mtimecmp_ps);
            Ok(())
        })
    }

    /// 中断委派失败（`RefCell` 争用）的累计次数。
    ///
    /// 非零 = 有一次中断操作**没有真的落到控制器上**——这条 IRQ 行丢了。
    pub fn intc_delegation_failures(&self) -> u32 {
        self.intc.failures.get()
    }
}
