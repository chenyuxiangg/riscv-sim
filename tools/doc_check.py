#!/usr/bin/env python3
"""Markdown 文档自检（七项）。

用法：python3 tools/doc_check.py <file.md> [更多文件...]

检查项：
    1. Markdown 表格列数一致（按 GitHub 的切法：原始 `|` 切 ⇒ 代码里的 `|` 也会切，与渲染一致）
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 表格分隔行 `|---|` 与表头列数一致
    5. 行内代码的反引号成对（**跨行的 code span 合法** ⇒ 按整篇文档配对，不按行）
    6. 左右括号配平（**在剥掉行内代码之后**统计 ⇒ 代码里的括号不算）
    7. GitHub 风格 slug 锚点唯一（标题重复会生成 `#a` `#a-1` 后缀）

退出码：0 = 无失败；1 = 有失败。
"""
import re
import sys
from pathlib import Path

fail = []


def err(path, msg):
    fail.append(f"{path}: {msg}")


def split_fences(lines):
    """返回 (围栏外的行, 围栏内的行标记)。围栏块内容整块剔除。"""
    out = []
    in_fence = False
    for line in lines:
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            out.append(None)          # 围栏行本身不算正文
            continue
        out.append(None if in_fence else line)
    return out


def strip_code_spans(body_lines):
    """按**整篇文档**剥掉行内代码 span（code span 可跨行 ⇒ 状态要跨行传递）。

    返回 (剥掉后的行列表, 未闭合 span 的起始行号列表)。
    """
    out = []
    unclosed = []
    in_span = False
    open_at = 0
    first_open_at = 0
    for idx, line in enumerate(body_lines, 1):
        if line is None:
            out.append(None)
            continue
        s = line
        if in_span:
            j = s.find("`")
            if j == -1:
                out.append("")
                continue
            s = s[j + 1:]
            in_span = False
        while True:
            i = s.find("`")
            if i == -1:
                break
            j = s.find("`", i + 1)
            if j == -1:
                s = s[:i]
                if not in_span:
                    open_at = idx
                    if not first_open_at:
                        first_open_at = idx
                in_span = True
                break
            s = s[:i] + s[j + 1:]
        out.append(s)
    if in_span:
        # 报**第一个**开启未闭合 span 的行 ⇒ 定位到真正的出错处，
        # 而不是被这个 span 一路吞下去的最后一行。
        unclosed.append(first_open_at or open_at)
    return out, unclosed


def split_row(s):
    r"""按**未转义**的 `|` 切表格行。

    ⚠️ GitHub 的表格里 `\|` 是**字面竖线**（不切列）
    ⇒ 朴素 `split("|")` 会把它误切成一列 ⇒ 误报。
    """
    cells = re.split(r"(?<!\\)\|", s.strip().strip("|"))
    return cells


def check_table_cols(path, lines):
    start = None
    cols = None
    for i, line in enumerate(lines, 1):
        if line is None:
            continue
        s = line.strip()
        if s.startswith("|") and s.endswith("|"):
            c = len(split_row(s))
            if start is None:
                start, cols = i, c
            elif c != cols:
                err(path, f"[表格] 第 {i} 行列数 {c} ≠ 首行 {cols}（表起于第 {start} 行）")
        else:
            start, cols = None, None


def check_heading_jump(path, lines):
    prev = 0
    for i, line in enumerate(lines, 1):
        if line is None:
            continue
        m = re.match(r"^(#{1,6})\s+\S", line)
        if not m:
            continue
        lvl = len(m.group(1))
        if prev and lvl > prev + 1:
            err(path, f"[标题] 第 {i} 行从 h{prev} 跳到 h{lvl}")
        prev = lvl


def check_fence(path, raw):
    n = sum(1 for l in raw if l.lstrip().startswith("```"))
    if n % 2 != 0:
        err(path, f"[围栏] 围栏数 {n} 为奇数 ⇒ 未配平")


def check_separator(path, lines):
    """表格的 `|---|---|` 分隔行列数必须与表头一致（Markdown 渲染要求）。"""
    for i, line in enumerate(lines, 1):
        if line is None:
            continue
        s = line.strip()
        if not re.fullmatch(r"\|[\s:-]+\|", s):
            continue
        if i == 1:
            continue
        head = lines[i - 2]
        head = head.strip() if head else ""
        if not (head.startswith("|") and head.endswith("|")):
            err(path, f"[分隔行] 第 {i} 行的分隔行上面不是表头")
            continue
        hc = len(split_row(head))
        sc = len(split_row(s))
        if hc != sc:
            err(path, f"[分隔行] 第 {i} 行列数 {sc} ≠ 表头 {hc}")


