//! M-16 中断控制器单元测试。
//!
//! ⚠️ **判别式全部落在控制器的输出上** —— 队列内容 / 交付结果 / 导出
//! 形状。「不抛错」「调用了某个内部函数」这类恒真断言**没有判别力**，本
//! 文件里一条都没有。
//!
//! ⚠️ **断言不自指**：期望值取自**契约层**的常量
//! （`riscv_sim_contract::csr`）或冻结规格（STAGE-4 §1.2 的优先级表、
//! §2.3 的字段语义）里的**字面量** —— 不用本模块自己定义的常量当期望
//! （本模块的常量由 [`the_trigger_mode_constants_match_the_frozen_spec`]
//! 单独钉死）。

use super::*;
use riscv_sim_contract::csr::{
    IRQ_BASE, IRQ_TIMER_COMPARE, IRQ_UART_RX_READY, IRQ_UART_TX_EMPTY, MIE_MEIE_BIT, MIE_MTIE_BIT,
    MSTATUS_MIE_BIT, NO_ACTIVE_IRQ,
};
use riscv_sim_contract::session::SessionSnapshotDto;
use std::cell::RefCell;
use std::rc::Rc;

/// `mie` 的「机器外部中断」位（→ 线 1 / 线 2）。
const MIE_EXTERNAL: u32 = 1 << MIE_MEIE_BIT;
/// `mie` 的「机器定时器中断」位（→ 线 3）。
const MIE_TIMER: u32 = 1 << MIE_MTIE_BIT;
/// `mstatus` 的全局中断使能位。
const MSTATUS_ON: u32 = 1 << MSTATUS_MIE_BIT;

/// 触发模式的**字面量**（契约层 `PendingIrqDto.trigger_type` 的字段语义：
/// `0` = 电平，`1` = 边沿）。
const LEVEL: u8 = 0;
const EDGE: u8 = 1;

// ---------------------------------------------------------------------------
// 请求 / 去重
// ---------------------------------------------------------------------------

/// **同线去重**：同一条线连来两次，队列里只有 **1** 条（LLD 评审 16-2）。
#[test]
fn a_repeated_request_on_the_same_line_is_folded_into_one_queue_entry() {
    let mut intc = InterruptController::new();

    assert!(
        intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
            .expect("request"),
        "第一次请求必须真的登记"
    );
    assert!(
        !intc
            .request(IRQ_UART_RX_READY, 200, "uart", EDGE)
            .expect("request"),
        "第二次请求必须被折叠"
    );

    assert_eq!(
        intc.pending_lines(),
        vec![IRQ_UART_RX_READY],
        "同线去重：队列里只能有一条"
    );
}

/// 被折叠掉的重复请求**不**改写原条目的时刻（否则「最早」会漂）。
#[test]
fn a_folded_request_does_not_rewrite_the_queued_entry() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");
    intc.request(IRQ_UART_RX_READY, 900, "uart", EDGE)
        .expect("request");

    let state = intc.export_state();
    assert_eq!(state.pending.len(), 1);
    assert_eq!(
        state.pending[0].request_time_ps, 100,
        "被折叠的请求不许改写原条目的请求时刻"
    );
}

/// 队列按 `(request_time_ps, insert_seq)` **规范序** —— 与请求的先后
/// 无关（迟到的请求排在前面）。
#[test]
fn the_queue_keeps_the_canonical_order_by_request_time() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_TIMER_COMPARE, 200, "timer", LEVEL)
        .expect("request");
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");

    assert_eq!(
        intc.pending_lines(),
        vec![IRQ_UART_RX_READY, IRQ_TIMER_COMPARE],
        "队列必须按请求时刻升序 —— 迟到的请求排在前面"
    );
}

// ---------------------------------------------------------------------------
// 仲裁
// ---------------------------------------------------------------------------

/// 仲裁取**最高优先级**（STAGE-4 §1.2 的表：线 3 = 2，线 1 / 2 = 1）。
#[test]
fn arbitration_takes_the_highest_priority_line() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");
    intc.request(IRQ_TIMER_COMPARE, 200, "timer", LEVEL)
        .expect("request");

    let delivered = intc
        .poll(300, MIE_EXTERNAL | MIE_TIMER, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");

    assert_eq!(
        delivered.line, IRQ_TIMER_COMPARE,
        "定时器线优先级最高（= 2），必须压过外部线（= 1）"
    );
    assert_eq!(intc.active_irq(), IRQ_TIMER_COMPARE);
}

