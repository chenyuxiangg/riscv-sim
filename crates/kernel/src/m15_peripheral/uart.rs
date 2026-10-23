//! Uart16550 — **ns16550-compatible** UART with a **lossless TX capture ring**
//! (v0.23 · Lane B).
//!
//! ## Why this file was rewritten (the old layout was *misaligned*)
//!
//! The previous model was a **custom** two-register peripheral:
//! `DATA @ 0x0` / `STATUS @ 0x4`, **word**-wide, with the STATUS bits
//! `TX_READY / RX_FULL / RX_OVERRUN / TX_OVERRUN`. The target RTOS (osz)
//! drives a **standard ns16550** whose registers are **byte** registers:
//!
//! | 寄存器 | 偏移 | osz 用途 |
//! | --- | --- | --- |
//! | THR / RHR / DLL | `0x0` | 发 / 收 / 波特率除数低 8 位 |
//! | IER / DLM | `0x1` | 中断使能 / 波特率除数高 8 位 |
//! | FCR | `0x2` | FIFO 控制 |
//! | LCR | `0x3` | 线路控制（DLAB 位在 bit7） |
//! | MCR | `0x4` | Modem 控制 |
//! | LSR | `0x5` | 线路状态（`LSR_RX_VALID` = bit0，`LSR_TX_IDLE` = bit5） |
//! | MSR | `0x6` | Modem 状态 |
//! | SCR | `0x7` | scratch |
//!
//! osz accesses them with `READ_BYTE` / `WRITE_BYTE`
//! (`*(volatile unsigned char*)`), so the old layout was not merely
//! "different" — it was **wrong**: osz's `hal_uart_set_irq_en()` writes
//! `IER @ 0x1`, which the old model took as **the second TX byte**; and
//! osz's `hal_uart_set_baudrate()` writes `DLL @ 0x0` / `DLM @ 0x1` with
//! `LCR.DLAB` set, which the old model emitted as **two garbage TX bytes**.
//!
//! ## The access rule (this is the answer to "how are the 4 bytes of a
//! 32-bit word counted?")
//!
//! In an ns16550, `0x0..=0x7` are **eight independent byte registers**
//! (`DLL = 0x0` and `THR = 0x0` are told apart by `LCR.DLAB`). There is
//! **no** 32-bit register in the file. Therefore:
//!
//! - `read(offset, size)` / `write(offset, size, value)` treat `offset`
//!   as a **byte offset** — *not* as "a word index plus a byte lane";
//! - the access covers the bytes `offset .. offset + width`; **every**
//!   covered byte must lie inside the 8-byte window, otherwise the
//!   access is rejected with `E_PER-02`;
//! - a multi-byte access is **assembled byte by byte, little-endian**:
//!   `value = b0 | b1 << 8 | b2 << 16 | b3 << 24`;
//! - a multi-byte **write** puts each byte into **its own register**
//!   (byte 0 → the register at `offset`, byte 1 → the register at
//!   `offset + 1`, …).
//!
//! ## DLAB (LCR bit 7)
//!
//! `DLAB = 1` turns `0x0` / `0x1` into `DLL` / `DLM` (the baud-rate
//! divisor latch); `DLAB = 0` turns them back into `THR` / `RHR` and
//! `IER`. The other six registers keep their meaning in both states.
//!
//! ## LSR (offset `0x5`)
//!
//! The bit meanings follow ns16550 (and the two macros osz actually
//! uses):
//!
//! | bit | 名字 | 含义 |
//! | --- | --- | --- |
//! | 0 | `LSR_RX_VALID` | RX FIFO 里有字节可读 |
//! | 1 | `LSR_THRE` | 发送保持寄存器空 —— 恒 1 |
//! | 2 | `LSR_OE` | RX overrun（**锁存**，W1C） |
//! | 3 | `LSR_TX_OVERRUN` | TX 捕获环溢出（**锁存**，W1C） |
//! | 4 | `LSR_FE` | 帧错误 —— 本模型不建模，恒 0 |
//! | 5 | `LSR_TX_IDLE` | 发送器**还能再收一个字节**（捕获环没满） |
//! | 6 | `LSR_TEMT` | 发送器空 —— 恒 1 |
//!
//! ⚠️ **bit5 的语义是 osz 的生死线**：osz 的
//! `hal_uart_putc()` 是 `while ((LSR & LSR_TX_IDLE) == 0);` —— 它在
//! 「发送器还能收一个字节」时**自旋**。所以 bit5 必须表达「**还能收**」
//! （捕获环没满），而**不是**「捕获环是空的」——后者会让 osz 每发一个
//! 字节就卡死一次，shell 的输出根本走不出去。
//!
//! ## 接收 FIFO（v0.24 · 一行 shell 输入要能**整个**送进去）
//!
//! ⚠️ **v0.23 的模型是「1 字节深」**（`rx: Option<u8>`）⇒ 宿主一条
//! `WriteConsole` 命令写三个字节，只有**第一个**进得去，后两个被丢并
//! 锁存 `LSR` 的 overrun 位 ⇒ **一行 shell 命令根本送不进去**（冻结规格
//! STAGE-9 裁决 B 第一步写的是「把**字节**塞进 UART 的 RX FIFO」——
//! 复数 ⇒ 规格假定 FIFO 能装多个字节）。
//!
//! ⇒ 现在接收侧是真正的 **FIFO**（[`RX_FIFO_CAPACITY`] 字节深）：
//!
//! - `LSR` 的 **RX_VALID(bit0)** = FIFO **非空**；
//! - guest 读 `RHR` = **弹出队头**（不是「读最后一个」，也不是「读了不清」）；
//! - [`Uart16550::receive`] = **入队尾**；
//! - FIFO **满**时**丢最新的那个**并**锁存** `LSR` 的 overrun 位（bit2，
//!   W1C）—— 与 TX 捕获环**同一口径**（见下面「环满时的策略」）。
//!
//! ⚠️ **TX 侧一个字节都没动**：无损 TX 捕获环是 v0.23 的成果。
//!
//! ## 无损 TX 捕获环
//!
//! TX 不再是 1 字节深的 FIFO，而是一个**可配置容量的环形缓冲**
//! （默认 [`DEFAULT_TX_CAPTURE_CAPACITY`] = 4 KiB）。guest 每写一次
//! `THR`，**一个字节进环**；宿主可以用 [`Uart16550::drain_tx`] **一次
//! 取走全部**已发字节。
//!
//! **不变式**：*只要环没满，guest 写进去的每一个字节都必须能被宿主取到，
//! 顺序不变、不丢不重。* 这是「两侧 UART 字节流逐字节比对」这个判决的
//! 基础。
//!
//! **环满时的策略**：**丢最新的那个字节**（保留最早的），并在 LSR 的
//! bit3 锁存一次 overrun。理由：最早的字节是 shell 输出的**前缀**，
//! 丢前缀比丢尾巴更容易诊断；而「还能收一个字节」（bit5）同时被清掉，
//! 于是遵守协议的 guest（osz 就是）会**自旋等待**，根本走不到这一步。
//! ⇒ 正常路径上环**不会**满，丢字节只可能发生在不守协议的 guest 上。
//!
//! ## 探针（D-12）
//!
//! 每**进环**一个字节，就上报**一次** [`SIGNAL_TX`] 脉冲事件，值 = 字节
//! 本身。脉冲而不是电平：两个相同的字节是**两个**事件。
//!
//! ## 会话快照（契约层的 `UartStateDto`）
//!
//! ⚠️ **v0.24 起**：契约层的 `UartStateDto` **多了 ns16550 的七个
//! 寄存器**（`LCR` / `IER` / `FCR` / `MCR` / `SCR` / `DLL` / `DLM`）
//! —— 会话快照的版本因此 **1 → 2**（破坏性变更）。快照带的是：
//!
//! - `tx` = 捕获环的**队头**（环非空时 = 那个字节；环空时 = `None`）；
//! - `status` = **LSR 的值**（ns16550 语义，`0x00..=0x7F`）；
//! - 那**七个寄存器**的**原值**（含 `LCR` 的 `DLAB` 位 —— 它决定
//!   `0x0`/`0x1` 是 `THR`/`IER` 还是 `DLL`/`DLM`）。
//!
//! ⚠️ LSR 的 bit5 是**导出值**（捕获环当时没满 ⇒ 1），而还原时环里只有
//! 一个字节 ⇒ bit5 仍是 1 ⇒ 往返**逐字段相等**。唯一的例外是「导出时
//! 环正好是满的」这一角落：那时导出的 bit5 = 0，还原后算出来是 1。

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{PendingWrite, WriteSize};
use riscv_sim_contract::port::{Peripheral, ProbeSink};
use riscv_sim_contract::session::{PeripheralSessionState, UartStateDto};

use std::collections::VecDeque;

use super::ProbeSlot;

/// Register window is 8 bytes wide — the ns16550 register file is eight
/// **byte** registers at offsets `0x0..=0x7`.
///
/// ⚠️ **不要把它改小**：osz 的 UART 基址是 `0x10000000`，寄存器窗口至少
/// 要覆盖 `0x0..=0x7`。
pub const UART_WINDOW: u32 = 0x8;

/// Default capacity of the lossless TX capture ring (4 KiB).
pub const DEFAULT_TX_CAPTURE_CAPACITY: usize = 4096;

