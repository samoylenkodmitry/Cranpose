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

RUST_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|failures:|panicked|thread '.*' panicked|test result: FAILED|could not compile|aborting)"
ANDROID_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|FAILURE|Exception|What went wrong|BUILD FAILED)"
WEB_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|FAILURE|wasm-bindgen.*error|Build failed|could not compile)"
ROBOT_LOG_PATTERN="(\\[FAIL\\]|TIMEOUT|panicked|thread '.*' panicked|BUILD FAILED|FAILED=[1-9][0-9]*|FAILED_TESTS=.+)"

echo "Slot Table V2 verification"
echo "root: $ROOT_DIR"
echo "cargo build jobs: $CARGO_BUILD_JOBS"
echo "rust test threads: $RUST_TEST_THREADS"
echo "gradle max workers: $GRADLE_MAX_WORKERS"
echo "robot jobs: 1"
echo "timeout: ${VERIFY_TIMEOUT_SECS}s"

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
echo "  $ROOT_DIR/cargo_fmt.tmp"
echo "  $ROOT_DIR/1.tmp"
echo "  $ROOT_DIR/2.tmp"
echo "  $ROOT_DIR/apps/android-demo/android/android_release.tmp"
echo "  $ROOT_DIR/apps/desktop-demo/web-build.tmp"
echo "  $ROOT_DIR/robot.tmp"
echo "  $ROOT_DIR/robot_test.log"
echo "  $ROOT_DIR/robot_test_summary.txt"
