#!/bin/bash

find_local_tool() {
    local tool_name="$1"
    if command -v "$tool_name" >/dev/null 2>&1; then
        command -v "$tool_name"
        return 0
    fi
    if [ -x "$HOME/.cargo/bin/$tool_name" ]; then
        printf '%s\n' "$HOME/.cargo/bin/$tool_name"
        return 0
    fi
    return 1
}

is_ci_env() {
    [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]
}

enable_local_sccache() {
    local sccache_bin

    if is_ci_env; then
        return 0
    fi
    if [ "${CRANPOSE_USE_SCCACHE:-1}" = "0" ]; then
        return 0
    fi
    if [ -n "${RUSTC_WRAPPER:-}" ]; then
        return 0
    fi
    if ! sccache_bin="$(find_local_tool sccache)"; then
        return 0
    fi

    export RUSTC_WRAPPER="$sccache_bin"
    "$sccache_bin" --start-server >/dev/null 2>&1 || true
}

local_temp_root() {
    if [ -n "${CRANPOSE_TMPDIR:-}" ]; then
        printf '%s\n' "$CRANPOSE_TMPDIR"
        return 0
    fi

    if [ -n "${XDG_CACHE_HOME:-}" ]; then
        printf '%s\n' "$XDG_CACHE_HOME/cranpose/tmp"
        return 0
    fi

    printf '%s\n' "$HOME/.cache/cranpose/tmp"
}

ensure_local_temp_root() {
    local temp_root

    temp_root="$(local_temp_root)"
    mkdir -p "$temp_root"
    printf '%s\n' "$temp_root"
}

enable_local_tmpdir() {
    local temp_root

    if is_ci_env; then
        return 0
    fi
    if [ "${CRANPOSE_USE_LOCAL_TMPDIR:-1}" = "0" ]; then
        return 0
    fi
    if [ -n "${TMPDIR:-}" ] && [ "$TMPDIR" != "/tmp" ]; then
        return 0
    fi

    temp_root="$(ensure_local_temp_root)"
    export TMPDIR="$temp_root"
}

create_local_temp_dir() {
    local prefix="${1:-cranpose}"
    local temp_root

    enable_local_tmpdir
    temp_root="${TMPDIR:-$(ensure_local_temp_root)}"
    mktemp -d "$temp_root/${prefix}.XXXXXX"
}

local_cargo_build_jobs_default() {
    local cpu_count
    local available_kib
    local jobs_by_cpu
    local jobs_by_memory
    local jobs

    cpu_count="$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)"
    jobs_by_cpu=$(( cpu_count / 4 ))
    if [ "$jobs_by_cpu" -lt 1 ]; then
        jobs_by_cpu=1
    fi

    available_kib="$(awk '/MemAvailable:/ { print $2; exit }' /proc/meminfo 2>/dev/null || true)"
    if [ -n "$available_kib" ]; then
        jobs_by_memory=$(( available_kib / 6291456 ))
        if [ "$jobs_by_memory" -lt 1 ]; then
            jobs_by_memory=1
        fi

        if [ "$jobs_by_memory" -lt "$jobs_by_cpu" ]; then
            jobs="$jobs_by_memory"
        else
            jobs="$jobs_by_cpu"
        fi
    else
        jobs="$jobs_by_cpu"
    fi

    if [ "$jobs" -gt 8 ]; then
        jobs=8
    fi

    printf '%s\n' "$jobs"
}

enable_local_cargo_job_limit() {
    if is_ci_env; then
        return 0
    fi
    if [ "${CRANPOSE_LIMIT_BUILD_JOBS:-1}" = "0" ]; then
        return 0
    fi
    if [ -n "${CARGO_BUILD_JOBS:-}" ]; then
        return 0
    fi

    export CARGO_BUILD_JOBS="${CRANPOSE_BUILD_JOBS:-$(local_cargo_build_jobs_default)}"
}
