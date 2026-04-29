run_robot_build_gate() {
    local root_dir="$1"
    local label="$2"
    local log_file="$3"
    local timeout_secs="$4"
    local attempts="$5"

    run_logged_timed_retry \
        "$label" \
        "$log_file" \
        "$root_dir" \
        "$timeout_secs" \
        "$attempts" \
        "$root_dir/run_robot_test.sh" --build-only
}

run_robot_shard_gate() {
    local root_dir="$1"
    local label_prefix="$2"
    local log_prefix="$3"
    local shard_spec="$4"
    local timeout_secs="$5"
    local attempts="$6"
    local shard_slug="${shard_spec//\//_of_}"
    local command_log="$root_dir/${log_prefix}_robot_shard_${shard_slug}.tmp"
    local robot_log="$root_dir/${log_prefix}_robot_test_shard_${shard_slug}.log"
    local summary_file="$root_dir/${log_prefix}_robot_summary_shard_${shard_slug}.txt"

    run_logged_timed_retry \
        "$label_prefix robot e2e shard $shard_spec" \
        "$command_log" \
        "$root_dir" \
        "$timeout_secs" \
        "$attempts" \
        env \
        CRANPOSE_ROBOT_LOG_FILE="$robot_log" \
        CRANPOSE_ROBOT_SUMMARY_FILE="$summary_file" \
        "$root_dir/run_robot_test.sh" --skip-build --sequential --shard "$shard_spec"
    verify_robot_summary_file "$summary_file"
    scan_log "$label_prefix robot e2e shard $shard_spec" "$command_log" "$ROBOT_LOG_PATTERN"
    scan_log "$label_prefix robot e2e shard $shard_spec full log" "$robot_log" "$ROBOT_LOG_PATTERN"
}
