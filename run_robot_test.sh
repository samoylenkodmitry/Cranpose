#!/bin/bash

# Robot test runner with logging
# Runs all robot tests, logs output, and summarizes results

LOG_FILE="robot_test.log"
SUMMARY_FILE="robot_test_summary.txt"

# Clean previous logs
rm -f "$LOG_FILE" "$SUMMARY_FILE"

echo "Cleaning up..."
echo "Building desktop-app examples..."
cargo build --package desktop-app --features robot-app --examples 2>&1 | tee -a "$LOG_FILE"

if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo "Build failed!" | tee -a "$LOG_FILE"
    exit 1
fi

# All robot test examples
EXAMPLES=(
    "robot_async_pause"
    "robot_async_tab_bug"
    "robot_click_drag"
    "robot_copy_paste"
    "robot_double_click"
    "robot_drag_selection"
    "robot_increment_bug"
    "robot_multiline_nav"
    "robot_offset_test"
    "robot_reactive_state"
    "robot_tab_navigation"
    "robot_tab_scroll"
    "robot_tab_selection"
    "robot_tabs_scroll"
    "robot_text_input"
    "robot_ui_breakage"
    "robot_demo"
    "robot_interactive"
)

echo "============================================" | tee -a "$LOG_FILE"
echo "Running Robot Test Suite" | tee -a "$LOG_FILE"
echo "Log file: $LOG_FILE" | tee -a "$LOG_FILE"
echo "============================================" | tee -a "$LOG_FILE"

PASSED=0
FAILED=0
FAILED_TESTS=()

for example in "${EXAMPLES[@]}"; do
    echo "--------------------------------------------------" >> "$LOG_FILE"
    echo "Running $example..." | tee -a "$LOG_FILE"
    echo "--------------------------------------------------" >> "$LOG_FILE"
    
    # Run with timeout, capture exit code
    if command -v timeout >/dev/null 2>&1; then
        timeout 60s cargo run --package desktop-app --example "$example" --features robot-app >> "$LOG_FILE" 2>&1
        EXIT_CODE=$?
    else
        cargo run --package desktop-app --example "$example" --features robot-app >> "$LOG_FILE" 2>&1
        EXIT_CODE=$?
    fi
    
    if [ $EXIT_CODE -eq 0 ]; then
        echo "  [PASS] $example" | tee -a "$LOG_FILE"
        ((PASSED++))
    else
        echo "  [FAIL] $example (exit: $EXIT_CODE)" | tee -a "$LOG_FILE"
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
