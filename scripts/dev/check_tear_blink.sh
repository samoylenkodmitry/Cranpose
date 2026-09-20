#!/usr/bin/env bash
# Records the moment a tool pane tears into its own window and saves every
# frame of it, cropped to the desk the two windows share.
#
#   check_tear_blink.sh <out-dir> [seconds] [grip-y] [steps] [ms]
#
# The grip is a pane title's offset down the primary window: 43 is the
# Player, 154 the Equalizer, 270 the Playlist, which is the pane that leaves
# the bottom edge and so shows the most.
#
# `steps` and `ms` are the pointer's own pace. A slow carry, say 40 steps of
# 40ms, shows whether the window that came up under the pointer keeps
# following it or stops where it appeared.
#
# A tear changes two things at once: the primary window loses a pane and
# shrinks, and a new window comes up under the pointer. A dock changes them
# back. Anything that is not atomic shows up here as a frame the user should
# never see: a black band where the pane was, the primary's last frame
# stretched over its new size, or the pane drawn in both windows at once.
# The recording covers the tear and the dock that puts the pane back.
#
# Writes <out-dir>/tear.mov and <out-dir>/frame-####.png, one per frame, and
# prints the frames where the picture changed most, which is the tear. Look
# at those. Needs the demo built and `cliclick` and `ffmpeg` installed;
# macOS only.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
drag="$here/drag_window.sh"
out="${1:?usage: check_tear_blink.sh <out-dir> [seconds]}"
seconds="${2:-4}"
grip_y="${3:-270}"
steps="${4:-18}"
step_ms="${5:-14}"
mkdir -p "$out"
log="$out/tear_blink.log"
mov="$out/tear.mov"

# Where on the title the pointer takes hold, and how far it carries it.
grip_x=100 carry_x=200 carry_y=150

"$drag" launch tool_windows "$log" > /dev/null
frame="$("$drag" oswindows tool_windows | grep ' Cranpose Tool Windows$')"
read -r px py pw ph _ <<< "$frame"
echo "primary window at $px,$py size ${pw}x${ph}"

from_x=$((px + grip_x)) from_y=$((py + grip_y))
to_x=$((from_x + carry_x)) to_y=$((from_y + carry_y))

"$drag" record "$mov" "$seconds"
"$drag" drag "$from_x,$from_y" "$to_x,$to_y" "$steps" "$step_ms" > /dev/null
sleep 1
torn="$("$drag" windows "$log" | awk -v y="$grip_y" 'NR > 0 && $1 != ""' | tail -1)"
if [ -n "$torn" ]; then
    read -r _ tx ty tw _ <<< "$torn"
    "$drag" click "$((tx + tw - 16)),$((ty + 10))" > /dev/null
    echo "docked from $((tx + tw - 16)),$((ty + 10))"
fi
while pgrep -x ffmpeg > /dev/null; do sleep 0.5; done

# ffmpeg records the screen in pixels, the windows are reported in points.
video_w="$(ffprobe -v error -select_streams v -show_entries stream=width -of csv=p=0 "$mov")"
screen_w="$(osascript -e 'tell application "Finder" to get bounds of window of desktop' | awk -F', *' '{ print $3 }')"
scale=$((video_w / screen_w))
[ "$scale" -ge 1 ] || scale=1
crop_w=$(((pw + carry_x + 60) * scale))
crop_h=$(((ph + carry_y + 60) * scale))
crop_x=$(((px - 30) * scale))
crop_y=$(((py - 30) * scale))
crop="crop=${crop_w}:${crop_h}:${crop_x}:${crop_y}"
echo "screen scale ${scale}x, cropping ${crop_w}x${crop_h} at ${crop_x},${crop_y}"

rm -f "$out"/frame-*.png
ffmpeg -nostats -loglevel error -y -i "$mov" -vf "$crop,scale=iw/2:ih/2" "$out/frame-%04d.png"
echo "frames: $(ls "$out"/frame-*.png | wc -l | tr -d ' ') in $out"

# The frames that differ most from the one before them: the tear itself and
# whatever the windows did on the way through it.
ffmpeg -nostats -loglevel error -i "$mov" \
    -vf "$crop,tblend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-" \
    -f null - \
    | awk -F= '/YAVG/ { printf "%d %.3f\n", ++n + 1, $2 }' \
    | sort -k2 -g -r | head -12 | sort -n \
    | awk '{ printf "changed frame-%04d by %.3f\n", $1, $2 }'
pkill -f 'examples/tool_windows' 2>/dev/null || true
