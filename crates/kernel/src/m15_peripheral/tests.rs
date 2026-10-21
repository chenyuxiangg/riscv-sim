//! M-15 unit tests (LLD §7 / M-15 v1.2 §7.1).
//!
//! Coverage (v0.7.1 — M-15 review remediation):
//! - UART register map + 1-byte FIFO + overrun (separate TX/RX bits) +
//!   take_tx + read_clear + W1C status + sub-register byte access
//! - GPIO directions, OUT latch, TOGGLE register (doc semantics:
//!   `1<<pin` flips), IN driven externally with direction-aware edge
//!   IRQ, deduplicated pending IRQs, sub-register byte access
//! - Timer mtime / mtimecmp advance + tick-based IRQ arming +
//!   mtime read-only enforcement + sub-register byte access
//! - PeripheralRegistry register/get/iter/describe + duplicate id
//!   detection
//! - Cross-peripheral interaction: UART TX → host bridge → GPIO LED

use super::*;
use riscv_sim_contract::model::WriteSize;
use riscv_sim_contract::port::ProbeSink;
use riscv_sim_contract::session::{
    GpioStateDto, M15PeriphDto, PeripheralSessionState, UartStateDto,
};
use std::cell::RefCell;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Probe reporting (D-12) — the M-15 → M-18 signal source layer.
// ---------------------------------------------------------------------------

/// Recorded probe observations: `(signal, value, now_ps)`.
type ProbeLog = Rc<RefCell<Vec<(String, u64, u64)>>>;

/// Install a probe sink that records every observation as
/// `(signal, value, now_ps)`.
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

#[test]
fn gpio_reports_out_changes_through_the_probe() {
    // D-12: a pin level change reaches the probe pipeline; an unchanged
    // level must not (it would spam the waveform).
    let (log, sink) = recording_probe();
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.on_probe(Some(sink));
    gpio.set_directions(0xFFFF_FFFF);
    gpio.tick(1_000).unwrap();

    // 初值 0 → 写 0：电平没变 → 不上报。
    gpio.write(0x4, WriteSize::Word, 0).unwrap();
    assert!(
        log.borrow().is_empty(),
        "an unchanged level must not report"
    );

    gpio.write(0x4, WriteSize::Word, 0b101).unwrap();
    let log = log.borrow();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].0, gpio::SIGNAL_OUT);
    assert_eq!(log[0].1, 0b101, "the reported value is the pin bitmap");
    assert_eq!(log[0].2, 1_000, "the timestamp is the peripheral's clock");
}

#[test]
fn gpio_toggle_also_reports_the_new_level() {
    let (log, sink) = recording_probe();
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.on_probe(Some(sink));
    gpio.set_directions(0xFFFF_FFFF);
    gpio.write(0xC, WriteSize::Word, 1 << 3).unwrap();
    let log = log.borrow();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].0, gpio::SIGNAL_OUT);
    assert_eq!(log[0].1, 1 << 3);
}

#[test]
fn uart_reports_tx_bytes_through_the_probe() {
    let (log, sink) = recording_probe();
    let mut uart = Uart16550::new("UART0", 0, 9600, 4);
    uart.on_probe(Some(sink));
    uart.tick(500).unwrap();

    uart.write(0x0, WriteSize::Byte, u32::from(b'A')).unwrap();
    let log = log.borrow();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].0, uart::SIGNAL_TX);
    assert_eq!(log[0].1, u64::from(b'A'), "the reported value is the byte");
    assert_eq!(log[0].2, 500);
}

#[test]
fn timer_reports_compare_hits_through_the_probe() {
    let (log, sink) = recording_probe();
    let mut tim = TimerRiscv::new("TIM0", 0, 7);
    tim.on_probe(Some(sink));
    tim.set_mtimecmp(1_000);

    tim.tick(500).unwrap();
    assert!(log.borrow().is_empty(), "no hit before the deadline");

    tim.tick(2_000).unwrap();
    let log = log.borrow();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].0, timer::SIGNAL_CMP);
    assert_eq!(log[0].1, 1, "the reported value is the hit flag");
    assert_eq!(log[0].2, 2_000);
}

#[test]
fn a_peripheral_without_a_probe_reports_nothing() {
    // 未挂探针的外设不能因为上报路径而出错。
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.write(0x4, WriteSize::Word, 0b1).unwrap();
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 1);
}

#[test]
fn registry_handle_shares_the_same_instance() {
    // D-12: the registry owns the instance; the bus gets a *handle* to
    // the very same object, so a write through the handle is visible
    // through the registry (and vice versa).
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART0", 0, 9600, 4)).unwrap();
    let handle = reg.handle("UART0").expect("handle");

    {
        let mut p = handle.borrow_mut();
        let _ = p.write(0x0, WriteSize::Byte, 0x41);
    }
    let status = reg
        .get("UART0")
        .unwrap()
        .read(0x4, WriteSize::Byte)
        .unwrap();
    assert_eq!(
        status & uart::status_bits::TX_READY,
        0,
        "the registry and the handle must see the same instance"
    );
}

// ---------------------------------------------------------------------------
// PeripheralRegistry
// ---------------------------------------------------------------------------

