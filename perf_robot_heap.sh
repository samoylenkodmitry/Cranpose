#!/usr/bin/env bash
set -euo pipefail

PROFILE="release"
EXAMPLE="robot_perf_harness"
DURATION_SECS="10"
OUTPUT_PREFIX="heaptrack"
MEM_VALIDATE="${CRANPOSE_MEM_VALIDATE:-1}"
PRESENT_MODE="${CRANPOSE_PRESENT_MODE:-immediate}"

usage() {
    cat <<EOF
Usage: $0 [--dev|--release] [--example NAME] [--duration SECS] [--output-prefix NAME]

Runs heaptrack on a robot test binary and prints allocation hotspots.
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
        --output-prefix)
            OUTPUT_PREFIX="$2"
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

if ! command -v heaptrack >/dev/null 2>&1; then
    echo "heaptrack is not installed or not on PATH."
    exit 1
fi

if ! command -v heaptrack_print >/dev/null 2>&1; then
    echo "heaptrack_print is not installed or not on PATH."
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

OUTPUT_FILE="${OUTPUT_PREFIX}.data"

CRANPOSE_PERF_DURATION_SECS="$DURATION_SECS" \
CRANPOSE_MEM_VALIDATE="$MEM_VALIDATE" \
CRANPOSE_PRESENT_MODE="$PRESENT_MODE" \
heaptrack --output "$OUTPUT_FILE" "$BIN"

heaptrack_print "$OUTPUT_FILE" | tee "${OUTPUT_PREFIX}_report.txt"
