#!/usr/bin/env bash
# Drive desktop windows with the real pointer to check draggable and
# droppable windows: dock panes torn between OS windows, window groups that
# follow a dragged peer, borderless windows moved by a drag area.
#
# A demo built from this tree is launched with every trace on and a throwaway
# HOME, the pointer is pressed, moved in steps and released with `cliclick`,
# and the traces the demo printed for that gesture are read back. Every
# gesture is one command, so a check is repeatable and its evidence is a log,
# not a memory of what the screen looked like.
#
#   drag_window.sh launch <example> <log>            start ./target/debug/examples/<example>
#   drag_window.sh windows <log>                      dock windows at rest: id x y w h panes=n
#   drag_window.sh click <x,y>                        press and release
#   drag_window.sh drag <x0,y0> <x1,y1> [steps] [ms]  press, move in steps, release
#   drag_window.sh drag-pane <log> <id> <lx,ly> <dx,dy> [steps]
#                                                     drag from a dock window's local point by a delta,
#                                                     then print what the dock did
#   drag_window.sh mark <log>                         line count, for `trace`
#   drag_window.sh trace <log> <since> [regex]        trace lines after <since>, poll noise dropped
#   drag_window.sh shot <png> [x,y,w,h]               screenshot a region
#
# macOS only: needs `cliclick` (brew install cliclick) and `screencapture`.
set -euo pipefail

tree="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

need() {
    command -v "$1" >/dev/null 2>&1 || { echo "drag_window.sh: $1 is required" >&2; exit 2; }
}

launch() {
    local example="$1" log="$2" home
    home="$(dirname "$log")/home-$example"
    mkdir -p "$home"
    pkill -f "examples/$example" 2>/dev/null || true
    sleep 1
    : > "$log"
    (cd "$tree" && RUST_BACKTRACE=1 CRANPOSE_DOCK_TRACE=1 CRANPOSE_NATIVE_TRACE=1 HOME="$home" \
        nohup "./target/debug/examples/$example" > "$log" 2>&1 &)
    local _
    for _ in $(seq 1 60); do
        grep -q "sync create" "$log" && break
        sleep 0.25
    done
    echo "launched $example log=$log lines=$(wc -l < "$log" | tr -d ' ')"
}

windows() {
    local log="$1"
    set +o pipefail
    grep -E 'dock trace: window id=' "$log" \
        | sed -E 's/.*id=([0-9]+) origin=\(([0-9.-]+),([0-9.-]+)\) size=\(([0-9.]+),([0-9.]+)\) panes=([0-9]+) parked=(true|false).*/\1 \2 \3 \4 \5 \6 \7/' \
        | awk '{ last[$1] = $0 } END { for (id in last) print last[id] }' \
        | awk '$7 == "false" { printf "%s %d %d %d %d panes=%s\n", $1, $2, $3, $4, $5, $6 }' \
        | sort -n
    set -o pipefail
}

click() {
    need cliclick
    cliclick "c:${1%,*},${1#*,}" "w:400"
}

drag() {
    need cliclick
    local from="$1" to="$2" steps="${3:-10}" pause="${4:-80}"
    local x0="${from%,*}" y0="${from#*,}" x1="${to%,*}" y1="${to#*,}"
    cliclick "dd:$x0,$y0" "w:250"
    local i x y
    for i in $(seq 1 "$steps"); do
        x=$(( x0 + (x1 - x0) * i / steps ))
        y=$(( y0 + (y1 - y0) * i / steps ))
        cliclick "dm:$x,$y" "w:$pause"
    done
    cliclick "du:$x1,$y1" "w:400"
}

drag_pane() {
    local log="$1" id="$2" local_point="$3" delta="$4" steps="${5:-10}"
    local origin
    origin="$(windows "$log" | awk -v id="$id" '$1 == id { print $2 "," $3 }')"
    [ -n "$origin" ] || { echo "drag_window.sh: no dock window $id in $log" >&2; exit 1; }
    local ox="${origin%,*}" oy="${origin#*,}"
    local x0=$(( ox + ${local_point%,*} )) y0=$(( oy + ${local_point#*,} ))
    local x1=$(( x0 + ${delta%,*} )) y1=$(( y0 + ${delta#*,} ))
    local since
    since="$(mark "$log")"
    drag "$x0,$y0" "$x1,$y1" "$steps"
    sleep 1
    echo "gesture ($x0,$y0) -> ($x1,$y1)"
    trace "$log" "$since" 'dock trace: (press|release)|step=(Carry|Join)|sync create|panicked' \
        | awk '/step=Carry/ { carry = $0; if (seen_carry++) next } { print } END { if (seen_carry > 1) print carry }'
    grep -q panicked "$log" && { echo "PANIC in $log" >&2; exit 1; }
    return 0
}

mark() {
    wc -l < "$1" | tr -d ' '
}

trace() {
    local log="$1" since="$2" pattern="${3:-.}"
    awk -v s="$since" 'NR>s' "$log" \
        | grep -v 'poll skipped' \
        | grep -E "$pattern" \
        | sed -e 's/native window trace: /NW /' -e 's/dock trace: /DOCK /' \
              -e 's/WindowId(\([0-9]\{4\}\)[0-9]*)/W\1/g' || true
}

shot() {
    need screencapture
    screencapture -x -R"${2:-0,100,1500,1000}" "$1" && echo "$1"
}

command="${1:-}"
shift || true
case "$command" in
    launch) launch "$@" ;;
    windows) windows "$@" ;;
    click) click "$@" ;;
    drag) drag "$@" ;;
    drag-pane) drag_pane "$@" ;;
    mark) mark "$@" ;;
    trace) trace "$@" ;;
    shot) shot "$@" ;;
    *) sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
