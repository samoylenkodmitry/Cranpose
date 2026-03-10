#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_RUNNER=("$SCRIPT_DIR/cargo-dev.sh")
# shellcheck disable=SC1091
. "$SCRIPT_DIR/scripts/dev_build_common.sh"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/scripts/perf_robot_common.sh"

if [ ! -x "${CARGO_RUNNER[0]}" ]; then
    CARGO_RUNNER=(cargo)
fi

enable_local_tmpdir
enable_local_sccache
enable_local_cargo_job_limit

PROFILE="native-release"
EXAMPLE="robot_perf_harness"
DURATION_SECS="10"
WARMUP_SECS="${CRANPOSE_PERF_WARMUP_SECS:-0}"
OUTPUT_PREFIX="heaptrack"
TOOL="${CRANPOSE_ALLOC_TOOL:-auto}"
TOP_CALLERS="5"
TIMEOUT_SLACK_SECS="${CRANPOSE_PERF_TIMEOUT_SLACK_SECS:-300}"
MEM_VALIDATE="${CRANPOSE_MEM_VALIDATE:-1}"
PRESENT_MODE="${CRANPOSE_PRESENT_MODE:-immediate}"
HEADLESS="${CRANPOSE_HEADLESS:-1}"
PERF_SCENARIOS=()

usage() {
    cat <<EOF
Usage: $0 [--dev|--release|--profile NAME] [--example NAME] [--duration SECS] [--warmup SECS] [--timeout-slack SECS] [--output-prefix NAME] [--tool auto|heaptrack|massif] [--top-callers N] [--scenario NAME]

Profiles heap allocations for robot perf scenarios and writes human-readable reports.
Scenarios: lazy_list_scroll, text_heavy_scroll, backdrop_blur, opaque_scene
EOF
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "$1 is not installed or not on PATH."
        exit 1
    fi
}

extract_massif_top_callers() {
    local massif_report="$1"
    local top_callers_report="$2"
    local top_callers="$3"

    awk -v limit="$top_callers" -v out="$top_callers_report" '
        function trim(value) {
            sub(/^[[:space:]]+/, "", value)
            sub(/[[:space:]]+$/, "", value)
            return value
        }

        function clean_detail(value) {
            value = trim(value)
            gsub(/^[|[:space:]]+/, "", value)
            sub(/^->/, "", value)
            return trim(value)
        }

        function is_symbolized(value) {
            return value ~ /0x[[:xdigit:]]+: / && value !~ /\?\?\?/ && value !~ /UnknownInlinedFun/
        }

        function flush_branch() {
            if (!in_peak || branch_header == "" || printed >= limit) {
                return
            }

            print trim(branch_header) >> out
            if (preferred_detail != "") {
                print "    " clean_detail(preferred_detail) >> out
            } else if (fallback_detail != "") {
                print "    " clean_detail(fallback_detail) >> out
            }
            print "" >> out

            printed++
            branch_header = ""
            preferred_detail = ""
            fallback_detail = ""
        }

        BEGIN {
            peak_snapshot = ""
            in_peak = 0
            printed = 0
            branch_header = ""
            preferred_detail = ""
            fallback_detail = ""
            print "Peak snapshot top callers" > out
            print "" >> out
        }

        /Detailed snapshots:/ {
            if (match($0, /([0-9]+) \(peak\)/, captures)) {
                peak_snapshot = captures[1]
            }
        }

        peak_snapshot != "" && $0 ~ "^[[:space:]]*" peak_snapshot "[[:space:]]" {
            in_peak = 1
            next
        }

        in_peak && /^--------------------------------------------------------------------------------$/ {
            flush_branch()
            exit
        }

        in_peak && /^->/ {
            flush_branch()
            branch_header = $0
            next
        }

        in_peak && branch_header != "" && is_symbolized($0) {
            if (fallback_detail == "") {
                fallback_detail = $0
            }
            if (preferred_detail == "" && $0 ~ /(cranpose_|wgpu_|glyphon::|alloc::)/) {
                preferred_detail = $0
            }
        }

        END {
            flush_branch()
        }
    ' "$massif_report"
}

run_target() {
    CRANPOSE_PERF_DURATION_SECS="$DURATION_SECS" \
    CRANPOSE_PERF_WARMUP_SECS="$WARMUP_SECS" \
    CRANPOSE_PERF_TIMEOUT_SLACK_SECS="$TIMEOUT_SLACK_SECS" \
    CRANPOSE_MEM_VALIDATE="$MEM_VALIDATE" \
    CRANPOSE_PRESENT_MODE="$PRESENT_MODE" \
    CRANPOSE_HEADLESS="$HEADLESS" \
    "$@"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dev)
            PROFILE="dev"
            shift
            ;;
        --release)
            PROFILE="release"
            shift
            ;;
        --profile)
            PROFILE="$2"
            shift 2
            ;;
        --example)
            EXAMPLE="$2"
            shift 2
            ;;
        --duration)
            DURATION_SECS="$2"
            shift 2
            ;;
        --warmup)
            WARMUP_SECS="$2"
            shift 2
            ;;
        --timeout-slack)
            TIMEOUT_SLACK_SECS="$2"
            shift 2
            ;;
        --output-prefix)
            OUTPUT_PREFIX="$2"
            shift 2
            ;;
        --tool)
            TOOL="$2"
            shift 2
            ;;
        --top-callers)
            TOP_CALLERS="$2"
            shift 2
            ;;
        --scenario)
            append_perf_scenario "$2"
            shift 2
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

