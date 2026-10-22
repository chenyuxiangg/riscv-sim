// CSR-capable golden environment — the M-mode variant of riscv-tests' env.
//
// SPDX-License-Identifier: BSD-3-Clause
// (matches the upstream riscv-tests licence; this file is an independent
//  reimplementation, not a copy of the upstream header).
//
// ---------------------------------------------------------------------------
// WHY THIS FILE EXISTS
// ---------------------------------------------------------------------------
// The zero-CSR environment next door (`../riscv_test.h`) cannot host the
// machine-level corpus: those tests *are* about CSRs, traps and `mret`, and
// the zero-CSR env has neither a trap vector nor a way to program `mtvec`.
//
// This variant provides exactly what `isa/rv32mi/*.S` needs:
//
//   * a **trap vector** that turns the PASS / FAIL `ecall` into a `tohost`
//     write (upstream's protocol), and
//   * a **reset sequence** that programs `mtvec` / `mie` / `mstatus`,
//     sets `mepc`, and returns with `mret`.
//
// The **test bodies** stay upstream's, unmodified — they are the source of
// truth.  This file only provides the room they run in.
//
// ---------------------------------------------------------------------------
// THE VERDICT PATH
// ---------------------------------------------------------------------------
// Upstream encodes the verdict in the `ecall` itself:
//
//     RVTEST_PASS : ecall(exit=0)  -> trap -> sw 1        -> tohost
//     RVTEST_FAIL : ecall(exit=n)  -> trap -> sw (n<<1)|1 -> tohost
//
// The trap vector recognises the three ECALL causes and writes `TESTNUM` to
// `tohost`; anything else is an unexpected exception and is reported as
// test number 1337.
//
// ⚠️ **A machine-mode `ecall` is caught by the same branch.**  That is
// upstream's design, not ours: upstream's own `rv64si/scall.S` says so in a
// comment ("the M-mode version of this test might not actually invoke the
// following handler ... Either way, we'll get the coverage we desire").
// We reproduce it **verbatim** — changing it would make the reference side
// and the candidate side run different programs.
//
// ---------------------------------------------------------------------------
// WHAT IS DIFFERENT FROM UPSTREAM'S `env/p/riscv_test.h`
// ---------------------------------------------------------------------------
// Only the parts that cannot work on this platform:
//
//   * `INIT_PMP` still writes `pmpaddr0` / `pmpcfg0`, but this platform has
//     **no PMP** ⇒ the writes are ignored and every access is permitted,
//     which is what upstream's "permit all" PMP entry achieves anyway;
//   * `INIT_RNMI` / `INIT_SATP` write `mnstatus` / `satp`, which are
//     ignored here for the same reason (no RNMI, no paging);
//   * `DELEGATE_NO_TRAPS` writes `medeleg` / `mideleg`, which are ignored
//     (this platform is M-mode only).
//
// ⚠️ The writes are **left in** rather than removed: they are part of what
// the upstream program does, and removing them would change the
// instruction stream the two sides execute.
//
// ---------------------------------------------------------------------------
// THE INCLUDE PATH
// ---------------------------------------------------------------------------
// `#include "../encoding.h"` resolves **relative to this file**, i.e. to
// `tests/golden/env/encoding.h` — the same directory relationship upstream
// has (`env/p/riscv_test.h` -> `env/encoding.h`).

#ifndef _GOLDEN_CSR_ENV_RISCV_TEST_H
#define _GOLDEN_CSR_ENV_RISCV_TEST_H

#include "../encoding.h"

// ---------------------------------------------------------------------------
// Begin macro
// ---------------------------------------------------------------------------

// User-mode variants: the `init` hook is empty.
#define RVTEST_RV64U  .macro init; .endm
#define RVTEST_RV32U  .macro init; .endm
#define RVTEST_RV64UF .macro init; .endm
#define RVTEST_RV32UF .macro init; .endm
#define RVTEST_RV64UV .macro init; .endm
#define RVTEST_RV32UV .macro init; .endm

// Machine-mode variants: `init` sets `mstatus.MPP` to M.
#define RVTEST_RV64M                                                    \
  .macro init;                                                          \
  RVTEST_ENABLE_MACHINE;                                                \
  .endm

#define RVTEST_RV32M                                                    \
  .macro init;                                                          \
  RVTEST_ENABLE_MACHINE;                                                \
  .endm

// Supervisor-mode variants: `init` sets `mstatus.MPP` to S and delegates
// the software interrupts.
//
// ⚠️ The platform is M-mode only, so `RVTEST_ENABLE_SUPERVISOR` is kept for
// source compatibility with the upstream test bodies; the `rv32si` corpus
// is out of scope (ledger D-94).
#define RVTEST_RV64S                                                    \
  .macro init;                                                          \
  RVTEST_ENABLE_SUPERVISOR;                                             \
  .endm

