#!/usr/bin/env bash
# The steady-frame ledger (decision 5 of #2247): for every bench layout, the
# `malloc` and `realloc` calls, the blocks left live, the bytes that memcpy
# copies and the bytes that `realloc` keeps a steady frame; for the still
# wall's start the same a layout node, and its allocator calls by group
# (scripts/perf/steady_ledger_groups.txt). Compared with
# scripts/perf/steady_ledger_budget.txt.
#
#   steady_ledger.sh                     build this checkout, measure, compare;
#                                        exit 1 when a value rises past its
#                                        budget plus its spread
#   steady_ledger.sh --repeats N         measure N times; print the budget
#                                        lines: each value's highest and spread
#   steady_ledger.sh --ab BASE HEAD      build both commits from one source
#                                        path, print the difference a row
#   steady_ledger.sh --host H --ab BASE HEAD
#                                        the same, built and run on host H
#                                        over ssh (valgrind runs on Linux)
#   --size SCRIPT                        with --ab, also run SCRIPT BASE HEAD
#                                        here (a code size A/B) and print it
#   --dir DIR                            builds and runs (default:
#                                        target/steady-ledger; with --host, a
#                                        path on H, default steady-ledger)
#   steady_ledger.sh --compare ROWS BUDGET GROUPS
#                                        compare a rows file with a budget
#
# The `steady_frames` example of the bench app counts the calls with
# stats_alloc. valgrind's DHAT in copy mode counts the bytes that memcpy
# copies, in one run: a copy whose stack holds the example's `start` or
# `steady_frames` belongs to that phase. A deep layout's stack can pass the
# 500 frames that valgrind keeps; such copies count as the run's minus a run
# with no counted frames; the starts of two runs differ a little. The bytes
# that `realloc` copies come
# apart: the C library's `realloc`, which valgrind replaces and DHAT does not
# see, from the allocator calls that valgrind traces in the same run (each
# keeps the smaller of its old and new size); the standard library's
# `realloc` of an over-aligned block, an allocation and a memcpy, from DHAT's
# copies under it. DHAT in heap mode places the start's calls in their
# groups. The dashboard row runs the `framebuffer_frame` bench, the pixel
# path of the ESP32 boards, under DHAT alone: a run minus a run of no
# frames. Needs valgrind 3.15 or newer and python3.
set -euo pipefail

# The measured window of every row: the frames after the start that run
# first, and the frames counted. framebuffer_frame warms up 60 frames itself.
readonly WARMUP_FRAMES=60
readonly COUNTED_FRAMES=60

# The rows: a name, then the bench app's command line for that layout.
readonly LAYOUTS=(
    "wall-t20 --scenario=wall --tier=20"
    "patchwork-t17 --scenario=patchwork --tier=17"
    "gauntlet-t16 --scenario=gauntlet --tier=16"
    "gauntlet-t12 --scenario=gauntlet --tier=12"
    "feed --scenario=feed --speed=12000 --fcols=4 --ui=0.3"
    "ticker --scenario=ticker --quotes=72"
    "particles --scenario=particles --particles=20000"
    "layers --scenario=layers --tcols=12 --trows=22"
    "grid --scenario=grid --rows=30 --cols=12"
    "grid-layer --scenario=grid_layer --rows=30 --cols=12"
    "deep --scenario=deep --depth=40 --chips=6"
    "deep-layer --scenario=deep_layer --depth=40 --chips=6"
)
# The layout whose start gets a row of its own, a layout node.
readonly START_ROW=wall-t20

self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

die() {
    echo "steady_ledger: $*" >&2
    exit 2
}

# One measurement process: `--job RUN SIDE KIND ROW [ARGS...]`, which
# run_jobs hands to xargs. RUN names the run's directory, SIDE the binaries.
job() {
    local run=$1 side=$2 kind=$3 row=$4
    shift 4
    local out="$dir/runs/$run/$row"
    local steady="$dir/bin/$side-steady_frames" fb="$dir/bin/$side-framebuffer_frame"
    local window=(--warmup="$WARMUP_FRAMES" --frames="$COUNTED_FRAMES")
    # Whole stacks, which place each copy or block in its phase, with full
    # source paths, which place a block in its group.
    local dhat=(valgrind --tool=dhat --num-callers=500 --read-inline-info=yes --fullpath-after=
        --dhat-out-file="$out.$kind.json")
    case $kind in
    count) "$steady" "${window[@]}" "$@" > "$out.count" 2>&1 ;;
    copy) "${dhat[@]}" --mode=copy --trace-malloc=yes "$steady" "${window[@]}" "$@" 2>&1 | ledger trace > "$out.$kind" ;;
    # The same run with no counted frames, for the copies whose stack
    # valgrind cut before it reached the counted frames' function.
    empty) "${dhat[@]}" --mode=copy "$steady" --warmup="$WARMUP_FRAMES" --frames=0 "$@" > "$out.$kind" 2>&1 ;;
    heap) "${dhat[@]}" "$steady" "${window[@]}" "$@" > "$out.$kind" 2>&1 ;;
    fb-*)
        SCREEN=dashboard PIXELS=rgb565 FRAMES="${kind#fb-}" "${dhat[@]}" --mode=copy --trace-malloc=yes "$fb" 2>&1 |
            ledger trace > "$out.$kind"
        ;;
    *) die "unknown job $kind" ;;
    esac
}

