//! 端到端的**会话级快照**用例（STAGE-3 §6 第 9 项）。
//!
//! 验收项 9：「组装出来的载荷能还原出一个**等价会话**」——跑 N 步 →
//! 快照 → 还原 → 再跑 M 步，与「不打断」跑 M 步的**最终状态逐字节相同**。
//!
//! ## 两层判别式
//!
//! - **层 2（主判别式 · 规格要求）**：Host A（capture → restore → 跑 M 步）
//!   vs Host B（不打断跑 M 步），**终态 7 项逐项相同**。它抓「某段状态
//!   整体丢失」。
//! - **层 1（补充判别式）**：同一 host 上 capture → **制造漂移** →
//!   restore → 再 capture → 两个载荷**六段逐段相等**。它抓「存了、却没
//!   恢复」——那正是层 2 容易因为巧合而放过的情况。
//!
//! ⚠️ **层 1 必须先制造漂移**：不漂移的话，「恢复了」与「根本没动过」
//! **不可区分**——一段既没被存、也没被恢复的状态，两次采集会**同样地**
//! 缺它，用例照样绿，而状态确实丢了。
//!
//! ⚠️ **不许用「非空」「长度 > 0」「没 panic」当断言**：那抓不到「某一段
//! 没进快照」这类缺陷。终态 7 项每一项都是**具体值**比对。

use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion, WriteSize,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};
use riscv_sim_contract::session::{
    CheckpointMetaDto, M15PeriphDto, M16IntcDto, SESSION_SNAPSHOT_SCHEMA_VERSION,
};
use riscv_sim_contract::snapshot::QueuedEventDto;

use riscv_sim_kernel::kernel_host::{
    KernelHost, DEFAULT_GPIO_BASE, DEFAULT_RAM_HIGH_BASE, DEFAULT_RAM_HIGH_SIZE,
    DEFAULT_RAM_LOW_BASE, DEFAULT_RAM_LOW_SIZE, DEMO_FIRMWARE_BYTES, GPIO_PERIPH_ID,
};

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 第一段跑多少条指令。
///
/// demo 固件的布局：
///
/// ```text
/// 0x00 addi x1, x0, 42   0x04 addi x2, x0, 7   0x08 add x3, x1, x2
/// 0x0c sw   x3, 0x100(x0)  0x10 jal x0, 0x14
/// 0x14 lui  x5, 0x40000  0x18 addi x6, x0, 1  0x1c sw x6, 4(x5)
/// 0x20 addi x4, x0, 1   0x24 jal x0, 0
/// ```
///
/// 跑 **8** 条 = 走到 `0x1c`（GPIO 那一写）**之后**、停在 `0x20`。
const N_STEPS: u32 = 8;

/// 第二段跑多少条指令（`0x20` 的 `addi` + 两圈自环）。
const M_STEPS: u32 = 3;

/// demo 固件写进内存的目标地址（`sw x3, 0x100(x0)`）。
const DEMO_STORE_ADDR: u32 = 0x100;

/// demo 固件写进内存的值（42 + 7）。
const DEMO_STORE_VALUE: u32 = 49;

/// 一条指令的虚拟时间成本（皮秒）—— M-12 的换算（1 µs 量子 ÷ 1000 条）。
///
/// ⚠️ **这条常量是「自指」的**（V-B 检视 · D-1）：它引用的是**实现自己**
/// 的 `INSTRUCTION_COST_PS`。所以**本文件的用例察觉不到「指令成本变了」**
/// ——实测把成本从 1000 ps 改成 2000 ps，本批的端到端 / 宿主用例**一条都不红**
/// （只有老批次 D-07 的 `the_quantum_is_one_microsecond_and_the_quota_is_a_thousand_instructions`
/// 会红）。
///
/// ⚠️ **这不影响本文件的判别力**：它要抓的是「某段状态没进快照 / 没被还原」，
/// 判别式落在**关系**（「(N+M) 个指令成本」）上，**不是**落在绝对值上。
/// **绝对值由 D-07 的用例守着**——两个批次**分工明确**。
/// ⚠️ **不要**把它改成字面量：那样这条夹具就与实现**完全脱钩**，
/// 实现改了成本夹具还在绿——**换一种形式的自指**。
const INSTRUCTION_COST_PS: u64 = riscv_sim_kernel::m12_engine::INSTRUCTION_COST_PS;

