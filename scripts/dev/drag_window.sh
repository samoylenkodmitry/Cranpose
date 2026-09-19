#!/usr/bin/env bash
# Drive desktop windows with the real pointer to check draggable and
# droppable windows: panes torn between OS windows, window groups that
# follow a dragged peer, borderless windows moved by a drag area.
#
# A demo built from this tree is launched with every trace on and a throwaway
# HOME, the pointer is pressed, moved in steps and released with `cliclick`,
# and the traces the demo printed for that gesture are read back. Every
# gesture is one command, so a check is repeatable and its evidence is a log,
# not a memory of what the screen looked like.
#
#   drag_window.sh launch <example> <log>            start ./target/debug/examples/<example>
#   drag_window.sh windows <log>                      demo windows at rest: id x y w h panes=n
#   drag_window.sh oswindows <example>                the OS windows of the running example: x y w h title
#   drag_window.sh click <x,y>                        press and release
#   drag_window.sh key <text>                         type text into the focused window
#   drag_window.sh drag <x0,y0> <x1,y1> [steps] [ms]  press, move in steps, release
#   drag_window.sh drag-pane <log> <id> <lx,ly> <dx,dy> [steps]
#                                                     drag from a demo window's local point by a delta,
#                                                     then print what the demo did
#   drag_window.sh snap <log> <id> <onto-id> above|below [lx,ly]
#                                                     carry a demo window's pane clear of everything, then
#                                                     bring it in line with another window, edge to edge
#   drag_window.sh mark <log>                         line count, for `trace`
#   drag_window.sh trace <log> <since> [regex]        trace lines after <since>, poll noise dropped
#   drag_window.sh shot <png> [x,y,w,h]               screenshot a region
#   drag_window.sh record <mov> <seconds>             record the screen in the background; gesture
#                                                     meanwhile, then `frames` when the time is up
#   drag_window.sh frame <mov> <n> <png>              save the nth recorded frame as a picture
#   drag_window.sh frames <mov> [dip]                 frame rate, brightness range, and every frame
#                                                     darker than both neighbours by more than dip
#
# macOS only: needs `cliclick` (brew install cliclick) and `screencapture`;
# `record`/`frame`/`frames` need ffmpeg (brew install ffmpeg), because
# `screencapture -V` records a still from a background shell.
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
    (cd "$tree" && RUST_BACKTRACE=1 CRANPOSE_DEMO_TRACE=1 CRANPOSE_NATIVE_TRACE=1 CRANPOSE_NATIVE_WINDOW_TIMING=1 HOME="$home" \
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
    local open
    open="$(grep -E 'demo trace: windows=' "$log" | tail -1 | sed -E 's/.*windows=//')"
    grep -E 'demo trace: window id=' "$log" \
        | sed -E 's/.*id=([0-9]+) origin=\(([0-9.-]+),([0-9.-]+)\) size=\(([0-9.]+),([0-9.]+)\) panes=([0-9]+) parked=(true|false).*/\1 \2 \3 \4 \5 \6 \7/' \
        | awk '{ last[$1] = $0 } END { for (id in last) print last[id] }' \
        | awk -v open=",$open," '$7 == "false" && index(open, "," $1 ",") { printf "%s %d %d %d %d panes=%s\n", $1, $2, $3, $4, $5, $6 }' \
        | sort -n
    set -o pipefail
}

key() {
    need cliclick
    cliclick "t:$1" "w:400"
}

