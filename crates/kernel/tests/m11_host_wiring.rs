//! Lane D —— **契约 v3 调试面（M-11）**的端到端接线验证。
//!
//! 冻结规格：`docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`
//! （§3.1~§3.4 的契约形状 · §5.2 的验收 #10~#18 · §9 的接线细则）。
//!
//! ## 为什么还要一份端到端
//!
//! `crates/kernel/src/kernel_host/tests_m11.rs` 是**单元**层（能看内部
//! 状态）。本文件走的是**生产路径**：契约信封 → `handle_message` /
//! `handle_json` → 契约事件 ⇒ 证明「接缝被接上」，而不只是「单元能过」。
//!
//! ## 这些用例在防什么
//!
//! - 四个占位分支**真的**被替换了（不是还回 `E_CTX-03`）；
//! - 主题常量与前端 `EVENT_TOPIC` **逐字相同**（两边各改各的 ⇒ 两边
//!   各自的测试会红）；
//! - **D-50 / D-51** 的两条闭环在生产路径上成立。

use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion, TraceFormat,
    TraceSource,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message, Payload};
use riscv_sim_kernel::kernel_host::{
    KernelHost, TOPIC_DISASM, TOPIC_DUMP, TOPIC_EXPORT, TOPIC_SOURCE, TOPIC_SYMBOL, TOPIC_TRACE,
};

/// demo 固件 —— **引用共享常量**，不抄一份。
use riscv_sim_kernel::kernel_host::DEMO_FIRMWARE_BYTES as DEMO_BYTES;

/// 造一个挂着 demo 固件的工程。
fn demo_project() -> Project {
    Project {
        id: "proj-m11-e2e".to_string(),
        name: "m11-e2e".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "demo-firmware.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(DEMO_BYTES.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

/// 派发一条命令，返回它产生的事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

// ---------------------------------------------------------------------------
// 命令 → 事件（生产路径）
// ---------------------------------------------------------------------------

#[test]
fn the_four_debug_commands_are_wired_not_placeholders() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // ① `Disasm` ⇒ 一条 `Disasm` 事件（**不是** `E_CTX-03`）。
    let events = dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 2 });
    assert_eq!(events.len(), 1);
    let EventPayload::Disasm { entries } = &events[0] else {
        panic!("`Disasm` 必须回 `Disasm` 事件：{events:?}");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].addr, 0);
    assert_eq!(entries[0].width, 4);

    // ② `ResolveLine` ⇒ 一条 `SourceLocation` 事件。
    let events = dispatch(&mut host, CommandPayload::ResolveLine { addr: 0 });
    assert_eq!(events.len(), 1);
    let EventPayload::SourceLocation { addr, file, line } = &events[0] else {
        panic!("`ResolveLine` 必须回 `SourceLocation` 事件：{events:?}");
    };
    assert_eq!(*addr, 0);
    assert!(file.is_empty());
    assert_eq!(*line, 0);

    // ③ `ExportTrace` ⇒ 一条 `TraceExport` 事件。
    let events = dispatch(
        &mut host,
        CommandPayload::ExportTrace {
            source: TraceSource::Execution,
            format: TraceFormat::Csv,
            offset: 0,
            limit: 0,
        },
    );
    assert_eq!(events.len(), 1);
    let EventPayload::TraceExport {
        source,
        format,
        offset,
        next_offset,
        total,
        bytes,
        chunk,
        overflow_marked,
    } = &events[0]
    else {
        panic!("`ExportTrace` 必须回 `TraceExport` 事件：{events:?}");
    };
    assert_eq!(*source, TraceSource::Execution);
    assert_eq!(*format, TraceFormat::Csv);
    assert_eq!(*offset, 0);
    assert_eq!(*next_offset, *total);
    assert_eq!(*bytes, chunk.len() as u64, "bytes == 块的长度");
    assert!(!*overflow_marked, "没有溢出 ⇒ overflow_marked 是 false");

    // ④ `SetTraceEnabled` ⇒ 一条 `TraceState` 事件。
    let events = dispatch(&mut host, CommandPayload::SetTraceEnabled { enabled: true });
    assert_eq!(events.len(), 1);
    let EventPayload::TraceState {
        enabled,
        len,
        dropped,
    } = &events[0]
    else {
        panic!("`SetTraceEnabled` 必须回 `TraceState` 事件：{events:?}");
    };
    assert!(*enabled);
    assert_eq!(*len as usize, host.trace_len());
    assert_eq!(*dropped, host.trace_dropped());
}

