//! **D-151** —— `time` / `timeh` 的真值接 M-15 的 CLINT。
//!
//! 冻结规格：`docs/detailed-design/STAGE-26_V041_Mtval_Time_Vacuity.md`
//! §4.2（五条判别式）· 裁决 F / G / H / I / J / K。
//!
//! ## 为什么判别力**全部**落在这里（裁决 J）
//!
//! 参考侧在**黄金配置**下 `time` **恒读 0**（事实 **F5**）⇒ ⇒ 两侧
//! **不可比** ⇒ ⇒ ⛔ **不许**新增一条查 `time` 的黄金语料（那会**假红**）
//! ⇒ ⇒ 五条判别式**全部**是 Rust 单测（验收 **B-10** / **X-3**）。
//!
//! ## 五条判别式（规格 §4.2 的表）
//!
//! | # | 判别式 | 本文件里的用例 |
//! | --- | --- | --- |
//! | ① | `csrr time` 的值**随虚拟时间推进而变** | [`the_time_csr_follows_the_platform_clock`] |
//! | ② | 写 `time` **不生效**（只读） | [`writing_time_does_not_change_what_it_reads_back`] |
//! | ③ | 与 CLINT 的 `mtime` 读数**逐字相同**（**同源**） | [`the_time_csr_is_the_clint_mtime_verbatim`] |
//! | ④ | `timeh` 与 `time` 的**高位配对一致** | [`the_two_halves_are_paired`] |
//! | ⑤ | S / U 模式的**使能链**不受影响 | [`the_counter_enable_chain_still_gates_time`] |
//!
//! ## 两条派发路径**同口径**（验收 **B-7** · D-11 的延续）
//!
//! [`the_two_dispatch_routes_see_the_same_clock`] 断言闭包口
//! （`Iss::step_one`）与总线口（`Iss::step_one_via_bus`）读到的 `time`
//! **逐字相同** ⇒ ⇒ 不会出现「一条路有、一条路没有」。
//!
//! ## 护栏**仍在**（裁决 H · 验收 **B-6**）
//!
//! [`a_malformed_payload_cannot_inject_a_time_value`] 断言：把 `time`
//! **塞进 CSR 文件**（畸形载荷）⇒ ⇒ 读回来的**仍然**是**时钟给的数**，
//! 不是文件里的数 ⇒ ⇒ 「`time` 永远不从 CSR 文件读」这条护栏**没被删**。

use std::cell::RefCell;
use std::rc::Rc;

use riscv_sim_contract::csr::{
    COUNTEREN_TM, CSR_MCOUNTEREN, CSR_SCOUNTEREN, CSR_TIME, CSR_TIMEH, CSR_UCOUNTEREN, PRIV_M,
    PRIV_S, PRIV_U,
};
use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{SimState, WriteSize};
use riscv_sim_contract::port::{Peripheral, TimeSource};

use crate::m13_iss::csr::{csr_read_with_time, csr_write};
use crate::m13_iss::{priv_set, Iss, StepOutcome, TrapCause};
use crate::m14_bus::Bus;
use crate::m15_peripheral::clint::Clint;

use super::platform::clint_time_source;

// ---------------------------------------------------------------------------
// Architectural literals —— 判别力全靠它们
// ---------------------------------------------------------------------------

/// 异常 cause **2** —— 非法指令（按级不可访问的 CSR）。
const CAUSE_ILLEGAL: u32 = 2;

/// `mtime` 的 **MMIO** 偏移（低 32 位）—— QEMU virt / osz 平台头。
///
/// ⚠️ 这是**架构字面量**（CLINT 的寄存器图）⇒ ⇒ 判别式 ③ 比的是「**CSR
/// 读**」与「**MMIO 读**」两条**不同**的路 ⇒ ⇒ 若是「各算一份」，两条路
/// 必然对不上。
const MTIME_LOW: u32 = 0xBFF8;
/// `mtime` 的 **MMIO** 偏移（高 32 位）。
const MTIME_HIGH: u32 = 0xBFFC;