/// 同优先级按 `(request_time_ps, insert_seq)` 取**最早**的。
#[test]
fn equal_priority_lines_are_ordered_by_request_time() {
    let mut intc = InterruptController::new();
    // 线 1 与线 2 的默认优先级**相同**（都是 1）。
    intc.request(IRQ_UART_RX_READY, 200, "uart", EDGE)
        .expect("request");
    intc.request(IRQ_UART_TX_EMPTY, 100, "uart", EDGE)
        .expect("request");

    let delivered = intc
        .poll(300, MIE_EXTERNAL, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");

    assert_eq!(
        delivered.line, IRQ_UART_TX_EMPTY,
        "同优先级取请求时刻**最早**的那条"
    );
}

/// 同优先级**且**同一时刻 ⇒ 按插入序取最早的那条。
#[test]
fn equal_priority_and_equal_time_lines_are_ordered_by_insert_seq() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");
    intc.request(IRQ_UART_TX_EMPTY, 100, "uart", EDGE)
        .expect("request");

    let delivered = intc
        .poll(300, MIE_EXTERNAL, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");

    assert_eq!(
        delivered.line, IRQ_UART_RX_READY,
        "同一时刻按插入序取最早的那条"
    );
}

/// **未使能的高优先级不阻塞**已使能的低优先级（LLD 评审 16-1）。
#[test]
fn a_disabled_high_priority_line_does_not_block_an_enabled_low_priority_one() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_TIMER_COMPARE, 100, "timer", LEVEL)
        .expect("request");
    intc.request(IRQ_UART_RX_READY, 200, "uart", EDGE)
        .expect("request");

    // `mie` 只开了外部中断位（bit 11），定时器位（bit 7）**没开**。
    let delivered = intc
        .poll(300, MIE_EXTERNAL, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");

    assert_eq!(
        delivered.line, IRQ_UART_RX_READY,
        "未使能的高优先级不许阻塞已使能的低优先级"
    );
    assert_eq!(
        intc.pending_lines(),
        vec![IRQ_TIMER_COMPARE, IRQ_UART_RX_READY],
        "未使能的那条**保持挂起**（不丢）"
    );
}

/// `mstatus.MIE == 0` ⇒ 不交付，但**挂起不丢**。
#[test]
fn the_global_enable_bit_gates_delivery_without_losing_the_pending_entry() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");

    let delivered = intc.poll(300, MIE_EXTERNAL, 0).expect("poll");
    assert!(delivered.is_none(), "mstatus.MIE == 0 ⇒ 本轮不交付");
    assert_eq!(
        intc.pending_lines(),
        vec![IRQ_UART_RX_READY],
        "全局关掉也不许把挂起丢掉"
    );
    assert_eq!(intc.active_irq(), NO_ACTIVE_IRQ);

    // 全局打开后，同一条挂起被交付 —— 「挂起不丢」的可观察后果。
    let delivered = intc
        .poll(400, MIE_EXTERNAL, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");
    assert_eq!(delivered.line, IRQ_UART_RX_READY);
}

/// **单级嵌套防护**：已交付未清除的中断不许被第二次仲裁覆盖
/// （LLD 评审 16-6）。
#[test]
fn a_delivered_interrupt_blocks_the_next_arbitration() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");

    let first = intc
        .poll(300, MIE_EXTERNAL, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");
    assert_eq!(intc.active_irq(), IRQ_UART_RX_READY);

    assert!(
        intc.poll(400, MIE_EXTERNAL, MSTATUS_ON)
            .expect("poll")
            .is_none(),
        "已有活动中断 ⇒ 本轮不交付（单级语义）"
    );
    assert_eq!(
        intc.active_irq(),
        IRQ_UART_RX_READY,
        "第二次仲裁不许覆盖已交付的中断"
    );

    // 取走交付**不等于**清除中断。
    assert_eq!(intc.take_intr(), Some(first));
    assert!(
        intc.poll(500, MIE_EXTERNAL, MSTATUS_ON)
            .expect("poll")
            .is_none(),
        "取走交付之后仍然不许再交付 —— 要等 handler 清除"
    );

    // 清除 ⇒ 可以再交付（清 → 再请求 → 再交付）。
    intc.clear(IRQ_UART_RX_READY);
    assert_eq!(intc.active_irq(), NO_ACTIVE_IRQ);
    intc.request(IRQ_UART_RX_READY, 600, "uart", EDGE)
        .expect("request");
    let again = intc
        .poll(700, MIE_EXTERNAL, MSTATUS_ON)
        .expect("poll")
        .expect("清除之后应当能再次交付");
    assert_eq!(again.line, IRQ_UART_RX_READY);
}

/// `clear(line)` 移除挂起项 + 撤销线电平 + 复位 `active_irq`。
#[test]
fn clear_removes_the_queue_entry_the_level_and_the_active_line() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");
    intc.poll(300, MIE_EXTERNAL, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");

    intc.clear(IRQ_UART_RX_READY);

    assert!(
        intc.pending_lines().is_empty(),
        "clear 之后队列里不该还有这条线"
    );
    assert!(intc.raised_lines().is_empty(), "clear 之后线电平必须被撤销");
    assert_eq!(
        intc.active_irq(),
        NO_ACTIVE_IRQ,
        "clear 之后 active_irq 必须复位"
    );
}

