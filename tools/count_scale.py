#!/usr/bin/env python3
"""规模数字重算（README / AGENTS.md 里那些数的**唯一**算法）。

用法：python3 tools/count_scale.py

口径（**从当前树重算**，不读任何历史值）：
  契约层     crates/contract/src/**/*.rs        —— 行数合计
  仿真内核   crates/kernel/src/**/*.rs           —— 行数合计（**含**内核宿主装配层）
  内核宿主   crates/kernel/src/kernel_host/**/*.rs —— 行数合计（**是**仿真内核的子集）
  前端       git ls-files front 下 *.js/*.mjs/*.html/*.css —— 行数合计
             ⚠️ **不含** front/assets/（wasm-pack 构建产物，被 git 忽略；
                计进去会让这个数**随「跑没跑过 wasm 构建」变** ⇒ 不可复现）
  文档       docs/ 下 *.html + *.md 的**文件数**（HTML / MD 分列）
  测试       由 `cargo test` 的输出统计（不在本脚本里跑，见 README 门禁表）

⚠️ 前端行数只数**受版本控制**的文件 ⇒ 未提交的新文件**不计入** ⇒
   收口时必须**先提交**再重算，否则 README 里的数会与仓库不一致。
"""
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# ⚠️ **前端分桶规则只有一处定义** ⇒ ⇒ 复用 `tools/front_test_attrib.py` 的
#    `bucket_of`（那是「用例归属」用的**同一套**目录桶）⇒ ⇒ **不写第二份**
#    ⇒ ⇒ 「一个约束一个落点」。
sys.path.insert(0, str(Path(__file__).resolve().parent))
from front_test_attrib import bucket_of  # noqa: E402  (必须在 sys.path 调整之后)


def run(cmd):
    return subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True).stdout


def tracked_glob(pattern):
    out = run(["git", "ls-files", pattern])
    return [f for f in out.splitlines() if f]


def count_lines(paths):
    total = 0
    for p in paths:
        f = ROOT / p
        if f.exists():
            total += len(f.read_text(encoding="utf-8", errors="replace").splitlines())
    return total, len(paths)


def rust_files(under):
    return [str(p.relative_to(ROOT)) for p in sorted((ROOT / under).rglob("*.rs"))]


def front_code_by_bucket(front_src):
    """前端**源码**按目录桶分列行数（台账 §0.1 前端两行的「代码」列）。

    ⚠️ **口径**（台账 §0.1 的口径说明段，v0.53 批起**由本函数跑出来**）：
      * **不含** `front/tests/`（那是**测试**不是源码）
      * **只数** `.js` / `.mjs` / `.html` / `.css`（⇒ ⇒ `front/package.json` 不算）
      * **桶**与 `tools/front_test_attrib.py` 的用例归属**同一套**
        （A = `app.js` / `index.html` / `style.css` / `views/`；B = 其余）
    """
    a_files, b_files = [], []
    for rel in front_src:
        if rel.startswith("front/tests/"):
            continue
        rel_in_front = rel[len("front/"):]
        (a_files if bucket_of(rel_in_front) == "A" else b_files).append(rel)
    a_lines, a_n = count_lines(a_files)
    b_lines, b_n = count_lines(b_files)
    return a_lines, a_n, b_lines, b_n


def main():
    contract = rust_files("crates/contract/src")
    kernel = rust_files("crates/kernel/src")
    host = rust_files("crates/kernel/src/kernel_host")
    front = [f for f in tracked_glob("front")
             if Path(f).suffix in (".js", ".mjs", ".html", ".css")]

    c_lines, c_files = count_lines(contract)
    k_lines, k_files = count_lines(kernel)
    h_lines, h_files = count_lines(host)
    f_lines, f_files = count_lines(front)

    html = [f for f in tracked_glob("docs") if f.endswith(".html")]
    md = [f for f in tracked_glob("docs") if f.endswith(".md")]
    stage = [f for f in tracked_glob("docs/detailed-design")
             if Path(f).name.startswith("STAGE-")]

    print("规模数字（从当前树重算）")
    print(f"  契约层        {c_lines:>7,} 行  （{c_files} 个 .rs）")
    print(f"  仿真内核      {k_lines:>7,} 行  （{k_files} 个 .rs，含内核宿主装配层）")
    print(f"  └ 内核宿主    {h_lines:>7,} 行  （{h_files} 个 .rs，是上面的**子集**）")
    print(f"  前端          {f_lines:>7,} 行  （{f_files} 个 git 跟踪的 .js/.mjs/.html/.css）")
    a_lines, a_n, b_lines, b_n = front_code_by_bucket(front)
    print(f"  └ {'M-01/02/03':<14}{a_lines:>7,} 行  （{a_n} 个文件 · 台账 §0.1「代码」列）")
    print(f"  └ {'M-04/05/07/10':<14}{b_lines:>7,} 行  （{b_n} 个文件 · 台账 §0.1「代码」列）")
    print(f"  文档          {len(html) + len(md):>7,} 篇  （HTML {len(html)} + MD {len(md)}）")
    print(f"  └ 冻结规格    {len(stage):>7,} 份  （STAGE-*）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
