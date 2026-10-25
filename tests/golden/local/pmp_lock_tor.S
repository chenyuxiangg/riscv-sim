// A golden case: a **locked** (`L` = 1) **TOR** PMP entry **freezes the
// previous entry's `pmpaddr`** — the write is silently dropped and the
// **old** value reads back.
//
// STAGE-21 (v0.37 · PMP) implements this per the RISC-V privileged spec
// (§3.7.6): "if the locked entry's `A` field is set to TOR, the `pmpaddr`
// register of the preceding entry is also frozen".  The platform's
// implementation is `m13_iss/pmp.rs` → `legalize_write`.
//
// ⚠️ **This case is the MEASUREMENT of the reference side.**  A corpus row
// with status `active` is executed on **both** sides (this platform and
// Spike) and the harness compares the two exit codes ⇒ ⇒ the row is the
// experiment: if Spike freezes too, the row is green on both sides ⇒ ⇒ the
// behaviour is pinned by the default gate; if Spike does **not** freeze,
// the row goes **red** ⇒ ⇒ and that is a **finding**, not a defect of this
// case.  ⇒ ⇒ the assertion below states the platform's spec-implemented
// behaviour and is **not** weakened to make a red row green.
//
// ⚠️ **The failure modes carry distinct numbers**, so a red row says *which*
// invariant broke:
//
//   TESTNUM = 1  ->  PASS (both sides agree)
//   TESTNUM = 3  ->  the **frozen** value was **not** frozen: the write to
//                    `pmpaddr2` landed although entry 3 is a locked TOR
//                    entry ⇒ ⇒ the freeze does not happen
//   TESTNUM = 4  ->  the **control**'s write did **not** land: `pmpaddr1`
//                    (whose *next* entry, entry 2, is a TOR entry but
//                    **not** locked) did not take the written value ⇒ ⇒
//                    the write is frozen even though it must not be
//   TESTNUM = 5  ->  the **control group** broke: a `pmpaddr` written while
//                    **nothing** was locked did not read back ⇒ ⇒ the case
//                    is not alive (or the `pmpaddr` path is broken) ⇒ ⇒
//                    every other verdict would be meaningless
//   TESTNUM = 6  ->  the `pmpcfg0` read-back did not match ⇒ ⇒ the lock
//                    never took effect ⇒ ⇒ the frozen scenario was not
//                    actually exercised
//
// ⚠️ **The control is MANDATORY.**  Without it, "the value was frozen" and
// "the write was **ignored**" are the **same observation** ⇒ ⇒ a platform
// whose `pmpaddr` writes are all dropped would be green too ⇒ ⇒ a false
// green.  The control here is entry 2: a **TOR** entry that is **not**
// locked ⇒ ⇒ its predecessor's `pmpaddr` (`pmpaddr1`) must still take a
// written value, **even though a locked entry exists elsewhere**.
//
// ⚠️ **The lock is per entry, and it is permanent.**  Once entry 3 is locked
// its `pmpcfg` byte is read-only ⇒ ⇒ it can never be unlocked ⇒ ⇒ the
// frozen scenario and the control **cannot share an entry** ⇒ ⇒ two
// different entries are used (3 for the freeze, 2 for the control) inside
// the one case.
//
// ⚠️ **The layout** (all `pmpaddr` values are `address >> 2`, so the
// granularity is 4 bytes; the `pmpcfg` byte layout is `L` = bit 7,
// `A` = bits 4:3, `X` = bit 2, `W` = bit 1, `R` = bit 0, and `A`:
// OFF = 0b00, TOR = 0b01, NA4 = 0b10, NAPOT = 0b11 — **RV32's `pmpcfg0`
// holds four entries**, byte 0 = entry 0 … byte 3 = entry 3):
//
//   entry 0: NAPOT, R + W + X, **not** locked  (the "permit all" entry)
//             pmpaddr0 = 0x7fffffff  ⇒ NAPOT over [0x0000_0000, 2^34)
//             pmpcfg0 byte 0 = 0x1f
//   entry 1: OFF
//             pmpcfg0 byte 1 = 0x00
//   entry 2: TOR, **not** locked               (the CONTROL entry)
//             pmpaddr1 = 0x1400_0000  ⇒ range [0x5000_0000, 0x5000_1000)
//             pmpaddr2 = 0x1400_0400  (upper bound — also the frozen one)
//             pmpcfg0 byte 2 = 0x08
//   entry 3: TOR, **locked**                   (the FREEZING entry)
//             pmpaddr3 = 0x1400_0800  ⇒ range [0x5000_1000, 0x5000_2000)
//             pmpcfg0 byte 3 = 0x88  (L = 1 | A = TOR)
//
//   ⇒ ⇒ `pmpcfg0` = **0x8808_001f**.
//
// ⚠️ **Why entry 0 is kept as the permit-all NAPOT** (it is the only
// deviation from the "obvious" minimal layout, and it is deliberate):
//
//   * **The access path must stay permissive on BOTH sides.**  The verdict
//     travels through an `ecall` → trap vector → **`tohost` store**, and the
//     `tohost` word sits at **`0x8000_1000`** (`.text.init` at `0x8000_0000`,
//     `.tohost` page-aligned after it — `tests/golden/env/link.ld`).
//     Entry 0 is the **lowest-numbered** entry and it matches **every**
//     address ⇒ ⇒ it decides every access ⇒ ⇒ it permits it ⇒ ⇒ the
//     verdict can always be written, whatever the other entries say.
//   * **A locked entry must not deny the program.**  Entry 3's range is
//     `[0x5000_1000, 0x5000_2000)` and entry 2's is
//     `[0x5000_0000, 0x5000_1000)` ⇒ ⇒ **neither** covers the code at
//     `0x8000_0000` **nor** the `tohost` word at `0x8000_1000` ⇒ ⇒ the
//     locked range cannot swallow the program or its verdict.  (Both ranges
//     are also far outside the RAM window `0x8000_0000` + 4 MiB, so they
//     cannot alias anything the program touches.)
//   * The case is about **CSR write behaviour**, not about access control ⇒ ⇒
//     the access path is deliberately kept boring, so that the *only* thing
//     the two sides can disagree about is the freeze.
//
// ⚠️ **`mstatus.MPRV` is NOT set.**  This case measures **CSR write
// behaviour**, not access control ⇒ ⇒ with `MPRV` = 0 an M-mode load or
// store bypasses PMP ⇒ ⇒ the verdict write is safe by construction.  (This
// is *not* the model of `local/pmp_deny.S`, which **does** set `MPRV` and
// therefore has to clear it on every verdict path — that case is about the
// access path, this one is not.)
//
// ⚠️ **Order matters.**  `pmpaddr2` is written **before** entry 3 is locked
// ⇒ ⇒ otherwise the *first* write would already be frozen and the case
// would measure nothing.
//
// The file is deliberately **self-contained**: it includes no riscv-tests
// source, so it keeps working if the upstream tree changes shape (same rule
// as `unmapped_store.S` / `pmp_deny.S`).
//
// ⚠️ This case runs under the **`csr`** environment (M mode), like
// `pmp_deny.S` / `mstatus_warl.S` — its verdict travels through the
// environment's `ecall` trap path, and that path is **not** what this case
// is about.
#include "riscv_test.h"

