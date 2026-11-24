//! 引擎 / 总线状态只读面：指令计数、总线句柄、虚拟时间、PC / 寄存器 / 内存读 / 引擎快照。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。

use riscv_sim_contract::protocol::EventPayload;

use super::KernelHost;
use crate::m14_bus::Bus;
use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::WriteSize;
use riscv_sim_contract::snapshot::EngineSnapshotDto;

impl KernelHost {
    /// 已经执行的指令总数（headless runner 的预算判据）。
    /// 取走 `run_until` 攒下的事件（**取走即清空**）。
    ///
    /// ⚠️ 与 `handle_message` 的返回值**不是**同一份：这是窗口内部产生的
    /// 事件，只在 `run_until` 之后有内容。
    pub fn take_pending_events(&mut self) -> Vec<EventPayload> {
        std::mem::take(&mut self.pending_events)
    }

    pub fn total_executed(&self) -> u64 {
        self.backend.total_executed()
    }
    /// 只读地看总线（诊断 / 测试用）。
    pub fn bus(&self) -> &Bus {
        self.backend.bus()
    }

    /// 可变地看总线（装载镜像 / 测试用）。
    pub fn bus_mut(&mut self) -> &mut Bus {
        self.backend.bus_mut()
    }
    /// 当前虚拟时间（皮秒）。
    pub fn now_ps(&self) -> u64 {
        self.backend.engine().now_ps()
    }
    /// 事件队列里**待处理**的事件数（D-07）。
    ///
    /// 时间驱动链路的心跳：跑过一轮之后，队列里应当留着一件待派发的
    /// 外设 tick 事件（下一个量子边界的那一次）。指令驱动的路径
    /// （`StepN`）不排期它 —— 所以这个数字就是「走的是哪条路」的可
    /// 观测证据。
    pub fn pending_event_count(&self) -> usize {
        self.backend.engine().pending_event_count()
    }
    /// 当前 PC。
    pub fn pc(&self) -> u32 {
        self.backend.engine().pc()
    }
    /// 读一个内存位置（不计入总线统计）。
    pub fn read_memory(&self, addr: u32, size: WriteSize) -> Result<u32, PlatformError> {
        self.backend.bus().read_only(addr, size.into())
    }
    /// 当前会话的**通用寄存器**（32 个 GPR）。
    ///
    /// ⚠️ **x0 恒为 0**（D-04）：取的是后端的寄存器文件，它已经把 x0
    /// 归零——一份被损坏的快照不能让 x0 泄出非零值。
    pub fn registers(&self) -> [u32; 32] {
        self.backend.register_file()
    }
    /// 当前会话的**引擎段**（队列内容 + 队列顺序 + 插入序计数器 + 指令
    /// 预算 + 平台状态）。
    ///
    /// ⚠️ 一个 [`EngineSnapshotDto`] 就覆盖了「事件队列 / 插入序 / 指令
    /// 预算」三项——它们在 DTO 里是**分开的字段**，不是一个笼统的
    /// 「引擎状态」。
    pub fn engine_snapshot(&self) -> EngineSnapshotDto {
        self.backend.engine().snapshot_dto()
    }
}
