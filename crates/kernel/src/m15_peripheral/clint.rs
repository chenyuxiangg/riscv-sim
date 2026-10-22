//! CLINT — Core Local Interruptor（QEMU virt 布局，v0.23 批 Lane C）。
//!
//! # 寄存器布局（QEMU virt / osz 平台头）
//!
//! | 寄存器 | 偏移 | 宽度 | osz 依据 |
//! | --- | --- | --- | --- |
//! | `msip` | `0x0` + 4×hart | 32 位 | osz `clint.h`：`CLINT_MSIP(hart)` |
//! | `mtimecmp` | `0x4000` + 8×hart | 64 位 | osz `clint.h`：`CLINT_MTIMECMP(hart)` |
//! | `mtime` | `0xBFF8` | 64 位 | osz `clint.h`：`CLINT_MTIME` |
//!
//! 基址 `0x02000000`（QEMU virt 的 `VIRT_CLINT_BASE`）。
//!
//! # 三条必须做对的细节
//!
//! 1. **`mtimecmp` 是 64 位，而 osz 的写法是「分步」的**——先写低位
//!    `0xFFFF_FFFF`（把比较器推到极大、避免更新过程里误触发），再写
//!    真正的低位，最后写高位。本模型**按半字组合**（低 32 位写只动低半、
//!    高 32 位写只动高半），因此这种分步写法天然被接受，最后一次写完
//!    立刻生效（见 [`Clint::mtimecmp_cycles`]）。
//! 2. **`mtime` / `mtimecmp` 都是 64 位访问**。契约层的
//!    [`WriteSize`](riscv_sim_contract::model::WriteSize) 只有
//!    Byte / Half / Word 三档，**没有 8 字节**；RV32 的 64 位访存本身就
//!    是两次 32 位访问（guest 的 `*(volatile UINT64*)` 编译成两条
//!    32 位 load/store），所以本模型按**两个 32 位半字**暴露 64 位寄存器
//!    ——**不动契约层**（取舍见文件末尾「64 位访问的取舍」）。
//! 3. **时间单位是「周期」不是「皮秒」**。osz 平台头
//!    `SYSTEM_BASE_FREQ = 10000000`（10 MHz）⇒ 1 周期 = 100 000 ps。
//!    本平台的虚拟时间是 ps ⇒ 换算 `cycles = ps × freq / 1e12`、
//!    `ps = cycles × 1e12 / freq`，**频率是构造参数**（默认 10 MHz），
//!    不写死。
//!
//! # 边界
//!
//! CLINT **只暴露状态与寄存器语义**，不直接调用 M-16（M-15 的外设一律
//! 「暴露状态、由宿主驱动」，同 `gpio.rs` / `timer.rs`）。宿主拿
//! [`Clint::mtimecmp_ps`] 与 [`Clint::timer_expired`] 自己去 M-16 抬线。
//!
//! ⚠️ 本外设**不参与会话快照**：契约层的 `PeripheralSessionState` 目前
//! 只有 uart / gpio / timer 三个变体，加 CLINT/PLIC 要动契约层 ⇒ 本批
//! 用契约层的默认实现（`session_state()` 返回 `None`）。

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{PendingWrite, WriteSize};
use riscv_sim_contract::port::{Peripheral, ProbeSink};

use super::reg::{extract_subword, register_offset};
use super::ProbeSlot;

/// QEMU virt 的 CLINT 基址（`VIRT_CLINT_BASE`）。
pub const CLINT_BASE: u32 = 0x0200_0000;

/// CLINT 的寄存器窗口大小（64 KiB）。
///
/// ⚠️ 必须容得下 `mtime`（`0xBFF8` + 4 字节）⇒ 窗口至少到 `0xBFFC`。
pub const CLINT_WINDOW: u32 = 0x1_0000;

/// 默认时钟频率：10 MHz（osz 平台头 `SYSTEM_BASE_FREQ = 10000000`）。
///
/// ⚠️ 这是**默认值**，不是常量约束：频率是构造参数（见
/// [`Clint::with_frequency`]），因为「1 周期 = 100 000 ps」只在 10 MHz
/// 下成立。
pub const DEFAULT_FREQ_HZ: u64 = 10_000_000;

