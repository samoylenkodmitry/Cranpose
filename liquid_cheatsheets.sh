#!/usr/bin/env bash
# Builds per-component animation cheatsheets for VISION judging:
# one image per component, LEFT = reference frames from example/target,
# RIGHT = the same interaction captured live by the robot runners.
#
# Usage: DISPLAY=:0 ./liquid_cheatsheets.sh [output-dir]
# Requires ImageMagick (magick) and the desktop,robot-app feature set.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
OUT="${1:-$ROOT/target/liquid-cheatsheets}"
SHOTS="$OUT/shots"
mkdir -p "$SHOTS"

run_runner() {
    local name="$1" dir="$2"
    if [ ! -d "$dir" ] || [ -z "$(ls "$dir" 2>/dev/null)" ]; then
        echo "== capturing $name =="
        ROBOT_SHOT_DIR="$dir" cargo run -p desktop-app \
            --example "$name" --features desktop,robot-app >/dev/null 2>&1 || true
    fi
}

# Reuse the contract runners: they already drive every liquid animation with
# exact-clock keyframes and label their frames.
run_runner robot_liquid_motion_contract "$SHOTS/motion"
run_runner robot_liquid_bubble_physics "$SHOTS/bubble"
run_runner robot_text_loupe "$SHOTS/loupe"

# grid <out> <tile-geometry> <crop> <files...>
# Tiles are normalized to one height so TARGET (iPhone 3x) and ACTUAL
# (robot scale 1) frames compare at equal visual size.
TILE_HEIGHT=220
grid() {
    local out="$1" tiles="$2" crop="$3"
    shift 3
    local tmp
    tmp="$(mktemp -d)"
    local i=0
    for f in "$@"; do
        [ -f "$f" ] || continue
        if [ "$crop" = "-" ]; then
            magick "$f" -resize "x$TILE_HEIGHT" "$tmp/$(printf '%03d' $i).png"
        else
            magick "$f" -crop "$crop" +repage -resize "x$TILE_HEIGHT" \
                "$tmp/$(printf '%03d' $i).png"
        fi
        i=$((i + 1))
    done
    magick montage "$tmp"/*.png -tile "$tiles" -geometry +2+2 \
        -background gray25 "$out"
    rm -rf "$tmp"
}

# pair <component> <target-grid> <actual-grid>
pair() {
    local name="$1" left="$2" right="$3"
    magick "$left" -bordercolor gray25 -border 4 \
        -gravity North -background gray25 -splice 0x28 \
        -fill white -pointsize 20 -annotate +0+4 "TARGET" /tmp/_l.png
    magick "$right" -bordercolor gray25 -border 4 \
        -gravity North -background gray25 -splice 0x28 \
        -fill white -pointsize 20 -annotate +0+4 "ACTUAL" /tmp/_r.png
    magick /tmp/_l.png /tmp/_r.png -background gray10 +smush 12 \
        "$OUT/cheatsheet_$name.png"
    echo "wrote $OUT/cheatsheet_$name.png"
}

T="$ROOT/example/target"

# ---- toggle: press/flip/settle --------------------------------------------
grid "$OUT/_toggle_target.png" 3x3 - \
    "$T"/toggle-press/f_00{2,6}.png "$T"/toggle-press/f_0{10,14,17,20}.png \
    "$T"/toggle-press/f_0{26,34,48}.png
grid "$OUT/_toggle_actual.png" 3x3 "190x86+740+272" \
    "$SHOTS"/motion/toggle-press-0{21,66}ms.png \
    "$SHOTS"/motion/toggle-press-{101,161,241,321}ms.png \
    "$SHOTS"/motion/toggle-release-{051,231,651}ms.png
pair toggle "$OUT/_toggle_target.png" "$OUT/_toggle_actual.png"

# ---- menu: droplet growth ---------------------------------------------------
grid "$OUT/_menu_target.png" 3x3 - \
    "$T"/menu-open/f_00{1,9}.png "$T"/menu-open/f_0{17,25,33,41}.png \
    "$T"/menu-open/f_0{57,81,99}.png
grid "$OUT/_menu_actual.png" 3x3 "360x330+540+40" \
    "$SHOTS"/motion/menu-grow-0{21,54,75}ms.png \
    "$SHOTS"/motion/menu-grow-1{00,30,65}ms.png \
    "$SHOTS"/motion/menu-grow-{205,330,500}ms.png
pair menu "$OUT/_menu_target.png" "$OUT/_menu_actual.png"

# ---- tab bar: lens swipe/flight --------------------------------------------
grid "$OUT/_tabswipe_target.png" 3x3 - \
    "$T"/tab-swipe/f_00{1,8}.png "$T"/tab-swipe/f_0{15,22,29,36}.png \
    "$T"/tab-swipe/f_0{43,50,57}.png
grid "$OUT/_tabswipe_actual.png" 3x3 "760x130+70+375" \
    "$SHOTS"/bubble/flight-00{16,61,91}ms.png \
    "$SHOTS"/bubble/flight-0{131,191,281}ms.png \
    "$SHOTS"/bubble/flight-0{421,621,901}ms.png
pair tabswipe "$OUT/_tabswipe_target.png" "$OUT/_tabswipe_actual.png"

# ---- loupe: birth, steady, collapse ----------------------------------------
grid "$OUT/_loupe_target.png" 3x3 - \
    "$T"/text-selection/loupe-grow/g_0{01,05,09}.png \
    "$T"/text-selection/loupe-steady/h_0{01,30,61}.png \
    "$T"/text-selection/loupe-dissolve/i_0{01,07,13}.png
grid "$OUT/_loupe_actual.png" 3x3 "560x420+400+30" \
    "$SHOTS"/loupe/02-birth-017ms.png "$SHOTS"/loupe/03-grow-100ms.png \
    "$SHOTS"/loupe/05-grow-200ms.png "$SHOTS"/loupe/06-follow-a.png \
    "$SHOTS"/loupe/08-steady.png "$SHOTS"/loupe/09-collapse-033ms.png \
    "$SHOTS"/loupe/10-collapse-083ms.png "$SHOTS"/loupe/11-collapse-183ms.png \
    "$SHOTS"/loupe/11b-gone-270ms.png
pair loupe "$OUT/_loupe_target.png" "$OUT/_loupe_actual.png"

echo "cheatsheets in $OUT"
