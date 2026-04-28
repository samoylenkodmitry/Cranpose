#!/bin/bash

# Robot test runner with parallel execution support
# Runs all robot tests in headless mode
# Usage: ./run_robot_test.sh [--parallel N] [--sequential]
#        ./run_robot_test.sh [--example robot_name]
# 
# Options:
#   --parallel N    Run N tests in parallel
#   --sequential    Run tests one at a time (default)
#   --example NAME  Run only the named robot example (repeatable)
#   --help          Show this help message

LOG_FILE="robot_test.log"
SUMMARY_FILE="robot_test_summary.txt"
ROBOT_DIR="apps/desktop-demo/robot-runners"
ROBOT_PROFILE="${CRANPOSE_ROBOT_PROFILE:-robot}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_RUNNER=("$SCRIPT_DIR/cargo-dev.sh")
# shellcheck disable=SC1091
. "$SCRIPT_DIR/scripts/dev_build_common.sh"

if [ ! -x "${CARGO_RUNNER[0]}" ]; then
    CARGO_RUNNER=(cargo)
fi

# Default to sequential execution for stability. Parallel runs remain opt-in.
if [ -n "${CRANPOSE_ROBOT_PARALLEL:-}" ]; then
    PARALLEL_JOBS="$CRANPOSE_ROBOT_PARALLEL"
else
    PARALLEL_JOBS=1
fi
ROBOT_TEST_TIMEOUT_CAP_SECS="${CRANPOSE_ROBOT_TEST_TIMEOUT_CAP_SECS:-900}"
ROBOT_TEST_ATTEMPTS="${CRANPOSE_ROBOT_TEST_ATTEMPTS:-1}"
ROBOT_FAILURE_LOG_LINES="${CRANPOSE_ROBOT_FAILURE_LOG_LINES:-220}"
SELECTED_EXAMPLES=()

profile_output_dir() {
    case "$1" in
        dev)
            echo "debug"
            ;;
        release)
            echo "release"
            ;;
        *)
            echo "$1"
            ;;
    esac
}

PROFILE_DIR=$(profile_output_dir "$ROBOT_PROFILE")
if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
    EXAMPLE_BIN_DIR="target/$CARGO_BUILD_TARGET/$PROFILE_DIR/examples"
else
    EXAMPLE_BIN_DIR="target/$PROFILE_DIR/examples"
fi

if ! [[ "$ROBOT_FAILURE_LOG_LINES" =~ ^[1-9][0-9]*$ ]]; then
    ROBOT_FAILURE_LOG_LINES=220
fi

enable_local_tmpdir
enable_local_sccache
enable_local_cargo_job_limit

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --parallel)
            PARALLEL_JOBS="$2"
            shift 2
            ;;
        --sequential)
            PARALLEL_JOBS=1
            shift
            ;;
        --example)
            SELECTED_EXAMPLES+=("$2")
            shift 2
            ;;
        --help)
            echo "Usage: $0 [--parallel N] [--sequential] [--example robot_name]"
            echo ""
            echo "Options:"
            echo "  --parallel N    Run N tests in parallel"
            echo "  --sequential    Run tests one at a time (default)"
            echo "  --example NAME  Run only the named robot example (repeatable)"
            echo "  --help          Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Clean previous logs
rm -f "$LOG_FILE" "$SUMMARY_FILE"

echo "Cleaning up..."
# Dynamically discover all robot tests from the robot-runners directory
# Exclude utility modules (files that don't have a main function)
EXAMPLES=()
RUN_EXAMPLES=()
for file in "$ROBOT_DIR"/robot_*.rs; do
    if [ -f "$file" ]; then
        # Extract the example name (filename without .rs extension)
        example=$(basename "$file" .rs)
        # Skip utility modules (they don't have fn main)
        if [ "$example" = "robot_test_utils" ]; then
            continue
        fi
        EXAMPLES+=("$example")
    fi
done

