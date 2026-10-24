#!/usr/bin/env bash
#
# tools/golden-run.sh — compare our ISS against Spike on the riscv-tests corpus.
#
# Two sides run the *same* ELF:
#
#   reference  Spike       --isa=$ISA   (external oracle, not part of this repo)
#   candidate  riscv-golden              (our M-13 ISS + M-14 bus + M-06 loader)
#
# Both must reach the same verdict:
#
#   PASS           the guest wrote tohost = 1                (exit code 0)
#   FAIL <n>       the guest wrote tohost = (n<<1)|1         (exit code n)
#   NOVERDICT      neither side got a verdict within the timeout / budget
#
# A FAIL verdict must agree on the *test number* too — that is what makes
# the comparison meaningful rather than merely binary.
#
# Prerequisites (see tools/fetch-golden-deps.sh):
#   * riscv-tests clone      $RISCV_TESTS_DIR
#   * riscv-none-elf-gcc    $TOOLCHAIN_DIR/bin
#   * spike                  on $PATH
#
# ## The memory map
#
# Spike's `-m` gets **two** segments:
#
#   * Spike's **initial stack page** (`0x7fff_f000`, 4096 bytes) —— a
#     **reference-side** feature, kept in this script on purpose;
#   * the **RAM window** —— **read from the candidate side**, which
#     prints it (`riscv-golden --print-memory`) after taking it from the
#     **platform description** ⇒ ⇒ the RAM window's single source of
#     truth is the description ⇒ ⇒ the two sides cannot drift apart
#     (STAGE-19 裁决 I).
#
# ⚠️ The two sides do **not** expose the **same** address set: the
# candidate's stack window is `[0x7fff_e000, 0x7fff_f000)` while Spike's
# initial stack page is `[0x7fff_f000, 0x7fff_f000 + 4096)` ⇒ ⇒ "the
# memory map **both** sides expose" would be **inaccurate** ⇒ ⇒ the
# honest wording is: **the RAM window agrees by construction**, the
# **stack window is each side's own feature**.
#
# ⚠️ `GOLDEN_MEM` still overrides the whole `-m` value ⇒ ⇒ when it is
# given, the printed window is **not** used.
#
# Usage:
#   tools/golden-run.sh [options]
#
#   --suite NAME     only run one suite (rv32ui / rv32um / rv32uc)
#   --filter REGEX   only run cases whose "suite/test" matches REGEX
#   --list           list the corpus and exit
#   --self-check     run the mutation canaries only (see below)
#   --release        build the candidate runner in release mode
#   --keep-going     do not stop at the first mismatch
#
# Exit status: 0 when every compared case agrees, 1 otherwise.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

RISCV_TESTS_DIR="${RISCV_TESTS_DIR:-$HOME/riscv-sim-golden/riscv-tests}"
TOOLCHAIN_DIR="${RISCV_TOOLCHAIN_DIR:-$HOME/riscv-sim-golden/toolchain}"
SPIKE="${SPIKE:-}"
ISA="${GOLDEN_ISA:-rv32imc_zicclsm_zifencei}"
# Spike's **initial stack page** —— a **reference-side** feature, kept here
# on purpose.
#
#   2147479552 = 0x7fff_f000   stack page, 4096 bytes
#
# ⚠️ Spike 默认把初始栈放在 0x7FFF_F000，缺了那一段会报
#   "Memory address 0x7ffff000 is invalid" 直接退出 ⇒ ⇒ 这一段**必须**留着。
#
# ⚠️ **它与候选侧的栈窗口不是同一段**：候选侧映射的是
#   [0x7fff_e000, 0x7fff_f000) ⇒ ⇒ 「两侧暴露同一地址集合」这句**不准确**
#   ⇒ ⇒ 诚实口径（STAGE-19 裁决 I）是：**RAM 窗口**两侧一致（**构造保证**）
#   · **栈窗口**各是本侧特性。
#
# ⚠️ Spike 的多区域 `-m` parser 只接受 **十进制** 数字（它的单区域形式
#   才接受十六进制）⇒ ⇒ 这里的字面量是十进制。
SPIKE_STACK_MEM="2147479552:4096"
# The **RAM window** is **not** written down here: the candidate side
# **prints** it (`riscv-golden --print-memory`) and the reference side
# **reads** that one line ⇒ ⇒ the RAM window's single source of truth is
# the **platform description** ⇒ ⇒ the two sides cannot drift apart
# (STAGE-19 裁决 I).  The value is assembled **after** the candidate has
# been built — see below.
MEM=""
TIMEOUT_SEC="${GOLDEN_TIMEOUT:-10}"
BUDGET="${GOLDEN_BUDGET:-1000000}"
OUT_DIR="${GOLDEN_OUT:-$REPO_ROOT/tests/golden/_out}"

