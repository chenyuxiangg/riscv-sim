#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""场景码的「**定义处 / 引用处**」清单 + 「**唯一定义**」不变式的机器化检查
（台账 **D-132** · v0.42 批 · STAGE-27 §2.3 / §3.3 · 裁决 C / D）。

台账 **D-132** 的现状（原文）：

    **场景码的定义处分散在契约层与内核 crate** ⇒ ⇒ 「场景码住哪」这条纪律
    **没有统一口径**（D-120 只禁了「同一码两份定义」，没禁「定义处分散」）。

⇒ ⇒ 那件事被拆成**两半**，本脚本只做**机械的那一半**：

* **机械的一半**（本脚本）：每个 `E_XXX_NN` 的**唯一定义处** + **全部引用处**
  ⇒ ⇒ 这是台账 **D-120** 那条不变式（「同一码只有一处定义」）的**机器化**。
* **判断的一半**（**人工**）：「这个码**住哪**」⇒ ⇒ 那是**判据 + 人工**
  （STAGE-27 §2.3 裁决 C）⇒ ⇒ ⛔ **本脚本不自动分类**（裁决 D）。

## 判据（**逐字** · 写死 · 裁决 C）

一个场景码**住哪** ⇒ ⇒ 看**两件事**是否**同时**成立（**「或」，不是「且」**）：

1. 它**跨模块共享**（**别的 crate** 要引用它），**或**
2. **前端要按码分派**（前端拿到这个码要决定做什么）。

⇒ ⇒ **满足 ⇒ 住契约层**；**不满足 ⇒ 住模块内**。

⚠️ 这条判据**不由本脚本判定** ⇒ ⇒ 本脚本只把判据需要的**事实**摆出来：

* **定义 crate**（这个码在哪个 crate 里定义）
* **引用 crate**（哪些 crate 引用了它 ⇒ ⇒ 跨 crate 引用 = 判据 ① 的材料）
* **前端提及**（前端**运行时代码**有没有点名它 ⇒ ⇒ 判据 ② 的材料）

## 本脚本**断言**的三件事（每条一个**独立**的失败种类）

| 种类 | 断言 | 对应 |
| --- | --- | --- |
| `重复定义` | 同一个 `E_XXX_NN` **只有一处**定义 | 台账 **D-120** |
| `值与名不符` | 常量的值 == 名字里的两位场景号 | 「一个码一个含义」 |
| `族未声明` | 族标签在 `error_families!` 里声明过 | 「**加码不加族**」 |

⚠️ **判据 ① 的键是「码」**（`E_XXX_NN`），**不是**常量的名字 ⇒ ⇒ 两处
**名字不同**、但**同一个码**的定义**照样**被抓出来（那正是 D-120 的形状）。

## 它**不能**查什么（裁决 D · 验收 A-5）

⛔ **它不判定「场景码住哪」** ⇒ ⇒ 那是**判据 + 人工**（见上）⇒ ⇒
⛔ **禁**把本脚本写成自动分类器 ⇒ ⇒ 每一次运行都打印这条边界声明。

## 扫描口径（**写死**）

* **定义** = **Rust** 源码里的 `const` 项 ⇒ ⇒ 名字形如
  `E_<FAM>_<NN>[_<MEANING>]` ⇒ ⇒ 前端**不**能定义 ⇒ ⇒
  ⚠️ 前端镜像契约层的码是**既有设计**（如 `front/lib/frame.js` 的
  `CRC_MISMATCH_CODE = "E_PROTO-06"`）⇒ ⇒ 把它算成「第二处定义」会把
  **既有设计**报成违例 ⇒ ⇒ 前端**只**算**引用**。
* **引用** = 源码里出现的**码**（两种拼写都算）：
  * **符号引用** = 那个 Rust 常量名（`E_PLT_07_OVER_BUDGET`）；
  * **文本提及** = 别的拼写（裸的 `E_PLT_07`、线上格式 `E_PLT-07`）。
* **前端提及**只统计**运行时代码** ⇒ ⇒ `front/tests/` **不算**
  ⇒ ⇒ 「测试里断言了一个码」**不是**「前端按码分派」。
* **不扫** `docs/` ⇒ ⇒ 冻结规格与台账里的是**散文**（还留着
  `E_PLAT_12` 这类**过期拼写**）⇒ ⇒ 扫它们会把**文档**报成违例。

## 用法

    python3 tools/error_code_check.py               # 扫真仓（清单 + 三条断言）
    python3 tools/error_code_check.py --no-listing   # 只报断言结果
    python3 tools/error_code_check.py --self-test    # 用**自带夹具**自测
    python3 tools/error_code_check.py --help

## 退出码

