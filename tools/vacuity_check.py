#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""「**空洞的一致**」的**机器可检**探测器（台账 **D-96** · v0.41 批 · STAGE-26 §4.3）。

台账 **D-96** 的现状（原文）：

    **三条「空洞的一致」被当成 `active` 覆盖** ⇒ ⇒ 已在 `tests/golden/README.md`
    的「`active` rows that agree **vacuously**」小节**逐条标注**（覆盖率不再被
    高估）⇒ ⇒ **留账的是「自动检出」**。

⇒ ⇒ 本脚本就是那个「自动检出」：**比对两侧的执行量**，差异超阈值即报
「空洞一致」。

## 它**能**查什么（裁决 L）

**集合关系** —— 两侧的**提交指令数**与**陷阱数**是不是**同量级**。

## 它**不能**查什么（裁决 L · 验收 C-2）

**「每条断言是否忠实于参考侧语义」** ⇒ ⇒ ⛔ **不许**把「探测器绿」当成
「语料有判别力」⇒ ⇒ **每一次运行都会打印这条边界声明**（见
`BOUNDARY_STATEMENT`）。

## 判据（逐字 · 阈值**写死** · 裁决 M）

一行被报成「**空洞一致**」当且仅当**同时**满足：

1. **两侧的判决逐字相同**（`PASS` / `PASS`、`FAIL:3` / `FAIL:3` …）
   —— 「空洞」的前提是「**一致**」⇒ ⇒ 两侧**分歧**的行**不归本工具管**
   （分歧是**看得见**的，黄金门禁已经盯着它）；
2. **指令口径**：参考侧在判决前**提交 ≥ `MIN_REFERENCE_COMMITS`（100）** 条指令，
   **且** 平台侧**退休**的指令数 **≤ 参考侧 / `HOLLOW_RATIO`（10）**
   ⇒ ⇒ 「**差一个数量级**」；
3. **陷阱口径**（**或**）：参考侧在判决前**陷入 ≥ `MIN_REFERENCE_TRAPS`（10）** 次，
   **且** 平台侧陷入的次数 **≤ 参考侧 / `HOLLOW_RATIO`（10）**。

⚠️ 三个阈值都是**写死的常量**（见本文件顶部的常量区）⇒ ⇒ 判据能**逐字复述**
⇒ ⇒ ⛔ 不是「差异大就报」。

⚠️ 「差一个数量级」的方向是**双向**的：两侧只要差一个数量级 ⇒ ⇒ 两侧
**跑的不是同一个程序** ⇒ ⇒ 「一致」就没有意义。

## 两侧的**数据源**与**解析口径**

### 参考侧（Spike）

`spike --isa=<ISA> -m<mem> -l --log-commits <elf>` ⇒ ⇒ 提交日志落在
**stderr**（实测：stdout 是空的）⇒ ⇒ 本脚本把 **stdout + stderr** 一起收。

提交日志的行型（实测）：

| 行型 | 形状 | 含义 |
|------|------|------|
| 提交行 | `core   0: 3 0x800005cc (0x0000006f) [x5 0x…] [mem 0x… 0x…]` | **一条提交一行** ⇒ ⇒ 行数 = 提交数 |
| 反汇编行 | `core   0: 0x800005cc (0x0000006f) j       pc + 0x0` | 指令的反汇编（**不是**提交） |
| 陷阱行 | `core   0: exception trap_illegal_instruction, epc 0x800000e4` | 一次陷入 |
| 判决写 | 提交行尾部的 `mem 0x<tohost> 0x00000001` | 判决写进 `tohost`（HTIF 的「退出请求」编码 = **bit 0 置位**） |

⚠️ **阈值按「提交数」定，不是按「日志行数」定**（STAGE-26 §10 边界 7）⇒ ⇒
本脚本数的是**提交行**（不是日志行数）⇒ ⇒ 有压缩指令时两者**不等**。

⚠️ **提交数在「判决写」处截断**：黄金 env 的 `RVTEST_PASS` 末尾是 `j .`
（死循环）⇒ ⇒ 参考侧在 HTIF 注意到判决**之前**会把那条自旋指令**再提交几千次**
⇒ ⇒ 不截断的话提交数会被自旋**淹没**（实测：`rv32ui/add` 全文 5000 条提交，
判决处只有 470 条）⇒ ⇒ **截断点是「第一次写 `tohost` 且值的 bit 0 置位」的那条提交行**。

### 平台侧（`riscv-golden`）

