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
# shellcheck disable=SC1091
. "$ROOT_DIR/scripts/robot_validation_common.sh"

ROBOT_LOG_PATTERN="(\\[FAIL\\]|TIMEOUT|panicked|thread '.*' panicked|BUILD FAILED|FAILED=[1-9][0-9]*|FAILED_TESTS=.+)"
STRESS_MODE="${CRANPOSE_SLOT_STRESS_MODE:-all}"
STRESS_PERF_SHARD=""
STRESS_ROBOT_SHARD=""

usage() {
    cat <<EOF
Usage: $0 [--all|--core|--perf-shard INDEX/TOTAL|--robot-build|--robot-shard INDEX/TOTAL]

Modes:
  --all                  Run the historical full stress path.
  --core                 Run slot validation and generated-frame model stress.
  --perf-shard N/M       Run slot-table perf stability shard N of M.
  --robot-build          Build all robot examples for later shards.
  --robot-shard N/M      Run robot shard N of M using existing binaries.

Environment:
  CRANPOSE_SLOT_STRESS_TIMEOUT_SECS defaults to 600. Set it to 0 only for an
  explicit uncapped manual run.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --all)
            STRESS_MODE="all"
            shift
            ;;
        --core)
            STRESS_MODE="core"
            shift
            ;;
        --perf-shard)
            STRESS_MODE="perf-shard"
            STRESS_PERF_SHARD="$2"
            shift 2
            ;;
        --robot-build)
            STRESS_MODE="robot-build"
            shift
            ;;
        --robot-shard)
            STRESS_MODE="robot-shard"
            STRESS_ROBOT_SHARD="$2"
            shift 2
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

if [ "$STRESS_MODE" = "perf-shard" ] && [ -z "$STRESS_PERF_SHARD" ]; then
    echo "--perf-shard requires INDEX/TOTAL" >&2
    exit 1
fi

if [ "$STRESS_MODE" = "robot-shard" ] && [ -z "$STRESS_ROBOT_SHARD" ]; then
    echo "--robot-shard requires INDEX/TOTAL" >&2
    exit 1
fi

echo "Slot Table V2 stress suite"
echo "root: $ROOT_DIR"
echo "model stress frames: $MODEL_STRESS_FRAMES"
echo "timeout: ${STRESS_TIMEOUT_SECS}s"
echo "mode: $STRESS_MODE"

run_stress_core() {
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
}

run_perf_shard() {
    local shard_slug="${STRESS_PERF_SHARD//\//_of_}"
    run_logged \
        "slot-table perf stability shard $STRESS_PERF_SHARD" \
        "$ROOT_DIR/slot_stress_perf_shard_${shard_slug}.tmp" \
        "$ROOT_DIR" \
        "$ROOT_DIR/perf_slot_table_v2.sh" --stability-check --stability-shard "$STRESS_PERF_SHARD"
}

print_core_logs() {
    echo "  $ROOT_DIR/slot_stress_validation.tmp"
    echo "  $ROOT_DIR/slot_stress_model.tmp"
}

case "$STRESS_MODE" in
    core)
        run_stress_core
        echo
        echo "Stress core passed. Logs:"
        print_core_logs
        ;;
    perf-shard)
        run_perf_shard
        ;;
    robot-build)
        run_robot_build_gate \
            "$ROOT_DIR" \
            "stress robot e2e build" \
            "$ROOT_DIR/slot_stress_robot_build.tmp" \
            "${CRANPOSE_SLOT_STRESS_ROBOT_BUILD_TIMEOUT_SECS:-600}" \
            "${CRANPOSE_SLOT_STRESS_ROBOT_ATTEMPTS:-2}"
        ;;
    robot-shard)
        run_robot_shard_gate \
            "$ROOT_DIR" \
            "stress" \
            "stress" \
            "$STRESS_ROBOT_SHARD" \
            "${CRANPOSE_SLOT_STRESS_ROBOT_SHARD_TIMEOUT_SECS:-600}" \
            "${CRANPOSE_SLOT_STRESS_ROBOT_ATTEMPTS:-2}"
        ;;
    all)
        run_stress_core
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
        print_core_logs
        echo "  $ROOT_DIR/slot_stress_perf.tmp"
        echo "  $ROOT_DIR/slot_stress_robot.tmp"
        echo "  $ROOT_DIR/robot_test.log"
        echo "  $ROOT_DIR/robot_test_summary.txt"
        ;;
esac
