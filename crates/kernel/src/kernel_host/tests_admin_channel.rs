//! 管理员通道（v0.44 · STAGE-29 · 验收 **B-1 ~ B-6** + 边界 **X-3 / X-4**）。
//!
//! 冻结规格：`docs/detailed-design/STAGE-29_V044_Contract_V8_Admin_Carrier_Code_Landing.md`
//! §3.1（宿主侧新增）· §3.2（M-21 / M-22 的转换）· §3.3（内核侧的字面量收口）·
//! §5.2（B 组）· §5.5（X 组）。
//!
//! ⚠️ **默认行为一字不变**（裁决 G · X-3）：本文件里「默认」那一批用例钉的是
//! 「宿主对 `Admin` 命令的回应**逐字不变**」⇒ ⇒ 任何动默认路径的实现都会让
//! 它们变红。
//!
//! ⚠️ **断言不许写软**：钉的是**具体的码**（`E_CTX-05`，不是「包含 `E_CTX`」）
//! 与**逐字的消息**，⇒ ⇒ 「把 `admin_enabled` 的判断写反」会让默认那一批与
//! 开启那一批**同时**变红 ⇒ ⇒ 判别力链（改前红 / 改后绿）成立。

use super::*;

use riscv_sim_contract::admin::AdminDiagDto;

// ---------------------------------------------------------------------------
// 测试辅助
// ---------------------------------------------------------------------------

/// 派发一条命令，返回它产生的事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 派发一条**管理员命令**。
fn admin(host: &mut KernelHost, op: AdminOp) -> Vec<EventPayload> {
    dispatch(host, CommandPayload::Admin { op })
}

/// 「管理通道未启用」那个错误的**逐字**样子（X-3 的基准）。
fn disabled_reply() -> EventPayload {
    EventPayload::Error {
        code: "E_CTX-05".to_string(),
        message: "管理通道未启用（admin op 不在当前会话的能力面内）".to_string(),
    }
}

/// 取**恰好一个** `AdminLogs` 事件的条目。
///
/// ⚠️ 「**恰好一个**」是硬要求（B-2）⇒ ⇒ 事件数不是 1 就**直接失败**
/// ⇒ ⇒ 绝不允许「一个 AdminLogs + 一个别的」。
fn only_admin_logs(events: &[EventPayload]) -> &[LogEntryDto] {
    assert_eq!(
        events.len(),
        1,
        "管理员通道**恰好一个**事件（不是两个，也不是零）：{events:?}"
    );
    match &events[0] {
        EventPayload::AdminLogs { entries } => entries,
        other => panic!("应当是 AdminLogs（**不是** E_CTX-05）：{other:?}"),
    }
}

/// 取**恰好一个** `AdminDiag` 事件的诊断包。
fn only_admin_diag(events: &[EventPayload]) -> &AdminDiagDto {
    assert_eq!(
        events.len(),
        1,
        "管理员通道**恰好一个**事件（不是两个，也不是零）：{events:?}"
    );
    match &events[0] {
        EventPayload::AdminDiag { diag } => diag,
        other => panic!("应当是 AdminDiag（**不是** E_CTX-05）：{other:?}"),
    }
}

/// M-21 环的一份**快照**（纯读）。
fn ring_snapshot(host: &KernelHost) -> Vec<LogEntry> {
    host.log_buffer.borrow().query(&LogQuery::default())
}

/// **独立地**从一份环快照算出「一次导出应该是什么」—— 走 `query`（带状
/// 返回）而不是 `query_recent`（全局最近）⇒ ⇒ 两条路径**必须**给出同一个
/// 答案，否则 B-3 就破了。
///
/// ⚠️ **快照必须是「导出那一刻」的**：截断发生时宿主会**事后**往环里写一条
/// 截断说明 ⇒ ⇒ 导出**之前**的快照才是这次导出的判据。
fn expected_export(ring: &[LogEntry], cap: usize) -> Vec<LogEntry> {
    let mut ring = ring.to_vec();
    // ⚠️ **稳定**排序（与 `query_recent` 的口径一致：排序键 = 虚拟时间，
    // 同刻的次序 = 带序 + 带内写入序）。
    ring.sort_by_key(|entry| entry.time_virt_ps);
    let start = ring.len().saturating_sub(cap);
    ring.split_off(start)
}