/// 一秒 = 10^12 ps（平台的虚拟时间单位是 ps）。
const PS_PER_SEC: u128 = 1_000_000_000_000;

/// 探针信号名：软件中断挂起位（bit 0）。
pub const SIGNAL_MSIP: &str = "MSIP";

/// 探针信号名：定时器比较值（周期）。
pub const SIGNAL_MTIMECMP: &str = "MTIMECMP";

/// 寄存器偏移（**相对基址**）。
mod reg {
    /// `msip` 数组基址：`0x0` + 4×hart（⇒ 数组起点就是 0，用不着单独
    /// 一个常量；下面的 `MSIP_STRIDE` 才是它的间距）。
    pub const MSIP_STRIDE: u32 = 4;
    /// `mtimecmp` 数组基址：`0x4000` + 8×hart。
    pub const MTIMECMP: u32 = 0x4000;
    /// `mtime`：`0xBFF8`（低 32 位）/ `0xBFFC`（高 32 位）。
    pub const MTIME: u32 = 0xBFF8;
    /// 每个 hart 的 `mtimecmp` 间距（8 字节）。
    pub const MTIMECMP_STRIDE: u32 = 8;
}

/// 「这个偏移没有对应的寄存器」——平台的**外设错误族**。
///
/// ⚠️ 族 / 场景码与 `gpio.rs` / `uart.rs` / `timer.rs` 完全一致
/// （`ErrorFamily::Peripheral` + 场景码 `2`）：同一个含义不能有两个码。
fn undefined_offset(off: u32) -> PlatformError {
    PlatformError::new(
        ErrorFamily::Peripheral,
        2,
        format!("CLINT offset 0x{:x} not defined", off),
    )
}

/// CLINT（核心本地中断器）——QEMU virt 布局。
#[derive(Debug)]
pub struct Clint {
    base: u32,
    id: String,
    /// 本平台的 hart 数量（`msip` / `mtimecmp` 各一份）。
    harts: u32,
    /// 时钟频率（Hz）——决定「周期 ↔ ps」的换算。
    freq_hz: u64,
    /// 每个 hart 的 `msip`（原始 32 位值；**bit 0 = 挂起**）。
    msip: Vec<u32>,
    /// 每个 hart 的 `mtimecmp`（**周期**）。
    mtimecmp: Vec<u64>,
    /// `mtime` 的锚点：`(锚点虚拟时间 ps, 锚点周期值)`。
    ///
    /// `mtime` 是**自由运行的计数器**，真相源是虚拟时间 ⇒
    /// `mtime = 锚点周期值 + cycles(now_ps - 锚点 ps)`。
    /// 锚点默认 `(0, 0)` ⇒ `mtime` 就是 `cycles(now_ps)`（自动路径）。
    /// 宿主用 [`Clint::set_mtime_cycles`] 喂值时锚点被挪到当前时刻 ⇒
    /// 那个值在之后的 tick 里**不会被冲掉**（快照恢复正是这个场景：
    /// 恢复出来的 `mtime` 必须从恢复点继续走，而不是被下一次 tick 拉回
    /// 「虚拟时间的函数」）。
    mtime_anchor: (u64, u64),
    /// 最近一次观察到的虚拟时间（ps）。
    now_ps: u64,
    /// M-18 探针回调（D-12）。空 = 没有观察者。
    probe: ProbeSlot,
}

impl Clint {
    /// 单 hart、10 MHz 的 CLINT。
    pub fn new(id: impl Into<String>, base: u32) -> Self {
        Self::with_harts(id, base, 1)
    }

    /// 指定 hart 数量的 CLINT（频率取默认 10 MHz）。
    pub fn with_harts(id: impl Into<String>, base: u32, harts: u32) -> Self {
        Self::with_frequency(id, base, harts, DEFAULT_FREQ_HZ)
    }

