// A golden case: a **store** to an unmapped address must trap.
//
// STAGE-7 adjudication H / ledger D-99: the reference side (Spike)
// traps with `mcause = 7` (store access fault) and `mtval` = the
// faulting address.  The candidate used to abort with the bus error
// (`E_BUS-01`) instead — the guest's trap handler never ran.
//
// ⚠️ **The verdict is the comparison.**  The harness compares the
// tohost exit code, so the **guest itself** checks `mcause` / `mtval` /
// `mepc` and only reports PASS when all three are exactly right.  A
// judge that only asked "did both sides stop erroring?" would be green
// for two sides that both abort — the guest's own check is what makes
// the comparison discriminating.
//
// ⚠️ The address `0x90000000` is **outside** both sides' memory maps
// (RAM is `0x8000_0000` + 4 MiB, the stack window is at
// `0x7fff_f000`), so the access really faults on both sides.
//
// The file is deliberately **self-contained**: it includes no
// riscv-tests source, so it keeps working if the upstream tree
// changes shape (same rule as `canary/fail_3.S`).
//
//   TESTNUM = 3  ->  the failure path reports FAIL 3 on both sides
//                   (the encoding is upstream's: tohost = (3 << 1) | 1)
#include "riscv_test.h"

RVTEST_RV32M
RVTEST_CODE_BEGIN

  // What the trap handler will check.
  li    s0, 7                  // mcause: store access fault
  li    s1, 0x90000000         // mtval:  the faulting address
  la    s2, faulting_store     // mepc:   the faulting instruction

  li    t1, 0x1234
faulting_store:
  sw    t1, 0(s1)              // <- the faulting store
  j     fail                   // (unreachable: the trap diverts first)

  .align 2
mtvec_handler:
  csrr  t0, mcause
  csrr  t1, mtval
  csrr  t2, mepc
  bne   t0, s0, fail
  bne   t1, s1, fail
  bne   t2, s2, fail
  // All three match: the store access fault is exactly what the
  // reference side reports.
  li    TESTNUM, 1
  RVTEST_PASS

fail:
  li    TESTNUM, 3
  RVTEST_FAIL

RVTEST_CODE_END

  .data
RVTEST_DATA_BEGIN

RVTEST_DATA_END