def check_backticks(path, body):
    """行内代码的反引号成对。**跨行的 code span 合法** ⇒ 状态跨行传递。"""
    _, unclosed = strip_code_spans(body)
    for at in unclosed:
        err(path, f"[反引号] 第 {at} 行起的行内代码没有闭合")


def check_parens(path, body):
    """左右括号配平（剥掉行内代码之后统计 ⇒ 代码里的括号不算）。

    ⚠️ 按**段落**统计而不是按行 ⇒ 散文里跨行的括号
    （例如 `(the scheduler may` 换行 `yield there)`）是**正常**的。
    """
    para = []
    start = 0

    def flush(para, start):
        if not para:
            return
        text = "\n".join(para)
        if text.count("(") != text.count(")"):
            err(path, f"[括号] 第 {start} 行起的段落左右括号数不等"
                      f"（{text.count('(')} vs {text.count(')')}）")

    for i, line in enumerate(body, 1):
        if line is None:
            continue
        if line.strip() == "":
            flush(para, start)
            para, start = [], 0
            continue
        if not para:
            start = i
        para.append(line)
    flush(para, start)


def slugify(title):
    t = title.strip().lower()
    t = re.sub(r"[`*_]", "", t)
    t = re.sub(r"[^\w一-鿿\- ]", "", t)
    t = t.replace(" ", "-")
    return t


def check_slugs(path, body):
    seen = {}
    for i, line in enumerate(body, 1):
        if line is None:
            continue
        m = re.match(r"^#{1,6}\s+(.*)$", line)
        if not m:
            continue
        s = slugify(m.group(1))
        if s in seen:
            err(path, f"[锚点] 第 {i} 行标题「{m.group(1).strip()}」与第 {seen[s]} 行重复 ⇒ slug 冲突（GitHub 会加 -1 后缀）")
        else:
            seen[s] = i


def normalize(msg):
    """把行号抹掉 ⇒ 基线只跟踪**问题的种类**，不跟踪位置。

    ⇒ 文档被编辑导致行号漂移时，历史欠账**不会**冒成新红灯；
    但**同一种问题**在新位置出现**仍会被认出来**（因为消息文本一致）。
    """
    return re.sub(r"第 \d+ 行", "第 ? 行", msg)


def main():
    args = sys.argv[1:]
    save_to = None
    baseline = None
    files = []
    i = 0
    while i < len(args):
        a = args[i]
        if a == "--save-baseline":
            i += 1
            save_to = args[i]
        elif a == "--baseline":
            i += 1
            baseline = args[i]
        else:
            files.append(a)
        i += 1
    if not files:
        print(__doc__)
        return 2

    for arg in files:
        p = Path(arg)
        if not p.exists():
            err(str(p), "[文件] 不存在")
            continue
        raw = p.read_text(encoding="utf-8").splitlines()
        body = split_fences(raw)
        check_table_cols(p.name, body)
        check_heading_jump(p.name, body)
        check_fence(p.name, raw)
        check_separator(p.name, body)
        check_backticks(p.name, body)
        stripped, _ = strip_code_spans(body)
        check_parens(p.name, stripped)
        check_slugs(p.name, body)

    if save_to:
        Path(save_to).write_text(
            "\n".join(sorted(normalize(f) for f in fail)) + ("\n" if fail else ""),
            encoding="utf-8")
        print(f"检查项：7  文件 {len(files)}  失败 {len(fail)}  → 基线已写入 {save_to}")
        for f in fail:
            print("  ·", f)
        return 0

    if baseline:
        known = set()
        bp = Path(baseline)
        if bp.exists():
            known = {l.strip() for l in bp.read_text(encoding="utf-8").splitlines() if l.strip()}
        new = [f for f in fail if normalize(f) not in known]
        print(f"检查项：7  文件 {len(files)}  失败 {len(fail)}（基线 {len(known)} 条）"
              f"  新增 {len(new)}")
        for f in new:
            print("  ✗", f)
        return 1 if new else 0

    print(f"检查项：7  文件 {len(files)}  失败 {len(fail)}")
    for f in fail:
        print("  ✗", f)
    return 1 if fail else 0


if __name__ == "__main__":
    sys.exit(main())
