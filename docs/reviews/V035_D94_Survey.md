# 第 18 批勘察报告 · D-94（S 扩展 / PMP / 计数器）

> **性质**：**勘察批** —— 本批**不改产品代码**，只把 D-94 的分歧**定位到源码行**，产出下一批的切分依据。
> **日期**：2026-10-24
> **分支**：`feature/v035-survey`
> **上游依据**：`docs/reviews/REVIEWS.md` §1.3 的 **D-94**（P1 · 「已登记未做」）+ `tests/golden/README.md` 的「不入库清单」。

---

## 0. 一句话结论

⚠️ **台账与 README 那张「9 例分歧」表已经过期** —— 用**正确的 march / env / ISA** 重跑后，**9 例里只有 5 例是真分歧**，另 3 例**现在两侧都 PASS**（其中 2 例的 PASS 是**空洞的**），1 例**连汇编都过不了**。

⇒ ⇒ **D-94 的体量比台账记的小得多**：真正要做的不是「半个特权级」，而是**三件可独立的事**，其中**一件已经做完了**（只是没入库）。

---

## 1. 怎么跑的（可复现）

⚠️ **关键**：这三个参数**缺一不可**，而台账与 README **都没记**：

| 参数 | 值 | 为什么 |
| --- | --- | --- |
| **march** | `rv32i_zicsr_zifencei` | ⚠️ 用语料里的 `rv32i_zifencei` ⇒ 汇编器直接报 `extension 'zicsr' required` ⇒ **9 例全 BUILD-FAILED**（看起来像「全部分歧」，其实是**一个都没跑起来**） |
| **env 变体** | `csr` | 仓内 `tests/golden/env/csr/riscv_test.h` 才有 `RVTEST_RV32S` 宏（默认 env 没有） |
| **Spike ISA** | `rv32imc_zicsr_zicclsm_zifencei_zicntr` | 默认 `rv32imc_zicclsm_zifencei` **不含 `s` / `zicsr` / `zicntr`** ⇒ 参考侧自己也不支持 S 模式 |

```bash
# 临时语料（**不进仓**）：收录 9 例，全部标 pending
#   ⇒ harness 只容忍「候选侧跑不了」（ERROR / NOVERDICT）
#   ⇒ 任何**真分歧**都会让 run 失败 ⇒ ⇒ 这次的**红**就是「真分歧」的证据
GOLDEN_MANIFEST=/tmp/v035-corpus.txt \
GOLDEN_ISA=rv32imc_zicsr_zicclsm_zifencei_zicntr \
  tools/golden-run.sh --keep-going
```

⚠️ `GOLDEN_MANIFEST` 是 `tools/golden-run.sh` 的**覆盖口**（本批新增）⇒ ⇒ 临时语料可以**放在仓外**、用仓内脚本直接跑 ⇒ ⇒ 不再需要「脚本副本」⇒ ⇒ 复现路径**只有一条**。

⚠️ 临时语料的**逐行内容**见 §8 ⇒ ⇒ 那 9 行**就是**本次勘察的输入。

---

## 2. 实测结果（9 例，逐例）

| 用例 | Spike | 我们 | 结论 | 归因（**已定位到源码行**） |
| --- | --- | --- | --- | --- |
| `rv32mi/illegal` | PASS | **NOVERDICT** | ❌ 真分歧 | `csrwi mip, MIP_SSIP` 被**忽略**（`mip` 写被过滤）⇒ 陷入永远不来 ⇒ 跑不完 |
| `rv32mi/pmpaddr` | PASS | **FAIL 1** | ❌ 真分歧 | ⚠️ **根因不是「没有 PMP」** —— 见 §3 的变异 M1 |
| `rv32mi/zicntr` | PASS | **PASS** | ⚠️ **空洞的 PASS** | 见 §3 |
| `rv32si/csr` | PASS | **FAIL 22** | ❌ 真分歧 | S 模式 ⇒ 差 22 个测试点 |
| `rv32si/ma_fetch` | PASS | **PASS** | ✅ **真实的 PASS** | ⚠️ 例子里 `#ifdef __MACHINE_MODE` 把 `sstatus`/`sret` **宏替换成 M 模式** ⇒ 它**测的不是 S 模式** ⇒ 可入库 |
| `rv32si/sbreak` | PASS | **FAIL 669** | ❌ 真分歧 | S 模式 |
| `rv32si/scall` | PASS | **FAIL 669** | ❌ 真分歧 | S 模式 |
| `rv32si/wfi` | PASS | **PASS** | ⚠️ **空洞的 PASS** | 见 §4 |
| `rv32si/dirty` | — | — | ❌ **连汇编都过不了** | 缺 `SATP_MODE` / `SATP_MODE_SV32`（分页） |