// ---------------------------------------------------------------------------
// B-1 / X-3 · 默认关，且默认回应**一字不变**
// ---------------------------------------------------------------------------

#[test]
fn the_admin_channel_is_off_by_default_and_its_reply_is_unchanged_word_for_word() {
    let mut host = KernelHost::new();
    assert!(
        !host.admin_enabled(),
        "管理通道**默认关**（裁决 G）⇒ 默认行为一字不变 ⇒ 零回归风险"
    );
    let events = admin(&mut host, AdminOp::DrainLogs);
    assert_eq!(
        events,
        vec![disabled_reply()],
        "默认回应**逐字不变**（X-3）：码与消息都一个字不许动"
    );
}

#[test]
fn every_admin_op_answers_the_same_unchanged_reply_while_the_channel_is_off() {
    let mut host = KernelHost::new();
    let ops = [
        AdminOp::DrainLogs,
        AdminOp::KillGuest,
        AdminOp::PersistProject,
        AdminOp::Custom("diag".to_string()),
        AdminOp::Custom("别的 op".to_string()),
    ];
    for op in ops {
        let events = admin(&mut host, op.clone());
        assert_eq!(
            events,
            vec![disabled_reply()],
            "关闭时**每一个** op 的回应都**一字不变**：{op:?}"
        );
    }
}

#[test]
fn the_disabled_channel_does_not_touch_the_log_ring() {
    let mut host = KernelHost::new();
    let before = host.log_buffer.borrow().len();
    let _ = admin(&mut host, AdminOp::DrainLogs);
    let after = host.log_buffer.borrow().len();
    assert_eq!(
        before, after,
        "默认路径**不碰**任何其它逻辑 ⇒ 默认行为零副作用"
    );
    assert_eq!(
        host.admin_drain_truncated_entries(),
        0,
        "默认路径不产生任何截断计数"
    );
}

// ---------------------------------------------------------------------------
// B-2 / B-3 · 开启后的日志导出
// ---------------------------------------------------------------------------

#[test]
fn the_enabled_channel_answers_drain_logs_with_exactly_one_logs_event() {
    let mut host = KernelHost::new().with_admin_channel(true);
    assert!(host.admin_enabled(), "开关**打开**了");
    let events = admin(&mut host, AdminOp::DrainLogs);
    // ⚠️ 「**恰好一个** `AdminLogs`（**不是** `E_CTX-05`）」（B-2）
    let entries = only_admin_logs(&events);
    assert_eq!(
        entries.len(),
        host.log_buffer.borrow().len(),
        "导出条数 = 环里现驻留的条数（没有凭空多出来的条目）"
    );
}

#[test]
fn the_drained_entries_match_the_log_ring_in_count_and_order() {
    let mut host = KernelHost::new().with_admin_channel(true);
    // ⚠️ 虚拟时间**各不相同** ⇒ 顺序这一条才有判别力（否则「全都一样」也
    // 能蒙对）。
    let levels = [
        LogLevel::Info,
        LogLevel::Warn,
        LogLevel::Debug,
        LogLevel::Error,
        LogLevel::Info,
    ];
    for (i, level) in levels.iter().enumerate() {
        host.log_buffer.borrow_mut().log(
            *level,
            "TEST-CTX",
            &format!("条目 {i}"),
            if i == 2 { "E_CTX-07" } else { "" },
            (i as u64 + 1) * 10,
            i as u64,
        );
    }
    let before = ring_snapshot(&host);
    let expected = expected_export(&before, ADMIN_DRAIN_MAX_ENTRIES);
    let events = admin(&mut host, AdminOp::DrainLogs);
    let entries = only_admin_logs(&events);

    assert_eq!(entries.len(), expected.len(), "条数与 M-21 环里一致（B-3）");
    for (dto, entry) in entries.iter().zip(expected.iter()) {
        assert_eq!(dto.time_virt_ps, entry.time_virt_ps, "虚拟时间逐条相同");
        assert_eq!(dto.time_host_ms, entry.time_host_ms, "宿主时间逐条相同");
        assert_eq!(dto.session_id, entry.session_id, "会话标签逐条相同");
        assert_eq!(dto.level, entry.level.as_str(), "级别取内核自己的拼写");
        assert_eq!(dto.ctx, entry.ctx, "ctx 逐条相同");
        assert_eq!(dto.msg, entry.msg, "消息逐条相同");
        assert_eq!(dto.truncated, entry.truncated, "截断标志逐条相同");
    }
    // ⚠️ 顺序判据**单独**写死（不靠上面的逐条比对蒙混过关）。
    for pair in entries.windows(2) {
        assert!(
            pair[0].time_virt_ps <= pair[1].time_virt_ps,
            "导出必须按**虚拟时间**升序（B-3）"
        );
    }
    // ⚠️ 转换语义：**空串 ⇒ None**，有码 ⇒ Some（裁决 I 的逐字镜像）。
    assert_eq!(
        entries[2].err_code,
        Some("E_CTX-07".to_string()),
        "有码的条目必须带出那个码"
    );
    assert_eq!(
        entries[0].err_code, None,
        "**空串 ⇒ None**（没有码 ≠ 空码）"
    );
}

