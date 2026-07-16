#!/usr/bin/env bash
# Builds one animation cheatsheet per example/target reference dir for
# VISION judging. Each sheet stacks a dense TARGET strip (every frame or
# every 2nd frame of the reference, millisecond-labeled), the matching
# dense ACTUAL strip captured live by robot_liquid_cheatsheet_capture
# (frames carry exact-clock ms in their filenames), and a DETAIL row of a
# few time-matched frames at large scale. Millisecond labels are the
# physics ruler: same ms → same expected pose.
#
# Usage: DISPLAY=:0 ./liquid_cheatsheets.sh [output-dir] [--recapture]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
OUT="${1:-$ROOT/target/liquid-cheatsheets}"
SHOTS="$OUT/shots"
CAP="$SHOTS/capture"
WORK="$OUT/work"
mkdir -p "$SHOTS"
rm -rf "$WORK"
mkdir -p "$WORK"
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

# Dense-strip tile height; DETAIL frames render at DETAIL_H.
TILE_H=106
DETAIL_H=340
N=0

# tile <src> <label> <out> [height] — one labeled tile. GRID_CROP=WxH+X+Y
# crops full-window robot shots to the stage region first.
tile() {
    local src="$1" label="$2" out="$3" h="${4:-$TILE_H}"
    magick "$src" ${GRID_CROP:+-crop "$GRID_CROP" +repage} \
        -resize "x$h" -bordercolor gray30 -border 1 \
        -gravity South -background gray15 -splice 0x15 \
        -fill white -pointsize 12 -annotate +0+1 "$label" "$out"
}

