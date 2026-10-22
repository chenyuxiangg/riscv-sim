// A golden case: a **compressed** store to an unmapped address must trap,
// and the trap context must carry the **exact** faulting PC.
//
// STAGE-7 adjudication I / ledger D-93: the trap entry writes the exact
// faulting address into `mepc` — it does **not** mask on write.  The
// reference side was measured doing exactly that (a `c.sw` at
// `0x8000014a`, whose low two bits are `0b10`, produced
// `mepc = 0x8000014a` — bit 1 preserved).
//
// ⚠️ **Why this case exists at all**: upstream riscv-tests never traps
// from a 2-byte-aligned instruction, so a write-side mask on `mepc`
// would be invisible to the rest of the corpus.  Masking `mepc` on
// write turns `0x8000014a` into `0x80000148` — one bit away, and the
// reference side disagrees.
//
// ⚠️ **The verdict is the comparison.**  The guest checks all three
// context values itself (`mcause` == 7, `mtval` == the faulting
// address, `mepc` == the faulting instruction) and only reports PASS
// when all three are exactly right.  "Both sides stopped erroring"
// would be green for two sides that both abort.
//
// The file is deliberately **self-contained**: it includes no
// riscv-tests source, so it keeps working if the upstream tree changes
// shape (same rule as `canary/fail_3.S` and `local/unmapped_store.S`).
//
//   TESTNUM = 3  ->  the failure path reports FAIL 3 on both sides
//                   (the encoding is upstream's: tohost = (3 << 1) | 1)
#include "riscv_test.h"

RVTEST_RV32M
RVTEST_CODE_BEGIN

  // `c.sw` needs its base and value registers inside x8..x15:
  // base = s1 (x9), value = a1 (x11).
  li    s1, 0x90000000        # unmapped on both sides
  li    a1, 0x1234

  .balign 4
  c.nop                       # 2 bytes -> the faulting store lands at 4k+2
fault_pc:
  c.sw  a1, 0(s1)             # <- the faulting compressed store
  j     fail                  # (unreachable: the trap diverts first)

mtvec_handler:
  csrr  t0, mcause
  csrr  t1, mtval
  csrr  t2, mepc
  la    t3, fault_pc           # the exact address of the faulting store
  li    t4, 7                  # mcause = store / AMO access fault
  bne   t0, t4, fail
  bne   t1, s1, fail           # mtval = the faulting address
  bne   t2, t3, fail           # mepc  = the **exact** faulting PC
  // All three match: the trap context is exactly what the reference
  // side reports — including the uncompressed bit of `mepc`.
  li    TESTNUM, 1
  RVTEST_PASS

fail:
  li    TESTNUM, 3
  RVTEST_FAIL

RVTEST_CODE_END

  .data
RVTEST_DATA_BEGIN

RVTEST_DATA_END
