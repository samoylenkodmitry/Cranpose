#!/usr/bin/env bash
set -euo pipefail

PROFILE="dev"
EXAMPLE="robot_perf_harness"
DURATION_SECS="10"
OUTPUT="perf_report.txt"
MEM_VALIDATE="${CRANPOSE_MEM_VALIDATE:-1}"
PRESENT_MODE="${CRANPOSE_PRESENT_MODE:-immediate}"

usage() {
    cat <<EOF
Usage: $0 [--dev|--release] [--example NAME] [--duration SECS] [--output PATH] [--no-mem]

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
    export CARGO_PROFILE_RELEASE_DEBUG=1
    export CARGO_PROFILE_RELEASE_STRIP=none
fi

cargo build "${BUILD_ARGS[@]}"

BIN="target/${PROFILE_DIR}/examples/${EXAMPLE}"
if [[ ! -x "$BIN" ]]; then
    echo "Binary not found: $BIN"
    exit 1
fi

CRANPOSE_PERF_DURATION_SECS="$DURATION_SECS" \
CRANPOSE_MEM_VALIDATE="$MEM_VALIDATE" \
CRANPOSE_PRESENT_MODE="$PRESENT_MODE" \
perf record -g --call-graph dwarf -o perf.data -- "$BIN"

perf report --stdio --percent-limit 1 --sort symbol,dso > "$OUTPUT"

echo "perf data: perf.data"
echo "report: $OUTPUT"
