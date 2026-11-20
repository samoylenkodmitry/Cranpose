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
#                                        budget plus its spread (and 2% of
#                                        the budget for bytes)
#   steady_ledger.sh --repeats N         measure N times; print the budget
#                                        lines: each value's highest and spread
#   steady_ledger.sh --ab BASE HEAD      build HEAD, print the difference a
#                                        row from BASE, with the callgrind
#                                        instructions of five windows; the
#                                        host keeps each BASE built and
#                                        measured in ~/hotpath/base/SHA for
#                                        every later A/B
#   steady_ledger.sh --base BASE         only build and measure BASE there
#   --host H                             with --ab or --base: build and run
#                                        on host H over ssh (valgrind runs
#                                        on Linux)
#   --size SCRIPT                        with --ab, also run SCRIPT BASE HEAD
#                                        here (a code size A/B) and print it
#   --dir DIR                            builds and runs (default:
#                                        target/steady-ledger; with --host, a
#                                        path on H, default steady-ledger)
#   steady_ledger.sh --compare ROWS BUDGET GROUPS
#                                        compare a rows file with a budget
#
# Every fat LTO build takes ~/hotpath/fatlto.lock, one at a time a host.
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
readonly MOVABLE_LAYOUTS=(
    "movable-lazy --case=lazy_movable_scroll"
    "movable-heavy --case=lazy_movable_heavy"
    "movable-context-menu --case=lazy_context_menu_scroll"
    "movable-flip --case=movable_parent_flip"
)
# The layout whose start gets a row of its own, a layout node.
readonly START_ROW=wall-t20
# The layouts whose counted frames an A/B also runs under callgrind, beside
# the start of START_ROW and the dashboard.
readonly INSTRUCTION_ROWS=" gauntlet-t12 patchwork-t17 feed movable-heavy "
# The layouts whose scene changes in the counted window: nodes come and go,
# texts change and caches fill. The blocks that their window leaves live
# follow the scene's phase (from -101 to +2 a frame on the gauntlet t16 at
# one commit), so the comparison prints that count and leaves it out of the
# verdict. Every other row must leave nothing live.
readonly PHASED_ROWS=" patchwork-t17 gauntlet-t16 gauntlet-t12 feed ticker "

# What the A/Bs of all lanes on a host share: the lock that lets one fat LTO
# build run at a time, and the measured bases. STEADY_LEDGER_SHARED moves
# them, on the host of --host too, under the same path below the home
# directory: a second directory gives a host a second build slot, with bases
# of its own. FORMAT names the rows that this script writes; a new one
# measures a kept base again.
readonly SHARED="${STEADY_LEDGER_SHARED:-$HOME/hotpath}"
readonly FORMAT=2

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
    local mc="$dir/bin/$side-movable_content"
    local window=(--warmup="$WARMUP_FRAMES" --frames="$COUNTED_FRAMES")
    # Whole stacks, which place each copy or block in its phase, with full
    # source paths, which place a block in its group.
    local dhat=(valgrind --tool=dhat --num-callers=500 --read-inline-info=yes --fullpath-after=
        --dhat-out-file="$out.$kind.json")
    local callgrind=(valgrind --tool=callgrind --callgrind-out-file="$out.$kind.out")
    case $kind in
    count) "$steady" "${window[@]}" "$@" > "$out.count" 2>&1 ;;
    copy) "${dhat[@]}" --mode=copy --trace-malloc=yes "$steady" "${window[@]}" "$@" 2>&1 | ledger trace > "$out.$kind" ;;
    # The same run with no counted frames, for the copies whose stack
    # valgrind cut before it reached the counted frames' function.
    empty) "${dhat[@]}" --mode=copy "$steady" --warmup="$WARMUP_FRAMES" --frames=0 "$@" > "$out.$kind" 2>&1 ;;
    heap) "${dhat[@]}" "$steady" "${window[@]}" "$@" > "$out.$kind" 2>&1 ;;
    # Instructions from the window's function on, of which the process's
    # exit after it adds a few.
    instr) "${callgrind[@]}" --zero-before='steady_frames::steady_frames*' "$steady" "${window[@]}" "$@" \
        > "$out.$kind" 2>&1 ;;
    instr-start) "${callgrind[@]}" --zero-before='steady_frames::start*' "$steady" --warmup=0 --frames=0 "$@" \
        > "$out.$kind" 2>&1 ;;
    mc-count) "$mc" --steady-ledger "${window[@]}" "$@" > "$out.count" 2>&1 ;;
    mc-copy)
        valgrind --tool=dhat --num-callers=500 --read-inline-info=yes --fullpath-after= \
            --dhat-out-file="$out.copy.json" --mode=copy --trace-malloc=yes \
            "$mc" --steady-ledger "${window[@]}" "$@" 2>&1 | ledger trace > "$out.copy"
        ;;
    mc-empty)
        valgrind --tool=dhat --num-callers=500 --read-inline-info=yes --fullpath-after= \
            --dhat-out-file="$out.empty.json" --mode=copy \
            "$mc" --steady-ledger --warmup="$WARMUP_FRAMES" --frames=0 "$@" > "$out.empty" 2>&1
        ;;
    mc-instr)
        valgrind --tool=callgrind --callgrind-out-file="$out.instr.out" \
            --zero-before='movable_content::steady_frames*' \
            "$mc" --steady-ledger "${window[@]}" "$@" > "$out.instr" 2>&1
        ;;
    fb-instr)
        SCREEN=dashboard PIXELS=rgb565 FRAMES="$COUNTED_FRAMES" "${callgrind[@]}" \
            --zero-before='framebuffer_frame::steady_frames*' "$fb" > "$out.$kind" 2>&1
        ;;
    fb-*)
        SCREEN=dashboard PIXELS=rgb565 FRAMES="${kind#fb-}" "${dhat[@]}" --mode=copy --trace-malloc=yes "$fb" 2>&1 |
            ledger trace > "$out.$kind"
        ;;
    *) die "unknown job $kind" ;;
    esac
}

