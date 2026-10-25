// Supervisor-mode golden environment — the S-mode variant of riscv-tests' env.
//
// SPDX-License-Identifier: BSD-3-Clause
// (matches the upstream riscv-tests licence; this file is an independent
//  reimplementation, not a copy of the upstream header).
//
// ---------------------------------------------------------------------------
// WHY THIS FILE EXISTS
// ---------------------------------------------------------------------------
// It is a **verbatim copy of the `csr` variant** (`../csr/riscv_test.h`) with
// **five** changes, each of which exists for exactly one reason.  The
// `csr` variant itself is **not** touched — it stays byte for byte what it
// was (STAGE-22 adjudication M / C-1), because the M-mode corpus depends on
// it.
//
// The reason for a *new* variant is **not** "S mode cannot be reached".  It
// is **"a green row that proves nothing"** (STAGE-22 §8 correction 2):
//
//   * the `csr` variant's M-level trap vector treats `CAUSE_USER_ECALL` (8),
//     `CAUSE_SUPERVISOR_ECALL` (9) and `CAUSE_MACHINE_ECALL` (11) as
//     **PASS** ⇒ ⇒ in `rv32si/scall`, a `sret` that wrongly lands in **S**
//     mode makes the `ecall`'s cause 9 ⇒ ⇒ the trap vector calls that
//     **PASS** ⇒ ⇒ **both sides report PASS** ⇒ ⇒ the row's discriminating
//     power is **zero**;
//   * tightening the trap vector alone is **not** enough, because the
//     `RVTEST_PASS` *inside* the S-level handler is another `ecall` ⇒ cause
//     9 ⇒ ⇒ **PASS again** ⇒ ⇒ the false green survives.
//
// ⇒ ⇒ The only way to take the false green away is to **take the verdict out
// of the trap path**: the verdict is written **directly** to `tohost`.
//
// ---------------------------------------------------------------------------
// THE FIVE CHANGES (and why each one is there)
// ---------------------------------------------------------------------------
//
// (1) THE VERDICT IS WRITTEN DIRECTLY TO `tohost`.
//
//     RVTEST_PASS : sw 1        -> tohost
//     RVTEST_FAIL : sw (n<<1)|1 -> tohost
//
//     ⚠️ The **encoding is byte-identical** to the `csr` variant's, which
//     reaches the same two words through `ecall` -> trap -> `sw`: Spike's
//     HTIF takes `tohost << 16 >> 16` and exits with `payload >> 1` ⇒ ⇒
//     PASS is still exit code 0 and FAIL n is still exit code n ⇒ ⇒ the
//     harness's two-sided comparison is unchanged.
//
// (2) THE M-LEVEL TRAP VECTOR IS **STRICT**.
//
//     No cause is treated as "PASS" any more.  An unexpected trap writes the
//     **failure** verdict (upstream's `1337` convention).  The structure
//     upstream has — "if a `mtvec_handler` is defined, jump to it" — is
//     **kept**, so that a test can handle its own M-level traps; a test
//     that does not define one falls through to the failure verdict.
//
// (3) `medeleg` GETS **ONE MORE BIT** than upstream's mask: bit 2
//     (illegal instruction).  See "THE DELEGATION MASK" below.
//
// (4) `stvec` STILL POINTS AT THE TEST'S OWN `stvec_handler` — unchanged
//     from the `csr` variant (upstream's shape:
//     `la t0, stvec_handler; beqz t0, 1f; csrw stvec, t0`).  It is listed
//     here because it is **load-bearing** — see "THE DELEGATION MASK".
//
// (5) `medeleg`'s mask value — see "THE DELEGATION MASK" below.
//
// ---------------------------------------------------------------------------
// THE DELEGATION MASK
// ---------------------------------------------------------------------------
// The mask is upstream's, **plus bit 2**:
//
//     (1 << CAUSE_MISALIGNED_FETCH)     -> bit 0
//     (1 << CAUSE_BREAKPOINT)           -> bit 3
//     (1 << CAUSE_USER_ECALL)           -> bit 8
//     (1 << CAUSE_FETCH_PAGE_FAULT)     -> bit 12
//     (1 << CAUSE_LOAD_PAGE_FAULT)      -> bit 13
//     (1 << CAUSE_STORE_PAGE_FAULT)     -> bit 15
//     (1 << CAUSE_ILLEGAL_INSTRUCTION)  -> bit 2   <-- **the one extra bit**
//
//     => 0xB109 | 0x4 = **0xB10D**
//
// ⚠️ **Why the extra bit**: in S mode, "read / write an M-mode CSR" is an
// **illegal instruction** (cause 2).  If cause 2 is **not** delegated, the
// S-level handler never sees it ⇒ ⇒ a test that wants to observe S mode's
// per-level accessibility cannot observe it at all ⇒ ⇒ the delegation
// surface has to be **live** for cause 2.
//
// ⚠️ **Both sides land on `0xB10C`, and that is fine**: the reference
// side's `medeleg` has a **legal bit set** of `0x0008_B3FE` (per-bit
// WARL) ⇒ ⇒ **bit 0** (misaligned instruction fetch) is **not**
// delegable ⇒ ⇒ the value written here (`0xB10D`) is stored as **`0xB10C`**
// on the reference side **and** on the platform side ⇒ ⇒ the two sides
// agree ⇒ ⇒ the mask value needs no change.
//
// ⚠️ **Why the mask is `0xB10D` and not `0x11AD`**: `0x11AD` is *not*
// "upstream's mask plus one bit" — upstream's mask is **0xB109** (bits
// {0, 3, 8, 12, 13, 15}), which is what `env/p/riscv_test.h` actually
// assembles (the six `CAUSE_*` shifts above).  `0x11AD` would be bits
// {0, 2, 3, 5, 7, 8, 12} ⇒ ⇒ it would **drop** the two page-fault bits
// and **add** two access-fault bits upstream does not delegate ⇒ ⇒ it
// would not be "upstream's mask plus one bit" at all.  ⇒ ⇒ This env keeps
// upstream's mask **verbatim** and adds **exactly one** bit.
//
// ⚠️ **The `stvec_handler` / `medeleg` COUPLING (load-bearing!)**: the reset
// sequence is
//
//     la t0, stvec_handler;
//     beqz t0, 1f;            <-- jumps **past** the `csrw medeleg`
//     csrw stvec, t0;
//     li t0, <mask>;
//     csrw medeleg, t0;
//  1:  csrwi mstatus, 0;
//
// ⇒ ⇒ a test that does **not** define `stvec_handler` never reaches the
// `csrw medeleg` ⇒ ⇒ **`medeleg` is never written** ⇒ ⇒ the delegation
// surface of that test is **zero**.  ⇒ ⇒ every test that wants to observe
// delegation **must** define `stvec_handler` (the `rv32si/*` cases do, and
// so does `tests/golden/local/smode_csr.S`).
//
// ⚠️ **The `csr` variant cannot be changed for this**: the M-mode corpus
// defines `__MACHINE_MODE`, which aliases `stvec_handler` to
// `mtvec_handler` ⇒ ⇒ the symbol does not exist ⇒ ⇒ `medeleg` is never
// written there ⇒ ⇒ the M-mode corpus's delegation surface is zero **by
// construction** ⇒ ⇒ that is the structural reason the `csr` variant stays
// byte for byte what it is (STAGE-22 §8 correction 4).
//
// ---------------------------------------------------------------------------
// STALE COMMENTS INHERITED FROM THE `csr` VARIANT
// ---------------------------------------------------------------------------
// ⚠️ The macro blocks below are copied **verbatim** from `../csr/riscv_test.h`
// ⇒ ⇒ three of their comments describe the **M-mode-only** platform and are
// **wrong here**.  They are left as they are (the blocks are copied
// verbatim on purpose — the *code* is what both sides must agree on) and the
// corrections are recorded here and in `tests/golden/README.md`:
//
//   * `INIT_PMP`: "Ignored on this platform (no PMP)" ⇒ ⇒ **stale since
//     v0.37** ⇒ ⇒ the platform implements PMP now ⇒ ⇒ the write takes
//     effect, but the entry it configures is a NAPOT entry covering
//     `[0, 2^34)` ⇒ ⇒ it still permits **everything** ⇒ ⇒ the behaviour is
//     unchanged.
//   * `DELEGATE_NO_TRAPS`: "medeleg / mideleg ... are ignored (this
//     platform is M-mode only)" ⇒ ⇒ **wrong here** ⇒ ⇒ in this env
//     `medeleg` is the **live** delegation mask (see above).
//   * `RVTEST_ENABLE_SUPERVISOR`: "the platform is M-mode only, so ...
//     the `rv32si` corpus is out of scope (ledger D-94)" ⇒ ⇒ **wrong here**
//     ⇒ ⇒ this env exists **for** the `rv32si` corpus.
//
// ---------------------------------------------------------------------------
// THE INCLUDE PATH
// ---------------------------------------------------------------------------
// `#include "../encoding.h"` resolves **relative to this file**, i.e. to
// `tests/golden/env/encoding.h` — the same directory relationship upstream
// has (`env/p/riscv_test.h` -> `env/encoding.h`).

