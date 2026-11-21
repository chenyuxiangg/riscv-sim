//! 会话级快照的采集 / 还原，以及段 5（中断控制器状态）的导出 / 回灌。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。

use super::{m15_peripheral, Disassembler, KernelHost, E_INT_01_CONTROLLER_BUSY};
use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::port::ExecutionBackend;
use riscv_sim_contract::session::{CheckpointMetaDto, M13StateDto, M16IntcDto, SessionSnapshotDto};

impl KernelHost {
    /// 采集一份**会话级**快照载荷（六段齐全，STAGE-3 §3.5）。
    ///
    /// 宿主是**采集器**，M-20 是**记录器**：六段分别来自各自的持有者
    /// （M-12 引擎 / M-14 总线 / M-15 外设 / M-16 控制器），最后交给
    /// [`SnapshotManager::capture_session`] 组装。
    ///
    /// ⚠️ **段 0 的 `t_virt` 由宿主代填**——它被设成引擎当前的虚拟时间
    /// （`m12_engine.state.virtual_time_ps`），**调用方给的值会被覆盖**。
    /// 这不是「造数据」：检查点的虚拟时间**就是**引擎的虚拟时间，两者是
    /// 同一个量；留两个落点就是「同一个量两个真相」（恢复时必须挑赢家，
    /// 输家默默腐烂）。段 0 的其余字段（检查点 id / 宿主时间 / 基线序列号）
    /// **仍然是调用方给的**，宿主不改写。
    ///
    /// ⚠️ **采集期若控制器被占用，段 5 会是空的**——载荷里**没有**
    /// 「本段不完整」的标记，一个空段 5 与「真的没有拉起任何 IRQ」**在
    /// 载荷里不可区分**。要判断采到没有，看
    /// [`Self::intc_delegation_failures`]：它非零就说明采集期发生过争用。
    /// （本批**不改**这个行为——改 `capture_session` 的返回类型会波及整条
    /// 链，而争用在生产路径上不可达；台账留账。）
    ///
    /// ⚠️ **段 2 只含 RAM 内容**（契约层裁决 B：寄存器的唯一落点是
    /// `m12_engine.state`，再存一份就是两个真相）。
    pub fn capture_session(&mut self, cp_meta: CheckpointMetaDto) -> SessionSnapshotDto {
        // ⚠️ 采集顺序：**先**把六段收齐（每段都是**独立**的一份导出），
        // **再**交给 M-20 组装——M-20 不碰 M-14 / M-15 / M-16。
        let engine = self.backend.engine().snapshot_dto();
        let m13_state = M13StateDto {
            mem_pages: self.backend.bus().export_pages(),
        };
        let m14_map = self.backend.bus().export_map();
        let m15_periph =
            m15_peripheral::collect_session_state(&self.backend.bus().peripheral_instances());
        let m16_intc = self.intc_state_inner();
        // ⚠️ 段 0 的 `t_virt` **代填**成引擎的虚拟时间——调用方给的值
        // 被覆盖（见本方法的文档）。
        let cp_meta = CheckpointMetaDto {
            t_virt: engine.state.virtual_time_ps,
            ..cp_meta
        };
        self.snapshots
            .capture_session(cp_meta, engine, m13_state, m14_map, m15_periph, m16_intc)
    }

