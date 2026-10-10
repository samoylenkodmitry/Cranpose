# Cranpose vs Jetpack Compose on a device

Two apps with the same UI, element for element: `cranpose-app` (Rust, Cranpose
from this repository) and `compose-app` (Kotlin, Jetpack Compose).
`measure.py` runs them alternately on one Android device and measures both
from outside either framework. The gauntlet also runs in `views-app` (Android
Views with RecyclerView), `flutter-app` (Flutter, which picks Impeller on
OpenGL ES on the Mate), `rn-app` (React Native on the New Architecture with
Hermes), `nativescript-app` (NativeScript: TypeScript on V8 driving Android
views), `lynx-app` (Lynx: ReactLynx on PrimJS driving Lynx's native
elements), `maui-app` (.NET MAUI, fully AOT-compiled), `avalonia-app` (Avalonia
on Skia, fully AOT-compiled), `egui-app` (egui in eframe on OpenGL ES, in a
GameActivity), `slint-app` (Slint on Skia) and `web-app` (a web page in
Capacitor, on the device's Chromium WebView: the stack Ionic, Tauri and
Dioxus apps run on). The Rust apps share `perf-data`, and `rust-android`
packages them: each crate's folder is its Android module, behind one launch
activity that hands the native side the `am start` extras. The frameworks that
target the browser also run there, as pages in Chrome ("In a browser", below).

This file describes the apps and how they are measured, not results: the
nightly measures every framework at its latest stable release, and the
desktop demo's Performance tab shows each device's latest numbers with the
versions measured ("Every night", below).

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
- `gauntlet`: `--ei tier` (1 to 16) and `--ei freeze K`, which stops on frame K
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
- **Stacked layers:** translucent dark panels stacked over the list, each
  220 dp wide, with a title, rows of eight numbered colored cells and a
  translucent white nested card with two lines of text. A panel's content
  never changes: each frame it drifts and tilts up to 6 degrees about its
  center, and its nested card tilts as far the other way. The panels overlap
  one another and the list, so every pixel under them blends several times.
  A framework that keeps a panel's drawing and only moves it does little
  work here; one that draws or lays it out again each frame pays for every
  cell.
- **Width:** the whole content's width follows `k` between 92% and 100%, so
  every visible node is measured again each frame.