#[test]
fn the_json_bridge_carries_the_new_commands() {
    // ⚠️ **生产路径**是 WASM 桥（`handle_json`）——前端拿到的是 JSON，
    // 不是 Rust 的枚举。走一遍 JSON 才证明「契约的形状真的到了前端」。
    let mut host = KernelHost::new();
    let load = serde_json::to_string(&Message::command(
        1,
        CommandPayload::LoadProject(demo_project()),
    ))
    .expect("json");
    let _ = host.handle_json(&load);

    let request = serde_json::to_string(&Message::command(
        2,
        CommandPayload::Disasm { addr: 0, count: 2 },
    ))
    .expect("json");
    let reply = host.handle_json(&request);
    let messages: Vec<Message> = serde_json::from_str(&reply).expect("回包必须是 JSON 数组");
    assert_eq!(messages.len(), 1);
    let Payload::Event(EventPayload::Disasm { entries }) = &messages[0].payload else {
        panic!("JSON 桥必须送出 `Disasm` 事件：{reply}");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].addr, 0);
    assert_eq!(entries[0].code, 0x02A0_0093);
}

// ---------------------------------------------------------------------------
// D-50 / D-51 的闭环（生产路径）
// ---------------------------------------------------------------------------

#[test]
fn the_m18_trace_ring_has_a_consumer_on_the_production_path() {
    // **D-51**：M-18 的访存 trace 环在宿主里**有消费者**（不再只写不读）。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(&mut host, CommandPayload::StepN { n: 4 });

    assert_eq!(
        host.probe_trace_len(),
        0,
        "跑完一批 ⇒ M-18 的环被转移走（否则它会常驻 3~5 MB）"
    );
    assert!(host.trace_len() > 0, "历史进了 M-11 的环");
}

#[test]
fn the_waveform_export_reads_the_m19_ring_on_the_production_path() {
    // **D-50**：波形导出的数据源是 M-19 的环 ⇒ M-19 的环**有读者**。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(&mut host, CommandPayload::StepN { n: 8 });

    let events = dispatch(
        &mut host,
        CommandPayload::ExportTrace {
            source: TraceSource::Waveform,
            format: TraceFormat::Csv,
            offset: 0,
            limit: 0,
        },
    );
    let EventPayload::TraceExport { chunk, total, .. } = &events[0] else {
        panic!("{events:?}");
    };
    assert!(*total > 0, "M-19 的环里有边沿");
    assert!(
        chunk.contains("GPIO0.OUT"),
        "波形导出必须来自 M-19 的环：{chunk}"
    );
}

// ---------------------------------------------------------------------------
// 主题表一致（STAGE-5 §3.5 / 验收 #22）
// ---------------------------------------------------------------------------

#[test]
fn the_rust_topic_constants_match_the_frontend_topic_table() {
    // ⚠️ Rust 侧**没有**集中的「变体 → topic」映射表（唯一真相在前端
    // `front/lib/message.js` 的 `EVENT_TOPIC`）⇒ 两侧的一致性靠「同一个
    // 字面量 + 各自被测试钉住」保证。这里直接读那份 JS 比对。
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../front/lib/message.js");
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读不到 {path}：{e}"));

    for (rust_topic, variant) in [
        (TOPIC_DISASM, "Disasm"),
        (TOPIC_SOURCE, "SourceLocation"),
        (TOPIC_EXPORT, "TraceExport"),
        (TOPIC_TRACE, "TraceState"),
        // ⚠️ **v0.57 批（STAGE-42 §2.7）**：两条新回复事件的 topic 与既有
        //   四个**同族**（`debug.*`）⇒ ⇒ 前端镜像的 `EVENT_TOPIC` 表**加两
        //   行** ⇒ ⇒ 两侧逐字相同这条纪律**一样**要守住。
        (TOPIC_SYMBOL, "SymbolResolved"),
        (TOPIC_DUMP, "MemoryDump"),
    ] {
        let needle = format!("{variant}: \"{rust_topic}\"");
        assert!(
            text.contains(&needle),
            "前端 `EVENT_TOPIC` 里没有 `{needle}` —— 两侧的主题必须逐字相同"
        );
    }
}
