# Cranpose, LVGL and Slint on ESP32 boards

Two screens, each built in more than one UI framework, on two boards: the
Hosyond ES3C35P (ESP32-S3 at 240 MHz, 8 MB PSRAM, ST77922 panel on quad
SPI) and the LCDWiki E32R35T (classic ESP32 at 240 MHz on one core, no
PSRAM, ST7796 panel on SPI), both 320 by 480.

- The demo screen: a title, a line of text, a bar that sweeps its width back
  and forth over 1.5 s, a counter that changes once a second and a box that
  counts taps. Every frame changes the bar. `apps/esp32-demo` in Cranpose
  and `lvgl-app` in LVGL 9.3, the most used UI library on ESP32 boards.
- The dashboard (`crates/cranpose/tests/support/dashboard.rs`): a ring gauge
  and its percentage, three level bars, a chart of 24 bars under a 32-point
  line, two switches, a slider and a list that scrolls by itself, below a
  clock and three readings. Most of the display changes every frame. In
  Cranpose (`--feature dashboard`), LVGL (`--sdkconfig
  benchmarks/esp32/sdkconfig.dashboard`) and Slint 1.18 (`slint-app`). Each
  value on it is the same function of the milliseconds since the first
  frame in every app, so the three show the same picture at the same time.

Every app runs with the same wiring, bus clock (40 MHz), panel start-up
bytes (`init_sequences.py` reads them from the demo's board modules), strip
of 24 rows (ES3C35P) or 8 rows (E32R35T), ESP-IDF 5.5 settings (the LVGL and
Slint builds list the demo's sdkconfig files), C code at `-O2` and 60 Hz
frame pacing woken by an `esp_timer`. The LVGL and Slint apps draw through
the same panel driver (`panel`, on ESP-IDF's `esp_lcd`); the Cranpose
platform has its own.

- LVGL uses its default fonts and options, plus the two it offers for
  speed: cached style lookups and its drawing functions in IRAM. The
  dashboard's objects take its memory pool to 112 KB.
- Slint uses its software renderer with the features it gives
  microcontrollers (without `std`), its glyphs pre-rendered from the demo's
  Noto Sans. On the ES3C35P it draws into a frame in PSRAM and sends the
  areas that changed, as Slint's ESP32-S3 boards do; on the E32R35T it draws
  line by line. Its build needs three things the others do not: a one-call
  fix in a copy of `i-slint-core` (1.18.1 calls `f32::rem_euclid` without
  `std`; `board_run.py` makes it), no LTO (with fat or thin LTO it stops on
  an integer division by zero in its gradient code on the ESP32-S3) and a
  48 KB main task stack (a `Path` takes some 30 KB of it).
- Cranpose builds with fat LTO and keeps its hottest per-frame code in IRAM
  (`cranpose_hot_code`, from profiles of the demo, its text lines and a
  busier screen; not of the dashboard).

## Measure

`perfstats` is an ESP-IDF component that counts cycles, instructions and
stall cycles of core 0, where every UI runs, with the Xtensa performance
counters over 5 s windows (`CONFIG_PERFSTATS_WINDOW_MS`), the time core 0 spent idle (FreeRTOS's run time
counter, which its `sdkconfig.defaults` turns on), and the internal RAM
left. Every app logs its frame time and the kilobytes a frame sends to the
panel. `scripts/esp32/board_run.py` builds an app with the component,
flashes it and saves the log; `summary.py` turns logs into per-frame
figures.

```sh
. ~/export-esp-1.100.0.sh
run() { python3 scripts/esp32/board_run.py --board $1 --component benchmarks/esp32/perfstats --seconds 27 --log target/esp32/bench-$2-$1.log "${@:3}"; }
for board in es3c35p e32r35t; do
  run $board demo --app demo
  run $board drawn --app demo --feature text-drawn
  run $board composed --app demo --feature text-composed
  run $board dashboard --app demo --feature dashboard
  run $board lvgl --app lvgl
  run $board lvgl-text --app lvgl --sdkconfig benchmarks/esp32/sdkconfig.text_every_frame
  run $board lvgl-dashboard --app lvgl --sdkconfig benchmarks/esp32/sdkconfig.dashboard
  run $board slint-dashboard --app slint
done
python3 benchmarks/esp32/summary.py target/esp32/bench-*.log
```

The LVGL build takes ESP-IDF from `~/.espressif`, where the demo's first
build installed it.

The nightly runs `just perf-esp32 BOARD OUTPUT` on the two boards attached to
samarch-1. Each screen has its own application image. The JSON report keeps its
byte size and the frame count of every second for both boots. The Performance
tab reads the published report. The browser runs each layout on a virtual ESP32-S3.
[Benchmark layouts](https://samoylenkodmitry.github.io/cranpose-esp32/benchmarks/) · [Measurement details](../../docs/performance_methodology.md)

### Compare revisions

A screen's frame time moves by up to 0.1 ms with where the linker puts the
code the order file leaves out, which any change shifts, so one build per
side measures luck. `just esp32-ab <board> <base> <revision>...` builds
each revision once with the demo's `screens-in-turn` feature (every screen
in turn, 6 s each, with 1 s windows), links it again for each of 4
placements (the order file, and with empty gaps a seed picks between its
patterns, the same for every revision), runs placement by placement and
prints each screen's mean frame time and instructions a frame, and each
revision's mean, smallest and largest difference from the base.
Builds go in a worktree for each board beside the checkout, as two
worktrees of one commit build different code, and stay there by commit.

The hot-code lists name functions. A change that renames, inlines or
removes a listed function leaves its code in flash until the lists are
made again, and the frame reads slower at fewer instructions. Compare such
a change with `--no-hot-code` among the revisions: every revision then
builds with the lists off (`scripts/esp32/sdkconfig.no_hot_code`), all
code runs from flash, and a placement shifts the whole of the flash code
by a gap a seed picks. The run stops when two placements of a revision
link to one image.

A verdict needs a null leg: the base with one function no code calls,
which moves code and changes no behaviour. `--null NAME` names it among
the revisions (`NAME=COMMIT` names a leg). A screen's tolerance is the
larger of the timer's step, 0.01 ms, and the largest difference of the null
leg from the base. A revision loses on a screen when every placement reads
above the base and the mean lies above the tolerance; it wins on the mirror
of that; it holds when the mean lies inside the tolerance; otherwise the
screen stays open and takes more placements. A run that lacks a screen, an
instruction count or a placement decides nothing and exits with an error.
`--unpaired` serves sides whose order files differ: a revision then loses
when all its readings lie above all of the base's.

## Results

The nightly records the first five seconds of each boot on both boards. The dashboard shows each start and the firmware size for each supported layout. [Performance results](https://samoylenkodmitry.github.io/Cranpose/?tab=performance)
