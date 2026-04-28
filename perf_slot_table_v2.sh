#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_RUNNER=("$SCRIPT_DIR/cargo-dev.sh")
ORIGINAL_ARGS=("$@")
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

    printf 'none\n'
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
if [[ -n "${CRANPOSE_SLOT_TABLE_STABILITY_THRESHOLD_PCT+x}" ]]; then
    STABILITY_THRESHOLD_CUSTOM="1"
else
    STABILITY_THRESHOLD_CUSTOM="0"
fi
STABILITY_THRESHOLD_PCT="${CRANPOSE_SLOT_TABLE_STABILITY_THRESHOLD_PCT:-5}"
STABILITY_WARMUP_RUNS="${CRANPOSE_SLOT_TABLE_STABILITY_WARMUP_RUNS:-2}"
STABILITY_ATTEMPTS="${CRANPOSE_SLOT_TABLE_STABILITY_ATTEMPTS:-4}"
STABILITY_TIMEOUT_SECS="${CRANPOSE_SLOT_TABLE_STABILITY_TIMEOUT_SECS:-600}"
PLOT="0"
SIBLING_THRESHOLD_MATRIX="0"
SIBLING_THRESHOLD_VALUES="${CRANPOSE_SIBLING_INDEX_THRESHOLDS:-4 8 16 32 64}"
STABILITY_BENCHMARKS=(
    "slot_table_v2_keyed_reverse_16"
    "slot_table_v2_keyed_reverse_64"
    "slot_table_v2_keyed_reverse_256"
    "slot_table_v2_keyed_reverse_1024"
    "slot_table_v2_keyed_reverse_4096"
    "slot_table_v2_keyed_rotate_front_to_back_1024"
    "slot_table_v2_keyed_random_shuffle_1024_seed_1"
    "slot_table_v2_keyed_random_shuffle_1024_seed_2"
    "slot_table_v2_conditional_toggle_front_1024"
    "slot_table_v2_conditional_toggle_middle_1024"
    "slot_table_v2_conditional_toggle_end_1024"
    "slot_table_v2_tab_switch_16_payload_groups"
    "slot_table_v2_tab_switch_192_payload_groups"
    "slot_table_v2_tab_switch_1024_payload_groups"
    "slot_table_v2_subcompose_scrolling"
    "slot_table_v2_lazy_scroll_steady"
    "slot_table_v2_lazy_scroll_jump_near"
    "slot_table_v2_lazy_scroll_jump_far"
)

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
    if ! [[ "$STABILITY_WARMUP_RUNS" =~ ^[0-9]+$ ]]; then
        echo "CRANPOSE_SLOT_TABLE_STABILITY_WARMUP_RUNS must be a non-negative integer." >&2
        exit 1
    fi
    if ! [[ "$STABILITY_ATTEMPTS" =~ ^[1-9][0-9]*$ ]]; then
        echo "CRANPOSE_SLOT_TABLE_STABILITY_ATTEMPTS must be a positive integer." >&2
        exit 1
    fi

    if [[ -z "$FILTER" ]]; then
        run_full_stability_check
        return
    fi

    if run_filtered_stability_check_with_retries; then
        echo "Stability check passed: all same-tree regression confidence intervals stayed within threshold."
    else
        echo "Stability check failed: same-tree regression confidence exceeded the configured threshold." >&2
        exit 1
    fi
}

enforce_stability_timeout_guard() {
    if [[ "${CRANPOSE_SLOT_TABLE_STABILITY_TIMEOUT_GUARD:-0}" == "1" || "$STABILITY_TIMEOUT_SECS" == "0" ]]; then
        return
    fi
    if ! [[ "$STABILITY_TIMEOUT_SECS" =~ ^[1-9][0-9]*$ ]]; then
        echo "CRANPOSE_SLOT_TABLE_STABILITY_TIMEOUT_SECS must be 0 or a positive integer." >&2
        exit 1
    fi
    if ! command -v timeout >/dev/null 2>&1; then
        echo "timeout is required to enforce the slot-table stability budget." >&2
        exit 1
    fi
    export CRANPOSE_SLOT_TABLE_STABILITY_TIMEOUT_GUARD=1
    exec timeout --signal=KILL "${STABILITY_TIMEOUT_SECS}s" "$0" "${ORIGINAL_ARGS[@]}"
}

