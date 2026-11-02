#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-49（v0.73 · B2 + G + D1 + D-177 的 C）的**变异自检驱动**（T-1 ~ T-7）。

用法：python3 tools/mutation_v073.py [--only T-3] [--json]

⚠️ **为什么要有这个工具**：规格 §7.2 里的「实测红集」是**长期文档里的数字**
⇒ ⇒ 没有工具的数字 = **手工数字** = **不可复算** ⇒ ⇒ 收口前**先补工具**。

⚠️ **三条纪律**（从 `tools/mutation_v072.py` 继承 ⇒ ⇒ **踩过的坑写成代码**）：

1. **每条变异前还原全部目标文件**（不是「被变异的那个」）⇒ ⇒ 上一条的
   **残留**不会漏进下一条。
2. **每条变异后复跑一次基线** ⇒ ⇒ 基线不绿就**停**。
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
    "crates/contract/src/platform.rs",
    "crates/contract/src/error.rs",
    "crates/kernel/src/kernel_host/platform.rs",
    "crates/kernel/src/kernel_host/mod.rs",
    "front/views/components/memory-map.js",
    "front/lib/message.js",
]

#: ⚠️ **两条门禁命令**（Rust 侧 / 前端侧）。
RUST_GATE = [
    "cargo", "test", "-p", "riscv_sim_kernel", "--lib", "tests_v073",
]
FRONT_GATE = ["node", "--test", "front/tests/memory-map.test.mjs"]

#: ⚠️ **变异表**（T-1 ~ T-7）—— 每条 = (编号, 落点文件, 旧串, 新串, 门禁)。
#:
#: ⚠️ **T-7 是对照**（只改注释）⇒ ⇒ 它**必须**全绿。
MUTATIONS = [
    {
        "id": "T-1",
        "what": "装配期分配改成「把缺省当 0」（**不真的分配**）",
        "file": "crates/kernel/src/kernel_host/platform.rs",
        "old": """        assigned.regions[index].set_base(base as u32);""",
        "new": """        assigned.regions[index].set_base(0);""",
        "gate": "rust",
    },
    {
        "id": "T-2",
        "what": "`base` 缺省时**拒**（不留缺省优先）",
        "file": "crates/contract/src/platform.rs",
        "old": """        if !SUPPORTED_PLATFORM_SCHEMA_VERSIONS.contains(&self.schema_version) {""",
        "new": """        if self.schema_version != PLATFORM_SCHEMA_VERSION {""",
        "gate": "rust",
    },
    {
        "id": "T-3",
        "what": "`timer_riscv` 的窗口抄成 `clint` 的",
        "file": "crates/kernel/src/kernel_host/platform.rs",
        "old": """        DeviceSpec::TimerRiscv { id, irq_line } => BuiltDevice::opaque(
            crate::m15_peripheral::timer::TimerRiscv::new(id.clone(), base, *irq_line),
        ),""",
        "new": """        DeviceSpec::TimerRiscv { id, irq_line } => BuiltDevice::opaque(
            crate::m15_peripheral::clint::Clint::new(id.clone(), base),
        ),""",
        "gate": "rust",
    },
    {
        "id": "T-4",
        "what": "`roles` 缺省时**不**回落到「第一个 X」（一律 `None`）",
        "file": "crates/kernel/src/kernel_host/platform.rs",
        "old": """        console: declared
            .and_then(|r| r.console.clone())
            .or_else(|| devices.first_uart().map(|e| e.id.clone())),""",
        "new": """        console: declared.and_then(|r| r.console.clone()),""",
        "gate": "rust",
    },
    {
        "id": "T-5",
        "what": "`roles` 指了不存在的器件**不拒**（V-20 改成永远通过）",
        "file": "crates/contract/src/platform.rs",
        "old": """                match declared.iter().find(|(did, _)| *did == id) {
                    None => {
                        return Err(PlatformError::new(
                            PLATFORM_FAMILY,
                            E_PLT_18_ROLES_INVALID,
                            format!("角色 {role} 指向的器件 {id} 不在本描述声明的器件集合里"),
                        ));
                    }""",
        "new": """                match declared.iter().find(|(did, _)| *did == id) {
                    None => {
                        continue;
                    }""",
        "gate": "rust",
    },
    {
        "id": "T-6",
        "what": "前端**自动**改写板档案的描述（把缺基址的段**填成 0**）",
        "file": "front/views/components/memory-map.js",
        "old": """            const base = regionBase(region);
            if (kind === "peripheral") {""",
        "new": """            const base = regionBase(region) ?? 0;
            if (kind === "peripheral") {""",
        "gate": "front",
    },
    {
        "id": "T-7",
        "what": "**对照**：只改注释（不许有任何判据变红）",
        "file": "crates/kernel/src/kernel_host/platform.rs",
        "old": """// 装配期分配基址（v0.73 · STAGE-49 · B2）""",
        "new": """// 装配期分配基址（v0.73 · STAGE-49 · B2 · 对照变异）""",
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
    行 ⇒ ⇒ 会把**真红的**判成「零红」⇒ ⇒ **假红**（v0.72 批实测踩到）⇒ ⇒
    **必须扫 `failures:` 块**。
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
                # `    kernel_host::tests_v073_b2::k2_…`
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

    BACKUP_DIR = tempfile.mkdtemp(prefix="mutation-v073-")
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
            print("STAGE-49 变异自检 · 实测红集")
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
