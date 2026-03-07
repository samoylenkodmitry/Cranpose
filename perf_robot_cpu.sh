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

PROFILE="native-release"
EXAMPLE="robot_perf_harness"
DURATION_SECS="10"
OUTPUT="perf_report.txt"
MEM_VALIDATE="${CRANPOSE_MEM_VALIDATE:-1}"
PRESENT_MODE="${CRANPOSE_PRESENT_MODE:-immediate}"
HEADLESS="${CRANPOSE_HEADLESS:-0}"

usage() {
    cat <<EOF
Usage: $0 [--dev|--release|--profile NAME] [--example NAME] [--duration SECS] [--output PATH] [--no-mem]

Runs perf recording on a robot test binary and writes a text report.
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

CRANPOSE_PERF_DURATION_SECS="$DURATION_SECS" \
CRANPOSE_MEM_VALIDATE="$MEM_VALIDATE" \
CRANPOSE_PRESENT_MODE="$PRESENT_MODE" \
CRANPOSE_HEADLESS="$HEADLESS" \
perf record -F 997 -g --call-graph fp -o perf.data -- "$BIN"

perf report --stdio --percent-limit 1 --sort symbol,dso > "$OUTPUT"

echo "perf data: perf.data"
echo "report: $OUTPUT"
