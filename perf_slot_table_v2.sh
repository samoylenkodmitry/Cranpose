#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_RUNNER=("$SCRIPT_DIR/cargo-dev.sh")
# shellcheck disable=SC1091
. "$SCRIPT_DIR/scripts/dev_build_common.sh"

if [ ! -x "${CARGO_RUNNER[0]}" ]; then
    CARGO_RUNNER=(cargo)
fi

enable_local_tmpdir
enable_local_sccache
enable_local_cargo_job_limit

detect_default_cpu_set() {
    if [[ -n "${CRANPOSE_SLOT_TABLE_CPU_SET:-}" ]]; then
        printf '%s\n' "${CRANPOSE_SLOT_TABLE_CPU_SET}"
        return
    fi

    if ! command -v taskset >/dev/null 2>&1; then
        printf 'none\n'
        return
    fi

    local allowed first_group first_cpu
    allowed="$(taskset -pc $$ 2>/dev/null | awk -F': ' 'END { print $2 }')"
    if [[ -z "$allowed" ]]; then
        printf 'none\n'
        return
    fi

    first_group="${allowed%%,*}"
    first_cpu="${first_group%%-*}"
    if [[ -z "$first_cpu" ]]; then
        printf 'none\n'
        return
    fi

    printf '%s\n' "$first_cpu"
}

PROFILE=""
FILTER=""
SAVE_BASELINE=""
COMPARE_BASELINE=""
MEASUREMENT_TIME="${CRANPOSE_SLOT_TABLE_MEASUREMENT_TIME:-5}"
WARMUP_TIME="${CRANPOSE_SLOT_TABLE_WARMUP_TIME:-1}"
SAMPLE_SIZE="${CRANPOSE_SLOT_TABLE_SAMPLE_SIZE:-30}"
CPU_SET="$(detect_default_cpu_set)"
COOLDOWN_SECS="${CRANPOSE_SLOT_TABLE_COOLDOWN_SECS:-20}"
STABILITY_CHECK="0"
STABILITY_THRESHOLD_PCT="${CRANPOSE_SLOT_TABLE_STABILITY_THRESHOLD_PCT:-5}"
PLOT="0"

baseline_stamp_path() {
    local baseline_name="$1"
    printf 'target/criterion/.slot_table_v2_baseline_%s.stamp\n' "$baseline_name"
}

record_baseline_timestamp() {
    local baseline_name="$1"
    local stamp_path
    stamp_path="$(baseline_stamp_path "$baseline_name")"
    mkdir -p "$(dirname "$stamp_path")"
    printf '%s\n' "$(date +%s)" > "$stamp_path"
}

maybe_wait_for_cooldown() {
    local baseline_name="$1"
    local cooldown_secs="$2"
    if [[ -z "$baseline_name" || "$cooldown_secs" == "0" ]]; then
        return
    fi

    local stamp_path
    stamp_path="$(baseline_stamp_path "$baseline_name")"
    if [[ ! -f "$stamp_path" ]]; then
        return
    fi

    local saved_at
    saved_at="$(cat "$stamp_path")"
    if [[ -z "$saved_at" ]]; then
        return
    fi

    local now elapsed remaining
    now="$(date +%s)"
    elapsed=$((now - saved_at))
    remaining=$((cooldown_secs - elapsed))
    if (( remaining > 0 )); then
        echo "Cooling down benchmark host for ${remaining}s before comparing against baseline '$baseline_name'"
        sleep "$remaining"
    fi
}

run_bench() {
    local save_baseline="${1:-}"
    local compare_baseline="${2:-}"

    maybe_wait_for_cooldown "$compare_baseline" "$COOLDOWN_SECS"

    local -a cmd
    cmd=("${CARGO_RUNNER[@]}" bench --package cranpose-ui --bench slot_table_v2)

    if [[ -n "$PROFILE" ]]; then
        cmd+=(--profile "$PROFILE")
    fi

    if [[ -n "$FILTER" ]]; then
        cmd+=("$FILTER")
    fi

    cmd+=(
        --
        --warm-up-time "$WARMUP_TIME"
        --measurement-time "$MEASUREMENT_TIME"
        --sample-size "$SAMPLE_SIZE"
    )

    if [[ "$PLOT" != "1" ]]; then
        cmd+=(--noplot)
    fi

    if [[ -n "$save_baseline" ]]; then
        cmd+=(--save-baseline "$save_baseline")
    fi

    if [[ -n "$compare_baseline" ]]; then
        cmd+=(--baseline "$compare_baseline")
    fi

    if [[ -n "$CPU_SET" && "$CPU_SET" != "none" ]]; then
        if ! command -v taskset >/dev/null 2>&1; then
            echo "taskset is not installed or not on PATH." >&2
            exit 1
        fi
        cmd=(taskset -c "$CPU_SET" "${cmd[@]}")
    fi

    "${cmd[@]}"

    if [[ -n "$save_baseline" ]]; then
        record_baseline_timestamp "$save_baseline"
    fi
}