    /// 指定 hart 数量与时钟频率的 CLINT。
    ///
    /// ⚠️ `freq_hz == 0` 没有物理意义（换算会除零）⇒ 这里**钳到 1 Hz**
    /// 而不是 panic：平台的规矩是「不许 panic」。
    pub fn with_frequency(id: impl Into<String>, base: u32, harts: u32, freq_hz: u64) -> Self {
        let harts = harts.max(1);
        Self {
            base,
            id: id.into(),
            harts,
            freq_hz: freq_hz.max(1),
            msip: vec![0; harts as usize],
            mtimecmp: vec![0; harts as usize],
            mtime_anchor: (0, 0),
            now_ps: 0,
            probe: ProbeSlot::default(),
        }
    }

    // -----------------------------------------------------------------------
    // 宿主侧接口
    // -----------------------------------------------------------------------

    /// 本平台的 hart 数量。
    pub fn harts(&self) -> u32 {
        self.harts
    }

    /// 时钟频率（Hz）。
    pub fn freq_hz(&self) -> u64 {
        self.freq_hz
    }

    /// 一个周期折合多少 ps（`1e12 / freq`）。
    ///
    /// ⚠️ 这是**四舍五入到整 ps** 的近似值（10 MHz ⇒ 恰好 100 000 ps）。
    /// 精确换算走 [`Clint::cycles_from_ps`] / [`Clint::ps_from_cycles`]。
    pub fn ps_per_cycle(&self) -> u64 {
        (PS_PER_SEC / self.freq_hz as u128).min(u64::MAX as u128) as u64
    }

    /// 虚拟时间（ps）→ 周期。
    pub fn cycles_from_ps(&self, ps: u64) -> u64 {
        ((ps as u128 * self.freq_hz as u128) / PS_PER_SEC).min(u64::MAX as u128) as u64
    }

    /// 周期 → 虚拟时间（ps）。
    ///
    /// ⚠️ 溢出时**饱和**到 `u64::MAX`（平台的规矩：不许 panic）。
    pub fn ps_from_cycles(&self, cycles: u64) -> u64 {
        ((cycles as u128 * PS_PER_SEC) / self.freq_hz as u128).min(u64::MAX as u128) as u64
    }

    /// 当前 `mtime`（**周期**）。
    pub fn mtime_cycles(&self) -> u64 {
        let elapsed_ps = self.now_ps.saturating_sub(self.mtime_anchor.0);
        self.mtime_anchor
            .1
            .saturating_add(self.cycles_from_ps(elapsed_ps))
    }

    /// 当前 `mtime` 折合的虚拟时间（ps）。
    pub fn mtime_ps(&self) -> u64 {
        self.ps_from_cycles(self.mtime_cycles())
    }

    /// 宿主喂 `mtime`（**周期**）。
    ///
    /// 喂进去的值**立即生效**，并且**不会被之后的 tick 冲掉**：锚点被挪到
    /// 当前虚拟时间，计数器从那个值继续走。
    ///
    /// ⚠️ 下一次 `tick` 之后 `mtime` 会继续按虚拟时间推进（锚点不动），
    /// 所以「喂一个绝对值」与「让 `mtime` 跟着虚拟时间走」不冲突。
    pub fn set_mtime_cycles(&mut self, cycles: u64) {
        self.mtime_anchor = (self.now_ps, cycles);
    }

    /// 最近一次观察到的虚拟时间（ps）。
    pub fn now_ps(&self) -> u64 {
        self.now_ps
    }

    /// hart 0 的 `mtimecmp`（**周期**）——guest 写进来的值，立即生效。
    pub fn mtimecmp_cycles(&self) -> u64 {
        self.mtimecmp_cycles_of(0).unwrap_or(0)
    }

    /// 指定 hart 的 `mtimecmp`（**周期**）。hart 越界 → `None`。
    pub fn mtimecmp_cycles_of(&self, hart: u32) -> Option<u64> {
        self.mtimecmp.get(hart as usize).copied()
    }