# The job lines of one run of SIDE's binaries into runs/RUN; with a third
# argument, `instructions`, the callgrind runs of an A/B too.
jobs_of() {
    local run=$1 side=$2 instructions=${3:-} layout row kind
    mkdir -p "$dir/runs/$run"
    : > "$dir/runs/$run/order"
    for layout in "${LAYOUTS[@]}"; do
        row=${layout%% *}
        echo "$row" >> "$dir/runs/$run/order"
        # The longest jobs first, so that they do not end the run alone.
        if [ "$row" = "$START_ROW" ]; then
            echo "$run $side heap $layout"
            [ -z "$instructions" ] || echo "$run $side instr-start $layout"
        fi
        if [ -n "$instructions" ] && [[ $INSTRUCTION_ROWS == *" $row "* ]]; then
            echo "$run $side instr $layout"
        fi
        for kind in copy empty count; do
            echo "$run $side $kind $layout"
        done
    done
    echo dashboard >> "$dir/runs/$run/order"
    [ -z "$instructions" ] || echo "$run $side fb-instr dashboard"
    echo "$run $side fb-0 dashboard"
    echo "$run $side fb-$COUNTED_FRAMES dashboard"
    if [ -x "$dir/bin/$side-movable_content" ]; then
        for layout in "${MOVABLE_LAYOUTS[@]}"; do
            row=${layout%% *}
            echo "$row" >> "$dir/runs/$run/order"
            if [ -n "$instructions" ] && [[ $INSTRUCTION_ROWS == *" $row "* ]]; then
                echo "$run $side mc-instr $layout"
            fi
            for kind in mc-copy mc-empty mc-count; do
                echo "$run $side $kind $layout"
            done
        done
    fi
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

# Runs a command that makes a fat LTO build, one at a time on the host, under
# SHARED/fatlto.lock. flock closes the lock in the command, so that no daemon
# that the build starts keeps it. A host without flock(1) (macOS) builds
# unlocked.
fatlto() {
    if [ -n "${STEADY_LEDGER_FATLTO:-}" ] || ! command -v flock > /dev/null; then
        "$@"
    else
        mkdir -p "$SHARED"
        STEADY_LEDGER_FATLTO=held flock -o "$SHARED/fatlto.lock" "$@"
    fi
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
        fatlto "$lock" --shared cargo build --release \
            --manifest-path benchmarks/compose-vs-cranpose/cranpose-app/Cargo.toml \
            --example steady_frames --message-format=json-render-diagnostics
    ) > "$dir/build-$side-app.json"
    (
        cd "$src"
        [ -z "$ws_target" ] || export CARGO_TARGET_DIR="$ws_target"
        fatlto "$lock" --shared cargo bench -p cranpose --no-default-features \
            --features framebuffer,embedded-default-font \
            --bench framebuffer_frame --no-run --message-format=json-render-diagnostics
    ) > "$dir/build-$side-fb.json"
    local names=(steady_frames framebuffer_frame)
    local build_jsons=("$dir/build-$side-app.json" "$dir/build-$side-fb.json")
    if [ -f "$src/crates/cranpose-app-shell/benches/movable_content.rs" ]; then
        (
            cd "$src"
            [ -z "$ws_target" ] || export CARGO_TARGET_DIR="$ws_target"
            fatlto "$lock" --shared cargo bench -p cranpose-app-shell \
                --bench movable_content --no-run --message-format=json-render-diagnostics
        ) > "$dir/build-$side-mc.json"
        names+=(movable_content)
        build_jsons+=("$dir/build-$side-mc.json")
    fi
    local name exe
    for name in "${names[@]}"; do
        exe=$(grep -h "\"name\":\"$name\"" "${build_jsons[@]}" |
            grep -o '"executable":"[^"]*"' | tail -1 | cut -d'"' -f4)
        [ -n "$exe" ] || die "the build of $name named no executable"
        cp "$exe" "$dir/bin/$side-$name"
    done
}