// ---------------------------------------------------------------------------
// B-4 · 诊断包
// ---------------------------------------------------------------------------

#[test]
fn the_enabled_channel_answers_the_diag_op_with_exactly_one_diag_event() {
    let mut host = KernelHost::new().with_admin_channel(true);
    let events = admin(&mut host, AdminOp::Custom(ADMIN_DIAG_OP.to_string()));
    let diag = only_admin_diag(&events);
    // ⚠️ 四个字段**逐字镜像** M-22 的 `DiagnosticKit`（裁决 I · 不多不少）。
    let kit = host.export_diagnostic_kit(None);
    assert_eq!(diag.err_code, kit.err_code, "err_code 逐字镜像");
    assert_eq!(diag.snapshot_summary, kit.snapshot_summary, "摘要逐字镜像");
    assert_eq!(
        diag.input_seq_tail, kit.input_seq_tail,
        "输入序列尾部逐字镜像"
    );
    assert_eq!(
        diag.host_clock_available, kit.host_clock_available,
        "宿主时钟事实逐字镜像"
    );
    // ⚠️ **谁持有时钟谁回答**（v0.43 · D-135 · 裁决 C）⇒ ⇒ 宿主注入的那
    // 一个事实就是 `started_at.is_available()`（M-22 不自己去问时钟）。
    assert_eq!(
        diag.host_clock_available,
        host.started_at.is_available(),
        "宿主时钟可不可用由**宿主**回答"
    );
}

// ---------------------------------------------------------------------------
// B-5 / X-4 · 未实现的 op **不假装实现了**
// ---------------------------------------------------------------------------