`riscv-golden <elf> --budget <n>` ⇒ ⇒ stdout 的第一行：

    GOLDEN-STATS retired=<n> traps=<n> tohost=0x<addr>

* `retired` = **退休**（= 提交）的指令数 ⇒ ⇒ 陷入的指令**不退休**（STAGE-20 裁决 C）
  ⇒ ⇒ 与参考侧的**提交数**同一口径；
* `traps` = 陷入次数 ⇒ ⇒ 与参考侧的 `exception trap_*` 行同一口径；
* `tohost` = `tohost` 字的地址 ⇒ ⇒ 参考侧日志里**判决写**落在哪个地址，
  本脚本据此**截断**参考侧日志 ⇒ ⇒ **平台侧是那个地址的来源**（M-11 查符号，
  平台里**只有那一个**符号读取者 ⇒ ⇒ 本脚本不自己写第二个）。

### 语料清单（`tests/golden/corpus.txt`）的**解析口径**

格式（与 `tools/golden-run.sh` **逐字一致**）：

    <suite> <test> <march> <status> [env] [reason]

* **空行**与**以 `# 开头的行**跳过（`#` 必须在**第 0 列**）；
* 其余行按**空白**切分：`suite` / `test` / `march` / `status` / `env` / `reason…`
  （`reason` **可以含空格** ⇒ ⇒ 前 5 个字段按位置取，其余归 `reason`）；
* `status` ∈ {`active`, `pending`}，`env` ∈ {空, `-`, `csr`, `smode`}
  ⇒ ⇒ 别的值是**硬错误**（与 `golden-run.sh` 同口径）⇒ ⇒ **不静默**；
* `suite` = `local` 时源码取 `tests/golden/local/<test>.S`，其余取
  `<riscv-tests>/isa/<suite>/<test>.S`。

## 用法

    python3 tools/vacuity_check.py                       # 跑真语料（默认清单）
    python3 tools/vacuity_check.py --filter rv32mi        # 只跑一部分
    python3 tools/vacuity_check.py --fixture <path>      # 跑一份「测量表」夹具
    python3 tools/vacuity_check.py --self-test           # 用仓内夹具自测
    python3 tools/vacuity_check.py --help

## 退出码

| 码 | 含义 |
|----|------|
| **0** | 没有报出空洞一致（工具本身没出错） |
| **1** | **报了空洞一致**（至少一行） |
| **2** | **工具自身出错**（参数错 / 工具链缺失 / 清单坏 / 某一行测不了） |

⚠️ **两种失败必须能分开** ⇒ ⇒ 「报出问题」是 **1**，「工具坏了」是 **2**。

## 夹具（验收 C-4）

`tools/fixtures/vacuity/` 下有两份**测量表**（不是清单）：

| 文件 | 内容 | 期望 |
|------|------|------|
| `good.txt` | 两侧提交数**同量级** | **0** 条发现 |
| `bad.txt` | 一行**两侧提交数差一个数量级**的假语料 | **≥ 1** 条发现 |

`--self-test` 把两份都跑一遍 ⇒ ⇒ **好夹具必须 0 红、坏夹具必须红**
⇒ ⇒ 证明这个探测器**不是恒绿**。

⚠️ **本脚本只读仓内文件** ⇒ ⇒ **不改**语料清单一个字节。
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = REPO_ROOT / "tests" / "golden" / "corpus.txt"
FIXTURE_DIR = REPO_ROOT / "tools" / "fixtures" / "vacuity"

# ---------------------------------------------------------------------------
# 阈值（**写死** ⇒ ⇒ 判据能逐字复述 —— 裁决 M）
# ---------------------------------------------------------------------------

#: 阈值 ①：参考侧在判决前**至少**提交这么多条指令，指令口径的判据才适用。
#:
#: ⚠️ 为什么要有这个**下界**：一条只提交了 3 条指令的用例，两侧差一个数量级
#: ⇒ ⇒ 差的那 2 条指令**说明不了任何事** ⇒ ⇒ 判据必须有一个「这件事大到
#: 值得报」的门槛。
MIN_REFERENCE_COMMITS = 100

#: 阈值 ②：参考侧在判决前**至少**陷入这么多次，陷阱口径的判据才适用。
#:
#: ⚠️ 同上的道理：参考侧陷入 3 次而平台侧 0 次 ⇒ ⇒ 那 3 次**说明不了**
#: 「平台侧跳过了整段程序」⇒ ⇒ 门槛写死。
MIN_REFERENCE_TRAPS = 10

