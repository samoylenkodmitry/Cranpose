#!/usr/bin/env bash
# Drags a tab out of the strip in the tabs demo and reports whether the
# window it becomes follows the pointer, the way a torn tool pane does.
#
#   check_tab_tear.sh <out-dir> [rounds]
#
# A tab that is alone in its window has nowhere to go, so each round adds a
# tab with the strip's + first and then carries the first tab out from under
# the strip. Exits 1 on a round where no window came up or the window stood
# still while the pointer moved on. macOS only; needs `cliclick`.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
drag="$here/drag_window.sh"
out="${1:?usage: check_tab_tear.sh <out-dir> [rounds]}"
rounds="${2:-3}"
mkdir -p "$out"
log="$out/tab_tear.log"
status=0

# Inside the tabs window: the strip runs along the top, the first tab is at
# its left, and the + button sits after the tabs that are there.
strip_y=18 first_tab_x=60 tab_width=140 new_tab_x=22
carry_x=200 carry_y=170

"$drag" launch chrome_tabs "$log" > /dev/null

for round in $(seq 1 "$rounds"); do
    home="$("$drag" windows "$log" | awk '$1 == 1' | tail -1)"
    read -r _ wx wy _ _ panes <<< "$home"
    tabs="${panes#panes=}"
    tabs="${tabs:-1}"
    "$drag" click "$((wx + tabs * tab_width + new_tab_x)),$((wy + strip_y))" > /dev/null
    sleep 1

    since="$("$drag" mark "$log")"
    from_x=$((wx + first_tab_x)) from_y=$((wy + strip_y))
    "$drag" drag "$from_x,$from_y" "$((from_x + carry_x)),$((from_y + carry_y))" 24 25 > /dev/null
    sleep 1

    torn_id="$("$drag" trace "$log" "$since" 'tab left the strip' | \
        grep -oE 'torn=Some\([0-9]+\)' | grep -oE '[0-9]+' | tail -1)"
    if [ -z "$torn_id" ]; then
        echo "round $round: FAIL the tab never left the strip"
        status=1
        continue
    fi
    moved="$("$drag" trace "$log" "$since" "window id=$torn_id origin" | \
        sed -E 's/.*origin=\(([0-9.]+),([0-9.]+)\).*/\1 \2/' | sort -u | wc -l | tr -d ' ')"
    if [ "$moved" -ge 3 ]; then
        echo "round $round: the tab became window $torn_id and took $moved positions with the pointer"
    else
        echo "round $round: FAIL window $torn_id stood still, $moved position(s)"
        status=1
    fi
done

pkill -f 'examples/chrome_tabs' 2>/dev/null || true
exit $status