| 码 | 含义 |
|----|------|
| **0** | 没报出问题（工具本身没出错） |
| **1** | 报了问题（至少一条断言红） |
| **2** | **工具自身出错**（参数错 / 源码树坏 / 找不到 `error_families!`） |

⚠️ **两种失败必须能分开** ⇒ ⇒ 「报出问题」是 **1**，「工具坏了」是 **2**。

## 夹具（验收 A-6）

⚠️ 夹具**内嵌在本文件里** ⇒ ⇒ `--self-test` 把它写进一个**临时目录**再跑
⇒ ⇒ **不往仓里写一个字节** ⇒ ⇒ 而「好夹具 0 红 / 坏夹具红」这条**对照**
本身**必须成立** ⇒ ⇒ 否则这个检查就是**恒绿**的。

* **好夹具**：每个码**一处**定义、值与名相符、族已声明 ⇒ ⇒ **0 红**。
* **坏夹具**：**三种坏法各一处** ⇒ ⇒ 红集 = {`重复定义`, `值与名不符`,
  `族未声明`} ⇒ ⇒ 三条断言被**单独**钉住（⇒ ⇒ 变异自检可以逐条打掉）。
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# ---------------------------------------------------------------------------
# 扫描口径（**写死**）
# ---------------------------------------------------------------------------

#: **定义**只能落在 Rust 源码里。
RUST_SUFFIXES = (".rs",)

#: 前端源码（**只**算引用，不算定义）。
FRONT_SUFFIXES = (".js", ".mjs")

#: 扫描根（相对 `--root`）。
CRATES_ROOT = "crates"
FRONT_ROOT = "front"

#: 前端的**测试**目录 ⇒ ⇒ 里面的提及**不算**「前端按码分派」。
FRONT_TEST_DIR = "front/tests"

#: Rust 的 `const` 定义 ⇒ ⇒ `E_<FAM>_<NN>[_<MEANING>]`。
DEF_RE = re.compile(
    r"(?m)^[ \t]*(?:pub(?:\s*\([^)]*\))?[ \t]+)?const[ \t]+"
    r"(E_[A-Z]{3,5}_[0-9]{2}[A-Z0-9_]*)[ \t]*:[ \t]*[A-Za-z_][A-Za-z0-9_]*"
    r"[ \t]*=[ \t]*([0-9]+|0[xX][0-9a-fA-F]+)[uUlL]*[ \t]*;"
)

#: 源码里出现的**码**（两种拼写 + 可选的常量名后缀）。
CODE_TOKEN_RE = re.compile(r"\bE_[A-Z]{3,5}(?:_[0-9]{2}|-[0-9]{2})(?:_[A-Z0-9]+)*\b")

#: 一个常量名里的**码**部分（`E_PLT_07_OVER_BUDGET` ⇒ `E_PLT_07`）。
CODE_PREFIX_RE = re.compile(r"^(E_[A-Z]{3,5})[-_]([0-9]{2})")

#: `error_families!` 宏的**调用块** ⇒ ⇒ 族标签的唯一落点。
#:
#: ⚠️ **必须锚在**第 0 列** ⇒ ⇒ 否则会匹配到 `macro_rules!` 的**文档示例**
#: （`/// error_families! {`）⇒ ⇒ 那只声明了一个族（`CTX`）⇒ ⇒ 真仓会
#: **全红**（实测踩过 ⇒ ⇒ 这条锚点是**必需的**，不是洁癖）。
FAMILY_BLOCK_RE = re.compile(r"(?ms)^error_families!\s*\{(.*?)^}")

#: 块里的一条族声明 ⇒ ⇒ `Variant => "TAG",`。
FAMILY_TAG_RE = re.compile(r'=>\s*"([A-Z]{3,5})"')

#: 契约层的族声明文件（相对 `--root`）。
FAMILY_DECL_PATH = "crates/contract/src/error.rs"

# ---------------------------------------------------------------------------
# 三条断言各自的**名字**（⇒ ⇒ 失败清单里能看出是哪一条红的）
# ---------------------------------------------------------------------------

#: 同一个码有**两处**定义 ⇒ ⇒ 台账 **D-120**。
KIND_DUPLICATE_DEFINITION = "重复定义"

#: 常量的值 **≠** 名字里的两位场景号。
KIND_VALUE_MISMATCH = "值与名不符"

#: 族标签**没有**在 `error_families!` 里声明过 ⇒ ⇒ 「加码不加族」。
KIND_FAMILY_UNDECLARED = "族未声明"

#: 退出码。
EXIT_OK = 0
EXIT_FINDINGS = 1
EXIT_TOOL_ERROR = 2