#: 阈值 ③：「**差一个数量级**」的倍数（STAGE-26 §4.3 的措辞）。
#:
#: ⚠️ 判据是 `平台侧 * HOLLOW_RATIO <= 参考侧`（**双向**）⇒ ⇒ 逐字可复述：
#: 「平台侧的提交数 ≤ 参考侧的十分之一 ⇒ ⇒ 报空洞一致」。
HOLLOW_RATIO = 10

#: 两种失败的口径（⇒ ⇒ 失败清单里能看出是哪一条红的）。
KIND_HOLLOW_INSTRUCTIONS = "空洞一致（指令口径）"
KIND_HOLLOW_TRAPS = "空洞一致（陷阱口径）"

#: 退出码。
EXIT_OK = 0
EXIT_HOLLOW = 1
EXIT_TOOL_ERROR = 2

#: **判别力边界声明**（裁决 L · 验收 C-2）—— **每一次运行都打印**。
BOUNDARY_STATEMENT = (
    "BOUNDARY: this probe compares SET RELATIONS only — the two sides' committed\n"
    "          instruction counts and trap counts.  It CANNOT tell whether each\n"
    "          assertion faithfully mirrors the reference side's semantics, so a\n"
    "          green run does NOT mean the corpus has discriminating power.\n"
    "          （本工具**不能**判定每条断言是否忠实于参考侧语义 ⇒ ⇒ 「绿」**不等于**\n"
    "          「语料有判别力」。）"
)

#: 参考侧的**提交行**（`core 0: <prv> <pc> (<enc>)`）⇒ ⇒ 一行 = 一次提交。
#:
#: ⚠️ 反汇编行（`core 0: <pc> (<enc>) <disasm>`）**没有**那个特权级字段
#: ⇒ ⇒ 本正则**不会**把它算成提交 ⇒ ⇒ 提交数 ≠ 日志行数（STAGE-26 §10 边界 7）。
COMMIT_RE = re.compile(r"^core\s+\d+:\s+[0-3]\s+0x[0-9a-fA-F]+\s+\(0x[0-9a-fA-F]+\)")

#: 参考侧的**陷阱行**。
TRAP_RE = re.compile(r"^core\s+\d+:\s+exception\s+(\S+)")

#: 提交行尾部的**访存标注**：地址 + 值（只有**写**才有值 ⇒ ⇒ 读不匹配）。
STORE_RE = re.compile(r"\bmem 0x([0-9a-fA-F]+) 0x([0-9a-fA-F]+)\b")

#: 平台侧的统计行。
STATS_RE = re.compile(
    r"^GOLDEN-STATS retired=(\d+) traps=(\d+) tohost=0x([0-9a-fA-F]+)\s*$", re.M
)

#: 平台侧的判决行。
VERDICT_RE = re.compile(r"^VERDICT (.+)$", re.M)


class ToolError(Exception):
    """工具自身出错（⇒ ⇒ 退出码 2，**不是**「报出了问题」）。"""


# ---------------------------------------------------------------------------
# 数据形状
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CorpusRow:
    """清单里的一行。"""

    suite: str
    test: str
    march: str
    status: str
    env: str
    reason: str

    @property
    def case(self) -> str:
        return f"{self.suite}/{self.test}"


@dataclass(frozen=True)
class Measurement:
    """一侧跑完之后，两侧的**可比量**。"""

    case: str
    ref_commits: int
    ref_traps: int
    cand_retired: int
    cand_traps: int
    ref_verdict: str
    cand_verdict: str


@dataclass(frozen=True)
class Finding:
    """一条「空洞一致」。"""

    kind: str
    case: str
    detail: str


@dataclass(frozen=True)
class Environment:
    """跑两侧要用的外部件。"""

    spike: str
    gcc: Path
    tests_dir: Path
    runner: Path
    isa: str
    mem: str
    budget: int
    timeout: int
    out_dir: Path


# ---------------------------------------------------------------------------
# 清单解析（口径写在文件头的 docstring 里）
# ---------------------------------------------------------------------------


