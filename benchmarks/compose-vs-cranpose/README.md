# Cranpose vs Jetpack Compose on a device

Two apps with the same UI, element for element: `cranpose-app` (Rust, Cranpose
from this repository) and `compose-app` (Kotlin, Jetpack Compose from BOM
2026.09.00, foundation 1.12). `measure.py` runs them alternately on one Android
device and measures both from outside either framework.

## Scenarios

Each app takes the scenario and its size from intent extras
(`--es scenario NAME --ei rows 30 …`) and animates itself from its frame clock
(`withFrameNanos` / `next_frame()`). Both apps therefore do the same work per
second, without touch input.

A scenario that Compose finishes at 60 fps cannot separate the two
frameworks, so `measure.py` runs every scenario at a **heavy** load. On the
Huawei Mate 20 X that load keeps Jetpack Compose itself below 60 fps.
`--load default` runs the apps' own, lighter sizes instead.

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

Knobs:

- `feed`: `--ef speed`, `--ei fcols`, `--ef ui` and `--ei comments`;
- `ticker`: `--ei quotes`;
- `particles`: `--ei particles`;
- `layers`: `--ei tcols` and `--ei trows`, plus `--ez opaque true`, which fixes
  alpha at 1;
- `grid` and `grid_layer`: `--ei rows` and `--ei cols`;
- `deep` and `deep_layer`: `--ei depth` and `--ei chips`;
- `--ez still true` holds the feed still for side-by-side layout checks.

## Parity rules

- **Data:** `data.rs` and `Data.kt` implement the same xorshift generator, so
  every post, comment, quote and particle is identical.
- **Fonts:** both apps load `/system/fonts/Roboto-Regular.ttf` and
  `Roboto-Bold.ttf` from the device and set a 1.4 em line height. The bundled
  Noto Sans Merged declares 2.1 em of ascent plus descent, which Compose honors
  and Cranpose does not, so it cannot be compared.
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

## Measurement

`perf_window.sh` runs on the device for one window, so no adb round trip lands
inside it. It reads:

- **Presented frames:** `dumpsys SurfaceFlinger --latency <app layer>`, polled
  every sample and merged. SurfaceFlinger keeps 128 frames on Android 10 and
  64 on Android 17, half a second at 120 Hz, so pass `--interval 0.25` on a
  120 Hz display. The report counts polls that fail to overlap the previous
  one (`latency_poll_gaps`). Each poll starts with the display's refresh
  period, which sets `vsync_ms` for the jank and missed-vsync counts.
  Per-layer timestats are not available on this Huawei build. Present times
  are in SurfaceFlinger's monotonic clock, which is calibrated against
  `/proc/uptime` from each poll's newest present, to within about one frame.
  The report keeps frames per second of the window and counts seconds with no
  present, so a ramp or a stall cannot hide in the mean.
- **Temperature:** the thermal HAL's live readings for the GPU, both CPU
  clusters, the phone shell and the battery, plus cooling-device levels,
  every 2 s. Temperature is a result in its own right, not a precondition.
  Runs never wait for the phone to cool.
- **Throttling:** the GPU and CPU-cluster clock caps every 0.5 s. A cap below
  the hardware maximum, or a nonzero cooling level, marks the run throttled.
- **CPU:** process and per-thread `utime + stime` from `/proc`.
- **Clocks:** the GPU, DDR and three CPU-cluster clocks every 0.5 s. Mali-G76
  has no GPU timestamp queries, so the GPU clock that DVFS chooses is the proxy
  for GPU load. The clock files are the Kirin 980's; other devices report no
  clocks.
- **Also:** `dumpsys meminfo` after the window, and `gfxinfo` for Compose only
  (HWUI does not see Cranpose's Vulkan surface).

Each scenario runs A B A B, then B A B A. With no cooling pauses, heat and any
throttling fall on both apps alike. Each run is a cold launch, a 5 s warm-up
and a 15 s window. Failed runs are kept in the report.

```bash
(cd cranpose-app/android && ./gradlew :app:assembleRelease)
(cd compose-app && ./gradlew :app:assembleRelease)
python3 measure.py --serial SERIAL --output results/RUN --install --reps 2 --screenshots
python3 summarize.py results/RUN/report.json
```

### At 120 Hz without a phone

An emulator gives a 120 Hz display. The AVD needs `hw.lcd.vsync=120` and
`hw.gpu.mode=host` in its `config.ini`, and a system image with a 120 Hz mode.
The goldfish image's default refresh rate of 60 caps every layer vote,
Cranpose's and HWUI's alike. Forcing the peak rate, as the developer option
does, gives both apps the same 120 Hz:

```bash
adb -s emulator-5680 shell settings put system min_refresh_rate 120.0
python3 measure.py --serial emulator-5680 --output results/RUN --reps 2 --interval 0.25
```

## Findings

The first run on a Huawei Mate 20 X (Kirin 980, Android 10) filed these
issues:

- #790: transformed graphics layers render offscreen every frame;
- #791: nested rotated layers use 1.7 GB and take seconds to reach 60 fps;
- #792: frames reach the screen 1–2 vsyncs after Compose's;
- #793: 2–3× Compose's memory on identical screens;
- #794: the GPU runs at about twice Compose's clock;
- #795: text with `Ellipsis` and `max_lines` wraps early and drops the "…";
- #796: the APK is 12× Compose's.

Re-run `measure.py` to check a fix; reports stay out of the repository.
