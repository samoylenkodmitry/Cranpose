#!/usr/bin/env bash
# Presented-surface cross-tab ghost probe. Drives the REAL demo binary with
# XTEST input (xdotool) and captures the presented window with X11 `import` —
# fully independent of the renderer's offscreen capture path, which rebuilds
# the scene and therefore can never show incremental-scene ghosts.
#
# Reproduced bug (fixed 2026-07-11): liquid tab -> wheel scroll -> switch to
# markdown left the liquid tab bar + search circle compositing over the
# markdown page. Root cause: structural recompositions routed through the
# scoped scene update never evicted removed subtrees from the persistent
# render graph (guarded by
# app_shell_tests::keyed_subtree_swap_with_pending_scoped_repass_evicts_stale_layers).
#
# Usage: DISPLAY=:0.0 apps/desktop-demo/tools/x11_ghost_probe.sh <binary> <outdir>
# The window must be placed on a REAL monitor: on multi-monitor X11 layouts
# the virtual screen can contain dead zones (regions with no monitor) where a
# WM may place the window — grabs still "work" there but the pointer cannot
# enter, so every click silently misses.
set -euo pipefail

BIN="${1:?usage: x11_ghost_probe.sh <demo-binary> <outdir>}"
OUT="${2:?usage: x11_ghost_probe.sh <demo-binary> <outdir>}"
mkdir -p "$OUT"

"$BIN" &
APP_PID=$!
trap 'kill "$APP_PID" 2>/dev/null || true' EXIT
sleep 4

WID=$(xdotool search --name "Cranpose Demo" | tail -1)
xdotool windowsize "$WID" 1400 900
# Keep the window inside a real monitor; adjust for the local layout.
xdotool windowmove "$WID" 2200 80
xdotool windowraise "$WID"
xdotool windowactivate "$WID"
sleep 1
eval "$(xdotool getwindowgeometry --shell "$WID")"

strip_y=$((Y + 71))
# The tab strip only consumes horizontal wheel; the shell maps Alt+wheel.
xdotool mousemove $((X + 700)) "$strip_y"
xdotool keydown alt
for _ in $(seq 1 28); do xdotool click 5; sleep 0.02; done
xdotool keyup alt
sleep 1

# Liquid UI tab sits near the strip's end at this width; Markdown follows it.
xdotool mousemove $((X + 1197)) "$strip_y" click 1
sleep 2
xdotool mousemove $((X + 700)) $((Y + 500))
for _ in 1 2 3 4 5 6; do xdotool click 5; sleep 0.08; done
sleep 1.5
xdotool mousemove $((X + 1345)) "$strip_y" click 1
sleep 1
import -window "$WID" "$OUT/markdown_after_liquid.png"
sleep 1.5
import -window "$WID" "$OUT/markdown_after_liquid_settled.png"

echo "grabs in $OUT — the markdown page must show no liquid tab bar/search circle"