#[test]
fn registry_register_get_iter_describe() {
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART0", 0x2000_0000, 115_200, 5))
        .unwrap();
    reg.register(GpioBank::new("GPIO0", 0x4000_0000)).unwrap();
    reg.register(TimerRiscv::new("TIM0", 0x6000_0000, 7))
        .unwrap();

    assert_eq!(reg.len(), 3);
    assert!(!reg.is_empty());

    assert!(reg.get("UART0").is_some());
    assert!(reg.get("GPIO0").is_some());
    assert!(reg.get("TIM0").is_some());
    assert!(reg.get("nope").is_none());

    let mut ids = reg.iter().map(|p| p.id().to_string()).collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, vec!["GPIO0", "TIM0", "UART0"]);

    assert_eq!(reg.describe("UART0"), Some("behavioural-level"));
    assert_eq!(reg.describe("nope"), None);
}

#[test]
fn registry_duplicate_id_returns_register_error() {
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART0", 0, 9600, 4)).unwrap();
    let err = reg
        .register(Uart16550::new("UART0", 0, 115_200, 5))
        .unwrap_err();
    assert_eq!(err, RegisterError::Duplicate("UART0".to_string()));
    // Original entry must still be present (no silent replacement).
    let uart = reg.get("UART0").unwrap();
    assert_eq!(uart.size(), uart::UART_WINDOW);
}

#[test]
fn registry_get_mut_allows_state_changes() {
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART0", 0, 9600, 4)).unwrap();
    {
        let u = reg.get_mut("UART0").expect("registered");
        assert_eq!(u.id(), "UART0");
    }
    // Mutate via the typed trait object: write DATA → TX FIFO.
    // `get_mut` hands out a `RefMut` guard (the registry owns the
    // instance behind a `RefCell`), so the binding must be mutable to
    // reach the `&mut self` methods.
    let mut u = reg.get_mut("UART0").expect("registered");
    let _ = u.write(0x0, WriteSize::Byte, 0x41);
    // Verify the mutation took effect through the STATUS register:
    // TX_READY is cleared while a byte is in the TX FIFO.
    let status = u.read(0x4, WriteSize::Byte).unwrap();
    assert_eq!(status & uart::status_bits::TX_READY, 0);
    // M-15 review #2: read_clear drains the read-destructive state.
    let read_back = u.read_clear(0x4, WriteSize::Word).unwrap();
    assert_eq!(read_back & uart::status_bits::TX_READY, 0);
}

#[test]
fn peripheral_trait_direct_re_export_is_resolvable() {
    // M-15 review #10: external code must be able to write
    // `m15_peripheral::Peripheral` (the previous `PeripheralTrait`
    // alias forced callers back to `riscv_sim_contract::port::Peripheral`).
    fn _accepts_peripheral(_p: &dyn Peripheral) {}
    let uart = Uart16550::new("UART0", 0, 9600, 0);
    _accepts_peripheral(&uart);
}

// ---------------------------------------------------------------------------
// Uart16550
// ---------------------------------------------------------------------------

#[test]
fn uart_register_window_layout() {
    let uart = Uart16550::new("UART0", 0x2000_0000, 115_200, 5);
    assert_eq!(uart.id(), "UART0");
    assert_eq!(uart.base_addr(), 0x2000_0000);
    assert_eq!(uart.size(), uart::UART_WINDOW);
    // Initial: TX ready, RX empty.
    let status = uart.read(0x4, WriteSize::Byte).unwrap();
    assert_eq!(
        status & uart::status_bits::TX_READY,
        uart::status_bits::TX_READY
    );
    assert_eq!(status & uart::status_bits::RX_FULL, 0);
}

#[test]
fn uart_transmit_then_take_tx_drains_byte() {
    // M-15 review #1: previously the TX byte was unreachable. Now
    // `take_tx` returns the byte and clears the busy state.
    let mut uart = Uart16550::new("UART0", 0, 9600, 4);
    let _ = uart.write(0x0, WriteSize::Byte, u32::from(b'A'));
    assert!(!uart.tx_pin(), "TX pin busy while FIFO holds a byte");

    let drained = uart.take_tx();
    assert_eq!(drained, Some(b'A'));
    assert!(uart.tx_pin(), "TX pin idle after drain");
    let status = uart.read(0x4, WriteSize::Byte).unwrap();
    assert_eq!(
        status & uart::status_bits::TX_READY,
        uart::status_bits::TX_READY
    );
}

#[test]
fn uart_transmit_overflow_latches_tx_overrun_bit() {
    // M-15 review #4: TX overflow used to set RX_OVERRUN. Now
    // TX_OVERRUN is a separate bit (0x8).
    let mut uart = Uart16550::new("UART0", 0, 9600, 0);
    let _ = uart.write(0x0, WriteSize::Byte, u32::from(b'A'));
    let _ = uart.write(0x0, WriteSize::Byte, u32::from(b'B')); // overflow
    let status = uart.read(0x4, WriteSize::Byte).unwrap();
    assert_eq!(
        status & uart::status_bits::TX_OVERRUN,
        uart::status_bits::TX_OVERRUN
    );
    assert_eq!(status & uart::status_bits::RX_OVERRUN, 0);
}

#[test]
fn uart_status_clear_is_w1c() {
    // M-15 review #14: W1C (write-1-to-clear). Writing 0 must NOT
    // clear the overrun bit; writing the bit's value must.
    let mut uart = Uart16550::new("UART0", 0, 9600, 0);
    uart.receive(b'Z');
    uart.receive(b'Y'); // triggers RX overrun
    let s = uart.read(0x4, WriteSize::Byte).unwrap();
    assert_eq!(
        s & uart::status_bits::RX_OVERRUN,
        uart::status_bits::RX_OVERRUN
    );

    // Write 0 — bit stays.
    let _ = uart.write(0x4, WriteSize::Byte, 0);
    let s = uart.read(0x4, WriteSize::Byte).unwrap();
    assert_eq!(
        s & uart::status_bits::RX_OVERRUN,
        uart::status_bits::RX_OVERRUN
    );

    // Write RX_OVERRUN bit (0x4) — clears RX_OVERRUN, leaves TX_OVERRUN
    // (untouched, must stay 0).
    let _ = uart.write(0x4, WriteSize::Byte, uart::status_bits::RX_OVERRUN);
    let s = uart.read(0x4, WriteSize::Byte).unwrap();
    assert_eq!(s & uart::status_bits::RX_OVERRUN, 0);
}

