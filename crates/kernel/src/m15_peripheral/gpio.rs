//! GpioBank — generic 32-pin GPIO bank (MVP).
//!
//! Register window:
//! - `0x00 DIR`  (rw): per-pin direction (1 = output, 0 = input).
//!   Used by the edge-detection logic to decide whether an external
//!   input change should latch a pending IRQ (M-15 review #7).
//! - `0x04 OUT`  (rw): output latch. Writing a pin that is configured
//!   as input is allowed but has no effect on the pin (the latch
//!   retains the value).
//! - `0x08 IN`   (ro): reflects the input pin values; writable from
//!   the host via [`GpioBank::set_external_inputs`] for button-press
//!   simulation. Writes from firmware are no-ops.
//! - `0x0C TOGGLE` (wo): atomic toggle — each bit set in `value`
//!   flips the corresponding pin's `out` in one shot (M-15 review #3).
//!   Reads return `0`. Example: `value = 1<<3 | 1<<0` flips pins 0
//!   and 3 atomically.
//! - `0x10 PENDING` (rw1c): sticky edge-triggered pending bitmap.
//!   Reading returns every pin that still has an unhandled edge; writing
//!   a `1` clears **only** those bits (write `0` is a no-op). This follows
//!   Renode's GPIO models: the peripheral owns the pending bits and the
//!   guest driver consumes them with W1C — the host must not clear them.
//!
//! IRQ: the bank is **one** interrupt source for the whole port. Any edge
//! on any pin latches a sticky bit in `pending_irqs`; the **host only
//! reflects that bitmap** onto the bank's single PLIC source (set when
//! non-zero, cleared when zero). The source number lives in the platform
//! description, not in this peripheral's state.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{PendingWrite, WriteSize};
use riscv_sim_contract::port::{Peripheral, ProbeSink};
use riscv_sim_contract::session::{GpioStateDto, PeripheralSessionState, GPIO_PIN_COUNT};

use super::reg::{extract_subword, register_offset};
use super::ProbeSlot;

pub const GPIO_WINDOW: u32 = 0x14;

/// Probe signal name for the output latch (D-12 / N5).
///
/// The M-18 channel is bound to `(peripheral id, signal)` — the signal
/// name is what the peripheral reports and what the channel definition
/// carries. `"OUT"` follows the contract's `ProbeSink` documentation
/// (M-18 v0.8 §18-2: "Signal id is a stable string per peripheral
/// (e.g. `"OUT"`, `"TX"`)") and the M-18 LLD's own channel examples.
///
/// The reported **value** is the pin bitmap (bit `n` = pin `n`), not a
/// single bit — a 32-pin bank needs 32 bits of information. A channel
/// bound to this signal should therefore be a multi-bit channel
/// (M-18 LLD: "ch=1 GPIO0 … valueWidth = 按端口位宽"); a single-bit
/// channel only ever sees pin 0 (M-18 masks the value to its LSB).
pub const SIGNAL_OUT: &str = "OUT";

/// Number of pins in the bank.
///
/// ⚠️ **One number, one home** — the contract layer owns it
/// ([`GPIO_PIN_COUNT`]); the model here just follows.
const PIN_COUNT: u32 = GPIO_PIN_COUNT as u32;
/// Mask of all valid pin bits for a 32-pin bank. MVP: bank is fixed at
/// 32 pins so the mask is simply `u32::MAX`. Reserved for narrower banks
/// (P1 evolution 评审 R5).
const PIN_COUNT_MASK: u32 = u32::MAX;

pub mod reg {
    pub const DIR: u32 = 0x0;
    pub const OUT: u32 = 0x4;
    pub const IN: u32 = 0x8;
    pub const TOGGLE: u32 = 0xC;
    /// Sticky pending bitmap — **read**: unhandled pins; **write**: W1C.
    pub const PENDING: u32 = 0x10;
}

#[derive(Debug)]
pub struct GpioBank {
    base: u32,
    id: String,
    directions: u32, // 1 = output
    outputs: u32,
    inputs: u32,
    /// Sticky edge-triggered pending IRQ vector. Low bit = pin 0.
    pending_irqs: u32,
    /// M-18 probe callback (D-12). Empty = no observer attached.
    probe: ProbeSlot,
    /// Last virtual time this peripheral observed (ps).
    ///
    /// The probe report has to carry a timestamp, but the contract's
    /// `Peripheral::write` / `read` carry no time argument. The
    /// authoritative clock is M-12's, and M-12 reaches the peripheral
    /// through `Peripheral::tick(now_ps)` once per quantum — so the
    /// peripheral remembers the last time it was ticked and stamps its
    /// probe reports with it (D-07 wires M-12 → `tick`).
    now_ps: u64,
}

impl GpioBank {
    pub fn new(id: impl Into<String>, base: u32) -> Self {
        Self {
            base,
            id: id.into(),
            directions: 0,
            outputs: 0,
            inputs: 0,
            pending_irqs: 0,
            probe: ProbeSlot::default(),
            now_ps: 0,
        }
    }

