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

## 六条断言（裁决 M / N + v0.39 批追加两条）

1. **标记区**（D-124 ~）每行**必须**带标记，且标记 ∈ {`✅ **已闭环**`, `⏳ **未闭环**`}
2. **历史行**（D-15 ~ D-123）每行**必须不带**标记
3. **末列非空**的行**必须不带**标记
4. **标记与正文不得自相矛盾**（标记区行）：标记说「未闭环」而正文写着「已闭环」（或反过来）
   ⇒ ⇒ 这条抓的是 **v0.33 批的「台账标记回填批」**那类**机械回填**留下的错
   （`4c5401c` 给 16 行统一插了 `⏳ **未闭环**`，**没逐行核状态** ⇒ ⇒ **D-124 被漏掉**）
5. **每一个「标记区未闭环」的 ID 都必须出现在 §2.1 的候选清单里**
   ⇒ ⇒ 抓的是「**人维护的清单**悄悄漏了一条」⇒ ⇒ 「漏了」变成**门禁能红的事**
6. **候选清单里列着的「标记区」项必须是未闭环的**
   ⇒ ⇒ 与断言 5 配对：清单**过期**（列着一条已闭环的项）也要红

⚠️ **断言 5 / 6 的方向**：
* 断言 5 是**单向**的：**未闭环 ⇒ 必须在清单里**；清单里**可以**多列**历史行**条目
  （D-75 / D-54 / D-87 / D-60 / D-62 / D-27、D-104 ~ D-112 —— 它们**不是标记区**行
  ⇒ ⇒ 不参与这两个判定）。
* 断言 6 只看**清单里的条目行**（**行首**就是编号的那些行）⇒ ⇒ 散文里顺带提到的编号
  （例如「**D-141** 已把器件参数怎么校验定死」）**不算**条目 ⇒ ⇒ 不会误报。

⚠️ **找不到候选清单块 ⇒ ⇒ 必须红**（`KIND_CANDIDATE_BLOCK_MISSING`）⇒ ⇒
否则断言 5 / 6 会**静默失效** ⇒ ⇒ 门禁**恒绿** ⇒ ⇒ 与这一批要治的病**同形**
⇒ ⇒ 台账的命名约定若改了，就要**同批改脚本**。

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

#: 断言 ④ 的名字：**标记与正文自相矛盾**（标记区行）。
KIND_MARKER_CONTRADICTS_BODY = "标记与正文自相矛盾"

#: 断言 ⑤ 的名字：**未闭环项没列进 §2.1 的候选清单**。
KIND_OPEN_ITEM_NOT_LISTED = "未闭环项未列入候选清单"

#: 断言 ⑥ 的名字：**候选清单里列着一条已闭环的标记区项**（清单过期）。
KIND_CANDIDATE_ALREADY_CLOSED = "候选清单列着已闭环项"

#: **找不到候选清单块**的名字（⇒ ⇒ 断言 ⑤ / ⑥ 会**静默失效** ⇒ ⇒ 必须红）。
KIND_CANDIDATE_BLOCK_MISSING = "找不到候选清单块"

#: 正文里那两个**关键词**（**不含**标记的 `**` 与 emoji ⇒ ⇒ 只当「散文里自述的状态」看）。
WORD_CLOSED = "已闭环"
WORD_OPEN = "未闭环"

#: §1.3 的**小节标题**（`### 1.3 跨模块中低危`）。
SECTION_HEADING_RE = re.compile(r"^#{2,6}\s+1\.3(?![0-9])")

#: §2.1 的**小节标题**（`### 2.1 批次拓扑（…）`）。
SECTION_2_1_HEADING_RE = re.compile(r"^#{2,6}\s+2\.1(?![0-9])")

#: 候选清单块的**标题行**判据（台账的命名约定：「候选（留到第 N 批及以后）」）。
CANDIDATE_HEADING_RE = re.compile(r"候选（留到第\s*\d+\s*批及以后）")

#: 候选清单里**一个条目**的判据（**行首**就是编号 ⇒ 散文里顺带提到的编号**不算**条目）。
CANDIDATE_ENTRY_RE = re.compile(r"^\s*D-\d+")