MANIFEST="$REPO_ROOT/tests/golden/corpus.txt"
ENV_DIR="$REPO_ROOT/tests/golden/env"
CANARY_DIR="$REPO_ROOT/tests/golden/canary"
MACRO_DIR_REL="isa/macros/scalar"

SUITE_FILTER=""
FILTER=""
LIST_ONLY=0
SELF_CHECK=0
KEEP_GOING=0
RELEASE=0

die() { echo "golden-run: $*" >&2; exit 2; }

while [ $# -gt 0 ]; do
    case "$1" in
        --suite)       [ $# -ge 2 ] || die "--suite needs a suite name"; SUITE_FILTER="$2"; shift 2 ;;
        --filter)      [ $# -ge 2 ] || die "--filter needs a REGEX"; FILTER="$2"; shift 2 ;;
        --list)        LIST_ONLY=1; shift ;;
        --self-check)  SELF_CHECK=1; shift ;;
        --release)     RELEASE=1; shift ;;
        --keep-going)  KEEP_GOING=1; shift ;;
        # ⚠️ The help text is the **header comment block** ⇒ ⇒ it must not
        # be a hard-coded line range (the header grows ⇒ ⇒ the range
        # would cut it in the middle) ⇒ ⇒ take it up to the first line
        # that is not a comment.
        -h|--help)     sed -n '2,/^[^#]/p' "$0" | sed '$d'; exit 0 ;;
        *) echo "golden-run: unknown option $1" >&2; exit 2 ;;
    esac
done

# ---------------------------------------------------------------- preflight
[ -f "$MANIFEST" ] || die "missing corpus manifest: $MANIFEST"
[ -d "$RISCV_TESTS_DIR/isa" ] || die "riscv-tests not found: $RISCV_TESTS_DIR (set \$RISCV_TESTS_DIR)"
[ -f "$ENV_DIR/riscv_test.h" ] || die "missing golden env: $ENV_DIR"
[ -f "$ENV_DIR/link.ld" ] || die "missing golden linker script: $ENV_DIR/link.ld"

GCC="$TOOLCHAIN_DIR/bin/riscv-none-elf-gcc"
[ -x "$GCC" ] || die "toolchain not found: $GCC (set \$RISCV_TOOLCHAIN_DIR)"

if [ -z "$SPIKE" ]; then
    SPIKE="$(command -v spike 2>/dev/null || true)"
fi
[ -n "$SPIKE" ] && [ -x "$SPIKE" ] || die "spike not found on PATH (set \$SPIKE)"

# ------------------------------------------------- env constants are upstream's
#
# `tests/golden/env/encoding.h` claims to carry **upstream's** values,
# verbatim.  That claim is load-bearing: the harness compiles **one**
# image and hands it to **both** sides, so a "cleaned up" constant does
# not show up as a side-vs-side mismatch — it silently replaces the
# upstream test program with a slightly different one, and the corpus
# keeps comparing two different programs while looking green.
#
# ⇒ The drift is checked here, before anything is built.  A constant we
# define that upstream does not define (or defines differently) is a
# hard error, not a warning.
#
# ⚠️ The comparison is **textual** (whitespace-insensitive, case
# insensitive): the constants are architectural encodings, so upstream's
# spelling is the definition.
check_env_constants() {
    local ours="$ENV_DIR/encoding.h"
    local upstream="$RISCV_TESTS_DIR/env/encoding.h"
    [ -f "$ours" ] || die "missing env encoding: $ours"
    [ -f "$upstream" ] || die "missing upstream encoding: $upstream"
    local name value upstream_value
    while IFS=$'\t' read -r name value; do
        case "$name" in _*) continue ;; esac   # include guards, not constants
        upstream_value="$(
            awk -v n="$name" '
                /^#define[ \t]/ {
                    line = $0
                    sub(/^#define[ \t]+/, "", line)
                    name = line
                    sub(/[ \t].*$/, "", name)
                    if (name == n) {
                        rest = line
                        sub(/^[^ \t]+[ \t]+/, "", rest)
                        print rest
                        exit
                    }
                }
            ' "$upstream"
        )"
        [ -n "$upstream_value" ] ||
            die "encoding.h: '$name' is not defined upstream ($upstream)"
        [ "$(printf '%s' "$value" | tr 'A-Z' 'a-z' | tr -d '[:space:]')" = \
          "$(printf '%s' "$upstream_value" | tr 'A-Z' 'a-z' | tr -d '[:space:]')" ] ||
            die "encoding.h: '$name' = '$value', upstream says '$upstream_value' — the corpus must run upstream's program"
    done < <(awk '
        /^#define[ \t]/ {
            line = $0
            sub(/^#define[ \t]+/, "", line)
            name = line
            sub(/[ \t].*$/, "", name)
            rest = line
            sub(/^[^ \t]+[ \t]+/, "", rest)
            print name "\t" rest
        }
    ' "$ours")
}
check_env_constants