#[test]
fn the_unimplemented_admin_ops_still_answer_e_ctx_05() {
    let mut host = KernelHost::new().with_admin_channel(true);
    let ops = [
        AdminOp::KillGuest,
        AdminOp::PersistProject,
        AdminOp::Custom("别的 op".to_string()),
        AdminOp::Custom(String::new()),
    ];
    for op in ops {
        let events = admin(&mut host, op.clone());
        assert_eq!(
            events,
            vec![disabled_reply()],
            "未实现的 op **不假装实现了**（B-5 / X-4）：{op:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// B-6 · 导出**有上界**，且截断**明确告知**（不是静默）
// ---------------------------------------------------------------------------

#[test]
fn the_drain_is_bounded_and_the_truncation_is_announced_not_silent() {
    let mut host = KernelHost::new().with_admin_channel(true);
    let overflow = 5usize;
    let total = ADMIN_DRAIN_MAX_ENTRIES + overflow;
    // ⚠️ 虚拟时间取**引擎当前时间**（真实宿主就是这么打戳的 ⇒ 环里每条
    // 条目的虚拟时间都 ≤ now_ps）⇒ ⇒ 截断说明那条（也是 now_ps）**排最新**
    // ⇒ ⇒ 「下一次导出看得见」这条成立。
    let now_ps = host.now_ps();
    for i in 0..total {
        host.log_buffer.borrow_mut().log(
            LogLevel::Info,
            "TEST-CTX",
            &format!("条目 {i}"),
            "",
            now_ps,
            i as u64,
        );
    }
    // ⚠️ **导出之前**的环快照（截断说明是**事后**写进环的 ⇒ 事后取快照就
    // 晚了一步 ⇒ 判据必须是「导出那一刻」的环）。
    let before = ring_snapshot(&host);
    let expected = expected_export(&before, ADMIN_DRAIN_MAX_ENTRIES);
    let events = admin(&mut host, AdminOp::DrainLogs);
    let entries = only_admin_logs(&events);

    // ① **有上界**（B-6）：一次导出**不是**整个环。
    assert_eq!(
        entries.len(),
        ADMIN_DRAIN_MAX_ENTRIES,
        "一次导出的条目数**有上界**（B-6）"
    );
    // ② **条数与顺序仍与环里一致**（B-3 在截断时也成立）⇒ ⇒ `entries` 里
    //    **没有**合成条目（每一条都对应环里真实存在的一条）。
    assert_eq!(entries.len(), expected.len(), "截断时条数仍与环里一致");
    for (dto, entry) in entries.iter().zip(expected.iter()) {
        assert_eq!(
            dto.time_virt_ps, entry.time_virt_ps,
            "逐条对应环里的真实条目"
        );
        assert_eq!(dto.msg, entry.msg, "逐条对应环里的真实条目");
    }
    // ③ **明确告知被截断**（不是静默）：M-21 自己的环里**多了一条**说明。
    let ring = host.log_buffer.borrow().query(&LogQuery::default());
    let notices: Vec<_> = ring
        .iter()
        .filter(|entry| entry.ctx == ADMIN_DRAIN_LOG_CTX)
        .collect();
    assert_eq!(
        notices.len(),
        1,
        "截断必须在 M-21 的环里**留一条说明**（**不是静默**）"
    );
    assert_eq!(notices[0].level, LogLevel::Warn, "截断说明的级别是 WARN");
    assert!(
        notices[0].msg.contains("截断"),
        "截断说明必须说清楚「被截断」：{}",
        notices[0].msg
    );
    assert!(
        !notices[0].truncated,
        "截断说明那条自己**不该**被截断（否则读不出完整）：{}",
        notices[0].msg
    );
    assert_eq!(
        notices[0].err_code, "",
        "截断说明**不发明**新码（没有码就是没有码）"
    );
    // ④ **机器可读**的计数也留着（截断这件事**两处**都看得见）。
    assert_eq!(
        host.admin_drain_truncated_entries(),
        overflow as u64,
        "被截掉的条数**看得见**（不是静默）"
    );
}

#[test]
fn the_truncation_notice_shows_up_in_the_next_drain() {
    let mut host = KernelHost::new().with_admin_channel(true);
    let now_ps = host.now_ps();
    for i in 0..(ADMIN_DRAIN_MAX_ENTRIES + 3) {
        host.log_buffer.borrow_mut().log(
            LogLevel::Info,
            "TEST-CTX",
            &format!("条目 {i}"),
            "",
            now_ps,
            i as u64,
        );
    }
    let _ = admin(&mut host, AdminOp::DrainLogs);
    // ⚠️ 下一次导出**看得见**那条说明 ⇒ ⇒ 「不是静默」这条对**用户**成立
    // （不只是在宿主内部有个计数）。
    let events = admin(&mut host, AdminOp::DrainLogs);
    let entries = only_admin_logs(&events);
    assert!(
        entries
            .iter()
            .any(|entry| entry.ctx == ADMIN_DRAIN_LOG_CTX && entry.msg.contains("截断")),
        "下一次导出必须**看得见**上一次截断的说明"
    );
}

// ---------------------------------------------------------------------------
// §3.3 · 字面量收口（一个约束一个落点）
// ---------------------------------------------------------------------------

#[test]
fn the_disabled_code_is_rendered_from_the_contract_constants() {
    // ⚠️ 契约层**只**给 `u16`（**不**给第二份字符串常量）⇒ ⇒ 宿主必须
    // **自己**把族名 + 场景号渲染成码 ⇒ ⇒ 渲染结果与旧字面量**逐字相同**。
    let host = KernelHost::new();
    let event = host.admin_channel_disabled_error();
    match event {
        EventPayload::Error { code, message } => {
            assert_eq!(
                code, "E_CTX-05",
                "码必须逐字渲染成 E_CTX-05（族名 + 两位场景号）"
            );
            assert_eq!(
                message, "管理通道未启用（admin op 不在当前会话的能力面内）",
                "消息一字不变"
            );
        }
        other => panic!("应当是 Error：{other:?}"),
    }
}
