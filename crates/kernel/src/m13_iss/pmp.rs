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
//! * an access is judged **piece by piece** (v0.39 实测) — a piece is the
//!   intersection of the access with one four-byte block ⇒ ⇒ **each piece**
//!   finds **its own** deciding entry ⇒ ⇒ the access is permitted **iff
//!   every piece** is ⇒ ⇒ the platform's **old** rule ("the deciding entry
//!   must cover **every** byte of the access") **diverged from the
//!   reference side in both directions** and is gone;
//! * `R` = 0 with `W` = 1 is **reserved** ⇒ ⇒ the reference side drops the
//!   `W` bit on write (F20) ⇒ ⇒ such an entry cannot write (F9);
//! * the **effective privilege level** of an access is
//!   "current privilege == M **and** `MPRV`" ? `MPP` : the current privilege
//!   level, and **M level is exempt** unless the deciding entry is **locked**
//!   — `MPRV` does **not** affect instruction fetch (F11);
//!   ⚠️ the platform used to **collapse** that into "only look at `MPRV`"
//!   (ledger **D-153**) ⇒ ⇒ that is **wrong in both directions**: too strict
//!   when `MPP` = M, and too lax in S mode ⇒ ⇒ ⚠️ a **security-direction
//!   hole** ⇒ ⇒ the formula now lives in the **contract layer**
//!   ([`pmp_effective_privilege`]) ⇒ ⇒ one constraint, one home;
//! * the `MPRV` branch is **gated** on "the current privilege is M" ⇒ ⇒ the
//!   state "current = S / U **and** `MPRV` = 1 **and** `MPP` = M" must **not**
//!   lift the effective privilege to M ⇒ ⇒ guest code cannot reach that state
//!   (entering S mode requires `MPP` = S and `mret` does not change `MPP`),
//!   but a **snapshot restore** can ⇒ ⇒ without the gate that is ⚠️ a
//!   **security-direction hole**;
//! * a **locked** entry is enforced in **every** privilege mode, and its
//!   registers stop accepting writes (F12 / F13);
//! * an instruction fetch needs the `X` permission (F14) ⇒ ⇒ no `X` ⇒
//!   `mcause = 1`.

use riscv_sim_contract::csr::{
    pmp_access_allowed, pmp_cfg_warl, pmp_decode_cfg, pmp_effective_privilege, pmp_exempt,
    PmpAccess, CSR_MSTATUS, CSR_PMPADDR0, CSR_PMPCFG0, CSR_PRIV, PMP_A_TOR, PMP_CFG_L,
    PMP_ENTRY_COUNT,
};
use riscv_sim_contract::model::{SimState, WriteSize};

use super::csr::priv_from_raw;

/// How many `pmpcfg` registers hold the 16 entries (four entries each).
const CFG_REGISTERS: usize = PMP_ENTRY_COUNT / 4;

