#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""台账 §1.3 的**口径自检**（D-144 · v0.33 · STAGE-18 §4.2 · 裁决 M / N）。

台账 `docs/reviews/REVIEWS.md` §1.3 的**末列在两批行里含义不同**：

* **历史行**（编号 ≤ **D-123**）：末列 = **处置**（「✅ **已闭环**（v0.XX，commit `xxx`）：…」）
* **标记区**（编号 ≥ **D-124**）：处置写进「**问题**」列、**开头带标记**
  （`✅ **已闭环**` / `⏳ **未闭环**`）、末列留空

⇒ ⇒ 机器统计「已闭环条数」**会得两个数** ⇒ ⇒ 本脚本把两个口径
**各自有名字地**分开报，并**断言**它们不再分叉。

## 用法

    python3 tools/ledger_check.py --ledger docs/reviews/REVIEWS.md   # 检查真台账（整合批跑）
    python3 tools/ledger_check.py --self-test                          # 用仓内夹具自测（Lane C 的对照体）

## 三条断言（裁决 N）

1. **标记区**（D-124 ~）每行**必须**带标记，且标记 ∈ {`✅ **已闭环**`, `⏳ **未闭环**`}
2. **历史行**（D-15 ~ D-123）每行**必须不带**标记
3. **末列非空**的行**必须不带**标记

⚠️ **分界线是写死的两个编号**（D-123 / D-124）⇒ ⇒ **不是**用正则去猜
「哪批是老行、哪批是新行」⇒ ⇒ 台账原文给出了这个事实（v0.27 起新登记的那批）。

⚠️ **「带标记」的判据 = 「问题」列开头**（不是「整行里出现过标记」）⇒ ⇒
历史行的末列里写着 `✅ **已闭环**` **不算**带标记 ⇒ ⇒ 两个口径才分得开。

⚠️ **本脚本只读台账** ⇒ ⇒ **不改**它一个字节。
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_LEDGER = REPO_ROOT / "docs" / "reviews" / "REVIEWS.md"
FIXTURE_DIR = REPO_ROOT / "tools" / "fixtures" / "ledger"

# ---------------------------------------------------------------------------
# 口径常量（**写死的两个编号** ⇒ 不是正则猜的 —— 裁决 M / N）
# ---------------------------------------------------------------------------

#: 历史行的**下界**（D-15）—— 台账 §1.3 的第一条。
HISTORY_LOWER_BOUND = 15

#: 历史行的**上界**（D-123）—— **分界线**：≤ 它 = 历史行。
HISTORY_UPPER_BOUND = 123

#: 标记区的**起点**（D-124）—— > D-123 = v0.27 起新登记的那批。
MARKED_ZONE_LOWER_BOUND = 124

#: 合法的两个标记（**只在「问题」列开头**才算数）。
MARKER_CLOSED = "✅ **已闭环**"
MARKER_OPEN = "⏳ **未闭环**"
MARKERS = (MARKER_CLOSED, MARKER_OPEN)

# 三条断言各自的**名字**（⇒ 失败清单里能看出是哪一条红的）。
KIND_MARKED_MISSING = "标记区缺标记"
KIND_HISTORY_HAS_MARKER = "历史行带标记"
KIND_TAIL_HAS_MARKER = "末列非空带标记"

#: §1.3 的**小节标题**（`### 1.3 跨模块中低危`）。
SECTION_HEADING_RE = re.compile(r"^#{2,6}\s+1\.3(?![0-9])")

#: 任何 Markdown 标题（⇒ 用来判断「小节结束了」）。
ANY_HEADING_RE = re.compile(r"^#{1,6}\s")

#: 台账条目号（`D-###`）。
LEDGER_ID_RE = re.compile(r"D-(\d+)")


# ---------------------------------------------------------------------------
# 解析
# ---------------------------------------------------------------------------


@dataclass
class LedgerRow:
    """台账 §1.3 的一行。"""

    line_no: int
    id_text: str
    id_number: int
    problem: str
    tail: str
    cell_count: int

    @property
    def marker(self):
        """这一行带的标记（**只看「问题」列开头**）⇒ 没有则 `None`。"""
        for marker in MARKERS:
            if self.problem.startswith(marker):
                return marker
        return None

    @property
    def zone(self) -> str:
        """这一行属于哪个口径（`历史行` / `标记区` / `越界`）。"""
        if self.id_number >= MARKED_ZONE_LOWER_BOUND:
            return "标记区"
        if HISTORY_LOWER_BOUND <= self.id_number <= HISTORY_UPPER_BOUND:
            return "历史行"
        return "越界"


@dataclass
class Failure:
    """一条断言失败。"""

    kind: str
    row: LedgerRow
    message: str


