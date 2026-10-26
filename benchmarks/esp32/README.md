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
counters over 5 s windows, the time core 0 spent idle (FreeRTOS's run time
counter, which its `sdkconfig.defaults` turns on), and the internal RAM
left. Every app logs its frame time and the kilobytes a frame sends to the
panel. `scripts/esp32/board_run.py` builds an app with the component,
flashes it and saves the log; `summary.py` turns logs into per-frame
figures.

```sh
. ~/export-esp-1.98.1.sh
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

## Results

2026-10-26. Frame is the time a frame takes from its start to its last
strip sent, as each app times it: Cranpose's composition, layout, drawing
and sending, LVGL's `lv_timer_handler` call that drew it, Slint's update,
render and send. CPU is the time core 0 did not idle, a frame's share of
it: the frame without the waits for the panel. Instructions and stall
cycles are per frame. Free RAM is byte-addressable internal RAM; LVGL's
pool is in it.

### The dashboard

| | Cranpose ES3C35P | LVGL ES3C35P | Slint ES3C35P | LVGL E32R35T | Slint E32R35T |
| --- | --- | --- | --- | --- | --- |
| Frame | 39.6 ms | 27.8 ms | 71.5 ms | 108.7 ms | 88.7 ms |
| Frames a second | 25.1 | 36.1 | 14.0 | 9.2 | 11.3 |
| CPU a frame | 39.2 ms | 27.7 ms | 69.5 ms | 101.4 ms | 76.6 ms |
| Sent a frame | 73 KB | 105 KB | 191 KB | 263 KB | 193 KB |
| Instructions | 4,023K | 4,045K | 7,933K | 9,844K | 6,445K |
| Cycles waiting for code | 2,939K | 901K | 3,384K | 8,322K | 6,782K |
| Cycles waiting for data | 1,042K | 82K | 1,908K | 900K | 1,509K |
| First frame after reset | 447 ms | 513 ms | 459 ms | 740 ms | 595 ms |
| Internal RAM free (lowest) | 104 KB (73 KB) | 73 KB (42 KB) | 105 KB (72 KB) | 155 KB (155 KB) | 71 KB (62 KB) |
| App image | 3.32 MB | 0.58 MB | 1.27 MB | 0.54 MB | 1.08 MB |

Cranpose does not run the dashboard on the E32R35T: it keeps 453 KB of
heap in 3,742 blocks for it (traced on the ES3C35P), and the classic ESP32
has 280 KB free at start, so its first composition runs out. The largest
parts: the shapes of the pixel renderer's two copies of the raster scene
(35 KB), layout nodes (34 KB), glyph masks (21 KB), the slot table's groups
(16 KB), draw lists (14 KB), modifier nodes (14 KB) and layer records
(#902).

On the ES3C35P Cranpose draws the dashboard in 0.55 of Slint's time and
LVGL in 0.70 of Cranpose's. Slint's dirty region is at most three
rectangles, so it redraws and sends 191 KB a frame against Cranpose's 73 KB
and LVGL's 105 KB. LVGL and Cranpose run the same number of instructions
(4.0M); Cranpose waits 3.3 times as long for code and 13 times as long for
data. A Cranpose frame runs through more code than its 125 KB of
IRAM and the 32 KB cache hold. Its waits for data are for PSRAM: the Rust
heap takes its blocks with `heap_caps_malloc(MALLOC_CAP_DEFAULT)`, which
the ESP32-S3 serves from PSRAM first, and 545 KB of the dashboard's heap
lies there (`just esp32-sim-profile dashboard data` counts 8.3K misses of
the 64 KB data cache a frame), while LVGL keeps its objects in a pool in
internal RAM. With the code copied to PSRAM at start
(`CONFIG_SPIRAM_XIP_FROM_PSRAM`, which `sdkconfig.es3c35p` leaves off) the
frame takes about 38 ms and the first frame comes 603 ms after reset.

### The demo screen

The demo's bar reads the animated width in its draw: each frame records the
bar's drawing again, as LVGL draws its bar again, without composing or
laying out the screen.

| | Cranpose ES3C35P | LVGL ES3C35P | Cranpose E32R35T | LVGL E32R35T |
| --- | --- | --- | --- | --- |
| Frame | 0.43 ms | 0.94 ms | 1.20 ms | 1.74 ms |
| Instructions | 60K | 131K | 55K | 139K |
| Cycles waiting for code | 26K | 63K | 217K | 159K |
| Cycles waiting for data | 8K | 9K | 50K | 12K |
| First frame after reset | 338 ms | 333 ms | 420 ms | 430 ms |
| Internal RAM free (lowest) | 101 KB (70 KB) | 121 KB (90 KB) | 157 KB (154 KB) | 203 KB (203 KB) |
| App image | 2.89 MB | 0.54 MB | 2.84 MB | 0.49 MB |

On the ESP32-S3 IRAM comes out of the same internal RAM as the heap: the
125 KB that holds Cranpose's per-frame code leaves it less of that RAM free
than LVGL. The classic ESP32 has
a separate region for IRAM.

### A line of text that changes every frame

The seconds line shows a number that counts up every frame instead, as a
clock or a sensor reading does: LVGL sets its label's text every frame
(`--sdkconfig benchmarks/esp32/sdkconfig.text_every_frame`), Cranpose
composes the number in a `Text` (`--feature text-composed`) or draws it in
`draw_behind`, which reads it while it draws (`--feature text-drawn`).

| Frame | ES3C35P | E32R35T |
| --- | --- | --- |
| LVGL, label set every frame | 2.42 ms | 3.51 ms |
| Cranpose, drawn | 2.18 ms | 3.87 ms |
| Cranpose, composed | 4.48 ms | 6.68 ms |

A drawn line takes no composition or layout, as the bar does. A composed
line composes its `Text` and lays it out each frame: fewer instructions
than LVGL's frame (185K against 298K on the ES3C35P), but over more code
than IRAM and the cache hold, so it waits more than three times as long for
code (812K cycles against 248K).

Before its first frame each app waits out the panel's reset and start-up
sequence, about 260 ms on the ES3C35P and 220 ms on the E32R35T. LVGL
draws its first picture after the wait. Cranpose creates its composition
meanwhile and draws its first picture ahead: whole into PSRAM on the
ES3C35P, so only its transfer is left after the wait, and the glyphs of its
text on the E32R35T, which has no PSRAM.
