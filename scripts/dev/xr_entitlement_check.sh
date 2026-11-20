#!/usr/bin/env bash
# Runs an immersive Cranpose app that was built with the `meta-entitlement` feature on a Meta Quest
# through adb. It shows what the entitlement check found and what the app did next. An APK that
# is not installed from the store fails the check, so the expected result of a sideloaded build is
# `NotEntitled`. The app then quits, or goes on when it chose `OnNotEntitled::Continue`.
#
# The caller holds the device lock and puts the headset on its wearer's face with
# `adb shell am broadcast -a com.oculus.vrpowermanager.prox_close`. The script sets no property and
# no setting. It saves the compositor's picture and the app's log lines.
#
# Usage: scripts/dev/xr_entitlement_check.sh <adb-serial> <package/activity> <output-dir> [apk]
set -euo pipefail

serial="${1:?adb serial}"
component="${2:?package/activity}"
out="${3:?output directory}"
apk="${4:-}"
package="${component%%/*}"
mkdir -p "$out"

adb_() { adb -s "$serial" "$@"; }

# The log of every process of the package since the last clear, which survives the process.
app_log() {
  adb_ logcat -d -v time |
    grep -E "Meta entitlement|PlatformSDK|cranpose xr: (entitlement check|ready|ends|Focused ran|[A-Za-z]+ ran [0-9]+ frames)|$package" |
    grep -v -E "^[-]+ beginning" || true
}

wait_for_line() {
  local pattern="$1" seconds="$2" i
  for ((i = 0; i < seconds * 4; i++)); do
    if app_log | grep -q -E "$pattern"; then
      return 0
    fi
    sleep 0.25
  done
  echo "xr-entitlement: '$pattern' did not appear in ${seconds} s" >&2
  return 1
}

if [[ -n "$apk" ]]; then
  adb_ install -r "$apk" >/dev/null
fi
adb_ shell am force-stop "$package"
adb_ logcat -c
started="$(date +%T)"
adb_ shell am start -n "$component" >/dev/null
echo "== started at $started"
wait_for_line "cranpose xr: entitlement check: " 45
sleep 1
adb_ exec-out screencap -p >"$out/after-check.png"
echo "== picture $out/after-check.png at $(date +%T)"
sleep 4
echo "== the process after the check: pid '$(adb_ shell pidof "$package" | tr -d '\r')'"
echo "== the front activity:"
adb_ shell dumpsys activity activities | grep -E "topResumedActivity" | head -2
app_log >"$out/check.log"
echo "== log, $out/check.log"
grep -E "entitlement|Entitlement|ends in|ready" "$out/check.log" | cut -c1-220
