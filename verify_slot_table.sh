#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_RUNNER=("$ROOT_DIR/cargo-dev.sh")
VERIFY_TIMEOUT_SECS="${CRANPOSE_SLOT_VERIFY_TIMEOUT_SECS:-600}"

if [[ "${CRANPOSE_SLOT_VERIFY_TIMEOUT_GUARD:-0}" != "1" && "$VERIFY_TIMEOUT_SECS" != "0" ]]; then
    if ! [[ "$VERIFY_TIMEOUT_SECS" =~ ^[1-9][0-9]*$ ]]; then
        echo "CRANPOSE_SLOT_VERIFY_TIMEOUT_SECS must be 0 or a positive integer." >&2
        exit 1
    fi
    if ! command -v timeout >/dev/null 2>&1; then
        echo "timeout is required to enforce the verification-suite wall-clock budget." >&2
        exit 1
    fi
    export CRANPOSE_SLOT_VERIFY_TIMEOUT_GUARD=1
    exec timeout --signal=KILL "${VERIFY_TIMEOUT_SECS}s" "$0" "$@"
fi

if [ ! -x "${CARGO_RUNNER[0]}" ]; then
    CARGO_RUNNER=(cargo)
fi

# shellcheck disable=SC1091
. "$ROOT_DIR/scripts/dev_build_common.sh"
# shellcheck disable=SC1091
. "$ROOT_DIR/scripts/verification_common.sh"
# shellcheck disable=SC1091
. "$ROOT_DIR/scripts/robot_validation_common.sh"

CRANPOSE_VERIFY_BUILD_JOBS="${CRANPOSE_VERIFY_BUILD_JOBS:-1}"
CRANPOSE_VERIFY_TEST_THREADS="${CRANPOSE_VERIFY_TEST_THREADS:-1}"
GRADLE_MAX_WORKERS="${CRANPOSE_VERIFY_GRADLE_WORKERS:-1}"
export CRANPOSE_BUILD_JOBS="${CRANPOSE_BUILD_JOBS:-$CRANPOSE_VERIFY_BUILD_JOBS}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-$CRANPOSE_BUILD_JOBS}"
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-$CRANPOSE_VERIFY_TEST_THREADS}"
export CRANPOSE_ROBOT_PARALLEL=1

enable_local_tmpdir
enable_local_sccache
enable_local_cargo_job_limit
enable_local_gradle_home

RUST_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|failures:|panicked|thread '.*' panicked|test result: FAILED|could not compile|aborting)"
ANDROID_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|FAILURE|Exception|What went wrong|BUILD FAILED)"
WEB_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|FAILURE|wasm-bindgen.*error|Build failed|could not compile)"
ROBOT_LOG_PATTERN="(\\[FAIL\\]|TIMEOUT|panicked|thread '.*' panicked|BUILD FAILED|FAILED=[1-9][0-9]*|FAILED_TESTS=.+)"

VERIFY_MODE="${CRANPOSE_SLOT_VERIFY_MODE:-all}"
VERIFY_ROBOT_SHARD=""

usage() {
    cat <<EOF
Usage: $0 [--all|--core|--robot-build|--robot-shard INDEX/TOTAL]

Modes:
  --all                  Run the historical full verification path.
  --core                 Run fmt, tests, clippy, Android release, and wasm.
  --robot-build          Build all robot examples for later shards.
  --robot-shard N/M      Run robot shard N of M using existing binaries.

Environment:
  CRANPOSE_SLOT_VERIFY_TIMEOUT_SECS defaults to 600. Set it to 0 only for an
  explicit uncapped manual run.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --all)
            VERIFY_MODE="all"
            shift
            ;;
        --core)
            VERIFY_MODE="core"
            shift
            ;;
        --robot-build)
            VERIFY_MODE="robot-build"
            shift
            ;;
        --robot-shard)
            VERIFY_MODE="robot-shard"
            VERIFY_ROBOT_SHARD="$2"
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

if [ "$VERIFY_MODE" = "robot-shard" ] && [ -z "$VERIFY_ROBOT_SHARD" ]; then
    echo "--robot-shard requires INDEX/TOTAL" >&2
    exit 1
fi

echo "Slot Table V2 verification"
echo "root: $ROOT_DIR"
echo "cargo build jobs: $CARGO_BUILD_JOBS"
echo "rust test threads: $RUST_TEST_THREADS"
echo "gradle max workers: $GRADLE_MAX_WORKERS"
echo "robot jobs: 1"
echo "timeout: ${VERIFY_TIMEOUT_SECS}s"
echo "mode: $VERIFY_MODE"

