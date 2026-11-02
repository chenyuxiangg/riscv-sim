#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-48（v0.72 · D-177）的**变异自检驱动**（T-1 ~ T-7）。

用法：python3 tools/mutation_v072.py [--only T-3] [--json]

⚠️ **为什么要有这个工具**：规格 §7.2 里的「实测红集」是**长期文档里的数字**
⇒ ⇒ 没有工具的数字 = **手工数字** = **不可复算** ⇒ ⇒ 收口前**先补工具**。

⚠️ **三条纪律**（踩过的坑 ⇒ ⇒ 写成代码而不是注释）：

1. **每条变异前还原全部目标文件**（不是「被变异的那个」）⇒ ⇒ 上一条的
   **残留**不会漏进下一条 ⇒ ⇒ 否则「看起来是对照不对照」。
2. **每条变异后复跑一次基线** ⇒ ⇒ 基线不绿就**停**（并把当前内容打出来）
   ⇒ ⇒ 否则「基线自己红了」会被当成「实现有 bug」。
3. **还原用 `cp` + `md5` + `cmp` + `touch`**（⛔ **不用 `git checkout --`**）
   ⇒ ⇒ 中途被杀也能**手动**还原 ⇒ ⇒ 且文件 mtime 会更新 ⇒ ⇒ 增量构建
   **不会**拿旧产物。

⚠️ **装信号处理**（`SIGTERM` / `SIGINT` / `SIGHUP`）⇒ ⇒ 中途被杀 ⇒ ⇒
**还原后**退出 ⇒ ⇒ 否则「一次 kill」就把后面所有测量变成假的。

