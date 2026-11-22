//! 命令执行调度族：运行 / 步进 / 步过、观察表达式、断点命中文限频、指令分块
//! 与看门狗检查窗口裁决、暂停、通用寄存器读写、协议能力协商、console 输入写入。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。被 lib 内测试平级直调
//! （`tests_v057_debug` / `tests_v059_reset` / `tests_watchdog`）与被兄弟
//! 子模块（`debug.rs`）回调的方法提为 `pub(crate)`；仅被父模块调度与同族内部
//! 调用的方法保持 `pub(super)`。

use std::collections::BTreeMap;

use riscv_sim_contract::error::{
    ErrorFamily, PlatformError, DEBUG_FAMILY, E_DBG_02_CONDITION_INVALID, E_ENG_03_NO_PROGRESS,
    E_PROTO_01_VERSION_MISMATCH, PROTO_FAMILY,
};
use riscv_sim_contract::model::{LocalVarInfo, StackFrame, WatchExpr, WatchValue, WriteSize};
use riscv_sim_contract::port::ExecutionBackend;
use riscv_sim_contract::protocol::{EventPayload, HaltReason};
use riscv_sim_contract::snapshot::DEFAULT_INSTRUCTION_BUDGET;
use riscv_sim_contract::SCHEMA_VERSION;

use crate::m08_breakpoint::watch_expr::WatchTarget;
use crate::m21_logging::LogLevel;
use crate::m22_hms::{HealthSignal, SignalCtx, SignalKind, SignalSrc};

use super::clock::HostInstant;
use super::watchdog::{
    classify_window_with_clock, WindowVerdict, HOST_WATCHDOG_BUDGET_MS, WATCHDOG_ERR_CODE,
    WATCHDOG_LOG_CTX,
};
use super::{
    kernel_descriptor, KernelHost, WatchEntry, E_OBS_02_PIPELINE_UNAVAILABLE, KERNEL_CAP_BITS,
};

impl KernelHost {
    /// 继续运行（`Continue` 或 `StepN`）。
    pub(super) fn on_run(&mut self, steps: Option<u32>) -> Vec<EventPayload> {
        self.on_run_inner(steps, false)
    }

    /// **步过**：后端识别调用并记下返回地址停点（不进入断点表），宿主把
    /// 它当作一次「一步」的运行窗口收尾（事件、tick、波形、停下原因同一套）。
    pub(super) fn on_step_over(&mut self) -> Vec<EventPayload> {
        self.on_run_inner(Some(1), true)
    }

