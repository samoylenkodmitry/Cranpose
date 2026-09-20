#!/usr/bin/env bash
# Checks that a tab alone in its own window can be dropped back onto another
# window's strip.
#
#   check_tab_dock_back.sh <out-dir> [rounds]
#
# A tab alone in its window has nothing to tear, so dragging it moves the
# window it already has. It must still be something another strip can take:
# each round adds a tab, carries the first one out into a window of its own,
# and then drags that lone tab back onto the first window's strip. Exits 1 on
# a round where the tab did not come out, or came out and would not go back.
# macOS only; needs `cliclick`.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
drag="$here/drag_window.sh"
out="${1:?usage: check_tab_dock_back.sh <out-dir> [rounds]}"
rounds="${2:-2}"
mkdir -p "$out"
log="$out/tab_dock_back.log"
status=0

strip_y=18 first_tab_x=60 tab_width=140 new_tab_x=22
carry_x=260 carry_y=200

"$drag" launch chrome_tabs "$log" > /dev/null
trap 'pkill -f "examples/chrome_tabs" 2> /dev/null || true' EXIT

for round in $(seq 1 "$rounds"); do
    read -r _ wx wy _ _ panes <<< "$("$drag" windows "$log" | awk '$1 == 1' | tail -1)"
    tabs="${panes#panes=}"
    tabs="${tabs:-1}"
    "$drag" click "$((wx + tabs * tab_width + new_tab_x)),$((wy + strip_y))" > /dev/null
    sleep 1

    since="$("$drag" mark "$log")"
    from_x=$((wx + first_tab_x)) from_y=$((wy + strip_y))
    "$drag" drag "$from_x,$from_y" "$((from_x + carry_x)),$((from_y + carry_y))" 24 25 > /dev/null
    sleep 1
    torn_id="$("$drag" trace "$log" "$since" 'tab left the strip' \
        | grep -oE 'torn=Some\([0-9]+\)' | grep -oE '[0-9]+' | tail -1)"
    if [ -z "$torn_id" ]; then
        echo "round $round: FAIL the tab never left the strip"
        status=1
        continue
    fi

    read -r _ tx ty _ _ _ <<< "$("$drag" windows "$log" | awk -v id="$torn_id" '$1 == id' | tail -1)"
    read -r _ wx wy _ _ panes <<< "$("$drag" windows "$log" | awk '$1 == 1' | tail -1)"
    tabs="${panes#panes=}"
    tabs="${tabs:-1}"
    since="$("$drag" mark "$log")"
    # Onto the strip past the tabs that are already there, where the + and the
    # free stretch of chrome sit: still the strip, and no tab to activate.
    "$drag" drag "$((tx + first_tab_x)),$((ty + strip_y))" \
        "$((wx + tabs * tab_width + new_tab_x + 60)),$((wy + strip_y))" 24 25 > /dev/null
    sleep 1

    if "$drag" trace "$log" "$since" 'transfer dropped' | grep -q "onto=1 moved=true"; then
        echo "round $round: window $torn_id's lone tab went back onto window 1's strip"
    else
        echo "round $round: FAIL the lone tab would not dock back"
        "$drag" trace "$log" "$since" 'transfer|windows=' | tail -5
        status=1
    fi
done

exit $status
