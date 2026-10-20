//! TimerRiscv — minimal RISC-V `mtime` + `mtimecmp` timer (评审 N3).
//!
//! MVP scope:
//! - 64-bit `mtime` counter, **read-only**; advanced through
//!   [`Peripheral::tick`] (the caller passes the current virtual time).
//! - 64-bit `mtimecmp` compare register, writable (M-15 review #8:
//!   firmware sets the next-deadline value; `mtime` is owned by the
//!   engine and is not firmware-writable).
//! - When `mtime >= mtimecmp` (and `mtimecmp > 0`), [`tick`] arms a
//!   sticky pending IRQ. The caller drains it via
//!   [`TimerRiscv::take_pending_irq`].
//! - Review N3: this MVP timer is the **fallback** for projects that
//!   don't wire M-16's RISC-V `mtime` CSR. When M-16 lands, the
//!   authoritative interrupt source is the M-16 counter, and this
//!   peripheral is reduced to a no-op stub.
//!
//! Register window:
//! - `0x00` mtime_lo (ro — fw can read both halves).
//! - `0x04` mtime_hi (ro).
//! - `0x08` mtimecmp_lo (rw).
//! - `0x0C` mtimecmp_hi (rw).

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{PendingWrite, WriteSize};
use riscv_sim_contract::port::{Peripheral, ProbeSink};

use super::reg::{extract_subword, register_offset};
use super::ProbeSlot;

pub const TIMER_WINDOW: u32 = 0x10;

/// Probe signal name for a compare hit (D-12 / N5).
///
/// The reported **value** is the hit flag (1 = the comparison fired).
/// Every compare hit is one event, so the channel bound to this signal
/// should be a **pulse** channel (M-18 `ChannelKind::Pulse`).
pub const SIGNAL_CMP: &str = "CMP";

mod reg {
    pub const MTIME_LO: u32 = 0x0;
    pub const MTIME_HI: u32 = 0x4;
    pub const MTIMECMP_LO: u32 = 0x8;
    pub const MTIMECMP_HI: u32 = 0xC;
}

#[derive(Debug)]
pub struct TimerRiscv {
    base: u32,
    id: String,
    irq_line: u16,
    /// 64-bit monotonic time, owned by the engine. M-15 review #8:
    /// firmware cannot write `mtime` — only `mtimecmp`.
    mtime: u64,
    /// 64-bit next-deadline comparator, firmware-writable.
    mtimecmp: u64,
    /// Sticky pending IRQ flag; armed when `mtime >= mtimecmp` and
    /// cleared when firmware rewrites `mtimecmp` to a future deadline
    /// or when the consumer drains via [`take_pending_irq`].
    pending_irq: bool,
    /// M-18 probe callback (D-12). Empty = no observer attached.
    probe: ProbeSlot,
    /// Last virtual time this peripheral observed (ps) — see
    /// [`GpioBank::now_ps`](super::gpio::GpioBank) for the rationale.
    now_ps: u64,
}

impl TimerRiscv {
    pub fn new(id: impl Into<String>, base: u32, irq_line: u16) -> Self {
        Self {
            base,
            id: id.into(),
            irq_line,
            mtime: 0,
            mtimecmp: 0,
            pending_irq: false,
            probe: ProbeSlot::default(),
            now_ps: 0,
        }
    }

    /// Set the IRQ line used to raise on compare. Replaces any
    /// previously configured line (M-15 review #12: 统一封装风格).
    pub fn set_irq_line(&mut self, line: u16) {
        self.irq_line = line;
    }

    /// Get the currently-configured IRQ line.
    pub fn irq_line(&self) -> u16 {
        self.irq_line
    }

    pub fn mtime(&self) -> u64 {
        self.mtime
    }
    pub fn mtimecmp(&self) -> u64 {
        self.mtimecmp
    }

    /// Set the next-deadline comparator. Firmware-writable via
    /// MTIME_CMP_* registers; this convenience helper is used by tests
    /// and host bridges that want to seed the comparator without going
    /// through the register interface.
    pub fn set_mtimecmp(&mut self, val: u64) {
        self.mtimecmp = val;
        self.pending_irq = false;
    }

