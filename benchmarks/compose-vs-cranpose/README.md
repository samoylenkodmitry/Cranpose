# Cranpose vs Jetpack Compose on a device

Two apps with the same UI, element for element: `cranpose-app` (Rust, Cranpose
from this repository) and `compose-app` (Kotlin, Jetpack Compose from BOM
2026.09.00, foundation 1.12). `measure.py` runs them alternately on one Android
device and measures both from outside either framework.

## Scenarios

Each app takes the scenario and its size from intent extras
(`--es scenario NAME --ei rows 30 …`) and animates itself from its frame clock
(`withFrameNanos` / `next_frame()`). Both apps use the same animation rules and
per-frame workload, without touch input. Their work per second depends on the
frame rate. Workspace quote streaming separately targets the same 16 ms cadence.

`measure.py` uses a **heavy** load for each scenario so the apps expose more
of their frame work. The Huawei Mate 20 X baseline kept Compose below 60 fps
at this load. `--load default` uses each app's lighter sizes. Record the device
model with every comparison because frame capacity varies by device.

| Scenario | What changes each frame | Heavy load | Stresses |
| --- | --- | --- | --- |
| `feed` | Endless `LazyColumn` of post cards that auto-scrolls. Each card has an avatar, 6 text blocks, a 24-bar gradient chart, and wrapping rows of chips and counters. Stable keys and content types. | 4 cards per row, everything at 0.3× size, 12,000 dp/s | Lazy composition, text layout |
| `ticker` | Quote rows each read the frame time and recompose: price and percent text change, and the bar width changes. | 72 rows | Recomposition, text shaping |
| `particles` | One canvas draws circles and rounded squares at positions computed from the frame time, read in the draw phase only. | 20,000 shapes | Draw recording, GPU fill |
| `layers` | Tiles with text rotate, scale and fade through a graphics layer block that reads the frame time. Nothing recomposes or redraws. | 12 × 22 tiles | Animated transformed layers |
| `grid` | The container width follows the frame clock, so a flat grid of cells with wrapping labels is measured again every frame. | 30 × 12 cells | Flat relayout, text re-measure |
| `grid_layer` | `grid`, with every cell in a static rotated graphics layer. | 30 × 12 cells | Relayout under many layers |
| `deep` | The same width animation over 40 levels nested inside one another, each with a row of wrapping chips. | 40 levels × 6 chips | Deep relayout |
| `deep_layer` | `deep`, with every level in a static rotated graphics layer. | 40 levels × 6 chips | Relayout under nested layers |
| `workspace` | GPUI trading workspace: quote subscriptions, watchlist, chart, order book, trades and statistics. | 200 symbols, 16 hidden subscriptions | Text updates, clipping, scrolling and hover |

Knobs:

- `feed`: `--ef speed`, `--ei fcols`, `--ef ui` and `--ei comments`;
- `ticker`: `--ei quotes`;
- `particles`: `--ei particles`;
- `layers`: `--ei tcols` and `--ei trows`, plus `--ez opaque true`, which fixes
  alpha at 1;
- `grid` and `grid_layer`: `--ei rows` and `--ei cols`;
- `deep` and `deep_layer`: `--ei depth` and `--ei chips`;
- `workspace`: `--es mode quotes`, `scroll` or `hover` (default `quotes`);
- `--ez still true` holds the feed still, or starts the workspace at tick zero
  with its stream, automatic scrolling and synthetic hover stopped.

## Parity rules

- **Data:** `data.rs` and `Data.kt` implement the same xorshift generator, so
  every post, comment, quote and particle is identical.
- **Fonts:** both apps load `/system/fonts/Roboto-Regular.ttf` and
  `Roboto-Bold.ttf` from the device and set a 1.4 em line height. The bundled
  Noto Sans Merged declares 2.1 em of ascent plus descent, which Compose honors
  and Cranpose does not, so it cannot be compared. The workspace also loads
  `Roboto-Medium.ttf` and follows GPUI's 1.618034 em line height, including the
  leading above and below single-line labels. Compose uses centered line
  height, `Trim.None` and `includeFontPadding = false` for that contract.
- **Window:** the same fullscreen theme, `singleTask` and `configChanges`, and
  one arm64 build each.
- **Release builds:**
  - Compose: R8 with resource shrinking, not debuggable, and fully
    AOT-compiled with `cmd package compile -m speed -f`. That is Compose's best
    case; Play installs use `speed-profile`.
  - Cranpose: `opt-level = 3`, fat LTO, one codegen unit, `panic = "abort"`,
    and R8 for its Java.