def parse_manifest(path: Path) -> list[CorpusRow]:
    """解析语料清单 ⇒ ⇒ 与 `tools/golden-run.sh` **同口径**。"""
    rows: list[CorpusRow] = []
    for lineno, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        if line == "" or line.startswith("#"):
            continue
        fields = line.split()
        if len(fields) < 4:
            raise ToolError(f"{path}:{lineno}: malformed manifest line: {line}")
        suite, test, march, status = fields[0], fields[1], fields[2], fields[3]
        env_col = fields[4] if len(fields) > 4 else ""
        reason = " ".join(fields[5:]) if len(fields) > 5 else ""
        if status not in ("active", "pending"):
            raise ToolError(f"{path}:{lineno}: unknown status '{status}'")
        if env_col in ("", "-"):
            env = ""
        elif env_col in ("csr", "smode"):
            env = env_col
        else:
            raise ToolError(f"{path}:{lineno}: unknown env variant '{env_col}'")
        rows.append(
            CorpusRow(
                suite=suite,
                test=test,
                march=march,
                status=status,
                env=env,
                reason=reason,
            )
        )
    if not rows:
        raise ToolError(f"{path}: the manifest has no rows")
    return rows


# ---------------------------------------------------------------------------
# 两侧的观测：解析
# ---------------------------------------------------------------------------


def parse_candidate_output(stdout: str) -> tuple[str, int, int, int]:
    """解析平台侧的输出 ⇒ ⇒ `(判决, 退休数, 陷入数, tohost 地址)`。"""
    stats = STATS_RE.search(stdout)
    if stats is None:
        raise ToolError("the candidate printed no GOLDEN-STATS line")
    verdict = VERDICT_RE.search(stdout)
    if verdict is None:
        raise ToolError("the candidate printed no VERDICT line")
    return (
        normalise_verdict(verdict.group(1)),
        int(stats.group(1)),
        int(stats.group(2)),
        int(stats.group(3), 16),
    )


def parse_spike_log(log: str, tohost: int) -> tuple[int, int]:
    """解析参考侧的提交日志 ⇒ ⇒ `(判决前的提交数, 判决前的陷入数)`。

    ⚠️ **提交数在「判决写」处截断**（见文件头 docstring）⇒ ⇒ 没有判决写 ⇒ ⇒
    抛 `ToolError`（⇒ ⇒ 「这一行测不了」**不静默**）。
    """
    commits = 0
    traps = 0
    for line in log.splitlines():
        if TRAP_RE.match(line):
            traps += 1
            continue
        if not COMMIT_RE.match(line):
            continue
        commits += 1
        store = STORE_RE.search(line)
        if store is None:
            continue
        address = int(store.group(1), 16)
        value = int(store.group(2), 16)
        # ⚠️ HTIF 的「退出请求」编码 = **bit 0 置位**（`fesvr/device.h`：
        # `payload & 1 -> the hart asks to exit`）。
        if address == tohost and value & 1:
            return commits, traps
    raise ToolError(
        f"the reference log has no verdict write to the tohost word "
        f"(0x{tohost:08x}) — the reference side never reached a verdict"
    )


def normalise_verdict(raw: str) -> str:
    """把判决归一成一个**可逐字比较**的记号。

    * `PASS` ⇒ `PASS`
    * `FAIL 3` ⇒ `FAIL:3`（失败编号也**参与**比较 ⇒ ⇒ 「同一个编号」才算一致）
    * `NOVERDICT` ⇒ `NOVERDICT`
    * `ERROR <msg>` ⇒ `ERROR`（**消息不参与**比较 ⇒ ⇒ 两边的错误文本本就不同）
    """
    parts = raw.strip().split(None, 1)
    kind = parts[0]
    detail = parts[1] if len(parts) > 1 else ""
    if kind == "FAIL" and detail:
        return f"FAIL:{detail}"
    return kind


# ---------------------------------------------------------------------------
# 判据
# ---------------------------------------------------------------------------


def judge(measurement: Measurement) -> list[Finding]:
    """按**写死的阈值**判一行 ⇒ ⇒ 返回它触发的发现。"""
    findings: list[Finding] = []
    # ⚠️ 「空洞」的前提是「**一致**」⇒ ⇒ 两侧分歧的行**不归本工具管**。
    if measurement.ref_verdict != measurement.cand_verdict:
        return findings
    if (
        measurement.ref_commits >= MIN_REFERENCE_COMMITS
        and measurement.cand_retired * HOLLOW_RATIO <= measurement.ref_commits
    ):
        findings.append(
            Finding(
                KIND_HOLLOW_INSTRUCTIONS,
                measurement.case,
                f"reference committed {measurement.ref_commits} instructions "
                f"before its verdict, the platform retired only "
                f"{measurement.cand_retired} "
                f"(<= {measurement.ref_commits} / {HOLLOW_RATIO})",
            )
        )
    if (
        measurement.ref_traps >= MIN_REFERENCE_TRAPS
        and measurement.cand_traps * HOLLOW_RATIO <= measurement.ref_traps
    ):
        findings.append(
            Finding(
                KIND_HOLLOW_TRAPS,
                measurement.case,
                f"reference took {measurement.ref_traps} traps before its verdict, "
                f"the platform took only {measurement.cand_traps} "
                f"(<= {measurement.ref_traps} / {HOLLOW_RATIO})",
            )
        )
    return findings