#: §2.1 里**一个批次块**的开头（`第 20 批 · ✅ …`）⇒ ⇒ 候选清单块到这儿为止。
BATCH_BLOCK_RE = re.compile(r"^第\s*\d+\s*批")

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
    def body(self) -> str:
        """「问题」列**去掉标记前缀**之后剩下的正文。

        ⚠️ 断言 ④（标记与正文自相矛盾）**只看正文** ⇒ ⇒ 标记本身
        （`⏳ **未闭环**` 里就带着「未闭环」三个字）**必须先剥掉**。
        """
        if self.marker is None:
            return self.problem
        return self.problem[len(self.marker) :]

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
    row: LedgerRow | None
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
# 候选清单（§2.1）
# ---------------------------------------------------------------------------


def extract_candidate_entries(text: str) -> tuple[dict[int, int], int]:
    """§2.1 的**当前**候选清单 ⇒ （条目编号 → 行号，标题行号）。

    ⚠️ **只取最后一个「候选（留到第 N 批及以后）」块** —— §2.1 里**每个批次块**
    都留过一份「下一批做什么」的清单 ⇒ ⇒ 旧批次那些块是**历史快照** ⇒ ⇒
    **只有最新那份**是「当前清单」⇒ ⇒
    ⚠️ **为什么不能把所有块并起来算**（**判别力的要害**）：
    并起来算 ⇒ ⇒ 「**从当前清单里删掉一条**」会被**旧块掩盖** ⇒ ⇒
    门禁**看不见**这次删除 ⇒ ⇒ **零判别力** ⇒ ⇒ 与断言 ⑤ 要抓的失败模式**正好相反**。

    ⚠️ **一个「条目」的判据 = 行首就是编号**（`CANDIDATE_ENTRY_RE`）⇒ ⇒
    散文里顺带提到的编号（例如候选块里那句「**D-141** 已把器件参数怎么校验定死」）
    **不算**条目 ⇒ ⇒ 断言 ⑥ 不会误报。

    ⚠️ 候选块**到下一个批次块为止**（`BATCH_BLOCK_RE`）⇒ ⇒ 也**到围栏结束为止**
    ⇒ ⇒ §2.1 围栏**外面**的散文**不参与**判定。
    """
    lines = text.split("\n")
    start = None
    for index, line in enumerate(lines):
        if SECTION_2_1_HEADING_RE.match(line):
            start = index
            break
    if start is None:
        return {}, 0
    end = len(lines)
    for index in range(start + 1, len(lines)):
        if ANY_HEADING_RE.match(lines[index]):
            end = index
            break
    section = lines[start + 1 : end]

    heading_index = None
    for index, line in enumerate(section):
        if CANDIDATE_HEADING_RE.search(line):
            heading_index = index
    if heading_index is None:
        return {}, 0

    entries: dict[int, int] = {}
    for index in range(heading_index + 1, len(section)):
        line = section[index]
        if BATCH_BLOCK_RE.match(line):
            break
        if line.lstrip().startswith("```"):
            break
        match = CANDIDATE_ENTRY_RE.match(line)
        if match:
            # ⚠️ **一个条目行里可能有多个编号**（例如「D-130 / D-131 / D-133 / …」
            # 与「D-75 / D-54 / D-87 / D-60 / D-62 / D-27」）⇒ ⇒ 全部收进来。
            for id_match in LEDGER_ID_RE.finditer(line):
                number = int(id_match.group(1))
                entries.setdefault(number, start + 2 + index)
    return entries, start + 2 + heading_index


