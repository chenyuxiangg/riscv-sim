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
use riscv_sim_contract::port::Peripheral;

use super::reg::{extract_subword, register_offset};

pub const GPIO_WINDOW: u32 = 0x10;
const PIN_COUNT: u32 = 32;
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
                Ok(())
            }
            reg::TOGGLE => {
                // M-15 review #3: mask-toggle semantics. Each set bit
                // in `value` flips that pin's `out` atomically:
                //   value = 1<<3 | 1<<0 → flip pins 0 and 3.
                // Reading this register returns 0 (write-only).
                let mask = value & PIN_COUNT_MASK;
                if mask != 0 {
                    self.outputs ^= mask;
                    self.latch_edges(mask);
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

    fn tick(&mut self, _now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        Ok(Vec::new())
    }
}

impl GpioBank {
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
