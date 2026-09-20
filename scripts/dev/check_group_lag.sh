#!/usr/bin/env bash
# Reports how far an attached window falls behind the one dragging it.
#
#   check_group_lag.sh <out-dir>
#
# Two torn panes are snapped together and one is dragged. While a window
# moves, the picture on screen is what the window server has, not what the
# application believes, so the demo's own trace cannot show this: it prints
# both positions from one composition and they always agree. What is read
# back here is each window's outer position as the window server holds it,
# sampled during the drag.
#
# A group that moves together keeps the same offset between the two windows
# for the whole gesture. Drift in that offset is the follower lagging, in
# pixels. Exits 1 when the drift exceeds what a snap distance would hide.
#
# Needs the example built:
#   cargo build -p desktop-app --features desktop,renderer-wgpu --example tool_windows
# macOS only; needs `cliclick`.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
drag="$here/drag_window.sh"
out="${1:?usage: check_group_lag.sh <out-dir>}"
mkdir -p "$out"
log="$out/group_lag.log"
samples="$out/samples.txt"
allowed_drift=6

command -v cliclick > /dev/null || { echo "check_group_lag.sh: cliclick is required" >&2; exit 2; }

cleanup() { pkill -f 'examples/tool_windows' 2> /dev/null || true; }
trap cleanup EXIT
cleanup
sleep 1

(cd "$root" && CRANPOSE_DEMO_TRACE=1 CRANPOSE_NATIVE_TRACE=1 \
    ./target/debug/examples/tool_windows > "$log" 2>&1 &)
sleep 6

read -r wx wy _ _ _ <<< "$("$drag" oswindows tool_windows | grep 'Cranpose Tool Windows')"
# The window wears a native title bar, so the first pane starts below it.
chrome=37 pane=116 grip=10 grab=96

# The first press on a window that is not frontmost is spent focusing it, so
# a tear that has not happened is tried again rather than reported as a
# broken demo.
tear_to() {
    local to="$1" title="$2" try
    for try in 1 2 3; do
        "$drag" drag "$((wx + grab)),$((wy + chrome + pane + grip))" "$to" 16 25 > /dev/null
        sleep 1
        "$drag" oswindows tool_windows | grep -q "$title" && return 0
    done
    return 1
}

# Carry the second and third panes out, dropping the third against the
# second's lower edge so the two attach.
tear_to "$((wx + 450)),$((wy + 60))" Equalizer || {
    echo "check_group_lag.sh: the equalizer would not come out of the stack" >&2
    exit 1
}
read -r ex ey _ _ _ <<< "$("$drag" oswindows tool_windows | grep Equalizer)"
tear_to "$((ex + grab)),$((ey + pane + grip))" Playlist || {
    echo "check_group_lag.sh: the playlist would not come out of the stack" >&2
    exit 1
}

"$drag" oswindows tool_windows | grep -E 'Equalizer|Playlist' > "$out/before.txt"
grep -q Equalizer "$out/before.txt" && grep -q Playlist "$out/before.txt" || {
    echo "check_group_lag.sh: the two panes did not come out; see $out/before.txt" >&2
    exit 1
}
read -r ex ey _ _ _ <<< "$(grep Equalizer "$out/before.txt")"
read -r px py _ _ _ <<< "$(grep Playlist "$out/before.txt")"
rest_dx=$((px - ex)) rest_dy=$((py - ey))
echo "at rest the playlist sits ${rest_dx},${rest_dy} from the equalizer"

: > "$samples"
# Both windows are read out of one CGWindowList snapshot, so the pair is
# from a single instant. Asking the accessibility API for them one at a
# time reads them milliseconds apart, and a window moving under a fast
# drag travels far enough between the two reads to look like lag that is
# not there.
(
    swift - > "$samples" 2>/dev/null <<'SWIFT' &
import CoreGraphics
import Foundation
let deadline = Date().addingTimeInterval(4.0)
while Date() < deadline {
    let rows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    for row in rows where (row["kCGWindowOwnerName"] as? String) == "tool_windows" {
        let name = row["kCGWindowName"] as? String ?? ""
        guard name == "Equalizer" || name == "Playlist" else { continue }
        let b = row["kCGWindowBounds"] as? [String: Any] ?? [:]
        let x = b["X"] as? Double ?? 0, y = b["Y"] as? Double ?? 0
        print("\(Int(x)) \(Int(y)) \(name)")
    }
    print("--")
    Thread.sleep(forTimeInterval: 0.02)
}
SWIFT
    sampler=$!
    "$drag" drag "$((ex + grab)),$((ey + grip))" "$((ex - 320)),$((ey + 300))" 30 40 > /dev/null
    wait "$sampler" 2> /dev/null || true
)

worst="$(awk -v dx="$rest_dx" -v dy="$rest_dy" '
    /^--$/ { if (have_e && have_p) {
                 drift_x = (px - ex) - dx; drift_y = (py - ey) - dy
                 if (drift_x < 0) drift_x = -drift_x
                 if (drift_y < 0) drift_y = -drift_y
                 drift = drift_x > drift_y ? drift_x : drift_y
                 if (drift > worst) worst = drift
             }
             have_e = have_p = 0; next }
    $3 == "Equalizer" { ex = $1; ey = $2; have_e = 1 }
    $3 == "Playlist"  { px = $1; py = $2; have_p = 1 }
    END { print worst + 0 }
' "$samples")"

echo "worst drift while dragging: ${worst}px"
[ "$worst" -le "$allowed_drift" ] || {
    echo "FAIL the attached window fell ${worst}px behind the one carrying it"
    exit 1
}
echo "the attached window kept up"