RVTEST_RV32M
RVTEST_CODE_BEGIN

  // ---- the address registers ---------------------------------------------
  // ⚠️ **Before anything is locked** ⇒ ⇒ every one of these writes must
  // land (that is what the control group below checks).
  li    t0, 0x7fffffff          // entry 0: NAPOT, permit-all
  csrw  pmpaddr0, t0
  li    t0, 0x14000000          // entry 2's lower bound (the control's value)
  csrw  pmpaddr1, t0
  li    t0, 0x14000400          // entry 3's lower bound (**the frozen value**)
  csrw  pmpaddr2, t0
  li    t0, 0x14000800          // entry 3's upper bound
  csrw  pmpaddr3, t0

  // ---- the control group: the case is alive ------------------------------
  // ⚠️ A `pmpaddr` written while **nothing** is locked must read back
  // **verbatim** (full 32-bit comparison, not a byte comparison) ⇒ ⇒ if this
  // fails, every other verdict below is meaningless.
  csrr  t1, pmpaddr0
  li    t2, 0x7fffffff
  bne   t1, t2, fail_control_group
  csrr  t1, pmpaddr1
  li    t2, 0x14000000
  bne   t1, t2, fail_control_group
  csrr  t1, pmpaddr2
  li    t2, 0x14000400
  bne   t1, t2, fail_control_group
  csrr  t1, pmpaddr3
  li    t2, 0x14000800
  bne   t1, t2, fail_control_group

  // ---- lock entry 3 (TOR) ------------------------------------------------
  // byte 0 = 0x1f (NAPOT, R + W + X) · byte 1 = 0x00 (OFF)
  // byte 2 = 0x08 (TOR, not locked)     · byte 3 = 0x88 (TOR, **locked**)
  li    t0, 0x8808001f
  csrw  pmpcfg0, t0

  // ⚠️ The read-back proves the **lock actually took effect** ⇒ ⇒ without
  // it, "the frozen value did not change" could just mean "the `pmpcfg`
  // write never landed" ⇒ ⇒ a separate verdict number for that.
  csrr  t1, pmpcfg0
  li    t2, 0x8808001f
  bne   t1, t2, fail_lock

  // ---- the frozen scenario -----------------------------------------------
  // Entry 3 is **locked** and its mode is **TOR** ⇒ ⇒ the spec freezes
  // `pmpaddr2` (the preceding entry's address register) ⇒ ⇒ this write must
  // be **dropped** and `pmpaddr2` must still read back as 0x1400_0400.
  li    t0, 0x14000c00
  csrw  pmpaddr2, t0
  csrr  t1, pmpaddr2
  li    t2, 0x14000400           // the **old** value — the write was frozen
  bne   t1, t2, fail_not_frozen

  // ---- the mandatory control ---------------------------------------------
  // Entry 2 is a TOR entry but it is **not** locked ⇒ ⇒ `pmpaddr1` is
  // **not** frozen ⇒ ⇒ this write must **land**, **even though** a locked
  // entry exists elsewhere in the same register ⇒ ⇒ the freeze is specific
  // to the *preceding* entry of the locked TOR entry, not a blanket rule.
  li    t0, 0x14000200
  csrw  pmpaddr1, t0
  csrr  t1, pmpaddr1
  li    t2, 0x14000200           // the **new** value — the write landed
  bne   t1, t2, fail_control_write

  // ---- everything matched ------------------------------------------------
  li    TESTNUM, 1
  RVTEST_PASS

  // ---- the failure paths -------------------------------------------------
fail_not_frozen:
  li    TESTNUM, 3
  j     report_fail
fail_control_write:
  li    TESTNUM, 4
  j     report_fail
fail_control_group:
  li    TESTNUM, 5
  j     report_fail
fail_lock:
  li    TESTNUM, 6
  j     report_fail

report_fail:
  RVTEST_FAIL

RVTEST_CODE_END

  .data
RVTEST_DATA_BEGIN

RVTEST_DATA_END