/// 接收 FIFO 的**深度**（字节数）。
///
/// ⚠️ **为什么是 16**：接收侧的用途是「宿主把**一行** shell 输入送进
/// guest」（冻结规格 STAGE-9 裁决 B）⇒ 深度必须**够放一行**：典型命令
/// 加换行（"help\n" 是 5 字节，"version\n" 是 8 字节）⇒ 16 字节既有
/// 余量，又不会把会话快照撑大（`UartStateDto.rx_queue` 最长 16 字节 ⇒
/// 载荷的增量**有界**）。
///
/// ⚠️ **为什么不是 1**（v0.23 的旧模型）：1 字节深的 FIFO 意味着「一条
/// 命令只能送进去**一个字符**」⇒ 一行命令送不进去 ⇒ 与本批的目标
/// （「让 osz 的 shell **真的能用**」）不符。
///
/// ⚠️ **满时的策略与 TX 环一致**：**丢最新、保最早**（见
/// [`Uart16550::receive`]）⇒ 丢是**看得见的**（`LSR` 的 overrun 位
/// 被锁存），不是静默的。
pub const RX_FIFO_CAPACITY: usize = 16;

/// Probe signal name for a transmitted byte (D-12 / N5).
///
/// The reported **value** is the byte itself (0x00..=0xFF). Every
/// transmitted byte is one event, so the channel bound to this signal
/// should be a **pulse** channel (M-18 `ChannelKind::Pulse`): a
/// multi-bit channel would swallow a repeated byte (same value = no
/// edge), and two identical bytes in a row are two distinct events.
pub const SIGNAL_TX: &str = "TX";

/// ns16550 register offsets — **byte** offsets, not word indices.
///
/// ⚠️ `THR` / `RHR` / `DLL` all live at `0x0`, and `IER` / `DLM` all live
/// at `0x1`: which one a byte access sees is decided by `LCR.DLAB`.
pub mod reg {
    /// `0x0` — transmit holding register (write) / receive holding
    /// register (read) / divisor latch low (while `LCR.DLAB` is set).
    pub const THR: u32 = 0x0;
    /// `0x1` — interrupt enable register / divisor latch high (while
    /// `LCR.DLAB` is set).
    pub const IER: u32 = 0x1;
    /// `0x0` while `LCR.DLAB` is set — divisor latch **low** byte.
    ///
    /// ⚠️ Same offset as `THR` / `RHR`: `DLAB` is what picks which one
    /// the byte access sees.
    pub const DLL: u32 = THR;
    /// `0x1` while `LCR.DLAB` is set — divisor latch **high** byte.
    ///
    /// ⚠️ Same offset as `IER`.
    pub const DLM: u32 = IER;
    /// `0x2` — FIFO control register.
    pub const FCR: u32 = 0x2;
    /// `0x3` — line control register (bit 7 = DLAB).
    pub const LCR: u32 = 0x3;
    /// `0x4` — modem control register.
    pub const MCR: u32 = 0x4;
    /// `0x5` — line status register.
    pub const LSR: u32 = 0x5;
    /// `0x6` — modem status register (read-only).
    pub const MSR: u32 = 0x6;
    /// `0x7` — scratch register.
    pub const SCR: u32 = 0x7;
}

/// `LCR` bits (ns16550; the names match osz's `hal_uart.h`).
pub mod lcr_bits {
    /// Word length bit 0 (5 bits).
    pub const WORD_LEN0: u8 = 0x01;
    /// Word length bit 1.
    pub const WORD_LEN1: u8 = 0x02;
    /// Stop bits.
    pub const STOP_BITS: u8 = 0x04;
    /// Parity enable.
    pub const ENABLE_PARITY: u8 = 0x08;
    /// Even parity select.
    pub const EVEN_PARITY: u8 = 0x10;
    /// Force parity.
    pub const FORCE_PARITY: u8 = 0x20;
    /// Break control.
    pub const BREAK: u8 = 0x40;
    /// **DLAB** — divisor latch access bit: `1` ⇒ `0x0`/`0x1` are
    /// `DLL`/`DLM`, `0` ⇒ they are `THR`/`IER`.
    pub const DLAB: u8 = 0x80;
}

/// `LSR` bits (ns16550 line status register).
pub mod lsr_bits {
    /// bit 0 — RX data ready (`DR`): the RX FIFO holds **at least one**
    /// byte. osz calls it `LSR_RX_VALID`.
    ///
    /// ⚠️ **v0.24**：接收 FIFO 不止一个字节 ⇒ 判据是「**非空**」，不是
    /// 「恰好一个字节」。
    pub const RX_VALID: u8 = 0x01;
    /// bit 1 — transmitter holding register empty (`THRE`): always 1
    /// (a THR write completes instantly in this model).
    pub const THRE: u8 = 0x02;
    /// bit 2 — overrun error (`OE`): the RX FIFO was overrun; **latched**
    /// and cleared by writing 1 (W1C) or by reading the RHR.
    pub const OE: u8 = 0x04;
    /// bit 3 — TX capture overrun: the capture ring was full and the
    /// **new** byte was dropped; **latched**, cleared by writing 1 (W1C).
    ///
    /// ⚠️ In a real 16550 this bit is the parity error. This model has
    /// no parity/framing errors to report, so the bit is reused for the
    /// only *other* error condition the model can produce.
    pub const TX_OVERRUN: u8 = 0x08;
    /// bit 4 — framing error (`FE`): not modelled, always 0.
    pub const FE: u8 = 0x10;
    /// bit 5 — **transmitter can accept another byte** (`LSR_TX_IDLE` in
    /// osz's naming): 1 while the TX capture ring has room.
    ///
    /// ⚠️ osz's `hal_uart_putc()` spins while this bit is **0**, so this
    /// bit must mean "there is room", not "the ring is empty" — see the
    /// module docs.
    pub const TX_IDLE: u8 = 0x20;
    /// bit 6 — transmitter empty (`TEMT`): always 1.
    pub const TEMT: u8 = 0x40;
    /// Every bit this model defines in the `LSR` — the acceptance mask
    /// for the session-snapshot `status` field.
    ///
    /// ⚠️ It is a **`u32`** (not a `u8`): the DTO's `status` field is a
    /// `u32`, so the "undefined bits" mask has to be computed on 32 bits
    /// — otherwise a state carrying `0x8000` would slip through.
    pub const KNOWN: u32 = 0x0000_007F;
}

/// `FCR` bits (ns16550 FIFO control register).
pub mod fcr_bits {
    /// bit 0 — FIFO enable.
    pub const FIFO_ENABLE: u8 = 0x01;
    /// bit 1 — reset the RX FIFO (clears the RX byte **and** its latched
    /// overrun).
    pub const RX_FIFO_RESET: u8 = 0x02;
    /// bit 2 — reset the TX FIFO.
    ///
    /// ⚠️ In this model the "TX FIFO" **is** the capture ring, so this
    /// bit **discards** the bytes the host has not drained yet. It is an
    /// explicit guest request, not a silent loss — but it does break the
    /// "every byte the guest wrote can be taken back" invariant, so the
    /// guest must ask for it deliberately.
    pub const TX_FIFO_RESET: u8 = 0x04;
}

/// The number of bytes a bus access of `size` covers.
#[inline]
fn width(size: WriteSize) -> u32 {
    match size {
        WriteSize::Byte => 1,
        WriteSize::Half => 2,
        WriteSize::Word => 4,
    }
}

/// The bytes an access of `size` bytes at `offset` covers — and the
/// check that **every** one of them lies inside the register window.
///
/// ⚠️ A halfword at `0x7` reaches into `0x8` ⇒ rejected; a word at
/// `0x5` reaches into `0x8` ⇒ rejected. Only the accesses that stay
/// inside `0x0..=0x7` are legal.
fn span(offset: u32, size: WriteSize) -> Result<u32, PlatformError> {
    let n = width(size);
    let end = u64::from(offset) + u64::from(n);
    if end > u64::from(UART_WINDOW) {
        return Err(PlatformError::new(
            ErrorFamily::Peripheral,
            2,
            format!(
                "UART offset 0x{offset:x} (+{n} bytes) is outside the {UART_WINDOW}-byte register window"
            ),
        ));
    }
    Ok(n)
}

/// The lossless TX capture ring.
///
/// A plain `VecDeque` with a **hard capacity**. The policy on overflow is
/// **drop-newest** (the oldest bytes are kept) — see the module docs.
#[derive(Debug, Clone)]
struct TxCapture {
    buf: VecDeque<u8>,
    capacity: usize,
    /// How many bytes were dropped because the ring was full (lifetime
    /// counter, not a register).
    dropped: u64,
}