/// 造一个挂着 demo 固件的工程。
///
/// ⚠️ **固件字节引用共享常量**（`kernel_host::DEMO_FIRMWARE_BYTES`）——
/// Rust 侧只有那一份，抄一份就是「两份副本各自有测试」的漂移形态。
fn demo_project() -> Project {
    Project {
        id: "proj-demo".to_string(),
        name: "demo".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "demo-firmware.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(DEMO_FIRMWARE_BYTES.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

/// 派发一条命令，返回它产生的事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 装载 demo 工程。
fn load_demo(host: &mut KernelHost) {
    let events = dispatch(host, CommandPayload::LoadProject(demo_project()));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装载 demo 工程必须成功：{events:?}"
    );
}

/// 跑恰好 n 条指令。
fn step(host: &mut KernelHost, n: u32) {
    let events = dispatch(host, CommandPayload::StepN { n });
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "步进 {n} 条必须成功：{events:?}"
    );
}

/// 造一个装好 demo 固件的宿主。
fn host_with_demo() -> KernelHost {
    let mut host = KernelHost::new();
    load_demo(&mut host);
    host
}

/// 走**时间驱动**路径（`Continue`）跑一轮 —— 跑到本轮指令预算耗尽。
fn run_until_budget(host: &mut KernelHost) {
    let events = dispatch(host, CommandPayload::Continue);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "时间驱动跑一轮必须成功：{events:?}"
    );
}

/// 检查点元数据（段 0）—— 调用方给的那一份。
fn meta(host: &KernelHost) -> CheckpointMetaDto {
    CheckpointMetaDto {
        id: 1,
        t_virt: host.now_ps(),
        t_host: 0,
        base_seq: 0,
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
    }
}

/// 往 GPIO 的 `OUT` 寄存器写一个值（制造**现场漂移**用）。
fn write_gpio_out(host: &mut KernelHost, value: u32) {
    let events = dispatch(
        host,
        CommandPayload::WriteMemory {
            addr: DEFAULT_GPIO_BASE + 4,
            size: WriteSize::Word,
            value,
        },
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "写 GPIO.OUT 必须成功：{events:?}"
    );
}

// ---------------------------------------------------------------------------
// 终态的取法
// ---------------------------------------------------------------------------

/// RAM 的一段：(基址, 逐字内容)。
#[derive(Debug)]
struct RamRegion {
    base: u32,
    words: Vec<u32>,
}

/// **终态** —— 层 2 要比对的 7 项，每项都取自**公开的读面**。
#[derive(Debug)]
struct FinalState {
    /// ① PC
    pc: u32,
    /// ② 32 个 GPR
    gpr: [u32; 32],
    /// ③ 虚拟时间
    now_ps: u64,
    /// ④ RAM 的全部内容（逐区域逐字节）
    ram: Vec<RamRegion>,
    /// ⑤ 外设状态（含外设自己的 `now_ps`）
    peripherals: M15PeriphDto,
    /// ⑥ 已拉起的 IRQ 行
    irq: M16IntcDto,
    /// ⑦ 事件队列（内容 + 顺序）
    queue: Vec<QueuedEventDto>,
    /// ⑦ 插入序计数器
    next_seq: u64,
    /// ⑦ 指令预算
    instruction_budget: u32,
}

/// 读出宿主当前的**终态**。
fn final_state(host: &KernelHost) -> FinalState {
    let engine = host.engine_snapshot();
    FinalState {
        pc: host.pc(),
        gpr: host.registers(),
        now_ps: host.now_ps(),
        ram: read_ram(host),
        peripherals: host.peripheral_states(),
        irq: host.intc_state(),
        queue: engine.queue,
        next_seq: engine.next_seq,
        instruction_budget: engine.instruction_budget,
    }
}

/// 读出**两块 RAM** 的全部内容（逐字）。
///
/// ⚠️ 走的是**公开的调试读面**（`read_memory`），不是「导出用的那条路」
/// ——两边的取数路径不同，才谈得上「终态相同」是个真结论。
fn read_ram(host: &KernelHost) -> Vec<RamRegion> {
    let mut regions = Vec::new();
    for (base, size) in [
        (DEFAULT_RAM_LOW_BASE, DEFAULT_RAM_LOW_SIZE),
        (DEFAULT_RAM_HIGH_BASE, DEFAULT_RAM_HIGH_SIZE),
    ] {
        let mut words = Vec::with_capacity((size / 4) as usize);
        let mut off = 0u32;
        while off < size {
            words.push(
                host.read_memory(base + off, WriteSize::Word)
                    .expect("读 RAM"),
            );
            off += 4;
        }
        regions.push(RamRegion { base, words });
    }
    regions
}