# ---------------------------------------------------------------- corpus
#
# Manifest format:  <suite> <test> <march> <status> [env] [reason]
#
#   env    the environment variant to build the case with:
#            (empty) / -   the default (CSR-free) environment, tests/golden/env/
#            csr           the CSR-capable environment, tests/golden/env/csr/
#          The variant's include path is searched **first**, so its
#          `riscv_test.h` wins over the default one (STAGE-6 adjudication F).
#          ⚠️ An unknown value is a **hard error**, not a reason: a reason
#          that accidentally lands in the env column would otherwise be
#          read as a variant name and silently select the wrong env.
#   reason REQUIRED for `pending`: why it is out of scope + the ledger
#          entry that tracks it.  The harness validates that every
#          pending row carries one.
cases=()
while IFS= read -r line; do
    case "$line" in ''|'#'*) continue ;; esac
    # The reason column may contain spaces, so `read` is used (it puts
    # the remainder of the line into the last variable).
    read -r suite test_name march status env_col reason <<< "$line"
    if [ -z "$suite" ] || [ -z "$test_name" ] || [ -z "$march" ] || [ -z "$status" ]; then
        die "malformed manifest line: $line"
    fi
    case "$status" in
        active|pending) ;;
        *) die "unknown status '$status' in manifest line: $line" ;;
    esac
    # The env column: empty / `-` = the default variant, otherwise a
    # known variant name.  Anything else is a manifest bug.
    case "$env_col" in
        ''|'-') env_variant='' ;;
        csr)     env_variant='csr' ;;
        *) die "unknown env variant '$env_col' in manifest line: $line" ;;
    esac
    # A pending case must say why it is out of scope and which ledger
    # entry tracks it -- otherwise the exception silently outlives the
    # reason that justified it.
    #
    # ⚠️ The reason must **start with the ledger id** (`D-<n> …`).
    # Without that shape, a free-text reason whose first word happens
    # to be a known env variant name ("csr …") would be parsed as the
    # env column and silently select the wrong environment -- the guard
    # above cannot tell "the 5th column is an env" from "the 5th column
    # is prose" by looking at the word alone.
    if [ "$status" = "pending" ]; then
        if [ -z "$reason" ]; then
            die "pending case '$suite/$test_name' has no reason / ledger entry"
        fi
        case "$reason" in
            D-[0-9]*) ;;
            *) die "pending case '$suite/$test_name' must cite a ledger id (D-<n> …) first in its reason, got: $reason" ;;
        esac
    fi
    [ -n "$SUITE_FILTER" ] && [ "$suite" != "$SUITE_FILTER" ] && continue
    if [ -n "$FILTER" ]; then
        echo "$suite/$test_name" | grep -Eq "$FILTER" || continue
    fi
    cases+=("$suite|$test_name|$march|$status|$env_variant")
done < "$MANIFEST"

# NOTE: macOS ships bash 3.2, where expanding an **empty** array under
# `set -u` is a fatal "unbound variable" error.  The length guard has
# to come before any "${cases[@]}" expansion -- including the `--list`
# branch.
if [ "${#cases[@]}" -gt 0 ]; then
    if [ "$LIST_ONLY" -eq 1 ]; then
        for c in "${cases[@]}"; do
            echo "$c" | tr '|' ' '
        done
        exit 0
    fi
else
    die "no cases selected (empty corpus, or the --suite / --filter selection matched nothing)"
fi