impl TxCapture {
    fn new(capacity: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            // A capacity of 0 would drop every byte; clamp to 1 so the
            // ring is always able to hold the byte the guest just wrote.
            capacity: capacity.max(1),
            dropped: 0,
        }
    }

    /// Push one byte. Returns `false` when the ring was full and the
    /// byte was **dropped** (the oldest bytes are kept).
    fn push(&mut self, byte: u8) -> bool {
        if self.buf.len() >= self.capacity {
            self.dropped += 1;
            return false;
        }
        self.buf.push_back(byte);
        true
    }

    /// Pop the **oldest** byte.
    fn pop(&mut self) -> Option<u8> {
        self.buf.pop_front()
    }

    /// The oldest byte without removing it.
    fn peek(&self) -> Option<u8> {
        self.buf.front().copied()
    }

    /// Take **all** pending bytes, oldest first.
    fn drain(&mut self) -> Vec<u8> {
        self.buf.drain(..).collect()
    }

    /// The **whole** ring content, oldest first — **without** draining it.
    ///
    /// ⚠️ **为什么要有它**（v0.26 · STAGE-11 裁决 E）：会话快照要带走
    /// 捕获环里的**全部**待取字节（只带走队头会**丢字节**）⇒ 快照的
    /// **导出**路径需要一个**只读**的视图 ⇒ 这里**不**动 [`Self::drain`]
    /// （那是宿主取走字节的出口）。
    fn bytes(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }

    /// Overwrite the lifetime drop counter.
    ///
    /// ⚠️ **为什么是「覆盖」而不是「累加」**（v0.26 · STAGE-11 裁决 E）：
    /// 快照携带的是**取样瞬间**的累计值（快照 = 状态）⇒ 还原后必须
    /// **等于**那个值 ⇒ 取「累加」会让还原后的计数与被快照的那次**不是
    /// 同一个数**。
    fn set_dropped(&mut self, dropped: u64) {
        self.dropped = dropped;
    }

    /// Restore the ring content, **overwriting** whatever was there.
    ///
    /// ⚠️ 深度由 [`Uart16550::check_status`] 查过（不可能超深）⇒ 这里
    /// 不会静默丢字节。
    fn restore(&mut self, bytes: &[u8]) {
        self.buf.clear();
        self.buf.extend(bytes.iter().copied());
    }

    fn len(&self) -> usize {
        self.buf.len()
    }

    fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    fn is_full(&self) -> bool {
        self.buf.len() >= self.capacity
    }

    fn clear(&mut self) {
        self.buf.clear();
    }
}

/// An ns16550-compatible UART.
#[derive(Debug)]
pub struct Uart16550 {
    base: u32,
    id: String,
    baud_rate: u32,
    /// `IER` (`0x1`, while `DLAB` is clear) — the interrupt enable mask.
    ier: u8,
    /// `FCR` (`0x2`).
    fcr: u8,
    /// `LCR` (`0x3`) — bit 7 is DLAB.
    lcr: u8,
    /// `MCR` (`0x4`).
    mcr: u8,
    /// `SCR` (`0x7`).
    scr: u8,
    /// `DLL` (`0x0`, while `DLAB` is set) — divisor latch low.
    dll: u8,
    /// `DLM` (`0x1`, while `DLAB` is set) — divisor latch high.
    dlm: u8,
    /// RX FIFO（最多 [`RX_FIFO_CAPACITY`] 字节；**队头**是下一个待读字节）。
    ///
    /// ⚠️ **v0.24**：从「1 字节深」（`Option<u8>`）加深成真正的 FIFO ——
    /// 见模块文档「接收 FIFO」。
    rx: VecDeque<u8>,
    /// Latched RX overrun (LSR bit 2).
    rx_overrun: bool,
    /// Latched TX capture overrun (LSR bit 3).
    tx_overrun: bool,
    /// The lossless TX capture ring.
    tx: TxCapture,
    /// IRQ line to raise on RX.
    irq_line: u16,
    /// M-18 probe callback (D-12). Empty = no observer attached.
    probe: ProbeSlot,
    /// Last virtual time this peripheral observed (ps) — see
    /// [`GpioBank::now_ps`](super::gpio::GpioBank) for the rationale.
    now_ps: u64,
}

impl Uart16550 {
    /// Construct a UART at `base` running at `baud_rate` bps, with the
    /// default TX capture ring capacity
    /// ([`DEFAULT_TX_CAPTURE_CAPACITY`]).
    pub fn new(id: impl Into<String>, base: u32, baud_rate: u32, irq_line: u16) -> Self {
        Self::new_with_tx_capacity(id, base, baud_rate, irq_line, DEFAULT_TX_CAPTURE_CAPACITY)
    }

    /// Construct a UART with an explicit TX capture ring capacity.
    ///
    /// The capacity is **clamped to at least 1** byte.
    pub fn new_with_tx_capacity(
        id: impl Into<String>,
        base: u32,
        baud_rate: u32,
        irq_line: u16,
        tx_capacity: usize,
    ) -> Self {
        Self {
            base,
            id: id.into(),
            baud_rate,
            ier: 0,
            fcr: 0,
            lcr: 0,
            mcr: 0,
            scr: 0,
            dll: 0,
            dlm: 0,
            rx: VecDeque::new(),
            rx_overrun: false,
            tx_overrun: false,
            tx: TxCapture::new(tx_capacity),
            irq_line,
            probe: ProbeSlot::default(),
            now_ps: 0,
        }
    }

    /// Set the IRQ line used to raise on RX. Replaces any previously
    /// configured line (M-15 review #12:统一封装风格).
    pub fn set_irq_line(&mut self, line: u16) {
        self.irq_line = line;
    }

    /// Get the currently-configured IRQ line.
    pub fn irq_line(&self) -> u16 {
        self.irq_line
    }

    /// The interrupt enable register (`IER @ 0x1`).
    ///
    /// ⚠️ The guest writes it through `WRITE_BYTE(base + IER, en)` — osz's
    /// `hal_uart_set_irq_en()` does exactly that. The **host / PLIC wiring**
    /// reads it through this accessor.
    pub fn interrupt_enable(&self) -> u8 {
        self.ier
    }

    /// The line control register (`LCR @ 0x3`).
    pub fn line_control(&self) -> u8 {
        self.lcr
    }

    /// The modem control register (`MCR @ 0x4`).
    pub fn modem_control(&self) -> u8 {
        self.mcr
    }

    /// The FIFO control register (`FCR @ 0x2`).
    pub fn fifo_control(&self) -> u8 {
        self.fcr
    }

    /// The scratch register (`SCR @ 0x7`).
    pub fn scratch(&self) -> u8 {
        self.scr
    }

    /// The line status register (`LSR @ 0x5`) — see [`lsr_bits`].
    pub fn line_status(&self) -> u8 {
        let mut s = lsr_bits::THRE | lsr_bits::TEMT;
        if !self.rx.is_empty() {
            s |= lsr_bits::RX_VALID;
        }
        if self.rx_overrun {
            s |= lsr_bits::OE;
        }
        if self.tx_overrun {
            s |= lsr_bits::TX_OVERRUN;
        }
        // "The transmitter can accept another byte" — the capture ring
        // has room. See the module docs for why this must NOT mean
        // "the capture ring is empty".
        if !self.tx.is_full() {
            s |= lsr_bits::TX_IDLE;
        }
        s
    }

    /// The modem status register (`MSR @ 0x6`).
    ///
    /// This model has **no** modem signals (no CTS / DSR / DCD lines), so
    /// the register reads 0 — and a write to it is accepted and has no
    /// effect (it is a read-only register in the hardware too).
    pub fn modem_status(&self) -> u8 {
        0
    }

    /// True while `LCR.DLAB` is set — i.e. while `0x0` / `0x1` are the
    /// divisor latch instead of `THR` / `IER`.
    pub fn dlab_enabled(&self) -> bool {
        self.lcr & lcr_bits::DLAB != 0
    }

    /// The baud-rate divisor held by the divisor latch
    /// (`(DLM << 8) | DLL`).
    pub fn divisor(&self) -> u16 {
        (u16::from(self.dlm) << 8) | u16::from(self.dll)
    }

    /// Push one byte into the TX capture ring (the guest wrote `THR`).
    ///
    /// ⚠️ **D-12**: a byte that *enters* the ring is one probe **pulse**
    /// whose value is the byte itself. A byte that was **dropped**
    /// (the ring was full) is **not** reported — it never went out.
    pub fn transmit(&mut self, byte: u8) {
        if self.tx.push(byte) {
            self.report(SIGNAL_TX, u64::from(byte));
        } else {
            // Drop-newest policy: the oldest bytes are kept, the overrun
            // is latched, and `LSR_TX_IDLE` goes to 0 so a guest that
            // polls the LSR can see that the transmitter is full.
            self.tx_overrun = true;
        }
    }

    /// Drain **one** byte from the TX capture ring (the oldest one).
    ///
    /// ⚠️ This is the **legacy** single-byte exit. It is **not** the only
    /// one any more: [`Uart16550::drain_tx`] takes everything at once,
    /// which is what the host bridge should use (a byte-at-a-time drain
    /// loses bytes as soon as the guest outruns the host).
    pub fn take_tx(&mut self) -> Option<u8> {
        self.tx.pop()
    }

    /// Take **all** bytes the guest has transmitted, oldest first.
    ///
    /// This is the **lossless** host-side exit: as long as the ring was
    /// not full, every byte the guest wrote is in here, in order, with
    /// neither a gap nor a repeat.
    pub fn drain_tx(&mut self) -> Vec<u8> {
        self.tx.drain()
    }

    /// How many bytes are waiting in the TX capture ring.
    pub fn tx_pending(&self) -> usize {
        self.tx.len()
    }

    /// The capacity of the TX capture ring.
    pub fn tx_capacity(&self) -> usize {
        self.tx.capacity
    }

    /// How many bytes were dropped because the ring was full.
    pub fn tx_dropped(&self) -> u64 {
        self.tx.dropped
    }