#[test]
fn uart_read_clear_drains_rx_fifo() {
    // M-15 review #2: RX FIFO read used to be non-destructive.
    // `read_clear` (default-trait method) takes the byte out.
    let mut uart = Uart16550::new("UART0", 0, 9600, 0);
    uart.receive(b'Z');
    let first = uart.read_clear(0x0, WriteSize::Byte).unwrap();
    assert_eq!(first, u32::from(b'Z'));
    // RX_FULL must clear.
    let status = uart.read(0x4, WriteSize::Byte).unwrap();
    assert_eq!(status & uart::status_bits::RX_FULL, 0);
    // Second read returns 0 (FIFO empty), no overrun latched.
    let second = uart.read_clear(0x0, WriteSize::Byte).unwrap();
    assert_eq!(second, 0);
    let status = uart.read(0x4, WriteSize::Byte).unwrap();
    assert_eq!(status & uart::status_bits::RX_OVERRUN, 0);
}

#[test]
fn uart_ps_per_bit_matches_baud_rate_and_floors_at_one() {
    let uart = Uart16550::new("UART0", 0, 115_200, 0);
    assert_eq!(uart.ps_per_bit(), 8_680); // 1e9 / 115_200 = 8680.55 → 8680
    let uart = Uart16550::new("UART0", 0, 9_600, 0);
    assert_eq!(uart.ps_per_bit(), 104_166); // 1e9 / 9_600 = 104166.66 → 104166
                                            // M-15 review #15: pathological baud > 1e9 must not return 0.
    let uart = Uart16550::new("UART0", 0, 2_000_000_000, 0);
    assert_eq!(uart.ps_per_bit(), 1);
}

#[test]
fn uart_undefined_offset_returns_e_per_02() {
    let mut uart = Uart16550::new("UART0", 0, 9600, 0);
    let err = uart.write(0x10, WriteSize::Byte, 0).unwrap_err();
    assert_eq!(err.scenario, 2);
    let err = uart.read(0x10, WriteSize::Byte).unwrap_err();
    assert_eq!(err.scenario, 2);
}

#[test]
fn uart_sub_register_byte_access() {
    // M-15 review #5: reading offset 0x1 (within the DATA word) must
    // return the high byte of RX FIFO, not an undefined-offset error.
    let mut uart = Uart16550::new("UART0", 0, 9600, 0);
    uart.receive(0xAB);
    assert_eq!(uart.read(0x0, WriteSize::Byte).unwrap(), 0xAB);
    assert_eq!(uart.read(0x1, WriteSize::Byte).unwrap(), 0x00);
    assert_eq!(uart.read(0x2, WriteSize::Byte).unwrap(), 0x00);
    assert_eq!(uart.read(0x3, WriteSize::Byte).unwrap(), 0x00);
    // Halfword reads span two bytes.
    assert_eq!(uart.read(0x0, WriteSize::Half).unwrap(), 0x00AB);
    // Sub-register read on STATUS register (offset 0x4 word).
    let status = uart.read(0x4, WriteSize::Word).unwrap();
    let lo = status & 0xFFFF;
    assert_eq!(uart.read(0x4, WriteSize::Half).unwrap(), lo);
    assert_eq!(
        uart.read(0x5, WriteSize::Byte).unwrap(),
        (status >> 8) & 0xFF
    );
}

// ---------------------------------------------------------------------------
// GpioBank
// ---------------------------------------------------------------------------

#[test]
fn gpio_set_directions_and_out_latch() {
    let mut gpio = GpioBank::new("GPIO0", 0x4000_0000);
    gpio.set_directions(0b1100); // pins 2,3 = output
    assert_eq!(gpio.read(0x0, WriteSize::Byte).unwrap(), 0b1100);
    let _ = gpio.write(0x4, WriteSize::Byte, 0b1010);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 0b1010);
}

#[test]
fn gpio_toggle_uses_documented_pin_bit_semantics() {
    // M-15 review #3: `1 << pin` flips that pin's output. Previous
    // semantics required bit 5 to be set, which conflicted with
    // documentation and confused firmware authors.
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_directions(0xFFFF_FFFF);
    let _ = gpio.write(0x4, WriteSize::Byte, 0);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 0);

    // value = 1 << 3 → flip pin 3
    let _ = gpio.write(0xC, WriteSize::Word, 1 << 3);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 1 << 3);

    // Toggle again → clears.
    let _ = gpio.write(0xC, WriteSize::Word, 1 << 3);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 0);

    // value = 1 << 0 → flip pin 0
    let _ = gpio.write(0xC, WriteSize::Word, 1 << 0);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 1);
}