    /// hart 0 的 `mtimecmp` 折合的虚拟时间（ps）——宿主拿它跟 `now_ps` 比。
    pub fn mtimecmp_ps(&self) -> u64 {
        self.mtimecmp_ps_of(0).unwrap_or(0)
    }

    /// 指定 hart 的 `mtimecmp` 折合的虚拟时间（ps）。hart 越界 → `None`。
    pub fn mtimecmp_ps_of(&self, hart: u32) -> Option<u64> {
        self.mtimecmp
            .get(hart as usize)
            .copied()
            .map(|c| self.ps_from_cycles(c))
    }

    /// hart 0 的软件中断是否挂起。
    pub fn msip_pending(&self) -> bool {
        self.msip_pending_of(0).unwrap_or(false)
    }

    /// 指定 hart 的软件中断是否挂起。hart 越界 → `None`。
    pub fn msip_pending_of(&self, hart: u32) -> Option<bool> {
        self.msip.get(hart as usize).map(|v| v & 1 == 1)
    }

    /// 指定 hart 的 `msip` 原始寄存器值。
    pub fn msip_value_of(&self, hart: u32) -> Option<u32> {
        self.msip.get(hart as usize).copied()
    }

    /// hart 0 的定时器是否到期（`mtime >= mtimecmp`）。
    ///
    /// ⚠️ `mtimecmp == 0` 视为「**没有期限**」（与 M-15 既有
    /// `timer.rs` 的口径一致：`mtimecmp > 0` 才 armed）——复位后
    /// `mtimecmp` 是 0，若当成「立即到期」复位就会自己狂打定时器中断。
    pub fn timer_expired(&self) -> bool {
        self.timer_expired_of(0).unwrap_or(false)
    }

    /// 指定 hart 的定时器是否到期。hart 越界 → `None`。
    pub fn timer_expired_of(&self, hart: u32) -> Option<bool> {
        let cmp = *self.mtimecmp.get(hart as usize)?;
        Some(cmp > 0 && self.mtime_cycles() >= cmp)
    }

    /// 上报一次探针观测（M-18 管道）。
    fn report(&mut self, signal: &str, value: u64) {
        let now = self.now_ps;
        self.probe.report(signal, value, now);
    }
}

/// 寄存器读写（`register_offset` / `extract_subword`）在 `super::reg`
///（M-15 review N1 的 DRY 合并）。
impl Peripheral for Clint {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_addr(&self) -> u32 {
        self.base
    }
    fn size(&self) -> u32 {
        CLINT_WINDOW
    }

    fn read(&self, offset: u32, size: WriteSize) -> Result<u32, PlatformError> {
        let word = self.read_word(register_offset(offset))?;
        Ok(extract_subword(word, offset, size))
    }

    fn write(&mut self, offset: u32, _size: WriteSize, value: u32) -> Result<(), PlatformError> {
        let off = register_offset(offset);

        // ---- `msip` 数组：`0x0` + 4×hart ----
        if off < reg::MTIMECMP {
            let hart = (off / reg::MSIP_STRIDE) as usize;
            let slot = match self.msip.get_mut(hart) {
                Some(s) => s,
                None => return Err(undefined_offset(off)),
            };
            let prev = *slot;
            // osz 的写法是「写 1 挂起、写 0 清除」⇒ 挂起位是 bit 0。
            // 这里存**原始 32 位值**（QEMU 的 clint 也是这么存的），
            // 挂起与否由 bit 0 判定。
            *slot = value;
            let pending = u64::from(value & 1);
            if (prev & 1) != (value & 1) {
                self.report(SIGNAL_MSIP, pending);
            }
            return Ok(());
        }

        // ---- `mtimecmp` 数组：`0x4000` + 8×hart（低 / 高各 4 字节）----
        if off < reg::MTIME {
            let rel = off - reg::MTIMECMP;
            let hart = (rel / reg::MTIMECMP_STRIDE) as usize;
            let half = rel % reg::MTIMECMP_STRIDE;
            let slot = match self.mtimecmp.get_mut(hart) {
                Some(s) => s,
                None => return Err(undefined_offset(off)),
            };
            let prev = *slot;
            // ⚠️ **按半字组合**：低 32 位写只动低半、高 32 位写只动高半。
            // 这正是 osz 的「先写低位 -1、再写低位、再写高位」能被正确
            // 拼出 64 位值的原因（也兼容「一次写全 8 字节」被拆成两条
            // 32 位访问的访存形态）。
            match half {
                0 => *slot = (*slot & 0xFFFF_FFFF_0000_0000) | u64::from(value),
                4 => *slot = (*slot & 0x0000_0000_FFFF_FFFF) | (u64::from(value) << 32),
                _ => return Err(undefined_offset(off)),
            }
            let new = *slot;
            if new != prev {
                self.report(SIGNAL_MTIMECMP, new);
            }
            return Ok(());
        }

        // ---- `mtime`：**只读**（guest 写它没有效果）----
        //
        // 与 `timer.rs` 的 `MTIME_*` 同一口径：`mtime` 归引擎 / 宿主管，
        // guest 只读不写。
        if off == reg::MTIME || off == reg::MTIME + 4 {
            return Ok(());
        }

        Err(undefined_offset(off))
    }