/// 终态 7 项**逐项**比对。
fn assert_same_final_state(a: &FinalState, b: &FinalState) {
    assert_eq!(a.pc, b.pc, "① PC 必须相同");
    assert_eq!(a.gpr, b.gpr, "② 32 个 GPR 必须逐个相同");
    assert_eq!(a.now_ps, b.now_ps, "③ 虚拟时间必须相同");
    assert_same_ram(&a.ram, &b.ram);
    assert_eq!(
        a.peripherals, b.peripherals,
        "⑤ 外设状态必须逐字段相同（含外设自己的 now_ps）"
    );
    assert_eq!(a.irq, b.irq, "⑥ 已拉起的 IRQ 行必须相同");
    assert_eq!(a.queue, b.queue, "⑦ 事件队列的内容与顺序必须相同");
    assert_eq!(a.next_seq, b.next_seq, "⑦ 插入序计数器必须相同");
    assert_eq!(
        a.instruction_budget, b.instruction_budget,
        "⑦ 指令预算必须相同"
    );
}

/// RAM 的**逐字节**比对（报出第一个不一样的地址）。
fn assert_same_ram(a: &[RamRegion], b: &[RamRegion]) {
    assert_eq!(
        a.len(),
        b.len(),
        "④ RAM 的区域数必须相同：{:?} vs {:?}",
        a.iter().map(|r| r.base).collect::<Vec<_>>(),
        b.iter().map(|r| r.base).collect::<Vec<_>>()
    );
    for (ra, rb) in a.iter().zip(b.iter()) {
        assert_eq!(ra.base, rb.base, "④ RAM 区域的基址必须相同");
        assert_eq!(
            ra.words.len(),
            rb.words.len(),
            "④ RAM 区域 {:#010x} 的长度必须相同",
            ra.base
        );
        for (i, (x, y)) in ra.words.iter().zip(rb.words.iter()).enumerate() {
            assert_eq!(
                x,
                y,
                "④ RAM 内容在 {:#010x} 上不同：{x:#010x} != {y:#010x}",
                ra.base + (i as u32) * 4
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 层 2 —— 主判别式（规格 §6 第 9 项）
// ---------------------------------------------------------------------------

/// 快照 → 还原 → 再跑 M 步，与「不打断」跑 M 步的**终态逐项相同**。
///
/// ```text
/// Host A: load → step_n(N) → capture → restore → step_n(M) → 终态 A
/// Host B: load → step_n(N) →                   step_n(M) → 终态 B
/// ```
#[test]
fn restoring_a_session_reproduces_the_uninterrupted_run() {
    // ---- Host A：中途打断。 ----
    let mut a = host_with_demo();
    step(&mut a, N_STEPS);
    a.raise_irq(5).expect("raise");
    let payload = a.capture_session(meta(&a));
    a.restore_session(&payload).expect("还原必须成功");
    step(&mut a, M_STEPS);

    // ---- Host B：不打断。 ----
    let mut b = host_with_demo();
    step(&mut b, N_STEPS);
    b.raise_irq(5).expect("raise");
    step(&mut b, M_STEPS);

    let fa = final_state(&a);
    let fb = final_state(&b);

    // 夹具自检：终态**不是**空的，否则下面的相等是「空 == 空」。
    assert_eq!(fa.pc, 0x24, "夹具：跑完 N+M 条之后 PC 必须停在自环上");
    assert_eq!(fa.gpr[1], 42, "夹具：x1 必须是 42");
    assert_eq!(fa.gpr[3], 49, "夹具：x3 必须是 49");
    assert_eq!(fa.gpr[4], 1, "夹具：x4 必须是 1");
    assert_eq!(
        fa.now_ps,
        (N_STEPS + M_STEPS) as u64 * INSTRUCTION_COST_PS,
        "夹具：虚拟时间必须是 (N+M) 个指令成本"
    );
    assert_eq!(
        a.read_memory(DEMO_STORE_ADDR, WriteSize::Word)
            .expect("read"),
        DEMO_STORE_VALUE,
        "夹具：固件必须已经把 42+7 存进 0x100"
    );
    assert_eq!(
        fa.peripherals.gpios.len(),
        1,
        "夹具：demo 内存图上有一个 GPIO"
    );
    assert_eq!(
        fa.peripherals.gpios[0].outputs, 1,
        "夹具：固件必须已经写过 GPIO.OUT"
    );
    assert_eq!(
        fa.peripherals.gpios[0].now_ps,
        (N_STEPS + M_STEPS - 1) as u64 * INSTRUCTION_COST_PS,
        "夹具：外设自己的时钟必须落在最后一次 tick 上"
    );
    assert_eq!(fa.irq.raised, vec![5], "夹具：段 5 有一行");

    assert_same_final_state(&fa, &fb);
}

/// 把载荷**还原进一个全新的会话**，再跑 M 步 —— 终态同样必须相同。
///
/// ⚠️ 这一条比上面那条**更强**：它证明载荷是**自足**的——内存内容、
/// 外设状态、引擎状态全都来自载荷，而不是「还原之前就已经在那儿」。
///
/// ⚠️ 它抓的是「某段状态没被还原」：新会话的 GPIO 状态是**空的**，
/// 载荷里的不是——不还原就一定对不上。
#[test]
fn a_payload_restores_an_equivalent_session_in_a_fresh_host() {
    // ---- 采一份载荷。 ----
    let mut source = host_with_demo();
    step(&mut source, N_STEPS);
    source.raise_irq(5).expect("raise");
    let payload = source.capture_session(meta(&source));

    // ---- 还原进一个**全新**的会话。 ----
    let mut restored = host_with_demo();

    // ⚠️ **夹具自检**（V-B 检视 · C-4）：全新会话在还原之前，CPU 状态
    // 必须是**空的**（PC = 0、通用寄存器全 0、没有拉起的 IRQ 行）。
    // 「不还原就一定对不上」这个**前提必须被断言**——否则它只是一句
    // 注释，而这个前提正是本用例的全部意义所在。
    // ⚠️ 原来的两行（`fr.gpr` / `fr.pc` 与参照比）**与主判别式的
    // ①② 完全重复**，且断的是**还原之后**的状态——那不是「夹具自检」。
    let pre = final_state(&restored);
    assert_eq!(pre.pc, 0, "夹具：全新会话还原前的 PC 必须是 0");
    assert_eq!(
        pre.gpr, [0u32; 32],
        "夹具：全新会话还原前的 32 个通用寄存器必须全 0"
    );
    assert_eq!(
        pre.irq.raised,
        Vec::<u16>::new(),
        "夹具：全新会话还原前没有拉起的 IRQ 行"
    );

    restored.restore_session(&payload).expect("还原必须成功");
    step(&mut restored, M_STEPS);

    // ---- 参照：不打断地跑 N+M 步。 ----
    let mut plain = host_with_demo();
    step(&mut plain, N_STEPS);
    plain.raise_irq(5).expect("raise");
    step(&mut plain, M_STEPS);

    let fr = final_state(&restored);
    let fp = final_state(&plain);

    assert_same_final_state(&fr, &fp);
}

/// 时间驱动路径（`Continue`）下的等价会话。
///
/// ⚠️ **为什么要有这一条**：`step_n` 走的是**指令驱动**路径，它**不排期**
/// 外设 tick 事件——队列恒空、插入序恒为 0，第 ⑦ 项（队列 / 插入序 / 指令
/// 预算）就**没有判别力**（比的是「空 == 空」）。
///
/// `Continue` 走 **时间驱动**路径（M-12 的唯一主循环）：量子边界会续排
/// 外设 tick，队列里**真的**有一件待派发的事件——第 ⑦ 项这才有牙齿。
#[test]
fn restoring_a_session_reproduces_the_uninterrupted_time_driven_run() {
    // ---- Host A：中途打断。 ----
    let mut a = host_with_demo();
    run_until_budget(&mut a);
    let payload = a.capture_session(meta(&a));
    a.restore_session(&payload).expect("还原必须成功");
    run_until_budget(&mut a);

    // ---- Host B：不打断。 ----
    let mut b = host_with_demo();
    run_until_budget(&mut b);
    run_until_budget(&mut b);

    let fa = final_state(&a);
    let fb = final_state(&b);

    // 夹具自检：队列**不是**空的——否则第 ⑦ 项比的是「空 == 空」。
    assert_eq!(
        fa.queue.len(),
        1,
        "夹具：时间驱动路径下队列里必须有一件待派发的外设 tick 事件"
    );
    assert!(
        fa.next_seq > 0,
        "夹具：插入序计数器必须已经动过（next_seq = {}）",
        fa.next_seq
    );
    assert_eq!(
        fa.instruction_budget, 10_000,
        "夹具：每轮指令预算必须是契约层定死的那个值"
    );

    assert_same_final_state(&fa, &fb);
}

// ---------------------------------------------------------------------------
// 层 1 —— 补充判别式（恢复是无损的）
// ---------------------------------------------------------------------------
/// 还原必须**无损**：还原之后再采一次载荷，六段**逐段相等**。
///
/// ⚠️ 断言是**六段逐段相等**，不是「整个结构相等」——笼统的相等说不清
/// 是**哪一段**不同。
#[test]
fn a_restored_session_reproduces_the_captured_payload_segment_by_segment() {
    let mut host = host_with_demo();
    step(&mut host, N_STEPS);
    host.raise_irq(5).expect("raise");
    let before = host.capture_session(meta(&host));
    assert_eq!(before.m16_intc.raised, vec![5], "夹具：段 5 有一行");

    // ---- 现场漂移：两处都动一下。 ----
    //
    // ⚠️ 不漂移的话，「恢复了」与「根本没动过」**不可区分**。
    host.lower_irq(5).expect("lower");
    write_gpio_out(&mut host, 0xF0);
    assert_eq!(host.intc_state().raised, Vec::<u16>::new());
    assert_eq!(host.peripheral_states().gpios[0].outputs, 0xF0);

    // ---- 还原。 ----
    host.restore_session(&before).expect("还原必须成功");
    let after = host.capture_session(meta(&host));

    assert_eq!(
        after.cp_meta, before.cp_meta,
        "段 0（检查点元数据）必须相等"
    );
    assert_eq!(
        after.m12_engine, before.m12_engine,
        "段 1（引擎段：队列 / 插入序 / 指令预算 / 平台状态）必须相等"
    );
    assert_eq!(after.m13_state, before.m13_state, "段 2（内存页）必须相等");
    assert_eq!(after.m14_map, before.m14_map, "段 3（区域拓扑）必须相等");
    assert_eq!(
        after.m15_periph, before.m15_periph,
        "段 4（外设状态）必须相等"
    );
    assert_eq!(
        after.m16_intc, before.m16_intc,
        "段 5（已拉起的 IRQ 行）必须相等"
    );
}

/// 层 1 的**自检**：它对段 5 的断言**不是恒真式**。
///
/// ⚠️ 撤掉 IRQ 行之后段 5 确实变了——所以层 1 的段 5 断言抓得住
/// 「存了、却没恢复」。
#[test]
fn the_lossless_restore_assertion_has_discriminating_power() {
    let mut host = host_with_demo();
    step(&mut host, N_STEPS);
    host.raise_irq(5).expect("raise");
    let before = host.capture_session(meta(&host));

    host.lower_irq(5).expect("lower");
    let after = host.capture_session(meta(&host));

    assert_eq!(
        after.m16_intc.raised,
        Vec::<u16>::new(),
        "撤掉之后段 5 变了"
    );
    assert_ne!(
        after.m16_intc, before.m16_intc,
        "所以层 1 的段 5 断言不是恒真式"
    );
}

/// 载荷还原后，**段 5 的具体那几行 IRQ** 必须回到会话里。
#[test]
fn the_raised_irq_lines_survive_the_round_trip() {
    let mut host = host_with_demo();
    step(&mut host, N_STEPS);
    host.raise_irq(5).expect("raise");
    host.raise_irq(9).expect("raise");
    let payload = host.capture_session(meta(&host));

    assert_eq!(payload.m16_intc.raised, vec![5, 9], "段 5 带着具体那两行");

    host.lower_irq(5).expect("lower");
    host.lower_irq(9).expect("lower");
    assert_eq!(host.intc_state().raised, Vec::<u16>::new());

    host.restore_session(&payload).expect("还原必须成功");
    assert_eq!(
        host.intc_state().raised,
        vec![5, 9],
        "还原后已拉起的 IRQ 行必须回到会话里"
    );
}

/// 还原后，**外设状态**必须回到载荷里的值（含外设自己的时钟）。
#[test]
fn the_peripheral_state_survives_the_round_trip() {
    let mut host = host_with_demo();
    step(&mut host, N_STEPS);
    let payload = host.capture_session(meta(&host));
    let want = payload.m15_periph.gpios[0].clone();
    assert_eq!(want.id, GPIO_PERIPH_ID);
    assert_eq!(want.outputs, 1, "固件写过 GPIO.OUT");

    // 漂移：把 OUT 改掉。
    write_gpio_out(&mut host, 0xF0);
    assert_eq!(host.peripheral_states().gpios[0].outputs, 0xF0);

    host.restore_session(&payload).expect("还原必须成功");
    let got = &host.peripheral_states().gpios[0];
    assert_eq!(got.id, want.id);
    assert_eq!(
        got.directions, want.directions,
        "方向掩码必须回到载荷里的值"
    );
    assert_eq!(got.outputs, want.outputs, "输出锁存必须回到载荷里的值");
    assert_eq!(got.inputs, want.inputs, "输入锁存必须回到载荷里的值");
    assert_eq!(got.pending_irqs, want.pending_irqs);
    assert_eq!(got.irq_lines, want.irq_lines);
    assert_eq!(got.now_ps, want.now_ps, "外设自己的时钟必须回到载荷里的值");
}