# Puts the tree of TAR at AREA/src, keeping the times of files whose content
# did not change, so that each build there is incremental; src and lock then
# point there.
unpack() {
    local tar=$1 area=$2
    [ -f "$tar" ] || die "no tree to build at $tar"
    if [ -d "$area/unpack" ]; then
        rm -r -- "$area/unpack"
    fi
    mkdir -p "$area/unpack" "$area/src"
    tar -x -m -C "$area/unpack" -f "$tar"
    rsync -rlpc --delete --exclude /target/ "$area/unpack/" "$area/src/"
    [ -f "$area/src/benchmarks/compose-vs-cranpose/cranpose-app/examples/steady_frames.rs" ] ||
        die "$tar holds no steady_frames example: both sides of an A/B need it"
    use_tree "$area/src"
}

use_tree() {
    src=$1
    lock="$src/scripts/ci/with_host_lock.sh"
}

# The base's directory on this host: its binaries, runs and rows.
base_dir() {
    echo "$SHARED/base/$1"
}

# Builds and measures the base SHA from TAR, unless a lane did so before;
# leaves dir at the base's directory. All bases build in one tree, with the
# fat LTO lock held from the unpack to the last build (--build-base).
ensure_base() {
    local tar=$1 sha=$2
    dir=$(base_dir "$sha")
    mkdir -p "$dir"
    if [ ! -f "$dir/bin/built" ]; then
        fatlto "$self" --dir "$dir" --build-base "$tar"
    fi
    use_tree "$SHARED/base/tree/src"
    if [ ! -f "$dir/rows.$FORMAT.txt" ]; then
        jobs_of base base instructions | run_jobs
        rows_of base > "$dir/rows.$FORMAT.txt.part"
        mv "$dir/rows.$FORMAT.txt.part" "$dir/rows.$FORMAT.txt"
    fi
}

# An A/B on this host: the base from BASE_TAR (or as kept), HEAD_TAR built
# in dir, the difference a row.
ab_tars() {
    local base_tar=$1 head_tar=$2 base_sha=$3 lane=$dir base
    ensure_base "$base_tar" "$base_sha"
    base=$dir
    dir=$lane
    unpack "$head_tar" "$dir"
    build head "$dir/target-app" "$dir/target-ws"
    jobs_of head head instructions | run_jobs
    rows_of head > "$dir/rows-head.txt"
    echo "steady ledger A/B: a steady frame ($COUNTED_FRAMES after $WARMUP_FRAMES); $START_ROW-start a layout node"
    ledger diff "$tools/steady_ledger_budget.txt" "$tools/steady_ledger_groups.txt" \
        "$base/rows.$FORMAT.txt" "$dir/rows-head.txt"
}

# The script, its arithmetic, its groups and its budget lie in one directory,
# which --host copies whole.
tools=$(dirname "$self")
readonly TOOL_FILES=(steady_ledger.sh steady_ledger.py steady_ledger_groups.txt steady_ledger_budget.txt)
root=""
src=""
lock=""
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
    --base) mode=base; args=("$2"); shift 2 ;;
    --ab-tars) mode="ab-tars"; args=("$2" "$3" "$4"); shift 4 ;;
    --base-tar) mode="base-tar"; args=("$2" "$3"); shift 3 ;;
    --build-base) mode="build-base"; args=("$2"); shift 2 ;;
    --compare) mode=compare; args=("$2" "$3" "$4"); shift 4 ;;
    --job) mode=job; shift; args=("$@"); break ;;
    *) die "unknown argument $1 (see the head of $self)" ;;
    esac
done

case $mode in
compare) ledger compare "${args[@]}" "$PHASED_ROWS"; exit ;;
job) job "${args[@]}"; exit ;;
build-base)
    # Under the fat LTO lock; a lane can have built this base while this one
    # waited for it.
    [ -f "$dir/bin/built" ] && exit 0
    unpack "${args[0]}" "$SHARED/base/tree"
    build base "$SHARED/base/tree/target-app" "$SHARED/base/tree/target-ws"
    touch "$dir/bin/built"
    exit
    ;;
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

