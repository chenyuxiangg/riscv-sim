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
# ## 用法
#
#   tools/boot-osz.sh [--elf <path>] [--spike <cmd>] [--timeout-ms <n>]
#
#   --elf <path>    要跑的 ELF（默认 $OSZ_ELF，其次 /tmp/osz-artifacts/osz.elf）
#   --spike <cmd>   Spike 可执行文件（默认 spike）
#   --timeout-ms    两侧各自的墙钟上限（默认 60000）
#
# ## 退出码
#
#   0  两侧逐字节相同
#   1  用法错误 / 缺 ELF / 缺 Spike
#   2  两侧不一致
#   3  某一侧没跑出判决
#
# ⚠️ **本脚本不进默认门禁**：它需要仓外的 osz ELF 与本机的 Spike。
# 仓内只进脚本与规格（判决的**规则**进仓，判决的**料**不进仓）。

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

ELF="${OSZ_ELF:-/tmp/osz-artifacts/osz.elf}"
SPIKE="${SPIKE:-spike}"
TIMEOUT_MS=60000

# Spike 的 ISA 串与内存图（v0.23 批实测确定，**不要**随便改）。
#
# ⚠️ 内存图的**栈窗口是必需的**：Spike 默认把初始栈放在 0x7FFF_F000，
# 缺了那一段会报 "Memory address 0x7ffff000 is invalid" 直接退出。
SPIKE_ISA="rv32imc_zicclsm_zifencei"
SPIKE_MEM="2147479552:4096,2147483648:134217728"

while [ $# -gt 0 ]; do
  case "$1" in
    --elf)        ELF="$2"; shift 2 ;;
    --spike)      SPIKE="$2"; shift 2 ;;
    --timeout-ms) TIMEOUT_MS="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,30p' "${BASH_SOURCE[0]}"
      exit 0 ;;
    *)
      echo "boot-osz.sh: unknown option $1" >&2
      exit 1 ;;
  esac
done

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

WORK="$(mktemp -d "${TMPDIR:-/tmp}/boot-osz.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

PLAT_OUT="$WORK/platform.bin"
SPIKE_OUT="$WORK/spike.bin"

echo "== osz 对拍 =="
echo "  elf    : $ELF"
echo "  spike  : $("$SPIKE" --help 2>&1 | head -1)"
echo

# ---- 候选侧（本平台） --------------------------------------------------
#
# ⚠️ 用仓内的 watchdog.pl 加超时：osz 是**死循环**，不超时不会自己停。
# 退出码 124 = HANG（不是失败，是「跑够了」）。
#
# ⚠️ **stdout 是原始字节、stderr 是判决** ⇒ stdout 可以直接与 Spike 的
# stdout 比。
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
cargo build --offline --release --bin osz-boot >/dev/null 2>&1 || {
  echo "boot-osz.sh: failed to build osz-boot" >&2
  exit 1
}

perl "$REPO_ROOT/tools/watchdog.pl" "$((TIMEOUT_MS / 1000 + 30))" \
  "$REPO_ROOT/target/release/osz-boot" "$ELF" \
  --budget 200000000 --timeout-ms "$TIMEOUT_MS" \
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