    /// The **whole** TX capture ring, oldest first — a **read-only** view.
    ///
    /// ⚠️ **与 [`Self::drain_tx`] 的区别**：`drain_tx` 是**出口**（把字节
    /// 取走）；这个是**快照的导出路径**（看一眼，不动它）。
    ///
    /// ⚠️ **为什么必须有它**（v0.26 · STAGE-11 裁决 E）：捕获环里的字节
    /// 是**访客已经发出去、还没被宿主取走**的内容 ⇒ 它**是会话状态**
    /// ⇒ 快照必须带走**全部**（只带走队头会**丢字节**）。
    pub fn tx_bytes(&self) -> Vec<u8> {
        self.tx.bytes()
    }

    /// Receive `byte` from the wire (host-supplied — the firmware will
    /// read the RHR later).
    ///
    /// ⚠️ **入队尾**：字节排在**队尾**，guest 读 `RHR` 取的是**队头**
    /// ⇒ 多个字节进来时**顺序不丢、不乱序**。
    ///
    /// ⚠️ **FIFO 满时丢最新的那个，并锁存 `LSR` 的 overrun 位（bit2）**
    /// —— 与 TX 捕获环**同一口径**（模块文档「环满时的策略」）：最早的
    /// 字节是输入流的**前缀**，丢前缀比丢尾巴更难诊断 ⇒ 丢**新来的**那个。
    /// ⇒ 「丢」是**看得见的**：overrun 位被**锁存**（W1C 清），不是静默的。
    pub fn receive(&mut self, byte: u8) {
        if self.rx.len() >= RX_FIFO_CAPACITY {
            // 满了：丢**最新**的这个字节（保住最早的），并锁存 overrun。
            self.rx_overrun = true;
            return;
        }
        self.rx.push_back(byte);
    }

    /// TX pin level: `true` when the transmitter is idle (nothing is
    /// waiting in the capture ring), `false` while bytes are pending.
    pub fn tx_pin(&self) -> bool {
        self.tx.is_empty()
    }

    /// RX pin level (debug — we don't model an electrical line in MVP).
    pub fn rx_pin(&self) -> bool {
        self.rx.is_empty()
    }

    /// Picoseconds per bit at the configured baud rate. Floored at
    /// 1 ps to avoid zero-length frames when `baud_rate > 1e9`
    /// (M-15 review #15).
    pub fn ps_per_bit(&self) -> u64 {
        // 评审 15-1: 1 GHz / baud. Falls back to 1 µs if `baud_rate == 0`.
        let raw = if self.baud_rate == 0 {
            1_000_000
        } else {
            1_000_000_000_u64 / u64::from(self.baud_rate)
        };
        raw.max(1)
    }

    /// Report one signal observation to the M-18 probe pipeline.
    fn report(&mut self, signal: &str, value: u64) {
        let now = self.now_ps;
        self.probe.report(signal, value, now);
    }

    /// The byte visible at `offset` — **without** any side effect.
    fn byte_at(&self, offset: u32) -> u8 {
        match offset {
            reg::THR => {
                if self.dlab_enabled() {
                    self.dll
                } else {
                    self.rx.front().copied().unwrap_or(0)
                }
            }
            reg::IER => {
                if self.dlab_enabled() {
                    self.dlm
                } else {
                    self.ier
                }
            }
            reg::FCR => self.fcr,
            reg::LCR => self.lcr,
            reg::MCR => self.mcr,
            reg::LSR => self.line_status(),
            reg::MSR => self.modem_status(),
            reg::SCR => self.scr,
            // Unreachable: the span is validated before this is called.
            _ => 0,
        }
    }

    /// Write one byte into the register at `offset`.
    fn write_byte(&mut self, offset: u32, value: u8) -> Result<(), PlatformError> {
        match offset {
            reg::THR => {
                if self.dlab_enabled() {
                    self.dll = value;
                } else {
                    self.transmit(value);
                }
            }
            reg::IER => {
                if self.dlab_enabled() {
                    self.dlm = value;
                } else {
                    self.ier = value;
                }
            }
            reg::FCR => {
                self.fcr = value;
                self.apply_fifo_control(value);
            }
            reg::LCR => self.lcr = value,
            reg::MCR => self.mcr = value,
            reg::LSR => self.clear_line_status(value),
            // MSR is read-only in the hardware too: the write is
            // accepted and has no effect.
            reg::MSR => {}
            reg::SCR => self.scr = value,
            other => {
                return Err(PlatformError::new(
                    ErrorFamily::Peripheral,
                    2,
                    format!("UART offset 0x{other:x} not defined"),
                ))
            }
        }
        Ok(())
    }

    /// Apply the FIFO control register's reset bits.
    fn apply_fifo_control(&mut self, fcr: u8) {
        if fcr & fcr_bits::RX_FIFO_RESET != 0 {
            self.rx.clear();
            self.rx_overrun = false;
        }
        if fcr & fcr_bits::TX_FIFO_RESET != 0 {
            self.tx.clear();
            self.tx_overrun = false;
        }
    }

    /// W1C (write-1-to-clear) for the latched `LSR` error bits.
    fn clear_line_status(&mut self, value: u8) {
        if value & lsr_bits::OE != 0 {
            self.rx_overrun = false;
        }
        if value & lsr_bits::TX_OVERRUN != 0 {
            self.tx_overrun = false;
        }
    }

    /// True when the access covers the RHR (offset `0x0` while `DLAB` is
    /// clear) — the only read-destructive register.
    ///
    /// The window starts at `0x0`, so an access covers the RHR exactly
    /// when it *starts* there.
    fn covers_rhr(&self, offset: u32) -> bool {
        offset == reg::THR && !self.dlab_enabled()
    }

    /// Read the bytes of an access, **without** side effects.
    fn read_bytes(&self, offset: u32, size: WriteSize) -> Result<[u8; 4], PlatformError> {
        let n = span(offset, size)?;
        let mut out = [0u8; 4];
        for (i, slot) in out.iter_mut().enumerate().take(n as usize) {
            *slot = self.byte_at(offset + i as u32);
        }
        Ok(out)
    }

    /// The UART's **own** integrity rules for a session state.
    ///
    /// ⚠️ Three rules, all **UART-specific** — the contract layer knows
    /// the *shape* of a UART state, but not what the `LSR` bits **mean**
    /// nor how deep this model's FIFO is:
    ///
    /// 1. **Only the defined bits.** The `LSR` may only carry the bits
    ///    this model defines. A state with other bits set could not have
    ///    been produced by this UART — it came from somewhere else (or
    ///    from a hand-built payload).
    /// 2. **The RX flag must agree with the RX FIFO.** `LSR_RX_VALID` is
    ///    set exactly when the RX FIFO holds **at least one** byte (v0.24:
    ///    the FIFO is deeper than one byte ⇒ 判据是「队列**非空**」，不是
    ///    「恰好一个字节」). A state that contradicts itself would restore
    ///    a UART whose register **lies** about its own FIFO — and the
    ///    firmware would believe it.
    /// 3. **The queue must fit this model's FIFO.** A payload carrying
    ///    more bytes than [`RX_FIFO_CAPACITY`] describes a UART that this
    ///    model cannot be ⇒ 还原时必然要**截断**（丢字节）⇒ 必须被拒。
    ///
    /// ⚠️ There is deliberately **no** rule about the TX side: the DTO can
    /// only carry **one** byte of the capture ring, so the ring's
    /// *fullness* — which is what `LSR_TX_IDLE` reports — is not
    /// representable and cannot be cross-checked.
    fn check_status(&self, state: &UartStateDto) -> Result<(), PlatformError> {
        // ⚠️ The mask must cover **every** bit above the defined ones
        // (the DTO's `status` is a `u32`), not just bit 7 — otherwise a
        // state carrying `0x8000` would slip through.
        const UNDEFINED_BITS: u32 = !lsr_bits::KNOWN;
        if state.status & UNDEFINED_BITS != 0 {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "UART '{}' 的会话状态带未定义的 LSR 位 {:#x}",
                    self.id,
                    state.status & UNDEFINED_BITS
                ),
            ));
        }
        if state.rx_queue.len() > RX_FIFO_CAPACITY {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "UART '{}' 的会话状态带 {} 个待读字节，器件的接收 FIFO 只有 {} 字节深",
                    self.id,
                    state.rx_queue.len(),
                    RX_FIFO_CAPACITY
                ),
            ));
        }
        // ⚠️ `rx` 必须是 `rx_queue` 的**队头** —— 两者不一致的载荷是
        // **自相矛盾**的（它同时说「队头是 0x41」和「队头是 0x42」）。
        //
        // ⚠️ 契约层的 `validate()` 也查这一条（**形状**规则）；这里再查一遍
        // 是**防御性冗余**：器件的恢复路径（注册表那条）**不经过**契约层
        // 的校验器 ⇒ 只在契约层查的话，这条路径会漏。
        if state.rx != state.rx_queue.first().copied() {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "UART '{}' 的会话状态自相矛盾：rx 是 {:?}，但 rx_queue 的队头是 {:?}",
                    self.id,
                    state.rx,
                    state.rx_queue.first().copied()
                ),
            ));
        }
        // ⚠️ **v0.26 · STAGE-11 裁决 E**：`tx` 必须是 `tx_ring` 的**队头**
        // —— 与上面那条 `rx` / `rx_queue` **同构**。
        //
        // ⚠️ 契约层的 `validate()` 也查这一条（**形状**规则）；这里再查一遍
        // 是**防御性冗余**：器件的恢复路径（注册表那条）**不经过**契约层
        // 的校验器 ⇒ 只在契约层查的话，这条路径会漏。
        if state.tx != state.tx_ring.first().copied() {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "UART '{}' 的会话状态自相矛盾：tx 是 {:?}，但 tx_ring 的队头是 {:?}",
                    self.id,
                    state.tx,
                    state.tx_ring.first().copied()
                ),
            ));
        }
        // ⚠️ **器件能力规则**（契约层查不了）：捕获环**不能比器件的环深**
        // —— 还原时超深的字节会被**静默丢掉**（丢最新、保最早）⇒ 必须
        // 在**写入之前**拒掉。
        //
        // ⚠️ 这是 `rx_queue.len() > RX_FIFO_CAPACITY` 那条的**同构**形状：
        // 「形状」归契约层，「深度」归器件。
        if state.tx_ring.len() > self.tx.capacity {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "UART '{}' 的会话状态带 {} 个待取字节，器件的 TX 捕获环只有 {} 字节深",
                    self.id,
                    state.tx_ring.len(),
                    self.tx.capacity
                ),
            ));
        }
        let rx_valid = state.status & u32::from(lsr_bits::RX_VALID) != 0;
        if rx_valid != !state.rx_queue.is_empty() {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "UART '{}' 的会话状态自相矛盾：LSR.RX_VALID = {}，但 RX FIFO 是 {}",
                    self.id,
                    rx_valid,
                    if state.rx_queue.is_empty() {
                        "空"
                    } else {
                        "非空"
                    }
                ),
            ));
        }
        Ok(())
    }
}

