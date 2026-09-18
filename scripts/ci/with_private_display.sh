#!/usr/bin/env bash
set -euo pipefail

# Runs a command on an X server of its own.
#
#   with_private_display.sh SCREEN command [args...]
#
# SCREEN is an Xvfb screen specification, for example 1280x800x24.
#
# A display has one pointer, one focus and one root window, so two robot
# examples that draw into real windows cannot share one: a drag in one lands
# in the other, and a screenshot catches the neighbour's window. Giving each
# its own server is what lets them run at the same time.
#
# Deliberately not `xvfb-run -a`. That picks a display number by probing lock
# files and retrying, which is a race: six starting at once choose the same
# number, and the loser's server takes the winner's down with it. Measured on
# samarch-1 as four of six parallel examples dying with "the X11 server closed
# the connection", one of them a headless example that only wanted the
# clipboard. `Xvfb -displayfd` instead has the server itself bind a free
# number and report it, with no window in which a second server can choose the
# same one.

if [[ "$#" -lt 2 ]]; then
    echo "usage: $0 SCREEN command [args...]" >&2
    exit 64
fi

screen="$1"
shift

work_dir="$(mktemp -d)"
server_pid=""

cleanup() {
    if [[ -n "$server_pid" ]]; then
        kill "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
    fi
    rm -r -- "$work_dir" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

number_file="$work_dir/display"
: > "$number_file"

Xvfb -screen 0 "$screen" -nolisten tcp -displayfd 3 3>"$number_file" &
server_pid=$!

display_number=""
for _ in $(seq 1 600); do
    display_number="$(tr -d '[:space:]' < "$number_file")"
    [[ -n "$display_number" ]] && break
    if ! kill -0 "$server_pid" 2>/dev/null; then
        echo "with_private_display: Xvfb exited before reporting a display" >&2
        exit 1
    fi
    sleep 0.05
done

if [[ -z "$display_number" ]]; then
    echo "with_private_display: Xvfb did not report a display within 30s" >&2
    exit 1
fi

DISPLAY=":$display_number" "$@"
