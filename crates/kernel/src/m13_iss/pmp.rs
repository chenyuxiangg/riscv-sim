//! PMP — the platform's physical-memory-protection configuration and the
//! access check (STAGE-21 · v0.37).
//!
//! # Why this module exists
//!
//! PMP has **two** halves that must not drift apart:
//!
//! * the **configuration** — `pmpcfg0..3` and `pmpaddr0..15`, which live in
//!   the **CSR file** (so the state is **not** a second copy: the file *is*
//!   the state, and a snapshot restore brings the configuration back for
//!   free);
//! * the **decision** — one function ([`pmp_allows`]) that every access
//!   path asks.  Three paths use it: the **load**, the **store** and the
//!   **instruction fetch**.
//!
//! ⚠️ The **decoding** itself lives in the **contract layer**
//! ([`pmp_access_allowed`]) ⇒ ⇒ this module only reads the state, hands the
//! entries over, and turns "denied" into an architectural trap.  One
//! constraint, one home.
//!
//! # The reference side is the oracle
//!
//! ⚠️ Every rule implemented here was **measured** against the reference
//! side first (frozen spec §1.2) — the platform is not allowed to guess:
//!
//! * `pmpcfg`'s byte layout is `L` = bit 7, `A` = bits 4:3, `X` = bit 2,
//!   `W` = bit 1, `R` = bit 0 (F1);
//! * `pmpaddr` holds the address **shifted right by two** (F3);
//! * the **lowest-numbered matching** entry decides (F7) — not "any entry
//!   that permits";
//! * `R` = 0 with `W` = 1 is **reserved** ⇒ ⇒ the reference side drops the
//!   `W` bit on write (F20) ⇒ ⇒ such an entry cannot write (F9);
//! * M-mode is exempt unless `MPRV` = 1 (F10) — and `MPRV` does **not**
//!   affect instruction fetch (F11);
//! * a **locked** entry is enforced in **every** privilege mode, and its
//!   registers stop accepting writes (F12 / F13);
//! * an instruction fetch needs the `X` permission (F14) ⇒ ⇒ no `X` ⇒
//!   `mcause = 1`.

use riscv_sim_contract::csr::{
    pmp_access_allowed, pmp_cfg_warl, pmp_decode_cfg, PmpAccess, CSR_MSTATUS, CSR_PMPADDR0,
    CSR_PMPCFG0, MSTATUS_MPRV_BIT, PMP_A_TOR, PMP_CFG_L, PMP_ENTRY_COUNT,
};
use riscv_sim_contract::model::{SimState, WriteSize};

/// How many `pmpcfg` registers hold the 16 entries (four entries each).
const CFG_REGISTERS: usize = PMP_ENTRY_COUNT / 4;

/// The PMP configuration, read out of the CSR file in **one pass**.
///
/// ⚠️ The CSR file is a plain sorted `Vec<(u16, u32)>` ⇒ ⇒ one linear pass
/// collects everything the decision needs ⇒ ⇒ the hot path does **not**
/// rebuild an entry array per access (it does, but only on the slow path —
/// see [`pmp_allows`]).
struct PmpView {
    /// `mstatus` — only `MPRV` is looked at.
    mstatus: u32,
    /// `pmpcfg0..3`, raw.
    cfg: [u32; CFG_REGISTERS],
    /// `pmpaddr0..15`, raw.
    addr: [u32; PMP_ENTRY_COUNT],
}

impl PmpView {
    /// Collect the configuration in one pass over the CSR file.
    fn read(state: &SimState) -> PmpView {
        let mut view = PmpView {
            mstatus: 0,
            cfg: [0; CFG_REGISTERS],
            addr: [0; PMP_ENTRY_COUNT],
        };
        for (number, value) in &state.csrs {
            let number = *number;
            if (CSR_PMPCFG0..CSR_PMPCFG0 + CFG_REGISTERS as u16).contains(&number) {
                view.cfg[(number - CSR_PMPCFG0) as usize] = *value;
            } else if (CSR_PMPADDR0..CSR_PMPADDR0 + PMP_ENTRY_COUNT as u16).contains(&number) {
                view.addr[(number - CSR_PMPADDR0) as usize] = *value;
            } else if number == CSR_MSTATUS {
                view.mstatus = *value;
            }
        }
        view
    }

    /// The configuration byte of entry `index`.
    fn cfg_byte(&self, index: usize) -> u8 {
        (self.cfg[index / 4] >> ((index % 4) * 8)) as u8
    }

    /// Is entry `index` **locked**?
    fn locked(&self, index: usize) -> bool {
        self.cfg_byte(index) & PMP_CFG_L != 0
    }
}

