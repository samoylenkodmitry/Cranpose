#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_RUNNER=("$ROOT_DIR/cargo-dev.sh")
STRESS_TIMEOUT_SECS="${CRANPOSE_SLOT_STRESS_TIMEOUT_SECS:-600}"

if [[ "${CRANPOSE_SLOT_STRESS_TIMEOUT_GUARD:-0}" != "1" && "$STRESS_TIMEOUT_SECS" != "0" ]]; then
    if ! [[ "$STRESS_TIMEOUT_SECS" =~ ^[1-9][0-9]*$ ]]; then
        echo "CRANPOSE_SLOT_STRESS_TIMEOUT_SECS must be 0 or a positive integer." >&2
        exit 1
    fi
    if ! command -v timeout >/dev/null 2>&1; then
        echo "timeout is required to enforce the stress-suite wall-clock budget." >&2
        exit 1
    fi
    export CRANPOSE_SLOT_STRESS_TIMEOUT_GUARD=1
    exec timeout --signal=KILL "${STRESS_TIMEOUT_SECS}s" "$0" "$@"
fi

if [ ! -x "${CARGO_RUNNER[0]}" ]; then
    CARGO_RUNNER=(cargo)
fi

MODEL_STRESS_FRAMES="${CRANPOSE_SLOT_MODEL_STRESS_FRAMES:-10000}"

# shellcheck disable=SC1091
. "$ROOT_DIR/scripts/verification_common.sh"

echo "Slot Table V2 stress suite"
echo "root: $ROOT_DIR"
echo "model stress frames: $MODEL_STRESS_FRAMES"
echo "timeout: ${STRESS_TIMEOUT_SECS}s"

run_logged \
    "slot validation workspace tests" \
    "$ROOT_DIR/slot_stress_validation.tmp" \
    "$ROOT_DIR" \
    env CRANPOSE_VALIDATE_SLOTS=1 "${CARGO_RUNNER[@]}" test --workspace

run_logged \
    "slot model generated-frame stress" \
    "$ROOT_DIR/slot_stress_model.tmp" \
    "$ROOT_DIR" \
    env CRANPOSE_SLOT_MODEL_STRESS_FRAMES="$MODEL_STRESS_FRAMES" \
        "${CARGO_RUNNER[@]}" test --release -p cranpose-core \
        deterministic_model_render_frames_match_slot_table -- --nocapture

run_logged \
    "slot-table perf stability" \
    "$ROOT_DIR/slot_stress_perf.tmp" \
    "$ROOT_DIR" \
    "$ROOT_DIR/perf_slot_table_v2.sh" --stability-check

run_logged \
    "sequential robot e2e" \
    "$ROOT_DIR/slot_stress_robot.tmp" \
    "$ROOT_DIR" \
    "$ROOT_DIR/run_robot_test.sh" --sequential
verify_robot_summary

echo
echo "Stress suite passed. Logs:"
echo "  $ROOT_DIR/slot_stress_validation.tmp"
echo "  $ROOT_DIR/slot_stress_model.tmp"
echo "  $ROOT_DIR/slot_stress_perf.tmp"
echo "  $ROOT_DIR/slot_stress_robot.tmp"
echo "  $ROOT_DIR/robot_test.log"
echo "  $ROOT_DIR/robot_test_summary.txt"
