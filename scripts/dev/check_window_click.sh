#!/usr/bin/env bash
# Checks that a borderless window's drag area still answers a click.
#
#   check_window_click.sh <out-dir>
#
# The pet is one big drag area: pressing it moves the window, and a press
# that lets go without travelling is a click that changes its mood. The two
# must not be confused, so this clicks it in place and reads the mood the
# demo traced, then drags it and reads the mood again: a drag is not a click,
# and the window has to have moved.
#
# Needs the example built:
#   cargo build -p desktop-app --features desktop,renderer-wgpu --example pet
# macOS only; needs `cliclick`.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
drag="$here/drag_window.sh"
out="${1:?usage: check_window_click.sh <out-dir>}"
mkdir -p "$out"
log="$out/pet.log"
status=0

command -v cliclick > /dev/null || { echo "check_window_click.sh: cliclick is required" >&2; exit 2; }
cleanup() { pkill -f 'examples/pet' 2> /dev/null || true; }
trap cleanup EXIT

"$drag" launch pet "$log" > /dev/null
sleep 2

mood() { grep -E 'demo trace: pet mood=' "$log" | tail -1 | sed -E 's/.*mood=([A-Za-z]+).*/\1/'; }
where() { "$drag" oswindows pet | grep -i pet | head -1; }

read -r x y w h _ <<< "$(where)"
[ -n "${x:-}" ] || { echo "check_window_click.sh: the pet never appeared" >&2; exit 1; }
centre_x=$(( x + w / 2 )) centre_y=$(( y + h / 2 ))
echo "pet at ${x},${y} ${w}x${h}"

# The first press on a window that is not frontmost is spent focusing it.
"$drag" click "$centre_x,$centre_y" > /dev/null
sleep 1
before="$(mood)"
"$drag" click "$centre_x,$centre_y" > /dev/null
sleep 1
after="$(mood)"
echo "mood before the click ${before}, after ${after}"
if [ -n "$before" ] && [ "$before" != "$after" ]; then
    echo "a press that let go where it began was a click"
else
    echo "FAIL a click on the drag area did not reach the window (${before} -> ${after})"
    status=1
fi

read -r x y _ _ _ <<< "$(where)"
"$drag" drag "$((x + w / 2)),$((y + 40))" "$((x + w / 2 + 120)),$((y + 40 + 90))" 12 40 > /dev/null
sleep 1
read -r nx ny _ _ _ <<< "$(where)"
dragged="$(mood)"
echo "pet dragged ${x},${y} -> ${nx},${ny}; mood ${after} -> ${dragged}"
if [ "$nx" != "$x" ] || [ "$ny" != "$y" ]; then
    echo "the drag area still moved the window"
else
    echo "FAIL the window did not move on a drag"
    status=1
fi
if [ "$after" = "$dragged" ]; then
    echo "and a drag was not counted as a click"
else
    echo "FAIL dragging the window counted as a click (${after} -> ${dragged})"
    status=1
fi

exit $status
