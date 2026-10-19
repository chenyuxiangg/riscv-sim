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

node --test "front/tests/*.test.mjs"
