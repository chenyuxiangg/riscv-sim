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

# NOTE: `wasm-pack build <crate-dir>` runs with the **crate directory** as
# its working directory, so a relative --out-dir lands inside
# crates/kernel/ instead of the repo root. Resolve to an absolute path.
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="$REPO_ROOT/front/assets"
mkdir -p "$TARGET_DIR"

# Target: `--target web`, NOT `--target bundler` and NOT `--target no-modules`.
#
#   * `--target bundler` is the one that needs a bundler — excluded by
#     ADR-6 (zero-build, zero-deps).
#   * `--target no-modules` emits a CommonJS-style IIFE with **no
#     `export` statements** (measured 2026-10-20: the glue is
#     `let wasm_bindgen = (function(exports){...})({__proto__: null})`).
#     The front-end is native ES modules, so it cannot `import` that
#     file's exports at all — a dynamic `import()` would yield an empty
#     namespace.
#   * `--target web` emits a **native ES module with named exports**
#     (`export function kernel_version() {...}` + `export { initSync,
#     __wbg_init as default }`) and resolves the `.wasm` via
#     `import.meta.url` — exactly what the zero-build front-end needs.
#     Verified 2026-10-20 under Node 22: `kernel_version()`,
#     `protocol_schema_version()` and `ping("你好")` all round-trip.
wasm-pack build \
    --target web \
    --out-dir "$TARGET_DIR/pkg" \
    --out-name "riscv_sim_kernel" \
    $PROFILE \
    crates/kernel

echo
echo "Built:"
ls -lh "$TARGET_DIR/pkg/" | awk '/riscv_sim_kernel/ {print "  " $5, $9}'
echo "Next: serve the front end over HTTP (ES modules and Workers need a"
echo "same-origin http(s) origin — a file:// page is blocked):"
echo "  python3 -m http.server 8080 --directory front"
echo "then open http://localhost:8080/ (the kernel worker will be wired"
echo "in services/m07_transport.js once the WASM exports are stable)."