⚠️ **判据映射**：Rust 侧的用例名带 `k<N>_` ⇒ ⇒ 映射成 `K-<N>`；
Node 侧的用例名以 `N-<N> ·` 开头 ⇒ ⇒ 映射成 `N-<N>`。
"""

import argparse
import hashlib
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **目标文件**（每条变异前**全部**还原 —— 纪律 ①）。
TARGETS = [
    "crates/kernel/src/kernel_host/platform.rs",
    "crates/kernel/src/kernel_host/mod.rs",
    "front/lib/wiring-report.js",
]

#: ⚠️ **两条门禁命令**（Rust 侧 / 前端侧）。
RUST_GATE = [
    "cargo", "test", "-p", "riscv_sim_kernel", "--lib", "tests_v072_wiring",
]
FRONT_GATE = ["node", "--test", "front/tests/wiring-report.test.mjs"]

#: ⚠️ **变异表**（T-1 ~ T-7）—— 每条 = (编号, 落点文件, 旧串, 新串, 门禁)。
#:
#: ⚠️ **T-7 是对照**（只改注释）⇒ ⇒ 它**必须**全绿。
MUTATIONS = [
    {
        "id": "T-1",
        "what": "「查不到就记一笔」改回「查不到就**静默丢**」",
        "file": "crates/kernel/src/kernel_host/platform.rs",
        "old": """            Err(reason) => dropped.push(DroppedWireDto {
                from_device: wire.from_device.clone(),
                event: wire.event.clone(),
                to_device: wire.to_device.clone(),
                source: wire.source,
                reason,
            }),""",
        "new": """            Err(_reason) => {}""",
        "gate": "rust",
    },
    {
        "id": "T-2",
        "what": "`sync_interrupts` 第 2 步只算**第一个 UART** 的 `rx_ready`",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": """            let Some(uart) = self.devices.uart(&wire.from_device) else {
                continue;
            };""",
        "new": """            let Some(uart) = self
                .devices
                .first_uart()
                .and_then(|entry| self.devices.uart(&entry.id))
            else {
                continue;
            };""",
        "gate": "rust",
    },
    {
        "id": "T-3",
        "what": "`sync_interrupts` 第 3 步只问**第一个 PLIC**",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": """        for plic in self.devices.plic_handles() {""",
        "new": """        for plic in self
            .devices
            .first_plic()
            .into_iter()
            .filter_map(|entry| self.devices.plic(&entry.id))
        {""",
        "gate": "rust",
    },
    {
        "id": "T-4",
        "what": "`declared` 改成数**连线表里**的线（含默认连线）",
        "file": "crates/kernel/src/kernel_host/platform.rs",
        "old": """    let declared = description.wiring.iter().flatten().count() as u32;""",
        "new": """    let declared = entries.len() as u32;""",
        "gate": "rust",
    },
    {
        "id": "T-5",
        "what": "被同键顶掉的那根**不记**（`overridden` 那一档消失）",
        "file": "crates/kernel/src/kernel_host/platform.rs",
        "old": """                if was_written_by_user {
                    shadowed.push(old);
                }""",
        "new": """                let _ = (was_written_by_user, old);""",
        "gate": "rust",
    },
    {
        "id": "T-6",
        "what": "前端**自己重算** `declared`（不显示内核给的那个数）",
        "file": "front/lib/wiring-report.js",
        "old": """        declared: Number(raw.declared ?? 0),""",
        "new": """        declared: Number(Array.isArray(raw.dropped) ? raw.dropped.length : 0),""",
        "gate": "front",
    },
    {
        "id": "T-7",
        "what": "**对照**：只改注释（不许有任何判据变红）",
        "file": "crates/kernel/src/kernel_host/platform.rs",
        "old": """/// ⚠️ **它取代了「三个固定角色句柄」**（裁决 A）⇒ ⇒ 「板上的器件实例」""",
        "new": """/// ⚠️ **它取代了「三个固定角色句柄」**（裁决 A · 对照变异）⇒ ⇒ 「板上的器件实例」""",
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
    """⚠️ **中途被杀 ⇒ ⇒ 还原后退出**（纪律 ③）。"""
    global INTERRUPTED
    INTERRUPTED = True
    restore_all()
    print(f"\n⚠️ 收到信号 {signum} ⇒ 已还原全部目标文件 ⇒ 退出。", file=sys.stderr)
    sys.exit(128 + signum)


def run_gate(gate):
    cmd = RUST_GATE if gate == "rust" else FRONT_GATE
    proc = subprocess.run(
        cmd, cwd=REPO_ROOT, capture_output=True, text=True, timeout=900,
    )
    return proc.stdout + proc.stderr


def reds_from_output(gate, out):
    """门禁输出 ⇒ **红集**（判据编号）。

    ⚠️ ⚠️ **Rust 侧的失败用例在 `failures:` 那个块里**（默认 harness **不是**
    逐行打 `test … FAILED`）⇒ ⇒ 只扫「`test ` 开头且以 `FAILED` 结尾」那种
    行 ⇒ ⇒ 会把**真红的**判成「零红」⇒ ⇒ **假红**（本批实测踩到：T-1 ~ T-5
    明明红了 5 条，脚本却报「零红」）⇒ ⇒ **必须扫 `failures:` 块**。
    """
    reds = set()
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
                # `    kernel_host::tests_v072_wiring::k2_…`
                for part in stripped.split("::"):
                    if part.startswith("k") and "_" in part:
                        num = part[1:].split("_")[0]
                        if num.isdigit():
                            reds.add(f"K-{num}")
                        break
    else:
        for line in out.splitlines():
            stripped = line.strip()
            if stripped.startswith("not ok"):
                # `not ok 3 - N-3 · ……`
                parts = stripped.split("-", 1)
                if len(parts) == 2:
                    head = parts[1].strip()
                    token = head.split("·")[0].strip()
                    if token and token[0] in "NABKX" and "-" in token:
                        reds.add(token)
    return reds


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

    BACKUP_DIR = tempfile.mkdtemp(prefix="mutation-v072-")
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
            reds = reds_from_output(m["gate"], out)
            results.append({
                "id": m["id"],
                "what": m["what"],
                "gate": m["gate"],
                "reds": sorted(reds),
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
            print("STAGE-48 变异自检 · 实测红集")
            for r in results:
                reds = "、".join(r["reds"]) if r["reds"] else "**零红**"
                print(f"  {r['id']} · {r['what']}")
                print(f"      实测红集：{reds}")
        return 0
    finally:
        restore_all()
        shutil.rmtree(BACKUP_DIR, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