if [ ${#EXAMPLES[@]} -eq 0 ]; then
    echo "No robot tests found in $ROBOT_DIR" | tee -a "$LOG_FILE"
    exit 1
fi

if [ ${#SELECTED_EXAMPLES[@]} -gt 0 ]; then
    FILTERED_EXAMPLES=()
    for requested in "${SELECTED_EXAMPLES[@]}"; do
        found=false
        for example in "${EXAMPLES[@]}"; do
            if [ "$example" = "$requested" ]; then
                FILTERED_EXAMPLES+=("$example")
                found=true
                break
            fi
        done
        if [ "$found" = false ]; then
            echo "Unknown robot example: $requested" | tee -a "$LOG_FILE"
            exit 1
        fi
    done
    EXAMPLES=("${FILTERED_EXAMPLES[@]}")
fi

BUILD_ARGS=(--profile "$ROBOT_PROFILE" --package desktop-app --features robot-app)
if [ ${#EXAMPLES[@]} -eq 1 ]; then
    BUILD_ARGS+=(--example "${EXAMPLES[0]}")
else
    BUILD_ARGS+=(--examples)
fi

echo "Building desktop-app examples with profile '$ROBOT_PROFILE'..."
if ! wait_for_host_capacity "robot build"; then
    echo "Host was not ready for robot build." | tee -a "$LOG_FILE"
    exit 1
fi
"${CARGO_RUNNER[@]}" build "${BUILD_ARGS[@]}" 2>&1 | tee -a "$LOG_FILE"

if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo "Build failed!" | tee -a "$LOG_FILE"
    exit 1
fi

echo "============================================" | tee -a "$LOG_FILE"
echo "Running Robot Test Suite" | tee -a "$LOG_FILE"
echo "Found ${#EXAMPLES[@]} robot tests" | tee -a "$LOG_FILE"
if [ "$PARALLEL_JOBS" -gt 1 ]; then
    echo "Running $PARALLEL_JOBS tests in parallel (headless mode)" | tee -a "$LOG_FILE"
else
    echo "Running tests sequentially" | tee -a "$LOG_FILE"
fi
echo "Log file: $LOG_FILE" | tee -a "$LOG_FILE"
echo "============================================" | tee -a "$LOG_FILE"

# Create temp directory for individual test results
RESULTS_DIR="$(create_local_temp_dir cranpose-robot-results)"
cleanup_results_dir() {
    if [ -d "$RESULTS_DIR" ]; then
        rm -r -- "$RESULTS_DIR"
    fi
}
trap cleanup_results_dir EXIT

# Function to run a single test
run_test() {
    local example="$1"
    local result_file="$RESULTS_DIR/$example.result"
    local output_file="$RESULTS_DIR/$example.output"
    local example_bin="$EXAMPLE_BIN_DIR/$example"

    if [ ! -x "$example_bin" ] && [ -x "${example_bin}.exe" ]; then
        example_bin="${example_bin}.exe"
    fi

    if [ ! -x "$example_bin" ]; then
        echo "FAIL:missing_binary" > "$result_file"
        {
            echo "Expected robot binary not found:"
            echo "  $example_bin"
        } > "$output_file"
        return
    fi
    
    # Run with timeout, capture exit code and output
    local timeout_secs=60
    case "$example" in
        robot_text_input)
            timeout_secs=120
            ;;
        robot_text_scroll_exact_external_contract)
            timeout_secs=180
            ;;
        robot_content_type_reuse|robot_lazy_perf_validation)
            timeout_secs=240
            ;;
        robot_fling_edge_cases)
            timeout_secs=240
            ;;
        robot_fling_precise)
            timeout_secs=180
            ;;
        robot_fling_interrupt)
            timeout_secs=180
            ;;
        robot_hacker_news_scroll)
            timeout_secs=240
            ;;
        robot_hacker_news_back_drag_scroll)
            timeout_secs=420
            ;;
        robot_markdown_end_drag_up)
            timeout_secs=600
            ;;
        robot_lazy_varheight_lifecycle)
            timeout_secs=150
            ;;
        robot_lazy_max_jump)
            timeout_secs=180
            ;;
        robot_lazy_scroll_edge_cases)
            timeout_secs=240
            ;;
        robot_no_fling_recording2)
            timeout_secs=240
            ;;
        robot_lazy_list_after_modifiers|robot_tab_navigation)
            timeout_secs=120
            ;;
        robot_tab_scroll)
            timeout_secs=120
            ;;
        robot_measure_shaders)
            timeout_secs=180
            ;;
        robot_shader_backdrop_drag)
            timeout_secs=300
            ;;
        robot_shadow_fields)
            timeout_secs=180
            ;;
        robot_scroll_decoration_invariance)
            timeout_secs=180
            ;;
        robot_modifier_render|robot_offset_test)
            timeout_secs=180
            ;;
        robot_render_translation_contract)
            timeout_secs=120
            ;;
        robot_double_click|robot_multiline_click|robot_multiline_nav)
            timeout_secs=90
            ;;
        robot_memory_leak)
            timeout_secs=900
            ;;
    esac

    if [[ "$ROBOT_TEST_TIMEOUT_CAP_SECS" =~ ^[1-9][0-9]*$ ]] \
        && [ "$timeout_secs" -gt "$ROBOT_TEST_TIMEOUT_CAP_SECS" ]; then
        timeout_secs="$ROBOT_TEST_TIMEOUT_CAP_SECS"
    fi

    local max_attempts="$ROBOT_TEST_ATTEMPTS"
    if ! [[ "$max_attempts" =~ ^[1-9][0-9]*$ ]]; then
        max_attempts=1
    fi

    local headless_env="CRANPOSE_HEADLESS=1"
    case "$example" in
        robot_text_scroll_exact_external_contract)
            headless_env="CRANPOSE_HEADLESS=0"
            ;;
    esac

    : > "$output_file"

    local attempt=1
    while [ "$attempt" -le "$max_attempts" ]; do
        local attempt_output="$RESULTS_DIR/$example.attempt.$attempt.output"
        {
            echo "Attempt $attempt/$max_attempts for $example (timeout=${timeout_secs}s)"
            echo "Host before attempt: $(host_state_summary)"
        } >> "$output_file"

        if ! wait_for_host_capacity "robot $example attempt $attempt" >> "$output_file" 2>&1; then
            echo "FAIL:host_not_ready" > "$result_file"
            return 75
        fi

        if command -v timeout >/dev/null 2>&1; then
            env "$headless_env" timeout --kill-after=15s "${timeout_secs}s" "$example_bin" > "$attempt_output" 2>&1
            local exit_code=$?
        else
            env "$headless_env" "$example_bin" > "$attempt_output" 2>&1
            local exit_code=$?
        fi

        cat "$attempt_output" >> "$output_file" 2>/dev/null

        local fail_in_log=false
        if grep -qiE '\bFAIL\b|\bFATAL\b|panicked' "$attempt_output"; then
            if grep -iE '\bFAIL\b|\bFATAL\b|panicked' "$attempt_output" | grep -qvE '^[[:space:]]*0 failed|test result:.*0 failed'; then
                fail_in_log=true
            fi
        fi

        if [ $exit_code -eq 0 ] && [ "$fail_in_log" = false ]; then
            echo "PASS" > "$result_file"
            return 0
        fi

        local reason=""
        [ $exit_code -ne 0 ] && reason="exit=$exit_code"
        [ "$fail_in_log" = true ] && reason="${reason:+$reason, }fail_in_log"
        if [ "$exit_code" = "124" ] || [ "$exit_code" = "137" ]; then
            reason="${reason:+$reason, }timeout=${timeout_secs}s"
            {
                echo "Attempt $attempt timed out or was killed."
                echo "Host after timeout: $(host_state_summary)"
            } >> "$output_file"
            if [ "$attempt" -lt "$max_attempts" ]; then
                attempt=$((attempt + 1))
                continue
            fi
            echo "FAIL:$reason" > "$result_file"
            return 75
        fi

        echo "FAIL:$reason" > "$result_file"
        return 0
    done
}

