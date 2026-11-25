#!/usr/bin/env bash
# boot-osz.sh —— 把一个真实 M 模式 RTOS（默认 osz）在**本平台**上跑起来，
# 并与 **Spike 参考侧**的 console 字节流**逐字节**比对。
#
# ## 为什么要有这个脚本
#
# 黄金对拍的老路（tools/golden-run.sh）给 **riscv-tests** 用：它要求镜像里
# 有一个 `tohost` 符号（HTIF 约定），跑到底后 guest 往 tohost 写判决码。
# **真实 RTOS 没有 tohost** —— osz 的「判决」是它自己打到 UART0 上的字节。
#
# ⇒ 这里的判决口径是「**两侧 UART TX 字节流逐字节相同**」。
#
# ## 候选侧只有**一条通道**：`POST <server>/api/boot`（R-12）
#
# ⚠️ **仓内只保留一份判定口径**：候选侧不再有「离线跑内核」的分支
#   （`osz-boot` 已改成该端点的**客户端**）⇒ ⇒ 板的选择、跑到哪算停、预算
#   与墙钟全在服务端（`RunSpec` + 内核 `run_until`）⇒ ⇒ 本脚本**不**在
#   这里再判一次。
#
# ## 内存图：RAM 窗口的**单一真相源 = 服务端那份平台描述**
#
# ⚠️ 候选侧把**它那块板的** RAM 窗口经回执头 `x-boot-ram-window`
#   （`osz-boot --print-memory`）交出来 ⇒ ⇒ 本脚本**读**那一行、**不**手抄
#   地址 ⇒ ⇒ 描述改了高段大小 ⇒ ⇒ 两边**自动**跟着变 ⇒ ⇒ 不可能漂
#   （STAGE-19 裁决 I）。
#
# ⚠️ **栈页是参考侧的特性**：Spike 的初始栈页留在脚本里（候选侧的栈窗口
#   是另一段）⇒ ⇒ 「两侧暴露同一地址集合」这句**不准确** ⇒ ⇒ 诚实口径：
#   **RAM 窗口**两侧一致（构造保证）· **栈窗口**各是本侧特性。
#
# ⚠️ **覆盖口**：`SPIKE_MEM` 环境变量仍然可以**整个替换** `-m` 的值 ⇒ ⇒
#   给了就**不**用候选侧打印的值。
#
# ## 用法
#
#   tools/boot-osz.sh [--elf <path>] [--spike <cmd>] [--timeout-ms <n>]
#                      [--server <url>] [--board <id>]
#
#   --elf <path>    要跑的 ELF（默认 $OSZ_ELF，其次 /tmp/osz-artifacts/osz.elf）
#   --spike <cmd>   Spike 可执行文件（默认 spike）
#   --timeout-ms    候选侧的墙钟上限（默认 60000；参考侧自动 +30 s）
#   --server <url>  服务端地址（默认 http://127.0.0.1:9876）
#                   ⚠️ 该服务必须以 `--allow-image-root` 覆盖 ELF 所在目录
#                      起法：cargo run -p riscv_sim_server -- --allow-image-root <dir>
#   --board <id>    板模板 id（默认 qemu-virt —— osz 要 UART0 与 0x8000_0000
#                   的 RAM；`default` 板上没有 UART）
#
# ## 退出码
#
#   0  两侧逐字节相同
#   1  用法错误 / 缺 ELF / 缺 Spike / 连不上服务
#   2  两侧不一致
#   3  某一侧没跑出判决
#
# ⚠️ **本脚本不进默认门禁**：它需要仓外的 osz ELF、本机的 Spike 与一个起着的
#   服务端。仓内只进脚本与规格（判决的**规则**进仓，判决的**料**不进仓）。

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

ELF="${OSZ_ELF:-/tmp/osz-artifacts/osz.elf}"
SPIKE="${SPIKE:-spike}"
TIMEOUT_MS=60000
# 候选侧的**唯一**通道（R-12）。
SERVER="${OSZ_SERVER:-http://127.0.0.1:9876}"
BOARD="${OSZ_BOARD:-qemu-virt}"

# Spike 的 ISA 串与内存图（v0.23 批实测确定，**不要**随便改）。
#
# ⚠️ 内存图的**栈页是必需的**：Spike 默认把初始栈放在 0x7FFF_F000，
# 缺了那一段会报 "Memory address 0x7ffff000 is invalid" 直接退出。
#
# ⚠️ **栈页是参考侧的特性**：候选侧映射的栈窗口是 [0x7fff_e000, 0x7fff_f000)
#   ⇒ ⇒ 「两侧暴露同一地址集合」这句**不准确** ⇒ ⇒ 诚实口径（STAGE-19 裁决 I）：
#   **RAM 窗口**两侧一致（**构造保证**）· **栈窗口**各是本侧特性。
SPIKE_ISA="rv32imc_zicsr_zicclsm_zifencei_zicntr"
SPIKE_STACK_MEM="2147479552:4096"
# The **RAM window** is **not** written down here: the server **reports** it
# (`x-boot-ram-window`, via `osz-boot --print-memory`) and this script
# **reads** that one line ⇒ ⇒ the RAM window's single source of truth is the
# **platform description** ⇒ ⇒ the two sides cannot drift apart
# (STAGE-19 裁决 I).
# ⚠️ `SPIKE_MEM` 一旦给了 ⇒ ⇒ **不**用打印值（**覆盖口优先**）。
SPIKE_MEM="${SPIKE_MEM:-}"