**汇总**：`真分歧 5` · `空洞 PASS 2` · `真实 PASS 1` · `编不过 1`（旧表记的是「9 例全部分歧」）。

---

## 3. 变异 M1 —— `pmpaddr` 的真根因**不是「没有 PMP」**

**变异**：`crates/contract/src/csr.rs` 的 `csr_write_is_ignored()` 去掉 `|| !csr_is_implemented(csr)` 这一条（= 让**未实现**的 CSR 的写**被存下来**，不再忽略）。

**结果**：`rv32mi/pmpaddr` 从 **FAIL 1 → PASS**（其余用例的红集不变）。

⇒ ⇒ **推论**：`pmpaddr` 的分歧**直接源于**「未实现 CSR 的写被忽略、读回 0」这条规则（契约层 `crates/contract/src/csr.rs` 的 `csr_write_is_ignored`，其文档明写这条是 D-92 定的）⇒ ⇒ **不是**因为「平台没有 PMP 寄存器」。

⚠️ **这推翻了 README 那句「the platform has no PMP ⇒ `pmpaddr0` reads back 0」** —— 读回 0 是**规则**的直接后果，不是「寄存器不存在」的直接后果。⇒ ⇒ **两者的修法不同**：
- 若只是想要「读回写进去的值」⇒ 改那条规则（但那会让 guest **凭空造出平台没有的寄存器** ⇒ **与 D-92 的裁决相反** ⇒ **不能这么改**）；
- 真正的修法 = **实现 PMP**（让 `pmpaddr` 落在**已实现**列表里 ⇒ 写不被忽略、读回真值）。

⇒ ⇒ **`pmpaddr` 是「实现 PMP」的判据**，不是「放宽 CSR 规则」的判据。

**还原**：`cp` + `md5` + `cmp` 逐字节校验（md5 回到 `a19fa0a1a1b052c7c55f3ff437daac88`）。

---

## 4. 为什么 `wfi` / `zicntr` 的 PASS 是**空洞的**

**根因链**（每一步都已定位到源码行）：

1. `rv32si/wfi.S` → `rv64si/wfi.S` 展开后是 `csrc sstatus, SSTATUS_SIE` / `csrs sie, SIP_SSIP` / `csrs sip, SIP_SSIP` / `wfi` ⇒ **全是 S 模式 CSR**。
2. 契约层**没有** `sstatus` / `sie` / `sip` ⇒ `csr_is_implemented()` 只列了 11 个 M 模式 CSR（`csr.rs`）⇒ ⇒ 这些写**被忽略**、读回 0（`csr_write_is_ignored()`）。
3. 用例**照样走到 `RVTEST_PASS`** ⇒ ⇒ **PASS 是空洞的**（它证明的是「未实现 CSR 被忽略」这条规则，**不是**「wfi 工作正常」）。

⚠️ **而 `wfi` 之所以能进 S 模式，本该靠 `RVTEST_RV32S` → `RVTEST_ENABLE_SUPERVISOR`** —— 它的展开是：

```
li a0, MSTATUS_MPP & (MSTATUS_MPP >> 1);   # = 0x0800 ⇒ MPP = 01 = S
csrs mstatus, a0;
li a0, SIP_SSIP | SIP_STIP;
csrs mideleg, a0;                            # delegation
```

⇒ ⇒ **但 `mret` 从不读 `MPP`**（⚠️ **实测更正**：**不是** `MPP` 写不进去）：

