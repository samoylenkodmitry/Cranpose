#!/system/bin/sh
# One measurement window, run on the device so no adb round trip lands inside it.
# Usage: perf_window.sh PIDS PACKAGE LAYER SAMPLES INTERVAL_S GFXINFO(0|1)
# PIDS is one process id, or a comma-separated list for an app that runs in
# several (a browser): the first is the one that must stay alive.
#
# SurfaceFlinger keeps only the last frames of a layer: 128 on Android 10, 64
# on Android 17, which is half a second at 120 Hz. A poller of its own reads
# the layer every INTERVAL_S, so pick it below that span; the slower clock and
# thermal samples run beside it and never delay a poll. The host merges the
# polls and counts any two that fail to overlap.
PIDS=$(echo "$1" | tr ',' ' ')
PKG=$2
LAYER=$3
SAMPLES=$4
INTERVAL=$5
GFX=$6
# The Mali GPU clock where the kernel exposes it (kHz on the Pixel 9 Pro);
# the Kirin 980 reports its clocks on the F line instead.
MALI_CLOCK=/sys/class/misc/mali0/device/cur_freq
LATENCY_LOG=/data/local/tmp/perf_window_latency.$$
STOP=/data/local/tmp/perf_window_stop.$$

snap() {
  first=1
  for pid in $PIDS; do
    stat=$(cat /proc/$pid/stat 2>/dev/null)
    # A helper process that ended is left out; the first, the app itself,
    # shows as an empty STAT line, which the host reads as the app's end.
    if [ -n "$stat" ] || [ $first = 1 ]; then echo "STAT $stat"; fi
    first=0
    for task in /proc/$pid/task/*; do
      echo "TASK $(cat $task/stat 2>/dev/null)"
    done
  done
}

thermal() {
  echo "THERMAL_BEGIN $(cat /proc/uptime)"
  dumpsys thermalservice | sed -n '/Current temperatures from HAL/,$p'
  echo "THERMAL_END"
}

latency() {
  echo "LAT_BEGIN $(cat /proc/uptime)"
  dumpsys SurfaceFlinger --latency "$LAYER"
  echo "LAT_END"
}

if [ "$GFX" = 1 ]; then dumpsys gfxinfo $PKG reset > /dev/null; fi
thermal
echo "T0 $(cat /proc/uptime)"
snap
(
  while [ ! -e "$STOP" ]; do
    latency
    sleep "$INTERVAL"
  done
) > "$LATENCY_LOG" &
POLLER=$!
i=0
read_node() { cat "$1" 2>/dev/null || echo -; }
while [ $i -lt $SAMPLES ]; do
  sleep $INTERVAL
  # Current clocks, then the caps thermal management applies to them; a
  # node the shell cannot read (the CPU caps, after some reboots) is "-".
  echo "F $(read_node /sys/class/devfreq/gpufreq/cur_freq) $(read_node /sys/class/devfreq/ddrfreq/cur_freq) $(read_node /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq) $(read_node /sys/devices/system/cpu/cpu4/cpufreq/scaling_cur_freq) $(read_node /sys/devices/system/cpu/cpu6/cpufreq/scaling_cur_freq) $(read_node /sys/class/devfreq/gpufreq/max_freq) $(read_node /sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq) $(read_node /sys/devices/system/cpu/cpu4/cpufreq/scaling_max_freq) $(read_node /sys/devices/system/cpu/cpu6/cpufreq/scaling_max_freq)"
  if [ -r "$MALI_CLOCK" ]; then echo "G $(cat $MALI_CLOCK)"; fi
  i=$((i+1))
  if [ $((i % 4)) = 0 ]; then thermal; fi
done
echo "T1 $(cat /proc/uptime)"
snap
touch "$STOP"
wait "$POLLER"
latency >> "$LATENCY_LOG"
cat "$LATENCY_LOG"
rm -f "$LATENCY_LOG" "$STOP"
thermal
if [ "$GFX" = 1 ]; then
  echo GFX_BEGIN
  dumpsys gfxinfo $PKG
  echo GFX_END
fi
