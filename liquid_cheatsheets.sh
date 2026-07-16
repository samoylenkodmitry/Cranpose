#!/usr/bin/env bash
# Builds one animation cheatsheet per example/target reference dir for
# VISION judging: LEFT = reference frames, RIGHT = the same interaction
# captured live from the Liquid UI demo by robot_liquid_cheatsheet_capture
# (plus robot_text_loupe for the text-selection cases).
#
# Usage: DISPLAY=:0 ./liquid_cheatsheets.sh [output-dir] [--recapture]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
OUT="${1:-$ROOT/target/liquid-cheatsheets}"
SHOTS="$OUT/shots"
CAP="$SHOTS/capture"
mkdir -p "$SHOTS"
if [ "${2:-}" = "--recapture" ]; then
    rm -rf "$CAP" "$SHOTS/loupe"
fi

if [ ! -d "$CAP" ] || [ -z "$(ls "$CAP" 2>/dev/null)" ]; then
    echo "== capturing liquid ui stages =="
    ROBOT_SHOT_DIR="$CAP" cargo run -p desktop-app \
        --example robot_liquid_cheatsheet_capture \
        --features desktop,robot-app >/dev/null 2>&1 || true
fi
if [ ! -d "$SHOTS/loupe" ] || [ -z "$(ls "$SHOTS/loupe" 2>/dev/null)" ]; then
    echo "== capturing text selection =="
    ROBOT_SHOT_DIR="$SHOTS/loupe" cargo run -p desktop-app \
        --example robot_text_loupe \
        --features desktop,robot-app >/dev/null 2>&1 || true
fi