    fn on_run_inner(&mut self, steps: Option<u32>, step_over: bool) -> Vec<EventPayload> {
        // 运行期**重试**装探针回调（V-A 检视 · 🟡-7）。
        //
        // 装配那一刻外设可能正被总线派发路径占用（`try_borrow_mut` 失败）
        // → 那一轮漏掉了。没有重试的话，这个漏掉的回调**永远不会**补上，
        // 那个信号从此再也不产生边沿，而宿主一声不吭。
        //
        // `attach_signal` 对同一 `(source, signal)` 是**幂等**的，所以重复
        // 调用不会冲突。
        self.attach_peripheral_probes();

        // ⚠️ **跑之前先同步一次外设中断源**（v0.23）。
        //
        // guest 在**上一批**里写下的 `mtimecmp` / PLIC 配置，只有宿主
        // 把它喂给 M-16 才生效 ⇒ 不先同步，这一批里定时器不会来。
        self.sync_interrupts();
        // ⚠️ **先把 UART 的字节取走**（捕获环满了会丢最新的字节）。
        self.collect_console();

        let mut events = Vec::new();
        // `Continue` → **时间驱动**：走 M-12 的唯一主循环（量子化执行 +
        // 到期事件派发 + 外设时钟推进）。一轮的边界是引擎自己的指令预算
        // （`DEFAULT_INSTRUCTION_BUDGET`），跑满就让出给宿主。
        // `StepN` → **指令驱动**：恰好 n 条（调试器语义）。
        //
        // 两条路径共用同一个执行器（`m17_backend::IssExecutor`），所以
        // 「按指令跑」与「按时间跑」不会各自长出一套 ISS 逻辑（D-07）。
        let run = if step_over {
            self.backend.step_over().map(|_| ())
        } else {
            match steps {
                Some(n) => self.step_in_chunks(n),
                // ⚠️ **`Continue` 分支就是看门狗的接线落点**（规格 §3.3）：
                // 一次 `continue_until_event` = **一个**检查窗口 ⇒ 停滞时
                // **不立即上抛**，而是**记一次窗口** ⇒ 连续三个窗口无进展才触发
                // 处置（裁决 G）。
                None => self.run_continue_windows(),
            }
        };
        match run {
            Ok(()) => {}
            Err(err) => {
                // The error comes **first**: it carries the command's
                // `reqId`, so M-05 rejects the command instead of letting
                // it time out. The state-reflecting events follow as
                // pushes.
                events.push(self.error_from(&err));
            }
        }
        // ⚠️ **D-51 的落点**：本批跑完后，把 M-18 的访存 trace 环**立刻**
        // 转进 M-11 的轨迹环。
        //
        // 「立刻」不是优化，是**要求**：M-18 的环（`TRACE_CAP = 200_000`）
        // 在 M-11 落地前**只写不读**（常驻 3~5 MB）；而它又是 M-11 要用的
        // 历史的**唯一来源** ⇒ drain 与转移必须同时发生，只 drain 会把
        // 历史冲掉。
        self.transfer_trace();
        // ⚠️ **跑之后再同步一次**（v0.23）：guest 在这一批里写下的
        // `mtimecmp` / PLIC 配置，下一批才用得上。
        self.sync_interrupts();
        // ⚠️ 跑完再取一次 —— 这一批里 guest 可能又写了 console。
        self.collect_console();
        // console 事件（v0.23 · 契约 v4）：订阅了才推。
        if let Some(console) = self.console_event() {
            events.push(console);
        }
        events.push(self.tick());
        if let Some(wave) = self.register_waveform(false) {
            events.push(wave);
        }
        // M-10 的增量状态：只发订阅了 `sim.state.delta` 的客户端。
        if let Some(delta) = self.state_delta_event(false) {
            events.push(delta);
        }
        // 探针波形（D-46）：本轮运行里外设状态变化产生的边沿。
        if let Some(wave) = self.waveform_event(false) {
            events.push(wave);
        }
        let reason = self.backend.last_stop_reason().or_else(|| {
            // `Continue` 跑到预算上限也是一个合法的停止条件（契约里
            // 最接近的取值是 `EndOfExecution`）；`StepN` 跑满 n 条是
            // **正常完成**，不是停止。
            if steps.is_none() && !step_over {
                Some(HaltReason::EndOfExecution)
            } else {
                None
            }
        });
        if let Some(reason) = reason {
            let pc = self.backend.engine().snapshot().pc;
            // 命中日志：**≥1s 限频**（同一次连续运行里同一断点反复停住时，
            // 不让日志环被同一条消息刷满）。
            if reason == HaltReason::Breakpoint {
                self.log_breakpoint_hit(pc);
            }
            // ⚠️ **store 边界的观察式命中**：`pc` 已推进 ⇒ ⇒ 另带「是哪条
            //    store 触发的」（STAGE-55 批 C）。其余停止原因取不到 ⇒ ⇒
            //    如实给 `None`。
            let trigger_addr = if reason == HaltReason::Watchpoint {
                self.backend.last_store_trigger_addr()
            } else {
                None
            };
            events.push(EventPayload::Halted {
                reason,
                pc,
                trigger_addr,
            });
        }
        // 停下（或本窗口跑完）之后轮询一次观察表达式：值变化 / 求值期
        // 错误都在同一个回执里带回。设置 / 列表命令也会走同一个方法。
        if !self.watch_items.is_empty() {
            let watch = self.watch_values_event();
            events.push(watch);
        }
        events
    }

    /// 设置 / 覆盖一条观察表达式。**语法非法 ⇒ 表不变**。
    pub(super) fn on_set_watch_expr(&mut self, expr: WatchExpr) -> Vec<EventPayload> {
        let target = match WatchTarget::parse_with(&expr.expr, &super::HostNames(&self.index)) {
            Ok(target) => target,
            Err(err) => {
                return vec![self.error_from(&PlatformError::new(
                    DEBUG_FAMILY,
                    E_DBG_02_CONDITION_INVALID,
                    err.message(),
                ))];
            }
        };
        if let Some(item) = self
            .watch_items
            .iter_mut()
            .find(|item| item.expr.id == expr.id)
        {
            item.expr = expr;
            item.target = target;
        } else {
            self.watch_items.push(WatchEntry { expr, target });
        }
        vec![self.watch_values_event()]
    }

    /// 对全部观察表达式求值一次，并计算 `changed`。
    ///
    /// 内存表达式走 M-17 的 `read_memory`（最终到 M-14 总线）；该地址当前
    /// 未映射 ⇒ **该项**带 `error`，其他项照常求值（不让一条坏观察项把整
    /// 张表打掉）。
    pub(super) fn watch_values_event(&mut self) -> EventPayload {
        let state = match self.backend.snapshot() {
            Ok(state) => state,
            Err(err) => return self.error_from(&err),
        };
        let mut values = Vec::with_capacity(self.watch_items.len());
        let mut next = BTreeMap::new();
        for item in &self.watch_items {
            let id = item.expr.id;
            let (value, error) = match item.target.value(&state) {
                Some(v) => (Some(v), None),
                None => match item.target.address(&state) {
                    Some(addr) => match self.backend.read_memory(addr, WriteSize::Word) {
                        Ok(v) => (Some(v), None),
                        Err(err) => (None, Some(format!("读取 0x{addr:08x} 失败：{err}"))),
                    },
                    None => (None, Some("无法解析出观察地址".to_string())),
                },
            };
            let previous = self.watch_last.get(&id);
            let changed = match previous {
                Some(prev) => prev.value != value || prev.error != error,
                None => true,
            };
            let value_dto = WatchValue {
                id,
                expr: item.expr.expr.clone(),
                value,
                error,
                changed,
            };
            next.insert(id, value_dto.clone());
            values.push(value_dto);
        }
        self.watch_last = next;
        EventPayload::WatchValues { values }
    }

