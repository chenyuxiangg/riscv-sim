#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-51（v0.75 · MCU 概念 + ISA 门禁 + 前端顺手整改）的**变异自检驱动**
（T-1 ~ T-5 + 顺手整改 N-4）。

用法：python3 tools/mutation_v075.py [--only T-3] [--json]

⚠️ **为什么要有这个工具**：规格 §7.2 里的「实测红集」是**长期文档里的数字**
⇒ ⇒ 没有工具的数字 = **手工数字** = **不可复算** ⇒ ⇒ 收口前**先补工具**。

⚠️ **五条纪律**（从 `tools/mutation_v074.py` 继承 ⇒ ⇒ **踩过的坑写成代码**）：

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

⚠️ **判据映射**：Rust 侧用例名带 `k<N>_` / `a<N>_`（含 `k1b_` / `a3b_` 这种
子编号）⇒ 映射成 `K-<N>` / `A-<N>`；Node 侧只取**缩进的那一行**（`not ok` 的
**子用例**行 ⇒ ⇒ 不是**套件**行）⇒ ⇒ 名字开头的 `N-<N>` ⇒ ⇒ 映射成本身。

⚠️ **本批变异表**（规格 §5）：
  - T-1 isa 解析退回前缀（c 位判据删掉）⇒ 红 **K-3**
  - T-2 缺省（无 mcu）也校验 ⇒ 红 **K-4**
  - T-3 class 判据删掉 ⇒ 红 **K-2**
  - T-4 契约词法只判前缀 ⇒ 红 **A-3**（实测可能连带 A-3b）
  - T-5 对照（只改注释）⇒ **零红**
  - N-4 顺手整改变异：前端「不自动实例化」被删掉 ⇒ 红 **N-4**
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
    "crates/kernel/src/kernel_host/mod.rs",
    "crates/contract/src/platform.rs",
    "front/views/components/memory-map.js",
]

#: ⚠️ **两条门禁命令**（Rust 侧 / 前端侧）。
RUST_GATE = [
    "cargo", "test", "-p", "riscv_sim_kernel", "--lib", "tests_v075",
]
FRONT_GATE = ["node", "--test", "front/tests/memory-map-mcu.test.mjs"]

#: ⚠️ **变异表** —— 每条 = (编号, 落点文件, 旧串, 新串, 门禁)。
#:
#: ⚠️ **T-5 是对照**（只改注释）⇒ ⇒ 它**必须**全绿。
MUTATIONS = [
    {
        "id": "T-1",
        "what": "isa 解析退回前缀（c 位判据删掉 ⇒ rv32i 板 + RVC 固件也放行）",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": "            if !info.has_c && p.elf_isa.has_rvc {",
        "new": "            if false && !info.has_c && p.elf_isa.has_rvc {",
        "gate": "rust",
    },
    {
        "id": "T-2",
        "what": "缺省（无 mcu）也校验（拿默认 rv32i 查 ⇒ 存量无 mcu 板被查）",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": "        let mcu = board.mcu.as_ref()?;",
        "new": """        let default_mcu = riscv_sim_contract::platform::McuSpec {
            isa: "rv32i".to_string(),
            vendor: None,
            model: None,
            vector_table: None,
        };
        let mcu = board.mcu.as_ref().unwrap_or(&default_mcu);""",
        "gate": "rust",
    },
    {
        "id": "T-3",
        "what": "class 判据删掉（rv64 声明 + ELF32 固件也放行）",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": "            if p.elf_isa.bits != info.bits {",
        "new": "            if false && p.elf_isa.bits != info.bits {",
        "gate": "rust",
    },
    {
        "id": "T-4",
        "what": "契约 isa 词法改成**只判前缀**（\"rv32x\" 也放行 ⇒ V-23 失守）",
        "file": "crates/contract/src/platform.rs",
        "old": "    let order: &[char] = &['i', 'e', 'm', 'a', 'f', 'd', 'c'];",
        "new": "    let order: &[char] = &['i', 'e', 'm', 'a', 'f', 'd', 'c', 'x'];",
        "gate": "rust",
    },
    {
        "id": "T-5",
        "what": "**对照**：只改注释（不许有任何判据变红）",
        "file": "crates/kernel/src/kernel_host/mod.rs",
        "old": "    /// **ISA 门禁的单一落点**（v0.75 · STAGE-51 · D-175 的 F）。",
        "new": "    /// **ISA 门禁的单一落点**（v0.75 · STAGE-51 · D-175 的 F）· 对照变异 T-5。",
        "gate": "rust",
    },
    {
        "id": "N-4",
        "what": "顺手整改被删：前端退回「把生效板描述当草稿」（加载固件自动实例化）",
        "file": "front/views/components/memory-map.js",
        "old": """export function currentDraft(state) {
    return state?.draft ?? defaultDraft();
}""",
        "new": """export function currentDraft(state) {
    if (state?.draft != null) return state.draft;
    const desc = state?.memoryMap?.description;
    return desc && typeof desc === "object" ? draftFromDescription(desc) : defaultDraft();
}""",
        "gate": "front",
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

    ⚠️ **Rust 侧的失败用例在 `failures:` 那个块里**（默认 harness **不是**
    逐行打 `test … FAILED`）⇒ ⇒ 只扫「`test ` 开头且以 `FAILED` 结尾」那种
    行 ⇒ ⇒ 会把**真红的**判成「零红」⇒ ⇒ **假红** ⇒ ⇒ **必须扫 `failures:`
    块**。

    ⚠️ **两个粒度**：**桶**（`K-3` / `N-4`）是**判据编号** ⇒ ⇒ 便于人读；
    **标题**（用例全名）是那条判据里具体哪一条用例 ⇒ ⇒ ⚠️ 一个桶里可能有
    **多条用例** ⇒ ⇒ 两条**不同的变异**可能落在**同一个桶** ⇒ ⇒ 细粒度必须
    一起报（「专属判据」那条纪律才判得下去）。
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
                # `    kernel_host::tests_v075_mcu::k1_…`
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

    BACKUP_DIR = tempfile.mkdtemp(prefix="mutation-v075-")
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
            print("STAGE-51 变异自检 · 实测红集")
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