#: **判别力边界声明**（裁决 D）—— **每一次运行都打印**。
BOUNDARY_STATEMENT = (
    "BOUNDARY: this tool reports FACTS only — the definition site, every\n"
    "          reference site, which crates reference the code, and whether the\n"
    "          front end names it.  It does NOT decide where a scenario code\n"
    "          'lives': that is the criterion (cross-module shared OR the front\n"
    "          end dispatches on it) plus a human judgement (STAGE-27 §2.3).\n"
    "          （本工具**不**判定「场景码住哪」⇒ ⇒ 那是**判据 + 人工**。）"
)


class ToolError(Exception):
    """工具自身出错（⇒ ⇒ 退出码 2，**不是**「报出了问题」）。"""


# ---------------------------------------------------------------------------
# 数据形状
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Definition:
    """一处**定义**。"""

    code: str
    const_name: str
    value: int
    rel_path: str
    line: int

    @property
    def crate(self) -> str:
        return crate_of(self.rel_path)


@dataclass(frozen=True)
class Occurrence:
    """一处**出现**（引用或文本提及）。"""

    rel_path: str
    line: int
    spelling: str
    comment: bool = False


@dataclass
class CodeReport:
    """一个**码**的全部事实。"""

    code: str
    definitions: list[Definition] = field(default_factory=list)
    symbol_refs: list[Occurrence] = field(default_factory=list)
    text_refs: list[Occurrence] = field(default_factory=list)
    front_refs: list[Occurrence] = field(default_factory=list)

    @property
    def def_crates(self) -> list[str]:
        return sorted({d.crate for d in self.definitions})

    @property
    def ref_crates(self) -> list[str]:
        """**Rust** 侧引用了这个码的 crate（前端**另算**，见 `front_refs`）。"""
        crates = {crate_of(o.rel_path) for o in self.symbol_refs}
        crates |= {crate_of(o.rel_path) for o in self.text_refs}
        return sorted(crates)

    @property
    def front_uses(self) -> list[Occurrence]:
        """前端**运行时代码**里**真的用到**这个码（`front/tests/` **不算**）。"""
        return [
            o
            for o in self.front_refs
            if not is_front_test(o.rel_path) and not o.comment
        ]

    @property
    def front_comments(self) -> list[Occurrence]:
        """前端**运行时代码**的**注释**里提到这个码（`front/tests/` **不算**）。"""
        return [
            o for o in self.front_refs if not is_front_test(o.rel_path) and o.comment
        ]


@dataclass(frozen=True)
class Finding:
    """一条**断言失败**。"""

    kind: str
    code: str
    detail: str


@dataclass
class Report:
    """一次扫描的**全部**结果。"""

    findings: list[Finding] = field(default_factory=list)
    codes: dict[str, CodeReport] = field(default_factory=dict)
    named_without_definition: list[str] = field(default_factory=list)
    undefined_occurrences: dict[str, list[Occurrence]] = field(default_factory=dict)
    family_tags: list[str] = field(default_factory=list)


# ---------------------------------------------------------------------------
# 扫描
# ---------------------------------------------------------------------------


def crate_of(rel_path: str) -> str:
    """一个相对路径属于哪个 crate（`crates/<name>/…` ⇒ `<name>`）。"""
    parts = rel_path.split("/")
    if parts[0] == CRATES_ROOT and len(parts) > 1:
        return parts[1]
    if parts[0] == FRONT_ROOT:
        return FRONT_ROOT
    return parts[0]


def is_front_test(rel_path: str) -> bool:
    """前端**测试**里的提及不算「前端按码分派」。"""
    return rel_path.startswith(FRONT_TEST_DIR + "/")


def collect_sources(root: Path) -> list[tuple[str, str]]:
    """扫出**要读**的源码 ⇒ ⇒ `[(相对路径, 文本)]`（相对 `--root`）。

    ⚠️ **不扫** `docs/` ⇒ ⇒ 冻结规格与台账里的是**散文**。
    """
    sources: list[tuple[str, str]] = []
    for rel_dir, suffixes in ((CRATES_ROOT, RUST_SUFFIXES), (FRONT_ROOT, FRONT_SUFFIXES)):
        base = root / rel_dir
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*")):
            if not path.is_file() or path.suffix not in suffixes:
                continue
            rel = path.relative_to(root).as_posix()
            try:
                text = path.read_text(encoding="utf-8")
            except UnicodeDecodeError as err:
                raise ToolError(f"{rel}: not valid UTF-8 ({err})") from err
            sources.append((rel, text))
    if not sources:
        raise ToolError(f"{root}: no source files found under {CRATES_ROOT}/ or {FRONT_ROOT}/")
    return sources