/// `clear` 也要丢掉**已登记但尚未取走**的交付 —— 「清掉了」就不能再
/// 交付（LLD §7.1 的 `irqClear` 用例）。
#[test]
fn clear_drops_a_delivery_that_was_not_taken_yet() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");
    intc.poll(300, MIE_EXTERNAL, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");
    assert!(intc.pending_intr().is_some());

    intc.clear(IRQ_UART_RX_READY);

    assert!(
        intc.pending_intr().is_none(),
        "清掉的线不许还挂着待交付的交接结构"
    );
}

// ---------------------------------------------------------------------------
// 使能掩码的量子边界刷新
// ---------------------------------------------------------------------------

/// 量子边界从 CSR `mie` **刷新**使能掩码（LLD 评审 16-5）。
#[test]
fn the_quantum_boundary_refreshes_the_enable_mask_from_mie() {
    let mut intc = InterruptController::new();

    intc.poll(0, MIE_EXTERNAL, 0).expect("poll");
    assert_eq!(
        intc.enabled_mask(),
        (1 << IRQ_UART_RX_READY) | (1 << IRQ_UART_TX_EMPTY),
        "mie 的 bit 11 ⇒ 线 1 与线 2 的使能位"
    );

    intc.poll(1, MIE_TIMER, 0).expect("poll");
    assert_eq!(
        intc.enabled_mask(),
        1 << IRQ_TIMER_COMPARE,
        "mie 的 bit 7 ⇒ 线 3 的使能位；外部位没开 ⇒ 线 1 / 2 被清掉"
    );
}

/// **没有** `mie` 位映射的线，刷新时**保持原值**（不静默清零）。
#[test]
fn a_line_without_a_mie_bit_keeps_its_enable_bit() {
    let mut intc = InterruptController::new();
    intc.set_enabled_mask(1 << 9);

    intc.poll(0, 0, 0).expect("poll");

    assert_eq!(
        intc.enabled_mask(),
        1 << 9,
        "没有 mie 位映射的线必须保持原值 —— 清零 = 静默关中断"
    );
}

// ---------------------------------------------------------------------------
// 定时器中断
// ---------------------------------------------------------------------------

/// `mtime >= mtimecmp` ⇒ 按**电平**语义挂起线 3（且**不堆积**）。
#[test]
fn the_timer_raises_line_three_when_mtime_reaches_the_compare_value() {
    let mut intc = InterruptController::new();
    intc.set_timer_compare(1000);

    let delivered = intc
        .poll(2000, MIE_TIMER, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");

    assert_eq!(delivered.line, IRQ_TIMER_COMPARE);
    assert_eq!(intc.mtime(), 2000, "mtime 必须跟着虚拟时间走");
    assert_eq!(
        intc.pending_lines(),
        vec![IRQ_TIMER_COMPARE],
        "定时器线只挂一条"
    );

    // 电平持续成立 —— 再采一次样**不许**堆积。
    intc.poll(3000, MIE_TIMER, MSTATUS_ON).expect("poll");
    assert_eq!(
        intc.pending_lines(),
        vec![IRQ_TIMER_COMPARE],
        "电平语义 + 同线去重 ⇒ 不堆积"
    );
}

/// `mtime == mtimecmp`（**恰好到达**）也必须挂起 —— `>=` 的**等号侧**。
///
/// ⚠️ 这条是**专门补的**（v0.19 检视轮 V-B · F-2）：上一条用例的名字
/// 叫「when mtime **reaches** the compare value」，但它用的是 `2000 > 1000`
/// —— **等号那一边根本没人测**。把 `>=` 改成 `>` 之后，**没有任何一条
/// 用例会红**（V-B 的变异 V-07 实测 0 红）。
///
/// 判别式落在**控制器自己的输出**上（交付了哪条线）。
#[test]
fn the_timer_raises_the_line_when_mtime_equals_the_compare_value() {
    let mut intc = InterruptController::new();
    intc.set_timer_compare(1000);

    let delivered = intc
        .poll(1000, MIE_TIMER, MSTATUS_ON)
        .expect("poll")
        .expect("**恰好**到达比较值也必须交付（`>=` 的等号侧）");

    assert_eq!(
        delivered.line, IRQ_TIMER_COMPARE,
        "mtime == mtimecmp 时定时器线必须挂起"
    );
    assert_eq!(
        intc.pending_lines(),
        vec![IRQ_TIMER_COMPARE],
        "等号侧也要真的排上队"
    );
}

/// `mtimecmp == 0` ⇒ 定时器**关闭**（否则 `mtime >= 0` 恒成立）。
#[test]
fn a_zero_compare_value_means_the_timer_is_off() {
    let mut intc = InterruptController::new();

    intc.poll(1000, MIE_TIMER, MSTATUS_ON).expect("poll");

    assert!(
        intc.pending_lines().is_empty(),
        "比较值为 0 ⇒ 定时器不许拉起任何线"
    );
}

/// 定时器条目是**电平**触发、请求源是 `"timer"`。
#[test]
fn the_timer_entry_is_level_triggered_and_comes_from_the_timer_source() {
    let mut intc = InterruptController::new();
    intc.set_timer_compare(10);
    intc.poll(100, MIE_TIMER, MSTATUS_ON).expect("poll");

    let state = intc.export_state();
    assert_eq!(state.pending.len(), 1);
    assert_eq!(
        state.pending[0].trigger_type, LEVEL,
        "定时器线是电平触发（契约层字段语义：0 = 电平）"
    );
    assert_eq!(state.pending[0].source_type, "timer");
    assert_eq!(state.pending[0].line, IRQ_TIMER_COMPARE);
}

// ---------------------------------------------------------------------------
// 脉冲回调（D-26）
// ---------------------------------------------------------------------------

/// `request()` 每次**真的登记**一条挂起时调一次脉冲回调；被折叠的重复
/// 请求**不发**脉冲。
#[test]
fn the_pulse_callback_fires_once_per_registered_request() {
    let pulses = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&pulses);
    let mut intc = InterruptController::new();
    intc.set_pulse_sink(Box::new(move |now_ps, line| {
        sink.borrow_mut().push((now_ps, line));
    }));

    intc.request(IRQ_UART_RX_READY, 42, "uart", EDGE)
        .expect("request");
    intc.request(IRQ_UART_RX_READY, 43, "uart", EDGE)
        .expect("request");
    intc.request(IRQ_UART_TX_EMPTY, 44, "uart", EDGE)
        .expect("request");

    assert_eq!(
        *pulses.borrow(),
        vec![(42, IRQ_UART_RX_READY), (44, IRQ_UART_TX_EMPTY)],
        "每条**真的登记**的挂起一条脉冲；被折叠的重复请求没有脉冲"
    );
}