- `MSTATUS_WARL_MASK = 0x807F_FFEA`（`crates/contract/src/csr.rs:218`）的 bit 11:12 = **1** ⇒ ⇒ **MPP 是可写的**（该掩码的测试 `the_mstatus_warl_mask_drops_the_reserved_bits_and_keeps_the_fields` 也把 `0x0000_1800`（MPP）列为「必须在掩码之内」）；
- ⇒ 真正的落点是 **`Op::Mret`（`crates/kernel/src/m13_iss/executor.rs:1207`）**：它**只**恢复 `MIE` / `MPIE`（`(mstatus & !(mie | mpie)) | restored | mpie`）⇒ ⇒ **从头到尾没有读 `MPP`** ⇒ ⇒ **无论 guest 把 `MPP` 写成什么，`mret` 都回 M 模式**；
- ⇒ （`mstatus.MPP` 在**陷入**时被写成 M 是**另一处**：`crates/kernel/src/m13_iss/csr.rs:263`，那是 `MPP := M` 的陷入语义，**与本条无关**。）

⇒ ⇒ **结论**：`wfi` / `zicntr` 的 PASS **不是**「这些功能可用」的证据 ⇒ ⇒ **不得入库**（台账 README 那句「`rv32si/ma_fetch` 空洞 ⇒ 不提升」**只针对 ma_fetch**，而 `wfi` / `zicntr` 当时是**分歧**、现在变 PASS ⇒ **它们的空洞性是新的、必须一并记账**）。

⚠️ **顺带订正**：`tests/golden/env/csr/riscv_test.h` 的注释说 `RVTEST_ENABLE_SUPERVISOR` 是「为源码兼容保留、已掏空」⇒ ⇒ **这句是过期的** —— 实测它与上游 `env/p/riscv_test.h` **逐字相同**（含 `csrs mstatus` 与 `csrs mideleg`）⇒ ⇒ **它没被掏空** ⇒ ⇒ 真正让 S 模式进不去的是**平台的 MPP 钳制**，不是 env。

---

## 5. 下一批的切分依据（三件可独立的事）

| 事 | 内容 | 判据 | 工作量 | 备注 |
| --- | --- | --- | --- | --- |
| **① 计数器**（Zicntr） | 实现 `cycle` / `time` / `instret` 三个计数器 + `mcycle` / `minstret` 等 | `rv32mi/zicntr` 入库 | **小** | ⚠️ 现状是「无计数器 ⇒ `minstret` 恒读 0 ⇒ 用例**无条件通过**」（README 记的空洞一致）⇒ ⇒ **实现前它测不到任何东西** ⇒ ⇒ **实现后才是真判据** |
| **② PMP** | 实现 PMP 寄存器组 + 访问判定 | `rv32mi/pmpaddr` 入库 | **中** | ⚠️ 变异 M1 证明：**光放宽 CSR 规则就能让它过** ⇒ ⇒ **那不是真修法**（会破坏 D-92 的裁决）⇒ ⇒ 必须**真的实现 PMP** |
| **③ S 模式** | `mstatus.MPP` 支持 S（WARL 放开）· `sstatus` / `sie` / `sip` / `stvec` / `sepc` / `scause` / `satp` 等 · `sret` / `sbreak` / `scall` / `wfi` 的 S 语义 · `mideleg` / `medeleg` delegation | `rv32si/csr` / `sbreak` / `scall` 入库 | **大** | ⚠️ `misa` 要报 **S 位** ⇒ ⇒ `PLATFORM_RULES_VERSION` **不用**动（那是**描述**的规则集，与 ISA 能力无关）⇒ ⇒ 契约层**不升版**（除非动 DTO） |

⚠️ **③ 的前置**：MPP 的 WARL 钳制是**契约层**的事（`MSTATUS_WARL_MASK`）⇒ ⇒ 若要支持 S，**必须**动那个常量 ⇒ ⇒ **它是「一条约束一个落点」的又一次同形**（掩码 + 语义 + `misa` 三处）⇒ ⇒ 下一批做 ③ 时要**一次改齐**。

⚠️ **三件的顺序建议**：**① → ② → ③**（工作量递增、依赖递增）⇒ ⇒ 若一批只做一件 ⇒ 先做 **①**（最小、且能把一个空洞一致变成真判据）。

---

## 6. 勘察批顺带的三件小事（已做 / 待做）