def parse_definitions(sources: list[tuple[str, str]]) -> list[Definition]:
    """把**定义**挑出来（Rust 的 `const` 项）。"""
    found: list[Definition] = []
    for rel, text in sources:
        if not rel.startswith(CRATES_ROOT + "/"):
            continue
        for match in DEF_RE.finditer(text):
            const_name = match.group(1)
            raw_value = match.group(2)
            value = int(raw_value, 16) if raw_value[:2].lower() == "0x" else int(raw_value)
            prefix = CODE_PREFIX_RE.match(const_name)
            if prefix is None:  # pragma: no cover —— DEF_RE 已经保证了形状
                continue
            line = text.count("\n", 0, match.start()) + 1
            found.append(
                Definition(
                    code=f"{prefix.group(1)}_{prefix.group(2)}",
                    const_name=const_name,
                    value=value,
                    rel_path=rel,
                    line=line,
                )
            )
    return found


def parse_family_tags(root: Path) -> list[str]:
    """从 `error_families!` 里读出**族标签**（唯一落点）。"""
    path = root / FAMILY_DECL_PATH
    if not path.is_file():
        raise ToolError(f"the family declaration file is missing: {path}")
    text = path.read_text(encoding="utf-8")
    block = FAMILY_BLOCK_RE.search(text)
    if block is None:
        raise ToolError(f"{FAMILY_DECL_PATH}: no `error_families! {{ … }}` block found")
    tags = FAMILY_TAG_RE.findall(block.group(1))
    if not tags:
        raise ToolError(f"{FAMILY_DECL_PATH}: the `error_families!` block declares no family")
    return tags


def comment_mask(text: str) -> list[bool]:
    """逐字符扫 ⇒ ⇒ 每个字符**是不是在注释里**（Rust 与 JS 共用）。

    ⚠️ **块注释也算**（`/* … */`、Rust 的 `/** … */`）⇒ ⇒ 只认 `//`
    会把块注释里的提及**误判**成「代码里在用」⇒ ⇒ 实测踩过
    （`front/app.js` 的 `/** … */` 文档块 ⇒ ⇒ 判别力缺口）。

    ⚠️ **字符串字面量不算注释** ⇒ ⇒ `"http://x"` 里的 `//` **不是**注释。
    ⚠️ Rust 的**生命周期**（`&'a str`）**不是**字符字面量 ⇒ ⇒ 别把它
    当成字符串的开始（否则整份文件的后半截都被当成字符串）。
    """
    mask: list[bool] = []
    in_block = False
    in_string: str | None = None
    index = 0
    length = len(text)
    while index < length:
        char = text[index]
        if in_block:
            if char == "*" and index + 1 < length and text[index + 1] == "/":
                mask.append(True)
                mask.append(True)
                index += 2
                in_block = False
                continue
            mask.append(True)
            index += 1
            continue
        if in_string is not None:
            mask.append(False)
            if char == "\\" and index + 1 < length:
                mask.append(False)
                index += 2
                continue
            if char == in_string:
                in_string = None
            index += 1
            continue
        # ---- 代码区 ----
        if char == "/" and index + 1 < length and text[index + 1] == "/":
            while index < length and text[index] != "\n":
                mask.append(True)
                index += 1
            continue
        if char == "/" and index + 1 < length and text[index + 1] == "*":
            mask.append(True)
            mask.append(True)
            index += 2
            in_block = True
            continue
        if char == "\"" or char == "`":
            in_string = char
            mask.append(False)
            index += 1
            continue
        if char == "'":
            # ⚠️ Rust 的**字符字面量**（`'a'`）与**生命周期**（`'a`）要分开看。
            if index + 2 < length and text[index + 2] == "'":
                mask.append(False)
                mask.append(False)
                mask.append(False)
                index += 3
                continue
            mask.append(False)
            index += 1
            continue
        mask.append(False)
        index += 1
    return mask


def collect_occurrences(sources: list[tuple[str, str]]) -> dict[str, list[Occurrence]]:
    """把**码的出现**收齐 ⇒ ⇒ `{码: [出现]}`。"""
    found: dict[str, list[Occurrence]] = {}
    for rel, text in sources:
        mask = comment_mask(text)
        # ⚠️ 按 **"\n"** 切（不是 `splitlines()`）⇒ ⇒ 每行后面**恰好**一个
        # 换行 ⇒ ⇒ 行首的字符下标 = 前面各行长度 + 1 ⇒ ⇒ 与掩码**逐字对齐**。
        offset = 0
        for lineno, line in enumerate(text.split("\n"), start=1):
            for match in CODE_TOKEN_RE.finditer(line):
                prefix = CODE_PREFIX_RE.match(match.group(0))
                if prefix is None:  # pragma: no cover —— CODE_TOKEN_RE 保证形状
                    continue
                code = f"{prefix.group(1)}_{prefix.group(2)}"
                found.setdefault(code, []).append(
                    Occurrence(
                        rel_path=rel,
                        line=lineno,
                        spelling=match.group(0),
                        comment=mask[offset + match.start()],
                    )
                )
            offset += len(line) + 1
    return found


