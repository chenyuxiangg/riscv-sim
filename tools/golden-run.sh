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
# The memory map **both** sides expose.
#
#   2147479552 = 0x7fff_f000   stack window, 4096 bytes
#   2147483648 = 0x80000000   RAM, 4 MiB
#
# Spike's multi-region `-m` parser only accepts **decimal** numbers
# (its single-region form does accept hex), hence the decimal literals.
#
# The two sides must expose the *same* addressable set: if the candidate
# mapped only the image's own span, a program touching an address
# outside the image but inside Spike's memory would fault on one side
# and run fine on the other — a false mismatch (review finding M-3).
MEM="${GOLDEN_MEM:-2147479552:4096,2147483648:4194304}"
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
        -h|--help)     sed -n '2,42p' "$0"; exit 0 ;;
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

# ---------------------------------------------------------------- corpus
cases=()
while IFS= read -r line; do
    case "$line" in ''|'#'*) continue ;; esac
    # The reason column may contain spaces, so `read` is used (it puts
    # the remainder of the line into the last variable).
    read -r suite test_name march status reason <<< "$line"
    if [ -z "$suite" ] || [ -z "$test_name" ] || [ -z "$march" ] || [ -z "$status" ]; then
        die "malformed manifest line: $line"
    fi
    case "$status" in
        active|pending) ;;
        *) die "unknown status '$status' in manifest line: $line" ;;
    esac
    # A pending case must say why it is out of scope and which ledger
    # entry tracks it -- otherwise the exception silently outlives the
    # reason that justified it.
    if [ "$status" = "pending" ] && [ -z "$reason" ]; then
        die "pending case '$suite/$test_name' has no reason / ledger entry"
    fi
    [ -n "$SUITE_FILTER" ] && [ "$suite" != "$SUITE_FILTER" ] && continue
    if [ -n "$FILTER" ]; then
        echo "$suite/$test_name" | grep -Eq "$FILTER" || continue
    fi
    cases+=("$suite|$test_name|$march|$status")
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
RUNNER="$REPO_ROOT/target/debug/riscv-golden"
[ "$RELEASE" -eq 1 ] && RUNNER="$REPO_ROOT/target/release/riscv-golden"

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
build_case() {
    local src="$1" elf="$2" march="$3"
    "$GCC" -march="$march" -mabi=ilp32 -static -mcmodel=medany \
        -fvisibility=hidden -nostdlib -nostartfiles \
        -I"$ENV_DIR" -I"$RISCV_TESTS_DIR/$MACRO_DIR_REL" \
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
    IFS='|' read -r suite test_name march status <<< "$c"
    total=$((total + 1))

    src="$RISCV_TESTS_DIR/isa/$suite/$test_name.S"
    elf="$OUT_DIR/$suite-$test_name.elf"

    if [ ! -f "$src" ]; then
        printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "-" "-" "MISSING-SOURCE"
        build_failed=$((build_failed + 1))
        failures+=("$suite/$test_name: source missing")
        [ "$KEEP_GOING" -eq 1 ] || break
        continue
    fi

    if ! build_case "$src" "$elf" "$march" > "$OUT_DIR/$suite-$test_name.build.log" 2>&1; then
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
        # all" is tolerated -- ERROR / NOVERDICT.  Any other
        # disagreement (e.g. Spike says PASS and we say FAIL 3) is a
        # real defect and must fail the run; swallowing it under
        # "expected gap" is exactly the kind of hole this ledger
        # exists to close.
        case "$our_verdict" in
            ERROR*|NOVERDICT)
                printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "$spike_verdict" "$our_verdict" "expected-gap"
                expected_gap=$((expected_gap + 1)) ;;
            *)
                printf '%-28s %-18s %-18s %s\n' "$suite/$test_name" "$spike_verdict" "$our_verdict" "MISMATCH"
                mismatch=$((mismatch + 1))
                failures+=("$suite/$test_name: spike=$spike_verdict ours=$our_verdict (pending)")
                [ "$KEEP_GOING" -eq 1 ] || break ;;
        esac
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