#define RVTEST_RV32S                                                    \
  .macro init;                                                          \
  RVTEST_ENABLE_SUPERVISOR;                                             \
  .endm

// XLEN check.
//
// ⚠️ The RV32 spelling (`bltz`) is the one that matters here; upstream
// picks between `bgez` / `bltz` on `__riscv_xlen`, and this environment is
// only ever assembled for RV32.
#define CHECK_XLEN li a0, 1; slli a0, a0, 31; bltz a0, 1f; RVTEST_PASS; 1:

// Zero every architectural register.  Keeps a run deterministic regardless
// of whatever the hart's reset value was.
#define INIT_XREG                                                           \
  li x1, 0;   li x2, 0;   li x3, 0;   li x4, 0;                           \
  li x5, 0;   li x6, 0;   li x7, 0;   li x8, 0;                           \
  li x9, 0;   li x10, 0;  li x11, 0;  li x12, 0;                          \
  li x13, 0;  li x14, 0;  li x15, 0;  li x16, 0;                          \
  li x17, 0;  li x18, 0;  li x19, 0;  li x20, 0;                          \
  li x21, 0;  li x22, 0;  li x23, 0;  li x24, 0;                          \
  li x25, 0;  li x26, 0;  li x27, 0;  li x28, 0;                          \
  li x29, 0;  li x30, 0;  li x31, 0;

// A permissive PMP entry.  ⚠️ **Ignored on this platform** (no PMP) — but
// the write stays, so that both sides execute the same instruction stream.
#define INIT_PMP                                                        \
  la t0, 1f;                                                            \
  csrw mtvec, t0;                                                       \
  li t0, (1 << (31 + (__riscv_xlen / 64) * (53 - 31))) - 1;             \
  csrw pmpaddr0, t0;                                                    \
  li t0, PMP_NAPOT | PMP_R | PMP_W | PMP_X;                             \
  csrw pmpcfg0, t0;                                                     \
  .align 2;                                                             \
1:

// RNMI (resume-from-non-maskable-interrupt).  ⚠️ **Ignored on this
// platform** — the write stays for the same reason as above.
#define INIT_RNMI                                                       \
  la t0, 1f;                                                            \
  csrw mtvec, t0;                                                       \
  csrwi CSR_MNSTATUS, MNSTATUS_NMIE;                                    \
  .align 2;                                                             \
1:

// Address translation.  ⚠️ **Ignored on this platform** (no paging).
#define INIT_SATP                                                      \
  la t0, 1f;                                                            \
  csrw mtvec, t0;                                                       \
  csrwi satp, 0;                                                       \
  .align 2;                                                             \
1:

// Delegate nothing, and point `mtvec` at the next instruction (so that a
// trap taken by the setup itself lands somewhere harmless).
#define DELEGATE_NO_TRAPS                                               \
  csrwi mie, 0;                                                         \
  la t0, 1f;                                                            \
  csrw mtvec, t0;                                                       \
  csrwi medeleg, 0;                                                     \
  csrwi mideleg, 0;                                                     \
  .align 2;                                                             \
1:

// Enter supervisor mode from machine mode.
#define RVTEST_ENABLE_SUPERVISOR                                        \
  li a0, MSTATUS_MPP & (MSTATUS_MPP >> 1);                              \
  csrs mstatus, a0;                                                     \
  li a0, SIP_SSIP | SIP_STIP;                                           \
  csrs mideleg, a0;                                                     \

// Enter machine mode from machine mode.
//
// ⚠️ This **only** sets `mstatus.MPP`.  It is the mode the hart resumes
// in after the reset sequence's `mret`.
#define RVTEST_ENABLE_MACHINE                                           \
  li a0, MSTATUS_MPP;                                                   \
  csrs mstatus, a0;                                                     \

// Only one hart.
#define RISCV_MULTICORE_DISABLE                                         \
  csrr a0, mhartid;                                                     \
  1: bnez a0, 1b

// Optional hooks referenced by the begin macro.
#define EXTRA_TVEC_USER
#define EXTRA_TVEC_MACHINE
#define EXTRA_INIT
#define EXTRA_INIT_TIMER
#define EXTRA_DATA
#define FILTER_TRAP
#define FILTER_PAGE_FAULT

// ⚠️ No interrupt is expected in the machine-level corpus.
#define INTERRUPT_HANDLER j other_exception

// Test number register, matching upstream (`gp`).
#define TESTNUM gp

// ---------------------------------------------------------------------------
// The reset sequence and the trap vector
// ---------------------------------------------------------------------------
#define RVTEST_CODE_BEGIN                                               \
        .section .text.init;                                            \
        .align  6;                                                      \
        .weak stvec_handler;                                            \
        .weak mtvec_handler;                                            \
        .globl _start;                                                  \