run_filtered_stability_check_with_retries() {
    local attempt
    for (( attempt = 1; attempt <= STABILITY_ATTEMPTS; attempt++ )); do
        if (( STABILITY_ATTEMPTS > 1 )); then
            echo "Running stability attempt ${attempt}/${STABILITY_ATTEMPTS}"
        fi
        if run_filtered_stability_check; then
            return 0
        fi
    done
    return 1
}

run_filtered_stability_check() {
    local baseline_name baseline_filter_suffix
    baseline_filter_suffix="${FILTER//[^[:alnum:]_]/_}"
    baseline_name="slot-table-stability-$(date +%s)-$$-${baseline_filter_suffix:-all}"

    echo "Running same-tree stability check with temporary baseline '$baseline_name'"
    if (( STABILITY_WARMUP_RUNS > 0 )); then
        local warmup_index
        for (( warmup_index = 1; warmup_index <= STABILITY_WARMUP_RUNS; warmup_index++ )); do
            echo "Running unrecorded stability warmup ${warmup_index}/${STABILITY_WARMUP_RUNS}"
            run_bench "" ""
        done
    fi
    run_bench "$baseline_name" ""
    local saved_cooldown_secs
    saved_cooldown_secs="$COOLDOWN_SECS"
    COOLDOWN_SECS=0
    run_bench "" "$baseline_name"
    COOLDOWN_SECS="$saved_cooldown_secs"

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
    echo "Stability change summary"
    for benchmark_dir in "${benchmark_dirs[@]}"; do
        local benchmark_name change_path mean_change lower_change regression_lower threshold_pct threshold_fraction
        benchmark_name="$(basename "$benchmark_dir")"
        change_path="$benchmark_dir/change/estimates.json"
        mean_change="$(jq -r '.mean.point_estimate' "$change_path")"
        lower_change="$(jq -r '.mean.confidence_interval.lower_bound' "$change_path")"
        regression_lower="$(awk -v value="$lower_change" 'BEGIN { if (value < 0) value = 0; printf "%.6f", value }')"
        threshold_pct="$(stability_threshold_pct_for_benchmark "$benchmark_name")"
        threshold_fraction="$(awk -v pct="$threshold_pct" 'BEGIN { printf "%.10f", pct / 100.0 }')"
        printf '  %s: %+.2f%% change (threshold %.2f%%)\n' \
            "$benchmark_name" \
            "$(awk -v value="$mean_change" 'BEGIN { printf "%.2f", value * 100.0 }')" \
            "$threshold_pct"
        if awk -v value="$regression_lower" -v limit="$threshold_fraction" 'BEGIN { exit !(value > limit) }'; then
            failure=1
        fi
    done

    return "$failure"
}

run_full_stability_check() {
    local original_filter failure benchmark_name
    original_filter="$FILTER"
    failure=0

    echo "Running full same-tree stability check with adjacent per-benchmark baselines"
    for benchmark_name in "${STABILITY_BENCHMARKS[@]}"; do
        FILTER="$benchmark_name"
        if ! run_filtered_stability_check_with_retries; then
            failure=1
        fi
    done
    FILTER="$original_filter"

    if (( failure != 0 )); then
        echo "Stability check failed: same-tree regression confidence exceeded the configured threshold." >&2
        exit 1
    fi

    echo "Stability check passed: all same-tree regression confidence intervals stayed within threshold."
}

stability_threshold_pct_for_benchmark() {
    local benchmark_name="$1"
    if [[ "$STABILITY_THRESHOLD_CUSTOM" == "1" ]]; then
        printf '%s\n' "$STABILITY_THRESHOLD_PCT"
        return
    fi

    case "$benchmark_name" in
        slot_table_v2_lazy_scroll_*|slot_table_v2_subcompose_scrolling)
            printf '7\n'
            ;;
        *)
            printf '%s\n' "$STABILITY_THRESHOLD_PCT"
            ;;
    esac
}

validate_sibling_threshold_matrix_values() {
    if [[ -z "${SIBLING_THRESHOLD_VALUES// /}" ]]; then
        echo "CRANPOSE_SIBLING_INDEX_THRESHOLDS must contain at least one positive integer." >&2
        exit 1
    fi

    local threshold
    for threshold in $SIBLING_THRESHOLD_VALUES; do
        if ! [[ "$threshold" =~ ^[1-9][0-9]*$ ]]; then
            echo "Invalid sibling index threshold '$threshold'. Use positive integers only." >&2
            exit 1
        fi
    done
}