def split_cells(line: str) -> list[str]:
    """一行 Markdown 表格 ⇒ 单元格列表。

    ⚠️ **切分口径 = `tools/doc_check.py` 的那一份**（**一个约束一个落点**）：
    按 GitHub 的规则切 ⇒ ⇒ **转义**的 `\\|` 是**字面竖线**、**不切列**。

    ⚠️ **为什么必须按 GitHub 规则**（实测，2026-10-24）：台账里有**转义**的
    `\\|`（例如 D-69 的那一格写着 `grep "E_PP" docs/ \\| grep -v …`）⇒ ⇒
    朴素 `split("|")` 会把它**误切**成两格 ⇒ ⇒ 「问题」列被**截断**、
    末列**错位** ⇒ ⇒ 判定读到的是**错位的格子** ⇒ ⇒ **假红 / 假绿**。

    ⚠️ **容忍**行**不以** `|` 结尾（台账里 D-83 / D-84 / D-85 是这样）⇒ ⇒
    GitHub 也容忍 ⇒ ⇒ 本脚本与渲染**同口径**。

    ⚠️ 末列 = **第 5 格及其后**的格子合起来（容忍畸形行）⇒ ⇒ 宁可多算，
    不可错位。
    """
    text = line.strip()
    if text.startswith("|"):
        text = text[1:]
    if text.endswith("|"):
        text = text[:-1]
    return [cell.strip() for cell in re.split(r"(?<!\\)\|", text)]


def parse_row(line: str, line_no: int):
    """一行 ⇒ `LedgerRow`（**不是台账行**则 `None`）。"""
    cells = split_cells(line)
    if not cells:
        return None
    match = LEDGER_ID_RE.search(cells[0])
    if not match:
        # 表头行 / 分隔行 / 别的表 ⇒ 不是台账条目行。
        return None
    problem = cells[3] if len(cells) > 3 else ""
    tail = "|".join(cells[4:]).strip() if len(cells) > 4 else ""
    return LedgerRow(
        line_no=line_no,
        id_text=cells[0],
        id_number=int(match.group(1)),
        problem=problem,
        tail=tail,
        cell_count=len(cells),
    )


def extract_section_rows(text: str) -> list[LedgerRow]:
    """台账 §1.3 那张表 ⇒ 台账行列表。

    ⚠️ **只取 §1.3**（§1.1 / §1.2 / §1.4 与 §3 的归档表**不在口径范围内**
    ⇒ ⇒ §1.1 的 D-01 ~ D-05 连「历史行」都不是）。
    """
    lines = text.split("\n")
    start = None
    for index, line in enumerate(lines):
        if SECTION_HEADING_RE.match(line):
            start = index
            break
    if start is None:
        raise SystemExit(f"找不到 §1.3 小节（{SECTION_HEADING_RE.pattern}）")
    rows = []
    for offset, line in enumerate(lines[start + 1 :]):
        if ANY_HEADING_RE.match(line):
            break
        if not line.startswith("|"):
            continue
        row = parse_row(line, start + 2 + offset)
        if row is not None:
            rows.append(row)
    return rows


# ---------------------------------------------------------------------------
# 判定
# ---------------------------------------------------------------------------


@dataclass
class Report:
    """两个口径**各自有名字**的统计（裁决 N）。"""

    history_count: int = 0
    marked_count: int = 0
    closed_count: int = 0
    open_count: int = 0
    out_of_range_count: int = 0

    def lines(self) -> list[str]:
        """报告的两段（**字面照规格**）。"""
        return [
            f"历史行 {self.history_count} 条（处置在末列，不参与标记统计）",
            f"标记区 {self.marked_count} 条 = 已闭环 {self.closed_count} + 未闭环 {self.open_count}",
        ]


def check_rows(rows: list[LedgerRow]) -> tuple[Report, list[Failure]]:
    """三条断言 ⇒ （报告，失败清单）。"""
    report = Report()
    failures: list[Failure] = []

    for row in rows:
        zone = row.zone
        if zone == "标记区":
            report.marked_count += 1
            if row.marker is None:
                failures.append(
                    Failure(
                        KIND_MARKED_MISSING,
                        row,
                        "标记区每行必须带标记，且标记在「问题」列开头"
                        f"（{MARKER_CLOSED} / {MARKER_OPEN}）",
                    )
                )
            elif row.marker == MARKER_CLOSED:
                report.closed_count += 1
            else:
                report.open_count += 1
        elif zone == "历史行":
            report.history_count += 1
            if row.marker is not None:
                failures.append(
                    Failure(
                        KIND_HISTORY_HAS_MARKER,
                        row,
                        "历史行的处置在末列，「问题」列必须不带标记",
                    )
                )
        else:
            report.out_of_range_count += 1

        # ---- 断言 ③：末列非空 ⇒ 不带标记（杜绝「两个真相」再次发生）----
        if row.tail and row.marker is not None:
            failures.append(
                Failure(
                    KIND_TAIL_HAS_MARKER,
                    row,
                    "末列非空却带了标记 ⇒ 处置在两处 ⇒ 两个真相",
                )
            )

    return report, failures


