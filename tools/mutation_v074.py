#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-50（v0.74 · 引脚级连线 + 通用连接路由 + 前端连线编辑面）的**变异自检驱动**
（T-1 ~ T-8）。

用法：python3 tools/mutation_v074.py [--only T-3] [--json]

⚠️ **为什么要有这个工具**：规格 §7.2 里的「实测红集」是**长期文档里的数字**
⇒ ⇒ 没有工具的数字 = **手工数字** = **不可复算** ⇒ ⇒ 收口前**先补工具**。

⚠️ **五条纪律**（从 `tools/mutation_v072.py` / `tools/mutation_v073.py` 继承
   ⇒ ⇒ **踩过的坑写成代码**）：

1. **每条变异前还原全部目标文件**（不是「被变异的那个」）⇒ ⇒ 上一条的
   **残留**不会漏进下一条。
2. **每条变异后复跑一次基线** ⇒ ⇒ 基线不绿就**停**。
3. **还原用 `cp` + `md5` + `cmp` + `touch`**（⛔ **不用 `git checkout --`**）
   ⇒ ⇒ 中途被杀也能**手动**还原 ⇒ ⇒ 且文件 mtime 会更新 ⇒ ⇒ 增量构建
   **不会**拿旧产物。
4. **装信号处理**（`SIGTERM` / `SIGINT` / `SIGHUP`）⇒ ⇒ 中途被杀 ⇒ ⇒
   **还原后**退出 ⇒ ⇒ 否则「一次 kill」就把后面所有测量变成假的。
5. **门禁输出 ⇒ 红集**的解析要认得 Rust 的 `failures:` 块（默认 harness
   **不是**逐行打 `test … FAILED`）⇒ ⇒ 否则**真红**会被报成「零红」。

⚠️ **判据映射**：Rust 侧的用例名带 `k<N>_` / `a<N>_` ⇒ ⇒ 映射成 `K-<N>` /
`A-<N>`；Node 侧只取**缩进的那一行**（`not ok` 的**子用例**行 ⇒ ⇒ 不是**套件**
行）⇒ ⇒ 名字开头的 `N-<N>` / `X-<N>` ⇒ ⇒ 映射成本身。
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

#: ⚠️ **目标文件**（每条变异前**全部**还原 —— 纪律 ①）。
TARGETS = [
    "crates/contract/src/platform.rs",
    "crates/kernel/src/kernel_host/platform.rs",
    "crates/kernel/src/kernel_host/mod.rs",
    "front/views/components/memory-map.js",
    "front/views/components/bridge-editor.js",
    "front/lib/message.js",
]

#: ⚠️ **两条门禁命令**（Rust 侧 / 前端侧）。
RUST_GATE = [
    "cargo", "test", "-p", "riscv_sim_kernel", "--lib", "tests_v074",
]
FRONT_GATE = ["node", "--test", "front/tests/bridge-editor.test.mjs"]

