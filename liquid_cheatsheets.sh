#!/usr/bin/env bash
# Runs the independent Liquid reference fixtures. Target sheets are static
# assets beside each fixture; E2E runs only capture presented X11 frames and
# append them beneath those stored targets.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"
OUT="${CRANPOSE_CHEATSHEET_OUT:-$ROOT/target/liquid-cheatsheets}"
PROFILE="${CRANPOSE_CHEATSHEET_PROFILE:-release}"

cases=(
    toggle_press
    menu_open
    tab_swipe
    segmented
    menu_expand
    bottom_bar_form
    on_white_click
    on_white_click_hold
    on_white_touched_up
    text_selection
)

if [ "$#" -gt 0 ]; then
    cases=("$@")
fi

profile_args=()
if [ "$PROFILE" = "release" ]; then
    profile_args=(--release)
elif [ "$PROFILE" != "debug" ]; then
    echo "CRANPOSE_CHEATSHEET_PROFILE must be 'release' or 'debug'" >&2
    exit 2
fi

build_args=(cargo build -p desktop-app --features desktop,robot-app)
build_args+=("${profile_args[@]}")
for case in "${cases[@]}"; do
    build_args+=(--example "robot_liquid_${case}_cheatsheet")
done
echo "== build ${#cases[@]} Liquid cheatsheet example(s) ($PROFILE) =="
"${build_args[@]}"

target_dir="${CARGO_TARGET_DIR:-$ROOT/target}"
if [[ "$target_dir" != /* ]]; then
    target_dir="$ROOT/$target_dir"
fi

for case in "${cases[@]}"; do
    example="robot_liquid_${case}_cheatsheet"
    echo "== $case: visible X11 capture ($PROFILE) =="
    ROBOT_SHOT_DIR="$OUT/$case" "$target_dir/$PROFILE/examples/$example"
done

overview_inputs=()
for case in "${cases[@]}"; do
    comparison="$OUT/$case/comparison-sheet.png"
    if [ ! -s "$comparison" ]; then
        echo "missing comparison sheet: $comparison" >&2
        exit 1
    fi
    overview_inputs+=(-label "$case" "$comparison")
done

magick montage \
    -background '#090c12' \
    -fill '#f5f7fa' \
    -pointsize 28 \
    -thumbnail '900x>' \
    "${overview_inputs[@]}" \
    -tile 2x \
    -geometry +18+18 \
    "$OUT/overview.png"

echo "comparison sheets: $OUT"
echo "overview: $OUT/overview.png"