/// 测试程序的地址。
const PC: u32 = 0x1000;
/// 测试 RAM 的大小。
const RAM_SIZE: usize = 64 * 1024;

/// 平台的时钟频率（10 MHz —— 与 CLINT 的默认档一致）。
const FREQ_HZ: u64 = 10_000_000;
/// 虚拟时间 1 ms（ps）。
const ONE_MS_PS: u64 = 1_000_000;

// ---------------------------------------------------------------------------
// Instruction encoders
// ---------------------------------------------------------------------------

/// `SYSTEM` 组的 opcode。
const SYSTEM: u8 = 0b111_0011;

/// Encode an I-type instruction.
fn enc_i(imm: i32, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    (((imm as u32) & 0xFFF) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | ((rd as u32) << 7)
        | (opcode as u32)
}

/// `csrr <rd>, <csr>` —— 一条**纯读**（`csrrs rd, csr, x0`）。
fn csrr(csr: u16, rd: u8) -> u32 {
    enc_i(csr as i32, 0, 0b010, rd, SYSTEM)
}

/// `csrw <csr>, <rs1>` —— 一条**写**（`csrrw csr, rs1, x0`）。
fn csrw(csr: u16, rs1: u8) -> u32 {
    enc_i(csr as i32, rs1, 0b001, 0, SYSTEM)
}

// ---------------------------------------------------------------------------
// Test scaffolding
// ---------------------------------------------------------------------------

/// 字节可寻址的小 RAM —— 测试程序的落脚点。
struct Ram {
    base: u32,
    bytes: Vec<u8>,
}

impl Ram {
    fn new(base: u32, size: usize) -> Self {
        Self {
            base,
            bytes: vec![0u8; size],
        }
    }

    fn offset(&self, addr: u32) -> Option<usize> {
        let rel = addr.checked_sub(self.base)? as usize;
        if rel < self.bytes.len() {
            Some(rel)
        } else {
            None
        }
    }

    fn load(&mut self, addr: u32, words: &[u32]) {
        for (i, word) in words.iter().enumerate() {
            let at = addr + (i as u32) * 4;
            let off = self.offset(at).expect("program image fits in RAM");
            self.bytes[off..off + 4].copy_from_slice(&word.to_le_bytes());
        }
    }

    fn read(&self, addr: u32, size: WriteSize) -> Result<u32, PlatformError> {
        let off = self
            .offset(addr)
            .ok_or_else(|| PlatformError::new(ErrorFamily::Iss, 1, "unmapped read"))?;
        let width = match size {
            WriteSize::Byte => 1,
            WriteSize::Half => 2,
            WriteSize::Word => 4,
        };
        let mut value = 0u32;
        for i in 0..width {
            value |= (self.bytes[off + i] as u32) << (8 * i);
        }
        Ok(value)
    }

    fn write(&mut self, addr: u32, size: WriteSize, value: u32) -> Result<(), PlatformError> {
        let off = self
            .offset(addr)
            .ok_or_else(|| PlatformError::new(ErrorFamily::Iss, 1, "unmapped write"))?;
        let width = match size {
            WriteSize::Byte => 1,
            WriteSize::Half => 2,
            WriteSize::Word => 4,
        };
        for i in 0..width {
            self.bytes[off + i] = (value >> (8 * i)) as u8;
        }
        Ok(())
    }
}

/// ISS + RAM —— 驱动**闭包口**（`Iss::step_one`）。
struct Driver {
    ram: RefCell<Ram>,
    iss: Iss,
}

impl Driver {
    /// 造一个驱动器；`time_source` = `None` ⇒ ⇒ 没有时钟 ⇒ ⇒ `time` 报 0。
    fn new(time_source: Option<Rc<dyn TimeSource>>) -> Self {
        let mut iss = Iss::default();
        iss.set_time_source(time_source);
        Self {
            ram: RefCell::new(Ram::new(0, RAM_SIZE)),
            iss,
        }
    }

    fn load(&mut self, addr: u32, words: &[u32]) {
        self.ram.borrow_mut().load(addr, words);
    }

    /// 在 `pc` 上跑一条指令（闭包口）。
    fn step_at(&mut self, state: &mut SimState, pc: u32, word: u32) -> StepOutcome {
        self.load(pc, &[word]);
        state.pc = pc;
        let ram = &self.ram;
        self.iss
            .step_one(
                state,
                |addr, size| ram.borrow().read(addr, size),
                |addr, size, value| ram.borrow_mut().write(addr, size, value),
            )
            .expect("step succeeds")
    }

    /// **客体视角**读一个 CSR —— 一条真的 `csrr <csr>, x2` 指令。
    ///
    /// ⚠️ 读**可能**陷阱（按级不可访问）⇒ ⇒ 结局与读到的值**一起**返回。
    fn guest_read(&mut self, state: &mut SimState, csr: u16) -> (StepOutcome, u32) {
        let pc = state.pc;
        let outcome = self.step_at(state, pc, csrr(csr, 2));
        (outcome, state.gpr[2])
    }
}

/// 一个 10 MHz 的 CLINT（宿主手里那**一个**实例）。
fn clint() -> Rc<RefCell<Clint>> {
    Rc::new(RefCell::new(Clint::with_frequency(
        "CLINT0",
        0x0200_0000,
        1,
        FREQ_HZ,
    )))
}

/// 让 CLINT 的虚拟时间走到 `now_ps`。
fn advance(clint: &Rc<RefCell<Clint>>, now_ps: u64) {
    clint.borrow_mut().tick(now_ps).expect("tick");
}

// ---------------------------------------------------------------------------
// ① `csrr time` 的值**随虚拟时间推进而变**
// ---------------------------------------------------------------------------

/// ① `csrr time` 的值**随虚拟时间推进而变**。
///
/// ⚠️ **改前必红**：落地前 `time` **恒读 0** ⇒ ⇒ 第二次读还是 0 ⇒ ⇒
/// `assert_ne!` 当场红。
///
/// ⚠️ **数字是算出来的**：10 MHz ⇒ 1 ms = **10 个周期** ⇒ ⇒ 断言的是
/// 「周期」，**不是** ps、**不是**指令数（裁决 G）。
#[test]
fn the_time_csr_follows_the_platform_clock() {
    let clint = clint();
    let mut driver = Driver::new(Some(clint_time_source(&clint)));
    let mut state = SimState::default();

    // 虚拟时间 1 ms ⇒ 10 个周期。
    advance(&clint, ONE_MS_PS);
    let first = driver.guest_read(&mut state, CSR_TIME).1;
    assert_eq!(
        first, 10,
        "`time` 必须是**平台时钟的周期数**（10 MHz × 1 ms = 10 周期）"
    );

    // 再走 1 ms ⇒ 20 个周期 ⇒ ⇒ **值必须变**。
    advance(&clint, 2 * ONE_MS_PS);
    let second = driver.guest_read(&mut state, CSR_TIME).1;
    assert_eq!(second, 20, "时钟再走 1 ms ⇒ `time` 必须跟着走");
    assert_ne!(
        first, second,
        "`time` **恒读 0** 就是 D-151 那条欠账的症状 ⇒ ⇒ 它必须随时间变"
    );
}

// ---------------------------------------------------------------------------
// ② 写 `time` **不生效**（只读）
// ---------------------------------------------------------------------------

/// ② 写 `time` **不生效** ⇒ ⇒ 写后读回**逐字不变**。
///
/// ⚠️ **时钟冻住**（不 `tick`）⇒ ⇒ 写前写后读回**必须完全一样** ⇒ ⇒
/// 「写进去了」与「写被忽略了」**分得开**。
///
/// ⚠️ **顺带钉住「CSR 文件里没有 `time` 那一条」** ⇒ ⇒ 写**根本没发生**
/// ⇒ ⇒ 不是「写进去了但读路径特殊处理」。
#[test]
fn writing_time_does_not_change_what_it_reads_back() {
    let clint = clint();
    advance(&clint, ONE_MS_PS);
    let mut driver = Driver::new(Some(clint_time_source(&clint)));
    let mut state = SimState::default();

    let before = driver.guest_read(&mut state, CSR_TIME).1;
    assert_eq!(before, 10, "前置条件：时钟给了 10 个周期");

    // `csrw time, x1`（x1 = 0xDEAD_BEEF）⇒ ⇒ `time` 是**只读**的 ⇒ ⇒ 写
    // **必须被忽略**。
    state.gpr[1] = 0xDEAD_BEEF;
    driver.step_at(&mut state, PC, csrw(CSR_TIME, 1));

    let after = driver.guest_read(&mut state, CSR_TIME).1;
    assert_eq!(after, before, "写 `time` **不生效** ⇒ ⇒ 读回必须不变");
    assert!(
        !state.csrs.iter().any(|(number, _)| *number == CSR_TIME),
        "CSR 文件里**不该**出现 `time` 那一条 ⇒ ⇒ 写根本没发生"
    );
}

// ---------------------------------------------------------------------------
// ③ 与 CLINT 的 `mtime` 读数**逐字相同**（**同源**）
// ---------------------------------------------------------------------------

/// ③ `csrr time` 与 CLINT 的 **MMIO** `mtime` 读数**逐字相同**。
///
/// ⚠️ **两条不同的路**：① guest 的 `csrr time`（ISS ⇒ `TimeSource` 端口 ⇒
/// CLINT）② CLINT 的 **MMIO** 窗口 `0xBFF8`（`Peripheral::read` ⇒ 同**一个**
/// CLINT 实例）⇒ ⇒ 若是「各算一份」，这两条路**必然对不上**。
///
/// ⚠️ 这就是「**同源**」的判别力：**不是**两个恰好相等的数。
#[test]
fn the_time_csr_is_the_clint_mtime_verbatim() {
    let clint = clint();
    advance(&clint, ONE_MS_PS);
    let mut driver = Driver::new(Some(clint_time_source(&clint)));
    let mut state = SimState::default();

    let via_csr = driver.guest_read(&mut state, CSR_TIME).1;
    let via_mmio = clint
        .borrow()
        .read(MTIME_LOW, WriteSize::Word)
        .expect("mtime low half is readable");
    assert_eq!(
        via_csr, via_mmio,
        "`csrr time` 与 CLINT 的 `mtime` **必须逐字相同** ⇒ ⇒ 两侧**同源**"
    );
    assert_eq!(via_csr, 10, "两条路都必须是 10 个周期");
}

// ---------------------------------------------------------------------------
// ④ `timeh` 与 `time` 的**高位配对一致**
// ---------------------------------------------------------------------------

/// ④ `timeh` 与 `time` 的**高位配对一致** ⇒ ⇒ 拼起来就是**一个** 64 位数。
///
/// ⚠️ 喂一个**高低两半都非零**的值 ⇒ ⇒ 只看低半部的话，配对错了也发现
/// 不了。
#[test]
fn the_two_halves_are_paired() {
    let clint = clint();
    clint.borrow_mut().set_mtime_cycles(0x1234_5678_9ABC_DEF0);
    let mut driver = Driver::new(Some(clint_time_source(&clint)));
    let mut state = SimState::default();

    let low = driver.guest_read(&mut state, CSR_TIME).1;
    let high = driver.guest_read(&mut state, CSR_TIMEH).1;
    assert_eq!(low, 0x9ABC_DEF0, "`time` 是低 32 位");
    assert_eq!(high, 0x1234_5678, "`timeh` 是高 32 位");
    assert_eq!(
        ((high as u64) << 32) | (low as u64),
        0x1234_5678_9ABC_DEF0,
        "两半拼起来必须**逐字**是那一个 64 位读数"
    );
    assert_eq!(
        high,
        clint
            .borrow()
            .read(MTIME_HIGH, WriteSize::Word)
            .expect("mtime high"),
        "`timeh` 也必须与 CLINT 的 `mtime` 高半部**逐字相同**"
    );
}

// ---------------------------------------------------------------------------
// ⑤ S / U 模式的**使能链**不受影响
// ---------------------------------------------------------------------------

/// ⑤ S / U 模式的**使能链**（`mcounteren.TM` / `scounteren.TM` /
/// `ucounteren.TM`）**不受影响** ⇒ ⇒ 缺一仍是 **cause 2**。
///
/// ⚠️ **判别力在「能不能读」上**，不是「读到什么」⇒ ⇒ 断言的是**陷阱**。
#[test]
fn the_counter_enable_chain_still_gates_time() {
    let clint = clint();
    advance(&clint, ONE_MS_PS);
    let mut driver = Driver::new(Some(clint_time_source(&clint)));

    // ---- S 模式：使能链缺一 ⇒ ⇒ cause 2 ----
    for (m, s, label) in [
        (0u32, 0u32, "两条都缺"),
        (1 << COUNTEREN_TM, 0, "缺 scounteren"),
    ] {
        let mut state = SimState::default();
        priv_set(&mut state, PRIV_S);
        csr_write(&mut state, CSR_MCOUNTEREN, m);
        csr_write(&mut state, CSR_SCOUNTEREN, s);
        let (outcome, _) = driver.guest_read(&mut state, CSR_TIME);
        assert_eq!(
            outcome,
            StepOutcome::Trapped {
                cause: TrapCause::IllegalInstruction
            },
            "S 模式 {label} ⇒ ⇒ `time` 必须是 **cause 2**（不是读回 0）"
        );
    }

    // ---- S 模式：链**齐了** ⇒ ⇒ 读得到真值 ----
    {
        let mut state = SimState::default();
        priv_set(&mut state, PRIV_S);
        csr_write(&mut state, CSR_MCOUNTEREN, 1 << COUNTEREN_TM);
        csr_write(&mut state, CSR_SCOUNTEREN, 1 << COUNTEREN_TM);
        let (outcome, value) = driver.guest_read(&mut state, CSR_TIME);
        assert!(
            matches!(outcome, StepOutcome::Advanced),
            "S 模式使能链齐了 ⇒ ⇒ `time` **读得到**"
        );
        assert_eq!(value, 10, "读到的必须是**平台的真值**");
    }

    // ---- U 模式：还缺 `ucounteren.TM` ⇒ ⇒ cause 2 ----
    {
        let mut state = SimState::default();
        priv_set(&mut state, PRIV_U);
        csr_write(&mut state, CSR_MCOUNTEREN, 1 << COUNTEREN_TM);
        csr_write(&mut state, CSR_SCOUNTEREN, 1 << COUNTEREN_TM);
        let (outcome, _) = driver.guest_read(&mut state, CSR_TIME);
        assert_eq!(
            outcome,
            StepOutcome::Trapped {
                cause: TrapCause::IllegalInstruction
            },
            "U 模式缺 `ucounteren.TM` ⇒ ⇒ `time` 必须是 **cause 2**"
        );
        assert_eq!(
            TrapCause::IllegalInstruction.mcause(),
            CAUSE_ILLEGAL,
            "cause 的**架构码**必须是 2"
        );
    }

    // ---- U 模式：三条链**齐了** ⇒ ⇒ 读得到真值 ----
    {
        let mut state = SimState::default();
        priv_set(&mut state, PRIV_U);
        csr_write(&mut state, CSR_MCOUNTEREN, 1 << COUNTEREN_TM);
        csr_write(&mut state, CSR_SCOUNTEREN, 1 << COUNTEREN_TM);
        csr_write(&mut state, CSR_UCOUNTEREN, 1 << COUNTEREN_TM);
        let (outcome, value) = driver.guest_read(&mut state, CSR_TIME);
        assert!(
            matches!(outcome, StepOutcome::Advanced),
            "U 模式三条链齐了 ⇒ ⇒ `time` **读得到**"
        );
        assert_eq!(value, 10, "读到的必须是**平台的真值**");
    }

    // ---- M 模式：不需要使能链 ----
    {
        let mut state = SimState::default();
        priv_set(&mut state, PRIV_M);
        let (outcome, value) = driver.guest_read(&mut state, CSR_TIME);
        assert!(
            matches!(outcome, StepOutcome::Advanced),
            "M 模式**不需要**使能链 ⇒ ⇒ `time` 读得到"
        );
        assert_eq!(value, 10, "读到的必须是**平台的真值**");
    }
}

// ---------------------------------------------------------------------------
// 护栏：`time` **永远不从 CSR 文件读**（裁决 H · 验收 B-6）
// ---------------------------------------------------------------------------

/// 畸形载荷把 `time` 塞进 CSR 文件 ⇒ ⇒ 读回来的**仍然**是**时钟给的数**。
///
/// ⚠️ 这是**裁决 H** 的护栏：改后规则是「**有真值源就报真值，没有就报 0**」
/// ⇒ ⇒ 「**永远不从 CSR 文件读**」**没被删**。
#[test]
fn a_malformed_payload_cannot_inject_a_time_value() {
    let clint = clint();
    advance(&clint, ONE_MS_PS);
    let mut driver = Driver::new(Some(clint_time_source(&clint)));
    let mut state = SimState::default();

    // 畸形载荷：CSR 文件里**有** `time` 那一条（值 = 0xDEAD_BEEF）。
    state.csrs.push((CSR_TIME, 0xDEAD_BEEF));

    let via_csr = driver.guest_read(&mut state, CSR_TIME).1;
    assert_eq!(
        via_csr, 10,
        "文件里那个值**不许**被读出来 ⇒ ⇒ `time` 永远只认**真值源**"
    );

    // 没有真值源时 ⇒ ⇒ 报 **0**（也不是文件里的那个数）。
    let mut driver_without_clock = Driver::new(None);
    let mut state_without_clock = SimState::default();
    state_without_clock.csrs.push((CSR_TIME, 0xDEAD_BEEF));
    let via_csr = driver_without_clock
        .guest_read(&mut state_without_clock, CSR_TIME)
        .1;
    assert_eq!(
        via_csr, 0,
        "没有真值源 ⇒ ⇒ 报 **0** ⇒ ⇒ 文件里的值**仍然**读不出来"
    );
}

// ---------------------------------------------------------------------------
// 两条派发路径**同口径**（验收 B-7 · D-11 的延续）
// ---------------------------------------------------------------------------

/// 闭包口与总线口读到的 `time` **逐字相同** ⇒ ⇒ **同一个时钟**。
#[test]
fn the_two_dispatch_routes_see_the_same_clock() {
    let clint = clint();
    advance(&clint, ONE_MS_PS);

    // ---- 闭包口 ----
    let mut driver = Driver::new(Some(clint_time_source(&clint)));
    let mut state = SimState::default();
    let via_closure = driver.guest_read(&mut state, CSR_TIME).1;

    // ---- 总线口 ----
    let mut iss = Iss::default().with_time_source(clint_time_source(&clint));
    let mut bus = Bus::new();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&csrr(CSR_TIME, 2).to_le_bytes());
    bus.map_ram(0, 0x1000, &bytes).expect("map program");
    let mut bus_state = SimState {
        pc: 0,
        ..SimState::default()
    };
    let outcome = iss
        .step_one_via_bus(&mut bus_state, &mut bus)
        .expect("step via bus");
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "总线口那条路必须**正常退休**"
    );
    let via_bus = bus_state.gpr[2];

    assert_eq!(via_closure, via_bus, "两条派发路径**必须同口径**");
    assert_eq!(via_bus, 10, "两条路都必须看到**同一个时钟**");
}

