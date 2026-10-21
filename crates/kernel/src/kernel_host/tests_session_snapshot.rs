//! 宿主层的**会话级快照**用例（STAGE-3）。
//!
//! 宿主是**采集器**、M-20 是**记录器**（规格 §3.5）：六段由宿主从各自
//! 的持有者（M-12 引擎 / M-14 总线 / M-15 外设 / M-16 控制器）收齐，
//! 再交给 M-20 组装成契约层的 [`SessionSnapshotDto`]。
//!
//! 本文件钉的是**宿主这一层**的三件事：
//!
//! 1. **委派是真的** —— `HostSinks::raise` / `lower` 不再是空实现，
//!    它们真的落到 M-16 的控制器上（段 5 不再恒为空）；
//! 2. **六段逐段来自各自的来源** —— 判别式落在**段的内容**上，不是
//!    「调用没 panic」；
//! 3. **恢复是无损的** —— 「校验先于应用」与「恢复顺序不可换」这两条
//!    不变量各有守卫。
//!
//! ⚠️ **判别式不许用「非空」「长度 > 0」**：那抓不到「某一段没进快照」
//! 这类缺陷。本文件的断言一律落在**具体值**上。

use super::*;

use riscv_sim_contract::error::ErrorFamily;
use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion, WriteSize,
};
use riscv_sim_contract::protocol::CommandPayload;
use riscv_sim_contract::session::{CheckpointMetaDto, RegionDto, SESSION_SNAPSHOT_SCHEMA_VERSION};

use crate::m17_backend::INSTRUCTION_COST_PS;

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// demo 固件 —— **引用共享常量**，不抄一份。
///
/// 权威副本在 [`DEMO_FIRMWARE_BYTES`]（Rust 侧唯一一份），它与前端
/// `front/dev/demo-firmware.js` 逐字节相同。
use super::DEMO_FIRMWARE_BYTES as DEMO_BYTES;

/// demo 固件写进内存的目标地址（`sw x3, 0x100(x0)`）。
const DEMO_STORE_ADDR: u32 = 0x100;

/// demo 固件写进内存的值（42 + 7）。
const DEMO_STORE_VALUE: u32 = 49;

/// demo 固件第一条指令的字面值（`addi x1, x0, 42` 的编码，小端）。
const DEMO_FIRST_WORD: u32 = 0x02a0_0093;

/// demo 固件里 GPIO `OUT` 寄存器的偏移（M-15 `gpio::reg::OUT`）。
const GPIO_OUT_OFFSET: u32 = 4;

