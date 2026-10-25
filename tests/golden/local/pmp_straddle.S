// A golden case: PMP decides a **misaligned** access **piece by piece**, not
// by one "deciding entry" that has to cover every byte of the access.
//
// ⚠️ **This case asserts the REFERENCE side's behaviour, not ours** (STAGE-23
// §4.3 裁决 L).  Until the platform is changed it is **RED on our side** —
// and that red row **is** the discriminating-power evidence.
//
// ---------------------------------------------------------------------------
// WHAT IT PINS DOWN
// ---------------------------------------------------------------------------
// Ledger D-152 (branch ②) said "an entry must cover **all** bytes of the
// access".  That wording describes the **platform's** old rule — and it was
// **wrong about the reference side**.  STAGE-23 §1.2 measured the reference
// side (Spike) and found a **third** rule: **per piece**, at the 4-byte PMP
// granularity.
//
//   * **F10** — a misaligned 4-byte **load** at `0x8003_FFFE` whose **tail**
//     lands in a **DENY** entry ⇒ the reference side **denies** it
//     (`mcause` = 5) ⇒ ⇒ "only the start address decides" is **falsified**.
//   * **F9**  — a misaligned 4-byte **load** at `0x800F_FFFE` that straddles
//     **two ALLOW** entries ⇒ the reference side **allows** it and the
//     loaded value is the **mixed** one (`0xBBBB_AAAA`)
//     ⇒ ⇒ "one entry must cover the whole access" is **falsified**.
//
// ⇒ ⇒ **Both scenarios are needed.**  F10 alone is also compatible with
// "only the start address decides", and F9 alone is also compatible with
// "the deciding entry must cover everything".  Only the **pair** separates
// the three readings (STAGE-23 F11) — a single case cannot.
//
// The third check is the **control group** ("the case is alive"): an
// **aligned** load and a **misaligned** load that stay **fully inside** the
// ALLOW region must both succeed and return the expected values.  The
// misaligned control is a **real** control, not an assumption: the reference
// side **executes** misaligned accesses (STAGE-23 F8), and so do we (F15).
//
// ---------------------------------------------------------------------------
// THE TWO PMP CONFIGURATIONS
// ---------------------------------------------------------------------------
// ⚠️ The PMP encodings follow the RISC-V privileged spec (§3.7.6), and every
// field was **measured against the reference side first**:
//
//   * `pmpcfg` byte layout: `L` = bit 7, `A` = bits 4:3, `X` = bit 2,
//     `W` = bit 1, `R` = bit 0.  `A`: OFF = 0b00, TOR = 0b01, NA4 = 0b10,
//     NAPOT = 0b11.
//   * `pmpaddr` holds the physical address **shifted right by 2**
//     (address bits 33:2), so the granularity is 4 bytes.
//   * NAPOT: let `n` = the number of trailing ones in `pmpaddr`; the entry
//     covers `[ (pmpaddr & ~((1<<n)-1)) << 2 , + 2^(n+3) )` bytes.
//   * Priority: the **lowest-numbered matching** entry decides a **piece**.
//
// **Configuration A** (scenario A + the control group):
//
//   entry 0: NAPOT, 4 KiB at 0x80040000, **no** permissions  ⇒ DENY
//             pmpaddr0 = 0x200101ff  (0x80040000 >> 2, plus 9 trailing ones
//                                     ⇒ 2^(9+3) = 4096 bytes)
//             pmpcfg0 byte 0 = 0x18 (A = NAPOT, no R/W/X)
//   entry 1: NAPOT, 1 MiB at 0x80000000, R + W + X           ⇒ ALLOW
//             pmpaddr1 = 0x2001ffff  (0x80000000 >> 2, plus 17 trailing ones
//                                     ⇒ 2^(17+3) = 1 MiB)
//             pmpcfg0 byte 1 = 0x1f (A = NAPOT, R + W + X)
//
// **Configuration B** (scenario B):
//
//   entry 0: NAPOT, 1 MiB at 0x80000000, R + W + X           ⇒ ALLOW
//             pmpaddr0 = 0x2001ffff · pmpcfg0 byte 0 = 0x1f
//   entry 1: NAPOT, 1 MiB at 0x80100000, R + W + X           ⇒ ALLOW
//             pmpaddr1 = 0x2005ffff  (0x80100000 >> 2, plus 17 trailing ones)
//             pmpcfg0 byte 1 = 0x1f
//
// ⚠️ **RV32's `pmpcfg0` holds FOUR entries** (byte 0 = entry 0, byte 1 =
// entry 1) ⇒ ⇒ `pmpcfg1` would configure entries 4..7 and is never used
// here.  Both config bytes survive the reference side's WARL legalisation
// unchanged (the reserved bits 6:5 read back as zero — our bytes already
// have them zero — and `R`=0 ∧ `W`=1 never occurs: `0x18` has neither `R`
// nor `W`, `0x1f` has both).
//
// ---------------------------------------------------------------------------
// ⚠️ WHY NO ENTRY MAY COVER THE CODE OR THE VERDICT WORD
// ---------------------------------------------------------------------------
// The verdict travels through a **store** to the `tohost` word, which lives
// at `0x8000_1000` while the code lives at `0x8000_0000` (see
// `tests/golden/env/link.ld`).  A **locked** entry is enforced in **every**
// privilege mode ⇒ ⇒ if a locked entry covered those addresses, the verdict
// write would be denied and the case would **hang** instead of failing.
//
// ⇒ ⇒ **no entry here is locked**, so the verdict path is safe.  Checking
// the regions against the real numbers:
//
//   * configuration A: entry 0 covers `[0x80040000, 0x80041000)` — that is
//     **neither** the code (`0x80000000`) **nor** `tohost` (`0x80001000`).
//     Entry 0 is the **lower-numbered** entry, so it decides every access
//     inside `[0x80040000, 0x80041000)` — and the code and the verdict word
//     are **outside** that window ⇒ ⇒ they are decided by entry 1, which
//     **allows** them ⇒ ⇒ the verdict path works.
//   * configuration B: entry 0 (the lowest-numbered matching entry) covers
//     `[0x80000000, 0x80100000)` ⇒ ⇒ it decides both the code and the
//     verdict word, and it **allows** them ⇒ ⇒ the verdict path works.
//
// ⚠️ And the **MPRV** rule is what makes the verdict path safe anyway: every
// verdict path **clears `MPRV`** first, so the `tohost` store runs at
// **M-mode** privilege ⇒ ⇒ PMP does not apply to it at all (no entry is
// locked).  The same pattern is used by `local/pmp_deny.S`.
//
// ---------------------------------------------------------------------------
// ⚠️ WHY `MPRV` **AND** `MPP` ARE SET
// ---------------------------------------------------------------------------
// M-mode loads/stores bypass PMP unless `mstatus.MPRV` = 1; and with
// `MPRV` = 1 the data accesses run at the privilege held in `mstatus.MPP`
// ⇒ ⇒ `MPP` must be **0 (U-mode)** for PMP to bite at all.  (The platform's
// own decision function ignores `MPP`, but the **reference side** honours
// it ⇒ ⇒ setting `MPP` = 0 is what makes the two sides comparable at all.)
//
// ⚠️ `MPP` = 0 also means an `mret` would drop the hart into **U-mode** —
// which is why the trap handler here **does not `mret`**: it simply carries
// on with the next scenario.  The hart therefore stays in M-mode for the
// whole case, and `MPRV` = 1 / `MPP` = 0 keeps every data access
// PMP-checked.
//
// ---------------------------------------------------------------------------
// ⚠️ `mtval` **IS** ASSERTED NOW — and it asserts the **failing piece**
// ---------------------------------------------------------------------------
// The reference side reports `mtval` = `0x8004_0000` — the address of the
// **failing piece** — while the platform used to report `mtval` = the
// **effective address** of the access (`0x8003_FFFE`).  The two differ by one
// offset, and the two sides' `mcause` are **the same** (5) ⇒ ⇒ `mcause`
// alone **cannot** tell them apart.
//
// ⇒ ⇒ the case now asserts `mtval` = **`0x8004_0000`**, which is what the
// reference side reports (measured; ledger **D-154**, closed in v0.41).
//
// ⚠️ **What exactly is asserted**: the access covers
// `[0x8003_FFFE, 0x8004_0002)` ⇒ ⇒ piece 1 = `[0x8003_FFFE, 0x8004_0000)`
// (inside the ALLOW entry) ⇒ ⇒ piece 2 = `[0x8004_0000, 0x8004_0002)`
// (inside the DENY entry) ⇒ ⇒ **the failing piece is the second one** ⇒ ⇒
// its **start** is `0x8004_0000` ⇒ ⇒ **not** the effective address
// `0x8003_FFFE`.
//
// ⚠️ **The `mcause` and `mepc` assertions stay**: `mcause` = 5 is the primary
// criterion ("is the verdict right?" — denied, and denied as a **load access
// fault**, not a misaligned-load trap (cause 4) and not a bus error), `mepc`
// pins "is it the *right* trap?".  `mtval` is the third, separate question:
// "is the reported address **exact**?".
//
// ⚠️ **The control group stays**: a load that stays **fully inside** the
// ALLOW region does **not** trap at all ⇒ ⇒ `mtval` has nothing to say there
// ⇒ ⇒ it **cannot** be used as the contrast for this assertion (the contrast
// is the access's own **effective address**, asserted in the trap handler).
//
// ---------------------------------------------------------------------------
// ⚠️ THE FAILURE NUMBERS (so a red row says *which* scenario broke)
// ---------------------------------------------------------------------------
//   TESTNUM = 1  ->  PASS (the reference side's exact behaviour)
//   TESTNUM = 3  ->  scenario A: the DENY-tail load was **NOT denied**
//                    (the platform is too **loose** — STAGE-23 F12, first)
//   TESTNUM = 4  ->  control: the **aligned** load inside the ALLOW region
//                    did not return the expected value
//   TESTNUM = 5  ->  a trap happened but `mcause` was **not** 5
//                    (wrong *kind* of denial)
//   TESTNUM = 6  ->  scenario A: `mcause` was 5 but `mepc` was **not** the
//                    faulting instruction (the trap came from elsewhere)
//   TESTNUM = 7  ->  scenario B: the cross-boundary load was **DENIED**
//                    (the platform is too **strict** — STAGE-23 F12, second)
//   TESTNUM = 8  ->  scenario B: the load was allowed but the value was
//                    **not** the mixed one ⇒ ⇒ it did not cross the boundary
//   TESTNUM = 9  ->  control: the **misaligned** load inside the ALLOW
//                    region did not return the expected value
//   TESTNUM = 10 ->  `mtval` was **not** the start of the failing piece
//                    (the reference side reports `0x8004_0000` — the start of
//                    the **second** piece — not the access's effective
//                    address `0x8003_FFFE`)
//
// The file is deliberately **self-contained**: it includes no riscv-tests
// source, so it keeps working if the upstream tree changes shape (same rule
// as `unmapped_store.S` / `pmp_deny.S`).
#include "riscv_test.h"

