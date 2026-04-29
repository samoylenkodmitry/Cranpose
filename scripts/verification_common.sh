run_logged() {
    local label="$1"
    local log_file="$2"
    local work_dir="$3"
    shift 3

    mkdir -p "$(dirname "$log_file")"
    echo "==> $label"
    if command -v wait_for_host_capacity >/dev/null 2>&1; then
        if ! wait_for_host_capacity "$label"; then
            echo "failed: $label" >&2
            echo "    host was not ready" >&2
            exit 1
        fi
    fi
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

run_logged_timed_retry() {
    local label="$1"
    local log_file="$2"
    local work_dir="$3"
    local timeout_secs="$4"
    local attempts="$5"
    shift 5

    mkdir -p "$(dirname "$log_file")"
    echo "==> $label"
    : > "$log_file"

    if ! [[ "$timeout_secs" =~ ^[0-9]+$ ]]; then
        timeout_secs=0
    fi
    if ! [[ "$attempts" =~ ^[1-9][0-9]*$ ]]; then
        attempts=1
    fi

    local attempt=1
    while [ "$attempt" -le "$attempts" ]; do
        if command -v wait_for_host_capacity >/dev/null 2>&1; then
            if ! wait_for_host_capacity "$label attempt $attempt"; then
                echo "failed: $label" >&2
                echo "    host was not ready" >&2
                if command -v host_state_summary >/dev/null 2>&1; then
                    echo "    host: $(host_state_summary)" >&2
                fi
                exit 1
            fi
        fi

        {
            echo "Attempt $attempt/$attempts for $label"
            if command -v host_state_summary >/dev/null 2>&1; then
                echo "Host before attempt: $(host_state_summary)"
            fi
        } >> "$log_file"

        local exit_code
        if (
            cd "$work_dir"
            if command -v timeout >/dev/null 2>&1 && [ "$timeout_secs" -gt 0 ]; then
                timeout --kill-after=15s "${timeout_secs}s" "$@"
            else
                "$@"
            fi
        ) >> "$log_file" 2>&1; then
            echo "ok: $label"
            echo "    log: $log_file"
            return 0
        else
            exit_code=$?
        fi

        if { [ "$exit_code" = "75" ] || [ "$exit_code" = "124" ] || [ "$exit_code" = "137" ]; } && [ "$attempt" -lt "$attempts" ]; then
            {
                echo
                if [ "$exit_code" = "75" ]; then
                    echo "Stopped on a retryable host or per-test timeout condition on attempt $attempt/$attempts."
                else
                    echo "Timed out after ${timeout_secs}s on attempt $attempt/$attempts."
                fi
                if command -v host_state_summary >/dev/null 2>&1; then
                    echo "Host state: $(host_state_summary)"
                fi
                echo "Cooling before retry..."
            } >> "$log_file"
            echo "retryable stop: $label attempt $attempt/$attempts; retrying after host check" >&2
            attempt=$((attempt + 1))
            continue
        fi

        echo "failed: $label" >&2
        echo "    log: $log_file" >&2
        if [ "$exit_code" = "75" ]; then
            echo "    stopped on a retryable host or per-test timeout condition" >&2
            if command -v host_state_summary >/dev/null 2>&1; then
                echo "    host: $(host_state_summary)" >&2
            fi
        elif [ "$exit_code" = "124" ] || [ "$exit_code" = "137" ]; then
            echo "    timed out after ${timeout_secs}s" >&2
            if command -v host_state_summary >/dev/null 2>&1; then
                echo "    host: $(host_state_summary)" >&2
            fi
        fi
        tail -n 120 "$log_file" >&2 || true
        exit 1
    done
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
    local root_dir
    if [ "$#" -gt 0 ]; then
        root_dir="$1"
    else
        root_dir="${ROOT_DIR:?ROOT_DIR must be set}"
    fi

    local summary_file="$root_dir/robot_test_summary.txt"
    verify_robot_summary_file "$summary_file"
}

verify_robot_summary_file() {
    local summary_file="$1"
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
