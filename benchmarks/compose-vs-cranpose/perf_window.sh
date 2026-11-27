#!/system/bin/sh
# One launch and its measurement window, run on the device so no adb round
# trip lands inside them.
# Usage: perf_window.sh PACKAGE PROCESSES LAYER RUN_S INTERVAL_S GFXINFO(0|1) SLOW_FPS SLOW_SECONDS START...
# The script reads the clock (a LAUNCH line), runs `am start -W START...`,
# finds the app's processes (a PIDS line) and its buffer layer (LAYER lines),
# and measures until RUN_S seconds after the start command on /proc/uptime's
# clock, so every run lasts the same from its launch. A start the system
# refused ends the script with LAUNCH_FAILED; an app whose processes or layer
# are not there FIND_S seconds after the start command returned, with
# NO_PROCESS or NO_LAYER. A run whose layer presented fewer than SLOW_FPS
# frames in each of SLOW_SECONDS seconds in a row since its first frame ends
# there, with a SLOW line: the rest of it would only repeat that the app is
# slow.
# PROCESSES is `main`, the process named PACKAGE, or `every`, every process
# whose name starts with PACKAGE but its zygote, the one named PACKAGE first
# (a browser): the first is the one that must stay alive.
# LAYER is an extended regular expression that a layer's whole name matches;
# the first layer that matches is measured.
# At the end, WHOLE lists every process of PACKAGE (named PACKAGE, or
# PACKAGE:name for its other processes), with the CPU it spent since it
# started: the app's whole run from its launch.
#
# Each interval also reads every process's CPU ticks and resident pages (PU,
# PS and PM lines) with the shell's own `read`, which starts no process, so
# the samples cost the device next to nothing.
#
# SurfaceFlinger keeps only the last frames of a layer: 128 on Android 10, 64
# on Android 17, which is half a second at 120 Hz. The poller starts as soon
# as the layer is there, before anything else the window does, and reads the
# layer every INTERVAL_S, so pick it below that span; the slower clock and
# thermal samples run beside it and never delay a poll. The host merges the
# polls and counts any two that fail to overlap. A host that polls after the
# start command comes too late when adb answers slowly: on the Huawei Mate 20
# X on 2026-11-15 the first frames had left the history by then, and
# launches read as 2 to 3.6 s without a frame.
PKG=$1
PROCESSES=$2
LAYER_PATTERN=$3
RUN_S=$4
INTERVAL=$5
GFX=$6
SLOW_FPS=$7
SLOW_SECONDS=$8
shift 8
FIND_S=10

read -r LAUNCHED idle < /proc/uptime
echo "LAUNCH $LAUNCHED"
started=$(am start -W "$@" 2>&1)
echo "$started"
case "$started" in
  *"Status: ok"* | *"Status: timeout"*) ;;
  *) echo LAUNCH_FAILED; exit 0 ;;
esac
END=$(awk -v launched="$LAUNCHED" -v run="$RUN_S" 'BEGIN { printf "%.2f", launched + run }')

app_pids() {
  if [ "$PROCESSES" = main ]; then
    pidof "$PKG" | cut -d' ' -f1
  else
    ps -A -o PID,NAME | awk -v package="$PKG" '
      index($2, package) == 1 && $2 !~ /_zygote$/ { if ($2 == package) main = $1; else rest = rest " " $1 }
      END { if (main) print main rest }'
  fi
}

