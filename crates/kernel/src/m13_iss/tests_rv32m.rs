//! RV32M coverage for M-13: the eight multiply / divide instructions,
//! their decode keys and the architectural edge cases (division by
//! zero, the signed overflow, the signed / unsigned mix-ups).
//!
//! The M extension shares the OP opcode with the base integer set, so
//! every case here also pins the **funct7** discrimination: a decoder
//! that keys on funct3 alone cannot tell `MUL` from `ADD`.

use riscv_sim_contract::model::SimState;

use super::decoder::Decoder;
use super::executor::{classify, Op, StepOutcome, TrapCause};
use super::tests_common::*;

/// The funct7 that selects the RV32M group (LLD §3.4).
const M_FUNCT7: u8 = 0b000_0001;

/// Encode one of the eight RV32M instructions.
fn enc_m(funct3: u8, rs2: u8, rs1: u8, rd: u8) -> u32 {
    enc_r(M_FUNCT7, rs2, rs1, funct3, rd, OP)
}

/// Run one M instruction with the given operands and return the value
/// it wrote into `rd`.
fn run_m(funct3: u8, lhs: u32, rhs: u32) -> u32 {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[3] = lhs;
    state.gpr[4] = rhs;
    let word = enc_m(funct3, 4, 3, 5);
    h.step_one_at(&mut state, 0, word);
    state.gpr[5]
}

// ---------------------------------------------------------------------------
// Decode
// ---------------------------------------------------------------------------

/// The eight M instructions must resolve to eight distinct operations.
///
/// The M group shares the OP opcode with `ADD` / `SUB` / … and is told
/// apart by `funct7` alone — the funct3 only tells the eight apart
/// from each other.
#[test]
fn the_eight_m_instructions_resolve_to_eight_distinct_operations() {
    let expected = [
        (0b000, Op::Mul),
        (0b001, Op::Mulh),
        (0b010, Op::Mulhsu),
        (0b011, Op::Mulhu),
        (0b100, Op::Div),
        (0b101, Op::Divu),
        (0b110, Op::Rem),
        (0b111, Op::Remu),
    ];
    for (funct3, op) in expected {
        let d = Decoder::default().decode(enc_m(funct3, 4, 3, 5), 0);
        assert_eq!(
            classify(&d),
            Some(op),
            "funct3 = {funct3:#07b} must decode to {op:?}"
        );
    }
}

/// The base integer set must keep working next to the M group.
///
/// `ADD` and `MUL` differ only in funct7 — a decoder that dropped the
/// funct7 check would turn every `MUL` into an `ADD` (or the other way
/// round), and both are legal encodings, so nothing else would catch
/// it.
#[test]
fn the_base_op_group_is_not_shadowed_by_the_m_group() {
    for (funct7, expected) in [
        (0b000_0000u8, Op::Add),
        (0b010_0000, Op::Sub),
        (0b000_0001, Op::Mul),
    ] {
        let d = Decoder::default().decode(enc_r(funct7, 4, 3, 0b000, 5, OP), 0);
        assert_eq!(
            classify(&d),
            Some(expected),
            "funct7 = {funct7:#09b} must decode to {expected:?}"
        );
    }
}

/// A funct7 that belongs to neither group must stay illegal.
#[test]
fn an_unknown_funct7_in_the_op_group_traps() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    let word = enc_r(0b000_0011, 4, 3, 0b000, 5, OP);
    let outcome = h.step_one_at(&mut state, 0, word);
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        }
    );
}

// ---------------------------------------------------------------------------
// Multiply
// ---------------------------------------------------------------------------

/// `MUL` keeps the **low** half of the product.
#[test]
fn mul_keeps_the_low_half_of_the_product() {
    assert_eq!(run_m(0b000, 0x1234_5678, 0x9ABC_DEF0), 0x242D_2080);
    assert_eq!(run_m(0b000, 0xFFFF_FFFF, 0xFFFF_FFFF), 0x0000_0001);
    assert_eq!(run_m(0b000, 0x7FFF_FFFF, 0x7FFF_FFFF), 0x0000_0001);
}

