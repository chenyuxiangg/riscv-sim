//! Pure ALU helpers for the RV32I base integer set and the RV32M
//! multiply / divide extension.
//!
//! Every operation is a total function of its operands: RISC-V has no
//! trap-on-overflow, so the arithmetic wraps silently (LLD §3.3).
//! The M extension has no trap either — a division by zero and the
//! signed overflow case have **defined** results (LLD §3.4), not
//! exceptions.

/// Mask applied to a shift amount — RV32 shifts use the low 5 bits of
/// the shift operand.
pub const SHAMT_MASK: u32 = 0x1F;

/// `rd = rs1 + rs2`, wrapping.
pub fn add(lhs: u32, rhs: u32) -> u32 {
    lhs.wrapping_add(rhs)
}

/// `rd = rs1 - rs2`, wrapping.
pub fn sub(lhs: u32, rhs: u32) -> u32 {
    lhs.wrapping_sub(rhs)
}

/// `rd = rs1 << (rs2 & 0x1f)`.
pub fn sll(lhs: u32, rhs: u32) -> u32 {
    lhs << (rhs & SHAMT_MASK)
}

/// `rd = rs1 >> (rs2 & 0x1f)`, logical.
pub fn srl(lhs: u32, rhs: u32) -> u32 {
    lhs >> (rhs & SHAMT_MASK)
}

/// `rd = rs1 >> (rs2 & 0x1f)`, arithmetic (sign preserving).
pub fn sra(lhs: u32, rhs: u32) -> u32 {
    ((lhs as i32) >> (rhs & SHAMT_MASK)) as u32
}

/// `rd = (signed) rs1 < (signed) rs2`.
pub fn slt(lhs: u32, rhs: u32) -> u32 {
    u32::from((lhs as i32) < (rhs as i32))
}

/// `rd = (unsigned) rs1 < (unsigned) rs2`.
pub fn sltu(lhs: u32, rhs: u32) -> u32 {
    u32::from(lhs < rhs)
}

/// `rd = rs1 ^ rs2`.
pub fn xor(lhs: u32, rhs: u32) -> u32 {
    lhs ^ rhs
}

/// `rd = rs1 | rs2`.
pub fn or(lhs: u32, rhs: u32) -> u32 {
    lhs | rhs
}

/// `rd = rs1 & rs2`.
pub fn and(lhs: u32, rhs: u32) -> u32 {
    lhs & rhs
}

/// Sign-extend the low 8 bits of `value` to 32 bits.
pub fn sext8(value: u32) -> u32 {
    (value as u8 as i8) as i32 as u32
}

/// Sign-extend the low 16 bits of `value` to 32 bits.
pub fn sext16(value: u32) -> u32 {
    (value as u16 as i16) as i32 as u32
}

/// Zero-extend the low 8 bits of `value` to 32 bits.
pub fn zext8(value: u32) -> u32 {
    value & 0xFF
}

/// Zero-extend the low 16 bits of `value` to 32 bits.
pub fn zext16(value: u32) -> u32 {
    value & 0xFFFF
}

// ---------------------------------------------------------------------------
// RV32M — multiply / divide extension (LLD §3.4)
// ---------------------------------------------------------------------------

/// `rd = (rs1 * rs2)[31:0]` — the low half of the product.
pub fn mul(lhs: u32, rhs: u32) -> u32 {
    lhs.wrapping_mul(rhs)
}

/// `rd = (signed rs1 * signed rs2)[63:32]` — the high half of the
/// signed product.
pub fn mulh(lhs: u32, rhs: u32) -> u32 {
    (((lhs as i32 as i64) * (rhs as i32 as i64)) >> 32) as u32
}

/// `rd = (signed rs1 * unsigned rs2)[63:32]` — the high half of the
/// mixed-sign product.
pub fn mulhsu(lhs: u32, rhs: u32) -> u32 {
    (((lhs as i32 as i64) * (rhs as u64 as i64)) >> 32) as u32
}

/// `rd = (unsigned rs1 * unsigned rs2)[63:32]` — the high half of the
/// unsigned product.
pub fn mulhu(lhs: u32, rhs: u32) -> u32 {
    (((lhs as u64) * (rhs as u64)) >> 32) as u32
}

/// `rd = signed rs1 / signed rs2`.
///
/// Two cases are defined by the spec rather than trapped on (LLD
/// §3.4):
///
/// * a division by zero yields the all-ones quotient,
/// * `INT_MIN / -1` overflows and yields `INT_MIN`.
///
/// The overflow case has to be caught explicitly: Rust's `i32`
/// division panics on it, and a panic in the middle of a guest
/// instruction would take the whole simulation down instead of
/// producing the architecturally defined result.
pub fn div(lhs: u32, rhs: u32) -> u32 {
    if rhs == 0 {
        u32::MAX
    } else if lhs == 0x8000_0000 && rhs == u32::MAX {
        lhs
    } else {
        ((lhs as i32) / (rhs as i32)) as u32
    }
}

/// `rd = unsigned rs1 / unsigned rs2`; a division by zero yields the
/// all-ones quotient.
pub fn divu(lhs: u32, rhs: u32) -> u32 {
    lhs.checked_div(rhs).unwrap_or(u32::MAX)
}

/// `rd = signed rs1 % signed rhs`.
///
/// A division by zero yields the dividend, and the `INT_MIN % -1`
/// overflow yields zero — the same overflow Rust would panic on.
pub fn rem(lhs: u32, rhs: u32) -> u32 {
    if rhs == 0 {
        lhs
    } else if lhs == 0x8000_0000 && rhs == u32::MAX {
        0
    } else {
        ((lhs as i32) % (rhs as i32)) as u32
    }
}

/// `rd = unsigned rs1 % unsigned rhs`; a division by zero yields the
/// dividend.
pub fn remu(lhs: u32, rhs: u32) -> u32 {
    lhs.checked_rem(rhs).unwrap_or(lhs)
}