# ---------------------------------------------------------------------------
# 三条断言
# ---------------------------------------------------------------------------


def judge(definitions: list[Definition], family_tags: list[str]) -> list[Finding]:
    """跑**三条断言** ⇒ ⇒ 返回失败清单。"""
    findings: list[Finding] = []
    by_code: dict[str, list[Definition]] = {}
    for definition in definitions:
        by_code.setdefault(definition.code, []).append(definition)

    for code in sorted(by_code):
        sites = by_code[code]
        # ---- 断言 ①：唯一定义（台账 D-120）----
        if len(sites) > 1:
            where = "、".join(f"{d.rel_path}:{d.line}（{d.const_name}）" for d in sites)
            findings.append(
                Finding(
                    kind=KIND_DUPLICATE_DEFINITION,
                    code=code,
                    detail=f"{len(sites)} 处定义：{where}",
                )
            )
        # ---- 断言 ②：值 == 名字里的两位场景号 ----
        number = int(code.rsplit("_", 1)[1])
        for definition in sites:
            if definition.value != number:
                findings.append(
                    Finding(
                        kind=KIND_VALUE_MISMATCH,
                        code=code,
                        detail=(
                            f"{definition.rel_path}:{definition.line}（{definition.const_name}）"
                            f"的值是 {definition.value}，而码 {code} 的场景号是 {number:02d}"
                        ),
                    )
                )
        # ---- 断言 ③：族已声明（「加码不加族」）----
        tag = code[2:].rsplit("_", 1)[0]
        if tag not in family_tags:
            findings.append(
                Finding(
                    kind=KIND_FAMILY_UNDECLARED,
                    code=code,
                    detail=(
                        f"族标签 {tag} 不在 `error_families!` 声明的族里"
                        f"（{', '.join(family_tags)}）"
                    ),
                )
            )
    return findings


# ---------------------------------------------------------------------------
# 报告
# ---------------------------------------------------------------------------


def build_report(root: Path) -> Report:
    """扫一遍 ⇒ ⇒ 拼出**报告**。"""
    sources = collect_sources(root)
    family_tags = parse_family_tags(root)
    definitions = parse_definitions(sources)
    occurrences = collect_occurrences(sources)

    codes: dict[str, CodeReport] = {}
    for definition in definitions:
        codes.setdefault(definition.code, CodeReport(code=definition.code)).definitions.append(
            definition
        )

    const_names = {d.const_name for d in definitions}
    for code, occs in occurrences.items():
        report = codes.get(code)
        for occurrence in occs:
            is_definition_site = any(
                occurrence.rel_path == d.rel_path
                and occurrence.line == d.line
                and occurrence.spelling == d.const_name
                for d in (report.definitions if report else [])
            )
            if is_definition_site:
                continue
            if report is None:
                # ⚠️ 这个码**没有** Rust 定义 ⇒ ⇒ 只出现在「被点名但无定义」里。
                continue
            if occurrence.rel_path.startswith(FRONT_ROOT + "/"):
                report.front_refs.append(occurrence)
            elif occurrence.spelling in const_names:
                report.symbol_refs.append(occurrence)
            else:
                report.text_refs.append(occurrence)

    named_without_definition = sorted(
        code
        for code, occs in occurrences.items()
        if code not in codes
        and any(not is_front_test(o.rel_path) for o in occs)
    )

    return Report(
        findings=judge(definitions, family_tags),
        codes=codes,
        named_without_definition=named_without_definition,
        undefined_occurrences={
            code: occs for code, occs in occurrences.items() if code not in codes
        },
        family_tags=family_tags,
    )


#: 注释里的提及的**标记**（⇒ ⇒ 「提到」**不等于**「用到」）。
COMMENT_MARK = "※"


def format_occurrences(occurrences: list[Occurrence]) -> list[str]:
    """把出现**按文件归拢** ⇒ ⇒ `["path:1, 2※, 3", …]`。

    ⚠️ `※` = 该处**在注释里** ⇒ ⇒ 「提到」**不等于**「引用」。
    """
    grouped: dict[str, dict[int, bool]] = {}
    for occurrence in occurrences:
        grouped.setdefault(occurrence.rel_path, {})[occurrence.line] = occurrence.comment
    out = []
    for rel in sorted(grouped):
        spots = []
        for line in sorted(grouped[rel]):
            spots.append(f"{line}{COMMENT_MARK}" if grouped[rel][line] else str(line))
        out.append(f"{rel} : {', '.join(spots)}")
    return out


