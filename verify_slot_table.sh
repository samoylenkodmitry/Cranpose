#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_RUNNER=("$ROOT_DIR/cargo-dev.sh")

if [ ! -x "${CARGO_RUNNER[0]}" ]; then
    CARGO_RUNNER=(cargo)
fi

run_logged() {
    local label="$1"
    local log_file="$2"
    local work_dir="$3"
    shift 3

    mkdir -p "$(dirname "$log_file")"
    echo "==> $label"
    if (cd "$work_dir" && "$@") > "$log_file" 2>&1; then
        echo "ok: $label"
        echo "    log: $log_file"
    else
        echo "failed: $label" >&2
        echo "    log: $log_file" >&2
        tail -n 120 "$log_file" >&2 || true
        exit 1
    fi
}

scan_log() {
    local label="$1"
    local log_file="$2"
    local pattern="$3"

    if [ ! -f "$log_file" ]; then
        echo "missing log for $label: $log_file" >&2
        exit 1
    fi

    if command -v rg >/dev/null 2>&1; then
        if rg -n "$pattern" "$log_file"; then
            echo "verification issue found in $label log: $log_file" >&2
            exit 1
        fi
    elif grep -nE "$pattern" "$log_file"; then
        echo "verification issue found in $label log: $log_file" >&2
        exit 1
    fi
}

verify_robot_summary() {
    local summary_file="$ROOT_DIR/robot_test_summary.txt"

    if [ ! -f "$summary_file" ]; then
        echo "missing robot summary: $summary_file" >&2
        exit 1
    fi

    local total passed failed
    total="$(awk -F= '$1 == "TOTAL" { print $2 }' "$summary_file")"
    passed="$(awk -F= '$1 == "PASSED" { print $2 }' "$summary_file")"
    failed="$(awk -F= '$1 == "FAILED" { print $2 }' "$summary_file")"

    if [ -z "$total" ] || [ -z "$passed" ] || [ -z "$failed" ]; then
        echo "robot summary is malformed: $summary_file" >&2
        cat "$summary_file" >&2
        exit 1
    fi

    if [ "$failed" != "0" ] || [ "$total" != "$passed" ]; then
        echo "robot suite failed: total=$total passed=$passed failed=$failed" >&2
        cat "$summary_file" >&2
        exit 1
    fi
}

RUST_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|failures:|panicked|thread '.*' panicked|test result: FAILED|could not compile|aborting)"
ANDROID_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|FAILURE|Exception|What went wrong|BUILD FAILED)"
WEB_LOG_PATTERN="(^error|^warning|error:|warning:|FAILED|FAILURE|wasm-bindgen.*error|Build failed|could not compile)"
ROBOT_LOG_PATTERN="(\\[FAIL\\]|TIMEOUT|panicked|thread '.*' panicked|BUILD FAILED|FAILED=[1-9][0-9]*|FAILED_TESTS=.+)"

echo "Slot Table V2 verification"
echo "root: $ROOT_DIR"

run_logged "cargo fmt" "$ROOT_DIR/cargo_fmt.tmp" "$ROOT_DIR" "${CARGO_RUNNER[@]}" fmt
scan_log "cargo fmt" "$ROOT_DIR/cargo_fmt.tmp" "$RUST_LOG_PATTERN"

run_logged "cargo test" "$ROOT_DIR/1.tmp" "$ROOT_DIR" "${CARGO_RUNNER[@]}" test
scan_log "cargo test" "$ROOT_DIR/1.tmp" "$RUST_LOG_PATTERN"

run_logged "cargo clippy" "$ROOT_DIR/2.tmp" "$ROOT_DIR" "${CARGO_RUNNER[@]}" clippy --workspace --all-targets -- -D warnings
scan_log "cargo clippy" "$ROOT_DIR/2.tmp" "$RUST_LOG_PATTERN"

run_logged \
    "Android :app:assembleRelease" \
    "$ROOT_DIR/apps/android-demo/android/android_release.tmp" \
    "$ROOT_DIR/apps/android-demo/android" \
    ./gradlew :app:assembleRelease
scan_log "Android :app:assembleRelease" "$ROOT_DIR/apps/android-demo/android/android_release.tmp" "$ANDROID_LOG_PATTERN"

run_logged \
    "wasm build" \
    "$ROOT_DIR/apps/desktop-demo/web-build.tmp" \
    "$ROOT_DIR" \
    "$ROOT_DIR/apps/desktop-demo/build-web.sh"
scan_log "wasm build" "$ROOT_DIR/apps/desktop-demo/web-build.tmp" "$WEB_LOG_PATTERN"

run_logged \
    "robot e2e" \
    "$ROOT_DIR/robot.tmp" \
    "$ROOT_DIR" \
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