// ---------------------------------------------------------------------------
// 契约层端口（InterruptSink）
// ---------------------------------------------------------------------------

/// `raise` 的语义 = **登记一条挂起**；`lower` 的语义 = **clear**。
#[test]
fn the_sink_raise_registers_a_pending_entry_and_lower_clears_it() {
    let mut intc = InterruptController::new();

    intc.raise(9).expect("raise");
    assert_eq!(intc.raised_lines(), vec![9], "raise 之后线被断言");
    assert_eq!(
        intc.pending_lines(),
        vec![9],
        "raise 的语义是 request —— 必须真的登记一条挂起"
    );

    intc.lower(9).expect("lower");
    assert!(intc.raised_lines().is_empty(), "lower 之后线电平被撤销");
    assert!(
        intc.pending_lines().is_empty(),
        "lower 的语义是 clear —— 挂起项必须被移除"
    );
}

/// `raise` 用**最近一次采样**的虚拟时间当请求时刻（端口没有时间参数）。
#[test]
fn the_sink_raise_uses_the_last_sampled_virtual_time() {
    let mut intc = InterruptController::new();
    intc.poll(1234, 0, 0).expect("poll");

    intc.raise(9).expect("raise");

    assert_eq!(
        intc.export_state().pending[0].request_time_ps,
        1234,
        "raise 的请求时刻必须是最近一次采样的**虚拟时间**（不是宿主时钟）"
    );
}

// ---------------------------------------------------------------------------
// 交付交接
// ---------------------------------------------------------------------------

/// `mcause` 的编码：中断位（`IRQ_BASE`）+ **架构 cause 码**。
///
/// ⚠️ **v0.23 批**：cause 报的是**架构码**（定时器 `7` / 外部 `11`），
/// **不是**平台的内部线号 —— 内部线号是 `1 / 2 / 3`，拿它当 cause 报的
/// 时候 guest 的中断分发**一个都匹配不上**。
///
/// ⚠️ 断言值是**架构字面量**（`0x8000_0007`），不是从映射函数反推的。
#[test]
fn the_delivery_cause_encodes_the_interrupt_bit_and_the_architectural_code() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_TIMER_COMPARE, 100, "timer", LEVEL)
        .expect("request");

    let delivered = intc
        .poll(300, MIE_TIMER, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");

    assert_eq!(
        delivered.cause, 0x8000_0007,
        "定时器线 ⇒ mcause = 中断位 | 架构码 7"
    );
    assert_eq!(
        delivered.cause & IRQ_BASE,
        IRQ_BASE,
        "中断的 mcause[31] 必须置位"
    );
    // ⚠️ **内部线号不许混进 cause** —— 定时器线的内部线号是 3。
    assert_ne!(
        delivered.cause & 0x7FFF_FFFF,
        u32::from(IRQ_TIMER_COMPARE),
        "cause 的低位必须是**架构码**（7），不是内部线号（3）"
    );
}