/// `MULH` returns the high half of the **signed** product.
#[test]
fn mulh_returns_the_high_half_of_the_signed_product() {
    assert_eq!(run_m(0b001, 0x1234_5678, 0x9ABC_DEF0), 0xF8CC_93D6);
    assert_eq!(run_m(0b001, 0xFFFF_FFFF, 0xFFFF_FFFF), 0x0000_0000);
    // -2^31 * -1 = +2^31 -> high half 0
    assert_eq!(run_m(0b001, 0x8000_0000, 0xFFFF_FFFF), 0x0000_0000);
}

/// `MULHSU` mixes a signed and an unsigned operand: the high half of
/// `signed(rs1) * unsigned(rs2)`.
///
/// The two halves must not be confused — `MULHU` on the same operands
/// gives a different answer, and a decoder that treats the unsigned
/// operand as signed would produce yet another one.
#[test]
fn mulhsu_mixes_a_signed_and_an_unsigned_operand() {
    assert_eq!(run_m(0b010, 0x1234_5678, 0x9ABC_DEF0), 0x0B00_EA4E);
    assert_eq!(run_m(0b010, 0xFFFF_FFFF, 0xFFFF_FFFF), 0xFFFF_FFFF);
    assert_eq!(run_m(0b010, 0x8000_0000, 0xFFFF_FFFF), 0x8000_0000);
}

/// `MULHU` returns the high half of the **unsigned** product.
#[test]
fn mulhu_returns_the_high_half_of_the_unsigned_product() {
    assert_eq!(run_m(0b011, 0x1234_5678, 0x9ABC_DEF0), 0x0B00_EA4E);
    assert_eq!(run_m(0b011, 0xFFFF_FFFF, 0xFFFF_FFFF), 0xFFFF_FFFE);
    assert_eq!(run_m(0b011, 0x8000_0000, 0xFFFF_FFFF), 0x7FFF_FFFF);
}

/// `MULH` and `MULHU` must disagree on a negative operand — that
/// difference is the whole point of having both.
#[test]
fn mulh_and_mulhu_disagree_on_a_negative_operand() {
    assert_ne!(
        run_m(0b001, 0xFFFF_FFFF, 0xFFFF_FFFF),
        run_m(0b011, 0xFFFF_FFFF, 0xFFFF_FFFF),
        "the signed and the unsigned high multiply must differ"
    );
}

// ---------------------------------------------------------------------------
// Divide
// ---------------------------------------------------------------------------

/// `DIV` is a **signed** division.
#[test]
fn div_divides_with_the_signed_interpretation() {
    assert_eq!(run_m(0b100, 100, 7), 14);
    assert_eq!(run_m(0b100, 0xFFFF_FFF9, 16), 0);
    // -7 / 2 = -3 (truncating toward zero)
    assert_eq!(run_m(0b100, 0xFFFF_FFF9, 2), 0xFFFF_FFFD);
}

/// `DIVU` is an **unsigned** division.
#[test]
fn divu_divides_with_the_unsigned_interpretation() {
    assert_eq!(run_m(0b101, 100, 7), 14);
    assert_eq!(run_m(0b101, 0xFFFF_FFF9, 16), 0x0FFF_FFFF);
    // The unsigned quotient of 0xfffffff9 / 16 is much larger than the
    // signed one, so the two must not be confused.
    assert_ne!(
        run_m(0b100, 0xFFFF_FFF9, 16),
        run_m(0b101, 0xFFFF_FFF9, 16),
        "the signed and the unsigned division must differ"
    );
}

/// `REM` is the signed remainder; the sign follows the dividend.
#[test]
fn rem_takes_the_signed_remainder() {
    assert_eq!(run_m(0b110, 100, 7), 2);
    assert_eq!(run_m(0b110, 0xFFFF_FFF9, 16), 0xFFFF_FFF9);
    assert_eq!(run_m(0b110, 0xFFFF_FFFF, 2), 0xFFFF_FFFF);
}

