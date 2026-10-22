// Minimal `encoding.h` for the golden corpus.
//
// SPDX-License-Identifier: BSD-3-Clause
//
// ---------------------------------------------------------------------------
// WHY THIS FILE EXISTS
// ---------------------------------------------------------------------------
// Upstream riscv-tests ships a 3000+-define `env/encoding.h`.  We only need
// the handful of constants that the **CSR-capable** environment variant
// (`tests/golden/env/csr/riscv_test.h`) and the upstream test bodies
// (`isa/rv32mi/*.S` and the `isa/rv64{mi,si}/*.S` they include) actually
// reference.
//
// ⚠️ **Only the constants that are really referenced are here.**  A constant
// that no source file uses is dead weight that has to be kept in sync with
// upstream for no benefit; a constant that *is* used but missing here makes
// the assembly fail with an opaque "unrecognized opcode" message.
//
// ⚠️ **The values are upstream's, not ours.**  They are architectural
// encodings (CSR numbers, `mcause` codes, `mstatus` bit masks) that the
// RISC-V specification fixes — they are **not** platform choices.  Copying
// them verbatim is the whole point: both sides of the golden comparison
// must agree on the *encoding* of the instruction stream, and the test
// bodies encode these numbers into the instructions they execute.
//
// ---------------------------------------------------------------------------
// HOW TO ADD A CONSTANT
// ---------------------------------------------------------------------------
// 1. Find the upstream value:
//        grep -E '^#define <NAME>\b' $RISCV_TESTS_DIR/env/encoding.h
// 2. Copy it **verbatim** (including any parenthesised expression) into the
//    matching group below.
// 3. Re-run the corpus: `tools/golden-run.sh --keep-going`.
//
// ⚠️ Do **not** "improve" a value, and do not derive one from another
// unless upstream does the same — a "cleaned up" constant is a **silent
// divergence** from the reference side, and the corpus would then be
// comparing two different programs.

#ifndef _GOLDEN_ENCODING_H
#define _GOLDEN_ENCODING_H

// ---------------------------------------------------------------------------
// `mstatus` bit masks (RV32)
// ---------------------------------------------------------------------------
#define MSTATUS_MIE         0x00000008
#define MSTATUS_MPP         0x00001800
#define MSTATUS_FS          0x00006000
#define MSTATUS_VS          0x00000600
#define MSTATUS_MPRV        0x00020000
#define MSTATUS_SUM         0x00040000
#define MSTATUS_TVM         0x00100000
#define MSTATUS_TSR         0x00400000

// ---------------------------------------------------------------------------
// `sstatus` bit masks (RV32) — the S-mode aliases the test bodies use
// ---------------------------------------------------------------------------
#define SSTATUS_SPP         0x00000100
#define SSTATUS_SIE         0x00000002
#define SSTATUS_SPIE        0x00000020
#define SSTATUS_SUM         0x00040000
#define SSTATUS_MXR         0x00080000
#define SSTATUS_UXL         0x0000000300000000

// ---------------------------------------------------------------------------
// Privilege levels
// ---------------------------------------------------------------------------
#define PRV_U 0
#define PRV_S 1
#define PRV_M 3

// ---------------------------------------------------------------------------
// `mcause` / `scause` codes
// ---------------------------------------------------------------------------
#define CAUSE_MISALIGNED_FETCH     0x0
#define CAUSE_ILLEGAL_INSTRUCTION  0x2
#define CAUSE_BREAKPOINT           0x3
#define CAUSE_MISALIGNED_LOAD      0x4
#define CAUSE_LOAD_ACCESS          0x5
#define CAUSE_MISALIGNED_STORE     0x6
#define CAUSE_STORE_ACCESS         0x7
#define CAUSE_USER_ECALL           0x8
#define CAUSE_SUPERVISOR_ECALL     0x9
#define CAUSE_MACHINE_ECALL        0xb
#define CAUSE_FETCH_PAGE_FAULT     0xc
#define CAUSE_LOAD_PAGE_FAULT      0xd
#define CAUSE_STORE_PAGE_FAULT     0xf

// ---------------------------------------------------------------------------
// Interrupt pending bits
// ---------------------------------------------------------------------------
#define IRQ_S_SOFT                 1
#define IRQ_M_SOFT                 3
#define IRQ_S_TIMER                5
#define IRQ_M_TIMER                7
#define IRQ_S_EXT                  9
#define IRQ_M_EXT                  11

#define MIP_SSIP            (1 << IRQ_S_SOFT)
#define MIP_STIP            (1 << IRQ_S_TIMER)
#define SIP_SSIP            MIP_SSIP
#define SIP_STIP            MIP_STIP

// ---------------------------------------------------------------------------
// PMP configuration bits
// ---------------------------------------------------------------------------
#define PMP_R     0x01
#define PMP_W     0x02
#define PMP_X     0x04
#define PMP_A     0x18
#define PMP_NAPOT 0x18

// ---------------------------------------------------------------------------
// Debug / trigger CSR bits
// ---------------------------------------------------------------------------
#define MCONTROL_LOAD       (1 << 0)
#define MCONTROL_STORE      (1 << 1)
#define MCONTROL_EXECUTE    (1 << 2)
#define MCONTROL_M          (1 << 6)

// ---------------------------------------------------------------------------
// CSR **numbers** the environment writes directly
// ---------------------------------------------------------------------------
#define CSR_MNSTATUS 0x744
#define MNSTATUS_NMIE 0x00000008

// ---------------------------------------------------------------------------
// Address translation
// ---------------------------------------------------------------------------
#define RISCV_PGSHIFT 12
#define RISCV_PGSIZE (1 << RISCV_PGSHIFT)
#define SATP_MODE_SV39 8

#endif
