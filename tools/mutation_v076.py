#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-52（v0.76 · 模板导入不兜底 + 引脚复用/pinmux）的**变异自检驱动**
（T-1 ~ T-7 + 顺手整改 N-4）。

用法：python3 tools/mutation_v076.py [--only T-3] [--json]

⚠️ **为什么要有这个工具**：规格 §7.2 里的「实测红集」是**长期文档里的数字**
⇒ ⇒ 没有工具的数字 = **手工数字** = **不可复算** ⇒ ⇒ 收口前**先补工具**。

⚠️ **五条纪律**（从 `tools/mutation_v075.py` 继承）：

1. **每条变异前还原全部目标文件**（不是「被变异的那个」）。
2. **每条变异后复跑一次基线** ⇒ ⇒ 基线不绿就**停**。
3. **还原用 `cp` + `md5` + `cmp` + `touch`**（⛔ **不用 `git checkout --`**）。
4. **装信号处理**（`SIGTERM` / `SIGINT` / `SIGHUP`）。
5. **门禁输出 ⇒ 红集**的解析要认得 Rust 的 `failures:` 块。

⚠️ **本批变异表**（规格 §5）：
  - T-1 pinmux 不接线（路由表空转）⇒ 红 **K-4**
  - T-2 中断绑引脚不绑功能（`first()` 不切）⇒ 红 **K-5**
  - T-3 冲突校验删（两源不一致也放行）⇒ 红 **K-7**
  - T-4 无板装载不拒（自动选档假复活）⇒ 红 **K-1**（B-169 同源）
  - T-5 未知系统判据失效 ⇒ 红 **K-3**
  - T-6 方向枚举 rename 破坏（合法值也拒）⇒ 红 **A-4**（a4b 合法面）
  - T-7 对照（只改注释）⇒ **零红**
  - N-4 前端退回「自动选择生效板」横幅 ⇒ 红 **N-3**
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

TARGETS = [
    "crates/kernel/src/kernel_host/mod.rs",
    "crates/contract/src/platform.rs",
    "front/views/components/memory-map.js",
]

RUST_GATE = [
    "cargo", "test", "-p", "riscv_sim_kernel", "--lib", "tests_v076",
]
CONTRACT_GATE = [
    "cargo", "test", "-p", "riscv_sim_contract", "--lib", "a4b",
]
FRONT_GATE = ["node", "--test", "front/tests/memory-map-templates.test.mjs"]

MUTATIONS = [
    {
        "id": "T-1",
        "what": "pinmux 不接线（2.5 分支路由表空转 ⇒ 引脚活动没人抬源）",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": "        for (gpio_id, pin, functions) in self.pinmux_routes.iter() {",
        "new": "        for (gpio_id, pin, functions) in self.pinmux_routes.iter().take(0) {",
        "gate": "rust",
    },
    {
        "id": "T-2",
        "what": "中断绑引脚不绑功能（永远取第一个功能 ⇒ 切功能不换源）",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": "            let Some(fn_spec) = functions.get(current as usize) else {",
        "new": "            let Some(fn_spec) = functions.first() else {",
        "gate": "rust",
    },
    {
        "id": "T-3",
        "what": "冲突校验删掉（两源不一致也放行）",
        "file": "crates/contract/src/platform.rs",
        "old": "                                if *wired != irq.source {",
        "new": "                                if false && *wired != irq.source {",
        "gate": "rust",
    },
    {
        "id": "T-4",
        "what": "无板装载不拒（B1 失效 ⇒ 自动选档假复活）",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": """            None => {
                let detail = "没有生效板：这台机器还没配置板 —— 请先从模板导入 \\
                     （default / minimal / qemu-virt）或配置一块板再装载"
                    .to_string();
                return vec![self.error_from(&PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_22_NO_BOARD,
                    detail,
                ))];
            }""",
        "new": """            None => {
                // ⚠️ 变异 T-4：不拒（自动选档链路假复活 ⇒ ⇒ B1 失效）。
                let detail = "没有生效板：这台机器还没配置板 —— 请先从模板导入 \\
                     （default / minimal / qemu-virt）或配置一块板再装载"
                    .to_string();
                if false {
                    return vec![self.error_from(&PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_22_NO_BOARD,
                        detail,
                    ))];
                }
            }""",
        "gate": "rust",
    },
    {
        "id": "T-5",
        "what": "未知系统门禁判据失效（落不在也放行）",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": "                if verdict.missing {",
        "new": "                if false && verdict.missing {",
        "gate": "rust",
    },
    {
        "id": "T-6",
        "what": "方向枚举 rename 破坏（合法小写值也被拒 ⇒ 封闭表错乱）",
        "file": "crates/contract/src/platform.rs",
        "old": """#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinDirection {""",
        "new": """#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PinDirection {""",
        "gate": "contract",
    },
    {
        "id": "T-7",
        "what": "**对照**：只改注释（不许有任何判据变红）",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": "        // ⚠️ **MVP 取第一个 pinmux 器件**（裁决 C1：所有 pinmux 器件同一份",
        "new": "        // ⚠️ **MVP 取第一个 pinmux 器件**（裁决 C1：所有 pinmux 器件同一份）· 对照变异 T-7。",
        "gate": "rust",
    },
    {
        "id": "N-4",
        "what": "顺手整改反向：无板引导面退回「自动选择生效板」横幅（B3 文案复活）",
        "file": "front/views/components/memory-map.js",
        "old": "                请先从这里导入一块或配置一块板。",
        "new": "                请先从这里导入一块或配置一块板。机器将自动选择一块生效板。",
        "gate": "front",
    },
]