# ---------------------------------------------------------------------------
# 测量表夹具（`--fixture` / `--self-test`）
# ---------------------------------------------------------------------------


def parse_measurement_file(path: Path) -> list[Measurement]:
    """解析一份**测量表**。

    格式（逗号分隔，**表头是注释行**）：

        case,ref_commits,ref_traps,cand_retired,cand_traps,ref_verdict,cand_verdict
    """
    rows: list[Measurement] = []
    for lineno, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        if line.strip() == "" or line.lstrip().startswith("#"):
            continue
        fields = [f.strip() for f in line.split(",")]
        if len(fields) != 7:
            raise ToolError(
                f"{path}:{lineno}: a measurement row needs 7 comma-separated fields, "
                f"got {len(fields)}: {line}"
            )
        case, ref_commits, ref_traps, cand_retired, cand_traps, ref_v, cand_v = fields
        try:
            rows.append(
                Measurement(
                    case=case,
                    ref_commits=int(ref_commits),
                    ref_traps=int(ref_traps),
                    cand_retired=int(cand_retired),
                    cand_traps=int(cand_traps),
                    ref_verdict=normalise_verdict(ref_v),
                    cand_verdict=normalise_verdict(cand_v),
                )
            )
        except ValueError as err:
            raise ToolError(f"{path}:{lineno}: bad measurement row: {line} ({err})") from err
    if not rows:
        raise ToolError(f"{path}: the measurement file has no rows")
    return rows


# ---------------------------------------------------------------------------
# 跑真语料
# ---------------------------------------------------------------------------


def resolve_environment(args: argparse.Namespace) -> Environment:
    """把外部件找齐 ⇒ ⇒ 缺什么就**报错**（⇒ ⇒ 退出码 2），不静默。"""
    spike = args.spike or os.environ.get("SPIKE") or shutil.which("spike")
    if not spike or not Path(spike).exists():
        raise ToolError("spike not found (set $SPIKE or pass --spike)")
    toolchain = Path(
        os.environ.get("RISCV_TOOLCHAIN_DIR", "~/riscv-sim-golden/toolchain")
    ).expanduser()
    gcc = toolchain / "bin" / "riscv-none-elf-gcc"
    if not gcc.exists():
        raise ToolError(f"toolchain not found: {gcc} (set $RISCV_TOOLCHAIN_DIR)")
    tests_dir = Path(
        os.environ.get("RISCV_TESTS_DIR", "~/riscv-sim-golden/riscv-tests")
    ).expanduser()
    if not (tests_dir / "isa").is_dir():
        raise ToolError(f"riscv-tests not found: {tests_dir} (set $RISCV_TESTS_DIR)")
    target_dir = Path(
        os.environ.get("CARGO_TARGET_DIR", str(REPO_ROOT / "target"))
    ).expanduser()
    runner = target_dir / "debug" / "riscv-golden"
    if not runner.exists():
        if not args.build:
            raise ToolError(
                f"the candidate runner is missing: {runner}\n"
                f"  build it with: cargo build -p riscv_sim_kernel --bin riscv-golden"
                f"   (or pass --build)"
            )
        print("==> building the candidate runner", flush=True)
        subprocess.run(
            ["cargo", "build", "-p", "riscv_sim_kernel", "--bin", "riscv-golden"],
            cwd=REPO_ROOT,
            check=True,
        )
    isa = args.isa or os.environ.get(
        "GOLDEN_ISA", "rv32imc_zicsr_zicclsm_zifencei_zicntr"
    )
    budget = args.budget
    timeout = args.timeout
    # ⚠️ **RAM 窗口取自平台描述**（候选侧打印）⇒ ⇒ 两侧**同源**
    # （STAGE-19 裁决 I 的同一形状）⇒ ⇒ 参考侧的 `-m` 与候选侧映射的窗口
    # **不可能**漂开。
    proc = subprocess.run(
        [str(runner), "--print-memory"], capture_output=True, text=True, timeout=timeout
    )
    window = proc.stdout.strip()
    if not re.fullmatch(r"[0-9]+:[0-9]+", window):
        raise ToolError(f"the candidate printed an unusable RAM window: '{window}'")
    # ⚠️ Spike 的**初始栈页**（参考侧特性）—— 与 `tools/golden-run.sh` 同一份。
    mem = f"2147479552:4096,{window}"
    out_dir = Path(args.out_dir) if args.out_dir else REPO_ROOT / "tests" / "golden" / "_out"
    out_dir.mkdir(parents=True, exist_ok=True)
    return Environment(
        spike=spike,
        gcc=gcc,
        tests_dir=tests_dir,
        runner=runner,
        isa=isa,
        mem=mem,
        budget=budget,
        timeout=timeout,
        out_dir=out_dir,
    )


