#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_RUNNER=("$ROOT_DIR/cargo-dev.sh")

if [ ! -x "${CARGO_RUNNER[0]}" ]; then
    CARGO_RUNNER=(cargo)
fi

# shellcheck disable=SC1091
. "$ROOT_DIR/scripts/dev_build_common.sh"
# shellcheck disable=SC1091
. "$ROOT_DIR/scripts/verification_common.sh"

enable_local_tmpdir
enable_local_sccache
enable_local_cargo_job_limit

RUST_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|failures:|panicked|thread '.*' panicked|test result: FAILED|could not compile|aborting)"
ANDROID_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|FAILURE|Exception|What went wrong|BUILD FAILED)"
WEB_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|FAILURE|wasm-bindgen.*error|Build failed|could not compile)"
ROBOT_LOG_PATTERN="(\\[FAIL\\]|TIMEOUT|panicked|thread '.*' panicked|BUILD FAILED|FAILED=[1-9][0-9]*|FAILED_TESTS=.+)"

echo "Slot Table V2 verification"
echo "root: $ROOT_DIR"

run_logged "cargo fmt" "$ROOT_DIR/cargo_fmt.tmp" "$ROOT_DIR" "${CARGO_RUNNER[@]}" fmt
scan_log "cargo fmt" "$ROOT_DIR/cargo_fmt.tmp" "$RUST_LOG_PATTERN"

run_logged_timed_retry \
    "cargo test" \
    "$ROOT_DIR/1.tmp" \
    "$ROOT_DIR" \
    "${CRANPOSE_VERIFY_CARGO_TEST_TIMEOUT_SECS:-1800}" \
    "${CRANPOSE_VERIFY_CARGO_TEST_ATTEMPTS:-2}" \
    "${CARGO_RUNNER[@]}" test
scan_log "cargo test" "$ROOT_DIR/1.tmp" "$RUST_LOG_PATTERN"

run_logged_timed_retry \
    "cargo clippy" \
    "$ROOT_DIR/2.tmp" \
    "$ROOT_DIR" \
    "${CRANPOSE_VERIFY_CARGO_CLIPPY_TIMEOUT_SECS:-1200}" \
    "${CRANPOSE_VERIFY_CARGO_CLIPPY_ATTEMPTS:-2}" \
    "${CARGO_RUNNER[@]}" clippy --workspace --all-targets -- -D warnings
scan_log "cargo clippy" "$ROOT_DIR/2.tmp" "$RUST_LOG_PATTERN"

run_logged_timed_retry \
    "Android :app:assembleRelease" \
    "$ROOT_DIR/apps/android-demo/android/android_release.tmp" \
    "$ROOT_DIR/apps/android-demo/android" \
    "${CRANPOSE_VERIFY_ANDROID_TIMEOUT_SECS:-1200}" \
    "${CRANPOSE_VERIFY_ANDROID_ATTEMPTS:-2}" \
    ./gradlew :app:assembleRelease
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