# strip <out> <cols> <file@label ...> — a dense labeled frame grid.
strip() {
    local out="$1" cols="$2"
    shift 2
    local tmp="$WORK/strip$((N++))"
    mkdir -p "$tmp"
    local i=0
    for spec in "$@"; do
        local f="${spec%@*}" label="${spec##*@}"
        [ -f "$f" ] || continue
        tile "$f" "$label" "$tmp/$(printf '%03d' $i).png"
        i=$((i + 1))
    done
    if [ "$i" -eq 0 ]; then
        echo "WARN: no frames for $out" >&2
        magick -size 400x100 xc:gray25 "$out"
        return
    fi
    magick montage "$tmp"/*.png -tile "${cols}x" -geometry +1+1 \
        -background gray25 "$out"
}

# section <title> <img> <out> — banner above a block.
section() {
    local title="$1" img="$2" out="$3"
    magick "$img" -gravity NorthWest -background gray10 -splice 0x24 \
        -fill '#9fd7ff' -pointsize 17 -annotate +6+3 "$title" "$out"
}

# sheet <name> <target_block> <actual_block> [detail_block]
# SHEET_NOTE, when set, is appended to the TARGET banner (not the filename).
sheet() {
    local name="$1" target="$2" actual="$3" detail="${4:-}"
    local parts=()
    section "TARGET — $name${SHEET_NOTE:+ — $SHEET_NOTE}" "$target" "$WORK/_t.png"
    section "ACTUAL" "$actual" "$WORK/_a.png"
    parts=("$WORK/_t.png" "$WORK/_a.png")
    if [ -n "$detail" ] && [ -f "$detail" ]; then
        section "DETAIL — time-matched frames at scale" "$detail" "$WORK/_d.png"
        parts+=("$WORK/_d.png")
    fi
    magick "${parts[@]}" -background gray10 -smush 10 "$OUT/cheatsheet_$name.png"
    echo "wrote $OUT/cheatsheet_$name.png"
}

# target_series <dir> <fps> <step> [start_n] — file@MSms per f_NNN.png
# frame, ms = (N - start_n) * 1000 / fps. start_n aligns t0 with the touch
# (frames before it are dropped so both timelines share their zero).
target_series() {
    local dir="$1" fps="$2" step="${3:-1}" start_n="${T0:-1}"
    find "$dir" -maxdepth 1 -name 'f_*.png' | LC_ALL=C sort | while read -r f; do
        local n="${f##*_}"
        n="${n%.png}"
        n=$((10#$n))
        if [ "$n" -ge "$start_n" ] && [ $(((n - start_n) % step)) -eq 0 ]; then
            echo "$f@$(((n - start_n) * 1000 / fps))ms"
        fi
    done
}

# actual_series <dir> — file@MSms from the runner's NN-TTTTms.png names.
actual_series() {
    local dir="$1"
    find "$dir" -maxdepth 1 -name '*.png' | LC_ALL=C sort | while read -r f; do
        local ms
        ms="$(basename "$f" .png | grep -oE '[0-9]+ms$' || true)"
        if [ -n "$ms" ]; then
            ms="$((10#${ms%ms}))ms"
        fi
        echo "$f@${ms:-$(basename "$f" .png)}"
    done
}

# nearest <ms> <file@label ...> — the spec whose ms label is closest.
nearest() {
    local want="$1"
    shift
    local best="" best_d=999999
    for spec in "$@"; do
        local label="${spec##*@}"
        local ms="${label%ms}"
        case "$ms" in '' | *[!0-9]*) continue ;; esac
        ms=$((10#$ms))
        local d=$((ms > want ? ms - want : want - ms))
        if [ "$d" -lt "$best_d" ]; then
            best_d=$d
            best="$spec"
        fi
    done
    echo "$best"
}

# detail_row <out> <height> <ms...> — for each requested ms, the nearest
# TARGET and ACTUAL frames side by side (one T|A pair per row) at <height>.
# Reads target specs from $WORK/dt.lst and actual specs from $WORK/da.lst.
detail_row() {
    local out="$1" h="$2"
    shift 2
    mapfile -t tspecs <"$WORK/dt.lst"
    mapfile -t aspecs <"$WORK/da.lst"
    local tmp="$WORK/detail$((N++))"
    mkdir -p "$tmp"
    local i=0
    for ms in "$@"; do
        local ts as
        ts="$(nearest "$ms" "${tspecs[@]}")"
        as="$(nearest "$ms" "${aspecs[@]}")"
        [ -n "$ts" ] && tile "${ts%@*}" "T ${ts##*@}" "$tmp/$(printf '%02d' $i)a.png" "$h"
        [ -n "$as" ] && tile "${as%@*}" "A ${as##*@}" "$tmp/$(printf '%02d' $i)b.png" "$h"
        i=$((i + 1))
    done
    magick montage "$tmp"/*.png -tile 2x -geometry +2+1 -background gray25 "$out"
}

# series_sheet <name> <target_dir> <fps> <t_step> <t_cols> <actual_dir>
#              <a_cols> <a_tile_h> <detail_h> [detail_ms...]
series_sheet() {
    local name="$1" tdir="$2" fps="$3" tstep="$4" tcols="$5"
    local adir="$6" acols="$7" ah="$8" dh="$9"
    shift 9
    mapfile -t tspecs < <(target_series "$tdir" "$fps" "$tstep")
    mapfile -t aspecs < <(actual_series "$adir")
    strip "$WORK/t.png" "$tcols" "${tspecs[@]}"
    TILE_H="$ah" strip "$WORK/a.png" "$acols" "${aspecs[@]}"
    local detail=""
    if [ "$#" -gt 0 ]; then
        printf '%s\n' "${tspecs[@]}" >"$WORK/dt.lst"
        printf '%s\n' "${aspecs[@]}" >"$WORK/da.lst"
        detail="$WORK/d.png"
        detail_row "$detail" "$dh" "$@"
    fi
    sheet "$name" "$WORK/t.png" "$WORK/a.png" "$detail"
}

T="$ROOT/example/target"

# ---- frame-series components ------------------------------------------------
# toggle-press: 54 frames @60fps from the press moment.
series_sheet toggle-press "$T/toggle-press" 60 1 9 \
    "$CAP/toggle-press" 9 "$TILE_H" 300 130 260 500
# menu-open: 108 frames @60fps; t0 = the touch (swell starts at f_025, so
# both timelines count from contact; the pre-touch idle is dropped).
T0=25 series_sheet menu-open "$T/menu-open" 60 2 12 \
    "$CAP/menu-open" 10 "$TILE_H" 300 75 150 400
# tab-swipe: 57 frames @30fps, ~1.9s ride.
series_sheet tab-swipe "$T/tab-swipe" 30 2 5 \
    "$CAP/tab-swipe" 3 80 140 400 1000 1600
# segmented (Receiving | Sending | Errored @60fps): wide flat tiles.
TILE_H=48 series_sheet segmented-tap-flight "$T/segmented/tap-flight" 60 1 5 \
    "$CAP/segmented-tap-flight" 5 48 130 75 150 300
TILE_H=48 series_sheet segmented-drag "$T/segmented/drag" 60 3 5 \
    "$CAP/segmented-drag" 5 48 130 500 1200 2000

# ---- menu-expand: dark sort/filter menu, accordion size morphs ---------------
menu_expand_phase() {
    local dir="$1" out="$2" title="$3"
    local specs
    mapfile -t specs < <(target_series "$dir" 60 2)
    strip "$WORK/mep.png" 12 "${specs[@]}"
    section "$title" "$WORK/mep.png" "$out"
}
menu_expand_phase "$T/menu-expand/open" "$WORK/me1.png" "open (pill -> two-row menu)"
menu_expand_phase "$T/menu-expand/expand" "$WORK/me2.png" "expand (accordion grows, rows materialize)"
menu_expand_phase "$T/menu-expand/collapse-row" "$WORK/me3.png" "collapse-row (back to two rows)"
menu_expand_phase "$T/menu-expand/close" "$WORK/me4.png" "close (sucked into the pill)"
magick "$WORK/me1.png" "$WORK/me2.png" "$WORK/me3.png" "$WORK/me4.png" \
    -background gray10 -smush 6 "$WORK/me_t.png"
mapfile -t mea < <(actual_series "$CAP/menu-expand")
strip "$WORK/me_a.png" 12 "${mea[@]}"
SHEET_NOTE="actual timeline: open 0+, expand 1000+, collapse 2000+, close 3000+" \
    sheet menu-expand "$WORK/me_t.png" "$WORK/me_a.png"

# ---- still components -------------------------------------------------------
strip "$WORK/bft.png" 1 \
    "$T/bottom-bar-form/bar_over_orange_purple.png@over orange-purple" \
    "$T/bottom-bar-form/bar_tiles_refracting.png@tiles refracting" \
    "$T/bottom-bar-form/bar_headers_folded.png@headers folded" \
    "$T/bottom-bar-form/bar_over_headers.png@over headers"
mapfile -t bfa < <(actual_series "$CAP/bottom-bar-form")
strip "$WORK/bfa.png" 1 "${bfa[@]}"
sheet bottom-bar-form "$WORK/bft.png" "$WORK/bfa.png"

# ---- on-white: reference ships pre-composed labeled state grids -------------
# (tile labels there are native frame numbers; recordings run ~60fps, so one
# frame ≈ 16.7ms — annotate that on the banner via the sheet name.)
on_white_sheet() {
    local name="$1" target_grid="$2" adir="$3" acols="$4"
    magick "$target_grid" -resize "1700x>" "$WORK/owt.png"
    mapfile -t aspecs < <(actual_series "$adir")
    strip "$WORK/owa.png" "$acols" "${aspecs[@]}"
    SHEET_NOTE="target labels = native frame# @60fps (16.7ms each)" \
        sheet "$name" "$WORK/owt.png" "$WORK/owa.png"
}
on_white_sheet on-white-click \
    "$T/on-white/bottom-bar-click/translate-to-conversation.png" \
    "$CAP/on-white-click" 5
on_white_sheet on-white-click-hold \
    "$T/on-white/bottom-bar-click-hold/gesture-1.png" \
    "$CAP/on-white-click-hold" 5
on_white_sheet touched-up-state \
    "$T/on-white/touched-up-state/touch-drag-release.png" \
    "$CAP/touched-up-state" 6

# ---- text selection ----------------------------------------------------------
# Target frames are named <x>_NNN.png where NNN is the NATIVE 120fps frame
# number of the source recording; ms counts from the first frame of the dir.
seq_series() {
    local dir="$1" fps="$2"
    local first=-1
    find "$dir" -maxdepth 1 -name '*_*.png' | LC_ALL=C sort | while read -r f; do
        local n="${f##*_}"
        n="${n%.png}"
        n=$((10#$n))
        if [ "$first" -lt 0 ]; then first=$n; fi
        echo "$f@$(((n - first) * 1000 / fps))ms"
    done
}
loupe_target_specs() {
    seq_series "$T/text-selection/loupe-grow" 120
    echo "$T/text-selection/loupe-steady/h_001.png@steady"
    echo "$T/text-selection/loupe-steady/h_030.png@steady"
    echo "$T/text-selection/loupe-steady/h_061.png@steady"
    seq_series "$T/text-selection/loupe-dissolve" 120 | sed 's/@/@rel /'
}
mapfile -t lts < <(loupe_target_specs)
strip "$WORK/lt.png" 7 "${lts[@]}"
mapfile -t las < <(actual_series "$SHOTS/loupe")
GRID_CROP="620x460+330+10" TILE_H=140 strip "$WORK/la.png" 7 "${las[@]}"
sheet text-selection-loupe "$WORK/lt.png" "$WORK/la.png"

# menu-materialize: 15 frames over 234ms.
mapfile -t mmt < <(seq_series "$T/text-selection/menu-materialize" 120)
strip "$WORK/mmt.png" 5 "${mmt[@]}"
mapfile -t mma < <(actual_series "$SHOTS/loupe" | grep -E '11[bcd]-|12-menu')
GRID_CROP="1060x260+40+300" TILE_H=90 strip "$WORK/mma.png" 4 "${mma[@]}"
sheet menu-materialize "$WORK/mmt.png" "$WORK/mma.png"

echo "cheatsheets in $OUT"