impl Peripheral for Uart16550 {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_addr(&self) -> u32 {
        self.base
    }
    fn size(&self) -> u32 {
        UART_WINDOW
    }

    /// Read the register file.
    ///
    /// ⚠️ **Byte offsets, not word indices**: the access covers the bytes
    /// `offset .. offset + width`, and the value is assembled
    /// little-endian. See the module docs.
    fn read(&self, offset: u32, size: WriteSize) -> Result<u32, PlatformError> {
        let bytes = self.read_bytes(offset, size)?;
        Ok(assemble_le(&bytes))
    }

    /// Read the register file **and** consume the RX byte when the access
    /// covers the RHR.
    ///
    /// ⚠️ The RHR is the only read-destructive register: reading it
    /// takes the byte out of the RX FIFO and clears the latched RX
    /// overrun — exactly what real hardware does.
    fn read_clear(&mut self, offset: u32, size: WriteSize) -> Result<u32, PlatformError> {
        let bytes = self.read_bytes(offset, size)?;
        if self.covers_rhr(offset) {
            // ⚠️ 读 `RHR` = **弹出队头**（不是「读了不清空」也不是「读最后一个」）。
            self.rx.pop_front();
            self.rx_overrun = false;
        }
        Ok(assemble_le(&bytes))
    }

    /// Write the register file — each byte goes into **its own**
    /// register, at its own byte offset.
    fn write(&mut self, offset: u32, size: WriteSize, value: u32) -> Result<(), PlatformError> {
        let n = span(offset, size)?;
        for i in 0..n {
            let byte = ((value >> (i * 8)) & 0xFF) as u8;
            self.write_byte(offset + i, byte)?;
        }
        Ok(())
    }

    fn tick(&mut self, now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        // No time-driven side-effects beyond the FIFO state (the host
        // calls `tick` once per quantum; bit-clock progression is out
        // of MVP scope 评审 15-1). The tick does carry the authoritative
        // virtual time, which the probe reports are stamped with.
        self.now_ps = now_ps;
        Ok(Vec::new())
    }

    /// Install / replace the M-18 probe callback (D-12).
    fn on_probe(&mut self, cb: Option<ProbeSink>) {
        self.probe.set(cb);
    }

    /// This UART's share of the **session snapshot** (segment 4 —
    /// STAGE-3 §3.3).
    ///
    /// Every field the UART owns **that the contract can carry** is
    /// exported: the RX byte, the **head** of the TX capture ring, the
    /// `LSR`, the IRQ line, the peripheral's **authoritative virtual
    /// time** and — **since v0.24** — the ns16550 registers `LCR` /
    /// `IER` / `FCR` / `MCR` / `SCR` / `DLL` / `DLM` (D-118).
    ///
    /// ⚠️ `now_ps` is not decoration: it is the timestamp the probe
    /// reports are stamped with. Losing it would make every edge that
    /// follows the restore land on the wrong point of the time axis.
    ///
    /// ⚠️ **`LCR` 的 `DLAB` 位必须带**（它是 `LCR` 的一部分）：`DLAB`
    /// 决定 `0x0` / `0x1` 是 `THR`/`IER` 还是 `DLL`/`DLM` ⇒ 少了这一位，
    /// 还原出来的 UART 处在**错误的寄存器映射**上。
    ///
    /// ⚠️ **`LSR` 不还原原值** —— 它是**重算**出来的（见
    /// [`Uart16550::restore_session_state`]）。
    fn session_state(&self) -> Option<PeripheralSessionState> {
        Some(PeripheralSessionState::Uart(UartStateDto {
            id: self.id.clone(),
            tx: self.tx.peek(),
            // ⚠️ **v0.26 · STAGE-11 裁决 E**：TX 捕获环的**全部**待取字节
            // 进快照（`tx_ring`），`tx` 保留为**队头**的冗余表示 ⇒ 两者
            // 必须一致（契约层的 `validate()` 查这条不变式）。
            //
            // ⚠️ **只带走队头会丢字节**：访客连写三个字节、宿主还没取走
            // ⇒ 快照/还原一次，那两个字节就没了 ⇒ 还原出来的会话与采
            // 快照的那次**不是同一个**。
            tx_ring: self.tx.bytes(),
            // ⚠️ **溢出丢弃的累计计数**也进快照 ⇒ 「丢最新、保最早」这
            // 件事**看得见**，不是静默的。
            tx_dropped: self.tx.dropped,
            // ⚠️ `rx` 是**队头**，`rx_queue` 是**全部**待读字节。
            rx: self.rx.front().copied(),
            rx_queue: self.rx.iter().copied().collect(),
            status: u32::from(self.line_status()),
            irq_line: self.irq_line,
            now_ps: self.now_ps,
            // ---- ns16550 的七个寄存器（v0.24 · D-118）----
            lcr: self.lcr,
            ier: self.ier,
            fcr: self.fcr,
            mcr: self.mcr,
            scr: self.scr,
            dll: self.dll,
            dlm: self.dlm,
        }))
    }

    /// Restore the UART's session state (segment 4's consumer).
    ///
    /// ⚠️ The `LSR` is **not** restored verbatim: it is **recomputed**
    /// from the restored FIFO state and the latched error bits it
    /// carries (bit 2 = RX overrun, bit 3 = TX capture overrun). The
    /// recomputed value is what the guest reads, so restoring the raw
    /// number would let the register contradict its own FIFOs.
    ///
    /// ⚠️ The seven ns16550 registers **are** restored verbatim (v0.24 ·
    /// D-118) — they are plain storage, no recomputation involved.
    ///
    /// ⚠️ **`FCR` 还原时不触发它的复位位**：`FCR` 的 bit1 / bit2 是
    /// 「复位 RX / TX FIFO」，而 FIFO 的内容**由载荷给**（`tx_ring` /
    /// `rx_queue`）⇒ 还原时若把复位位也执行一遍，刚还原回来的字节会被
    /// **抹掉**。
    ///
    /// ⚠️ **TX 捕获环整体还原**（v0.26 · STAGE-11 裁决 E）：载荷带的是
    /// `tx_ring`（**全部**待取字节），`tx` 只是它的**队头**（冗余表示）
    /// ⇒ 还原 `tx_ring` 即可 ⇒ 还原后 `drain_tx()` 拿回的字节与采快照
    /// 那次**逐字节相同**。
    ///
    /// ⚠️ **溢出计数取「覆盖」**：快照 = 状态 ⇒ 还原后的 `tx_dropped`
    /// 必须**等于**载荷里的那个值。
    ///
    /// ⚠️ The UART's **own** rules are checked here as well
    /// ([`Uart16550::check_status`]) — **defensive redundancy**: the gate
    /// is the read-only pass
    /// ([`Peripheral::validate_session_state`]), which runs **before**
    /// any peripheral is written (STAGE-3 §5).
    fn restore_session_state(
        &mut self,
        state: &PeripheralSessionState,
    ) -> Result<(), PlatformError> {
        let PeripheralSessionState::Uart(s) = state else {
            return Err(super::wrong_state_kind(
                &self.id,
                super::SessionKind::Uart,
                super::SessionKind::of(state),
            ));
        };
        super::check_same_id(&self.id, &s.id)?;
        self.check_status(s)?;
        // ---- ns16550 的七个寄存器（v0.24 · D-118）----
        // ⚠️ 顺序：**先**寄存器、**后** FIFO —— `FCR` 是按「纯存储」
        // 还原（不执行它的复位位），所以这里不会把刚还原的 FIFO 抹掉。
        self.lcr = s.lcr;
        self.ier = s.ier;
        self.fcr = s.fcr;
        self.mcr = s.mcr;
        self.scr = s.scr;
        self.dll = s.dll;
        self.dlm = s.dlm;
        // ⚠️ **还原整个 TX 捕获环**（v0.26 · STAGE-11 裁决 E）—— 只还
        // 原队头会**丢字节** ⇒ 还原出来的会话与采快照的那次**不是同一
        // 个**。
        //
        // ⚠️ **`tx` 不单独还原**：它是 `tx_ring` 的**冗余**表示（队头）⇒
        // 还原 `tx_ring` 就够了（与 `rx` / `rx_queue` 的口径一致）。
        //
        // ⚠️ 环的深度由 [`Uart16550::check_status`] 查过（不可能超深）⇒
        // 这里不会截断、不会静默丢字节。
        self.tx.restore(&s.tx_ring);
        // ⚠️ **溢出计数取「覆盖」而不是「累加」**：快照携带的是取样瞬间
        // 的累计值（快照 = 状态）⇒ 还原后必须**等于**那个值。
        self.tx.set_dropped(s.tx_dropped);
        self.rx.clear();
        // ⚠️ **还原整个接收队列**（v0.24）—— 只还原队头会**丢字节** ⇒
        // 还原出来的会话与采快照的那次**不是同一个**。
        // ⚠️ 队列深度由 [`Uart16550::check_status`] 查过（不可能超深）⇒
        // 这里不会截断。
        for byte in &s.rx_queue {
            self.rx.push_back(*byte);
        }
        self.rx_overrun = s.status & u32::from(lsr_bits::OE) != 0;
        self.tx_overrun = s.status & u32::from(lsr_bits::TX_OVERRUN) != 0;
        self.irq_line = s.irq_line;
        self.now_ps = s.now_ps;
        Ok(())
    }