export -f run_test
export -f is_ci_env
export -f host_cpu_min_mhz
export -f host_cpu_freq_summary
export -f host_max_temp_c
export -f host_state_summary
export -f number_lt
export -f number_gt
export -f wait_for_host_capacity
export RESULTS_DIR
export EXAMPLE_BIN_DIR
export ROBOT_TEST_TIMEOUT_CAP_SECS
export ROBOT_TEST_ATTEMPTS

print_failure_excerpt() {
    local example="$1"
    local output_file="$RESULTS_DIR/$example.output"

    echo "" | tee -a "$LOG_FILE"
    echo "First failed test output ($example, first $ROBOT_FAILURE_LOG_LINES lines):" | tee -a "$LOG_FILE"
    echo "--------------------------------------------------" | tee -a "$LOG_FILE"
    if [ -s "$output_file" ]; then
        head -n "$ROBOT_FAILURE_LOG_LINES" "$output_file" | tee -a "$LOG_FILE"
    else
        echo "No captured output for $example" | tee -a "$LOG_FILE"
    fi
    echo "--------------------------------------------------" | tee -a "$LOG_FILE"
}

# Run tests in parallel using xargs or GNU parallel
STOPPED_EARLY=0
if [ "$PARALLEL_JOBS" -gt 1 ]; then
    RUN_EXAMPLES=("${EXAMPLES[@]}")
    # Use xargs for parallel execution
    if ! printf '%s\n' "${EXAMPLES[@]}" | xargs -P "$PARALLEL_JOBS" -I {} bash -c 'run_test "$@"' _ {}; then
        echo "One or more robot workers stopped before completing their assigned examples" | tee -a "$LOG_FILE"
        STOPPED_EARLY=1
    fi
