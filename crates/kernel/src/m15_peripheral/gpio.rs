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
//!
//! IRQ: each pin can raise its own IRQ on either edge (configurable
//! via [`GpioBank::set_irq_line_for_pin`]). MVP does NOT mask: each
//! edge latches a sticky bit in `pending_irqs`; `take_pending_irqs`
//! consumes the pending vector and **deduplicates** by IRQ line so
//! that multi-pin → same-line bindings do not flood the consumer
//! (M-15 review #13).

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{PendingWrite, WriteSize};
use riscv_sim_contract::port::{Peripheral, ProbeSink};
use riscv_sim_contract::session::{GpioStateDto, PeripheralSessionState, GPIO_PIN_COUNT};

use super::reg::{extract_subword, register_offset};
use super::ProbeSlot;

pub const GPIO_WINDOW: u32 = 0x10;

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
/// ([`GPIO_PIN_COUNT`]). The number is spelled out in the *contract*
/// because the **payload's** shape depends on it (the per-pin IRQ list
/// must have exactly this many entries); the model here just follows.
const PIN_COUNT: u32 = GPIO_PIN_COUNT as u32;
/// Mask of all valid pin bits for a 32-pin bank. MVP: bank is fixed at
/// 32 pins so the mask is simply `u32::MAX`. Reserved for narrower banks
/// (P1 evolution 评审 R5).
const PIN_COUNT_MASK: u32 = u32::MAX;

mod reg {
    pub const DIR: u32 = 0x0;
    pub const OUT: u32 = 0x4;
    pub const IN: u32 = 0x8;
    pub const TOGGLE: u32 = 0xC;
}

