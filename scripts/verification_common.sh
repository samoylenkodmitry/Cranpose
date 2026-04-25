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
    local root_dir
    if [ "$#" -gt 0 ]; then
        root_dir="$1"
    else
        root_dir="${ROOT_DIR:?ROOT_DIR must be set}"
    fi

    local summary_file="$root_dir/robot_test_summary.txt"

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