/// The PMP configuration, read out of the CSR file in **one pass**.
///
/// ⚠️ The CSR file is a plain sorted `Vec<(u16, u32)>` ⇒ ⇒ one linear pass
/// collects everything the decision needs ⇒ ⇒ the hot path does **not**
/// rebuild an entry array per access (it does, but only on the slow path —
/// see [`pmp_allows`]).
struct PmpView {
    /// `mstatus` —— only `MPRV` and `MPP` are looked at.
    mstatus: u32,
    /// The **current privilege level** —— the pseudo CSR `CSR_PRIV`'s **raw**
    /// entry (`None` ⇒ the file has no such entry ⇒ ⇒ reset ⇒ machine level).
    r#priv: Option<u32>,
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
            r#priv: None,
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
            } else if number == CSR_PRIV {
                // ⚠️ **当前特权级在同一次扫描里一起收上来** ⇒ ⇒ 热路径**不**多扫
                // 一遍 CSR 文件 ⇒ ⇒ 「文件里没有这一条 ⇒ 复位即机器态」那条
                // 规则**仍然只有一个落点**（`csr::priv_from_raw`）。
                view.r#priv = Some(*value);
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
/// ⚠️ **`size` is the size of the WHOLE access** ⇒ ⇒ the **split** into
/// pieces happens in the **contract layer** ([`pmp_access_allowed`]) ⇒ ⇒
/// this function does **not** need to know anything about alignment ⇒ ⇒
/// one landing: the *rule* lives in the contract layer, this function only
/// reads the state and hands the entries over.
///
/// ⚠️ **The hot path is the M-mode one**: an access whose **effective
/// privilege level** is M is **exempt** unless some entry is **locked** ⇒ ⇒
/// the common case answers with one pass over the CSR file and a couple of
/// mask tests — no entry array is rebuilt (STAGE-21 裁决 I).
///
/// ⚠️ **That hot path is still exact under the per-piece rule**: with no
/// locked entry, **every** piece is either decided by an unlocked entry
/// (⇒ M-mode exempt ⇒ allowed) or matches no entry at all (⇒ M-mode ⇒
/// allowed) ⇒ ⇒ every piece is allowed ⇒ ⇒ the whole access is allowed.
///
/// ⚠️ **Instruction fetch never looks at `MPRV`** (F11) ⇒ ⇒ a fetch's
/// effective privilege level is **the current one** ⇒ ⇒ an M-mode fetch is
/// exempt the same way — only a **locked** entry constrains it (F12).
///
/// ⚠️ **口径变更（v0.40 · D-153）**：**有效特权级** = （当前特权级 == M **且**
/// `MPRV`）? `MPP` : 当前特权级（取指**永远**用当前特权级 ⇒ ⇒ 实测 F11）⇒ ⇒
/// 公式**在契约层** ([`pmp_effective_privilege`]) ⇒ ⇒ 豁免规则也**在契约层**
/// ([`pmp_exempt`]) ⇒ ⇒ 本函数**只**读状态、算有效特权级、把手里的条目交出去
/// ⇒ ⇒ 一个约束一个落点.
///
/// ⚠️ **S / U 模式下的访存与取指都要过 PMP**（**实测 F5 / F6 / F7 / F8**）⇒ ⇒
/// 这就是那条**安全方向**的漏洞被修掉的地方 ⇒ ⇒ 参考侧在 S 模式 + `MPRV` = 0
/// 时**施加** PMP（落在无权限条目里 ⇒ ⇒ 拒，`mcause` 5）· S 模式的取指落在无
/// `X` 权限的条目里 ⇒ ⇒ 拒（`mcause` 1）
pub(crate) fn pmp_allows(state: &SimState, addr: u32, size: u32, access: PmpAccess) -> bool {
    let view = PmpView::read(state);

    // ---- 有效特权级 ----------------------------------------------------
    // ⚠️ **公式在契约层** ⇒ ⇒ 有效特权级 = （当前特权级 == M **且** `MPRV`）?
    // `MPP` : 当前特权级（取指**永远**用当前特权级 ⇒ ⇒ 实测 F11）。
    //
    // ⚠️ **那个门不能省**：当前级是 S / U 时**不许**把有效特权级抬到 M ⇒ ⇒
    // 那个状态**能从快照恢复到达** ⇒ ⇒ 不带门就是**安全方向的洞**。
    //
    // ⚠️ **当前特权级住在 CSR 文件的伪 CSR 里** ⇒ ⇒ 它**在同一次扫描**里被收
    // 上来（见 [`PmpView::read`]）⇒ ⇒ 热路径**不**多扫一遍 CSR 文件。
    let current = priv_from_raw(view.r#priv);
    let effective = pmp_effective_privilege(view.mstatus, current, access);

    // ---- hot path: 有效特权级是 M 且没有条目被锁 ⇒ 豁免 ----------------
    // ⚠️ This is the whole answer for an M-mode access: the spec says
    // "if L is cleared, and the access is M-mode, the access succeeds", and
    // "if no PMP entry matches an M-mode effective address, the access
    // succeeds" ⇒ ⇒ **no** entry can deny it.
    //
    // ⚠️ **口径变更（v0.40 · D-153）**：这一支**以前**只看 `MPRV` ⇒ ⇒
    // **S 模式**的访存也被它放行 ⇒ ⇒ ⚠️ **安全方向的漏洞** ⇒ ⇒ 现在它看
    // **有效特权级** ⇒ ⇒ S 模式的访存**不再**走这条路。
    let any_locked = (0..PMP_ENTRY_COUNT).any(|index| view.locked(index));
    if pmp_exempt(effective, any_locked) {
        return true;
    }

    // ---- slow path: hand the entries to the contract layer ------------
    let mut entries = [(pmp_decode_cfg(0), 0u32); PMP_ENTRY_COUNT];
    for (index, slot) in entries.iter_mut().enumerate() {
        *slot = (pmp_decode_cfg(view.cfg_byte(index)), view.addr[index]);
    }
    pmp_access_allowed(&entries, addr, size, access, effective)
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