#[test]
fn gpio_external_inputs_latch_pending_irqs_only_for_input_pins() {
    // M-15 review #7: previously `directions` was stored but never
    // consulted. Now external-input edge IRQs latch only for pins
    // configured as input (direction bit = 0).
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_irq_line_for_pin(0, 11); // input pin
    gpio.set_irq_line_for_pin(1, 12); // output pin (per next line)
    gpio.set_directions(0b10); // pin 0 = input, pin 1 = output

    // Rising edge on pin 0 (input) → IRQ latched.
    gpio.set_external_inputs(0b01);
    assert_eq!(gpio.take_pending_irqs(), vec![11]);

    // Rising edge on pin 1 (output) → NO IRQ (output pin transitions
    // come from firmware via OUT/TOGGLE, not from the wire).
    gpio.set_external_inputs(0b11);
    assert!(gpio.take_pending_irqs().is_empty());
}

#[test]
fn gpio_out_write_latches_irq_on_each_edge() {
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_directions(0xFFFF_FFFF);
    gpio.set_irq_line_for_pin(2, 13);
    let _ = gpio.write(0x4, WriteSize::Byte, 0);
    // First OUT write: pin 2 rises 0 → 1.
    let _ = gpio.write(0x4, WriteSize::Byte, 1 << 2);
    assert_eq!(gpio.take_pending_irqs(), vec![13]);
    // Second OUT write: pin 2 stays high → no edge.
    let _ = gpio.write(0x4, WriteSize::Byte, 1 << 2);
    assert!(gpio.take_pending_irqs().is_empty());
    // Third OUT write: pin 2 falls 1 → 0.
    let _ = gpio.write(0x4, WriteSize::Byte, 0);
    assert_eq!(gpio.take_pending_irqs(), vec![13]);
}

#[test]
fn gpio_take_pending_irqs_dedupes_by_line() {
    // M-15 review #13: multiple pins binding the same IRQ line should
    // not produce duplicate entries in `take_pending_irqs`.
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_irq_line_for_pin(0, 11);
    gpio.set_irq_line_for_pin(1, 11);
    gpio.set_irq_line_for_pin(2, 12);
    // Trigger edges on pins 0 and 1 (both → line 11) and pin 2 (→ line 12).
    let _ = gpio.write(0x4, WriteSize::Byte, 0b111);
    let pending = gpio.take_pending_irqs();
    assert_eq!(pending.len(), 2);
    assert!(pending.contains(&11));
    assert!(pending.contains(&12));
}

#[test]
fn gpio_in_write_is_a_no_op_for_firmware_driven_lines() {
    let mut gpio = GpioBank::new("GPIO0", 0);
    let before = gpio.read(0x8, WriteSize::Byte).unwrap();
    let _ = gpio.write(0x8, WriteSize::Byte, 0xFF);
    let after = gpio.read(0x8, WriteSize::Byte).unwrap();
    assert_eq!(before, after);
}

#[test]
fn gpio_in_reflects_output_latch_on_output_direction_pins() {
    // M-15 review N2: firmware writing OUT to drive a pin must be
    // able to read the level back via IN. Previously IN returned
    // only `self.inputs`, breaking the "data direction register
    // controls who drives the pin" mental model. Required for
    // E2E-01 N5 "GPIO 翻转可见" + M-18 波形 pipeline to observe
    // firmware-driven edges.
    let mut gpio = GpioBank::new("GPIO0", 0);
    // Pins 0,1 = output; pins 2,3 = input.
    gpio.set_directions(0b0011);
    // Drive OUT[0]=1, OUT[1]=0.
    let _ = gpio.write(0x4, WriteSize::Byte, 0b0001);
    // Set external inputs for the input pins.
    gpio.set_external_inputs(0b1100);
    let in_reg = gpio.read(0x8, WriteSize::Byte).unwrap();
    // pin 0 (out, 1) + pin 2 (in, 1) + pin 3 (in, 1) = 0b1101
    assert_eq!(in_reg, 0b1101);
}

#[test]
fn gpio_take_pending_irqs_dedupes_lines_in_upper_half() {
    // M-15 review N3: previously the dedup bitmap only covered
    // `line < 16`; lines in [16, 32) bypassed dedup entirely. Now
    // the bitmap covers `line < 32` (full u16 space minus 0xFFFF).
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_irq_line_for_pin(0, 17);
    gpio.set_irq_line_for_pin(1, 17);
    gpio.set_irq_line_for_pin(2, 31);
    // Trigger edges on pins 0 and 1 (both → line 17) and pin 2 (→ line 31).
    let _ = gpio.write(0x4, WriteSize::Byte, 0b111);
    let pending = gpio.take_pending_irqs();
    assert_eq!(pending.len(), 2);
    assert!(pending.contains(&17));
    assert!(pending.contains(&31));
}

#[test]
fn gpio_in_reflects_input_pins_and_output_latch_simultaneously() {
    // Edge case for N2: same byte mixes input + output pins; IN
    // must return the bitwise merge.
    let mut gpio = GpioBank::new("GPIO0", 0);
    // pin 0 = output, pin 1 = input.
    gpio.set_directions(0b01);
    let _ = gpio.write(0x4, WriteSize::Byte, 0b01); // OUT[0] = 1
    gpio.set_external_inputs(0b10); // external drives pin 1 high
    let in_reg = gpio.read(0x8, WriteSize::Byte).unwrap();
    // pin 0 (out=1) + pin 1 (in=1) = 0b11
    assert_eq!(in_reg, 0b11);
}