RVTEST_RV32M
RVTEST_CODE_BEGIN

  // =====================================================================
  // 0. the control group — "the case is alive"
  // =====================================================================
  // Configuration A (see the header): entry 0 = DENY
  // `[0x80040000, 0x80041000)`, entry 1 = ALLOW `[0x80000000, 0x80100000)`.
  li    t0, 0x200101ff          // entry 0: NAPOT, 4 KiB at 0x80040000
  csrw  pmpaddr0, t0
  li    t0, 0x2001ffff          // entry 1: NAPOT, 1 MiB at 0x80000000
  csrw  pmpaddr1, t0
  li    t0, 0x1f18              // entry 0 = 0x18 (NAPOT, no permissions)
                                // entry 1 = 0x1f (NAPOT, R + W + X)
  csrw  pmpcfg0, t0

  // ---- make the data accesses subject to PMP ---------------------------
  li    t0, 1
  slli  t0, t0, 17              // mstatus.MPRV
  csrs  mstatus, t0
  li    t0, 0x1800              // mstatus.MPP
  csrc  mstatus, t0             // ⇒ MPP = 0 (U-mode) ⇒ PMP really applies

  // ---- control 1: an ALIGNED 4-byte load fully inside the ALLOW region --
  // 0x80002000 is inside entry 1 and outside entry 0 ⇒ ⇒ it must succeed.
  // Without this control the case would also pass if the platform denied
  // *every* access, which is not what PMP means.
  li    t1, 0x80002000
  li    t3, 0x5a5a5a5a
  sw    t3, 0(t1)               // permitted write (entry 1 has W)
  lw    t2, 0(t1)               // permitted read  (entry 1 has R)
  li    t3, 0x5a5a5a5a
  bne   t2, t3, control_aligned_failed

  // ---- control 2: a MISALIGNED 4-byte load fully inside the ALLOW region
  // ⇒ the reference side **executes** misaligned accesses (STAGE-23 F8) and
  // so do we (F15) ⇒ ⇒ this is a measured control, not an assumption.
  addi  t1, t1, 2               // 0x80002002
  li    t3, 0xaa
  sb    t3, 0(t1)
  sb    t3, 1(t1)
  li    t3, 0xbb
  sb    t3, 2(t1)
  sb    t3, 3(t1)
  li    t3, 0xbbbbaaaa          // little-endian ⇒ 0xAA 0xAA 0xBB 0xBB
  lw    t2, 0(t1)               // the misaligned load
  bne   t2, t3, control_misaligned_failed

  // =====================================================================
  // 1. scenario A — the TAIL lands in a DENY entry ⇒ must be DENIED
  // =====================================================================
  // The access: a misaligned 4-byte load at 0x8003FFFE ⇒ it covers
  // [0x8003FFFE, 0x80040002) ⇒ the low 2 bytes are inside entry 1 (ALLOW)
  // and the high 2 bytes are inside entry 0 (DENY).
  //
  // ⚠️ The **start address** 0x8003FFFE is inside entry 1 ⇒ ⇒ the "only
  // the start address decides" reading says ALLOW, and the "one entry must
  // cover everything" reading says ALLOW as well (entry 1 covers the whole
  // access) ⇒ ⇒ **only** the per-piece reading says DENY.
  //
  // ⇒ ⇒ the reference side denies it with `mcause` = 5 (STAGE-23 F10).
  li    s0, 5                   // the expected mcause (load access fault)
  la    s2, faulting_load_a     // the expected mepc (the faulting instruction)
  li    s5, 1                   // phase = 1 (scenario A)

  li    t1, 0x8003fffe
