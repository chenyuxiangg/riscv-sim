//! Uart16550 — minimal behaviour-level UART (评审 15-1, 15-7, 15-9).
//!
//! MVP scope:
//! - 1-byte TX/RX FIFO each (评审 15-9: smaller than the hardware 16-byte
//!   queue). Two writes to DATA before the host reads trigger an overrun.
//! - 8-byte register window; sub-register byte/halfword/word reads from
//!   `offset` are supported via `offset >> 2` register selection and
//!   `size`-driven extraction (M-15 review #5).
//! - Bit-clock divisor = `1e9 / baud_rate` picoseconds-per-bit
//!   (评审 15-1), floored at 1 ps to avoid zero-length frames
//!   (M-15 review #15). MVP does NOT advance virtual time on writes —
//!   the caller schedules `Peripheral::tick(now_ps)` themselves.
//! - TX pin level reported through `tx_pin()` for probe wiring
//!   (evaluated at any virtual time: idle=`true`, busy=`false` while
//!   a TX byte is in flight). TX bytes are drained via [`take_tx`]
//!   (M-15 review #1).
//! - No flow-control beyond FIFO overruns (评审 15-7 simplified).
//!
//! IRQ line (set via [`set_irq_line`]): raised when the RX FIFO
//! receives a byte. MVP does NOT implement TX-empty IRQs.
//!
//! Read-clear semantics (M-15 review #2): UART RX FIFO is read via
//! `read_clear` (default-trait method), which clears the byte after
//! returning it. `read` remains non-destructive for parity with
//! other peripherals.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{PendingWrite, WriteSize};
use riscv_sim_contract::port::{Peripheral, ProbeSink};

use super::reg::{extract_subword, register_offset};
use super::ProbeSlot;

/// Register window is 8 bytes wide (评审 15-9).
pub const UART_WINDOW: u32 = 0x8;

/// Probe signal name for a transmitted byte (D-12 / N5).
///
/// The reported **value** is the byte itself (0x00..=0xFF). Every
/// transmitted byte is one event, so the channel bound to this signal
/// should be a **pulse** channel (M-18 `ChannelKind::Pulse`): a
/// multi-bit channel would swallow a repeated byte (same value = no
/// edge), and two identical bytes in a row are two distinct events.
pub const SIGNAL_TX: &str = "TX";

/// Register offsets (word-aligned; sub-register accesses map via
/// `offset >> 2` per M-15 review #5).
mod reg {
    pub const DATA: u32 = 0x0;
    pub const STATUS: u32 = 0x4;
}

/// STATUS bits (read-only; cleared via W1C writes per M-15 review #14).
pub mod status_bits {
    /// TX register empty (ready for next byte).
    pub const TX_READY: u32 = 0x1;
    /// RX register full.
    pub const RX_FULL: u32 = 0x2;
    /// RX overrun — host wrote a 2nd byte before reading the 1st.
    pub const RX_OVERRUN: u32 = 0x4;
    /// TX overrun — firmware wrote a 2nd byte before the host drained
    /// the 1st (M-15 review #4: was previously folded into RX_OVERRUN).
    pub const TX_OVERRUN: u32 = 0x8;
}

#[derive(Debug)]
pub struct Uart16550 {
    base: u32,
    id: String,
    baud_rate: u32,
    /// TX FIFO (1 byte deep, MVP).
    tx: Option<u8>,
    /// RX FIFO (1 byte deep, MVP).
    rx: Option<u8>,
    /// Latched status register.
    status: u32,
    /// IRQ line to raise on RX.
    irq_line: u16,
    /// M-18 probe callback (D-12). Empty = no observer attached.
    probe: ProbeSlot,
    /// Last virtual time this peripheral observed (ps) — see
    /// [`GpioBank::now_ps`](super::gpio::GpioBank) for the rationale.
    now_ps: u64,
}