def source_path(row: CorpusRow, env: Environment) -> Path:
    """清单里的一行 ⇒ ⇒ 源码文件。"""
    if row.suite == "local":
        return REPO_ROOT / "tests" / "golden" / "local" / f"{row.test}.S"
    return env.tests_dir / "isa" / row.suite / f"{row.test}.S"


def build_case(row: CorpusRow, source: Path, elf: Path, env: Environment) -> None:
    """按 `tools/golden-run.sh` 的**同一份**命令行编译一个用例。"""
    env_dir = REPO_ROOT / "tests" / "golden" / "env"
    if row.env:
        env_dir = env_dir / row.env
    cmd = [
        str(env.gcc),
        f"-march={row.march}",
        "-mabi=ilp32",
        "-static",
        "-mcmodel=medany",
        "-fvisibility=hidden",
        "-nostdlib",
        "-nostartfiles",
        f"-I{env_dir}",
        f"-I{REPO_ROOT / 'tests' / 'golden' / 'env'}",
        f"-I{env.tests_dir / 'isa' / 'macros' / 'scalar'}",
        f"-T{REPO_ROOT / 'tests' / 'golden' / 'env' / 'link.ld'}",
        str(source),
        "-o",
        str(elf),
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        raise ToolError(f"build failed: {proc.stderr.strip()[:400]}")


def run_reference(elf: Path, env: Environment) -> tuple[str, str]:
    """跑参考侧 ⇒ ⇒ `(判决, 提交日志)`。"""
    cmd = [
        env.spike,
        f"--isa={env.isa}",
        f"-m{env.mem}",
        "-l",
        "--log-commits",
        str(elf),
    ]
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=env.timeout
        )
    except subprocess.TimeoutExpired:
        return "NOVERDICT", ""
    log = proc.stdout + proc.stderr
    # ⚠️ Spike 的判决在**退出码**里（与 `tools/golden-run.sh` 同一口径）：
    #   0 ⇒ PASS；非 0 ⇒ `FAIL <rc>`；被超时杀掉 ⇒ NOVERDICT。
    if proc.returncode == 0:
        verdict = "PASS"
    elif proc.returncode in (124, 142):
        verdict = "NOVERDICT"
    else:
        verdict = f"FAIL {proc.returncode}"
    return verdict, log


def run_candidate(elf: Path, env: Environment) -> tuple[str, int, int, int]:
    """跑平台侧 ⇒ ⇒ `(判决, 退休数, 陷入数, tohost 地址)`。"""
    cmd = [str(env.runner), str(elf), "--budget", str(env.budget)]
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=env.timeout
        )
    except subprocess.TimeoutExpired:
        raise ToolError("the candidate timed out") from None
    return parse_candidate_output(proc.stdout)


def measure_case(row: CorpusRow, env: Environment) -> Measurement:
    """跑一侧 ⇒ ⇒ 两侧的**可比量**。"""
    source = source_path(row, env)
    if not source.exists():
        raise ToolError(f"source missing: {source}")
    elf = env.out_dir / f"{row.suite}-{row.test}.elf"
    build_case(row, source, elf, env)
    # ⚠️ **平台侧先跑** ⇒ ⇒ 它给出 `tohost` 的地址 ⇒ ⇒ 参考侧的日志据此截断。
    cand_verdict, cand_retired, cand_traps, tohost = run_candidate(elf, env)
    ref_verdict, log = run_reference(elf, env)
    ref_commits, ref_traps = parse_spike_log(log, tohost)
    return Measurement(
        case=row.case,
        ref_commits=ref_commits,
        ref_traps=ref_traps,
        cand_retired=cand_retired,
        cand_traps=cand_traps,
        ref_verdict=normalise_verdict(ref_verdict),
        cand_verdict=cand_verdict,
    )


# ---------------------------------------------------------------------------
# 报告
# ---------------------------------------------------------------------------