oswindows() {
    osascript -e "tell application \"System Events\" to tell (first process whose name is \"$1\") to get {position, size, title} of every window" 2>&1
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
    [ -n "$origin" ] || { echo "drag_window.sh: no demo window $id in $log" >&2; exit 1; }
    local ox="${origin%,*}" oy="${origin#*,}"
    local x0=$(( ox + ${local_point%,*} )) y0=$(( oy + ${local_point#*,} ))
    local x1=$(( x0 + ${delta%,*} )) y1=$(( y0 + ${delta#*,} ))
    local since
    since="$(mark "$log")"
    drag "$x0,$y0" "$x1,$y1" "$steps"
    sleep 1
    echo "gesture ($x0,$y0) -> ($x1,$y1)"
    trace "$log" "$since" 'demo trace: (press|release)|step=(Carry|Join)|sync create|presented=|panicked' \
        | awk '/step=Carry/ { carry = $0; if (seen_carry++) next } { print } END { if (seen_carry > 1) print carry }'
    grep -q panicked "$log" && { echo "PANIC in $log" >&2; exit 1; }
    return 0
}

snap() {
    need cliclick
    local log="$1" id="$2" onto="$3" side="$4" local_point="${5:-137,10}"
    local mine theirs
    mine="$(windows "$log" | awk -v id="$id" '$1 == id')"
    theirs="$(windows "$log" | awk -v id="$onto" '$1 == id')"
    [ -n "$mine" ] && [ -n "$theirs" ] || { echo "drag_window.sh: windows $id and $onto must both be at rest in $log" >&2; exit 1; }
    local ox oy ow oh tx ty tw th
    read -r _ ox oy ow oh _ <<< "$mine"
    read -r _ tx ty tw th _ <<< "$theirs"
    local gx=$(( ox + ${local_point%,*} )) gy=$(( oy + ${local_point#*,} ))
    local dest_y
    case "$side" in
        above) dest_y=$(( ty - oh - 3 )) ;;
        below) dest_y=$(( ty + th + 3 )) ;;
        *) echo "drag_window.sh: side is above or below" >&2; exit 2 ;;
    esac
    local ex=$(( tx + ${local_point%,*} )) ey=$(( dest_y + ${local_point#*,} ))
    local since
    since="$(mark "$log")"
    cliclick "dd:$gx,$gy" "w:250"
    local i x y clear_x=$(( gx + 90 )) clear_y=$(( gy + 90 ))
    for i in 1 2 3 4; do
        cliclick "dm:$(( gx + (clear_x - gx) * i / 4 )),$(( gy + (clear_y - gy) * i / 4 ))" "w:80"
    done
    for i in $(seq 1 12); do
        x=$(( clear_x + (ex - clear_x) * i / 12 ))
        y=$(( clear_y + (ey - clear_y) * i / 12 ))
        cliclick "dm:$x,$y" "w:80"
    done
    cliclick "du:$ex,$ey" "w:400"
    sleep 1
    echo "snap $id $side $onto: ($gx,$gy) -> ($ex,$ey)"
    trace "$log" "$since" 'demo trace: (press|release)|step=Join|sync create|presented=|panicked' | awk '!seen[$0]++'
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
        | sed -e 's/native window trace: /NW /' -e 's/demo trace: /DEMO /' \
              -e 's/WindowId(\([0-9]\{4\}\)[0-9]*)/W\1/g' || true
}

shot() {
    need screencapture
    screencapture -x -R"${2:-0,100,1500,1000}" "$1" && echo "$1"
}

screen_device() {
    ffmpeg -hide_banner -f avfoundation -list_devices true -i "" 2>&1 \
        | sed -n -E 's/.*\[([0-9]+)\] Capture screen 0$/\1/p' \
        | head -1 || true
}

record() {
    need ffmpeg
    local mov="$1" seconds="$2" device
    device="$(screen_device)"
    [ -n "$device" ] || { echo "drag_window.sh: ffmpeg sees no screen to capture" >&2; exit 2; }
    rm -f "$mov"
    (ffmpeg -nostats -loglevel error -y -f avfoundation -framerate 60 -capture_cursor 1 \
        -i "$device:none" -t "$seconds" -pix_fmt yuv420p "$mov" &)
    sleep 1.5
    echo "recording $mov for ${seconds}s"
}

frame() {
    need ffmpeg
    local mov="$1" index="$2" png="$3"
    ffmpeg -nostats -loglevel error -y -i "$mov" -vf "select=eq(n\,$index)" -vframes 1 "$png" && echo "$png"
}

frames() {
    need ffmpeg
    local mov="$1" dip="${2:-2}"
    while pgrep -x ffmpeg >/dev/null; do sleep 0.5; done
    ffprobe -v error -select_streams v -show_entries stream=r_frame_rate,nb_frames -of csv=p=0 "$mov"
    ffmpeg -nostats -loglevel error -i "$mov" \
        -vf "tblend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-" -f null - \
        | awk -F= '/YAVG/ { if ($2 > m) m = $2 } END { printf "motion between frames: max %.2f (0 means a still)\n", m }'
    ffmpeg -nostats -loglevel error -i "$mov" \
        -vf "signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-" -f null - \
        | awk -F= -v dip="$dip" '/YAVG/ { y[n++] = $2 }
            END {
                lo = y[0]; hi = y[0];
                for (i = 1; i < n; i++) { if (y[i] < lo) lo = y[i]; if (y[i] > hi) hi = y[i] }
                printf "frames=%d brightness min=%.1f max=%.1f\n", n, lo, hi;
                for (i = 1; i < n - 1; i++)
                    if (y[i] < y[i-1] - dip && y[i] < y[i+1] - dip)
                        printf "DIP frame=%d %.1f between %.1f and %.1f\n", i, y[i], y[i-1], y[i+1]
            }'
}

command="${1:-}"
shift || true
case "$command" in
    launch) launch "$@" ;;
    windows) windows "$@" ;;
    click) click "$@" ;;
    oswindows) oswindows "$@" ;;
    key) key "$@" ;;
    drag) drag "$@" ;;
    drag-pane) drag_pane "$@" ;;
    snap) snap "$@" ;;
    mark) mark "$@" ;;
    trace) trace "$@" ;;
    shot) shot "$@" ;;
    record) record "$@" ;;
    frame) frame "$@" ;;
    frames) frames "$@" ;;
    *) sed -n '2,24p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
