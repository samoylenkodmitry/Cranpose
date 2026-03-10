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
DURATION_SECS="10"
OUTPUT="perf_report.txt"
MEM_VALIDATE="${CRANPOSE_MEM_VALIDATE:-1}"
PRESENT_MODE="${CRANPOSE_PRESENT_MODE:-immediate}"
HEADLESS="${CRANPOSE_HEADLESS:-0}"
PERF_SCENARIOS=()

usage() {
    cat <<EOF
Usage: $0 [--dev|--release|--profile NAME] [--example NAME] [--duration SECS] [--output PATH] [--scenario NAME] [--no-mem]

Runs perf recording on robot perf scenarios and writes a text report.
Scenarios: lazy_list_scroll, text_heavy_scroll, backdrop_blur, opaque_scene
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
        --output)
            OUTPUT="$2"
            shift 2
            ;;
        --scenario)
            append_perf_scenario "$2"
            shift 2
            ;;
        --no-mem)
            MEM_VALIDATE="0"
            shift
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

if ! command -v perf >/dev/null 2>&1; then
    echo "perf is not installed or not on PATH."
    exit 1
fi

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

if [[ "$PROFILE" != "dev" ]]; then
    PROFILE_ENV=$(echo "$PROFILE" | tr '[:lower:]-' '[:upper:]_')
    export "CARGO_PROFILE_${PROFILE_ENV}_DEBUG=1"
    export "CARGO_PROFILE_${PROFILE_ENV}_STRIP=none"
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
    echo "CPU perf summary"
    echo "profile=$PROFILE"
    echo "example=$EXAMPLE"
    echo "duration_secs=$DURATION_SECS"
    echo "scenarios=${PERF_SCENARIOS[*]}"
    echo
} > "$OUTPUT"

for scenario in "${PERF_SCENARIOS[@]}"; do
    DATA_FILE="${OUTPUT_BASE}_${scenario}.data"
    LOG_FILE="${OUTPUT_BASE}_${scenario}.log"
    REPORT_FILE="${OUTPUT_BASE}_${scenario}_perf_report.txt"

    echo "Running perf scenario: $scenario"
    CRANPOSE_PERF_SCENARIO="$scenario" \
    CRANPOSE_PERF_DURATION_SECS="$DURATION_SECS" \
    CRANPOSE_MEM_VALIDATE="$MEM_VALIDATE" \
    CRANPOSE_PRESENT_MODE="$PRESENT_MODE" \
    CRANPOSE_HEADLESS="$HEADLESS" \
    perf record -F 997 -g --call-graph fp -o "$DATA_FILE" -- "$BIN" \
        2>&1 | tee "$LOG_FILE"

    perf report --stdio --percent-limit 1 --sort symbol,dso -i "$DATA_FILE" > "$REPORT_FILE"

    append_perf_summary_block "$OUTPUT" "$scenario" "$LOG_FILE"
    {
        echo "perf_data=$DATA_FILE"
        echo "perf_report=$REPORT_FILE"
        echo "app_log=$LOG_FILE"
        echo
    } >> "$OUTPUT"
done

echo "report: $OUTPUT"