#[test]
fn reg_helpers_are_dry() {
    // M-15 review N1: smoke test that `super::reg` exports the
    // expected helpers with the review #5 semantics. If the dedup
    // regresses (e.g. someone re-inlines the helpers in one of the
    // three peripheral modules), this test will still pass — but
    // its existence makes the DRY surface explicit.
    use super::reg::{extract_subword, register_offset};

    assert_eq!(register_offset(0x1), 0x0);
    assert_eq!(register_offset(0x4), 0x4);
    assert_eq!(register_offset(0x7), 0x4);

    // Word at offset 0x1 should yield the byte at position 1.
    let word = 0xAABBCCDD;
    assert_eq!(extract_subword(word, 0x0, WriteSize::Byte), 0xDD);
    assert_eq!(extract_subword(word, 0x1, WriteSize::Byte), 0xCC);
    assert_eq!(extract_subword(word, 0x2, WriteSize::Byte), 0xBB);
    assert_eq!(extract_subword(word, 0x3, WriteSize::Byte), 0xAA);
    assert_eq!(extract_subword(word, 0x0, WriteSize::Half), 0xCCDD);
    assert_eq!(extract_subword(word, 0x2, WriteSize::Half), 0xAABB);
    assert_eq!(extract_subword(word, 0x0, WriteSize::Word), word);
}

#[test]
fn gpio_sub_register_byte_access() {
    // M-15 review #5: register-internal byte reads must not return
    // E_PER-02 (offset "not defined").
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_directions(0x0403_0201);
    // Word reads at offset 0x0 / 0x1 / 0x2 / 0x3 all return same word.
    let w0 = gpio.read(0x0, WriteSize::Word).unwrap();
    assert_eq!(w0, 0x0403_0201);
    let w1 = gpio.read(0x1, WriteSize::Word).unwrap();
    assert_eq!(w1, 0x0403_0201);
    // Byte reads extract the corresponding byte.
    assert_eq!(gpio.read(0x0, WriteSize::Byte).unwrap(), 0x01);
    assert_eq!(gpio.read(0x1, WriteSize::Byte).unwrap(), 0x02);
    assert_eq!(gpio.read(0x2, WriteSize::Byte).unwrap(), 0x03);
    assert_eq!(gpio.read(0x3, WriteSize::Byte).unwrap(), 0x04);
}

// ---------------------------------------------------------------------------
// TimerRiscv
// ---------------------------------------------------------------------------

#[test]
fn timer_read_split_between_lo_and_hi_halves() {
    // M-15 review #8: mtime is read-only (engine-driven via tick).
    // We set mtime by ticking the timer to the target value.
    let mut tim = TimerRiscv::new("TIM0", 0, 9);
    tim.tick(0x1122_3344_5566_7788).unwrap();
    assert_eq!(tim.read(0x0, WriteSize::Word).unwrap(), 0x5566_7788);
    assert_eq!(tim.read(0x4, WriteSize::Word).unwrap(), 0x1122_3344);

    // mtimecmp is firmware-writable in either order.
    let _ = tim.write(0x8, WriteSize::Word, 0xDEAD_BEEF);
    let _ = tim.write(0xC, WriteSize::Word, 0x0000_0001);
    assert_eq!(tim.mtimecmp(), 0x0000_0001_DEAD_BEEF);
}

#[test]
fn timer_mtime_writes_from_firmware_are_silently_ignored() {
    // M-15 review #8: mtime is owned by the engine. Firmware writes
    // to MTIME_LO / MTIME_HI must be no-ops (no error, but no effect).
    let mut tim = TimerRiscv::new("TIM0", 0, 0);
    tim.tick(0xAAAA).unwrap();
    let _ = tim.write(0x0, WriteSize::Word, 0xFFFF_FFFF);
    let _ = tim.write(0x4, WriteSize::Word, 0xFFFF_FFFF);
    // mtime must still be what tick set it to.
    assert_eq!(tim.mtime(), 0xAAAA);
}

#[test]
fn timer_tick_advances_mtime_and_arms_irq_at_compare() {
    let mut tim = TimerRiscv::new("TIM0", 0, 7);
    tim.set_mtimecmp(1_000_000); // 1 ms

    // Advance to mtime = 500µs — no IRQ yet.
    tim.tick(500_000).unwrap();
    assert_eq!(tim.mtime(), 500_000);
    assert_eq!(tim.take_pending_irq(), None);

    // Advance past 1 ms — IRQ arm.
    tim.tick(2_000_000).unwrap();
    assert_eq!(tim.mtime(), 2_000_000);
    assert_eq!(tim.take_pending_irq(), Some(7));
    // IRQ is consumed (sticky arm only after re-comparison).
    assert_eq!(tim.take_pending_irq(), None);
}

#[test]
fn timer_undefined_offset_returns_e_per_02() {
    let mut tim = TimerRiscv::new("TIM0", 0, 0);
    let err = tim.read(0x20, WriteSize::Byte).unwrap_err();
    assert_eq!(err.scenario, 2);
    let err = tim.write(0x20, WriteSize::Byte, 0).unwrap_err();
    assert_eq!(err.scenario, 2);
}

#[test]
fn timer_sub_register_byte_access() {
    // M-15 review #5: byte/half reads inside a word register.
    let mut tim = TimerRiscv::new("TIM0", 0, 0);
    tim.tick(0xAABBCCDD).unwrap();
    assert_eq!(tim.read(0x0, WriteSize::Byte).unwrap(), 0xDD);
    assert_eq!(tim.read(0x1, WriteSize::Byte).unwrap(), 0xCC);
    assert_eq!(tim.read(0x2, WriteSize::Byte).unwrap(), 0xBB);
    assert_eq!(tim.read(0x3, WriteSize::Byte).unwrap(), 0xAA);
    // Halfword spans 2 bytes within the same word.
    assert_eq!(tim.read(0x0, WriteSize::Half).unwrap(), 0xCCDD);
    assert_eq!(tim.read(0x2, WriteSize::Half).unwrap(), 0xAABB);
}