faulting_load_a:
  lw    t2, 0(t1)               // <- the misaligned load whose tail is denied
  j     not_denied              // (unreachable: the trap diverts first)

not_denied:
  li    TESTNUM, 3
  j     report_fail

control_aligned_failed:
  li    TESTNUM, 4
  j     report_fail

control_misaligned_failed:
  li    TESTNUM, 9
  j     report_fail

mixed_value_failed:
  li    TESTNUM, 8
  j     report_fail

report_pass:
  li    t3, 0x20000            // MPRV
  csrc  mstatus, t3            // ⚠️ clear MPRV ⇒ the `tohost` write is never blocked
  RVTEST_PASS

report_fail:
  li    t3, 0x20000            // MPRV
  csrc  mstatus, t3            // ⚠️ 同上 —— **the failure path must be able to report**
  RVTEST_FAIL

  .align 2
mtvec_handler:
  // ⚠️ The environment's trap vector hands every **non-ECALL** exception
  // here ⇒ ⇒ a load access fault (cause 5) reaches this handler instead of
  // the generic `other_exception` (which would report the useless 1337).
  csrr  t0, mcause
  csrr  t1, mepc
  li    t2, 1
  beq   s5, t2, trap_in_scenario_a

  // ---- phase 2 (scenario B): a trap here means the platform denied the
  //      cross-boundary load ⇒ ⇒ that is the "too strict" divergence.
  li    t2, 5
  bne   t0, t2, wrong_cause
  li    TESTNUM, 7
  j     report_fail