    /// 写一条「命中断点」日志，**≥1s 限频**。
    ///
    /// ⚠️ 被压掉的条数会累计，并在**下一条放行**的日志里如实带出
    /// ⇒ ⇒ 不是静默丢弃。
    pub(crate) fn log_breakpoint_hit(&mut self, pc: u32) {
        const BP_HIT_LOG_THROTTLE_PS: u64 = 1_000_000_000_000; // 1s
        let now_ps = self.backend.engine().now_ps();
        let allow = match self.bp_hit_log_last_ps {
            None => true,
            Some(last) => now_ps.saturating_sub(last) >= BP_HIT_LOG_THROTTLE_PS,
        };
        if !allow {
            self.bp_hit_log_suppressed = self.bp_hit_log_suppressed.saturating_add(1);
            return;
        }
        let suppressed = self.bp_hit_log_suppressed;
        self.bp_hit_log_suppressed = 0;
        self.bp_hit_log_last_ps = Some(now_ps);
        let msg = if suppressed == 0 {
            format!("命中断点：pc=0x{pc:08x}")
        } else {
            format!("命中断点：pc=0x{pc:08x}（限频窗口内另有 {suppressed} 次命中未记日志）")
        };
        let t_host_ms = self.host_now_ms();
        self.log_buffer
            .borrow_mut()
            .log(LogLevel::Info, "breakpoint", &msg, "", now_ps, t_host_ms);
    }

    /// 跑 `n` 条指令，**按指令预算分块**（`DEFAULT_INSTRUCTION_BUDGET`）。
    ///
    /// ⚠️ **为什么要分块**（v0.20 检视 A · 重要 1）：
    /// M-18 的访存 trace 环是**有界**的（`TRACE_CAP = 200_000`，溢出丢最旧）
    /// ⇒ 它同时是**反汇编缓存的失效通道**（裁决 G：访存写事件 ⇒ 失效对应
    /// 地址）⇒ 环一旦溢出，**写事件被丢掉 ⇒ 缓存条目不失效 ⇒ 反汇编给出
    /// 旧机器码**（调试器给出**错误**的指令）。
    ///
    /// ⇒ 分块 ⇒ 每块至多 `DEFAULT_INSTRUCTION_BUDGET`（10 000）条指令，
    /// 每条至多 5 次总线访问（1 次取指 + 至多 4 个非对齐字节片）⇒ 单块
    /// 事件数 ≤ 50 000 ≪ 200 000 ⇒ **失效通道永远无损**。
    ///
    /// ⚠️ **顺带消掉一处上界不对称**：`Continue` 走时间驱动主循环，一轮就是
    /// `DEFAULT_INSTRUCTION_BUDGET` 条；而 `StepN` 此前**无上界** ⇒ 一条
    /// `StepN { n: 500000 }` 能把宿主堵住很久，并让上面的环溢出。
    ///
    /// ⚠️ **语义不变**：仍然「恰好 n 条」——中途撞上断点 / 观察点 / 陷阱 /
    /// 暂停就**停在那里**，剩下的**不**跑（与一次性 `step_n(n)` 一致）。
    fn step_in_chunks(&mut self, n: u32) -> Result<(), PlatformError> {
        // ⚠️ **幂等**：内核已被看门狗终止 ⇒ 不再跑（规格 §3.3 的处置
        // 第 ④ 步之后，内核就是死的）。
        if self.watchdog.fired() {
            return Err(self.terminated_error("内核已被看门狗终止，不再接受新的运行"));
        }
        let mut remaining = n;
        while remaining > 0 {
            let chunk = remaining.min(DEFAULT_INSTRUCTION_BUDGET);
            // ⚠️ **看门狗判定必须落在它自己的 `while remaining > 0` 循环体内**
            // （规格 §3.3「接线落点的精确位置」）：现有实现是
            // `let executed = self.backend.step_n(chunk)?;` ⇒ chunk 一旦停滞
            // 就**当场 `?` 上抛**、循环直接退出 ⇒ **三次确认在循环外根本
            // 攒不起来**。
            // ⇒ 正确形态：「chunk 停滞 ⇒ 记一次窗口 ⇒ `streak < 3` 就
            // **继续下一个 chunk**（`remaining` 不减，因为那一条没跑成）⇒
            // `streak >= 3` 才返回 `Err`」。
            let started = HostInstant::now();
            let result = self.backend.step_n(chunk);
            let elapsed_ms = started.elapsed_ms_since();
            let executed = match self.judge_window(elapsed_ms, &result) {
                WindowVerdict::Completed { executed } => executed,
                // ⚠️ **停滞（未达确认次数）⇒ 吞掉**并继续下一个 chunk ——
                // `remaining` **不减**（那一条指令没跑成，算不上进度）。
                WindowVerdict::Stalled => {
                    // ⚠️ **块间转移照样要做**（D-51：drain 与转移必须同时发生）
                    // ⇒ 停滞的批次也不能让 M-18 的环涨起来。
                    self.transfer_trace();
                    continue;
                }
                // 看门狗触发（处置已做）⇒ 上抛；别的错误 ⇒ 照旧上抛。
                WindowVerdict::Fired(err) | WindowVerdict::Failed(err) => return Err(err),
            };
            remaining -= executed;
            // ⚠️ **块没跑满 ⇒ 提前停**（停止条件命中）⇒ 剩下的**不**再跑。
            if executed < chunk {
                break;
            }
            // ⚠️ **块间转移**（D-51：drain 与转移必须同时发生）⇒ M-18 的
            // 环不会涨到它的上限 ⇒ 失效通道保持无损。
            self.transfer_trace();
        }
        Ok(())
    }