#ifndef _GOLDEN_SMODE_ENV_RISCV_TEST_H
#define _GOLDEN_SMODE_ENV_RISCV_TEST_H

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
        /* ⚠️ **STRICT** (this variant's one behavioural change): */     \
        /* no cause is treated as "PASS" here any more.  The `csr` */   \
        /* variant branched to `write_tohost` for the three ECALL */    \
        /* causes, which is exactly what made `rv32si/scall` a false  */\
        /* green: a `sret` that lands in S mode turns the `ecall` into  */\
        /* cause 9, and cause 9 was reported as PASS on **both** sides. */\
        /* ⇒ ⇒ now an unexpected trap is a **failure**. */               \
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
        /* ⚠️ **upstream's mask plus ONE bit**: bit 2 (illegal          \
           instruction).  Without it, "read / write an M-mode CSR in S    \
           mode" would never reach the S-level handler ⇒ ⇒ the          \
           delegation surface for per-level accessibility would be dead.  \
           ⇒ ⇒ 0xB109 | 0x4 = 0xB10D  (see the header). */            \
        li t0, (1 << CAUSE_LOAD_PAGE_FAULT) |                           \
               (1 << CAUSE_STORE_PAGE_FAULT) |                          \
               (1 << CAUSE_FETCH_PAGE_FAULT) |                          \
               (1 << CAUSE_MISALIGNED_FETCH) |                          \
               (1 << CAUSE_USER_ECALL) |                                \
               (1 << CAUSE_BREAKPOINT) |                                \
               (1 << CAUSE_ILLEGAL_INSTRUCTION);                        \
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
// ⚠️ The verdict does **not** travel through the trap path in this variant:
// it is written **straight to `tohost`**.  That is the whole point of the
// variant (see the header): the `csr` variant's M-level trap vector reports
// the three ECALL causes as PASS, which turns a wrong `sret` landing into a
// **false green on both sides**.
//
// The **encoding is the same** as the `csr` variant's, byte for byte:
// Spike's HTIF reads `tohost << 16 >> 16` and exits with `payload >> 1`
// ⇒ ⇒ PASS is `tohost = 1` (exit 0) and FAIL n is `tohost = (n<<1)|1`
// (exit n) — exactly what the `ecall` route ends up writing.
//
// ⚠️ `sw TESTNUM, tohost, t5` is the **three-operand** store: the assembler
// materialises `tohost`'s address into `t5` and stores to it.  It is
// upstream's own spelling (same line as the `csr` variant) ⇒ ⇒ the two
// variants write the same two words at the same address.
//
// ⚠️ After the verdict is written the hart **spins** in `write_tohost`
// instead of returning: the verdict must not be overwritten by whatever
// follows it (upstream's `ecall` never returns either).

#define RVTEST_PASS                                                     \
        fence;                                                          \
        li TESTNUM, 1;                                                  \
        sw TESTNUM, tohost, t5;                                         \
        sw zero, tohost + 4, t5;                                        \
        j write_tohost

#define RVTEST_FAIL                                                     \
        fence;                                                          \
1:      beqz TESTNUM, 1b;                                               \
        sll TESTNUM, TESTNUM, 1;                                        \
        or TESTNUM, TESTNUM, 1;                                         \
        sw TESTNUM, tohost, t5;                                         \
        sw zero, tohost + 4, t5;                                        \
        j write_tohost

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