// ---------------------------------------------------------------------------
// M-17 的**调试读**与 guest 读**同源**（验收 B-9）
// ---------------------------------------------------------------------------

/// M-17 的**调试读** CSR 与 guest 读 `time` **同源** ⇒ ⇒ **逐字相同**。
///
/// ⚠️ 判别力：调试读走的是 [`SelfIssBackend::read_guest_csr`] ⇒ ⇒ 它必须
/// 与 guest 那条路**用同一个时钟** ⇒ ⇒ 若调试读用的是 `csr_read`（薄封装，
/// `time_ticks = 0`）⇒ ⇒ 这里**必然对不上** ⇒ ⇒ 当场红。
#[test]
fn the_debug_view_and_the_guest_read_the_same_clock() {
    use crate::m17_backend::SelfIssBackend;

    let clint = clint();
    advance(&clint, ONE_MS_PS);

    // 一个**没有**装时钟的后端 ⇒ ⇒ 调试读报 0（与 guest 的 `None` 分支
    // **同口径**）。
    let mut backend = SelfIssBackend::new(NullSinks, NullSinks);
    assert_eq!(
        backend.read_guest_csr(CSR_TIME),
        0,
        "没有时钟 ⇒ ⇒ 调试读报 **0**"
    );

    // 装上**同一个** CLINT ⇒ ⇒ 调试读报**真值**。
    backend.install_time_source(Some(clint_time_source(&clint)));
    assert_eq!(
        backend.read_guest_csr(CSR_TIME),
        10,
        "调试读必须与 guest 读**同源** ⇒ ⇒ 同一个时钟的同一个读数"
    );
    assert_eq!(
        backend.read_guest_csr(CSR_TIMEH),
        clint
            .borrow()
            .read(MTIME_HIGH, WriteSize::Word)
            .expect("mtime high"),
        "高半部也必须**同源**"
    );
}