- **Idioms:** both apps use the optimized pattern in their framework:
  - state is read in the phase that needs it (a lambda graphics layer, a draw
    lambda, the row's own recompose scope);
  - drawing allocates no per-frame objects;
  - lazy items have stable keys and content types;
    - numbers are formatted by hand.

## Workspace reference and interaction checks

The [GPUI reference fixture](reference/README.md) preserves the exact source patch,
font hashes and setup steps used for the desktop comparison.

The workspace follows `gpui_perf/src/showcase` from GPUI reference commit
`7ab23f46f2ba3a040ceb27d387383a2896bc5ae1`. Its canvas is 1280 × 820 logical
pixels, scaled uniformly to fit a narrower display. The light theme, tick-zero
market data and device Roboto files must match before comparing screenshots.
Crop OS chrome and the unused area below the Android canvas; compare the
client areas at the same logical size. Font rasterization and density rounding
can still differ across Android and macOS.

The layout contract includes a 40-pixel showcase toolbar, 256-pixel gallery
sidebar, 40-pixel workspace toolbar, 48-pixel icon rail and 28-pixel diagnostic
footer. The page header keeps its natural text height and 16-pixel vertical
padding. Dock tabs are 32 pixels high; middle and right columns are 460 and
380 pixels wide. One-pixel borders consume layout space. The selected quote
uses baseline-aligned prices and two equal columns of statistics. Charts keep
the same plot, price-axis and time-axis coordinates as GPUI.

The shared interactive controls are symbol search, dock tabs, quote streaming
and Off/Sidebar/Watchlist automatic scrolling. Search gains editing focus at
launch and opens Android's software keyboard when tapped. Alternate dock tabs show the
subscription count when opened, as GPUI does. Destination chips, market-filter
chips and chart-period chips remain static in the reference. Full gallery
pages, gallery refresh and GPUI's retention toggle are outside this workspace
benchmark; the corresponding unsupported Compose controls are disabled.

`quotes` delivers 16 quote events every 16 ms; `scroll` and `hover` deliver 8.
Automatic scrolling advances 32 logical pixels per frame callback. Hover
dispatches real mouse events through the view's input path: at x = 120 within
the watchlist row viewport, it visits each complete visible row's center and
bounces back, one row per callback. `still` disables this driver at launch.
The footer's **UI updates/s** counts frame-clock callbacks, while benchmark
reports measure presented frames independently through SurfaceFlinger.

Compose's device integration tests cover search input, keyboard visibility on
programmatic focus and real taps, opening and closing a dock panel, pausing and
resuming quotes, starting and stopping visible scrolling, and the visible
highlight produced by the hover driver:

```bash
cd compose-app
./gradlew :app:connectedDebugAndroidTest
```

For a paused reference capture, cold-launch each app with `--es scenario
workspace --ez still true`. Capture GPUI in a separate fixture with its stream
off before the first tick, light theme and those same three Roboto files;
preserve the original reference checkout and benchmark executable.

## Measurement

Check the animation clock before comparing frame-driven workloads:

```bash
python3 benchmarks/compose-vs-cranpose/tests/android_frame_clock.py --serial SERIAL --app cranpose \
  --apk PATH --sha256 SHA256 --ocr OCR_EXECUTABLE \
  --viewport X Y WIDTH HEIGHT \
  --output benchmarks/compose-vs-cranpose/results/FRAME_CLOCK
```

This device check warms the paused, quote, scroll and hover workspaces, then
compares three public `UI updates/s` readings with the display refresh rate.
It allows 15% sampling tolerance plus one update, requires positive callback
progress and visible changes in active modes, and saves screenshots, capture
times and separate SurfaceFlinger timestamps. In active modes, the median callback
rate must also keep up with the median presentation rate within that tolerance.
This catches delayed animation delivery without requiring an overloaded app to
sustain the display rate. A paused workspace only repaints its footer and does
not need to present at the display rate. The same command supports `--app compose`.

`benchmarks/compose-vs-cranpose/perf_window.sh` runs on the device for one
window, so adb round trips stay outside the measured interval. The report reads:

- **Presented frames:** `dumpsys SurfaceFlinger --latency <app layer>`, polled
  every `--interval` by a loop of its own and merged. SurfaceFlinger keeps 128
  frames on Android 10 and 64 on Android 17, half a second at 120 Hz, so pass
  `--interval 0.25` on a 120 Hz display. The clock and thermal samples run
  beside the poller: in the same loop, a slow `dumpsys thermalservice` made
  every fourth poll miss frames, and the busier app lost more of them. The
  report counts polls that fail to overlap the previous one
  (`latency_poll_gaps`); it should be 0. Each poll starts with the display's refresh
  period, which sets `vsync_ms` for the jank and missed-vsync counts.
  Per-layer timestats are not available on this Huawei build. Present times
  are in SurfaceFlinger's monotonic clock, which is calibrated against
  `/proc/uptime` from each poll's newest present, to within about one frame.
  The report keeps frames per second of the window and counts seconds with no
  present, so a ramp or a stall cannot hide in the mean.
- **Presentation timestamp deltas:** Android 10 reports desired present time,
  actual present time, and frame ready time in that order. The report names the
  first-to-second delta `desired_to_present_p50_ms` and the first-to-third delta
  `desired_to_ready_p50_ms`. A zero or pending timestamp excludes only the
  affected delta; a valid actual present still counts toward FPS. No valid delay
  samples produces JSON `null` and `n/a` in the summary.
  The desired timestamp is selected by the producer. With Android's automatic
  timestamp, `Surface::queueBuffer` supplies the current monotonic time; an
  explicit timestamp can refer to a different point. Ready time comes from the
  buffer's acquire fence, with desired time substituted when no valid fence
  exists. These deltas measure neither GPU execution time nor input latency.
  See the pinned Android 10 sources for
  [field order](https://android.googlesource.com/platform/frameworks/native/+/refs/tags/android-10.0.0_r1/services/surfaceflinger/FrameTracker.cpp#234),
  [automatic timestamps](https://android.googlesource.com/platform/frameworks/native/+/refs/tags/android-10.0.0_r1/libs/gui/Surface.cpp#700),
  and [ready-time fallback](https://android.googlesource.com/platform/frameworks/native/+/refs/tags/android-10.0.0_r1/services/surfaceflinger/BufferLayer.cpp#367).
  Stock Android 10 HWUI defaults to automatic timestamps; render-ahead selects
  a future vsync timestamp. Cranpose requests Vulkan presentation time zero,
  which leaves the automatic timestamp in place. The Huawei vendor implementation
  has not been verified, so cross-renderer delay comparisons must not assume
  matching origins or that HWUI measures an entire frame from its starting vsync.
  See [HWUI timestamp selection](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-10.0.0_r1/libs/hwui/renderthread/CanvasContext.cpp#413)
  and [Vulkan timestamp selection](https://android.googlesource.com/platform/frameworks/native/+/refs/tags/android-10.0.0_r1/vulkan/libvulkan/swapchain.cpp#1680).
  Historical raw reports retain their original field names and values; they must
  not be interpreted as proof of queue latency or rewritten as corrected data.
- **Cranpose runtime telemetry:** `present_return_to_display_ms` measures from
  the monotonic timestamp recorded after the present call returns to the actual
  presentation time returned by `VK_GOOGLE_display_timing`. It excludes time
  inside that call and is separate from the SurfaceFlinger deltas above. The
  frame lead controller uses this same present-return-to-display signal.
- **Temperature:** the thermal HAL's live readings for the GPU, both CPU
  clusters, the phone shell and the battery, plus cooling-device levels,
  every 2 s. Temperature is a result in its own right, not a precondition.
  Runs never wait for the phone to cool.
- **Throttling:** the GPU and CPU-cluster clock caps every 0.5 s. A cap below
  the hardware maximum, or a nonzero cooling level, marks the run throttled.
- **CPU:** process and per-thread `utime + stime` from `/proc`.
- **Clocks:** the GPU, DDR and three CPU-cluster clocks every 0.5 s. Mali-G76
  has no GPU timestamp queries, so the GPU clock that DVFS chooses is the proxy
  for GPU load. The clock files are the Kirin 980's. Elsewhere only the GPU
  clock is read, from the Mali node `/sys/class/misc/mali0/device/cur_freq`
  (the Pixel 9 Pro's, in kHz), each sample.
- **Also:** `dumpsys meminfo` after the window, and `gfxinfo` for Compose only
  (HWUI does not see Cranpose's Vulkan surface).

Each scenario runs A B A B, then B A B A. With no cooling pauses, heat and any
throttling fall on both apps alike. Each run is a cold launch, a 5 s warm-up
and a 15 s window. Failed runs are kept in the report.

```bash
(cd benchmarks/compose-vs-cranpose/cranpose-app/android && ./gradlew :app:assembleRelease)
(cd benchmarks/compose-vs-cranpose/compose-app && ./gradlew :app:assembleRelease)
python3 benchmarks/compose-vs-cranpose/measure.py --serial SERIAL --output benchmarks/compose-vs-cranpose/results/RUN --install --reps 2 --screenshots
python3 benchmarks/compose-vs-cranpose/summarize.py benchmarks/compose-vs-cranpose/results/RUN/report.json
```

### At 120 Hz without a phone

An emulator gives a 120 Hz display. The AVD needs `hw.lcd.vsync=120` and
`hw.gpu.mode=host` in its `config.ini`, and a system image with a 120 Hz mode.
The goldfish image's default refresh rate of 60 caps every layer vote,
Cranpose's and HWUI's alike. Forcing the peak rate, as the developer option
does, gives both apps the same 120 Hz:

```bash
adb -s emulator-5680 shell settings put system min_refresh_rate 120.0
python3 benchmarks/compose-vs-cranpose/measure.py --serial emulator-5680 --output benchmarks/compose-vs-cranpose/results/RUN --reps 2 --interval 0.25
```

## Findings

Historical baseline findings on a Huawei Mate 20 X (Kirin 980, Android 10)
filed these issues:

- #790: transformed graphics layers render offscreen every frame;
- #791: nested rotated layers use 1.7 GB and take seconds to reach 60 fps;
- #792: frames reach the screen 1–2 vsyncs after Compose's;
- #793: 2–3× Compose's memory on identical screens;
- #794: the GPU runs at about twice Compose's clock;
- #795: text with `Ellipsis` and `max_lines` wraps early and drops the "…";
- #796: the APK is 12× Compose's.

Re-run `measure.py` to check a fix; reports stay out of the repository.
