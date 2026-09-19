#!/usr/bin/env bash
# Tears the Equalizer pane out of the tool_windows demo with the real pointer
# and reads back what the desktop did: the held press handed to the window
# that appeared, the drag polled from that press, the window at rest under
# the pointer where it let go, and the pane docked again by a click on its
# "dock" label. The primary window is measured at each step: it wraps the
# stack of panes, so it is as wide as a pane and loses the torn pane's
# height. The one picture taken is the torn window's own region.
#
#   check_tool_tear.sh <out-dir>
#
# Needs the demo built:
#   cargo build -p desktop-app --features desktop,renderer-wgpu --example tool_windows
# Writes <out-dir>/tool_tear.log and <out-dir>/torn-window.png; exits 1 when
# a trace the tear must print is missing, the torn window is not under the
# release point, the dock click leaves it torn, or the primary window is
# not the width of a pane. macOS only, like the drag tool it drives.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
drag="$here/drag_window.sh"
out="${1:?usage: check_tool_tear.sh <out-dir>}"
mkdir -p "$out"
log="$out/tool_tear.log"
status=0

# The Equalizer title, measured from the primary window's frame origin, and
# how far the pointer carries it.
grip_x=100 grip_y=154 carry_x=200 carry_y=150
pane_width=275

fail() {
    echo "FAIL: $*"
    status=1
}

primary_frame() {
    local frame
    frame="$("$drag" oswindows tool_windows | grep ' Cranpose Tool Windows$')"
    read -r px py pw ph _ <<< "$frame"
    echo "primary window at $px,$py size ${pw}x$ph ($1)"
    if [ "$pw" -ne "$pane_width" ]; then
        fail "the primary window is not wrapping the panes ($1)"
    fi
}

expect_trace() {
    local since="$1" pattern="$2" what="$3"
    if "$drag" trace "$log" "$since" "$pattern" | grep -q .; then
        echo "ok: $what"
    else
        fail "$what: no trace matching '$pattern'"
    fi
}

"$drag" launch tool_windows "$log"
sleep 1
primary_frame "at launch"
x0=$(( px + grip_x )); y0=$(( py + grip_y ))
x1=$(( x0 + carry_x )); y1=$(( y0 + carry_y ))
since="$("$drag" mark "$log")"
"$drag" drag "$x0,$y0" "$x1,$y1" 12 40
sleep 1
echo "== tear traces"
"$drag" trace "$log" "$since" 'demo trace: torn|held press|drag (start|finish)|drag target|panicked' | awk '!seen[$0]++'
expect_trace "$since" 'demo trace: torn tool=2' 'the Equalizer tore'
expect_trace "$since" 'held press handed to' 'the held press went to the new window'
expect_trace "$since" 'drag start polling' 'the window drag polls from the press'
expect_trace "$since" 'drag finish' 'the drag finished on release'
primary_frame "after the tear"

torn="$("$drag" windows "$log" | awk '$1 == 2')"
if [ -z "$torn" ]; then
    fail "no torn window at rest"
else
    read -r _ tx ty tw th _ <<< "$torn"
    echo "torn window at $tx,$ty size ${tw}x$th; the pointer let go at $x1,$y1"
    if [ "$x1" -ge "$tx" ] && [ "$x1" -le $(( tx + tw )) ] && [ "$y1" -ge "$ty" ] && [ "$y1" -le $(( ty + th )) ]; then
        echo "ok: the torn window followed the pointer"
    else
        fail "the torn window is not under the release point"
    fi
    "$drag" shot "$out/torn-window.png" "$(( tx - 4 )),$(( ty - 4 )),$(( tw + 8 )),$(( th + 8 ))"
    since="$("$drag" mark "$log")"
    "$drag" click "$(( tx + tw - 16 )),$(( ty + 10 ))"
    sleep 1
    expect_trace "$since" 'demo trace: docked tool=2' 'the dock label put the pane back'
    primary_frame "after the dock"
fi
if grep -q panicked "$log"; then
    fail "the demo panicked"
fi
pkill -f examples/tool_windows || true
exit "$status"