run_core_validation() {
    run_logged "cargo fmt" "$ROOT_DIR/cargo_fmt.tmp" "$ROOT_DIR" "${CARGO_RUNNER[@]}" fmt
    scan_log "cargo fmt" "$ROOT_DIR/cargo_fmt.tmp" "$RUST_LOG_PATTERN"

    run_logged_timed_retry \
        "cargo test" \
        "$ROOT_DIR/1.tmp" \
        "$ROOT_DIR" \
        "${CRANPOSE_VERIFY_CARGO_TEST_TIMEOUT_SECS:-1800}" \
        "${CRANPOSE_VERIFY_CARGO_TEST_ATTEMPTS:-2}" \
        "${CARGO_RUNNER[@]}" test --jobs "$CARGO_BUILD_JOBS" -- --test-threads "$RUST_TEST_THREADS"
    scan_log "cargo test" "$ROOT_DIR/1.tmp" "$RUST_LOG_PATTERN"

    run_logged_timed_retry \
        "cargo clippy" \
        "$ROOT_DIR/2.tmp" \
        "$ROOT_DIR" \
        "${CRANPOSE_VERIFY_CARGO_CLIPPY_TIMEOUT_SECS:-1200}" \
        "${CRANPOSE_VERIFY_CARGO_CLIPPY_ATTEMPTS:-2}" \
        "${CARGO_RUNNER[@]}" clippy --workspace --all-targets --jobs "$CARGO_BUILD_JOBS" -- -D warnings
    scan_log "cargo clippy" "$ROOT_DIR/2.tmp" "$RUST_LOG_PATTERN"

    run_logged_timed_retry \
        "Android :app:assembleRelease" \
        "$ROOT_DIR/apps/android-demo/android/android_release.tmp" \
        "$ROOT_DIR/apps/android-demo/android" \
        "${CRANPOSE_VERIFY_ANDROID_TIMEOUT_SECS:-1200}" \
        "${CRANPOSE_VERIFY_ANDROID_ATTEMPTS:-2}" \
        ./gradlew --no-parallel --max-workers="$GRADLE_MAX_WORKERS" :app:assembleRelease
    scan_log "Android :app:assembleRelease" "$ROOT_DIR/apps/android-demo/android/android_release.tmp" "$ANDROID_LOG_PATTERN"

    run_logged_timed_retry \
        "wasm build" \
        "$ROOT_DIR/apps/desktop-demo/web-build.tmp" \
        "$ROOT_DIR" \
        "${CRANPOSE_VERIFY_WASM_TIMEOUT_SECS:-1200}" \
        "${CRANPOSE_VERIFY_WASM_ATTEMPTS:-2}" \
        "$ROOT_DIR/apps/desktop-demo/build-web.sh"
    scan_log "wasm build" "$ROOT_DIR/apps/desktop-demo/web-build.tmp" "$WEB_LOG_PATTERN"
}

print_core_logs() {
    echo "  $ROOT_DIR/cargo_fmt.tmp"
    echo "  $ROOT_DIR/1.tmp"
    echo "  $ROOT_DIR/2.tmp"
    echo "  $ROOT_DIR/apps/android-demo/android/android_release.tmp"
    echo "  $ROOT_DIR/apps/desktop-demo/web-build.tmp"
}

case "$VERIFY_MODE" in
    core)
        run_core_validation
        echo
        echo "Verification core passed. Logs:"
        print_core_logs
        ;;
    robot-build)
        run_robot_build_gate \
            "$ROOT_DIR" \
            "robot e2e build" \
            "$ROOT_DIR/robot_build.tmp" \
            "${CRANPOSE_VERIFY_ROBOT_BUILD_TIMEOUT_SECS:-600}" \
            "${CRANPOSE_VERIFY_ROBOT_ATTEMPTS:-2}"
        echo
        echo "Verification robot build passed. Logs:"
        echo "  $ROOT_DIR/robot_build.tmp"
        ;;
    robot-shard)
        run_robot_shard_gate \
            "$ROOT_DIR" \
            "verification" \
            "verify" \
            "$VERIFY_ROBOT_SHARD" \
            "${CRANPOSE_VERIFY_ROBOT_SHARD_TIMEOUT_SECS:-600}" \
            "${CRANPOSE_VERIFY_ROBOT_ATTEMPTS:-2}"
        ;;
    all)
        run_core_validation
        run_logged_timed_retry \
            "robot e2e" \
            "$ROOT_DIR/robot.tmp" \
            "$ROOT_DIR" \
            "${CRANPOSE_VERIFY_ROBOT_TIMEOUT_SECS:-5400}" \
            "${CRANPOSE_VERIFY_ROBOT_ATTEMPTS:-2}" \
            "$ROOT_DIR/run_robot_test.sh" --sequential
        verify_robot_summary
        scan_log "robot e2e" "$ROOT_DIR/robot.tmp" "$ROBOT_LOG_PATTERN"
        scan_log "robot e2e full log" "$ROOT_DIR/robot_test.log" "$ROBOT_LOG_PATTERN"

        echo
        echo "Verification passed. Logs:"
        print_core_logs
        echo "  $ROOT_DIR/robot.tmp"
        echo "  $ROOT_DIR/robot_test.log"
        echo "  $ROOT_DIR/robot_test_summary.txt"
        ;;
esac