impl Uart16550 {
    /// Construct a UART at `base` running at `baud_rate` bps.
    pub fn new(id: impl Into<String>, base: u32, baud_rate: u32, irq_line: u16) -> Self {
        let mut s = Self {
            base,
            id: id.into(),
            baud_rate,
            tx: None,
            rx: None,
            status: 0,
            irq_line,
            probe: ProbeSlot::default(),
            now_ps: 0,
        };
        s.refresh_status();
        s
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

    /// Drive `byte` into the TX FIFO (firmware wrote DATA).
    pub fn transmit(&mut self, byte: u8) {
        if self.tx.is_some() {
            // MVP 1-byte FIFO: drop the new byte and mark overrun so
            // the firmware can poll the flag.
            self.refresh_status_after_tx_overrun();
            return;
        }
        self.tx = Some(byte);
        self.refresh_status();
        // D-12: a transmitted byte is a waveform event.
        self.report(SIGNAL_TX, u64::from(byte));
    }

    /// Drain the TX FIFO (host bridge consumes the byte the firmware
    /// emitted). Returns the byte and clears the TX_READY-=1 condition.
    /// M-15 review #1: this is the missing producer-side sink.
    pub fn take_tx(&mut self) -> Option<u8> {
        let byte = self.tx.take();
        self.refresh_status();
        byte
    }

    /// Receive `byte` from the wire (host-supplied — the firmware will
    /// read DATA later).
    pub fn receive(&mut self, byte: u8) {
        if self.rx.is_some() {
            self.refresh_status_after_rx_overrun();
            return;
        }
        self.rx = Some(byte);
        self.refresh_status();
    }

    /// TX pin level: `true` when idle (no byte in flight), `false`
    /// while a TX byte is pending (MVP: "pending" = `tx.is_some()`).
    pub fn tx_pin(&self) -> bool {
        self.tx.is_none()
    }

    /// RX pin level (debug — we don't model an electrical line in MVP).
    pub fn rx_pin(&self) -> bool {
        self.rx.is_none()
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

    /// Recompute the STATUS register from the FIFO state. This is
    /// the **single** writer for STATUS bits other than overrun
    /// flags (M-15 review #16).
    fn refresh_status(&mut self) {
        let mut s = 0u32;
        if self.tx.is_none() {
            s |= status_bits::TX_READY;
        }
        if self.rx.is_some() {
            s |= status_bits::RX_FULL;
        }
        // Preserve latched overrun flags; only refresh the FIFO flags.
        // (Caller mutates overrun bits via W1C; see refresh_status_after_*.)
        s |= self.status & (status_bits::RX_OVERRUN | status_bits::TX_OVERRUN);
        self.status = s;
    }

    fn refresh_status_after_tx_overrun(&mut self) {
        self.status |= status_bits::TX_OVERRUN;
        // TX_READY stays 0 while FIFO holds a byte; refresh keeps that.
        self.refresh_status();
    }

    fn refresh_status_after_rx_overrun(&mut self) {
        self.status |= status_bits::RX_OVERRUN;
        self.refresh_status();
    }
}

/// Extract a sub-register value from a 32-bit word read. `offset`
/// within the word selects which byte/halfword the firmware wants.
/// M-15 review #5:固件对 4 字节寄存器窗口内做 `lb`/`sb` 不应拿到虚假
/// 的 E_PER-02;`offset & 0x3` 选字节, `WriteSize` 决定提取粒度.
///
/// **Moved to `super::reg::extract_subword` in M-15 review N1**
/// (DRY consolidation across UART/GPIO/Timer).
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

    fn read(&self, offset: u32, size: WriteSize) -> Result<u32, PlatformError> {
        let word = self.read_word(register_offset(offset))?;
        Ok(extract_subword(word, offset, size))
    }

    fn read_clear(&mut self, offset: u32, size: WriteSize) -> Result<u32, PlatformError> {
        let word = self.read_word_mut(register_offset(offset))?;
        Ok(extract_subword(word, offset, size))
    }

    fn write(&mut self, offset: u32, _size: WriteSize, value: u32) -> Result<(), PlatformError> {
        let reg = register_offset(offset);
        match reg {
            reg::DATA => {
                // MVP DATA is byte-oriented; ignore size ambiguity and
                // take the low byte.
                self.transmit(value as u8);
                Ok(())
            }
            reg::STATUS => {
                // W1C (write-1-to-clear) for overrun flags
                // (M-15 review #14): firmware writes 1 to clear the
                // matching bit. Writing 0 to an overrun bit leaves it.
                if value & status_bits::RX_OVERRUN != 0 {
                    self.status &= !status_bits::RX_OVERRUN;
                }
                if value & status_bits::TX_OVERRUN != 0 {
                    self.status &= !status_bits::TX_OVERRUN;
                }
                self.refresh_status();
                Ok(())
            }
            other => Err(PlatformError::new(
                ErrorFamily::Peripheral,
                2,
                format!("UART offset 0x{:x} not defined", other),
            )),
        }
    }

    fn tick(&mut self, now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        // No time-driven side-effects beyond RX FIFO state (the host
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
}

// Internal helpers: the non-destructive `read` returns the word at
// the word-aligned register offset; `read_word_mut` is the read-clear
// variant that mutates FIFO state. Centralised so the `read` and
// `read_clear` paths share error mapping.
impl Uart16550 {
    /// Report one signal observation to the M-18 probe pipeline.
    fn report(&mut self, signal: &str, value: u64) {
        let now = self.now_ps;
        self.probe.report(signal, value, now);
    }

    fn read_word(&self, word_offset: u32) -> Result<u32, PlatformError> {
        match word_offset {
            reg::DATA => Ok(self.rx.map(u32::from).unwrap_or(0)),
            reg::STATUS => Ok(self.status),
            other => Err(PlatformError::new(
                ErrorFamily::Peripheral,
                2,
                format!("UART offset 0x{:x} not defined", other),
            )),
        }
    }

    fn read_word_mut(&mut self, word_offset: u32) -> Result<u32, PlatformError> {
        match word_offset {
            reg::DATA => {
                let byte = self.rx.take();
                self.refresh_status();
                Ok(byte.map(u32::from).unwrap_or(0))
            }
            reg::STATUS => Ok(self.status),
            other => Err(PlatformError::new(
                ErrorFamily::Peripheral,
                2,
                format!("UART offset 0x{:x} not defined", other),
            )),
        }
    }
}
