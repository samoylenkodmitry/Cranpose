#!/usr/bin/env bash
# Checks that torn panes line up with each other and travel together.
#
#   check_window_snap.sh <out-dir>
#
# Snapping is the application's own code now, not the framework's, so this
# drives the demo the way a person would: two panes are carried out, the
# second is let go a few pixels short of the first's lower edge, and the
# positions the window server reports are compared against the exact stack
# they should have formed. Then the upper one is dragged and the lower one
# has to arrive with it.
#
# Needs the example built:
#   cargo build -p desktop-app --features desktop,renderer-wgpu --example tool_windows
# macOS only; needs `cliclick`.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
drag="$here/drag_window.sh"
out="${1:?usage: check_window_snap.sh <out-dir>}"
mkdir -p "$out"
status=0

command -v cliclick > /dev/null || { echo "check_window_snap.sh: cliclick is required" >&2; exit 2; }
cleanup() { pkill -f 'examples/tool_windows' 2> /dev/null || true; }
trap cleanup EXIT
cleanup
sleep 1

(cd "$root" && CRANPOSE_DEMO_TRACE=1 ./target/debug/examples/tool_windows > "$out/snap.log" 2>&1 &)
sleep 6

chrome=37 pane=116 grip=10 grab=96
read -r wx wy _ _ _t <<< "$("$drag" oswindows tool_windows | grep 'Cranpose Tool Windows')"
# The first press on a window that is not frontmost is spent focusing it.
tear_to() {
    local to="$1" title="$2" try
    for try in 1 2 3 4; do
        "$drag" drag "$((wx + grab)),$((wy + chrome + pane + grip))" "$to" 16 25 > /dev/null
        sleep 1
        "$drag" oswindows tool_windows | grep -q "$title" && return 0
    done
    return 1
}

tear_to "$((wx + 450)),$((wy + 60))" Equalizer || {
    echo "check_window_snap.sh: the equalizer would not come out" >&2
    exit 1
}
read -r ex ey _ eh _t <<< "$("$drag" oswindows tool_windows | grep Equalizer)"

# Let the playlist go a little short of, and a little right of, where it
# belongs: close enough to reach, far enough that only snapping lands it.
# The pointer holds the pane at the point it was grabbed, so the window's
# own corner is the release point less that grab.
tear_to "$((ex + 5 + grab)),$((ey + eh - 7 + grip))" Playlist || {
    echo "check_window_snap.sh: the playlist would not come out" >&2
    exit 1
}
sleep 1
read -r ex ey _ eh _t <<< "$("$drag" oswindows tool_windows | grep Equalizer)"
read -r px py _ ph _t <<< "$("$drag" oswindows tool_windows | grep Playlist)"
echo "equalizer ${ex},${ey} ${eh} tall; playlist ${px},${py} ${ph} tall"

# The contract is that the pane lines up on an edge, not which edge it picks:
# where it lands is where the person let it go.
touching() {
    local ax=$1 ay=$2 ah=$3 bx=$4 by=$5 bh=$6 width=275 reach=3
    local dx=$((ax - bx)) dy=$((ay - by))
    [ "${dx#-}" -le $reach ] && return 0
    [ $((ax + width)) -ge $((bx - reach)) ] && [ $((ax + width)) -le $((bx + reach)) ] && return 0
    [ $((bx + width)) -ge $((ax - reach)) ] && [ $((bx + width)) -le $((ax + reach)) ] && return 0
    [ $((ay + ah)) -ge $((by - reach)) ] && [ $((ay + ah)) -le $((by + reach)) ] && return 0
    [ $((by + bh)) -ge $((ay - reach)) ] && [ $((by + bh)) -le $((ay + reach)) ] && return 0
    [ "${dy#-}" -le $reach ] && return 0
    return 1
}

if touching "$px" "$py" "$ph" "$ex" "$ey" "$eh"; then
    echo "the playlist lined up on one of the equalizer's edges"
else
    echo "FAIL the playlist came to rest at ${px},${py} touching nothing"
    status=1
fi

rest_dx=$((px - ex)) rest_dy=$((py - ey))
"$drag" drag "$((ex + grab)),$((ey + grip))" "$((ex - 260)),$((ey + 190))" 24 30 > /dev/null
sleep 1
read -r nx ny _ _ _t <<< "$("$drag" oswindows tool_windows | grep Equalizer)"
read -r qx qy _ _ _t <<< "$("$drag" oswindows tool_windows | grep Playlist)"
echo "equalizer went to ${nx},${ny}; playlist followed to ${qx},${qy}"
if [ $((qx - nx)) -eq "$rest_dx" ] && [ $((qy - ny)) -eq "$rest_dy" ]; then
    echo "the playlist travelled with it, keeping ${rest_dx},${rest_dy}"
else
    echo "FAIL the playlist should have kept ${rest_dx},${rest_dy} and kept $((qx - nx)),$((qy - ny))"
    status=1
fi

exit $status