def print_criterion() -> None:
    """把**写死的阈值**打印出来 ⇒ ⇒ 判据逐字可复述（裁决 M）。"""
    print("criterion (hard-coded):")
    print(
        "  a row is a HOLLOW AGREEMENT iff the two sides reached the SAME verdict and"
    )
    print(
        f"    ( ref_commits >= {MIN_REFERENCE_COMMITS} and cand_retired * "
        f"{HOLLOW_RATIO} <= ref_commits )     # instruction count"
    )
    print(
        f" or ( ref_traps   >= {MIN_REFERENCE_TRAPS} and cand_traps   * "
        f"{HOLLOW_RATIO} <= ref_traps   )     # trap count"
    )
    print()


def print_table(measurements: list[Measurement]) -> None:
    """逐行打印两侧的可比量。"""
    header = (
        f"{'CASE':<32} {'REF-CMT':>8} {'REF-TRP':>8} {'CAND-RET':>9} "
        f"{'CAND-TRP':>9} {'VERDICTS':<20} RESULT"
    )
    print(header)
    print("-" * len(header))
    for m in measurements:
        same = "same" if m.ref_verdict == m.cand_verdict else "DIFFERENT"
        verdicts = f"{m.ref_verdict}/{m.cand_verdict} {same}"
        print(
            f"{m.case:<32} {m.ref_commits:>8} {m.ref_traps:>8} {m.cand_retired:>9} "
            f"{m.cand_traps:>9} {verdicts:<20}"
        )


def report(measurements: list[Measurement], findings: list[Finding]) -> None:
    """打印报告（表格 + 失败清单 + 边界声明）。"""
    print_table(measurements)
    print()
    if findings:
        print(f"findings: {len(findings)}")
        for finding in findings:
            print(f"  - [{finding.kind}] {finding.case}: {finding.detail}")
    else:
        print("findings: 0")
    print()
    print(BOUNDARY_STATEMENT)


# ---------------------------------------------------------------------------
# 三种模式
# ---------------------------------------------------------------------------


def check_measurements(
    measurements: list[Measurement], source: str
) -> tuple[int, list[Finding]]:
    """判一批测量 ⇒ ⇒ `(空洞条数, 发现清单)`。"""
    findings: list[Finding] = []
    for measurement in measurements:
        findings.extend(judge(measurement))
    print(f"source: {source}")
    print_criterion()
    report(measurements, findings)
    return len(findings), findings


def check_fixture(path: Path) -> int:
    """`--fixture` 模式 ⇒ ⇒ 判一份测量表。"""
    measurements = parse_measurement_file(path)
    count, _ = check_measurements(measurements, str(path))
    if count:
        print(f"\nRESULT: {count} hollow agreement(s) reported")
        return EXIT_HOLLOW
    print("\nRESULT: no hollow agreement reported")
    return EXIT_OK


def check_manifest(path: Path, args: argparse.Namespace) -> int:
    """跑真语料 ⇒ ⇒ 两侧都跑一遍。"""
    rows = parse_manifest(path)
    if args.filter:
        pattern = re.compile(args.filter)
        rows = [row for row in rows if pattern.search(row.case)]
    if args.limit is not None:
        rows = rows[: args.limit]
    if not rows:
        raise ToolError("no rows selected")
    env = resolve_environment(args)
    print(f"==> manifest: {path}  ({len(rows)} rows)")
    print(f"==> reference: {env.spike} --isa={env.isa} -l --log-commits")
    print(f"==> candidate: {env.runner}")
    print()
    measurements: list[Measurement] = []
    problems: list[str] = []
    for row in rows:
        try:
            measurements.append(measure_case(row, env))
        except ToolError as err:
            problems.append(f"{row.case}: {err}")
    count, findings = check_measurements(measurements, str(path))
    if problems:
        print()
        print(f"not measured: {len(problems)}")
        for problem in problems:
            print(f"  - {problem}")
    print()
    print(
        f"summary: rows={len(rows)} measured={len(measurements)} "
        f"hollow={count} not-measured={len(problems)}"
    )
    if problems:
        # ⚠️ 「测不了」是**工具自身**的问题 ⇒ ⇒ 退出码 2（与「报出问题」分开）。
        return EXIT_TOOL_ERROR
    if count:
        print(f"RESULT: {count} hollow agreement(s) reported")
        return EXIT_HOLLOW
    print("RESULT: no hollow agreement reported")
    return EXIT_OK


