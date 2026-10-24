#!/usr/bin/env python3
"""前端门禁「测试」列的归属口径 —— **台账 §0.1 的唯一算法**。

用法：python3 tools/front_test_attrib.py [--repo <路径>] [--counts <逐文件用例数文件>]

⚠️ **为什么要它**：台账 §0.1 前端两行（M-01/02/03 与 M-04/05/07/10）的「测试」列
   此前是**逐批人工累加**的 ⇒ ⇒ 同一个数在树变过之后仍留着旧值（实测：
   `disasm.test.mjs` 在 v0.20 批整file记作 20 条归 M-02，后来往该文件里追加了
   「mock 内核」等组时**没回算**）⇒ ⇒ 本脚本把它变成**从当前树重算**。

**判据**（台账 §0.1 v0.17 批新立的口径，原文见 `docs/reviews/REVIEWS.md` §0.1 统计口径段）：
  「前端门禁用例住在 `front/tests/`（**不按模块目录分**），故按**被测源码文件所属的
    目录桶**归属——**桶内文件多者胜**（`front/views` + `front/app.js` +
    `front/index.html` + `front/style.css` → **M-01/02/03**；其余 →
    **M-04/05/07/10**）；该规则与每个套件文件头的自述模块**逐条一致**。」

  1. **粒度 = 套件文件级**：取该套件文件引用到的**被测源码文件集合**；
  2. 按**目录桶**归类 ⇒ **桶内文件多者胜**（**文件数**，不是引用次数）；
  3. 平手 ⇒ **自述**裁决（套件文件头 / describe 组标题里的模块标记）；
  4. **跨层**文件（各 describe 组的判定不一致）⇒ 按 describe 组分别记
     （台账 v0.33 批 `board-rules.test.mjs` 的 7 条 service + 7 条 app 先例）；
  5. 仍判不出 ⇒ 记 TIE（**不静默取一边**）。

**两个必须记住的例外**（都是实测出来的，不是拍脑袋）：
  * `front/lib/message.js` = **契约层的 JS 镜像** ⇒ 前端**每一层**的用例都会用它
    ⇒ ⇒ 它**不专属**任何一层 ⇒ ⇒ **不参与判桶**。
  * `front/dev/` 下的 mock 内核 / demo 固件**按目录归 B 桶**
    （台账 v0.17 批自述：`demo-firmware` 9 条归 M-04/05/07/10）⇒ ⇒ **不**因为
    「它是替身」就排除。

**判别力证据**：本口径在 v0.31 / v0.32 / v0.33 三棵树上**逐字复现**台账当时登记的
历史值（220/194 · 227/194 · 234/201）⇒ ⇒ 口径本身不是新造的。
（复现脚本见 `--selftest`，需要那三棵树的 git 提交号。）
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tempfile

BUCKET_A_FILES = {"app.js", "index.html", "style.css"}
BUCKET_A_PREFIX = "views/"

# 跨层共享 ⇒ 不参与判桶
SHARED_FILES = {"lib/message.js"}

# 自述标记 → 桶
SELF_A = ("M-01", "M-02", "M-03", "app 层", "视图")
SELF_B = ("M-04", "M-05", "M-07", "M-10", "service 层", "服务层")

# 历史回归锚点：(提交说明, 期望 A, 期望 B, 期望合计)
# ⚠️ 这三个值**逐字**来自台账 §0.1 的历史登记段 ⇒ 本脚本改坏了就会红。
REGRESSION = [
    ("v0.31", "baed235", 220, 194, 414),
    ("v0.32", "91a452e", 227, 194, 421),
    ("v0.33", "3676c3b", 234, 201, 435),
]


# --------------------------------------------------------------------------- 词法


def strip_code(text):
    """抹掉注释与字符串字面量（**保留长度** ⇒ 偏移不变）。"""
    out = list(text)
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            while i < n and text[i] != "\n":
                out[i] = " "
                i += 1
        elif c == "/" and i + 1 < n and text[i + 1] == "*":
            while i < n and not (text[i] == "*" and i + 1 < n and text[i + 1] == "/"):
                if text[i] != "\n":
                    out[i] = " "
                i += 1
            for _ in range(2):
                if i < n:
                    out[i] = " "
                    i += 1
        elif c in "\"'`":
            q = c
            i += 1
            while i < n:
                if text[i] == "\\":
                    out[i] = " "
                    if i + 1 < n:
                        out[i + 1] = " "
                    i += 2
                    continue
                if text[i] == q:
                    break
                if text[i] != "\n":
                    out[i] = " "
                i += 1
            i += 1
        else:
            i += 1
    return "".join(out)


def match_paren(code, open_idx):
    depth = 0
    for i in range(open_idx, len(code)):
        if code[i] in "({[":
            depth += 1
        elif code[i] in ")}]":
            depth -= 1
            if depth == 0:
                return i
    return -1


def top_level_describes(code, text):
    """顶层 describe 块：(名字, 抹码后的正文, **原文**正文)。"""
    blocks = []
    for mt in re.finditer(r"(^|\n)describe\s*\(", code):
        lp = code.find("(", mt.start(1) + 1)
        if lp < 0:
            continue
        rp = match_paren(code, lp)
        if rp < 0:
            continue
        arrow = code.find("=>", lp)
        if arrow < 0 or arrow > rp:
            continue
        lb = code.find("{", arrow)
        if lb < 0 or lb > rp:
            continue
        rb = match_paren(code, lb)
        if rb < 0:
            continue
        name = text[lp + 1:rp].split(",")[0].strip().strip("\"'`")
        blocks.append((name, code[lb + 1:rb], text[lb + 1:rb]))
    return blocks


def count_tests(code):
    """一条 node:test 用例 = 一次 `test()` 调用。"""
    return len(re.findall(r"(^|[^\w.])test\s*\(", code))


def imports_of(text):
    """源文件 → 该文件在本套件里的**引用形态**（四种写法都认）。"""
    forms = {}

    def add(rel, form):
        forms.setdefault(rel, []).append(form)

    for a, rel in re.findall(
        r"const\s+(\w+)\s*=\s*await\s+importFront\(\s*[\"']([^\"']+)[\"']\s*\)", text
    ):
        add(rel, "member:" + a)
    for names, rel in re.findall(
        r"const\s*\{(.*?)\}\s*=\s*await\s+importFront\(\s*[\"']([^\"']+)[\"']\s*\)", text, re.S
    ):
        for nm in names.split(","):
            nm = nm.strip().split(":")[-1].strip()
            if nm:
                add(rel, "call:" + nm)
    for names, rel in re.findall(
        r"import\s*\{(.*?)\}\s*from\s*[\"']\.\./([^\"']+)[\"']", text, re.S
    ):
        for nm in names.split(","):
            nm = nm.strip().split(" as ")[-1].strip()
            if nm:
                add(rel, "call:" + nm)
    for rel in re.findall(r"resolve\(\s*frontDir\s*,\s*[\"']([^\"']+)[\"']", text):
        add(rel, "path:" + rel)
    return forms


def local_helpers(code):
    """本套件文件里定义的**顶层**本地函数：名字 → 函数体。

    用例常通过本地辅助函数间接碰被测源码 ⇒ 只扫用例正文会漏。
    """
    helpers = {}
    for mt in re.finditer(r"(^|\n)function\s+(\w+)\s*\(", code):
        lb = code.find("{", mt.end())
        if lb < 0:
            continue
        rb = match_paren(code, lb)
        if rb < 0:
            continue
        helpers[mt.group(2)] = code[lb + 1:rb]
    for mt in re.finditer(r"(^|\n)const\s+(\w+)\s*=\s*(?:async\s*)?\([^)]*\)\s*=>\s*\{", code):
        lb = code.find("{", mt.end() - 1)
        rb = match_paren(code, lb)
        if rb < 0:
            continue
        helpers[mt.group(2)] = code[lb + 1:rb]
    return helpers


def referenced_files(body, forms, helpers=None, depth=0, raw=None):
    hit = set()
    for rel, fs in forms.items():
        for f in fs:
            if f.startswith("member:"):
                if re.search(r"\b%s\s*\." % re.escape(f[7:]), body):
                    hit.add(rel)
            elif f.startswith("call:"):
                if re.search(r"\b%s\s*[\(\.]" % re.escape(f[5:]), body):
                    hit.add(rel)
            elif f.startswith("path:"):
                hay = raw if raw is not None else body
                if re.search(r"[\"']%s[\"']" % re.escape(f[5:]), hay):
                    hit.add(rel)
    if helpers and depth < 3:
        for hname, hbody in helpers.items():
            if re.search(r"\b%s\s*\(" % re.escape(hname), body):
                hit |= referenced_files(hbody, forms, helpers, depth + 1, raw)
    return hit


def bucket_of(rel):
    return "A" if (rel in BUCKET_A_FILES or rel.startswith(BUCKET_A_PREFIX)) else "B"


def self_bucket(*texts):
    blob = " ".join(texts)
    a = any(m in blob for m in SELF_A)
    b = any(m in blob for m in SELF_B)
    if a and not b:
        return "A"
    if b and not a:
        return "B"
    return None


def pick_bucket(files, *selftexts):
    if not files:
        return None, {}
    judged = {f for f in files if f not in SHARED_FILES}
    a = sorted(f for f in judged if bucket_of(f) == "A")
    b = sorted(f for f in judged if bucket_of(f) == "B")
    det = {"A": a, "B": b, "shared": sorted(set(files) & SHARED_FILES)}
    if len(a) > len(b):
        return "A", det
    if len(b) > len(a):
        return "B", det
    if not a and not b:
        return None, det
    return self_bucket(*selftexts), det


# --------------------------------------------------------------------------- 主


def attribute(repo, counts):
    """counts = {套件文件名: 用例条数} ⇒ (A, B, TIE, rows)"""
    rows = []
    ta = tb = tt = 0
    for name in sorted(counts):
        path = os.path.join(repo, "front", "tests", name)
        with open(path, encoding="utf-8") as fh:
            text = fh.read()
        code = strip_code(text)
        forms = imports_of(text)
        helpers = local_helpers(code)
        header = "\n".join(text.splitlines()[:40])
        groups = top_level_describes(code, text)
        total = count_tests(code)

        file_pick, file_det = pick_bucket(
            referenced_files(code, forms, helpers, raw=text), header)

        group_rows = []
        assigned = 0
        for gname, gbody, gtext in groups:
            n = count_tests(gbody)
            if n == 0:
                continue
            assigned += n
            gp, gdet = pick_bucket(
                referenced_files(gbody, forms, helpers, raw=gtext), gname, header)
            group_rows.append((gname, n, gp, gdet))

        picks = {gp for _, _, gp, _ in group_rows if gp in ("A", "B")}
        if len(picks) > 1:
            # 跨层 ⇒ 按组分别记
            for gname, n, gp, gdet in group_rows:
                pick = gp or file_pick or self_bucket(gname, header)
                rows.append({"file": name, "group": gname, "tests": n,
                             "pick": pick, "files": gdet})
                ta += n if pick == "A" else 0
                tb += n if pick == "B" else 0
                tt += n if pick not in ("A", "B") else 0
            top = total - assigned
            if top:
                pick = file_pick or self_bucket(header)
                rows.append({"file": name, "group": "<顶层用例>", "tests": top,
                             "pick": pick, "files": file_det})
                ta += top if pick == "A" else 0
                tb += top if pick == "B" else 0
                tt += top if pick not in ("A", "B") else 0
        else:
            pick = (file_pick or (group_rows[0][2] if group_rows else None)
                    or self_bucket(header))
            rows.append({"file": name, "group": "<整file>", "tests": total,
                         "pick": pick, "files": file_det})
            ta += total if pick == "A" else 0
            tb += total if pick == "B" else 0
            tt += total if pick not in ("A", "B") else 0
    return ta, tb, tt, rows


def run_front_gate_counts(repo):
    """逐个跑前端套件，取**权威**的用例条数（TAP 尾部的 `# tests N`）。

    ⚠️ **必须逐文件跑**：`node --test` 整跑虽然也会给总数，但**不按文件分桶**
       ⇒ ⇒ 拿不到「每个文件多少条」⇒ ⇒ 归属算法就没有输入。
    """
    tests_dir = os.path.join(repo, "front", "tests")
    out = {}
    for name in sorted(os.listdir(tests_dir)):
        if not name.endswith(".test.mjs"):
            continue
        proc = subprocess.run(
            ["node", "--test", "--test-reporter=tap",
             os.path.join("front", "tests", name)],
            cwd=repo, capture_output=True, text=True)
        m = re.search(r"^# tests (\d+)", proc.stdout, re.M)
        if not m:
            raise SystemExit("error: 拿不到 %s 的用例数（node --test 失败？）" % name)
        out[name] = int(m.group(1))
    return out


def selftest(repo):
    """判别力回归：在 v0.31 / v0.32 / v0.33 三棵树上**逐字**复现台账历史值。

    ⚠️ 回归用的 worktree 建在**仓外**（`$TMPDIR`）⇒ 不污染仓库目录。
    """
    ok = True
    regress_root = os.path.join(tempfile.gettempdir(), "front-attrib-regress")
    for label, commit, exp_a, exp_b, exp_total in REGRESSION:
        work = os.path.join(regress_root, label)
        if not os.path.isdir(work):
            subprocess.run(["git", "worktree", "add", "--detach", work, commit],
                           cwd=repo, check=True, capture_output=True, text=True)
        counts = run_front_gate_counts(work)
        a, b, t, _ = attribute(work, counts)
        good = (a, b, sum(counts.values())) == (exp_a, exp_b, exp_total)
        ok = ok and good
        print("  %-8s 期望 %d/%d（合计 %d）  实测 %d/%d（合计 %d）  %s" % (
            label, exp_a, exp_b, exp_total, a, b, sum(counts.values()),
            "OK" if good else "★红★"))
    return ok


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=os.path.dirname(os.path.dirname(
        os.path.abspath(__file__))))
    ap.add_argument("--counts", help="逐文件用例数文件（`名字|条数` 每行一条）")
    ap.add_argument("--selftest", action="store_true",
                    help="判别力回归：在 v0.31/v0.32/v0.33 三棵树上复现台账历史值")
    ap.add_argument("--json", help="把逐文件归属明细写到这个文件")
    args = ap.parse_args()

    if args.selftest:
        print("前端归属口径 · 历史回归（判别力证据）")
        return 0 if selftest(args.repo) else 1

    if args.counts:
        counts = {}
        with open(args.counts, encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                name, n = line.split("|")[0], line.split("|")[1]
                if n.isdigit():
                    counts[name] = int(n)
    else:
        counts = run_front_gate_counts(args.repo)

    a, b, t, rows = attribute(args.repo, counts)
    for r in rows:
        if r["pick"] not in ("A", "B"):
            print("TIE/未知: %-28s %-44s %3d" % (r["file"], r["group"][:44], r["tests"]))
    print("前端门禁「测试」列归属（从当前树重算）")
    print("  M-01/02/03      %4d 条" % a)
    print("  M-04/05/07/10   %4d 条" % b)
    print("  TIE（待消解）    %4d 条" % t)
    print("  合计            %4d 条" % (a + b + t))
    if args.json:
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump(rows, fh, ensure_ascii=False, indent=1)
    return 1 if t else 0


if __name__ == "__main__":
    sys.exit(main())