    /// 还原一份**会话级**快照载荷（STAGE-3 §3.7）。
    ///
    /// ## 顺序是**不变量**，不可换
    ///
    /// ```text
    /// 0. payload.validate()           ← 先校验：一个字都还没改
    /// 0.5 外设状态的**只读**预检        ← 一个字都还没改（见下）
    /// 1. M-14 load_map(map)           ← 重建区域拓扑
    /// 2. M-14 load_pages(pages)       ← 把 RAM 内容写进去（需要区域已存在）
    /// 3. M-15 restore_session_states  ← 外设内部状态
    /// 4. M-16 load_state(intc)        ← 已拉起的 IRQ 行
    /// 5. M-17 restore_full(engine)    ← 最后还原引擎段（覆盖 gpr / pc / 虚拟时间）
    /// ```
    ///
    /// 页写不进不存在的区域；外设状态写不进未绑定的实例；引擎段**最后**
    /// 还原——它会**覆盖** `gpr` / `pc` / 虚拟时间。
    ///
    /// ## 「校验先于应用」是真的
    ///
    /// - 第 0 步失败 → **总线、外设、intc、引擎一个字节都不许动**；
    /// - 第 0.5 步（外设状态的**只读**预检）失败 → 同样**一个字节都不许
    ///   动**：它跑在 `load_map` / `load_pages` **之前**，所以「外设状态
    ///   畸形」不会留下「内存来自载荷、寄存器来自恢复前」的**拼接体**；
    /// - 第 1 / 2 / 3 步各自的消费者（Lane A 写的）**已经是**「全量校验
    ///   → 再写入」，所以**每一步失败时，那一步自己没写**；之前步骤写下的
    ///   内容是**合法**的（前面每步都通过了各自的校验）。
    ///
    /// ## ⚠️ 什么**不在**载荷里（STAGE-3 §7）
    ///
    /// **断点表 / 观察点表 / 探针环 / 波形环 / 总线统计计数器**都**不在**
    /// 会话载荷里。所以「还原出的会话等价吗」这个问题的答案**取决于目标
    /// 宿主**：**同宿主**还原会保留它们（它们本来就在那儿），**跨宿主**
    /// 还原会**丢**它们（新宿主里那些表是空的）。本方法不改变这一点——
    /// 规格冻结如此——只是**说清楚**，免得调用方以为载荷是自足的。
    pub fn restore_session(&mut self, payload: &SessionSnapshotDto) -> Result<(), PlatformError> {
        // ---- 第 0 步：先校验。一个字都还没改。 ----
        payload.validate()?;

        // ---- 第 0.5 步：外设状态的**只读**预检。一个字都还没改。 ----
        //
        // ⚠️ 这一步跑在 `load_map` / `load_pages` **之前**，是为了不让
        // 「外设状态畸形」留下一具**拼接体**：第 1 / 2 步已经写过内存，
        // 第 3 步才因外设畸形而失败的话，失败后的会话是「内存来自载荷、
        // 寄存器来自恢复前、外设来自恢复前」的**缝合怪**——而调用方只拿到
        // 一个 `Err`。
        //
        // ⚠️ 预检用的是 M-15 的**同一个**入口（[`m15_peripheral::validate_session_states`]），
        // 宿主不重写任何一条规则——一个约束一个落点。
        let instances = self.backend.bus().peripheral_instances();
        m15_peripheral::validate_session_states(&instances, &payload.m15_periph)?;

        // ---- 第 1 步：重建区域拓扑（RAM / ROM 重建；外设原地保留）。 ----
        self.backend.bus_mut().load_map(&payload.m14_map)?;

        // ---- 第 2 步：回灌 RAM 内容（页要写进**已存在**的区域里）。 ----
        self.backend
            .bus_mut()
            .load_pages(&payload.m13_state.mem_pages)?;

        // ---- 第 3 步：外设内部状态（按 id 双向匹配；检查全在写入之前）。 ----
        m15_peripheral::restore_session_states(&instances, &payload.m15_periph)?;

        // ---- 第 3.5 步：作废 M-11 的**会话级**缓存与历史。 ----
        //
        // ⚠️ 还原会**换掉内存里的机器码** ⇒ 反汇编缓存里那些按地址缓存的
        // 条目**可能陈旧** ⇒ 必须**作废**（否则 `Disasm` 给出**上个会话**的
        // 指令 ⇒ 调试器给出**错误**的指令）。
        //
        // ⚠️ 轨迹环是**会话级历史** ⇒ 还原后是**新会话** ⇒ 旧历史必须
        // **清空**（否则两个会话的轨迹混在一起，而 `overflow_marked` 仍说
        // 「没溢出」⇒ 前端会把不连续的轨迹当完整会话）。
        //
        // ⚠️ **符号表与行表是「工程级」的**（它们来自已装载工程的 ELF，
        // 还原会话**不换工程**）⇒ **保留**，不清。
        //
        // ⚠️ 这与 STAGE-5 §9.2 是**同一条规矩**（「装载新工程 = 新会话 ⇒
        // 旧的索引全部作废」）：还原会话也是新会话。
        self.disassembler = Disassembler::new();
        self.trace.clear();

        // ---- 第 4 步：已拉起的 IRQ 行。 ----
        self.load_intc_state(&payload.m16_intc)?;

        // ---- 第 5 步：最后还原引擎段（队列 / 寄存器 / 虚拟时间）。 ----
        self.backend.restore_full(&payload.m12_engine)?;

        Ok(())
    }

    /// 读出当前已拉起的 IRQ 行（段 5 的同一份导出）。
    ///
    /// ⚠️ 争用时（`RefCell` 拿不到）**记一次失败**并返回**空**段 5 ——
    /// 失败是**可见的**（[`Self::intc_delegation_failures`]），不是静默的
    /// 「丢一段状态却不吭声」。
    ///
    /// ⚠️ 但**载荷里没有「本段不完整」的标记**：空段 5 与「真的没有拉起
    /// 任何 IRQ」在载荷里**不可区分**。要判断采到没有，看
    /// [`Self::intc_delegation_failures`]。本批**不改**这个行为（改
    /// `capture_session` 的返回类型会波及整条链，而争用在生产路径上
    /// 不可达）——台账留账，下一批处理。
    pub(crate) fn intc_state_inner(&self) -> M16IntcDto {
        match self.intc.controller.try_borrow() {
            Ok(intc) => intc.export_state(),
            Err(_) => {
                self.intc
                    .failures
                    .set(self.intc.failures.get().wrapping_add(1));
                M16IntcDto::default()
            }
        }
    }

    /// 回灌已拉起的 IRQ 行（段 5 的消费者）。
    ///
    /// ⚠️ 争用（`RefCell` 拿不到）时**记一次失败并返回 `Err`**——与
    /// [`HostSinks::delegate`] **同一种结果**：同一个失败条件必须是
    /// 同一个口径（`Err` + 计数 +1），否则「同一个量两个真相」。
    fn load_intc_state(&self, state: &M16IntcDto) -> Result<(), PlatformError> {
        let mut intc = match self.intc.controller.try_borrow_mut() {
            Ok(intc) => intc,
            Err(_) => {
                self.intc
                    .failures
                    .set(self.intc.failures.get().wrapping_add(1));
                return Err(PlatformError::new(
                    ErrorFamily::Interrupt,
                    E_INT_01_CONTROLLER_BUSY,
                    "中断控制器正被占用，无法回灌已拉起的 IRQ 行",
                ));
            }
        };
        intc.load_state(state);
        Ok(())
    }
}