    fn tick(&mut self, now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        // 引擎拥有虚拟时钟（M-12）；`tick` 是它触达外设的路径。
        // 这里只**记住时间**——`mtime` 是 `now_ps` 的函数（见
        // `mtime_anchor`），不在 tick 里另存一份，免得两个真相源漂移。
        self.now_ps = now_ps;
        Ok(Vec::new())
    }

    /// 装上 / 摘掉 M-18 探针回调（D-12）。
    fn on_probe(&mut self, cb: Option<ProbeSink>) {
        self.probe.set(cb);
    }
}

impl Clint {
    /// 读一个字（32 位对齐）寄存器。
    fn read_word(&self, off: u32) -> Result<u32, PlatformError> {
        // ---- `msip` 数组 ----
        if off < reg::MTIMECMP {
            let hart = (off / reg::MSIP_STRIDE) as usize;
            return self
                .msip
                .get(hart)
                .copied()
                .ok_or_else(|| undefined_offset(off));
        }

        // ---- `mtimecmp` 数组 ----
        if off < reg::MTIME {
            let rel = off - reg::MTIMECMP;
            let hart = (rel / reg::MTIMECMP_STRIDE) as usize;
            let half = rel % reg::MTIMECMP_STRIDE;
            let cmp = *self
                .mtimecmp
                .get(hart)
                .ok_or_else(|| undefined_offset(off))?;
            return Ok(match half {
                0 => cmp as u32,
                4 => (cmp >> 32) as u32,
                _ => return Err(undefined_offset(off)),
            });
        }

        // ---- `mtime` ----
        match off {
            reg::MTIME => Ok(self.mtime_cycles() as u32),
            o if o == reg::MTIME + 4 => Ok((self.mtime_cycles() >> 32) as u32),
            _ => Err(undefined_offset(off)),
        }
    }
}

