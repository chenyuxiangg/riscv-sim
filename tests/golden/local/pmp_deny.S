// A golden case: with `mstatus.MPRV = 1`, a **store that PMP denies** must
// trap — and the trap must be a PMP denial, not a bus error.
//
// STAGE-21 (v0.37 · PMP) adjudication A: the platform had **no PMP at all**,
// so the store below simply succeeded.  The reference side (Spike) denies it
// with `mcause = 7` (store access fault) and `mtval` = the faulting address.
//
// ⚠️ **The verdict is the comparison.**  The harness compares the tohost
// exit code, so the **guest itself** checks `mcause` / `mtval` / `mepc` and
// only reports PASS when all three are exactly right.
//
// ⚠️ **The denied address must be MAPPED on both sides.**  `0x80040000` is
// inside both sides' RAM window (`0x8000_0000` + 4 MiB), so the store can
// only fail because of PMP — not because the address is unmapped.  That is
// what separates this case from `unmapped_store.S` (which faults on
// `0x90000000`, unmapped on both sides).
//
// ⚠️ **The PMP encodings below follow the RISC-V privileged spec (§3.7.6),
// and every field was measured against the reference side first:**
//
//   * `pmpcfg` byte layout: `L` = bit 7, `A` = bits 4:3, `X` = bit 2,
//     `W` = bit 1, `R` = bit 0.  `A`: OFF = 0b00, TOR = 0b01, NA4 = 0b10,
//     NAPOT = 0b11.
//   * `pmpaddr` holds the physical address **shifted right by 2**
//     (address bits 33:2), so the granularity is 4 bytes.
//   * NAPOT: let `n` = the number of trailing ones in `pmpaddr`; the entry
//     covers `[ (pmpaddr & ~((1<<n)-1)) << 2 , + 2^(n+3) )` bytes.
//   * Priority: the **lowest-numbered matching** entry decides the access
//     (not "any entry that permits").
//   * M-mode loads/stores bypass PMP unless `mstatus.MPRV = 1` (or the
//     matching entry is locked, `L` = 1).
//
// Entry 0 is the DENY entry and entry 1 is the ALLOW entry: entry 0 has the
// lower number, so it wins on the overlapping address — that is what makes
// the store fail even though entry 1 would have permitted it.
//
//   entry 0: NAPOT, 4 KiB at 0x80040000, **no** permissions
//             pmpaddr0 = 0x200101ff  (0x80040000 >> 2, plus 9 trailing ones
//                                     ⇒ 2^(9+3) = 4096 bytes)
//             pmpcfg0  = 0x18        (A = NAPOT, no R/W/X)
//   entry 1: NAPOT, 1 MiB at 0x80000000, R + W + X
//             pmpaddr1 = 0x2001ffff  (0x80000000 >> 2, plus 17 trailing ones
//                                     ⇒ 2^(17+3) = 1 MiB)
//             pmpcfg1  = 0x1f        (A = NAPOT, R + W + X)
//
// The file is deliberately **self-contained**: it includes no riscv-tests
// source, so it keeps working if the upstream tree changes shape (same rule
// as `unmapped_store.S`).
//
// ⚠️ **The failure path must always be able to report.**  The verdict travels
// through an `ecall`, and the environment's trap vector turns it into the
// `tohost` write — which is a **store** ⇒ ⇒ with `mstatus.MPRV` still set,
// that write is itself subject to PMP ⇒ ⇒ if no entry covers the `tohost`
// address, the case could not report anything and would **hang** instead of
// failing.  ⇒ ⇒ every verdict path clears `MPRV` first (the same rule the
// probe infrastructure follows).
//
// ⚠️ **The failure modes carry distinct numbers**, so a red row says *which*
// invariant broke:
//
//   TESTNUM = 1  ->  PASS (the reference side's exact behaviour)
//   TESTNUM = 3  ->  the denied store was **not** denied (PMP did not refuse)
//   TESTNUM = 4  ->  the **control group** broke (PMP refused an access that
//                    entry 1 permits, or the read-back did not match)
//   TESTNUM = 5  ->  the trap happened but `mcause` / `mtval` / `mepc` did
//                    **not** match (e.g. PMP refused the *wrong* address)
#include "riscv_test.h"

RVTEST_RV32M
RVTEST_CODE_BEGIN

  // ---- the PMP configuration ------------------------------------------
  // ⚠️ RV32's `pmpcfg` register holds **four** 8-bit entries: byte 0 is
  // entry 0, byte 1 is entry 1.  So entry 1 must be packed into
  // `pmpcfg0` — writing `pmpcfg1` would configure entries 4..7 instead.
  li    t0, 0x200101ff          // entry 0: NAPOT 4 KiB at 0x80040000
  csrw  pmpaddr0, t0
  li    t0, 0x2001ffff          // entry 1: NAPOT 1 MiB at 0x80000000
  csrw  pmpaddr1, t0
  li    t0, 0x1f18              // entry 0 = 0x18 (NAPOT, no permissions)
                                // entry 1 = 0x1f (NAPOT, R + W + X)
  csrw  pmpcfg0, t0

  // ---- make M-mode loads/stores subject to PMP -------------------------
  li    t0, 1
  slli  t0, t0, 17              // mstatus.MPRV
  csrs  mstatus, t0

  // ---- control: a permitted access must still go through ---------------
  // 0x80002000 is inside entry 1 (and outside entry 0) ⇒ the store and the
  // load must both succeed.  Without this control the case would pass even
  // if the platform denied *every* access, which is not what PMP means.
  li    t1, 0x80002000
  li    t3, 0x55
  sw    t3, 0(t1)                // permitted write (entry 1 has W)
  lw    t2, 0(t1)                // permitted read  (entry 1 has R)
  bne   t2, t3, control_failed   // the read must deliver the stored value

  // What the trap handler will check.
  li    s0, 7                   // mcause: store access fault
  li    s1, 0x80040000          // mtval:  the faulting address
  la    s2, faulting_store      // mepc:   the faulting instruction

  li    t1, 0x1234
faulting_store:
  sw    t1, 0(s1)               // <- the faulting store
  j     not_denied              // (unreachable: the trap diverts first)

  .align 2
mtvec_handler:
  csrr  t0, mcause
  csrr  t1, mtval
  csrr  t2, mepc
  bne   t0, s0, wrong_trap
  bne   t1, s1, wrong_trap
  bne   t2, s2, wrong_trap
  // All three match: the store access fault is exactly what the
  // reference side reports.
  li    TESTNUM, 1
  j     report_pass

control_failed:
  li    TESTNUM, 4
  j     report_fail

not_denied:
  li    TESTNUM, 3
  j     report_fail

wrong_trap:
  li    TESTNUM, 5
  j     report_fail

report_pass:
  li    t3, 0x20000       // MPRV
  csrc  mstatus, t3       // ⚠️ clear MPRV ⇒ the `tohost` write is never blocked
  RVTEST_PASS

report_fail:
  li    t3, 0x20000       // MPRV
  csrc  mstatus, t3       // ⚠️ 同上 —— **失败路径必须也能报告**
  RVTEST_FAIL

RVTEST_CODE_END

  .data
RVTEST_DATA_BEGIN

RVTEST_DATA_END