    /// Drain the sticky IRQ. Returns the configured [`Self::irq_line`]
    /// the one time the comparison fires; `None` otherwise.
    pub fn take_pending_irq(&mut self) -> Option<u16> {
        if self.pending_irq {
            self.pending_irq = false;
            // Re-arm: comparison remains true until firmware rewrites
            // mtimecmp to a future deadline. MVP simplification.
            Some(self.irq_line)
        } else {
            None
        }
    }

    /// Force a pending IRQ without advancing time. Test-only — gated
    /// behind `#[cfg(test)]` to keep production binaries clean
    /// (M-15 review #18).
    #[cfg(test)]
    pub fn force_irq_for_test(&mut self) {
        self.pending_irq = true;
    }
}

/// Register helpers (register_offset / extract_subword) live in
/// `super::reg` (M-15 review N1 DRY consolidation).
impl Peripheral for TimerRiscv {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_addr(&self) -> u32 {
        self.base
    }
    fn size(&self) -> u32 {
        TIMER_WINDOW
    }

    fn read(&self, offset: u32, size: WriteSize) -> Result<u32, PlatformError> {
        let word = self.read_word(register_offset(offset))?;
        Ok(extract_subword(word, offset, size))
    }

    fn write(&mut self, offset: u32, _size: WriteSize, value: u32) -> Result<(), PlatformError> {
        // Allow firmware to write the 64-bit mtimecmp register in
        // either order; we compose by storing into the appropriate
        // half. mtime is read-only (M-15 review #8): writes to
        // MTIME_* are silently ignored — firmware observes time but
        // does not drive it.
        match register_offset(offset) {
            reg::MTIME_LO | reg::MTIME_HI => Ok(()),
            reg::MTIMECMP_LO => {
                self.mtimecmp = (self.mtimecmp & 0xFFFF_FFFF_0000_0000) | u64::from(value);
                self.pending_irq = false;
                Ok(())
            }
            reg::MTIMECMP_HI => {
                self.mtimecmp = (self.mtimecmp & 0x0000_0000_FFFF_FFFF) | (u64::from(value) << 32);
                self.pending_irq = false;
                Ok(())
            }
            other => Err(PlatformError::new(
                ErrorFamily::Peripheral,
                2,
                format!("TIMER offset 0x{:x} not defined", other),
            )),
        }
    }

    fn tick(&mut self, now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        // 评审 N3: authoritative source is M-16; this peripheral is a
        // fallback. We still expose mtime advancement for projects
        // that rely on the bus-side timer.
        self.now_ps = now_ps;
        self.mtime = now_ps;
        if self.mtimecmp > 0 && self.mtime >= self.mtimecmp && !self.pending_irq {
            self.pending_irq = true;
            // D-12: the compare hit is a waveform event.
            self.report(SIGNAL_CMP, 1);
        }
        Ok(Vec::new())
    }

    /// Install / replace the M-18 probe callback (D-12).
    fn on_probe(&mut self, cb: Option<ProbeSink>) {
        self.probe.set(cb);
    }
}

impl TimerRiscv {
    /// Report one signal observation to the M-18 probe pipeline.
    fn report(&mut self, signal: &str, value: u64) {
        let now = self.now_ps;
        self.probe.report(signal, value, now);
    }

    fn read_word(&self, word_offset: u32) -> Result<u32, PlatformError> {
        match word_offset {
            reg::MTIME_LO => Ok(self.mtime as u32),
            reg::MTIME_HI => Ok((self.mtime >> 32) as u32),
            reg::MTIMECMP_LO => Ok(self.mtimecmp as u32),
            reg::MTIMECMP_HI => Ok((self.mtimecmp >> 32) as u32),
            other => Err(PlatformError::new(
                ErrorFamily::Peripheral,
                2,
                format!("TIMER offset 0x{:x} not defined", other),
            )),
        }
    }
}