    /// `Continue` 的驱动循环 —— **每个窗口一次 `continue_until_event`**。
    ///
    /// ⚠️ **接线落点**（规格 §3.3）：`on_run` 的 `Continue` 分支。一次
    /// `continue_until_event` = 引擎跑满一个指令预算窗口 = **一个窗口**。
    ///
    /// ⚠️ **停滞不立即上抛**（裁决 G）：第 1、2 个窗口的停滞被**吞掉**，
    /// 宿主**重入引擎**继续跑；第 3 个窗口才触发处置并上抛。
    ///
    /// ⚠️ **重入是安全的**：M-12 的守卫触发时只是 `return Err`，**不改任何
    /// 引擎状态**（不推进虚拟时间、不改寄存器、不动事件队列的游标）⇒ 宿主
    /// 再次调 `run_until` 得到**相同**的引擎快照（验收 **D-7**）。
    ///
    /// ⚠️ **实际空转 3 × `NO_PROGRESS_WINDOW` 次**：M-12 的 `no_progress` 是
    /// `run_until` 的**局部变量** ⇒ **每次重入都从 0 重新计数** ⇒ 三次确认
    /// 实际空转的是 3 × 1024 次，**不是** 1024 次（否则后来人会以为
    /// 「1024 次就停」）。
    fn run_continue_windows(&mut self) -> Result<(), PlatformError> {
        loop {
            match self.observe_continue_window() {
                WindowVerdict::Completed { .. } => return Ok(()),
                // 第 1、2 个窗口：吞掉停滞 ⇒ **重入引擎**再跑一个窗口。
                WindowVerdict::Stalled => continue,
                // 看门狗触发（处置已做）⇒ 上抛终止错误。
                WindowVerdict::Fired(err) => return Err(err),
                // 别的错误（不是活锁）⇒ 照旧上抛。
                WindowVerdict::Failed(err) => return Err(err),
            }
        }
    }

    /// 跑**一个**检查窗口（时间驱动）并判定（规格 §3.3 的六步）。
    pub(crate) fn observe_continue_window(&mut self) -> WindowVerdict {
        // ⚠️ **幂等**：看门狗已经触发过 ⇒ 内核已被终止 ⇒ **不再跑引擎**。
        // （否则死循环：引擎仍然是死的，而 `observe` 不会再返回 `true` ⇒
        // 停滞永远被吞掉 ⇒ 永远重入。）
        if self.watchdog.fired() {
            return WindowVerdict::Fired(
                self.terminated_error("内核已被看门狗终止，不再接受新的运行"),
            );
        }
        // ① 记起点（宿主单调时钟）
        let started = HostInstant::now();
        // ② 跑引擎（一次调用 = 一个指令预算窗口）
        let result = self.backend.continue_until_event().map(|()| 0u32);
        // ③ 算耗时
        let elapsed_ms = started.elapsed_ms_since();
        // ④⑤⑥ 判定 + 喂给看门狗
        self.judge_window(elapsed_ms, &result)
    }