/// ⚠️ **外部线**交付时 `mcause` = 中断位 | **架构码 11**。
///
/// 两条 UART 线（内部线号 1 / 2）都映射到**外部中断**（架构码 11）——
/// M 模式只有**一个**外部中断位（`MEIE` = 11），两条 UART 线与 `MEIP`
/// 一样共享它。
#[test]
fn an_external_line_is_delivered_with_the_architectural_external_cause() {
    for line in [IRQ_UART_RX_READY, IRQ_UART_TX_EMPTY] {
        let mut intc = InterruptController::new();
        intc.request(line, 100, "uart", EDGE).expect("request");

        let delivered = intc
            .poll(300, MIE_EXTERNAL, MSTATUS_ON)
            .expect("poll")
            .expect("应当交付一条");

        assert_eq!(
            delivered.cause, 0x8000_000B,
            "外部线 {line} ⇒ mcause = 中断位 | 架构码 11"
        );
        assert_ne!(
            delivered.cause & 0x7FFF_FFFF,
            u32::from(line),
            "cause 的低位必须是**架构码**（11），不是内部线号"
        );
    }
}

/// ⚠️ **交付的 cause 落到 guest 的 `mcause` 上** —— 端到端。
///
/// 这是本批修的那个缺陷的**真正判别式**：guest 的中断分发读的就是
/// `mcause` ⇒ 它必须是**架构码**。
///
/// ⚠️ 判别式落在**输出**上（guest 读 `mcause` 的值），不是「控制器
/// 调了某个函数」。
#[test]
fn the_delivered_cause_reaches_the_guest_as_mcause() {
    // (线, `mie` 使能位, 期望的 guest `mcause`)
    for (line, mie, expected) in [
        (IRQ_TIMER_COMPARE, MIE_TIMER, 0x8000_0007u32),
        (IRQ_UART_RX_READY, MIE_EXTERNAL, 0x8000_000B),
    ] {
        let mut intc = InterruptController::new();
        intc.request(line, 100, "irq", EDGE).expect("request");
        let delivered = intc
            .poll(300, mie, MSTATUS_ON)
            .expect("poll")
            .expect("应当交付一条");

        // 交付的 cause 送进**唯一的陷阱入口** ⇒ guest 的 `mcause`。
        let mut state = riscv_sim_contract::model::SimState::default();
        crate::m13_iss::enter_trap(&mut state, delivered.cause, 0);
        assert_eq!(
            crate::m13_iss::csr::csr_read(&state, riscv_sim_contract::csr::CSR_MCAUSE),
            expected,
            "线 {line} ⇒ guest 的 mcause 必须是架构码（0x8000_0007 / 0x8000_000B）"
        );
    }
}

/// ⚠️ **没有架构 cause 码的线不交付** —— 交付一条 `mcause` 不是架构码的
/// 中断，等于把本批修掉的缺陷换个形式再犯一次。
///
/// 这种线**保持挂起**（与「未使能不丢」同一条不变式）。
#[test]
fn a_line_without_an_architectural_cause_is_not_delivered() {
    let mut intc = InterruptController::new();
    // 线 9：平台没有它 ⇒ 没有 `mie` 位，也没有架构 cause 码。
    intc.request(9, 100, "platform", EDGE).expect("request");
    // 强行打开它的使能位（平台 API 允许），使它成为唯一的候选。
    intc.set_enabled_mask(1 << 9);

    let delivered = intc.poll(300, 0, MSTATUS_ON).expect("poll 本身不报错");
    assert!(
        delivered.is_none(),
        "没有架构 cause 码的线**不得**交付 —— 实得 {delivered:?}"
    );
    // ⚠️ **保持挂起** —— 挂起不丢。
    assert_eq!(
        intc.pending_lines(),
        vec![9],
        "没有架构 cause 码的线必须**保持挂起**"
    );
}

/// `take_intr()` 取走的**就是** `poll()` 登记的那一条（不许两个真相）。
#[test]
fn take_intr_hands_out_exactly_what_poll_registered() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");

    let registered = intc
        .poll(300, MIE_EXTERNAL, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");

    assert_eq!(intc.pending_intr(), Some(registered));
    assert_eq!(intc.take_intr(), Some(registered));
    assert_eq!(intc.pending_intr(), None, "取走即消失");
}

// ---------------------------------------------------------------------------
// 优先级表
// ---------------------------------------------------------------------------

/// 平台的**默认优先级**（STAGE-4 §1.2 的表）：线 3 = 2，线 1 / 2 = 1，
/// 其它线 = 0。
#[test]
fn the_priority_table_falls_back_to_the_platform_defaults() {
    let intc = InterruptController::new();
    assert_eq!(intc.priority_of(IRQ_TIMER_COMPARE), 2);
    assert_eq!(intc.priority_of(IRQ_UART_RX_READY), 1);
    assert_eq!(intc.priority_of(IRQ_UART_TX_EMPTY), 1);
    assert_eq!(intc.priority_of(77), 0, "未知线回落 0（最低）");
}