def check_candidates(rows: list[LedgerRow], text: str) -> list[Failure]:
    """断言 ⑤ / ⑥ ⇒ 失败清单。"""
    failures: list[Failure] = []
    entries, _heading_line = extract_candidate_entries(text)
    if _heading_line == 0:
        # ⚠️ **找不到候选清单块 ⇒ ⇒ 断言 ⑤ / ⑥ 会「静默不生效」**
        # ⇒ ⇒ 那就等于门禁**恒绿** ⇒ ⇒ 与这一批要治的病**同形**
        # ⇒ ⇒ 所以这里**必须红**（⇒ ⇒ 命名约定改了就要同批改脚本）。
        failures.append(
            Failure(
                KIND_CANDIDATE_BLOCK_MISSING,
                None,
                "台账里找不到 §2.1 的候选清单块（标题行须含「候选（留到第 N 批及以后）」）"
                " ⇒ ⇒ 断言 ⑤ / ⑥ **静默失效** ⇒ ⇒ 命名约定若改了就要同批改脚本",
            )
        )
        return failures
    by_number = {row.id_number: row for row in rows}

    # ---- 断言 ⑤：每一个「标记区未闭环」的 ID 都必须列在候选清单里 ----
    for number in sorted(by_number):
        row = by_number[number]
        if row.zone != "标记区" or row.marker != MARKER_OPEN:
            continue
        if number not in entries:
            failures.append(
                Failure(
                    KIND_OPEN_ITEM_NOT_LISTED,
                    row,
                    "这一行是「标记区未闭环」⇒ ⇒ 必须出现在 §2.1 的候选清单里"
                    f"（当前清单在第 {_heading_line} 行那块）",
                )
            )

    # ---- 断言 ⑥：候选清单里列着的**标记区**项必须是未闭环的（否则清单过期）----
    for number in sorted(entries):
        row = by_number.get(number)
        if row is None or row.zone != "标记区":
            # 不是台账条目、或者是**历史行** ⇒ 清单里多列着**允许**（断言 5 是单向的）。
            continue
        if row.marker == MARKER_CLOSED:
            failures.append(
                Failure(
                    KIND_CANDIDATE_ALREADY_CLOSED,
                    row,
                    "候选清单里列着这一条 ⇒ ⇒ 但它在 §1.3 里**已闭环**"
                    " ⇒ ⇒ 清单过期 ⇒ ⇒ 把这一条从清单里划掉",
                )
            )
    return failures


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

        # ---- 断言 ④：标记与正文不得自相矛盾（标记区行）----
        if zone == "标记区" and row.marker is not None:
            body = row.body
            if row.marker == MARKER_OPEN and WORD_CLOSED in body:
                failures.append(
                    Failure(
                        KIND_MARKER_CONTRADICTS_BODY,
                        row,
                        f"标记是「未闭环」而正文写着「{WORD_CLOSED}」"
                        " ⇒ ⇒ 标记与正文自相矛盾 ⇒ ⇒ 核实这一行的真实状态",
                    )
                )
            elif row.marker == MARKER_CLOSED and WORD_OPEN in body:
                failures.append(
                    Failure(
                        KIND_MARKER_CONTRADICTS_BODY,
                        row,
                        f"标记是「已闭环」而正文写着「{WORD_OPEN}」"
                        " ⇒ ⇒ 标记与正文自相矛盾 ⇒ ⇒ 核实这一行的真实状态",
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
            where = (
                f"第 {failure.row.line_no} 行 · {failure.row.id_text}"
                if failure.row is not None
                else "（无对应台账行）"
            )
            print(f"    {where} · {failure.kind}：{failure.message}")
    else:
        print("  失败 0 条")


def check_ledger(path: Path) -> int:
    """检查**真台账** ⇒ 退出码（0 = 通过）。"""
    if not path.exists():
        print(f"台账不存在：{path}", file=sys.stderr)
        return 2
    text = path.read_text(encoding="utf-8")
    rows = extract_section_rows(text)
    report, failures = check_rows(rows)
    failures += check_candidates(rows, text)
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
    KIND_MARKER_CONTRADICTS_BODY,
    KIND_OPEN_ITEM_NOT_LISTED,
    KIND_CANDIDATE_ALREADY_CLOSED,
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
    good_text = good_path.read_text(encoding="utf-8")
    good_rows = extract_section_rows(good_text)
    good_report, good_failures = check_rows(good_rows)
    good_failures += check_candidates(good_rows, good_text)
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
    bad_text = bad_path.read_text(encoding="utf-8")
    bad_rows = extract_section_rows(bad_text)
    bad_report, bad_failures = check_rows(bad_rows)
    bad_failures += check_candidates(bad_rows, bad_text)
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