# Tiles are normalized to one height so TARGET (iPhone 3x) and ACTUAL
# (robot scale 1) frames compare at equal visual size.
TILE_HEIGHT=200
# grid <out> <tiles> <files...>; set GRID_CROP=WxH+X+Y to crop every frame
# (robot full-window shots) before the tile resize.
grid() {
    local out="$1" tiles="$2"
    shift 2
    local tmp
    tmp="$(mktemp -d)"
    local i=0
    for f in "$@"; do
        [ -f "$f" ] || continue
        if [ -n "${GRID_CROP:-}" ]; then
            magick "$f" -crop "$GRID_CROP" +repage -resize "x$TILE_HEIGHT" \
                "$tmp/$(printf '%03d' $i).png"
        else
            magick "$f" -resize "x$TILE_HEIGHT" "$tmp/$(printf '%03d' $i).png"
        fi
        i=$((i + 1))
    done
    if [ "$i" -eq 0 ]; then
        echo "WARN: no frames for $out" >&2
        magick -size 200x100 xc:gray25 "$out"
        return
    fi
    magick montage "$tmp"/*.png -tile "$tiles" -geometry +2+2 \
        -background gray25 "$out"
    rm -rf "$tmp"
}

pair() {
    local name="$1" left="$2" right="$3"
    magick "$left" -bordercolor gray25 -border 4 \
        -gravity North -background gray25 -splice 0x28 \
        -fill white -pointsize 20 -annotate +0+4 "TARGET" /tmp/_cs_l.png
    magick "$right" -bordercolor gray25 -border 4 \
        -gravity North -background gray25 -splice 0x28 \
        -fill white -pointsize 20 -annotate +0+4 "ACTUAL" /tmp/_cs_r.png
    magick /tmp/_cs_l.png /tmp/_cs_r.png -background gray10 +smush 12 \
        "$OUT/cheatsheet_$name.png"
    echo "wrote $OUT/cheatsheet_$name.png"
}

# subsample <dir> <count>: evenly spaced frame paths from a target dir
subsample() {
    local dir="$1" count="$2"
    find "$dir" -maxdepth 1 -name "*.png" | sort | awk -v n="$count" '
        { lines[NR] = $0 }
        END {
            if (NR == 0) exit
            for (i = 0; i < n; i++) {
                idx = int(1 + i * (NR - 1) / (n - 1 < 1 ? 1 : n - 1))
                print lines[idx]
            }
        }'
}

T="$ROOT/example/target"

# ---- frame-series components ------------------------------------------------
series_sheet() {
    local name="$1" target_dir="$2" actual_dir="$3" tiles="$4"
    mapfile -t targets < <(subsample "$target_dir" 9)
    grid "$OUT/_${name}_t.png" "$tiles" "${targets[@]}"
    grid "$OUT/_${name}_a.png" "$tiles" "$actual_dir"/*.png
    pair "$name" "$OUT/_${name}_t.png" "$OUT/_${name}_a.png"
}

series_sheet toggle-press "$T/toggle-press" "$CAP/toggle-press" 3x3
series_sheet menu-open "$T/menu-open" "$CAP/menu-open" 3x3
series_sheet tab-swipe "$T/tab-swipe" "$CAP/tab-swipe" 2x5

# ---- still components -------------------------------------------------------
grid "$OUT/_barform_t.png" 1x4 \
    "$T"/bottom-bar-form/bar_over_orange_purple.png \
    "$T"/bottom-bar-form/bar_tiles_refracting.png \
    "$T"/bottom-bar-form/bar_headers_folded.png \
    "$T"/bottom-bar-form/bar_over_headers.png
grid "$OUT/_barform_a.png" 1x4 "$CAP"/bottom-bar-form/*.png
pair bottom-bar-form "$OUT/_barform_t.png" "$OUT/_barform_a.png"

# ---- on-white: the reference ships pre-composed state grids -----------------
grid "$OUT/_owc_a.png" 3x3 "$CAP"/on-white-click/*.png
pair on-white-click "$T/on-white/bottom-bar-click/translate-to-conversation.png" \
    "$OUT/_owc_a.png"
grid "$OUT/_owch_a.png" 3x4 "$CAP"/on-white-click-hold/*.png
pair on-white-click-hold "$T/on-white/bottom-bar-click-hold/gesture-1.png" \
    "$OUT/_owch_a.png"
grid "$OUT/_tus_a.png" 3x4 "$CAP"/touched-up-state/*.png
pair touched-up-state "$T/on-white/touched-up-state/touch-drag-release.png" \
    "$OUT/_tus_a.png"

# ---- text selection ----------------------------------------------------------
grid "$OUT/_loupe_t.png" 3x3 \
    "$T"/text-selection/loupe-grow/g_0{01,05,09}.png \
    "$T"/text-selection/loupe-steady/h_0{01,30,61}.png \
    "$T"/text-selection/loupe-dissolve/i_0{01,07,13}.png
GRID_CROP="620x460+330+10" grid "$OUT/_loupe_a.png" 3x3 \
    "$SHOTS"/loupe/02-birth-017ms.png "$SHOTS"/loupe/03-grow-100ms.png \
    "$SHOTS"/loupe/05-grow-200ms.png "$SHOTS"/loupe/06-follow-a.png \
    "$SHOTS"/loupe/08-steady.png "$SHOTS"/loupe/09-collapse-033ms.png \
    "$SHOTS"/loupe/10-collapse-083ms.png "$SHOTS"/loupe/11-collapse-183ms.png \
    "$SHOTS"/loupe/11b-gone-270ms.png
pair text-selection-loupe "$OUT/_loupe_t.png" "$OUT/_loupe_a.png"

mapfile -t menu_mat < <(subsample "$T/text-selection/menu-materialize" 4)
grid "$OUT/_mm_t.png" 2x2 "${menu_mat[@]}"
GRID_CROP="1060x260+40+300" grid "$OUT/_mm_a.png" 2x2 \
    "$SHOTS"/loupe/11b-gone-270ms.png "$SHOTS"/loupe/11c-menu-350ms.png \
    "$SHOTS"/loupe/11d-menu-450ms.png "$SHOTS"/loupe/12-menu-returned.png
pair menu-materialize "$OUT/_mm_t.png" "$OUT/_mm_a.png"

echo "cheatsheets in $OUT"