trap_in_scenario_a:
  li    t2, 5
  bne   t0, t2, wrong_cause      // mcause must be 5 (load access fault)
  bne   t1, s2, wrong_mepc       // mepc must be the faulting instruction
  // ⚠️ **`mtval` 报的是「出错那一片的起始地址」**（台账 D-154 · 参考侧实测）
  //    ⇒ ⇒ 访问覆盖 [0x8003FFFE, 0x80040002) ⇒ ⇒ 片 1 = [0x8003FFFE,
  //    0x80040000)（ALLOW 条目）⇒ ⇒ 片 2 = [0x80040000, 0x80040002)（DENY
  //    条目）⇒ ⇒ **失败片是第二片** ⇒ ⇒ 参考侧报 **`0x8004_0000`** ⇒ ⇒
  //    **不是**访问的有效地址（`0x8003_FFFE`）。
  csrr  t3, mtval
  li    t2, 0x80040000
  bne   t3, t2, wrong_mtval
  // ⇒ ⇒ the denial is exactly what the reference side reports ⇒ ⇒ carry on
  //      with scenario B.  ⚠️ **No `mret`**: `MPP` = 0 would drop the hart
  //      into U-mode, and the case never needs to return to `mepc`.
  j     scenario_b_setup

wrong_cause:
  li    TESTNUM, 5
  j     report_fail