    /// **Read-only** acceptance check for this UART's session state
    /// (STAGE-3 §3.3) — the gate that runs **before** anything is
    /// written.
    ///
    /// ⚠️ The rules that live **only** here are the ones the contract
    /// layer cannot check: it knows the *shape* of a UART state, but not
    /// what the `LSR` bits **mean** (see [`Uart16550::check_status`]).
    fn validate_session_state(&self, state: &PeripheralSessionState) -> Result<(), PlatformError> {
        let PeripheralSessionState::Uart(s) = state else {
            return Err(super::wrong_state_kind(
                &self.id,
                super::SessionKind::Uart,
                super::SessionKind::of(state),
            ));
        };
        super::check_same_id(&self.id, &s.id)?;
        self.check_status(s)
    }
}

/// Assemble the bytes of an access into the returned value —
/// **little-endian**: `b0 | b1 << 8 | b2 << 16 | b3 << 24`.
#[inline]
fn assemble_le(bytes: &[u8; 4]) -> u32 {
    u32::from(bytes[0])
        | (u32::from(bytes[1]) << 8)
        | (u32::from(bytes[2]) << 16)
        | (u32::from(bytes[3]) << 24)
}

#[cfg(test)]
mod tests {
    //! Unit tests for the ns16550 register file and the lossless TX
    //! capture ring.
    //!
    //! ⚠️ **断言值一律写具体字面量**（`0x80` / `0x01` / `0x20` …），不
    //! 从被测实现反推——否则实现改了，用例跟着改，永远绿。

    use super::*;
    use riscv_sim_contract::model::WriteSize;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Recorded probe observations: `(signal, value, now_ps)`.
    type ProbeLog = Rc<RefCell<Vec<(String, u64, u64)>>>;

    /// Install a probe sink that records every observation.
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

    fn uart() -> Uart16550 {
        Uart16550::new("UART0", 0x1000_0000, 115_200, 4)
    }

    // -------------------------------------------------------------------
    // 一、ns16550 寄存器布局
    // -------------------------------------------------------------------

    /// ⚠️ **DLAB 必须真的切换 `0x0` / `0x1`。**
    ///
    /// 旧布局里 `0x0` 永远是 DATA、写入就是发字节——osz 的
    /// `hal_uart_set_baudrate()`（写 DLL/DLM）会把两个除数字节当 TX
    /// 发出去。这条用例钉住「DLAB=1 时 `0x0`/`0x1` 是 DLL/DLM」。
    #[test]
    fn dlab_switches_the_first_two_registers_between_thr_ier_and_dll_dlm() {
        let mut u = uart();
        assert!(!u.dlab_enabled(), "DLAB starts cleared");

        // DLAB = 1 → 0x0/0x1 是除数锁存器。
        u.write(reg::LCR, WriteSize::Byte, u32::from(lcr_bits::DLAB))
            .expect("set DLAB");
        assert!(u.dlab_enabled());
        u.write(reg::THR, WriteSize::Byte, 0x34).expect("write DLL");
        u.write(reg::IER, WriteSize::Byte, 0x12).expect("write DLM");
        assert_eq!(u.divisor(), 0x1234, "the divisor latch holds DLL/DLM");
        assert_eq!(
            u.tx_pending(),
            0,
            "the divisor latch writes must NOT be transmitted"
        );

        // DLAB = 0 → 0x0/0x1 变回 THR/IER。
        u.write(reg::LCR, WriteSize::Byte, 0x00)
            .expect("clear DLAB");
        assert!(!u.dlab_enabled());
        assert_eq!(
            u.read(reg::IER, WriteSize::Byte).unwrap(),
            0x00,
            "IER is not the divisor latch high byte any more"
        );
        u.write(reg::IER, WriteSize::Byte, 0x0F).expect("write IER");
        assert_eq!(u.interrupt_enable(), 0x0F);
        u.write(reg::THR, WriteSize::Byte, u32::from(b'X'))
            .expect("write THR");
        assert_eq!(
            u.drain_tx(),
            vec![b'X'],
            "only the THR write is transmitted"
        );
    }

    /// ⚠️ **字节宽度访问**：`lb` 读 `LSR @ 0x5` 拿到的必须是**状态**，
    /// 不是 RX 数据。
    ///
    /// 旧布局里 `0x5` 是 DATA 字的第 1 个字节 ⇒ 读出来是 RX 数据（或
    /// 0），**完全不是状态**。osz 的 `drv_uart_int_default_handle()`
    /// 正是这么读的（`LSR` 判 `LSR_RX_VALID`，再读 `RHR`）。
    #[test]
    fn reading_lsr_returns_the_line_status_not_the_rx_data() {
        let mut u = uart();
        u.receive(b'Z');
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_ne!(lsr, u32::from(b'Z'), "LSR must never return the RX byte");
        assert_eq!(
            lsr & u32::from(lsr_bits::RX_VALID),
            u32::from(lsr_bits::RX_VALID),
            "the RX byte is pending → LSR_RX_VALID is set"
        );
        assert_eq!(
            lsr & u32::from(lsr_bits::THRE),
            u32::from(lsr_bits::THRE),
            "THRE is always 1"
        );
        assert_eq!(
            lsr & u32::from(lsr_bits::TX_IDLE),
            u32::from(lsr_bits::TX_IDLE),
            "the capture ring has room → TX_IDLE is set"
        );
        assert_eq!(
            lsr & u32::from(lsr_bits::TEMT),
            u32::from(lsr_bits::TEMT),
            "TEMT is always 1"
        );
        assert_eq!(lsr & u32::from(lsr_bits::OE), 0, "no overrun yet");
    }

    /// ⚠️ **旧布局最坑的错位陷阱**：`IER @ 0x1` 写进去的值**不会**被
    /// 当成 TX 数据发出去。
    ///
    /// 旧布局把 `0x1` 当 DATA 的第 2 个字节 ⇒ osz 的
    /// `hal_uart_set_irq_en()` 会把中断使能值**当 TX 字节发出去**。
    #[test]
    fn writing_ier_does_not_transmit_a_byte() {
        let mut u = uart();
        u.write(reg::IER, WriteSize::Byte, 0x01).expect("write IER");
        assert_eq!(u.interrupt_enable(), 0x01, "IER keeps the value");
        assert_eq!(u.tx_pending(), 0, "IER must not enter the TX stream");
        assert!(
            u.drain_tx().is_empty(),
            "the TX stream must be empty after an IER write"
        );
    }

    /// `LSR` 的两个 osz 宏语义正确：`LSR_RX_VALID` 跟着 RX FIFO 走，
    /// `LSR_TX_IDLE` 跟着捕获环的**余量**走（不是「环是否为空」）。
    #[test]
    fn line_status_reports_rx_valid_and_tx_idle() {
        let mut u = Uart16550::new_with_tx_capacity("UART0", 0x1000_0000, 115_200, 4, 4);

        // 空：RX_VALID 清、TX_IDLE 置。
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(lsr & u32::from(lsr_bits::RX_VALID), 0);
        assert_eq!(lsr & u32::from(lsr_bits::TX_IDLE), 0x20);

        // RX 里有字节 → RX_VALID 置。
        u.receive(b'Z');
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(lsr & u32::from(lsr_bits::RX_VALID), 0x01);
        // 读完 RHR → RX_VALID 清。
        u.read_clear(reg::THR, WriteSize::Byte).expect("read RHR");
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(lsr & u32::from(lsr_bits::RX_VALID), 0);

        // 捕获环写满 → TX_IDLE 清（**还能再收一个字节**才叫 idle）。
        for i in 0..4u8 {
            u.write(reg::THR, WriteSize::Byte, u32::from(i))
                .expect("tx");
        }
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(
            lsr & u32::from(lsr_bits::TX_IDLE),
            0,
            "a full capture ring means the transmitter cannot take another byte"
        );
        // 宿主取走一个字节 → 又有余量了。
        u.drain_tx();
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(lsr & u32::from(lsr_bits::TX_IDLE), 0x20);
    }