| 事 | 状态 |
| --- | --- |
| **订正台账「M-20 / M-23 仍是 stub」** | ⚠️ **已过期** —— M-20 有 `SnapshotManager` + `capture_session` + `validate_session`（570 行）、M-23 有 `DataStore` trait + `InMemoryDataStore`（255 行）⇒ ⇒ **不是 stub** ⇒ ⇒ **已在本批的台账提交里订正** |
| **D-150 做法 C**（`golden-run.sh --check-mem`） | ✅ **已做**（本批）—— ⚠️ **价值边界如实记**：两侧窗口**构造上同源** ⇒ ⇒ 正常时**必然绿** ⇒ ⇒ 本条抓的是「**构造被破坏**」⇒ ⇒ 治根仍需 **做法 B**（大跨度语料） |
| **`front_test_attrib.py` 并入默认门禁** | ✅ **已做**（本批）—— 挂在 `tools/check-front.sh` 之后 ⇒ ⇒ 归属口径与**历史回归**都进默认门禁；⚠️ 自检在**仓外**建三个 worktree 并**自己收掉** ⇒ ⇒ 门禁不留垃圾 |

---

## 7. 本批**不做**的事

- ❌ **不动产品代码**（勘察批的纪律 ⇒ 变异只用于**取证**、取完即还原）
- ❌ **不把任何用例加进 `corpus.txt`** ⇒ ⇒ 那 5 例真分歧会让 run 失败（`pending` 规则只容忍「跑不了」）⇒ ⇒ **必须先实现再入库**
- ❌ **不写冻结规格**（规格在**实现批**写，勘察结论即其输入）
- ❌ **不实现 PMP / 计数器 / S 模式**（那是下一批的事）
- ❌ **不放宽「未实现 CSR 的写被忽略」那条规则**（那是 D-92 的裁决；变异 M1 证明它能「让 `pmpaddr` 过」，但那**不是**真修法 ⇒ ⇒ **不能这么改**）

---

## 8. 复现命令

⚠️ 临时语料与临时产物**不进仓**（`/tmp`）⇒ ⇒ 仓内 `corpus.txt` 与 `tests/golden/env/` **一字未改**。

```bash
cd /Users/zyzs/lczyzs/code/riscv_sim

# ① 临时语料（**不进仓**）—— 格式：<suite> <test> <march> <status> <env> <reason>
#    ⚠️ 全部标 `pending` ⇒ harness 只容忍「候选侧跑不了」（ERROR / NOVERDICT）
#    ⇒ 任何**真分歧**都会让 run 失败 ⇒ ⇒ 这次的**红**就是「真分歧」的证据
#    ⚠️ `pending` 的理由**必须以台账号开头**（`D-<n> …`）⇒ 否则 harness 拒收
cat > /tmp/v035-corpus.txt <<'EOF'
rv32mi illegal rv32i_zicsr_zifencei pending csr D-94 勘察：S 模式语义
rv32mi pmpaddr rv32i_zicsr_zifencei pending csr D-94 勘察：无 PMP
rv32mi zicntr rv32i_zicsr_zifencei pending csr D-94 勘察：无 Zicntr
rv32si csr rv32i_zicsr_zifencei pending csr D-94 勘察：S 模式
rv32si ma_fetch rv32i_zicsr_zifencei pending csr D-94 勘察：S 模式（空洞的一致）
rv32si sbreak rv32i_zicsr_zifencei pending csr D-94 勘察：S 模式
rv32si scall rv32i_zicsr_zifencei pending csr D-94 勘察：S 模式
rv32si wfi rv32i_zicsr_zifencei pending csr D-94 勘察：S 模式
rv32si dirty rv32i_zicsr_zifencei pending csr D-94 勘察：S 模式（连汇编都过不了）
EOF

# ② 跑（三个参数**缺一不可**）
GOLDEN_MANIFEST=/tmp/v035-corpus.txt \
GOLDEN_ISA=rv32imc_zicsr_zicclsm_zifencei_zicntr \
  tools/golden-run.sh --keep-going
```

⚠️ **预期输出**：**红**（退出码 1）—— 那 5 例**真分歧**（`illegal` / `pmpaddr` / `csr` / `sbreak` / `scall`）会让 run 失败 ⇒ ⇒ **红本身就是证据** ⇒ ⇒ 判据是「红集 == §2 那张表的真分歧那几行」。

⚠️ `dirty` 会 **BUILD-FAILED**（缺 `SATP_MODE` / `SATP_MODE_SV32`）⇒ ⇒ 它**不参与**判决 ⇒ ⇒ 但它**证明**了「这一列不是分歧、而是编不过」。