def print_listing(report: Report) -> None:
    """打印**清单**（每个码的定义处 + 全部引用处）。"""
    print(f"== 场景码清单（{len(report.codes)} 个码）==")
    print("判据（裁决 C，**不由本脚本判定**）：跨模块共享 **或** 前端要按码分派。")
    print()
    for code in sorted(report.codes):
        item = report.codes[code]
        names = "、".join(sorted({d.const_name for d in item.definitions}))
        print(f"{code}  ({names})")
        for definition in item.definitions:
            print(f"  定义处   : {definition.rel_path}:{definition.line}")
        if item.symbol_refs:
            print("  符号引用 :")
            for line in format_occurrences(item.symbol_refs):
                print(f"    {line}")
        if item.text_refs:
            print("  文本提及 :")
            for line in format_occurrences(item.text_refs):
                print(f"    {line}")
        print(f"  定义 crate : {', '.join(item.def_crates)}")
        print(f"  引用 crate : {', '.join(item.ref_crates) if item.ref_crates else '（无）'}")
        front_use = format_occurrences(item.front_uses)
        front_note = format_occurrences(item.front_comments)
        print(f"  前端**使用** : {'; '.join(front_use) if front_use else '（无）'}")
        print(f"  前端**注释** : {'; '.join(front_note) if front_note else '（无）'}")
        print()

    if report.named_without_definition:
        print("== 被点名、但 Rust 侧**没有定义**的码（**观察**，不是发现）==")
        print("⚠️ 这些码**没有** Rust 定义 ⇒ ⇒ 它们要么**只活在前端**（宿主侧），")
        print("   要么是**只被提到**、从没被定义过 ⇒ ⇒ 「住哪」要按判据判。")
        for code in report.named_without_definition:
            spots = format_occurrences(
                [o for o in report.undefined_occurrences.get(code, []) if not is_front_test(o.rel_path)]
            )
            print(f"  {code} : {'; '.join(spots) if spots else '（无）'}")
        print()


def print_report(report: Report, source: str) -> None:
    """打印**断言结果** + **边界声明**。"""
    print(f"source: {source}")
    if report.findings:
        print(f"findings: {len(report.findings)}")
        for finding in report.findings:
            print(f"  - [{finding.kind}] {finding.code}: {finding.detail}")
    else:
        print("findings: 0")
    print()
    print(BOUNDARY_STATEMENT)


# ---------------------------------------------------------------------------
# 三种模式
# ---------------------------------------------------------------------------


def check_root(root: Path, listing: bool) -> Report:
    """扫一棵源码树 ⇒ ⇒ 判三条断言。"""
    report = build_report(root)
    if listing:
        print_listing(report)
    print_report(report, str(root))
    return report


def check_tree(tree: dict[str, str], label: str) -> Report:
    """把一棵**假**源码树写进临时目录 ⇒ ⇒ 扫它。"""
    with tempfile.TemporaryDirectory(prefix="error_code_check_") as tmp:
        base = Path(tmp)
        for rel, text in tree.items():
            path = base / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
        return check_root(base, listing=False)


# ---------------------------------------------------------------------------
# 夹具（**内嵌** ⇒ ⇒ 不往仓里写一个字节）
# ---------------------------------------------------------------------------

#: 一棵**假**源码树的 `error_families!` 块（好/坏夹具**共用**）。
#:
#: ⚠️ 块**前面**故意放一个**宏定义**，宏里带一个**文档示例**
#: （`/// error_families! {`，**缩进**）⇒ ⇒ 它只声明了一个族（`CTX`）⇒ ⇒
#: 解析若**不**锚在第 0 列 ⇒ ⇒ 读到的**只有**示例里的那个族 ⇒ ⇒ 块里的
#: `BUS` / `PLT` / `LDR` 全变成「族未声明」⇒ ⇒ **好夹具会红** ⇒ ⇒ 这段
#: 注释把「锚点」这条口径**钉住**。
#:
#: ⚠️ 这条口径**是实测踩出来的**：解析最初**不**锚在第 0 列 ⇒ ⇒ 真仓
#: **51 条红**（每个码一条「族未声明」）⇒ ⇒ 补上锚点后回到 0 红 ⇒ ⇒ 而
#: **夹具**必须能**独立**复现它 ⇒ ⇒ 所以夹具里要有这个宏定义。
_FIXTURE_FAMILIES = """//! 夹具的契约层错误码。
//!
//! ⚠️ 下面这个**宏定义**里的**文档示例**不是声明处 ⇒ ⇒ 解析必须锚在第 0 列。
macro_rules! error_families {
    /// ```ignore
    /// error_families! {
    ///     Context => "CTX",
    /// }
    /// ```
    ($( $variant:ident => $tag:literal, )*) => {
        /// 生成的族枚举。
        pub enum ErrorFamily {
            $(
                $variant,
            )*
        }
    };
}

error_families! {
    /// Session / command / view-store context.
    Context => "CTX",
    /// Bus and memory map.
    Bus => "BUS",
    /// Platform-description errors.
    Platform => "PLT",
    /// ELF / DWARF / object parsing.
    Loader => "LDR",
}
"""