#[derive(Debug)]
pub struct GpioBank {
    base: u32,
    id: String,
    directions: u32, // 1 = output
    outputs: u32,
    inputs: u32,
    /// Per-pin IRQ line (16-bit; 0xFFFF = no IRQ).
    irq_lines: [u16; PIN_COUNT as usize],
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
            irq_lines: [0xFFFF; PIN_COUNT as usize],
            pending_irqs: 0,
            probe: ProbeSlot::default(),
            now_ps: 0,
        }
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

    /// Take the pending-IRQ pin mask and clear it. Returns a
    /// deduplicated list of IRQ lines (M-15 review #13): if multiple
    /// pins bind to the same line, that line appears once.
    pub fn take_pending_irqs(&mut self) -> Vec<u16> {
        let pending = std::mem::take(&mut self.pending_irqs);
        // M-15 review N3: bit-set of lines already emitted. Covers
        // `line ∈ [0, 32)` (was `[0, 16)` — `line ∈ [16, 32)` previously
        // bypassed dedup entirely, contradicting the documented "the
        // IRQ line per pin is unique" contract).
        let mut seen: u32 = 0;
        let mut out = Vec::new();
        for pin in 0..PIN_COUNT {
            if (pending >> pin) & 1 == 1 {
                if let Some(&line) = self.irq_lines.get(pin as usize) {
                    if line != 0xFFFF && line < 32 {
                        let bit = 1u32 << line;
                        if seen & bit == 0 {
                            seen |= bit;
                            out.push(line);
                        }
                    } else if line != 0xFFFF {
                        // Lines >= 32 cannot be represented in the
                        // 32-bit dedup bitmap; emit them as-is without
                        // dedup (they are very rare in MVP — most
                        // RISC-V MCUs use line numbers < 32).
                        out.push(line);
                    }
                }
            }
        }
        out
    }

    /// Bind an IRQ line to a pin (0..32). Use `0xFFFF` to disable.
    pub fn set_irq_line_for_pin(&mut self, pin: u32, line: u16) {
        if pin < PIN_COUNT {
            self.irq_lines[pin as usize] = line;
        }
    }

    /// Internal: latch a sticky pending IRQ for every pin with a set
    /// bit in `changed`. Pins with `0xFFFF` IRQ line are skipped.
    fn latch_edges(&mut self, changed: u32) {
        for pin in 0..PIN_COUNT {
            if (changed >> pin) & 1 == 1 {
                let line = self.irq_lines[pin as usize];
                if line != 0xFFFF {
                    self.pending_irqs |= 1 << pin;
                }
            }
        }
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
    /// The per-pin IRQ lines are exported as a **list in pin order**
    /// (pin 0 first): the DTO carries a `Vec`, the bank owns a
    /// `[u16; PIN_COUNT]`, and the order is the whole meaning of the
    /// list. A reordering would silently move a pin's IRQ line onto
    /// another pin.
    fn session_state(&self) -> Option<PeripheralSessionState> {
        Some(PeripheralSessionState::Gpio(GpioStateDto {
            id: self.id.clone(),
            directions: self.directions,
            outputs: self.outputs,
            inputs: self.inputs,
            irq_lines: self.irq_lines.to_vec(),
            pending_irqs: self.pending_irqs,
            now_ps: self.now_ps,
        }))
    }

    /// Restore the GPIO bank's session state (segment 4's consumer).
    ///
    /// **Every** field is restored: the direction mask, the output
    /// latch, the input latch, the per-pin IRQ lines, the sticky
    /// pending-IRQ vector and `now_ps`.
    ///
    /// ⚠️ The IRQ-line list must have **exactly one entry per pin** —
    /// a shorter list would leave the remaining pins' lines undefined
    /// and a longer one would carry lines that do not exist. Neither
    /// is repaired silently: the state is **rejected**
    /// ([`E_PER_04_UNSUPPORTED_SESSION_STATE`]).
    ///
    /// ⚠️ The same rule is also checked **before** the write stage
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
        self.check_irq_lines(&s.irq_lines)?;
        self.directions = s.directions & PIN_COUNT_MASK;
        self.outputs = s.outputs & PIN_COUNT_MASK;
        self.inputs = s.inputs & PIN_COUNT_MASK;
        self.irq_lines.copy_from_slice(&s.irq_lines);
        self.pending_irqs = s.pending_irqs & PIN_COUNT_MASK;
        self.now_ps = s.now_ps;
        Ok(())
    }

    /// **Read-only** acceptance check for this bank's session state
    /// (STAGE-3 §3.3) — the gate that runs **before** anything is
    /// written.
    ///
    /// ⚠️ The rule that lives **only here**: the per-pin IRQ list must
    /// have **exactly one entry per pin** — no padding, no truncation.
    /// The contract layer checks the same shape rule on the *payload*
    /// (it knows the pin count), but the **peripheral** owns the
    /// meaning of the list, so the peripheral checks it too — and the
    /// peripheral's check is the one that guards the **write** stage.
    fn validate_session_state(&self, state: &PeripheralSessionState) -> Result<(), PlatformError> {
        let PeripheralSessionState::Gpio(s) = state else {
            return Err(super::wrong_state_kind(
                &self.id,
                super::SessionKind::Gpio,
                super::SessionKind::of(state),
            ));
        };
        super::check_same_id(&self.id, &s.id)?;
        self.check_irq_lines(&s.irq_lines)
    }
}

impl GpioBank {
    /// The bank's **own** integrity rule: the per-pin IRQ list must have
    /// exactly one entry per pin.
    ///
    /// ⚠️ **Neither padded nor truncated**: padding fabricates IRQ
    /// lines the firmware never wrote, truncation drops lines it did.
    /// Both are rejected with the contract's "this peripheral cannot
    /// take that state" code.
    fn check_irq_lines(&self, lines: &[u16]) -> Result<(), PlatformError> {
        if lines.len() != GPIO_PIN_COUNT {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "GPIO '{}' 的会话状态带 {} 条 IRQ 线，应为 {GPIO_PIN_COUNT} 条",
                    self.id,
                    lines.len()
                ),
            ));
        }
        Ok(())
    }

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
            reg::TOGGLE => Ok(0), // write-only register
            other => Err(PlatformError::new(
                ErrorFamily::Peripheral,
                2,
                format!("GPIO offset 0x{:x} not defined", other),
            )),
        }
    }
}