else
    # Sequential execution with progress
    for example in "${EXAMPLES[@]}"; do
        RUN_EXAMPLES+=("$example")
        echo "Running $example..." | tee -a "$LOG_FILE"
        if run_test "$example"; then
            run_status=0
        else
            run_status=$?
        fi
        if [ "$run_status" -eq 75 ]; then
            echo "Stopping robot suite after host or timeout limit for $example" | tee -a "$LOG_FILE"
            STOPPED_EARLY=1
            break
        fi
        if [ "$run_status" -ne 0 ]; then
            echo "Robot runner returned status $run_status for $example" | tee -a "$LOG_FILE"
        fi
        sleep 0.1
    done
fi

# Wait for all tests to complete and gather results
PASSED=0
FAILED=0
FAILED_TESTS=()
RETRYABLE_FAILURE=0

for example in "${RUN_EXAMPLES[@]}"; do
    result_file="$RESULTS_DIR/$example.result"
    output_file="$RESULTS_DIR/$example.output"
    
    # Wait for result file (in case of race condition)
    wait_count=0
    while [ ! -f "$result_file" ] && [ $wait_count -lt 600 ]; do
        sleep 0.1
        ((wait_count++))
    done
    
    # Append output to main log
    echo "--------------------------------------------------" >> "$LOG_FILE"
    echo "Test: $example" >> "$LOG_FILE"
    echo "--------------------------------------------------" >> "$LOG_FILE"
    cat "$output_file" >> "$LOG_FILE" 2>/dev/null
    
    if [ -f "$result_file" ]; then
        result=$(cat "$result_file")
        if [ "$result" = "PASS" ]; then
            echo "  [PASS] $example" | tee -a "$LOG_FILE"
            ((PASSED++))
        else
            reason="${result#FAIL:}"
            echo "  [FAIL] $example ($reason)" | tee -a "$LOG_FILE"
            ((FAILED++))
            FAILED_TESTS+=("$example")
            case "$reason" in
                host_not_ready|*timeout=*|*exit=124*|*exit=137*)
                    RETRYABLE_FAILURE=1
                    ;;
            esac
        fi
    else
        echo "  [FAIL] $example (no result)" | tee -a "$LOG_FILE"
        ((FAILED++))
        FAILED_TESTS+=("$example")
    fi
done

# Generate summary
echo "" | tee -a "$LOG_FILE"
echo "============================================" | tee -a "$LOG_FILE"
echo "Test Suite Summary" | tee -a "$LOG_FILE"
echo "============================================" | tee -a "$LOG_FILE"
echo "Total: $((PASSED + FAILED))" | tee -a "$LOG_FILE"
echo "Passed: $PASSED" | tee -a "$LOG_FILE"
echo "Failed: $FAILED" | tee -a "$LOG_FILE"

# Write summary file for easy parsing
{
    echo "TOTAL=$((PASSED + FAILED))"
    echo "PASSED=$PASSED"
    echo "FAILED=$FAILED"
    echo "FAILED_TESTS=${FAILED_TESTS[*]}"
    echo "STOPPED_EARLY=$STOPPED_EARLY"
} > "$SUMMARY_FILE"

if [ $FAILED -eq 0 ]; then
    echo "" | tee -a "$LOG_FILE"
    echo "[OK] All $PASSED tests PASSED!" | tee -a "$LOG_FILE"
    exit 0
else
    echo "" | tee -a "$LOG_FILE"
    echo "[ERROR] $FAILED tests FAILED:" | tee -a "$LOG_FILE"
    for test in "${FAILED_TESTS[@]}"; do
        echo "  - $test" | tee -a "$LOG_FILE"
    done
    for test in "${FAILED_TESTS[@]}"; do
        print_failure_excerpt "$test"
    done
    echo "" | tee -a "$LOG_FILE"
    echo "See $LOG_FILE for full output" | tee -a "$LOG_FILE"
    if [ "$RETRYABLE_FAILURE" -eq 1 ] || [ "$STOPPED_EARLY" -eq 1 ]; then
        exit 75
    fi
    exit 1
fi
