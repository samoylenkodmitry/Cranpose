#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_RUNNER=("$SCRIPT_DIR/cargo-dev.sh")
# shellcheck disable=SC1091
. "$SCRIPT_DIR/scripts/dev_build_common.sh"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/scripts/perf_robot_common.sh"

if [ ! -x "${CARGO_RUNNER[0]}" ]; then
    CARGO_RUNNER=(cargo)
fi

enable_local_tmpdir
enable_local_sccache
enable_local_cargo_job_limit

PROFILE="native-release"
EXAMPLE="robot_perf_harness"
DURATION_SECS="8"
WARMUP_SECS="3"
MIN_FPS="${CRANPOSE_PERF_MIN_FPS:-120}"
MAX_P95_FRAME_MS="${CRANPOSE_PERF_MAX_P95_FRAME_MS:-8.33}"
MAX_50MS_STALLS="${CRANPOSE_PERF_MAX_50MS_STALLS:-0}"
MEM_VALIDATE="${CRANPOSE_MEM_VALIDATE:-0}"
PRESENT_MODE="${CRANPOSE_PRESENT_MODE:-immediate}"
HEADLESS="${CRANPOSE_HEADLESS:-0}"
WAIT_IDLE_AFTER_DRAG="${CRANPOSE_PERF_WAIT_IDLE_AFTER_DRAG:-0}"
TIMEOUT_SLACK_SECS="${CRANPOSE_PERF_TIMEOUT_SLACK_SECS:-60}"
OUTPUT="${CRANPOSE_PERF_FPS_OUTPUT:-${XDG_CACHE_HOME:-$HOME/.cache}/cranpose/perf/fps_gate_report.txt}"
PERF_SCENARIOS=()

usage() {
    cat <<EOF
Usage: $0 [--dev|--release|--profile NAME] [--example NAME] [--duration SECS] [--warmup SECS] [--min-fps FPS] [--max-p95-ms MS] [--max-50ms-stalls COUNT] [--output PATH] [--scenario NAME]

Runs the robot performance harness and fails when any selected scenario is not above the FPS budget.
Default budget: >120 FPS.
Scenarios: lazy_list_scroll, text_heavy_scroll, markdown_scroll, markdown_viewer_scroll, markdown_default_viewer_scroll, backdrop_blur, opaque_scene
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dev)
            PROFILE="dev"
            shift
            ;;
        --release)
            PROFILE="release"
            shift
            ;;
        --profile)
            PROFILE="$2"
            shift 2
            ;;
        --example)
            EXAMPLE="$2"
            shift 2
            ;;
        --duration)
            DURATION_SECS="$2"
            shift 2
            ;;
        --warmup)
            WARMUP_SECS="$2"
            shift 2
            ;;
        --min-fps)
            MIN_FPS="$2"
            shift 2
            ;;
        --max-p95-ms)
            MAX_P95_FRAME_MS="$2"
            shift 2
            ;;
        --max-50ms-stalls)
            MAX_50MS_STALLS="$2"
            shift 2
            ;;
        --output)
            OUTPUT="$2"
            shift 2
            ;;
        --scenario)
            append_perf_scenario "$2"
            shift 2
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

finalize_perf_scenarios

PROFILE_DIR="debug"
BUILD_ARGS=(--package desktop-app --example "$EXAMPLE" --features robot-app)

if [[ "$PROFILE" == "release" ]]; then
    PROFILE_DIR="release"
    BUILD_ARGS+=(--release)
elif [[ "$PROFILE" != "dev" ]]; then
    PROFILE_DIR="$PROFILE"
    BUILD_ARGS+=(--profile "$PROFILE")
fi

"${CARGO_RUNNER[@]}" build "${BUILD_ARGS[@]}"

BIN="target/${PROFILE_DIR}/examples/${EXAMPLE}"
if [[ ! -x "$BIN" ]]; then
    echo "Binary not found: $BIN"
    exit 1
fi

OUTPUT_DIR="$(dirname "$OUTPUT")"
mkdir -p "$OUTPUT_DIR"
OUTPUT_BASE="${OUTPUT%.*}"
if [[ "$OUTPUT_BASE" == "$OUTPUT" ]]; then
    OUTPUT_BASE="${OUTPUT}_artifacts"
fi

{
    echo "FPS perf gate"
    echo "profile=$PROFILE"
    echo "example=$EXAMPLE"
    echo "duration_secs=$DURATION_SECS"
    echo "warmup_secs=$WARMUP_SECS"
    echo "min_fps=$MIN_FPS"
    echo "max_p95_frame_ms=$MAX_P95_FRAME_MS"
    echo "max_50ms_stalls=$MAX_50MS_STALLS"
    echo "headless=$HEADLESS"
    echo "present_mode=$PRESENT_MODE"
    echo "wait_idle_after_drag=$WAIT_IDLE_AFTER_DRAG"
    echo "host=$(host_state_summary)"
    echo "scenarios=${PERF_SCENARIOS[*]}"
    echo
} > "$OUTPUT"

for scenario in "${PERF_SCENARIOS[@]}"; do
    LOG_FILE="${OUTPUT_BASE}_${scenario}.log"

    wait_for_host_capacity "fps perf scenario $scenario"
    echo "Running FPS perf scenario: $scenario"
    CRANPOSE_PERF_SCENARIO="$scenario" \
    CRANPOSE_PERF_DURATION_SECS="$DURATION_SECS" \
    CRANPOSE_PERF_WARMUP_SECS="$WARMUP_SECS" \
    CRANPOSE_PERF_TIMEOUT_SLACK_SECS="$TIMEOUT_SLACK_SECS" \
    CRANPOSE_PERF_MIN_FPS="$MIN_FPS" \
    CRANPOSE_PERF_MAX_P95_FRAME_MS="$MAX_P95_FRAME_MS" \
    CRANPOSE_PERF_MAX_50MS_STALLS="$MAX_50MS_STALLS" \
    CRANPOSE_MEM_VALIDATE="$MEM_VALIDATE" \
    CRANPOSE_PRESENT_MODE="$PRESENT_MODE" \
    CRANPOSE_HEADLESS="$HEADLESS" \
    CRANPOSE_PERF_WAIT_IDLE_AFTER_DRAG="$WAIT_IDLE_AFTER_DRAG" \
    "$BIN" 2>&1 | tee "$LOG_FILE"

    append_perf_summary_block "$OUTPUT" "$scenario" "$LOG_FILE"
    {
        echo "app_log=$LOG_FILE"
        echo
    } >> "$OUTPUT"
done

echo "report: $OUTPUT"
