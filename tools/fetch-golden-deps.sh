#!/usr/bin/env bash
#
# Fetch the external golden-test corpus (riscv-tests) and the RISC-V
# cross toolchain.
#
# These are **external tools, not part of the main source tree** (see
# AGENTS.md「正确性对拍：Spike golden + riscv-tests（外部工具，不进主代码）」).
# This script makes the setup reproducible for whoever runs the golden
# comparison next — a previous round had the toolchain installed by
# hand and then silently lost, which cost a day of "why does the
# golden script not work" debugging.
#
# Usage:
#   tools/fetch-golden-deps.sh [DEST_DIR]
#
# Default DEST_DIR: $HOME/riscv-sim-golden
#
# Layout produced:
#   DEST_DIR/riscv-tests/          git clone of riscv-software-src/riscv-tests
#   DEST_DIR/toolchain/            xpack riscv-none-elf-gcc (rv32imc capable)
#
# Prerequisites already on this machine (installed via brew):
#   riscv-isa-sim (spike)  ·  riscv-pk
#   →  brew install riscv-software-src/riscv/riscv-isa-sim \
#                riscv-software-src/riscv/riscv-pk
#
# Note on the toolchain binary name: the xpack build ships
# `riscv-none-elf-gcc` (NOT `riscv32-unknown-elf-gcc`, which is the
# upstream SiFive / newlib naming).  An earlier revision of this script
# looked for the SiFive name and therefore always reported the toolchain
# as missing even after a successful download.
#
set -euo pipefail

DEST="${1:-$HOME/riscv-sim-golden}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

RISCV_TESTS_REPO="https://github.com/riscv-software-src/riscv-tests.git"
XPACK_VERSION="15.2.0-1"
XPACK_URL="https://github.com/xpack-dev-tools/riscv-none-elf-gcc-xpack/releases/download/v${XPACK_VERSION}/xpack-riscv-none-elf-gcc-${XPACK_VERSION}-darwin-arm64.tar.gz"

mkdir -p "$DEST"

# ---------------------------------------------------------------- riscv-tests
if [ -d "$DEST/riscv-tests/.git" ]; then
    echo "==> riscv-tests 已在 $DEST/riscv-tests（跳过 clone）"
else
    echo "==> clone riscv-tests → $DEST/riscv-tests"
    # --depth 1 + --recurse-submodules：isa/ 下的 rv32*.S 是 rv64*/ 的薄包装，
    # env/ 子模块提供 riscv_test.h（tohost / ecall 自检协议的定义）。
    git clone --depth 1 --recurse-submodules --shallow-submodules \
        "$RISCV_TESTS_REPO" "$DEST/riscv-tests"
fi

# ---------------------------------------------------------------- toolchain
if [ -x "$DEST/toolchain/bin/riscv-none-elf-gcc" ]; then
    echo "==> 工具链已在 $DEST/toolchain（跳过下载）"
else
    echo "==> 下载 xpack riscv-none-elf-gcc ${XPACK_VERSION}（darwin-arm64）"
    TMP_TGZ="$DEST/riscv-none-elf-gcc.tar.gz"
    curl -sSL --retry 3 -o "$TMP_TGZ" "$XPACK_URL"
    echo "==> 解压"
    tar xzf "$TMP_TGZ" -C "$DEST"
    # xpack 解出的是 xpack-riscv-none-elf-gcc-<ver>/ 目录，统一重命名为 toolchain/
    SRC_DIR="$(find "$DEST" -maxdepth 1 -type d -name 'xpack-riscv-none-elf-gcc-*' | head -1)"
    if [ -n "$SRC_DIR" ] && [ "$SRC_DIR" != "$DEST/toolchain" ]; then
        rm -rf "$DEST/toolchain"
        mv "$SRC_DIR" "$DEST/toolchain"
    fi
    rm -f "$TMP_TGZ"
fi

# ---------------------------------------------------------------- 自检
GCC="$DEST/toolchain/bin/riscv-none-elf-gcc"
AS="$DEST/toolchain/bin/riscv-none-elf-as"
OBJCOPY="$DEST/toolchain/bin/riscv-none-elf-objcopy"

echo
echo "==> 自检"
for bin in "$GCC" "$AS" "$OBJCOPY"; do
    if [ -x "$bin" ]; then
        echo "  ✓ $(basename "$bin")"
    else
        echo "  ✗ 缺失: $bin" >&2
        exit 1
    fi
done

# rv32 multilib 必须可用：riscv-tests 的 rv32ui 用例要 -march=rv32i*
if "$GCC" -print-multi-lib 2>/dev/null | grep -q "rv32im"; then
    echo "  ✓ rv32 multilib 可用"
else
    echo "  ⚠ 未检出 rv32 multilib（riscv-tests 的 rv32 用例可能编不过）" >&2
fi

# riscv-tests 的 isa/ 子树必须齐全：rv32ui 是 RV32I 语料的主体
if [ -d "$DEST/riscv-tests/isa/rv32ui" ]; then
    echo "  ✓ riscv-tests 语料齐全（rv32ui / rv32um / rv32uc ...）"
else
    echo "  ✗ riscv-tests 缺少 isa/rv32ui（clone 可能不完整）" >&2
    exit 1
fi

echo
echo "环境就绪："
echo "  riscv-tests : $DEST/riscv-tests"
echo "  toolchain   : $DEST/toolchain"
echo "  导出到 PATH : export PATH=\"$DEST/toolchain/bin:\$PATH\""
echo
echo "下一步："
echo "  1) 对拍自检（变异金丝雀，两侧都应报 FAIL 3）"
echo "     tools/golden-run.sh --self-check"
echo "  2) 全量对拍"
echo "     tools/golden-run.sh --keep-going"
echo
echo "覆盖范围与口径见 tests/golden/README.md。"
