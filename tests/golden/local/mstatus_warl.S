// A golden case: `mstatus.MPP` is WARL, and writing the **reserved** value
// 2 does something very specific — the field is **legalised to 0 (U)** and
// the **rest of the same write lands normally**.
//
// STAGE-22 (v0.38 · S mode) adjudication N / §8 correction 9: the platform
// must implement `mstatus_warl` as a **function**, because a mask cannot
// express "this field is legalised, the other bits land".
//
// ⚠️ **The reserved value is `2`, and it lives in bits 12:11.**  `MPP = 2`
// is therefore the write value **`0x1000`** — writing `0x1800` writes
// `MPP = 3`, which is **legal** (`M`), and would therefore prove nothing.
//
// ⚠️ **The reference side's measured behaviour** (Spike, `--priv=MSU`; one
// ELF per observation, the 32-bit value read back **byte by byte** so that
// nothing is truncated):
//
//   * old `MPP` = 0, write `0x1002` (`MPP = 2` reserved, `SIE = 1`)
//     ⇒ read back **`0x0000_0002`**;
//   * old `MPP` = 1, write `0x1002` ⇒ read back **`0x0000_0002`**;
//   * old `MPP` = 3, write `0x1002` ⇒ read back **`0x0000_0002`**;
//
// ⇒ ⇒ **the reserved value is legalised to 0, whatever the old value was**,
// and **`SIE` lands** ⇒ ⇒ "the whole write is discarded" is **wrong** (it
// would lose `SIE` ⇒ the read-back would be `0x0000_0000`).
//
// ⚠️ **The first round's "keeps the old value" was a truncation artefact**:
// the probe reported the value through the HTIF **exit code**, and a POSIX
// exit code is **8 bits** ⇒ ⇒ `0x0000_0002` was seen as `0x02` ⇒ ⇒ and a
// value whose low bits are 0 cannot be told apart from "the old value was
// kept" unless the **old** value is non-zero ⇒ ⇒ the case below sets it to
// 3 first.
//
// ⚠️ **Why the old value matters, and why this case sets it.**  With the old
// value `0`, "legalised to 0" and "kept the old value" are **the same
// observation** ⇒ ⇒ a case that only ever starts from 0 cannot tell them
// apart ⇒ ⇒ the old value is set to **3** (legal, `M`) first ⇒ ⇒ that is
// the observation that discriminates: the reference side reads back
// `0x0000_0002` (legalised), while a "keeps the old value" implementation
// would read back `0x0000_1802`.
//
// ⚠️ **The control group is what makes the case meaningful.**  Without it, a
// platform that **never** changed `MPP` would also read 0 ⇒ ⇒ the case also
// checks that a **legal** `MPP` value (1 = S) *is* taken ⇒ ⇒ the reserved
// value's behaviour is distinguishable from "the field is simply
// read-only".
//
// ⚠️ **The verdict is the guest's own check.**  The harness compares the
// `tohost` exit code, so the guest checks every value itself.
//
// ⚠️ **The failure modes carry distinct numbers**, so a red row says which
// invariant broke:
//
//   TESTNUM = 3  ->  `SIE` was **lost** ⇒ ⇒ the whole write was discarded
//   TESTNUM = 4  ->  the `MPP` field did **not** legalise to 0 (it kept the
//                    old value, or it took the reserved value as written)
//   TESTNUM = 5  ->  the **control group** broke (a legal `MPP` value was
//                    not taken ⇒ ⇒ the field is simply read-only)
//
// The file is deliberately **self-contained**: it includes no riscv-tests
// source, so it keeps working if the upstream tree changes shape (same rule
// as `unmapped_store.S` / `pmp_deny.S`).
//
// ⚠️ This case runs under the **`csr`** environment (M mode), like
// `pmp_deny.S` — its verdict travels through the environment's `ecall`
// trap path, and that path is **not** what this case is about.
#include "riscv_test.h"

RVTEST_RV32M
RVTEST_CODE_BEGIN

  // ---- check 1 · reserved MPP = 2, the old value is 0 --------------------
  // ⇒ ⇒ `SIE` must land, and `MPP` must be 0.
  //
  // ⚠️ The write is a **full** `csrw`, not a `csrs`: `csrs` would OR the
  // bits in, so the `MPP` field would never see the reserved value `0b10`
  // at all ⇒ ⇒ the case would prove nothing.
  csrw mstatus, zero
  li t0, 0x1002                     // MPP = 2 (reserved) | SIE = 1
  csrw mstatus, t0
  csrr a0, mstatus
  andi a1, a0, 0xff                // the low byte
  li t2, 0x02                      // SIE
  bne a1, t2, fail_sie
  srli a1, a0, 11
  andi a1, a1, 0x3                 // the MPP field
  bnez a1, fail_mpp

  // ---- check 2 · reserved MPP = 2, the old value is 3 --------------------
  // ⇒ ⇒ `SIE` must land again, and `MPP` must be **legalised to 0** — this
  // is the observation that separates "legalised" from "kept the old value".
  li t0, MSTATUS_MPP                // MPP = 3 (M) — legal
  csrw mstatus, t0
  li t0, 0x1002                     // MPP = 2 (reserved) | SIE = 1
  csrw mstatus, t0
  csrr a0, mstatus
  andi a1, a0, 0xff
  li t2, 0x02
  bne a1, t2, fail_sie
  srli a1, a0, 11
  andi a1, a1, 0x3
  bnez a1, fail_mpp                 // the reference side legalises to 0

  // ---- check 3 · the control group --------------------------------------
  // A **legal** `MPP` value must be taken ⇒ ⇒ the reserved value's
  // behaviour above is not just "the field is read-only".
  li t0, MSTATUS_MPP & (MSTATUS_MPP >> 1)   // MPP = 1 (S) — legal
  csrw mstatus, t0
  csrr a0, mstatus
  srli a1, a0, 11
  andi a1, a1, 0x3
  li t2, 1
  bne a1, t2, fail_control

  // ---- everything matched ------------------------------------------------
  li TESTNUM, 1
  RVTEST_PASS

  // ---- the failure paths -------------------------------------------------
fail_sie:
  li TESTNUM, 3
  j report_fail
fail_mpp:
  li TESTNUM, 4
  j report_fail
fail_control:
  li TESTNUM, 5
  j report_fail

report_fail:
  RVTEST_FAIL

RVTEST_CODE_END

  .data
RVTEST_DATA_BEGIN

RVTEST_DATA_END
