//! 契约 v2（`schema_version = 2`）的**内核接线**用例。
//!
//! 冻结规格：`docs/detailed-design/STAGE-2_Contract_V2.md`
//!
//! - §5 —— 三个新命令（`ReadRegisters` / `WriteRegister` / `Negotiate`）
//!   的**内核接线语义**；
//! - §4 —— 新命令 / 新事件的**形状**（契约层已冻结，这里只验内核侧的
//!   生产与消费是否对得上）。
//!
//! ## 判别力纪律（本仓库的硬规矩）
//!
//! 每条用例的断言都落在**被依赖方自己身上**，而不是「事件发出去了」：
//!
//! - 读寄存器 → 断言**引擎里的寄存器表**（M-17 → M-12 那份 `SimState`）
//!   里就是那个值；
//! - 写寄存器 → 断言**引擎里的寄存器表**变了，而且**下一次执行会把它
//!   覆盖掉**（证明 ISS 执行的就是那份状态，而不是某个影子副本）；
//! - 拒绝写 → 断言**整个寄存器表逐字节没变**。
//!
//! ⚠️ 断言「事件流里有 `Registers`」是**不够**的：宿主自己维护一份影子
//! 寄存器表也能让那种断言变绿，而引擎里的值一动不动——那正是调试器
//! 「改了寄存器但程序不认」的经典故障。

use super::*;

use riscv_sim_contract::model::{ProductTree, ProjectVersion, SimState};

/// demo 固件 —— **引用共享常量**，不抄一份。
///
/// 权威副本在 `kernel_host::DEMO_FIRMWARE_BYTES`（Rust 侧唯一一份）。
use super::DEMO_FIRMWARE_BYTES as DEMO_BYTES;

/// demo 固件里 `addi x1, x0, 42` 之后的寄存器值。
///
/// demo 固件的前三条指令是：
///
/// ```text
/// 0x00  addi x1, x0, 42
/// 0x04  addi x2, x0, 7
/// 0x08  add  x3, x1, x2      ← x3 = 49
/// ```
const DEMO_X1: u32 = 42;
const DEMO_X2: u32 = 7;
const DEMO_X3: u32 = 49;

/// demo 固件写的 GPIO 通道 id（M-18 的 `"{source}.{signal}"`）。
const GPIO_STREAM_ID: &str = "GPIO0.OUT";