BACKUP_DIR = None
INTERRUPTED = False


def md5(path):
    return hashlib.md5(Path(path).read_bytes()).hexdigest()


def restore_all():
    if BACKUP_DIR is None:
        return
    for rel in TARGETS:
        src = Path(BACKUP_DIR) / rel.replace("/", "__")
        dst = REPO_ROOT / rel
        if not src.exists():
            continue
        shutil.copyfile(src, dst)
        os.utime(dst, None)
        if md5(src) != md5(dst):
            raise SystemExit(f"还原失败（md5 不符）：{rel}")


def on_signal(signum, _frame):
    global INTERRUPTED
    INTERRUPTED = True
    restore_all()
    print(f"\n⚠️ 收到信号 {signum} ⇒ 已还原全部目标文件 ⇒ 退出。", file=sys.stderr)
    sys.exit(128 + signum)


def run_gate(gate):
    if gate == "rust":
        cmd = RUST_GATE
    elif gate == "contract":
        cmd = CONTRACT_GATE
    else:
        cmd = FRONT_GATE
    proc = subprocess.run(
        cmd, cwd=REPO_ROOT, capture_output=True, text=True, timeout=1800,
    )
    return proc.stdout + proc.stderr


def reds_from_output(gate, out):
    """门禁输出 ⇒ 红集（判据编号）。

    ⚠️ Rust 侧失败用例在 `failures:` 块里 ⇒ ⇒ 必须扫那个块。
    ⚠️ 两个粒度：**桶**（`K-4` / `A-4`）判据编号 + **标题**（具体用例）。
    ⚠️ 模板套件的 K-1 / M-1 / M-2 / M-3 用例名**不带** `k1_`/`m1_` 前缀
    （`loading_without_a_board_...` 等描述式命名）⇒ ⇒ 靠**已知映射表**补桶。
    """
    KNOWN = {
        "loading_without_a_board_is_rejected_with_the_template_guidance": "K-1",
        "the_template_list_reports_the_three_builtins_sorted_by_priority": "M-1",
        "getting_a_template_returns_its_full_description": "M-2",
        "an_unknown_template_id_is_rejected_with_e_plt_22": "M-3",
    }
    reds = set()
    titles = []
    if gate in ("rust", "contract"):
        in_failures = False
        for line in out.splitlines():
            stripped = line.strip()
            if stripped == "failures:":
                in_failures = True
                continue
            if in_failures:
                if stripped.startswith("test result:"):
                    break
                if not stripped:
                    continue
                parts = stripped.split("::")
                title = parts[-1].strip()
                bucket = KNOWN.get(title)
                if bucket is None:
                    for part in parts:
                        m = re.match(r"^([ka])(\d+)([a-z]*)_", part)
                        if m:
                            bucket = f"{m.group(1).upper()}-{m.group(2)}"
                            break
                if bucket is not None:
                    reds.add(bucket)
                    titles.append(title)
    else:
        for line in out.splitlines():
            if not line.startswith("    "):
                continue
            stripped = line.strip()
            if not stripped.startswith("not ok"):
                continue
            head = stripped.split("-", 1)[1].strip() if "-" in stripped else ""
            m = re.match(r"^([NX])-(\d+)", head)
            if m:
                reds.add(f"{m.group(1)}-{m.group(2)}")
                titles.append(head)
    return reds, titles