run_stability_check() {
    if ! command -v jq >/dev/null 2>&1; then
        echo "jq is required for --stability-check." >&2
        exit 1
    fi

    local baseline_name
    baseline_name="slot-table-stability-$(date +%s)-$$"

    echo "Running same-tree stability check with temporary baseline '$baseline_name'"
    run_bench "$baseline_name" ""
    run_bench "" "$baseline_name"

    local -a benchmark_dirs
    mapfile -t benchmark_dirs < <(
        find target/criterion -mindepth 1 -maxdepth 1 -type d \
            -exec test -f "{}/$baseline_name/benchmark.json" ';' -print | sort
    )

    if [[ ${#benchmark_dirs[@]} -eq 0 ]]; then
        echo "No Criterion benchmark artifacts found for stability baseline '$baseline_name'." >&2
        exit 1
    fi

    local failure=0
    local threshold_fraction
    threshold_fraction="$(awk -v pct="$STABILITY_THRESHOLD_PCT" 'BEGIN { printf "%.10f", pct / 100.0 }')"

    echo "Stability drift summary"
    for benchmark_dir in "${benchmark_dirs[@]}"; do
        local benchmark_name change_path mean_change abs_change
        benchmark_name="$(basename "$benchmark_dir")"
        change_path="$benchmark_dir/change/estimates.json"
        mean_change="$(jq -r '.mean.point_estimate' "$change_path")"
        abs_change="$(awk -v value="$mean_change" 'BEGIN { if (value < 0) value = -value; printf "%.6f", value }')"
        printf '  %s: %.2f%% drift\n' \
            "$benchmark_name" \
            "$(awk -v value="$abs_change" 'BEGIN { printf "%.2f", value * 100.0 }')"
        if awk -v value="$abs_change" -v limit="$threshold_fraction" 'BEGIN { exit !(value > limit) }'; then
            failure=1
        fi
    done

    if (( failure != 0 )); then
        echo "Stability check failed: drift exceeded ${STABILITY_THRESHOLD_PCT}%." >&2
        exit 1
    fi

    echo "Stability check passed: all drifts stayed within ${STABILITY_THRESHOLD_PCT}%."
}

usage() {
    cat <<EOF
Usage: $0 [--profile NAME] [--filter NAME] [--save-baseline NAME] [--baseline NAME] [--measurement-time SECS] [--warmup-time SECS] [--sample-size N] [--cpu-set LIST|none] [--cooldown-secs SECS] [--stability-check] [--stability-threshold-pct N] [--plot]

Runs the slot-table Criterion benchmark suite with stable defaults.

Benchmarks:
  slot_table_v2_keyed_list_reorder
  slot_table_v2_tab_switching
  slot_table_v2_subcompose_scrolling
  slot_table_v2_lazy_list_scroll_reuse

Examples:
  $0 --save-baseline main
  $0 --baseline main
  $0 --filter lazy_list_scroll_reuse --save-baseline lazy-list
  $0 --stability-check
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --profile)
            PROFILE="$2"
            shift 2
            ;;
        --filter)
            FILTER="$2"
            shift 2
            ;;
        --save-baseline)
            SAVE_BASELINE="$2"
            shift 2
            ;;
        --baseline)
            COMPARE_BASELINE="$2"
            shift 2
            ;;
        --measurement-time)
            MEASUREMENT_TIME="$2"
            shift 2
            ;;
        --warmup-time)
            WARMUP_TIME="$2"
            shift 2
            ;;
        --sample-size)
            SAMPLE_SIZE="$2"
            shift 2
            ;;
        --cpu-set)
            CPU_SET="$2"
            shift 2
            ;;
        --cooldown-secs)
            COOLDOWN_SECS="$2"
            shift 2
            ;;
        --stability-check)
            STABILITY_CHECK="1"
            shift
            ;;
        --stability-threshold-pct)
            STABILITY_THRESHOLD_PCT="$2"
            shift 2
            ;;
        --plot)
            PLOT="1"
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage
            exit 1
            ;;
    esac
done

if [[ "$STABILITY_CHECK" == "1" && ( -n "$SAVE_BASELINE" || -n "$COMPARE_BASELINE" ) ]]; then
    echo "--stability-check cannot be combined with --save-baseline or --baseline." >&2
    exit 1
fi

if [[ -n "$SAVE_BASELINE" && -n "$COMPARE_BASELINE" ]]; then
    echo "Use either --save-baseline or --baseline in one invocation, or use --stability-check." >&2
    exit 1
fi

echo "Running slot table perf baselines"
echo "  filter=${FILTER:-<all>}"
echo "  save_baseline=${SAVE_BASELINE:-<none>}"
echo "  compare_baseline=${COMPARE_BASELINE:-<none>}"
echo "  measurement_time=${MEASUREMENT_TIME}s"
echo "  warmup_time=${WARMUP_TIME}s"
echo "  sample_size=$SAMPLE_SIZE"
echo "  cpu_set=${CPU_SET:-<none>}"
echo "  cooldown_secs=${COOLDOWN_SECS}s"

if [[ "$STABILITY_CHECK" == "1" ]]; then
    run_stability_check
else
    run_bench "$SAVE_BASELINE" "$COMPARE_BASELINE"
fi