// ---------------------------------------------------------------------------
// Cross-peripheral test: UART TX → host bridge → GPIO LED
// ---------------------------------------------------------------------------

#[test]
fn uart_byte_drives_gpio_led_pin() {
    let mut uart = Uart16550::new("UART0", 0, 9600, 4);
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_directions(0xFFFF_FFFF);
    gpio.set_irq_line_for_pin(0, 11);

    // The host bridge translates one UART TX byte into a LED pin toggle:
    // a single-shot "echo back" that demonstrates how peripheral
    // outputs compose without M-14 wiring yet.
    let _ = uart.write(0x0, WriteSize::Byte, u32::from(b'T'));
    assert!(!uart.tx_pin(), "TX pin should reflect busy state");

    // Bridge: drain UART TX byte and toggle GPIO pin 0 (the LED).
    assert_eq!(uart.take_tx(), Some(b'T'));
    assert!(uart.tx_pin());

    // Toggle pin 0 with doc semantics: 1<<0.
    let _ = gpio.write(0xC, WriteSize::Word, 1 << 0);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 1);
    let pending = gpio.take_pending_irqs();
    assert_eq!(pending, vec![11]);
}

// ---------------------------------------------------------------------------
// Session snapshot (STAGE-3 §3.3) — segment 4 (peripheral internal state).
// ---------------------------------------------------------------------------

#[test]
fn uart_session_state_round_trips_every_field() {
    let mut uart = Uart16550::new("UART0", 0x1000, 9600, 4);
    uart.write(0x0, WriteSize::Byte, u32::from(b'A'))
        .expect("tx");
    uart.receive(b'Z');
    uart.tick(1_234).expect("tick");

    let exported = uart
        .session_state()
        .expect("the UART participates in the session state");
    let PeripheralSessionState::Uart(state) = exported.clone() else {
        panic!("a UART must export a UART state");
    };
    assert_eq!(state.id, "UART0");
    assert_eq!(state.tx, Some(b'A'), "the TX byte must be carried");
    assert_eq!(state.rx, Some(b'Z'), "the RX byte must be carried");
    assert_eq!(state.irq_line, 4);
    assert_eq!(
        state.now_ps, 1_234,
        "the peripheral's virtual time must be carried"
    );
    assert_eq!(
        state.status, 0x2,
        "the latched status register must be carried"
    );

    // Restore into a *fresh* UART → the exported state must come back
    // field for field. A dropped field (status / irq_line / now_ps)
    // shows up here, not as a silent divergence later on.
    let mut fresh = Uart16550::new("UART0", 0x1000, 9600, 0);
    fresh
        .restore_session_state(&exported)
        .expect("the UART must accept its own state");
    assert_eq!(
        fresh.session_state(),
        Some(exported),
        "the round-trip must be field-for-field equal"
    );
}

#[test]
fn gpio_session_state_round_trips_every_field() {
    let mut gpio = GpioBank::new("GPIO0", 0x2000);
    gpio.set_directions(0xFFFF_FFFF);
    gpio.set_irq_line_for_pin(3, 12);
    gpio.set_irq_line_for_pin(31, 20);
    gpio.write(0x4, WriteSize::Word, 0x0000_0008).expect("out");
    gpio.set_external_inputs(0x0000_00F0);
    gpio.tick(4_321).expect("tick");

    let exported = gpio
        .session_state()
        .expect("the GPIO participates in the session state");
    let PeripheralSessionState::Gpio(state) = exported.clone() else {
        panic!("a GPIO must export a GPIO state");
    };
    assert_eq!(state.id, "GPIO0");
    assert_eq!(state.directions, 0xFFFF_FFFF);
    assert_eq!(state.outputs, 0x8);
    assert_eq!(state.inputs, 0xF0);
    assert_eq!(
        state.pending_irqs, 0x8,
        "the sticky pending vector is carried"
    );
    assert_eq!(state.now_ps, 4_321);
    assert_eq!(
        state.irq_lines.len(),
        32,
        "one IRQ line per pin must be carried"
    );
    assert_eq!(state.irq_lines[3], 12, "the lines are in **pin order**");
    assert_eq!(state.irq_lines[31], 20, "the lines are in **pin order**");
    assert_eq!(
        state.irq_lines[0], 0xFFFF,
        "an unconfigured pin keeps its default"
    );

    let mut fresh = GpioBank::new("GPIO0", 0x2000);
    fresh
        .restore_session_state(&exported)
        .expect("the GPIO must accept its own state");
    assert_eq!(
        fresh.session_state(),
        Some(exported),
        "the round-trip must be field-for-field equal"
    );
}

#[test]
fn timer_session_state_round_trips_every_field() {
    let mut tim = TimerRiscv::new("TIM0", 0x3000, 7);
    tim.set_mtimecmp(9_000);
    tim.tick(5_000).expect("tick");

    let exported = tim
        .session_state()
        .expect("the timer participates in the session state");
    let PeripheralSessionState::Timer(state) = exported.clone() else {
        panic!("a timer must export a timer state");
    };
    assert_eq!(state.id, "TIM0");
    assert_eq!(state.irq_line, 7);
    assert_eq!(state.mtime, 5_000);
    assert_eq!(state.mtimecmp, 9_000);

    let mut fresh = TimerRiscv::new("TIM0", 0x3000, 0);
    fresh
        .restore_session_state(&exported)
        .expect("the timer must accept its own state");
    assert_eq!(
        fresh.session_state(),
        Some(exported),
        "the round-trip must be field-for-field equal"
    );
    assert_eq!(fresh.mtime(), 5_000, "mtime must be restored");
    assert_eq!(fresh.mtimecmp(), 9_000, "mtimecmp must be restored");
}