    /// 把**一个窗口**的结果喂给看门狗（规格 §3.3 的第 ④⑤⑥ 步）。
    ///
    /// ⚠️ **只认一个码**：`stalled` 要求 family == `Engine` **且** scenario ==
    /// `E_ENG-03` —— 别的错误**不是**活锁（它们照旧上抛）。
    pub(crate) fn judge_window(
        &mut self,
        elapsed_ms: u64,
        result: &Result<u32, PlatformError>,
    ) -> WindowVerdict {
        // ④ `stalled` = 该批次返回 `Err` **且** family == Engine **且**
        //    scenario == E_ENG_03。
        let stalled = match result {
            Err(err) => err.family == ErrorFamily::Engine && err.scenario == E_ENG_03_NO_PROGRESS,
            Ok(_) => false,
        };
        // ⑤ 窗口判定（**纯函数** ⇒ 不依赖真实时钟 ⇒ 可单测）
        //    ⚠️ **v0.43 批（D-135 · 裁决 C / K）**：**走带时钟可用性的那个**
        //    ⇒ ⇒ `clock_available` 由**宿主**取（**谁持有时钟谁回答**）⇒ ⇒
        //    **宿主时钟读不到时，「时间预算」那一路恒不触发**（**不是**「按 0
        //    算」），而「**引擎停滞**」那一路**照常**。
        let outcome =
            classify_window_with_clock(elapsed_ms, stalled, self.started_at.is_available());
        // ⑥ 喂给看门狗 ⇒ 返回 `true` 表示**本次触发了**
        if self.watchdog.observe(outcome) {
            let err = self.handle_watchdog_trigger(elapsed_ms, stalled);
            return WindowVerdict::Fired(err);
        }
        match result {
            Ok(executed) => WindowVerdict::Completed {
                executed: *executed,
            },
            // 停滞但**未达**确认次数 ⇒ 吞掉这个错误（宿主继续跑）。
            Err(_) if stalled => WindowVerdict::Stalled,
            // 别的错误 ⇒ 照旧上抛。
            Err(err) => WindowVerdict::Failed(err.clone()),
        }
    }

    /// 看门狗触发后的**处置**（顺序固定 · 规格 §3.3）。
    ///
    /// 1. 上报健康信号（M-22）⇒ `classify` 判成 `Severity::Fatal` ⇒
    ///    `Health::Critical`；
    /// 2. 记**一条** M-21 日志（`ctx = "ENG-WATCHDOG"`）；
    /// 3. 置幂等标志（`mark_watchdog_fired`）；
    /// 4. 上报终止错误。
    ///
    /// ⚠️ **降级由 M-22 的策略分支触发（宿主不自己调）** —— v0.46 · STAGE-31
    /// 裁决 **G**：① 上报健康信号那一步里，M-22 查 `degrade_policy` ⇒ ⇒
    /// 命中 `Fatal → Terminate` ⇒ ⇒ M-22 **问**宿主装的降级执行器 ⇒ ⇒ 执行器
    /// 表态 `Done` ⇒ ⇒ M-22 置 `terminated`。
    ///
    /// ⚠️ **为什么宿主不再自己调 `degrade`**：「策略分支 ＋ 宿主直做」并存 ⇒ ⇒
    /// **重复降级** ⇒ ⇒ 降级在宿主里**只有一个入口**（M-22 的策略分支）。
    ///
    /// ⚠️ **顺序要点**：降级发生在**幂等标志置位之前** ⇒ ⇒ `report` 里的降级
    /// 闸门（`!watchdog_fired`）才**开着** ⇒ ⇒ 策略分支**能**生效。
    ///
    /// ⚠️ **场景码分工**（规格 §8.1）：`E_HMS_01_DEGRADE_FAILED` 由 **M-22**
    /// 上报；**`E_HMS_02_FATAL_TERMINATED` 由宿主（这里）上报** ⇒ 前端据此
    /// 区分「内核被终止」与「普通错误」。
    fn handle_watchdog_trigger(&mut self, elapsed_ms: u64, stalled: bool) -> PlatformError {
        let now_ps = self.backend.engine().now_ps();
        let t_host_ms = self.host_now_ms();
        let streak = self.watchdog.streak();
        // ① 健康信号（**先分级再入流** ⇒ 流里存的是 `Severity::Fatal`）。
        //    ⚠️ `err_code` 用**引擎侧**的停滞码（`E_ENG-03`）—— 那是让宿主
        //    判定「停滞」的那个码；HMS 自己的终止码是 `E_HMS-02`（下面报）。
        //    ⚠️ **降级就在这一步发生**（M-22 查策略 ⇒ 问执行器）⇒ ⇒ 宿主
        //    **不再**自己调 `degrade`（裁决 G）。
        self.hms.report(HealthSignal {
            src: SignalSrc::Engine,
            kind: SignalKind::Watchdog,
            ctx: SignalCtx::GuestRun,
            err_code: Some(WATCHDOG_ERR_CODE.to_string()),
            t_virt_ps: now_ps,
            t_host_ms,
        });
        // ② M-21 环里一条 `WATCHDOG` 日志（`ctx` 12 字节 / `err_code` 9 字节
        //    ⇒ 都在 M-21 的上限内 ⇒ **不会被截断**）。
        let msg = format!("看门狗触发：连续 {streak} 个检查窗口无进展，已终止内核");
        self.log_buffer.borrow_mut().log(
            LogLevel::Error,
            WATCHDOG_LOG_CTX,
            &msg,
            WATCHDOG_ERR_CODE,
            now_ps,
            t_host_ms,
        );
        // ③ 幂等标志（一次运行只处置一次）。
        self.hms.mark_watchdog_fired();
        // ④ 上报终止错误（由**宿主**上报 —— 见上面的场景码分工）。
        let detail = if stalled {
            format!("{msg}（停滞场景码 {WATCHDOG_ERR_CODE}）")
        } else {
            format!("{msg}（宿主时间超窗：本窗口 {elapsed_ms} ms > {HOST_WATCHDOG_BUDGET_MS} ms）")
        };
        self.terminated_error(&detail)
    }