# The job lines of one run of SIDE's binaries into runs/RUN.
jobs_of() {
    local run=$1 side=$2 layout row kind
    mkdir -p "$dir/runs/$run"
    : > "$dir/runs/$run/order"
    for layout in "${LAYOUTS[@]}"; do
        row=${layout%% *}
        echo "$row" >> "$dir/runs/$run/order"
        # The longest job first, so that it does not end the run alone.
        if [ "$row" = "$START_ROW" ]; then
            echo "$run $side heap $layout"
        fi
        for kind in copy empty count; do
            echo "$run $side $kind $layout"
        done
    done
    echo dashboard >> "$dir/runs/$run/order"
    echo "$run $side fb-0 dashboard"
    echo "$run $side fb-$COUNTED_FRAMES dashboard"
}

# Runs the job lines on stdin, as many at once as the host has cores.
run_jobs() {
    command -v valgrind > /dev/null || die "valgrind 3.15 or newer is needed (DHAT's copy mode)"
    "$lock" --shared xargs -P "$(nproc)" -L 1 "$self" --dir "$dir" --job
}

ledger() {
    python3 "$tools/steady_ledger.py" "$@"
}

rows_of() {
    ledger rows "$dir/runs/$1" "$COUNTED_FRAMES" "$START_ROW" "$tools/steady_ledger_groups.txt" "$src"
}

# Builds the bench app's steady_frames example with the app's release profile
# and the framebuffer_frame bench of the tree at src as bin/SIDE-*, with line
# tables, so that DHAT names the start's sites.
build() {
    local side=$1 app_target=${2:-} ws_target=${3:-}
    mkdir -p "$dir/bin"
    export CARGO_PROFILE_RELEASE_DEBUG=line-tables-only CARGO_PROFILE_RELEASE_STRIP=none
    export CARGO_PROFILE_BENCH_DEBUG=line-tables-only CARGO_PROFILE_BENCH_STRIP=none
    (
        cd "$src"
        [ -z "$app_target" ] || export CARGO_TARGET_DIR="$app_target"
        "$lock" --shared cargo build --release --manifest-path benchmarks/compose-vs-cranpose/cranpose-app/Cargo.toml \
            --example steady_frames --message-format=json-render-diagnostics
    ) > "$dir/build-$side-app.json"
    (
        cd "$src"
        [ -z "$ws_target" ] || export CARGO_TARGET_DIR="$ws_target"
        "$lock" --shared cargo bench -p cranpose --no-default-features --features framebuffer,embedded-default-font \
            --bench framebuffer_frame --no-run --message-format=json-render-diagnostics
    ) > "$dir/build-$side-fb.json"
    local name exe
    for name in steady_frames framebuffer_frame; do
        exe=$(grep -h "\"name\":\"$name\"" "$dir/build-$side-app.json" "$dir/build-$side-fb.json" |
            grep -o '"executable":"[^"]*"' | tail -1 | cut -d'"' -f4)
        [ -n "$exe" ] || die "the build of $name named no executable"
        cp "$exe" "$dir/bin/$side-$name"
    done
}

# Puts the tree of TAR at dir/src, keeping the times of files whose content
# did not change, so both sides of an A/B build from one path incrementally.
unpack() {
    if [ -d "$dir/unpack" ]; then
        rm -r -- "$dir/unpack"
    fi
    mkdir -p "$dir/unpack" "$dir/src"
    tar -x -m -C "$dir/unpack" -f "$1"
    rsync -rlpc --delete --exclude /target/ "$dir/unpack/" "$dir/src/"
}

ab_tars() {
    local side
    src="$dir/src"
    lock="$src/scripts/ci/with_host_lock.sh"
    for side in base head; do
        if [ "$side" = base ]; then unpack "$1"; else unpack "$2"; fi
        [ -f "$src/benchmarks/compose-vs-cranpose/cranpose-app/examples/steady_frames.rs" ] ||
            die "the $side commit has no steady_frames example: both sides of an A/B need it"
        build "$side" "$dir/target-app" "$dir/target-ws"
    done
    { jobs_of base base; jobs_of head head; } | run_jobs
    rows_of base > "$dir/rows-base.txt"
    rows_of head > "$dir/rows-head.txt"
    echo "steady ledger A/B: a steady frame ($COUNTED_FRAMES after $WARMUP_FRAMES); $START_ROW-start a layout node"
    ledger diff "$tools/steady_ledger_budget.txt" "$tools/steady_ledger_groups.txt" "$dir/rows-base.txt" "$dir/rows-head.txt"
}