/// `set_priority` 真的改变仲裁结果（不是「设了但没人看」）。
#[test]
fn set_priority_changes_the_arbitration_outcome() {
    let mut intc = InterruptController::new();
    intc.set_priority(IRQ_UART_RX_READY, 9);
    assert_eq!(intc.priority_of(IRQ_UART_RX_READY), 9);

    // ⚠️ **夹具的讲究**：先请求定时器线（默认优先级 2，队列里排第一），
    // 再请求被提权的外部线（优先级 9，队列里排第二）—— 这样「按优先级
    // 选」与「取队列第一条」给出**不同**的答案，断言才有判别力（否则
    // 「无视优先级」的退化会照样绿）。
    intc.request(IRQ_TIMER_COMPARE, 100, "timer", LEVEL)
        .expect("request");
    intc.request(IRQ_UART_RX_READY, 200, "uart", EDGE)
        .expect("request");
    assert_eq!(
        intc.pending_lines(),
        vec![IRQ_TIMER_COMPARE, IRQ_UART_RX_READY],
        "夹具：被提权的那条在队列里排第二"
    );

    let delivered = intc
        .poll(300, MIE_EXTERNAL | MIE_TIMER, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");
    assert_eq!(
        delivered.line, IRQ_UART_RX_READY,
        "优先级被改成 9 之后，外部线必须压过定时器线 —— 即便它排在队列第二条"
    );
}

// ---------------------------------------------------------------------------
// 会话状态（契约载荷段 5）
// ---------------------------------------------------------------------------

/// `export_state()` / `load_state()` 往返 ⇒ 队列 / 优先级 / 使能 /
/// active / mtime 全部还原。
#[test]
fn export_and_load_round_trip_restores_the_whole_controller_state() {
    let mut intc = InterruptController::new();
    intc.set_priority(IRQ_TIMER_COMPARE, 7);
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");
    intc.request(IRQ_TIMER_COMPARE, 200, "timer", LEVEL)
        .expect("request");
    intc.set_timer_compare(555);
    // 线 3 没使能 ⇒ 交付的是线 1，`active_irq` 因此非空。
    intc.poll(300, MIE_EXTERNAL, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");

    let state = intc.export_state();
    let mut fresh = InterruptController::new();
    fresh.load_state(&state);

    assert_eq!(fresh.export_state(), state, "往返必须逐字段相等");
    assert_eq!(
        fresh.pending_lines(),
        vec![IRQ_UART_RX_READY, IRQ_TIMER_COMPARE],
        "队列必须被还原"
    );
    assert_eq!(fresh.priority_of(IRQ_TIMER_COMPARE), 7, "优先级必须被还原");
    assert_eq!(fresh.active_irq(), IRQ_UART_RX_READY, "active 必须被还原");
    assert_eq!(fresh.mtime(), 300, "mtime 必须被还原");
    assert_eq!(fresh.timer_compare(), 555, "mtimecmp 必须被还原");
    assert_eq!(fresh.enabled_mask(), intc.enabled_mask(), "使能必须被还原");
}

/// `load_state()` 是**整体替换** —— 载荷里没有的不该残留。
#[test]
fn load_state_replaces_the_whole_state() {
    let mut intc = InterruptController::new();
    intc.request(9, 100, "platform", EDGE).expect("request");
    intc.set_priority(9, 4);
    intc.set_enabled_mask(0b1010);
    intc.set_timer_compare(42);

    intc.load_state(&M16IntcDto {
        raised: vec![2],
        ..M16IntcDto::default()
    });

    assert!(
        intc.pending_lines().is_empty(),
        "整体替换：载荷里没有的队列条目不该残留"
    );
    assert_eq!(intc.raised_lines(), vec![2], "已断言的线按载荷还原");
    assert_eq!(
        intc.priority_of(9),
        0,
        "整体替换：优先级表被换掉 ⇒ 线 9 回落默认 0"
    );
    assert_eq!(intc.enabled_mask(), 0, "整体替换：使能掩码被换掉");
    assert_eq!(intc.active_irq(), NO_ACTIVE_IRQ);
    assert_eq!(intc.timer_compare(), 0);
    assert_eq!(intc.mtime(), 0);
}

/// `load_state()` 必须吃下**旧形状**载荷（只有 `raised`）—— 那个时代真的
/// 没有队列，所以**不**凭空造出队列条目。
#[test]
fn load_state_accepts_the_old_payload_shape() {
    let old_shape: M16IntcDto =
        serde_json::from_str(r#"{"raised":[5,7]}"#).expect("旧形状载荷必须能反序列化");

    let mut intc = InterruptController::new();
    intc.load_state(&old_shape);

    assert_eq!(intc.raised_lines(), vec![5, 7], "已断言的线必须被还原");
    assert!(
        intc.pending_lines().is_empty(),
        "旧形状载荷**不得**凭空造出队列条目"
    );
    assert_eq!(intc.active_irq(), NO_ACTIVE_IRQ);
    assert_eq!(intc.enabled_mask(), 0);
    assert_eq!(intc.export_state().pending.len(), 0);
}

/// 回灌之后新请求的**插入序**必须越过载荷里已有的条目 —— 否则两条会
/// 撞序号，导出的载荷就过不了契约层的校验。
#[test]
fn a_request_after_a_restore_continues_the_insert_sequence() {
    let mut intc = InterruptController::new();
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");
    let state = intc.export_state();

    let mut fresh = InterruptController::new();
    fresh.load_state(&state);
    fresh
        .request(IRQ_UART_TX_EMPTY, 200, "uart", EDGE)
        .expect("request");

    let exported = fresh.export_state();
    assert_eq!(exported.pending.len(), 2);
    assert!(
        exported.pending[1].insert_seq > exported.pending[0].insert_seq,
        "插入序必须继续往上走（否则两条会撞序号）"
    );
}

/// 导出的段 5 必须能过**契约层自己的校验**（队列同线不重复 + 规范序 +
/// `active_irq` 在队列里 + `pending ⊆ raised`）。
#[test]
fn the_exported_segment_satisfies_the_contract_validator() {
    let mut intc = InterruptController::new();
    // ⚠️ **夹具的讲究**：请求时刻**递减**（先 200 后 100）—— 这样「按请求
    // 时刻排序」与「按请求先后追加」给出**不同**的队列顺序，校验器那条
    // 「严格升序」的检查才有判别力（递增的夹具会让两种退化看起来一样）。
    intc.request(IRQ_TIMER_COMPARE, 200, "timer", LEVEL)
        .expect("request");
    intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
        .expect("request");
    assert_eq!(
        intc.pending_lines(),
        vec![IRQ_UART_RX_READY, IRQ_TIMER_COMPARE],
        "夹具：队列按请求时刻排序，不是按请求先后"
    );
    intc.poll(300, MIE_EXTERNAL, MSTATUS_ON)
        .expect("poll")
        .expect("应当交付一条");

    let payload = SessionSnapshotDto {
        m16_intc: intc.export_state(),
        ..SessionSnapshotDto::default()
    };
    payload
        .validate()
        .expect("导出的段 5 必须能过契约层的校验（pending ⊆ raised 等）");
}

/// 同一串时间戳化的输入 ⇒ **固定**的交付序列（确定性，AD §8.1 D0）。
#[test]
fn one_timestamped_input_sequence_yields_one_fixed_delivery_sequence() {
    fn run() -> Vec<(u16, u32)> {
        let mut intc = InterruptController::new();
        intc.request(IRQ_UART_RX_READY, 100, "uart", EDGE)
            .expect("request");
        intc.request(IRQ_TIMER_COMPARE, 200, "timer", LEVEL)
            .expect("request");

        let mut out = Vec::new();
        for now in [300u64, 400, 500] {
            if let Some(delivered) = intc
                .poll(now, MIE_EXTERNAL | MIE_TIMER, MSTATUS_ON)
                .expect("poll")
            {
                out.push((delivered.line, delivered.cause));
                intc.clear(delivered.line);
            }
        }
        out
    }

    assert_eq!(
        run(),
        vec![
            // ⚠️ 架构 cause 码（定时器 7 / 外部 11），不是内部线号。
            (IRQ_TIMER_COMPARE, 0x8000_0007),
            (IRQ_UART_RX_READY, 0x8000_000B),
        ],
        "同一串输入 ⇒ 同一串交付（先高优先级，再剩下的）"
    );
}

// ---------------------------------------------------------------------------
// 规格数字的钉死
// ---------------------------------------------------------------------------

/// 触发模式的常量必须是冻结规格 §4.1 的那两个数字。
#[test]
fn the_trigger_mode_constants_match_the_frozen_spec() {
    assert_eq!(TRIGGER_LEVEL, 0, "电平 = 0（契约层字段语义）");
    assert_eq!(TRIGGER_EDGE, 1, "边沿 = 1（契约层字段语义）");
}

// ---------------------------------------------------------------------------
// 定时器条件的「由真变假」⇒ 撤线（STAGE-40 · v0.55 · D-158）
// ---------------------------------------------------------------------------

/// 把控制器武装成「定时器已到期且已交付」的状态。
///
/// ⚠️ **为什么必须先交付**：撤线的效果里有一条是「复位 `active_irq`」⇒ ⇒
/// 只有**交付过**的线才有 `active_irq` 可复位 ⇒ ⇒ 「武装」而不交付的话，
/// 那条断言是**真空**的。
fn arm_and_deliver_the_timer(intc: &mut InterruptController) {
    intc.set_timer_compare(100);
    intc.poll(200, MIE_TIMER, MSTATUS_ON)
        .expect("poll")
        .expect("定时器到期 ⇒ 应当交付一条");
    assert_eq!(
        intc.active_irq(),
        IRQ_TIMER_COMPARE,
        "前置：定时器线必须真的被交付过"
    );
}

/// **A-1**：定时器条件**由真变假** ⇒ ⇒ **撤掉定时器线**。
///
/// ⚠️ 判别式落在**控制器的输出状态**上（`raised` / `pending` / `active_irq`）
/// —— 不是「不抛错」、不是「调用了某个函数」。
#[test]
fn a_timer_condition_that_turns_false_releases_the_timer_line() {
    let mut intc = InterruptController::new();
    arm_and_deliver_the_timer(&mut intc);

    // 条件由真变假：比较值被推到「当前时间之后」。
    intc.set_timer_compare(1_000);

    assert!(
        !intc.raised_lines().contains(&IRQ_TIMER_COMPARE),
        "条件已经为假 ⇒ `raised` 里不许还有定时器线 {:?}",
        intc.raised_lines()
    );
    assert!(
        intc.pending_lines().is_empty(),
        "条件已经为假 ⇒ 挂起队列里不许还有定时器线 {:?}",
        intc.pending_lines()
    );
    assert_eq!(
        intc.active_irq(),
        NO_ACTIVE_IRQ,
        "条件已经为假 ⇒ `active_irq` 必须复位（否则这条线被单级防护永久堵死）"
    );
}

/// **A-2**：条件**仍然成立**时（新的比较值**仍然落在过去**）⇒ ⇒ **不许**撤线。
///
/// ⚠️ **这条是「撤线判据」的那道防线**：判据必须是「条件由真变假」⇒ ⇒
/// 「新值 ≠ 旧值」**不够** —— 新值仍然落在过去 ⇒ ⇒ 条件**仍然成立** ⇒ ⇒
/// 挂起项必须**留着**（它还要被交付）。
#[test]
fn a_timer_condition_that_stays_true_keeps_the_timer_line() {
    let mut intc = InterruptController::new();
    arm_and_deliver_the_timer(&mut intc);

    // 条件**仍然成立**：新的比较值（150）仍然落在当前时间（200）之前。
    intc.set_timer_compare(150);

    assert!(
        intc.raised_lines().contains(&IRQ_TIMER_COMPARE),
        "条件仍然成立 ⇒ `raised` 里必须还有定时器线 {:?}",
        intc.raised_lines()
    );
    assert!(
        intc.pending_lines().contains(&IRQ_TIMER_COMPARE),
        "条件仍然成立 ⇒ 挂起队列里必须还有定时器线 {:?}",
        intc.pending_lines()
    );
    assert_eq!(
        intc.active_irq(),
        IRQ_TIMER_COMPARE,
        "条件仍然成立 ⇒ `active_irq` 不许复位（这条线还没被解除）"
    );
}

/// **A-3**：`set_timer_compare(0)`（**关定时器**）⇒ ⇒ **也**撤线。
///
/// ⚠️ `0` 在 CLINT 与 M-16 两边**同义**地表示「定时器关闭」⇒ ⇒ 条件
/// 同样不成立 ⇒ ⇒ 同样必须撤线。
#[test]
fn disabling_the_timer_releases_the_timer_line() {
    let mut intc = InterruptController::new();
    arm_and_deliver_the_timer(&mut intc);

    intc.set_timer_compare(0);

    assert!(
        !intc.raised_lines().contains(&IRQ_TIMER_COMPARE),
        "定时器被关掉 ⇒ `raised` 里不许还有定时器线 {:?}",
        intc.raised_lines()
    );
    assert!(
        intc.pending_lines().is_empty(),
        "定时器被关掉 ⇒ 挂起队列里不许还有定时器线 {:?}",
        intc.pending_lines()
    );
    assert_eq!(
        intc.active_irq(),
        NO_ACTIVE_IRQ,
        "定时器被关掉 ⇒ `active_irq` 必须复位"
    );
}

/// **A-4**：撤线**只**动定时器线 ⇒ ⇒ **别的线不受影响**。
///
/// ⚠️ **这条是「撤线的范围」那道防线**：判据落在**别的线还在**上 ⇒ ⇒
/// 「把整条队列都清了」这种实现**必须**红。
#[test]
fn releasing_the_timer_line_leaves_the_other_lines_alone() {
    let mut intc = InterruptController::new();
    arm_and_deliver_the_timer(&mut intc);
    intc.request(IRQ_UART_RX_READY, 300, "uart", EDGE)
        .expect("request");

    intc.set_timer_compare(1_000);

    assert!(
        intc.raised_lines().contains(&IRQ_UART_RX_READY),
        "撤定时器线不许连别的线一起撤 {:?}",
        intc.raised_lines()
    );
    assert!(
        intc.pending_lines().contains(&IRQ_UART_RX_READY),
        "撤定时器线不许连别的线的挂起项一起清 {:?}",
        intc.pending_lines()
    );
}