#: **好夹具** —— 每个码**一处**定义、值与名相符、族已声明 ⇒ ⇒ 期望 **0 红**。
FIXTURE_GOOD: dict[str, str] = {
    "crates/contract/src/error.rs": (
        "//! 好夹具的契约层错误码。\n" + _FIXTURE_FAMILIES + """
pub const E_CTX_07_INVALID_OR_DUPLICATE: u16 = 7;
pub const E_PLT_01_SCHEMA_VERSION: u16 = 1;
"""
    ),
    "crates/contract/src/port.rs": (
        "//! 好夹具里契约层的另一个文件。\n"
        "pub const E_PLT_02_EMPTY_ID: u16 = 2;\n"
    ),
    "crates/kernel/src/m08_breakpoint/mod.rs": (
        "//! 好夹具：内核引用契约层的码。\n"
        "use riscv_sim_contract::error::E_CTX_07_INVALID_OR_DUPLICATE;\n"
        "pub const E_BUS_03_OVERLAP: u16 = 3;\n"
    ),
    "crates/kernel/src/m14_bus/mod.rs": (
        "//! 好夹具：内核自有的码。\n"
        "pub const E_LDR_02_PARSE_LIMIT: u16 = 2;\n"
        "/// 文档里提到 `E_BUS-03`（**文本提及**）。\n"
    ),
    "front/services/m05_commandrouter.js": (
        '// 好夹具：前端点名一个契约层的码。\n'
        'export const UNKNOWN = "E_PLT-01";\n'
        '// ⚠️ 注释里**提到** `E_PLT-02` ⇒ ⇒ 「提到」**不等于**「用到」。\n'
    ),
    "front/tests/only-in-tests.mjs": (
        "// ⚠️ 前端**测试**里的提及**不算**「前端按码分派」。\n"
        'export const ONLY_IN_TESTS = "E_LDR-02";\n'
    ),
}

#: **坏夹具** —— **三种坏法各一处** ⇒ ⇒ 红集 = 三种失败各一次。
FIXTURE_BAD: dict[str, str] = {
    "crates/contract/src/error.rs": (
        "//! 坏夹具的契约层错误码。\n" + _FIXTURE_FAMILIES + """
pub const E_CTX_07_INVALID_OR_DUPLICATE: u16 = 7;
pub const E_PLT_01_SCHEMA_VERSION: u16 = 2;
pub const E_ZZZ_01_UNKNOWN_FAMILY: u16 = 1;
"""
    ),
    "crates/kernel/src/m08_breakpoint/mod.rs": (
        "//! 坏夹具 ①：**同一个码**（`E_CTX_07`）在**内核**又定义了一次，\n"
        "//! 而且**名字不同** ⇒ ⇒ 键是「码」不是「名字」⇒ ⇒ 照样被抓出来。\n"
        "pub const E_CTX_07_BAD_BREAKPOINT: u16 = 7;\n"
    ),
    "crates/kernel/src/m14_bus/mod.rs": (
        "//! 坏夹具 ② 的对照行：一个**正常**的码（⇒ ⇒ 它**不该**红）。\n"
        "pub const E_BUS_03_OVERLAP: u16 = 3;\n"
    ),
    "front/services/m05_commandrouter.js": (
        "// 坏夹具：前端点名一个**存在**的码（⇒ ⇒ 前端提及不是失败）。\n"
        'export const UNKNOWN = "E_PLT-01";\n'
    ),
}

#: 坏夹具的**期望红集**（⇒ ⇒ 三条断言被**单独**钉住）。
BAD_EXPECTED_KINDS = sorted(
    [KIND_DUPLICATE_DEFINITION, KIND_VALUE_MISMATCH, KIND_FAMILY_UNDECLARED]
)

#: 坏夹具的**期望失败条数**（每种坏法各一次）。
BAD_EXPECTED_COUNT = 3


