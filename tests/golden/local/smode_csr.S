// A golden case: **S mode** — the S-mode CSRs, the per-level accessibility
// rule, and the delegation routing.
//
// STAGE-22 (v0.38 · S mode) adjudication N: the upstream `rv32si/*` cases
// cover `sbreak` / `scall` / `csr`, but none of them can observe **why** an
// S-mode trap went where it went, and none of them touches the S-mode CSR
// set (the upstream `rv64si/csr.S` only uses `sscratch`).  This case is
// **self-contained** (it includes no riscv-tests source) and it pins:
//
//   1. `sscratch`  — writable, read back verbatim;
//   2. `stvec`    — writable, and its MODE field is a **1-bit WARL**;
//   3. `sepc`     — writable, and its low bit is **always** 0 (IALIGN = 32);
//   4. `scause` / `stval` — writable;
//   5. `sstatus`  — a **bit view** of `mstatus`, writable mask `0x22`;
//   6. reading an **M-mode** CSR in S mode ⇒ **illegal instruction** (cause 2)
//      and the trap is **delegated** ⇒ it lands on `stvec`;
//   7. writing an **M-mode** CSR in S mode ⇒ the same;
//   8. reading an **unimplemented** CSR in S mode ⇒ the same;
//   9. the **delegation bit decides the entry**: with `medeleg`'s bit 2
//      cleared, the very same trap lands on **`mtvec`** instead;
//  10. `sret` with `sstatus.SPP = 0` lands in **U mode** — which is visible
//      because a U-mode `sstatus` read is an illegal instruction.
//
// ⚠️ **The verdict is the guest's own check.**  The harness compares the
// `tohost` exit code, so the guest checks every value itself and only
// reports PASS when all of them are exactly right.
//
// ⚠️ **Every assertion can tell "implemented" from "not implemented".**  A
// platform that does **not** trap on an S-mode access to an M-mode CSR
// would read back 0 and carry on — the case would then have to notice that
// **no trap happened**.  ⇒ ⇒ the trap handlers set a "a trap really
// happened" flag (`s2`) and every trap-shaped check verifies that flag ⇒ ⇒
// "no trap" can never be mistaken for "pass".
//
// ⚠️ **The delegation surface has to be live.**  The `smode` environment
// writes `medeleg` **only if** the test defines an `stvec_handler` (the
// `beqz` that guards the `csrw medeleg` jumps past it) ⇒ ⇒ this case
// defines one ⇒ ⇒ `medeleg` really is programmed.
//
// ⚠️ **Why the case steps into M mode in the middle.**  `medeleg` is an
// **M-mode** CSR ⇒ ⇒ S mode cannot write it ⇒ ⇒ the only way to change the
// delegation mask from inside the test is to be in M mode when the change
// happens.  ⇒ ⇒ the only way *into* M mode from S mode is a trap that is
// **not** delegated ⇒ ⇒ an S-mode `ecall` (cause 9; `medeleg`'s bit 9 is
// clear) ⇒ ⇒ it lands on `mtvec` ⇒ ⇒ `mtvec_handler` runs **in M mode**
// ⇒ ⇒ that is where `medeleg`'s bit 2 is cleared (check 9) and put back.
//
// ⚠️ **The failure modes carry distinct numbers**, so a red row says
// *which* invariant broke.  The table is registered in
// `tests/golden/README.md`:
//
//   TESTNUM = 1  ->  `sscratch` did not read back what was written
//   TESTNUM = 2  ->  `stvec` did not read back what was written / MODE not legalised
//   TESTNUM = 3  ->  `sepc`'s low bit was not masked off
//   TESTNUM = 4  ->  `scause` / `stval` did not read back what was written
//   TESTNUM = 5  ->  `sstatus` is not the `0x22`-masked view of `mstatus`
//   TESTNUM = 6  ->  reading an M-mode CSR in S mode did not trap with cause 2 on `stvec`
//   TESTNUM = 7  ->  writing an M-mode CSR in S mode did not trap with cause 2 on `stvec`
//   TESTNUM = 8  ->  reading an unimplemented CSR in S mode did not trap with cause 2
//   TESTNUM = 9  ->  the delegation bit did not decide the trap entry
//   TESTNUM = 10 ->  `sret` did not land in U mode
#include "riscv_test.h"

