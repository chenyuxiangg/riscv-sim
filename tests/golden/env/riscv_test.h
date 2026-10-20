// Minimal, CSR-free drop-in replacement for riscv-tests' `env/p/riscv_test.h`.
//
// SPDX-License-Identifier: BSD-3-Clause
// (matches the upstream riscv-tests licence; this file is an independent
//  reimplementation, not a copy of the upstream header).
//
// ---------------------------------------------------------------------------
// WHY THIS FILE EXISTS
// ---------------------------------------------------------------------------
// The upstream riscv-tests environment is machine-mode oriented:
// `RVTEST_CODE_BEGIN` programs `mtvec` / `mstatus` / `mepc` / `satp` /
// `pmpaddr0` / `pmpcfg0` / `mie` / `medeleg` / `mideleg` / `mnstatus`,
// reads `mhartid`, and finishes with `mret`; `RVTEST_PASS` and
// `RVTEST_FAIL` then report the verdict by raising an ECALL (`a7 = 93`)
// which the trap vector converts into a `sw` to `tohost`.
//
// Assembling the upstream environment needs the `zicsr` extension, and
// running it needs full trap delivery.  Our ISS (M-13) implements the
// RV32I *user-level* ISA; trap delivery and the interrupt controller
// (M-16) are a later phase (see the project plan, phase 5).
//
// The wire protocol that Spike's built-in HTIF speaks is far simpler than
// the route upstream takes to reach it:
//
//     tohost = (exit_code << 1) | 1
//
// Spike's `syscall_t::handle_syscall` does exactly
// `if (cmd.payload() & 1) { htif->exitcode = cmd.payload(); }`, and
// `htif_t::exit_code()` is `exitcode >> 1`.  The upstream encoding
// already matches that exactly:
//
//     upstream RVTEST_PASS : ecall(exit=0) -> trap -> sw 1        -> tohost
//     upstream RVTEST_FAIL : ecall(exit=n) -> trap -> sw (n<<1)|1 -> tohost
//
// so the whole CSR + trap machinery can be dropped and `tohost` written
// directly.  The verdict encoding stays byte-for-byte identical to
// upstream, which is what makes the result comparable with a reference
// run.
//
// ---------------------------------------------------------------------------
// WHAT IS PRESERVED
// ---------------------------------------------------------------------------
//   * `TESTNUM` semantics (`gp` holds the failing test number)
//   * the PASS / FAIL verdict encoding
//   * the `tohost` / `fromhost` global symbols and the `.tohost` layout
//   * the `init` hook that `RVTEST_RV32U` defines
//   * `CHECK_XLEN`, so a wrong XLEN still reports PASS immediately
//
// ---------------------------------------------------------------------------
// WHAT IS DROPPED — and why that is safe for this corpus
// ---------------------------------------------------------------------------
//   * PMP configuration (`pmpcfg0` / `pmpaddr0`) — upstream sets a
//     permissive PMP so that S-mode would work; we only ever run M-mode
//   * delegation (`medeleg` / `mideleg` / `mie` / `mstatus`) — no
//     S-mode, no interrupts
//   * `mnstatus` / `satp` — no RNMI, no paging
//   * the trap vector and the ECALL round-trip
//
// Tests that genuinely *need* a trap are therefore **not** supported by
// this environment and are excluded from the corpus:
//
//   * `isa/rv32mi/*` — machine-level: CSR access, breakpoints, PMP,
//     `mret`, trap-and-continue handlers
//   * `isa/rv32si/*` — supervisor-level: the same plus S-mode
//
// They come back when M-16 (interrupt controller) and trap delivery land.
// `tests/golden/corpus.txt` is the authoritative list of what is in scope.

#ifndef _GOLDEN_MINI_ENV_RISCV_TEST_H
#define _GOLDEN_MINI_ENV_RISCV_TEST_H

// Test number register, matching upstream (`gp`).
#define TESTNUM gp