# ---------------------------------------------------------------- build
build_args=(-p riscv_sim_kernel --bin riscv-golden)
[ "$RELEASE" -eq 1 ] && build_args+=(--release)
echo "==> building candidate runner (cargo ${build_args[*]})"
cargo build "${build_args[@]}" || die "cargo build failed"
# The runner lives in the **cargo target directory** — which is not
# necessarily the workspace's own `target/` (an isolated
# `CARGO_TARGET_DIR` is how parallel lanes avoid sharing a cargo
# lock).  Hard-coding `$REPO_ROOT/target` made the script run a stale
# binary (or none at all) whenever the target dir was overridden.
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
RUNNER="$TARGET_DIR/debug/riscv-golden"
[ "$RELEASE" -eq 1 ] && RUNNER="$TARGET_DIR/release/riscv-golden"

# ------------------------------------------------------- the RAM window
#
# ⚠️ **单一真相源 = 平台描述**（STAGE-19 裁决 I）：候选侧从**描述**取窗口
# 并**打印**它（`--print-memory`）⇒ ⇒ 这里**不写地址** ⇒ ⇒ 描述改了高段
# 大小 ⇒ ⇒ 候选侧**自动**跟着变 ⇒ ⇒ 参考侧**自动**跟着变 ⇒ ⇒ 不可能漂。
#
# ⚠️ **覆盖口优先**：`GOLDEN_MEM` 一旦给了 ⇒ ⇒ **不**用打印值（也不去问
# 候选侧）⇒ ⇒ 手工覆盖**永远压得住**自动值。
if [ -n "${GOLDEN_MEM:-}" ]; then
    MEM="$GOLDEN_MEM"
else
    CANDIDATE_MEM="$("$RUNNER" --print-memory)" || die "the candidate could not print its RAM window"
    # ⚠️ 形状必须是 `base:size` 且**十进制** ⇒ ⇒ Spike 的 `-m` 才吃得下。
    if ! printf '%s' "$CANDIDATE_MEM" | grep -Eq '^[0-9]+:[0-9]+$'; then
        die "the candidate printed an unusable RAM window: '$CANDIDATE_MEM'"
    fi
    MEM="$SPIKE_STACK_MEM,$CANDIDATE_MEM"
fi
echo "==> spike memory: $MEM"

mkdir -p "$OUT_DIR"

# Run a command with a wall-clock cap.  macOS has no coreutils `timeout`,
# so we use perl's alarm(), which survives the exec and kills the child
# with SIGALRM (observed as exit status 142).
run_with_timeout() {
    local secs="$1"; shift
    perl -e 'alarm shift; exec(@ARGV) or die "exec failed: $!"' "$secs" "$@"
}

is_timeout() { [ "$1" -eq 142 ] || [ "$1" -eq 124 ]; }

# The timeout must be a **positive integer number of seconds**.
#
# `perl -e 'alarm shift; ...'` truncates a fractional argument to 0, and
# `alarm(0)` *cancels* the alarm instead of arming it — a fractional
# `GOLDEN_TIMEOUT` would silently disable the timeout altogether.  Reject
# it loudly rather than run unbounded.
case "$TIMEOUT_SEC" in
    ''|*[!0-9]*) die "GOLDEN_TIMEOUT must be a positive integer number of seconds (got '$TIMEOUT_SEC')" ;;
esac
[ "$TIMEOUT_SEC" -ge 1 ] || die "GOLDEN_TIMEOUT must be >= 1 (got '$TIMEOUT_SEC')"

# Assemble one source file with the golden environment.
#
# build_case <src> <elf> <march> [env-variant]
#
# The env variant selects the include directory:
#   (empty)  the default environment (tests/golden/env/)
#   csr      the CSR-capable environment (tests/golden/env/csr/)
#
# ⚠️ The variant's include path comes **first** so that its `riscv_test.h`
# wins over the default one; the default directory stays on the path as
# well, because the variant's `encoding.h` is looked up as
# `../encoding.h` — a path relative to the *including file*, i.e. inside
# the default directory (STAGE-6 adjudication F).
#
# ⚠️ The linker script is the **default** one for every variant: upstream's
# `env/p/link.ld` and ours are line-for-line equivalent (same layout, same
# `.tohost` alignment), so a variant-specific script would be a copy.
build_case() {
    local src="$1" elf="$2" march="$3" env_variant="${4:-}"
    local env_dir="$ENV_DIR"
    [ -n "$env_variant" ] && env_dir="$ENV_DIR/$env_variant"
    "$GCC" -march="$march" -mabi=ilp32 -static -mcmodel=medany \
        -fvisibility=hidden -nostdlib -nostartfiles \
        -I"$env_dir" -I"$ENV_DIR" -I"$RISCV_TESTS_DIR/$MACRO_DIR_REL" \
        -T"$ENV_DIR/link.ld" "$src" -o "$elf"
}

