#!/usr/bin/env bash
# Checks an immersive Cranpose app's focus and pause behavior on a Meta Quest through adb: no
# person looks into the headset. It starts the app, opens a system panel over it (the app loses
# input focus), brings the app back, then makes the activity pause and resume (the session stops
# and begins again). Each step saves the compositor's stereo picture with `screencap` and the
# app's log lines about the session.
#
# The caller holds the device lock and puts the headset on its wearer's face with
# `adb shell am broadcast -a com.oculus.vrpowermanager.prox_close`. The script sets no property
# and no setting. The system panel is Meta's documented `systemux://` deep link to the Files app.
#
# Usage: scripts/dev/xr_focus_check.sh <adb-serial> <package/activity> <output-dir> [apk]
set -euo pipefail

serial="${1:?adb serial}"
component="${2:?package/activity}"
out="${3:?output directory}"
apk="${4:-}"
package="${component%%/*}"
mkdir -p "$out"

adb_() { adb -s "$serial" "$@"; }

# Waits until the app's log holds `pattern` at least `count` times, at most `seconds` seconds.
wait_for() {
  local pattern="$1" count="$2" seconds="$3" i
  for ((i = 0; i < seconds * 4; i++)); do
    if (($(session_log | grep -c -E "$pattern" || true) >= count)); then
      return 0
    fi
    sleep 0.25
  done
  echo "xr-focus: '$pattern' did not appear $count times in ${seconds} s" >&2
  return 1
}

session_log() {
  local pid
  pid="$(adb_ shell pidof "$package" | tr -d '\r')"
  if [[ -z "$pid" ]]; then
    return 0
  fi
  adb_ logcat -d -v time --pid="$pid" |
    grep -E "cranpose xr: (session|[A-Za-z]+ ran [0-9]+ frames|ends|ready)|ActivityLifecycleListener" || true
}

shot() { adb_ exec-out screencap -p >"$out/$1.png"; }

if [[ -n "$apk" ]]; then
  adb_ install -r "$apk" >/dev/null
fi
adb_ logcat -c
adb_ shell am start -n "$component" >/dev/null
wait_for "session FOCUSED" 1 30
sleep 2
shot 1-focused
echo "== focused, picture $out/1-focused.png at $(date +%T)"

adb_ shell am start -a android.intent.action.VIEW -n com.oculus.vrshell/.MainActivity \
  -d "systemux://file-manager" -e uri "systemux://file-manager" >/dev/null
wait_for "session VISIBLE" 1 15
sleep 2
shot 2-visible-under-panel
echo "== input focus lost under a system panel, picture $out/2-visible-under-panel.png at $(date +%T)"

adb_ shell am start -n "$component" >/dev/null
wait_for "session FOCUSED" 2 30
sleep 2
shot 3-resumed
echo "== the app is back, picture $out/3-resumed.png at $(date +%T)"

session_log >"$out/session.log"
echo "== log of the session, $out/session.log"
cat "$out/session.log"