/// 造一个挂着 demo 固件的工程。
///
/// ⚠️ **工程构造照抄** `tests.rs` 的 `demo_project()`——那份是**另一个
/// 模块**的私有函数（兄弟模块取不到），但**固件字节必须引用共享常量**
/// （`DEMO_FIRMWARE_BYTES`），不许抄第二份。
fn demo_project() -> Project {
    Project {
        id: "proj-demo".to_string(),
        name: "demo".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "demo-firmware.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(DEMO_BYTES.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

/// 检查点元数据（段 0）—— 调用方给的那一份。
fn meta() -> CheckpointMetaDto {
    CheckpointMetaDto {
        id: 7,
        t_virt: 12_345,
        t_host: 42,
        base_seq: 3,
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
    }
}

/// 造一个装好 demo 固件、跑过 `steps` 条指令的宿主。
fn host_with_demo(steps: u32) -> KernelHost {
    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::LoadProject(demo_project()),
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装载 demo 工程必须成功：{events:?}"
    );
    if steps > 0 {
        host.backend.step_n(steps).expect("step");
    }
    host
}

/// 往 GPIO 的 `OUT` 寄存器写一个值（用来制造**现场漂移**）。
fn write_gpio_out(host: &mut KernelHost, value: u32) {
    let events = host.handle_message(&Message::command(
        9,
        CommandPayload::WriteMemory {
            addr: DEFAULT_GPIO_BASE + GPIO_OUT_OFFSET,
            size: WriteSize::Word,
            value,
        },
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "写 GPIO.OUT 必须成功：{events:?}"
    );
}

// ---------------------------------------------------------------------------
// 工作项 A —— 中断汇真的委派给 M-16 的控制器
// ---------------------------------------------------------------------------

/// `HostSinks::raise` / `lower` **真的**落到控制器上。
///
/// ⚠️ 判别式是**具体值 5**——「非空」「长度 > 0」抓不到「委派又变回空
/// 实现」这个退化（那正是本用例要抓的）。
#[test]
fn the_interrupt_sink_delegates_to_the_controller() {
    let mut host = KernelHost::new();
    assert_eq!(
        host.intc_state().raised,
        Vec::<u16>::new(),
        "刚建好的宿主不该有已拉起的 IRQ 行"
    );

    host.raise_irq(5).expect("raise");
    assert_eq!(
        host.intc_state().raised,
        vec![5],
        "raise(5) 必须真的落到 M-16 的控制器上——以前这里是空实现，段 5 因此恒为空"
    );

    host.raise_irq(3).expect("raise");
    assert_eq!(
        host.intc_state().raised,
        vec![3, 5],
        "两行都要在，且必须升序（控制器的状态是 BTreeSet，序是构造出来的）"
    );

    host.lower_irq(5).expect("lower");
    assert_eq!(host.intc_state().raised, vec![3], "lower(5) 之后不该还有 5");
    assert_eq!(
        host.intc_delegation_failures(),
        0,
        "没有争用就不该有委派失败"
    );
}

/// 委派**不许 panic**：拿不到控制器就**记一次失败**并返回 `Ok(())`。
///
/// ⚠️ M-14 的总线 hook 在 `RefCell` 争用下 panic 是本项目**已经修过**的
/// 坑（V-A 检视 F-3）——同一个坑不许踩第二次。所以这条用例**主动制造
/// 争用**：控制器被借住的时候再 `raise`。
#[test]
fn the_interrupt_delegation_never_panics_under_contention() {
    let mut host = KernelHost::new();
    // 制造争用：把控制器借住不放。
    //
    // ⚠️ 必须**先克隆 `Rc`**：直接 `host.intc.controller.borrow_mut()` 会把
    // `host` 借走，之后就没法再 `&mut host` 了。
    let intc = Rc::clone(&host.intc);
    let borrowed = intc.controller.borrow_mut();
    // 这一行**必须**返回 `Ok(())` 而不是 panic —— `unwrap` / `borrow_mut`
    // 硬取在这里就是踩 F-3 那个坑。
    let result = host.raise_irq(5);
    assert!(
        result.is_ok(),
        "争用时委派必须返回 Ok(())（失败由计数器暴露），不能是错误、更不能 panic"
    );
    assert_eq!(
        host.intc_delegation_failures(),
        1,
        "争用必须被记成一次失败——静默丢掉一条 IRQ 行是本项目最恨的形态"
    );
    drop(borrowed);
    assert_eq!(
        host.intc_state().raised,
        Vec::<u16>::new(),
        "争用时那次 raise 确实没有落到控制器上"
    );
}

// ---------------------------------------------------------------------------
// 工作项 B —— 六段逐段来自各自的来源
// ---------------------------------------------------------------------------

/// 采集出来的载荷**六段齐全**，且**逐段等于**各自的来源。
///
/// ⚠️ 判别式落在**段的内容**上：页数 == 总线导出的页数、区域数 == 总线
/// 导出的区域数、外设数 == 总线上的外设数——不是「非空」。
#[test]
fn capture_session_assembles_the_six_segments_from_their_sources() {
    let mut host = host_with_demo(4);
    let payload = host.capture_session(meta());

    assert_eq!(
        payload.schema_version, SESSION_SNAPSHOT_SCHEMA_VERSION,
        "会话载荷的版本号必须由 M-20 钉死"
    );

    // 段 0 —— 调用方给的那一份，原样落位。
    assert_eq!(payload.cp_meta, meta(), "段 0 必须原样落位");

    // 段 1 —— M-12 引擎段。
    assert_eq!(
        payload.m12_engine,
        host.engine_snapshot(),
        "段 1 必须等于引擎的可序列化完整快照"
    );
    assert_eq!(
        payload.m12_engine.state.pc, 0x10,
        "跑了 4 条指令之后 PC 必须落在 0x10（addi x2, x0, 7 之后）"
    );
    assert_eq!(
        payload.m12_engine.state.gpr[1], 42,
        "段 1 里的 GPR 必须是执行后的真值"
    );

    // 段 2 —— M-14 的内存页。
    let pages = host.backend.bus().export_pages();
    assert_eq!(
        payload.m13_state.mem_pages, pages,
        "段 2 必须逐页等于总线的导出"
    );
    assert_eq!(
        payload.m13_state.mem_pages.len(),
        80,
        "低段 1 MiB（16 页）+ 高段 4 MiB（64 页），按 64 KiB 分页"
    );
    assert_eq!(
        payload.m13_state.mem_pages[0].base, 0,
        "第一页必须落在低段 RAM 的起点"
    );
    assert_eq!(
        payload.m13_state.mem_pages[0].bytes[0..4],
        DEMO_BYTES[0..4],
        "第一页的头四个字节必须是固件的头四个字节"
    );

    // 段 3 —— M-14 的区域拓扑。
    let map = host.backend.bus().export_map();
    assert_eq!(payload.m14_map, map, "段 3 必须等于总线的拓扑导出");
    assert_eq!(
        payload.m14_map.regions.len(),
        3,
        "两段 RAM + 一个 GPIO 外设窗口"
    );
    assert_eq!(payload.m14_map.regions[0].base, 0);
    assert_eq!(payload.m14_map.regions[0].kind, "ram");
    assert_eq!(payload.m14_map.regions[1].base, DEFAULT_GPIO_BASE);
    assert_eq!(payload.m14_map.regions[1].kind, "peripheral");
    assert_eq!(payload.m14_map.regions[1].target, GPIO_PERIPH_ID);
    assert_eq!(payload.m14_map.regions[2].base, DEFAULT_RAM_HIGH_BASE);
    assert_eq!(payload.m14_map.regions[2].kind, "ram");

    // 段 4 —— M-15 的外设内部状态。
    assert_eq!(
        payload.m15_periph,
        host.peripheral_states(),
        "段 4 必须等于外设的导出"
    );
    assert_eq!(
        payload.m15_periph.gpios.len(),
        1,
        "demo 内存图上只有一个 GPIO"
    );
    assert_eq!(payload.m15_periph.gpios[0].id, GPIO_PERIPH_ID);
    assert_eq!(payload.m15_periph.gpios[0].outputs, 0);
    assert_eq!(payload.m15_periph.gpios[0].directions, 0);
    assert_eq!(payload.m15_periph.gpios[0].inputs, 0);
    assert_eq!(payload.m15_periph.gpios[0].pending_irqs, 0);
    assert_eq!(
        payload.m15_periph.gpios[0].now_ps,
        3 * INSTRUCTION_COST_PS,
        "外设自己的时钟必须落在最后一次 tick 上（第 4 条指令之前 = 3 个指令成本）"
    );

    // 段 5 —— M-16 的已拉起 IRQ 行。
    assert_eq!(
        payload.m16_intc,
        host.intc_state(),
        "段 5 必须等于控制器的导出"
    );
    assert_eq!(payload.m16_intc.raised, Vec::<u16>::new());

    // 整份载荷必须自洽。
    payload.validate().expect("宿主组装出来的载荷必须能过校验");
}

/// 段 5 必须**真的**带着已拉起的 IRQ 行——不是「非空」。
#[test]
fn capture_session_carries_the_raised_irq_lines() {
    let mut host = host_with_demo(2);
    host.raise_irq(5).expect("raise");
    host.raise_irq(9).expect("raise");

    let payload = host.capture_session(meta());
    assert_eq!(
        payload.m16_intc.raised,
        vec![5, 9],
        "段 5 必须带着具体的那两行 IRQ"
    );
    payload.validate().expect("带 IRQ 行的载荷也必须自洽");
}

// ---------------------------------------------------------------------------
// 工作项 C —— 恢复：先校验、顺序不可换、失败的那一步不写东西
// ---------------------------------------------------------------------------

/// **畸形载荷**（区域表非升序）→ 拒，且**总线内容一字未改**。
#[test]
fn restore_session_rejects_a_malformed_payload_without_touching_the_bus() {
    let mut host = host_with_demo(4);
    let firmware_word = host.read_memory(0, WriteSize::Word).expect("read");
    let stored = host
        .read_memory(DEMO_STORE_ADDR, WriteSize::Word)
        .expect("read");
    assert_eq!(firmware_word, DEMO_FIRST_WORD, "固件必须已经装载");
    assert_eq!(stored, DEMO_STORE_VALUE, "固件必须已经把 42+7 存进 0x100");

    let mut payload = host.capture_session(meta());
    // 故意打乱区域表：大 base 的在前。
    payload.m14_map.regions.swap(0, 1);

    let err = host
        .restore_session(&payload)
        .expect_err("结构畸形的载荷必须被拒");
    assert_eq!(err.family, ErrorFamily::Snapshot, "族必须是 SNAP");
    assert_eq!(err.scenario, 6, "必须是 E_SNAP-06（会话载荷畸形）");

    assert_eq!(
        host.read_memory(0, WriteSize::Word).expect("read"),
        firmware_word,
        "被拒的载荷不许动总线——一个字都不许动"
    );
    assert_eq!(
        host.read_memory(DEMO_STORE_ADDR, WriteSize::Word)
            .expect("read"),
        stored,
        "被拒的载荷不许动总线——一个字都不许动"
    );
    assert_eq!(host.registers()[1], 42, "引擎也不能动");
}

/// **版本号不对** → 拒，且状态未改。
#[test]
fn restore_session_rejects_an_unknown_schema_version_without_touching_the_state() {
    let mut host = host_with_demo(4);
    let regs = host.registers();
    let mut payload = host.capture_session(meta());
    payload.schema_version = 999;

    let err = host
        .restore_session(&payload)
        .expect_err("版本不认识的载荷必须被拒");
    assert_eq!(err.family, ErrorFamily::Snapshot);
    assert_eq!(err.scenario, 5, "必须是 E_SNAP-05（版本不受支持）");

    assert_eq!(host.registers(), regs, "寄存器一个都不许动");
    assert_eq!(
        host.read_memory(DEMO_STORE_ADDR, WriteSize::Word)
            .expect("read"),
        DEMO_STORE_VALUE,
        "总线不许动"
    );
    assert_eq!(host.now_ps(), 4 * INSTRUCTION_COST_PS, "虚拟时间不许动");
}

/// **含未知外设 id** → 拒，且**外设状态未改**。
///
/// ⚠️ 这一条同时钉住了「失败的那一步没有写入任何东西」：外设回灌
/// （第 3 步）的检查**全在写入之前**跑完，所以它失败时外设一个字段
/// 都没动。
#[test]
fn restore_session_rejects_an_unknown_peripheral_id_without_touching_the_peripheral_state() {
    let mut host = host_with_demo(8);
    // 固件的 `sw x6, 4(x5)` 已经把 GPIO.OUT 写成 1。
    assert_eq!(
        host.peripheral_states().gpios[0].outputs,
        1,
        "固件必须已经写过 GPIO.OUT"
    );

    let mut payload = host.capture_session(meta());
    payload.m15_periph.gpios[0].id = "NO_SUCH_GPIO".to_string();

    let err = host
        .restore_session(&payload)
        .expect_err("载荷里的外设 id 对不上活实例时必须被拒");
    assert_eq!(err.family, ErrorFamily::Peripheral, "族必须是 PER");
    assert_eq!(err.scenario, 5, "必须是 E_PER-05（外设集合对不上）");

    let gpio = &host.peripheral_states().gpios[0];
    assert_eq!(gpio.id, GPIO_PERIPH_ID, "活实例不许被换掉");
    assert_eq!(gpio.outputs, 1, "失败的那一步（外设回灌）没有写入任何东西");
    assert_eq!(
        gpio.now_ps,
        7 * INSTRUCTION_COST_PS,
        "失败的那一步没有写入任何东西——外设自己的时钟也不能动"
    );
}

/// **第 1 步（区域拓扑）失败** → 总线**一字未改**。
///
/// ⚠️ 夹具用「载荷里说总线有一块 ROM，总线上却没有」：契约层**不**查
/// ROM 的存在（它只查顺序 / 重叠 / 大小），所以这一条能走到 M-14 的
/// 加载器里——它必须在**写任何东西之前**就拒掉。
///
/// ⚠️ 这正是规格 §3.2 里 ROM 那一段的防线：ROM 内容随固件走、不进快照，
/// 重建即清零 = **固件被抹掉**。
#[test]
fn a_failing_topology_step_leaves_the_bus_untouched() {
    let mut host = host_with_demo(4);
    let firmware_word = host.read_memory(0, WriteSize::Word).expect("read");
    let mut payload = host.capture_session(meta());
    // 载荷里多一块 ROM —— 当前总线上没有对应区域。
    payload.m14_map.regions.push(RegionDto {
        base: 0x9000_0000,
        size: 0x1000,
        kind: "rom".to_string(),
        target: String::new(),
    });

    let err = host
        .restore_session(&payload)
        .expect_err("载荷里的 ROM 在总线上没有对应区域时必须被拒");
    assert_eq!(err.family, ErrorFamily::Bus, "族必须是 BUS");
    assert_eq!(err.scenario, 12, "必须是 E_BUS-12（ROM 拓扑不匹配）");

    assert_eq!(
        host.read_memory(0, WriteSize::Word).expect("read"),
        firmware_word,
        "失败的那一步（区域拓扑）没有写入任何东西"
    );
    assert_eq!(host.registers()[1], 42, "引擎也不能动");
}

/// 恢复成功后，**外设状态 / IRQ 行 / 寄存器 / 虚拟时间**逐字段等于载荷
/// 里的值。
#[test]
fn a_successful_restore_puts_every_field_back() {
    let mut host = host_with_demo(8);
    host.raise_irq(5).expect("raise");
    let payload = host.capture_session(meta());

    // 制造漂移：IRQ 行撤掉、GPIO.OUT 改掉。
    host.lower_irq(5).expect("lower");
    write_gpio_out(&mut host, 0xF0);
    assert_eq!(host.intc_state().raised, Vec::<u16>::new());
    assert_eq!(host.peripheral_states().gpios[0].outputs, 0xF0);

    host.restore_session(&payload).expect("restore");

    assert_eq!(
        host.intc_state().raised,
        vec![5],
        "IRQ 行必须回到载荷里的值"
    );
    let gpio = &host.peripheral_states().gpios[0];
    let want = &payload.m15_periph.gpios[0];
    assert_eq!(gpio.id, want.id);
    assert_eq!(gpio.directions, want.directions);
    assert_eq!(
        gpio.outputs, want.outputs,
        "GPIO 输出锁存必须回到载荷里的值"
    );
    assert_eq!(gpio.inputs, want.inputs);
    assert_eq!(gpio.pending_irqs, want.pending_irqs);
    assert_eq!(gpio.now_ps, want.now_ps, "外设自己的时钟必须回到载荷里的值");
    assert_eq!(gpio.irq_lines, want.irq_lines);
    assert_eq!(
        host.registers(),
        payload.m12_engine.state.gpr,
        "GPR 必须回到载荷里的值"
    );
    assert_eq!(host.now_ps(), payload.m12_engine.state.virtual_time_ps);
}

/// 恢复必须**按载荷重建区域拓扑**——不是「原样留着」。
///
/// ⚠️ 这一条钉的是**段 3 真的被应用了**：载荷里多一段 RAM，恢复后总线
/// 上就必须多出那一段。少了它，「存了却没恢复」就会溜过去。
#[test]
fn restore_session_rebuilds_the_memory_map_topology() {
    let mut host = host_with_demo(2);
    let mut payload = host.capture_session(meta());
    assert_eq!(host.memory_map().regions.len(), 3, "夹具的拓扑是三段");

    // 给载荷加一段 RAM（插在升序的位置上：0x0 < 0x2000_0000 < 0x4000_0000）。
    payload.m14_map.regions.insert(
        1,
        RegionDto {
            base: 0x2000_0000,
            size: 0x1000,
            kind: "ram".to_string(),
            target: String::new(),
        },
    );

    host.restore_session(&payload).expect("restore");

    let map = host.memory_map();
    assert_eq!(map.regions.len(), 4, "恢复后总线必须照载荷重建出四段区域");
    assert_eq!(map.regions[1].base, 0x2000_0000);
    assert_eq!(map.regions[1].size, 0x1000);
    assert_eq!(map.regions[1].kind, "ram");
}

// ---------------------------------------------------------------------------
// 层 1 —— 恢复是**无损**的（逐段相等）
// ---------------------------------------------------------------------------

/// 恢复必须**无损**：恢复之后再采一次载荷，六段**逐段相等**。
///
/// ⚠️ **为什么要先制造漂移**：不漂移的话，「恢复了」与「根本没动过」
/// **不可区分**——一段既没被存、也没被恢复的状态，两次采集会**同样地**
/// 缺它，用例照样绿。这里把 IRQ 行、GPIO 输出、区域拓扑三处都动一下，
/// 恢复必须把它们**逐段**抹平。
///
/// ⚠️ 断言是**六段逐段相等**，不是「整个结构相等」——笼统的相等说不清
/// 是**哪一段**不同。
#[test]
fn a_restored_session_reproduces_the_captured_payload_segment_by_segment() {
    let mut host = host_with_demo(8);
    host.raise_irq(5).expect("raise");
    let before = host.capture_session(meta());
    assert_eq!(before.m16_intc.raised, vec![5], "夹具：段 5 有一行");

    // ---- 现场漂移：三处都动一下。 ----
    host.lower_irq(5).expect("lower");
    write_gpio_out(&mut host, 0xF0);
    host.backend
        .bus_mut()
        .map_ram(0x2000_0000, 0x1000, &[])
        .expect("map ram");
    assert_eq!(host.intc_state().raised, Vec::<u16>::new());
    assert_eq!(host.peripheral_states().gpios[0].outputs, 0xF0);
    assert_eq!(host.memory_map().regions.len(), 4);

    // ---- 恢复。 ----
    host.restore_session(&before).expect("restore");
    let after = host.capture_session(meta());

    assert_eq!(
        after.cp_meta, before.cp_meta,
        "段 0（检查点元数据）必须相等"
    );
    assert_eq!(
        after.m12_engine, before.m12_engine,
        "段 1（引擎段）必须相等——队列 / 插入序 / 指令预算都在里面"
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

/// 层 1 的**反面**：段 5 少一行时，**必须**被抓住。
///
/// ⚠️ 这条是**层 1 的自检**：把漂移撤掉（不制造 IRQ 漂移），层 1 的
/// 段 5 断言就**恒真**——那样「存了却没恢复」会溜过去。这条用例证明
/// 层 1 的段 5 断言**有判别力**：它只在真的还原了 IRQ 行时才绿。
#[test]
fn the_lossless_restore_assertion_has_discriminating_power_for_the_irq_segment() {
    let mut host = host_with_demo(2);
    host.raise_irq(5).expect("raise");
    let before = host.capture_session(meta());

    // 撤掉 IRQ 行，但**不**恢复它 —— 模拟「存了、却没恢复」。
    host.lower_irq(5).expect("lower");
    let after = host.capture_session(meta());

    assert_eq!(
        after.m16_intc.raised,
        Vec::<u16>::new(),
        "撤掉之后段 5 确实变了"
    );
    assert_ne!(
        after.m16_intc, before.m16_intc,
        "所以层 1 的段 5 断言不是恒真式——它抓得住「存了却没恢复」"
    );
}