def self_test() -> int:
    """用**仓内夹具**自测 ⇒ ⇒ 证明这个探测器**不是恒绿**（验收 C-4 / X-4）。"""
    good_path = FIXTURE_DIR / "good.txt"
    bad_path = FIXTURE_DIR / "bad.txt"
    problems: list[str] = []

    for path in (good_path, bad_path):
        if not path.exists():
            print(f"夹具不存在：{path}", file=sys.stderr)
            return EXIT_TOOL_ERROR

    print("== 好夹具（期望 0 红）==")
    good = parse_measurement_file(good_path)
    good_count, good_findings = check_measurements(good, str(good_path))
    if good_count != 0:
        problems.append(
            f"好夹具应当 0 红，却红了 {good_count} 条："
            + " / ".join(f.kind for f in good_findings)
        )

    print()
    print("== 坏夹具（期望红）==")
    bad = parse_measurement_file(bad_path)
    bad_count, bad_findings = check_measurements(bad, str(bad_path))
    if bad_count == 0:
        problems.append("坏夹具应当红，却 0 红 ⇒ ⇒ 探测器恒绿 ⇒ ⇒ 判别力为零")
    if BAD_EXPECTED_KINDS != sorted(f.kind for f in bad_findings):
        problems.append(
            "坏夹具的红集与期望不符："
            f"期望 {BAD_EXPECTED_KINDS}，实际 {sorted(f.kind for f in bad_findings)}"
        )

    if problems:
        print("\nFAIL: 自测不通过", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return EXIT_HOLLOW
    print("\nPASS: 探测器自测全过（好夹具 0 红，坏夹具红）")
    return EXIT_OK


#: 坏夹具的**期望红集**（⇒ ⇒ 两种口径各红一次 ⇒ ⇒ 两条判据都被夹具钉住）。
BAD_EXPECTED_KINDS = sorted([KIND_HOLLOW_INSTRUCTIONS, KIND_HOLLOW_TRAPS])


# ---------------------------------------------------------------------------
# 入口
# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "「空洞的一致」的机器可检探测器（台账 D-96 · STAGE-26 §4.3）。"
            " 比较两侧的提交指令数与陷入数；差异超阈值即报「空洞一致」。"
        ),
        epilog=(
            "退出码：0 = 没报出问题；1 = 报了空洞一致；2 = 工具自身出错。\n"
            "\n"
            "判别力边界（裁决 L）：\n"
            "  本工具**不能**判定每条断言是否忠实于参考侧语义 ⇒ ⇒ 「绿」**不等于**\n"
            "  「语料有判别力」。它只比对**集合关系**（两侧的提交指令数与陷入数）。"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--self-test",
        action="store_true",
        help="用仓内夹具自测（tools/fixtures/vacuity/**）",
    )
    mode.add_argument(
        "--fixture",
        metavar="PATH",
        help="判一份「测量表」（逗号分隔），不跑任何一侧",
    )
    parser.add_argument(
        "--manifest",
        metavar="PATH",
        default=str(DEFAULT_MANIFEST),
        help=f"语料清单（默认 {DEFAULT_MANIFEST}）",
    )
    parser.add_argument("--filter", metavar="REGEX", help="只跑匹配的行")
    parser.add_argument("--limit", type=int, metavar="N", help="只跑前 N 行")
    parser.add_argument("--spike", metavar="PATH", help="参考侧的可执行文件")
    parser.add_argument("--isa", metavar="STR", help="参考侧的 ISA 串")
    parser.add_argument(
        "--budget", type=int, default=1_000_000, help="平台侧的指令预算"
    )
    parser.add_argument(
        "--timeout", type=int, default=60, help="每个用例的墙钟上限（秒）"
    )
    parser.add_argument("--out-dir", metavar="DIR", help="ELF 的落地目录")
    parser.add_argument(
        "--build",
        action="store_true",
        help="候选侧二进制不在时先 cargo build",
    )
    args = parser.parse_args(argv)

    try:
        if args.self_test:
            return self_test()
        if args.fixture:
            path = Path(args.fixture)
            if not path.is_absolute():
                path = (Path.cwd() / path).resolve()
            return check_fixture(path)
        path = Path(args.manifest)
        if not path.is_absolute():
            path = (Path.cwd() / path).resolve()
        return check_manifest(path, args)
    except ToolError as err:
        print(f"vacuity_check: {err}", file=sys.stderr)
        return EXIT_TOOL_ERROR
    except subprocess.CalledProcessError as err:
        print(f"vacuity_check: a subprocess failed: {err}", file=sys.stderr)
        return EXIT_TOOL_ERROR


if __name__ == "__main__":
    sys.exit(main())
