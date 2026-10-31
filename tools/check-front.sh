#!/usr/bin/env bash
# Front-end gate — ESM link check + DOM-stub smoke test.
#
# The front end had **no** automated check of any kind: 203 Rust tests
# were green while `front/index.html` was 100% blank, because a missing
# export in front/views/*.js fails at ESM *link* time — before any code
# runs (FRONTEND_Review.md #16, REVIEWS.md D-23).
#
# This gate boots the real front end against a minimal DOM double and
# asserts the behaviour the review found broken. It needs Node >= 22
# (node:test) and has no third-party dependency.
#
# Usage: tools/check-front.sh

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v node >/dev/null 2>&1; then
    echo "error: node not found — the front-end gate needs Node >= 22" >&2
    exit 1
fi

# `node --test` with a quoted glob needs Node >= 21; the gate also uses the
# node:test API surface of 18+. Check explicitly so an old Node fails with
# a clear message instead of a cryptic one (FRONTEND_Review.md R-10).
node -e 'const major = +process.versions.node.split(".")[0];
if (major < 22) {
  console.error(`error: Node >= 22 required, found ${process.versions.node}`);
  process.exit(1);
}'

node --test "front/tests/*.test.mjs"

# ---------------------------------------------------------------------------
# The ledger's front-end attribution (v0.34 · `tools/front_test_attrib.py`)
# ---------------------------------------------------------------------------
#
# ⚠️ **Why this is here**: the ledger's §0.1 front-end rows used to be
#   accumulated **by hand** per batch ⇒ ⇒ the same tree could not reproduce
#   the number that was recorded ⇒ ⇒ "recompute from the current tree" was
#   **not true** for those two rows.  The attribution now has **one**
#   algorithm ⇒ ⇒ it is worth **watching in the default gate**.
#
# ⚠️ **Two steps, on purpose**:
#   1. the **attribution** itself (the number the ledger now prints) — it
#      must have **no TIE** (a TIE means "nobody decided" ⇒ ⇒ **red**);
#   2. the **historical regression** (`--selftest`) — the algorithm must
#      reproduce the ledger's recorded values on the v0.31 / v0.32 / v0.33
#      trees ⇒ ⇒ that is what proves the algorithm is not a new invention.
#
# ⚠️ Step 2 builds three git worktrees **outside the repo** and removes
#   them again ⇒ ⇒ it leaves nothing behind.
#
# ⚠️ `python3` may be absent ⇒ ⇒ the check is **skipped with a notice**
#   (not a silent pass) ⇒ ⇒ a machine without python3 still runs the
#   front-end gate.
if command -v python3 >/dev/null 2>&1; then
    echo
    echo "==> ledger front-end attribution (tools/front_test_attrib.py)"
    python3 tools/front_test_attrib.py || exit 1
    python3 tools/front_test_attrib.py --selftest || exit 1
else
    echo
    echo "==> SKIP: ledger front-end attribution (python3 not found)"
fi

# ---------------------------------------------------------------------------
# The dev server's cache headers (v0.61 · `tools/serve-front.py`)
# ---------------------------------------------------------------------------
#
# ⚠️ **Why this is in the default gate**: `python3 -m http.server` sends
#   `Last-Modified` but **no `Cache-Control`** ⇒ ⇒ the browser applies
#   *heuristic freshness* to the ES modules ⇒ ⇒ "I changed the code but the
#   page looks the same" ⇒ ⇒ that reads like "the code is wrong" ⇒ ⇒ it
#   cost a real debugging cycle (2026-10-31 · v0.60 batch).
#
# ⇒ ⇒ `tools/serve-front.py` is the repo's **one** way to serve the front end
#   ⇒ ⇒ it must send `Cache-Control: no-store` ⇒ ⇒ **that header is the
#   invariant**, ⇒ ⇒ and an invariant nobody checks rots.
#
# ⚠️ `python3` may be absent ⇒ ⇒ **skipped with a notice** (not a silent pass).
if command -v python3 >/dev/null 2>&1; then
    echo
    echo "==> dev server cache headers (tools/serve-front.py --self-check)"
    python3 tools/serve-front.py --self-check || exit 1
else
    echo
    echo "==> SKIP: dev server cache headers (python3 not found)"
fi
