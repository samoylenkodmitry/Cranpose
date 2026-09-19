#!/usr/bin/env bash
# Carries a tab out of its strip under a virtual X server and reports whether
# the window it becomes follows the pointer.
#
#   check_tab_tear_linux.sh <out-dir> [rounds]
#
# The Linux counterpart of check_tab_tear.sh, on Xvfb and `xdotool` the way
# check_tear_linux.sh is. A tab alone in its window has nowhere to go, so
# each round adds one with the strip's + first.
#
# Needs the example built:
#   cargo build -p desktop-app --features desktop,renderer-wgpu --example chrome_tabs
# Exits 1 on a round whose window did not follow the pointer.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
out="${1:?usage: check_tab_tear_linux.sh <out-dir> [rounds]}"
rounds="${2:-3}"
mkdir -p "$out"
log="$out/tab_tear_linux.log"
display="${CRANPOSE_TEST_DISPLAY:-:98}"
status=0

strip_y=18 first_tab_x=60 tab_width=140 new_tab_x=22 carry_x=200 carry_y=170

for tool in Xvfb xdotool; do
    command -v "$tool" > /dev/null || { echo "check_tab_tear_linux.sh: $tool is required" >&2; exit 2; }
done

Xvfb "$display" -screen 0 1600x1000x24 > "$out/xvfb.log" 2>&1 &
xvfb=$!
cleanup() {
    pkill -f 'examples/chrome_tabs' 2> /dev/null || true
    kill "$xvfb" 2> /dev/null || true
}
trap cleanup EXIT
sleep 2

(cd "$root" && DISPLAY="$display" CRANPOSE_DEMO_TRACE=1 CRANPOSE_NATIVE_TRACE=1 \
    ./target/debug/examples/chrome_tabs > "$log" 2>&1 &)
for _ in $(seq 1 40); do
    grep -q 'demo trace: windows=' "$log" 2> /dev/null && break
    sleep 0.5
done
sleep 2

for round in $(seq 1 "$rounds"); do
    home="$(grep -E 'window id=1 origin' "$log" | tail -1)"
    [ -n "$home" ] || { echo "round $round: FAIL the first window never reported itself"; status=1; break; }
    read -r wx wy <<< "$(printf '%s\n' "$home" | \
        sed -E 's/.*origin=\(([0-9.-]+)\.[0-9]*,([0-9.-]+)\.[0-9]*\).*/\1 \2/')"
    tabs="$(printf '%s\n' "$home" | sed -E 's/.*panes=([0-9]+).*/\1/')"
    DISPLAY="$display" xdotool mousemove $((wx + tabs * tab_width + new_tab_x)) $((wy + strip_y)) click 1
    sleep 1

    mark="$(wc -l < "$log")"
    from_x=$((wx + first_tab_x)) from_y=$((wy + strip_y))
    DISPLAY="$display" xdotool mousemove "$from_x" "$from_y" mousedown 1
    for i in $(seq 1 18); do
        DISPLAY="$display" xdotool mousemove \
            $((from_x + carry_x * i / 18)) $((from_y + carry_y * i / 18))
        sleep 0.03
    done
    DISPLAY="$display" xdotool mouseup 1
    sleep 1

    since="$(tail -n +$((mark + 1)) "$log")"
    torn="$(printf '%s\n' "$since" | grep -oE 'torn=Some\([0-9]+\)' | grep -oE '[0-9]+' | tail -1)"
    moved=0
    if [ -n "$torn" ]; then
        moved="$(printf '%s\n' "$since" | grep -E "window id=$torn origin" | \
            sed -E 's/.*origin=\(([0-9.-]+),([0-9.-]+)\).*/\1 \2/' | sort -u | wc -l)"
    fi
    if [ -n "$torn" ] && [ "$moved" -ge 3 ]; then
        echo "round $round: the tab became window $torn and took $moved positions with the pointer"
    else
        echo "round $round: FAIL window ${torn:-none} took $moved position(s)"
        status=1
    fi
done

exit $status
