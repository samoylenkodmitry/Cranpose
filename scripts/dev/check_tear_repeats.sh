#!/usr/bin/env bash
# Tears a tool pane out and docks it again, over and over, and reports for
# each round whether the window that came up took the press that was still
# held and followed the pointer.
#
#   check_tear_repeats.sh <out-dir> [rounds] [pane-id] [grip-y]
#
# A tear hands the held press to the window it just made, which then follows
# the pointer until the button goes up. That handover stops happening after
# a few rounds, and the torn window stands still where it appeared; one
# round on its own never shows it. Exits 1 on the first round that tears a
# window the press did not follow into.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
drag="$here/drag_window.sh"
out="${1:?usage: check_tear_repeats.sh <out-dir> [rounds] [pane-id] [grip-y]}"
rounds="${2:-8}"
pane="${3:-2}"
grip_y="${4:-154}"
mkdir -p "$out"
log="$out/tear_repeats.log"
status=0

grip_x=100 carry_x=200 carry_y=150

"$drag" launch tool_windows "$log" > /dev/null

for round in $(seq 1 "$rounds"); do
    frame="$("$drag" oswindows tool_windows | grep ' Cranpose Tool Windows$')"
    read -r px py _ _ _ <<< "$frame"
    from_x=$((px + grip_x)) from_y=$((py + grip_y))
    since="$("$drag" mark "$log")"
    "$drag" drag "$from_x,$from_y" "$((from_x + carry_x)),$((from_y + carry_y))" 18 14 > /dev/null
    sleep 1

    handed="$("$drag" trace "$log" "$since" 'held press handed to' 2>/dev/null | wc -l | tr -d ' ')"
    moved="$("$drag" trace "$log" "$since" "window id=$pane origin" 2>/dev/null | \
        sed -E 's/.*origin=\(([0-9.]+),([0-9.]+)\).*/\1 \2/' | sort -u | wc -l | tr -d ' ')"
    if [ "$handed" -ge 1 ] && [ "$moved" -ge 3 ]; then
        echo "round $round: the press went with the window, which took $moved positions"
    else
        echo "round $round: FAIL the window took $moved position(s) and the press was handed over $handed time(s)"
        status=1
    fi

    torn="$("$drag" windows "$log" | awk -v pane="$pane" '$1 == pane')"
    if [ -z "$torn" ]; then
        echo "round $round: no torn window to dock; stopping"
        status=1
        break
    fi
    read -r _ tx ty tw _ <<< "$torn"
    "$drag" click "$((tx + tw - 16)),$((ty + 10))" > /dev/null
    sleep 1
done

pkill -f 'examples/tool_windows' 2>/dev/null || true
exit $status