while [ $# -gt 0 ]; do
  case "$1" in
    --elf)        ELF="$2"; shift 2 ;;
    --spike)      SPIKE="$2"; shift 2 ;;
    --timeout-ms) TIMEOUT_MS="$2"; shift 2 ;;
    --server)     SERVER="$2"; shift 2 ;;
    --board)      BOARD="$2"; shift 2 ;;
    -h|--help)
      # ⚠️ 帮助文本 = **头部注释块** ⇒ ⇒ 不能写死行号（头部会变长 ⇒ ⇒ 行号
      # 会把帮助**截断在半句**）⇒ ⇒ 取到第一行**不是**注释为止。
      sed -n '2,/^[^#]/p' "${BASH_SOURCE[0]}" | sed '$d'
      exit 0 ;;
    *)
      echo "boot-osz.sh: unknown option $1" >&2
      exit 1 ;;
  esac
done

SERVER="${SERVER%/}"

if [ ! -f "$ELF" ]; then
  echo "boot-osz.sh: ELF not found: $ELF" >&2
  echo "  hint: 真实 RTOS 的产物在**仓外**，用 \$OSZ_ELF 指定" >&2
  exit 1
fi

if ! command -v "$SPIKE" >/dev/null 2>&1; then
  echo "boot-osz.sh: spike not found: $SPIKE" >&2
  echo "  hint: 参考侧需要本机装 Spike（不是仓内依赖）" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "boot-osz.sh: curl not found（/api/boot 通道需要它）" >&2
  exit 1
fi

# ⚠️ 先问一句「服务在不在」：`/api/boot` 失败时的报错信息不够直白，
#   这里给一条能照着做的提示（起服务的命令与镜像根白名单）。
if ! HEALTH="$(curl -sS --max-time 5 "$SERVER/healthz" 2>/dev/null)"; then
  echo "boot-osz.sh: no live server at $SERVER" >&2
  echo "  hint: cargo run -p riscv_sim_server -- --allow-image-root $(dirname "$ELF")" >&2
  exit 1
fi
# ⚠️ `--timeout-ms` 会被服务端的 `--max-run-ms` 硬顶住（超了直接 400）⇒ ⇒
#   在这里**先说清楚**，别等打完请求再回来解 400 的报错体。
MAX_RUN_MS="$(printf '%s' "$HEALTH" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("max_run_ms", 0))' 2>/dev/null || echo 0)"
if [ -n "$MAX_RUN_MS" ] && [ "$MAX_RUN_MS" != "0" ] && [ "$TIMEOUT_MS" -gt "$MAX_RUN_MS" ]; then
  echo "boot-osz.sh: --timeout-ms $TIMEOUT_MS exceeds the server's --max-run-ms ($MAX_RUN_MS)" >&2
  echo "  hint: 起服务时给 --max-run-ms ${TIMEOUT_MS}（或把 --timeout-ms 调小）" >&2
  exit 1
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/boot-osz.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

PLAT_OUT="$WORK/platform.bin"
SPIKE_OUT="$WORK/spike.bin"

echo "== osz 对拍 =="
echo "  elf    : $ELF"
echo "  server : $SERVER （board=${BOARD}）"
echo "  spike  : $("$SPIKE" --help 2>&1 | head -1)"
echo

# ---- 候选侧：`osz-boot` 是 `/api/boot` 的客户端（R-12） -------------------
#
# ⚠️ **stdout 是原始字节、stderr 是判决** ⇒ stdout 可以直接与 Spike 的
# stdout 比。停因与板都来自服务端（`x-boot-stop`）⇒ ⇒ 本脚本不再自己判。
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
cargo build --offline --release --bin osz-boot >/dev/null 2>&1 || {
  echo "boot-osz.sh: failed to build osz-boot（/api/boot 的客户端）" >&2
  exit 1
}

