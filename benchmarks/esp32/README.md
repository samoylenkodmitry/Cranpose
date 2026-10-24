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

2026-10-24, the demo at the commit that added this directory. Frame is the
CPU time of a frame: Cranpose's composition, layout, drawing and sending,
LVGL's `lv_timer_handler` call that drew it. Instructions and stall cycles
are per frame. Free RAM is byte-addressable internal RAM.

| | Cranpose ES3C35P | LVGL ES3C35P | Cranpose E32R35T | LVGL E32R35T |
| --- | --- | --- | --- | --- |
| Frame | 1.43 ms | 1.05 ms | 4.49 ms | 2.21 ms |
| Instructions | 87K | 163K | 77K | 182K |
| Cycles waiting for code | 239K | 27K | 960K | 229K |
| Cycles waiting for data | 34K | 6K | 137K | 35K |
| First frame after reset | 777 ms | 371 ms | 510 ms | 439 ms |
| Internal RAM free (lowest) | 84 KB (53 KB) | 118 KB (86 KB) | 61 KB (31 KB) | 198 KB (198 KB) |
| App image | 2.96 MB | 0.52 MB | 2.90 MB | 0.47 MB |

Cranpose runs half the instructions of LVGL for this frame and loses the
time waiting for its code: a frame runs through several hundred KB of
framework code, more than the 32 KB cache holds, while LVGL's frame waits
27K cycles for its code on the ES3C35P.
