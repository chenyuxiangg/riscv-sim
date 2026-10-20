#!/usr/bin/env bash
# Front-end gate — ESM link check + DOM-stub smoke test.
#
# The front end had **no** automated check of any kind: 203 Rust tests
# were green while `front/index.html` was 100% blank, because a missing
# export in front/views/*.js fails at ESM *link* time — before any code
# runs (FRONTEND_Review.md #16, CROSS_MODULE_DEBT.md D-23).
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