case "$TOOL" in
    auto)
        if command -v heaptrack >/dev/null 2>&1 && command -v heaptrack_print >/dev/null 2>&1; then
            TOOL="heaptrack"
        else
            TOOL="massif"
        fi
        ;;
    heaptrack)
        require_command heaptrack
        require_command heaptrack_print
        ;;
    massif)
        require_command valgrind
        require_command ms_print
        ;;
    *)
        echo "Unknown tool: $TOOL"
        usage
        exit 1
        ;;
esac

if [[ "$TOOL" == "massif" ]]; then
    require_command valgrind
    require_command ms_print
fi

finalize_perf_scenarios

PROFILE_DIR="debug"
BUILD_ARGS=(--package desktop-app --example "$EXAMPLE" --features robot-app)

if [[ "$PROFILE" == "release" ]]; then
    PROFILE_DIR="release"
    BUILD_ARGS+=(--release)
elif [[ "$PROFILE" != "dev" ]]; then
    PROFILE_DIR="$PROFILE"
    BUILD_ARGS+=(--profile "$PROFILE")
fi

if [[ "$PROFILE" != "dev" ]]; then
    PROFILE_ENV=$(echo "$PROFILE" | tr '[:lower:]-' '[:upper:]_')
    export "CARGO_PROFILE_${PROFILE_ENV}_DEBUG=1"
    export "CARGO_PROFILE_${PROFILE_ENV}_STRIP=none"
fi

"${CARGO_RUNNER[@]}" build "${BUILD_ARGS[@]}"

BIN="target/${PROFILE_DIR}/examples/${EXAMPLE}"
if [[ ! -x "$BIN" ]]; then
    echo "Binary not found: $BIN"
    exit 1
fi

OUTPUT_DIR="$(dirname "$OUTPUT_PREFIX")"
mkdir -p "$OUTPUT_DIR"
SUMMARY_REPORT="${OUTPUT_PREFIX}_summary.txt"

{
    echo "Heap perf summary"
    echo "tool=$TOOL"
    echo "profile=$PROFILE"
    echo "example=$EXAMPLE"
    echo "duration_secs=$DURATION_SECS"
    echo "warmup_secs=$WARMUP_SECS"
    echo "scenarios=${PERF_SCENARIOS[*]}"
    echo
} > "$SUMMARY_REPORT"

for scenario in "${PERF_SCENARIOS[@]}"; do
    LOG_FILE="${OUTPUT_PREFIX}_${scenario}.log"
    echo "Running heap scenario: $scenario ($TOOL)"

    if [[ "$TOOL" == "heaptrack" ]]; then
        OUTPUT_FILE="${OUTPUT_PREFIX}_${scenario}.data"
        REPORT_FILE="${OUTPUT_PREFIX}_${scenario}_report.txt"
        CRANPOSE_PERF_SCENARIO="$scenario" \
            run_target heaptrack --output "$OUTPUT_FILE" "$BIN" 2>&1 | tee "$LOG_FILE"
        heaptrack_print "$OUTPUT_FILE" | tee "$REPORT_FILE"

        append_perf_summary_block "$SUMMARY_REPORT" "$scenario" "$LOG_FILE"
        {
            echo "tool=$TOOL"
            echo "data=$OUTPUT_FILE"
            echo "report=$REPORT_FILE"
            echo "app_log=$LOG_FILE"
            echo
        } >> "$SUMMARY_REPORT"
        continue
    fi

    MASSIF_OUT="${OUTPUT_PREFIX}_${scenario}.massif.out"
    MASSIF_REPORT="${OUTPUT_PREFIX}_${scenario}_massif_report.txt"
    XTREE_OUT="${OUTPUT_PREFIX}_${scenario}.xtmemory"
    TOP_CALLERS_REPORT="${OUTPUT_PREFIX}_${scenario}_top_callers.txt"

    CRANPOSE_PERF_SCENARIO="$scenario" \
        run_target \
            valgrind \
            --tool=massif \
            --massif-out-file="$MASSIF_OUT" \
            --time-unit=B \
            --stacks=no \
            --xtree-memory=full \
            --xtree-memory-file="$XTREE_OUT" \
            "$BIN" \
            2>&1 | tee "$LOG_FILE"

    ms_print "$MASSIF_OUT" | tee "$MASSIF_REPORT"
    extract_massif_top_callers "$MASSIF_REPORT" "$TOP_CALLERS_REPORT" "$TOP_CALLERS"

    append_perf_summary_block "$SUMMARY_REPORT" "$scenario" "$LOG_FILE"
    {
        echo "tool=$TOOL"
        echo "massif_data=$MASSIF_OUT"
        echo "massif_report=$MASSIF_REPORT"
        echo "xtree_data=$XTREE_OUT"
        echo "top_callers=$TOP_CALLERS_REPORT"
        echo "app_log=$LOG_FILE"
        echo
    } >> "$SUMMARY_REPORT"
done

echo "summary: $SUMMARY_REPORT"
