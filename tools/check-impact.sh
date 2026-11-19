#!/usr/bin/env bash
# 改完立刻验：按**影响面**跑该跑的那一部分（前端用例 / 真机门禁 / 内核单测 / 文档台账）。
#
# 为什么有它：全量门禁 4~15 分钟、前端 758 条 ⇒ 改一行也跑全量 ⇒ 「改完立刻验」
# 在成本上不成立 ⇒ 红点攒到收口才暴露。这个脚本把「查计划 → 只跑受影响的部分」
# 合成一条命令。
#
# ⚠️ **它不是收口判据**。收口仍是「全量 0 ✗」：`browser-verify` 末尾会明确写
#   「本趟不是全量绿」。选测只用于定位期。
#
# 用法：
#   tools/check-impact.sh                 # 与 origin/main 比
#   BASE=HEAD tools/check-impact.sh       # 只看未提交的改动（改完立刻验）
#   tools/check-impact.sh --plan-only     # 只打印计划，不跑

set -uo pipefail
cd "$(dirname "$0")/.."

BASE="${BASE:-origin/main}"
PLAN_ONLY=0
for arg in "$@"; do
    case "$arg" in
        --plan-only) PLAN_ONLY=1 ;;
        --help|-h)
            echo "usage: BASE=<rev> tools/check-impact.sh [--plan-only]"
            echo "  BASE 默认 origin/main；--plan-only 只打印计划"
            exit 0 ;;
        *) echo "error: unknown argument: $arg" >&2; exit 2 ;;
    esac
done

echo "==> impact plan（基线 ${BASE}）"
node tools/impact.mjs --base "${BASE}" || exit 1
if [ "$PLAN_ONLY" = "1" ]; then exit 0; fi

FAIL=0

# ---- ① 前端用例（只跑依赖改动的那些） ---------------------------------------
echo
echo "==> 前端用例（check-front.sh --impact）"
IMPACT_BASE="${BASE}" bash tools/check-front.sh --impact || FAIL=1

# ---- ② 内核单测（只跑受影响的包） -------------------------------------------
PKGS="$(node tools/impact.mjs --json --base "${BASE}" 2>/dev/null \
    | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{
        try { const p=JSON.parse(s); console.log((p.cargo||[]).join(" ")); } catch { console.log(""); }
      })')"
if [ -n "$PKGS" ]; then
    echo
    echo "==> 内核单测：cargo test -p $PKGS"
    # shellcheck disable=SC2086
    cargo test -p $PKGS || FAIL=1
else
    echo
    echo "==> 内核单测：本次改动不涉及 crates/*（跳过）"
fi

# ---- ③ 文档台账（只有动了 docs/*.md 才跑） ---------------------------------
DOCS="$(node tools/impact.mjs --json --base "${BASE}" 2>/dev/null \
    | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{
        try { const p=JSON.parse(s); console.log(p.docs === true ? "yes" : "no"); } catch { console.log("no"); }
      })')"
if [ "$DOCS" = "yes" ] && command -v python3 >/dev/null 2>&1; then
    echo
    echo "==> 文档台账：tools/doc_check.py"
    python3 tools/doc_check.py docs/progress/PROGRESS.md || FAIL=1
fi

# ---- ④ 真机门禁（只跑受影响的段；拿不准自动收敛全量） ----------------------
echo
echo "==> 真机门禁（browser-verify --impact）"
node tools/browser-verify.mjs --impact --impact-base "${BASE}" || FAIL=1

echo
if [ "$FAIL" = "0" ]; then
    echo "check-impact: 选测全部通过 —— ⚠️ 这不是收口判据（收口仍须全量 0 ✗）"
else
    echo "check-impact: 有失败项（见上）" >&2
    exit 1
fi