RVTEST_RV32S
RVTEST_CODE_BEGIN

  // ---- the bookkeeping registers ---------------------------------------
  //   s0  phase: what the program expects right now
  //       1 = "the trap must be delegated"  (checks 6, 7, 8, 10)
  //       2 = "the trap must NOT be delegated" (check 9)
  //       3 = "step into M mode" (the bootstrap)
  //   s1  the cause the trap handler saw
  //   s2  "a trap really happened" flag
  //   s3  which handler ran (1 = `stvec_handler`, 2 = `mtvec_handler`)
  //   s4  the environment's `stvec` value (saved before check 2 overwrites it)

  li s0, 0
  li s1, 0
  li s2, 0
  li s3, 0

  // =================== check 1 · sscratch ==============================
  li t0, 0x1234
  csrw sscratch, t0
  csrr t1, sscratch
  li t2, 0x1234
  bne t1, t2, fail1
  li t0, 0xdeadbeef
  csrw sscratch, t0
  csrr t1, sscratch
  li t2, 0xdeadbeef
  bne t1, t2, fail1

  // =================== check 2 · stvec =================================
  // ⚠️ The MODE field (bits 1:0) is a **1-bit WARL**: 0 and 1 are legal,
  // 2 and 3 are not.  Writing 2 must read back as 0, writing 3 as 1.
  //
  // ⚠️ The environment's `stvec` value is saved first and restored right
  // after: this check **overwrites the trap entry**, and every later check
  // needs the entry to point at `stvec_handler` again.
  csrr s4, stvec
  li t0, 0x1234
  csrw stvec, t0
  csrr t1, stvec
  li t2, 0x1234
  bne t1, t2, fail2
  li t0, 0x1236
  csrw stvec, t0
  csrr t1, stvec
  li t2, 0x1234
  bne t1, t2, fail2
  li t0, 0x1237
  csrw stvec, t0
  csrr t1, stvec
  li t2, 0x1235
  bne t1, t2, fail2
  csrw stvec, s4

  // =================== check 3 · sepc ==================================
  // ⚠️ IALIGN is 32 bits ⇒ the low bit of `sepc` is always 0.
  li t0, 0x1235
  csrw sepc, t0
  csrr t1, sepc
  li t2, 0x1234
  bne t1, t2, fail3

  // =================== check 4 · scause / stval =======================
  li t0, 0x55
  csrw scause, t0
  csrr t1, scause
  li t2, 0x55
  bne t1, t2, fail4
  li t0, 0x77
  csrw stval, t0
  csrr t1, stval
  li t2, 0x77
  bne t1, t2, fail4

  // =================== check 5 · sstatus ==============================
  // ⚠️ **The reference side's `sstatus` is WIDER than the contract's
  // `SSTATUS_WARL_MASK = 0x22`.**  Measured on the reference side (Spike,
  // `--priv=MSU`), from **both** M mode and S mode:
  //
  //   write 0xFFFFFFFF -> read back **0x800C6122**  (low byte `0x22`)
  //   write 0x100      -> read back **0x00000100**  (`SPP` is **set**)
  //   write 0x22       -> read back **0x00000022**
  //
  // ⇒ ⇒ the low byte of the all-ones write is `0x22` — but `SPP` **is**
  // writable from S mode (which is also what the privileged spec says:
  // `SPP` is writable in S mode).  ⇒ ⇒ the assertions below are the
  // **reference side's measured behaviour**, not the contract's mask ⇒ ⇒
  // a platform whose `sstatus` follows the `0x22` mask alone reports
  // **FAIL 5** here ⇒ ⇒ that divergence is **visible**, not hidden.
  // (The finding is registered in `tests/golden/README.md`.)
  li t0, 0xffffffff
  csrw sstatus, t0
  csrr t1, sstatus
  andi t1, t1, 0xff                // the low byte is SIE | SPIE
  li t2, 0x22
  bne t1, t2, fail5
  li t0, 0x100                     // SPP on its own
  csrw sstatus, t0
  csrr t1, sstatus
  andi t1, t1, SSTATUS_SPP
  beqz t1, fail5                   // the reference side leaves SPP set

  // =================== check 6 · read an M-mode CSR ====================
  // In S mode, `mstatus` is an M-mode CSR ⇒ illegal instruction (cause 2).
  // Cause 2 is delegated ⇒ the trap lands on `stvec` ⇒ `scause == 2`.
  li s0, 1
  li s2, 0
  li s3, 0
  csrr t0, mstatus
  bnez s2, c6_trapped
  j fail6
c6_trapped:
  li t2, 1
  bne s3, t2, fail6                 // it must have reached the S-level handler
  li t2, 2
  bne s1, t2, fail6                 // cause 2 = illegal instruction

  // =================== check 7 · write an M-mode CSR ===================
  li s0, 1
  li s2, 0
  li s3, 0
  li t0, 0
  csrw mstatus, t0
  bnez s2, c7_trapped
  j fail7
c7_trapped:
  li t2, 1
  bne s3, t2, fail7
  li t2, 2
  bne s1, t2, fail7

  // =================== check 8 · an unimplemented CSR ==================
  // ⚠️ `0x0C1` is **not** implemented by this platform (the pseudo CSR
  // `CSR_PRIV` is `0x0C0`; `0x0C1` is one past it) ⇒ ⇒ in S mode it is an
  // illegal instruction, exactly like an M-mode CSR.
  li s0, 1
  li s2, 0
  li s3, 0
  csrr t0, 0x0c1
  bnez s2, c8_trapped
  j fail8
