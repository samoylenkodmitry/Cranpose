#!/system/bin/sh
# One measurement window, run on the device so no adb round trip lands inside it.
# Usage: perf_window.sh PIDS PACKAGE LAYER END INTERVAL_S GFXINFO(0|1) START SLOW_FPS SLOW_WINDOW_S
# END is the run's end on /proc/uptime's clock: the launch plus the run's
# length, so every run lasts the same from its launch. START is the launch on
# that clock. A run whose layer presented fewer than SLOW_FPS frames a second
# over the last SLOW_WINDOW_S seconds ends there, with a SLOW line: the rest
# of it would only repeat that the app is slow.
# PIDS is one process id, or a comma-separated list for an app that runs in
# several (a browser): the first is the one that must stay alive.
# At the end, WHOLE lists every process of PACKAGE (named PACKAGE, or
# PACKAGE:name for its other processes), with the CPU it spent since it
# started: the app's whole run from its launch.
#
# Each interval also reads every process's CPU ticks and resident pages (PU,
# PS and PM lines) with the shell's own `read`, which starts no process, so
# the samples cost the device next to nothing.
#
# SurfaceFlinger keeps only the last frames of a layer: 128 on Android 10, 64
# on Android 17, which is half a second at 120 Hz. A poller of its own reads
# the layer every INTERVAL_S, so pick it below that span; the slower clock and
# thermal samples run beside it and never delay a poll. The host merges the
# polls and counts any two that fail to overlap.
PIDS=$(echo "$1" | tr ',' ' ')
PKG=$2
LAYER=$3
END=$4
INTERVAL=$5
GFX=$6
START=$7
SLOW_FPS=$8
SLOW_WINDOW=$9
# The Mali GPU clock where the kernel exposes it (kHz on the Pixel 9 Pro);
# the Kirin 980 reports its clocks on the F line instead.
MALI_CLOCK=/sys/class/misc/mali0/device/cur_freq
LATENCY_LOG=/data/local/tmp/perf_window_latency.$$
STOP=/data/local/tmp/perf_window_stop.$$
POLL=/data/local/tmp/perf_window_poll.$$
# One "UPTIME TOTAL" line per poll: the frames the layer presented so far.
COUNTS=/data/local/tmp/perf_window_counts.$$

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
  # The newest present counted, and the presents counted so far. A frame
  # whose present is still pending reads INT64_MAX and is counted later.
  newest=0
  total=0
  while [ ! -e "$STOP" ]; do
    latency > "$POLL"
    cat "$POLL"
    counted=$(awk -v newest="$newest" -v total="$total" '
      NF == 3 && $1 ~ /^[0-9]+$/ && $2 > newest && $2 < 9e18 { total++; if ($2 > last) last = $2 }
      END { printf "%.0f %d", (last > newest ? last : newest), total }' "$POLL")
    newest=${counted% *}
    total=${counted#* }
    echo "$(cut -d' ' -f1 /proc/uptime) $total" >> "$COUNTS"
    sleep "$INTERVAL"
  done
) > "$LATENCY_LOG" &
POLLER=$!
i=0
read_node() { cat "$1" 2>/dev/null || echo -; }
before_end() { awk -v now="$(cut -d' ' -f1 /proc/uptime)" -v end="$END" 'BEGIN { exit !(now < end) }'; }
# Whether, at the newest poll, the run was SLOW_WINDOW_S old and its last
# SLOW_WINDOW_S seconds held fewer presents than SLOW_FPS a second. The
# launch counts as a poll that found none.
slow() {
  awk -v start="$START" -v fps="$SLOW_FPS" -v window="$SLOW_WINDOW" '
    { at[NR] = $1; total[NR] = $2 }
    END {
      if (NR == 0 || at[NR] - start < window) exit 1
      before = 0
      for (i = NR; i > 0; i--) if (at[i] <= at[NR] - window) { before = total[i]; break }
      exit !(total[NR] - before < fps * window)
    }' "$COUNTS" 2>/dev/null
}
while before_end; do
  sleep $INTERVAL
  read -r up idle < /proc/uptime
  echo "PU $up"
  if slow; then
    echo "SLOW $up"
    break
  fi
  for pid in $PIDS; do
    if [ -r /proc/$pid/stat ]; then
      read -r stat < /proc/$pid/stat
      read -r pages < /proc/$pid/statm
      echo "PS $stat"
      echo "PM $pid $pages"
    fi
  done
  # Current clocks, then the caps thermal management applies to them; a
  # node the shell cannot read (the CPU caps, after some reboots) is "-".
  echo "F $(read_node /sys/class/devfreq/gpufreq/cur_freq) $(read_node /sys/class/devfreq/ddrfreq/cur_freq) $(read_node /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq) $(read_node /sys/devices/system/cpu/cpu4/cpufreq/scaling_cur_freq) $(read_node /sys/devices/system/cpu/cpu6/cpufreq/scaling_cur_freq) $(read_node /sys/class/devfreq/gpufreq/max_freq) $(read_node /sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq) $(read_node /sys/devices/system/cpu/cpu4/cpufreq/scaling_max_freq) $(read_node /sys/devices/system/cpu/cpu6/cpufreq/scaling_max_freq)"
  if [ -r "$MALI_CLOCK" ]; then echo "G $(cat $MALI_CLOCK)"; fi
  i=$((i+1))
  if [ $((i % 4)) = 0 ]; then thermal; fi
done
echo "T1 $(cat /proc/uptime)"
snap
echo WHOLE
for pid in $(ps -A -o PID,NAME | awk -v package="$PKG" '$2 == package || index($2, package ":") == 1 { print $1 }'); do
  echo "STAT $(cat /proc/$pid/stat 2>/dev/null)"
  for task in /proc/$pid/task/*; do
    echo "TASK $(cat $task/stat 2>/dev/null)"
  done
done
echo WHOLE_END
touch "$STOP"
wait "$POLLER"
latency >> "$LATENCY_LOG"
cat "$LATENCY_LOG"
rm -f "$LATENCY_LOG" "$STOP" "$POLL" "$COUNTS"
thermal
if [ "$GFX" = 1 ]; then
  echo GFX_BEGIN
  dumpsys gfxinfo $PKG
  echo GFX_END
fi
