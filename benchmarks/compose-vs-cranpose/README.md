# Cranpose vs Jetpack Compose on a device

Two apps with the same UI, element for element: `cranpose-app` (Rust, Cranpose
from this repository) and `compose-app` (Kotlin, Jetpack Compose from BOM
2026.09.00, foundation 1.12). `measure.py` runs them alternately on one Android
device and measures both from outside either framework. The gauntlet also runs
in `views-app` (Android Views, RecyclerView 1.4) and `flutter-app` (Flutter
3.47, which picks Impeller on OpenGL ES on the Mate).

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
| `gauntlet` | Every stage at once (below). Advances a frame index each frame, never wall time. | tier 5 | Composition, layout, intrinsics, text, images, paths, shadows, layers |

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
  with its stream, automatic scrolling and synthetic hover stopped;
- `gauntlet`: `--ei tier` (1 to 8) and `--ei freeze K`, which stops on frame K
  for picture comparisons.

### Gauntlet

One screen heavy enough that no framework holds 60 fps on the device it runs
on, so every framework's frame rate shows its per-frame cost. Every frame
advances a frame index `k`, and everything follows from `k`, so each framework
does the same work per frame. Printed values are integer arithmetic of `k`,
so both apps show the same digits on the same frame.

- **Ticker strip:** quote tiles in a flow row whose price and change text and
  bar change every frame: recomposition, text shaping and reflow.
- **Card grid:** a lazy grid scrolling 3 dp a frame. Each card has an elevation
  shadow and a border, a circle-clipped avatar bitmap, a two-line title, an
  annotated subtitle, four lines of body text, a progress bar and percent,
  a 48-point sparkline with a gradient fill, a chip flow row, and a footer of
  counters split by dividers in an `IntrinsicSize.Min` row. Every fifth card
  carries a translucent badge tilting in its graphics layer.
- **Deep clusters:** after every five card rows, a cluster of nested levels.
- **Width:** the whole content's width follows `k` between 92% and 100%, so
  every visible node is measured again each frame.

Each app is written in its own framework's best idiom, not translated line by
line. Compose uses `LazyVerticalGrid` with full-span clusters, reads the
width in `Modifier.layout`, and isolates every per-frame read in its own small
composable or draw lambda. Cranpose uses `LazyColumn` rows (it has no lazy
grid yet, #1165), reads the width at the screen root above one argument-stable
call, and isolates the same reads. Views uses a `RecyclerView` of card rows
and clusters, measures the width in a parent `onMeasure`, and redraws bars and
sparklines in `onDraw`. Flutter uses a `ListView` of rows, lays the width out
in a `SingleChildLayoutDelegate` that relayouts on the frame, rebuilds only the
changing texts in `ValueListenableBuilder`s, and paints in `CustomPainter`s
that repaint on the frame.

| Tier | Columns | Scale | Ticker tiles | Cluster depth |
| --- | ---: | ---: | ---: | ---: |
| 1 | 1 | 1.0 | 8 | 6 |
| 2 | 2 | 0.85 | 12 | 8 |
| 3 | 2 | 0.7 | 16 | 10 |
| 4 | 3 | 0.6 | 20 | 12 |
| 5 | 3 | 0.5 | 28 | 14 |
| 6 | 4 | 0.45 | 36 | 16 |
| 7 | 4 | 0.4 | 44 | 20 |
| 8 | 5 | 0.35 | 56 | 24 |

Calibration fixes one tier per device: the Huawei Mate 20 X runs tier 5. On
2026-10-05 Cranpose drew 52.8 fps there, Views 52 to 54, Flutter 29.5 and
Compose 26.5. Raising a device's tier starts a new series rather than changing
an old one.

`parity.py` launches two apps frozen on the same frame and compares the two
captures the way the eye does: softened and cut into tiles. Each band of tiles
is found in the other capture up to 160 pixels higher or lower. Each tile is
then matched at the best offset within 8 pixels of its band's, quarter by
quarter of the neighbouring bands' where they drifted differently. A tile
that still differs on average is changed; a band that has drifted past the
other capture's edge is left out. The
bands absorb drift: the frameworks put lines of text on different pixel grids,
so a list scrolled 720 dp shows its rows a few dozen pixels apart without
looking any different. Flutter rounds each line to whole logical pixels and
Compose rounds it up to whole device pixels. Any difference for the same
composable code is a Cranpose bug. On 2026-10-05, against Compose at tier 5:

| App | Changed tiles | What differs |
| --- | ---: | --- |
| Cranpose | 1.24% | Compose lays out in whole pixels (#1215) |
| Flutter | 0.22% | Its unhinted text is about 2% wider, so a few lines break a word earlier |
| Views | 0.21% | Lines of text a pixel apart |

## Parity rules

- **Data:** `data.rs`, `shared-kotlin/dev/perfcompare/shared/PerfData.kt` (the
  Compose and Views apps) and `flutter-app/lib/data.dart` implement the same
  xorshift generator, so every post, comment, quote and particle is identical.
- **Fonts:** every app loads `/system/fonts/Roboto-Regular.ttf` and
  `Roboto-Bold.ttf` from the device and sets a 1.4 em line height. The bundled
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

### Short A/B comparisons

`ab.py` compares two installed builds as briefly as the evidence allows. It
holds the device lock throughout and launches each build once unmeasured.
Then it measures legs of 2 s warm-up plus a 5 s window, in pairs, A B then
B A. From the second pair on, it stops a scenario once fps, CPU per frame and
desired → present are each settled:
- **Same:** medians within half the threshold, and each build's own range
  narrower than the threshold.
- **Different:** ranges apart, with medians apart by the threshold times
  three with two legs a side, two with three, one with four.

The thresholds are 3% of fps, 3% of CPU per frame and 2 ms. After four pairs
anything still open is inconclusive. On the Mate, a same-build A/A comparison
settles in 4 legs for 60 fps scenes and runs 8 legs for the gauntlet, whose
launch-to-launch noise is about 3%. Cranpose against Compose on the gauntlet
took 99 s.

```bash
python3 benchmarks/compose-vs-cranpose/ab.py --serial SERIAL --a cranpose-release --b cranpose \
  --scenarios gauntlet,feed,grid --output OUTPUT
```

`cranpose-release` is the Cranpose app built with `-PperfCompareSuffix=.release`,
so a second build installs beside the first.

### Long comparisons

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
(cd benchmarks/compose-vs-cranpose/views-app && ./gradlew :app:assembleRelease)
(cd benchmarks/compose-vs-cranpose/flutter-app && flutter build apk --release --target-platform android-arm64)
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
