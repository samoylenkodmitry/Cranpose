#!/bin/bash

# Robot test runner with parallel execution support
# Runs all robot tests in headless mode for parallel execution
# Usage: ./run_robot_test.sh [--parallel N] [--sequential]
# 
# Options:
#   --parallel N    Run N tests in parallel (default: number of CPU cores)
#   --sequential    Run tests one at a time (legacy mode)
#   --help          Show this help message

LOG_FILE="robot_test.log"
SUMMARY_FILE="robot_test_summary.txt"
ROBOT_DIR="apps/desktop-demo/robot-runners"

# Default to parallel execution with number of CPU cores / 2 (GPU-bound work)
PARALLEL_JOBS=$(( $(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4) / 2 ))
[ "$PARALLEL_JOBS" -lt 1 ] && PARALLEL_JOBS=1

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
        --help)
            echo "Usage: $0 [--parallel N] [--sequential]"
            echo ""
            echo "Options:"
            echo "  --parallel N    Run N tests in parallel (default: $(nproc 2>/dev/null || echo 4)/2)"
            echo "  --sequential    Run tests one at a time (legacy mode)"
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
echo "Building desktop-app examples..."
cargo build --package desktop-app --features robot-app --examples 2>&1 | tee -a "$LOG_FILE"

if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo "Build failed!" | tee -a "$LOG_FILE"
    exit 1
fi

# Dynamically discover all robot tests from the robot-runners directory
# Exclude utility modules (files that don't have a main function)
EXAMPLES=()
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
RESULTS_DIR=$(mktemp -d)
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
    
    # Run with timeout, capture exit code and output
    local timeout_secs=60
    if [ "$example" = "robot_text_input" ]; then
        timeout_secs=120
    fi

    if command -v timeout >/dev/null 2>&1; then
        timeout "${timeout_secs}s" cargo run --package desktop-app --example "$example" --features robot-app > "$output_file" 2>&1
        local exit_code=$?
    else
        cargo run --package desktop-app --example "$example" --features robot-app > "$output_file" 2>&1
        local exit_code=$?
    fi

    # Check for failure patterns in output
    local fail_in_log=false
    if grep -qiE '\bFAIL\b|\bFATAL\b|panicked' "$output_file"; then
        if grep -iE '\bFAIL\b|\bFATAL\b|panicked' "$output_file" | grep -qvE '^[[:space:]]*0 failed|test result:.*0 failed'; then
            fail_in_log=true
        fi
    fi

    # Write result
    if [ $exit_code -eq 0 ] && [ "$fail_in_log" = false ]; then
        echo "PASS" > "$result_file"
    else
        local reason=""
        [ $exit_code -ne 0 ] && reason="exit=$exit_code"
        [ "$fail_in_log" = true ] && reason="${reason:+$reason, }fail_in_log"
        echo "FAIL:$reason" > "$result_file"
    fi
}

export -f run_test
export RESULTS_DIR

# Run tests in parallel using xargs or GNU parallel
if [ "$PARALLEL_JOBS" -gt 1 ]; then
    # Use xargs for parallel execution
    printf '%s\n' "${EXAMPLES[@]}" | xargs -P "$PARALLEL_JOBS" -I {} bash -c 'run_test "$@"' _ {}
else
    # Sequential execution with progress
    for example in "${EXAMPLES[@]}"; do
        echo "Running $example..." | tee -a "$LOG_FILE"
        run_test "$example"
        sleep 0.1
    done
fi

# Wait for all tests to complete and gather results
PASSED=0
FAILED=0
FAILED_TESTS=()

for example in "${EXAMPLES[@]}"; do
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
    echo "" | tee -a "$LOG_FILE"
    echo "See $LOG_FILE for full output" | tee -a "$LOG_FILE"
    exit 1
fi