_start:                                                                 \
        /* reset vector */                                              \
        j reset_vector;                                                 \
        .align 2;                                                       \
trap_vector:                                                            \
        /* test whether the test came from pass/fail */                 \
        csrr t5, mcause;                                                \
        li t6, CAUSE_USER_ECALL;                                        \
        beq t5, t6, write_tohost;                                       \
        li t6, CAUSE_SUPERVISOR_ECALL;                                  \
        beq t5, t6, write_tohost;                                       \
        li t6, CAUSE_MACHINE_ECALL;                                     \
        beq t5, t6, write_tohost;                                       \
        /* if an mtvec_handler is defined, jump to it */                \
        la t5, mtvec_handler;                                           \
        beqz t5, 1f;                                                    \
        jr t5;                                                          \
        /* was it an interrupt or an exception? */                      \
  1:    csrr t5, mcause;                                                \
        bgez t5, handle_exception;                                      \
        INTERRUPT_HANDLER;                                              \
handle_exception:                                                       \
        /* we don't know how to handle whatever the exception was */    \
  other_exception:                                                      \
        /* some unhandlable exception occurred */                       \
  1:    ori TESTNUM, TESTNUM, 1337;                                     \
  write_tohost:                                                         \
        sw TESTNUM, tohost, t5;                                         \
        sw zero, tohost + 4, t5;                                        \
        j write_tohost;                                                 \
reset_vector:                                                           \
        INIT_XREG;                                                      \
        RISCV_MULTICORE_DISABLE;                                        \
        INIT_RNMI;                                                      \
        INIT_SATP;                                                      \
        INIT_PMP;                                                       \
        DELEGATE_NO_TRAPS;                                              \
        li TESTNUM, 0;                                                  \
        la t0, trap_vector;                                             \
        csrw mtvec, t0;                                                 \
        CHECK_XLEN;                                                     \
        /* if an stvec_handler is defined, delegate exceptions to it */ \
        la t0, stvec_handler;                                           \
        beqz t0, 1f;                                                    \
        csrw stvec, t0;                                                 \
        li t0, (1 << CAUSE_LOAD_PAGE_FAULT) |                           \
               (1 << CAUSE_STORE_PAGE_FAULT) |                          \
               (1 << CAUSE_FETCH_PAGE_FAULT) |                          \
               (1 << CAUSE_MISALIGNED_FETCH) |                          \
               (1 << CAUSE_USER_ECALL) |                                \
               (1 << CAUSE_BREAKPOINT);                                 \
        csrw medeleg, t0;                                               \
1:      csrwi mstatus, 0;                                               \
        init;                                                           \
        EXTRA_INIT;                                                     \
        EXTRA_INIT_TIMER;                                               \
        la t0, 1f;                                                      \
        csrw mepc, t0;                                                  \
        csrr a0, mhartid;                                               \
        mret;                                                           \
1:

#define RVTEST_CODE_END                                                 \
        unimp

// ---------------------------------------------------------------------------
// Pass / Fail
// ---------------------------------------------------------------------------
//
// ⚠️ The verdict is carried by the **ECALL**, not by a direct `tohost`
// write: the trap vector turns it into the `tohost` write.  That is what
// makes the machine-mode corpus meaningful — the trap path is exercised on
// every single test.

#define RVTEST_PASS                                                     \
        fence;                                                          \
        li TESTNUM, 1;                                                  \
        li a7, 93;                                                      \
        li a0, 0;                                                       \
        ecall

#define RVTEST_FAIL                                                     \
        fence;                                                          \
1:      beqz TESTNUM, 1b;                                               \
        sll TESTNUM, TESTNUM, 1;                                        \
        or TESTNUM, TESTNUM, 1;                                         \
        li a7, 93;                                                      \
        addi a0, TESTNUM, 0;                                            \
        ecall

// ---------------------------------------------------------------------------
// Data section
// ---------------------------------------------------------------------------
//
// `tohost` / `fromhost` must be **global symbols** — Spike's HTIF locates
// them through the ELF symbol table and refuses to start without them.

#define RVTEST_DATA_BEGIN                                               \
        EXTRA_DATA                                                      \
        .pushsection .tohost,"aw",@progbits;                            \
        .align 6; .global tohost; tohost: .dword 0; .size tohost, 8;    \
        .align 6; .global fromhost; fromhost: .dword 0; .size fromhost, 8;\
        .popsection;                                                    \
        .align 4; .global begin_signature; begin_signature:

#define RVTEST_DATA_END .align 4; .global end_signature; end_signature:

#endif