// ---------------------------------------------------------------------------
// 64 位访问的取舍（写在这里，免得以后有人「顺手优化」掉）
// ---------------------------------------------------------------------------
//
// 契约层的 [`WriteSize`](riscv_sim_contract::model::WriteSize) 只有
// Byte / Half / Word，**没有 8 字节**。加一档 `Double` 会动契约层
// （契约层是所有模块的公共词汇表，改它要统筹方拍板），本批**不动**。
//
// 取舍：**用两次 32 位访问拼出 64 位**。
//
// - **为什么够用**：RV32 **没有 64 位访存指令**。guest 的
//   `*(volatile UINT64*)` 必然被编译成两条 32 位 load/store ⇒ 总线
//   （M-14，宽度上限 32 位）交给外设的**本来就是**两次 32 位访问。
//   换句话说，「8 字节访问」在 RV32 上**根本不会发生**——契约层不加档
//   不是能力缺失，是物理事实。
// - **代价**：宿主若想**一次**写完整 64 位，只能写两趟（低半、高半）。
//   这是 RV32 的固有限制，不是本模型的缺陷。
// - **顺带的好处**：osz 的「分步写」（先写低位 -1、再写低位、再写高位）
//   与「一次性写」在**同一个**按半字组合的实现里都能正确工作——不需要
//   两套路径，也就不需要「怎么判断这次写是不是最后一步」这种脆弱逻辑。

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// 记录探针观测的日志。
    type ProbeLog = Rc<RefCell<Vec<(String, u64, u64)>>>;

    fn recording_probe() -> (ProbeLog, ProbeSink) {
        let log: ProbeLog = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&log);
        (
            log,
            Box::new(move |signal: &str, value: u64, now_ps: u64| {
                sink.borrow_mut().push((signal.to_string(), value, now_ps));
            }),
        )
    }

    /// osz 的 `mtimecmp` 分步写：先写低位 `-1`，再写真正的低位，最后写高位。
    #[test]
    fn mtimecmp_stepwise_write_composes_the_full_64_bit_value() {
        let mut clint = Clint::new("CLINT0", CLINT_BASE);

        // ① 先写低位 = -1（osz 的写法：把比较器推到极大，避免更新过程误触发）
        clint
            .write(reg::MTIMECMP, WriteSize::Word, 0xFFFF_FFFF)
            .unwrap();
        assert_eq!(
            clint.mtimecmp_cycles(),
            0x0000_0000_FFFF_FFFF,
            "第一步之后，低半必须已经落地"
        );

        // ② 再写真正的低位
        clint
            .write(reg::MTIMECMP, WriteSize::Word, 0x89AB_CDEF)
            .unwrap();
        assert_eq!(
            clint.mtimecmp_cycles(),
            0x0000_0000_89AB_CDEF,
            "第二步之后，高半必须还是原值（不能被冲掉）"
        );

        // ③ 最后写高位
        clint
            .write(reg::MTIMECMP + 4, WriteSize::Word, 0x0123_4567)
            .unwrap();
        assert_eq!(
            clint.mtimecmp_cycles(),
            0x0123_4567_89AB_CDEF,
            "分步写必须拼出完整的 64 位值"
        );

        // 寄存器读回也是同一个值（低半 / 高半）。
        assert_eq!(
            clint.read(reg::MTIMECMP, WriteSize::Word).unwrap(),
            0x89AB_CDEF,
            "低半读回"
        );
        assert_eq!(
            clint.read(reg::MTIMECMP + 4, WriteSize::Word).unwrap(),
            0x0123_4567,
            "高半读回"
        );
    }

    /// guest 写完 `mtimecmp` **立刻生效**（不等下一次 tick）。
    #[test]
    fn mtimecmp_takes_effect_immediately_after_the_guest_write() {
        let mut clint = Clint::new("CLINT0", CLINT_BASE);
        clint
            .write(reg::MTIMECMP, WriteSize::Word, 0x0001_0000)
            .unwrap();
        clint
            .write(reg::MTIMECMP + 4, WriteSize::Word, 0x0000_0000)
            .unwrap();

        assert_eq!(clint.mtimecmp_cycles(), 0x0000_0000_0001_0000);
        // 10 MHz ⇒ 1 周期 = 100 000 ps ⇒ 65536 周期 = 6 553 600 000 ps。
        assert_eq!(clint.mtimecmp_ps(), 6_553_600_000);
        assert_eq!(clint.cycles_from_ps(6_553_600_000), 0x0001_0000);
    }

    /// `mtime` 的 64 位读：与宿主喂进来的周期数一致。
    #[test]
    fn mtime_read_matches_the_cycle_count_the_host_fed_in() {
        let mut clint = Clint::new("CLINT0", CLINT_BASE);

        clint.set_mtime_cycles(0x0000_0001_2345_6789);
        assert_eq!(
            clint.read(reg::MTIME, WriteSize::Word).unwrap(),
            0x2345_6789,
            "mtime 低 32 位"
        );
        assert_eq!(
            clint.read(reg::MTIME + 4, WriteSize::Word).unwrap(),
            0x0000_0001,
            "mtime 高 32 位"
        );
        assert_eq!(clint.mtime_cycles(), 0x0000_0001_2345_6789);
    }

    /// `mtime` 跟着虚拟时间走（10 MHz ⇒ 100 000 ps 一个周期）。
    #[test]
    fn mtime_follows_the_virtual_clock() {
        let mut clint = Clint::new("CLINT0", CLINT_BASE);

        clint.tick(100_000).unwrap();
        assert_eq!(clint.mtime_cycles(), 1, "100 000 ps = 1 个周期");

        clint.tick(1_000_000).unwrap();
        assert_eq!(clint.mtime_cycles(), 10, "1 000 000 ps = 10 个周期");

        assert_eq!(
            clint.read(reg::MTIME + 4, WriteSize::Word).unwrap(),
            0,
            "高 32 位此刻仍是 0"
        );
    }

    /// 宿主喂进去的 `mtime` 不会被之后的 tick 冲掉（快照恢复场景）。
    #[test]
    fn a_host_fed_mtime_survives_later_ticks() {
        let mut clint = Clint::new("CLINT0", CLINT_BASE);
        clint.set_mtime_cycles(0x0000_1000);
        assert_eq!(clint.mtime_cycles(), 0x0000_1000);

        clint.tick(100_000).unwrap();
        assert_eq!(
            clint.mtime_cycles(),
            0x0000_1001,
            "喂进去的值必须从喂入点继续走，而不是被拉回 0"
        );
    }

    /// `msip`：写 1 挂起、写 0 清除。
    #[test]
    fn msip_write_one_pends_and_write_zero_clears() {
        let mut clint = Clint::new("CLINT0", CLINT_BASE);
        assert!(!clint.msip_pending(), "复位时不得挂起");

        // `msip` 数组基址 = 0x0（`0x0` + 4×hart）。
        clint.write(0x0, WriteSize::Word, 1).unwrap();
        assert!(clint.msip_pending(), "写 1 必须挂起");

        clint.write(0x0, WriteSize::Word, 0).unwrap();
        assert!(!clint.msip_pending(), "写 0 必须清除");
    }

    /// `msip` 是**每个 hart 一份**的（偏移 `0x0` + 4×hart）。
    #[test]
    fn msip_is_one_register_per_hart() {
        let mut clint = Clint::with_harts("CLINT0", CLINT_BASE, 2);

        clint.write(0x0, WriteSize::Word, 1).unwrap();
        assert!(clint.msip_pending_of(0).unwrap());
        assert!(
            !clint.msip_pending_of(1).unwrap(),
            "hart 1 不该被 hart 0 的写影响"
        );

        clint.write(0x4, WriteSize::Word, 1).unwrap();
        assert!(clint.msip_pending_of(1).unwrap(), "hart 1 挂在 0x4");
    }

    /// 非法 / 未映射的偏移必须报**平台的错误族**，不许 panic、不许静默。
    #[test]
    fn undefined_offset_reports_the_peripheral_error_family() {
        let mut clint = Clint::new("CLINT0", CLINT_BASE);

        let err = clint
            .read(0x1234, WriteSize::Word)
            .expect_err("未映射的偏移必须报错");
        assert_eq!(err.family, ErrorFamily::Peripheral, "族必须是 PER");
        assert_eq!(err.scenario, 2, "场景码必须与 gpio/uart/timer 一致");

        let err = clint
            .write(0x1234, WriteSize::Word, 0)
            .expect_err("未映射的偏移必须报错");
        assert_eq!(err.family, ErrorFamily::Peripheral);

        // `mtimecmp` 数组之外（hart 越界）也是未映射。
        let err = clint
            .read(reg::MTIMECMP + 8, WriteSize::Word)
            .expect_err("hart 越界的 mtimecmp 必须报错");
        assert_eq!(err.family, ErrorFamily::Peripheral);
    }

    /// 频率是**可配置**的：1 MHz ⇒ 1 周期 = 1 000 000 ps。
    #[test]
    fn the_cycle_to_ps_conversion_follows_the_configured_frequency() {
        let mut clint = Clint::with_frequency("CLINT0", CLINT_BASE, 1, 1_000_000);
        assert_eq!(clint.ps_per_cycle(), 1_000_000);

        clint
            .write(reg::MTIMECMP, WriteSize::Word, 0x0001_0000)
            .unwrap();
        assert_eq!(
            clint.mtimecmp_ps(),
            65_536_000_000,
            "1 MHz 下 65536 周期 = 65 536 000 000 ps"
        );

        // 默认 10 MHz ⇒ 同样的周期数换算结果不同。
        let mut def = Clint::new("CLINT0", CLINT_BASE);
        def.write(reg::MTIMECMP, WriteSize::Word, 0x0001_0000)
            .unwrap();
        assert_eq!(def.ps_per_cycle(), 100_000);
        assert_eq!(def.mtimecmp_ps(), 6_553_600_000);
    }

    /// `mtime` 对 guest 是**只读**的（guest 写它不改变任何东西）。
    #[test]
    fn mtime_is_read_only_for_the_guest() {
        let mut clint = Clint::new("CLINT0", CLINT_BASE);
        clint.set_mtime_cycles(0x0000_0000_0000_1234);

        clint
            .write(reg::MTIME, WriteSize::Word, 0xFFFF_FFFF)
            .unwrap();
        clint
            .write(reg::MTIME + 4, WriteSize::Word, 0xFFFF_FFFF)
            .unwrap();

        assert_eq!(
            clint.mtime_cycles(),
            0x0000_0000_0000_1234,
            "guest 写 mtime 不得改变它"
        );
    }

    /// 宿主用 `timer_expired` 判断「定时器到期没有」（宿主拿它去抬 M-16 的线）。
    #[test]
    fn the_host_can_ask_whether_the_timer_expired() {
        let mut clint = Clint::new("CLINT0", CLINT_BASE);

        // `mtimecmp == 0` = 没有期限（与 M-15 `timer.rs` 同一口径）。
        assert!(!clint.timer_expired(), "复位后不得立刻到期");

        clint
            .write(reg::MTIMECMP, WriteSize::Word, 0x0001_0000)
            .unwrap();
        clint
            .write(reg::MTIMECMP + 4, WriteSize::Word, 0x0000_0000)
            .unwrap();
        assert_eq!(clint.mtimecmp_cycles(), 0x0000_0000_0001_0000);

        clint.tick(100_000).unwrap();
        assert!(!clint.timer_expired(), "还没到期限");

        // 1 000 000 个周期 > 65 536 个周期的期限 ⇒ 到期。
        clint.tick(100_000_000_000).unwrap();
        assert_eq!(clint.mtime_cycles(), 1_000_000);
        assert!(clint.timer_expired(), "过了期限必须报到期");
    }

    /// 探针管道能看到 `msip` / `mtimecmp` 的变化（D-12 口径）。
    #[test]
    fn the_probe_pipeline_sees_msip_and_mtimecmp_changes() {
        let (log, sink) = recording_probe();
        let mut clint = Clint::new("CLINT0", CLINT_BASE);
        clint.on_probe(Some(sink));
        clint.tick(1_000).unwrap();

        clint.write(0x0, WriteSize::Word, 1).unwrap();
        clint
            .write(reg::MTIMECMP, WriteSize::Word, 0x0000_0010)
            .unwrap();

        let log = log.borrow();
        assert_eq!(log.len(), 2, "两处变化各上报一次");
        assert_eq!(log[0].0, SIGNAL_MSIP);
        assert_eq!(log[0].1, 1, "msip 上报的是挂起位");
        assert_eq!(log[0].2, 1_000, "时间戳是外设的时钟");
        assert_eq!(log[1].0, SIGNAL_MTIMECMP);
        assert_eq!(log[1].1, 0x0000_0010);
    }
}
