# Performance benchmark methodology

Every nightly benchmark measures each framework from outside the app process.
No benchmark app counts its own frames or computes its own frame rate, CPU usage,
or memory footprint.

## External measurement

### macOS desktop and Chrome browser

[`desktop.py`](../benchmarks/compose-vs-cranpose/desktop.py) runs each app alone
in a 1280 x 820 window on an idle Mac.

- **Frames and intervals:** [`FrameCount.app`](../benchmarks/compose-vs-cranpose/framecount/FrameCount.swift)
  captures the window from a separate process through `ScreenCaptureKit`. It
  hashes the captured pixels at 1/8 scale and counts only frames whose pixels
  changed from the previous frame. This filters out duplicate `WindowServer`
  composites caused by anything else on the display. Cross-checking against a
  Metal layer drawable hook matched the count within 1 fps.
- **Display validation:** Before the first leg and after the last leg,
  `FrameCount.app` checks `CVDisplayLink` ticks and measures an unchanged blank
  window. A run fails and publishes nothing when the display is asleep, locked,
  not 60 Hz, not 2x scale, or disturbed by ambient motion.
- **CPU:** `ps` and `libproc` (`proc_pid_rusage`) sum the CPU time of the app,
  every child process, launchd WebKit services for WKWebView apps, and
  `WindowServer`. Other processes are tracked separately; a disturbed leg is
  retried and kept out of medians.
- **Memory and clocks:** macOS `footprint` reports total physical footprint and
  GPU memory (`IOAccelerator` and `IOSurface`). `macmon` reads performance-core
  and GPU clocks from hardware counters.

### Android native and Chrome browser

[`measure.py`](../benchmarks/compose-vs-cranpose/measure.py),
[`frameworks.py`](../benchmarks/compose-vs-cranpose/frameworks.py), and
[`perf_window.sh`](../benchmarks/compose-vs-cranpose/perf_window.sh) measure
each app on a physical Android device without adb round trips inside the timed
window.

- **Frames and intervals:** `perf_window.sh` polls
  `dumpsys SurfaceFlinger --latency <layer>` on the device. Frame counts,
  `first_frame_s`, and intervals come from SurfaceFlinger presentation
  timestamps.
- **CPU, memory, clocks, and thermals:** `/proc/<pid>/stat` and
  `/proc/<pid>/task/*/stat` provide CPU ticks. `dumpsys meminfo` provides PSS
  and GPU memory (`GL mtrack` and `EGL mtrack`), while `/proc/<pid>/statm`
  samples RSS during the run. Sysfs `devfreq` and `cpufreq` nodes report clocks
  and thermal caps. `dumpsys thermalservice` reports live HAL temperatures.

## What lives inside each app

Framework source links on the Performance tab open the app source file directly.
Two patterns appear in every app and are not measurement logic:

1. **Deterministic workload stepper (`frame += 1`):** Each app advances an
   integer frame index through its own framework's frame callback
   (`next_frame()`, `withFrameNanos`, `request_repaint`, `window::frames`,
   `request_animation_frame`, `CADisplayLink`, `createTicker`,
   `requestAnimationFrame`, or equivalent). Every animation, scroll offset, and
   text value derives from that integer index rather than wall time. Every
   framework therefore performs the same work per frame.
2. **One-shot lifecycle markers:**
   - `PERF first_frame` logs once as a liveness check. On native desktop and
     Android, `first_frame_s` comes from `FrameCount.app` (`showsContent`) or
     SurfaceFlinger. On desktop browser runs, `PERF first_frame` acts only as a
     lower bound so Chrome's own shell does not count as page content.
   - `PERF frozen frame=N` fires only when `PERF_FREEZE > 0` during `--parity`
     screenshot comparisons. Nightly performance runs set `freeze = 0`.

## Protocol and reading the numbers

- **Full launch span:** Every leg runs for the same fixed span from its launch
  command (10 s by default). Startup time and first-frame delay count toward
  headline `fps`.
- **First start vs second start:** First start clears app caches (and re-signs
  a fresh bundle copy on macOS or runs `pm clear` on Android). Second start runs
  immediately after with warm caches.
- **60 Hz display cap:** Displays run at 60 Hz. When multiple frameworks hold
  60 fps in steady state, headline `fps` reflects startup speed while
  `CPU ms per frame` distinguishes steady-state cost.
- **Scenarios:**
  - `gauntlet`: scrolling card grid, ticker strip, deep nested clusters,
    translucent rotating layers, and per-frame width changes.
  - `wall`: thousands of static tiles with only a clock text and a cursor
    outline changing each frame.
  - `patchwork`: nine independent patches with targeted mutations beside a
    streaming chat pane.

## Source references

- Full benchmark specification: [`benchmarks/compose-vs-cranpose/README.md`](../benchmarks/compose-vs-cranpose/README.md)
- macOS harness: [`benchmarks/compose-vs-cranpose/desktop.py`](../benchmarks/compose-vs-cranpose/desktop.py)
- macOS external frame counter: [`benchmarks/compose-vs-cranpose/framecount/FrameCount.swift`](../benchmarks/compose-vs-cranpose/framecount/FrameCount.swift)
- Android harness: [`benchmarks/compose-vs-cranpose/measure.py`](../benchmarks/compose-vs-cranpose/measure.py)
- On-device Android sampler: [`benchmarks/compose-vs-cranpose/perf_window.sh`](../benchmarks/compose-vs-cranpose/perf_window.sh)
- Nightly workflow: [`.github/workflows/perf-nightly.yml`](../.github/workflows/perf-nightly.yml)