# ---------------------------------------------------------------------------
# 报告
# ---------------------------------------------------------------------------


def print_report(path: Path, report: Report, failures: list[Failure]) -> None:
    print(f"台账口径自检：{path}")
    for line in report.lines():
        print(f"  {line}")
    if report.out_of_range_count:
        print(
            f"  （越界行 {report.out_of_range_count} 条：编号 < D-{HISTORY_LOWER_BOUND} ⇒ 不在两个口径内）"
        )
    if failures:
        print(f"  失败 {len(failures)} 条：")
        for failure in failures:
            print(
                f"    第 {failure.row.line_no} 行 · {failure.row.id_text} · {failure.kind}："
                f"{failure.message}"
            )
    else:
        print("  失败 0 条")


def check_ledger(path: Path) -> int:
    """检查**真台账** ⇒ 退出码（0 = 通过）。"""
    if not path.exists():
        print(f"台账不存在：{path}", file=sys.stderr)
        return 2
    rows = extract_section_rows(path.read_text(encoding="utf-8"))
    report, failures = check_rows(rows)
    print_report(path, report, failures)
    return 1 if failures else 0


# ---------------------------------------------------------------------------
# 自测（--self-test）
# ---------------------------------------------------------------------------

#: 好夹具的期望报告（历史行条数 / 标记区条数 / 已闭环 / 未闭环）。
#:
#: ⚠️ **D-150 那行是「切分口径」的判别力证据** —— 它的「问题」列里含一个
#: **转义**的竖线（`\|`）⇒ ⇒ 按 GitHub 规则切 ⇒ 末格**为空** ⇒ ⇒ 不红；
#: ⇒ ⇒ 朴素 `split("|")` 会把它误切 ⇒ ⇒ 末格**错位**成非空 ⇒ ⇒ **断言 3 红**
#: ⇒ ⇒ 「切分口径对不对」**不是靠嘴说**，是**夹具逼出来的**。
GOOD_EXPECTED_REPORT = Report(
    history_count=2,
    marked_count=4,
    closed_count=3,
    open_count=1,
)

#: 坏夹具的期望失败（**每种坏法各红一次** ⇒ ⇒ 证明脚本有判别力）。
BAD_EXPECTED_KINDS = (
    KIND_MARKED_MISSING,
    KIND_HISTORY_HAS_MARKER,
    KIND_TAIL_HAS_MARKER,
)


def self_test() -> int:
    """用**仓内夹具**自测 ⇒ 退出码（0 = 通过）。"""
    good_path = FIXTURE_DIR / "good.txt"
    bad_path = FIXTURE_DIR / "bad.txt"
    problems: list[str] = []

    # ---- 好的一组：期望 0 红 ----
    if not good_path.exists():
        print(f"夹具不存在：{good_path}", file=sys.stderr)
        return 2
    good_rows = extract_section_rows(good_path.read_text(encoding="utf-8"))
    good_report, good_failures = check_rows(good_rows)
    print_report(good_path, good_report, good_failures)
    if good_failures:
        problems.append(
            f"好夹具应当 0 红，却红了 {len(good_failures)} 条："
            + " / ".join(f.kind for f in good_failures)
        )
    if good_report != GOOD_EXPECTED_REPORT:
        problems.append(
            "好夹具的报告与期望不符："
            f"期望 {GOOD_EXPECTED_REPORT.lines()}，实际 {good_report.lines()}"
        )

    # ---- 坏的一组：期望**每种坏法各红一次** ----
    if not bad_path.exists():
        print(f"夹具不存在：{bad_path}", file=sys.stderr)
        return 2
    bad_rows = extract_section_rows(bad_path.read_text(encoding="utf-8"))
    bad_report, bad_failures = check_rows(bad_rows)
    print_report(bad_path, bad_report, bad_failures)
    got_kinds = sorted(f.kind for f in bad_failures)
    want_kinds = sorted(BAD_EXPECTED_KINDS)
    if got_kinds != want_kinds:
        problems.append(
            "坏夹具的红集与期望不符："
            f"期望 {want_kinds}，实际 {got_kinds}"
        )

    if problems:
        print("\nFAIL: 自测不通过", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1
    print("\nPASS: 台账口径自检自测全过")
    return 0


# ---------------------------------------------------------------------------
# 入口
# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="台账 §1.3 的口径自检（D-144 · STAGE-18 §4.2）。",
    )
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument(
        "--ledger",
        nargs="?",
        const=str(DEFAULT_LEDGER),
        default=None,
        metavar="PATH",
        help=f"检查真台账（默认 {DEFAULT_LEDGER}）",
    )
    group.add_argument(
        "--self-test",
        action="store_true",
        help="用仓内夹具自测（tools/fixtures/ledger/**）",
    )
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()
    path = Path(args.ledger)
    if not path.is_absolute():
        path = (Path.cwd() / path).resolve()
    return check_ledger(path)


if __name__ == "__main__":
    sys.exit(main())