    /// 宿主请求暂停。
    pub(super) fn on_pause(&mut self) -> Vec<EventPayload> {
        let _ = self.backend.pause();
        let pc = self.backend.engine().snapshot().pc;
        vec![
            self.tick(),
            EventPayload::Halted {
                reason: HaltReason::UserPause,
                pc,
                // 宿主暂停与 store 边界无关。
                trigger_addr: None,
            },
        ]
    }

    /// 读一段通用寄存器（契约 v2 · `ReadRegisters`）。
    ///
    /// 语义逐字取自 `docs/detailed-design/STAGE-2_Contract_V2.md` §5
    /// （`count = 0` 的口径经父会话订正：见下面「回复空区间」）：
    ///
    /// - `index` / `count` **钳到** `[0, 32)`；`index` 钳到 `0..31`
    ///   （即 x31），区间右端也钳到寄存器表末尾；
    /// - `count == 0` → **回复空区间**（`values` 是空数组）；
    /// - 回复 `Registers { index, values }` 里的 `index` 是**实际**起点，
    ///   `values.len()` 是**实际**条数。
    ///
    /// ⚠️ **读是钳位、写是拒绝**：读越界没有副作用，钳到最近的合法寄存器
    /// 是合理的降级；写越界若也钳位，会**静默**写到别的寄存器。
    ///
    /// ⚠️ **走引擎状态**：寄存器值来自 M-17 → M-12 的那份 [`SimState`]，
    /// 宿主**不持有**影子副本（否则会出现「事件发出去了、引擎里的值没变」
    /// 这种两边漂移）。
    pub(super) fn on_read_registers(&mut self, index: u32, count: u32) -> Vec<EventPayload> {
        let start = index.min(31);
        // 区间右端不能越过寄存器表末尾：`start + count <= 32`。
        let count = count.min(32 - start);
        // ⚠️ `count == 0` **照样回复**（`values` 是空数组）：请求必须被
        // **回答**。M-05 按 `reqId` 配对请求与回复——「没有回复」与「消息
        // 丢失」在协议上**不可区分**，零回复会被前端当成传输故障，在
        // 500 ms 后以 `E_CTX-06`（命令超时）拒绝：用户看到的是「内核没
        // 应答」，而不是「读到了空区间」。
        let gpr = self.backend.register_file();
        let end = (start + count) as usize;
        let values = gpr[start as usize..end].to_vec();
        vec![EventPayload::Registers {
            index: start,
            values,
        }]
    }

    /// 栈回溯（`Backtrace` · STAGE-55 栈回溯批）。
    ///
    /// 逐帧往上爬：`frames[0]` 是**当前** pc，之后每帧是「上一层函数的返回
    /// 地址」。每一帧只给 **pc + 函数名**；**不**解析上层帧的变量（名字只在
    /// 当前帧有效 —— 用户拍板收窄的范围）。
    ///
    /// 爬不上去的**四种**成因统一折成 `truncated = true`：
    ///
    /// 1. `pc` 不在任何 FDE 里（没有 CFI）→ 不知道 CFA 在哪；
    /// 2. 这一行没给 RA 规则（`ra_offset == None`）→ 不知道返回地址存哪；
    /// 3. 返回地址读不出来（`[CFA + ra_offset]` 越界 / 总线拒绝）；
    /// 4. 帧数到上限或**检测到环**（返回地址回到已出现过的 pc）。
    ///
    /// ⚠️ **返回地址为 0 不算截断**：那是栈底（`main` 之上没有调用者），
    /// 是**正常终点**。把它当截断会让每一次回溯都挂个「被截断」的旗。
    ///
    /// ⚠️ **已知近似**：只追 CFA 与 RA 两条规则，callee-saved 寄存器**不**
    /// 逐帧恢复 ⇒ ⇒ 帧里 `sp` 用上一帧的 CFA（这条是精确的），但若 CFA
    /// 基于 `s0` 之类的基址寄存器，爬到深帧时用的是**当前帧**的寄存器值。
    /// 这一层近似写进台账，不做完整 unwind。
    pub(super) fn on_backtrace(&mut self) -> Vec<EventPayload> {
        /// 帧数上限：够覆盖正常调用深度，又不至于让环检测跑飞。
        const MAX_FRAMES: usize = 32;

        let mut regs = self.backend.register_file();
        let mut pc = self.backend.pc();
        let mut frames: Vec<StackFrame> = Vec::new();
        let mut truncated = false;

        loop {
            frames.push(StackFrame {
                pc,
                function: self.index.function_at(pc),
            });
            if frames.len() >= MAX_FRAMES {
                // 还有下一帧没取 ⇒ 是「没爬完」，不是「到顶」。
                truncated = true;
                break;
            }
            let Some(row) = self.index.cfi().row_at(pc) else {
                truncated = true;
                break;
            };
            let (Some(cfa), Some(ra_offset)) = (row.cfa, row.ra_offset) else {
                truncated = true;
                break;
            };
            let base = match regs.get(usize::from(cfa.reg)) {
                Some(value) => *value,
                None => {
                    truncated = true;
                    break;
                }
            };
            let cfa_value = match base.checked_add_signed(cfa.offset) {
                Some(value) => value,
                None => {
                    truncated = true;
                    break;
                }
            };
            let ra_addr = match cfa_value.checked_add_signed(ra_offset) {
                Some(value) => value,
                None => {
                    truncated = true;
                    break;
                }
            };
            let Ok(ra) = self.backend.read_memory(ra_addr, WriteSize::Word) else {
                truncated = true;
                break;
            };
            // 返回地址 0 ⇒ 栈底（正常终点，不是截断）。
            if ra == 0 {
                break;
            }
            // 环：返回地址又落回已经出现过的帧。
            if frames.iter().any(|frame| frame.pc == ra) {
                truncated = true;
                break;
            }
            // 上一层的 `sp` 就是这一层的 CFA（这条在 RISC-V 上是精确的）。
            regs[2] = cfa_value;
            pc = ra;
        }

        vec![EventPayload::Backtrace { frames, truncated }]
    }