# ---------------------------------------------------------------- compare
# compare_one <elf> -> "<spike-verdict> <our-verdict>"
compare_one() {
    local elf="$1" spike_verdict our_verdict

    run_with_timeout "$TIMEOUT_SEC" "$SPIKE" --isa="$ISA" -m"$MEM" "$elf" \
        > "${elf%.elf}.spike.log" 2>&1
    local spike_rc=$?
    if is_timeout "$spike_rc"; then
        spike_verdict="NOVERDICT"
    elif [ "$spike_rc" -eq 0 ]; then
        spike_verdict="PASS"
    else
        spike_verdict="FAIL $spike_rc"
    fi

    # The candidate gets a wall-clock cap too.  Its instruction budget
    # is the primary bound, but a budget of u64::MAX would otherwise let
    # the harness hang forever (review finding L-6).
    our_out="${elf%.elf}.ours.out"
    run_with_timeout "$TIMEOUT_SEC" "$RUNNER" "$elf" --budget "$BUDGET" \
        > "$our_out" 2>"${elf%.elf}.ours.log"
    local our_rc=$?
    if is_timeout "$our_rc"; then
        our_verdict="NOVERDICT"
    else
        our_verdict="$(sed -n 's/^VERDICT //p' "$our_out")"
        [ -n "$our_verdict" ] || our_verdict="ERROR"
    fi

    echo "$spike_verdict|$our_verdict"
}

# ---------------------------------------------------------------- self-check
#
# The mutation canaries.  A comparison harness that cannot fail is worse
# than no harness: if both sides always answered PASS, every corpus case
# would "agree" and the run would look green while proving nothing.
#
# Two canaries, both expected to report **FAIL 3** on both sides:
#
#   canary/fail_3.S   self-contained; proves the FAIL verdict encoding
#                     decodes to the right test number on both sides
#   mutated add.S     the upstream RV32I `add` case with its expected
#                     value for test 3 corrupted; proves a *behavioural*
#                     ISA difference is actually detected
#
# run_canaries -> 0 when every canary held, 1 otherwise.
#
# The canaries are **not** an optional extra: they run on every corpus
# pass as well as under `--self-check`.  The corpus itself contains only
# PASS cases, so without the canaries the FAIL-number path would be
# guarded by nothing at all in the default gate (review finding M-2).
run_canaries() {
    canary_fail=0

    check_canary() {
        local name="$1" elf="$2" expect="$3"
        local got
        got="$(compare_one "$elf")"
        local spike_verdict="${got%%|*}"
        local our_verdict="${got##*|}"
        if [ "$spike_verdict" = "$expect" ] && [ "$our_verdict" = "$expect" ]; then
            printf '  %-22s spike=%-10s ours=%-10s ok\n' "$name" "$spike_verdict" "$our_verdict"
        else
            printf '  %-22s spike=%-10s ours=%-10s FAILED (expected %s)\n' \
                "$name" "$spike_verdict" "$our_verdict" "$expect"
            canary_fail=1
        fi
    }

    # --- canary A: self-contained failing case ---
    elf_a="$OUT_DIR/canary-fail-3.elf"
    if build_case "$CANARY_DIR/fail_3.S" "$elf_a" rv32i_zifencei; then
        check_canary "canary/fail_3" "$elf_a" "FAIL 3"
    else
        printf '  %-22s BUILD FAILED\n' "canary/fail_3"
        canary_fail=1
    fi

    # --- canary B: upstream add, unmodified -- a positive control ---
    #
    # The two canaries above only exercise the FAIL path.  A positive
    # control closes the other direction: the harness must be able to
    # recognise a PASS on both sides, not just a FAIL.  Without it, a
    # harness that reported "FAIL" for everything would look perfectly
    # healthy.
    up_add_plain="$RISCV_TESTS_DIR/isa/rv64ui/add.S"
    if [ -f "$up_add_plain" ]; then
        elf_p="$OUT_DIR/canary-add-plain.elf"
        if build_case "$up_add_plain" "$elf_p" rv32i_zifencei; then
            check_canary "add (positive)" "$elf_p" "PASS"
        else
            printf '  %-22s BUILD FAILED\n' "add (positive)"
            canary_fail=1
        fi
    else
        printf '  %-22s SKIPPED (upstream add.S not found)\n' "add (positive)"
        canary_fail=1
    fi

    # --- canary C: upstream add with a corrupted expectation ---
    up_add="$RISCV_TESTS_DIR/isa/rv64ui/add.S"
    if [ -f "$up_add" ]; then
        mut_src="$OUT_DIR/canary-add-mutated.S"
        sed -E 's/(TEST_RR_OP\( *3, *add, *)0x[0-9a-fA-F]+/\10x00000003/' \
            "$up_add" > "$mut_src"
        elf_b="$OUT_DIR/canary-add-mutated.elf"
        if build_case "$mut_src" "$elf_b" rv32i_zifencei; then
            check_canary "mutated add" "$elf_b" "FAIL 3"
        else
            printf '  %-22s BUILD FAILED\n' "mutated add"
            canary_fail=1
        fi
    else
        # A skipped canary is NOT a passing canary.  AGENTS.md's criterion
        # says "both canaries must report FAIL 3" — a SKIP silently
        # degrades the proof from two independent checks to one without
        # anyone noticing.
        printf '  %-22s SKIPPED (upstream add.S not found)\n' "mutated add"
        canary_fail=1
    fi

    return "$canary_fail"
}

