#!/usr/bin/env bash
# Open the desktop demo on one tab and list the windows it puts on screen.
#
#   scripts/dev/show_demo_tab.sh <tab> [seconds-to-settle]
#
# The tab is the demo's own startup argument, for example `windows` or
# `winamp`. The demo runs with its traces on and its own HOME, so a run
# leaves nothing in the real one. Pair it with `drag_window.sh shotwindow`
# to picture a single window rather than the whole screen.
set -u

tab="${1:-windows}"
settle="${2:-6}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
binary="$root/target/debug/desktop-app"
run_dir="${CRANPOSE_DEMO_RUN_DIR:-${TMPDIR:-/tmp}/cranpose-demo-$tab}"
log="$run_dir/demo.log"

if [ ! -x "$binary" ]; then
    echo "no demo binary at $binary; cargo build -p desktop-app --features desktop,renderer-wgpu --bin desktop-app" >&2
    exit 1
fi

pkill -f 'target/debug/desktop-app' 2>/dev/null
sleep 1
mkdir -p "$run_dir/home"
(cd "$root" && CRANPOSE_DEMO_TRACE=1 CRANPOSE_NATIVE_TRACE=1 HOME="$run_dir/home" \
    nohup "$binary" "$tab" > "$log" 2>&1 &) > /dev/null 2>&1 < /dev/null
sleep "$settle"
echo "log: $log"
"$root/scripts/dev/drag_window.sh" oswindows desktop-app