#[test]
fn restoring_a_state_of_the_wrong_kind_is_rejected() {
    // A UART must not swallow a GPIO state: the kinds are not
    // interchangeable, and applying one would move another peripheral's
    // registers into this one.
    let mut uart = Uart16550::new("UART0", 0x1000, 9600, 4);
    let gpio_state = PeripheralSessionState::Gpio(GpioStateDto {
        id: "UART0".to_string(),
        directions: 0,
        outputs: 0,
        inputs: 0,
        irq_lines: vec![0xFFFF; 32],
        pending_irqs: 0,
        now_ps: 0,
    });
    let err = uart
        .restore_session_state(&gpio_state)
        .expect_err("a UART must reject a GPIO state");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
}

#[test]
fn restoring_a_state_that_names_another_peripheral_is_rejected() {
    // The id inside the payload is the peripheral's own — a state that
    // names a different peripheral was handed to the wrong instance.
    let mut uart = Uart16550::new("UART0", 0x1000, 9600, 4);
    let foreign = PeripheralSessionState::Uart(UartStateDto {
        id: "UART1".to_string(),
        tx: Some(b'X'),
        rx: None,
        status: 0,
        irq_line: 1,
        now_ps: 0,
    });
    let err = uart
        .restore_session_state(&foreign)
        .expect_err("a state of another peripheral must be rejected");
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);
}

#[test]
fn gpio_rejects_a_state_with_the_wrong_number_of_irq_lines() {
    // A short list would leave the other pins' lines undefined; a long
    // one would carry lines that do not exist. Neither is repaired
    // silently.
    let mut gpio = GpioBank::new("GPIO0", 0x2000);
    let bad = PeripheralSessionState::Gpio(GpioStateDto {
        id: "GPIO0".to_string(),
        directions: 0,
        outputs: 0,
        inputs: 0,
        irq_lines: vec![1, 2, 3],
        pending_irqs: 0,
        now_ps: 0,
    });
    let err = gpio
        .restore_session_state(&bad)
        .expect_err("a malformed GPIO state must be rejected");
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
}

#[test]
fn registry_export_state_groups_by_kind_and_orders_by_id() {
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART1", 0x1000, 9600, 1))
        .expect("uart1");
    reg.register(GpioBank::new("GPIO0", 0x2000)).expect("gpio");
    reg.register(Uart16550::new("UART0", 0x3000, 9600, 2))
        .expect("uart0");
    reg.register(TimerRiscv::new("TIM0", 0x4000, 3))
        .expect("timer");

    let state = reg.export_state();
    assert_eq!(state.uarts.len(), 2);
    assert_eq!(state.uarts[0].id, "UART0", "the groups are ordered by id");
    assert_eq!(state.uarts[1].id, "UART1");
    assert_eq!(state.gpios.len(), 1);
    assert_eq!(state.gpios[0].id, "GPIO0");
    assert_eq!(state.timers.len(), 1);
    assert_eq!(state.timers[0].id, "TIM0");
}

#[test]
fn registry_load_state_restores_every_peripheral() {
    let mut source = PeripheralRegistry::new();
    source
        .register(Uart16550::new("UART0", 0x1000, 9600, 4))
        .expect("uart");
    source
        .register(GpioBank::new("GPIO0", 0x2000))
        .expect("gpio");
    source
        .register(TimerRiscv::new("TIM0", 0x3000, 7))
        .expect("timer");
    // Give them some state through the register interface.
    source
        .get_mut("UART0")
        .expect("uart")
        .write(0x0, WriteSize::Byte, u32::from(b'Q'))
        .expect("tx");
    source
        .get_mut("GPIO0")
        .expect("gpio")
        .write(0x0, WriteSize::Word, 0xF0)
        .expect("dir");
    source
        .get_mut("TIM0")
        .expect("timer")
        .write(0x8, WriteSize::Word, 4_242)
        .expect("mtimecmp");

    let state = source.export_state();

    // A fresh registry with the same ids — the restore must reproduce
    // the captured state field for field.
    let mut target = PeripheralRegistry::new();
    target
        .register(Uart16550::new("UART0", 0x1000, 9600, 0))
        .expect("uart");
    target
        .register(GpioBank::new("GPIO0", 0x2000))
        .expect("gpio");
    target
        .register(TimerRiscv::new("TIM0", 0x3000, 0))
        .expect("timer");
    target.load_state(&state).expect("load_state");

    assert_eq!(
        target.export_state(),
        state,
        "the restored state must be field-for-field equal"
    );
}

#[test]
fn registry_load_state_rejects_an_unknown_id() {
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART0", 0x1000, 9600, 4))
        .expect("uart");
    // The payload describes the **live** peripheral **and** an unknown
    // one: the "every live peripheral is described" check is satisfied,
    // so only the "every id in the payload is live" check can catch the
    // unknown id. (Two checks, two directions — a test that only had
    // the unknown entry would be satisfied by either of them.)
    let state = M15PeriphDto {
        uarts: vec![
            UartStateDto {
                id: "UART0".to_string(),
                tx: None,
                rx: None,
                status: 0,
                irq_line: 0,
                now_ps: 0,
            },
            UartStateDto {
                id: "NOPE".to_string(),
                tx: None,
                rx: None,
                status: 0,
                irq_line: 0,
                now_ps: 0,
            },
        ],
        ..Default::default()
    };
    let err = reg
        .load_state(&state)
        .expect_err("an unknown peripheral id must be rejected");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);
}