def self_test() -> int:
    """用**自带夹具**自测 ⇒ ⇒ 证明这个检查**不是恒绿**（验收 A-6 / X-4）。"""
    problems: list[str] = []

    print("== 好夹具（期望 0 红）==")
    good = check_tree(FIXTURE_GOOD, "good fixture")
    if good.findings:
        problems.append(
            f"好夹具应当 0 红，却红了 {len(good.findings)} 条："
            + " / ".join(f.kind for f in good.findings)
        )
    # ⚠️ 前端**测试**里的提及**不算**「前端按码分派」⇒ ⇒ 钉住这条口径。
    if "E_LDR_02" in good.named_without_definition:
        problems.append(
            "前端测试目录里的提及被算成了「前端按码分派」⇒ ⇒ 口径破了"
        )
    if "E_LDR_02" not in good.codes:
        problems.append("好夹具里 `E_LDR_02` 消失了 ⇒ ⇒ 清单没列全")
    elif good.codes["E_LDR_02"].front_uses or good.codes["E_LDR_02"].front_comments:
        problems.append(
            "前端**测试**目录里的提及被算成了「前端提及」⇒ ⇒ 口径破了"
        )
    if "E_CTX_07" not in good.codes:
        problems.append("好夹具里 `E_CTX_07` 消失了 ⇒ ⇒ 清单没列全")
    elif not good.codes["E_CTX_07"].symbol_refs:
        problems.append("好夹具里 `E_CTX_07` 的跨 crate 引用没列出来 ⇒ ⇒ 清单不全")
    # ⚠️ 「注释里提到」**不等于**「前端用到」⇒ ⇒ 钉住这条口径。
    if not good.codes.get("E_PLT_01") or not good.codes["E_PLT_01"].front_uses:
        problems.append("好夹具里 `E_PLT_01` 的前端**使用**没列出来 ⇒ ⇒ 清单不全")
    if good.codes.get("E_PLT_02") and good.codes["E_PLT_02"].front_uses:
        problems.append("注释里的提及被算成了「前端**使用**」⇒ ⇒ 口径破了")
    if not good.codes.get("E_PLT_02") or not good.codes["E_PLT_02"].front_comments:
        problems.append("好夹具里 `E_PLT_02` 的前端**注释**没列出来 ⇒ ⇒ 清单不全")

    print()
    print("== 坏夹具（期望红）==")
    bad = check_tree(FIXTURE_BAD, "bad fixture")
    got_kinds = sorted(f.kind for f in bad.findings)
    if got_kinds != BAD_EXPECTED_KINDS:
        problems.append(
            f"坏夹具的红集与期望不符：期望 {BAD_EXPECTED_KINDS}，实际 {got_kinds}"
        )
    if len(bad.findings) != BAD_EXPECTED_COUNT:
        problems.append(
            "坏夹具的失败条数与期望不符："
            f"期望 {BAD_EXPECTED_COUNT} 条，实际 {len(bad.findings)} 条"
        )

    if problems:
        print("\nFAIL: 自测不通过", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return EXIT_FINDINGS
    print("\nPASS: 场景码检查自测全过（好夹具 0 红，坏夹具红且红集相符）")
    return EXIT_OK


# ---------------------------------------------------------------------------
# 入口
# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "场景码的「定义处 / 引用处」清单 + 「唯一定义」不变式的机器化检查"
            "（台账 D-132 · STAGE-27 §2.3 / §3.3）。"
        ),
        epilog=(
            "退出码：0 = 没报出问题；1 = 报了问题；2 = 工具自身出错。\n"
            "\n"
            "判别力边界（裁决 D）：\n"
            "  本工具**不**判定「场景码住哪」—— 那是**判据 + 人工**（跨模块共享\n"
            "  **或** 前端要按码分派）。它只报**事实**：定义处、引用处、引用跨越了\n"
            "  哪些 crate、前端有没有点名。"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="用**内嵌**夹具自测（好夹具 0 红 / 坏夹具红）",
    )
    parser.add_argument(
        "--root",
        metavar="PATH",
        default=str(REPO_ROOT),
        help=f"要扫的源码树（默认 {REPO_ROOT}）",
    )
    parser.add_argument(
        "--no-listing",
        action="store_true",
        help="不打印清单，只报断言结果",
    )
    args = parser.parse_args(argv)

    try:
        if args.self_test:
            return self_test()
        root = Path(args.root)
        if not root.is_absolute():
            root = (Path.cwd() / root).resolve()
        if not root.is_dir():
            raise ToolError(f"not a directory: {root}")
        report = check_root(root, listing=not args.no_listing)
        if report.findings:
            print(f"\nRESULT: {len(report.findings)} finding(s) reported")
            return EXIT_FINDINGS
        print("\nRESULT: no finding reported")
        return EXIT_OK
    except ToolError as err:
        print(f"error_code_check: {err}", file=sys.stderr)
        return EXIT_TOOL_ERROR


if __name__ == "__main__":
    sys.exit(main())
