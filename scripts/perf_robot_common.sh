#!/usr/bin/env bash

readonly DEFAULT_PERF_SCENARIOS=(
    lazy_list_scroll
    text_heavy_scroll
    markdown_scroll
    markdown_viewer_scroll
    markdown_default_viewer_scroll
    backdrop_blur
    opaque_scene
)

is_valid_perf_scenario() {
    local scenario="$1"
    case "$scenario" in
        lazy_list_scroll|text_heavy_scroll|markdown_scroll|markdown_viewer_scroll|markdown_default_viewer_scroll|backdrop_blur|opaque_scene|glass_lazy_scroll)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

append_perf_scenario() {
    local scenario="$1"
    if ! is_valid_perf_scenario "$scenario"; then
        echo "Unknown perf scenario: $scenario" >&2
        exit 1
    fi
    PERF_SCENARIOS+=("$scenario")
}

finalize_perf_scenarios() {
    if [[ ${#PERF_SCENARIOS[@]} -eq 0 ]]; then
        PERF_SCENARIOS=("${DEFAULT_PERF_SCENARIOS[@]}")
    fi
}

extract_perf_summary_line() {
    local prefix="$1"
    local log_file="$2"

    grep "^${prefix} " "$log_file" | tail -n 1 || true
}

append_perf_summary_block() {
    local report_file="$1"
    local scenario="$2"
    local log_file="$3"

    {
        echo "=== ${scenario} ==="
        local render_line
        render_line=$(extract_perf_summary_line "PERF_RENDER_SUMMARY" "$log_file")
        local memory_line
        memory_line=$(extract_perf_summary_line "PERF_MEMORY_SUMMARY" "$log_file")
        local fps_line
        fps_line=$(extract_perf_summary_line "PERF_FPS_SUMMARY" "$log_file")
        local fps_budget_line
        fps_budget_line=$(extract_perf_summary_line "PERF_FPS_BUDGET" "$log_file")
        local p95_budget_line
        p95_budget_line=$(extract_perf_summary_line "PERF_FRAME_P95_BUDGET" "$log_file")
        local stall_budget_line
        stall_budget_line=$(extract_perf_summary_line "PERF_STALL_BUDGET" "$log_file")
        local complete_line
        complete_line=$(extract_perf_summary_line "PERF_SCENARIO_COMPLETE" "$log_file")

        if [[ -n "$render_line" ]]; then
            echo "$render_line"
        else
            echo "PERF_RENDER_SUMMARY missing"
        fi

        if [[ -n "$memory_line" ]]; then
            echo "$memory_line"
        else
            echo "PERF_MEMORY_SUMMARY missing"
        fi

        if [[ -n "$fps_line" ]]; then
            echo "$fps_line"
        else
            echo "PERF_FPS_SUMMARY missing"
        fi

        if [[ -n "$fps_budget_line" ]]; then
            echo "$fps_budget_line"
        fi

        if [[ -n "$p95_budget_line" ]]; then
            echo "$p95_budget_line"
        fi

        if [[ -n "$stall_budget_line" ]]; then
            echo "$stall_budget_line"
        fi

        if [[ -n "$complete_line" ]]; then
            echo "$complete_line"
        else
            echo "PERF_SCENARIO_COMPLETE missing"
        fi
        echo
    } >> "$report_file"
}
