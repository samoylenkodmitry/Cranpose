# Cranpose and LVGL on ESP32 boards

The screen of the Cranpose ESP32 demo, built twice: `apps/esp32-demo` in
Cranpose and `lvgl-app` in LVGL 9.3, the most used UI library on ESP32 boards.
A title, a line of text, a bar that sweeps its width back and forth over
1.5 s, a counter that changes once a second and a box that counts taps. Every
frame changes the bar.

Both apps run on the same two boards with the same wiring, bus clock, panel
start-up bytes (`init_sequences.py` reads them from the demo's board modules),
strip of 24 rows (ES3C35P) or 8 rows (E32R35T), ESP-IDF 5.5 settings (the
LVGL build lists the demo's sdkconfig files) and 60 Hz frame pacing woken by
an `esp_timer`. LVGL uses its default fonts and options, plus the two it
offers for speed: cached style lookups and its drawing functions in IRAM.

## Measure

`perfstats` is an ESP-IDF component that counts cycles, instructions and stall
cycles of core 0, where both UIs run, with the Xtensa performance counters
over 5 s windows, and logs the internal RAM left. `scripts/esp32/board_run.py`
builds an app with it, flashes it and saves the log; `summary.py` turns logs
into per-frame figures.

```sh
. ~/export-esp-1.98.1.sh
for app in demo lvgl; do
  for board in es3c35p e32r35t; do
    python3 scripts/esp32/board_run.py --board $board --app $app --component benchmarks/esp32/perfstats --seconds 22 --log target/esp32/bench-$app-$board.log
  done
done
python3 benchmarks/esp32/summary.py target/esp32/bench-*.log
```

The LVGL build takes ESP-IDF from `~/.espressif`, where the demo's first
build installed it.

## Results

2026-10-24. Frame is the CPU time of a frame: Cranpose's composition,
layout, drawing and sending, LVGL's `lv_timer_handler` call that drew it.
Instructions and stall cycles are per frame. Free RAM is byte-addressable
internal RAM.

The demo's bar reads the animated width in its draw: each frame records the
bar's drawing again, as LVGL draws its bar again, without composing or
laying out the screen. The framework code such a frame runs is in IRAM
(`cranpose_hot_code`, from a profile of this demo).

| | Cranpose ES3C35P | LVGL ES3C35P | Cranpose E32R35T | LVGL E32R35T |
| --- | --- | --- | --- | --- |
| Frame | 0.49 ms | 1.05 ms | 1.80 ms | 2.21 ms |
| Instructions | 67K | 163K | 57K | 182K |
| Cycles waiting for code | 38K | 27K | 361K | 229K |
| Cycles waiting for data | 11K | 6K | 64K | 35K |
| First frame after reset | 567 ms | 371 ms | 490 ms | 439 ms |
| Internal RAM free (lowest) | 86 KB (55 KB) | 118 KB (86 KB) | 64 KB (35 KB) | 198 KB (198 KB) |
| App image | 2.96 MB | 0.52 MB | 2.90 MB | 0.47 MB |

Cranpose runs a third of LVGL's instructions for this frame. Most of the
rest of its time waits for code, more on the classic ESP32, whose 64 KB of
IRAM for the framework holds less of a frame's code than the ESP32-S3's
128 KB.

Before its first frame, Cranpose's ES3C35P build copies its 2.7 MB of code
and read-only data to PSRAM (180 ms, `CONFIG_SPIRAM_XIP_FROM_PSRAM`), where
LVGL copies 0.5 MB. Both apps then wait the panel's reset and start-up
sequence, about 270 ms on the ES3C35P and 220 ms on the E32R35T; Cranpose
creates its composition meanwhile and draws its first picture after it.