    /// **当前输入电平**（只读 · v0.74 · STAGE-50 · 引脚级用例用）。
    ///
    /// ⚠️ **为什么需要它**：用例要「**翻转**某个引脚的电平」来制造边沿
    ///   ⇒ ⇒ 而 `inputs` 是**私有**的 ⇒ ⇒ ⇒ 给一个**只读**入口（⇒ ⇒ 不
    ///   破坏封装）。
    pub fn inputs(&self) -> u32 {
        self.inputs
    }

    /// Set the pin direction bitmask (1 = output, 0 = input).
    pub fn set_directions(&mut self, mask: u32) {
        self.directions = mask & PIN_COUNT_MASK;
    }

    /// Drive external inputs (simulate button press / sensor value).
    /// Latches a pending IRQ on every edge — but only for pins that
    /// are configured as **inputs** (direction bit = 0). Output-pin
    /// transitions never come from outside, so they cannot trigger
    /// input-edge IRQs (M-15 review #7).
    pub fn set_external_inputs(&mut self, value: u32) {
        let value = value & PIN_COUNT_MASK;
        let prev = self.inputs;
        self.inputs = value;
        // Edge-triggered only on currently-configured inputs. Outputs
        // are driven by firmware via OUT/TOGGLE; their transitions
        // produce output-edge IRQs through `write(OUT/TOGGLE)`.
        let input_mask = !self.directions & PIN_COUNT_MASK;
        let changed = (prev ^ value) & input_mask;
        self.latch_edges(changed);
    }

    /// **当前 pending 位图**（**只读**）。
    ///
    /// 宿主用它决定要不要抬/撤整块 GPIO 的那一个 PLIC 源；它**不**消费
    /// 任何位 —— 消费只走 guest 对 [`reg::PENDING`] 的 W1C 写。
    pub fn pending_irqs(&self) -> u32 {
        self.pending_irqs
    }

    /// **该引脚有没有 pending 活动**（**只读**）。
    pub fn pending_pin(&self, pin: u32) -> bool {
        if pin >= PIN_COUNT {
            return false;
        }
        (self.pending_irqs >> pin) & 1 == 1
    }

    /// Internal: latch a sticky pending IRQ for every pin with a set
    /// bit in `changed`.
    ///
    /// 锁存与中断号**无关**：沿 ⇒ 置位（方向过滤在调用方：外部喂入只
    /// 对 input 引脚、OUT/TOGGLE 对任意引脚）；整块 GPIO 共用一个 PLIC
    /// 源号，源号由板级描述决定。
    fn latch_edges(&mut self, changed: u32) {
        self.pending_irqs |= changed & PIN_COUNT_MASK;
    }

    /// Effective pin level as observed by firmware reading the IN
    /// register. M-15 review N2: previously IN returned only
    /// `self.inputs`, so firmware writing OUT to drive a pin could
    /// never read the level back. Now: for output-direction pins we
    /// expose the OUT latch; for input-direction pins we expose the
    /// last external drive. This matches the
    /// "data direction register controls who drives the pin" mental
    /// model used by PL061 / STM32 GPIO and is required for the M-18
    /// waveform pipeline + E2E-01 N5 "GPIO 翻转可见" to actually
    /// observe firmware-driven edges.
    fn pin_levels(&self) -> u32 {
        let output_pins = self.outputs & self.directions & PIN_COUNT_MASK;
        let input_pins = self.inputs & !self.directions & PIN_COUNT_MASK;
        output_pins | input_pins
    }
}

/// Register helpers (register_offset / extract_subword) live in
/// `super::reg` (M-15 review N1 DRY consolidation).
impl Peripheral for GpioBank {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_addr(&self) -> u32 {
        self.base
    }
    fn size(&self) -> u32 {
        GPIO_WINDOW
    }

    fn read(&self, offset: u32, size: WriteSize) -> Result<u32, PlatformError> {
        let word = self.read_word(register_offset(offset))?;
        Ok(extract_subword(word, offset, size))
    }

    fn write(&mut self, offset: u32, _size: WriteSize, value: u32) -> Result<(), PlatformError> {
        match register_offset(offset) {
            reg::DIR => {
                self.directions = value & PIN_COUNT_MASK;
                Ok(())
            }
            reg::OUT => {
                let prev = self.outputs;
                self.outputs = value & PIN_COUNT_MASK;
                // Output-pin transitions latch output-edge IRQs (any
                // configured pin, regardless of direction — the OUT
                // latch is firmware-controlled).
                self.latch_edges(prev ^ self.outputs);
                // D-12: report the new pin level to the probe
                // pipeline. Only on an actual change — an unchanged
                // level must not spam the waveform.
                if prev != self.outputs {
                    self.report(SIGNAL_OUT, self.outputs);
                }
                Ok(())
            }
            reg::TOGGLE => {
                // M-15 review #3: mask-toggle semantics. Each set bit
                // in `value` flips that pin's `out` atomically:
                //   value = 1<<3 | 1<<0 → flip pins 0 and 3.
                // Reading this register returns 0 (write-only).
                let mask = value & PIN_COUNT_MASK;
                if mask != 0 {
                    let prev = self.outputs;
                    self.outputs ^= mask;
                    self.latch_edges(mask);
                    // D-12: a toggle is a level change too.
                    if prev != self.outputs {
                        self.report(SIGNAL_OUT, self.outputs);
                    }
                }
                Ok(())
            }
            reg::PENDING => {
                // W1C: write 1 clears the corresponding sticky bit;
                // write 0 leaves it untouched.
                self.pending_irqs &= !(value & PIN_COUNT_MASK);
                Ok(())
            }
            reg::IN => {
                // MVP: IN is read-only; writes from firmware are a no-op
                // (the firmware observes, it does not drive inputs).
                Ok(())
            }
            other => Err(PlatformError::new(
                ErrorFamily::Peripheral,
                2,
                format!("GPIO offset 0x{:x} not defined", other),
            )),
        }
    }

