#!/usr/bin/env bash
# Checks the spatial audio of an immersive Cranpose app on a headset through adb with numbers. No
# person listens. The app is a build of apps/xr-demo made with `XR_DEMO_SOUND=swing` and with the
# `audio-tap` feature of `cranpose-xr`. The build plays its hum from a place that stays level with
# the head. The place is 50 cm to the left of the head for three seconds. Then it is 50 cm to the
# right of the head for three seconds. The tap logs one line for each second of output. The line
# holds the root mean square of the left channel and of the right channel of the mix and where the
# source stands relative to the head. The script reads the lines. It fails unless the left level is
# the higher one while the source is at the left and the right level is the higher one while the
# source is at the right.
#
# The caller holds the device lock and puts the headset on its wearer's face with
# `adb shell am broadcast -a com.oculus.vrpowermanager.prox_close`. The script sets no property and no
# setting.
#
# Usage: scripts/dev/xr_audio_check.sh <adb-serial> <package/activity> <output-dir> [apk]
set -euo pipefail

serial="${1:?adb serial}"
component="${2:?package/activity}"
out="${3:?output directory}"
apk="${4:-}"
package="${component%%/*}"
mkdir -p "$out"

adb_() { adb -s "$serial" "$@"; }

# The app's log lines of the tap.
tap_lines() {
  local pid
  pid="$(adb_ shell pidof "$package" | tr -d '\r')"
  if [[ -z "$pid" ]]; then
    return 0
  fi
  adb_ logcat -d -v time --pid="$pid" | grep -E "cranpose xr: audio tap" || true
}

if [[ -n "$apk" ]]; then
  adb_ install -r "$apk" >/dev/null
fi
adb_ logcat -c
adb_ shell am start -n "$component" >/dev/null

# Waits for 14 seconds of unpaused output: the source holds each side for three seconds.
for ((i = 0; i < 120; i++)); do
  if (($(tap_lines | grep -c "paused false" || true) >= 14)); then
    break
  fi
  sleep 0.5
done
tap_lines >"$out/tap.log"
echo "== the tap's lines, $out/tap.log"
cat "$out/tap.log"

# One row for each line: the second, the left level, the right level, the paused flag and the
# source's x relative to the head. A line logs the levels of the second that just ended and the place
# of the source at the time of the line. The first line after the source changed its side holds
# levels that mix the two sides, so the check leaves it out.
rows="$(sed -nE 's/.*second ([0-9]+): left ([0-9.]+) right ([0-9.]+) \(rms\), paused (true|false), the source is at x ([-+0-9.]+) y .*/\1 \2 \3 \4 \5/p' "$out/tap.log")"
echo "$rows" | awk '
  $4 == "true" { next }
  { side = ($5 < -0.3) ? "left" : (($5 > 0.3) ? "right" : "middle") }
  side != previous { previous = side; next }
  side == "left" { lefts++; if ($2 > $3) { left_wins++ } }
  side == "right" { rights++; if ($3 > $2) { right_wins++ } }
  END {
    printf "source at the left: %d seconds, the left level higher in %d\n", lefts, left_wins
    printf "source at the right: %d seconds, the right level higher in %d\n", rights, right_wins
    if (lefts < 2 || rights < 2 || left_wins != lefts || right_wins != rights) {
      print "xr-audio: the levels do not follow the side of the source" > "/dev/stderr"
      exit 1
    }
    print "xr-audio: the left level rises when the source is at the left, the right when it is at the right"
  }'