c8_trapped:
  li t2, 1
  bne s3, t2, fail8
  li t2, 2
  bne s1, t2, fail8

  // =================== step into M mode ================================
  // An S-mode `ecall` is cause 9, and `medeleg`'s bit 9 is **clear** ⇒ ⇒
  // the trap is **not** delegated ⇒ ⇒ it lands on `mtvec` ⇒ ⇒ the M-level
  // handler runs in **M mode** ⇒ ⇒ from there `medeleg` can be written.
  li s0, 3
  li s2, 0
  li s3, 0
  ecall
  bnez s2, boot_trapped
  j fail9
boot_trapped:
  li t2, 2
  bne s3, t2, fail9                 // it must have reached the M-level handler
  li t2, 9
  bne s1, t2, fail9                 // cause 9 = supervisor ecall

  // =================== check 9 · the delegation bit ====================
  // `medeleg`'s bit 2 was cleared by the M-level handler above ⇒ ⇒ the
  // very same illegal-instruction trap is **not** delegated any more ⇒ ⇒
  // it must land on **`mtvec`** instead of `stvec`.
  li s0, 2
  li s2, 0
  li s3, 0
  csrr t0, mstatus
  bnez s2, c9_trapped
  j fail9
c9_trapped:
  li t2, 2
  bne s3, t2, fail9                 // it must have reached the M-level handler
  li t2, 2
  bne s1, t2, fail9                 // cause 2 = illegal instruction

  // =================== check 10 · where does `sret` land? ==============
  // `sstatus.SPP` is 0 (the platform clears it when `sret`ing out of a
  // trap) ⇒ ⇒ `sret` must land in **U mode** ⇒ ⇒ reading `sstatus` there
  // is an illegal instruction (cause 2) ⇒ ⇒ delegated ⇒ ⇒ `stvec`.
  li s0, 1
  li s2, 0
  li s3, 0
  la t0, after_sret
  csrw sepc, t0
  sret
  j fail10                          // unreachable: `sret` does not return here
after_sret:
  csrr t0, sstatus                   // <- traps: cause 2, delegated -> stvec
  bnez s2, c10_trapped
  j fail10
c10_trapped:
  li t2, 1
  bne s3, t2, fail10                // it must have reached the S-level handler
  li t2, 2
  bne s1, t2, fail10                // cause 2 = illegal instruction

  // ---- everything matched ------------------------------------------------
  li TESTNUM, 1
  RVTEST_PASS

  // ---- the failure paths -------------------------------------------------
fail1:
  li TESTNUM, 1
  j report_fail
fail2:
  li TESTNUM, 2
  j report_fail
fail3:
  li TESTNUM, 3
  j report_fail
fail4:
  li TESTNUM, 4
  j report_fail
fail5:
  li TESTNUM, 5
  j report_fail
fail6:
  li TESTNUM, 6
  j report_fail
fail7:
  li TESTNUM, 7
  j report_fail
fail8:
  li TESTNUM, 8
  j report_fail
fail9:
  li TESTNUM, 9
  j report_fail
fail10:
  li TESTNUM, 10
  j report_fail

report_fail:
  RVTEST_FAIL

  // ---- the S-level trap handler -----------------------------------------
  // It records what happened and resumes **after** the faulting instruction
  // (which is 4 bytes long — this corpus is assembled without the C
  // extension).
  .align 4
  .globl stvec_handler
stvec_handler:
  csrr s1, scause
  li s2, 1
  li s3, 1
  csrr t0, sepc
  addi t0, t0, 4
  csrw sepc, t0
  sret

  // ---- the M-level trap handler -----------------------------------------
  // ⚠️ It runs in **M mode** — that is the whole point of it: `medeleg`
  // cannot be written from S mode.
  .align 4
  .globl mtvec_handler
mtvec_handler:
  csrr s1, mcause
  li s2, 1
  li s3, 2
  li t2, 3
  beq s0, t2, boot_phase
  li t2, 2
  beq s0, t2, check9_phase
  j fail9
boot_phase:
  li t2, 9
  bne s1, t2, fail9                // the bootstrap trap must be cause 9
  li t0, 4                          // bit 2 = illegal instruction
  csrc medeleg, t0                  // ⇒ cause 2 is no longer delegated
  j mtvec_return
check9_phase:
  li t2, 2
  bne s1, t2, fail9                // the delegation test must be cause 2
  li t0, 4
  csrs medeleg, t0                  // ⇒ put the delegation back
mtvec_return:
  csrr t0, mepc
  addi t0, t0, 4
  csrw mepc, t0
  li t0, MSTATUS_MPP & (MSTATUS_MPP >> 1)   // MPP = S
  csrs mstatus, t0
  // Normalise `sstatus.SPP` (bit 8) so that the next `sret` lands in U
  // mode.  ⚠️ `csrc` only takes a **5-bit** immediate ⇒ read / mask / write.
  csrr t0, mstatus
  li t1, ~SSTATUS_SPP
  and t0, t0, t1
  csrw mstatus, t0
  mret

RVTEST_CODE_END

  .data
RVTEST_DATA_BEGIN

RVTEST_DATA_END
