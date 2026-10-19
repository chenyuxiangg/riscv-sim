#!/usr/bin/env bash
# Build the kernel as a WASM module for the browser front-end.
#
# Prerequisites:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-pack
#
# Usage:
#   tools/build-wasm.sh           # debug build, outputs front/assets/riscv_sim_kernel_bg.wasm
#   tools/build-wasm.sh --release # release build (smaller, slower compile)
#
# After the build, the front-end (`front/app.js`) imports the
# generated package; see the TODO in `front/services/m07_transport.js`.

set -euo pipefail

cd "$(dirname "$0")/.."

PROFILE="--dev"
case "${1:-}" in
    --release) PROFILE="--release" ;;
    "") ;;
    *) echo "usage: $0 [--release]" >&2; exit 2 ;;
esac

if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "wasm-pack not found. Install with:" >&2
    echo "  cargo install wasm-pack" >&2
    exit 1
fi

if ! rustup target list --installed | grep -q wasm32-unknown-unknown; then
    echo "wasm32-unknown-unknown target missing. Install with:" >&2
    echo "  rustup target add wasm32-unknown-unknown" >&2
    exit 1
fi

TARGET_DIR="front/assets"
mkdir -p "$TARGET_DIR"

# We deliberately do NOT use --target web / bundler here. The
# kernel exposes a tiny C ABI surface; the front-end's `app.js`
# loads the raw .wasm + .js via dynamic import and wraps the
# exports. See AD ADR-6 (zero-build, zero-deps).
wasm-pack build \
    --target no-modules \
    --out-dir "$TARGET_DIR/pkg" \
    --out-name "riscv_sim_kernel" \
    $PROFILE \
    crates/kernel

echo
echo "Built $(ls -lh "$TARGET_DIR/pkg/" | awk '/riscv_sim_kernel/ {print $5, $9}')"
echo "Next: open front/index.html in a browser (the kernel worker will"
echo "be wired in services/m07_transport.js once the WASM exports"
echo "are stable)."