    fn tick(&mut self, now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        // The engine owns the virtual clock (M-12); `tick` is how it
        // reaches the peripheral. Remembering the time here is what
        // lets the probe reports carry a meaningful timestamp.
        self.now_ps = now_ps;
        Ok(Vec::new())
    }

    /// Install / replace the M-18 probe callback (D-12).
    fn on_probe(&mut self, cb: Option<ProbeSink>) {
        self.probe.set(cb);
    }

    /// This GPIO bank's share of the **session snapshot** (segment 4 —
    /// STAGE-3 §3.3).
    ///
    /// The IRQ source number is **not** part of this state: it belongs
    /// to the board description, not to the peripheral's runtime.
    fn session_state(&self) -> Option<PeripheralSessionState> {
        Some(PeripheralSessionState::Gpio(GpioStateDto {
            id: self.id.clone(),
            directions: self.directions,
            outputs: self.outputs,
            inputs: self.inputs,
            pending_irqs: self.pending_irqs,
            now_ps: self.now_ps,
        }))
    }

    /// Restore the GPIO bank's session state (segment 4's consumer).
    ///
    /// **Every** field is restored: the direction mask, the output
    /// latch, the input latch, the sticky pending-IRQ vector and
    /// `now_ps`.
    ///
    /// ⚠️ The same id check is also run **before** the write stage
    /// ([`Peripheral::validate_session_state`]) — the check here is
    /// **defensive redundancy**, not the gate. The gate is the
    /// read-only pass, because a check that only lives here would let
    /// the restore write the *other* peripherals first and then fail
    /// (STAGE-3 §5).
    fn restore_session_state(
        &mut self,
        state: &PeripheralSessionState,
    ) -> Result<(), PlatformError> {
        let PeripheralSessionState::Gpio(s) = state else {
            return Err(super::wrong_state_kind(
                &self.id,
                super::SessionKind::Gpio,
                super::SessionKind::of(state),
            ));
        };
        super::check_same_id(&self.id, &s.id)?;
        self.directions = s.directions & PIN_COUNT_MASK;
        self.outputs = s.outputs & PIN_COUNT_MASK;
        self.inputs = s.inputs & PIN_COUNT_MASK;
        self.pending_irqs = s.pending_irqs & PIN_COUNT_MASK;
        self.now_ps = s.now_ps;
        Ok(())
    }

    /// **Read-only** acceptance check for this bank's session state
    /// (STAGE-3 §3.3) — the gate that runs **before** anything is
    /// written.
    ///
    /// ⚠️ The rule that lives **only here**: the state must name this
    /// bank (same id). The rest of the shape is the contract's.
    fn validate_session_state(&self, state: &PeripheralSessionState) -> Result<(), PlatformError> {
        let PeripheralSessionState::Gpio(s) = state else {
            return Err(super::wrong_state_kind(
                &self.id,
                super::SessionKind::Gpio,
                super::SessionKind::of(state),
            ));
        };
        super::check_same_id(&self.id, &s.id)?;
        Ok(())
    }
}

impl GpioBank {
    /// Report one signal observation to the M-18 probe pipeline.
    ///
    /// The value is the pin bitmap; the timestamp is the last virtual
    /// time the engine ticked this peripheral (see [`GpioBank::now_ps`]).
    fn report(&mut self, signal: &str, value: u32) {
        let now = self.now_ps;
        self.probe.report(signal, u64::from(value), now);
    }

    fn read_word(&self, word_offset: u32) -> Result<u32, PlatformError> {
        match word_offset {
            reg::DIR => Ok(self.directions),
            reg::OUT => Ok(self.outputs),
            reg::IN => Ok(self.pin_levels()), // M-15 review N2: IN reflects
            // output latch on output-direction
            // pins so firmware can read back
            // what it drove.
            reg::PENDING => Ok(self.pending_irqs),
            reg::TOGGLE => Ok(0), // write-only register
            other => Err(PlatformError::new(
                ErrorFamily::Peripheral,
                2,
                format!("GPIO offset 0x{:x} not defined", other),
            )),
        }
    }
}
