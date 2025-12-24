#!/bin/bash

# Robot test runner with logging
# Runs all robot tests in the robot-runners directory, logs output, and summarizes results

LOG_FILE="robot_test.log"
SUMMARY_FILE="robot_test_summary.txt"
ROBOT_DIR="apps/desktop-demo/robot-runners"

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
echo "Log file: $LOG_FILE" | tee -a "$LOG_FILE"
echo "============================================" | tee -a "$LOG_FILE"

PASSED=0
FAILED=0
FAILED_TESTS=()

for example in "${EXAMPLES[@]}"; do
    echo "--------------------------------------------------" >> "$LOG_FILE"
    echo "Running $example..." | tee -a "$LOG_FILE"
    echo "--------------------------------------------------" >> "$LOG_FILE"

    # Run with timeout, capture exit code and output
    timeout_secs=60
    if [ "$example" = "robot_text_input" ]; then
        timeout_secs=120
    fi

    # Create temp file for this test's output
    TEST_OUTPUT=$(mktemp)

    if command -v timeout >/dev/null 2>&1; then
        timeout "${timeout_secs}s" cargo run --package desktop-app --example "$example" --features robot-app > "$TEST_OUTPUT" 2>&1
        EXIT_CODE=$?
    else
        cargo run --package desktop-app --example "$example" --features robot-app > "$TEST_OUTPUT" 2>&1
        EXIT_CODE=$?
    fi

    # Append output to main log
    cat "$TEST_OUTPUT" >> "$LOG_FILE"

    # Check for failure patterns in output (case insensitive)
    # Look for FAIL, FATAL, or panicked, but exclude "0 failed" style messages
    FAIL_IN_LOG=false
    if grep -qiE '\bFAIL\b|\bFATAL\b|panicked' "$TEST_OUTPUT"; then
        # Make sure it's not a false positive like "0 failed"
        if grep -iE '\bFAIL\b|\bFATAL\b|panicked' "$TEST_OUTPUT" | grep -qvE '^[[:space:]]*0 failed|test result:.*0 failed'; then
            FAIL_IN_LOG=true
        fi
    fi

    rm -f "$TEST_OUTPUT"

    if [ $EXIT_CODE -eq 0 ] && [ "$FAIL_IN_LOG" = false ]; then
        echo "  [PASS] $example" | tee -a "$LOG_FILE"
        ((PASSED++))
    else
        REASON=""
        [ $EXIT_CODE -ne 0 ] && REASON="exit=$EXIT_CODE"
        [ "$FAIL_IN_LOG" = true ] && REASON="${REASON:+$REASON, }fail_in_log"
        echo "  [FAIL] $example ($REASON)" | tee -a "$LOG_FILE"
        ((FAILED++))
        FAILED_TESTS+=("$example")
    fi

    sleep 0.5
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