    /// RX overrun 锁存在 `LSR` 的 bit2，且是 W1C。
    ///
    /// ⚠️ **v0.24**：RX FIFO 是 [`RX_FIFO_CAPACITY`] 字节深 ⇒ 要触发
    /// overrun 必须**先把 FIFO 填满**，再塞一个 ⇒ 夹具里灌的是
    /// `0x00..=0x0F` 这 16 个字节，第 17 个（`0xFF`）被丢。
    #[test]
    fn the_rx_overrun_is_latched_in_lsr_bit_two_and_is_w1c() {
        let mut u = uart();
        for i in 0..RX_FIFO_CAPACITY {
            u.receive(i as u8);
        }
        u.receive(0xFF); // overrun: the RX FIFO is full
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(lsr & u32::from(lsr_bits::OE), 0x04);

        // 写 0 不清。
        u.write(reg::LSR, WriteSize::Byte, 0x00).expect("write 0");
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(lsr & u32::from(lsr_bits::OE), 0x04);

        // 写 1 清。
        u.write(reg::LSR, WriteSize::Byte, u32::from(lsr_bits::OE))
            .expect("write OE");
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(lsr & u32::from(lsr_bits::OE), 0);
    }

    /// ⚠️ **v0.24 · 接收 FIFO 能装多个字节**，且**顺序不丢、不乱序**。
    ///
    /// ⚠️ 这是「一行 shell 输入能整个送进去」这条批次目标的**内核侧**落点
    /// （冻结规格 STAGE-9 裁决 B 第一步）。
    #[test]
    fn the_rx_fifo_holds_several_bytes_in_order() {
        let mut u = uart();
        for byte in [0x41u8, 0x42, 0x43] {
            u.receive(byte);
        }

        // LSR 的 RX_VALID = FIFO 非空。
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(
            lsr & u32::from(lsr_bits::RX_VALID),
            u32::from(lsr_bits::RX_VALID),
            "FIFO 里有字节 ⇒ RX_VALID 必须置位"
        );
        assert_eq!(
            lsr & u32::from(lsr_bits::OE),
            0,
            "三个字节远没塞满 ⇒ 不许锁存 overrun"
        );

        // 读 RHR = **弹出队头**：0x41 → 0x42 → 0x43 → 空。
        assert_eq!(u.read_clear(reg::THR, WriteSize::Byte).unwrap(), 0x41);
        assert_eq!(u.read_clear(reg::THR, WriteSize::Byte).unwrap(), 0x42);
        assert_eq!(u.read_clear(reg::THR, WriteSize::Byte).unwrap(), 0x43);
        assert_eq!(
            u.read_clear(reg::THR, WriteSize::Byte).unwrap(),
            0,
            "FIFO 空了 ⇒ RHR 读出来是 0"
        );

        // 读空之后 LSR 的 RX_VALID 也要跟着清。
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(
            lsr & u32::from(lsr_bits::RX_VALID),
            0,
            "FIFO 空了 ⇒ RX_VALID 必须清"
        );
    }

    /// ⚠️ **v0.24 · FIFO 满时丢最新的那个**（与 TX 捕获环同一口径），
    /// 并**锁存** overrun 位。
    ///
    /// ⚠️ **「最早的一个都不能丢」**：最早的字节是输入流的**前缀** ⇒
    /// 丢前缀比丢尾巴更难诊断。
    #[test]
    fn the_rx_fifo_drops_the_newest_byte_when_it_is_full_and_latches_the_overrun() {
        let mut u = uart();
        for i in 0..RX_FIFO_CAPACITY {
            u.receive(i as u8);
        }
        u.receive(0xFF); // 第 17 个字节 ⇒ 被丢

        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(
            lsr & u32::from(lsr_bits::OE),
            u32::from(lsr_bits::OE),
            "丢是**看得见的**：overrun 位必须锁存"
        );

        // 留下的是**最早**的 16 个字节，顺序不变。
        for i in 0..RX_FIFO_CAPACITY {
            assert_eq!(
                u.read_clear(reg::THR, WriteSize::Byte).unwrap(),
                u32::from(i as u8),
                "留下的必须是**最早**的字节，且顺序不变"
            );
        }
        assert_eq!(
            u.read_clear(reg::THR, WriteSize::Byte).unwrap(),
            0,
            "被丢的那个字节（0xFF）不许出现在 FIFO 里"
        );
    }

    /// `FCR` 的 RX FIFO 复位位（bit1）清掉**整个**队列（不止一个字节）。
    #[test]
    fn the_rx_fifo_reset_bit_clears_the_whole_queue() {
        let mut u = uart();
        u.receive(0x41);
        u.receive(0x42);
        u.write(
            reg::FCR,
            WriteSize::Byte,
            u32::from(fcr_bits::RX_FIFO_RESET),
        )
        .expect("fcr reset");
        assert_eq!(
            u.read_clear(reg::THR, WriteSize::Byte).unwrap(),
            0,
            "复位位必须把**整个**队列清掉"
        );
    }

    /// 其它寄存器各就各位：写进去读得回来。
    #[test]
    fn the_ns16550_register_file_round_trips_every_register() {
        let mut u = uart();
        u.write(reg::LCR, WriteSize::Byte, 0x03).expect("lcr");
        u.write(reg::MCR, WriteSize::Byte, 0x0B).expect("mcr");
        u.write(reg::SCR, WriteSize::Byte, 0x5A).expect("scr");
        u.write(reg::FCR, WriteSize::Byte, 0x07).expect("fcr");
        u.write(reg::IER, WriteSize::Byte, 0x02).expect("ier");

        assert_eq!(u.read(reg::LCR, WriteSize::Byte).unwrap(), 0x03);
        assert_eq!(u.read(reg::MCR, WriteSize::Byte).unwrap(), 0x0B);
        assert_eq!(u.read(reg::SCR, WriteSize::Byte).unwrap(), 0x5A);
        assert_eq!(u.read(reg::FCR, WriteSize::Byte).unwrap(), 0x07);
        assert_eq!(u.read(reg::IER, WriteSize::Byte).unwrap(), 0x02);
    }

    /// ⚠️ **多字节访问 = 按字节偏移读，再按 size 组装（小端）**。
    ///
    /// 半字读 `0x0` = RHR | IER<<8；字读 `0x4` = MCR | LSR<<8 |
    /// MSR<<16 | SCR<<24。
    #[test]
    fn multi_byte_accesses_are_assembled_from_byte_offsets() {
        let mut u = uart();
        u.receive(0xAB);
        u.write(reg::IER, WriteSize::Byte, 0x12).expect("ier");
        u.write(reg::MCR, WriteSize::Byte, 0x34).expect("mcr");
        u.write(reg::SCR, WriteSize::Byte, 0x78).expect("scr");

        // Half at 0x0 → byte 0 (RHR) | byte 1 (IER) << 8.
        assert_eq!(u.read(0x0, WriteSize::Half).unwrap(), 0x12AB);
        // Word at 0x4 → MCR | LSR<<8 | MSR<<16 | SCR<<24.
        let word = u.read(0x4, WriteSize::Word).unwrap();
        assert_eq!(word & 0xFF, 0x34, "byte 0 is MCR");
        assert_eq!(
            (word >> 8) & 0xFF,
            0x63,
            "byte 1 is the LSR (RX_VALID(0x01) | THRE(0x02) | TX_IDLE(0x20) | TEMT(0x40))"
        );
        assert_eq!((word >> 16) & 0xFF, 0x00, "byte 2 is the MSR");
        assert_eq!((word >> 24) & 0xFF, 0x78, "byte 3 is the SCR");
    }

    /// 越界的访问必须被拒（`E_PER-02`）——半字读到 `0x8`、字读到
    /// `0x8` 都是越界；**刚好**盖住窗口末尾的半字（`0x6`）是**合法**的。
    #[test]
    fn an_access_reaching_past_the_register_window_is_rejected() {
        let mut u = uart();
        // Half at 0x7 covers 0x7..0x8 → 0x8 is outside the window.
        let err = u.read(0x7, WriteSize::Half).unwrap_err();
        assert_eq!(err.scenario, 2);
        // Word at 0x5 covers 0x5..0x8 → 0x8 is outside the window.
        let err = u.read(0x5, WriteSize::Word).unwrap_err();
        assert_eq!(err.scenario, 2);
        // Half at 0x7 on the write side, too.
        let err = u.write(0x7, WriteSize::Half, 0).unwrap_err();
        assert_eq!(err.scenario, 2);
        // Half at 0x6 covers 0x6..0x7 → **inside** the window ⇒ legal.
        u.write(0x6, WriteSize::Half, 0).expect("a legal halfword");
        // A plain out-of-window offset is rejected too.
        let err = u.write(0x10, WriteSize::Byte, 0).unwrap_err();
        assert_eq!(err.scenario, 2);
        let err = u.read(0x10, WriteSize::Byte).unwrap_err();
        assert_eq!(err.scenario, 2);
    }

    /// `RHR` 是**唯一**的读破坏寄存器：读走它，RX FIFO 就空了。
    #[test]
    fn reading_the_rhr_consumes_the_rx_byte() {
        let mut u = uart();
        u.receive(b'Z');
        assert_eq!(u.read_clear(reg::THR, WriteSize::Byte).unwrap(), 0x5A);
        assert_eq!(
            u.read_clear(reg::THR, WriteSize::Byte).unwrap(),
            0,
            "the RX FIFO is empty now"
        );
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(lsr & u32::from(lsr_bits::RX_VALID), 0);
    }