Each app is written in its own framework's best idiom, not translated line by
line. Compose uses `LazyVerticalGrid` with full-span clusters, reads the
width in `Modifier.layout`, and isolates every per-frame read in its own small
composable or draw lambda. Cranpose uses `LazyColumn` rows (it has no lazy
grid yet, #1165), reads the width at the screen root above one argument-stable
call, and isolates the same reads. Both move and tilt each stacked panel in a
graphics layer whose block reads the frame, so the frame records no panel
again. Views uses a `RecyclerView` of card rows
and clusters, measures the width in a parent `onMeasure`, and redraws bars and
sparklines in `onDraw`. Flutter uses a `ListView` of rows, lays the width out
in a `SingleChildLayoutDelegate` that relayouts on the frame, rebuilds only the
changing texts in `ValueListenableBuilder`s, and paints in `CustomPainter`s
that repaint on the frame. React Native runs one JavaScript frame loop, so
each frame is one React commit: the changing texts, bars, badges and the
width are small memoized components reading the frame through
`useSyncExternalStore`, the list is a FlashList of rows, sparklines are Skia
paths, and the footer's dividers stretch in Yoga's row. NativeScript
advances the frame on Android's Choreographer: its own requestAnimationFrame
runs from the native choreographer's vsync, which the main thread serves
before its Java messages, so once frames take longer than a vsync the window
never reports its first draw. Its core layouts and labels are Android views
that the views on screen update with only what changed, the list is a
ListView whose two row templates each recycle their own kind, and sparklines
are an Android view written in TypeScript. Lynx runs ReactLynx's frame
loop on its background thread, so each frame is one React commit, as in
React Native, which Lynx applies to native views on its main thread. The
loop starts a frame only once the main thread has applied the last one:
left alone, the background thread commits every vsync and the main thread
falls further behind each frame. Lynx lays out on its own thread
(`PART_ON_LAYOUT`): with layout on the UI thread, the default, the window
never reports its first draw from tier 6 on. The list is a `<list>` of
deferred `<list-item>` rows: a row's components exist only while it is on
screen. Sparklines are `<svg>` elements whose markup each frame rebuilds.
R8 keeps Lynx and Fresco, its image library, whole: under their published
rules the sparklines and the avatars do not draw. MAUI advances the
frame on its animation ticker; views on screen set only what changed, the
list is a CollectionView of rows scrolled through its RecyclerView (MAUI
scrolls to items, not offsets), sparklines and bars are GraphicsView
drawables, and the footer is a Grid whose row is as tall as its tallest
cell. Avalonia advances the frame on the top level's animation frame
callback; controls on screen set only what changed, the list is an
ItemsControl over a VirtualizingStackPanel whose card rows and clusters each
recycle only their own kind, flow rows are WrapPanels, cards are Borders with
a box shadow, and bars and sparklines are controls that draw themselves.
egui lays out and paints the whole screen every frame, as immediate
mode does: boxes reserve their background and paint it once their content is
laid out, fixed-size pieces allocate their size and paint, and the list lays
out only the rows on screen from an anchor that moves as rows scroll off.
Slint declares the screen in compiled `.slint` markup: every per-frame value
is a binding on a `Clock.frame` global, flow rows are `FlexboxLayout`s, rows
come from a Rust `Model` as the `ListView` shows them, and a pure Rust
callback draws the sparkline paths; Slint components cannot contain
themselves, so a cluster's levels are boxes stacked from the outermost in.
The web page lays everything out in CSS, trims paragraph leading with
`text-box`, keeps only the rows on screen in the DOM, recycling rows that
scroll off behind a spacer as tall, and sets only what changed each
animation frame; sparklines are SVG paths in a box CSS stretches.

Each app moves and tilts the stacked panels in its own way. Views sets
each panel's render node properties; Flutter turns a `Transform` over a
`RepaintBoundary`, so the panel paints once; React Native, Lynx and
NativeScript set the panel view's transform; MAUI and Avalonia set its
render transform; SwiftUI applies rotation and offset effects; AppKit sets
the panel layer's position and transform; the web page and Dioxus set a CSS
transform on an element that `will-change` keeps on a compositor layer of
its own; Slint binds the panel's position and rotation to the frame;
Xilem memoizes the panel's content and sets only its box's transform. egui,
iced and gpui lay out and paint every panel again each frame, turning each
shape; gpui turns text as glyph outlines, as it draws no turned text. Fyne
draws no turned object, so its panels only drift.

| Tier | Columns | Scale | Ticker tiles | Cluster depth | Layers | Cell rows |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1 | 1.0 | 8 | 6 | 1 | 2 |
| 2 | 2 | 0.85 | 12 | 8 | 1 | 2 |
| 3 | 2 | 0.7 | 16 | 10 | 2 | 3 |
| 4 | 3 | 0.6 | 20 | 12 | 2 | 3 |
| 5 | 3 | 0.5 | 28 | 14 | 2 | 4 |
| 6 | 4 | 0.45 | 36 | 16 | 3 | 4 |
| 7 | 4 | 0.4 | 44 | 20 | 3 | 5 |
| 8 | 5 | 0.35 | 56 | 24 | 3 | 5 |
| 9 | 6 | 0.3 | 72 | 28 | 4 | 6 |
| 10 | 7 | 0.27 | 96 | 32 | 4 | 6 |
| 11 | 8 | 0.25 | 120 | 40 | 4 | 7 |
| 12 | 10 | 0.2 | 160 | 48 | 5 | 7 |
| 13 | 12 | 0.18 | 200 | 56 | 5 | 8 |
| 14 | 14 | 0.16 | 240 | 64 | 5 | 8 |
| 15 | 16 | 0.14 | 300 | 72 | 6 | 9 |
| 16 | 20 | 0.12 | 400 | 80 | 6 | 10 |

Calibration fixes one tier per device: the lightest at which every
framework draws below 60 fps. The Huawei Mate 20 X runs tier 12 and the
Apple M3 Pro desktop tier 16. Raising a device's tier starts a new series
rather than changing an old one. The web view renders in a sandboxed process
of its own, which the phone's CPU count leaves out.

`parity.py` launches two apps frozen on frame 120 and compares the two
captures the way the eye does: softened and cut into tiles. On frame 120 a
cluster sits mid-screen; on frame 240 one sits at the top of the list, where
its look-alike levels make the drift ambiguous. Each band of tiles
is found in the other capture up to 160 pixels higher or lower. Each tile is
then matched at the best offset within 8 pixels of its band's, by halves of
its quarters at the neighbouring bands' where they drifted differently. A
band whose tiles changed, as where the still stacked panels lie over the
list, matches them again at every offset a whole band matched at. A tile
that still differs on average is changed; a band that has drifted past the
other capture's edge, or behind its still content, is left out. Each
capture's tiles are found in the other, and the worse way decides. The
bands absorb drift: the frameworks put lines of text on different pixel grids,
so a list scrolled 720 dp shows its rows a few dozen pixels apart without
looking any different. Flutter rounds each line to whole logical pixels and
Compose rounds it up to whole device pixels. Any difference for the same
composable code is a Cranpose bug. Against Compose at tier 5 every app but
Flutter, NativeScript and Slint stays under the 2% gate; what the other
frameworks draw differently, by design:

- The stacked panels lie still over a list that drifts as each framework
  rounds its lines: where a tilted panel's edge crosses the list, the list
  beneath it has moved. Flutter's and NativeScript's lists drift the most,
  about 50 pixels under the second panel's edge, so the row of tiles along
  that edge changes: 2.9% and 3.4% of tiles. Slint's ticker strip breaks
  into one more line, so its panels and its list sit a line lower: 2.9%.

- Flutter and Slint set text slightly wider, so a few lines break a word
  earlier.
- React Native keeps a paragraph's leading above its first line and below its
  last.
- egui filters textures in linear light, so the striped avatars average
  lighter, and puts each glyph on a whole pixel.
- Views, MAUI and the web page put lines of text a pixel apart.

## Parity rules

- **Data:** `data.rs`, `shared-kotlin/dev/perfcompare/shared/PerfData.kt` (the
  Compose and Views apps), `flutter-app/lib/data.dart`, `rn-app/src/data.ts`,
  `shared-cs/PerfData.cs`, `shared-swift/PerfData.swift` and
  `fyne-app/data.go` implement the same xorshift generator, so every
  post, comment, quote and particle is identical. `data.rs` is `perf-data`,
  which the Cranpose, egui and Slint apps share; React Native, NativeScript
  and the web page share `shared-ts/data.ts`, MAUI and Avalonia share
  `shared-cs`, and SwiftUI and AppKit share `shared-swift/PerfData.swift`.
- **Fonts:** every app loads `/system/fonts/Roboto-Regular.ttf` and
  `Roboto-Bold.ttf` from the device and sets a 1.4 em line height. React
  Native registers them with its font manager, Lynx with its typeface cache,
  NativeScript copies them into the app's `fonts` folder, where its
  `fontFamily` looks, MAUI serves them from its
  own `IFontManager` and Avalonia from a font collection; the Huawei system
  font is wider. The bundled
  Noto Sans Merged declares 2.1 em of ascent plus descent, which Compose honors
  and Cranpose does not, so it cannot be compared. The workspace also loads
  `Roboto-Medium.ttf` and follows GPUI's 1.618034 em line height, including the
  leading above and below single-line labels. Compose uses centered line
  height, `Trim.None` and `includeFontPadding = false` for that contract.
- **Window:** the same fullscreen theme, `singleTask` and `configChanges`, and
  one arm64 build each.
- **Shadows:** egui has no elevation, so its cards draw Android's two
  elevation shadows themselves: the ambient one and the spot one cast from
  the light above the window's top centre, with Android's alphas, offsets and
  blur widths. egui's text uses its light theme, which blends glyph coverage
  as the other apps do.
- **Accessibility:** the apps keep the tree that accessibility services read,
  as their toolkits do by default. egui builds AccessKit's tree every frame
  with a label for each text. Its Android adapter attaches to GameActivity's
  view, which R8 must keep, and needs `accesskit_winit`'s `accesskit_android`
  feature, which egui-winit leaves off. Avalonia serves its tree by default:
  uiautomator reads 765 nodes at tier 5. Slint's Android backend has no
  accessibility support, so the Slint app does none of this work.
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
holds the device lock throughout and measures legs in pairs, A B then B A.
Every leg is one launch measured from its start command for 10 s (`--run`),
nothing left out: a build's first launch after its install is a leg like the
others, and the launch, the first frame and the first seconds count as the
rest of the run does. From the second pair on, it stops a scenario once fps, CPU per frame and
desired → present are each settled:
- **Same:** medians within half the threshold, and each build's own range
  narrower than the threshold.
- **Different:** ranges apart, with medians apart by the threshold times
  three with two legs a side, two with three, one with four.

The thresholds are 3% of fps, 3% of CPU per frame and 2 ms. After four pairs
anything still open is inconclusive. On the Mate, a same-build A/A comparison
settles in 4 legs for 60 fps scenes and runs 8 legs for the gauntlet, whose
launch-to-launch noise is about 3%.

```bash
python3 benchmarks/compose-vs-cranpose/ab.py --serial SERIAL --a cranpose-release --b cranpose \
  --scenarios gauntlet,feed,grid --output OUTPUT
```

`cranpose-release` is the Cranpose app built with `-PperfCompareSuffix=.release`,
so a second build installs beside the first. Every leg runs the same span,
whatever the build's frame rate.

`frameworks.py` measures every framework's app on the gauntlet for the
dashboard: three rounds of one leg each, the order reversed every round, the
first round each app's first launch after its install. Every leg is one launch
measured from its start command for 10 s (`--run`), the same span for every
app, and keeps the frames of every second from the launch; the dashboard
charts those seconds for every framework. Each leg records the app's frame rate and CPU per frame, its
PSS and the part of it GPU buffers hold (GL and EGL mtrack), and the mean
clocks of the big cores and the GPU. `--install DIR` first installs each app's
`APP.apk` from DIR and compiles every app's Java with `speed`, as the Compose
app's best case.

```bash
python3 benchmarks/compose-vs-cranpose/frameworks.py --serial SERIAL --output OUTPUT
```

### Every night

`.github/workflows/perf-nightly.yml` measures main's latest commit at 01:30,
unless the dashboard already holds it. Cranpose runs twice: its latest
release (`cranpose-release`, once a release draws the gauntlet) and main.

1. On macm3, `versions.py bump` moves each framework's pin to its latest
   stable release, `build_apps.sh` builds every desktop app, every Android
   app but Cranpose's, and the release's desktop app, and `desktop.py`
   measures the desktop apps at tier 16. A moved pin that does not build is
   put back. The run goes to the dashboard, and the pins that moved and
   built go to the pull request `perf/framework-versions`. `build_apps.sh`
   keeps each app's last build with a stamp of the files, pin and
   toolchains it read, so a night builds again only the apps that moved and
   Cranpose's.
   `build_apps.sh browser` also builds the apps that target the browser, and
   `desktop.py --browser` measures them in Chrome at tier 16 (below).
2. On the Mac the Mate 20 X is attached to, `scripts/perf/nightly.py`
   compares the latest release with main, and `just perf-frameworks` installs
   macm3's Android builds beside Cranpose's release and main and runs
   `frameworks.py`. `just perf-browser` then opens macm3's browser builds in
   Chrome on the phone at tier 8.

Each run names the version of every framework it measured: the pins
`versions.py` reads, the Flutter SDK and .NET MAUI workload `build_apps.sh`
keeps at their latest, the phone's WebView, the Mac's Chrome or Safari's
WebKit, and the macOS SwiftUI and AppKit ship with. `python3 versions.py check` lists each pin beside
its registry's latest release.

The Performance tab of the desktop demo reads the runs from the `perf-data`
branch: the latest night's release against main, the trend, and the latest
framework comparison of each device. Chips choose what the trend and each
comparison show: frames per second, CPU per frame, memory, GPU memory, or
the CPU and GPU clocks. Each framework there opens its gauntlet's source at
the commit measured.

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
run, started right after the launch command, so adb round trips stay outside
the measured interval. The run's zero is the device clock read just before
the launch command. The report reads:

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
  The report keeps the frames of every second from the launch
  (`fps_by_second`) and the time to the first frame, and counts seconds after
  the first frame with no present, so a slow start, a ramp or a stall cannot
  hide in the mean. When the first poll already holds a full history, frames
  of the launch may be lost and the run says so (`first_poll_full`).
- **CPU:** every process whose name starts with the app's package, with the
  ticks it spent since it started; the launch stopped the earlier ones, so
  these are the run's, its start included.
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
throttling fall on both apps alike. Each run is a cold launch measured from
its start command for 10 s (`--run`), its first seconds included. Failed runs
are kept in the report.

```bash
(cd benchmarks/compose-vs-cranpose/cranpose-app/android && ./gradlew :app:assembleRelease)
(cd benchmarks/compose-vs-cranpose/compose-app && ./gradlew :app:assembleRelease)
(cd benchmarks/compose-vs-cranpose/views-app && ./gradlew :app:assembleRelease)
(cd benchmarks/compose-vs-cranpose/flutter-app && flutter build apk --release --target-platform android-arm64)
(cd benchmarks/compose-vs-cranpose/rn-app && npm ci && cd android && ./gradlew :app:assembleRelease)
(cd benchmarks/compose-vs-cranpose/lynx-app/page && npm ci && npm run build && cd .. && ./gradlew :app:assembleRelease)
(cd benchmarks/compose-vs-cranpose/nativescript-app && npm ci && npx ns build android --release --gradleArgs=-Pabis=arm64-v8a --key-store-path ~/.android/debug.keystore --key-store-password android --key-store-alias androiddebugkey --key-store-alias-password android)
(cd benchmarks/compose-vs-cranpose/maui-app && dotnet publish -c Release -f net10.0-android)
(cd benchmarks/compose-vs-cranpose/avalonia-app && dotnet publish -c Release -f net10.0-android)
(cd benchmarks/compose-vs-cranpose/rust-android && ./gradlew :egui:assembleRelease :slint:assembleRelease)
(cd benchmarks/compose-vs-cranpose/web-app && npm ci && npm run build && cd android && ./gradlew :app:assembleRelease)
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

### On a Mac desktop

`desktop.py` runs the gauntlet's desktop apps on a Mac, one at a time, each in
a window of 1280 x 820 points (`perf-data`'s `DESKTOP_WINDOW`). Each app reads
the tier from `PERF_TIER`, the frame to freeze on from `PERF_FREEZE` and the
Roboto files from `PERF_FONTS`; Cranpose takes `--tier=N` arguments and the
web page reads its address. `framecount/FrameCount.app` counts the frames the
window presents through ScreenCaptureKit, as SurfaceFlinger counts a phone
app's, and takes the pictures `--parity` compares. `ps` counts the CPU time
of the app and every process it started, `footprint` the memory they hold at
the window's end and the part of it that is the GPU's (Metal's buffers and
textures, and the surfaces the window server composites), and `macmon` the
mean clocks of the performance cores and the GPU over the window, from the
chip's own counters and without root. Three rounds measure each app once
each. Every leg is one launch measured from its start for 10 s (`--run`), the
same span for every app: FrameCount waits for the app's window, records the
time of every frame and says where its capture began, so the frames of every
second from the launch are kept. A leg disturbed by other processes stays in
the run, marked, and out of the medians. Desktop numbers feed the dashboard
only; merges are judged on the slowest phone.

At tier 5 against Compose Multiplatform, SwiftUI and AppKit change 0.0% of
tiles, Flutter 0.1%, the web page and Tauri 0.2%, Dioxus and egui 0.3%,
Cranpose 0.35%, Avalonia 0.5%, Slint 0.7%, GPUI 0.9% and iced 1.1%. Fyne
changes 2.7%: its panels only drift. Xilem changes 5.6%: its labels keep the
font's own line height, so its cards are shorter and its list drifts further,
and the panels lie over a different part of the cluster's look-alike levels.

| App | Stack |
| --- | --- |
| `cranpose` | `cranpose-app`'s desktop binary |
| `compose` | `compose-desktop-app`: Compose Multiplatform on the JVM, drawing the composables `compose-app` draws, from `shared-compose` |
| `egui`, `slint` | the Android crates' desktop binaries |
| `iced` | `iced-app`: iced on wgpu |
| `gpui` | `gpui-app`: Zed's GPUI as `gpui-pre` publishes it |
| `avalonia` | `avalonia-app`'s desktop head, Native AOT |
| `swiftui` | `swiftui-app`: SwiftUI |
| `appkit` | `appkit-app`: AppKit's layer-backed views, laid out by hand, with a list that recycles its rows |
| `flutter` | `flutter-app`'s macOS runner, on Impeller |
| `web` | the web page in a Chrome app window, the engine Electron apps ship |
| `tauri` | the same page in a Tauri window, on the system's WKWebView; the page asks the app for the tier and logs through its commands |
| `dioxus` | `dioxus-app`: Dioxus components over the web page's CSS, on Dioxus's desktop renderer (WKWebView) |
| `xilem` | `xilem-app`: Xilem's views over Masonry's widgets, drawn by Vello on wgpu |
| `fyne` | `fyne-app`: Fyne on OpenGL, canvas objects the app places itself |

What each framework lacks and how its app does without:

- iced text has no line limit or ellipsis: a box as tall as the lines clips
  the paragraph. iced has no list that lays out only the rows on screen: the
  app keeps the first row on screen and its top, and sensors around the rows
  report their heights.
- GPUI draws text only upright: the badge's text is Roboto Bold's outlines,
  drawn as paths. Nested flex columns made GPUI's layout engine measure each
  cluster level again for its parent, so the levels stack in block layout.
  GPUI slows a window that is not in front to 30 fps; the app turns that off.
- Slint's Skia renderer draws through wgpu on macOS and tells the rendering
  notifier that a frame was drawn only with the `unstable-wgpu-30` feature.
- AppKit's views cost too much to move thousands each frame: a card, a row
  and a cluster are views, and what they show are Core Animation layers. A
  text layer draws again only when its text changes or a new width wraps or
  cuts it; sparklines are shape layers over a masked gradient.
- Tauri and Dioxus run their pages in WebKit services that launchd starts
  outside the app's process tree; `desktop.py` counts them as the app's.
  Tauri lets the page `desktop.py` serves call its commands through a
  capability for that address.
- Dioxus advances the frame on the page's animation frame, which it hears
  through `eval`, and serves the avatars and Roboto from a custom protocol.
  It has no list that renders only the rows on screen: rows report their
  heights through `onresize`, as iced's sensors do.
- Xilem has no wrapping row, no box that clips, scrolls or turns about its
  centre, no canvas, and shadows only on buttons and text inputs: the app
  adds Masonry widgets for these, with a view for each, and a widget that
  hands each animation frame to the app logic. Its labels keep the font's
  own line height and have no line limit, so a box as tall as the lines
  clips a paragraph, and the subtitle is three labels in a row. Its virtual
  scroll moves by whole rows, so rows report their heights, as iced's do.
- Fyne sizes an object before it knows its width, so the app wraps
  paragraphs and places every object itself, as Fyne's custom widgets do. It
  rotates no object: the badge stays upright. Its canvas has no path: the
  sparkline's line is 47 segments, and its fading fill is a vertical
  gradient under white polygons above the line, four side by side, since
  Fyne fills a polygon of at most 16 vertices.
- The JVM opens no window outside the login session, so `desktop.py` starts
  every app bundle through `open`. macOS then asks the user before such an
  app reads a removable volume, so the fonts and Chrome's profile sit in a
  temporary folder.

An app's CPU counts every process it started, the WebKit services launchd
starts for a WKWebView (Tauri and Dioxus), and the window server: SwiftUI,
AppKit and WebKit hand it layer trees to render, where a Metal app hands it
one surface. Each leg also records the window server's share alone, and the
cores every other process spent beside it.

FrameCount needs the Screen Recording permission once: `framecount/build.sh`
signs it with a requirement on its bundle identifier, so rebuilds keep it.
`macmon` comes from Homebrew (`brew install macmon`); without it a run has no
clocks.

```bash
sh benchmarks/compose-vs-cranpose/framecount/build.sh
(cd benchmarks/compose-vs-cranpose/cranpose-app && cargo build --release)
(cd benchmarks/compose-vs-cranpose/egui-app && cargo build --release)
(cd benchmarks/compose-vs-cranpose/slint-app && cargo build --release)
(cd benchmarks/compose-vs-cranpose/iced-app && cargo build --release)
(cd benchmarks/compose-vs-cranpose/gpui-app && cargo build --release)
(cd benchmarks/compose-vs-cranpose/avalonia-app && dotnet publish -c Release -f net10.0 -p:TargetFrameworks=net10.0 -r osx-arm64)
(cd benchmarks/compose-vs-cranpose/swiftui-app && ./build.sh)
(cd benchmarks/compose-vs-cranpose/appkit-app && ./build.sh)
(cd benchmarks/compose-vs-cranpose/flutter-app && flutter build macos --release)
(cd benchmarks/compose-vs-cranpose/compose-desktop-app && ./gradlew createDistributable)
(cd benchmarks/compose-vs-cranpose/web-app && npm ci && npx tsc -p tsconfig.json)
(cd benchmarks/compose-vs-cranpose/tauri-app && cargo build --release)
(cd benchmarks/compose-vs-cranpose/dioxus-app && cargo build --release)
(cd benchmarks/compose-vs-cranpose/xilem-app && cargo build --release)
(cd benchmarks/compose-vs-cranpose/fyne-app && go build -o build/perf-compare-fyne .)
python3 benchmarks/compose-vs-cranpose/desktop.py --output benchmarks/compose-vs-cranpose/results/desktop --tier 16
python3 benchmarks/compose-vs-cranpose/desktop.py --output benchmarks/compose-vs-cranpose/results/desktop-parity --parity --tier 5
```

### In a browser

The frameworks that target the browser run there too, as pages: the same
gauntlet, the same data and the same sources, compiled for the web. A page is
no app: it shares its browser's processes, its renderer is the browser's, and
its memory is the browser's. So these runs are of kind `browser`, on a device
named for the browser (`Apple M3 Pro · Chrome 154`, `EVR-AL00 · Chrome 154`),
and the dashboard shows them beside the native comparison, not in place of
it. `build_apps.sh browser DIR` makes `DIR/APP/index.html`:

| App | In the browser |
| --- | --- |
| `cranpose` | `cranpose-app` with `--features web`, on wgpu: WebGPU where the browser has it, WebGL2 where not (`?backend=gl` or `?backend=webgpu` forces one), the query string read as its launch arguments |
| `compose` | `compose-browser-app`: Compose Multiplatform compiled to Kotlin/Wasm, the composables of `shared-compose`, drawn by Skia |
| `flutter` | `flutter build web --wasm`: skwasm, and the JavaScript build for a browser without WasmGC |
| `egui` | eframe on glow, which is WebGL2 there; no AccessKit tree to keep |
| `slint` | FemtoVG on WebGL2: Skia does not run in a browser |
| `iced` | wgpu: WebGPU where the browser has it, WebGL2 where not |
| `dioxus` | the web renderer, which patches the DOM directly instead of sending the patches to a webview |
| `avalonia` | `avalonia-app/Browser`: Avalonia on Skia, compiled ahead of time to WebAssembly |
| `web` | the plain page of DOM and CSS, which every other app is held to |

GPUI, Xilem, Fyne, Tauri and the native toolkits have no browser target, and
React Native, Lynx, NativeScript, MAUI and Views are not pages. Every page
takes its tier and freeze frame from the query string, `?tier=12&freeze=120`,
and logs its `PERF` lines on the console. `browser.py` serves the pages, and
puts one script in each page's head that forwards those lines to the server,
because Chrome on a phone logs a page's console nowhere a harness can read.

`desktop.py --browser DIR` opens each page in its own Chrome app window, as
it opens the plain page, and counts frames, CPU, memory and clocks as for any
desktop app: the browser, its renderer and its GPU process are the app.
`frameworks.py --browser DIR` opens each in Chrome on the phone over `adb
reverse`, takes its frames from Chrome's page surface
(`com.android.chrome/ChromeChildSurface#0`) and sums the CPU and memory of
every Chrome process. A page that fails to start leaves its legs out and the
run goes on, as a dash in the dashboard.

**The tier.** A browser caps a page at the display's rate, and a framework
that draws in a browser tab loses much of what it has natively, so each
browser has a tier of its own, calibrated like the devices': the lightest at
which no page reaches 60 fps, so every page's frame rate shows its cost. If
that tier leaves the heaviest pages under 5 fps, it is relaxed a tier or two:
a load that crawls for all tells the pages apart no better. Frame rates of one
round each, 2026-10-08:

| Tier | Cranpose | Compose | Flutter | egui | Slint | iced | Dioxus | Avalonia | Web |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| M3 Pro 8 | 47.9 | 34.5 | 48.3 | 60.1 | 51.7 | 60.2 | 60.2 | 39.6 | 60.0 |
| M3 Pro 12 | 29.3 | 25.6 | 52.1 | 60.2 | 34.2 | 42.2 | 56.5 | 27.4 | 57.6 |
| **M3 Pro 16** | 23.6 | 22.9 | 31.2 | 49.8 | 24.7 | 33.5 | 35.1 | 21.3 | 37.8 |
| Mate 4 | 10.4 | 13.0 | 59.5 | 60.2 | 19.5 | fails | 59.3 | 15.1 | 59.2 |
| **Mate 8** | 6.1 | 6.2 | 28.4 | 54.8 | 8.7 | fails | 33.6 | 5.9 | 42.2 |
| Mate 12 | 2.5 | 2.5 | 10.5 | 42.2 | 4.7 | fails | 12.4 | 2.3 | 16.0 |

On the desktop the heaviest tier, 16, already keeps egui, the fastest, under
60. On the phone tier 8 is the first at which nothing reaches 60, and tier 12
leaves three pages at about 2 fps. Chrome on the Mate has no WebGPU
(Android 10), so every page that can draws on WebGL2. iced's WebGL fragment
shader for gradients does not compile on the Mate's GPU driver, and its page
stops at start. Raising a browser's tier starts a new series. The Cranpose
column above ran on WebGL2, before the page took WebGPU where the browser has
it.

## Findings

What the comparisons find is filed as issues, not kept here. Re-run
`measure.py` to check a fix; reports stay out of the repository.