# Newer Android lists each layer as `RequestedLayerState{name parentId=...}`.
app_layers() {
  dumpsys SurfaceFlinger --list | awk -v pattern="$LAYER_PATTERN" '
    { sub(/^ +/, ""); sub(/^RequestedLayerState[{]/, ""); sub(/ parentId=.*/, "") }
    $0 ~ ("^(" pattern ")$") { print }'
}

before() { awk -v now="$(cut -d' ' -f1 /proc/uptime)" -v end="$1" 'BEGIN { exit !(now < end) }'; }

found_by=$(awk -v now="$(cut -d' ' -f1 /proc/uptime)" -v wait="$FIND_S" 'BEGIN { printf "%.2f", now + wait }')
PIDS=
LAYERS=
while :; do
  [ -n "$PIDS" ] || PIDS=$(app_pids)
  [ -n "$LAYERS" ] || LAYERS=$(app_layers)
  if [ -n "$PIDS" ] && [ -n "$LAYERS" ]; then break; fi
  # Android still shows the system splash when the app has drawn no buffer by
  # the full run's deadline. Keep the live process's CPU and its zero frames.
  if [ "$SLOW_FPS" = 0 ] && [ -n "$PIDS" ] && ! before "$END"; then
    case "$started" in
      *"Status: timeout"*)
        if dumpsys SurfaceFlinger --list | grep -Fq "Splash Screen $PKG#"; then
          echo SPLASH_ONLY
          break
        fi ;;
    esac
  fi
  if ! before "$found_by"; then
    if [ -z "$PIDS" ]; then echo NO_PROCESS; else echo NO_LAYER; fi
    exit 0
  fi
  sleep 0.1
done
echo "PIDS $PIDS"
if [ -n "$LAYERS" ]; then echo "$LAYERS" | sed 's/^/LAYER /'; fi
LAYER=$(echo "$LAYERS" | head -n 1)
# The Mali GPU clock where the kernel exposes it (kHz on the Pixel 9 Pro);
# the Kirin 980 reports its clocks on the F line instead.
MALI_CLOCK=/sys/class/misc/mali0/device/cur_freq
LATENCY_LOG=/data/local/tmp/perf_window_latency.$$
STOP=/data/local/tmp/perf_window_stop.$$
POLL=/data/local/tmp/perf_window_poll.$$
# One "UPTIME TOTAL" line per poll: the frames the layer presented so far.
COUNTS=/data/local/tmp/perf_window_counts.$$

snap() {
  for pid in $PIDS; do
    # A process that ended shows as an empty STAT line. The host checks after
    # the run that the app's own process still runs.
    echo "STAT $(cat /proc/$pid/stat 2>/dev/null)"
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
  [ -n "$LAYER" ] || return 0
  echo "LAT_BEGIN $(cat /proc/uptime)"
  dumpsys SurfaceFlinger --latency "$LAYER"
  echo "LAT_END"
}

(
  # The newest present counted, and the presents counted so far. A frame
  # whose present is still pending reads INT64_MAX and is counted later; a
  # frame's desired time may read -1 (Chrome's layer), so only the present
  # column decides.
  newest=0
  total=0
  while [ -n "$LAYER" ] && [ ! -e "$STOP" ]; do
    latency > "$POLL"
    cat "$POLL"
    counted=$(awk -v newest="$newest" -v total="$total" '
      NF == 3 && $2 ~ /^[0-9]+$/ && $2 > newest && $2 < 9e18 { total++; if ($2 > last) last = $2 }
      END { printf "%.0f %d", (last > newest ? last : newest), total }' "$POLL")
    newest=${counted% *}
    total=${counted#* }
    echo "$(cut -d' ' -f1 /proc/uptime) $total" >> "$COUNTS"
    sleep "$INTERVAL"
  done
) > "$LATENCY_LOG" &
POLLER=$!
if [ "$GFX" = 1 ]; then dumpsys gfxinfo $PKG reset > /dev/null; fi
thermal
i=0
read_node() { cat "$1" 2>/dev/null || echo -; }
# Whether, at the newest poll, each of the last SLOW_SECONDS whole seconds
# since the first frame held fewer presents than SLOW_FPS. The first poll that
# found a present stands for the first frame. A second's presents are the
# count of the newest poll at or before its end less that at or before its
# start. Before the first frame the app is still starting, which every run
# measures whole.
slow() {
  awk -v fps="$SLOW_FPS" -v seconds="$SLOW_SECONDS" '
    function total_at(t,    i) { for (i = NR; i > 0; i--) if (at[i] <= t) return total[i]; return 0 }
    { at[NR] = $1; total[NR] = $2; if (!started && $2 > 0) { started = 1; start = $1 } }
    END {
      if (!started) exit 1
      done = int(at[NR] - start)
      if (done < seconds) exit 1
      for (s = done - seconds; s < done; s++)
        if (total_at(start + s + 1) - total_at(start + s) >= fps) exit 1
      exit 0
    }' "$COUNTS" 2>/dev/null
}
while before "$END"; do
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