if [ "$SELF_CHECK" -eq 1 ]; then
    printf '\n%s\n' "== mutation self-check =="
    run_canaries
    if [ $? -ne 0 ]; then
        echo "self-check FAILED — the comparison is not discriminating."
        exit 1
    fi
    echo "self-check ok — both sides agree on a failing case (FAIL 3) and on a passing one (PASS)."
    exit 0
fi

[ "${#cases[@]}" -gt 0 ] || die "no cases selected"

# ---------------------------------------------------------------- corpus run
total=0; ok=0; now_passing=0; expected_gap=0; mismatch=0; build_failed=0; no_verdict=0
declare -a failures=()

printf '\n%s\n' "== mutation self-check =="
if ! run_canaries; then
    echo "self-check FAILED — the comparison is not discriminating; the corpus run is abandoned."
    exit 1
fi
echo "self-check ok — both sides agree on a failing case (FAIL 3) and on a passing one (PASS)."

printf '\n%-28s %-18s %-18s %s\n' "CASE" "SPIKE" "OURS" "RESULT"
printf '%s\n' "------------------------------------------------------------------------"

for c in "${cases[@]}"; do
    IFS='|' read -r suite test_name march status env_variant <<< "$c"
    total=$((total + 1))

    src="$RISCV_TESTS_DIR/isa/$suite/$test_name.S"
    if [ ! -f "$src" ] && [ "$suite" = "local" ]; then
        # The `local` suite is the repository's **own** self-contained
        # cases (`tests/golden/local/`) — the ones that cannot come
        # from the upstream riscv-tests tree because they test a
        # behaviour upstream does not have a case for (e.g. an unmapped
        # access trapping).  Every other suite resolves inside the
        # upstream tree, so this fallback can never shadow an upstream
        # case.
        src="$REPO_ROOT/tests/golden/local/$test_name.S"
    fi
    elf="$OUT_DIR/$suite-$test_name.elf"

    if [ ! -f "$src" ]; then
        printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "-" "-" "MISSING-SOURCE"
        build_failed=$((build_failed + 1))
        failures+=("$suite/$test_name: source missing")
        [ "$KEEP_GOING" -eq 1 ] || break
        continue
    fi

    if ! build_case "$src" "$elf" "$march" "$env_variant" > "$OUT_DIR/$suite-$test_name.build.log" 2>&1; then
        printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "-" "-" "BUILD-FAILED"
        build_failed=$((build_failed + 1))
        failures+=("$suite/$test_name: build failed")
        [ "$KEEP_GOING" -eq 1 ] || break
        continue
    fi

    got="$(compare_one "$elf")"
    spike_verdict="${got%%|*}"
    our_verdict="${got##*|}"

    if [ "$spike_verdict" = "$our_verdict" ] && [ "$status" = "pending" ]; then
        # A pending case that now agrees is a **stale flag**: the
        # extension it was waiting for has landed, so the case belongs
        # in the `active` set.  Failing the run is what keeps the flag
        # honest -- otherwise the exception silently outlives its
        # reason.
        printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "$spike_verdict" "$our_verdict" "NOW-PASSING"
        now_passing=$((now_passing + 1))
        failures+=("$suite/$test_name: pending case now agrees — flip its status to 'active'")
        [ "$KEEP_GOING" -eq 1 ] || break
    elif [ "$spike_verdict" = "NOVERDICT" ] || [ "$our_verdict" = "NOVERDICT" ]; then
        # A missing verdict is NOT an agreement.  Counting
        # NOVERDICT == NOVERDICT as "ok" is how a harness goes green
        # while both sides are simply broken — the reference side timed
        # out and the candidate ran out of budget, and the comparison
        # reported them as consistent.
        printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "$spike_verdict" "$our_verdict" "NO-VERDICT"
        no_verdict=$((no_verdict + 1))
        failures+=("$suite/$test_name: no verdict (spike=$spike_verdict ours=$our_verdict)")
        [ "$KEEP_GOING" -eq 1 ] || break
    elif [ "$spike_verdict" = "$our_verdict" ]; then
        if [ "$status" = "pending" ]; then
            printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "$spike_verdict" "$our_verdict" "NOW-PASSING"
            now_passing=$((now_passing + 1))
        else
            printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "$spike_verdict" "$our_verdict" "ok"
            ok=$((ok + 1))
        fi
    elif [ "$status" = "pending" ]; then
        # Expected gap: the case needs an ISA extension M-13 does not
        # implement yet.  **Only** "the candidate cannot execute it at
        # all" is tolerated.  Any other disagreement (e.g. Spike says
        # PASS and we say FAIL 3) is a real defect and must fail the
        # run; swallowing it under "expected gap" is exactly the kind
        # of hole this ledger exists to close.
        #
        # ⚠️ **白名单，不是黑名单**：可容忍的只有**执行之前**就失败的那几种
        #   ——读文件 / 解析镜像 / 找符号 / 建内存图 / 写段。这些是
        #   「harness 根本没跑起来」。
        #   **运行期**的失败（`step@…` = guest fault、`trap@…` = runner
        #   因为陷阱中止、`read tohost@…`）**一律不容忍** ⇒ 那是**行为**，
        #   不是「跑不了」⇒ 参考侧在那里是**陷入**并让 guest 的 handler
        #   继续跑（cause 5/7），我们却中止 ⇒ **真分歧**。
        #   ⇒ 用黑名单（「只排除 step@」）会漏掉 `trap@…` ⇒ 改白名单。
        tolerable=0
        case "$our_verdict" in
            NOVERDICT) tolerable=1 ;;
            "ERROR read:"*|"ERROR load:"*|"ERROR symbols:"*|\
            "ERROR map_ram:"*|"ERROR map_stack:"*|"ERROR load@"*) tolerable=1 ;;
        esac
        if [ "$tolerable" -eq 1 ]; then
            printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "$spike_verdict" "$our_verdict" "expected-gap"
            expected_gap=$((expected_gap + 1))
        else
            printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "$spike_verdict" "$our_verdict" "MISMATCH"
            mismatch=$((mismatch + 1))
            failures+=("$suite/$test_name: spike=$spike_verdict ours=$our_verdict (pending)")
            [ "$KEEP_GOING" -eq 1 ] || break
        fi
    else
        printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "$spike_verdict" "$our_verdict" "MISMATCH"
        mismatch=$((mismatch + 1))
        failures+=("$suite/$test_name: spike=$spike_verdict ours=$our_verdict")
        [ "$KEEP_GOING" -eq 1 ] || break
    fi
done

printf '\n%s\n' "------------------------------------------------------------------------"
printf 'total=%d ok=%d now-passing=%d expected-gap=%d mismatch=%d no-verdict=%d build-failed=%d\n' \
    "$total" "$ok" "$now_passing" "$expected_gap" "$mismatch" "$no_verdict" "$build_failed"

if [ "${#failures[@]}" -gt 0 ]; then
    printf '\n%s\n' "failures:"
    for f in "${failures[@]}"; do
        printf '  - %s\n' "$f"
    done
fi

printf '\nartifacts: %s\n' "$OUT_DIR"

if [ "$mismatch" -gt 0 ] || [ "$build_failed" -gt 0 ] || [ "$no_verdict" -gt 0 ] \
   || [ "$now_passing" -gt 0 ]; then
    exit 1
fi
exit 0