    /// 列出**当前帧的局部变量**（`ListLocals` · 真相候选列表批）。
    ///
    /// 只看**当前 pc 所在的函数**（用户拍板：跨帧变量不做）；每条只给
    /// 「名字 + 位置」的人读文本 —— **不带值**（值要读内存 / 读寄存器，
    /// 是另一条路）、**不带类型**（要顺 `DW_AT_type` 再走一遍 DIE）。
    ///
    /// 三种合法结果：
    ///
    /// * `function = None` ⇒ 当前 pc 不在任何已知函数里（没有调试信息 /
    ///   停在库代码 / 停在裸地址）；
    /// * `function = Some(..)` 且 `vars` 空 ⇒ 函数区间在、变量没了（读不
    ///   到 CFI 就这结果），**不是**「没有调试信息」；
    /// * `function = Some(..)` 且 `vars` 有货 ⇒ 正常。
    pub(super) fn on_list_locals(&mut self) -> Vec<EventPayload> {
        /// 变量数上限：一个函数的局部变量 / 形参通常个位数；
        /// 真超了也要明确「列表不完整」，不能悄悄截。
        const MAX_VARS: usize = 128;

        let pc = self.backend.pc();
        let Some(scope) = self.index.locals().scope_at(pc) else {
            return vec![EventPayload::Locals {
                function: None,
                vars: Vec::new(),
                truncated: false,
            }];
        };
        let vars: Vec<LocalVarInfo> = scope
            .vars
            .iter()
            .take(MAX_VARS)
            .map(|var| LocalVarInfo {
                name: var.name.clone(),
                location: var.location.describe(),
            })
            .collect();
        vec![EventPayload::Locals {
            function: Some(scope.name.clone()),
            truncated: scope.vars.len() > MAX_VARS,
            vars,
        }]
    }

    /// 写**一个**通用寄存器（契约 v2 · `WriteRegister`）。
    ///
    /// 语义逐字取自 `docs/detailed-design/STAGE-2_Contract_V2.md` §5：
    ///
    /// - `index == 0` → **丢弃**（D-04：x0 恒等于零），但**仍回复**
    ///   （回复里的值恒为 0）；
    /// - `index > 31` → **拒绝**（`E_DBG-01`），寄存器文件**不变**；
    /// - 回复 `Registers { index, values }`，`values.len() == 1`。
    ///
    /// 回复里的值是**写完之后从引擎状态读回来的**实际值——不是命令里
    /// 带来的那个值：x0 的写被丢弃了，回 0 才是真的。
    pub(super) fn on_write_register(&mut self, index: u32, value: u32) -> Vec<EventPayload> {
        if let Err(err) = self.backend.set_gpr(index, value) {
            // `index > 31`：拒绝（`E_DBG-01`）。寄存器文件**原样不动**。
            return vec![self.error_from(&err)];
        }
        let actual = self.backend.register_file()[index as usize];
        vec![EventPayload::Registers {
            index,
            values: vec![actual],
        }]
    }

