// A golden case: an **instruction fetch** from an unmapped address must
// trap with `mcause = 1` (instruction access fault).
//
// STAGE-10 adjudication B / ledger D-103: the reference side (Spike)
// traps with `mcause = 1` (instruction access fault) and `mtval` = the
// faulting fetch address.
//
// ⚠️ This is the **sibling** of `unmapped_load.S` / `unmapped_store.S`,
// and it is not redundant: RISC-V numbers the three access faults
// differently (instruction `1`, load `5`, store `7`), so a platform that
// trapped with one number for all three would agree with the reference
// side on **one** of the three cases and diverge on the other two.  One
// case per number is what makes the split observable.
//
// ⚠️ **The verdict is the comparison.**  The harness compares the
// tohost exit code, so the **guest itself** checks `mcause` / `mtval` /
// `mepc` and only reports PASS when all three are exactly right.  A
// judge that only asked "did both sides stop erroring?" would be green
// for two sides that both abort — the guest's own check is what makes
// the comparison discriminating.
//
// ⚠️ **All three values are checked, and the reference side agrees on
// all three.**  Measured on the reference side (Spike) through this
// very pipeline — the same `RVTEST_PASS` / `RVTEST_FAIL` tohost
// protocol the corpus itself uses: a variant that checks all three
// reports PASS (exit code 0), and the **reverse control** (the same
// variant with the `mepc` expectation changed to `0`) reports FAIL 3
// (exit code 3) ⇒ the `mepc` check really has discriminating power and
// is not "never reached".
//
// `mepc` is the address of the instruction that caused the trap.  For an
// instruction access fault that is the address that **could not be
// fetched** — the fetch address — which is exactly what the platform's
// own trap entry (`mepc := state.pc`, taken at the trap entry) reports.
//
// ⚠️ The address `0x90000000` is **outside** both sides' memory maps
// (RAM is `0x8000_0000` + 4 MiB, the stack window is at
// `0x7fff_f000`), so the fetch really faults on both sides.
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
  li    s0, 1                  // mcause: instruction access fault
  li    s1, 0x90000000         // mtval:  the faulting fetch address
                                // mepc:   the same address — the
                                //          instruction that could not
                                //          be fetched

faulting_fetch:
  jalr  x0, 0(s1)              // <- the fetch that faults
  j     fail                   // (unreachable: the trap diverts first)

  .align 2
mtvec_handler:
  csrr  t0, mcause
  csrr  t1, mtval
  csrr  t2, mepc
  bne   t0, s0, fail
  bne   t1, s1, fail
  bne   t2, s1, fail
  // All three match: the instruction access fault is exactly what the
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