# ⚠️ **二进制路径必须跟着 `CARGO_TARGET_DIR` 走**（v0.24 门禁抓到的真缺陷）：
# 上面那次 `cargo build` 认 `CARGO_TARGET_DIR`，而运行侧若写死
# `$REPO_ROOT/target/release/osz-boot`，两者就会**指向不同的文件** ⇒
# 跑的是**上一批**留下的旧二进制（或者干脆不存在）⇒ 判决错得莫名其妙
# （表现为「候选侧文件缺失」而不是「候选侧跑挂了」）。
# ⇒ 一条约束一个落点：构建目录只有一个真相，就是 `CARGO_TARGET_DIR`。
PLAT_BIN="${CARGO_TARGET_DIR}/release/osz-boot"
if [ ! -x "$PLAT_BIN" ]; then
  echo "boot-osz.sh: candidate binary not found: $PLAT_BIN" >&2
  echo "  hint: cargo build 用的 CARGO_TARGET_DIR 与运行侧必须一致" >&2
  exit 1
fi

# ---- 候选侧报告**它那块板的** RAM 窗口 ------------------------------------
#
# ⚠️ **单一真相源 = 平台描述**（STAGE-19 裁决 I）：窗口由**服务端**那块板
# 给出（回执头 `x-boot-ram-window`）⇒ ⇒ 这里**不写地址** ⇒ ⇒ 描述改了
# 高段大小 ⇒ ⇒ 两边**自动**跟着变 ⇒ ⇒ 不可能漂。
#
# ⚠️ **覆盖口优先**：`SPIKE_MEM` 一旦给了 ⇒ ⇒ **不**用打印值。
if [ -z "$SPIKE_MEM" ]; then
  CANDIDATE_MEM="$("$PLAT_BIN" --print-memory "$ELF" --server "$SERVER" --board "$BOARD" 2>"$WORK/memory.stderr")" || {
    echo "boot-osz.sh: the candidate could not print its RAM window" >&2
    sed 's/^/  /' "$WORK/memory.stderr" >&2
    exit 1
  }
  # ⚠️ 形状必须是 `base:size` 且**十进制** ⇒ ⇒ Spike 的 `-m` 才吃得下。
  if ! printf '%s' "$CANDIDATE_MEM" | grep -Eq '^[0-9]+:[0-9]+$'; then
    echo "boot-osz.sh: the candidate printed an unusable RAM window: '$CANDIDATE_MEM'" >&2
    exit 1
  fi
  SPIKE_MEM="$SPIKE_STACK_MEM,$CANDIDATE_MEM"
fi
echo "  spike 内存图: $SPIKE_MEM"

"$PLAT_BIN" "$ELF" \
  --server "$SERVER" --board "$BOARD" \
  --budget 100000000 --timeout-ms "$TIMEOUT_MS" \
  --prefix "OSZ\$ " --out "$PLAT_OUT" \
  > "$WORK/platform.stdout" 2> "$WORK/platform.stderr"
PLAT_RC=$?

echo "-- 候选侧 --"
cat "$WORK/platform.stderr"
PLAT_BYTES=$(wc -c < "$PLAT_OUT" | tr -d ' ')
echo "   console 字节数: $PLAT_BYTES (exit=$PLAT_RC)"
echo

# ---- 参考侧（Spike） ---------------------------------------------------
#
# ⚠️ Spike 也要超时：osz 是死循环。
perl "$REPO_ROOT/tools/watchdog.pl" "$((TIMEOUT_MS / 1000 + 30))" \
  "$SPIKE" "--isa=$SPIKE_ISA" "-m$SPIKE_MEM" "$ELF" \
  > "$SPIKE_OUT" 2> "$WORK/spike.stderr"
SPIKE_RC=$?

echo "-- 参考侧 --"
echo "   console 字节数: $(wc -c < "$SPIKE_OUT" | tr -d ' ') (exit=$SPIKE_RC)"
if [ -s "$WORK/spike.stderr" ]; then
  echo "   spike stderr:"
  sed 's/^/     /' "$WORK/spike.stderr" | head -5
fi
echo

# ---- 判决 --------------------------------------------------------------
if [ "$PLAT_BYTES" -eq 0 ]; then
  echo "VERDICT NO-VERDICT 候选侧一个字节都没产出" >&2
  exit 3
fi
if [ ! -s "$SPIKE_OUT" ]; then
  echo "VERDICT NO-VERDICT 参考侧一个字节都没产出" >&2
  exit 3
fi

if cmp -s "$PLAT_OUT" "$SPIKE_OUT"; then
  echo "VERDICT MATCH 两侧 console 字节流逐字节相同（$PLAT_BYTES 字节）"
  exit 0
fi

echo "VERDICT MISMATCH 两侧 console 字节流不同" >&2
echo "--- 候选侧 ---" >&2
xxd "$PLAT_OUT" >&2 | head -8
echo "--- 参考侧 ---" >&2
xxd "$SPIKE_OUT" >&2 | head -8
echo "--- 差异 ---" >&2
cmp "$PLAT_OUT" "$SPIKE_OUT" >&2 || true
exit 2
