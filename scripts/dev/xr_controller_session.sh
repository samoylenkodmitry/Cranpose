#!/usr/bin/env bash
# Runs one xr-demo build of the controller models on a Meta Quest through adb and saves what a
# check needs: the app's log with the runtime's answers about the controllers' models, the frame
# counts that the demo logs once a second, and the compositor's stereo picture.
#
# The script has not run on a headset yet. The caller holds the device lock and puts the headset
# on its wearer's face with `adb shell am broadcast -a com.oculus.vrpowermanager.prox_close`. The
# script sets no property and no setting. It installs the APK and starts the app. It saves the log
# of the first `seconds` seconds and one screenshot taken `shot-after` seconds after the session has
# focus. Then it stops the app.
#
# Usage: scripts/dev/xr_controller_session.sh <adb-serial> <apk> <output-dir> [seconds] [shot-after]
#
# The log lines of the demo's `XR_DEMO_CONTROLLERS=only` builds feed `xr_controller_check.py`. The
# summary of the frame rate is the mean of the demo's per-second frame counts over the run.
set -euo pipefail

serial="${1:?adb serial}"
apk="${2:?apk}"
out="${3:?output directory}"
seconds="${4:-20}"
shot_after="${5:-8}"
component="dev.cranpose.xrdemo/dev.cranpose.android.CranposeActivity"
package="${component%%/*}"
mkdir -p "$out"

adb_() { adb -s "$serial" "$@"; }

adb_ install -r "$apk" >/dev/null
adb_ logcat -c
adb_ shell am start -n "$component" >/dev/null

# Waits until the app's log holds `pattern`, at most `limit` seconds.
wait_for() {
  local pattern="$1" limit="$2" i
  for ((i = 0; i < limit * 4; i++)); do
    if (($(adb_ logcat -d -v time | grep -c -E "$pattern" || true) >= 1)); then
      return 0
    fi
    sleep 0.25
  done
  echo "xr-controller: '$pattern' did not appear in ${limit} s" >&2
  return 1
}

wait_for "cranpose xr: session FOCUSED" 40
sleep "$shot_after"
adb_ exec-out screencap -p >"$out/shot.png"
sleep "$((seconds - shot_after))"

pid="$(adb_ shell pidof "$package" | tr -d '\r')"
adb_ logcat -d -v time >"$out/logcat-all.txt"
if [[ -n "$pid" ]]; then
  adb_ logcat -d -v time --pid="$pid" >"$out/logcat.txt"
fi
adb_ shell am force-stop "$package"

grep -E "cranpose xr:|xr demo:|glTF" "$out/logcat.txt" >"$out/session.log" || true
echo "== log of the session, $out/session.log"
grep -E "controller|render model|model" "$out/session.log" | cut -c1-260 | head -60 || true
python3 - "$out/session.log" <<'PY'
import re
import sys

counts = [int(n) for n in re.findall(r"frames (\d+)$", open(sys.argv[1]).read(), re.M)]
if counts:
    # The first and the last second are partial
    middle = counts[1:-1] or counts
    print(f"== frames in each logged second: {counts}; mean {sum(middle) / len(middle):.1f}")
PY