/// 造一个挂着 demo 固件的工程。
fn demo_project() -> Project {
    Project {
        id: "proj-c2".to_string(),
        name: "c2".to_string(),
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

/// 取第一条 `Registers` 事件（`(index, values)`）。
fn first_registers(events: &[EventPayload]) -> (u32, Vec<u32>) {
    for e in events {
        if let EventPayload::Registers { index, values } = e {
            return (*index, values.clone());
        }
    }
    panic!("没有 Registers 事件：{events:?}");
}

/// 取第一条 `Negotiated` 事件（`(schema_version, cap_bits, kernel)`）。
fn first_negotiated(events: &[EventPayload]) -> (u32, u64, String) {
    for e in events {
        if let EventPayload::Negotiated {
            schema_version,
            cap_bits,
            kernel,
        } = e
        {
            return (*schema_version, *cap_bits, kernel.clone());
        }
    }
    panic!("没有 Negotiated 事件：{events:?}");
}

/// 取第一条 `Error` 事件的错误码。
fn first_error_code(events: &[EventPayload]) -> String {
    for e in events {
        if let EventPayload::Error { code, .. } = e {
            return code.clone();
        }
    }
    panic!("没有 Error 事件：{events:?}");
}

/// 装载 demo 固件并跑完前 `n` 条指令的宿主。
fn host_after_steps(n: u32) -> KernelHost {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(&mut host, CommandPayload::StepN { n });
    host
}

// -- ReadRegisters ---------------------------------------------------------

#[test]
fn read_registers_returns_the_real_register_values() {
    let mut host = host_after_steps(3);

    let events = dispatch(
        &mut host,
        CommandPayload::ReadRegisters { index: 1, count: 3 },
    );
    let (index, values) = first_registers(&events);
    assert_eq!(index, 1, "回复里的 index 是实际起点");
    assert_eq!(
        values,
        vec![DEMO_X1, DEMO_X2, DEMO_X3],
        "读出来的是**跑过固件之后**的真实寄存器值"
    );

    // 判别力：断言落在**引擎自己的寄存器表**上——宿主自维护影子副本的
    // 实现（事件能对上、引擎没动）在这里会红。
    let gpr = host.backend.register_file();
    assert_eq!(gpr[1], DEMO_X1, "引擎里的 x1 必须是 42");
    assert_eq!(gpr[2], DEMO_X2, "引擎里的 x2 必须是 7");
    assert_eq!(gpr[3], DEMO_X3, "引擎里的 x3 必须是 49");
}

#[test]
fn read_registers_clamps_an_out_of_range_index() {
    let mut host = host_after_steps(1);

    // index 越界 → 钳到 x31（**实际**起点要出现在回复里）。
    let events = dispatch(
        &mut host,
        CommandPayload::ReadRegisters {
            index: 100,
            count: 1,
        },
    );
    let (index, values) = first_registers(&events);
    assert_eq!(index, 31, "index 必须被钳到寄存器表末尾（x31）");
    assert_eq!(values.len(), 1, "实际读到的条数");
    assert_eq!(values[0], host.backend.register_file()[31]);

    // 恰好越界一个（32）也一样钳到 31。
    let events = dispatch(
        &mut host,
        CommandPayload::ReadRegisters {
            index: 32,
            count: 1,
        },
    );
    let (index, _) = first_registers(&events);
    assert_eq!(index, 31, "index = 32 也钳到 x31");
}

#[test]
fn read_registers_clamps_the_count_to_the_end_of_the_file() {
    let mut host = host_after_steps(1);

    // 从 x30 起要 100 个 → 只剩 2 个（x30 / x31）。
    let events = dispatch(
        &mut host,
        CommandPayload::ReadRegisters {
            index: 30,
            count: 100,
        },
    );
    let (index, values) = first_registers(&events);
    assert_eq!(index, 30, "起点不变");
    assert_eq!(
        values.len(),
        2,
        "count 必须被钳到寄存器表末尾——多读会越界或读出别的寄存器"
    );

    // 整表：count = 32 → 32 条。
    let events = dispatch(
        &mut host,
        CommandPayload::ReadRegisters {
            index: 0,
            count: 32,
        },
    );
    let (_, values) = first_registers(&events);
    assert_eq!(values.len(), 32, "整表正好 32 条");
}

#[test]
fn read_registers_with_zero_count_replies_an_empty_range() {
    let mut host = host_after_steps(1);

    let events = dispatch(
        &mut host,
        CommandPayload::ReadRegisters { index: 0, count: 0 },
    );
    // ⚠️ **回复必须存在**：M-05 按 `reqId` 配对请求与回复——「没有回复」
    // 与「消息丢失」在协议上**不可区分**。零回复会被前端当成传输故障，
    // 500 ms 后以 `E_CTX-06`（命令超时）拒绝：用户看到的是「内核没应答」，
    // 而不是「读到了空区间」。
    //
    // `first_registers` 在**没有** `Registers` 事件时会 panic —— 这就是
    // 「回复存在」的断言。
    let (index, values) = first_registers(&events);
    assert_eq!(index, 0, "回复里的 index 是钳位后的实际起点");
    assert!(
        values.is_empty(),
        "count = 0 → 空区间（长度 0），不是「不回复」：{values:?}"
    );

    // index 越界时，回复的**起点**仍是钳位后的值。
    let events = dispatch(
        &mut host,
        CommandPayload::ReadRegisters {
            index: 100,
            count: 0,
        },
    );
    let (index, values) = first_registers(&events);
    assert_eq!(index, 31, "index 越界时，回复的起点是钳位后的值");
    assert!(values.is_empty(), "count = 0 → 空区间：{values:?}");
}

// -- WriteRegister ---------------------------------------------------------

#[test]
fn write_register_lands_in_the_engine_state() {
    let mut host = host_after_steps(1);

    let events = dispatch(
        &mut host,
        CommandPayload::WriteRegister {
            index: 5,
            value: 0x1234,
        },
    );
    let (index, values) = first_registers(&events);
    assert_eq!(index, 5);
    assert_eq!(values, vec![0x1234], "回复携带写进去的值");

    // 判别力：值落在**引擎的寄存器表**里。
    assert_eq!(
        host.backend.register_file()[5],
        0x1234,
        "写进去的值必须落在引擎状态里，而不是宿主自己的影子副本"
    );

    // 再读回来（走的是引擎那份状态）。
    let events = dispatch(
        &mut host,
        CommandPayload::ReadRegisters { index: 5, count: 1 },
    );
    let (_, values) = first_registers(&events);
    assert_eq!(values, vec![0x1234], "读回来的值必须是写进去的那个");
}

#[test]
fn a_written_register_is_visible_to_the_next_execution() {
    // 跑前两条：`addi x1, x0, 42` + `addi x2, x0, 7` → x1 = 42、x2 = 7。
    let mut host = host_after_steps(2);
    assert_eq!(host.backend.register_file()[2], DEMO_X2);

    // 把 x1 从 42 改成 5。
    dispatch(
        &mut host,
        CommandPayload::WriteRegister { index: 1, value: 5 },
    );
    assert_eq!(host.backend.register_file()[1], 5);

    // 跑 demo 固件的第 3 条（`add x3, x1, x2`）。x2 已经是 7，所以
    // x3 必须是 **5 + 7 = 12**——而不是固件原本的 42 + 7 = 49。
    //
    // ⚠️ 这条断言钉的是「**ISS 执行的就是引擎里那份寄存器表**」：
    // 如果写只落到了某个影子副本上（或者 ISS 读的是另一份），x3 会是 49
    // 或者压根算不出来——而「事件发出去了」那种断言是抓不到的。
    host.backend.set_pc(0x08);
    dispatch(&mut host, CommandPayload::StepN { n: 1 });
    assert_eq!(
        host.backend.register_file()[3],
        12,
        "ISS 必须用**写进去的那个 x1** 参与运算（5 + 7 = 12）"
    );
}

#[test]
fn write_register_to_x0_is_discarded_but_still_replies() {
    let mut host = host_after_steps(1);

    let events = dispatch(
        &mut host,
        CommandPayload::WriteRegister {
            index: 0,
            value: 0x1234,
        },
    );
    // 仍然要回复（调用方不该把它当成一次失败）……
    let (index, values) = first_registers(&events);
    assert_eq!(index, 0);
    // ……但值恒为 0（D-04：x0 恒等于零）。
    assert_eq!(values, vec![0], "写 x0 被丢弃，回复的值恒为 0");

    // x0 本身也必须还是 0。
    assert_eq!(host.backend.register_file()[0], 0, "x0 恒等于零");
    // ⚠️ **原始槽位**也要没被碰过：`register_file()` 会把 x0 归零（防御
    // 一份脏快照），只看它**看不出**「写 x0 真的落进了原始槽位」——
    // 判别力必须落在原始状态上。
    assert_eq!(
        host.backend.engine().snapshot().gpr[0],
        0,
        "写 x0 必须被丢弃——连原始槽位都不能碰"
    );
    let events = dispatch(
        &mut host,
        CommandPayload::ReadRegisters { index: 0, count: 1 },
    );
    let (_, values) = first_registers(&events);
    assert_eq!(values, vec![0], "读 x0 恒为 0");
}

#[test]
fn write_register_with_an_out_of_range_index_is_rejected() {
    let mut host = host_after_steps(1);

    // 先写一个合法的值，让「寄存器表变了」这件事**可观测**。
    dispatch(
        &mut host,
        CommandPayload::WriteRegister {
            index: 7,
            value: 0xAAAA,
        },
    );
    let before = host.backend.register_file();
    assert_eq!(before[7], 0xAAAA);

    // 越界写 → 拒绝（**不是**钳位：钳位会静默写到别的寄存器）。
    let events = dispatch(
        &mut host,
        CommandPayload::WriteRegister {
            index: 32,
            value: 0xBBBB,
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_DBG-01",
        "越界写必须报 E_DBG-01：{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Registers { .. })),
        "被拒绝的写不产生 Registers 回复：{events:?}"
    );

    // 寄存器表**原样不动**。
    let after = host.backend.register_file();
    assert_eq!(after, before, "被拒绝的写不能碰寄存器文件——哪怕一个位");
    assert!(
        !after.contains(&0xBBBB),
        "被拒绝的值不能落到**任何一个**寄存器上"
    );
}

// -- Negotiate -------------------------------------------------------------

#[test]
fn negotiate_reports_the_current_schema_version() {
    let mut host = KernelHost::new();

    let events = dispatch(
        &mut host,
        CommandPayload::Negotiate {
            min_schema_version: 2,
            cap_bits: 0b111,
        },
    );
    let (schema_version, cap_bits, kernel) = first_negotiated(&events);
    assert_eq!(
        schema_version, 2,
        "协商结果必须是当前契约版本（schema_version = 2）"
    );
    assert_eq!(
        cap_bits, KERNEL_CAP_BITS,
        "授予的能力位必须是内核实际授予的那一组"
    );
    assert_ne!(cap_bits, 0, "能力位不能是空的（空 = 前端按位降级）");
    assert!(kernel.contains("RV32IMC"), "内核描述串必须带 ISA：{kernel}");
    assert!(
        kernel.starts_with("riscv_sim_kernel/"),
        "内核描述串必须是「crate/version ISA」形：{kernel}"
    );
}

#[test]
fn negotiate_rejects_a_minimum_version_above_the_supported_one() {
    let mut host = KernelHost::new();

    let events = dispatch(
        &mut host,
        CommandPayload::Negotiate {
            min_schema_version: 3,
            cap_bits: 0,
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PROTO-01",
        "宿主要求的最低版本高于内核支持的版本 → 必须拒绝：{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Negotiated { .. })),
        "被拒绝的协商不产生 Negotiated 回复：{events:?}"
    );
}

#[test]
fn reading_x0_always_yields_zero_even_after_a_corrupted_restore() {
    let mut host = KernelHost::new();

    // 一份 x0 被污染的快照（`Restore` 允许带进脏 x0）。
    let mut dirty = SimState::default();
    dirty.gpr[0] = 0xDEAD_BEEF;
    dirty.gpr[1] = 7;
    dispatch(
        &mut host,
        CommandPayload::Restore {
            state: Box::new(dirty),
        },
    );

    // 原始槽位确实是脏的（证明这条用例**真的**在测「读路径归零」，
    // 而不是「反正 x0 本来就是 0」）。
    assert_eq!(
        host.backend.engine().snapshot().gpr[0],
        0xDEAD_BEEF,
        "夹具前提：快照确实把脏值带进了原始槽位"
    );

    // 但**读** x0 恒为 0（D-04）：x0 在体系结构上恒等于零。
    let events = dispatch(
        &mut host,
        CommandPayload::ReadRegisters { index: 0, count: 2 },
    );
    let (_, values) = first_registers(&events);
    assert_eq!(
        values,
        vec![0, 7],
        "读路径必须把 x0 归零，其余寄存器照常读出"
    );
}

// -- 改名适配 ---------------------------------------------------------------

#[test]
fn waveform_edges_arrive_as_the_renamed_waveform_history_event() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_WAVEFORM.to_string(),
        },
    );

    // demo 固件的第 8 条（`sw x6, 4(x5)`）写 GPIO0.OUT → 产生边沿。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 8 });

    // 契约 v2 之后，波形事件的变体名是 `WaveformHistory`（history 形），
    // 不再是 `WaveformDelta`（那是 live 批次形的 DTO，不是事件变体）。
    let wave = events
        .iter()
        .find_map(|e| match e {
            EventPayload::WaveformHistory(w) => Some(w.clone()),
            _ => None,
        })
        .expect("跑过 GPIO 写入后必须有一条 WaveformHistory 事件");

    let gpio = wave
        .streams
        .iter()
        .find(|s| s.id == GPIO_STREAM_ID)
        .expect("波形里必须有 GPIO0.OUT 通道");
    assert_eq!(
        gpio.edges.len(),
        1,
        "GPIO 写一次就该有一条边沿：{:?}",
        gpio.edges
    );
    assert_eq!(gpio.edges[0].value, 1, "边沿的值是固件写进 OUT 的那个 1");
}

// -- Continue（D-57 的端口文档订正 · 命令层的那根钉子）-----------------

/// `Continue` 命令跑的是**本轮指令预算**，不是「下一个量子边界」。
///
/// D-57 把契约的端口文档从「跑到下一个**量子边界**」订正为「跑到本轮
/// **指令预算**耗尽」。⚠️ 判别式必须落在**行为**上，否则「改了文档但
/// 没改行为」与「没改文档」同样无法分辨：
///
/// - 量子配额是 1 000 条，指令预算是 10 000 条 —— 两者差 **10 倍**。
///   断言「正好 10 000 条」就能把「预算耗尽」与「停在量子边界」分开；
/// - 同时断言它是量子配额的**整数倍**（量子化调度）。
///
/// ⚠️ 这条用例守的是**命令**这一层（M-05 → kernel_host → M-17）；
/// 端口方法那一层由 `m17_backend::tests::
/// continue_until_event_runs_the_quantum_main_loop` 守着。
/// **两者是不同的表面**，各要有各的钉子 —— 只钉一处，另一处照样能漂。
#[test]
fn the_continue_command_runs_the_budget_not_one_quantum() {
    let mut host = host_after_steps(0);
    let before = host.backend.total_executed();
    assert_eq!(before, 0, "夹具：Continue 之前一条指令都没跑");

    let events = dispatch(&mut host, CommandPayload::Continue);
    let executed = host.backend.total_executed() - before;

    assert_eq!(
        executed,
        u64::from(crate::m12_engine::DEFAULT_INSTRUCTION_BUDGET),
        "没有断点 / 陷阱时，Continue 必须跑满**指令预算**（10 000 条）—— \
         停在量子边界是 1 000 条，两者差 10 倍"
    );
    // ⚠️ 上面那条断言是**自指**的（用的就是实现自己用的那个常量），
    // 所以它**单独**钉不住「预算 vs 量子边界」——把预算常量改成 1 000，
    // 它照样绿。判别式必须落在**量子配额**上（一个与预算无关的量）：
    // 「跑过**不止一个量子**」。D-73。
    assert!(
        executed > u64::from(crate::m12_engine::QUANTUM_INSTR_QUOTA),
        "Continue 必须跑过**不止一个量子**（量子配额 {} 条）—— \
         停在量子边界是恰好一个量子，跑满指令预算才是多个量子",
        crate::m12_engine::QUANTUM_INSTR_QUOTA
    );
    assert_eq!(
        executed % u64::from(crate::m12_engine::QUANTUM_INSTR_QUOTA),
        0,
        "一轮的执行条数必须是量子配额的整数倍（量子化调度）"
    );
    assert!(
        !events.is_empty(),
        "Continue 至少要回一个事件（Tick / Halted）"
    );
}