wrong_mepc:
  li    TESTNUM, 6
  j     report_fail

wrong_mtval:
  li    TESTNUM, 10
  j     report_fail

scenario_b_setup:
  // =====================================================================
  // 2. scenario B — across TWO ALLOW entries ⇒ must be ALLOWED
  // =====================================================================
  // Configuration B: entry 0 = ALLOW `[0x80000000, 0x80100000)`,
  //                 entry 1 = ALLOW `[0x80100000, 0x80200000)`.
  //
  // ⚠️ **`pmpcfg0` is written FIRST**, on purpose: that way **no**
  // intermediate state ever has a DENY entry covering the code or the
  // `tohost` word ⇒ ⇒ the verdict path can never be blocked.
  li    t0, 0x1f1f              // entry 0 = entry 1 = 0x1f (NAPOT, R + W + X)
  csrw  pmpcfg0, t0
  li    t0, 0x2001ffff          // entry 0: NAPOT, 1 MiB at 0x80000000
  csrw  pmpaddr0, t0
  li    t0, 0x2005ffff          // entry 1: NAPOT, 1 MiB at 0x80100000
  csrw  pmpaddr1, t0

  li    s5, 2                   // phase = 2 (scenario B)

  // ---- pre-fill the two halves with DIFFERENT bytes -------------------
  // ⇒ ⇒ the mixed value is visible ⇒ ⇒ "no trap" alone would **not** prove
  // that the load really crossed the boundary.
  li    t1, 0x800ffffe
  li    t3, 0xaa
  sb    t3, 0(t1)               // low  half — inside entry 0
  sb    t3, 1(t1)
  li    t3, 0xbb
  sb    t3, 2(t1)               // high half — inside entry 1 (0x80100000)
  sb    t3, 3(t1)

  li    t3, 0xbbbbaaaa          // little-endian ⇒ 0xAA 0xAA 0xBB 0xBB
  lw    t2, 0(t1)               // <- the misaligned load across two ALLOW entries
  bne   t2, t3, mixed_value_failed
  j     report_pass

RVTEST_CODE_END

  .data
RVTEST_DATA_BEGIN

RVTEST_DATA_END
