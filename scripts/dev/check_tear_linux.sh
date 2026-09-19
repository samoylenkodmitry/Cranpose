#!/usr/bin/env bash
# Tears a tool pane out under a virtual X server, over and over, and reports
# whether the window that came up took the press that was still held and
# followed the pointer.
#
#   check_tear_linux.sh <out-dir> [rounds]
#
# The Linux counterpart of check_tear_repeats.sh. That one drives the real
# pointer with `cliclick` and reads the desktop through AppleScript, neither
# of which exists here: Xvfb gives the demo a screen, `xdotool` carries the
# pointer, and the demo's own traces say where its windows went. The two
# platforms take different paths through the handover -- X11 has a global
# pointer to poll and macOS has to relay the press from the window that
# holds it -- so a pass on one says nothing about the other.
#
# Needs the example built:
#   cargo build -p desktop-app --features desktop,renderer-wgpu --example tool_windows
# Exits 1 on the first round whose window did not follow the pointer.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
out="${1:?usage: check_tear_linux.sh <out-dir> [rounds]}"
rounds="${2:-6}"
mkdir -p "$out"
log="$out/tear_linux.log"
display="${CRANPOSE_TEST_DISPLAY:-:99}"
status=0

# Where a pane's title sits inside the window, with no decoration around it,
# and how far the pointer carries it.
grip_x=100 grip_y=122 carry_x=200 carry_y=150

for tool in Xvfb xdotool; do
    command -v "$tool" > /dev/null || { echo "check_tear_linux.sh: $tool is required" >&2; exit 2; }
done

Xvfb "$display" -screen 0 1600x1000x24 > "$out/xvfb.log" 2>&1 &
xvfb=$!
cleanup() {
    pkill -f 'examples/tool_windows' 2> /dev/null || true
    kill "$xvfb" 2> /dev/null || true
}
trap cleanup EXIT
sleep 2

(cd "$root" && DISPLAY="$display" CRANPOSE_DEMO_TRACE=1 CRANPOSE_NATIVE_TRACE=1 \
    ./target/debug/examples/tool_windows > "$log" 2>&1 &)
for _ in $(seq 1 40); do
    grep -q 'demo trace: windows=' "$log" 2> /dev/null && break
    sleep 0.5
done
sleep 2

stack="$(DISPLAY="$display" xdotool search --name 'Cranpose Tool Windows' | head -1)"
[ -n "$stack" ] || { echo "check_tear_linux.sh: the demo put no window on $display" >&2; exit 1; }
eval "$(DISPLAY="$display" xdotool getwindowgeometry --shell "$stack" | grep -E '^(X|Y)=')"
echo "the stack is at ${X:-?},${Y:-?} on $display"

carry() {
    local x="$1" y="$2" steps="$3"
    local i
    for i in $(seq 1 "$steps"); do
        DISPLAY="$display" xdotool mousemove $((x + carry_x * i / steps)) $((y + carry_y * i / steps))
        sleep 0.03
    done
}

for round in $(seq 1 "$rounds"); do
    from_x=$((X + grip_x)) from_y=$((Y + grip_y))
    mark="$(wc -l < "$log")"
    DISPLAY="$display" xdotool mousemove "$from_x" "$from_y" mousedown 1
    carry "$from_x" "$from_y" 18
    DISPLAY="$display" xdotool mouseup 1
    sleep 1

    since="$(tail -n +$((mark + 1)) "$log")"
    handed="$(printf '%s\n' "$since" | grep -c 'held press handed to' || true)"
    torn="$(printf '%s\n' "$since" | grep -oE 'torn tool=[0-9]+' | grep -oE '[0-9]+' | tail -1)"
    moved=0
    if [ -n "$torn" ]; then
        moved="$(printf '%s\n' "$since" | grep -E "window id=$torn origin" | \
            sed -E 's/.*origin=\(([0-9.-]+),([0-9.-]+)\).*/\1 \2/' | sort -u | wc -l)"
    fi
    if [ -n "$torn" ] && [ "$moved" -ge 3 ]; then
        echo "round $round: pane $torn took $moved positions with the pointer, press handed over $handed time(s)"
    else
        echo "round $round: FAIL pane ${torn:-none} took $moved position(s), press handed over $handed time(s)"
        status=1
    fi

    dock="$(printf '%s\n' "$since" | grep -E "window id=${torn:-x} origin" | tail -1)"
    if [ -n "$dock" ]; then
        read -r dx dy <<< "$(printf '%s\n' "$dock" | \
            sed -E 's/.*origin=\(([0-9.-]+)\.[0-9]*,([0-9.-]+)\.[0-9]*\).*/\1 \2/')"
        width="$(printf '%s\n' "$dock" | sed -E 's/.*size=\(([0-9]+)\.[0-9]*,.*/\1/')"
        DISPLAY="$display" xdotool mousemove $((dx + width - 16)) $((dy + 10)) click 1
        sleep 1
    fi
done

exit $status