// "init" hook: upstream's `RVTEST_RV32U` defines it as an empty macro and
// `RVTEST_CODE_BEGIN` invokes it.  We keep the same shape.
//
// Note that `isa/rv32ui/*.S` redefine `RVTEST_RV64U` to `RVTEST_RV32U`
// before pulling in the rv64 source, so **only one** of these may expand
// per translation unit — otherwise the assembler would see `init` defined
// twice.  The rv32 wrappers expand exactly one.
#define RVTEST_RV32U  .macro init; .endm
#define RVTEST_RV32UF .macro init; .endm
#define RVTEST_RV32UV .macro init; .endm
#define RVTEST_RV64U  .macro init; .endm
#define RVTEST_RV64UF .macro init; .endm
#define RVTEST_RV64UV .macro init; .endm

// Optional hooks referenced by upstream `RVTEST_CODE_BEGIN`.
#define EXTRA_INIT
#define EXTRA_INIT_TIMER
#define EXTRA_DATA
#define EXTRA_TVEC_USER
#define EXTRA_TVEC_MACHINE
#define FILTER_TRAP
#define FILTER_PAGE_FAULT

// XLEN check.  On RV32, `a0 = 1 << 31` must be negative.  Upstream reaches
// `RVTEST_PASS` from here; ours writes `tohost` directly.
//
// This macro **is** called from `RVTEST_CODE_BEGIN` -- upstream calls it
// too (right after the CSR setup).  A defined-but-never-called macro is
// a lie in the documentation, and the XLEN check is the cheapest possible
// sanity gate on the whole environment.
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

// Reset vector.  No CSR access, no `mret`: control falls straight through.
#define RVTEST_CODE_BEGIN                                                  \
        .section .text.init;                                               \
        .align  6;                                                         \
        .globl _start;                                                     \
_start:                                                                    \
        j reset_vector;                                                    \
        .align 2;                                                          \
reset_vector:                                                              \
        INIT_XREG;                                                         \
        li sp, 0x7ffff000;                                                 \
        li TESTNUM, 0;                                                     \
        CHECK_XLEN;                                                        \
        init;                                                              \
        EXTRA_INIT;                                                        \
        EXTRA_INIT_TIMER;                                                  \
        1:

#define RVTEST_CODE_END unimp

// PASS: `tohost = 1`  =>  Spike exit code `1 >> 1 == 0`.
//
// `sw rs2, symbol, base` is GNU as' RISC-V form for "store to the absolute
// address of `symbol`, using `base` as the scratch base register"; it
// expands to `auipc base, ...` + `sw rs2, off(base)`.  Upstream
// riscv-tests uses exactly this form, so we stay compatible with it.
#define RVTEST_PASS                                                         \
        fence;                                                             \
        li TESTNUM, 1;                                                     \
        sw TESTNUM, tohost, t5;                                            \
        sw zero, tohost + 4, t5;                                           \
        j .

// FAIL: `tohost = (testnum << 1) | 1`  =>  Spike exit code == testnum.
#define RVTEST_FAIL                                                         \
        fence;                                                             \
1:      beqz TESTNUM, 1b;                                                  \
        sll TESTNUM, TESTNUM, 1;                                           \
        or TESTNUM, TESTNUM, 1;                                            \
        sw TESTNUM, tohost, t5;                                            \
        sw zero, tohost + 4, t5;                                           \
        j .

// Data section.  `tohost` / `fromhost` must be **global symbols** —
// Spike's HTIF locates them through the ELF symbol table, not through a
// fixed address, and refuses to start without them
// ("tohost and fromhost symbols not in ELF; can't communicate with target").
#define RVTEST_DATA_BEGIN                                                  \
        .pushsection .tohost,"aw",@progbits;                               \
        .align 6; .global tohost; tohost: .dword 0; .size tohost, 8;       \
        .align 6; .global fromhost; fromhost: .dword 0;                    \
        .size fromhost, 8;                                                 \
        .popsection;                                                       \
        .align 4; .global begin_signature; begin_signature:

#define RVTEST_DATA_END .align 4; .global end_signature; end_signature:

#endif