# The script, its arithmetic, its groups and its budget lie in one directory,
# which --host copies whole.
tools=$(dirname "$self")
readonly TOOL_FILES=(steady_ledger.sh steady_ledger.py steady_ledger_groups.txt steady_ledger_budget.txt)
root="$(cd "$tools/../.." && pwd -P)"
src=$root
lock="$root/scripts/ci/with_host_lock.sh"
dir=""
host=""
size=""
repeats=0
mode=check
args=()
while [ $# -gt 0 ]; do
    case $1 in
    --dir) dir=$2; shift 2 ;;
    --host) host=$2; shift 2 ;;
    --size) size=$2; shift 2 ;;
    --repeats) mode=repeats; repeats=$2; shift 2 ;;
    --ab) mode=ab; args=("$2" "$3"); shift 3 ;;
    --ab-tars) mode=ab-tars; args=("$2" "$3"); shift 3 ;;
    --compare) mode=compare; args=("$2" "$3" "$4"); shift 4 ;;
    --job) mode=job; shift; args=("$@"); break ;;
    *) die "unknown argument $1 (see the head of $self)" ;;
    esac
done

case $mode in
compare) ledger compare "${args[@]}"; exit ;;
job) job "${args[@]}"; exit ;;
esac

tmp=""
size_pid=""
# Removes the archives of --host and stops the code size A/B when the ledger
# ends before it.
cleanup() {
    if [ -n "$size_pid" ] && kill -0 "$size_pid" 2> /dev/null; then
        disown "$size_pid"
        kill -- "-$size_pid" 2> /dev/null || true
    fi
    if [ -n "$tmp" ]; then
        rm -r -- "$tmp"
    fi
}
trap cleanup EXIT

if [ "$mode" = ab ]; then
    base_sha=$(git -C "$root" rev-parse "${args[0]}^{commit}")
    head_sha=$(git -C "$root" rev-parse "${args[1]}^{commit}")
    tmp=$(mktemp -d)
    if [ -n "$size" ]; then
        # Its own process group, which cleanup stops with all its builds.
        set -m
        "$size" "$base_sha" "$head_sha" > "$tmp/size.log" 2>&1 &
        size_pid=$!
        set +m
    fi
fi

if [ -n "$host" ]; then
    [ "$mode" = ab ] || die "--host runs an A/B: give --ab BASE HEAD"
    dir=${dir:-steady-ledger}
    git -C "$root" archive "$base_sha" > "$tmp/base.tar"
    git -C "$root" archive "$head_sha" > "$tmp/head.tar"
    ssh "$host" "mkdir -p '$dir'"
    scp -q "$tmp/base.tar" "$tmp/head.tar" "${TOOL_FILES[@]/#/$tools/}" "$host:$dir/"
    ssh "$host" "bash '$dir/steady_ledger.sh' --dir '$dir' --ab-tars '$dir/base.tar' '$dir/head.tar'"
else
    dir=${dir:-$root/target/steady-ledger}
    mkdir -p "$dir"
    dir=$(cd "$dir" && pwd -P)
    case $mode in
    ab)
        git -C "$root" archive "$base_sha" > "$dir/base.tar"
        git -C "$root" archive "$head_sha" > "$dir/head.tar"
        ab_tars "$dir/base.tar" "$dir/head.tar"
        ;;
    ab-tars)
        ab_tars "${args[0]}" "${args[1]}"
        ;;
    repeats)
        build main
        files=()
        for index in $(seq 1 "$repeats"); do
            jobs_of "main.$index" main | run_jobs
            rows_of "main.$index" > "$dir/rows-$index.txt"
            files+=("$dir/rows-$index.txt")
        done
        echo "# row             metric                    budget     spread   ($repeats runs)"
        ledger spread "${files[@]}"
        ;;
    check)
        build main
        jobs_of main.1 main | run_jobs
        rows_of main.1 > "$dir/rows.txt"
        echo "steady ledger: a steady frame ($COUNTED_FRAMES after $WARMUP_FRAMES); $START_ROW-start a layout node"
        ledger compare "$dir/rows.txt" "$tools/steady_ledger_budget.txt" "$tools/steady_ledger_groups.txt"
        ;;
    esac
fi

if [ -n "$size_pid" ]; then
    wait "$size_pid" || true
    size_pid=""
    echo "code size ($size):"
    grep -E '^(wasm|android|armv7):' "$tmp/size.log" || cat "$tmp/size.log"
fi