/// 一个**什么都不做**的事件汇与中断汇 —— 测试用。
#[derive(Debug, Clone, Copy)]
struct NullSinks;

impl riscv_sim_contract::port::EventBus for NullSinks {
    fn publish(
        &mut self,
        _evt: riscv_sim_contract::protocol::EventPayload,
    ) -> Result<(), PlatformError> {
        Ok(())
    }
}

impl riscv_sim_contract::port::InterruptSink for NullSinks {
    fn raise(&mut self, _irq: u16) -> Result<(), PlatformError> {
        Ok(())
    }
    fn lower(&mut self, _irq: u16) -> Result<(), PlatformError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// `csr_read_with_time` 这一个落点本身
// ---------------------------------------------------------------------------

/// 「`time` 报什么」这条规则**只有 `csr_read_with_time` 这一个落点** ⇒ ⇒
/// `csr_read`（薄封装）报 **0**，而带真值的那个报**真值**。
#[test]
fn the_thin_wrapper_reports_zero_and_the_real_one_reports_the_truth() {
    let state = SimState::default();
    assert_eq!(
        csr_read_with_time(&state, CSR_TIME, 0x1234_5678_9ABC_DEF0),
        0x9ABC_DEF0,
        "带真值 ⇒ ⇒ 报**低 32 位**"
    );
    assert_eq!(
        csr_read_with_time(&state, CSR_TIMEH, 0x1234_5678_9ABC_DEF0),
        0x1234_5678,
        "`timeh` 取**高 32 位**"
    );
    assert_eq!(
        crate::m13_iss::csr::csr_read(&state, CSR_TIME),
        0,
        "薄封装（`time_ticks = 0`）⇒ ⇒ 报 **0**"
    );
}