# The top of the repository around the current directory, else around this
# script: `git archive` in a subdirectory archives only that subdirectory.
repository_root() {
    local top
    top=$(git rev-parse --show-toplevel 2> /dev/null || git -C "$tools" rev-parse --show-toplevel 2> /dev/null) ||
        die "no git repository around $(pwd) or $tools"
    cd "$top" && pwd -P
}

case $mode in
ab | base | check | repeats)
    root=$(repository_root)
    use_tree "$root"
    ;;
esac

case $mode in
ab | base)
    base_sha=$(git -C "$root" rev-parse "${args[0]}^{commit}")
    [ "$mode" = base ] || head_sha=$(git -C "$root" rev-parse "${args[1]}^{commit}")
    tmp=$(mktemp -d)
    if [ "$mode" = ab ] && [ -n "$size" ]; then
        # Its own process group, which cleanup stops with all its builds.
        set -m
        "$size" "$base_sha" "$head_sha" > "$tmp/size.log" 2>&1 &
        size_pid=$!
        set +m
    fi
    ;;
esac

# The trees that an A/B sends: the base only when the host has not measured
# it yet ("-" otherwise), at BASE_TAR and HEAD_TAR.
archive() {
    local kept=$1 into=$2
    BASE_TAR=-
    if ! $kept; then
        archive_commit "$base_sha" "$into/base.tar"
        BASE_TAR=$into/base.tar
    fi
    if [ "$mode" = ab ]; then
        archive_commit "$head_sha" "$into/head.tar"
        HEAD_TAR=$into/head.tar
    fi
}

# Archives the tree of commit SHA at OUT; a tree with no Cargo.toml at its
# top is no Cranpose tree.
archive_commit() {
    git -C "$root" archive "$1" > "$2"
    tar -tf "$2" Cargo.toml > /dev/null 2>&1 ||
        die "the archive of $1 from $root holds no Cargo.toml at its top"
}

if [ -n "$host" ]; then
    case $mode in ab | base) ;; *) die "--host runs an A/B or a base: give --ab BASE HEAD or --base BASE" ;; esac
    dir=${dir:-steady-ledger}
    ssh "$host" "mkdir -p '$dir'"
    kept=false
    # The shared directory on the host: the same path below its home
    # directory, which the host's shell names, or the same path outside one.
    case $SHARED in
    "$HOME"/*) shared_on_host="\$HOME/${SHARED#"$HOME"/}" ;;
    *) shared_on_host=$SHARED ;;
    esac
    if ssh "$host" "test -f \"$shared_on_host/base/$base_sha/rows.$FORMAT.txt\""; then
        kept=true
    fi
    archive "$kept" "$tmp"
    send=("${TOOL_FILES[@]/#/$tools/}")
    [ "$BASE_TAR" = - ] || send+=("$BASE_TAR")
    [ "$mode" = base ] || send+=("$HEAD_TAR")
    scp -q "${send[@]}" "$host:$dir/"
    remote_base=-
    [ "$BASE_TAR" = - ] || remote_base="$dir/base.tar"
    if [ "$mode" = ab ]; then
        remote=(--ab-tars "$remote_base" "$dir/head.tar" "$base_sha")
    else
        remote=(--base-tar "$remote_base" "$base_sha")
    fi
    # At a low priority, as the host can be a desktop in use.
    ssh "$host" "STEADY_LEDGER_SHARED=\"$shared_on_host\" nice -n 10 bash '$dir/steady_ledger.sh' --dir '$dir' ${remote[*]}"
else
    dir=${dir:-$root/target/steady-ledger}
    mkdir -p "$dir"
    dir=$(cd "$dir" && pwd -P)
    case $mode in
    ab | base)
        kept=false
        [ ! -f "$(base_dir "$base_sha")/rows.$FORMAT.txt" ] || kept=true
        archive "$kept" "$dir"
        if [ "$mode" = ab ]; then
            ab_tars "$BASE_TAR" "$HEAD_TAR" "$base_sha"
        else
            ensure_base "$BASE_TAR" "$base_sha"
            echo "steady ledger base $base_sha: $dir/rows.$FORMAT.txt"
        fi
        ;;
    ab-tars)
        ab_tars "${args[0]}" "${args[1]}" "${args[2]}"
        ;;
    base-tar)
        ensure_base "${args[0]}" "${args[1]}"
        echo "steady ledger base ${args[1]}: $dir/rows.$FORMAT.txt"
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
        ledger compare "$dir/rows.txt" "$tools/steady_ledger_budget.txt" "$tools/steady_ledger_groups.txt" \
            "$PHASED_ROWS"
        ;;
    esac
fi

if [ -n "$size_pid" ]; then
    wait "$size_pid" || true
    size_pid=""
    echo "code size ($size):"
    grep -E '^(wasm|android|armv7):' "$tmp/size.log" || cat "$tmp/size.log"
fi
