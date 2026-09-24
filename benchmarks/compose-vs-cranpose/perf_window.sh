#!/system/bin/sh
# One measurement window, run on the device so no adb round trip lands inside it.
# Usage: perf_window.sh PID PACKAGE LAYER SAMPLES INTERVAL_S GFXINFO(0|1)
#
# SurfaceFlinger keeps the last 128 frames of a layer; polling every second at
# 60 Hz (60 frames) never loses one, and the host merges the polls.
PID=$1
PKG=$2
LAYER=$3
SAMPLES=$4
INTERVAL=$5
GFX=$6

snap() {
  echo "STAT $(cat /proc/$PID/stat)"
  for task in /proc/$PID/task/*; do
    echo "TASK $(cat $task/stat 2>/dev/null)"
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
latency
i=0
while [ $i -lt $SAMPLES ]; do
  sleep $INTERVAL
  # Current clocks, then the caps thermal management applies to them.
  echo "F $(cat /sys/class/devfreq/gpufreq/cur_freq) $(cat /sys/class/devfreq/ddrfreq/cur_freq) $(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq) $(cat /sys/devices/system/cpu/cpu4/cpufreq/scaling_cur_freq) $(cat /sys/devices/system/cpu/cpu6/cpufreq/scaling_cur_freq) $(cat /sys/class/devfreq/gpufreq/max_freq) $(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq) $(cat /sys/devices/system/cpu/cpu4/cpufreq/scaling_max_freq) $(cat /sys/devices/system/cpu/cpu6/cpufreq/scaling_max_freq)"
  i=$((i+1))
  if [ $((i % 2)) = 0 ]; then latency; fi
  if [ $((i % 4)) = 0 ]; then thermal; fi
done
echo "T1 $(cat /proc/uptime)"
snap
latency
thermal
if [ "$GFX" = 1 ]; then
  echo GFX_BEGIN
  dumpsys gfxinfo $PKG
  echo GFX_END
fi