    /// `FCR` 的 RX FIFO 复位位清掉 RX 字节。
    #[test]
    fn the_fifo_control_reset_bit_clears_the_rx_fifo() {
        let mut u = uart();
        u.receive(b'Z');
        u.write(
            reg::FCR,
            WriteSize::Byte,
            u32::from(fcr_bits::RX_FIFO_RESET),
        )
        .expect("fcr");
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(lsr & u32::from(lsr_bits::RX_VALID), 0);
    }

    /// ⚠️ **osz 的真实序列**：`hal_uart_set_baudrate()` + `hal_uart_putc()`。
    ///
    /// 除数锁存器的两个写**不许**进 TX 流；`putc` 的那一个字节**必须**
    /// 进。这就是「两侧 UART 字节流逐字节比对」的第一颗钉子。
    #[test]
    fn the_osz_baud_rate_setup_does_not_pollute_the_tx_stream() {
        let mut u = uart();
        // hal_uart_set_baudrate(9600)
        u.write(reg::LCR, WriteSize::Byte, u32::from(lcr_bits::DLAB))
            .expect("lcr dlab");
        u.write(reg::DLL, WriteSize::Byte, 0x18).expect("dll");
        u.write(reg::DLM, WriteSize::Byte, 0x00).expect("dlm");
        u.write(reg::LCR, WriteSize::Byte, 0x00).expect("lcr 0");
        // hal_uart_putc('A')
        u.write(reg::THR, WriteSize::Byte, u32::from(b'A'))
            .expect("putc");

        assert_eq!(
            u.drain_tx(),
            vec![b'A'],
            "the TX stream must carry the putc byte and nothing else"
        );
    }

    // -------------------------------------------------------------------
    // 二、无损 TX 捕获环
    // -------------------------------------------------------------------

    /// ⚠️ **核心不变式**：环没满时，guest 写进去的每一个字节都必须能被
    /// 宿主取到，顺序不变、不丢不重。
    ///
    /// N 取 3000（> 4096 的一半）⇒ 覆盖「环被用掉大半」的情形。
    #[test]
    fn the_tx_capture_ring_delivers_every_byte_in_order() {
        let mut u = uart();
        let n = 3000usize;
        for i in 0..n {
            u.write(reg::THR, WriteSize::Byte, (i % 256) as u32)
                .expect("tx");
        }
        assert_eq!(u.tx_pending(), n, "every byte is pending");
        let drained = u.drain_tx();
        assert_eq!(drained.len(), n, "not one byte is lost");
        for (i, byte) in drained.iter().enumerate() {
            assert_eq!(*byte, (i % 256) as u8, "byte {i} is out of order");
        }
        assert_eq!(u.tx_pending(), 0, "the ring is empty after the drain");
    }

    /// ⚠️ **环满时的策略**：丢**最新**的那个字节（保留最早的），并在
    /// `LSR` 的 bit3 锁存一次 overrun。
    #[test]
    fn a_full_capture_ring_drops_the_newest_byte_and_latches_the_overrun() {
        let mut u = Uart16550::new_with_tx_capacity("UART0", 0x1000_0000, 115_200, 4, 4);
        for b in *b"abcd" {
            u.write(reg::THR, WriteSize::Byte, u32::from(b))
                .expect("tx");
        }
        // 环满了：第五个字节被丢掉。
        u.write(reg::THR, WriteSize::Byte, u32::from(b'e'))
            .expect("tx");
        assert_eq!(u.tx_pending(), 4, "the ring holds exactly its capacity");
        assert_eq!(u.tx_dropped(), 1, "exactly one byte was dropped");
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(
            lsr & u32::from(lsr_bits::TX_OVERRUN),
            0x08,
            "the overrun is latched in LSR bit 3"
        );
        assert_eq!(
            lsr & u32::from(lsr_bits::TX_IDLE),
            0,
            "a full ring means the transmitter cannot take another byte"
        );
        // 留下的是**最早**的四个字节。
        assert_eq!(u.drain_tx(), vec![b'a', b'b', b'c', b'd']);
        // W1C 清掉 overrun。
        u.write(reg::LSR, WriteSize::Byte, u32::from(lsr_bits::TX_OVERRUN))
            .expect("w1c");
        let lsr = u.read(reg::LSR, WriteSize::Byte).unwrap();
        assert_eq!(lsr & u32::from(lsr_bits::TX_OVERRUN), 0);
    }

    /// ⚠️ **D-12 的 TX 探针不许丢**：每**进环**一个字节 = 一次脉冲，
    /// 值 = 字节本身。
    #[test]
    fn every_transmitted_byte_reports_one_probe_pulse() {
        let (log, sink) = recording_probe();
        let mut u = uart();
        u.on_probe(Some(sink));
        u.tick(500).expect("tick");

        for b in *b"AAB" {
            u.write(reg::THR, WriteSize::Byte, u32::from(b))
                .expect("tx");
        }
        let log = log.borrow();
        assert_eq!(log.len(), 3, "one pulse per transmitted byte");
        assert_eq!(log[0].0, SIGNAL_TX);
        assert_eq!(log[0].1, u64::from(b'A'));
        assert_eq!(log[0].2, 500, "the timestamp is the peripheral's clock");
        assert_eq!(log[1].1, u64::from(b'A'), "two equal bytes are two pulses");
        assert_eq!(log[2].1, u64::from(b'B'));
    }

    /// 被**丢掉**的字节没有发出去 ⇒ 不上报脉冲。
    #[test]
    fn a_dropped_byte_reports_no_probe_pulse() {
        let (log, sink) = recording_probe();
        let mut u = Uart16550::new_with_tx_capacity("UART0", 0x1000_0000, 115_200, 4, 1);
        u.on_probe(Some(sink));
        u.write(reg::THR, WriteSize::Byte, u32::from(b'a'))
            .expect("tx");
        u.write(reg::THR, WriteSize::Byte, u32::from(b'b'))
            .expect("tx");
        assert_eq!(log.borrow().len(), 1, "only the byte that went out");
        assert_eq!(log.borrow()[0].1, u64::from(b'a'));
    }

    /// 旧的单字节出口还在，但它**不是唯一的出口**。
    #[test]
    fn take_tx_drains_one_byte_and_drain_tx_drains_them_all() {
        let mut u = uart();
        for b in *b"123" {
            u.write(reg::THR, WriteSize::Byte, u32::from(b))
                .expect("tx");
        }
        assert_eq!(u.take_tx(), Some(b'1'), "the oldest byte first");
        assert_eq!(u.tx_pending(), 2);
        assert_eq!(u.drain_tx(), vec![b'2', b'3'], "the rest, in order");
        assert_eq!(u.take_tx(), None, "the ring is empty");
    }

    // -------------------------------------------------------------------
    // 三、会话快照
    // -------------------------------------------------------------------

    /// 快照里的 `tx` 是捕获环的**队头**，`status` 是 **LSR**。
    #[test]
    fn the_session_state_carries_the_capture_head_and_the_line_status() {
        let mut u = uart();
        u.write(reg::THR, WriteSize::Byte, u32::from(b'K'))
            .expect("tx");
        u.write(reg::THR, WriteSize::Byte, u32::from(b'L'))
            .expect("tx");
        u.receive(b'Z');
        u.tick(1_234).expect("tick");

        let exported = u.session_state().expect("the UART participates");
        let PeripheralSessionState::Uart(state) = exported.clone() else {
            panic!("a UART must export a UART state");
        };
        assert_eq!(state.tx, Some(b'K'), "the head of the capture ring");
        assert_eq!(state.rx, Some(b'Z'));
        assert_eq!(
            state.status, 0x63,
            "LSR = RX_VALID(0x01) | THRE(0x02) | TX_IDLE(0x20) | TEMT(0x40)"
        );
        assert_eq!(state.now_ps, 1_234);

        // 还原之后逐字段相等（捕获环只剩队头，LSR 的各位不变）。
        let mut fresh = Uart16550::new("UART0", 0x1000_0000, 115_200, 0);
        fresh
            .restore_session_state(&exported)
            .expect("the UART must accept its own state");
        assert_eq!(fresh.session_state(), Some(exported));
    }

    /// `LSR` 的 RX_VALID 与 RX FIFO 自相矛盾的状态必须被拒。
    #[test]
    fn a_state_whose_line_status_contradicts_its_rx_fifo_is_rejected() {
        let u = uart();
        let bad = PeripheralSessionState::Uart(UartStateDto {
            id: "UART0".to_string(),
            tx: None,
            rx: None,
            // RX_VALID 置位，可 RX FIFO 是空的。
            status: 0x01,
            irq_line: 4,
            now_ps: 0,

            ..UartStateDto::default()
        });
        let err = u
            .validate_session_state(&bad)
            .expect_err("a contradictory state must be rejected");
        assert_eq!(
            err.scenario,
            super::super::E_PER_04_UNSUPPORTED_SESSION_STATE
        );
    }

    /// `LSR` 里出现本模型没有的位 ⇒ 拒（契约层查不了这条）。
    #[test]
    fn a_state_with_undefined_line_status_bits_is_rejected() {
        let u = uart();
        let bad = PeripheralSessionState::Uart(UartStateDto {
            id: "UART0".to_string(),
            tx: None,
            rx: None,
            status: 0x8000,
            irq_line: 4,
            now_ps: 0,

            ..UartStateDto::default()
        });
        let err = u
            .validate_session_state(&bad)
            .expect_err("undefined LSR bits must be rejected");
        assert_eq!(
            err.scenario,
            super::super::E_PER_04_UNSUPPORTED_SESSION_STATE
        );
    }
}