/// The number of bytes one memory access touches.
pub(crate) fn access_bytes(size: WriteSize) -> u32 {
    match size {
        WriteSize::Byte => 1,
        WriteSize::Half => 2,
        WriteSize::Word => 4,
    }
}

/// Does PMP allow this access?
///
/// * `access` — which kind of access it is (`Load` / `Store` / `Fetch`);
///   the required permission bit follows from it.
/// * `size` — the number of bytes the access touches.
///
/// ⚠️ **The hot path is the M-mode one**: an M-mode access with `MPRV` = 0 is
/// **exempt** unless some entry is **locked** ⇒ ⇒ the common case answers
/// with one pass over the CSR file and a couple of mask tests — no entry
/// array is rebuilt (STAGE-21 裁决 I).
///
/// ⚠️ **Instruction fetch never looks at `MPRV`** (F11) ⇒ ⇒ an M-mode fetch
/// is exempt the same way — only a **locked** entry constrains it (F12).
pub(crate) fn pmp_allows(state: &SimState, addr: u32, size: u32, access: PmpAccess) -> bool {
    let view = PmpView::read(state);

    // ---- hot path: no entry is locked ⇒ M-mode is exempt --------------
    // ⚠️ This is the whole answer for an M-mode access: the spec says
    // "if L is cleared, and the access is M-mode, the access succeeds", and
    // "if no PMP entry matches an M-mode effective address, the access
    // succeeds" ⇒ ⇒ **no** entry can deny it.
    let any_locked = (0..PMP_ENTRY_COUNT).any(|index| view.locked(index));
    if !any_locked {
        match access {
            PmpAccess::Fetch => return true,
            PmpAccess::Load | PmpAccess::Store => {
                if view.mstatus & (1u32 << MSTATUS_MPRV_BIT) == 0 {
                    return true;
                }
            }
        }
    }

    // ---- slow path: hand the entries to the contract layer ------------
    let mut entries = [(pmp_decode_cfg(0), 0u32); PMP_ENTRY_COUNT];
    for (index, slot) in entries.iter_mut().enumerate() {
        *slot = (pmp_decode_cfg(view.cfg_byte(index)), view.addr[index]);
    }
    let mprv = view.mstatus & (1u32 << MSTATUS_MPRV_BIT) != 0;
    let m_mode_bypass = match access {
        PmpAccess::Fetch => true,
        PmpAccess::Load | PmpAccess::Store => !mprv,
    };
    pmp_access_allowed(&entries, addr, size, access, m_mode_bypass)
}

/// The write rules of the PMP CSRs — `Some(value)` means "the PMP owns this
/// CSR and this is what gets written".
///
/// ⚠️ **The lock is per *entry*** (measured): writing `pmpcfg0` keeps the
/// bytes of the **locked** entries and writes the **unlocked** ones (F20's
/// counterpart ⇒ §8 订正 6).
///
/// ⚠️ A `pmpcfg` byte is **legalised** on the way in (F20): the reserved
/// bits 6:5 read back as zero, and the reserved `R`=0 ∧ `W`=1 combination
/// loses its `W` bit.
///
/// ⚠️ A `pmpaddr` is stored **verbatim** (F16) — but not when its entry is
/// locked, and not when the **next** entry is a **locked** TOR entry (the
/// spec: a locked TOR entry also freezes `pmpaddr[i-1]`).
pub(crate) fn legalize_write(state: &SimState, csr: u16, value: u32) -> Option<u32> {
    let view = PmpView::read(state);
    if (CSR_PMPCFG0..CSR_PMPCFG0 + CFG_REGISTERS as u16).contains(&csr) {
        let first = (csr - CSR_PMPCFG0) as usize * 4;
        let mut out = 0u32;
        for slot in 0..4 {
            let entry = first + slot;
            let byte = if view.locked(entry) {
                view.cfg_byte(entry)
            } else {
                pmp_cfg_warl((value >> (slot * 8)) as u8)
            };
            out |= u32::from(byte) << (slot * 8);
        }
        return Some(out);
    }
    if (CSR_PMPADDR0..CSR_PMPADDR0 + PMP_ENTRY_COUNT as u16).contains(&csr) {
        let entry = (csr - CSR_PMPADDR0) as usize;
        if view.locked(entry) {
            return Some(view.addr[entry]);
        }
        // The spec: a **locked** entry whose mode is TOR also freezes the
        // **previous** entry's address register.
        if entry + 1 < PMP_ENTRY_COUNT {
            let next = entry + 1;
            if view.locked(next) && pmp_decode_cfg(view.cfg_byte(next)).a == PMP_A_TOR {
                return Some(view.addr[entry]);
            }
        }
        return Some(value);
    }
    None
}