    /// 协议 / 能力协商（契约 v2 · `Negotiate`）。
    ///
    /// - 宿主要的**最低**版本高于内核支持的版本 → **拒绝**（`E_PROTO-01`）；
    /// - 否则回 `SCHEMA_VERSION` 与内核**授予**的能力位
    ///   （[`KERNEL_CAP_BITS`]）。
    ///
    /// ⚠️ **最低版本，不是最高版本**：宿主说「我最低能说 v2」= 宿主懂
    /// v2 的全部语法；内核回它自己认得的版本（= `SCHEMA_VERSION`）。
    /// 宿主说「我最低能说 v3」= 宿主要内核至少懂 v3，而内核只懂 v2 →
    /// 拒。
    pub(super) fn on_negotiate(
        &mut self,
        min_schema_version: u32,
        _host_cap_bits: u64,
    ) -> Vec<EventPayload> {
        if min_schema_version > SCHEMA_VERSION {
            let err = PlatformError::new(
                PROTO_FAMILY,
                E_PROTO_01_VERSION_MISMATCH,
                format!(
                    "宿主要求的最低协议版本 {min_schema_version} 高于内核支持的 {SCHEMA_VERSION}"
                ),
            );
            return vec![self.error_from(&err)];
        }
        // ⚠️ 宿主**提供**的 `cap_bits` 在这里**不参与**裁决：契约规格
        // §5 只说「回内核授予的 cap_bits」，没定义「取交集」之类的规则。
        // 宁可按规格字面实现（内核授予什么就是什么），也不自己发明一条
        // 规格里没有的语义——那样前端按位降级的依据就成了猜测。
        vec![EventPayload::Negotiated {
            schema_version: SCHEMA_VERSION,
            cap_bits: KERNEL_CAP_BITS,
            kernel: kernel_descriptor(),
        }]
    }

    // -- 契约 v5 的 console 输入方向（STAGE-9 裁决 A） ---------------------

    /// 往 console UART **写**（契约 v5 · `WriteConsole`）。
    ///
    /// ⚠️ **抬线是同步器的活，不是命令的活**：命令只负责「把字节放进去 +
    /// 让同步器跑一次」。**一条命令不产生两个真相** —— 宿主**不**另写
    /// 一份抬线逻辑（那会让「UART 就绪」与「中断线」各有一个判据）。
    ///
    /// ## 这个命令做**三件事**，顺序不能换
    ///
    /// 1. **把字节逐个塞进 UART 的接收 FIFO**（走
    ///    [`Self::uart_receive`] ⇒ `Uart16550::receive`）。
    /// 2. **`LSR` 的 `RX_VALID`（bit0）随之置位** —— 这是 **UART 模型
    ///    自己的事**（`receive()` 已经做了）⇒ 宿主**不手动改 LSR**
    ///    （手动改就是「第二个真相」：寄存器与 FIFO 各说各话）。
    /// 3. **调一次同步器**（[`Self::sync_interrupts`]）⇒ UART 的 RX 就绪
    ///    **且** `IER` 的 RX 中断使能位开着 ⇒ **按连线表**抬 PLIC 的源
    ///    （v0.34 · D-125：那个源号来自**描述**，不是源码里的字面量）⇒
    ///    PLIC 交付 ⇒ **M-16 的外部中断线**被拉起（`IRQ_UART_RX_READY` = 1）
    ///    ⇒ guest 看到的 `mcause` 是 **`0x8000_000B`**（架构码 11）。
    ///
    /// ## 回复
    ///
    /// ⚠️ **必须至少有一个事件**：前端的 M-05 命令路由是「**第一个**带
    /// `reqId` 的事件 resolve 掉这个命令」⇒ 返回空数组 ⇒ 前端的
    /// Promise **永远不落地**（只能等超时）。⇒ 与 `SetBreakpoint` /
    /// `ClearBreakpoint` / `Nop` 一致：返回 `vec![self.tick()]`。
    ///
    /// ## 空字节
    ///
    /// `bytes` 为空 ⇒ **什么都不做**（不碰 FIFO、不跑同步器），但
    /// **照样回复** —— 幂等，**不是**错误。
    ///
    /// ⚠️ **本批不做「往哪个 UART 写」的选择**：平台只有 UART0（且只有
    /// QEMU virt 档才映射它）。默认档没有 UART ⇒ 字节无处可去 ⇒
    /// [`Self::uart_receive`] 什么都不做（**不报错**）。
    pub(super) fn on_write_console(&mut self, bytes: Vec<u8>) -> Vec<EventPayload> {
        if bytes.is_empty() {
            // ⚠️ 幂等：什么都不做，但**照样回复**（前端要看到回复落地）。
            return vec![self.tick()];
        }
        for byte in bytes {
            self.uart_receive(byte);
        }
        // ⚠️ 同步器**跑一次** ⇒ 外设的中断源被同步到 M-16（UART RX 就绪
        // ⇒ PLIC 源 10 ⇒ M-16 的外部中断线）。
        self.sync_interrupts();
        vec![self.tick()]
    }
}

/// 「M-19 的波形管道不可用」的错误（`E_OBS-02`）。
pub(crate) fn pipeline_unavailable(message: &str) -> PlatformError {
    PlatformError::new(
        ErrorFamily::Observability,
        E_OBS_02_PIPELINE_UNAVAILABLE,
        message.to_string(),
    )
}