#[test]
fn registry_load_state_writes_nothing_when_one_id_is_unknown() {
    // "Validate before apply" (STAGE-3 §5): the valid entry must not
    // be applied when another entry is rejected.
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART0", 0x1000, 9600, 4))
        .expect("uart");
    let state = M15PeriphDto {
        uarts: vec![
            UartStateDto {
                id: "UART0".to_string(),
                tx: Some(b'Z'),
                rx: None,
                status: 0,
                irq_line: 9,
                now_ps: 42,
            },
            UartStateDto {
                id: "GHOST".to_string(),
                tx: None,
                rx: None,
                status: 0,
                irq_line: 0,
                now_ps: 0,
            },
        ],
        ..Default::default()
    };
    let err = reg
        .load_state(&state)
        .expect_err("the unknown id must be rejected");
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);

    let restored = reg.export_state();
    assert_eq!(
        restored.uarts[0].irq_line, 4,
        "the valid entry must not have been applied"
    );
    assert_eq!(restored.uarts[0].now_ps, 0);
}

#[test]
fn collect_session_state_reads_the_live_bus_instances() {
    // The host binds its peripherals straight onto the bus (it keeps no
    // registry) — the free functions are its entry point.
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let gpio = share(GpioBank::new("GPIO0", 0x2000));
    let tim = share(TimerRiscv::new("TIM0", 0x3000, 5));
    let instances: Vec<(u32, SharedPeripheral)> =
        vec![(0x1000, uart), (0x2000, gpio), (0x3000, tim)];

    let state = collect_session_state(&instances);
    assert_eq!(state.uarts.len(), 1);
    assert_eq!(state.uarts[0].id, "UART0");
    assert_eq!(state.gpios.len(), 1);
    assert_eq!(state.gpios[0].id, "GPIO0");
    assert_eq!(state.timers.len(), 1);
    assert_eq!(state.timers[0].id, "TIM0");
}

#[test]
fn collect_session_state_orders_each_group_by_id() {
    // The bus order is a *placement*, not an identity: two UARTs bound
    // in the "wrong" order must still be exported in id order.
    let uart1 = share(Uart16550::new("UART1", 0x1000, 9600, 1));
    let uart0 = share(Uart16550::new("UART0", 0x2000, 9600, 2));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x1000, uart1), (0x2000, uart0)];

    let state = collect_session_state(&instances);
    assert_eq!(state.uarts.len(), 2);
    assert_eq!(state.uarts[0].id, "UART0", "the group is ordered by id");
    assert_eq!(state.uarts[1].id, "UART1");
}

#[test]
fn collect_session_state_releases_its_borrows() {
    // The collection must not hold a borrow across the loop: the
    // restore path takes a **mutable** borrow of the very same cell,
    // and holding both at once would panic.
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x1000, Rc::clone(&uart))];
    let _state = collect_session_state(&instances);

    let mut periph = uart.borrow_mut();
    periph
        .write(0x0, WriteSize::Byte, u32::from(b'X'))
        .expect("the borrow must be free");
}

#[test]
fn restore_session_states_round_trips_the_bus_instances() {
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let gpio = share(GpioBank::new("GPIO0", 0x2000));
    {
        let mut p = uart.borrow_mut();
        p.write(0x0, WriteSize::Byte, u32::from(b'K')).expect("tx");
    }
    {
        let mut p = gpio.borrow_mut();
        p.write(0x0, WriteSize::Word, 0x0F0F_0F0F).expect("dir");
    }
    let instances: Vec<(u32, SharedPeripheral)> =
        vec![(0x1000, Rc::clone(&uart)), (0x2000, Rc::clone(&gpio))];
    let state = collect_session_state(&instances);

    // Fresh instances, same ids.
    let fresh_uart = share(Uart16550::new("UART0", 0x1000, 9600, 0));
    let fresh_gpio = share(GpioBank::new("GPIO0", 0x2000));
    let fresh: Vec<(u32, SharedPeripheral)> = vec![(0x1000, fresh_uart), (0x2000, fresh_gpio)];
    restore_session_states(&fresh, &state).expect("restore");

    assert_eq!(
        collect_session_state(&fresh),
        state,
        "the restored state must be field-for-field equal"
    );
}

#[test]
fn restore_session_states_rejects_an_unknown_id() {
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x1000, uart)];
    // The payload describes the **live** peripheral **and** an unknown
    // one, so the "every live peripheral is described" check is
    // satisfied and only the "every id in the payload is live" check
    // can catch the unknown id.
    let state = M15PeriphDto {
        uarts: vec![
            UartStateDto {
                id: "UART0".to_string(),
                tx: None,
                rx: None,
                status: 0,
                irq_line: 0,
                now_ps: 0,
            },
            UartStateDto {
                id: "GHOST".to_string(),
                tx: None,
                rx: None,
                status: 0,
                irq_line: 0,
                now_ps: 0,
            },
        ],
        ..Default::default()
    };
    let err = restore_session_states(&instances, &state)
        .expect_err("an unknown peripheral id must be rejected");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);
}

#[test]
fn restore_session_states_rejects_a_payload_that_misses_a_live_peripheral() {
    // The other direction: a live peripheral that the payload does not
    // describe would silently keep its initial state — the restored
    // session would differ from the captured one.
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x1000, uart)];
    let err = restore_session_states(&instances, &M15PeriphDto::default())
        .expect_err("a payload that misses a live peripheral must be rejected");
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);
}