run_sibling_threshold_matrix() {
    validate_sibling_threshold_matrix_values

    local -a filters=(
        "keyed_reverse_16"
        "keyed_reverse_64"
        "keyed_reverse_256"
        "keyed_reverse_1024"
        "keyed_rotate_front_to_back_1024"
        "keyed_random_shuffle_1024_seed_1"
    )

    local threshold filter target_dir
    for threshold in $SIBLING_THRESHOLD_VALUES; do
        target_dir="$SCRIPT_DIR/target/sibling-threshold-$threshold"
        for filter in "${filters[@]}"; do
            echo "Running sibling index threshold ${threshold} for ${filter}"
            (
                export CRANPOSE_SIBLING_INDEX_THRESHOLD="$threshold"
                export CARGO_TARGET_DIR="$target_dir"
                FILTER="$filter"
                run_bench "" ""
            )
        done
    done
}

usage() {
    cat <<EOF
Usage: $0 [--profile NAME] [--filter NAME] [--save-baseline NAME] [--baseline NAME] [--measurement-time SECS] [--warmup-time SECS] [--sample-size N] [--cpu-set LIST|none] [--cooldown-secs SECS] [--stability-check] [--stability-threshold-pct N] [--sibling-threshold-matrix] [--plot]

Runs the slot-table Criterion benchmark suite with stable defaults.

Environment:
  CRANPOSE_SIBLING_INDEX_THRESHOLD sets the compile-time sibling index threshold for one run.
  CRANPOSE_SIBLING_INDEX_THRESHOLDS sets the matrix values for --sibling-threshold-matrix.

Benchmarks:
  slot_table_v2_keyed_reverse_16
  slot_table_v2_keyed_reverse_64
  slot_table_v2_keyed_reverse_256
  slot_table_v2_keyed_reverse_1024
  slot_table_v2_keyed_reverse_4096
  slot_table_v2_keyed_rotate_front_to_back_1024
  slot_table_v2_keyed_random_shuffle_1024_seed_1
  slot_table_v2_keyed_random_shuffle_1024_seed_2
  slot_table_v2_conditional_toggle_front_1024
  slot_table_v2_conditional_toggle_middle_1024
  slot_table_v2_conditional_toggle_end_1024
  slot_table_v2_tab_switch_16_payload_groups
  slot_table_v2_tab_switch_192_payload_groups
  slot_table_v2_tab_switch_1024_payload_groups
  slot_table_v2_subcompose_scrolling
  slot_table_v2_lazy_scroll_steady
  slot_table_v2_lazy_scroll_jump_near
  slot_table_v2_lazy_scroll_jump_far

Examples:
  $0 --save-baseline main
  $0 --baseline main
  $0 --filter lazy_scroll_steady --save-baseline lazy-list
  $0 --stability-check
  $0 --sibling-threshold-matrix
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
        --sibling-threshold-matrix)
            SIBLING_THRESHOLD_MATRIX="1"
            shift
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

if [[ "$SIBLING_THRESHOLD_MATRIX" == "1" && ( -n "$SAVE_BASELINE" || -n "$COMPARE_BASELINE" || "$STABILITY_CHECK" == "1" || -n "$FILTER" ) ]]; then
    echo "--sibling-threshold-matrix cannot be combined with --filter, --save-baseline, --baseline, or --stability-check." >&2
    exit 1
fi

if [[ -n "$SAVE_BASELINE" && -n "$COMPARE_BASELINE" ]]; then
    echo "Use either --save-baseline or --baseline in one invocation, or use --stability-check." >&2
    exit 1
fi

if [[ "$STABILITY_CHECK" == "1" ]]; then
    enforce_stability_timeout_guard
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
    echo "  stability_warmup_runs=$STABILITY_WARMUP_RUNS"
    echo "  stability_attempts=$STABILITY_ATTEMPTS"
    echo "  stability_timeout=${STABILITY_TIMEOUT_SECS}s"
fi
echo "  sibling_threshold=${CRANPOSE_SIBLING_INDEX_THRESHOLD:-<default>}"

if [[ "$SIBLING_THRESHOLD_MATRIX" == "1" ]]; then
    echo "  sibling_threshold_matrix=$SIBLING_THRESHOLD_VALUES"
    run_sibling_threshold_matrix
elif [[ "$STABILITY_CHECK" == "1" ]]; then
    run_stability_check
else
    run_bench "$SAVE_BASELINE" "$COMPARE_BASELINE"
fi