def compile_failed(gate, out):
    if gate in ("rust", "contract"):
        return "error[E" in out or "error: could not compile" in out
    return "SyntaxError" in out or "Cannot find module" in out


def green(gate, out):
    if gate in ("rust", "contract"):
        return "test result: ok." in out and "FAILED" not in out
    return "# fail 0" in out


def main():
    global BACKUP_DIR
    parser = argparse.ArgumentParser()
    parser.add_argument("--only", default=None, help="只跑某一条（如 T-3）")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(sig, on_signal)

    BACKUP_DIR = tempfile.mkdtemp(prefix="mutation-v076-")
    try:
        for rel in TARGETS:
            shutil.copyfile(REPO_ROOT / rel, Path(BACKUP_DIR) / rel.replace("/", "__"))

        results = []
        mutations = [m for m in MUTATIONS if not args.only or m["id"] == args.only]
        if args.only and not mutations:
            print(f"✗ 没有编号为 {args.only} 的变异")
            return 1

        for m in mutations:
            restore_all()
            # 基线必须绿（纪律 ②）
            base_out = run_gate(m["gate"])
            if not green(m["gate"], base_out):
                print(f"✗ {m['id']}：基线不绿（{m['gate']}）⇒ ⇒ 停")
                restore_all()
                return 1

            f = REPO_ROOT / m["file"]
            src = f.read_text(encoding="utf-8")
            if m["old"] not in src:
                print(f"✗ {m['id']}：落点找不到 ⇒ ⇒ 变异落点不对，停")
                restore_all()
                return 1
            f.write_text(src.replace(m["old"], m["new"], 1), encoding="utf-8")

            out = run_gate(m["gate"])
            reds, titles = reds_from_output(m["gate"], out)

            if compile_failed(m["gate"], out):
                print(f"✗ {m['id']}：变异导致编译失败 ⇒ ⇒ 落点不对，停")
                restore_all()
                return 1

            expected = {
                "T-1": {"K-4"},
                "T-2": {"K-5"},
                "T-3": {"K-7"},
                "T-4": {"K-1"},
                "T-5": {"K-3"},
                "T-6": {"A-4"},
                "T-7": set(),
                "N-4": {"N-3"},
            }[m["id"]]
            # ⚠️ 判别力口径：**预期红集 ⊆ 实测红集**（同一判据被多条变异
            #   「连带」击杀是合理的 —— 变异的落点不同、红集可重叠）＋
            #   **至少红一条**（变异真的改变了行为）。T-7 对照必须全绿。
            if m["id"] == "T-7":
                ok = green(m["gate"], out)
                if not ok:
                    print(f"✗ T-7：对照变异居然红了（{sorted(reds)}）⇒ ⇒ 停")
                    restore_all()
                    return 1
            elif not expected.issubset(reds) or green(m["gate"], out):
                print(
                    f"✗ {m['id']}：预期红集 {sorted(expected)} ⊄ 实测 {sorted(reds)}"
                    f"（标题：{titles[:4]}）⇒ ⇒ 判别力不对，停"
                )
                restore_all()
                return 1

            results.append({
                "id": m["id"],
                "ok": True,
                "reds": sorted(reds),
                "titles": titles,
            })
            print(f"  ✓ {m['id']} 红集 = {sorted(reds)}（{m['what']}）")

        restore_all()
        if args.json:
            print(json.dumps(results, ensure_ascii=False, indent=2))
        else:
            print(f"\n变异自检（STAGE-52）：{len(results)} 条全过")
            for r in results:
                print(f"  {r['id']}: {r['reds']}")
        return 0
    finally:
        restore_all()


if __name__ == "__main__":
    sys.exit(main())