#: ⚠️ **变异表**（T-1 ~ T-8）—— 每条 = (编号, 落点文件, 旧串, 新串, 门禁)。
#:
#: ⚠️ **T-8 是对照**（只改注释）⇒ ⇒ 它**必须**全绿。
MUTATIONS = [
    {
        "id": "T-1",
        "what": "连线事件词表**退回单常量比较**（`pin_irq` 一律被拒）",
        "file": "crates/contract/src/platform.rs",
        "old": """pub fn bridge_event_info(event: &str) -> Option<&'static BridgeEventInfo> {
    BRIDGE_EVENTS.iter().find(|info| info.name == event)
}""",
        "new": """pub fn bridge_event_info(event: &str) -> Option<&'static BridgeEventInfo> {
    if event != BRIDGE_EVENT_RX_READY {
        return None;
    }
    BRIDGE_EVENTS.iter().find(|info| info.name == event)
}""",
        "gate": "rust",
    },
    {
        "id": "T-2",
        "what": "引脚 IRQ 改成**电平**语义（**不消费** ⇒ ⇒ 抬了不清 ⇒ ⇒ 活锁）",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": """                    let line = g.take_irq_line_for_pin(pin);""",
        "new": """                    let line = g.irq_line_for_pin(pin);""",
        "gate": "rust",
    },
    {
        "id": "T-3",
        "what": "`pin` **越界不判**（≥ 引脚数也放过）",
        "file": "crates/contract/src/platform.rs",
        "old": """            if let Some(pin) = spec.from.pin {
                if pin as usize >= GPIO_PIN_COUNT {""",
        "new": """            if let Some(pin) = spec.from.pin {
                if false && pin as usize >= GPIO_PIN_COUNT {""",
        "gate": "rust",
    },
    {
        "id": "T-4",
        "what": "引脚级事件的源**不校验是不是 GPIO**（结构要求被删）",
        "file": "crates/contract/src/platform.rs",
        "old": """            if info.needs_pin {
                let source_kind = self""",
        "new": """            if false && info.needs_pin {
                let source_kind = self""",
        "gate": "rust",
    },
    {
        "id": "T-5",
        "what": "给了 `pin` 但事件**不是**引脚级**不判**（`pin` 被静默忽略）",
        "file": "crates/contract/src/platform.rs",
        "old": """            if spec.from.pin.is_some() && !info.needs_pin {""",
        "new": """            if false && spec.from.pin.is_some() && !info.needs_pin {""",
        "gate": "rust",
    },
    {
        "id": "T-6",
        "what": "前端**裁剪**引脚号（越界也放过 ⇒ ⇒ 前端替内核做了判定）",
        "file": "front/views/components/memory-map.js",
        "old": """            const n = Number(pin);
            from.pin = Number.isFinite(n) ? Math.max(0, Math.trunc(n)) : 0;""",
        "new": """            const n = Number(pin);
            from.pin = Number.isFinite(n) ? (Math.max(0, Math.trunc(n)) & 0xff) : 0;""",
        "gate": "front",
    },
    {
        "id": "T-7",
        "what": "前端**不采集**连线（画出来的线**丢**了）",
        "file": "front/views/components/memory-map.js",
        "old": """    return collectBridge(root, next);""",
        "new": """    return next;""",
        "gate": "front",
    },
    {
        "id": "T-8",
        "what": "**对照**：只改注释（不许有任何判据变红）",
        "file": "crates/contract/src/platform.rs",
        "old": """/// **查事件词表** —— 事件名 → 它的信息（**不在词表里 ⇒ `None`**）。""",
        "new": """/// **查事件词表** —— 事件名 → 它的信息（**不在词表里 ⇒ `None`**）· 对照变异。""",
        "gate": "rust",
    },
]

BACKUP_DIR = None
INTERRUPTED = False


def md5(path):
    return hashlib.md5(Path(path).read_bytes()).hexdigest()


def restore_all():
    """**还原全部目标文件**（纪律 ①）⇒ ⇒ 校验 md5。"""
    if BACKUP_DIR is None:
        return
    for rel in TARGETS:
        src = Path(BACKUP_DIR) / rel.replace("/", "__")
        dst = REPO_ROOT / rel
        if not src.exists():
            continue
        shutil.copyfile(src, dst)
        os.utime(dst, None)          # ⇒ mtime 更新 ⇒ ⇒ 增量构建不拿旧产物
        if md5(src) != md5(dst):
            raise SystemExit(f"还原失败（md5 不符）：{rel}")


def on_signal(signum, _frame):
    """⚠️ **中途被杀 ⇒ ⇒ 还原后退出**（纪律 ④）。"""
    global INTERRUPTED
    INTERRUPTED = True
    restore_all()
    print(f"\n⚠️ 收到信号 {signum} ⇒ 已还原全部目标文件 ⇒ 退出。", file=sys.stderr)
    sys.exit(128 + signum)


def run_gate(gate):
    cmd = RUST_GATE if gate == "rust" else FRONT_GATE
    proc = subprocess.run(
        cmd, cwd=REPO_ROOT, capture_output=True, text=True, timeout=1800,
    )
    return proc.stdout + proc.stderr


def reds_from_output(gate, out):
    """门禁输出 ⇒ **红集**（判据编号）。

    ⚠️ ⚠️ **Rust 侧的失败用例在 `failures:` 那个块里**（默认 harness **不是**
    逐行打 `test … FAILED`）⇒ ⇒ 只扫「`test ` 开头且以 `FAILED` 结尾」那种
    行 ⇒ ⇒ 会把**真红的**判成「零红」⇒ ⇒ **假红** ⇒ ⇒ **必须扫 `failures:`
    块**。

    ⚠️ **两个粒度**（**为什么要两个**）：**桶**（`K-7` / `N-4`）是**判据编号**
    ⇒ ⇒ 便于人读；**标题**（用例全名）是**那条判据里具体哪一条用例**⇒ ⇒
    ⚠️ **一个桶里可能有多条用例** ⇒ ⇒ ⇒ 两条**不同的变异**可能落在**同一个桶**
    ⇒ ⇒ ⇒ 只看桶就**分不清「各有专属判据」还是「打的是同一条」**⇒ ⇒ ⇒
    ⇒ ⇒ **细粒度必须一起报**（⇒ ⇒ 「专属判据」那条纪律才判得下去）。
    """
    reds = set()
    titles = []
    if gate == "rust":
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
                # `    kernel_host::tests_v074_pin_routing::k1_…`
                parts = stripped.split("::")
                title = parts[-1].strip()
                for part in parts:
                    m = re.match(r"^([ka])(\d+[a-z]?)_", part)
                    if m:
                        reds.add(f"{m.group(1).upper()}-{m.group(2)}")
                        titles.append(title)
                        break
    else:
        for line in out.splitlines():
            # ⚠️ **只取缩进的那一行** ⇒ ⇒ 那是**子用例**（`not ok 1 - N-4 …`）
            # ⇒ ⇒ 套件行（顶格）**不算**（否则「连带红」会混进来）。
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
    """门禁**是不是编译就挂了**⇒ ⇒ 那**不是**「零红」，是「变异落点不对」。"""
    if gate == "rust":
        return "error[E" in out or "error: could not compile" in out
    return "SyntaxError" in out or "Cannot find module" in out


def green(gate, out):
    if gate == "rust":
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

    BACKUP_DIR = tempfile.mkdtemp(prefix="mutation-v074-")
    try:
        for rel in TARGETS:
            shutil.copyfile(REPO_ROOT / rel, Path(BACKUP_DIR) / rel.replace("/", "__"))

        results = []
        mutations = [m for m in MUTATIONS if not args.only or m["id"] == args.only]
        if args.only and not mutations:
            print(f"✗ 没有编号为 {args.only} 的变异")
            return 1

        for m in mutations:
            # ---- ① 还原**全部**目标文件（纪律 ①）------------------------
            restore_all()
            # ---- ② 基线必须绿（纪律 ②）--------------------------------
            base_out = run_gate(m["gate"])
            if not green(m["gate"], base_out):
                print(f"✗ 基线不绿 ⇒ 停（{m['id']} 之前）")
                print(base_out[-3000:])
                return 1

            # ---- ③ 施加变异 ---------------------------------------------
            target = REPO_ROOT / m["file"]
            text = target.read_text(encoding="utf-8")
            if m["old"] not in text:
                print(f"✗ {m['id']}：落点**找不到**（代码已经变了？）⇒ {m['file']}")
                return 1
            target.write_text(text.replace(m["old"], m["new"], 1), encoding="utf-8")
            os.utime(target, None)

            # ---- ④ 跑门禁 ⇒ ⇒ 红集 ------------------------------------
            out = run_gate(m["gate"])
            # ⚠️ **编译就挂了 ⇒ ⇒ 那不是「零红」** ⇒ ⇒ 是「变异落点不对」
            #   ⇒ ⇒ **必须报出来**（否则会把「编译错」当成「零判别力」）。
            if compile_failed(m["gate"], out):
                print(f"✗ {m['id']}：变异**编译不过** ⇒ ⇒ 落点写错了（不是零红）")
                print(out[-3000:])
                return 1
            reds, titles = reds_from_output(m["gate"], out)
            results.append({
                "id": m["id"],
                "what": m["what"],
                "gate": m["gate"],
                "reds": sorted(reds),
                "titles": sorted(set(titles)),
            })

            # ---- ⑤ 还原 + 复跑基线（纪律 ②）----------------------------
            restore_all()
            again = run_gate(m["gate"])
            if not green(m["gate"], again):
                print(f"✗ 还原后基线**不绿** ⇒ 停（{m['id']} 之后）⇒ ⇒ 有残留")
                print(again[-3000:])
                return 1

        if args.json:
            print(json.dumps(results, ensure_ascii=False, indent=2))
        else:
            print("STAGE-50 变异自检 · 实测红集")
            for r in results:
                reds = "、".join(r["reds"]) if r["reds"] else "**零红**"
                print(f"  {r['id']} · {r['what']}")
                print(f"      实测红集：{reds}")
                if len(r["reds"]) >= 1:
                    print("      落到哪几条用例：")
                    for t in r["titles"]:
                        print(f"        - {t}")
        return 0
    finally:
        restore_all()
        shutil.rmtree(BACKUP_DIR, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
