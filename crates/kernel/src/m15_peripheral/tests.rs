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