/// `REMU` is the unsigned remainder.
#[test]
fn remu_takes_the_unsigned_remainder() {
    assert_eq!(run_m(0b111, 100, 7), 2);
    assert_eq!(run_m(0b111, 0xFFFF_FFF9, 16), 9);
}

// ---------------------------------------------------------------------------
// The two architecturally defined edge cases
// ---------------------------------------------------------------------------

/// A division by zero is **defined**, not a trap: the quotient is all
/// ones and the remainder is the dividend (LLD §3.4).
#[test]
fn a_division_by_zero_yields_the_defined_quotient_and_remainder() {
    assert_eq!(run_m(0b100, 7, 0), 0xFFFF_FFFF, "signed quotient");
    assert_eq!(run_m(0b101, 7, 0), 0xFFFF_FFFF, "unsigned quotient");
    assert_eq!(run_m(0b110, 7, 0), 7, "signed remainder");
    assert_eq!(run_m(0b111, 7, 0), 7, "unsigned remainder");
    // The dividend is zero as well: the remainder is still the
    // dividend, not some other value.
    assert_eq!(run_m(0b110, 0, 0), 0);
    assert_eq!(run_m(0b111, 0, 0), 0);
}

/// A division by zero must not raise the divide-by-zero trap cause.
///
/// The cause exists in the taxonomy (it is reserved for the M
/// extension), but the architecture **defines** the result instead of
/// trapping, so the trap must never be reported.
#[test]
fn a_division_by_zero_does_not_trap() {
    for funct3 in [0b100u8, 0b101, 0b110, 0b111] {
        let mut h = Harness::new();
        let mut state = SimState::default();
        state.gpr[4] = 0;
        let outcome = h.step_one_at(&mut state, 0, enc_m(funct3, 4, 3, 5));
        assert_eq!(
            outcome,
            StepOutcome::Advanced,
            "a division by zero must retire, not trap"
        );
    }
}

/// `INT_MIN / -1` overflows: the quotient is `INT_MIN` and the
/// remainder is zero.
///
/// This is the case that makes a naive implementation panic — Rust's
/// `i32` division overflows — so it needs an explicit test.
#[test]
fn the_signed_division_overflow_yields_int_min() {
    assert_eq!(run_m(0b100, 0x8000_0000, 0xFFFF_FFFF), 0x8000_0000);
    assert_eq!(run_m(0b110, 0x8000_0000, 0xFFFF_FFFF), 0x0000_0000);
}

// ---------------------------------------------------------------------------
// Dispatch-route parity and register hygiene
// ---------------------------------------------------------------------------

/// The M extension must behave the same on both dispatch routes
/// (D-11: one execution path).
#[test]
fn the_m_extension_behaves_the_same_on_both_dispatch_routes() {
    let word = enc_m(0b001, 4, 3, 5);
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[3] = 0x1234_5678;
    state.gpr[4] = 0x9ABC_DEF0;
    h.step_one_at(&mut state, 0, word);
    let closure_result = state.gpr[5];

    let mut bytes = image(&[word]);
    let mut bh = BusHarness::new(&bytes);
    let mut bus_state = SimState::default();
    bus_state.gpr[3] = 0x1234_5678;
    bus_state.gpr[4] = 0x9ABC_DEF0;
    bh.step(&mut bus_state).expect("bus route must not error");
    assert_eq!(
        bus_state.gpr[5], closure_result,
        "both dispatch routes must produce the same result"
    );
    bytes.clear();
}

/// Writing to x0 through the M extension must be discarded, like every
/// other register write (D-04).
#[test]
fn the_m_extension_cannot_write_x0() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[3] = 7;
    state.gpr[4] = 3;
    h.step_one_at(&mut state, 0, enc_m(0b000, 4, 3, 0));
    assert_eq!(state.gpr[0], 0, "x0 must stay hardwired to zero");
}
